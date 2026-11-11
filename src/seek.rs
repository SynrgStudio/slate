//! Seek (github.com/SynrgStudio/seek): the engine that finds files by name
//! on every drive, greps folders and knows what changed lately. A
//! background process keeps the index; this talks to it over its pipe
//! (`\\.\pipe\seek-<user>`): a little-endian u32 length, then that many
//! bytes of JSON, both ways, as Seek's PROTOCOL.md (version 1) says. If the pipe isn't there, the installed
//! `seek.exe --daemon` is started (it outlives Slate, and serves agents
//! and other programs from the same index).
//!
//! Everything about Seek lives here, so moving to Seek's own Rust crate
//! later only changes this module.
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};

/// Never taken for a newer Seek (a newer build makes the daemon step aside).
const BUILD: &str = concat!("0.0.0 slate ", env!("CARGO_PKG_VERSION"));
/// The protocol version this was written for.
const API: u32 = 1;
/// How long to wait for a daemon that is starting.
const START_WAIT: Duration = Duration::from_secs(15);
/// Right after Seek starts, its indexes load (about a second).
const LOADING_WAIT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub(crate) enum Error {
    /// No pipe, and no `seek.exe` to start.
    NotInstalled,
    /// The pipe broke or never answered.
    Unreachable(String),
    /// Seek answered with an error: its `code` (`bad_args`, `not_found`,
    /// `unknown_command`...; empty from a Seek before 0.5.0) and why.
    Seek { code: String, text: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotInstalled => write!(
                f,
                "Seek isn't installed: get it from github.com/SynrgStudio/seek"
            ),
            Error::Unreachable(why) => write!(f, "Seek didn't answer ({why})"),
            Error::Seek { code, text }
                if code == "unknown_command" || code == "api_too_new" || text.contains("Unknown tool") =>
            {
                write!(f, "Seek is too old for Slate: update it")
            }
            Error::Seek { text, .. } => write!(f, "{}", text.lines().next().unwrap_or(text)),
        }
    }
}

/// How the names matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Match {
    Real,
    /// Close names: the query had a typo, or nothing matched as typed.
    Near,
    /// Letters spread through the names.
    Scattered,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Rows {
    pub total: u64,
    /// `total` is exact, not a lower bound.
    pub exact: bool,
    pub matched: Match,
    /// `(is a folder, path)`, ranked.
    pub entries: Vec<(bool, PathBuf)>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Drive {
    pub letter: char,
    /// `loading`, `indexing`, `ready` or `none`.
    pub state: String,
    pub names: u64,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Status {
    pub version: String,
    pub drives: Vec<Drive>,
}

impl Status {
    /// "indexing D: (812,345 names)" while a first index runs.
    pub(crate) fn indexing(&self) -> Option<String> {
        let busy: Vec<String> = self
            .drives
            .iter()
            .filter(|drive| drive.state == "indexing")
            .map(|drive| format!("{}: ({} names)", drive.letter, thousands(drive.names)))
            .collect();
        (!busy.is_empty()).then(|| format!("indexing {}", busy.join(", ")))
    }
}

/// One connection to Seek, kept open between calls. One per thread.
#[derive(Default)]
pub(crate) struct Client {
    pipe: Option<File>,
}

impl Client {
    /// Names matching `query` (Seek's syntax: `.md`, `!word`, `src/name`,
    /// `"exact"`, `*`), only below `path` when given (skipping what
    /// .gitignore leaves out there), ranked closer to `near` first.
    pub(crate) fn rows(
        &mut self,
        query: &str,
        path: Option<&Path>,
        near: Option<&Path>,
        max: usize,
    ) -> Result<Rows, Error> {
        let mut args = json!({ "query": query, "max": max });
        if let Some(path) = path {
            args["path"] = json!(path.display().to_string());
        }
        if let Some(near) = near {
            args["near"] = json!(near.display().to_string());
        }
        let text = self.call(json!({ "tool": "find.rows", "args": args }))?;
        parse_rows(&text)
    }

    /// The state of each drive's index.
    pub(crate) fn status(&mut self) -> Result<Status, Error> {
        let text = self.call(json!({ "tool": "status.rows" }))?;
        parse_status(&text)
    }

    /// Tells Seek the user opened `path`, so it ranks higher and shows in
    /// recent files.
    pub(crate) fn opened(&mut self, path: &Path) -> Result<(), Error> {
        self.call(json!({ "tool": "opened", "args": { "path": path.display().to_string() } }))
            .map(|_| ())
    }

    fn call(&mut self, mut request: Value) -> Result<String, Error> {
        request["api"] = json!(API);
        request["build"] = json!(BUILD);
        let request = request.to_string();
        let loading_until = Instant::now() + LOADING_WAIT;
        let mut broken = false;
        loop {
            let pipe = match self.pipe.as_mut() {
                Some(pipe) => pipe,
                None => self.pipe.insert(connect()?),
            };
            match exchange(pipe, request.as_bytes()).map(|reply| parse_reply(&reply)) {
                // Seek has just started: its indexes are loading.
                Ok(Err(Error::Seek { code, .. }))
                    if code == "index_loading" && Instant::now() < loading_until =>
                {
                    thread::sleep(Duration::from_millis(100));
                }
                Ok(result) => return result,
                // A pipe kept open may have closed (Seek restarted): one retry.
                Err(error) => {
                    self.pipe = None;
                    if broken {
                        return Err(Error::Unreachable(error.to_string()));
                    }
                    broken = true;
                }
            }
        }
    }
}

/// Tells Seek a file was opened, off the caller's thread.
pub(crate) fn report_opened(path: &Path) {
    if cfg!(test) {
        // Tests open temporary files: they are not the user's.
        return;
    }
    static OPENED: OnceLock<mpsc::Sender<PathBuf>> = OnceLock::new();
    let sender = OPENED.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<PathBuf>();
        thread::spawn(move || {
            let mut client = Client::default();
            for path in receiver {
                let _ = client.opened(&path);
            }
        });
        sender
    });
    let _ = sender.send(path.to_path_buf());
}

