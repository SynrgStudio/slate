//! Seek (github.com/SynrgStudio/seek): the engine that finds files by name
//! on every drive, greps folders and knows what changed lately. A
//! background process keeps the index and serves agents and other programs
//! from it too; Slate talks to it through Seek's own crate, `seek-client`.
//!
//! The crate starts Seek when it isn't running (PROTOCOL.md, "Starting
//! Seek"): the newest seek.exe there is, the installed one at the same
//! version. With its `bundled` feature Slate carries one, written to
//! `%LOCALAPPDATA%\slate\seek\` only when it is the one started. It also
//! keeps the time limits, reconnects, and replaces an older Seek running.
//!
//! This module adds what is Slate's: its messages, and the types Periscope
//! reads.
use seek_client::{Code, DriveState, MatchKind, Seek};
use std::{
    path::{Path, PathBuf},
    sync::{OnceLock, mpsc},
    thread,
};

/// Who asks, in Seek's log and status.
const CLIENT: &str = concat!("slate ", env!("CARGO_PKG_VERSION"));

/// The Seek release Slate carries ("0.5.1"), for the license notice.
#[allow(dead_code)]
pub(crate) const BUNDLED: Option<&str> = seek_client::BUNDLED_SEEK;

#[derive(Debug)]
pub(crate) struct Error(seek_client::Error);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use seek_client::Error as E;
        match &self.0 {
            E::NotInstalled => write!(
                f,
                "Seek isn't installed: get it from github.com/SynrgStudio/seek"
            ),
            E::DidNotStart => write!(f, "Seek was started but didn't answer"),
            E::TimedOut => write!(
                f,
                "Seek didn't answer in time: it may be stuck (`seek --status` says)"
            ),
            E::TooOld { version } => write!(f, "Seek {version} is too old for Slate: update it"),
            E::Seek {
                code: Code::UnknownCommand | Code::ApiTooNew,
                ..
            } => write!(f, "Seek is too old for Slate: update it"),
            E::Seek { message, .. } => {
                write!(f, "{}", message.lines().next().unwrap_or(message))
            }
            E::Io(error) => write!(f, "Seek didn't answer ({error})"),
            other => write!(f, "{other}"),
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

impl From<seek_client::Found> for Rows {
    fn from(found: seek_client::Found) -> Self {
        Rows {
            total: found.total,
            exact: found.exact,
            matched: match found.kind {
                MatchKind::Near => Match::Near,
                MatchKind::Scattered => Match::Scattered,
                MatchKind::Real => Match::Real,
            },
            entries: found
                .entries
                .into_iter()
                .map(|entry| (entry.is_dir, entry.path))
                .collect(),
        }
    }
}

/// Each drive's index.
#[derive(Debug)]
pub(crate) struct Status(seek_client::Status);

impl Status {
    /// "indexing D: (812,345 names)" while a first index runs.
    pub(crate) fn indexing(&self) -> Option<String> {
        let busy: Vec<String> = self
            .0
            .drives
            .iter()
            .filter(|drive| drive.state == DriveState::Indexing)
            .map(|drive| format!("{}: ({} names)", drive.letter, thousands(drive.names)))
            .collect();
        (!busy.is_empty()).then(|| format!("indexing {}", busy.join(", ")))
    }
}

/// One connection to Seek, made on the first call and kept. One per thread.
#[derive(Default)]
pub(crate) struct Client {
    seek: Option<Seek>,
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
        self.call(|seek| {
            let mut find = seek.find(query).max(max.min(u32::MAX as usize) as u32);
            if let Some(path) = path {
                find = find.path(path);
            }
            if let Some(near) = near {
                find = find.near(near);
            }
            find.run().map(Rows::from)
        })
    }

    /// The state of each drive's index.
    pub(crate) fn status(&mut self) -> Result<Status, Error> {
        self.call(|seek| seek.status().map(Status))
    }

    /// Tells Seek the user opened `path`, so it ranks higher and shows in
    /// recent files.
    pub(crate) fn opened(&mut self, path: &Path) -> Result<(), Error> {
        self.call(|seek| seek.opened(path))
    }

    /// Connects when there is no connection yet (a failed connect is tried
    /// again on the next call).
    fn call<T>(
        &mut self,
        request: impl Fn(&Seek) -> seek_client::Result<T>,
    ) -> Result<T, Error> {
        let seek = match &self.seek {
            Some(seek) => seek,
            None => self.seek.insert(
                Seek::builder()
                    .app("slate")
                    .client(CLIENT)
                    .connect()
                    .map_err(Error)?,
            ),
        };
        request(seek).map_err(Error)
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
    use seek_client::{Drive, Entry, Found, IndexSource};

    #[test]
    fn found_names_keep_their_kind_and_folders() {
        let rows = Rows::from(Found {
            total: 3,
            exact: true,
            kind: MatchKind::Near,
            entries: vec![
                Entry {
                    path: PathBuf::from("C:\\notes\\inbox.md"),
                    is_dir: false,
                },
                Entry {
                    path: PathBuf::from("C:\\notes\\in box"),
                    is_dir: true,
                },
            ],
        });
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
    }

    #[test]
    fn errors_say_what_to_do() {
        let too_old = Error(seek_client::Error::Seek {
            code: Code::UnknownCommand,
            message: "no such command".into(),
        });
        assert_eq!(too_old.to_string(), "Seek is too old for Slate: update it");
        let missing = Error(seek_client::Error::Seek {
            code: Code::NotFound,
            message: "C:/nope isn't there\nmore".into(),
        });
        assert_eq!(missing.to_string(), "C:/nope isn't there");
        assert!(
            Error(seek_client::Error::NotInstalled)
                .to_string()
                .contains("github.com/SynrgStudio/seek")
        );
    }

    #[test]
    fn status_says_which_drives_are_being_indexed() {
        let drive = |letter, state, names| Drive {
            letter,
            state,
            names,
            source: IndexSource::Own,
            taken: None,
        };
        let status = Status(seek_client::Status {
            version: "0.5.1".into(),
            drives: vec![
                drive('C', DriveState::Ready, 2_216_900),
                drive('D', DriveState::Indexing, 812_345),
            ],
        });
        assert_eq!(status.indexing().as_deref(), Some("indexing D: (812,345 names)"));
        let ready = Status(seek_client::Status {
            version: "0.5.1".into(),
            drives: vec![drive('C', DriveState::Ready, 5)],
        });
        assert_eq!(ready.indexing(), None);
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(12), "12");
    }

    /// Slate carries a Seek, so it works where Seek isn't installed.
    #[test]
    fn slate_carries_a_seek() {
        if cfg!(windows) {
            assert!(BUNDLED.is_some_and(|version| !version.is_empty()));
        }
    }

    /// A machine without Seek: the one Slate carries is written out and
    /// started. Run with LOCALAPPDATA and SEEK_HOME set to temporary folders
    /// (no installed Seek, and a Seek instance of its own, never the
    /// user's), with the output piped (`| tail`): the pipeline must end,
    /// since Seek inherits none of Slate's handles.
    #[test]
    #[ignore]
    fn where_seek_isnt_installed_slate_starts_its_own() {
        assert!(std::env::var_os("SEEK_HOME").is_some(), "set SEEK_HOME: never the user's Seek");
        let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
        assert!(!local.join(r"Programs\Seek\seek.exe").exists(), "set LOCALAPPDATA to an empty folder");
        let mut client = Client::default();
        let status = client.status().expect("the Seek Slate carries starts and answers");
        assert_eq!(Some(status.0.version.as_str()), BUNDLED);
        let exe = local.join(format!(r"slate\seek\{}\seek.exe", BUNDLED.unwrap()));
        assert!(exe.is_file(), "{} is written", exe.display());
    }

    /// Talks to the Seek on this machine. `cargo test seek -- --ignored`.
    #[test]
    #[ignore]
    fn live_seek_finds_this_repo_and_reports_its_drives() {
        let mut client = Client::default();
        let status = client.status().expect("Seek answers");
        assert!(!status.0.version.is_empty());
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let rows = client.rows("Cargo.toml", Some(here), None, 50).expect("rows");
        assert!(rows.entries.iter().any(|(_, path)| path.ends_with("Cargo.toml")));
        assert!(
            rows.entries.iter().all(|(_, path)| !path.starts_with(here.join("target"))),
            "what .gitignore leaves out stays out"
        );
    }
}
