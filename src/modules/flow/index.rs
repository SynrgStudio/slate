//! Flow's index of one vault: a SQLite file in the module's data folder.
//! Derived data only: the notes are the truth, and deleting the file loses
//! nothing (the next scan rebuilds it).
use super::{FlowItemKind, FlowTask, flow_item_state_label, parse_flow_tasks};
use rusqlite::{Connection, params};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS files (
 path TEXT PRIMARY KEY, mtime INTEGER NOT NULL, size INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS items (
 path TEXT NOT NULL, line INTEGER NOT NULL, id INTEGER NOT NULL,
 kind TEXT NOT NULL, state TEXT NOT NULL, title TEXT NOT NULL,
 tags TEXT NOT NULL, projects TEXT NOT NULL,
 priority TEXT, due TEXT, scheduled TEXT, heading TEXT,
 PRIMARY KEY(path, line));
CREATE INDEX IF NOT EXISTS items_kind_state ON items(kind, state);";

/// A task, note, idea or inbox entry of a note.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    /// Vault-relative, `/`-separated.
    pub(crate) path: String,
    /// 0-based.
    pub(crate) line: usize,
    /// Identity by content (path, kind, title and which repeat of that
    /// title), so an item keeps it when lines move around it.
    pub(crate) id: u64,
    pub(crate) kind: &'static str,
    /// todo, doing, done (tasks); note, idea, inbox otherwise.
    pub(crate) state: &'static str,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) projects: Vec<String>,
    pub(crate) priority: Option<String>,
    pub(crate) due: Option<String>,
    pub(crate) scheduled: Option<String>,
    pub(crate) heading: Option<String>,
}

/// The items of note `path` (vault-relative) with this text.
pub(crate) fn items_of(path: &str, text: &str) -> Vec<Item> {
    let mut repeats: HashMap<(FlowItemKind, String), u32> = HashMap::new();
    parse_flow_tasks(text)
        .into_iter()
        .map(|task: FlowTask| {
            let repeat = repeats.entry((task.kind, task.title.clone())).or_default();
            let id = fnv(&[
                path.as_bytes(),
                task.kind.as_str().as_bytes(),
                task.title.as_bytes(),
                &repeat.to_le_bytes(),
            ]);
            *repeat += 1;
            Item {
                path: path.to_string(),
                line: task.line_index,
                id,
                kind: task.kind.as_str(),
                state: flow_item_state_label(&task),
                title: task.title,
                tags: task.tags,
                projects: task.projects,
                priority: task.priority.map(|p| p.as_str().to_string()),
                due: task.metadata.due,
                scheduled: task.metadata.scheduled,
                heading: task.heading,
            }
        })
        .collect()
}

/// FNV-1a: stable across builds (unlike `DefaultHasher`).
pub(crate) fn fnv(parts: &[&[u8]]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for part in parts {
        for byte in part.iter().chain(&[0xff]) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// The index file of `vault` inside `data_dir`: readable name, unique hash.
pub(crate) fn file_for(data_dir: &Path, vault: &Path) -> PathBuf {
    let key = vault.to_string_lossy().replace('\\', "/").to_lowercase();
    let name: String = vault
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    data_dir.join(format!("{name}-{:016x}.sqlite3", fnv(&[key.as_bytes()])))
}

/// One note as stored: its path, change stamp and items.
pub(crate) struct Note {
    pub(crate) path: String,
    pub(crate) mtime: i64,
    pub(crate) size: i64,
    pub(crate) items: Vec<Item>,
}

pub(crate) struct Index {
    db: Connection,
}

impl Index {
    pub(crate) fn open(file: &Path) -> Result<Self, String> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let db = Connection::open(file).map_err(|e| format!("Flow index: {e}"))?;
        // Every Slate window indexes on its own thread; they take turns.
        db.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| e.to_string())?;
        db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Self { db })
    }

    /// Change stamps (mtime, size) of the notes indexed so far.
    pub(crate) fn files(&self) -> Result<HashMap<String, (i64, i64)>, String> {
        let mut statement = self
            .db
            .prepare("SELECT path, mtime, size FROM files")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// Replaces what is stored for these notes, in one transaction.
    pub(crate) fn write(&mut self, notes: &[Note]) -> Result<(), String> {
        let transaction = self.db.transaction().map_err(|e| e.to_string())?;
        {
            let mut file = transaction
                .prepare_cached("INSERT OR REPLACE INTO files (path, mtime, size) VALUES (?, ?, ?)")
                .map_err(|e| e.to_string())?;
            let mut clear = transaction
                .prepare_cached("DELETE FROM items WHERE path = ?")
                .map_err(|e| e.to_string())?;
            let mut item = transaction
                .prepare_cached(
                    "INSERT OR REPLACE INTO items (path, line, id, kind, state, title, tags, \
                     projects, priority, due, scheduled, heading) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(|e| e.to_string())?;
            for note in notes {
                file.execute(params![note.path, note.mtime, note.size])
                    .map_err(|e| e.to_string())?;
                clear.execute([&note.path]).map_err(|e| e.to_string())?;
                for entry in &note.items {
                    item.execute(params![
                        entry.path,
                        entry.line as i64,
                        entry.id as i64,
                        entry.kind,
                        entry.state,
                        entry.title,
                        entry.tags.join(" "),
                        entry.projects.join(" "),
                        entry.priority,
                        entry.due,
                        entry.scheduled,
                        entry.heading,
                    ])
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        transaction.commit().map_err(|e| e.to_string())
    }

    /// Forgets notes that no longer exist.
    pub(crate) fn remove(&mut self, paths: &[String]) -> Result<(), String> {
        let transaction = self.db.transaction().map_err(|e| e.to_string())?;
        for path in paths {
            transaction
                .execute("DELETE FROM items WHERE path = ?", [path])
                .map_err(|e| e.to_string())?;
            transaction
                .execute("DELETE FROM files WHERE path = ?", [path])
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())
    }

    pub(crate) fn clear(&mut self) -> Result<(), String> {
        self.db
            .execute_batch("DELETE FROM items; DELETE FROM files;")
            .map_err(|e| e.to_string())
    }

    /// Tasks not done yet.
    pub(crate) fn open_tasks(&self) -> Result<Vec<Item>, String> {
        let mut statement = self
            .db
            .prepare(
                "SELECT path, line, id, state, title, tags, projects, priority, due, scheduled, \
                 heading FROM items WHERE kind = 'task' AND state != 'done'",
            )
            .map_err(|e| e.to_string())?;
        let words = |text: String| -> Vec<String> {
            text.split_whitespace().map(str::to_string).collect()
        };
        let rows = statement
            .query_map([], |row| {
                Ok(Item {
                    path: row.get(0)?,
                    line: row.get::<_, i64>(1)? as usize,
                    id: row.get::<_, i64>(2)? as u64,
                    kind: "task",
                    state: if row.get::<_, String>(3)? == "doing" {
                        "doing"
                    } else {
                        "todo"
                    },
                    title: row.get(4)?,
                    tags: words(row.get(5)?),
                    projects: words(row.get(6)?),
                    priority: row.get(7)?,
                    due: row.get(8)?,
                    scheduled: row.get(9)?,
                    heading: row.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }
}
