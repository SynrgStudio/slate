//! Regression tests for the extras audit fixes (agenda writes, refile and
//! inbox files, Lance, Recent).
use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "slate-bugfix-{name}-{}-{}",
        std::process::id(),
        crate::test_unique()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

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

fn tag(line: &mut String) -> bool {
    line.push_str(" #x");
    true
}

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

#[test]
fn blocks_go_under_headings_in_crlf_files_without_changing_line_endings() {
    let dir = temp_dir("crlf");
    let path = dir.join("destino.md");
    fs::write(
        &path,
        "# Uno\r\ntexto\r\n## Dos\r\nmás\r\n# Tres\r\nfin\r\n",
    )
    .unwrap();
    let headings = link_ui::markdown_heading_options(&fs::read_to_string(&path).unwrap());
    let dos = headings.iter().find(|h| h.text == "Dos").unwrap().clone();
    append_markdown_block_to_heading(&path, &dos, "- [ ] movida\n").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "# Uno\r\ntexto\r\n## Dos\r\nmás\r\n\r\n- [ ] movida\r\n# Tres\r\nfin\r\n"
    );
    append_markdown_block(&path, "- [ ] al final\n").unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.ends_with("fin\r\n\r\n- [ ] al final\r\n"), "{text:?}");
    assert!(
        !text.replace("\r\n", "").contains('\n'),
        "only CRLF endings"
    );
    let _ = fs::remove_dir_all(dir);
}

fn app() -> SlateApp {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx);
    let mut app = SlateApp::new(&cc, None, true, false, None);
    app.settings_path_override = Some(temp_dir("settings").join("config.toml"));
    app
}

#[test]
fn lance_delete_only_removes_a_mark_the_filter_shows() {
    let mut app = app();
    app.lance_files = ["alpha.md", "beta.md", "gamma.md"]
        .iter()
        .map(PathBuf::from)
        .collect();
    app.lance_query = "beta".into();
    app.selected_lance_file = 0; // alpha: hidden by the filter
    app.remove_selected_lance_file();
    assert_eq!(app.lance_files.len(), 3, "a hidden mark is not deleted");
    app.selected_lance_file = 1;
    app.remove_selected_lance_file();
    assert_eq!(
        app.lance_files,
        vec![PathBuf::from("alpha.md"), PathBuf::from("gamma.md")]
    );
}

#[test]
fn recent_filter_takes_spaces_and_closes_the_other_pickers() {
    let mut app = app();
    app.recent_files = vec![PathBuf::from("C:/notas/mi nota.md")];
    app.lance_open = true;
    app.file_picker_open = true;
    app.open_recent_picker_with_query(String::new());
    assert!(app.recent_picker_open && !app.lance_open && !app.file_picker_open);
    let ctx = egui::Context::default();
    for (key, text) in [(Key::M, "mi"), (Key::Space, " "), (Key::N, "nota")] {
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Text(text.into()),
                ],
                ..Default::default()
            },
            |ui| {
                app.shortcuts(ui.ctx());
            },
        );
    }
    assert_eq!(app.recent_query, "mi nota");
    assert!(app.recent_picker_open, "space did not open a file");
}

#[test]
fn find_note_ignores_case_beyond_ascii() {
    let dir = temp_dir("find-note");
    let path = dir.join("Árbol genealógico.md");
    fs::write(&path, "# Familia\nUNA CANCIÓN DE CUNA\n").unwrap();
    assert!(find_note_file_matches(&dir, &path, "árbol"));
    assert!(find_note_file_matches(&dir, &path, "canción"));
    assert_eq!(
        find_note_match_hint(&dir, &path, "canción"),
        "UNA CANCIÓN DE CUNA"
    );
    let _ = fs::remove_dir_all(dir);
}
