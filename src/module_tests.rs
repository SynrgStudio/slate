//! Modules through the core: commands, background work, results, settings.
use super::*;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "slate-modules-{name}-{}-{}",
        std::process::id(),
        crate::test_unique()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path) -> (SlateApp, egui::Context) {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, None);
    app.scratch = false;
    app.settings_path_override = Some(root.join("config.toml"));
    (app, ctx)
}

#[test]
fn backlinks_run_in_the_background_and_open_from_the_result_list() {
    let root = temp_root("backlinks");
    let target = root.join("target.md");
    fs::write(&target, "# Target\n").unwrap();
    fs::write(root.join("one.md"), "intro\nsee [t](target.md)\n").unwrap();
    fs::write(root.join("two.md"), "[[target]]\n").unwrap();
    let (mut app, ctx) = app(&root);
    app.vaults = vec![root.clone()];
    app.path = Some(target.clone());
    app.buffer.set_text("# Target\n".into());

    app.command_line = ":bl".into();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.run_command_line(ui.ctx()));
    assert!(app.status.contains("searching"), "{}", app.status);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !app.results.open && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.poll_modules(ui.ctx()));
    }
    assert!(app.results.open, "results arrive: {}", app.status);
    assert_eq!(app.status, "Backlinks of target.md: 2");

    // Filter to one.md and open it at its line.
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![egui::Event::Text("one".into()), key(Key::Enter)],
            ..Default::default()
        },
        |ui| {
            app.shortcuts(ui.ctx());
        },
    );
    assert!(!app.results.open);
    let pending = app.pending_file_open.as_ref().expect("opening one.md");
    assert_eq!(pending.path, root.join("one.md"));
    app.cancel_file_open();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn modules_can_be_turned_off_and_vaults_survive_a_restart() {
    let root = temp_root("settings");
    let (mut app, _ctx) = app(&root);
    assert!(commands::canonical_command_name("backlinks").is_some());
    app.add_vault(root.clone());
    app.set_module_enabled("references", false);
    assert!(
        commands::canonical_command_name("backlinks").is_none(),
        "an off module's commands are gone"
    );
    settings_writer::flush();
    let saved = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(saved.contains("module.references = false"), "{saved}");

    let (mut restarted, _ctx) = app_with_config(&root);
    restarted.load_settings();
    assert_eq!(restarted.vaults, vec![std::path::absolute(&root).unwrap()]);
    assert!(!restarted.module_host.is_enabled("references"));
    restarted.set_module_enabled("references", true);
    assert!(commands::canonical_command_name("bl").is_some());
    let _ = fs::remove_dir_all(root);
}

fn app_with_config(root: &Path) -> (SlateApp, egui::Context) {
    app(root)
}
