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
            "slate-resident-{}-{}",
            std::process::id(),
            crate::test_unique()
        ));
        fs::create_dir_all(&root).unwrap();
        app.settings_path_override = Some(root.join("config.toml"));
        // As if this process owned the tray icon (no real window in tests).
        app.resident_active = true;
        let path = root.join("note.md");
        fs::write(&path, "hola\n").unwrap();
        app.path = Some(path);
        app.buffer.set_text("hola\n".into());
        app.dirty = false;
        Self { app, ctx, root }
    }

    /// Runs one frame in which the window manager asked to close the window.
    fn close(&mut self) -> Vec<egui::ViewportCommand> {
        let mut raw = egui::RawInput::default();
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                events: vec![egui::ViewportEvent::Close],
                ..Default::default()
            },
        );
        let app = &mut self.app;
        let output = self
            .ctx
            .run_ui(raw, |ui| app.handle_window_close_request(&ui.ctx().clone()));
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.commands.clone())
            .unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.document_safety.flush();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn closing_the_window_hides_it_and_keeps_the_tabs() {
    let mut f = Fixture::new();
    f.app.agenda_open = true;
    let commands = f.close();
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(commands.contains(&egui::ViewportCommand::Visible(false)));
    assert!(f.app.window_hidden);
    assert_eq!(f.app.path, Some(f.root.join("note.md")));
    assert_eq!(f.app.buffer.as_str(), "hola\n");
    assert!(!f.app.agenda_open, "transient panels close");
    assert!(f.root.join("config.toml").exists(), "settings persisted");
    assert!(f.root.join("session.json").exists(), "tabs persisted");
}

#[test]
fn hiding_keeps_unsaved_text_without_asking() {
    let mut f = Fixture::new();
    f.app.buffer.set_text("sin guardar".into());
    f.app.dirty = true;
    let commands = f.close();
    assert!(commands.contains(&egui::ViewportCommand::Visible(false)));
    assert!(f.app.pending_action.is_none());
    assert!(f.app.dirty);
    assert_eq!(f.app.buffer.as_str(), "sin guardar");
}

#[test]
fn quit_all_really_closes() {
    let mut f = Fixture::new();
    f.app.quit_for_real = true;
    let commands = f.close();
    assert!(!commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(!f.app.window_hidden);
}

#[test]
fn quit_all_asks_for_each_unsaved_tab_first() {
    let mut f = Fixture::new();
    f.app.buffer.set_text("sin guardar".into());
    f.app.dirty = true;
    f.app.new_tab();
    assert_eq!(f.app.tabs.active, 1);
    f.app.quit_for_real = true;
    let commands = f.close();
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(f.app.tabs.active, 0, "jumps to the unsaved tab");
    assert!(f.app.pending_action == Some(PendingAction::Quit));
}

#[test]
fn showing_a_hidden_window_makes_it_visible_and_focused() {
    let mut f = Fixture::new();
    f.close();
    let app = &mut f.app;
    let output = f.ctx.run_ui(egui::RawInput::default(), |ui| {
        app.show_window(&ui.ctx().clone())
    });
    let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(commands.contains(&egui::ViewportCommand::Visible(true)));
    assert!(commands.contains(&egui::ViewportCommand::Focus));
    assert!(!f.app.window_hidden);
}

/// Ctrl+key in the editor goes through the Ctrl layer: the key is recorded
/// while Ctrl is held and the sequence runs when Ctrl is released.
fn press_ctrl(f: &mut Fixture, key: egui::Key) -> Vec<egui::ViewportCommand> {
    let press = egui::RawInput {
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
        modifiers: egui::Modifiers::CTRL,
        ..Default::default()
    };
    // Releasing the key too, or egui reports the next press as a repeat.
    let release = egui::RawInput {
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut commands = Vec::new();
    for raw in [press, release] {
        let app = &mut f.app;
        let output = f.ctx.run_ui(raw, |ui| app.shortcuts(&ui.ctx().clone()));
        if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
            commands.extend(viewport.commands.iter().cloned());
        }
    }
    commands
}

#[test]
fn tab_keys_close_the_tab_with_ctrl_w_and_the_window_with_ctrl_q() {
    let mut f = Fixture::new();
    press_ctrl(&mut f, egui::Key::T);
    assert_eq!(f.app.tabs.list.len(), 2, "Ctrl+T opens a tab");
    assert_eq!(f.app.buffer.as_str(), "");

    let commands = press_ctrl(&mut f, egui::Key::W);
    assert!(!commands.contains(&egui::ViewportCommand::Close));
    assert_eq!(f.app.tabs.list.len(), 1, "Ctrl+W closes the tab");
    assert_eq!(f.app.buffer.as_str(), "hola\n", "back to the other tab");

    f.app.buffer.set_text("sin guardar".into());
    f.app.dirty = true;
    press_ctrl(&mut f, egui::Key::W);
    assert!(
        f.app.pending_action == Some(PendingAction::CloseTab),
        "asks first"
    );
    assert_eq!(f.app.buffer.as_str(), "sin guardar");
    f.app.pending_action = None;
    f.app.dirty = false;

    press_ctrl(&mut f, egui::Key::W);
    assert_eq!(f.app.tabs.list.len(), 1, "the last tab stays, empty");
    assert_eq!(f.app.buffer.as_str(), "");

    let commands = press_ctrl(&mut f, egui::Key::Q);
    assert!(commands.contains(&egui::ViewportCommand::Close));
    assert!(
        !f.app.quit_for_real,
        "Ctrl+Q closes the window, not the tray"
    );
}
