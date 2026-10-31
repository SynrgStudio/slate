//! Temporary timing instrumentation. Disk writes happen on a dedicated thread.
//! Records metadata only: paths, counts, elapsed times, never document contents.
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Instant,
};

enum Record {
    Line(String),
    Flush(mpsc::Sender<()>),
}
struct Logger {
    started: Instant,
    tx: mpsc::Sender<Record>,
    first_frame: AtomicBool,
    pending_opens: Mutex<Vec<PendingOpen>>,
}
struct PendingOpen {
    id: u64,
    started: Instant,
    loaded: Instant,
    source_ui_end: Option<Instant>,
}
static LOGGER: OnceLock<Logger> = OnceLock::new();
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Time since `main` started, independent of whether the log file could be created.
pub fn since_start() -> Option<std::time::Duration> {
    PROCESS_START.get().map(Instant::elapsed)
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn init(started: Instant) {
    let _ = PROCESS_START.set(started);
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs_next::data_local_dir)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("slate").join("logs");
    let path = dir.join(format!(
        "performance-{}-{}.log",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        std::process::id()
    ));
    let Ok(file) = fs::create_dir_all(&dir).and_then(|_| File::create(&path)) else {
        eprintln!("Slate: could not create timing log {}", path.display());
        return;
    };
    let (tx, rx) = mpsc::channel::<Record>();
    let worker = std::thread::Builder::new()
        .name("slate-perf-log".into())
        .spawn(move || {
            let mut writer = BufWriter::new(file);
            for record in rx {
                match record {
                    Record::Line(line) => {
                        if writeln!(writer, "{line}")
                            .and_then(|_| writer.flush())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Record::Flush(done) => {
                        let _ = writer.flush();
                        let _ = done.send(());
                    }
                }
            }
        });
    if worker.is_err() {
        return;
    }
    let _ = LOGGER.set(Logger {
        started,
        tx,
        first_frame: AtomicBool::new(true),
        pending_opens: Mutex::new(Vec::new()),
    });
    mark(
        "process.main_enter",
        "clock starts at main; excludes OS loader/Defender before main",
    );
    mark(
        "logging.ready",
        &format!(
            "log={:?} build={} os={} sqlite=embedded sqlite_version={}",
            path,
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            std::env::consts::OS,
            rusqlite::version()
        ),
    );
}

pub fn mark(event: &str, detail: &str) {
    if let Some(log) = LOGGER.get() {
        let _ = log.tx.send(Record::Line(format!(
            "{} elapsed_ms={:.3} event={} {}",
            chrono::Utc::now().to_rfc3339(),
            log.started.elapsed().as_secs_f64() * 1000.0,
            event,
            detail.replace(['\r', '\n'], " ")
        )));
    }
}

pub fn flush() {
    if let Some(log) = LOGGER.get() {
        let (tx, rx) = mpsc::channel();
        if log.tx.send(Record::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(1));
        }
    }
}

pub struct Span {
    id: u64,
    name: &'static str,
    started: Instant,
    last: Instant,
}
impl Span {
    pub fn new(name: &'static str, detail: &str) -> Self {
        let now = Instant::now();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        mark(name, &format!("id={id} phase=begin {detail}"));
        Self {
            id,
            name,
            started: now,
            last: now,
        }
    }
    pub fn step(&mut self, step: &str) {
        let now = Instant::now();
        mark(
            self.name,
            &format!(
                "id={} step={} step_ms={:.3} total_ms={:.3}",
                self.id,
                step,
                now.duration_since(self.last).as_secs_f64() * 1000.0,
                now.duration_since(self.started).as_secs_f64() * 1000.0
            ),
        );
        self.last = now;
    }
    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
    pub fn wait_for_ui(&self) {
        if let Some(log) = LOGGER.get() {
            if let Ok(mut pending) = log.pending_opens.lock() {
                pending.push(PendingOpen {
                    id: self.id,
                    started: self.started,
                    loaded: Instant::now(),
                    source_ui_end: None,
                });
            }
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        mark(
            self.name,
            &format!(
                "id={} phase=end duration_ms={:.3}",
                self.id,
                self.started.elapsed().as_secs_f64() * 1000.0
            ),
        );
    }
}

pub struct UiFrame {
    first: bool,
    started: Instant,
    last: Instant,
    opens: Vec<PendingOpen>,
}
impl UiFrame {
    pub fn step(&mut self, name: &str) {
        if self.first || !self.opens.is_empty() {
            let now = Instant::now();
            mark(
                "ui.measured_frame",
                &format!(
                    "step={name} step_ms={:.3}",
                    now.duration_since(self.last).as_secs_f64() * 1000.0
                ),
            );
            self.last = now;
        }
    }
}
pub fn ui_frame() -> UiFrame {
    let started = Instant::now();
    let mut frame = UiFrame {
        first: false,
        started,
        last: started,
        opens: Vec::new(),
    };
    if let Some(log) = LOGGER.get() {
        frame.first = log.first_frame.swap(false, Ordering::Relaxed);
        if let Ok(mut pending) = log.pending_opens.lock() {
            frame.opens = std::mem::take(&mut *pending);
        }
    }
    if frame.first {
        mark("startup.first_ui_begin", "");
    }
    for open in &frame.opens {
        mark(
            "file_open.next_ui_begin",
            &format!(
                "id={} loaded_to_ui_begin_ms={:.3} between_ui_frames_ms={}",
                open.id,
                started.duration_since(open.loaded).as_secs_f64() * 1000.0,
                open.source_ui_end
                    .map(|end| format!("{:.3}", started.duration_since(end).as_secs_f64() * 1000.0))
                    .unwrap_or_else(|| "n/a".to_string())
            ),
        );
    }
    frame
}
impl Drop for UiFrame {
    fn drop(&mut self) {
        self.step("remaining_ui_build");
        // Opens triggered late in this frame have not yet built the new editor.
        // Record its end without consuming them; the next frame measures the gap.
        if let Some(log) = LOGGER.get() {
            if let Ok(mut pending) = log.pending_opens.lock() {
                for open in pending
                    .iter_mut()
                    .filter(|open| open.source_ui_end.is_none())
                {
                    let now = Instant::now();
                    open.source_ui_end = Some(now);
                    mark(
                        "file_open.source_ui_end",
                        &format!(
                            "id={} after_load_ms={:.3}",
                            open.id,
                            now.duration_since(open.loaded).as_secs_f64() * 1000.0
                        ),
                    );
                }
            }
        }
        if self.first {
            mark(
                "startup.first_ui_end",
                &format!(
                    "ui_ms={:.3} CPU UI built; not GPU present",
                    self.started.elapsed().as_secs_f64() * 1000.0
                ),
            );
        }
        for open in &self.opens {
            mark(
                "file_open.ui_frame_end",
                &format!(
                    "id={} open_to_ui_ms={:.3} next_ui_cpu_ms={:.3} CPU UI built; not GPU present",
                    open.id,
                    open.started.elapsed().as_secs_f64() * 1000.0,
                    self.started.elapsed().as_secs_f64() * 1000.0
                ),
            );
        }
    }
}
