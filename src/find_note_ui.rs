//! Responsive Find note view; search and opening remain in their existing handlers.
use super::*;
#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct Geometry {
    pub input: egui::Rect,
    pub list: egui::Rect,
    pub rows: Vec<egui::Rect>,
    pub start: usize,
    pub end: usize,
}
impl SlateApp {
    pub(super) fn find_note_dialog(&mut self, ctx: &egui::Context) {
        if !self.find_note_open {
            return;
        }
        let root = self.project_root();
        let clicked = dialog_ui::show(
            ctx,
            "find_note_dialog".into(),
            "Find note",
            &root.display().to_string(),
            &[
                ("Esc", "close"),
                ("↑", "previous"),
                ("↓", "next"),
                ("Tab/←/→", "headings"),
                ("Enter", "open main"),
                ("Shift+Enter", "front modal"),
            ],
            |ui| {
                let font = FontId::monospace(13.0);
                let accent = notice_ui::ACCENT;
                let text = notice_ui::TEXT;
                let dim = notice_ui::DIM;
                let faint = Color32::from_rgb(94, 105, 126);
                let body = ui.max_rect();
                let input_height = 30.0_f32.min(body.height().max(0.0));
                let input =
                    egui::Rect::from_min_size(body.min, Vec2::new(body.width(), input_height));
                let painter = ui.painter_at(input);
                painter.rect_filled(input, 0.0, Color32::from_rgb(30, 36, 48));
                painter.text(
                    egui::pos2(input.left() + 10.0, input.center().y),
                    egui::Align2::LEFT_CENTER,
                    ">",
                    font.clone(),
                    accent,
                );
                let query = if self.find_note_query.is_empty() {
                    "type to filter notes"
                } else {
                    &self.find_note_query
                };
                let query_rect = egui::Rect::from_min_max(
                    egui::pos2((input.left() + 32.0).min(input.right()), input.top()),
                    input.max,
                );
                let painter = painter.with_clip_rect(query_rect.intersect(ui.clip_rect()));
                let color = if self.find_note_query.is_empty() {
                    faint
                } else {
                    text
                };
                let galley = painter.layout_no_wrap(query.into(), font.clone(), color);
                let x = if self.find_note_query.is_empty() {
                    query_rect.left()
                } else {
                    query_rect
                        .left()
                        .min(query_rect.right() - galley.size().x - 3.0)
                };
                let cursor = if self.find_note_query.is_empty() {
                    query_rect.left()
                } else {
                    (x + galley.size().x + 2.0).min(query_rect.right() - 1.0)
                };
                painter.galley(
                    egui::pos2(x, input.center().y - galley.size().y * 0.5),
                    galley,
                    color,
                );
                if input_height >= 16.0 {
                    painter.line_segment(
                        [
                            egui::pos2(cursor, input.top() + 7.0),
                            egui::pos2(cursor, input.bottom() - 7.0),
                        ],
                        Stroke::new(1.0_f32, accent),
                    );
                }
                let list = egui::Rect::from_min_max(
                    egui::pos2(body.left(), (input.bottom() + 8.0).min(body.bottom())),
                    body.max,
                );
                let painter = ui
                    .painter_at(list)
                    .with_clip_rect(list.intersect(ui.clip_rect()));
                painter.rect_filled(list, 0.0, Color32::from_rgb(22, 28, 37));
                let row_height = 24.0;
                let heading_height = if list.height() >= 48.0 {
                    row_height
                } else {
                    0.0
                };
                let capacity = ((list.height() - heading_height) / row_height)
                    .floor()
                    .max(0.0) as usize;
                if ctx
                    .pointer_hover_pos()
                    .is_some_and(|pos| list.contains(pos))
                {
                    let delta = ctx.input(|input| {
                        input
                            .events
                            .iter()
                            .filter_map(|event| match event {
                                egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                                _ => None,
                            })
                            .sum::<f32>()
                    });
                    if delta.abs() > 0.5 {
                        self.move_find_note_selection(if delta > 0.0 { -1 } else { 1 });
                        ctx.request_repaint();
                    }
                }
                let selected = self
                    .selected_find_note_row
                    .min(self.find_note_rows.len().saturating_sub(1));
                let start = file_dialogs::centered_window_start(
                    selected,
                    capacity.max(1),
                    self.find_note_rows.len(),
                );
                let end = (start + capacity).min(self.find_note_rows.len());
                let hint_width = if list.width() >= 480.0 {
                    (list.width() * 0.3).min(245.0)
                } else {
                    0.0
                };
                let hint_x = list.right() - hint_width - 10.0;
                if heading_height > 0.0 {
                    painter.text(
                        egui::pos2(list.left() + 32.0, list.top() + 12.0),
                        egui::Align2::LEFT_CENTER,
                        "note · wheel / ↑↓ select",
                        font.clone(),
                        faint,
                    );
                    if hint_width > 0.0 {
                        painter.text(
                            egui::pos2(hint_x, list.top() + 12.0),
                            egui::Align2::LEFT_CENTER,
                            "match",
                            font.clone(),
                            faint,
                        );
                    }
                }
                if self.find_note_rows.is_empty() && list.height() > 0.0 {
                    painter.text(
                        list.center(),
                        egui::Align2::CENTER_CENTER,
                        "no notes found",
                        font.clone(),
                        faint,
                    );
                }
                #[cfg(test)]
                let mut row_rects = Vec::new();
                let mut open = false;
                for (row, index) in (start..end).enumerate() {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(
                            list.left() + 4.0,
                            list.top() + heading_height + row as f32 * row_height,
                        ),
                        Vec2::new((list.width() - 8.0).max(0.0), row_height),
                    );
                    let selected = index == self.selected_find_note_row;
                    if selected {
                        painter.rect_filled(rect, 0.0, Color32::from_rgb(38, 47, 61));
                    }
                    let (label, hint, prefix, is_heading) = match &self.find_note_rows[index] {
                        FindNoteRow::File { path, match_hint } => {
                            // Preserve the existing fresh heading check; no document-content cache.
                            let has_headings = fs::read_to_string(path)
                                .map(|contents| {
                                    contents
                                        .lines()
                                        .any(|line| parse_heading_line(line).is_some())
                                })
                                .unwrap_or(false);
                            let prefix = if has_headings {
                                if self.find_note_expanded_files.contains(path) {
                                    "▾ "
                                } else {
                                    "▸ "
                                }
                            } else {
                                "  "
                            };
                            (
                                path.strip_prefix(&root)
                                    .unwrap_or(path)
                                    .display()
                                    .to_string(),
                                match_hint.clone(),
                                prefix,
                                false,
                            )
                        }
                        FindNoteRow::Heading {
                            line, title, level, ..
                        } => (
                            format!("{}# {}", "  ".repeat(level.saturating_sub(1)), title),
                            format!("line {line}"),
                            "  ",
                            true,
                        ),
                    };
                    painter.text(
                        egui::pos2(rect.left() + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        if selected { ">" } else { " " },
                        font.clone(),
                        accent,
                    );
                    let label_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.left() + 28.0, rect.top()),
                        egui::pos2(
                            if hint_width > 0.0 {
                                hint_x - 12.0
                            } else {
                                rect.right() - 4.0
                            },
                            rect.bottom(),
                        ),
                    );
                    painter
                        .with_clip_rect(label_rect.intersect(list).intersect(ui.clip_rect()))
                        .text(
                            egui::pos2(label_rect.left(), rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            Self::text_for_width(
                                &format!("{prefix}{label}"),
                                label_rect.width().max(0.0),
                                13.0,
                            ),
                            font.clone(),
                            if selected {
                                text
                            } else if is_heading {
                                dim
                            } else {
                                accent
                            },
                        );
                    if hint_width > 0.0 {
                        painter.text(
                            egui::pos2(hint_x, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            Self::text_for_width(&hint, hint_width, 13.0),
                            font.clone(),
                            faint,
                        );
                    }
                    let full_path = match &self.find_note_rows[index] {
                        FindNoteRow::File { path, .. } | FindNoteRow::Heading { path, .. } => {
                            path.display().to_string()
                        }
                    };
                    let response = ui
                        .interact(
                            rect,
                            ui.id().with(("find_note", index)),
                            egui::Sense::click(),
                        )
                        .on_hover_ui(|ui| {
                            footer_ui::style_hints(ui);
                            ui.label(&full_path);
                            if is_heading {
                                ui.label(&label);
                            }
                            ui.label(&hint);
                        });
                    if response.clicked() || response.double_clicked() {
                        self.selected_find_note_row = index;
                    }
                    if response.double_clicked() {
                        open = true;
                    }
                    #[cfg(test)]
                    row_rects.push(rect);
                }
                #[cfg(test)]
                ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("find_note_list_geometry"),
                        Geometry {
                            input,
                            list,
                            rows: row_rects,
                            start,
                            end,
                        },
                    )
                });
                if open {
                    self.open_selected_find_note(false);
                }
            },
        );
        if clicked[0] {
            self.find_note_open = false;
            self.focus_editor_once = true;
        } else if clicked[1] {
            self.move_find_note_selection(-1);
        } else if clicked[2] {
            self.move_find_note_selection(1);
        } else if clicked[3] {
            self.toggle_selected_find_note_file();
        } else if clicked[4] {
            self.open_selected_find_note(false);
        } else if clicked[5] {
            self.open_selected_find_note(true);
        }
    }
}
