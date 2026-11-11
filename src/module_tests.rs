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

#[test]
fn tasks_list_the_vault_with_unsaved_text_after_a_pause() {
    let root = temp_root("tasks");
    let note = root.join("plan.md");
    fs::write(&note, "- [ ] saved task
").unwrap();
    fs::write(root.join("other.md"), "- [ ] elsewhere #x
- [x] finished
").unwrap();
    let (mut app, ctx) = app(&root);
    app.vaults = vec![root.clone()];
    app.path = Some(note.clone());
    app.buffer
        .set_text("- [ ] saved task
- [ ] typed, not saved
".into());
    // A pause after typing: modules see the unsaved text.
    app.module_idle_at = Some(std::time::Instant::now());
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.poll_modules(ui.ctx()));

    app.command_line = ":flow".into();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.run_command_line(ui.ctx()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !app.results.open && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.poll_modules(ui.ctx()));
    }
    assert!(app.results.open, "results arrive: {}", app.status);
    let mut labels: Vec<_> = app.results.items.iter().map(|item| item.label.clone()).collect();
    labels.sort();
    assert_eq!(
        labels,
        vec!["[ ] elsewhere  #x", "[ ] saved task", "[ ] typed, not saved"]
    );
    let typed = app
        .results
        .items
        .iter()
        .find(|item| item.label.contains("typed"))
        .unwrap();
    assert_eq!((typed.path.clone(), typed.line), (note.clone(), Some(1)));

    // Outside a vault Flow does nothing and says where to add one.
    app.vaults.clear();
    app.results.open = false;
    app.command_line = ":flow".into();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.run_command_line(ui.ctx()));
    assert!(app.status.contains("Settings"), "{}", app.status);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn vaults_dialog_lists_removes_and_keeps_letters_out_of_the_editor() {
    let root = temp_root("vaults-dialog");
    let (one, two) = (root.join("one"), root.join("two"));
    fs::create_dir_all(&one).unwrap();
    fs::create_dir_all(&two).unwrap();
    fs::write(two.join("note.md"), "x
").unwrap();
    let (mut app, ctx) = app(&root);
    app.add_vault(one.clone());
    app.add_vault(two.clone());
    app.path = Some(two.join("note.md"));
    app.buffer.set_text("x
".into());

    app.command_line = ":vaults".into();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.run_command_line(ui.ctx()));
    assert!(app.vaults_dialog.open);
    assert_eq!(app.vaults_dialog.selected, 1, "starts on the open note's vault");
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![key(Key::ArrowUp), egui::Event::Text("q".into()), key(Key::Delete)],
            ..Default::default()
        },
        |ui| {
            app.shortcuts(ui.ctx());
            app.vaults_dialog(ui.ctx());
        },
    );
    assert_eq!(app.vaults, vec![std::path::absolute(&two).unwrap()], "one is no longer a vault");
    assert!(one.is_dir(), "its folder is untouched");
    assert_eq!(app.buffer.as_str(), "x
", "typing in the dialog does not edit the note");
    let saved = { settings_writer::flush(); fs::read_to_string(root.join("config.toml")).unwrap() };
    assert!(!saved.contains("one\""), "{saved}");
    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![key(Key::Escape)],
            ..Default::default()
        },
        |ui| app.shortcuts(ui.ctx()),
    );
    assert!(!app.vaults_dialog.open);
    let _ = fs::remove_dir_all(root);
}

fn app_with_config(root: &Path) -> (SlateApp, egui::Context) {
    app(root)
}

