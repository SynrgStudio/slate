//! Bounded cold-file I/O, with one coalesced queued request and no UI-thread joins.
use crate::{editor_buffer::EditorBuffer, perf_log};
use eframe::egui;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
};
type Reader = dyn Fn(&Path, &mut perf_log::Span) -> Result<EditorBuffer, String> + Send + Sync;
struct Job {
    id: u64,
    path: PathBuf,
    timing: perf_log::Span,
}
#[derive(Default)]
struct Queue {
    next: Option<Job>,
    closed: bool,
}
#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}
pub(crate) struct Loaded {
    pub id: u64,
    pub path: PathBuf,
    pub result: Result<EditorBuffer, String>,
    pub fingerprint: Option<crate::safe_file::Fingerprint>,
    pub timing: perf_log::Span,
}
pub(crate) struct Loader {
    shared: Arc<Shared>,
    sender: mpsc::SyncSender<Loaded>,
    receiver: mpsc::Receiver<Loaded>,
    reader: Arc<Reader>,
    ctx: egui::Context,
    started: bool,
    next_id: u64,
}
impl Loader {
    pub fn new(ctx: egui::Context) -> Self {
        Self::with_reader(ctx, read_file)
    }
    pub(crate) fn with_reader(
        ctx: egui::Context,
        reader: impl Fn(&Path, &mut perf_log::Span) -> Result<EditorBuffer, String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel(2);
        Self {
            shared: Arc::default(),
            sender,
            receiver,
            reader: Arc::new(reader),
            ctx,
            started: false,
            next_id: 0,
        }
    }
    fn start(&mut self) -> Result<(), String> {
        if self.started {
            return Ok(());
        }
        let mut count = 0;
        for index in 0..2 {
            let shared = self.shared.clone();
            let sender = self.sender.clone();
            let reader = self.reader.clone();
            let ctx = self.ctx.clone();
            match std::thread::Builder::new()
                .name(format!("slate-file-reader-{index}"))
                .spawn(move || {
                    loop {
                        let mut job = {
                            let Ok(mut queue) = shared.queue.lock() else {
                                return;
                            };
                            while queue.next.is_none() && !queue.closed {
                                queue = match shared.wake.wait(queue) {
                                    Ok(q) => q,
                                    Err(_) => return,
                                };
                            }
                            if queue.closed {
                                return;
                            }
                            queue.next.take().expect("queued job")
                        };
                        job.timing.step("worker_queue_wait");
                        let result = reader(&job.path, &mut job.timing);
                        let fingerprint = result.as_ref().ok().map(|buffer| {
                            crate::safe_file::fingerprint(buffer.as_str().as_bytes())
                        });
                        job.timing.step("fingerprint_worker");
                        if sender
                            .send(Loaded {
                                id: job.id,
                                path: job.path,
                                result,
                                fingerprint,
                                timing: job.timing,
                            })
                            .is_err()
                        {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }) {
                Ok(_) => count += 1,
                Err(err) => perf_log::mark("file_open.worker_spawn_failed", &err.to_string()),
            }
        }
        self.started = count > 0;
        if self.started {
            Ok(())
        } else {
            Err("cannot start file-reading worker".into())
        }
    }
    pub fn request(&mut self, path: PathBuf) -> Result<u64, String> {
        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        let mut timing = perf_log::Span::new(
            "file_open",
            &format!("request={id} path={path:?} async=true"),
        );
        self.start()?;
        timing.step("worker_pool_ready");
        timing.step("enqueue_read");
        let mut queue = self.shared.queue.lock().map_err(|e| e.to_string())?;
        if let Some(old) = queue.next.replace(Job { id, path, timing }) {
            perf_log::mark(
                "file_open.superseded_before_read",
                &format!("request={}", old.id),
            );
        }
        self.shared.wake.notify_one();
        self.ctx.request_repaint();
        Ok(id)
    }
    pub fn try_recv(&self) -> Option<Loaded> {
        self.receiver.try_recv().ok()
    }
    pub fn cancel_queued(&self) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.next = None;
        }
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.closed = true;
            queue.next = None;
        }
        self.shared.wake.notify_all();
        // In-flight OS reads cannot be cancelled portably. Dropping the receiver releases
        // blocked sends; never join a slow filesystem reader on the UI/closing thread.
    }
}
fn read_file(path: &Path, timing: &mut perf_log::Span) -> Result<EditorBuffer, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    timing.step("open_file");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    timing.step("read_bytes");
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    timing.step("decode_utf8");
    let buffer = EditorBuffer::from_text(text);
    timing.step("build_buffer_worker");
    Ok(buffer)
}
