//! Slate-owned Save As modal: same chrome, size and columns as the file
//! picker; ← at a drive's root (or Ctrl+L) chooses another drive.
use super::*;

impl SlateApp {
    pub(super) fn save_as_dialog(&mut self, ctx: &egui::Context) {
        if !self.save_as_open {
            return;
        }

        let matches = self.save_as_entry_indices();
        let selected_position = matches
            .iter()
            .position(|index| *index == self.selected_save_as_entry)
            .unwrap_or(0);

        window_ui::DialogArea::new("save_as_dialog".into())
            .fixed_chrome()
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -16.0])
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
                        // 80% of the window, like the file picker.
                        ui.set_width((ctx.content_rect().width() * 0.8 - 30.0).max(1.0));
                        ui.set_height(((ctx.content_rect().height() - 30.0) * 0.8 - 30.0).max(1.0));
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);

                        let (header, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 20.0),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(header);
                        let title = painter.text(
                            header.left_center(),
                            egui::Align2::LEFT_CENTER,
                            "save as",
                            title_font,
                            accent,
                        );
                        painter.text(
                            header.left_center() + Vec2::new(title.width() + 10.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            Self::text_for_width(
                                &format!(
                                    "{} · {} entries",
                                    self.save_as_dir.display(),
                                    self.save_as_entries.len()
                                ),
                                (header.width() - title.width() - 110.0).max(0.0),
                                13.0,
                            ),
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            header.right_center(),
                            egui::Align2::RIGHT_CENTER,
                            "[esc] close",
                            font.clone(),
                            warn,
                        );
                        ui.add_space(8.0);

                        let input_height = 28.0;
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
                        let filename_empty = self.save_as_filename.is_empty();
                        let prompt = painter.text(
                            egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            "save as:",
                            font.clone(),
                            accent,
                        );
                        let text_left = prompt.right() + 10.0;
                        if self.save_as_name_fresh && !filename_empty {
                            let galley = painter.layout_no_wrap(
                                self.save_as_filename.clone(),
                                font.clone(),
                                text,
                            );
                            painter.rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(text_left - 2.0, input_rect.center().y - 9.0),
                                    Vec2::new(galley.size().x + 4.0, 18.0),
                                ),
                                2.0,
                                Color32::from_rgb(59, 78, 104),
                            );
                        }
                        let filename_rect = painter.text(
                            egui::pos2(text_left, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            if filename_empty {
                                "type file name"
                            } else {
                                self.save_as_filename.as_str()
                            },
                            font.clone(),
                            if filename_empty { faint } else { text },
                        );
                        let cursor_x = if filename_empty {
                            text_left
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

                        ui.add_space(4.0);
                        let chosen_dir = self
                            .save_as_armed
                            .then(|| self.save_as_entries.get(self.selected_save_as_entry))
                            .flatten()
                            .filter(|path| path.is_dir());
                        let target = match chosen_dir {
                            Some(dir) => format!("enter: open {}", dir.display()),
                            None if self.save_as_filename.trim().is_empty() => {
                                "type a file name".to_string()
                            }
                            None if self.save_as_name_fresh => format!(
                                "enter: save as {} · type to search folders or to rename",
                                self.save_as_dir.join(self.save_as_filename.trim()).display()
                            ),
                            None => format!(
                                "enter: save as {}",
                                self.save_as_dir.join(self.save_as_filename.trim()).display()
                            ),
                        };
                        ui.add(
                            egui::Label::new(RichText::new(target).font(font.clone()).color(dim))
                                .truncate(),
                        );
                        ui.add_space(6.0);
                        let row_height = 24.0;
                        let list_height = (ui.available_height() - 28.0).max(row_height * 2.0);
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        // One row for the column titles.
                        let visible_rows = ((list_height / row_height).floor() as usize)
                            .saturating_sub(1)
                            .max(1);
                        let start = file_dialogs::centered_window_start(
                            selected_position,
                            visible_rows,
                            matches.len(),
                        );
                        let end = (start + visible_rows).min(matches.len());
                        let columns = file_dialogs::PickerColumns::new(list_rect.width(), true);
                        let name_width = columns
                            .size_right
                            .map_or(list_rect.width() - 40.0, |x| x - 64.0 - 40.0)
                            .max(0.0);
                        let painter = ui.painter_at(list_rect);
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
                        if let Some(x) = columns.size_right {
                            painter.text(
                                egui::pos2(list_rect.left() + x, header_y),
                                egui::Align2::RIGHT_CENTER,
                                "size",
                                font.clone(),
                                faint,
                            );
                        }
                        if let Some(x) = columns.modified_right {
                            painter.text(
                                egui::pos2(list_rect.left() + x, header_y),
                                egui::Align2::RIGHT_CENTER,
                                "modified",
                                font.clone(),
                                faint,
                            );
                        }

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
                            let selected =
                                self.save_as_armed && index == self.selected_save_as_entry;
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
                                    name_width,
                                    13.0,
                                ),
                                font.clone(),
                                if selected { text } else { accent },
                            );
                            if let Some(x) = columns.size_right {
                                painter.text(
                                    egui::pos2(list_rect.left() + x - 16.0, y),
                                    egui::Align2::RIGHT_CENTER,
                                    size_label,
                                    font.clone(),
                                    if selected { dim } else { faint },
                                );
                            }
                            if let Some(x) = columns.modified_right {
                                painter.text(
                                    egui::pos2(list_rect.left() + x - 16.0, y),
                                    egui::Align2::RIGHT_CENTER,
                                    modified_label,
                                    font.clone(),
                                    if selected { dim } else { faint },
                                );
                            }

                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("save_as_entry", index)),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_save_as_entry = index;
                                self.save_as_armed = true;
                                if path.is_file() {
                                    self.save_as_filename = name.to_string();
                                    self.save_as_edited = false;
                                    self.save_as_name_fresh = true;
                                }
                            }
                            if response.double_clicked() {
                                self.selected_save_as_entry = index;
                                if path.is_dir() {
                                    self.save_as_enter_selected_dir();
                                } else {
                                    self.save_as_filename = name.to_string();
                                    self.save_as_edited = false;
                                    self.save_as_armed = false;
                                    self.confirm_save_as();
                                }
                                break; // the entry list may have changed
                            }
                        }

                        ui.add_space(8.0);
                        let hints = [
                            ("↑↓", "select"),
                            ("type", "name/filter"),
                            ("→", "enter dir"),
                            ("←", "parent/drives"),
                            ("Ctrl+L", "drives"),
                            ("enter", "save / open chosen folder"),
                            ("esc", "close"),
                        ]
                        .iter()
                        .map(|(key, label)| format!("[{key}] {label}"))
                        .collect::<Vec<_>>()
                        .join("  ");
                        ui.add(
                            egui::Label::new(RichText::new(hints).font(font.clone()).color(dim))
                                .truncate(),
                        );
                    });
            });
        if let Some(dir) = drive_picker::show(ctx) {
            self.save_as_show_dir(dir);
        }
        self.save_as_overwrite_prompt(ctx);
    }

    /// "Replace it?" over the dialog while an existing file would be replaced.
    fn save_as_overwrite_prompt(&mut self, ctx: &egui::Context) {
        let Some((path, _)) = self.save_as_overwrite.clone() else {
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = path
            .parent()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        let (modified, size) = std::fs::metadata(&path)
            .ok()
            .map(|_| {
                let (size, modified) = file_dialogs::file_metadata_labels(&path);
                (modified, size)
            })
            .unwrap_or_default();
        let mut answer = None;
        egui::Area::new("save_as_overwrite".into())
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -16.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(30, 36, 48))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(235, 203, 139)))
                    .inner_margin(16.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(170),
                    })
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        let font = FontId::monospace(13.0);
                        ui.label(
                            RichText::new(format!("{name} already exists"))
                                .font(FontId::monospace(15.0))
                                .color(Color32::from_rgb(235, 203, 139)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!("in {folder}"))
                                .font(font.clone())
                                .color(Color32::from_rgb(136, 154, 176)),
                        );
                        if !size.is_empty() {
                            ui.label(
                                RichText::new(format!("{size} · modified {modified}"))
                                    .font(font.clone())
                                    .color(Color32::from_rgb(136, 154, 176)),
                            );
                        }
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Replace it with this document?")
                                .font(font.clone())
                                .color(Color32::from_rgb(216, 222, 233)),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(RichText::new("[Enter] replace").font(font.clone()))
                                .clicked()
                            {
                                answer = Some(true);
                            }
                            ui.add_space(8.0);
                            if ui
                                .button(RichText::new("[Esc] keep it").font(font.clone()))
                                .clicked()
                            {
                                answer = Some(false);
                            }
                        });
                    });
            });
        if let Some(replace) = answer {
            self.answer_save_as_overwrite(replace);
        }
    }
}
