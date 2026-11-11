//! `:undo` / Ctrl+Shift+Z: the undo history as a graph. Every state the
//! document went through, newest first, says what changed; the diff above
//! shows what the selected step did, or (Tab) how that state differs from
//! the text now. Enter goes there. Ctrl+Enter brings its lines that are
//! missing now back where they were, Ctrl+Shift+Enter at the cursor, and
//! Ctrl+O opens that state in a new tab. Typing searches the changes.
//!
//! The editor does not move while browsing: only Enter and the bring-back
//! keys change the document, each as one more undoable step.
use super::*;
use crate::history::{History, Kind, Walker, NONE};
use crate::line_diff::{Diff, Op};

const ROW_HEIGHT: f32 = 22.0;
const DIFF_ROW_HEIGHT: f32 = 18.0;
const LANE_WIDTH: f32 = 14.0;
const MAX_LANES_SHOWN: usize = 10;
const ORANGE: Color32 = Color32::from_rgb(208, 135, 112);
const PURPLE: Color32 = Color32::from_rgb(180, 142, 173);
const BRANCH: Color32 = Color32::from_rgb(76, 86, 106);
const DELETE_FILL: Color32 = Color32::from_rgb(61, 38, 45);
const INSERT_FILL: Color32 = Color32::from_rgb(37, 54, 44);
const DELETE_STRONG: Color32 = Color32::from_rgb(110, 52, 62);
const INSERT_STRONG: Color32 = Color32::from_rgb(58, 92, 64);

/// Footer actions, in the order `dialog_ui::show` reports clicks.
const ACTIONS: &[(&str, &str)] = &[
    ("Esc", "close"),
    ("↑↓", "state"),
    ("←→", "hunk"),
    ("Enter", "go there"),
    ("Ctrl+Enter", "bring back"),
    ("Ctrl+Shift+Enter", "bring to cursor"),
    ("Ctrl+O", "open in tab"),
    ("Tab", "this step / vs now"),
];

#[derive(Default)]
pub(super) struct UndoDialog {
    pub(super) open: bool,
    /// The selected state (its stable id).
    selected: u64,
    query: String,
    /// The diff compares the selected state with the text now, instead of
    /// showing what its step did.
    vs_now: bool,
    /// The diff hunk Ctrl+Enter acts on.
    hunk: usize,
    /// Lines picked inside the diff (indices into `View::bring`); empty: the
    /// whole focused hunk.
    picked: Vec<usize>,
    reveal_row: bool,
    reveal_hunk: bool,
    list_scroll: (f32, f32),
    diff_scroll: (f32, f32),
    rows: Option<Rows>,
    view: Option<View>,
    /// What Ctrl+Enter would do to the text now.
    preview: Option<Preview>,
    reveal_preview: bool,
    preview_scroll: (f32, f32),
    walker: Option<(u64, Walker)>,
}

/// The graph, one row per state, newest first.
struct Rows {
    key: (u64, u64, String),
    rows: Vec<Row>,
    lanes: usize,
    branches: usize,
}

struct Row {
    node: u32,
    lane: usize,
    /// Edges crossing the top of the row and the bottom of it.
    above: Vec<Lane>,
    below: Vec<Lane>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Lane {
    /// The older state this edge goes to (NONE: a free lane).
    to: u32,
    /// The newer state it comes from.
    from: u32,
}

const FREE: Lane = Lane { to: NONE, from: NONE };

#[cfg(test)]
impl UndoDialog {
    pub(super) fn vs_now_for_tests(&self) -> bool {
        self.vs_now
    }

    /// The text the preview shows after Ctrl+Enter, or why nothing changes.
    pub(super) fn preview_for_tests(&self) -> Result<String, &'static str> {
        match &self.preview.as_ref().expect("preview").result {
            Ok(diff) => Ok(diff.new.clone()),
            Err(why) => Err(why),
        }
    }

    pub(super) fn rows_for_tests(&self) -> usize {
        self.rows.as_ref().map_or(0, |rows| rows.rows.len())
    }
}

#[cfg(test)]
impl SlateApp {
    pub(super) fn undo_step_for_tests(&mut self, step: isize) {
        self.refresh_undo_rows();
        self.undo_step(step);
    }

    pub(super) fn toggle_undo_view_for_tests(&mut self) {
        self.undo_dialog.vs_now = !self.undo_dialog.vs_now;
        self.undo_selection_changed();
    }
}

/// The diff of the selected state and what Ctrl+Enter can bring from it.
struct View {
    key: ViewKey,
    diff: Diff,
    bring: Vec<Bring>,
    /// Shown when there is no diff.
    empty: &'static str,
}

/// The text now with the lines Ctrl+Enter would bring, or why it would
/// bring none.
struct Preview {
    key: (ViewKey, usize, Vec<usize>, bool, usize),
    result: Result<Diff, &'static str>,
    cursor_line: usize,
}

type ViewKey = (u64, u64, u64, u64, bool);

/// A line of the diff that is missing from the document now.
struct Bring {
    hunk: usize,
    line: usize,
    /// The line of the document it goes before.
    at: usize,
    text: String,
    /// The document has another version of it in its place (typed on, or
    /// edited since): only picking it brings it, never the whole hunk.
    changed: bool,
}

impl SlateApp {
    pub(super) fn open_undo_dialog(&mut self) {
        let dialog = &mut self.undo_dialog;
        dialog.open = true;
        dialog.selected = self.buffer.history().current_seq();
        dialog.query.clear();
        dialog.vs_now = false;
        dialog.hunk = 0;
        dialog.picked.clear();
        dialog.reveal_row = true;
        dialog.reveal_hunk = true;
        self.focus_editor_once = false;
    }

    fn close_undo_dialog(&mut self) {
        self.undo_dialog.open = false;
        self.undo_dialog.view = None;
        self.undo_dialog.preview = None;
        self.undo_dialog.walker = None;
        self.focus_editor_once = true;
    }

