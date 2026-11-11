//! Settings modal: one tab of rows at a time (Tab / Shift+Tab switch tabs).
use super::*;

const SELECTED_FILL: Color32 = Color32::from_rgb(46, 56, 72);
const NORMAL_FILL: Color32 = Color32::from_rgb(30, 36, 48);
const ACCENT: Color32 = Color32::from_rgb(136, 192, 208);
const TEXT: Color32 = Color32::from_rgb(216, 222, 233);
const HINT: Color32 = Color32::from_rgb(136, 154, 176);

fn row_frame(selected: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(if selected { SELECTED_FILL } else { NORMAL_FILL })
        .inner_margin(6.0)
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(FontId::monospace(13.0)).color(HINT));
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).font(FontId::monospace(14.0)).color(ACCENT));
}

/// A row with a title, its control on the right and a hint below.
fn titled_row(
    ui: &mut egui::Ui,
    selected: bool,
    title: &str,
    hint_text: &str,
    control: impl FnOnce(&mut egui::Ui),
) -> egui::Rect {
    let row = row_frame(selected).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if selected { ">" } else { " " })
                    .font(FontId::monospace(14.0))
                    .color(ACCENT),
            );
            ui.label(RichText::new(title).font(FontId::monospace(14.0)).color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
        });
        ui.add_space(4.0);
        hint(ui, hint_text);
    });
    ui.add_space(6.0);
    row.response.rect
}

/// A row that is a single checkbox; true when it was clicked.
fn checkbox_row(ui: &mut egui::Ui, selected: bool, value: bool, label: &str) -> (egui::Rect, Option<bool>) {
    let mut changed = None;
    let row = row_frame(selected).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut on = value;
        if ui
            .checkbox(&mut on, RichText::new(label).font(FontId::monospace(14.0)))
            .changed()
        {
            changed = Some(on);
        }
    });
    (row.response.rect, changed)
}

