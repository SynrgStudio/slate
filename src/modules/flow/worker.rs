//! One background thread for Flow writes that the editor does not wait on.
//!
//! The active-buffer heartbeat (every 3 s while a Flow file is open), the index
//! refresh after a save, and the agenda sync after a typing pause near a task
//! all touch shared files/SQLite that other Slate processes also write. On the
//! UI thread they were usually < 1 ms but spiked to 10 ms (p99) and 184 ms (max)
//! when they hit a lock, freezing typing. Jobs run in order on a single thread,
//! so older contents never land after newer ones; when jobs queue up behind a
//! slow one, only the newest heartbeat and the newest job of each kind per file
//! run (moved to the end so ordering between files' contents stays newest-last).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;

use crate::modules::agenda::{self, AgendaItem};
use crate::modules::flow::{self as flow, runtime as flow_runtime};

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
    },
    AgendaSync {
        project_root: PathBuf,
        sqlite_path: PathBuf,
        path: PathBuf,
        contents: String,
        load_items: bool,
    },
}

impl Job {
    /// Jobs with the same key supersede each other.
    fn key(&self) -> (u8, &PathBuf) {
        match self {
            Job::Heartbeat { path, .. } => (0, path),
            Job::SaveIndex { path, .. } => (1, path),
            Job::AgendaSync { path, .. } => (2, path),
        }
    }
}

pub(crate) enum Report {
    SaveIndex {
        path: PathBuf,
        result: Result<flow::FlowIndexSummary, String>,
    },
    AgendaSync {
        path: PathBuf,
        result: Result<AgendaSynced, String>,
    },
}

pub(crate) struct AgendaSynced {
    /// Fresh cache items when the agenda panel was open at request time.
    pub(crate) items: Option<Vec<AgendaItem>>,
}

pub(crate) struct FlowWorker {
    jobs: Sender<Job>,
    reports: Receiver<Report>,
}

impl FlowWorker {
    pub(crate) fn start(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (report_tx, reports) = mpsc::channel();
        std::thread::Builder::new()
            .name("slate-flow-writer".into())
            .spawn(move || run(job_rx, report_tx, ctx))
            .expect("spawn Flow writer thread");
        Self { jobs, reports }
    }

    pub(crate) fn heartbeat(&self, sqlite_path: PathBuf, scope: PathBuf, path: PathBuf) {
        let _ = self.jobs.send(Job::Heartbeat {
            sqlite_path,
            scope,
            path,
        });
    }

    pub(crate) fn save_index(
        &self,
        project_root: PathBuf,
        sqlite_path: PathBuf,
        path: PathBuf,
        contents: String,
    ) {
        let _ = self.jobs.send(Job::SaveIndex {
            project_root,
            sqlite_path,
            path,
            contents,
        });
    }

    pub(crate) fn agenda_sync(
        &self,
        project_root: PathBuf,
        sqlite_path: PathBuf,
        path: PathBuf,
        contents: String,
        load_items: bool,
    ) {
        let _ = self.jobs.send(Job::AgendaSync {
            project_root,
            sqlite_path,
            path,
            contents,
            load_items,
        });
    }

    pub(crate) fn take_reports(&self) -> Vec<Report> {
        self.reports.try_iter().collect()
    }
}

