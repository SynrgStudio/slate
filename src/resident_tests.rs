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
        let mut app = SlateApp::new(&cc, None, true, false, None);
        let root = std::env::temp_dir().join(format!(
            "slate-resident-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
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
fn closing_a_clean_document_hides_instead_of_exiting() {
    let mut f = Fixture::new();
    f.app.agenda_open = true;
    let commands = f.close();
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(commands.contains(&egui::ViewportCommand::Visible(false)));
    assert!(f.app.window_hidden);
    // Next show starts like a fresh launch.
    assert_eq!(f.app.path, None);
    assert_eq!(f.app.buffer.as_str(), "");
    assert!(!f.app.agenda_open);
    assert!(f.root.join("config.toml").exists(), "settings persisted");
}

#[test]
fn closing_a_dirty_document_asks_first_and_stays_visible() {
    let mut f = Fixture::new();
    f.app.dirty = true;
    let commands = f.close();
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(!commands.contains(&egui::ViewportCommand::Visible(false)));
    assert!(!f.app.window_hidden);
    assert!(f.app.pending_action == Some(PendingAction::Quit));
    assert_eq!(f.app.buffer.as_str(), "hola\n");
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
fn showing_a_hidden_window_makes_it_visible_and_focused() {
    let mut f = Fixture::new();
    f.close();
    let app = &mut f.app;
    let output = f.ctx.run_ui(egui::RawInput::default(), |ui| {
        app.show_window(&ui.ctx().clone(), true)
    });
    let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(commands.contains(&egui::ViewportCommand::Visible(true)));
    assert!(commands.contains(&egui::ViewportCommand::Focus));
    assert!(!f.app.window_hidden);
}
