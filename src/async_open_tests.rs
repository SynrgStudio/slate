use super::*;
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

struct Fixture {
    app: SlateApp,
    ctx: egui::Context,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, false, None);
        let root = std::env::temp_dir().join(format!(
            "slate-async-open-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        let original = root.join("original.cmd");
        fs::write(&original, "original buffer\n").unwrap();
        app.buffer.set_text("original buffer\n".into());
        app.path = Some(original);
        app.dirty = false;
        app.settings_path_override = Some(root.join("test-config.toml"));
        Self { app, ctx, root }
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, text).unwrap();
        path
    }
    fn until(&mut self, condition: impl Fn(&SlateApp) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition(&self.app) {
            let _ = self.ctx.run(egui::RawInput::default(), |ctx| {
                self.app.poll_file_open(ctx)
            });
            assert!(Instant::now() < deadline, "timed out: {}", self.app.status);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn gated_reader(&mut self) -> (Gate, mpsc::Receiver<String>, Arc<Mutex<Vec<String>>>) {
        let gate = Gate(Arc::new((Mutex::new(false), Condvar::new())));
        let shared = gate.0.clone();
        let (started, receive) = mpsc::channel();
        let reads = Arc::new(Mutex::new(Vec::new()));
        let recorded = reads.clone();
        self.app.file_loader =
            file_loader::Loader::with_reader(self.ctx.clone(), move |path, _| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                recorded.lock().unwrap().push(name.clone());
                started.send(name.clone()).unwrap();
                if name.starts_with("slow") {
                    let (lock, wake) = &*shared;
                    let mut released = lock.lock().unwrap();
                    while !*released {
                        released = wake.wait(released).unwrap();
                    }
                }
                fs::read_to_string(path)
                    .map(EditorBuffer::from_text)
                    .map_err(|e| e.to_string())
            });
        (gate, receive, reads)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.cancel_file_open();
        if let Some(rx) = self.app.flow_background_rx.take() {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
        index_export::flush();
        if let Ok(cache) = SlateApp::flow_sqlite_path_for_root(&self.root) {
            let _ = fs::remove_dir_all(cache.parent().unwrap());
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Gate(Arc<(Mutex<bool>, Condvar)>);
impl Gate {
    fn release(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.release();
    }
}

#[test]
fn slow_read_keeps_old_document_visible_and_ui_can_build_frames() {
    let mut f = Fixture::new();
    let path = f.file("slow.cmd", "new buffer\n");
    let (gate, started, _) = f.gated_reader();
    f.app.open_path(path.clone());
    assert_eq!(
        started.recv_timeout(Duration::from_secs(2)).unwrap(),
        "slow.cmd"
    );
    assert_eq!(f.app.buffer.as_str(), "original buffer\n");
    let mut frame = eframe::Frame::_new_kittest();
    for n in 0..3 {
        let output = f.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                time: Some(n as f64 * 0.016),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| eframe::App::ui(&mut f.app, ui, &mut frame));
            },
        );
        assert!(!output.shapes.is_empty());
        assert_eq!(f.app.buffer.as_str(), "original buffer\n");
        assert!(f.app.pending_file_open.is_some());
    }
    gate.release();
    f.until(|app| app.path.as_ref() == Some(&path));
    assert_eq!(f.app.buffer.as_str(), "new buffer\n");
    assert!(!f.app.dirty);
}

#[test]
fn picker_enter_starts_loading_without_editing_the_visible_old_buffer() {
    let mut f = Fixture::new();
    let path = f.file("slow.cmd", "opened via picker\n");
    let (gate, started, _) = f.gated_reader();
    f.app.file_picker_open = true;
    f.app.file_picker_mode = FilePickerMode::Open;
    f.app.file_picker_dir = f.root.clone();
    f.app.project_files = vec![path.clone()];
    f.app.selected_project_file = 0;
    f.app.file_query.clear();
    let mut frame = eframe::Frame::_new_kittest();
    let _ = f.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events: vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| eframe::App::ui(&mut f.app, ui, &mut frame));
        },
    );
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(!f.app.file_picker_open);
    assert!(f.app.pending_file_open.is_some());
    assert!(!f.app.dirty);
    assert_eq!(f.app.buffer.as_str(), "original buffer\n");
    gate.release();
    f.until(|app| app.path.as_ref() == Some(&path));
    assert_eq!(f.app.buffer.as_str(), "opened via picker\n");
}

