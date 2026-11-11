//! The undo history graph (Ctrl+Shift+Z / `:undo`) through the app: going to
//! another branch, bringing lines back, searching, opening a state in a tab,
//! and the saved state coming back with undo.
use super::*;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "slate-undo-{name}-{}-{}",
        std::process::id(),
        crate::test_unique()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

/// An app with `text` open from a file, clean.
fn app_with(root: &Path, text: &str) -> (SlateApp, egui::Context) {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, None);
    app.scratch = false;
    app.settings_path_override = Some(root.join("config.toml"));
    let path = root.join("compras.md");
    fs::write(&path, text).unwrap();
    app.buffer = EditorBuffer::from_text(text.to_string());
    app.path = Some(path.clone());
    app.disk_base = Some(safe_file::fingerprint(text.as_bytes()));
    app.dirty = false;
    (app, ctx)
}

fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn plain(key_code: Key) -> egui::Event {
    key(key_code, egui::Modifiers::NONE)
}

/// One frame with these events through the shortcuts and the dialog.
fn frame(app: &mut SlateApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let _ = ctx.run_ui(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ui| {
            app.shortcuts(ui.ctx());
            app.undo_dialog(ui.ctx());
        },
    );
}

fn open_graph(app: &mut SlateApp, ctx: &egui::Context) {
    frame(
        app,
        ctx,
        vec![key(Key::Z, egui::Modifiers::CTRL | egui::Modifiers::SHIFT)],
    );
    assert!(app.undo_dialog.open, "Ctrl+Shift+Z opens the undo history");
}

/// Replaces the first `old` in the buffer with `new`, as one edit.
fn replace(app: &mut SlateApp, old: &str, new: &str) {
    let start = app.buffer.as_str().find(old).expect("text to replace");
    app.buffer.set_selection(start, start + old.len());
    app.buffer.insert_text(new);
    app.dirty = true;
}