/// One request and its reply on a stream.
fn exchange(stream: &mut (impl Read + Write), request: &[u8]) -> io::Result<Vec<u8>> {
    stream.write_all(&(request.len() as u32).to_le_bytes())?;
    stream.write_all(request)?;
    stream.flush()?;
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > 256 << 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "reply too long"));
    }
    let mut reply = vec![0; length];
    stream.read_exact(&mut reply)?;
    Ok(reply)
}

/// The reply's text, or Seek's error.
fn parse_reply(reply: &[u8]) -> Result<String, Error> {
    let value: Value = serde_json::from_slice(reply)
        .map_err(|error| Error::Unreachable(format!("unreadable reply: {error}")))?;
    let text = value["text"].as_str().unwrap_or_default().to_string();
    if value["error"].as_bool() == Some(true) {
        // A code this doesn't know is as good as "failed".
        let code = value["code"].as_str().unwrap_or("failed").to_string();
        return Err(Error::Seek { code, text });
    }
    Ok(text)
}

/// `#rows\t<total>\t<exact>\t<real|near|scattered>`, then `D|F\t<path>`.
fn parse_rows(text: &str) -> Result<Rows, Error> {
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    if head.first() != Some(&"#rows") {
        return Err(unexpected(text));
    }
    // Rows may gain fields at the end: read the ones known, by position.
    let entries = lines
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let kind = fields.next()?;
            let path = fields.next()?;
            Some((kind == "D", PathBuf::from(path)))
        })
        .collect();
    Ok(Rows {
        total: head.get(1).and_then(|n| n.parse().ok()).unwrap_or(0),
        exact: head.get(2) != Some(&"0"),
        matched: match head.get(3).copied() {
            Some("near") => Match::Near,
            Some("scattered") => Match::Scattered,
            _ => Match::Real,
        },
        entries,
    })
}

/// `#rows\t<version>\t<drives>`, then `<letter>\t<state>\t<names>\t...`.
fn parse_status(text: &str) -> Result<Status, Error> {
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    if head.first() != Some(&"#rows") {
        return Err(unexpected(text));
    }
    let drives = lines
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let letter = fields.next()?.chars().next()?;
            let state = fields.next()?.to_string();
            let names = fields.next()?.parse().unwrap_or(0);
            Some(Drive {
                letter,
                state,
                names,
            })
        })
        .collect();
    Ok(Status {
        version: head.get(1).unwrap_or(&"").to_string(),
        drives,
    })
}

fn unexpected(text: &str) -> Error {
    Error::Seek {
        code: "failed".into(),
        text: text.to_string(),
    }
}

