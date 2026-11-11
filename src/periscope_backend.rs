//! Periscope's searches, on a worker thread that always answers the newest
//! query. Names come from Seek (see `seek.rs`): every drive, or below the
//! project folder, ranked by Seek (exact names, what the user used lately,
//! the folders they use, closeness to the project) and tolerant of typos.
use crate::seek;
use std::{
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Rows asked of Seek: more than are shown, so folders can be grouped.
const ASKED: usize = 500;
/// An open search is asked again this often (files come and go).
const REFRESH: Duration = Duration::from_secs(5);
/// Typing settles this long before a search goes out.
const SETTLE: Duration = Duration::from_millis(40);

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub query: String,
    /// Only below this folder.
    pub project: Option<PathBuf>,
    /// Ranks what is close to this folder first (searching every drive).
    pub near: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct ResultSet {
    pub id: u64,
    pub paths: Vec<PathBuf>,
    pub status: String,
}

/// What a search found, before it is published.
pub(crate) struct Found {
    pub paths: Vec<PathBuf>,
    pub status: String,
}

/// Runs one search; Seek in the app, a stand-in in tests.
pub(crate) type Engine = Box<dyn FnMut(&Request) -> Result<Found, String> + Send>;

struct Shared {
    pending: Mutex<Option<(u64, Request)>>,
    /// Wakes the idle worker: a request arrived, or it must stop.
    wake: Condvar,
    result: Mutex<Option<ResultSet>>,
    latest: AtomicU64,
    stop: AtomicBool,
}

pub(crate) struct Manager {
    shared: Arc<Shared>,
    engine: Option<Engine>,
}

impl Default for Manager {
    fn default() -> Self {
        Self::with_engine(seek_engine())
    }
}

impl Manager {
    pub(crate) fn with_engine(engine: Engine) -> Self {
        Self {
            shared: Arc::new(Shared {
                pending: Mutex::new(None),
                wake: Condvar::new(),
                result: Mutex::new(None),
                latest: AtomicU64::new(0),
                stop: AtomicBool::new(false),
            }),
            engine: Some(engine),
        }
    }

    pub(crate) fn request(&mut self, request: Request) -> u64 {
        let id = self.shared.latest.fetch_add(1, Ordering::Relaxed) + 1;
        *self.shared.pending.lock().unwrap() = Some((id, request));
        self.shared.wake.notify_one();
        if let Some(engine) = self.engine.take() {
            let shared = self.shared.clone();
            thread::spawn(move || worker(shared, engine));
        }
        id
    }

    pub(crate) fn poll(&self) -> Option<ResultSet> {
        self.shared.result.lock().unwrap().take()
    }

    pub(crate) fn cancel(&self) {
        self.shared.latest.fetch_add(1, Ordering::Relaxed);
        *self.shared.pending.lock().unwrap() = None;
        *self.shared.result.lock().unwrap() = None;
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        // Under the lock, so the worker cannot miss it between its check and its wait.
        let _pending = self.shared.pending.lock().unwrap();
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake.notify_one();
    }
}

fn publish(shared: &Shared, id: u64, paths: Vec<PathBuf>, status: String) {
    if shared.latest.load(Ordering::Relaxed) == id {
        *shared.result.lock().unwrap() = Some(ResultSet { id, paths, status });
    }
}

fn worker(shared: Arc<Shared>, mut engine: Engine) {
    let mut active: Option<(u64, Request)> = None;
    let mut received = Instant::now();
    let mut last_search = Instant::now();
    let mut needs_search = false;
    while !shared.stop.load(Ordering::Relaxed) {
        let pending = {
            let mut pending = shared.pending.lock().unwrap();
            // Nothing to search (Periscope closed or cancelled): sleep until
            // the next request instead of polling.
            if active.is_none() {
                pending = shared
                    .wake
                    .wait_while(pending, |p| p.is_none() && !shared.stop.load(Ordering::Relaxed))
                    .unwrap();
            }
            pending.take()
        };
        if let Some(request) = pending {
            received = Instant::now();
            needs_search = true;
            active = Some(request);
        }
        let Some((id, request)) = &active else {
            continue;
        };
        if shared.latest.load(Ordering::Relaxed) != *id {
            active = None;
            continue;
        }
        if received.elapsed() < SETTLE {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        if request.query.trim().is_empty() {
            if needs_search {
                needs_search = false;
                publish(
                    &shared,
                    *id,
                    Vec::new(),
                    "type a name · .md  !word  src/name  \"exact\"".into(),
                );
            }
        } else if needs_search || last_search.elapsed() > REFRESH {
            needs_search = false;
            last_search = Instant::now();
            let started = Instant::now();
            match engine(request) {
                Ok(found) => {
                    crate::perf_log::mark(
                        "periscope.query",
                        &format!(
                            "id={id} backend=seek results={} worker_ms={:.3}",
                            found.paths.len(),
                            started.elapsed().as_secs_f64() * 1000.0
                        ),
                    );
                    publish(&shared, *id, found.paths, found.status);
                }
                Err(error) => publish(&shared, *id, Vec::new(), error),
            }
        }
        thread::sleep(Duration::from_millis(15));
    }
}

/// Searches with Seek, keeping one connection for the worker's life.
fn seek_engine() -> Engine {
    let mut client = seek::Client::default();
    // Whether a drive is being indexed for the first time, asked now and then.
    let mut indexing: Option<(Instant, Option<String>)> = None;
    Box::new(move |request: &Request| {
        let rows = client
            .rows(&request.query, request.project.as_deref(), request.near.as_deref(), ASKED)
            .map_err(|error| error.to_string())?;
        if indexing
            .as_ref()
            .is_none_or(|(asked, busy)| asked.elapsed() > Duration::from_secs(if busy.is_some() { 2 } else { 60 }))
        {
            let busy = client.status().ok().and_then(|status| status.indexing());
            indexing = Some((Instant::now(), busy));
        }
        let busy = indexing.as_ref().and_then(|(_, busy)| busy.clone());
        Ok(found_from_rows(rows, busy))
    })
}

/// The files of Seek's rows, best first, and what to say about them.
fn found_from_rows(rows: seek::Rows, indexing: Option<String>) -> Found {
    let paths: Vec<PathBuf> = rows
        .entries
        .into_iter()
        .filter(|(folder, _)| !folder)
        .map(|(_, path)| path)
        .take(crate::PERISCOPE_RESULT_LIMIT)
        .collect();
    let count = if rows.exact {
        seek::thousands(rows.total)
    } else {
        format!("{}+", seek::thousands(rows.total))
    };
    let mut status = match rows.matched {
        _ if rows.total == 0 => "Seek · nothing matches".to_string(),
        seek::Match::Near => "Seek · no exact match · close names".to_string(),
        seek::Match::Scattered => format!("Seek · {count} loose matches"),
        seek::Match::Real if rows.total as usize > paths.len() => {
            format!("Seek · {count} matches · best {}", paths.len())
        }
        seek::Match::Real => format!("Seek · {count} matches"),
    };
    if let Some(indexing) = indexing {
        status.push_str(&format!(" · Seek is {indexing}: results may miss it"));
    }
    Found { paths, status }
}

/// Periscope used to keep a copy of every path on every drive in
/// `%LOCALAPPDATA%\slate\periscope` (gigabytes). Seek replaced it: the
/// copy goes, once, off the UI thread.
pub(crate) fn remove_old_path_cache() {
    // LOCALAPPDATA first, like the logs: a run with its own (a smoke test)
    // never touches the user's folder.
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs_next::data_local_dir);
    let Some(folder) = local.map(|dir| dir.join("slate").join("periscope")) else {
        return;
    };
    if folder.join("paths.sqlite3").exists() {
        thread::spawn(move || {
            let _ = std::fs::remove_dir_all(&folder);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(manager: &Manager) -> ResultSet {
        let started = Instant::now();
        loop {
            if let Some(result) = manager.poll() {
                return result;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "no result");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn request(query: &str) -> Request {
        Request {
            query: query.into(),
            project: None,
            near: None,
        }
    }

    #[test]
    fn only_the_newest_query_is_searched_and_published() {
        let searched = Arc::new(Mutex::new(Vec::new()));
        let log = searched.clone();
        let mut manager = Manager::with_engine(Box::new(move |request: &Request| {
            log.lock().unwrap().push(request.query.clone());
            Ok(Found {
                paths: vec![PathBuf::from(format!("C:\\{}.md", request.query))],
                status: "ok".into(),
            })
        }));
        manager.request(request("i"));
        manager.request(request("in"));
        let id = manager.request(request("inbox"));
        let result = wait_for(&manager);
        assert_eq!(result.id, id);
        assert_eq!(result.paths, vec![PathBuf::from("C:\\inbox.md")]);
        assert_eq!(*searched.lock().unwrap(), vec!["inbox".to_string()], "typing settles first");
        // Cancelled: a late answer is dropped.
        manager.cancel();
        thread::sleep(Duration::from_millis(80));
        assert!(manager.poll().is_none());
    }

    #[test]
    fn an_empty_query_never_reaches_the_engine_and_errors_are_shown() {
        let mut manager = Manager::with_engine(Box::new(|request: &Request| {
            assert!(!request.query.trim().is_empty());
            Err("Seek isn't installed: get it from github.com/SynrgStudio/seek".into())
        }));
        manager.request(request("  "));
        assert!(wait_for(&manager).paths.is_empty());
        manager.request(request("nota"));
        let result = wait_for(&manager);
        assert!(result.status.starts_with("Seek isn't installed"), "{}", result.status);
    }

    /// Periscope's real path, on this machine's Seek: how long the first
    /// answer takes. `cargo test live_periscope -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_periscope_answers_through_seek() {
        let mut manager = Manager::default();
        for query in ["Cargo.toml", "cargo tmol", "readme .md"] {
            let started = Instant::now();
            manager.request(request(query));
            let result = wait_for(&manager);
            println!(
                "{query:?}: {} files in {:.0} ms (40 ms of it is the typing pause) · {}",
                result.paths.len(),
                started.elapsed().as_secs_f64() * 1000.0,
                result.status
            );
            assert!(!result.paths.is_empty(), "{}", result.status);
        }
    }

    #[test]
    fn rows_become_files_and_say_how_they_matched() {
        let rows = |matched, total, entries: Vec<(bool, &str)>| seek::Rows {
            total,
            exact: true,
            matched,
            entries: entries
                .into_iter()
                .map(|(folder, path)| (folder, PathBuf::from(path)))
                .collect(),
        };
        let found = found_from_rows(
            rows(
                seek::Match::Real,
                2,
                vec![(true, "C:\\notes"), (false, "C:\\notes\\inbox.md"), (false, "D:\\inbox.md")],
            ),
            None,
        );
        assert_eq!(
            found.paths,
            vec![PathBuf::from("C:\\notes\\inbox.md"), PathBuf::from("D:\\inbox.md")],
            "folders are left out, Seek's order kept"
        );
        assert_eq!(found.status, "Seek · 2 matches");
        let near = found_from_rows(rows(seek::Match::Near, 1, vec![(false, "C:\\inbox.md")]), None);
        assert_eq!(near.status, "Seek · no exact match · close names");
        let many: Vec<(bool, &str)> = (0..300).map(|_| (false, "C:\\a.md")).collect();
        let busy = found_from_rows(
            rows(seek::Match::Real, 12_000, many),
            Some("indexing D: (812,345 names)".into()),
        );
        assert_eq!(busy.paths.len(), crate::PERISCOPE_RESULT_LIMIT);
        assert_eq!(
            busy.status,
            "Seek · 12,000 matches · best 100 · Seek is indexing D: (812,345 names): results may miss it"
        );
    }
}