    /// Keys while the undo history is open; true when it took them.
    pub(super) fn handle_undo_keys(&mut self, ctx: &egui::Context) -> bool {
        if !self.undo_dialog.open {
            return false;
        }
        let mut close = false;
        let (mut go, mut bring, mut bring_to_cursor, mut open_tab, mut toggle) =
            (false, false, false, false, false);
        let (mut step, mut hunk_step, mut pick_step) = (0isize, 0isize, 0isize);
        let mut query_changed = false;
        ctx.input_mut(|input| {
            close |= input.consume_key(egui::Modifiers::NONE, Key::Escape);
            // Most specific first: Shift and Alt extras match plain keys too.
            bring_to_cursor |=
                input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::Enter);
            bring |= input.consume_key(egui::Modifiers::CTRL, Key::Enter);
            go |= input.consume_key(egui::Modifiers::NONE, Key::Enter);
            open_tab |= input.consume_key(egui::Modifiers::CTRL, Key::O);
            toggle |= input.consume_key(egui::Modifiers::NONE, Key::Tab);
            if input.consume_key(egui::Modifiers::SHIFT, Key::ArrowUp) {
                pick_step -= 1;
            }
            if input.consume_key(egui::Modifiers::SHIFT, Key::ArrowDown) {
                pick_step += 1;
            }
            for (key, delta) in [
                (Key::ArrowUp, -1),
                (Key::ArrowDown, 1),
                (Key::PageUp, -10),
                (Key::PageDown, 10),
                (Key::Home, isize::MIN / 2),
                (Key::End, isize::MAX / 2),
            ] {
                if input.consume_key(egui::Modifiers::NONE, key) {
                    step = step.saturating_add(delta);
                }
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowLeft) {
                hunk_step -= 1;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowRight) {
                hunk_step += 1;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Backspace) {
                query_changed |= self.undo_dialog.query.pop().is_some();
            }
            for event in &input.events {
                if let egui::Event::Text(text) = event {
                    if !text.chars().any(char::is_control) {
                        self.undo_dialog.query.push_str(text);
                        query_changed = true;
                    }
                }
            }
            // Nothing typed here reaches the document.
            input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                )
            });
        });
        if close {
            self.close_undo_dialog();
            return true;
        }
        self.refresh_undo_rows();
        if query_changed {
            // The newest match is selected.
            if let Some(node) = self.undo_rows().first().map(|row| row.node) {
                self.undo_dialog.selected = self.buffer.history().node(node).seq;
                self.undo_selection_changed();
            }
        }
        if step != 0 {
            self.undo_step(step);
        }
        if toggle {
            self.undo_dialog.vs_now = !self.undo_dialog.vs_now;
            self.undo_selection_changed();
        }
        self.refresh_undo_view();
        if hunk_step != 0 {
            self.undo_hunk_step(hunk_step);
        }
        if pick_step != 0 {
            self.undo_pick_step(pick_step);
        }
        if go {
            self.go_to_selected_state();
        } else if bring_to_cursor {
            self.bring_back_lines(true);
        } else if bring {
            self.bring_back_lines(false);
        } else if open_tab {
            self.open_selected_state_in_tab();
        }
        true
    }

    fn undo_rows(&self) -> &[Row] {
        self.undo_dialog
            .rows
            .as_ref()
            .map_or(&[][..], |rows| rows.rows.as_slice())
    }

    fn selected_row_position(&self) -> Option<usize> {
        let history = self.buffer.history();
        let selected = history.index_of(self.undo_dialog.selected)?;
        self.undo_rows().iter().position(|row| row.node == selected)
    }

    fn undo_step(&mut self, step: isize) {
        let rows = self.undo_rows();
        if rows.is_empty() {
            return;
        }
        let position = self.selected_row_position().unwrap_or(0) as isize;
        let target = position.saturating_add(step).clamp(0, rows.len() as isize - 1) as usize;
        let node = rows[target].node;
        self.undo_dialog.selected = self.buffer.history().node(node).seq;
        self.undo_selection_changed();
    }

    fn undo_selection_changed(&mut self) {
        let dialog = &mut self.undo_dialog;
        dialog.hunk = 0;
        dialog.picked.clear();
        dialog.reveal_row = true;
        dialog.reveal_hunk = true;
    }

    fn undo_hunk_step(&mut self, step: isize) {
        let Some(view) = &self.undo_dialog.view else {
            return;
        };
        let hunks = view.diff.hunks.len();
        if hunks == 0 {
            return;
        }
        let dialog = &mut self.undo_dialog;
        dialog.hunk = (dialog.hunk as isize + step).clamp(0, hunks as isize - 1) as usize;
        dialog.picked.clear();
        dialog.reveal_hunk = true;
    }

    /// Shift+↑↓: one line picked among the lines that can come back.
    fn undo_pick_step(&mut self, step: isize) {
        let Some(view) = &self.undo_dialog.view else {
            return;
        };
        if view.bring.is_empty() {
            return;
        }
        let next = match self.undo_dialog.picked.last() {
            Some(&last) => (last as isize + step).clamp(0, view.bring.len() as isize - 1) as usize,
            None => view
                .bring
                .iter()
                .position(|bring| bring.hunk == self.undo_dialog.hunk)
                .unwrap_or(0),
        };
        let hunk = view.bring[next].hunk;
        let dialog = &mut self.undo_dialog;
        dialog.picked = vec![next];
        dialog.hunk = hunk;
        dialog.reveal_hunk = true;
    }

    /// The graph rows for the active document (and the search).
    fn refresh_undo_rows(&mut self) {
        let history = self.buffer.history();
        let key = (
            self.buffer.identity,
            history.version,
            self.undo_dialog.query.trim().to_lowercase(),
        );
        if self.undo_dialog.rows.as_ref().is_some_and(|rows| rows.key == key) {
            return;
        }
        let rows = if key.2.is_empty() {
            graph_rows(history)
        } else {
            let rows = (0..history.len() as u32)
                .rev()
                .filter(|index| state_matches(history, *index, &key.2))
                .map(|node| Row {
                    node,
                    lane: 0,
                    above: Vec::new(),
                    below: Vec::new(),
                })
                .collect();
            (rows, 1)
        };
        let branches = history.nodes.iter().filter(|node| node.children == 0).count();
        self.undo_dialog.rows = Some(Rows {
            key,
            lanes: rows.1,
            rows: rows.0,
            branches,
        });
        if history.index_of(self.undo_dialog.selected).is_none() {
            self.undo_dialog.selected = history.current_seq();
        }
    }

    /// The diff of the selected state, and the lines it can bring back.
    fn refresh_undo_view(&mut self) {
        let history = self.buffer.history();
        let selected = match history.index_of(self.undo_dialog.selected) {
            Some(index) => index,
            None => {
                self.undo_dialog.selected = history.current_seq();
                history.current
            }
        };
        let key = (
            self.buffer.identity,
            history.version,
            history.current_seq(),
            self.undo_dialog.selected,
            self.undo_dialog.vs_now,
        );
        if self.undo_dialog.view.as_ref().is_some_and(|view| view.key == key) {
            return;
        }
        let now = self.buffer.as_str();
        if !matches!(&self.undo_dialog.walker, Some((identity, _)) if *identity == self.buffer.identity)
        {
            self.undo_dialog.walker = Some((self.buffer.identity, Walker::new()));
        }
        let walker = &mut self.undo_dialog.walker.as_mut().expect("walker").1;
        history.walk(walker, now, selected);
        let version = walker.text();
        let node = history.node(selected);
        let mut bring = Vec::new();
        let (diff, empty) = if self.undo_dialog.vs_now {
            let diff = Diff::new(now.to_string(), version, 3);
            let inserted = inserted_lines(&diff);
            for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
                for (line_index, line) in hunk.lines.iter().enumerate() {
                    if line.op == Op::Insert {
                        let (at, changed) = inserted.get(&line.new).copied().unwrap_or((line.old, false));
                        bring.push(Bring {
                            hunk: hunk_index,
                            line: line_index,
                            at,
                            text: diff.new_line(line.new).to_string(),
                            changed,
                        });
                    }
                }
            }
            (diff, "this is the text now")
        } else if node.parent == NONE {
            (
                Diff::new(version.clone(), version, 3),
                "the start of this history: the text as it was opened",
            )
        } else {
            let mut parent = version.clone();
            parent.replace_range(node.start..node.start + node.inserted.len(), &node.removed);
            let diff = Diff::new(parent.clone(), version.clone(), 3);
            // What the step added, if it is missing now; else what it
            // removed. A line the document has a newer version of (the
            // step typed part of it, say) is not missing, only pickable.
            let sides: Vec<Vec<Bring>> = [(Op::Insert, version), (Op::Delete, parent)]
                .into_iter()
                .map(|(side, text)| {
                    let missing = missing_lines(now, text);
                    let mut lines = Vec::new();
                    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
                        for (line_index, line) in hunk.lines.iter().enumerate() {
                            let number = if side == Op::Insert { line.new } else { line.old };
                            if line.op != side {
                                continue;
                            }
                            if let Some(&(at, changed)) = missing.get(&number) {
                                lines.push(Bring {
                                    hunk: hunk_index,
                                    line: line_index,
                                    at,
                                    text: diff.text(line).to_string(),
                                    changed,
                                });
                            }
                        }
                    }
                    lines
                })
                .collect();
            let missing = |lines: &Vec<Bring>| {
                lines
                    .iter()
                    .any(|line| !line.changed && !line.text.trim().is_empty())
            };
            bring = sides
                .iter()
                .position(missing)
                .or_else(|| sides.iter().position(|lines| !lines.is_empty()))
                .map(|side| sides.into_iter().nth(side).unwrap_or_default())
                .unwrap_or_default();
            (diff, "this step changed nothing")
        };
        let hunks = diff.hunks.len();
        let dialog = &mut self.undo_dialog;
        dialog.hunk = dialog.hunk.min(hunks.saturating_sub(1));
        dialog.picked.retain(|pick| *pick < bring.len());
        dialog.view = Some(View {
            key,
            diff,
            bring,
            empty,
        });
    }

    /// The lines Ctrl+Enter acts on: the picked ones, else the focused hunk's.
    fn undo_targets(&self) -> Vec<&Bring> {
        let Some(view) = &self.undo_dialog.view else {
            return Vec::new();
        };
        if !self.undo_dialog.picked.is_empty() {
            let mut picked = self.undo_dialog.picked.clone();
            picked.sort_unstable();
            picked.dedup();
            return picked.iter().filter_map(|index| view.bring.get(*index)).collect();
        }
        view.bring
            .iter()
            .filter(|bring| bring.hunk == self.undo_dialog.hunk && !bring.changed)
            .collect()
    }

    fn selected_state_age(&self) -> String {
        let history = self.buffer.history();
        history
            .index_of(self.undo_dialog.selected)
            .map(|index| ago(history.node(index).at))
            .unwrap_or_default()
    }

    /// The document changed under the history view: what every edit does.
    fn after_history_change(&mut self) {
        self.dirty = !self.buffer.at_saved_state();
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.notify_modules_edited();
    }

    fn go_to_selected_state(&mut self) {
        let age = self.selected_state_age();
        if self.buffer.jump_to(self.undo_dialog.selected) {
            self.after_history_change();
            self.status = format!("Went to the text from {age} · nothing was lost, Ctrl+Shift+Z shows it all");
        }
        self.close_undo_dialog();
    }

    /// What Ctrl+Enter (or Ctrl+Shift+Enter, `at_cursor`) would insert now:
    /// `(line it goes before, text)`, or why nothing would. The preview and
    /// the action both use this, so the preview is what happens.
    fn bring_plan(&self, at_cursor: bool) -> Result<Vec<(usize, String)>, &'static str> {
        let cursor_line = self.buffer.line_index_for_byte(self.buffer.cursor());
        let targets: Vec<(usize, String)> = self
            .undo_targets()
            .into_iter()
            .map(|bring| (if at_cursor { cursor_line } else { bring.at }, bring.text.clone()))
            .collect();
        // Blank lines come along with others, not on their own (unless picked).
        let only_blank = self.undo_dialog.picked.is_empty()
            && targets.iter().all(|(_, text)| text.trim().is_empty());
        if !targets.is_empty() && !only_blank {
            return Ok(targets);
        }
        let bring = self.undo_dialog.view.as_ref().map(|view| &view.bring);
        Err(if bring.is_none_or(|bring| bring.is_empty()) {
            "Nothing to bring back: the document already has those lines"
        } else if bring.is_some_and(|bring| bring.iter().all(|line| line.changed)) {
            "Nothing to bring back: the document has newer versions of those lines · Shift+↑↓ picks one anyway"
        } else if only_blank && !targets.is_empty() {
            "Nothing to bring back here but blank lines · Shift+↑↓ picks one"
        } else {
            "Nothing to bring back in this hunk (←→ moves to another)"
        })
    }

    /// The text now with what Ctrl+Enter would bring, as a diff: where each
    /// line lands, among the lines around it.
    fn refresh_undo_preview(&mut self, at_cursor: bool) {
        let Some(view) = &self.undo_dialog.view else {
            return;
        };
        let cursor_line = self.buffer.line_index_for_byte(self.buffer.cursor());
        let key = (
            view.key,
            self.undo_dialog.hunk,
            self.undo_dialog.picked.clone(),
            at_cursor,
            if at_cursor { cursor_line } else { 0 },
        );
        if self.undo_dialog.preview.as_ref().is_some_and(|preview| preview.key == key) {
            return;
        }
        let result = self.bring_plan(at_cursor).map(|targets| {
            let now = self.buffer.as_str();
            let (text, _) = insert_lines(now, &targets);
            Diff::new(now.to_string(), text, 2)
        });
        self.undo_dialog.preview = Some(Preview {
            key,
            result,
            cursor_line,
        });
        self.undo_dialog.reveal_preview = true;
    }

    /// Ctrl+Enter: the target lines go back where they were;
    /// Ctrl+Shift+Enter: they go in at the cursor's line.
    fn bring_back_lines(&mut self, at_cursor: bool) {
        let targets = match self.bring_plan(at_cursor) {
            Ok(targets) => targets,
            Err(why) => {
                self.status = why.to_string();
                return;
            }
        };
        let count = targets.len();
        let (text, cursor) = insert_lines(self.buffer.as_str(), &targets);
        let age = self.selected_state_age();
        if self
            .buffer
            .replace_text_as(&text, Kind::BroughtBack, Some(cursor))
        {
            self.after_history_change();
            self.undo_dialog.picked.clear();
            self.status = format!(
                "Brought back {count} line{} from {age} · Ctrl+Z undoes it",
                if count == 1 { "" } else { "s" }
            );
        }
    }

    fn open_selected_state_in_tab(&mut self) {
        let history = self.buffer.history();
        let Some(index) = history.index_of(self.undo_dialog.selected) else {
            return;
        };
        let text = history.text_of(self.buffer.as_str(), index);
        let age = ago(history.node(index).at);
        let name = tabs::file_label(self.path.as_deref());
        self.close_undo_dialog();
        self.new_tab();
        self.buffer.set_text(text);
        self.buffer.set_cursor(0);
        self.dirty = true;
        self.focus_editor_once = true;
        self.status = format!("{name} as it was {age}, in a new untitled tab");
    }

    pub(super) fn undo_dialog(&mut self, ctx: &egui::Context) {
        if !self.undo_dialog.open {
            return;
        }
        self.refresh_undo_rows();
        self.refresh_undo_view();
        // Holding Ctrl+Shift previews Ctrl+Shift+Enter: at the cursor.
        let at_cursor = ctx.input(|input| input.modifiers.ctrl && input.modifiers.shift);
        self.refresh_undo_preview(at_cursor);
        let history = self.buffer.history();
        let current = history.current;
        let saved = history.saved_seq();
        let mut on_path = vec![false; history.len()];
        let mut walk = current;
        while walk != NONE {
            on_path[walk as usize] = true;
            walk = history.node(walk).parent;
        }
        let rows = self.undo_dialog.rows.as_ref().expect("rows");
        let view = self.undo_dialog.view.as_ref().expect("view");
        let vs_now = self.undo_dialog.vs_now;
        let subtitle = format!(
            "{} · {} state{} · {} branch{} · tab: {}",
            tabs::file_label(self.path.as_deref()),
            history.len(),
            if history.len() == 1 { "" } else { "s" },
            rows.branches,
            if rows.branches == 1 { "" } else { "es" },
            if vs_now { "this step" } else { "vs now" },
        );
        let selected_position = rows.rows.iter().position(|row| {
            history.node(row.node).seq == self.undo_dialog.selected
        });
        let targets: Vec<(usize, usize)> = self
            .undo_targets()
            .iter()
            .map(|bring| (bring.hunk, bring.line))
            .collect();
        let mut clicked_row: Option<(u32, bool)> = None;
        let mut clicked_line: Option<(usize, egui::Modifiers)> = None;
        let mut list_scroll = self.undo_dialog.list_scroll;
        let mut diff_scroll = self.undo_dialog.diff_scroll;
        let reveal_row = std::mem::take(&mut self.undo_dialog.reveal_row);
        let reveal_hunk = std::mem::take(&mut self.undo_dialog.reveal_hunk);
        let reveal_preview = std::mem::take(&mut self.undo_dialog.reveal_preview);
        let mut preview_scroll = self.undo_dialog.preview_scroll;
        let preview = self.undo_dialog.preview.as_ref().expect("preview");
        let preview_title = match (&preview.result, at_cursor) {
            (Err(_), _) => "Ctrl+Enter: nothing would change".to_string(),
            (Ok(_), false) => {
                "Ctrl+Enter puts them here · hold Ctrl+Shift: at the cursor".to_string()
            }
            (Ok(_), true) => format!(
                "Ctrl+Shift+Enter puts them at the cursor (line {})",
                preview.cursor_line + 1
            ),
        };
        let focused_hunk = self.undo_dialog.hunk;
        let query = self.undo_dialog.query.clone();
        let clicked = dialog_ui::show(
            ctx,
            "undo_dialog".into(),
            "undo history",
            &subtitle,
            ACTIONS,
            |ui| {
                let body = ui.max_rect();
                ui.painter()
                    .rect_filled(body, 0.0, Color32::from_rgb(22, 28, 37));
                let diff_height = (body.height() * 0.46).max(DIFF_ROW_HEIGHT * 4.0);
                let diff_rect =
                    egui::Rect::from_min_size(body.min, Vec2::new(body.width(), diff_height));
                let search_rect = egui::Rect::from_min_size(
                    egui::pos2(body.left(), diff_rect.bottom() + 1.0),
                    Vec2::new(body.width(), 26.0),
                );
                let list_rect =
                    egui::Rect::from_min_max(egui::pos2(body.left(), search_rect.bottom()), body.max);
                ui.painter().line_segment(
                    [diff_rect.left_bottom(), diff_rect.right_bottom()],
                    Stroke::new(1.0_f32, Color32::from_rgb(52, 62, 78)),
                );

                // The step's diff on the left, what Ctrl+Enter would do to
                // the text now on the right.
                let half = (diff_rect.width() / 2.0).floor();
                let step_rect = diff_rect.with_max_x(diff_rect.left() + half);
                let preview_rect = diff_rect.with_min_x(step_rect.right() + 1.0);
                ui.painter().line_segment(
                    [step_rect.right_top(), step_rect.right_bottom()],
                    Stroke::new(1.0_f32, Color32::from_rgb(52, 62, 78)),
                );
                let pane_header = |ui: &egui::Ui, rect: egui::Rect, title: &str| {
                    let header = rect.with_max_y(rect.top() + DIFF_ROW_HEIGHT + 4.0);
                    ui.painter()
                        .rect_filled(header, 0.0, Color32::from_rgb(28, 35, 46));
                    ui.painter().text(
                        header.left_center() + Vec2::new(10.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        SlateApp::text_for_width(title, header.width() - 20.0, 12.0),
                        FontId::monospace(12.0),
                        notice_ui::DIM,
                    );
                    rect.with_min_y(header.bottom())
                };
                let step_title = if view.diff.is_empty() {
                    "this state"
                } else if vs_now {
                    "from now to this state · green can come back"
                } else {
                    "what this step did"
                };
                let step_body = pane_header(ui, step_rect, step_title);
                let mut diff_ui = ui.new_child(egui::UiBuilder::new().max_rect(step_body));
                diff_ui.spacing_mut().item_spacing.y = 0.0;
                draw_diff(
                    &mut diff_ui,
                    DiffPane {
                        id: "undo_diff",
                        diff: Some(&view.diff),
                        empty: view.empty,
                        bring: &view.bring,
                        focused_hunk: Some(focused_hunk),
                        targets: &targets,
                        numbers_after: false,
                    },
                    reveal_hunk,
                    &mut diff_scroll,
                    &mut clicked_line,
                );
                let preview_body = pane_header(ui, preview_rect, &preview_title);
                let mut preview_ui = ui.new_child(egui::UiBuilder::new().max_rect(preview_body));
                preview_ui.spacing_mut().item_spacing.y = 0.0;
                let (preview_diff, preview_empty) = match &preview.result {
                    Ok(diff) => (Some(diff), ""),
                    Err(why) => (None, *why),
                };
                draw_diff(
                    &mut preview_ui,
                    DiffPane {
                        id: "undo_preview",
                        diff: preview_diff,
                        empty: preview_empty,
                        bring: &[],
                        focused_hunk: None,
                        targets: &[],
                        numbers_after: true,
                    },
                    reveal_preview,
                    &mut preview_scroll,
                    &mut None,
                );

                // The search line.
                let painter = ui.painter_at(search_rect);
                let (text, color) = if query.is_empty() {
                    ("type to search what changed".to_string(), notice_ui::DIM)
                } else {
                    (format!("search: {query}▏"), notice_ui::ACCENT)
                };
                painter.text(
                    search_rect.left_center() + Vec2::new(10.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    text,
                    FontId::monospace(13.0),
                    color,
                );
                if !query.is_empty() {
                    painter.text(
                        search_rect.right_center() - Vec2::new(10.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        match rows.rows.len() {
                            1 => "1 change".to_string(),
                            n => format!("{n} changes"),
                        },
                        FontId::monospace(12.0),
                        notice_ui::DIM,
                    );
                }

                // The graph.
                let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
                // Rows touch, so the graph's lines join.
                list_ui.spacing_mut().item_spacing.y = 0.0;
                if rows.rows.is_empty() {
                    list_ui.painter().text(
                        list_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "no change matches the search",
                        FontId::monospace(13.0),
                        notice_ui::DIM,
                    );
                    return;
                }
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("undo_list")
                    .auto_shrink([false, false]);
                if let (true, Some(position)) = (reveal_row, selected_position) {
                    let (offset, height) = list_scroll;
                    let top = position as f32 * ROW_HEIGHT;
                    if top < offset || top + ROW_HEIGHT > offset + height {
                        scroll = scroll.vertical_scroll_offset((top - height / 2.0).max(0.0));
                    }
                }
                let lanes_width =
                    16.0 + rows.lanes.min(MAX_LANES_SHOWN) as f32 * LANE_WIDTH;
                let output = scroll.show_rows(&mut list_ui, ROW_HEIGHT, rows.rows.len(), |ui, range| {
                    for position in range {
                        let row = &rows.rows[position];
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), ROW_HEIGHT),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            clicked_row = Some((row.node, false));
                        }
                        if response.double_clicked() {
                            clicked_row = Some((row.node, true));
                        }
                        let node = history.node(row.node);
                        let selected = Some(position) == selected_position;
                        let painter = ui.painter_at(rect);
                        if selected {
                            painter.rect_filled(rect, 0.0, Color32::from_rgb(38, 47, 61));
                        } else if response.hovered() {
                            painter.rect_filled(rect, 0.0, Color32::from_rgb(30, 37, 48));
                        }
                        draw_graph_cell(&painter, rect, row, &on_path, current, saved, history);
                        let text_left = rect.left() + lanes_width;
                        let age = painter.text(
                            egui::pos2(text_left, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("{:<9}", ago(node.at)),
                            FontId::monospace(12.0),
                            notice_ui::DIM,
                        );
                        let mut tags = Vec::new();
                        if row.node == current {
                            tags.push(("● here", ORANGE));
                        }
                        if saved == Some(node.seq) {
                            tags.push(("saved", notice_ui::GREEN));
                        }
                        let tags_width = tags
                            .iter()
                            .map(|(tag, _)| tag.chars().count() as f32 * 7.6 + 12.0)
                            .sum::<f32>();
                        let summary_left = age.right() + 12.0;
                        let summary = SlateApp::text_for_width(
                            &summary(node),
                            (rect.right() - summary_left - tags_width - 8.0).max(20.0),
                            13.0,
                        );
                        let summary_rect = painter.text(
                            egui::pos2(summary_left, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            summary,
                            FontId::monospace(13.0),
                            if selected || on_path[row.node as usize] {
                                notice_ui::TEXT
                            } else {
                                notice_ui::DIM
                            },
                        );
                        let mut x = summary_rect.right() + 12.0;
                        for (tag, color) in tags {
                            let tag_rect = painter.text(
                                egui::pos2(x, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                tag,
                                FontId::monospace(12.0),
                                color,
                            );
                            x = tag_rect.right() + 10.0;
                        }
                    }
                });
                list_scroll = (output.state.offset.y, output.inner_rect.height());
            },
        );
        self.undo_dialog.list_scroll = list_scroll;
        self.undo_dialog.diff_scroll = diff_scroll;
        self.undo_dialog.preview_scroll = preview_scroll;
        if let Some((node, double)) = clicked_row {
            self.undo_dialog.selected = self.buffer.history().node(node).seq;
            self.undo_selection_changed();
            self.undo_dialog.reveal_row = false;
            if double {
                self.go_to_selected_state();
                return;
            }
        }
        if let Some((line, modifiers)) = clicked_line {
            self.pick_diff_line(line, modifiers);
        }
        let action = |n: usize| clicked.get(n).copied().unwrap_or(false);
        if action(0) {
            self.close_undo_dialog();
        } else if action(3) {
            self.go_to_selected_state();
        } else if action(4) {
            self.bring_back_lines(false);
        } else if action(5) {
            self.bring_back_lines(true);
        } else if action(6) {
            self.open_selected_state_in_tab();
        } else if action(7) {
            self.undo_dialog.vs_now = !self.undo_dialog.vs_now;
            self.undo_selection_changed();
        } else if action(2) {
            self.undo_hunk_step(1);
        } else if action(1) {
            self.undo_step(1);
        }
    }

    /// A click on a line that can come back picks it (Ctrl adds or removes
    /// it, Shift picks the run up to it).
    fn pick_diff_line(&mut self, bring_index: usize, modifiers: egui::Modifiers) {
        let dialog = &mut self.undo_dialog;
        let Some(view) = &dialog.view else {
            return;
        };
        if modifiers.ctrl {
            if let Some(position) = dialog.picked.iter().position(|pick| *pick == bring_index) {
                dialog.picked.remove(position);
            } else {
                dialog.picked.push(bring_index);
            }
        } else if let (true, Some(&anchor)) = (modifiers.shift, dialog.picked.last()) {
            let (from, to) = (anchor.min(bring_index), anchor.max(bring_index));
            dialog.picked = (from..=to).collect();
        } else {
            dialog.picked = vec![bring_index];
        }
        if let Some(bring) = view.bring.get(bring_index) {
            dialog.hunk = bring.hunk;
        }
    }
}

/// One diff on screen: the step's (lines can be picked) or the preview.
struct DiffPane<'a> {
    id: &'static str,
    diff: Option<&'a Diff>,
    /// Shown instead when there is no diff, or it is empty.
    empty: &'a str,
    bring: &'a [Bring],
    focused_hunk: Option<usize>,
    targets: &'a [(usize, usize)],
    /// Line numbers of the text after the change (the preview: where the
    /// lines land), not before it.
    numbers_after: bool,
}

fn draw_diff(
    ui: &mut egui::Ui,
    pane: DiffPane,
    reveal_hunk: bool,
    scroll_state: &mut (f32, f32),
    clicked_line: &mut Option<(usize, egui::Modifiers)>,
) {
    let area = ui.max_rect();
    let Some(diff) = pane.diff.filter(|diff| !diff.is_empty()) else {
        let galley = ui.painter().layout(
            pane.empty.to_string(),
            FontId::monospace(13.0),
            notice_ui::DIM,
            (area.width() - 32.0).max(40.0),
        );
        let at = area.center() - galley.size() / 2.0;
        ui.painter().galley(at, galley, notice_ui::DIM);
        return;
    };
    let focused_hunk = pane.focused_hunk.unwrap_or(0);
    // Hunks one after another, a gap row between them.
    let mut rows: Vec<Option<(usize, usize)>> = Vec::new();
    let mut hunk_first_row = Vec::new();
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        if hunk_index > 0 {
            rows.push(None);
        }
        hunk_first_row.push(rows.len());
        rows.extend((0..hunk.lines.len()).map(|line| Some((hunk_index, line))));
    }
    let bring_index: std::collections::HashMap<(usize, usize), usize> = pane
        .bring
        .iter()
        .enumerate()
        .map(|(index, bring)| ((bring.hunk, bring.line), index))
        .collect();
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(pane.id)
        .auto_shrink([false, false]);
    if reveal_hunk {
        let (offset, height) = *scroll_state;
        let first = hunk_first_row.get(focused_hunk).copied().unwrap_or(0) as f32 * DIFF_ROW_HEIGHT;
        let last = diff
            .hunks
            .get(focused_hunk)
            .map_or(first, |hunk| first + hunk.lines.len() as f32 * DIFF_ROW_HEIGHT);
        if first < offset || last > offset + height {
            scroll = scroll.vertical_scroll_offset((first - DIFF_ROW_HEIGHT).max(0.0));
        }
    }
    let font = FontId::monospace(13.0);
    let output = scroll.show_rows(ui, DIFF_ROW_HEIGHT, rows.len(), |ui, range| {
        for row in &rows[range] {
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), DIFF_ROW_HEIGHT),
                egui::Sense::click(),
            );
            let painter = ui.painter_at(rect);
            let Some((hunk_index, line_index)) = *row else {
                painter.text(
                    rect.left_center() + Vec2::new(18.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "⋯",
                    font.clone(),
                    notice_ui::DIM,
                );
                continue;
            };
            let line = &diff.hunks[hunk_index].lines[line_index];
            let bring = bring_index.get(&(hunk_index, line_index)).copied();
            let targeted = pane.targets.contains(&(hunk_index, line_index));
            let fill = match (line.op, targeted) {
                (Op::Delete, false) => Some(DELETE_FILL),
                (Op::Insert, false) => Some(INSERT_FILL),
                (Op::Delete, true) => Some(DELETE_STRONG),
                (Op::Insert, true) => Some(INSERT_STRONG),
                (Op::Equal, _) => None,
            };
            if let Some(fill) = fill {
                painter.rect_filled(rect, 0.0, fill);
            }
            if pane.focused_hunk == Some(hunk_index) {
                painter.rect_filled(
                    egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
                    0.0,
                    notice_ui::ACCENT,
                );
            }
            if let (Some(index), true) = (bring, response.clicked()) {
                *clicked_line = Some((index, ui.input(|input| input.modifiers)));
            }
            let number = match line.op {
                Op::Insert => line.new,
                _ if pane.numbers_after => line.new,
                _ => line.old,
            } + 1;
            painter.text(
                egui::pos2(rect.left() + 46.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                number.to_string(),
                font.clone(),
                notice_ui::DIM,
            );
            let (sign, color) = match line.op {
                Op::Delete => ("-", notice_ui::RED),
                Op::Insert => ("+", notice_ui::GREEN),
                Op::Equal => (" ", notice_ui::TEXT),
            };
            painter.text(
                egui::pos2(rect.left() + 54.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                if targeted { "»" } else { sign },
                font.clone(),
                // Lines Ctrl+Enter brings stand out; changed ones only pick.
                if bring.is_some_and(|index| !pane.bring[index].changed) {
                    notice_ui::WARN
                } else {
                    color
                },
            );
            let text = diff.text(line);
            let text_left = rect.left() + 70.0;
            if let Some((start, end)) = line.changed {
                let width = |text: &str| {
                    ui.ctx().fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(text.replace('\t', "    "), font.clone(), color)
                            .size()
                            .x
                    })
                };
                let left = text_left + width(&text[..start]);
                let right = text_left + width(&text[..end]);
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(left, rect.top() + 2.0),
                        egui::pos2(right.max(left + 2.0), rect.bottom() - 2.0),
                    ),
                    2.0,
                    if line.op == Op::Delete {
                        Color32::from_rgb(130, 58, 70)
                    } else {
                        Color32::from_rgb(70, 112, 74)
                    },
                );
            }
            painter.text(
                egui::pos2(text_left, rect.center().y),
                egui::Align2::LEFT_CENTER,
                text.replace('\t', "    "),
                font.clone(),
                if line.op == Op::Equal {
                    Color32::from_rgb(170, 180, 196)
                } else {
                    color
                },
            );
        }
    });
    *scroll_state = (output.state.offset.y, output.inner_rect.height());
}

