//! Coalesced queries and independent, metadata-only fallback scanner.
use crate::{
    everything_ipc,
    periscope_store::{Entry, Store},
};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub query: String,
    pub project: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
    pub everything: bool,
}
#[derive(Debug)]
pub(crate) struct ResultSet {
    pub id: u64,
    pub paths: Vec<PathBuf>,
    pub status: String,
}
struct Shared {
    provider: fn(&str, usize, &dyn Fn() -> bool) -> Result<everything_ipc::Page, String>,
    normal_provider:
        Option<fn(&str, usize, bool, &dyn Fn() -> bool) -> Result<everything_ipc::Page, String>>,
    pending: Mutex<Option<(u64, Request)>>,
    /// Wakes the idle worker: a request arrived, or it must stop.
    wake: Condvar,
    result: Mutex<Option<ResultSet>>,
    latest: AtomicU64,
    stop: AtomicBool,
}
pub(crate) struct Manager {
    shared: Arc<Shared>,
    started: bool,
    cache: PathBuf,
}
impl Default for Manager {
    fn default() -> Self {
        Self::new(
            dirs_next::data_local_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("slate/periscope/paths.sqlite3"),
        )
    }
}
impl Manager {
    pub fn new(cache: PathBuf) -> Self {
        Self {
            shared: Arc::new(Shared {
                provider: everything_ipc::query_cancellable,
                normal_provider: Some(everything_ipc::normal_query),
                pending: Mutex::new(None),
                wake: Condvar::new(),
                result: Mutex::new(None),
                latest: AtomicU64::new(0),
                stop: AtomicBool::new(false),
            }),
            started: false,
            cache,
        }
    }
    pub fn request(&mut self, request: Request) -> u64 {
        let id = self.shared.latest.fetch_add(1, Ordering::Relaxed) + 1;
        *self.shared.pending.lock().unwrap() = Some((id, request));
        self.shared.wake.notify_one();
        if !self.started {
            self.started = true;
            let shared = self.shared.clone();
            let cache = self.cache.clone();
            thread::spawn(move || worker(shared, cache));
        }
        id
    }
    pub fn poll(&self) -> Option<ResultSet> {
        self.shared.result.lock().unwrap().take()
    }
    pub fn cancel(&self) {
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
fn worker(shared: Arc<Shared>, cache: PathBuf) {
    let scan = Arc::new(Scan {
        roots: Mutex::new(Vec::new()),
        status: Arc::new(Mutex::new("fallback cache · scan queued".into())),
        one_shot: false,
        fallback: Mutex::new(None),
        completion: Mutex::new(None),
        revision: AtomicU64::new(0),
        stop: Arc::new(AtomicBool::new(false)),
    });
    let mut scanner_started = false;
    let mut active: Option<(u64, Request)> = None;
    let mut last_refresh = Instant::now();
    let mut seen_revision = 0;
    let mut defaults: Option<Vec<PathBuf>> = None;
    let mut fallback_snapshot: Option<(u64, u64)> = None;
    let mut received = Instant::now();
    let mut needs_query = false;
    while !shared.stop.load(Ordering::Relaxed) {
        let pending = {
            let mut pending = shared.pending.lock().unwrap();
            // Nothing to search (Periscope closed or cancelled): sleep until
            // the next request instead of polling.
            if active.is_none() {
                pending = shared
                    .wake
                    .wait_while(pending, |p| {
                        p.is_none() && !shared.stop.load(Ordering::Relaxed)
                    })
                    .unwrap();
            }
            pending.take()
        };

        if let Some(request) = pending {
            received = Instant::now();
            needs_query = true;
            active = Some(request);
        }
        let scan_revision = scan.revision.load(Ordering::Relaxed);
        if let Some((id, request)) = &active {
            if received.elapsed() < Duration::from_millis(40) {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            if shared.latest.load(Ordering::Relaxed) != *id {
                active = None;
                continue;
            }
            let changed = needs_query;
            if request.query.trim().is_empty() {
                needs_query = false;
                if changed {
                    publish(
                        &shared,
                        *id,
                        Vec::new(),
                        "Ready · type a filename or path".into(),
                    );
                }
                thread::sleep(Duration::from_millis(15));
                continue;
            }
            if changed
                || (scanner_started
                    && scan_revision != seen_revision
                    && last_refresh.elapsed() > Duration::from_millis(250))
                || last_refresh.elapsed() > Duration::from_secs(5)
            {
                needs_query = false;
                last_refresh = Instant::now();
                let started = Instant::now();
                seen_revision = scan_revision;
                let roots = if let Some(project) = &request.project {
                    vec![project.clone()]
                } else if request.roots.is_empty() {
                    defaults.get_or_insert_with(default_roots).clone()
                } else {
                    request.roots.clone()
                };
                let mut reason = "Everything disabled".to_string();
                if request.everything {
                    match search_everything(&roots, &request.query, &shared, *id) {
                        Ok((paths, total, evaluated, route)) => {
                            crate::perf_log::mark(
                                "periscope.query",
                                &format!(
                                    "id={id} backend=everything results={} candidates={evaluated} total={total} worker_ms={:.3}",
                                    paths.len(),
                                    started.elapsed().as_secs_f64() * 1000.0
                                ),
                            );
                            publish(
                                &shared,
                                *id,
                                paths,
                                format!(
                                    "Everything {route} · {evaluated}/{total} candidates · top 100{}",
                                    if evaluated < total {
                                        " · partial ranking: refine query"
                                    } else {
                                        ""
                                    }
                                ),
                            );
                            if scanner_started {
                                scan.roots.lock().unwrap().clear();
                            }
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(error) => reason = error,
                    }
                }
                if shared.latest.load(Ordering::Relaxed) != *id
                    || shared.stop.load(Ordering::Relaxed)
                {
                    continue;
                }
                if !changed && fallback_snapshot == Some((*id, scan_revision)) {
                    continue;
                }
                {
                    let mut desired = scan.roots.lock().unwrap();
                    *desired = roots.clone();
                }
                if !scanner_started {
                    scanner_started = true;
                    let scan = scan.clone();
                    let cache = cache.clone();
                    thread::spawn(move || scanner(scan, cache));
                }
                match Store::open(&cache)
                    .and_then(|store| store.search(&roots, &request.query, &shared.latest, *id))
                {
                    Ok((paths, examined)) => {
                        fallback_snapshot = Some((*id, scan_revision));
                        crate::perf_log::mark(
                            "periscope.query",
                            &format!(
                                "id={id} backend=sqlite results={} cached_paths={examined} worker_ms={:.3}",
                                paths.len(),
                                started.elapsed().as_secs_f64() * 1000.0
                            ),
                        );
                        publish(
                            &shared,
                            *id,
                            paths,
                            format!(
                                "SQLite · {examined} cached paths · {} · {reason}",
                                scan.status.lock().unwrap()
                            ),
                        );
                    }
                    Err(error) => publish(
                        &shared,
                        *id,
                        Vec::new(),
                        format!("Periscope fallback unavailable: {error} · {reason}"),
                    ),
                }
            }
        }
        thread::sleep(Duration::from_millis(15));
    }
    scan.stop.store(true, Ordering::Relaxed);
}
fn search_everything(
    roots: &[PathBuf],
    query: &str,
    shared: &Shared,
    id: u64,
) -> Result<(Vec<PathBuf>, usize, usize, &'static str), String> {
    if roots.is_empty() {
        return Err("No search roots available; configure periscope_root".into());
    }
    let cancel =
        || shared.latest.load(Ordering::Relaxed) != id || shared.stop.load(Ordering::Relaxed);
    if let (Some(provider), Some((search, match_path))) = (
        shared.normal_provider,
        everything_ipc::native_search(query, roots),
    ) {
        let page = provider(&search, 0, match_path, &cancel)?;
        let mut paths = page.paths;
        retain_scoped_paths(&mut paths, roots);
        if !paths.is_empty() {
            return Ok((
                crate::periscope::rank_native_paths(paths, query, 100),
                page.total,
                page.returned,
                "native",
            ));
        }
        if cancel() {
            return Err("cancelled".into());
        }
    }
    if !query.contains(['/', '\\']) {
        let named = search_everything_expression(
            roots,
            query,
            shared,
            id,
            &everything_ipc::filename_expression(query, roots),
        )?;
        if !named.0.is_empty() {
            return Ok((named.0, named.1, named.2, "fuzzy name"));
        }
    }
    search_everything_expression(
        roots,
        query,
        shared,
        id,
        &everything_ipc::expression(query, roots),
    )
    .map(|value| (value.0, value.1, value.2, "fuzzy path"))
}
fn retain_scoped_paths(paths: &mut Vec<PathBuf>, roots: &[PathBuf]) {
    paths.retain(|path| {
        let value = crate::periscope_store::normalize(path);
        roots.iter().any(|root| {
            let root = crate::periscope_store::normalize(root);
            value == root || value.starts_with(&format!("{root}/"))
        })
    });
    paths.sort();
    paths.dedup();
}
fn search_everything_expression(
    roots: &[PathBuf],
    query: &str,
    shared: &Shared,
    id: u64,
    expression: &str,
) -> Result<(Vec<PathBuf>, usize, usize), String> {
    let mut paths = Vec::new();
    let mut offset = 0;
    let mut total = 0;
    while offset < 4096 {
        if shared.latest.load(Ordering::Relaxed) != id || shared.stop.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let page = (shared.provider)(&expression, offset, &|| {
            shared.latest.load(Ordering::Relaxed) != id || shared.stop.load(Ordering::Relaxed)
        })?;
        total = page.total;
        let returned = page.returned;
        paths.extend(page.paths);
        offset += returned;
        if returned == 0 || offset >= total || query.trim().is_empty() {
            break;
        }
    }
    paths.retain(|path| {
        let normalized = crate::periscope_store::normalize(path);
        roots.iter().any(|root| {
            let root = crate::periscope_store::normalize(root);
            normalized == root || normalized.starts_with(&format!("{root}/"))
        })
    });
    paths.sort();
    paths.dedup();
    let root = roots
        .first()
        .map(PathBuf::as_path)
        .unwrap_or_else(|| Path::new(""));
    Ok((
        crate::periscope::rank_paths(paths, root, query, crate::PERISCOPE_RESULT_LIMIT),
        total,
        offset,
    ))
}
struct Scan {
    roots: Mutex<Vec<PathBuf>>,
    status: Arc<Mutex<String>>,
    revision: AtomicU64,
    stop: Arc<AtomicBool>,
    one_shot: bool,
    fallback: Mutex<Option<String>>,
    completion: Mutex<Option<Result<Prepared, String>>>,
}
#[derive(Clone, Debug)]
pub(crate) struct Prepared {
    pub files: usize,
    pub errors: usize,
}
pub(crate) struct Preparation {
    scan: Arc<Scan>,
}
impl Preparation {
    pub fn status(&self) -> String {
        let status = self.scan.status.lock().unwrap().clone();
        match self.scan.fallback.lock().unwrap().as_ref() {
            Some(reason) => format!("Filesystem fallback ({reason}) · {status}"),
            None => status,
        }
    }
    pub fn result(&self) -> Option<Result<Prepared, String>> {
        self.scan.completion.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.scan.stop.store(true, Ordering::Relaxed);
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Manager {
    pub fn cache_path(&self) -> PathBuf {
        self.cache.clone()
    }
    #[cfg(test)]
    pub fn prepare(&self, roots: Vec<PathBuf>) -> Preparation {
        self.prepare_using(roots, false)
    }
    pub fn prepare_using(&self, roots: Vec<PathBuf>, everything: bool) -> Preparation {
        let scan = Arc::new(Scan {
            roots: Mutex::new(roots),
            status: Arc::new(Mutex::new("Preparing metadata index…".into())),
            revision: AtomicU64::new(0),
            stop: Arc::new(AtomicBool::new(false)),
            one_shot: true,
            fallback: Mutex::new(if everything {
                None
            } else {
                Some("Everything disabled".into())
            }),
            completion: Mutex::new(None),
        });
        let worker = scan.clone();
        let cache = self.cache.clone();
        thread::spawn(move || {
            if everything {
                let roots = worker.roots.lock().unwrap().clone();
                match import_inventory(
                    &worker,
                    &cache,
                    &roots,
                    &mut crate::everything_ipc::inventory,
                ) {
                    Ok(result) => {
                        *worker.completion.lock().unwrap() = Some(Ok(result));
                        return;
                    }
                    Err(ImportError::Failed(error)) => {
                        if !worker.stop.load(Ordering::Relaxed) {
                            *worker.completion.lock().unwrap() = Some(Err(error));
                        }
                        return;
                    }
                    Err(ImportError::Unavailable {
                        roots: missing,
                        files,
                        reason,
                    }) => {
                        *worker.fallback.lock().unwrap() =
                            Some(format!("{files} files imported from Everything; {reason}"));
                        let fallback = Arc::new(Scan {
                            roots: Mutex::new(missing),
                            status: worker.status.clone(),
                            revision: AtomicU64::new(0),
                            stop: worker.stop.clone(),
                            one_shot: true,
                            fallback: Mutex::new(None),
                            completion: Mutex::new(None),
                        });
                        scanner(fallback.clone(), cache.clone());
                        if worker.stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let result = fallback.completion.lock().unwrap().clone();
                        if let Some(result) = result {
                            *worker.completion.lock().unwrap() =
                                Some(result.and_then(|mut result| {
                                    result.files += files;
                                    Store::open(&cache).and_then(|mut store| {
                                        store.mark_prepared(&roots, result.errors)
                                    })?;
                                    Ok(result)
                                }));
                        }
                        return;
                    }
                }
            }
            scanner(worker, cache);
        });
        Preparation { scan }
    }
}
#[derive(Debug)]
enum ImportError {
    Unavailable {
        roots: Vec<PathBuf>,
        files: usize,
        reason: String,
    },
    Failed(String),
}
fn import_inventory(
    scan: &Scan,
    cache: &Path,
    roots: &[PathBuf],
    provider: &mut impl FnMut(&str, usize) -> Result<crate::everything_inventory::Page, String>,
) -> Result<Prepared, ImportError> {
    let started = Instant::now();
    let mut store: Option<Store> = None;
    let mut files = 0;
    let mut missing = Vec::new();
    let mut reasons = Vec::new();
    if roots.is_empty() {
        return Err(ImportError::Failed("No inventory roots available".into()));
    }
    for (root_index, root) in roots.iter().enumerate() {
        let scope = crate::periscope_store::normalize(root);
        let expression = everything_ipc::inventory_expression(
            root,
            cache
                .parent()
                .ok_or_else(|| ImportError::Failed("Invalid cache path".into()))?,
        );
        let mut offset = 0;
        let mut expected = None;
        let mut root_files = 0;
        let mut generation = 0;
        loop {
            if scan.stop.load(Ordering::Relaxed) {
                return Err(ImportError::Failed("Inventory import cancelled".into()));
            }
            *scan.status.lock().unwrap() = format!(
                "Everything → SQLite · {} · {offset}/{} initial entries · {files} files cached · {root_index}/{} roots complete",
                root.display(),
                expected
                    .map(|v: usize| v.to_string())
                    .unwrap_or_else(|| "?".into()),
                roots.len()
            );
            let page = match provider(&expression, offset) {
                Ok(page) => page,
                Err(error) if offset == 0 => {
                    missing.push(root.clone());
                    reasons.push(format!("{}: {error}", root.display()));
                    break;
                }
                Err(error) => {
                    return Err(ImportError::Failed(format!(
                        "Inventory interrupted; partial cache retained: {error}"
                    )));
                }
            };
            if offset == 0 && page.total == 0 && page.returned == 0 {
                missing.push(root.clone());
                reasons.push(format!("{}: no indexed inventory", root.display()));
                break;
            }
            // Everything is a live index, not a pinned snapshot. Shrinkage may
            // put the requested offset past EOF; growth must not extend this pass forever.
            let empty_eof = offset > 0
                && page.returned == 0
                && page.entries.is_empty()
                && page.total <= offset
                && page.offset <= offset;
            if !empty_eof
                && (page.offset != offset
                    || page.entries.len() > page.returned
                    || page.returned > 8192
                    || page.returned == 0
                    || offset
                        .checked_add(page.returned)
                        .is_none_or(|end| end > page.total))
            {
                return Err(ImportError::Failed(
                    "Incomplete/invalid inventory page; no ready marker written".into(),
                ));
            }
            expected.get_or_insert(page.total);
            for entry in &page.entries {
                let value = crate::periscope_store::normalize(&entry.path);
                if !value.starts_with(&format!("{scope}/"))
                    || value.split('/').any(|part| part == ".." || part == ".")
                {
                    return Err(ImportError::Failed(
                        "Inventory returned a path outside the consented root".into(),
                    ));
                }
            }
            if store.is_none() {
                store = Some(Store::open(cache).map_err(ImportError::Failed)?);
            }
            let store = store.as_mut().unwrap();
            if offset == 0 {
                generation = store.next_generation().map_err(ImportError::Failed)?;
            }
            for batch in page.entries.chunks(2048) {
                if scan.stop.load(Ordering::Relaxed) {
                    return Err(ImportError::Failed("Inventory import cancelled".into()));
                }
                store
                    .write(root, generation, batch)
                    .map_err(ImportError::Failed)?;
            }
            files += page.entries.len();
            root_files += page.entries.len();
            offset += page.returned;
            if empty_eof || offset >= expected.unwrap().min(page.total) {
                // Inserts are idempotent. Pagination shifts may repeat paths;
                // count unique committed rows instead of treating normal churn as failure.
                files -= root_files;
                files += store
                    .inventory_count(root, generation)
                    .map_err(ImportError::Failed)?;
                store
                    .inventory_finished(root)
                    .map_err(ImportError::Failed)?;
                break;
            }
        }
    }
    if scan.stop.load(Ordering::Relaxed) {
        return Err(ImportError::Failed("Inventory import cancelled".into()));
    }
    if !missing.is_empty() {
        return Err(ImportError::Unavailable {
            roots: missing,
            files,
            reason: reasons.join("; "),
        });
    }
    store
        .as_mut()
        .unwrap()
        .mark_prepared(roots, 0)
        .map_err(ImportError::Failed)?;
    crate::perf_log::mark(
        "periscope.prepare",
        &format!(
            "backend=everything files={files} roots={} wall_ms={:.3}",
            roots.len(),
            started.elapsed().as_secs_f64() * 1000.0
        ),
    );
    Ok(Prepared { files, errors: 0 })
}
struct Task {
    root: PathBuf,
    dirs: VecDeque<PathBuf>,
    generation: i64,
    errors: usize,
    files: usize,
    unavailable: Option<String>,
    failed: Option<String>,
}
fn scanner(scan: Arc<Scan>, cache: PathBuf) {
    let mut store = loop {
        if scan.stop.load(Ordering::Relaxed) {
            return;
        }
        match Store::open(&cache) {
            Ok(store) => break store,
            Err(error) => {
                if scan.one_shot {
                    *scan.completion.lock().unwrap() =
                        Some(Err(format!("Cannot open index: {error}")));
                    return;
                }
                *scan.status.lock().unwrap() = format!("index open failed: {error} · retrying");
                scan.revision.fetch_add(1, Ordering::Relaxed);
                for _ in 0..10 {
                    if scan.stop.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    };
    let mut scheduled = HashSet::new();
    let mut tasks: VecDeque<Task> = VecDeque::new();
    let mut sweep = Instant::now();
    let mut completed = 0_usize;
    let mut prepared = Prepared {
        files: 0,
        errors: 0,
    };
    while !scan.stop.load(Ordering::Relaxed) {
        let roots = scan.roots.lock().unwrap().clone();
        if roots.is_empty() {
            if scan.one_shot {
                *scan.completion.lock().unwrap() = Some(Err("No search roots available".into()));
                return;
            }
            tasks.clear();
            scheduled.clear();
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        let wanted: HashSet<String> = roots
            .iter()
            .map(|root| crate::periscope_store::normalize(root))
            .collect();
        tasks.retain(|task| wanted.contains(&crate::periscope_store::normalize(&task.root)));
        scheduled.retain(|root| wanted.contains(root));
        if tasks.is_empty() && sweep.elapsed() > Duration::from_secs(300) {
            scheduled.clear();
            completed = 0;
            prepared = Prepared {
                files: 0,
                errors: 0,
            };
            sweep = Instant::now();
        }
        for root in roots.iter().rev() {
            if scheduled.insert(crate::periscope_store::normalize(root)) {
                let generation = match store.next_generation() {
                    Ok(generation) => generation,
                    Err(error) => {
                        scheduled.remove(&crate::periscope_store::normalize(root));
                        if scan.one_shot {
                            *scan.completion.lock().unwrap() =
                                Some(Err(format!("Cannot start scan: {error}")));
                            return;
                        }
                        *scan.status.lock().unwrap() = format!("index busy: {error} · retrying");
                        continue;
                    }
                };
                tasks.push_front(Task {
                    root: root.clone(),
                    dirs: VecDeque::from([root.clone()]),
                    generation,
                    errors: 0,
                    files: 0,
                    unavailable: None,
                    failed: None,
                });
            }
        }
        let Some(mut task) = tasks.pop_front() else {
            if scan.one_shot {
                if scan.stop.load(Ordering::Relaxed) {
                    return;
                }
                let result = store
                    .mark_prepared(&roots, prepared.errors)
                    .map(|()| prepared.clone());
                *scan.completion.lock().unwrap() = Some(result);
                return;
            }
            thread::sleep(Duration::from_millis(100));
            continue;
        };
        if let Some(dir) = task.dirs.pop_front() {
            *scan.status.lock().unwrap() = format!(
                "indexing {} · {} files · {} skipped/errors · {completed}/{} roots complete",
                task.root.display(),
                task.files,
                task.errors,
                roots.len()
            );
            match fs::read_dir(&dir) {
                Err(error) => {
                    task.errors += 1;
                    if dir == task.root {
                        task.unavailable = Some(error.to_string());
                    }
                }
                Ok(entries) => {
                    let mut batch = Vec::new();
                    let mut directory_ok = true;
                    for entry in entries {
                        if scan.stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(_) => {
                                task.errors += 1;
                                directory_ok = false;
                                continue;
                            }
                        };
                        let path = entry.path();
                        if cache
                            .parent()
                            .is_some_and(|parent| path.starts_with(parent))
                        {
                            continue;
                        }
                        if path.to_str().is_none() {
                            task.errors += 1;
                            directory_ok = false;
                            continue;
                        }
                        let metadata = match fs::symlink_metadata(&path) {
                            Ok(metadata) => metadata,
                            Err(_) => {
                                task.errors += 1;
                                directory_ok = false;
                                continue;
                            }
                        };
                        if special(&metadata) {
                            continue;
                        }
                        if metadata.is_dir() {
                            task.dirs.push_back(path);
                        } else if metadata.is_file() {
                            let modified = metadata
                                .modified()
                                .ok()
                                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|time| time.as_secs() as i64)
                                .unwrap_or(0);
                            batch.push(Entry {
                                path,
                                size: metadata.len(),
                                modified,
                            });
                            task.files += 1;
                        }
                        if batch.len() >= 256 {
                            if let Err(error) = store.write(&task.root, task.generation, &batch) {
                                task.failed = Some(error);
                                task.errors += 1;
                                directory_ok = false;
                            }
                            batch.clear();
                            *scan.status.lock().unwrap() = format!(
                                "indexing {} · {} files · {} skipped/errors · {completed}/{} roots complete",
                                task.root.display(),
                                task.files,
                                task.errors,
                                roots.len()
                            );
                            scan.revision.fetch_add(1, Ordering::Relaxed);
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                    if let Err(error) = store.write(&task.root, task.generation, &batch) {
                        task.failed = Some(error);
                        task.errors += 1;
                        directory_ok = false;
                    }
                    if directory_ok {
                        if let Err(error) =
                            store.finish_directory(&task.root, &dir, task.generation)
                        {
                            task.failed = Some(error);
                            task.errors += 1;
                        }
                    }
                }
            }
        }
        if scan.one_shot {
            if let Some(error) = &task.failed {
                *scan.completion.lock().unwrap() =
                    Some(Err(format!("Index write failed: {error}")));
                return;
            }
        }
        if task.dirs.is_empty() {
            if let Err(error) = store.finish(&task.root, task.generation, task.errors) {
                if scan.one_shot {
                    *scan.completion.lock().unwrap() =
                        Some(Err(format!("Cannot finish scan: {error}")));
                    return;
                }
                task.errors += 1;
            }
            if scan.one_shot {
                if let Some(error) = &task.unavailable {
                    *scan.completion.lock().unwrap() = Some(Err(format!(
                        "Root unavailable {}: {error}",
                        task.root.display()
                    )));
                    return;
                }
            }
            completed += 1;
            prepared.files += task.files;
            prepared.errors += task.errors;
            *scan.status.lock().unwrap() = if let Some(error) = task.unavailable {
                format!(
                    "root unavailable {}: {error} · cached paths retained",
                    task.root.display()
                )
            } else {
                format!(
                    "scan complete {} · {} files · {} skipped/errors · periodic refresh 5 min",
                    task.root.display(),
                    task.files,
                    task.errors
                )
            };
        } else {
            tasks.push_back(task);
        }
        scan.revision.fetch_add(1, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(5));
    }
}
fn special(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}
pub(crate) fn default_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLogicalDrives() -> u32;
            fn GetDriveTypeW(root: *const u16) -> u32;
        }
        let mask = unsafe { GetLogicalDrives() };
        (0..26)
            .filter_map(|index| {
                if mask & (1 << index) == 0 {
                    return None;
                }
                let root = format!("{}:\\", (b'A' + index) as char);
                let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
                (unsafe { GetDriveTypeW(wide.as_ptr()) } == 3).then(|| PathBuf::from(root))
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        dirs_next::home_dir().into_iter().collect()
    }
}
impl crate::SlateApp {
    pub(super) fn set_periscope_everything(&mut self, enabled: bool) {
        self.periscope_everything = enabled;
        self.status = match self.save_settings() {
            Ok(()) => "Periscope preferences saved".into(),
            Err(error) => error,
        };
        if self.periscope_open {
            self.refresh_periscope_results();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "manual read-only global Everything search for the user's Desktop CMD"]
    fn live_global_search_finds_abbreviated_spaced_desktop_filename() {
        let manager =
            Manager::new(std::env::temp_dir().join("slate-readonly-search-unused.sqlite3"));
        manager.shared.latest.store(1, Ordering::Relaxed);
        if let Some(desktop) = dirs_next::desktop_dir() {
            let scoped = search_everything(&[desktop], "abrir mi", &manager.shared, 1).unwrap();
            assert_eq!(scoped.3, "native");
            assert!(
                scoped
                    .0
                    .iter()
                    .any(|path| path.file_name().is_some_and(|name| name
                        .to_string_lossy()
                        .eq_ignore_ascii_case("Abrir Miniaturas Lychee.cmd")))
            );
        }
        for query in ["abrir mi", "mi abrir", "Abrir minis"] {
            let started = Instant::now();
            let (paths, total, evaluated, route) =
                search_everything(&default_roots(), query, &manager.shared, 1).unwrap();
            assert!(
                paths
                    .iter()
                    .any(|path| path.file_name().is_some_and(|name| name
                        .to_string_lossy()
                        .eq_ignore_ascii_case("Abrir Miniaturas Lychee.cmd"))),
                "missing Desktop CMD: query={query} total={total} evaluated={evaluated}"
            );
            if query != "Abrir minis" {
                assert_eq!(route, "native");
            }
            println!(
                "DESKTOP_CMD_QUERY query={query:?} route={route} total={total} evaluated={evaluated} results={} wall_ms={:.3}",
                paths.len(),
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    #[test]
    #[ignore = "manual Everything multi-page import into owned temporary DB"]
    fn live_inventory_import_repo_metadata_into_temporary_cache() {
        let root = PathBuf::from("D:/slate");
        let base =
            std::env::temp_dir().join(format!("slate-inventory-live-{}", crate::test_unique()));
        let cache = base.join("paths.sqlite3");
        let scan = import_scan(vec![root.clone()]);
        let start = Instant::now();
        let result = import_inventory(
            &scan,
            &cache,
            &[root.clone()],
            &mut everything_ipc::inventory,
        );
        if let Ok(result) = &result {
            assert!(Store::prepared(&cache, &[root]).unwrap());
            println!(
                "EVERYTHING_IMPORT files={} wall_ms={:.3}",
                result.files,
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        let _ = fs::remove_dir_all(base);
        result.unwrap();
    }
    fn import_scan(roots: Vec<PathBuf>) -> Scan {
        Scan {
            roots: Mutex::new(roots),
            status: Arc::new(Mutex::new(String::new())),
            revision: AtomicU64::new(0),
            stop: Arc::new(AtomicBool::new(false)),
            one_shot: true,
            fallback: Mutex::new(None),
            completion: Mutex::new(None),
        }
    }
    #[test]
    fn inventory_import_pages_all_paths_and_metadata_without_any_filesystem_inventory() {
        let base = std::env::temp_dir().join(format!("slate-inventory-{}", crate::test_unique()));
        let root = base.join("does-not-exist");
        let cache = base.join("cache/index.sqlite3");
        let scan = import_scan(vec![root.clone()]);
        let mut calls = 0;
        let result = import_inventory(&scan, &cache, &[root.clone()], &mut |_, offset| {
            calls += 1;
            let count = (20_010 - offset).min(8192);
            Ok(crate::everything_inventory::Page {
                total: 20_010,
                offset,
                returned: count,
                entries: (offset..offset + count)
                    .map(|index| Entry {
                        path: root.join(format!("file-{index:05}.md")),
                        size: 77,
                        modified: 99,
                    })
                    .collect(),
            })
        })
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(result.files, 20_010);
        assert!(!root.exists());
        assert!(Store::prepared(&cache, &[root.clone()]).unwrap());
        let store = Store::open(&cache).unwrap();
        let result = store
            .search(&[root.clone()], "file-20009", &AtomicU64::new(1), 1)
            .unwrap();
        assert_eq!(result.0, vec![root.join("file-20009.md")]);
        drop(store);
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn interrupted_or_malformed_inventory_never_marks_ready_or_switches_to_a_disk_walk() {
        for malformed in [false, true] {
            let base = std::env::temp_dir().join(format!(
                "slate-inventory-interrupt-{}",
                crate::test_unique()
            ));
            let root = base.join("no-filesystem-root");
            let cache = base.join("cache/index.sqlite3");
            let scan = import_scan(vec![root.clone()]);
            let result = import_inventory(&scan, &cache, &[root.clone()], &mut |_, offset| {
                if offset == 0 {
                    Ok(crate::everything_inventory::Page {
                        total: 2,
                        offset,
                        returned: 1,
                        entries: vec![Entry {
                            path: root.join("first.md"),
                            size: 1,
                            modified: 1,
                        }],
                    })
                } else if malformed {
                    Ok(crate::everything_inventory::Page {
                        total: 3,
                        offset: 42,
                        returned: 1,
                        entries: vec![],
                    })
                } else {
                    Err("IPC disconnected".into())
                }
            });
            assert!(matches!(result, Err(ImportError::Failed(_))));
            assert!(!Store::prepared(&cache, &[root.clone()]).unwrap());
            let store = Store::open(&cache).unwrap();
            assert_eq!(
                store
                    .search(&[root], "first.md", &AtomicU64::new(1), 1)
                    .unwrap()
                    .0
                    .len(),
                1
            );
            drop(store);
            let _ = fs::remove_dir_all(base);
        }
    }
    #[test]
    fn live_inventory_growth_shrinkage_and_duplicate_paths_finish_without_restarting() {
        for mode in 0..4 {
            let base = std::env::temp_dir()
                .join(format!("slate-inventory-churn-{}", crate::test_unique()));
            let root = base.join("nonexistent-root");
            let cache = base.join("cache/index.sqlite3");
            let scan = import_scan(vec![root.clone()]);
            let mut offsets = Vec::new();
            let result = import_inventory(&scan, &cache, &[root.clone()], &mut |_, offset| {
                offsets.push(offset);
                assert!(
                    offsets.len() <= 2,
                    "must not restart or chase continuous growth"
                );
                let (total, count, first) = if offset == 0 {
                    (20, 10, 0)
                } else {
                    match mode {
                        0 => (20 + offset * 100, 10, offset),
                        1 => (15, 5, offset),
                        2 => (3, 0, offset),
                        _ => (20 + offset * 100, 10, 0),
                    }
                };
                Ok(crate::everything_inventory::Page {
                    total,
                    returned: count,
                    offset,
                    entries: (first..first + count)
                        .map(|index| Entry {
                            path: root.join(format!("file-{index}.md")),
                            size: 42,
                            modified: 99,
                        })
                        .collect(),
                })
            })
            .unwrap();
            assert_eq!(offsets, vec![0, 10]);
            assert_eq!(result.files, [20, 15, 10, 10][mode]);
            assert!(Store::prepared(&cache, &[root.clone()]).unwrap());
            assert!(!root.exists());
            let _ = fs::remove_dir_all(base);
        }
    }
    #[test]
    fn native_inventory_only_requests_fallback_for_unindexed_roots() {
        let base =
            std::env::temp_dir().join(format!("slate-inventory-mixed-{}", crate::test_unique()));
        let native = base.join("native-without-a-filesystem-root");
        let fallback = base.join("unindexed-root");
        let cache = base.join("cache/index.sqlite3");
        let roots = vec![native.clone(), fallback.clone()];
        let scan = import_scan(roots.clone());
        let mut calls = 0;
        let result = import_inventory(&scan, &cache, &roots, &mut |_, _| {
            calls += 1;
            if calls == 1 {
                Ok(crate::everything_inventory::Page {
                    total: 1,
                    returned: 1,
                    offset: 0,
                    entries: vec![Entry {
                        path: native.join("native.md"),
                        size: 123,
                        modified: 456,
                    }],
                })
            } else {
                Ok(crate::everything_inventory::Page {
                    total: 0,
                    returned: 0,
                    offset: 0,
                    entries: vec![],
                })
            }
        });
        let Err(ImportError::Unavailable {
            roots: missing,
            files,
            ..
        }) = result
        else {
            panic!("expected one unindexed scope");
        };
        assert_eq!(missing, vec![fallback]);
        assert_eq!(files, 1);
        assert!(!native.exists());
        assert!(!Store::prepared(&cache, &roots).unwrap());
        let store = Store::open(&cache).unwrap();
        assert_eq!(
            store
                .search(&[native], "native", &AtomicU64::new(1), 1)
                .unwrap()
                .0
                .len(),
            1
        );
        drop(store);
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn a_new_native_query_cancels_an_inflight_one_without_triggering_filesystem_fallback() {
        static ENTERED: AtomicBool = AtomicBool::new(false);
        static CANCELLED: AtomicBool = AtomicBool::new(false);
        fn provider(
            search: &str,
            _: usize,
            _: bool,
            cancel: &dyn Fn() -> bool,
        ) -> Result<everything_ipc::Page, String> {
            if search.contains("\"slow\"") {
                ENTERED.store(true, Ordering::SeqCst);
                let start = Instant::now();
                while !cancel() {
                    assert!(start.elapsed() < Duration::from_secs(3));
                    thread::sleep(Duration::from_millis(1));
                }
                CANCELLED.store(true, Ordering::SeqCst);
                return Err("cancelled".into());
            }
            Ok(everything_ipc::Page {
                paths: vec![PathBuf::from("C:/fixture/latest.cmd")],
                total: 1,
                returned: 1,
            })
        }
        ENTERED.store(false, Ordering::SeqCst);
        CANCELLED.store(false, Ordering::SeqCst);
        let cache = std::env::temp_dir().join(format!(
            "slate-cancel-native-{}.sqlite3",
            crate::test_unique()
        ));
        let mut manager = Manager::new(cache.clone());
        Arc::get_mut(&mut manager.shared).unwrap().normal_provider = Some(provider);
        let request = |query: &str| Request {
            query: query.into(),
            project: None,
            roots: vec![PathBuf::from("C:/fixture")],
            everything: true,
        };
        manager.request(request("slow"));
        let start = Instant::now();
        while !ENTERED.load(Ordering::SeqCst) {
            assert!(start.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(1));
        }
        let latest = manager.request(request("latest"));
        loop {
            if let Some(result) = manager.poll() {
                assert_eq!(result.id, latest);
                assert!(result.status.contains("native"));
                assert_eq!(result.paths, vec![PathBuf::from("C:/fixture/latest.cmd")]);
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(1));
        }
        assert!(CANCELLED.load(Ordering::SeqCst));
        assert!(!cache.exists());
    }
    #[test]
    fn empty_search_is_idle_without_ipc_or_starting_a_filesystem_scan() {
        fn unexpected(
            _: &str,
            _: usize,
            _: &dyn Fn() -> bool,
        ) -> Result<everything_ipc::Page, String> {
            panic!("empty search must not query IPC");
        }
        let cache = std::env::temp_dir().join(format!(
            "slate-empty-query-{}.sqlite3",
            crate::test_unique()
        ));
        let mut manager = Manager::new(cache.clone());
        Arc::get_mut(&mut manager.shared).unwrap().provider = unexpected;
        Arc::get_mut(&mut manager.shared).unwrap().normal_provider = None;
        manager.request(Request {
            query: String::new(),
            project: None,
            roots: vec![cache.with_extension("unavailable-root")],
            everything: true,
        });
        let start = Instant::now();
        loop {
            if let Some(result) = manager.poll() {
                assert!(result.paths.is_empty());
                assert!(result.status.contains("Ready"));
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!cache.exists());
    }
    #[test]
    fn inventory_cancel_and_out_of_scope_entries_do_not_mark_ready() {
        let base =
            std::env::temp_dir().join(format!("slate-inventory-cancel-{}", crate::test_unique()));
        let root = base.join("root");
        let cache = base.join("cache/index.sqlite3");
        let scan = import_scan(vec![root.clone()]);
        scan.stop.store(true, Ordering::Relaxed);
        assert!(
            import_inventory(&scan, &cache, &[root.clone()], &mut |_, _| panic!(
                "cancelled before IPC"
            ))
            .is_err()
        );
        assert!(!cache.exists());
        scan.stop.store(false, Ordering::Relaxed);
        assert!(matches!(
            import_inventory(&scan, &cache, &[root.clone()], &mut |_, _| Ok(
                crate::everything_inventory::Page {
                    total: 1,
                    returned: 1,
                    offset: 0,
                    entries: vec![Entry {
                        path: base.join("outside.md"),
                        size: 0,
                        modified: 0
                    }]
                }
            )),
            Err(ImportError::Failed(_))
        ));
        assert!(!cache.exists());
    }
    #[test]
    fn periscope_settings_roundtrip_roots_and_keyboard_toggle_use_owned_config() {
        let root =
            std::env::temp_dir().join(format!("slate-periscope-config-{}", crate::test_unique()));
        fs::create_dir_all(&root).unwrap();
        let ctx = eframe::egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = crate::SlateApp::new(&cc, None, true, None);
        app.settings_path_override = Some(root.join("settings.toml"));
        app.periscope_roots = vec![root.join("notes_100%")];
        app.set_periscope_everything(false);
        app.periscope_everything = true;
        app.periscope_roots.clear();
        app.load_settings();
        assert!(!app.periscope_everything);
        assert_eq!(app.periscope_roots, vec![root.join("notes_100%")]);
        app.settings_open = true;
        app.selected_setting = 18;
        let _ = ctx.run(
            eframe::egui::RawInput {
                events: vec![eframe::egui::Event::Key {
                    key: eframe::egui::Key::Enter,
                    physical_key: Some(eframe::egui::Key::Enter),
                    pressed: true,
                    repeat: false,
                    modifiers: eframe::egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| app.shortcuts(ctx),
        );
        assert!(app.periscope_everything);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn app_applies_only_latest_search_and_keeps_dirty_main_when_open_is_requested() {
        let root =
            std::env::temp_dir().join(format!("slate-periscope-ui-{}", crate::test_unique()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("earlier.md"), "earlier").unwrap();
        fs::write(root.join("latest.md"), "latest").unwrap();
        let ctx = eframe::egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut app = crate::SlateApp::new(&cc, None, true, None);
        app.periscope_engine = Manager::new(root.join("cache/index.sqlite3"));
        app.periscope_everything = false;
        app.periscope_roots = vec![root.clone()];
        app.periscope_mode = crate::periscope::PeriscopeMode::Global;
        app.periscope_open = true;
        app.buffer.set_text("visible unsaved text".into());
        app.dirty = true;
        app.periscope_query = "earlier.md".into();
        app.refresh_periscope_results();
        app.periscope_query = "latest.md".into();
        app.refresh_periscope_results();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            app.poll_periscope_global_index();
            if app.periscope_results.contains(&root.join("latest.md")) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.periscope_query, "latest.md");
        assert_eq!(app.periscope_request, Some(2));
        assert_eq!(app.periscope_results.first(), Some(&root.join("latest.md")));
        app.open_selected_periscope_result();
        // A window without tabs asks before replacing the unsaved text.
        assert!(!app.periscope_open);
        assert!(matches!(
            app.pending_action,
            Some(crate::PendingAction::OpenProjectFile)
        ));
        assert!(app.dirty);
        assert_eq!(app.buffer.as_str(), "visible unsaved text");
        app.poll_periscope_global_index();
        assert!(app.periscope_request.is_none());
        drop(app);
        thread::sleep(Duration::from_millis(150));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn unavailable_everything_automatically_falls_back_without_touching_documents() {
        let root =
            std::env::temp_dir().join(format!("slate-periscope-fallback-{}", crate::test_unique()));
        fs::create_dir_all(&root).unwrap();
        let document = root.join("needle.md");
        fs::write(&document, "document unchanged").unwrap();
        let mut manager = Manager::new(root.join("cache/index.sqlite3"));
        Arc::get_mut(&mut manager.shared).unwrap().provider =
            |_, _, _| Err("simulated Everything unavailable".into());
        Arc::get_mut(&mut manager.shared).unwrap().normal_provider = None;
        manager.request(Request {
            query: "needle".into(),
            project: Some(root.clone()),
            roots: vec![],
            everything: true,
        });
        let end = Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while Instant::now() < end {
            if let Some(result) = manager.poll() {
                assert!(result.status.contains("simulated Everything unavailable"));
                if result.paths.contains(&document) {
                    assert!(result.status.contains("SQLite"));
                    found = true;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(found);
        assert_eq!(fs::read_to_string(document).unwrap(), "document unchanged");
        drop(manager);
        thread::sleep(Duration::from_millis(150));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn coalescing_returns_latest_query_and_cancel_discards_late_result() {
        let root =
            std::env::temp_dir().join(format!("slate-periscope-worker-{}", crate::test_unique()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("café.md"), "not read by scanner").unwrap();
        fs::write(root.join("latest.md"), "text").unwrap();
        let cache = root.join("cache/index.sqlite3");
        let mut manager = Manager::new(cache);
        for query in ["café", "latest"] {
            manager.request(Request {
                query: query.into(),
                project: Some(root.clone()),
                roots: vec![],
                everything: false,
            });
        }
        let end = Instant::now() + Duration::from_secs(5);
        let mut result = None;
        while Instant::now() < end {
            if let Some(value) = manager.poll() {
                assert_eq!(value.id, 2);
                if value.paths.iter().any(|path| path.ends_with("latest.md")) {
                    result = Some(value);
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(result.is_some());
        manager.cancel();
        assert!(manager.poll().is_none());
        drop(manager);
        thread::sleep(Duration::from_millis(150));
        let _ = fs::remove_dir_all(root);
    }
}
