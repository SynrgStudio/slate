//! Instant "open" for an already running Slate.
//!
//! The first main-editor process listens on a loopback-only port and records
//! `port token` in its local data directory. A later `slate file.md` connects,
//! hands over the absolute path, waits for an acknowledgement and exits — no new
//! window, GL context or font atlas is created. Any failure (no instance, stale
//! file, refused/slow connection, wrong token) falls back to a normal start, so
//! forwarding can never prevent Slate from opening.
//!
//! The token keeps other local users/processes that cannot read the user's data
//! directory from injecting open requests. Payloads are a path, a line number
//! and, for a new window, where to put it and the moved tab's handoff file;
//! documents are never transferred.
//!
//! The same channel claims the ready hidden window kept by `spare`.

use std::{
    hash::{BuildHasher, Hasher},
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(150);
const REPLY_TIMEOUT: Duration = Duration::from_millis(1500);
const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OpenRequest {
    pub(crate) path: Option<PathBuf>,
    pub(crate) line: Option<usize>,
    /// New windows only: a moved tab's text, and the window's outer
    /// top-left in points.
    pub(crate) handoff: Option<PathBuf>,
    pub(crate) position: Option<(f32, f32)>,
    /// A tab dropped onto this window, at this screen point (physical
    /// pixels): add it as a tab there (see `tab_drop`).
    pub(crate) tab_at: Option<(f32, f32)>,
    /// New window that follows a dragged tab: show without taking focus.
    pub(crate) follow: bool,
    /// A tab dragged over this window, labelled so (`tab_at` is where);
    /// an empty label with no `tab_at` means the drag left.
    pub(crate) hover: Option<String>,
    /// Send this window's tab to the window listening at this file, as if
    /// dropped at `tab_at` (the end of a drag that tore it off).
    pub(crate) send_to: Option<PathBuf>,
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs_next::data_local_dir)?;
    Some(base.join("slate"))
}

pub(crate) fn instance_file() -> Option<PathBuf> {
    Some(data_dir()?.join("instance"))
}

/// Where the ready hidden window (see `spare`) listens.
pub(crate) fn spare_file() -> Option<PathBuf> {
    Some(data_dir()?.join("spare"))
}

/// Tries to hand `request` to a running Slate. Returns true only when the
/// running instance acknowledged it.
pub(crate) fn forward(instance_file: &Path, request: &OpenRequest) -> bool {
    // The running (possibly hidden) window is about to show itself; let it take
    // the foreground that this freshly launched process owns. Not while a tab
    // is being dragged: a window taking the foreground then takes the mouse
    // from the window doing the drag, which never hears the button go up.
    if request.hover.is_none() && !request.follow && request.send_to.is_none() {
        crate::resident::allow_foreground_handoff();
    }
    send(instance_file, |token| encode_request(token, request))
}

/// The process listening at `file`, as it recorded itself.
pub(crate) fn listener_process_id(file: &Path) -> Option<u32> {
    std::fs::read_to_string(file)
        .ok()?
        .split_whitespace()
        .nth(2)?
        .parse()
        .ok()
}

/// True when a running Slate answers; it does not show or open anything.
pub(crate) fn is_running(instance_file: &Path) -> bool {
    send(instance_file, |token| format!("{token}\nPING\n"))
}

fn send(instance_file: &Path, encode: impl FnOnce(&str) -> String) -> bool {
    let Ok(contents) = std::fs::read_to_string(instance_file) else {
        return false;
    };
    let Some((port, token)) = parse_instance_file(&contents) else {
        return false;
    };
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(REPLY_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REPLY_TIMEOUT));
    if stream.write_all(encode(&token).as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim_end() == "OK"
}

/// Listener owned by the primary instance. Dropping it removes the instance
/// file if it still points at this process.
pub(crate) struct Server {
    requests: Arc<Mutex<Vec<OpenRequest>>>,
    instance_file: PathBuf,
    contents: String,
}