#[allow(clippy::too_many_arguments)]
fn draw_graph_cell(
    painter: &egui::Painter,
    rect: egui::Rect,
    row: &Row,
    on_path: &[bool],
    current: u32,
    saved: Option<u64>,
    history: &History,
) {
    let x = |lane: usize| rect.left() + 14.0 + lane.min(MAX_LANES_SHOWN) as f32 * LANE_WIDTH;
    let (top, middle, bottom) = (rect.top(), rect.center().y, rect.bottom());
    let color = |from: u32| {
        if from != NONE && on_path[from as usize] {
            notice_ui::ACCENT
        } else {
            BRANCH
        }
    };
    for (index, lane) in row.above.iter().enumerate() {
        if lane.to == NONE {
            continue;
        }
        let stroke = Stroke::new(1.5_f32, color(lane.from));
        if lane.to == row.node {
            // A newer state comes down into this one.
            painter.line_segment(
                [egui::pos2(x(index), top), egui::pos2(x(row.lane), middle)],
                stroke,
            );
        } else {
            painter.line_segment([egui::pos2(x(index), top), egui::pos2(x(index), bottom)], stroke);
        }
    }
    if row.below.get(row.lane).is_some_and(|lane| lane.from == row.node && lane.to != NONE) {
        painter.line_segment(
            [egui::pos2(x(row.lane), middle), egui::pos2(x(row.lane), bottom)],
            Stroke::new(1.5_f32, color(row.node)),
        );
    }
    let node = history.node(row.node);
    let center = egui::pos2(x(row.lane), middle);
    let fill = if row.node == current {
        ORANGE
    } else if saved == Some(node.seq) {
        notice_ui::GREEN
    } else if node.kind == Kind::External {
        PURPLE
    } else if node.kind == Kind::BroughtBack {
        notice_ui::WARN
    } else if on_path[row.node as usize] {
        notice_ui::ACCENT
    } else {
        Color32::from_rgb(120, 132, 150)
    };
    if row.node == current {
        painter.circle_stroke(center, 6.5, Stroke::new(1.5_f32, ORANGE));
    }
    painter.circle_filled(center, 4.0, fill);
}

