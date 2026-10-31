#![allow(deprecated)]
use super::*;
struct Fixture {
    app: SlateApp,
    ctx: egui::Context,
    root: PathBuf,
    time: f64,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "slate-find-ui-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("target.md"),
            "# Alpha\nbody\n## Beta\nmore text\n",
        )
        .unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, false, None);
        app.settings_path_override = Some(root.join("config.toml"));
        app.path = Some(root.join("current.md"));
        app.buffer.set_text("preserve unsaved text".into());
        app.dirty = true;
        app.find_note_open = true;
        app.find_note_rows = (0..100)
            .map(|i| FindNoteRow::File {
                path: root.join(format!(
                    "{}note-{i}.md",
                    "long Unicode ñ directory/".repeat(15)
                )),
                match_hint: "long match description ".repeat(30),
            })
            .collect();
        Self {
            app,
            ctx,
            root,
            time: 0.0,
        }
    }
    fn draw(&mut self, size: Vec2, events: Vec<egui::Event>, keys: bool) {
        self.time += 0.05;
        let _ = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
                if keys {
                    self.app.shortcuts(ctx);
                }
                self.app.find_note_dialog(ctx);
            },
        );
    }
    fn geometry(&self) -> dialog_ui::Geometry {
        self.ctx
            .data(|data| data.get_temp(egui::Id::new("find_note_dialog").with("geometry")))
            .unwrap()
    }
    fn list(&self) -> find_note_ui::Geometry {
        self.ctx
            .data(|data| data.get_temp(egui::Id::new("find_note_list_geometry")))
            .unwrap()
    }
    fn key(&mut self, key: Key, modifiers: egui::Modifiers) {
        self.draw(
            Vec2::new(900.0, 600.0),
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            true,
        );
    }
    fn click_action(&mut self, index: usize) {
        let rect = self.geometry().actions[index];
        for pressed in [true, false] {
            self.draw(
                Vec2::new(900.0, 600.0),
                vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::PointerButton {
                        pos: rect.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                false,
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn find_note_rows_query_and_chrome_fit_resize_and_keep_last_selection_visible() {
    let mut f = Fixture::new();
    f.app.find_note_query = "huge query ñ ".repeat(100);
    f.app.selected_find_note_row = 99;
    for size in [
        Vec2::new(1200.0, 800.0),
        Vec2::new(420.0, 260.0),
        Vec2::new(900.0, 600.0),
    ] {
        for _ in 0..3 {
            f.draw(size, vec![], false);
        }
        let geo = f.geometry();
        let list = f.list();
        assert!((geo.frame.width() - size.x * 0.8).abs() < 1.0);
        assert!((geo.frame.height() - (size.y - 30.0) * 0.8).abs() < 1.0);
        assert!(
            (geo.frame.center() - egui::pos2(size.x * 0.5, size.y * 0.5 + 15.0)).length() < 1.0
        );
        assert!(geo.body.contains_rect(list.input));
        assert!(geo.body.contains_rect(list.list));
        for row in &list.rows {
            assert!(list.list.contains_rect(*row));
        }
        if !list.rows.is_empty() {
            assert!(list.start <= 99 && list.end > 99);
        }
        for rect in &geo.actions {
            assert!(geo.footer.contains_rect(*rect));
        }
        assert_eq!(f.app.buffer.as_str(), "preserve unsaved text");
        assert!(f.app.dirty);
    }
}
#[test]
fn wheel_keyboard_and_click_navigation_leave_chrome_stationary() {
    let mut f = Fixture::new();
    let size = Vec2::new(900.0, 600.0);
    for _ in 0..3 {
        f.draw(size, vec![], false);
    }
    let before = f.geometry();
    let pos = f.list().list.center();
    f.draw(
        size,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: Vec2::new(0.0, -60.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
        false,
    );
    assert_eq!(f.app.selected_find_note_row, 1);
    f.key(Key::ArrowDown, egui::Modifiers::NONE);
    assert_eq!(f.app.selected_find_note_row, 2);
    f.click_action(1);
    assert_eq!(f.app.selected_find_note_row, 1);
    let after = f.geometry();
    assert_eq!(before.header, after.header);
    assert_eq!(before.footer, after.footer);
    assert_eq!(before.frame, after.frame);
    f.key(Key::Escape, egui::Modifiers::NONE);
    assert!(!f.app.find_note_open);
    assert_eq!(f.app.buffer.as_str(), "preserve unsaved text");
    assert!(f.app.dirty);
}
#[test]
fn heading_toggle_filter_and_modal_action_retain_existing_handlers() {
    let mut f = Fixture::new();
    let target = f.root.join("target.md");
    f.app.find_note_files = vec![target.clone()];
    f.app.find_note_rows = vec![FindNoteRow::File {
        path: target.clone(),
        match_hint: String::new(),
    }];
    for _ in 0..3 {
        f.draw(Vec2::new(900.0, 600.0), vec![], false);
    }
    f.click_action(3);
    assert!(f.app.find_note_expanded_files.contains(&target));
    assert!(
        f.app
            .find_note_rows
            .iter()
            .any(|row| matches!(row, FindNoteRow::Heading { line: 3, .. }))
    );
    f.key(Key::Tab, egui::Modifiers::NONE);
    assert!(!f.app.find_note_expanded_files.contains(&target));
    f.draw(
        Vec2::new(900.0, 600.0),
        vec![egui::Event::Text("Alpha".into())],
        true,
    );
    assert_eq!(f.app.find_note_query, "Alpha");
    assert!(
        f.app
            .find_note_rows
            .iter()
            .any(|row| matches!(row,FindNoteRow::File {path,..} if path==&target))
    );
    f.app.selected_find_note_row = 0;
    f.click_action(5);
    assert!(f.app.link_preview_open);
    assert_eq!(f.app.link_preview_path, Some(target));
    assert_eq!(f.app.buffer.as_str(), "preserve unsaved text");
    assert!(f.app.dirty);
}
#[test]
fn empty_results_preserve_query_and_close_action() {
    let mut f = Fixture::new();
    f.app.find_note_rows.clear();
    f.app.find_note_query = "missing ñ".into();
    for _ in 0..3 {
        f.draw(Vec2::new(900.0, 600.0), vec![], false);
    }
    assert!(f.list().rows.is_empty());
    f.click_action(0);
    assert!(!f.app.find_note_open);
    assert_eq!(f.app.find_note_query, "missing ñ");
    assert!(f.app.dirty);
}
#[test]
fn clean_main_action_uses_async_open_without_replacing_the_buffer_in_the_dialog() {
    let mut f = Fixture::new();
    f.app.dirty = false;
    let target = f.root.join("target.md");
    f.app.find_note_rows = vec![FindNoteRow::File {
        path: target,
        match_hint: String::new(),
    }];
    for _ in 0..3 {
        f.draw(Vec2::new(900.0, 600.0), vec![], false);
    }
    f.click_action(4);
    assert!(!f.app.find_note_open);
    assert!(f.app.pending_file_open.is_some());
    assert_eq!(f.app.buffer.as_str(), "preserve unsaved text");
    assert_eq!(f.app.path, Some(f.root.join("current.md")));
    f.app.cancel_file_open();
}
#[test]
fn main_open_action_refuses_dirty_document_without_modifying_disk() {
    let mut f = Fixture::new();
    let target = f.root.join("target.md");
    let bytes = fs::read(&target).unwrap();
    f.app.find_note_rows = vec![FindNoteRow::File {
        path: target.clone(),
        match_hint: String::new(),
    }];
    for _ in 0..3 {
        f.draw(Vec2::new(900.0, 600.0), vec![], false);
    }
    f.click_action(4);
    assert!(!f.app.find_note_open);
    assert!(f.app.status.contains("Save or discard"));
    assert_eq!(f.app.path, Some(f.root.join("current.md")));
    assert_eq!(f.app.buffer.as_str(), "preserve unsaved text");
    assert!(f.app.dirty);
    assert_eq!(fs::read(target).unwrap(), bytes);
}
