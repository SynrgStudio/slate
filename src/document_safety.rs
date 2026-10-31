//! Coalesced checkpoints and independent external-edit checks; never auto-writes source files.
use crate::{
    perf_log,
    safe_file::{self, Fingerprint},
};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub(crate) enum Slot {
    Main,
    Modal,
    Scratch,
    /// An inactive editor tab. Switching tabs swaps roles with `Main`, so each
    /// document keeps its own durable record.
    Tab(u64),
}
impl Slot {
    fn name(self) -> String {
        match self {
            Self::Main => "main.json".into(),
            Self::Modal => "modal.json".into(),
            Self::Scratch => "scratch.json".into(),
            Self::Tab(id) => format!("tab-{id}.json"),
        }
    }
    fn is_record_name(name: &str) -> bool {
        matches!(name, "main.json" | "modal.json" | "scratch.json")
            || name
                .strip_prefix("tab-")
                .and_then(|rest| rest.strip_suffix(".json"))
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Draft {
    pub version: u8,
    pub slot: Slot,
    pub path: Option<PathBuf>,
    pub base: Option<Fingerprint>,
    pub text: String,
    pub cursor: usize,
    pub saved_ms: u64,
}
#[derive(Clone)]
pub(crate) struct Recovered {
    pub record: PathBuf,
    pub draft: Draft,
}
pub(crate) enum Event {
    Found(Vec<Recovered>),
    Changed {
        slot: Slot,
        path: PathBuf,
        expected: Option<Fingerprint>,
    },
    Error(String),
    CheckpointFailed(Slot),
}
enum Job {
    Put(Draft, Vec<PathBuf>),
    Clear(Slot, Vec<PathBuf>),
}
impl Job {
    fn retirements(&mut self) -> &mut Vec<PathBuf> {
        match self {
            Self::Put(_, paths) | Self::Clear(_, paths) => paths,
        }
    }
}
enum Command {
    Wake,
    Flush(mpsc::Sender<()>),
    Stop,
}
type Pending = Arc<Mutex<HashMap<Slot, Job>>>;
type Watches = Arc<Mutex<HashMap<Slot, (PathBuf, Option<Fingerprint>)>>>;
#[derive(Clone, Eq, PartialEq)]
struct Stamp {
    path: Option<PathBuf>,
    revision: u64,
    dirty: bool,
    base: Option<Fingerprint>,
}
struct Seen {
    stamp: Stamp,
    last: f64,
}
pub(crate) struct Manager {
    storage: HashMap<Slot, Slot>,
    sender: Option<mpsc::Sender<Command>>,
    pending: Pending,
    watch_sender: Option<mpsc::SyncSender<()>>,
    watch_pending: Watches,
    events: mpsc::Receiver<Event>,
    seen: HashMap<Slot, Seen>,
    watches: HashMap<Slot, f64>,
}
pub(crate) fn default_root() -> Option<PathBuf> {
    Some(dirs_next::data_local_dir()?.join("slate").join("recovery"))
}
impl Manager {
    pub fn disabled() -> Self {
        let (_, events) = mpsc::channel();
        Self {
            storage: HashMap::new(),
            sender: None,
            pending: Arc::default(),
            watch_sender: None,
            watch_pending: Arc::default(),
            events,
            seen: HashMap::new(),
            watches: HashMap::new(),
        }
    }
    fn failed(message: String) -> Self {
        let mut manager = Self::disabled();
        let (sender, events) = mpsc::channel();
        let _ = sender.send(Event::Error(message));
        manager.events = events;
        manager
    }
    pub fn new(ctx: egui::Context, root: Option<PathBuf>) -> Self {
        let Some(root) = root else {
            return Self::failed("Recovery unavailable: local data directory not found".into());
        };
        let (sender, commands) = mpsc::channel();
        let (report, events) = mpsc::channel();
        let pending: Pending = Arc::default();
        let session = root.join(format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let queue = pending.clone();
        let worker_root = root.clone();
        let worker_session = session.clone();
        let worker_report = report.clone();
        let worker_ctx = ctx.clone();
        let spawn = std::thread::Builder::new()
            .name("slate-recovery-writer".into())
            .spawn(move || {
                while let Ok(command) = commands.recv() {
                    let jobs =
                        std::mem::take(&mut *queue.lock().unwrap_or_else(|e| e.into_inner()));
                    for (slot, job) in jobs {
                        let result = match job {
                            Job::Put(draft, retire) => (|| {
                                private_dir(&worker_session)?;
                                let data = serde_json::to_vec(&draft).map_err(|e| e.to_string())?;
                                safe_file::atomic_replace(
                                    &worker_session.join(slot.name()),
                                    &data,
                                )?;
                                for path in retire {
                                    retire_record(&worker_root, &path)?;
                                }
                                Ok::<_, String>(())
                            })(),
                            Job::Clear(slot, retire) => (|| {
                                remove_if_exists(&worker_session.join(slot.name()))?;
                                for path in retire {
                                    retire_record(&worker_root, &path)?;
                                }
                                Ok::<_, String>(())
                            })(),
                        };
                        if let Err(error) = result {
                            let _ = worker_report.send(Event::CheckpointFailed(slot));
                            let _ = worker_report
                                .send(Event::Error(format!("Recovery checkpoint failed: {error}")));
                            worker_ctx.request_repaint();
                        }
                    }
                    match command {
                        Command::Flush(done) => {
                            let _ = done.send(());
                        }
                        Command::Stop => {
                            let _ = fs::remove_dir(&worker_session);
                            break;
                        }
                        Command::Wake => {}
                    }
                }
            });
        if let Err(error) = spawn {
            return Self::failed(format!("Recovery worker unavailable: {error}"));
        }
        // Watch reads can stall on a network drive; they must not hold up local checkpoints.
        let (watch_sender, watch_jobs) = mpsc::sync_channel(1);
        let watch_pending: Watches = Arc::default();
        let watch_queue = watch_pending.clone();
        let watch_report = report.clone();
        let watch_ctx = ctx.clone();
        let watch_spawn = std::thread::Builder::new()
            .name("slate-external-file-watch".into())
            .spawn(move || {
                while watch_jobs.recv().is_ok() {
                    let jobs =
                        std::mem::take(&mut *watch_queue.lock().unwrap_or_else(|e| e.into_inner()));
                    for (slot, (path, expected)) in jobs {
                        match safe_file::disk_fingerprint(&path) {
                            Ok(current) if current != expected => {
                                let _ = watch_report.send(Event::Changed {
                                    slot,
                                    path,
                                    expected,
                                });
                                watch_ctx.request_repaint();
                            }
                            Ok(_) => {}
                            Err(error) => perf_log::mark(
                                "document.watch.failed",
                                &format!("path={path:?} error={error}"),
                            ),
                        }
                    }
                }
            });
        let watch_sender = match watch_spawn {
            Ok(_) => Some(watch_sender),
            Err(error) => {
                let _ = report.send(Event::Error(format!(
                    "External file watcher unavailable: {error}"
                )));
                None
            }
        };
        // Discovery is independent of both the writer and the watcher.
        let scan_report = report.clone();
        let scan_ctx = ctx.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("slate-recovery-discovery".into())
            .spawn(move || {
                let found = scan(&root, &session);
                if !found.is_empty() {
                    let _ = scan_report.send(Event::Found(found));
                    scan_ctx.request_repaint();
                }
            })
        {
            let _ = report.send(Event::Error(format!("Recovery discovery failed: {error}")));
        }
        Self {
            storage: HashMap::new(),
            sender: Some(sender),
            pending,
            watch_sender,
            watch_pending,
            events,
            seen: HashMap::new(),
            watches: HashMap::new(),
        }
    }
    fn record_slot(&self, slot: Slot) -> Slot {
        self.storage.get(&slot).copied().unwrap_or(slot)
    }
    pub fn swap_roles(&mut self, first: Slot, second: Slot) {
        // Keep each buffer's durable record identity across UI swaps/promotions.
        let a = self.record_slot(first);
        let b = self.record_slot(second);
        self.storage.insert(first, b);
        self.storage.insert(second, a);
        let a = self.seen.remove(&first);
        let b = self.seen.remove(&second);
        if let Some(seen) = a {
            self.seen.insert(second, seen);
        }
        if let Some(seen) = b {
            self.seen.insert(first, seen);
        }
        self.watches.clear();
    }
    fn enqueue(&self, slot: Slot, mut job: Job) {
        let Some(sender) = &self.sender else {
            return;
        };
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let wake = pending.is_empty();
        if let Some(mut previous) = pending.remove(&slot) {
            job.retirements().append(previous.retirements());
            job.retirements().sort();
            job.retirements().dedup();
        }
        pending.insert(slot, job);
        if wake {
            let _ = sender.send(Command::Wake);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &mut self,
        slot: Slot,
        path: Option<&Path>,
        base: Option<Fingerprint>,
        text: &str,
        revision: u64,
        cursor: usize,
        dirty: bool,
        now: f64,
        force: bool,
        retire: Option<PathBuf>,
    ) {
        if self.sender.is_none() {
            return;
        }
        let stamp = Stamp {
            path: path.map(Path::to_path_buf),
            revision,
            dirty,
            base,
        };
        let submit = force
            || self
                .seen
                .get(&slot)
                .is_none_or(|seen| seen.stamp != stamp && (!dirty || now - seen.last >= 1.0));
        if submit {
            let record_slot = self.record_slot(slot);
            let job = if dirty {
                Job::Put(
                    Draft {
                        version: 1,
                        slot: record_slot,
                        path: stamp.path.clone(),
                        base,
                        text: text.to_string(),
                        cursor,
                        saved_ms: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    },
                    retire.into_iter().collect(),
                )
            } else {
                Job::Clear(record_slot, retire.into_iter().collect())
            };
            self.enqueue(record_slot, job);
            self.seen.insert(slot, Seen { stamp, last: now });
        }
        if let (Some(path), Some(sender)) = (path, &self.watch_sender) {
            if !force && self.watches.get(&slot).is_some_and(|last| now - last < 2.0) {
                return;
            }
            self.watch_pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(slot, (path.to_path_buf(), base));
            let _ = sender.try_send(());
            self.watches.insert(slot, now);
        }
    }
    pub fn clear(&mut self, slot: Slot, retire: Option<PathBuf>) {
        let record_slot = self.record_slot(slot);
        self.enqueue(
            record_slot,
            Job::Clear(record_slot, retire.into_iter().collect()),
        );
        self.seen.remove(&slot);
    }
    pub fn poll(&mut self) -> Option<Event> {
        let event = self.events.try_recv().ok()?;
        if let Event::CheckpointFailed(slot) = &event {
            let logical = [Slot::Main, Slot::Modal, Slot::Scratch]
                .into_iter()
                .chain(self.storage.keys().copied())
                .find(|s| self.record_slot(*s) == *slot)
                .unwrap_or(*slot);
            self.seen.remove(&logical);
        }
        Some(event)
    }
    pub fn flush(&self) {
        if let Some(sender) = &self.sender {
            let (done, wait) = mpsc::channel();
            if sender.send(Command::Flush(done)).is_ok()
                && wait.recv_timeout(Duration::from_secs(2)).is_err()
            {
                perf_log::mark("recovery.flush.timeout", "last durable checkpoint retained");
            }
        }
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.flush();
        if let Some(sender) = &self.sender {
            let _ = sender.send(Command::Stop);
        }
    }
}
fn private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn remove_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn retire_record(root: &Path, path: &Path) -> Result<(), String> {
    if !path.starts_with(root)
        || !Slot::is_record_name(path.file_name().and_then(|n| n.to_str()).unwrap_or(""))
    {
        return Err("invalid recovery record path".into());
    }
    if path.parent().and_then(Path::parent) != Some(root) {
        return Err("invalid recovery session path".into());
    }
    let parent = path.parent().unwrap();
    match fs::symlink_metadata(parent) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        _ => {}
    }
    if is_link(parent) {
        return Err("linked recovery session refused".into());
    }
    remove_if_exists(path)
}
fn is_link(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return true;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}
fn scan(root: &Path, own: &Path) -> Vec<Recovered> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if folder == own || is_link(&folder) || !folder.is_dir() {
            continue;
        }
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.split('-').next()?.parse::<u32>().ok())
        else {
            continue;
        };
        if process_alive(pid) {
            continue;
        }
        let mut names: Vec<String> = fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .filter(|name| Slot::is_record_name(name))
            .collect();
        names.sort();
        for name in names {
            let record = folder.join(&name);
            if is_link(&record) {
                continue;
            }
            match fs::read(&record)
                .map_err(|e| e.to_string())
                .and_then(|data| serde_json::from_slice::<Draft>(&data).map_err(|e| e.to_string()))
            {
                Ok(draft) if draft.version == 1 && draft.slot.name() == name => {
                    found.push(Recovered { record, draft })
                }
                Ok(_) => {
                    perf_log::mark("recovery.invalid", "unsupported recovery record; retained")
                }
                Err(e) => perf_log::mark(
                    "recovery.read.failed",
                    &format!("path={record:?} error={e}; retained"),
                ),
            }
        }
    }
    found.sort_by_key(|item| std::cmp::Reverse(item.draft.saved_ms));
    found
}
#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(handle: *mut c_void, exit: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    unsafe {
        let handle = OpenProcess(0x1000, 0, pid);
        if handle.is_null() {
            return std::io::Error::last_os_error().raw_os_error() != Some(87);
        }
        let mut exit = 0;
        let ok = GetExitCodeProcess(handle, &mut exit);
        CloseHandle(handle);
        ok == 0 || exit == 259
    }
}
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    unsafe { kill(pid as i32, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1) }
}
#[cfg(not(any(unix, windows)))]
fn process_alive(_pid: u32) -> bool {
    true
}
#[cfg(test)]
#[path = "document_safety_tests.rs"]
mod tests;
