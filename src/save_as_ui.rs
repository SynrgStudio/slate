//! Slate-owned Save As modal.
use super::*;

impl SlateApp {
    pub(super) fn save_as_dialog(&mut self, ctx: &egui::Context) {
        if !self.save_as_open {
            return;
        }

        let matches = self.save_as_entry_indices();
        let visible_rows = 12usize;
        let selected_position = matches
            .iter()
            .position(|index| *index == self.selected_save_as_entry)
            .unwrap_or(0);
        let start =
            file_dialogs::centered_window_start(selected_position, visible_rows, matches.len());
        let end = (start + visible_rows).min(matches.len());

        window_ui::DialogArea::new("save_as_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(820.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("save as").font(title_font).color(accent));
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} entries",
                                    self.save_as_dir.display(),
                                    self.save_as_entries.len()
                                ))
                                .font(font.clone())
                                .color(faint),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);

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
                        let filename = if self.save_as_filename.is_empty() {
                            "type file name".to_string()
                        } else {
                            self.save_as_filename.clone()
                        };
                        let filename_color = if self.save_as_filename.is_empty() {
                            faint
                        } else {
                            text
                        };
                        painter.text(
                            egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            "save as: ",
                            font.clone(),
                            accent,
                        );
                        let filename_rect = painter.text(
                            egui::pos2(input_rect.left() + 90.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            filename,
                            font.clone(),
                            filename_color,
                        );
                        let cursor_x = if self.save_as_filename.is_empty() {
                            input_rect.left() + 90.0
                        } else {
                            filename_rect.right() + 2.0
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
                        let list_height = (visible_rows.max(1) as f32 + 1.0) * row_height;
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(list_rect).with_clip_rect(list_rect);
                        painter.rect_filled(list_rect, 0.0, Color32::from_rgb(22, 28, 37));
                        painter.rect_stroke(
                            list_rect,
                            0.0,
                            Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let header_y = list_rect.top() + row_height * 0.5;
                        painter.text(
                            egui::pos2(list_rect.left() + 32.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "name",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 300.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "size",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 390.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "modified",
                            font.clone(),
                            faint,
                        );

                        if matches.is_empty() {
                            painter.text(
                                list_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "no matching entries",
                                font.clone(),
                                faint,
                            );
                        }

                        for (row, index) in matches[start..end].iter().copied().enumerate() {
                            let row_top = list_rect.top() + (row as f32 + 1.0) * row_height;
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(list_rect.left() + 4.0, row_top),
                                Vec2::new(list_rect.width() - 8.0, row_height),
                            );
                            let selected = index == self.selected_save_as_entry;
                            if selected {
                                painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                            }
                            let path = &self.save_as_entries[index];
                            let name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("unknown");
                            let (size_label, modified_label) = if path.is_dir() {
                                ("dir".to_string(), "".to_string())
                            } else {
                                file_dialogs::file_metadata_labels(path)
                            };
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
                                Self::text_for_width(
                                    &format!("{}{}", if path.is_dir() { "▸ " } else { "  " }, name),
                                    250.0,
                                    13.0,
                                ),
                                font.clone(),
                                if selected { text } else { accent },
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 300.0, y),
                                egui::Align2::LEFT_CENTER,
                                size_label,
                                font.clone(),
                                if selected { dim } else { faint },
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 390.0, y),
                                egui::Align2::LEFT_CENTER,
                                modified_label,
                                font.clone(),
                                if selected { dim } else { faint },
                            );

                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("save_as_entry", index)),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_save_as_entry = index;
                                if path.is_file() {
                                    self.save_as_filename = name.to_string();
                                }
                            }
                            if response.double_clicked() {
                                self.selected_save_as_entry = index;
                                if path.is_dir() {
                                    self.save_as_enter_selected_dir();
                                } else {
                                    self.save_as_filename = name.to_string();
                                    self.confirm_save_as();
                                }
                            }
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [
                                ("↑↓", "select"),
                                ("type", "name/filter"),
                                ("→", "enter dir"),
                                ("←", "parent"),
                                ("enter", "save"),
                                ("esc", "close"),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }
}