/// Lays the tree out newest first, like `git log --graph`: a state continues
/// the lane of its newest child, the other children's lanes merge into it,
/// and a state nobody came from yet (a branch tip) opens a lane.
fn graph_rows(history: &History) -> (Vec<Row>, usize) {
    let mut lanes: Vec<Lane> = Vec::new();
    let mut rows = Vec::with_capacity(history.len());
    let mut widest = 1;
    for index in (0..history.len() as u32).rev() {
        let above = lanes.clone();
        let mut lane = None;
        for (slot, edge) in lanes.iter_mut().enumerate() {
            if edge.to == index {
                lane.get_or_insert(slot);
                *edge = FREE;
            }
        }
        let lane = lane.unwrap_or_else(|| {
            lanes.iter().position(|edge| edge.to == NONE).unwrap_or_else(|| {
                lanes.push(FREE);
                lanes.len() - 1
            })
        });
        let parent = history.node(index).parent;
        lanes[lane] = if parent == NONE {
            FREE
        } else {
            Lane {
                to: parent,
                from: index,
            }
        };
        while lanes.last() == Some(&FREE) {
            lanes.pop();
        }
        widest = widest.max(lanes.len()).max(lane + 1);
        rows.push(Row {
            node: index,
            lane,
            above,
            below: lanes.clone(),
        });
    }
    (rows, widest)
}

