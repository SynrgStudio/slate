//! Periscope results modal.
use super::*;

impl SlateApp {
    pub(super) fn periscope_dialog(&mut self, ctx: &egui::Context) {
        if !self.periscope_open {
            return;
        }
        self.poll_periscope_global_index();
        let rows = self.periscope_rows();
        let mode_label = match self.periscope_mode {
            PeriscopeMode::Project => "project",
            PeriscopeMode::Global => "global",
        };
        let scope = match self.periscope_mode {
            PeriscopeMode::Project => self
                .periscope_project_root
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "no project".to_string()),
            PeriscopeMode::Global => {
                if self.periscope_roots.is_empty() {
                    "local fixed drives · Everything / SQLite".into()
                } else {
                    self.periscope_roots
                        .iter()
                        .map(|root| root.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" · ")
                }
            }
        };

        let subtitle = format!("{scope} · {}", self.periscope_backend_status);
        let clicked = dialog_ui::show(
            ctx,
            "periscope_dialog".into(),
            &format!("Periscope · {mode_label}"),
            &subtitle,
            &[
                ("Esc", "close"),
                ("Tab", "project/global"),
                ("Enter", "open/expand"),
                ("Ctrl+Enter", "modal"),
            ],
            |ui| {
                let font = FontId::new(13.0, FontFamily::Monospace);
                let accent = Color32::from_rgb(136, 192, 208);
                let text = Color32::from_rgb(216, 222, 233);
                let dim = Color32::from_rgb(136, 154, 176);
                let faint = Color32::from_rgb(94, 105, 126);
                let warn = Color32::from_rgb(235, 203, 139);
                let danger = Color32::from_rgb(191, 97, 106);

                let input_height = 30.0;
                let (input_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), input_height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(input_rect);
                painter.rect_filled(input_rect, 0.0, Color32::from_rgb(30, 36, 48));
                painter.rect_stroke(
                    input_rect,
                    0.0,
                    Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
                    egui::StrokeKind::Outside,
                );
                let query = if self.periscope_query.is_empty() {
                    "type to search files".to_string()
                } else {
                    self.periscope_query.clone()
                };
                let query_color = if self.periscope_query.is_empty() {
                    faint
                } else {
                    text
                };
                painter.text(
                    egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                    egui::Align2::LEFT_CENTER,
                    ">",
                    font.clone(),
                    accent,
                );
                let query_rect = painter.text(
                    egui::pos2(input_rect.left() + 32.0, input_rect.center().y - 0.5),
                    egui::Align2::LEFT_CENTER,
                    query,
                    font.clone(),
                    query_color,
                );
                let cursor_x = if self.periscope_query.is_empty() {
                    input_rect.left() + 32.0
                } else {
                    query_rect.right() + 2.0
                };
                painter.line_segment(
                    [
                        egui::pos2(cursor_x, input_rect.top() + 7.0),
                        egui::pos2(cursor_x, input_rect.bottom() - 7.0),
                    ],
                    Stroke::new(1.0_f32, accent),
                );

                ui.add_space(8.0);
                let row_height = 24.0;
                let list_height = ui.available_height().max(1.0);
                let heading_height = if list_height >= row_height * 2.0 {
                    row_height
                } else {
                    0.0
                };
                let visible_rows = ((list_height - heading_height) / row_height)
                    .floor()
                    .max(0.0) as usize;
                let selected = self
                    .selected_periscope_result
                    .min(rows.len().saturating_sub(1));
                let start =
                    file_dialogs::centered_window_start(selected, visible_rows.max(1), rows.len());
                let end = (start + visible_rows).min(rows.len());
                let (list_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), list_height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(list_rect);
                painter.rect_filled(list_rect, 0.0, Color32::from_rgb(22, 28, 37));
                if heading_height > 0.0 {
                    painter.text(
                        egui::pos2(list_rect.left() + 8.0, list_rect.top() + row_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        "path · ↑↓ select · wheel scroll",
                        font.clone(),
                        faint,
                    );
                }
                if ctx
                    .pointer_hover_pos()
                    .is_some_and(|pos| list_rect.contains(pos))
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
                        self.move_periscope_selection(if delta > 0.0 { -1 } else { 1 });
                        ctx.request_repaint();
                    }
                }
                #[cfg(test)]
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("periscope_list_rect"), list_rect)
                });
                if rows.is_empty() {
                    let message = if self.periscope_mode == PeriscopeMode::Global
                        && self.periscope_global_loading
                    {
                        "searching index…"
                    } else if self.periscope_mode == PeriscopeMode::Global
                        && self.periscope_query.trim().is_empty()
                    {
                        "type to search configured drives · top 100"
                    } else if self.periscope_query.trim().is_empty() {
                        "type a query to search this project"
                    } else {
                        "no matches"
                    };
                    painter.text(
                        list_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        message,
                        font.clone(),
                        faint,
                    );
                }

                for (row, index) in (start..end).enumerate() {
                    let row_top = list_rect.top() + heading_height + row as f32 * row_height;
                    let row_rect = egui::Rect::from_min_size(
                        egui::pos2(list_rect.left() + 4.0, row_top),
                        Vec2::new(list_rect.width() - 8.0, row_height),
                    );
                    let selected = index == self.selected_periscope_result;
                    if selected {
                        painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                    }
                    let row = &rows[index];
                    let (path, path_text, is_folder) = match row {
                        PeriscopeRow::Folder { path, expanded } => {
                            let mut label = format!("{}/", path.display());
                            if *expanded {
                                label.push_str("  expanded");
                            }
                            (path, label, true)
                        }
                        PeriscopeRow::File { path, group } => {
                            let label = if let Some(group) = group {
                                format!(
                                    "---/{}",
                                    path.strip_prefix(group).unwrap_or(path).display()
                                )
                            } else if self.periscope_mode == PeriscopeMode::Project {
                                if let Some(root) = &self.periscope_project_root {
                                    path.strip_prefix(root)
                                        .unwrap_or(path)
                                        .display()
                                        .to_string()
                                } else {
                                    path.display().to_string()
                                }
                            } else {
                                path.display().to_string()
                            };
                            let label = match self.aliases.get(path) {
                                Some(alias) => format!("{alias} · {label}"),
                                None => label,
                            };
                            (path, label, false)
                        }
                    };
                    let full = path.display().to_string();
                    let sensitive = periscope::is_sensitive_path(path);
                    let y = row_rect.center().y - 0.5;
                    painter.text(
                        egui::pos2(row_rect.left() + 8.0, y),
                        egui::Align2::LEFT_CENTER,
                        if selected { ">" } else { " " },
                        font.clone(),
                        accent,
                    );
                    painter.text(
                        egui::pos2(row_rect.left() + 28.0, y),
                        egui::Align2::LEFT_CENTER,
                        Self::text_for_width(&path_text, list_rect.width() - 56.0, 13.0),
                        font.clone(),
                        if sensitive {
                            danger
                        } else if is_folder {
                            warn
                        } else if selected {
                            text
                        } else {
                            dim
                        },
                    );
                    let response = ui.interact(
                        row_rect,
                        ui.id().with(("periscope", index)),
                        egui::Sense::click(),
                    );
                    let response = response.on_hover_ui(|ui| {
                        footer_ui::style_hints(ui);
                        ui.label(&full);
                    });
                    if response.clicked() || response.double_clicked() {
                        self.selected_periscope_result = index;
                    }
                    if response.double_clicked() {
                        self.open_selected_periscope_result();
                    }
                }
            },
        );
        if clicked[0] {
            self.periscope_open = false;
            self.periscope_engine.cancel();
            self.periscope_request = None;
            self.focus_editor_once = true;
        }
        if clicked[1] {
            self.toggle_periscope_mode();
        }
        if clicked[2] {
            self.open_selected_periscope_result();
        }
        if clicked[3] {
            self.open_selected_periscope_result_as_modal();
        }
    }
}
