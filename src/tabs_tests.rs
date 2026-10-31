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
        // Built as scratch so the user's real "reopen last file" setting is not
        // applied, then switched to a main editor window.
        let mut app = SlateApp::new(&cc, None, true, false, None);
        app.scratch = false;
        let root = std::env::temp_dir().join(format!(
            "slate-tabs-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
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

    /// Runs frames until the asynchronous file open has landed.
    fn settle(&mut self) {
        for _ in 0..400 {
            let app = &mut self.app;
            let _ = self
                .ctx
                .run_ui(egui::RawInput::default(), |ui| app.poll_file_open(ui.ctx()));
            if self.app.pending_file_open.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("file open did not finish");
    }

    fn open(&mut self, path: &Path) {
        self.app
            .request_file_open(path.to_path_buf(), OpenDestination::Restore);
        self.settle();
    }

    fn labels(&self) -> Vec<String> {
        self.app.tab_labels().into_iter().map(|l| l.text).collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.document_safety.flush();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn opening_files_uses_tabs_reuses_a_blank_tab_and_never_duplicates() {
    let mut f = Fixture::new();
    let a = f.file("a.md", "alpha\n");
    let b = f.file("b.md", "beta\n");
    f.open(&a);
    assert_eq!(f.labels(), ["a.md"], "the empty first tab is reused");
    f.open(&b);
    assert_eq!(f.labels(), ["a.md", "b.md"]);
    assert_eq!(f.app.buffer.as_str(), "beta\n");
    f.open(&a);
    assert_eq!(f.labels(), ["a.md", "b.md"], "no duplicate tab");
    assert_eq!(f.app.tabs.active, 0);
    assert_eq!(f.app.buffer.as_str(), "alpha\n");
}

#[test]
fn each_tab_keeps_its_text_undo_cursor_and_unsaved_state() {
    let mut f = Fixture::new();
    let a = f.file("a.md", "alpha\n");
    f.open(&a);
    f.app.buffer.set_cursor(5);
    f.app.buffer.type_text("!");
    f.app.dirty = true;
    f.app.new_tab();
    f.app.buffer.type_text("nuevo");
    f.app.dirty = true;

    f.app.activate_tab(0);
    assert_eq!(f.app.buffer.as_str(), "alpha!\n");
    assert_eq!(f.app.buffer.cursor(), 6);
    assert!(f.app.dirty);
    assert!(f.app.buffer.undo());
    assert_eq!(f.app.buffer.as_str(), "alpha\n", "undo history came along");

    f.app.activate_tab(1);
    assert_eq!(f.app.buffer.as_str(), "nuevo");
    let labels = f.app.tab_labels();
    assert!(labels[0].dirty && labels[1].dirty);
}

#[test]
fn opening_while_unsaved_continues_in_a_new_tab_without_asking() {
    let mut f = Fixture::new();
    f.app.buffer.type_text("borrador");
    f.app.dirty = true;
    f.app.confirm(PendingAction::Open);
    assert!(f.app.pending_action.is_none(), "nothing to discard");
    assert_eq!(f.app.tabs.list.len(), 2);
    assert_eq!(f.app.buffer.as_str(), "", "continues in the new tab");
    f.app.activate_tab(0);
    assert_eq!(f.app.buffer.as_str(), "borrador");
}

#[test]
fn closing_a_tab_shows_its_neighbour_and_the_last_tab_stays_empty() {
    let mut f = Fixture::new();
    let a = f.file("a.md", "alpha\n");
    let b = f.file("b.md", "beta\n");
    let c = f.file("c.md", "gamma\n");
    f.open(&a);
    f.open(&b);
    f.open(&c);
    f.app.activate_tab(1);
    f.app.close_active_tab();
    assert_eq!(f.labels(), ["a.md", "c.md"]);
    assert_eq!(f.app.buffer.as_str(), "gamma\n", "the tab to the right");
    f.app.close_active_tab();
    assert_eq!(f.labels(), ["a.md"]);
    assert_eq!(f.app.buffer.as_str(), "alpha\n");
    f.app.close_active_tab();
    assert_eq!(f.labels(), ["untitled"]);
    assert_eq!(f.app.buffer.as_str(), "");
}

#[test]
fn session_restores_tabs_and_loads_inactive_ones_when_shown() {
    let mut f = Fixture::new();
    let a = f.file("a.md", "alpha\n");
    let b = f.file("b.md", "beta\n");
    f.open(&a);
    f.open(&b);
    f.app.activate_tab(0);
    f.app.save_settings().unwrap();

    let mut g = Fixture::new();
    g.app.settings_path_override = f.app.settings_path_override.clone();
    assert!(g.app.restore_session());
    assert_eq!(g.labels(), ["a.md", "b.md"]);
    assert_eq!(g.app.tabs.active, 0);
    g.settle();
    assert_eq!(g.app.buffer.as_str(), "alpha\n");
    g.app.activate_tab(1);
    g.settle();
    assert_eq!(g.app.buffer.as_str(), "beta\n", "loaded on first show");
    assert_eq!(g.app.path.as_deref(), Some(b.as_path()));
}

fn ctrl_events(f: &mut Fixture, mut events: Vec<egui::Event>) {
    // Release the keys too, or egui reports the next press as a repeat.
    let releases: Vec<egui::Event> = events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Key { key, modifiers, .. } => Some(egui::Event::Key {
                key: *key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: *modifiers,
            }),
            _ => None,
        })
        .collect();
    events.extend(releases);
    let raw = egui::RawInput {
        events,
        modifiers: egui::Modifiers::CTRL,
        ..Default::default()
    };
    let app = &mut f.app;
    let _ = f.ctx.run_ui(raw, |ui| app.shortcuts(&ui.ctx().clone()));
    let app = &mut f.app;
    let _ = f.ctx.run_ui(egui::RawInput::default(), |ui| {
        app.shortcuts(&ui.ctx().clone())
    });
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn ctrl_tab_ctrl_number_and_ctrl_wheel_switch_tabs() {
    let mut f = Fixture::new();
    f.app.new_tab();
    f.app.new_tab();
    f.app.activate_tab(0);
    let ctrl_shift = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;

    ctrl_events(&mut f, vec![key(egui::Key::Tab, egui::Modifiers::CTRL)]);
    assert_eq!(f.app.tabs.active, 1, "Ctrl+Tab");
    ctrl_events(&mut f, vec![key(egui::Key::Tab, ctrl_shift)]);
    assert_eq!(f.app.tabs.active, 0, "Ctrl+Shift+Tab");
    ctrl_events(&mut f, vec![key(egui::Key::Tab, ctrl_shift)]);
    assert_eq!(f.app.tabs.active, 2, "wraps around");
    ctrl_events(&mut f, vec![key(egui::Key::Num1, egui::Modifiers::CTRL)]);
    assert_eq!(f.app.tabs.active, 0, "Ctrl+1");
    ctrl_events(&mut f, vec![key(egui::Key::Num9, egui::Modifiers::CTRL)]);
    assert_eq!(f.app.tabs.active, 2, "Ctrl+9 is the last tab");

    let wheel = |y: f32| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, y),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::CTRL,
    };
    ctrl_events(&mut f, vec![wheel(1.0)]);
    assert_eq!(f.app.tabs.active, 0, "wheel up = next (wraps)");
    ctrl_events(&mut f, vec![wheel(-1.0)]);
    assert_eq!(f.app.tabs.active, 2, "wheel down = previous");
    // A touchpad sends small steps: one tab per accumulated line.
    ctrl_events(&mut f, vec![wheel(0.4), wheel(0.4)]);
    assert_eq!(f.app.tabs.active, 2);
    ctrl_events(&mut f, vec![wheel(0.4)]);
    assert_eq!(f.app.tabs.active, 0);
}

/// One frame with Ctrl held: press and release `key`.
fn tap_with_ctrl_held(f: &mut Fixture, key: egui::Key) {
    for pressed in [true, false] {
        let raw = egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
            modifiers: egui::Modifiers::CTRL,
            ..Default::default()
        };
        let app = &mut f.app;
        let _ = f.ctx.run_ui(raw, |ui| app.shortcuts(&ui.ctx().clone()));
    }
}

