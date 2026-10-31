//! Shortcut and command reference modal.
use super::*;

impl SlateApp {
    pub(super) fn shortcut_help_dialog(&mut self, ctx: &egui::Context) {
        if !self.shortcut_help_open {
            return;
        }

        let shortcut_groups: &[(&str, &[(&str, &str)])] = &[
            (
                "global",
                &[
                    ("Ctrl+.", "open commandline"),
                    ("Ctrl+P", "open command palette/browser"),
                    ("Ctrl+H", "open this help"),
                    ("Esc", "close modal / cancel active mode"),
                    ("Ctrl+S", "save"),
                    ("Ctrl+Alt+S", "save as"),
                    ("Ctrl+O", "open file"),
                    ("Ctrl+M", "toggle Markdown preview"),
                    ("Ctrl+T", "new tab"),
                    ("Ctrl+W", "close tab"),
                    ("Ctrl+Tab", "next tab (Shift: previous)"),
                    ("Ctrl+1…9", "go to tab (9 = last)"),
                    ("Ctrl+Shift+T", "reopen closed tab"),
                    ("double-click tab", "alias (name inside Slate)"),
                    ("Ctrl+wheel", "next / previous tab"),
                    ("Ctrl+Q", "close window (stays in tray)"),
                ],
            ),
            (
                "ctrl layer",
                &[
                    ("Ctrl: s", "save"),
                    ("Ctrl: t / w / q", "new tab / close tab / close window"),
                    ("Ctrl: o", "open file"),
                    ("Ctrl: ol", "open last file"),
                    ("Ctrl: r", "recent files"),
                    ("Ctrl: f/b", "find next / previous"),
                    ("Ctrl: sw/sl", "select word / line"),
                    ("Ctrl: dw/dl", "delete word / line"),
                    ("Ctrl: dup", "duplicate line"),
                    ("Ctrl: dupp", "duplicate and place"),
                    ("Ctrl: gt/gb", "go top / bottom"),
                ],
            ),
            (
                "movement layers",
                &[
                    ("Ctrl+Shift", "live cursor movement"),
                    ("CapsLock", "system nav layer: ijkl arrows"),
                    ("Alt up/down", "move line/block"),
                    ("Alt double", "move to paragraph edge"),
                    ("Alt left/right", "word selection"),
                    ("Shift+Alt", "paragraph / word / line-edge jumps"),
                    ("duplicate mode", "move · Enter/Space place · Esc cancel"),
                ],
            ),
            (
                "commandline",
                &[
                    ("Tab", "accept completion"),
                    ("↑/↓", "history / picker selection"),
                    ("Enter", "run command / accept picker item"),
                    (":recent query", "filter recent files"),
                    (":find text", "find in file"),
                    (":g 10 / :g +5", "absolute / relative goto"),
                ],
            ),
        ];

        let viewport_width = ctx.input(|i| {
            i.viewport()
                .inner_rect
                .map(|rect| rect.width())
                .unwrap_or_else(|| i.content_rect().width())
        });
        // This is the frame *content* width. The frame itself adds margin/stroke,
        // so keep extra room for tiled/WM-small windows instead of filling edge-to-edge.
        let modal_width = (viewport_width * 0.8 - 30.0).max(1.0);
        let shortcut_width = (modal_width * 0.32).clamp(300.0, 380.0);
        let gutter_width = 18.0;
        let command_width = (modal_width - shortcut_width - gutter_width - 30.0).max(260.0);
        let key_x = (shortcut_width * 0.28).clamp(84.0, 112.0);
        let arrow_x = key_x + 22.0;
        let desc_x = arrow_x + 24.0;
        let compact_help = modal_width < 900.0;
        let command_table_width = (command_width - 44.0).max(240.0);
        let command_col = if compact_help {
            (command_table_width * 0.72).max(160.0)
        } else {
            (command_table_width * 0.30).clamp(150.0, 240.0)
        };
        let aliases_col = if compact_help {
            0.0
        } else {
            (command_table_width * 0.17).clamp(90.0, 140.0)
        };
        let summary_col = if compact_help {
            0.0
        } else {
            (command_table_width * 0.37).clamp(220.0, 330.0)
        };
        let hint_col = if compact_help {
            (command_table_width - command_col - 10.0).max(110.0)
        } else {
            (command_table_width * 0.16).clamp(110.0, 150.0)
        };

        window_ui::DialogArea::new("shortcut_help_dialog".into())
            .fixed_chrome()
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
                        ui.set_width(modal_width);
                        ui.set_height(((ctx.content_rect().height() - 30.0) * 0.8 - 30.0).max(1.0));
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("shortcuts + commands")
                                    .font(FontId::new(16.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(136, 192, 208)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close")
                                            .font(FontId::new(13.0, FontFamily::Monospace))
                                            .color(Color32::from_rgb(235, 203, 139)),
                                    );
                                },
                            );
                        });
                        ui.add_space(8.0);

                        egui::ScrollArea::both()
                            .id_salt("shortcut_help_body")
                            .auto_shrink([false, false])
                            .max_height(ui.available_height())
                            .show(ui, |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(shortcut_width);
                                        for (title, entries) in shortcut_groups {
                                            ui.label(
                                                RichText::new(*title)
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(163, 190, 140)),
                                            );
                                            ui.add_space(3.0);
                                            for (key, desc) in *entries {
                                                let (row_rect, _) = ui.allocate_exact_size(
                                                    Vec2::new(shortcut_width, 15.0),
                                                    egui::Sense::hover(),
                                                );
                                                let painter = ui.painter_at(row_rect);
                                                let y = row_rect.center().y - 0.5;
                                                painter.text(
                                                    egui::pos2(row_rect.left() + key_x, y),
                                                    egui::Align2::RIGHT_CENTER,
                                                    *key,
                                                    FontId::new(12.5, FontFamily::Monospace),
                                                    Color32::from_rgb(235, 203, 139),
                                                );
                                                painter.text(
                                                    egui::pos2(row_rect.left() + arrow_x, y),
                                                    egui::Align2::CENTER_CENTER,
                                                    "→",
                                                    FontId::new(12.5, FontFamily::Monospace),
                                                    Color32::from_rgb(94, 105, 126),
                                                );
                                                painter.text(
                                                    egui::pos2(row_rect.left() + desc_x, y),
                                                    egui::Align2::LEFT_CENTER,
                                                    Self::text_for_width(
                                                        Self::compact_shortcut_desc(desc),
                                                        (shortcut_width - desc_x - 4.0).max(24.0),
                                                        12.5,
                                                    ),
                                                    FontId::new(12.5, FontFamily::Monospace),
                                                    Color32::from_rgb(216, 222, 233),
                                                );
                                            }
                                            ui.add_space(7.0);
                                        }
                                    });

                                    ui.add_space(gutter_width);
                                    ui.vertical(|ui| {
                                        ui.set_width(command_width);
                                        ui.label(
                                            RichText::new("available commands")
                                                .font(FontId::new(14.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(163, 190, 140)),
                                        );
                                        ui.add_space(3.0);
                                        let header_height = 16.0;
                                        let row_height = 15.0;
                                        let table_height = header_height
                                            + COMMAND_SPECS.len() as f32 * row_height
                                            + 16.0;
                                        let (table_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(command_width, table_height),
                                            egui::Sense::hover(),
                                        );
                                        let painter =
                                            ui.painter_at(table_rect).with_clip_rect(table_rect);
                                        painter.rect_filled(
                                            table_rect,
                                            0.0,
                                            Color32::from_rgb(22, 28, 37),
                                        );
                                        painter.rect_stroke(
                                            table_rect,
                                            0.0,
                                            Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
                                            egui::StrokeKind::Outside,
                                        );

                                        let left = table_rect.left() + 8.0;
                                        let top = table_rect.top() + 8.0;
                                        let text_y_offset = 0.5;
                                        if compact_help {
                                            let cols = [command_col, hint_col];
                                            let col_lefts = [left, left + command_col + 10.0];
                                            for (idx, header) in
                                                ["command", "hint"].iter().enumerate()
                                            {
                                                painter.text(
                                                    egui::pos2(
                                                        col_lefts[idx] + cols[idx] * 0.5,
                                                        top + header_height * 0.5,
                                                    ),
                                                    egui::Align2::CENTER_CENTER,
                                                    *header,
                                                    FontId::new(11.5, FontFamily::Monospace),
                                                    Color32::from_rgb(136, 154, 176),
                                                );
                                            }

                                            for (row, spec) in COMMAND_SPECS.iter().enumerate() {
                                                let row_top =
                                                    top + header_height + row as f32 * row_height;
                                                let row_rect = egui::Rect::from_min_size(
                                                    egui::pos2(table_rect.left() + 4.0, row_top),
                                                    Vec2::new(command_width - 8.0, row_height),
                                                );
                                                if row % 2 == 0 {
                                                    painter.rect_filled(
                                                        row_rect,
                                                        0.0,
                                                        Color32::from_rgb(38, 47, 61),
                                                    );
                                                }
                                                let y = row_rect.center().y - text_y_offset;
                                                let command = Self::text_for_width(
                                                    &format!(":{}", spec.name),
                                                    command_col - 6.0,
                                                    12.5,
                                                );
                                                let hint = Self::text_for_width(
                                                    spec.hint,
                                                    hint_col - 6.0,
                                                    11.5,
                                                );
                                                for (idx, (text, color, size)) in [
                                                    (
                                                        command,
                                                        Color32::from_rgb(136, 192, 208),
                                                        12.5,
                                                    ),
                                                    (hint, Color32::from_rgb(235, 203, 139), 11.5),
                                                ]
                                                .into_iter()
                                                .enumerate()
                                                {
                                                    painter.text(
                                                        egui::pos2(
                                                            col_lefts[idx] + cols[idx] * 0.5,
                                                            y,
                                                        ),
                                                        egui::Align2::CENTER_CENTER,
                                                        text,
                                                        FontId::new(size, FontFamily::Monospace),
                                                        color,
                                                    );
                                                }
                                            }
                                        } else {
                                            let cols =
                                                [command_col, aliases_col, summary_col, hint_col];
                                            let mut col_lefts = [left, left, left, left];
                                            for idx in 1..cols.len() {
                                                col_lefts[idx] =
                                                    col_lefts[idx - 1] + cols[idx - 1] + 10.0;
                                            }
                                            for (idx, header) in
                                                ["command", "aliases", "summary", "hint"]
                                                    .iter()
                                                    .enumerate()
                                            {
                                                painter.text(
                                                    egui::pos2(
                                                        col_lefts[idx] + cols[idx] * 0.5,
                                                        top + header_height * 0.5,
                                                    ),
                                                    egui::Align2::CENTER_CENTER,
                                                    *header,
                                                    FontId::new(11.5, FontFamily::Monospace),
                                                    Color32::from_rgb(136, 154, 176),
                                                );
                                            }

                                            for (row, spec) in COMMAND_SPECS.iter().enumerate() {
                                                let row_top =
                                                    top + header_height + row as f32 * row_height;
                                                let row_rect = egui::Rect::from_min_size(
                                                    egui::pos2(table_rect.left() + 4.0, row_top),
                                                    Vec2::new(command_width - 8.0, row_height),
                                                );
                                                if row % 2 == 0 {
                                                    painter.rect_filled(
                                                        row_rect,
                                                        0.0,
                                                        Color32::from_rgb(38, 47, 61),
                                                    );
                                                }
                                                let y = row_rect.center().y - text_y_offset;
                                                let aliases = if spec.aliases.is_empty() {
                                                    "".to_string()
                                                } else {
                                                    spec.aliases.join(", ")
                                                };
                                                let command = Self::text_for_width(
                                                    &format!(":{}", spec.name),
                                                    command_col - 6.0,
                                                    12.5,
                                                );
                                                let aliases = Self::text_for_width(
                                                    &aliases,
                                                    aliases_col - 6.0,
                                                    11.5,
                                                );
                                                let summary = Self::text_for_width(
                                                    spec.summary,
                                                    summary_col - 6.0,
                                                    11.5,
                                                );
                                                let hint = Self::text_for_width(
                                                    spec.hint,
                                                    hint_col - 6.0,
                                                    11.5,
                                                );
                                                for (idx, (text, color, size)) in [
                                                    (
                                                        command,
                                                        Color32::from_rgb(136, 192, 208),
                                                        12.5,
                                                    ),
                                                    (
                                                        aliases,
                                                        Color32::from_rgb(94, 105, 126),
                                                        11.5,
                                                    ),
                                                    (
                                                        summary,
                                                        Color32::from_rgb(136, 154, 176),
                                                        11.5,
                                                    ),
                                                    (hint, Color32::from_rgb(235, 203, 139), 11.5),
                                                ]
                                                .into_iter()
                                                .enumerate()
                                                {
                                                    painter.text(
                                                        egui::pos2(
                                                            col_lefts[idx] + cols[idx] * 0.5,
                                                            y,
                                                        ),
                                                        egui::Align2::CENTER_CENTER,
                                                        text,
                                                        FontId::new(size, FontFamily::Monospace),
                                                        color,
                                                    );
                                                }
                                            }
                                        }
                                    });
                                });
                            });
                    });
            });
    }
}
