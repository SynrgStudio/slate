//! Streaming background indexing: one schema setup/connection, at most 32 files per transaction.
use crate::modules::flow::{
    self as flow, runtime as flow_runtime,
    store::{CacheWriter, Document},
};
use crate::perf_log;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) const BATCH_FILES: usize = 32;
#[derive(Default, Debug)]
pub(crate) struct ScanSummary {
    pub indexed_files: usize,
    pub indexed_tasks: usize,
    pub skipped_live: usize,
    pub batches: usize,
    pub errors: usize,
    pub first_error: Option<String>,
}
impl ScanSummary {
    fn error(&mut self, detail: String) {
        self.errors += 1;
        if self.errors <= 5 {
            perf_log::mark("flow.background_rescan.error", &detail);
        }
        self.first_error.get_or_insert(detail);
    }
}
pub(crate) fn live_paths(sqlite_path: &Path) -> HashSet<String> {
    let Ok(path) = flow_runtime::active_buffers_path(sqlite_path) else {
        return HashSet::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return HashSet::new();
    };
    let now = flow_runtime::unix_now();
    raw.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            fields.next()?;
            let seen = fields.next()?.parse::<u64>().ok()?;
            let path = fields.next()?;
            (now.saturating_sub(seen) <= 10).then(|| path.to_string())
        })
        .collect()
}

pub(crate) fn index_files(
    root: &Path,
    sqlite_path: &Path,
    files: &[PathBuf],
    protected: &[PathBuf],
) -> Result<ScanSummary, String> {
    index_files_with_rebuild(root, sqlite_path, files, protected, false)
}

pub(crate) fn index_files_with_rebuild(
    root: &Path,
    sqlite_path: &Path,
    files: &[PathBuf],
    protected: &[PathBuf],
    rebuild: bool,
) -> Result<ScanSummary, String> {
    let mut timing = perf_log::Span::new(
        "flow.embedded_index",
        &format!("files={} batch_limit={BATCH_FILES}", files.len()),
    );
    let mut writer = CacheWriter::open(sqlite_path, Duration::from_millis(250))?;
    timing.step("open_and_migrate_once");
    let protected: HashSet<_> = protected
        .iter()
        .map(|p| flow_runtime::relative_path(root, p))
        .collect();
    if rebuild {
        let mut live = live_paths(sqlite_path);
        live.extend(protected.iter().cloned());
        writer.clear_except(&live)?;
        timing.step("clear_derived_rows_keep_live_buffers");
    }
    let mut summary = ScanSummary::default();
    for paths in files.chunks(BATCH_FILES) {
        let mut live = live_paths(sqlite_path);
        live.extend(protected.iter().cloned());
        let mut documents = Vec::with_capacity(paths.len());
        for path in paths {
            if live.contains(&flow_runtime::relative_path(root, path)) {
                summary.skipped_live += 1;
                continue;
            }
            let contents = match fs::read_to_string(path) {
                Ok(text) => text,
                Err(err) => {
                    summary.error(format!("read failed for {path:?}: {err}"));
                    continue;
                }
            };
            let tasks = flow::parse_flow_tasks(&contents);
            let known_file = flow::build_known_file(root, path, &contents, &tasks);
            documents.push(Document {
                known_file,
                tasks,
                contents,
            });
        }
        // A buffer may have become active while reading/parsing this batch.
        let latest_live = live_paths(sqlite_path);
        documents.retain(|document| {
            let keep = !latest_live.contains(&document.known_file.path);
            if !keep {
                summary.skipped_live += 1;
            }
            keep
        });
        if documents.is_empty() {
            continue;
        }
        if let Err(err) = writer.write_batch(&documents) {
            summary.error(format!("SQLite batch rolled back: {err}"));
            break; // Do not retry the same backend failure for every remaining file.
        }
        summary.indexed_files += documents.len();
        summary.indexed_tasks += documents.iter().map(|d| d.tasks.len()).sum::<usize>();
        summary.batches += 1;
        // The writer lock has been released between chunks; readers and UI updates can run.
        std::thread::yield_now();
    }
    timing.step("read_parse_commit_batches");
    if let Err(err) = flow::export_cache_index(root, &writer) {
        summary.error(format!("TSV export failed: {err}"));
    }
    timing.step("export_index_once");
    perf_log::mark(
        "flow.embedded_index.result",
        &format!(
            "indexed_files={} indexed_tasks={} batches={} skipped_live={} errors={}",
            summary.indexed_files,
            summary.indexed_tasks,
            summary.batches,
            summary.skipped_live,
            summary.errors
        ),
    );
    Ok(summary)
}
