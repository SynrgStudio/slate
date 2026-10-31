//! One background thread for Flow writes that the editor does not wait on.
//!
//! The active-buffer heartbeat (every 3 s while a Flow file is open) and the
//! index refresh after a save both touch shared files/SQLite that other Slate
//! processes also write. On the UI thread they were usually < 1 ms but spiked
//! to 10 ms (p99) and 184 ms (max) when they hit a lock, freezing typing. Jobs
//! run in order on a single thread, so an older save never lands after a newer
//! one; heartbeats and saves queued behind a slow job are coalesced.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;

use crate::{flow, flow_runtime};

enum Job {
    Heartbeat {
        sqlite_path: PathBuf,
        scope: PathBuf,
        path: PathBuf,
    },
    SaveIndex {
        project_root: PathBuf,
        sqlite_path: PathBuf,
        path: PathBuf,
        contents: String,
        seq: u64,
    },
}

pub(crate) struct SaveIndexResult {
    seq: u64,
    pub(crate) path: PathBuf,
    pub(crate) result: Result<flow::FlowIndexSummary, String>,
}

pub(crate) struct FlowWorker {
    jobs: Sender<Job>,
    results: Receiver<SaveIndexResult>,
    sent_seq: u64,
    done_seq: u64,
}

impl FlowWorker {
    pub(crate) fn start(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (result_tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("slate-flow-writer".into())
            .spawn(move || run(job_rx, result_tx, ctx))
            .expect("spawn Flow writer thread");
        Self {
            jobs,
            results,
            sent_seq: 0,
            done_seq: 0,
        }
    }

    pub(crate) fn heartbeat(&self, sqlite_path: PathBuf, scope: PathBuf, path: PathBuf) {
        let _ = self.jobs.send(Job::Heartbeat {
            sqlite_path,
            scope,
            path,
        });
    }

    pub(crate) fn save_index(
        &mut self,
        project_root: PathBuf,
        sqlite_path: PathBuf,
        path: PathBuf,
        contents: String,
    ) {
        self.sent_seq += 1;
        let _ = self.jobs.send(Job::SaveIndex {
            project_root,
            sqlite_path,
            path,
            contents,
            seq: self.sent_seq,
        });
    }

    /// True while the newest save's index refresh has not been reported back.
    pub(crate) fn save_pending(&self) -> bool {
        self.done_seq < self.sent_seq
    }

    pub(crate) fn take_results(&mut self) -> Vec<SaveIndexResult> {
        let results: Vec<_> = self.results.try_iter().collect();
        if let Some(max) = results.iter().map(|r| r.seq).max() {
            self.done_seq = self.done_seq.max(max);
        }
        results
    }
}

fn run(jobs: Receiver<Job>, results: Sender<SaveIndexResult>, ctx: egui::Context) {
    while let Ok(first) = jobs.recv() {
        let mut batch = vec![first];
        batch.extend(jobs.try_iter());
        for job in coalesce(batch) {
            match job {
                Job::Heartbeat {
                    sqlite_path,
                    scope,
                    path,
                } => {
                    let _ =
                        flow_runtime::publish_active_buffer_heartbeat(&sqlite_path, &scope, &path);
                }
                Job::SaveIndex {
                    project_root,
                    sqlite_path,
                    path,
                    contents,
                    seq,
                } => {
                    let started = std::time::Instant::now();
                    let result = flow::update_flow_index_for_file(
                        &project_root,
                        &sqlite_path,
                        &path,
                        &contents,
                    );
                    crate::perf_log::mark(
                        "flow.save_index_background",
                        &format!(
                            "ms={:.2} ok={}",
                            started.elapsed().as_secs_f64() * 1000.0,
                            result.is_ok()
                        ),
                    );
                    if results.send(SaveIndexResult { seq, path, result }).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            }
        }
    }
}

/// Keeps only the newest heartbeat, and only the newest save per file (its
/// higher sequence number also settles the saves it replaced).
fn coalesce(batch: Vec<Job>) -> Vec<Job> {
    let mut kept: Vec<Job> = Vec::with_capacity(batch.len());
    for job in batch {
        match &job {
            Job::Heartbeat { .. } => kept.retain(|k| !matches!(k, Job::Heartbeat { .. })),
            Job::SaveIndex { path, .. } => {
                if let Some(old) = kept.iter_mut().find(
                    |k| matches!(k, Job::SaveIndex { path: old_path, .. } if old_path == path),
                ) {
                    *old = job;
                    continue;
                }
            }
        }
        kept.push(job);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(path: &str, seq: u64) -> Job {
        Job::SaveIndex {
            project_root: PathBuf::new(),
            sqlite_path: PathBuf::new(),
            path: PathBuf::from(path),
            contents: String::new(),
            seq,
        }
    }

    fn heartbeat(path: &str) -> Job {
        Job::Heartbeat {
            sqlite_path: PathBuf::new(),
            scope: PathBuf::new(),
            path: PathBuf::from(path),
        }
    }

    fn describe(jobs: &[Job]) -> Vec<String> {
        jobs.iter()
            .map(|job| match job {
                Job::Heartbeat { path, .. } => format!("hb:{}", path.display()),
                Job::SaveIndex { path, seq, .. } => format!("save:{}:{seq}", path.display()),
            })
            .collect()
    }

    #[test]
    fn coalesce_keeps_newest_save_per_file_and_one_heartbeat() {
        let batch = vec![
            heartbeat("a"),
            save("a", 1),
            save("b", 2),
            heartbeat("b"),
            save("a", 3),
        ];
        assert_eq!(
            describe(&coalesce(batch)),
            ["save:a:3", "save:b:2", "hb:b"]
        );
    }

    #[test]
    fn worker_reports_newest_save_and_clears_pending() {
        let dir = std::env::temp_dir().join(format!("slate-flow-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("note.md");
        let sqlite = dir.join("agenda.db");
        let mut worker = FlowWorker::start(egui::Context::default());
        worker.save_index(dir.clone(), sqlite.clone(), note.clone(), "- [ ] one\n".into());
        worker.save_index(
            dir.clone(),
            sqlite,
            note.clone(),
            "- [ ] one\n- [ ] two\n".into(),
        );
        assert!(worker.save_pending());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut last = None;
        while worker.save_pending() && std::time::Instant::now() < deadline {
            if let Some(result) = worker.take_results().pop() {
                last = Some(result);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!worker.save_pending());
        let last = last.expect("save result");
        assert_eq!(last.path, note);
        assert_eq!(last.result.expect("indexed").tasks.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
