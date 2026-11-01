//! Embedded SQLite cache. Connections never cross threads; writers use bounded transactions.
#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
use crate::flow::{FlowKnownFile, FlowTask};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS files (
 path TEXT PRIMARY KEY, last_seen_unix INTEGER NOT NULL, mtime_unix INTEGER NOT NULL,
 task_count INTEGER NOT NULL, has_tasks INTEGER NOT NULL, has_links INTEGER NOT NULL, has_tags INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS tasks (
 path TEXT NOT NULL, line_number INTEGER NOT NULL, line_index INTEGER NOT NULL,
 kind TEXT NOT NULL DEFAULT 'task', state TEXT NOT NULL, title TEXT NOT NULL, raw_text TEXT NOT NULL,
 tags TEXT NOT NULL, projects TEXT NOT NULL, priority TEXT, due TEXT, scheduled TEXT, heading TEXT,
 related TEXT NOT NULL, notes TEXT NOT NULL, source_snippet TEXT NOT NULL,
 PRIMARY KEY(path, line_number, title));";

pub(crate) struct Document {
    pub known_file: FlowKnownFile,
    pub tasks: Vec<FlowTask>,
    pub contents: String,
}

pub(crate) struct CacheWriter {
    connection: Connection,
}
impl CacheWriter {
    pub fn open(path: &Path, timeout: Duration) -> Result<Self, String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut connection =
            Connection::open(path).map_err(|e| format!("SQLite open failed: {e}"))?;
        connection
            .busy_timeout(timeout)
            .map_err(|e| e.to_string())?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        // Serialize migrations with other Slate processes. Existing caches are never dropped.
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        transaction
            .execute_batch(SCHEMA)
            .map_err(|e| e.to_string())?;
        if !has_kind(&transaction).map_err(|e| e.to_string())? {
            transaction
                .execute_batch("ALTER TABLE tasks ADD COLUMN kind TEXT NOT NULL DEFAULT 'task';")
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }

    pub fn write_batch(&mut self, documents: &[Document]) -> Result<(), String> {
        if documents.is_empty() {
            return Ok(());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        {
            let mut file = transaction.prepare_cached("INSERT OR REPLACE INTO files (path,last_seen_unix,mtime_unix,task_count,has_tasks,has_links,has_tags) VALUES (?,?,?,?,?,?,?);").map_err(|e| e.to_string())?;
            let mut delete = transaction
                .prepare_cached("DELETE FROM tasks WHERE path = ?;")
                .map_err(|e| e.to_string())?;
            let mut task_stmt = transaction.prepare_cached("INSERT OR REPLACE INTO tasks (path,line_number,line_index,kind,state,title,raw_text,tags,projects,priority,due,scheduled,heading,related,notes,source_snippet) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?);").map_err(|e| e.to_string())?;
            for document in documents {
                let known = &document.known_file;
                file.execute(params![
                    known.path,
                    known.last_seen_unix,
                    known.mtime_unix,
                    known.task_count,
                    known.has_tasks,
                    known.has_links,
                    known.has_tags
                ])
                .map_err(|e| e.to_string())?;
                delete
                    .execute(params![known.path])
                    .map_err(|e| e.to_string())?;
                let lines: Vec<_> = document.contents.lines().collect();
                for task in &document.tasks {
                    let snippet = lines
                        .get(task.line_index)
                        .copied()
                        .unwrap_or(&task.raw_text);
                    task_stmt
                        .execute(params![
                            known.path,
                            task.line_number,
                            task.line_index,
                            task.kind.as_str(),
                            crate::flow::flow_item_state_label(task),
                            task.title,
                            task.raw_text,
                            task.tags.join(","),
                            task.projects.join(","),
                            task.priority.map(|p| p.as_str()),
                            task.metadata.due,
                            task.metadata.scheduled,
                            task.heading,
                            task.metadata.related.join("\n"),
                            task.metadata.notes.join("\n"),
                            snippet
                        ])
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        transaction.commit().map_err(|e| e.to_string())
    }

    #[cfg(test)]
    pub fn clear(&mut self) -> Result<(), String> {
        self.clear_except(&HashSet::new())
    }

    pub fn clear_except(&mut self, protected: &HashSet<String>) -> Result<(), String> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS slate_live_paths(path TEXT PRIMARY KEY); DELETE FROM slate_live_paths;").map_err(|e| e.to_string())?;
        {
            let mut insert = transaction
                .prepare("INSERT INTO slate_live_paths VALUES (?);")
                .map_err(|e| e.to_string())?;
            for path in protected {
                insert.execute([path]).map_err(|e| e.to_string())?;
            }
        }
        transaction.execute_batch("DELETE FROM tasks WHERE path NOT IN (SELECT path FROM slate_live_paths); DELETE FROM files WHERE path NOT IN (SELECT path FROM slate_live_paths);").map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())
    }

    pub fn known_files(&self) -> Result<Vec<FlowKnownFile>, String> {
        known_files(&self.connection)
    }
}

fn known_files(connection: &Connection) -> Result<Vec<FlowKnownFile>, String> {
    let mut statement = connection.prepare("SELECT path,last_seen_unix,mtime_unix,task_count,has_tasks,has_links,has_tags FROM files ORDER BY path;").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(FlowKnownFile {
                path: row.get(0)?,
                last_seen_unix: row.get(1)?,
                mtime_unix: row.get(2)?,
                task_count: row.get(3)?,
                has_tasks: row.get(4)?,
                has_links: row.get(5)?,
                has_tags: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
pub(crate) fn read_known_files(path: &Path) -> Result<Vec<FlowKnownFile>, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    connection
        .busy_timeout(Duration::from_millis(50))
        .map_err(|e| e.to_string())?;
    known_files(&connection)
}

thread_local! {
    // UI/save/API updates reuse a small connection cache. Background scans own their writer.
    static WRITERS: RefCell<HashMap<PathBuf, CacheWriter>> = RefCell::new(HashMap::new());
}
pub(crate) fn with_writer<R>(
    path: &Path,
    f: impl FnOnce(&mut CacheWriter) -> Result<R, String>,
) -> Result<R, String> {
    WRITERS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(path) {
            if cache.len() >= 4 {
                if let Some(key) = cache.keys().next().cloned() {
                    cache.remove(&key);
                }
            }
            cache.insert(
                path.to_path_buf(),
                CacheWriter::open(path, Duration::from_millis(25))?,
            );
        }
        f(cache.get_mut(path).expect("writer was inserted"))
    })
}

pub(crate) fn is_busy_error(error: &str) -> bool {
    error.contains("database is locked") || error.contains("database table is locked")
}

fn has_kind(connection: &Connection) -> rusqlite::Result<bool> {
    let mut statement = connection.prepare("PRAGMA table_info(tasks);")?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == "kind" {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug)]
pub(crate) struct CacheTask {
    pub path: String,
    pub line_number: usize,
    pub line_index: usize,
    pub kind: String,
    pub state: String,
    pub title: String,
    pub tags: String,
    pub projects: String,
    pub priority: String,
    pub due: String,
    pub scheduled: String,
    pub heading: String,
    pub source_snippet: String,
}
pub(crate) fn read_tasks(path: &Path) -> Result<Vec<CacheTask>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("SQLite read failed: {e}"))?;
    connection
        .busy_timeout(Duration::from_millis(50))
        .map_err(|e| e.to_string())?;
    // Read older caches without a kind column, even before the first writer migrates them.
    let kind = if has_kind(&connection).map_err(|e| e.to_string())? {
        "COALESCE(kind,'task')"
    } else {
        "'task'"
    };
    let query = format!(
        "SELECT path,line_number,line_index,{kind},state,title,tags,projects,COALESCE(priority,''),COALESCE(due,''),COALESCE(scheduled,''),COALESCE(heading,''),source_snippet FROM tasks ORDER BY COALESCE(scheduled,due,''),path,line_number;"
    );
    let mut statement = connection.prepare(&query).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(CacheTask {
                path: row.get(0)?,
                line_number: row.get(1)?,
                line_index: row.get(2)?,
                kind: row.get(3)?,
                state: row.get(4)?,
                title: row.get(5)?,
                tags: row.get(6)?,
                projects: row.get(7)?,
                priority: row.get(8)?,
                due: row.get(9)?,
                scheduled: row.get(10)?,
                heading: row.get(11)?,
                source_snippet: row.get(12)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