/// Opens the pipe, starting Seek if it isn't running.
fn connect() -> Result<File, Error> {
    let name = pipe_name();
    let mut started = false;
    let deadline = Instant::now() + START_WAIT;
    loop {
        match OpenOptions::new().read(true).write(true).open(&name) {
            Ok(pipe) => return Ok(pipe),
            // All instances busy: another client is mid-call.
            Err(error) if error.raw_os_error() == Some(231) => {}
            Err(error) if !started => {
                if error.kind() != io::ErrorKind::NotFound {
                    return Err(Error::Unreachable(error.to_string()));
                }
                start_daemon()?;
                started = true;
            }
            Err(_) => {}
        }
        if Instant::now() > deadline {
            return Err(Error::Unreachable("its background process didn't start".into()));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// `\\.\pipe\seek-<user>`, and with SEEK_HOME set, that instance's pipe
/// (named as Seek names it).
fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let mut name = format!(r"\\.\pipe\seek-{user}");
    if let Some(home) = std::env::var_os("SEEK_HOME").filter(|home| !home.is_empty()) {
        let home = home.to_string_lossy().replace('/', "\\");
        let home = home.trim_end_matches('\\').to_lowercase();
        let hash = home
            .encode_utf16()
            .fold(2_166_136_261u32, |hash, unit| (hash ^ unit as u32).wrapping_mul(16_777_619));
        name.push_str(&format!("-{hash:08x}"));
    }
    name
}

/// The installed Seek.
fn seek_exe() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs_next::data_local_dir)?;
    let installed = local.join("Programs").join("Seek").join("seek.exe");
    installed.is_file().then_some(installed)
}

/// Starts `seek.exe --daemon` detached, out of Slate's job when allowed (so
/// it outlives Slate), and inheriting **no** handles: it lives for hours,
/// and `std::process::Command` would hand it every inheritable handle Slate
/// has, such as the pipes of whoever started Slate (a `cargo test | tail`
/// then never ends). As Seek starts itself.
#[cfg(windows)]
fn start_daemon() -> Result<(), Error> {
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct StartupInfo {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        reserved2_size: u16,
        reserved2: *mut u8,
        std_input: *mut std::ffi::c_void,
        std_output: *mut std::ffi::c_void,
        std_error: *mut std::ffi::c_void,
    }
    #[repr(C)]
    struct ProcessInformation {
        process: *mut std::ffi::c_void,
        thread: *mut std::ffi::c_void,
        process_id: u32,
        thread_id: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateProcessW(
            application: *const u16,
            command_line: *mut u16,
            process_attributes: *const std::ffi::c_void,
            thread_attributes: *const std::ffi::c_void,
            inherit_handles: i32,
            creation_flags: u32,
            environment: *const std::ffi::c_void,
            current_directory: *const u16,
            startup_info: *const StartupInfo,
            process_information: *mut ProcessInformation,
        ) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    let exe = seek_exe().ok_or(Error::NotInstalled)?;
    let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT;
    let mut last_error = io::Error::other("not started");
    // Out of Slate's job if the job allows it; else inside it.
    for flags in [flags | CREATE_BREAKAWAY_FROM_JOB, flags] {
        let mut command_line: Vec<u16> = format!("\"{}\" --daemon", exe.display())
            .encode_utf16()
            .chain([0])
            .collect();
        let application: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
        let startup = StartupInfo {
            cb: std::mem::size_of::<StartupInfo>() as u32,
            reserved: std::ptr::null_mut(),
            desktop: std::ptr::null_mut(),
            title: std::ptr::null_mut(),
            x: 0,
            y: 0,
            x_size: 0,
            y_size: 0,
            x_count_chars: 0,
            y_count_chars: 0,
            fill_attribute: 0,
            flags: 0,
            show_window: 0,
            reserved2_size: 0,
            reserved2: std::ptr::null_mut(),
            std_input: std::ptr::null_mut(),
            std_output: std::ptr::null_mut(),
            std_error: std::ptr::null_mut(),
        };
        let mut info = ProcessInformation {
            process: std::ptr::null_mut(),
            thread: std::ptr::null_mut(),
            process_id: 0,
            thread_id: 0,
        };
        // SAFETY: every pointer is valid for the call, the command line is
        // writable and NUL-terminated, and both handles returned are closed.
        let started = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                flags,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut info,
            )
        };
        if started != 0 {
            unsafe {
                CloseHandle(info.thread);
                CloseHandle(info.process);
            }
            return Ok(());
        }
        last_error = io::Error::last_os_error();
    }
    Err(Error::Unreachable(format!("couldn't start {}: {last_error}", exe.display())))
}

#[cfg(not(windows))]
fn start_daemon() -> Result<(), Error> {
    Err(Error::NotInstalled)
}