fn run(jobs: Receiver<Job>, reports: Sender<Report>, ctx: egui::Context) {
    while let Ok(first) = jobs.recv() {
        let mut batch = vec![first];
        batch.extend(jobs.try_iter());
        for job in coalesce(batch) {
            let started = std::time::Instant::now();
            let report = match job {
                Job::Heartbeat {
                    sqlite_path,
                    scope,
                    path,
                } => {
                    let _ =
                        flow_runtime::publish_active_buffer_heartbeat(&sqlite_path, &scope, &path);
                    continue;
                }
                Job::SaveIndex {
                    project_root,
                    sqlite_path,
                    path,
                    contents,
                } => {
                    let result = flow::update_flow_index_for_file(
                        &project_root,
                        &sqlite_path,
                        &path,
                        &contents,
                    );
                    Report::SaveIndex { path, result }
                }
                Job::AgendaSync {
                    project_root,
                    sqlite_path,
                    path,
                    contents,
                    load_items,
                } => {
                    let result =
                        agenda_sync(&project_root, &sqlite_path, &path, &contents, load_items);
                    Report::AgendaSync { path, result }
                }
            };
            crate::perf_log::mark(
                "flow.writer_job",
                &format!(
                    "kind={} ms={:.2}",
                    match report {
                        Report::SaveIndex { .. } => "save_index",
                        Report::AgendaSync { .. } => "agenda_sync",
                    },
                    started.elapsed().as_secs_f64() * 1000.0
                ),
            );
            if reports.send(report).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

fn agenda_sync(
    project_root: &std::path::Path,
    sqlite_path: &std::path::Path,
    path: &std::path::Path,
    contents: &str,
    load_items: bool,
) -> Result<AgendaSynced, String> {
    // Logging captures/completions to the daily log was done here through
    // the editor; that link is gone until Daily comes back as a module.
    flow::update_flow_index_for_file(project_root, sqlite_path, path, contents)?;
    let items = if load_items {
        agenda::load_items_from_cache(sqlite_path, project_root).ok()
    } else {
        None
    };
    Ok(AgendaSynced { items })
}

/// Keeps only the newest job per kind and file, moved to the newest position.
fn coalesce(batch: Vec<Job>) -> Vec<Job> {
    let mut kept: Vec<Job> = Vec::with_capacity(batch.len());
    for job in batch {
        let heartbeat = matches!(job, Job::Heartbeat { .. });
        kept.retain(|old| {
            if heartbeat {
                !matches!(old, Job::Heartbeat { .. })
            } else {
                old.key() != job.key()
            }
        });
        kept.push(job);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(path: &str, contents: &str) -> Job {
        Job::SaveIndex {
            project_root: PathBuf::new(),
            sqlite_path: PathBuf::new(),
            path: PathBuf::from(path),
            contents: contents.into(),
        }
    }

    fn sync(path: &str, contents: &str) -> Job {
        Job::AgendaSync {
            project_root: PathBuf::new(),
            sqlite_path: PathBuf::new(),
            path: PathBuf::from(path),
            contents: contents.into(),
            load_items: false,
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
                Job::SaveIndex { path, contents, .. } => {
                    format!("save:{}:{contents}", path.display())
                }
                Job::AgendaSync { path, contents, .. } => {
                    format!("sync:{}:{contents}", path.display())
                }
            })
            .collect()
    }

    #[test]
    fn coalesce_keeps_newest_per_kind_and_file_in_newest_position() {
        let batch = vec![
            heartbeat("a"),
            save("a", "1"),
            sync("a", "2"),
            save("b", "x"),
            heartbeat("b"),
            save("a", "3"),
        ];
        // The newest contents of `a` (save 3) run after the older sync 2.
        assert_eq!(
            describe(&coalesce(batch)),
            ["sync:a:2", "save:b:x", "hb:b", "save:a:3"]
        );
    }

    #[test]
    fn worker_indexes_and_syncs_in_order() {
        let dir = std::env::temp_dir().join(format!("slate-flow-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("note.md");
        // The agenda only lists tasks of files that still exist.
        std::fs::write(&note, "- [ ] one\n- [ ] two\n").unwrap();
        let sqlite = dir.join("agenda.db");
        let worker = FlowWorker::start(egui::Context::default());
        worker.save_index(
            dir.clone(),
            sqlite.clone(),
            note.clone(),
            "- [ ] one\n".into(),
        );
        worker.agenda_sync(
            dir.clone(),
            sqlite,
            note.clone(),
            "- [ ] one\n- [ ] two\n".into(),
            true,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut reports = Vec::new();
        while reports.len() < 2 && std::time::Instant::now() < deadline {
            reports.extend(worker.take_reports());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let [
            Report::SaveIndex { result: saved, .. },
            Report::AgendaSync { path, result },
        ] = &reports[..]
        else {
            panic!("expected a save report then a sync report");
        };
        assert_eq!(saved.as_ref().expect("indexed").tasks.len(), 1);
        assert_eq!(path, &note);
        let synced = result.as_ref().expect("synced");
        let titles: Vec<_> = synced
            .items
            .as_ref()
            .expect("items loaded")
            .iter()
            .map(|item| item.title.clone())
            .collect();
        assert_eq!(titles, ["one", "two"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
