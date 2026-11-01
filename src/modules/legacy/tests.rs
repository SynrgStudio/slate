// ---- from bug_fix_tests.rs: fn item
fn item(line_index: usize, title: &str) -> AgendaItem {
    AgendaItem {
        kind: "task".into(),
        path: PathBuf::from("C:/notes/tasks.md"),
        relative_path: "tasks.md".into(),
        line_number: line_index + 1,
        line_index,
        state: "open".into(),
        title: title.into(),
        tags: Vec::new(),
        projects: Vec::new(),
        priority: None,
        due: None,
        scheduled: None,
        heading: None,
    }
}


// ---- from bug_fix_tests.rs: fn tag
fn tag(line: &mut String) -> bool {
    line.push_str(" #x");
    true
}


// ---- from bug_fix_tests.rs: fn agenda_edits_find_their_task_and_keep_line_endings
#[test]
fn agenda_edits_find_their_task_and_keep_line_endings() {
    // Indexed on line 1; a line was inserted above since.
    let text = "# Tareas\r\nnueva línea\r\n- [ ] comprar pan\r\n- [ ] llamar\r\n";
    let edited = SlateApp::edit_flow_item_text(text, &item(1, "comprar pan"), tag)
        .unwrap()
        .unwrap();
    assert_eq!(
        edited,
        "# Tareas\r\nnueva línea\r\n- [ ] comprar pan #x\r\n- [ ] llamar\r\n"
    );
    // Its line now holds something else and it is nowhere: never overwrite.
    let gone = "# Tareas\n- [ ] otra cosa\n";
    assert!(SlateApp::edit_flow_item_text(gone, &item(1, "comprar pan"), tag).is_err());
    // No change asked: nothing to write.
    assert_eq!(
        SlateApp::edit_flow_item_text("- [ ] comprar pan\n", &item(0, "comprar pan"), |_| false),
        Ok(None)
    );
    // The nearest of two equal titles is the one.
    let twice = "- [ ] a\n- [ ] x\n- [ ] b\n- [ ] c\n- [ ] x\n";
    let edited = SlateApp::edit_flow_item_text(twice, &item(3, "x"), tag)
        .unwrap()
        .unwrap();
    assert_eq!(edited, "- [ ] a\n- [ ] x\n- [ ] b\n- [ ] c\n- [ ] x #x\n");
}

// ---- from tabs_tests.rs: fn the_agenda_sees_unsaved_tasks_of_every_tab
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

// ---- from picker_tests.rs: fn background_index_uses_embedded_sqlite
#[test]
fn background_index_uses_embedded_sqlite() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, None);
    let root = std::env::temp_dir().join(format!(
        "slate-embedded-test-{}-{}",
        std::process::id(),
        crate::test_unique()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("notes.md"), "- [ ] embedded task\n").unwrap();
    app.start_flow_background_rescan(root.clone());
    let rx = app.flow_background_rx.take().unwrap();
    let result = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(result.errors, 0, "{:?}", result.first_error);
    assert_eq!(result.indexed_files, 1);
    assert_eq!(result.indexed_tasks, 1);
    let cache = SlateApp::flow_sqlite_path_for_root(&root).unwrap();
    assert_eq!(
        flow_store::read_tasks(&cache).unwrap()[0].title,
        "embedded task"
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(cache.parent().unwrap());
}