#[test]
fn later_fast_open_wins_even_if_older_read_finishes_last() {
    let mut f = Fixture::new();
    let slow = f.file("slow.cmd", "old result\n");
    let fast = f.file("fast.cmd", "latest result\n");
    let (gate, started, _) = f.gated_reader();
    f.app.open_path(slow);
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    f.app.open_path(fast.clone());
    f.until(|app| app.path.as_ref() == Some(&fast));
    let status = f.app.status.clone();
    gate.release();
    // Confirm both workers have left the reader before draining the stale result.
    std::thread::sleep(Duration::from_millis(20));
    f.app.poll_file_open(&f.ctx);
    assert_eq!(f.app.path, Some(fast));
    assert_eq!(f.app.buffer.as_str(), "latest result\n");
    assert_eq!(f.app.status, status);
}

#[test]
fn queued_opens_coalesce_and_at_most_two_reads_run() {
    let mut f = Fixture::new();
    let slow1 = f.file("slow-one.cmd", "one");
    let slow2 = f.file("slow-two.cmd", "two");
    let skipped = f.file("skipped.cmd", "must not be read");
    let latest = f.file("latest.cmd", "latest");
    let (gate, started, reads) = f.gated_reader();
    f.app.open_path(slow1);
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    f.app.open_path(slow2);
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    f.app.open_path(skipped);
    f.app.open_path(latest.clone());
    assert_eq!(reads.lock().unwrap().len(), 2);
    gate.release();
    f.until(|app| app.path.as_ref() == Some(&latest));
    assert_eq!(reads.lock().unwrap().len(), 3);
    assert!(
        !reads
            .lock()
            .unwrap()
            .iter()
            .any(|name| name == "skipped.cmd")
    );
}

#[test]
fn edits_during_loading_are_preserved_even_after_dirty_is_cleared() {
    for dirty in [true, false] {
        let mut f = Fixture::new();
        let path = f.file("slow.cmd", "incoming\n");
        let original_path = f.app.path.clone();
        let (gate, started, _) = f.gated_reader();
        f.app.open_path(path);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        f.app.buffer.set_text("user edits\n".into());
        f.app.dirty = dirty; // False models saving or undoing while a read is pending.
        f.app.poll_file_open(&f.ctx);
        assert!(f.app.pending_file_open.is_none());
        gate.release();
        std::thread::sleep(Duration::from_millis(20));
        f.app.poll_file_open(&f.ctx);
        assert_eq!(f.app.path, original_path);
        assert_eq!(f.app.buffer.as_str(), "user edits\n");
        assert_eq!(f.app.dirty, dirty);
    }
}

#[test]
fn same_frame_text_input_is_not_retargeted_to_loaded_document() {
    let mut f = Fixture::new();
    let incoming = f.file("incoming.cmd", "incoming");
    f.app.open_path(incoming);
    let _ = f.ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Text("typed".into())],
            ..Default::default()
        },
        |ctx| {
            f.app.poll_file_open(ctx);
            assert_eq!(f.app.buffer.as_str(), "original buffer\n");
            f.app.buffer.set_text("original buffer plus typed".into());
            f.app.dirty = true;
        },
    );
    f.app.poll_file_open(&f.ctx);
    assert!(f.app.pending_file_open.is_none());
    assert_eq!(f.app.buffer.as_str(), "original buffer plus typed");
}