/// 812345 → "812,345".
pub(crate) fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream whose reads come from a canned reply.
    struct Fake {
        reply: io::Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl Read for Fake {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reply.read(buf)
        }
    }

    impl Write for Fake {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.sent.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn framed(body: &str) -> Vec<u8> {
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(body.as_bytes());
        out
    }

    #[test]
    fn a_request_and_its_reply_are_length_prefixed() {
        let reply = r#"{"text":"ok"}"#;
        let mut fake = Fake {
            reply: io::Cursor::new(framed(reply)),
            sent: Vec::new(),
        };
        let got = exchange(&mut fake, br#"{"tool":"__opened"}"#).unwrap();
        assert_eq!(got, reply.as_bytes());
        assert_eq!(fake.sent, framed(r#"{"tool":"__opened"}"#));
        // A reply cut short is an error, not a hang or a panic.
        let mut cut = Fake {
            reply: io::Cursor::new(framed(reply)[..6].to_vec()),
            sent: Vec::new(),
        };
        assert!(exchange(&mut cut, b"{}").is_err());
    }

    #[test]
    fn rows_parse_with_their_kind_and_folders() {
        // The second row has a field a later Seek may add: it is ignored.
        let text = "#rows\t3\t1\tnear\nF\tC:\\notes\\inbox.md\nD\tC:\\notes\\in box\tnew field\n";
        let rows = parse_rows(text).unwrap();
        assert_eq!(rows.total, 3);
        assert!(rows.exact);
        assert_eq!(rows.matched, Match::Near);
        assert_eq!(
            rows.entries,
            vec![
                (false, PathBuf::from("C:\\notes\\inbox.md")),
                (true, PathBuf::from("C:\\notes\\in box")),
            ]
        );
        assert!(matches!(parse_rows("find needs a query"), Err(Error::Seek { .. })));
    }

    #[test]
    fn replies_with_an_error_say_why() {
        let error = parse_reply(
            br#"{"api":1,"version":"0.5.0","text":"no such command","error":true,"code":"unknown_command"}"#,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "Seek is too old for Slate: update it");
        // Before 0.5.0: no code, and the text said so.
        let old = parse_reply(br#"{"text":"Unknown tool: find.rows","error":true}"#).unwrap_err();
        assert_eq!(old.to_string(), "Seek is too old for Slate: update it");
        match parse_reply(br#"{"text":"C:/nope isn't there","error":true,"code":"not_found"}"#) {
            Err(Error::Seek { code, text }) => {
                assert_eq!(code, "not_found");
                assert_eq!(text, "C:/nope isn't there");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(parse_reply(br#"{"text":"ok"}"#).unwrap(), "ok");
        assert!(matches!(parse_reply(b"not json"), Err(Error::Unreachable(_))));
    }

    #[test]
    fn status_says_which_drives_are_being_indexed() {
        let text = "#rows\t0.4.10\t2\nC\tready\t2216900\tjolt\t1794358309\nD\tindexing\t812345\town\t0\n";
        let status = parse_status(text).unwrap();
        assert_eq!(status.version, "0.4.10");
        assert_eq!(status.drives.len(), 2);
        assert_eq!(status.indexing().as_deref(), Some("indexing D: (812,345 names)"));
        let ready = parse_status("#rows\t0.4.10\t1\nC\tready\t5\tjolt\t1\n").unwrap();
        assert_eq!(ready.indexing(), None);
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(12), "12");
    }

    /// Starts a Seek of its own: run with SEEK_HOME set to a temporary
    /// folder (a separate instance) and the output piped, as in
    /// `cargo test starting_seek -- --ignored | tail`; the pipeline must end.
    #[test]
    #[ignore]
    fn starting_seek_never_holds_the_callers_pipes() {
        assert!(std::env::var_os("SEEK_HOME").is_some(), "set SEEK_HOME: never the user's Seek");
        let mut client = Client::default();
        let status = client.status().expect("a Seek of its own starts and answers");
        println!("started Seek {}", status.version);
    }

    /// Talks to the Seek on this machine. `cargo test seek -- --ignored`.
    #[test]
    #[ignore]
    fn live_seek_finds_this_repo_and_reports_its_drives() {
        let mut client = Client::default();
        let status = client.status().expect("Seek answers");
        assert!(!status.version.is_empty());
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let rows = client.rows("Cargo.toml", Some(here), None, 50).expect("rows");
        assert!(rows.entries.iter().any(|(_, path)| path.ends_with("Cargo.toml")));
        assert!(
            rows.entries.iter().all(|(_, path)| !path.starts_with(here.join("target"))),
            "what .gitignore leaves out stays out"
        );
    }
}
