//! Flow as a module: an index of the tasks in your vaults, kept current
//! from the editor's document events, and `:flow` to see them.
//!
//! - Only declared vaults (Settings → Modules & vaults) are indexed.
//! - Opening a note in a vault scans the vault in the background: only notes
//!   whose size or modification time changed are read again.
//! - Saving a note reindexes it; a pause in typing (`document_idle`) makes
//!   its unsaved text visible to `:flow` without writing it anywhere.
//! - Everything runs on the module's own thread; hooks only send it work.
use super::index::{self, Index, Item, Note};
use crate::commands::{Command, CommandSpec};
use crate::module_api::{Cx, DocumentRef, Module, ResultItem, Waker};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant, UNIX_EPOCH},
};

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "flow",
        aliases: &["todo"],
        summary: "Open tasks of this note's vault (Flow)",
        hint: ":flow",
        palette_command: Some(Command::Module("flow")),
    },
    CommandSpec {
        name: "flow-reindex",
        aliases: &[],
        summary: "Rebuild the Flow index of this note's vault",
        hint: ":flow-reindex",
        palette_command: Some(Command::Module("flow-reindex")),
    },
];

/// Notes looked at in one vault, at most.
const MAX_NOTES: usize = 20_000;
/// Notes written per transaction while scanning.
const BATCH: usize = 64;
/// Opening notes rescans a vault at most this often.
const RESCAN_AFTER: Duration = Duration::from_secs(60);

enum Job {
    Scan {
        vault: PathBuf,
        rebuild: bool,
        report: bool,
    },
    Saved {
        vault: PathBuf,
        path: PathBuf,
        text: String,
    },
    Live {
        vault: PathBuf,
        path: PathBuf,
        text: String,
    },
    Forget {
        path: PathBuf,
    },
    Tasks {
        vault: PathBuf,
        query: u64,
    },
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ScanReport {
    pub(crate) notes: usize,
    pub(crate) read: usize,
    pub(crate) removed: usize,
    pub(crate) truncated: bool,
}

#[derive(Default)]
struct Outbox {
    messages: Vec<String>,
    tasks: Option<(u64, PathBuf, Vec<ResultItem>)>,
}

#[derive(Default)]
pub(crate) struct Flow {
    jobs: Option<Sender<Job>>,
    outbox: Arc<Mutex<Outbox>>,
    scanned: HashMap<PathBuf, Instant>,
    /// Open notes in a vault, to forget their unsaved text when closed.
    open: HashMap<u64, PathBuf>,
    query: u64,
}

impl Flow {
    fn send(&mut self, cx: &Cx, job: Job) {
        let jobs = self.jobs.get_or_insert_with(|| {
            let (sender, receiver) = channel();
            let outbox = self.outbox.clone();
            let waker = cx.waker();
            let data_dir = cx.data_dir().to_path_buf();
            std::thread::Builder::new()
                .name("slate-flow".into())
                .spawn(move || run(receiver, data_dir, outbox, waker))
                .expect("Flow thread");
            sender
        });
        let _ = jobs.send(job);
    }

    fn scan(&mut self, cx: &Cx, vault: &Path, rebuild: bool, report: bool) {
        let fresh = self
            .scanned
            .get(vault)
            .is_some_and(|at| at.elapsed() < RESCAN_AFTER);
        if fresh && !rebuild && !report {
            return;
        }
        self.scanned.insert(vault.to_path_buf(), Instant::now());
        self.send(
            cx,
            Job::Scan {
                vault: vault.to_path_buf(),
                rebuild,
                report,
            },
        );
    }

    /// The note and its vault, when the note is in one.
    fn note_in_vault(cx: &Cx, doc: &DocumentRef) -> Option<(PathBuf, PathBuf)> {
        let path = doc.path?;
        if !crate::vaults::is_note(path) {
            return None;
        }
        Some((path.to_path_buf(), cx.declared_vault(path)?))
    }
}

impl Module for Flow {
    fn id(&self) -> &'static str {
        "flow"
    }