impl SlateApp {
    pub(super) fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let reveal_selected = self.settings_last_selected != Some(self.selected_setting);
        self.settings_last_selected = Some(self.selected_setting);
        let tabs = self.settings_tabs();
        let tab = self.settings_tab();
        let mut setting_rects: Vec<(usize, egui::Rect)> = Vec::new();
        #[cfg(test)]
        let mut geometry = SettingsDialogGeometry::default();
        let dialog_rect = window_ui::DialogArea::new("settings_dialog".into())
            .fixed_chrome()
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -30.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new().inner_margin(14).show(ui, |ui| {
                    ui.set_width((ctx.content_rect().width() * 0.8 - 28.0).max(1.0));
                    ui.set_height(((ctx.content_rect().height() - 30.0) * 0.8 - 28.0).max(1.0));
                    let _header = ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("settings")
                                .font(FontId::new(16.0, FontFamily::Monospace))
                                .color(ACCENT),
                        );
                        ui.add_space(12.0);
                        for (index, (name, rows)) in tabs.iter().enumerate() {
                            let label = RichText::new(*name).font(FontId::monospace(14.0));
                            let label = if index == tab {
                                label.color(TEXT)
                            } else {
                                label.color(HINT)
                            };
                            if ui.selectable_label(index == tab, label).clicked() {
                                self.selected_setting = rows[0];
                            }
                        }
                    });
                    #[cfg(test)]
                    {
                        geometry.header = _header.response.rect;
                    }
                    ui.add_space(10.0);

                    let body = egui::ScrollArea::vertical()
                        .id_salt(("settings_body", tab))
                        .auto_shrink([false, false])
                        .min_scrolled_height(1.0)
                        .max_height((ui.available_height() - 52.0).max(1.0))
                        .show(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                            match tabs[tab].0 {
                                "General" => self.settings_general(ui, &mut setting_rects),
                                "Editor" => self.settings_editor(ui, &mut setting_rects),
                                "Cursor" => self.settings_cursor(ui, &mut setting_rects),
                                "Footer" => self.settings_footer(ui, &mut setting_rects),
                                "Periscope" => self.settings_periscope(ui, &mut setting_rects),
                                _ => self.settings_modules(ui, &mut setting_rects),
                            }
                            if reveal_selected
                                && let Some((_, rect)) = setting_rects
                                    .iter()
                                    .find(|(row, _)| *row == self.selected_setting)
                            {
                                ui.scroll_to_rect(*rect, Some(egui::Align::Min));
                            }
                        });
                    #[cfg(test)]
                    {
                        geometry.body = body.inner_rect;
                        geometry.rows = vec![egui::Rect::NOTHING; self.settings_last_row() + 1];
                        for (row, rect) in &setting_rects {
                            geometry.rows[*row] = *rect;
                        }
                    }
                    let _ = body;
                    ui.add_space(12.0);
                    let _footer = ui.horizontal_wrapped(|ui| {
                        for (key, label) in [
                            ("tab", "next tab"),
                            ("↑↓", "select"),
                            ("←→", "adjust"),
                            ("enter", "toggle"),
                            ("esc", "close"),
                        ] {
                            ui.label(
                                RichText::new(format!("[{key}]"))
                                    .font(FontId::new(13.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(235, 203, 139)),
                            );
                            ui.label(
                                RichText::new(label)
                                    .font(FontId::new(13.0, FontFamily::Monospace))
                                    .color(HINT),
                            );
                            ui.add_space(10.0);
                        }
                    });
                    #[cfg(test)]
                    {
                        geometry.footer = _footer.response.rect;
                    }
                });
            });
        #[cfg(test)]
        {
            geometry.frame = dialog_rect;
            ctx.data_mut(|d| d.insert_temp("settings_geometry".into(), geometry));
        }
        let _ = dialog_rect;
    }

    fn settings_general(&mut self, ui: &mut egui::Ui, rects: &mut Vec<(usize, egui::Rect)>) {
        let selected = self.selected_setting;
        let rect = titled_row(
            ui,
            selected == 0,
            "History length",
            "Visible command history rows when Ctrl+. opens the commandline.",
            |ui| {
                let response = ui.add(
                    egui::DragValue::new(&mut self.command_history_limit)
                        .range(1..=50)
                        .speed(1),
                );
                if response.changed() {
                    self.set_command_history_limit(self.command_history_limit);
                }
            },
        );
        rects.push((0, rect));
        let movement_hint = self.ctrl_shift_move_mode.hint().to_string();
        let rect = titled_row(ui, selected == 2, "Ctrl+Shift movement", &movement_hint, |ui| {
            if ui
                .button(self.ctrl_shift_move_mode.label())
                .on_hover_text("Toggle Vim hjkl / Slate ijkl live movement")
                .clicked()
            {
                self.set_ctrl_shift_move_mode(self.ctrl_shift_move_mode.next());
            }
        });
        rects.push((2, rect));
        let rect = titled_row(
            ui,
            selected == 6,
            "Reopen last file on startup",
            "Off by default. Ctrl+O L / open-last remains the intentional manual workflow.",
            |ui| {
                if ui
                    .button(if self.reopen_last_file_on_startup { "on" } else { "off" })
                    .on_hover_text("Open Slate with the last opened file when no file path is provided")
                    .clicked()
                {
                    self.set_reopen_last_file_on_startup(!self.reopen_last_file_on_startup);
                }
            },
        );
        rects.push((6, rect));
        section(ui, "Window · resident mode");
        let (rect, changed) = checkbox_row(
            ui,
            selected == 19,
            self.resident,
            "Keep running in the tray when closed",
        );
        rects.push((19, rect));
        if let Some(on) = changed {
            self.selected_setting = 19;
            self.set_resident(on);
        }
        hint(
            ui,
            "Reopening is instant (~20 ms instead of ~300 ms). Quit for real from the tray icon or :quit-all.",
        );
        let (rect, changed) = checkbox_row(
            ui,
            selected == 20,
            self.start_with_windows,
            "Start hidden with Windows",
        );
        rects.push((20, rect));
        if let Some(on) = changed {
            self.selected_setting = 20;
            self.set_start_with_windows(on);
        }
        hint(
            ui,
            "Makes the first open of the day instant too. Current user only; off by default.",
        );
    }

    fn settings_editor(&mut self, ui: &mut egui::Ui, rects: &mut Vec<(usize, egui::Rect)>) {
        let selected = self.selected_setting;
        let rect = titled_row(
            ui,
            selected == 1,
            "Line numbers",
            "Absolute shows file line numbers. Relative treats the cursor line as 1 and counts distance above/below.",
            |ui| {
                let next_mode = match self.line_number_mode {
                    LineNumberMode::Absolute => LineNumberMode::Relative,
                    LineNumberMode::Relative => LineNumberMode::Absolute,
                };
                if ui
                    .button(self.line_number_mode.label())
                    .on_hover_text("Toggle absolute/relative line numbers")
                    .clicked()
                {
                    self.set_line_number_mode(next_mode);
                }
            },
        );
        rects.push((1, rect));
        let rect = titled_row(
            ui,
            selected == 3,
            "Word wrap",
            "Persisted editor wrapping preference. Command: :wrap on|off",
            |ui| {
                if ui
                    .button(if self.wrap { "on" } else { "off" })
                    .on_hover_text("Toggle editor word wrap")
                    .clicked()
                {
                    self.set_wrap_mode(!self.wrap);
                }
            },
        );
        rects.push((3, rect));
        let rect = titled_row(
            ui,
            selected == 4,
            "Markdown preview",
            "Persisted preview/split preference. Command: :preview on|off",
            |ui| {
                if ui
                    .button(if self.preview { "on" } else { "off" })
                    .on_hover_text("Toggle Markdown preview split")
                    .clicked()
                {
                    self.set_preview_mode(!self.preview);
                }
            },
        );
        rects.push((4, rect));
        let rect = titled_row(
            ui,
            selected == 5,
            "Markdown live rendering",
            "Preview split always keeps the editor side plain and renders Markdown on the right.",
            |ui| {
                if ui
                    .button(if self.markdown_live_rendering { "live" } else { "plain" })
                    .on_hover_text(
                        "Choose plain source text or live inline Markdown affordances while editing",
                    )
                    .clicked()
                {
                    self.set_markdown_live_rendering(!self.markdown_live_rendering);
                }
            },
        );
        rects.push((5, rect));
        let rect = titled_row(
            ui,
            selected == 7,
            "Editor font size",
            "8–32 pt. Default: 12 pt, matching line numbers. Enter resets the size.",
            |ui| {
                let mut size = self.editor_font_size;
                if ui
                    .add(
                        egui::DragValue::new(&mut size)
                            .range(8.0..=32.0)
                            .speed(1.0)
                            .suffix(" pt"),
                    )
                    .changed()
                {
                    self.set_editor_font_size(size);
                }
            },
        );
        rects.push((7, rect));
        let look = self.editor_look;
        let row = row_frame(selected == 21).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Font").font(FontId::monospace(14.0)));
                for font in [
                    editor_view::EditorFont::Hack,
                    editor_view::EditorFont::JetBrainsMono,
                ] {
                    let sample = RichText::new(font.label()).font(FontId::new(14.0, font.family()));
                    if ui.selectable_label(look.font == font, sample).clicked() {
                        self.selected_setting = 21;
                        self.set_editor_look(editor_view::EditorLook { font, ..look });
                    }
                }
            });
        });
        rects.push((21, row.response.rect));
        ui.add_space(6.0);
        let (rect, changed) = checkbox_row(
            ui,
            selected == 30,
            look.syntax_highlighting,
            "Syntax highlighting in code files and fenced code",
        );
        rects.push((30, rect));
        if let Some(on) = changed {
            self.selected_setting = 30;
            self.set_editor_look(editor_view::EditorLook {
                syntax_highlighting: on,
                ..look
            });
        }
    }

    fn settings_cursor(&mut self, ui: &mut egui::Ui, rects: &mut Vec<(usize, egui::Rect)>) {
        let selected = self.selected_setting;
        let look = self.editor_look;
        let set = |app: &mut Self, row: usize, look: editor_view::EditorLook| {
            app.selected_setting = row;
            app.set_editor_look(look);
        };
        let toggles: [(usize, bool, &str, fn(editor_view::EditorLook, bool) -> editor_view::EditorLook); 2] = [
            (22, look.block_cursor, "Block cursor (covers the whole character)", |look, on| {
                editor_view::EditorLook { block_cursor: on, ..look }
            }),
            (23, look.animate_cursor, "Animate cursor jumps", |look, on| {
                editor_view::EditorLook { animate_cursor: on, ..look }
            }),
        ];
        for (row, value, label, apply) in toggles {
            let (rect, changed) = checkbox_row(ui, selected == row, value, label);
            rects.push((row, rect));
            if let Some(on) = changed {
                set(self, row, apply(look, on));
            }
        }
        let row = row_frame(selected == 24).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Cursor color").font(FontId::monospace(14.0)));
                for (name, color) in editor_view::CURSOR_COLORS {
                    let (swatch, response) =
                        ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::click());
                    ui.painter().rect_filled(swatch.shrink(1.0), 2.0, color);
                    if look.cursor_color == color {
                        ui.painter().rect_stroke(
                            swatch,
                            2.0,
                            egui::Stroke::new(1.5_f32, Color32::WHITE),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.on_hover_text(name).clicked() {
                        set(self, 24, editor_view::EditorLook { cursor_color: color, ..look });
                    }
                }
                let mut rgb = [
                    look.cursor_color.r(),
                    look.cursor_color.g(),
                    look.cursor_color.b(),
                ];
                if ui
                    .color_edit_button_srgb(&mut rgb)
                    .on_hover_text("any color")
                    .changed()
                {
                    set(
                        self,
                        24,
                        editor_view::EditorLook {
                            cursor_color: Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                            ..look
                        },
                    );
                }
            });
        });
        rects.push((24, row.response.rect));
        let (rect, changed) = checkbox_row(
            ui,
            selected == 25,
            look.cursor_trail,
            "Cursor trail while it glides",
        );
        rects.push((25, rect));
        if let Some(on) = changed {
            set(self, 25, editor_view::EditorLook { cursor_trail: on, ..look });
        }
        let row = row_frame(selected == 26).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Trail weight").font(FontId::monospace(14.0)));
                let mut weight = look.cursor_trail_weight;
                if ui.add(egui::Slider::new(&mut weight, 1..=5)).changed() {
                    set(
                        self,
                        26,
                        editor_view::EditorLook {
                            cursor_trail_weight: weight,
                            ..look
                        },
                    );
                }
            });
        });
        rects.push((26, row.response.rect));
        let toggles: [(usize, bool, &str, fn(editor_view::EditorLook, bool) -> editor_view::EditorLook); 3] = [
            (27, look.cursor_trail_typing, "Trail while typing too", |look, on| {
                editor_view::EditorLook { cursor_trail_typing: on, ..look }
            }),
            (28, look.cursor_stretch, "Stretch toward the jump", |look, on| {
                editor_view::EditorLook { cursor_stretch: on, ..look }
            }),
            (29, look.cursor_pulse, "Pulse where it lands", |look, on| {
                editor_view::EditorLook { cursor_pulse: on, ..look }
            }),
        ];
        for (row, value, label, apply) in toggles {
            let (rect, changed) = checkbox_row(ui, selected == row, value, label);
            rects.push((row, rect));
            if let Some(on) = changed {
                set(self, row, apply(look, on));
            }
        }
        hint(
            ui,
            "The cursor glides when it jumps and lands at 90 ms. Text is never delayed: only the cursor and its trail animate. Weight 1–5: longer, stronger trail; Enter resets to 3.",
        );
    }

    fn settings_footer(&mut self, ui: &mut egui::Ui, rects: &mut Vec<(usize, egui::Rect)>) {
        hint(
            ui,
            "Choose what the footer shows. Secondary fields hide first when space is tight; hover the footer for all details.",
        );
        for (index, (_, label)) in footer_ui::FIELDS.iter().enumerate() {
            ui.add_space(6.0);
            let row = index + 8;
            let (rect, changed) = checkbox_row(
                ui,
                self.selected_setting == row,
                self.footer_options.enabled[index],
                label,
            );
            rects.push((row, rect));
            if let Some(on) = changed {
                self.selected_setting = row;
                self.set_footer_option(index, on);
            }
        }
    }

    fn settings_periscope(&mut self, ui: &mut egui::Ui, rects: &mut Vec<(usize, egui::Rect)>) {
        let (rect, changed) = checkbox_row(
            ui,
            self.selected_setting == 18,
            self.periscope_everything,
            "Use Everything when available",
        );
        rects.push((18, rect));
        if let Some(on) = changed {
            self.selected_setting = 18;
            self.set_periscope_everything(on);
        }
        hint(
            ui,
            "Otherwise: SQLite metadata index in the background. No service is installed.",
        );
        let scope = if self.periscope_roots.is_empty() {
            "Global scope: local fixed drives (Windows).".to_string()
        } else {
            format!(
                "Global scope: {}",
                self.periscope_roots
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
        };
        hint(ui, &scope);
    }

    fn settings_modules(&mut self, ui: &mut egui::Ui, rects: &mut Vec<(usize, egui::Rect)>) {
        for (offset, (id, name, description, enabled)) in
            self.module_host.list().into_iter().enumerate()
        {
            let row = MODULE_ROWS_START + offset;
            let frame = row_frame(self.selected_setting == row).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut on = enabled;
                if ui
                    .checkbox(&mut on, RichText::new(name).font(FontId::monospace(14.0)))
                    .on_hover_text(description)
                    .changed()
                {
                    self.selected_setting = row;
                    self.set_module_enabled(id, on);
                }
                hint(ui, description);
            });
            rects.push((row, frame.response.rect));
            ui.add_space(6.0);
        }
        let vault_row = MODULE_ROWS_START + self.module_host.list().len();
        let mut remove = None;
        let frame = row_frame(self.selected_setting == vault_row).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Vaults").font(FontId::monospace(14.0)).color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Add folder…")
                        .on_hover_text("Choose a folder of notes")
                        .clicked()
                    {
                        self.selected_setting = vault_row;
                        self.open_vault_picker();
                    }
                    if ui
                        .button("Manage… (:vaults)")
                        .on_hover_text("All vaults: browse, add, remove, open in Explorer")
                        .clicked()
                    {
                        self.settings_open = false;
                        self.open_vaults_dialog();
                    }
                });
            });
            if self.vaults.is_empty() {
                hint(
                    ui,
                    "none yet · notes in a .git folder, or the file's own folder, are used instead",
                );
            }
            for (index, vault) in self.vaults.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("✕")
                        .on_hover_text("Stop using this folder as a vault (files are not touched)")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                    ui.label(
                        RichText::new(vault.display().to_string())
                            .font(FontId::monospace(13.0))
                            .color(Color32::from_rgb(190, 200, 216)),
                    );
                });
            }
        });
        rects.push((vault_row, frame.response.rect));
        if let Some(index) = remove {
            self.remove_vault(index);
        }
        hint(
            ui,
            "Modules add features on top of the editor; off means not running at all. Vaults are your note folders: modules that look across notes (backlinks…) search there.",
        );
    }
}
