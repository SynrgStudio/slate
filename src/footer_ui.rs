use super::*;
pub(crate) const FIELDS: [(&str, &str); 10] = [
    ("footer_filename", "File name"),
    ("footer_state", "Saved / modified indicator"),
    ("footer_cursor", "Cursor position"),
    ("footer_mode", "Active mode / search context"),
    ("footer_wrap", "Wrap mode"),
    ("footer_lines", "Total lines"),
    ("footer_words", "Total words"),
    ("footer_characters", "Total characters"),
    ("footer_shortcuts", "Shortcut hints"),
    ("footer_messages", "Temporary status messages"),
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Options {
    pub enabled: [bool; 10],
}
impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: [
                true, true, true, true, true, false, false, false, true, true,
            ],
        }
    }
}
impl Options {
    pub fn key_index(key: &str) -> Option<usize> {
        FIELDS.iter().position(|(name, _)| *name == key)
    }
}
#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct Geometry {
    pub frame: egui::Rect,
    pub left: egui::Rect,
    pub right: Vec<egui::Rect>,
}
/// Text-only palette launcher on the command row, never on the titlebar.
pub(super) fn palette_launcher(
    ui: &mut egui::Ui,
    row: egui::Rect,
    visible: bool,
) -> (egui::Rect, bool) {
    let font = FontId::monospace(12.0);
    let dim = Color32::from_rgb(136, 154, 176);
    let text = "Menú · Ctrl+P";
    let galley = ui.painter().layout_no_wrap(text.into(), font, dim);
    if !visible || row.width() < galley.size().x + 100.0 {
        return (row, false);
    }
    let rect = egui::Rect::from_min_max(
        egui::pos2(row.right() - 10.0 - galley.size().x, row.top()),
        egui::pos2(row.right() - 10.0, row.bottom()),
    );
    let response = ui.interact(rect, ui.id().with("palette_launcher"), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    ui.painter_at(rect).galley_with_override_text_color(
        egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5),
        galley,
        if response.hovered() || response.has_focus() {
            Color32::from_rgb(136, 192, 208)
        } else {
            dim
        },
    );
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("palette_launcher_rect"), rect));
    let clicked = response.clicked();
    response.on_hover_ui(|ui| {
        style_hints(ui);
        ui.label("Command palette · Ctrl+P · Crear Proyecto");
    });
    (
        egui::Rect::from_min_max(row.min, egui::pos2(rect.left() - 10.0, row.bottom())),
        clicked,
    )
}
fn width(p: &egui::Painter, text: &str, font: &FontId) -> f32 {
    p.layout_no_wrap(text.into(), font.clone(), Color32::WHITE)
        .size()
        .x
}
fn elide(p: &egui::Painter, text: &str, font: &FontId, max: f32) -> String {
    if max <= 0.0 {
        return String::new();
    }
    if width(p, text, font) <= max {
        return text.into();
    }
    let ellipsis = width(p, "…", font);
    if ellipsis > max {
        return String::new();
    }
    let mut result = String::new();
    for ch in text.chars() {
        result.push(ch);
        if width(p, &result, font) + ellipsis > max {
            result.pop();
            break;
        }
    }
    result.push('…');
    result
}
/// Local tooltip styling; do not modify application-wide typography.
pub(super) fn style_hints(ui: &mut egui::Ui) {
    ui.style_mut().override_font_id = Some(FontId::monospace(12.0));
    ui.visuals_mut().override_text_color = Some(Color32::from_rgb(136, 154, 176));
}
/// Keep only complete help blocks; never clip a shortcut or leave a trailing separator.
pub(super) fn fitted_hints(
    painter: &egui::Painter,
    font: &FontId,
    available: f32,
    text: &str,
    color: Color32,
    accent: Color32,
) -> Vec<(String, Color32)> {
    let (label, rest) = text.split_once("  ").unwrap_or((text, ""));
    if width(painter, &format!(":{label}"), font) > available {
        return Vec::new();
    }
    let mut body = label.to_string();
    for (index, block) in rest
        .split(" · ")
        .filter(|block| !block.is_empty())
        .enumerate()
    {
        let candidate = format!("{body}{}{block}", if index == 0 { "  " } else { " · " });
        if width(painter, &format!(":{candidate}"), font) > available {
            break;
        }
        body = candidate;
    }
    vec![(":".into(), accent), (body, color)]
}
impl SlateApp {
    pub(super) fn open_command_palette(&mut self) {
        if self.periscope_setup.visible
            || self.pending_action.is_some()
            || self.recovery_open
            || self.file_conflict.is_some()
            || self.flow_item_edit.is_some()
        {
            return;
        }
        self.recent_picker_open = false;
        self.lance_open = false;
        self.doc_tasks_open = false;
        self.file_picker_open = false;
        self.periscope_open = false;
        self.periscope_prompt_open = false;
        self.find_note_open = false;
        self.link_heading_picker_open = false;
        if !self.link_preview_dirty {
            self.link_preview_open = false;
        }
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.capture_modal_open = false;
        self.settings_open = false;
        self.periscope_engine.cancel();
        self.periscope_request = None;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.focus_editor_once = false;
        self.palette_open = true;
        self.palette_query.clear();
        self.selected_command = 0;
    }
    pub(super) fn set_footer_option(&mut self, index: usize, value: bool) {
        if let Some(enabled) = self.footer_options.enabled.get_mut(index) {
            *enabled = value;
        }
        self.status = match self.save_settings() {
            Ok(()) => "Footer preferences saved".into(),
            Err(error) => error,
        };
    }
    pub(super) fn footer_counts(&mut self) -> (usize, usize) {
        self.footer_counts.sync(&self.buffer)
    }
    pub(super) fn compact_footer(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        active_mode: &str,
    ) {
        let font = FontId::monospace(12.0);
        let accent = Color32::from_rgb(136, 192, 208);
        let dim = Color32::from_rgb(136, 154, 176);
        let text = Color32::from_rgb(216, 222, 233);
        let warn = Color32::from_rgb(235, 203, 139);
        let ok = Color32::from_rgb(163, 190, 140);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(25, 31, 40));
        if self.footer_status_seen != self.status {
            self.footer_status_seen = self.status.clone();
            self.footer_status_since = Some(std::time::Instant::now());
        }
        let flags = self.footer_options.enabled;
        let path = self
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "untitled".into());
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());
        let (line, column) = self.buffer.cursor_line_col();
        let lines = self.buffer.line_count();
        let mut items: Vec<(String, Color32, u8)> = Vec::new();
        if flags[2] {
            items.push((format!("Ln {} · Col {}", line + 1, column + 1), dim, 0));
        }
        if flags[3] {
            if let Some(search) = &self.search_state {
                items.push((
                    format!(
                        "find {}/{}",
                        if search.matches.is_empty() {
                            0
                        } else {
                            search.selected + 1
                        },
                        search.matches.len()
                    ),
                    accent,
                    1,
                ));
            } else if active_mode != "edit" && active_mode != "preview" {
                items.push((active_mode.into(), accent, 1));
            } else if self.preview {
                items.push(("preview".into(), dim, 5));
            }
        }
        if flags[4] {
            items.push((if self.wrap { "wrap" } else { "nowrap" }.into(), dim, 4));
        }
        if flags[5] {
            items.push((format!("{lines} lines"), dim, 6));
        }
        if flags[6] || flags[7] {
            let (words, characters) = self.footer_counts();
            if flags[6] {
                items.push((format!("{words} words"), dim, 7));
            }
            if flags[7] {
                items.push((format!("{characters} chars"), dim, 8));
            }
        }
        if flags[8] {
            items.push(("[Ctrl+P] [Ctrl+H]".into(), accent, 9));
        }
        let inner = rect.shrink2(Vec2::new(10.0, 0.0));
        let gap = 12.0;
        let minimum_left = if flags[0] {
            80.0
        } else if flags[1] {
            45.0
        } else {
            0.0
        };
        let budget = (inner.width() - minimum_left - gap).max(0.0);
        let total = |items: &[(String, Color32, u8)]| {
            items
                .iter()
                .map(|(value, _, _)| width(&painter, value, &font) + gap)
                .sum::<f32>()
        };
        while !items.is_empty() && total(&items) > budget {
            let index = items
                .iter()
                .enumerate()
                .max_by_key(|(_, item)| item.2)
                .unwrap()
                .0;
            items.remove(index);
        }
        let mut x = inner.right();
        #[cfg(test)]
        let mut right_rects = Vec::new();
        for (value, color, _) in items.iter().rev() {
            let measured = width(&painter, value, &font);
            let item_rect = egui::Rect::from_min_max(
                egui::pos2(x - measured, rect.top()),
                egui::pos2(x, rect.bottom()),
            );
            painter.with_clip_rect(item_rect.intersect(rect)).text(
                egui::pos2(x, rect.center().y - 0.5),
                egui::Align2::RIGHT_CENTER,
                value,
                font.clone(),
                *color,
            );
            x -= measured + gap;
            #[cfg(test)]
            right_rects.push(item_rect);
        }
        let left =
            egui::Rect::from_min_max(inner.min, egui::pos2(x.max(inner.left()), inner.bottom()));
        let left_painter = painter.with_clip_rect(left.intersect(rect));
        let mut left_x = left.left();
        if flags[1] {
            let state = if self.dirty { "●" } else { "saved" };
            let measured = width(&painter, state, &font);
            left_painter.text(
                egui::pos2(left_x, rect.center().y - 0.5),
                egui::Align2::LEFT_CENTER,
                state,
                font.clone(),
                if self.dirty { warn } else { ok },
            );
            left_x += measured + 8.0;
        }
        let message_visible = flags[9]
            && self.status != "Ready"
            && self
                .footer_status_since
                .is_some_and(|since| since.elapsed() < std::time::Duration::from_secs(4));
        if message_visible {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(150));
        }
        if flags[0] {
            let available = (left.right() - left_x).max(0.0);
            let limit = if message_visible {
                available.min(inner.width() * 0.35)
            } else {
                available
            };
            let label = elide(&painter, &name, &font, limit);
            left_painter.text(
                egui::pos2(left_x, rect.center().y - 0.5),
                egui::Align2::LEFT_CENTER,
                &label,
                font.clone(),
                text,
            );
            left_x += width(&painter, &label, &font) + gap;
        }
        if message_visible {
            let message = elide(
                &painter,
                &self.status,
                &font,
                (left.right() - left_x).max(0.0),
            );
            left_painter.text(
                egui::pos2(left_x, rect.center().y - 0.5),
                egui::Align2::LEFT_CENTER,
                message,
                font.clone(),
                dim,
            );
        }
        let response = ui.interact(rect, ui.id().with("footer_details"), egui::Sense::hover());
        if response.hovered() {
            let (words, characters) = self.footer_counts();
            response.on_hover_ui(|ui| {
                style_hints(ui);
                ui.label(path);
                ui.label(if self.dirty {
                    "Modified — unsaved changes"
                } else {
                    "Saved"
                });
                ui.label(format!(
                    "Ln {} · Col {} · {lines} lines · {words} words · {characters} characters",
                    line + 1,
                    column + 1
                ));
                ui.label(format!(
                    "{active_mode} · {}",
                    if self.wrap { "wrap" } else { "nowrap" }
                ));
                ui.label(&self.status);
                ui.label("Choose visible fields in Settings → Footer.");
            });
        }
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                "compact_footer_geometry".into(),
                Geometry {
                    frame: rect,
                    left,
                    right: right_rects,
                },
            )
        });
    }
}