/// Whether state `index`'s change contains `query` (lowercase). Words typed
/// one step at a time are read together, so a typed phrase is found too.
fn state_matches(history: &History, index: u32, query: &str) -> bool {
    let node = history.node(index);
    if node.removed.to_lowercase().contains(query) {
        return true;
    }
    if node.kind != Kind::Typing {
        return node.inserted.to_lowercase().contains(query);
    }
    let mut typed = node.inserted.clone();
    let mut start = node.start;
    let mut at = index;
    while typed.len() < 200 {
        let parent = history.node(at).parent;
        if parent == NONE {
            break;
        }
        let previous = history.node(parent);
        if previous.kind != Kind::Typing
            || !previous.removed.is_empty()
            || previous.start + previous.inserted.len() != start
        {
            break;
        }
        typed.insert_str(0, &previous.inserted);
        start = previous.start;
        at = parent;
    }
    typed.to_lowercase().contains(query)
}

/// For every line of `version` the text `now` lacks, the line of `now` it
/// would go before, and whether `now` has a changed version of it there.
fn missing_lines(now: &str, version: String) -> std::collections::HashMap<usize, (usize, bool)> {
    inserted_lines(&Diff::new(now.to_string(), version, 0))
}

/// For each line the new side of `diff` has and the old side lacks: the
/// old side's line it goes before, and whether the old side has a changed
/// version of it (a line typed on, or edited) rather than nothing. Each
/// change block pairs its inserted lines with similar deleted ones; a line
/// with no pair goes after the pairs before it, keeping its place.
fn inserted_lines(diff: &Diff) -> std::collections::HashMap<usize, (usize, bool)> {
    let mut inserted = std::collections::HashMap::new();
    for hunk in &diff.hunks {
        for block in hunk.lines.split(|line| line.op == Op::Equal) {
            // Deletes come first in a block: either way the block's start.
            let Some(first) = block.first() else {
                continue;
            };
            let mut at = first.old;
            let mut deleted: Vec<(usize, Option<&str>)> = block
                .iter()
                .filter(|line| line.op == Op::Delete)
                .map(|line| (line.old, Some(diff.text(line))))
                .collect();
            for line in block.iter().filter(|line| line.op == Op::Insert) {
                let text = diff.text(line);
                let pair = deleted
                    .iter_mut()
                    .find(|(_, old)| old.is_some_and(|old| same_line_edited(old, text)));
                match pair {
                    Some((index, old)) => {
                        *old = None;
                        at = at.max(*index + 1);
                        inserted.insert(line.new, (at, true));
                    }
                    None => {
                        inserted.insert(line.new, (at, false));
                    }
                }
            }
        }
    }
    inserted
}

