//! Writes settings and session files off the UI thread.
//!
//! The session is replaced atomically with fsync (~20 ms), and both files are
//! saved on file opens, tab changes and setting toggles. One writer thread
//! keeps only the newest pending contents per file; `flush` waits for it
//! before the process exits. Tests write synchronously.

use std::{
    path::PathBuf,
    sync::{Condvar, Mutex, OnceLock},
    time::Duration,
};

#[derive(Clone, Copy)]
pub(crate) enum Durability {
    /// Write a temp file and rename it over the destination.
    Quick,
    /// `safe_file::atomic_replace`: synced to disk before it replaces the file.
    Synced,
}

struct Job {
    path: PathBuf,
    bytes: Vec<u8>,
    durability: Durability,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    busy: bool,
}

struct Writer {
    queue: Mutex<Queue>,
    changed: Condvar,
}

static WRITER: OnceLock<&'static Writer> = OnceLock::new();

fn writer() -> &'static Writer {
    WRITER.get_or_init(|| {
        let writer: &'static Writer = Box::leak(Box::new(Writer {
            queue: Mutex::new(Queue::default()),
            changed: Condvar::new(),
        }));
        std::thread::Builder::new()
            .name("slate-settings-writer".into())
            .spawn(move || run(writer))
            .expect("spawn settings writer");
        writer
    })
}

pub(crate) fn write(path: PathBuf, bytes: Vec<u8>, durability: Durability) {
    let job = Job {
        path,
        bytes,
        durability,
    };
    if cfg!(test) {
        perform(job);
        return;
    }
    let writer = writer();
    let mut queue = writer.queue.lock().unwrap();
    queue.jobs.retain(|old| old.path != job.path);
    queue.jobs.push(job);
    writer.changed.notify_all();
}

/// Waits (up to 2 s) until everything handed to `write` is on disk.
pub(crate) fn flush() {
    let Some(writer) = WRITER.get() else {
        return;
    };
    let queue = writer.queue.lock().unwrap();
    let _ = writer
        .changed
        .wait_timeout_while(queue, Duration::from_secs(2), |queue| {
            queue.busy || !queue.jobs.is_empty()
        });
}

fn run(writer: &Writer) {
    loop {
        let job = {
            let mut queue = writer
                .changed
                .wait_while(writer.queue.lock().unwrap(), |queue| queue.jobs.is_empty())
                .unwrap();
            queue.busy = true;
            queue.jobs.remove(0)
        };
        perform(job);
        writer.queue.lock().unwrap().busy = false;
        writer.changed.notify_all();
    }
}

fn perform(job: Job) {
    let result = match job.durability {
        Durability::Synced => crate::safe_file::atomic_replace(&job.path, &job.bytes),
        Durability::Quick => {
            let temp = job.path.with_extension("tmp-write");
            std::fs::write(&temp, &job.bytes)
                .and_then(|()| std::fs::rename(&temp, &job.path))
                .map_err(|err| err.to_string())
        }
    };
    if let Err(err) = result {
        crate::perf_log::mark(
            "settings.write_failed",
            &format!("path={:?} error={err}", job.path),
        );
    }
}