fn release_ctrl(f: &mut Fixture) {
    let app = &mut f.app;
    let _ = f.ctx.run_ui(egui::RawInput::default(), |ui| {
        app.shortcuts(&ui.ctx().clone())
    });
}

#[test]
fn ctrl_t_and_ctrl_w_act_while_ctrl_is_still_held() {
    let mut f = Fixture::new();
    tap_with_ctrl_held(&mut f, egui::Key::T);
    tap_with_ctrl_held(&mut f, egui::Key::T);
    assert_eq!(
        f.app.tabs.list.len(),
        3,
        "two new tabs before releasing Ctrl"
    );
    tap_with_ctrl_held(&mut f, egui::Key::W);
    assert_eq!(f.app.tabs.list.len(), 2, "closed before releasing Ctrl");
    release_ctrl(&mut f);
    assert_eq!(
        f.app.tabs.list.len(),
        2,
        "release does not repeat the action"
    );
}

#[test]
fn ctrl_sequences_containing_w_still_run_on_release() {
    let mut f = Fixture::new();
    f.app.buffer.set_text("uno dos".into());
    f.app.buffer.set_cursor(0);
    tap_with_ctrl_held(&mut f, egui::Key::D);
    tap_with_ctrl_held(&mut f, egui::Key::W);
    assert_eq!(f.app.tabs.list.len(), 1, "w inside `dw` is not close-tab");
    release_ctrl(&mut f);
    assert_ne!(f.app.buffer.as_str(), "uno dos", "`dw` deleted a word");
    assert_eq!(f.app.tabs.list.len(), 1);
}