    fn name(&self) -> &'static str {
        "Flow · tasks"
    }

    fn description(&self) -> &'static str {
        "Indexes the tasks (- [ ] …) of your vaults in the background; :flow lists the open ones"
    }

    fn commands(&self) -> &'static [CommandSpec] {
        COMMANDS
    }

    fn run(&mut self, cx: &mut Cx, command: &str, _args: &str, doc: Option<DocumentRef>) {
        let vault = doc
            .and_then(|doc| doc.path)
            .and_then(|path| cx.declared_vault(path))
            .or_else(|| (cx.vaults.len() == 1).then(|| cx.vaults[0].clone()));
        let Some(vault) = vault else {
            cx.status(if cx.vaults.is_empty() {
                "Flow: add your notes folder in Settings → Modules (Tab to it), Vaults · Add folder…"
            } else {
                "Flow: this note is not in a vault (Settings → Modules lists them)"
            });
            return;
        };
        let name = folder_name(&vault);
        if command == "flow-reindex" {
            self.scan(cx, &vault, true, true);
            cx.status(format!("Flow: rebuilding the index of {name}…"));
            return;
        }
        // Scanned this session: answer from the index at once, then catch
        // up with changes made outside Slate. Never scanned: scan first.
        let scanned = self.scanned.contains_key(&vault);
        if !scanned {
            self.scan(cx, &vault, false, false);
        }
        self.query += 1;
        let query = self.query;
        self.send(
            cx,
            Job::Tasks {
                vault: vault.clone(),
                query,
            },
        );
        if scanned {
            self.scan(cx, &vault, false, false);
        }
        cx.status(format!("Tasks of {name}…"));
    }

    fn document_opened(&mut self, cx: &mut Cx, doc: DocumentRef) {
        if let Some((path, vault)) = Self::note_in_vault(cx, &doc) {
            self.open.insert(doc.id, path);
            self.scan(cx, &vault, false, false);
        }
    }

    fn document_idle(&mut self, cx: &mut Cx, doc: DocumentRef) {
        if let Some((path, vault)) = Self::note_in_vault(cx, &doc) {
            self.open.insert(doc.id, path.clone());
            let text = doc.text.to_string();
            self.send(cx, Job::Live { vault, path, text });
        }
    }

    fn document_saved(&mut self, cx: &mut Cx, doc: DocumentRef) {
        if let Some((path, vault)) = Self::note_in_vault(cx, &doc) {
            self.open.insert(doc.id, path.clone());
            let text = doc.text.to_string();
            self.send(cx, Job::Saved { vault, path, text });
        }
    }

    fn document_closed(&mut self, cx: &mut Cx, id: u64) {
        if let Some(path) = self.open.remove(&id) {
            self.send(cx, Job::Forget { path });
        }
    }

    fn wake(&mut self, cx: &mut Cx) {
        let (messages, tasks) = {
            let Ok(mut outbox) = self.outbox.lock() else {
                return;
            };
            (std::mem::take(&mut outbox.messages), outbox.tasks.take())
        };
        for message in messages {
            cx.status(message);
        }
        if let Some((query, vault, items)) = tasks
            && query == self.query
        {
            cx.status(format!("Tasks of {}: {} open", folder_name(&vault), items.len()));
            cx.show_results(format!("Tasks · {}", folder_name(&vault)), items);
        }
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The vault-relative, `/`-separated key of `path`.
fn relative(vault: &Path, path: &Path) -> String {
    path.strip_prefix(vault)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn stamp(path: &Path) -> Option<(i64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos() as i64;
    Some((mtime, meta.len() as i64))
}

/// The worker: owns the vaults' indexes and the unsaved text of open notes.
struct Worker {
    data_dir: PathBuf,
    indexes: HashMap<PathBuf, Index>,
    /// Unsaved items of open notes (absolute path → vault, items).
    live: HashMap<PathBuf, (PathBuf, Vec<Item>)>,
}

fn run(jobs: Receiver<Job>, data_dir: PathBuf, outbox: Arc<Mutex<Outbox>>, waker: Waker) {
    let mut worker = Worker {
        data_dir,
        indexes: HashMap::new(),
        live: HashMap::new(),
    };
    while let Ok(job) = jobs.recv() {
        let started = Instant::now();
        let kind = match &job {
            Job::Scan { .. } => "scan",
            Job::Saved { .. } => "saved",
            Job::Live { .. } => "live",
            Job::Forget { .. } => "forget",
            Job::Tasks { .. } => "tasks",
        };
        let message = match job {
            Job::Scan {
                vault,
                rebuild,
                report,
            } => match worker.scan(&vault, rebuild) {
                Ok(done) => {
                    crate::perf_log::mark(
                        "flow.scan",
                        &format!(
                            "notes={} read={} removed={} ms={:.1}",
                            done.notes,
                            done.read,
                            done.removed,
                            started.elapsed().as_secs_f64() * 1000.0
                        ),
                    );
                    report.then(|| {
                        format!(
                            "Flow: {} indexed · {} notes{}",
                            folder_name(&vault),
                            done.notes,
                            if done.truncated {
                                format!(" (stopped at {MAX_NOTES})")
                            } else {
                                String::new()
                            }
                        )
                    })
                }
                Err(err) => Some(format!("Flow: index of {} failed: {err}", folder_name(&vault))),
            },
            Job::Saved { vault, path, text } => {
                worker.live.remove(&path);
                worker
                    .saved(&vault, &path, &text)
                    .err()
                    .map(|err| format!("Flow: could not index {}: {err}", folder_name(&path)))
            }
            Job::Live { vault, path, text } => {
                let items = index::items_of(&relative(&vault, &path), &text);
                worker.live.insert(path, (vault, items));
                None
            }
            Job::Forget { path } => {
                worker.live.remove(&path);
                None
            }
            Job::Tasks { vault, query } => match worker.open_tasks(&vault) {
                Ok(items) => {
                    if let Ok(mut outbox) = outbox.lock() {
                        outbox.tasks = Some((query, vault, items));
                    }
                    waker.wake();
                    None
                }
                Err(err) => Some(format!("Flow: tasks of {} failed: {err}", folder_name(&vault))),
            },
        };
        let elapsed = started.elapsed();
        if elapsed > Duration::from_millis(50) && kind != "scan" {
            crate::perf_log::mark(
                "flow.job",
                &format!("job={kind} ms={:.1}", elapsed.as_secs_f64() * 1000.0),
            );
        }
        if let Some(message) = message {
            if let Ok(mut outbox) = outbox.lock() {
                outbox.messages.push(message);
            }
            waker.wake();
        }
    }
}

impl Worker {
    fn index(&mut self, vault: &Path) -> Result<&mut Index, String> {
        if !self.indexes.contains_key(vault) {
            let index = Index::open(&index::file_for(&self.data_dir, vault))?;
            self.indexes.insert(vault.to_path_buf(), index);
        }
        Ok(self.indexes.get_mut(vault).expect("just opened"))
    }

    /// Reads the notes that changed since the last scan; forgets the gone.
    fn scan(&mut self, vault: &Path, rebuild: bool) -> Result<ScanReport, String> {
        let index = self.index(vault)?;
        if rebuild {
            index.clear()?;
        }
        let known = index.files()?;
        let (notes, truncated) = crate::vaults::notes(vault, MAX_NOTES);
        let mut seen = HashSet::new();
        let changed: Vec<(PathBuf, String, (i64, i64))> = notes
            .iter()
            .filter_map(|path| {
                let key = relative(vault, path);
                seen.insert(key.clone());
                let stamp = stamp(path)?;
                (known.get(&key) != Some(&stamp)).then(|| (path.clone(), key, stamp))
            })
            .collect();
        for chunk in changed.chunks(BATCH) {
            let notes = crate::parallel::par_map(chunk, |(path, key, (mtime, size))| Note {
                path: key.clone(),
                mtime: *mtime,
                size: *size,
                // Unreadable (not UTF-8): stored without items, not retried.
                items: std::fs::read_to_string(path)
                    .map(|text| index::items_of(key, &text))
                    .unwrap_or_default(),
            });
            index.write(&notes)?;
        }
        let removed: Vec<String> = if truncated {
            Vec::new()
        } else {
            known.into_keys().filter(|key| !seen.contains(key)).collect()
        };
        index.remove(&removed)?;
        Ok(ScanReport {
            notes: notes.len(),
            read: changed.len(),
            removed: removed.len(),
            truncated,
        })
    }

    fn saved(&mut self, vault: &Path, path: &Path, text: &str) -> Result<(), String> {
        let key = relative(vault, path);
        let (mtime, size) = stamp(path).unwrap_or_default();
        let items = index::items_of(&key, text);
        self.index(vault)?.write(&[Note {
            path: key,
            mtime,
            size,
            items,
        }])
    }

    /// Open tasks of `vault`, unsaved text included, most urgent first.
    fn open_tasks(&mut self, vault: &Path) -> Result<Vec<ResultItem>, String> {
        let live: Vec<(String, Vec<Item>)> = self
            .live
            .iter()
            .filter(|(_, (live_vault, _))| live_vault == vault)
            .map(|(path, (_, items))| (relative(vault, path), items.clone()))
            .collect();
        let shadowed: HashSet<&str> = live.iter().map(|(key, _)| key.as_str()).collect();
        let mut items: Vec<Item> = self
            .index(vault)?
            .open_tasks()?
            .into_iter()
            .filter(|item| !shadowed.contains(item.path.as_str()))
            .collect();
        items.extend(
            live.iter()
                .flat_map(|(_, items)| items.iter())
                .filter(|item| item.kind == "task" && item.state != "done")
                .cloned(),
        );
        items.sort_by(|a, b| {
            let due = |item: &Item| item.due.clone().unwrap_or_else(|| "~".into());
            let priority = |item: &Item| match item.priority.as_deref() {
                Some("ALTA") => 0,
                Some("MEDIA") => 1,
                Some("BAJA") => 2,
                _ => 3,
            };
            (due(a), priority(a), &a.path, a.line).cmp(&(due(b), priority(b), &b.path, b.line))
        });
        Ok(items
            .into_iter()
            .map(|item| result_item(vault, item))
            .collect())
    }
}

fn result_item(vault: &Path, item: Item) -> ResultItem {
    let mut label = format!(
        "{} {}",
        if item.state == "doing" { "[/]" } else { "[ ]" },
        item.title
    );
    if let Some(priority) = &item.priority {
        label.push_str(&format!("  {priority}"));
    }
    if let Some(due) = &item.due {
        label.push_str(&format!("  due {due}"));
    }
    for tag in &item.tags {
        label.push_str(&format!("  #{tag}"));
    }
    for project in &item.projects {
        label.push_str(&format!("  @{project}"));
    }
    let mut detail = format!("{}:{}", item.path, item.line + 1);
    if let Some(heading) = &item.heading {
        detail.push_str(&format!(" · {heading}"));
    }
    ResultItem {
        path: vault.join(&item.path),
        line: Some(item.line),
        label,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "slate-flow-{name}-{}-{}",
            std::process::id(),
            crate::test_unique()
        ));
        std::fs::create_dir_all(root.join("vault/sub")).unwrap();
        root
    }

    #[test]
    fn scans_only_what_changed_and_unsaved_text_shadows_the_index() {
        let root = temp("scan");
        let vault = root.join("vault");
        std::fs::write(vault.join("a.md"), "# Work\n- [ ] write report #work\n- [x] old\n").unwrap();
        std::fs::write(
            vault.join("sub/b.md"),
            "- [/] review @slate ALTA\n  due: 2026-11-03\n",
        )
        .unwrap();
        std::fs::write(vault.join("notes.txt"), "- [ ] not a note\n").unwrap();
        let mut worker = Worker {
            data_dir: root.join("data"),
            indexes: HashMap::new(),
            live: HashMap::new(),
        };
        let first = worker.scan(&vault, false).unwrap();
        assert_eq!((first.notes, first.read, first.removed), (2, 2, 0));
        let titles = |worker: &mut Worker| -> Vec<String> {
            worker
                .open_tasks(&vault)
                .unwrap()
                .into_iter()
                .map(|item| item.label)
                .collect()
        };
        assert_eq!(
            titles(&mut worker),
            vec![
                "[/] review  ALTA  due 2026-11-03  @slate".to_string(),
                "[ ] write report  #work".to_string(),
            ],
            "dated first; done tasks left out"
        );

        let again = worker.scan(&vault, false).unwrap();
        assert_eq!(again.read, 0, "nothing changed, nothing read");
        std::fs::remove_file(vault.join("sub/b.md")).unwrap();
        let after = worker.scan(&vault, false).unwrap();
        assert_eq!((after.read, after.removed), (0, 1));

        // Unsaved text replaces the note's indexed tasks until it is saved.
        let a = vault.join("a.md");
        worker.live.insert(
            a.clone(),
            (vault.clone(), index::items_of("a.md", "- [ ] draft\n- [ ] write report\n")),
        );
        assert_eq!(titles(&mut worker), vec!["[ ] draft", "[ ] write report"]);
        worker.live.remove(&a);
        worker.saved(&vault, &a, "- [x] write report\n").unwrap();
        assert!(titles(&mut worker).is_empty());

        // The result opens the note at the task's line.
        worker.saved(&vault, &a, "intro\n\n- [ ] third line\n").unwrap();
        let item = &worker.open_tasks(&vault).unwrap()[0];
        assert_eq!((item.path.clone(), item.line), (a.clone(), Some(2)));
        let _ = std::fs::remove_dir_all(root);
    }

    /// `SLATE_FLOW_BENCH=<folder> cargo test --release flow_scan_bench -- --ignored --nocapture`:
    /// cold and warm scans of a real vault (read-only; the index goes to temp).
    #[test]
    #[ignore]
    fn flow_scan_bench() {
        let vault = PathBuf::from(std::env::var("SLATE_FLOW_BENCH").expect("SLATE_FLOW_BENCH"));
        let root = temp("bench");
        let mut worker = Worker {
            data_dir: root.join("data"),
            indexes: HashMap::new(),
            live: HashMap::new(),
        };
        for pass in ["cold", "warm"] {
            let started = Instant::now();
            let report = worker.scan(&vault, false).unwrap();
            println!("{pass}: {report:?} in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        let started = Instant::now();
        let tasks = worker.open_tasks(&vault).unwrap();
        println!("open tasks: {} in {:.1} ms", tasks.len(), started.elapsed().as_secs_f64() * 1000.0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ids_follow_the_content_not_the_line() {
        let before = index::items_of("a.md", "- [ ] one\n- [ ] two\n- [ ] one\n");
        let after = index::items_of("a.md", "intro\n- [ ] two\n- [ ] one\n\n- [ ] one\n");
        let id = |items: &[Item], title: &str, repeat: usize| {
            items.iter().filter(|i| i.title == title).nth(repeat).unwrap().id
        };
        assert_eq!(id(&before, "two", 0), id(&after, "two", 0));
        assert_eq!(id(&before, "one", 0), id(&after, "one", 0));
        assert_eq!(id(&before, "one", 1), id(&after, "one", 1));
        assert_ne!(id(&before, "one", 0), id(&before, "one", 1));
    }
}