impl Server {
    /// `wake` is called from the listener thread with each request after
    /// queuing it (the app passes a closure that requests an egui repaint).
    pub(crate) fn start(
        instance_file: PathBuf,
        wake: impl Fn(&OpenRequest) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::listen(instance_file, false, wake)
    }

    /// Accepts a single request, then stops listening and removes its file:
    /// a second claimer is refused and falls back to its own start.
    pub(crate) fn start_one_shot(
        instance_file: PathBuf,
        wake: impl Fn(&OpenRequest) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::listen(instance_file, true, wake)
    }

    fn listen(
        instance_file: PathBuf,
        one_shot: bool,
        wake: impl Fn(&OpenRequest) + Send + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let token = random_token();
        let contents = format!("{port} {token} {}\n", std::process::id());
        if let Some(parent) = instance_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::safe_file::atomic_replace(&instance_file, contents.as_bytes())
            .map_err(std::io::Error::other)?;

        let requests = Arc::new(Mutex::new(Vec::new()));
        let queue = Arc::clone(&requests);
        let (file, written) = (instance_file.clone(), contents.clone());
        std::thread::Builder::new()
            .name("slate-instance".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    if let Some(request) = handle_connection(stream, &token) {
                        if one_shot {
                            remove_if_ours(&file, &written);
                        }
                        if let Ok(mut queue) = queue.lock() {
                            queue.push(request.clone());
                        }
                        wake(&request);
                        if one_shot {
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            requests,
            instance_file,
            contents,
        })
    }

    pub(crate) fn take_requests(&self) -> Vec<OpenRequest> {
        self.requests
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        remove_if_ours(&self.instance_file, &self.contents);
    }
}

pub(crate) fn remove_if_ours(file: &Path, contents: &str) {
    if std::fs::read_to_string(file).is_ok_and(|c| c == contents) {
        let _ = std::fs::remove_file(file);
    }
}

fn handle_connection(stream: TcpStream, token: &str) -> Option<OpenRequest> {
    let _ = stream.set_read_timeout(Some(REPLY_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REPLY_TIMEOUT));
    let mut writer = stream.try_clone().ok()?;
    let mut reader = BufReader::new(std::io::Read::take(stream, MAX_REQUEST_BYTES));
    let mut message = String::new();
    // Exactly two lines: token, then the OPEN or PING record.
    for _ in 0..2 {
        if reader.read_line(&mut message).ok()? == 0 {
            break;
        }
    }
    let ping = is_ping(token, &message);
    let request = if ping {
        None
    } else {
        decode_request(token, &message)
    };
    let reply = if ping || request.is_some() {
        "OK\n"
    } else {
        "ERR\n"
    };
    let _ = writer.write_all(reply.as_bytes());
    request
}

fn is_ping(token: &str, message: &str) -> bool {
    let mut lines = message.lines();
    lines.next() == Some(token) && lines.next() == Some("PING")
}

fn parse_instance_file(contents: &str) -> Option<(u16, String)> {
    let mut parts = contents.split_whitespace();
    let port = parts.next()?.parse().ok()?;
    let token = parts.next()?.to_string();
    (!token.is_empty()).then_some((port, token))
}

fn encode_request(token: &str, request: &OpenRequest) -> String {
    let text = |path: &Option<PathBuf>| {
        path.as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let point = |at: Option<(f32, f32)>| at.map(|(x, y)| format!("{x},{y}")).unwrap_or_default();
    let path = text(&request.path);
    let line = request.line.map(|l| l.to_string()).unwrap_or_default();
    let handoff = text(&request.handoff);
    let at = point(request.tab_at);
    let record = if let Some(label) = &request.hover {
        if request.tab_at.is_none() {
            "LEAVE".to_string()
        } else {
            format!("HOVER\t{at}\t{label}")
        }
    } else if request.send_to.is_some() {
        format!("SENDTO\t{at}\t{}", text(&request.send_to))
    } else if request.tab_at.is_some() {
        format!("TAB\t{at}\t{handoff}\t{path}")
    } else if request.handoff.is_none() && request.position.is_none() && !request.follow {
        format!("OPEN\t{line}\t{path}")
    } else {
        let position = point(request.position);
        let follow = if request.follow { "follow" } else { "" };
        format!("WINDOW\t{line}\t{position}\t{handoff}\t{follow}\t{path}")
    };
    format!("{token}\n{record}\n")
}

fn decode_request(token: &str, message: &str) -> Option<OpenRequest> {
    let mut lines = message.lines();
    if lines.next()? != token {
        return None;
    }
    let record = lines.next()?;
    let point = |text: &str| -> Option<Option<(f32, f32)>> {
        if text.is_empty() {
            return Some(None);
        }
        let (x, y) = text.split_once(',')?;
        Some(Some((x.parse().ok()?, y.parse().ok()?)))
    };
    let file = |text: &str| (!text.is_empty()).then(|| PathBuf::from(text));
    let (kind, rest) = record.split_once('\t').unwrap_or((record, ""));
    // Paths and labels go last: they may themselves contain tabs.
    match kind {
        "LEAVE" => Some(OpenRequest {
            hover: Some(String::new()),
            ..Default::default()
        }),
        "HOVER" => {
            let (at, label) = rest.split_once('\t')?;
            Some(OpenRequest {
                tab_at: Some(point(at)??),
                hover: Some(label.to_string()),
                ..Default::default()
            })
        }
        "SENDTO" => {
            let (at, target) = rest.split_once('\t')?;
            Some(OpenRequest {
                tab_at: Some(point(at)??),
                send_to: Some(file(target)?),
                ..Default::default()
            })
        }
        "TAB" => {
            let mut fields = rest.splitn(3, '\t');
            let at = point(fields.next()?)??;
            let handoff = fields.next()?;
            let path = fields.next()?;
            Some(OpenRequest {
                path: file(path),
                handoff: file(handoff),
                tab_at: Some(at),
                ..Default::default()
            })
        }
        "OPEN" | "WINDOW" => {
            let window = kind == "WINDOW";
            let mut fields = rest.splitn(if window { 5 } else { 2 }, '\t');
            let line = fields.next()?;
            let (position, handoff, follow) = if window {
                (
                    point(fields.next()?)?,
                    file(fields.next()?),
                    fields.next()? == "follow",
                )
            } else {
                (None, None, false)
            };
            let path = fields.next()?;
            Some(OpenRequest {
                line: if line.is_empty() {
                    None
                } else {
                    Some(line.parse().ok()?)
                },
                path: file(path),
                handoff,
                position,
                follow,
                ..Default::default()
            })
        }
        _ => None,
    }
}

/// 128 bits from std's per-process randomly seeded hasher keys.
fn random_token() -> String {
    let mut token = String::new();
    for salt in 0..2u64 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(salt);
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        );
        hasher.write_u32(std::process::id());
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn temp_instance_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("slate-instance-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("instance")
    }

    #[test]
    fn request_round_trips_through_encoding() {
        let request = OpenRequest {
            path: Some(PathBuf::from("C:\\notas\\día\ttab.md")),
            line: Some(42),
            ..Default::default()
        };
        let encoded = encode_request("abc", &request);
        assert!(
            encoded.contains("\nOPEN\t"),
            "plain opens keep the old record"
        );
        assert_eq!(decode_request("abc", &encoded), Some(request));
        assert_eq!(decode_request("wrong", &encoded), None);
        let empty = OpenRequest::default();
        assert_eq!(
            decode_request("abc", &encode_request("abc", &empty)),
            Some(empty)
        );
        let window = OpenRequest {
            path: Some(PathBuf::from("C:\\notas\\día\ttab.md")),
            line: None,
            handoff: Some(PathBuf::from("C:\\data\\handoff\\1-2.json")),
            position: Some((-1280.5, 40.0)),
            follow: true,
            ..Default::default()
        };
        let encoded = encode_request("abc", &window);
        assert!(encoded.contains("\nWINDOW\t"));
        assert_eq!(decode_request("abc", &encoded), Some(window));
        let tab = OpenRequest {
            path: Some(PathBuf::from("C:\\notas\\día\ttab.md")),
            handoff: Some(PathBuf::from("C:\\data\\handoff\\1-2.json")),
            tab_at: Some((2560.0, -12.0)),
            ..Default::default()
        };
        let encoded = encode_request("abc", &tab);
        assert!(encoded.contains("\nTAB\t"));
        assert_eq!(decode_request("abc", &encoded), Some(tab));
        for request in [
            OpenRequest {
                tab_at: Some((10.5, 20.0)),
                hover: Some("notas\tdía.md".into()),
                ..Default::default()
            },
            OpenRequest {
                hover: Some(String::new()),
                ..Default::default()
            },
            OpenRequest {
                tab_at: Some((1.0, 2.0)),
                send_to: Some(PathBuf::from("C:\\data\\windows\\42")),
                ..Default::default()
            },
        ] {
            let encoded = encode_request("abc", &request);
            assert_eq!(decode_request("abc", &encoded), Some(request), "{encoded}");
        }
    }

    #[test]
    fn forwards_open_requests_to_the_running_instance() {
        let file = temp_instance_file("forward");
        let (woke_tx, woke_rx) = mpsc::channel();
        let server = Server::start(file.clone(), move |_| {
            let _ = woke_tx.send(());
        })
        .unwrap();
        let request = OpenRequest {
            path: Some(PathBuf::from("/tmp/notes.md")),
            line: Some(7),
            ..Default::default()
        };

        assert!(forward(&file, &request));
        woke_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(server.take_requests(), vec![request]);

        drop(server);
        assert!(!file.exists(), "instance file is removed on shutdown");
    }

    #[test]
    fn ping_answers_without_queuing_or_waking() {
        let file = temp_instance_file("ping");
        assert!(!is_running(&file), "no instance yet");
        let (woke_tx, woke_rx) = mpsc::channel();
        let server = Server::start(file.clone(), move |_| {
            let _ = woke_tx.send(());
        })
        .unwrap();

        assert!(is_running(&file));
        assert!(woke_rx.recv_timeout(Duration::from_millis(200)).is_err());
        assert!(server.take_requests().is_empty());
    }

    #[test]
    fn stale_or_forged_instance_files_fall_back_to_normal_start() {
        let file = temp_instance_file("stale");
        let request = OpenRequest::default();
        assert!(!forward(&file, &request), "missing file");

        // A port nobody listens on any more.
        let closed_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(&file, format!("{closed_port} token\n")).unwrap();
        assert!(!forward(&file, &request), "stale port");

        let server = Server::start(file.clone(), |_| {}).unwrap();
        let port = parse_instance_file(&std::fs::read_to_string(&file).unwrap())
            .unwrap()
            .0;
        std::fs::write(&file, format!("{port} forged-token\n")).unwrap();
        assert!(!forward(&file, &request), "wrong token is rejected");
        assert!(server.take_requests().is_empty());
    }

    #[test]
    fn a_one_shot_listener_takes_one_claim_then_steps_aside() {
        let file = temp_instance_file("one-shot");
        let (woke_tx, woke_rx) = mpsc::channel();
        let server = Server::start_one_shot(file.clone(), move |request| {
            let _ = woke_tx.send(request.clone());
        })
        .unwrap();
        assert!(is_running(&file), "pings do not use up the claim");
        let claim = OpenRequest {
            position: Some((10.0, 20.0)),
            ..Default::default()
        };
        assert!(forward(&file, &claim));
        assert_eq!(woke_rx.recv_timeout(Duration::from_secs(2)).unwrap(), claim);
        assert!(!file.exists(), "a claimed window is no longer offered");
        assert!(!forward(&file, &claim));
        assert_eq!(server.take_requests(), vec![claim]);
    }
}