#[test]
fn enter_goes_to_a_branch_that_plain_undo_would_have_lost() {
    let root = temp_root("branch");
    let (mut app, ctx) = app_with(&root, "## Hoy\n- [ ] comprar leche\n");
    replace(&mut app, "leche", "yerba");
    assert!(app.buffer.undo());
    replace(&mut app, "- [ ] comprar leche\n", "- [ ] comprar leche\n- [ ] pagar la luz\n");

    open_graph(&mut app, &ctx);
    // Newest first: "pagar la luz" (here), "leche → yerba", the start.
    frame(&mut app, &ctx, vec![plain(Key::ArrowDown), plain(Key::Enter)]);
    assert!(!app.undo_dialog.open);
    assert_eq!(app.buffer.as_str(), "## Hoy\n- [ ] comprar yerba\n");
    assert!(app.dirty);
    assert!(app.status.contains("Went to the text from"), "{}", app.status);

    // One undo back is the text on disk: nothing unsaved.
    frame(&mut app, &ctx, vec![key(Key::Z, egui::Modifiers::CTRL)]);
    assert_eq!(app.buffer.as_str(), "## Hoy\n- [ ] comprar leche\n");
    assert!(!app.dirty, "undo back to the saved text leaves nothing unsaved");
    // And redo follows the branch visited last.
    frame(&mut app, &ctx, vec![key(Key::Y, egui::Modifiers::CTRL)]);
    assert_eq!(app.buffer.as_str(), "## Hoy\n- [ ] comprar yerba\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ctrl_enter_brings_a_deleted_line_back_where_it_was() {
    let root = temp_root("bring");
    let (mut app, ctx) = app_with(
        &root,
        "## Hoy\n- [ ] comprar pan\n- [ ] comprar yerba\n",
    );
    replace(&mut app, "- [ ] comprar pan\n", "");
    // Later work elsewhere: the line still goes back to its place.
    replace(&mut app, "- [ ] comprar yerba\n", "- [ ] comprar yerba\n- [ ] pagar la luz\n");

    open_graph(&mut app, &ctx);
    // The step that deleted the line.
    frame(&mut app, &ctx, vec![plain(Key::ArrowDown)]);
    frame(
        &mut app,
        &ctx,
        vec![key(Key::Enter, egui::Modifiers::CTRL)],
    );
    assert_eq!(
        app.buffer.as_str(),
        "## Hoy\n- [ ] comprar pan\n- [ ] comprar yerba\n- [ ] pagar la luz\n"
    );
    assert!(app.undo_dialog.open, "the history stays open to bring more");
    assert!(app.status.starts_with("Brought back 1 line"), "{}", app.status);
    assert_eq!(
        app.buffer.history().current_node().kind,
        crate::history::Kind::BroughtBack
    );
    // Already there: nothing more to bring from that step.
    frame(
        &mut app,
        &ctx,
        vec![key(Key::Enter, egui::Modifiers::CTRL)],
    );
    assert!(app.status.starts_with("Nothing to bring back"), "{}", app.status);
    assert_eq!(app.buffer.as_str().matches("comprar pan").count(), 1);

    frame(&mut app, &ctx, vec![plain(Key::Escape)]);
    frame(&mut app, &ctx, vec![key(Key::Z, egui::Modifiers::CTRL)]);
    assert_eq!(
        app.buffer.as_str(),
        "## Hoy\n- [ ] comprar yerba\n- [ ] pagar la luz\n",
        "bringing back is one undoable step"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ctrl_shift_enter_brings_lines_to_the_cursor_from_the_vs_now_diff() {
    let root = temp_root("cursor");
    let (mut app, ctx) = app_with(&root, "uno\ndos\ntres\n");
    replace(&mut app, "dos\n", "");
    replace(&mut app, "tres\n", "tres\ncuatro\n");
    app.buffer.set_cursor(0);

    open_graph(&mut app, &ctx);
    // Tab: compare with now; End: the start, which still had "dos".
    frame(&mut app, &ctx, vec![plain(Key::Tab), plain(Key::End)]);
    assert!(app.undo_dialog.vs_now_for_tests());
    frame(
        &mut app,
        &ctx,
        vec![key(
            Key::Enter,
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        )],
    );
    assert_eq!(app.buffer.as_str(), "dos\nuno\ntres\ncuatro\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn typing_in_the_history_searches_and_never_edits_the_document() {
    let root = temp_root("search");
    let (mut app, ctx) = app_with(&root, "## Hoy\n");
    app.buffer.move_to_bottom();
    for ch in "- [ ] pagar la luz\n- [ ] comprar pan".chars() {
        app.buffer.type_text(&ch.to_string());
    }
    let before = app.buffer.as_str().to_string();
    open_graph(&mut app, &ctx);
    frame(&mut app, &ctx, vec![egui::Event::Text("la luz".into())]);
    assert_eq!(app.buffer.as_str(), before, "typing searches instead");
    assert_eq!(app.undo_dialog.rows_for_tests(), 1, "one change typed \"la luz\"");
    frame(&mut app, &ctx, vec![egui::Event::Text("zzz".into())]);
    assert_eq!(app.undo_dialog.rows_for_tests(), 0);
    for _ in 0..3 {
        frame(&mut app, &ctx, vec![plain(Key::Backspace)]);
    }
    assert_eq!(app.undo_dialog.rows_for_tests(), 1);
    assert_eq!(app.buffer.as_str(), before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ctrl_o_opens_the_selected_state_in_a_new_tab() {
    let root = temp_root("tab");
    let (mut app, ctx) = app_with(&root, "original\n");
    replace(&mut app, "original", "edited");
    let tabs = app.tabs_len();
    open_graph(&mut app, &ctx);
    frame(&mut app, &ctx, vec![plain(Key::End)]);
    frame(&mut app, &ctx, vec![key(Key::O, egui::Modifiers::CTRL)]);
    assert!(!app.undo_dialog.open);
    assert_eq!(app.tabs_len(), tabs + 1);
    assert_eq!(app.buffer.as_str(), "original\n");
    assert!(app.path.is_none(), "an untitled copy, not the file");
    assert!(app.dirty);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cancelling_a_duplicate_placement_keeps_the_undo_history() {
    let root = temp_root("duplicate");
    let (mut app, _ctx) = app_with(&root, "uno\ndos\n");
    replace(&mut app, "dos", "tres");
    app.buffer.set_cursor(0);
    app.start_duplicate_placement();
    app.move_duplicate_placement_down();
    assert_eq!(app.buffer.as_str(), "uno\ntres\nuno\n");
    app.cancel_duplicate_placement();
    assert_eq!(app.buffer.as_str(), "uno\ntres\n");
    assert!(app.dirty);
    assert!(app.buffer.undo(), "the edit before the duplicate is still there");
    assert_eq!(app.buffer.as_str(), "uno\ndos\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_background_save_marks_the_state_it_wrote_even_if_typing_went_on() {
    let root = temp_root("save");
    let (mut app, ctx) = app_with(&root, "");
    app.sync_saves = false;
    app.buffer.type_text("a");
    app.dirty = true;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.save_in_background(ui.ctx())
    });
    // Typing while the write is on its way is a new step.
    app.buffer.type_text("b");
    app.finish_background_save();
    assert!(app.dirty, "the b is not on disk");
    assert_eq!(fs::read_to_string(app.path.clone().unwrap()).unwrap(), "a");
    frame(&mut app, &ctx, vec![key(Key::Z, egui::Modifiers::CTRL)]);
    assert_eq!(app.buffer.as_str(), "a");
    assert!(!app.dirty, "undo back to what was saved");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_preview_shows_exactly_what_ctrl_enter_will_do() {
    let root = temp_root("preview");
    let (mut app, ctx) = app_with(&root, "uno\ndos\ntres\ncuatro\n");
    replace(&mut app, "dos\n", "");
    replace(&mut app, "cuatro\n", "cuatro\ncinco\n");
    app.buffer.set_cursor(0);
    open_graph(&mut app, &ctx);
    // The newest step added "cinco", which is there: nothing would change.
    assert!(app.undo_dialog.preview_for_tests().is_err());
    // The step that deleted "dos": it lands between "uno" and "tres".
    frame(&mut app, &ctx, vec![plain(Key::ArrowDown)]);
    let preview = app.undo_dialog.preview_for_tests().expect("a preview");
    assert_eq!(preview, "uno\ndos\ntres\ncuatro\ncinco\n");
    // Holding Ctrl+Shift: at the cursor's line instead.
    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            ..Default::default()
        },
        |ui| {
            app.shortcuts(ui.ctx());
            app.undo_dialog(ui.ctx());
        },
    );
    let at_cursor = app.undo_dialog.preview_for_tests().expect("a preview");
    assert_eq!(at_cursor, "dos\nuno\ntres\ncuatro\ncinco\n");
    // Letting go: back to where the line was; Ctrl+Enter does just that.
    frame(&mut app, &ctx, vec![]);
    assert_eq!(app.undo_dialog.preview_for_tests().unwrap(), preview);
    frame(&mut app, &ctx, vec![key(Key::Enter, egui::Modifiers::CTRL)]);
    assert_eq!(app.buffer.as_str(), preview);
    // And now that it is back, the preview says nothing would change.
    frame(&mut app, &ctx, vec![]);
    assert!(app.undo_dialog.preview_for_tests().is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn ctrl_enter_on_the_line_being_typed_adds_nothing() {
    // The history opens on the newest step, the line being typed: its
    // shorter version before that word is not a line missing now.
    let root = temp_root("typing");
    let (mut app, ctx) = app_with(&root, "## Hoy\n- [ ] comprar pan\n");
    app.buffer.move_to_bottom();
    for ch in "- [ ] renovar dni".chars() {
        app.buffer.type_text(&ch.to_string());
    }
    app.dirty = true;
    let before = app.buffer.as_str().to_string();
    open_graph(&mut app, &ctx);
    frame(&mut app, &ctx, vec![key(Key::Enter, egui::Modifiers::CTRL)]);
    assert_eq!(app.buffer.as_str(), before, "nothing appended at the end");
    assert!(app.status.starts_with("Nothing to bring back"), "{}", app.status);
    // Every older step of that line: still nothing to bring.
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![plain(Key::ArrowDown)]);
        frame(&mut app, &ctx, vec![key(Key::Enter, egui::Modifiers::CTRL)]);
        assert_eq!(app.buffer.as_str(), before);
    }
    // Picked on purpose, an older version does come, after the new one.
    frame(&mut app, &ctx, vec![plain(Key::Home)]);
    frame(&mut app, &ctx, vec![key(Key::ArrowDown, egui::Modifiers::SHIFT)]);
    frame(&mut app, &ctx, vec![key(Key::Enter, egui::Modifiers::CTRL)]);
    assert_eq!(
        app.buffer.as_str(),
        "## Hoy\n- [ ] comprar pan\n- [ ] renovar dni\n- [ ] renovar"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_line_deleted_before_typing_elsewhere_comes_back_in_place() {
    let root = temp_root("place");
    let (mut app, ctx) = app_with(
        &root,
        "## Hoy\n- [ ] comprar pan\n- [ ] comprar leche\n- [x] llamar al banco\n",
    );
    let at = app.buffer.as_str().find("- [ ] comprar leche").unwrap();
    app.buffer.set_cursor(at);
    assert!(app.buffer.delete_current_line());
    app.buffer.move_to_bottom();
    for ch in "- [ ] renovar dni".chars() {
        app.buffer.type_text(&ch.to_string());
    }
    app.dirty = true;
    open_graph(&mut app, &ctx);
    // The oldest state is the start; the one above it, the deletion.
    frame(&mut app, &ctx, vec![plain(Key::End)]);
    frame(&mut app, &ctx, vec![plain(Key::ArrowUp)]);
    frame(&mut app, &ctx, vec![key(Key::Enter, egui::Modifiers::CTRL)]);
    assert_eq!(
        app.buffer.as_str(),
        "## Hoy\n- [ ] comprar pan\n- [ ] comprar leche\n- [x] llamar al banco\n- [ ] renovar dni"
    );
    let _ = fs::remove_dir_all(root);
}

