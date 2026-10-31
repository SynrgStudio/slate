//! Shared Slate Picker modal (open, open-buffer, recent, Lance).
use super::*;

impl SlateApp {
    pub(super) fn slate_picker_dialog(&mut self, ctx: &egui::Context) {
        enum ActivePicker {
            File,
            Lance,
            Recent,
        }

        let active = if self.file_picker_open {
            ActivePicker::File
        } else if self.lance_open {
            ActivePicker::Lance
        } else if self.recent_picker_open {
            ActivePicker::Recent
        } else {
            return;
        };

        let file_matches = if matches!(active, ActivePicker::File) {
            self.project_file_indices()
        } else {
            Vec::new()
        };
        let lance_matches = if matches!(active, ActivePicker::Lance) {
            self.lance_file_indices()
        } else {
            Vec::new()
        };
        let recent_matches = if matches!(active, ActivePicker::Recent) {
            self.recent_file_indices()
        } else {
            Vec::new()
        };

        let (title, subtitle, prompt, query, selected_index, total_len, _preferred_rows) =
            match active {
                ActivePicker::File => {
                    let title = match self.file_picker_mode {
                        FilePickerMode::Open => "open",
                        FilePickerMode::OpenBuffer => "open buffer",
                        FilePickerMode::AddLance => "add lance",
                        FilePickerMode::Browse => "files",
                        FilePickerMode::InsertMarkdownLink => "insert link file",
                        FilePickerMode::Refile => "refile target",
                        FilePickerMode::SetPeriscopeProject => "create project",
                    };
                    (
                        title.to_string(),
                        format!(
                            "{} entries · {}",
                            self.project_files.len(),
                            self.file_picker_dir.display()
                        ),
                        "type to fuzzy-find files and folders".to_string(),
                        self.file_query.clone(),
                        file_matches
                            .iter()
                            .position(|index| *index == self.selected_project_file)
                            .unwrap_or(0),
                        file_matches.len(),
                        16usize,
                    )
                }
                ActivePicker::Lance => (
                    "lance".to_string(),
                    format!("{} marks", self.lance_files.len()),
                    "filter marks".to_string(),
                    self.lance_query.clone(),
                    lance_matches
                        .iter()
                        .position(|index| *index == self.selected_lance_file)
                        .unwrap_or(0),
                    lance_matches.len(),
                    10usize,
                ),
                ActivePicker::Recent => (
                    "recent".to_string(),
                    format!("{} files", self.recent_files.len()),
                    "filter recent files".to_string(),
                    self.recent_query.clone(),
                    recent_matches
                        .iter()
                        .position(|index| *index == self.selected_recent_file)
                        .unwrap_or(0),
                    recent_matches.len(),
                    10usize,
                ),
            };

        let selection_key = (
            selected_index,
            format!("{title}|{}|{query}", self.file_picker_dir.display()),
        );
        let selection_id = egui::Id::new("slate_picker_scroll_selection");
        let selection_changed = ctx.data_mut(|data| {
            let changed =
                data.get_temp::<(usize, String)>(selection_id).as_ref() != Some(&selection_key);
            data.insert_temp(selection_id, selection_key);
            changed
        });

        window_ui::DialogArea::new("slate_picker_dialog".into())
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
                        painter.text(
                            header.left_center(),
                            egui::Align2::LEFT_CENTER,
                            &title,
                            title_font,
                            accent,
                        );
                        let title_width = title.len() as f32 * 10.0 + 10.0;
                        painter.text(
                            header.left_center() + Vec2::new(title_width, 0.0),
                            egui::Align2::LEFT_CENTER,
                            Self::text_for_width(
                                &subtitle,
                                (header.width() - title_width - 100.0).max(0.0),
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
                        ui.add_space(4.0);
                        let input_height = 24.0;
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
                        let query_text = if query.is_empty() {
                            prompt
                        } else {
                            query.clone()
                        };
                        let query_color = if query.is_empty() { faint } else { text };
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
                            query_text,
                            font.clone(),
                            query_color,
                        );
                        let cursor_x = if query.is_empty() {
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
                        let list_height = (ui.available_height() - 28.0).max(row_height);
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(list_rect);
                        let columns = file_dialogs::PickerColumns::new(
                            list_rect.width(),
                            matches!(active, ActivePicker::File),
                        );
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
                        if let Some((x, _)) = columns.detail {
                            painter.text(
                                egui::pos2(list_rect.left() + x, header_y),
                                egui::Align2::LEFT_CENTER,
                                "path / detail",
                                font.clone(),
                                faint,
                            );
                        }
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

                        if total_len == 0 {
                            painter.text(
                                list_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "no matches",
                                font.clone(),
                                faint,
                            );
                        }

                        let body_rect = egui::Rect::from_min_max(
                            list_rect.min + Vec2::new(4.0, row_height),
                            list_rect.max - Vec2::new(4.0, 0.0),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(body_rect), |ui| {
                            ui.set_clip_rect(ui.clip_rect().intersect(body_rect));
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let mut scroll = egui::ScrollArea::vertical()
                                .id_salt("slate_picker_rows")
                                .min_scrolled_height(0.0)
                                .auto_shrink([false, false])
                                .max_height(body_rect.height());
                            if selection_changed {
                                scroll = scroll.vertical_scroll_offset(
                                    (selected_index as f32 * row_height
                                        - (body_rect.height() - row_height) * 0.5)
                                        .max(0.0),
                                );
                            }
                            scroll.show_rows(ui, row_height, total_len, |ui, range| {
                                for match_position in range {
                                    let (row_rect, _) = ui.allocate_exact_size(
                                        Vec2::new(ui.available_width(), row_height),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter_at(row_rect);
                                    let selected = match_position == selected_index;
                                    if selected {
                                        painter.rect_filled(
                                            row_rect,
                                            0.0,
                                            Color32::from_rgb(38, 47, 61),
                                        );
                                    }

                                    let (
                                        item_index,
                                        name,
                                        detail,
                                        size_label,
                                        modified_label,
                                        is_dir,
                                    ) = match active {
                                        ActivePicker::File => {
                                            let index = file_matches[match_position];
                                            let path = &self.project_files[index];
                                            let name = path
                                                .file_name()
                                                .and_then(|name| name.to_str())
                                                .unwrap_or("unknown");
                                            let relative = path
                                                .strip_prefix(&self.file_picker_dir)
                                                .unwrap_or(path)
                                                .display()
                                                .to_string();
                                            let (size, modified) = if path.is_dir() {
                                                ("dir".to_string(), "".to_string())
                                            } else {
                                                file_dialogs::file_metadata_labels(path)
                                            };
                                            (
                                                index,
                                                name.to_string(),
                                                relative,
                                                size,
                                                modified,
                                                path.is_dir(),
                                            )
                                        }
                                        ActivePicker::Lance => {
                                            let index = lance_matches[match_position];
                                            let path = &self.lance_files[index];
                                            let name = self.aliases.labeled(path);
                                            (
                                                index,
                                                format!("{} {}", index + 1, name),
                                                path.display().to_string(),
                                                String::new(),
                                                String::new(),
                                                false,
                                            )
                                        }
                                        ActivePicker::Recent => {
                                            let index = recent_matches[match_position];
                                            let path = &self.recent_files[index];
                                            let name = self.aliases.labeled(path);
                                            (
                                                index,
                                                name.to_string(),
                                                path.display().to_string(),
                                                String::new(),
                                                String::new(),
                                                false,
                                            )
                                        }
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
                                            &format!("{}{}", if is_dir { "▸ " } else { "" }, name),
                                            columns.name_width - 16.0,
                                            13.0,
                                        ),
                                        font.clone(),
                                        if selected { text } else { accent },
                                    );
                                    if let Some((x, width)) = columns.detail {
                                        painter.text(
                                            egui::pos2(list_rect.left() + x, y),
                                            egui::Align2::LEFT_CENTER,
                                            Self::text_for_width(&detail, width, 13.0),
                                            font.clone(),
                                            if selected { dim } else { faint },
                                        );
                                    }
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
                                        ui.id().with(("slate_picker", match_position)),
                                        egui::Sense::click(),
                                    );
                                    if response.clicked() || response.double_clicked() {
                                        match active {
                                            ActivePicker::File => {
                                                self.selected_project_file = item_index
                                            }
                                            ActivePicker::Lance => {
                                                self.selected_lance_file = item_index
                                            }
                                            ActivePicker::Recent => {
                                                self.selected_recent_file = item_index
                                            }
                                        }
                                    }
                                    if response.double_clicked() {
                                        match active {
                                            ActivePicker::File => self.open_selected_project_file(),
                                            ActivePicker::Lance => self.open_selected_lance_file(),
                                            ActivePicker::Recent => {
                                                self.open_selected_recent_file()
                                            }
                                        }
                                        ctx.request_repaint(); // Build the opened editor without waiting for another input event.
                                        perf_log::mark(
                                            "file_picker.open_action_complete",
                                            "immediate repaint requested",
                                        );
                                        break; // Directory navigation replaces the backing entry list.
                                    }
                                }
                            });
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let actions: &[(&str, &str)] = match active {
                                ActivePicker::File
                                    if self.file_picker_mode
                                        == FilePickerMode::SetPeriscopeProject =>
                                {
                                    &[
                                        ("↑↓", "select"),
                                        ("→", "navigate"),
                                        ("Enter", "choose folder"),
                                        ("Ctrl+Enter", "use current folder"),
                                        ("Esc", "cancel"),
                                    ]
                                }
                                ActivePicker::File => &[
                                    ("↑↓", "select"),
                                    ("type", "filter"),
                                    ("→", "enter dir"),
                                    ("←", "parent/drives"),
                                    ("Ctrl+L", "drives"),
                                    ("enter", "open"),
                                    ("esc", "close"),
                                ],
                                ActivePicker::Lance => &[
                                    ("↑↓", "select"),
                                    ("type", "filter"),
                                    ("enter", "open"),
                                    ("Ctrl+Enter", "modal"),
                                    ("Ctrl+A", "add"),
                                    ("Del", "remove"),
                                ],
                                ActivePicker::Recent => &[
                                    ("↑↓", "select"),
                                    ("type", "filter"),
                                    ("enter", "open"),
                                    ("esc", "close"),
                                ],
                            };
                            let hints = actions
                                .iter()
                                .map(|(key, label)| format!("[{key}] {label}"))
                                .collect::<Vec<_>>()
                                .join("  ");
                            ui.add(
                                egui::Label::new(
                                    RichText::new(hints).font(font.clone()).color(dim),
                                )
                                .truncate(),
                            );
                        });
                    });
            });
        if self.file_picker_open {
            if let Some(dir) = drive_picker::show(ctx) {
                self.file_picker_dir = dir;
                self.file_query.clear();
                self.refresh_file_picker_entries();
            }
        }
    }
}
