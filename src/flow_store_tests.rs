use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "slate-store-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn db(&self) -> PathBuf {
        self.0.join("agenda.db")
    }
    fn doc(&self, name: &str, text: &str) -> Document {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        let tasks = crate::flow::parse_flow_tasks(text);
        let known_file = crate::flow::build_known_file(&self.0, &path, text, &tasks);
        Document {
            known_file,
            tasks,
            contents: text.to_string(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn asynchronous_exports_converge_to_latest_committed_metadata() {
    let f = Fixture::new();
    let mut writer = CacheWriter::open(&f.db(), Duration::from_millis(25)).unwrap();
    for count in 1..=12 {
        let text = (0..count)
            .map(|n| format!("- [ ] task {n}\n"))
            .collect::<String>();
        writer.write_batch(&[f.doc("live.md", &text)]).unwrap();
        crate::index_export::request(&f.0, &f.db());
    }
    crate::index_export::flush();
    let index = fs::read_to_string(f.0.join(".slate/index.tsv")).unwrap();
    let row = index
        .lines()
        .find(|line| line.starts_with("live.md\t"))
        .unwrap();
    assert_eq!(row.split('\t').nth(3), Some("12"));
}

#[test]
fn embedded_rows_preserve_quotes_unicode_tabs_and_nulls() {
    let f = Fixture::new();
    let mut writer = CacheWriter::open(&f.db(), Duration::from_millis(25)).unwrap();
    let mut doc = f.doc("quotes '雪'.md", "- [ ] O'Brien\t雪\n");
    doc.tasks[0].title = "O'Brien\t雪\nsecond line\0tail".into();
    doc.tasks[0].heading = Some("heading\twith\nnewlines".into());
    let expected = doc.tasks[0].title.clone();
    writer.write_batch(&[doc]).unwrap();
    let rows = read_tasks(&f.db()).unwrap();
    assert_eq!(rows[0].title, expected);
    assert_eq!(rows[0].heading, "heading\twith\nnewlines");
    assert_eq!(rows[0].priority, "");
    assert!(rows[0].source_snippet.contains('\t'));
    let agenda = crate::agenda::load_items_from_cache(&f.db(), &f.0).unwrap();
    assert_eq!(agenda[0].title, expected);
}

#[test]
fn legacy_cache_reads_and_migrates_in_place_without_losing_rows() {
    let f = Fixture::new();
    fs::write(f.0.join("old.md"), "- [ ] original\n").unwrap();
    {
        let legacy = Connection::open(f.db()).unwrap();
        legacy
            .execute_batch(&SCHEMA.replace("kind TEXT NOT NULL DEFAULT 'task',", ""))
            .unwrap();
        legacy.execute_batch("INSERT INTO tasks (path,line_number,line_index,state,title,raw_text,tags,projects,priority,due,scheduled,heading,related,notes,source_snippet) VALUES ('old.md',1,0,'todo','original','original','','',NULL,NULL,NULL,NULL,'','','original'); CREATE TABLE sentinel (value TEXT); INSERT INTO sentinel VALUES ('keep me');").unwrap();
    }
    assert_eq!(read_tasks(&f.db()).unwrap()[0].kind, "task");
    let mut writer = CacheWriter::open(&f.db(), Duration::from_millis(25)).unwrap();
    assert!(has_kind(&writer.connection).unwrap());
    writer
        .write_batch(&[f.doc("new.md", ":idea new idea\n")])
        .unwrap();
    let rows = read_tasks(&f.db()).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .any(|r| r.title == "original" && r.kind == "task")
    );
    assert!(
        rows.iter()
            .any(|r| r.title == "new idea" && r.kind == "idea")
    );
    let sentinel: String = writer
        .connection
        .query_row("SELECT value FROM sentinel", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sentinel, "keep me");
    writer.clear().unwrap();
    assert!(read_tasks(&f.db()).unwrap().is_empty());
    assert!(has_kind(&writer.connection).unwrap());
    assert_eq!(
        writer
            .connection
            .query_row("SELECT value FROM sentinel", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep me"
    );
}

#[test]
fn failed_batch_rolls_back_file_and_task_changes_together() {
    let f = Fixture::new();
    let mut writer = CacheWriter::open(&f.db(), Duration::from_millis(25)).unwrap();
    writer
        .write_batch(&[f.doc("keep.md", "- [ ] original\n")])
        .unwrap();
    writer.connection.execute_batch("CREATE TRIGGER fail_task BEFORE INSERT ON tasks WHEN NEW.title='fail' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let result = writer.write_batch(&[
        f.doc("keep.md", "- [ ] replacement\n"),
        f.doc("bad.md", "- [ ] fail\n"),
    ]);
    assert!(result.unwrap_err().contains("injected failure"));
    let rows = read_tasks(&f.db()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "original");
    assert_eq!(writer.known_files().unwrap().len(), 1);
}

#[test]
fn wal_readers_see_committed_data_and_ui_lock_wait_is_bounded() {
    let f = Fixture::new();
    let mut writer = CacheWriter::open(&f.db(), Duration::from_millis(25)).unwrap();
    writer
        .write_batch(&[f.doc("notes.md", "- [ ] committed\n")])
        .unwrap();
    let lock = Connection::open(f.db()).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE; UPDATE tasks SET title='uncommitted';")
        .unwrap();
    assert_eq!(read_tasks(&f.db()).unwrap()[0].title, "committed");
    let started = std::time::Instant::now();
    let err = writer
        .write_batch(&[f.doc("notes.md", "- [ ] next\n")])
        .unwrap_err();
    assert!(err.contains("locked"), "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "UI must not wait for a multi-second SQLite timeout"
    );
    lock.execute_batch("ROLLBACK;").unwrap();
    writer
        .write_batch(&[f.doc("notes.md", "- [ ] next\n")])
        .unwrap();
    assert_eq!(read_tasks(&f.db()).unwrap()[0].title, "next");
}

#[test]
fn bounded_scan_preserves_unsaved_live_buffers_and_exports_once() {
    let f = Fixture::new();
    let mut files = Vec::new();
    for index in 0..70 {
        files.push(
            f.doc(&format!("note-{index}.md"), "- [ ] on disk\n")
                .known_file
                .path,
        );
    }
    let paths: Vec<_> = files.iter().map(|p| f.0.join(p)).collect();
    {
        let mut writer = CacheWriter::open(&f.db(), Duration::from_millis(25)).unwrap();
        let mut live = f.doc("note-0.md", "- [ ] on disk\n");
        live.tasks[0].title = "unsaved editor task".into();
        writer.write_batch(&[live]).unwrap();
    }
    crate::flow_runtime::publish_active_buffer_heartbeat(&f.db(), &f.0, &paths[0]).unwrap();
    let summary = crate::flow_batch::index_files(&f.0, &f.db(), &paths, &[]).unwrap();
    assert_eq!(summary.errors, 0);
    assert_eq!(summary.indexed_files, 69);
    assert_eq!(summary.skipped_live, 1);
    assert_eq!(summary.batches, 3);
    assert_eq!(read_tasks(&f.db()).unwrap().len(), 70);
    assert!(
        read_tasks(&f.db())
            .unwrap()
            .iter()
            .any(|r| r.path == "note-0.md" && r.title == "unsaved editor task")
    );
    let exported = fs::read_to_string(f.0.join(".slate/index.tsv")).unwrap();
    assert_eq!(
        exported
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .count(),
        70
    );
    let rebuilt =
        crate::flow_batch::index_files_with_rebuild(&f.0, &f.db(), &paths, &[], true).unwrap();
    assert_eq!(rebuilt.errors, 0);
    assert_eq!(rebuilt.skipped_live, 1);
    assert_eq!(read_tasks(&f.db()).unwrap().len(), 70);
    assert!(
        read_tasks(&f.db())
            .unwrap()
            .iter()
            .any(|r| r.path == "note-0.md" && r.title == "unsaved editor task")
    );
}