#[test]
fn save_as_left_at_a_drive_root_chooses_another_drive() {
    let root = temp_root("save-as-drives");
    let (mut app, ctx) = app(&root);
    app.open_save_as_modal();
    let drive = std::path::absolute(&root)
        .unwrap()
        .ancestors()
        .last()
        .unwrap()
        .to_path_buf();
    app.save_as_dir = drive.clone();
    app.save_as_filename.clear();
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![key(Key::ArrowLeft, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ui| app.shortcuts(ui.ctx()),
    );
    assert!(drive_picker::is_open(&ctx), "← at {} lists the drives", drive.display());
    // Enter picks a drive; the dialog lists it.
    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![key(Key::Enter, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ui| app.shortcuts(ui.ctx()),
    );
    assert!(!drive_picker::is_open(&ctx));
    assert!(app.save_as_open);
    assert!(app.save_as_dir.parent().is_none(), "{}", app.save_as_dir.display());
    // Ctrl+L too, from any folder.
    app.save_as_dir = root.clone();
    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![key(Key::L, egui::Modifiers::CTRL)],
            ..Default::default()
        },
        |ui| app.shortcuts(ui.ctx()),
    );
    assert!(drive_picker::is_open(&ctx));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn save_as_keeps_the_name_while_choosing_a_folder_and_saves_inside_it() {
    let root = temp_root("save-as-folder");
    fs::create_dir_all(root.join("vaulttest")).unwrap();
    fs::create_dir_all(root.join("other")).unwrap();
    let (mut app, ctx) = app(&root);
    app.buffer.set_text("veamos como funciona\n".into());
    app.path = Some(root.join("test1.md"));
    fs::write(root.join("test1.md"), "old\n").unwrap();
    app.open_save_as_modal();
    assert_eq!(app.save_as_filename, "test1.md");
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let mut press = |app: &mut SlateApp, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| app.shortcuts(ui.ctx()),
        );
    };
    // The name starts selected: typing replaces it and searches folders.
    press(&mut app, vec![egui::Event::Text("vault".into())]);
    assert_eq!(app.save_as_filename, "vault");
    assert!(app.save_as_armed, "found a folder");
    // Deleting the search gives the name back.
    for _ in 0.."vault".len() {
        press(&mut app, vec![key(Key::Backspace)]);
    }
    assert_eq!(app.save_as_filename, "test1.md");
    assert!(!app.save_as_armed);
    press(&mut app, vec![egui::Event::Text("vault".into())]);
    press(&mut app, vec![key(Key::Enter)]);
    assert!(app.save_as_open);
    assert_eq!(app.save_as_dir, root.join("vaulttest"));
    assert_eq!(app.save_as_filename, "test1.md", "the name survives finding a folder");
    // Enter now saves here (the folder is empty: nothing is chosen).
    press(&mut app, vec![key(Key::Enter)]);
    assert!(!app.save_as_open);
    app.finish_background_save();
    assert_eq!(
        fs::read_to_string(root.join("vaulttest/test1.md")).unwrap(),
        "veamos como funciona\n"
    );

    // Arrows choose a folder; → or Enter goes in, the name stays.
    app.open_save_as_modal();
    app.save_as_dir = root.clone();
    app.refresh_save_as_entries();
    press(&mut app, vec![key(Key::ArrowDown)]);
    assert!(app.save_as_armed);
    press(&mut app, vec![key(Key::Enter)]);
    assert_ne!(app.save_as_dir, root, "went into the chosen folder");
    assert_eq!(app.save_as_filename, "test1.md");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn save_as_over_an_existing_file_asks_first() {
    let root = temp_root("save-as-overwrite");
    fs::write(root.join("other.md"), "keep me\n").unwrap();
    let (mut app, ctx) = app(&root);
    app.buffer.set_text("new text\n".into());
    app.path = Some(root.join("mine.md"));
    app.open_save_as_modal();
    app.save_as_dir = root.clone();
    app.refresh_save_as_entries();
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let mut press = |app: &mut SlateApp, events: Vec<egui::Event>| {
        let _ = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| app.shortcuts(ui.ctx()),
        );
    };
    press(&mut app, vec![egui::Event::Text("other.md".into()), key(Key::Enter)]);
    assert!(app.save_as_overwrite.is_some(), "asks before replacing");
    assert_eq!(fs::read_to_string(root.join("other.md")).unwrap(), "keep me\n");
    // Esc keeps it and goes back to the dialog; typing meanwhile is ignored.
    press(&mut app, vec![egui::Event::Text("zz".into()), key(Key::Escape)]);
    assert!(app.save_as_overwrite.is_none());
    assert!(app.save_as_open, "back in Save As");
    assert_eq!(app.save_as_filename, "other.md");
    assert_eq!(fs::read_to_string(root.join("other.md")).unwrap(), "keep me\n");
    // Enter twice: ask, then replace.
    press(&mut app, vec![key(Key::Enter)]);
    press(&mut app, vec![key(Key::Enter)]);
    assert!(!app.save_as_open);
    assert_eq!(fs::read_to_string(root.join("other.md")).unwrap(), "new text\n");
    let _ = fs::remove_dir_all(root);
}
