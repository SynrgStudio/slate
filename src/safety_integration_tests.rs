#![allow(deprecated)]
use super::*;
struct Fixture {
    app: SlateApp,
    ctx: egui::Context,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, None);
        let root = std::env::temp_dir().join(format!(
            "slate-safety-ui-{}-{}",
            std::process::id(),
            crate::test_unique()
        ));
        fs::create_dir_all(&root).unwrap();
        app.settings_path_override = Some(root.join("config.toml"));
        Self { app, ctx, root }
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.document_safety.flush();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn save_conflict_keeps_both_disk_and_dirty_editor_and_save_copy_succeeds() {
    let mut f = Fixture::new();
    let path = f.file("file.txt", "original");
    f.app.path = Some(path.clone());
    f.app.disk_base = Some(safe_file::fingerprint(b"original"));
    f.app.buffer.set_text("my changes".into());
    f.app.dirty = true;
    fs::write(&path, "external changes").unwrap();
    f.app.save_path(path.clone());
    assert!(f.app.dirty);
    assert!(f.app.file_conflict.is_some());
    assert_eq!(f.app.buffer.as_str(), "my changes");
    assert_eq!(fs::read_to_string(&path).unwrap(), "external changes");
    let copy = f.root.join("copy.txt");
    f.app.save_path(copy.clone());
    assert!(!f.app.dirty);
    assert_eq!(f.app.path, Some(copy.clone()));
    assert_eq!(fs::read_to_string(copy).unwrap(), "my changes");
    assert_eq!(fs::read_to_string(path).unwrap(), "external changes");
}
#[test]
fn modal_save_and_promotion_do_not_overwrite_other_dirty_text() {
    let mut f = Fixture::new();
    let path = f.file("modal.txt", "base");
    f.app.open_link_preview(path.clone(), None);
    f.app.link_preview_buffer.set_text("modal edit".into());
    f.app.link_preview_dirty = true;
    fs::write(&path, "outside").unwrap();
    f.app.save_link_preview();
    assert!(f.app.link_preview_dirty);
    assert_eq!(fs::read_to_string(&path).unwrap(), "outside");
    f.app.buffer.set_text("main edit".into());
    f.app.dirty = true;
    f.app.commit_link_preview();
    assert_eq!(f.app.buffer.as_str(), "main edit");
    assert_eq!(f.app.link_preview_buffer.as_str(), "modal edit");
    assert!(f.app.link_preview_dirty);
}
#[test]
fn recovery_restores_dirty_text_without_writing_source_and_refuses_dirty_replacement() {
    let mut f = Fixture::new();
    let source = f.file("source.txt", "disk text");
    let record = f.root.join("record.json");
    fs::write(&record, b"placeholder").unwrap();
    f.app.recovery_records.push(document_safety::Recovered {
        record: record.clone(),
        draft: document_safety::Draft {
            version: 1,
            slot: document_safety::Slot::Main,
            path: Some(source.clone()),
            base: Some(safe_file::fingerprint(b"disk text")),
            text: "recovered 雪".into(),
            cursor: 3,
            saved_ms: 1,
        },
    });
    f.app.dirty = true;
    f.app.buffer.set_text("current unsaved".into());
    f.app.restore_selected_recovery();
    assert_eq!(f.app.buffer.as_str(), "current unsaved");
    f.app.dirty = false;
    f.app.restore_selected_recovery();
    assert!(f.app.dirty);
    assert_eq!(f.app.buffer.as_str(), "recovered 雪");
    assert_eq!(fs::read_to_string(source).unwrap(), "disk text");
    assert!(record.exists());
}
#[test]
fn dirty_modal_requires_quit_confirmation_even_when_main_is_clean() {
    let mut f = Fixture::new();
    f.app.scratch = false;
    f.app.link_preview_dirty = true;
    f.app.request_close(&f.ctx);
    assert!(matches!(f.app.pending_action, Some(PendingAction::Quit)));
}
#[test]
fn snapshots_include_main_modal_and_scratch_and_clean_main_does_not_drop_modal() {
    let mut f = Fixture::new();
    let recovery = f.root.join("recovery");
    f.app.document_safety = document_safety::Manager::new(f.ctx.clone(), Some(recovery.clone()));
    f.app.buffer.set_text("main".into());
    f.app.dirty = true;
    f.app.link_preview_buffer.set_text("modal".into());
    f.app.link_preview_dirty = true;
    f.app.scratch_buffer.set_text("scratch".into());
    f.app.observe_document_safety(0.0, true);
    f.app.document_safety.flush();
    let session = fs::read_dir(&recovery)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    for file in ["main.json", "modal.json", "scratch.json"] {
        assert!(session.join(file).exists());
    }
    f.app.dirty = false;
    f.app.observe_document_safety(1.0, true);
    f.app.document_safety.flush();
    assert!(!session.join("main.json").exists());
    assert!(session.join("modal.json").exists());
    assert!(session.join("scratch.json").exists());
}
#[test]
fn swapping_keeps_each_checkpoint_identity_and_cleaning_one_keeps_the_other() {
    let mut f = Fixture::new();
    let root = f.root.join("recovery");
    f.app.document_safety = document_safety::Manager::new(f.ctx.clone(), Some(root.clone()));
    f.app.buffer.set_text("main unsaved".into());
    f.app.dirty = true;
    f.app.path = Some(f.file("main.txt", "main base"));
    f.app.link_preview_buffer.set_text("modal unsaved".into());
    f.app.link_preview_dirty = true;
    f.app.link_preview_path = Some(f.file("modal.txt", "modal base"));
    f.app.observe_document_safety(0.0, true);
    f.app.document_safety.flush();
    let session = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    f.app.swap_link_preview_with_main();
    f.app.observe_document_safety(1.0, true);
    f.app.document_safety.flush();
    let read = |name: &str| {
        serde_json::from_slice::<document_safety::Draft>(&fs::read(session.join(name)).unwrap())
            .unwrap()
            .text
    };
    assert_eq!(read("main.json"), "main unsaved");
    assert_eq!(read("modal.json"), "modal unsaved");
    assert_eq!(f.app.buffer.as_str(), "modal unsaved");
    f.app.dirty = false;
    f.app.observe_document_safety(2.0, true);
    f.app.document_safety.flush();
    assert!(!session.join("modal.json").exists());
    assert_eq!(read("main.json"), "main unsaved");
}
#[test]
fn promoting_modal_does_not_delete_its_checkpoint_before_main_is_written() {
    let mut f = Fixture::new();
    let root = f.root.join("recovery");
    f.app.document_safety = document_safety::Manager::new(f.ctx.clone(), Some(root.clone()));
    let source = f.file("modal.txt", "disk");
    f.app.open_link_preview(source.clone(), None);
    f.app
        .link_preview_buffer
        .set_text("critical modal text".into());
    f.app.link_preview_dirty = true;
    f.app.observe_document_safety(0.0, true);
    f.app.document_safety.flush();
    let session = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    f.app.commit_link_preview();
    f.app.document_safety.flush();
    assert!(session.join("modal.json").exists());
    assert_eq!(f.app.buffer.as_str(), "critical modal text");
    assert!(f.app.dirty);
    f.app.observe_document_safety(1.0, true);
    f.app.document_safety.flush();
    assert!(session.join("modal.json").exists());
    assert_eq!(fs::read_to_string(source).unwrap(), "disk");
}
fn notice_key(f: &mut Fixture, key: Key) {
    let _ = f.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            events: vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| f.app.document_safety_dialog(ctx),
    );
}
#[test]
fn saving_copy_of_clean_modal_uses_visible_modal_text_not_main_or_new_disk_text() {
    let mut f = Fixture::new();
    let path = f.file("modal.txt", "visible modal text");
    f.app.buffer.set_text("main text".into());
    f.app.open_link_preview(path.clone(), None);
    fs::write(&path, "new external text").unwrap();
    f.app.file_conflict = Some((document_safety::Slot::Modal, path.clone()));
    notice_key(&mut f, Key::C);
    assert_eq!(f.app.buffer.as_str(), "visible modal text");
    assert!(f.app.dirty);
    assert!(f.app.save_as_open);
    assert!(!f.app.link_preview_open);
    assert_eq!(fs::read_to_string(path).unwrap(), "new external text");
}
#[test]
fn reload_of_dirty_main_keeps_the_unsaved_text_one_undo_away() {
    let mut f = Fixture::new();
    let path = f.file("main.txt", "external text");
    f.app.path = Some(path.clone());
    f.app.buffer.set_text("my unsaved text".into());
    f.app.dirty = true;
    f.app.file_conflict = Some((document_safety::Slot::Main, path));
    notice_key(&mut f, Key::R);
    assert!(!f.app.dirty);
    assert!(f.app.file_conflict.is_none());
    assert_eq!(f.app.buffer.as_str(), "external text");
    assert!(f.app.status.contains("Ctrl+Z brings back your unsaved text"));
    let reload = f.app.buffer.history().current_node();
    assert_eq!(reload.kind, crate::history::Kind::External);
    assert!(f.app.buffer.undo());
    assert_eq!(f.app.buffer.as_str(), "my unsaved text");
}