#[test]
fn equal_file_names_show_as_many_folders_as_needed() {
    let a = PathBuf::from("C:/notas/README.md");
    let b = PathBuf::from("C:/slate/README.md");
    let c = PathBuf::from("C:/x/docs/plan.md");
    let d = PathBuf::from("C:/y/docs/plan.md");
    let e = PathBuf::from("C:/solo/unico.md");
    let names =
        tabs::disambiguated_names(&[Some(&a), Some(&b), Some(&c), Some(&d), Some(&e), None]);
    assert_eq!(
        names,
        [
            "README.md · notas",
            "README.md · slate",
            "plan.md · x/docs",
            "plan.md · y/docs",
            "unico.md",
            "untitled",
        ]
    );
}

#[test]
fn moving_a_tab_keeps_the_active_document() {
    let mut f = Fixture::new();
    let a = f.file("a.md", "alpha\n");
    let b = f.file("b.md", "beta\n");
    let c = f.file("c.md", "gamma\n");
    f.open(&a);
    f.open(&b);
    f.open(&c);
    f.app.activate_tab(1);
    f.app.move_tab(1, 2);
    assert_eq!(f.labels(), ["a.md", "c.md", "b.md"]);
    assert_eq!(f.app.tabs.active, 2);
    assert_eq!(f.app.buffer.as_str(), "beta\n");
    f.app.move_tab(0, 2);
    assert_eq!(f.labels(), ["c.md", "b.md", "a.md"]);
    assert_eq!(f.app.tabs.active, 1, "active follows its tab");
    f.app.activate_tab(2);
    assert_eq!(f.app.buffer.as_str(), "alpha\n");
}

#[test]
fn ctrl_shift_t_reopens_closed_tabs_newest_first() {
    let mut f = Fixture::new();
    let a = f.file("a.md", "alpha\n");
    let b = f.file("b.md", "beta\n");
    f.open(&a);
    f.open(&b);
    f.app.close_active_tab();
    f.app.close_active_tab();
    assert_eq!(f.labels(), ["untitled"]);
    let ctrl_shift = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
    ctrl_events(&mut f, vec![key(egui::Key::T, ctrl_shift)]);
    f.settle();
    assert_eq!(f.labels(), ["a.md"], "reuses the empty tab");
    ctrl_events(&mut f, vec![key(egui::Key::T, ctrl_shift)]);
    f.settle();
    assert_eq!(f.labels(), ["a.md", "b.md"]);
    f.app.reopen_closed_tab();
    assert_eq!(f.app.status, "No closed tab to reopen");
}

#[test]
fn a_dragged_tab_swaps_only_past_the_neighbours_centre() {
    let rect = |x: f32, w: f32| egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(w, 26.0));
    let rects = [rect(0.0, 200.0), rect(202.0, 90.0), rect(294.0, 90.0)];
    assert_eq!(
        window_ui::drag_target(&rects, 0, 240.0),
        0,
        "before the centre"
    );
    assert_eq!(
        window_ui::drag_target(&rects, 0, 250.0),
        1,
        "past the centre"
    );
    assert_eq!(
        window_ui::drag_target(&rects, 0, 500.0),
        2,
        "beyond the end"
    );
    assert_eq!(window_ui::drag_target(&rects, 2, 10.0), 0);
}

#[test]
fn a_file_changed_on_disk_while_its_tab_was_inactive_is_flagged_when_shown() {
    let mut f = Fixture::new();
    f.app.document_safety =
        document_safety::Manager::new(f.ctx.clone(), Some(f.root.join("recovery")));
    let a = f.file("a.md", "alpha\n");
    let b = f.file("b.md", "beta\n");
    f.open(&a);
    f.open(&b);
    fs::write(&a, "changed outside Slate\n").unwrap();
    f.app.activate_tab(0);
    for _ in 0..300 {
        let app = &mut f.app;
        let _ = f.ctx.run_ui(egui::RawInput::default(), |ui| {
            app.poll_document_safety(ui.ctx())
        });
        if f.app.file_conflict.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(f.app.file_conflict.is_some(), "conflict flagged on return");
    assert_eq!(f.app.buffer.as_str(), "alpha\n", "editor text is kept");
}

#[test]
fn the_agenda_sees_unsaved_tasks_of_every_tab() {
    let mut f = Fixture::new();
    let a = f.file("a.md", "");
    let b = f.file("b.md", "");
    f.open(&a);
    f.app.buffer.set_text("- [ ] comprar pan\n".into());
    f.app.dirty = true;
    f.open(&b);
    f.app.buffer.set_text("- [ ] llamar a Ana\n".into());
    f.app.dirty = true;
    let mut items = Vec::new();
    f.app.overlay_current_buffer_agenda_items(&mut items);
    let mut titles: Vec<_> = items.iter().map(|item| item.title.clone()).collect();
    titles.sort();
    assert_eq!(titles, ["comprar pan", "llamar a Ana"]);
}