/// Word and character totals kept per line, so an edit only recounts the
/// lines it touched instead of the whole document.
#[derive(Default)]
pub(crate) struct DocumentCounts {
    key: Option<(u64, u64)>,
    lines: Vec<(u32, u32)>,
    words: usize,
    characters: usize,
}

impl DocumentCounts {
    fn count(line: &str) -> (u32, u32) {
        (
            line.split_whitespace().count() as u32,
            line.chars().count() as u32,
        )
    }

    /// Words and characters (newlines included) of the buffer.
    pub(crate) fn sync(&mut self, buffer: &EditorBuffer) -> (usize, usize) {
        let key = (buffer.identity, buffer.revision);
        if self.key != Some(key) {
            let count = buffer.line_count();
            let edges = self
                .key
                .filter(|(identity, _)| *identity == buffer.identity)
                .and_then(|(_, revision)| buffer.unchanged_layout_edges(revision));
            let (prefix, removed_end) = match edges {
                Some((prefix, suffix)) => {
                    let old = self.lines.len();
                    let prefix = prefix.min(old).min(count);
                    let suffix = suffix.min(old - prefix).min(count - prefix);
                    (prefix, old - suffix)
                }
                None => {
                    self.words = 0;
                    self.characters = 0;
                    (0, self.lines.len())
                }
            };
            let suffix = self.lines.len() - removed_end;
            let fresh: Vec<_> = (prefix..count - suffix)
                .map(|index| Self::count(buffer.line(index)))
                .collect();
            let removed: Vec<_> = self
                .lines
                .splice(prefix..removed_end, fresh.iter().copied())
                .collect();
            if edges.is_some() {
                for (words, characters) in removed {
                    self.words -= words as usize;
                    self.characters -= characters as usize;
                }
            }
            for (words, characters) in fresh {
                self.words += words as usize;
                self.characters += characters as usize;
            }
            self.key = Some(key);
        }
        (
            self.words,
            self.characters + self.lines.len().saturating_sub(1),
        )
    }

    #[cfg(test)]
    pub(crate) fn revision(&self) -> Option<u64> {
        self.key.map(|(_, revision)| revision)
    }
}