#[test]
fn new_buffer_and_escape_cancel_without_late_replacement() {
    for new_buffer in [false, true] {
        let mut f = Fixture::new();
        let path = f.file("slow.cmd", "incoming");
        let (gate, started, _) = f.gated_reader();
        f.app.open_path(path);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        if new_buffer {
            f.app.new_buffer();
        } else {
            let _ = f.ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| f.app.shortcuts(ctx),
            );
        }
        assert!(f.app.pending_file_open.is_none());
        let before = f.app.buffer.as_str().to_string();
        gate.release();
        std::thread::sleep(Duration::from_millis(20));
        f.app.poll_file_open(&f.ctx);
        assert_eq!(f.app.buffer.as_str(), before);
    }
}

#[test]
fn failed_or_invalid_utf8_open_does_not_change_document_or_recents() {
    let mut f = Fixture::new();
    let path = f.root.join("invalid.cmd");
    fs::write(&path, [0xff, 0xfe]).unwrap();
    let original = f.app.path.clone();
    let recents = f.app.recent_files.clone();
    for path in [path, f.root.join("missing.cmd")] {
        f.app.open_path(path);
        f.until(|app| app.pending_file_open.is_none());
        assert!(f.app.status.starts_with("Open failed:"));
        assert_eq!(f.app.path, original);
        assert_eq!(f.app.buffer.as_str(), "original buffer\n");
        assert_eq!(f.app.recent_files, recents);
    }
    assert!(!f.root.join("test-config.toml").exists());
}

#[test]
fn constructor_defers_startup_file_and_line_until_read_finishes() {
    let mut f = Fixture::new();
    let path = f.file("startup.cmd", "one\ntwo\nthree\nfour\n");
    let cc = eframe::CreationContext::_new_kittest(f.ctx.clone());
    f.app = SlateApp::new(&cc, Some(path.clone()), true, false, Some(3));
    f.app.settings_path_override = Some(f.root.join("test-config.toml"));
    assert!(f.app.path.is_none());
    assert_eq!(f.app.buffer.as_str(), "");
    f.until(|app| app.path.as_ref() == Some(&path));
    assert_eq!(f.app.buffer.cursor_line_col().0, 2);
}

#[test]
fn reopening_reads_external_changes_instead_of_serving_stale_text() {
    let mut f = Fixture::new();
    let path = f.file("reload.cmd", "version one");
    f.app.open_path(path.clone());
    f.until(|app| app.pending_file_open.is_none());
    assert_eq!(f.app.buffer.as_str(), "version one");
    fs::write(&path, "version two").unwrap();
    f.app.open_path(path);
    f.until(|app| app.pending_file_open.is_none());
    assert_eq!(f.app.buffer.as_str(), "version two");
}

#[test]
fn direct_open_refuses_to_discard_an_already_dirty_buffer() {
    let mut f = Fixture::new();
    let path = f.file("incoming.cmd", "incoming");
    f.app.dirty = true;
    f.app.open_path(path);
    assert!(f.app.pending_file_open.is_none());
    assert!(f.app.dirty);
    assert_eq!(f.app.buffer.as_str(), "original buffer\n");
}

#[test]
fn startup_line_and_navigation_destinations_apply_after_loading() {
    let mut f = Fixture::new();
    let path = f.file("destination.cmd", "# Log\n\n## 2026-10-10\ntext\n");
    let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
    for destination in [
        OpenDestination::Line(3),
        OpenDestination::Fragment("L3".into()),
        OpenDestination::Daily {
            date,
            refreshing: false,
        },
        OpenDestination::Agenda(2),
    ] {
        f.app.request_file_open(path.clone(), destination);
        assert!(f.app.pending_file_open.is_some());
        f.until(|app| app.pending_file_open.is_none());
        assert_eq!(f.app.path.as_ref(), Some(&path));
        assert_eq!(f.app.buffer.cursor_line_col().0, 2);
    }
}