/// Two versions of one line: one inside the other (typed on, indented),
/// or mostly the same start and end past the list or heading marker (so
/// two different tasks are not one).
fn same_line_edited(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() || b.is_empty() {
        return a.is_empty() && b.is_empty();
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if long.contains(short) {
        return true;
    }
    let (a, b) = (line_body(a), line_body(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let prefix = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let suffix = a
        .chars()
        .rev()
        .zip(b.chars().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let longest = a.chars().count().max(b.chars().count());
    (prefix + suffix).min(longest) * 2 >= longest
}

/// A line without its Markdown markers: `- [ ] `, `1. `, `## `, `> `.
fn line_body(line: &str) -> &str {
    let mut body = line.trim();
    loop {
        let before = body;
        body = body.trim_start_matches(['#', '>']).trim_start();
        if let Some(rest) = body.strip_prefix(['-', '*', '+']) {
            if rest.is_empty() || rest.starts_with(' ') {
                body = rest.trim_start();
            }
        }
        let digits = body.len() - body.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 {
            if let Some(rest) = body[digits..].strip_prefix(['.', ')']) {
                body = rest.trim_start();
            }
        }
        for checkbox in ["[ ]", "[x]", "[X]"] {
            if let Some(rest) = body.strip_prefix(checkbox) {
                body = rest.trim_start();
            }
        }
        if body == before {
            return body;
        }
    }
}

/// `text` with each `(line, text)` inserted as a line before line `line`
/// (in order), and the byte just after the last line inserted.
fn insert_lines(text: &str, inserts: &[(usize, String)]) -> (String, usize) {
    let mut inserts: Vec<(usize, usize, &str)> = inserts
        .iter()
        .enumerate()
        .map(|(order, (at, line))| (*at, order, line.as_str()))
        .collect();
    inserts.sort();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = String::with_capacity(text.len() + inserts.iter().map(|i| i.2.len() + 1).sum::<usize>());
    let mut cursor = 0;
    let mut next = inserts.iter().peekable();
    for (index, line) in lines.iter().enumerate() {
        while let Some((_, _, inserted)) = next.next_if(|(at, _, _)| *at <= index) {
            out.push_str(inserted);
            cursor = out.len();
            out.push('\n');
        }
        out.push_str(line);
        if index + 1 < lines.len() {
            out.push('\n');
        }
    }
    for (_, _, inserted) in next {
        out.push('\n');
        out.push_str(inserted);
        cursor = out.len();
    }
    (out, cursor)
}

/// "5 min ago", for a history row.
fn ago(at: std::time::SystemTime) -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(at)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    match seconds {
        0..=4 => "now".to_string(),
        5..=59 => format!("{seconds} s ago"),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        _ => chrono::DateTime::<chrono::Local>::from(at)
            .format("%d/%m %H:%M")
            .to_string(),
    }
}

/// What a step did, in a few words.
fn summary(node: &crate::history::Node) -> String {
    match node.kind {
        Kind::Start => "start · the text as opened".to_string(),
        Kind::External => "reloaded from disk (changed outside Slate)".to_string(),
        Kind::BroughtBack => {
            let lines = node.inserted.matches('\n').count() as isize
                - node.removed.matches('\n').count() as isize;
            match lines.max(1) {
                1 => "brought back 1 line".to_string(),
                n => format!("brought back {n} lines"),
            }
        }
        Kind::Edit | Kind::Typing => {
            // Only what differs: a line replaced by itself plus another is
            // the other line added.
            let (removed, inserted) = differing(&node.removed, &node.inserted);
            match (removed.is_empty(), inserted.is_empty()) {
                (true, true) => "no change".to_string(),
                (true, false) => format!("+ {}", quote(inserted)),
                (false, true) => format!("− {}", quote(removed)),
                (false, false) => format!("{} → {}", quote(removed), quote(inserted)),
            }
        }
    }
}

/// `removed` and `inserted` without what they share at either end.
fn differing<'a>(removed: &'a str, inserted: &'a str) -> (&'a str, &'a str) {
    let prefix: usize = removed
        .chars()
        .zip(inserted.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let suffix: usize = removed[prefix..]
        .chars()
        .rev()
        .zip(inserted[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    (
        &removed[prefix..removed.len() - suffix],
        &inserted[prefix..inserted.len() - suffix],
    )
}

fn quote(text: &str) -> String {
    let filled: Vec<&str> = text
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let clip = |line: &str, max: usize| {
        if line.chars().count() <= max {
            line.to_string()
        } else {
            format!("{}…", line.chars().take(max - 1).collect::<String>())
        }
    };
    match filled.as_slice() {
        [] => match text.matches('\n').count() {
            0 => "space".to_string(),
            1 => "line break".to_string(),
            n => format!("{n} line breaks"),
        },
        [line] => format!("\"{}\"", clip(line, 48)),
        [first, ..] => format!("{} lines: \"{}\"", filled.len(), clip(first, 32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_go_in_before_the_line_they_name() {
        let (text, cursor) = insert_lines(
            "## Hoy\n- [ ] comprar yerba\n",
            &[(1, "- [ ] comprar pan".to_string())],
        );
        assert_eq!(text, "## Hoy\n- [ ] comprar pan\n- [ ] comprar yerba\n");
        assert_eq!(&text[..cursor], "## Hoy\n- [ ] comprar pan");
        let (text, _) = insert_lines("a\nb", &[(5, "c".to_string()), (0, "z".to_string())]);
        assert_eq!(text, "z\na\nb\nc");
    }

    #[test]
    fn summaries_say_what_changed() {
        let mut history = History::new(true);
        history.push(0, String::new(), "- [ ] pagar la luz\n".into(), 0, None, Kind::Edit);
        assert_eq!(summary(history.current_node()), "+ \"- [ ] pagar la luz\"");
        history.push(6, "leche".into(), "yerba".into(), 0, None, Kind::Edit);
        assert_eq!(summary(history.current_node()), "\"leche\" → \"yerba\"");
        history.push(0, "a\nb\nc\n".into(), String::new(), 0, None, Kind::Edit);
        assert_eq!(summary(history.current_node()), "− 3 lines: \"a\"");
        history.push(0, "uno\n".into(), "uno\ndos\n".into(), 0, None, Kind::Edit);
        assert_eq!(summary(history.current_node()), "+ \"dos\"");
    }

    #[test]
    fn a_line_typed_on_or_edited_is_not_missing() {
        assert!(same_line_edited("- [ ] renovar", "- [ ] renovar dni"));
        assert!(same_line_edited("- [", "- [ ] renovar dni"));
        assert!(same_line_edited("- [ ] comprar leche", "- [ ] comprar yerba"));
        assert!(same_line_edited("foo", "    foo"));
        assert!(same_line_edited("- [ ] comprar pan", "- [ ] comprar mucho pan"));
        assert!(same_line_edited("- [x] llamar al banco", "- [ ] llamar al banco"));
        assert!(!same_line_edited("hola mundo", "chau gente"));
        assert!(!same_line_edited("- [ ] dos", "- [ ] tres!"), "two tasks, not one");
        assert!(!same_line_edited("## Hoy", "## Semana"));
        assert!(!same_line_edited("", "algo"));
        assert_eq!(line_body("  - [x] 1. ## hola"), "hola");
        // Old side now, new side the state: "renovar" was typed on, "pan" is gone.
        let diff = Diff::new(
            "- [ ] renovar dni\n".to_string(),
            "- [ ] comprar pan\n- [ ] renovar\n".to_string(),
            0,
        );
        let inserted = inserted_lines(&diff);
        assert_eq!(inserted.get(&0), Some(&(0, false)), "\"pan\" is gone: it goes back first");
        assert_eq!(inserted.get(&1), Some(&(1, true)), "\"renovar\" was typed on");
        // A gone line between changed ones keeps its place.
        let missing = missing_lines(
            "a\n- [ ] uno!\n- [ ] tres!\n",
            "a\n- [ ] uno\n- [ ] dos\n- [ ] tres\n".into(),
        );
        assert_eq!(missing.get(&1), Some(&(2, true)));
        assert_eq!(missing.get(&2), Some(&(2, false)), "\"dos\" goes after \"uno!\"");
        assert_eq!(missing.get(&3), Some(&(3, true)));
    }
}
