//! Settings modal.
use super::*;

impl SlateApp {
    pub(super) fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let reveal_selected = self.settings_last_selected != Some(self.selected_setting);
        self.settings_last_selected = Some(self.selected_setting);
        let mut setting_rects = Vec::new();
        #[cfg(test)]
        let mut geometry = SettingsDialogGeometry::default();
        let dialog_rect = window_ui::DialogArea::new("settings_dialog".into())
            .fixed_chrome()
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -30.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .inner_margin(14)
                    .show(ui, |ui| {
                        ui.set_width((ctx.content_rect().width() * 0.8 - 28.0).max(1.0));
                        ui.set_height(((ctx.content_rect().height() - 30.0) * 0.8 - 28.0).max(1.0));
                        let _header = ui.label(
                            RichText::new("settings")
                                .font(FontId::new(16.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(136, 192, 208)),
                        );
                        #[cfg(test)] { geometry.header = _header.rect; }
                        ui.add_space(10.0);

                        let body = egui::ScrollArea::vertical()
                            .id_salt("settings_body")
                            .auto_shrink([false, false])
                            .min_scrolled_height(1.0)
                            .max_height((ui.available_height() - 52.0).max(1.0))
                            .show(ui, |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                let selected_fill = Color32::from_rgb(46, 56, 72);
                                let normal_fill = Color32::from_rgb(30, 36, 48);

                                let history_selected = self.selected_setting == 0;
                                let row = egui::Frame::new()
                                    .fill(if history_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if history_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("History length")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let response = ui.add(
                                                        egui::DragValue::new(&mut self.command_history_limit)
                                                            .range(1..=50)
                                                            .speed(1),
                                                    );
                                                    if response.changed() {
                                                        self.set_command_history_limit(
                                                            self.command_history_limit,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Visible command history rows when Ctrl+. opens the commandline.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                setting_rects.push(row.response.rect);
                                let line_numbers_selected = self.selected_setting == 1;
                                let row = egui::Frame::new()
                                    .fill(if line_numbers_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if line_numbers_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Line numbers")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
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
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Absolute shows file line numbers. Relative treats the cursor line as 1 and counts distance above/below.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                setting_rects.push(row.response.rect);
                                let movement_selected = self.selected_setting == 2;
                                let row = egui::Frame::new()
                                    .fill(if movement_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if movement_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Ctrl+Shift movement")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(self.ctrl_shift_move_mode.label())
                                                        .on_hover_text("Toggle Vim hjkl / Slate ijkl live movement")
                                                        .clicked()
                                                    {
                                                        self.set_ctrl_shift_move_mode(
                                                            self.ctrl_shift_move_mode.next(),
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(self.ctrl_shift_move_mode.hint())
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                setting_rects.push(row.response.rect);
                                let wrap_selected = self.selected_setting == 3;
                                let row = egui::Frame::new()
                                    .fill(if wrap_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if wrap_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Word wrap")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
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
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Persisted editor wrapping preference. Command: :wrap on|off")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                setting_rects.push(row.response.rect);
                                let preview_selected = self.selected_setting == 4;
                                let row = egui::Frame::new()
                                    .fill(if preview_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if preview_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Markdown preview")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
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
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Persisted preview/split preference. Command: :preview on|off")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                setting_rects.push(row.response.rect);
                                let markdown_live_selected = self.selected_setting == 5;
                                let row = egui::Frame::new()
                                    .fill(if markdown_live_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if markdown_live_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Markdown live rendering")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.markdown_live_rendering { "live" } else { "plain" })
                                                        .on_hover_text("Choose plain source text or live inline Markdown affordances while editing")
                                                        .clicked()
                                                    {
                                                        self.set_markdown_live_rendering(
                                                            !self.markdown_live_rendering,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Preview split always keeps the editor side plain and renders Markdown on the right.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                setting_rects.push(row.response.rect);
                                let virtual_refs_selected = self.selected_setting == 6;
                                let row = egui::Frame::new()
                                    .fill(if virtual_refs_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if virtual_refs_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Virtual backlinks/mentions")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.virtual_references_enabled { "show" } else { "hide" })
                                                        .on_hover_text("Render the virtual Backlinks/Mentions section at the end of saved Markdown files")
                                                        .clicked()
                                                    {
                                                        self.set_virtual_references_enabled(
                                                            !self.virtual_references_enabled,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Shows a non-saved end-of-file references section. Click the virtual header, Backlinks, or Mentions to collapse/expand.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                setting_rects.push(row.response.rect);
                                let reopen_selected = self.selected_setting == 7;
                                let row = egui::Frame::new()
                                    .fill(if reopen_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if reopen_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Reopen last file on startup")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.reopen_last_file_on_startup { "on" } else { "off" })
                                                        .on_hover_text("Open Slate with the last opened file when no file path is provided")
                                                        .clicked()
                                                    {
                                                        self.set_reopen_last_file_on_startup(
                                                            !self.reopen_last_file_on_startup,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Off by default. Ctrl+O L / open-last remains the intentional manual workflow.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });
                                setting_rects.push(row.response.rect);
                                ui.add_space(6.0);
                                let font_selected = self.selected_setting == 8;
                                let row = egui::Frame::new()
                                    .fill(if font_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(if font_selected { ">" } else { " " }).monospace().color(Color32::from_rgb(136, 192, 208)));
                                            ui.label(RichText::new("Editor font size").font(FontId::new(14.0, FontFamily::Monospace)));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let mut size = self.editor_font_size;
                                                if ui.add(egui::DragValue::new(&mut size).range(8.0..=32.0).speed(1.0).suffix(" pt")).changed() {
                                                    self.set_editor_font_size(size);
                                                }
                                            });
                                        });
                                        ui.label(RichText::new("8–32 pt. Default: 12 pt, matching line numbers. Enter resets the size.").font(FontId::new(13.0, FontFamily::Monospace)).color(Color32::from_rgb(136,154,176)));
                                    });
                                setting_rects.push(row.response.rect);
                                ui.add_space(10.0);
                                ui.label(RichText::new("Footer · choose visible information").font(FontId::new(14.0,FontFamily::Monospace)).color(Color32::from_rgb(136,192,208)));
                                ui.label(RichText::new("Secondary fields hide first when space is tight. Hover the footer for all details.").font(FontId::new(13.0,FontFamily::Monospace)).color(Color32::from_rgb(136,154,176)));
                                for (index,(_,label)) in footer_ui::FIELDS.iter().enumerate() {
                                    ui.add_space(6.0);
                                    let selected=self.selected_setting==index+9;
                                    let row=egui::Frame::new().fill(if selected {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(if selected {">"} else {" "}).monospace().color(Color32::from_rgb(136,192,208)));
                                            let mut enabled=self.footer_options.enabled[index];
                                            if ui.checkbox(&mut enabled,RichText::new(*label).font(FontId::new(14.0,FontFamily::Monospace))).changed() {self.selected_setting=index+9;self.set_footer_option(index,enabled);}
                                        });
                                    });
                                    setting_rects.push(row.response.rect);
                                }
                                ui.add_space(10.0);
                                ui.label(RichText::new("Periscope · file index").font(FontId::monospace(14.0)).color(Color32::from_rgb(136,192,208)));
                                let row=egui::Frame::new().fill(if self.selected_setting==19 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());let mut enabled=self.periscope_everything;
                                    if ui.checkbox(&mut enabled,RichText::new("Use Everything when available").font(FontId::monospace(14.0))).changed() {self.selected_setting=19;self.set_periscope_everything(enabled);}
                                });setting_rects.push(row.response.rect);
                                ui.label(RichText::new("Otherwise: SQLite metadata index in the background. No service is installed.").font(FontId::monospace(13.0)).color(Color32::from_rgb(136,154,176)));
                                ui.label(RichText::new(if self.periscope_roots.is_empty() {"Global scope: local fixed drives (Windows).".into()} else {format!("Global scope: {}",self.periscope_roots.iter().map(|root|root.display().to_string()).collect::<Vec<_>>().join(" · "))}).font(FontId::monospace(13.0)).color(Color32::from_rgb(136,154,176)));
                                ui.add_space(10.0);
                                ui.label(RichText::new("Window · resident mode").font(FontId::monospace(14.0)).color(Color32::from_rgb(136,192,208)));
                                let row=egui::Frame::new().fill(if self.selected_setting==20 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());let mut enabled=self.resident;
                                    if ui.checkbox(&mut enabled,RichText::new("Keep running in the tray when closed").font(FontId::monospace(14.0))).changed() {self.selected_setting=20;self.set_resident(enabled);}
                                });setting_rects.push(row.response.rect);
                                ui.label(RichText::new("Reopening is instant (~20 ms instead of ~300 ms). Quit for real from the tray icon or :quit-all.").font(FontId::monospace(13.0)).color(Color32::from_rgb(136,154,176)));
                                let row=egui::Frame::new().fill(if self.selected_setting==21 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());let mut enabled=self.start_with_windows;
                                    if ui.checkbox(&mut enabled,RichText::new("Start hidden with Windows").font(FontId::monospace(14.0))).changed() {self.selected_setting=21;self.set_start_with_windows(enabled);}
                                });setting_rects.push(row.response.rect);
                                ui.label(RichText::new("Makes the first open of the day instant too. Current user only; off by default.").font(FontId::monospace(13.0)).color(Color32::from_rgb(136,154,176)));
                                ui.add_space(10.0);
                                ui.label(RichText::new("Editor · font and cursor").font(FontId::monospace(14.0)).color(Color32::from_rgb(136,192,208)));
                                let look=self.editor_look;
                                let row=egui::Frame::new().fill(if self.selected_setting==22 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Font").font(FontId::monospace(14.0)));
                                        for font in [editor_view::EditorFont::Hack, editor_view::EditorFont::JetBrainsMono] {
                                            let sample=RichText::new(font.label()).font(FontId::new(14.0,font.family()));
                                            if ui.selectable_label(look.font==font,sample).clicked() {self.selected_setting=22;self.set_editor_look(editor_view::EditorLook{font,..look});}
                                        }
                                    });
                                });setting_rects.push(row.response.rect);
                                let row=egui::Frame::new().fill(if self.selected_setting==23 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());let mut enabled=look.block_cursor;
                                    if ui.checkbox(&mut enabled,RichText::new("Block cursor (covers the whole character)").font(FontId::monospace(14.0))).changed() {self.selected_setting=23;self.set_editor_look(editor_view::EditorLook{block_cursor:enabled,..look});}
                                });setting_rects.push(row.response.rect);
                                let row=egui::Frame::new().fill(if self.selected_setting==24 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());let mut enabled=look.animate_cursor;
                                    if ui.checkbox(&mut enabled,RichText::new("Animate cursor jumps").font(FontId::monospace(14.0))).changed() {self.selected_setting=24;self.set_editor_look(editor_view::EditorLook{animate_cursor:enabled,..look});}
                                });setting_rects.push(row.response.rect);
                                let row=egui::Frame::new().fill(if self.selected_setting==25 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new("Cursor color").font(FontId::monospace(14.0)));
                                        for (name,color) in editor_view::CURSOR_COLORS {
                                            let (swatch,response)=ui.allocate_exact_size(Vec2::splat(16.0),egui::Sense::click());
                                            ui.painter().rect_filled(swatch.shrink(1.0),2.0,color);
                                            if look.cursor_color==color {ui.painter().rect_stroke(swatch,2.0,egui::Stroke::new(1.5_f32,Color32::WHITE),egui::StrokeKind::Outside);}
                                            if response.on_hover_text(name).clicked() {self.selected_setting=25;self.set_editor_look(editor_view::EditorLook{cursor_color:color,..look});}
                                        }
                                        let mut rgb=[look.cursor_color.r(),look.cursor_color.g(),look.cursor_color.b()];
                                        if ui.color_edit_button_srgb(&mut rgb).on_hover_text("any color").changed() {self.selected_setting=25;self.set_editor_look(editor_view::EditorLook{cursor_color:Color32::from_rgb(rgb[0],rgb[1],rgb[2]),..look});}
                                    });
                                });setting_rects.push(row.response.rect);
                                let row=egui::Frame::new().fill(if self.selected_setting==26 {selected_fill} else {normal_fill}).inner_margin(6).show(ui,|ui| {
                                    ui.set_width(ui.available_width());let mut enabled=look.cursor_trail;
                                    if ui.checkbox(&mut enabled,RichText::new("Cursor trail while it glides").font(FontId::monospace(14.0))).changed() {self.selected_setting=26;self.set_editor_look(editor_view::EditorLook{cursor_trail:enabled,..look});}
                                });setting_rects.push(row.response.rect);
                                ui.label(RichText::new("The cursor glides when it jumps; typing stays instant. Text is never delayed. The trail needs animation on.").font(FontId::monospace(13.0)).color(Color32::from_rgb(136,154,176)));
                                if reveal_selected {
                                    if let Some(rect) = setting_rects.get(self.selected_setting) { ui.scroll_to_rect(*rect, Some(egui::Align::Min)); }
                                }
                            });
                        #[cfg(test)] { geometry.body = body.inner_rect; geometry.rows = setting_rects.clone(); }
                        let _ = body;
                        ui.add_space(12.0);
                        let _footer = ui.horizontal_wrapped(|ui| {
                            for (key, label) in [("↑↓", "select"), ("←→", "adjust"), ("enter", "toggle"), ("esc", "close")] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(FontId::new(13.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(235, 203, 139)),
                                );
                                ui.label(
                                    RichText::new(label)
                                        .font(FontId::new(13.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(136, 154, 176)),
                                );
                                ui.add_space(10.0);
                            }
                        });
                        #[cfg(test)] { geometry.footer = _footer.response.rect; }
                    });
            });
        #[cfg(test)]
        {
            geometry.frame = dialog_rect;
            ctx.data_mut(|d| d.insert_temp("settings_geometry".into(), geometry));
        }
        let _ = dialog_rect;
    }
}
