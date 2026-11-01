//! Coalesced TSV exports are derived work, never a reason to block the editor.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{OnceLock, mpsc},
    time::Duration,
};
enum Request {
    Export(PathBuf, PathBuf),
    Flush(mpsc::Sender<()>),
}
static EXPORTER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();
pub(crate) fn request(root: &std::path::Path, database: &std::path::Path) {
    let sender = EXPORTER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        let spawn = std::thread::Builder::new()
            .name("slate-index-export".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let mut pending = HashMap::new();
                    let mut barriers = Vec::new();
                    let mut add = |request| match request {
                        Request::Export(root, database) => {
                            pending.insert(root, database);
                        }
                        Request::Flush(done) => {
                            barriers.push(done);
                        }
                    };
                    add(request);
                    while let Ok(request) = receiver.try_recv() {
                        add(request);
                    }
                    for (root, database) in pending {
                        if let Err(err) =
                            crate::modules::flow::export_database_index(&root, &database)
                        {
                            crate::perf_log::mark(
                                "flow.tsv_export.failed",
                                &format!("root={root:?} error={err}"),
                            );
                        }
                    }
                    for done in barriers {
                        let _ = done.send(());
                    }
                }
            });
        if let Err(err) = spawn {
            crate::perf_log::mark("flow.tsv_export.worker_failed", &err.to_string());
        }
        sender
    });
    if sender
        .send(Request::Export(root.to_path_buf(), database.to_path_buf()))
        .is_err()
    {
        crate::perf_log::mark(
            "flow.tsv_export.enqueue_failed",
            "SQLite is committed; TSV is rebuildable",
        );
    }
}
pub(crate) fn flush() {
    let Some(sender) = EXPORTER.get() else {
        return;
    };
    let (done, wait) = mpsc::channel();
    if sender.send(Request::Flush(done)).is_ok() {
        let _ = wait.recv_timeout(Duration::from_secs(2));
    }
}
