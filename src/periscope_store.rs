//! Separate metadata-only SQLite index. No Flow tables or document contents.
use rusqlite::{Connection, params};
use std::{
    collections::BinaryHeap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
pub(crate) fn normalize(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
        .trim_end_matches('/')
        .to_string()
}
fn like_prefix(path: &str) -> String {
    format!(
        "{}/%",
        path.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}
pub(crate) struct Entry {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
}
pub(crate) struct Store {
    db: Connection,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let db = Connection::open(path).map_err(|error| error.to_string())?;
        db.busy_timeout(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS paths(root TEXT NOT NULL,path TEXT NOT NULL,normalized TEXT NOT NULL,name TEXT NOT NULL,parent TEXT NOT NULL,size INTEGER NOT NULL,modified INTEGER NOT NULL,generation INTEGER NOT NULL,PRIMARY KEY(root,normalized)); CREATE INDEX IF NOT EXISTS paths_scope ON paths(normalized); CREATE INDEX IF NOT EXISTS paths_parent ON paths(root,parent); CREATE TABLE IF NOT EXISTS scans(root TEXT PRIMARY KEY,finished INTEGER NOT NULL,errors INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS sequence(slot INTEGER PRIMARY KEY,value INTEGER NOT NULL); INSERT OR IGNORE INTO sequence VALUES(1,0); CREATE TABLE IF NOT EXISTS preparations(scope TEXT PRIMARY KEY,finished INTEGER NOT NULL,errors INTEGER NOT NULL);").map_err(|e|e.to_string())?;
        Ok(Self { db })
    }
    fn scope_key(roots: &[PathBuf]) -> String {
        let mut roots = roots.iter().map(|root| normalize(root)).collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        serde_json::to_string(&roots).unwrap()
    }
    pub fn prepared(path: &Path, roots: &[PathBuf]) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        db.busy_timeout(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='preparations')",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Ok(false);
        }
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM preparations WHERE scope=?)",
            [Self::scope_key(roots)],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
    }
    pub fn inventory_count(&self, root: &Path, generation: i64) -> Result<usize, String> {
        self.db
            .query_row(
                "SELECT COUNT(*) FROM paths WHERE root=? AND generation=?",
                params![normalize(root), generation],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }
    pub fn inventory_finished(&mut self, root: &Path) -> Result<(), String> {
        // Everything exclusions are not proof that an older filesystem entry was deleted.
        self.db.execute("INSERT INTO scans VALUES(?,?,0) ON CONFLICT(root) DO UPDATE SET finished=excluded.finished,errors=0",params![normalize(root),chrono::Utc::now().timestamp()]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn mark_prepared(&mut self, roots: &[PathBuf], errors: usize) -> Result<(), String> {
        self.db.execute("INSERT INTO preparations VALUES(?,?,?) ON CONFLICT(scope) DO UPDATE SET finished=excluded.finished,errors=excluded.errors",params![Self::scope_key(roots),chrono::Utc::now().timestamp(),errors]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn next_generation(&mut self) -> Result<i64, String> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute("UPDATE sequence SET value=value+1 WHERE slot=1", [])
            .map_err(|e| e.to_string())?;
        let value = tx
            .query_row("SELECT value FROM sequence WHERE slot=1", [], |row| {
                row.get(0)
            })
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(value)
    }
    pub fn write(&mut self, root: &Path, generation: i64, entries: &[Entry]) -> Result<(), String> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        {
            let mut statement=tx.prepare_cached("INSERT INTO paths VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(root,normalized) DO UPDATE SET path=excluded.path,name=excluded.name,parent=excluded.parent,size=excluded.size,modified=excluded.modified,generation=excluded.generation WHERE paths.generation<=excluded.generation").map_err(|e|e.to_string())?;
            let root = normalize(root);
            for entry in entries {
                statement
                    .execute(params![
                        root,
                        entry.path.to_string_lossy(),
                        normalize(&entry.path),
                        entry
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase(),
                        entry.path.parent().map(normalize).unwrap_or_default(),
                        entry.size as i64,
                        entry.modified,
                        generation
                    ])
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn finish_directory(
        &mut self,
        root: &Path,
        parent: &Path,
        generation: i64,
    ) -> Result<(), String> {
        self.db
            .execute(
                "DELETE FROM paths WHERE root=? AND parent=? AND generation<?",
                params![normalize(root), normalize(parent), generation],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn finish(&mut self, root: &Path, generation: i64, errors: usize) -> Result<(), String> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        // Never purge an incomplete/inaccessible volume. Directory-level successful scans
        // still retire missing files; stale inaccessible entries are retained.
        if errors == 0 {
            tx.execute(
                "DELETE FROM paths WHERE root=? AND generation<?",
                params![normalize(root), generation],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute("INSERT INTO scans VALUES(?,?,?) ON CONFLICT(root) DO UPDATE SET finished=excluded.finished,errors=excluded.errors",params![normalize(root),chrono::Utc::now().timestamp(),errors]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn search(
        &self,
        roots: &[PathBuf],
        query: &str,
        latest: &AtomicU64,
        id: u64,
    ) -> Result<(Vec<PathBuf>, usize), String> {
        let query = query.trim().to_lowercase().replace('\\', "/");
        let mut heap: BinaryHeap<(usize, String)> = BinaryHeap::new();
        let mut examined = 0;
        let mut sql = String::from("SELECT path,normalized,name FROM paths WHERE ");
        let mut values = Vec::new();
        for (index, root) in roots.iter().enumerate() {
            if index > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str("(normalized=? OR normalized LIKE ? ESCAPE '\\')");
            let value = normalize(root);
            values.push(value.clone());
            values.push(like_prefix(&value));
        }
        if roots.is_empty() {
            return Ok((Vec::new(), 0));
        }
        sql.push_str(" GROUP BY normalized ORDER BY normalized");
        let mut statement = self.db.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = statement
            .query(rusqlite::params_from_iter(values))
            .map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if examined % 256 == 0 && latest.load(Ordering::Relaxed) != id {
                return Err("cancelled".into());
            }
            examined += 1;
            let path: String = row.get(0).map_err(|e| e.to_string())?;
            let lower: String = row.get(1).map_err(|e| e.to_string())?;
            let name: String = row.get(2).map_err(|e| e.to_string())?;
            let score = if query.is_empty() {
                Some(0)
            } else {
                crate::periscope::path_score_lower(&lower, &name, &query)
            };
            if let Some(score) = score {
                let candidate = (score, path);
                if heap.len() < crate::PERISCOPE_RESULT_LIMIT {
                    heap.push(candidate);
                } else if heap.peek().is_some_and(|value| candidate < *value) {
                    heap.pop();
                    heap.push(candidate);
                }
            }
        }
        let mut best = heap.into_vec();
        best.sort();
        Ok((
            best.into_iter()
                .map(|(_, path)| PathBuf::from(path))
                .collect(),
            examined,
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn overlapping_scans_do_not_overwrite_or_purge_newer_metadata() {
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "slate-periscope-generations-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        )));
        let path = fixture.0.join("index.sqlite3");
        let mut older = Store::open(&path).unwrap();
        let mut newer = Store::open(&path).unwrap();
        let a = older.next_generation().unwrap();
        let b = newer.next_generation().unwrap();
        assert!(b > a);
        let root = fixture.0.join("scope");
        let entry = || Entry {
            path: root.join("new.md"),
            size: 7,
            modified: 1,
        };
        newer.write(&root, b, &[entry()]).unwrap();
        older.write(&root, a, &[entry()]).unwrap();
        older.finish_directory(&root, &root, a).unwrap();
        older.finish(&root, a, 0).unwrap();
        assert_eq!(
            newer
                .db
                .query_row("SELECT generation FROM paths", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            b
        );
    }
    #[test]
    fn fallback_index_has_no_twenty_thousand_file_cutoff_and_literal_scope_wildcards() {
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "slate-periscope-size-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        )));
        let mut store = Store::open(&fixture.0.join("index.sqlite3")).unwrap();
        let root = fixture.0.join("notes_100%");
        let entries = (0..20_010)
            .map(|index| Entry {
                path: root.join(format!("file-{index:05}.md")),
                size: 0,
                modified: 0,
            })
            .collect::<Vec<_>>();
        for batch in entries.chunks(256) {
            store.write(&root, 1, batch).unwrap();
        }
        store
            .write(
                &fixture.0.join("notesX100-other"),
                1,
                &[Entry {
                    path: fixture.0.join("notesX100-other/file-20009.md"),
                    size: 0,
                    modified: 0,
                }],
            )
            .unwrap();
        let (paths, examined) = store
            .search(&[root.clone()], "file-20009.md", &AtomicU64::new(1), 1)
            .unwrap();
        assert_eq!(examined, 20_010);
        assert_eq!(paths, vec![root.join("file-20009.md")]);
        assert!(
            store
                .search(&[root], "file", &AtomicU64::new(2), 1)
                .is_err()
        );
    }
    #[test]
    fn unicode_scope_boundaries_persistence_and_incomplete_scan_retention() {
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "slate-periscope-store-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        )));
        let path = fixture.0.join("index.sqlite3");
        let root = fixture.0.join("notes");
        let other = fixture.0.join("notes-other");
        let mut store = Store::open(&path).unwrap();
        for (dir, name) in [(&root, "café 'quoted'.md"), (&other, "café.md")] {
            store
                .write(
                    dir,
                    1,
                    &[Entry {
                        path: dir.join(name),
                        size: 7,
                        modified: 1,
                    }],
                )
                .unwrap();
        }
        let latest = AtomicU64::new(2);
        let (results, _) = store
            .search(std::slice::from_ref(&root), "café", &latest, 2)
            .unwrap();
        assert_eq!(results, vec![root.join("café 'quoted'.md")]);
        store.finish(&root, 2, 1).unwrap();
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .search(std::slice::from_ref(&root), "café", &latest, 2)
                .unwrap()
                .0
                .len(),
            1
        );
        store.finish(&root, 3, 0).unwrap();
        assert!(
            store
                .search(&[root], "café", &latest, 2)
                .unwrap()
                .0
                .is_empty()
        );
        assert_eq!(
            store.search(&[other], "café", &latest, 2).unwrap().0.len(),
            1
        );
    }
}
