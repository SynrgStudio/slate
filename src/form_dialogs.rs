use super::*;
impl SlateApp {
    pub(super) fn render_periscope_prompt(&mut self, ctx: &egui::Context) {
        if !self.periscope_prompt_open {
            return;
        }
        let clicked = dialog_ui::show(
            ctx,
            "periscope_prompt_dialog".into(),
            "Periscope",
            "Choose the scope for file search",
            &[
                (
                    "Y/Enter",
                    if self.periscope_project_root.is_some() {
                        "project"
                    } else {
                        "global (no project)"
                    },
                ),
                ("N", "global search"),
                ("Esc", "cancel"),
            ],
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("prompt_content")
                    .auto_shrink([false, false])
                    .min_scrolled_height(1.0)
                    .max_height(ui.available_height())
                    .show(ui, |ui| {
                        if let Some(root)=&self.periscope_project_root {ui.label(format!("Document project: {}",root.display()));} else {ui.label("No project assigned to this document.");}
                        ui.add_space(8.0);
                        ui.label("Search this document's project folder, or the configured global drives. Projects are assigned through Menu → Crear Proyecto.");
                    });
            },
        );
        if clicked[0] {
            self.choose_periscope_project();
        } else if clicked[1] {
            self.open_global_periscope();
        } else if clicked[2] {
            self.periscope_prompt_open = false;
        }
    }
    pub(super) fn render_scratch_form(&mut self, ctx: &egui::Context) {
        if !self.scratch_modal_open {
            return;
        }
        let subtitle = scratch::archive_path()
            .map(|path| format!("Quick capture · {}", path.display()))
            .unwrap_or_else(|| "Quick capture · archive location unavailable".into());
        let clicked = dialog_ui::show(
            ctx,
            "scratch_modal_dialog".into(),
            "Scratch",
            &subtitle,
            &[
                ("Ctrl+S", "archive"),
                ("Ctrl+E", "entries"),
                ("Esc", "hide"),
            ],
            |ui| {
                let (response, changed) = self.scratch_view.render(
                    ui,
                    &mut self.scratch_buffer,
                    self.wrap,
                    None,
                    self.line_number_mode,
                    true,
                    true,
                    None,
                    false,
                    None,
                );
                response.request_focus();
                if changed {
                    self.status = "Scratch capture modified".into();
                }
            },
        );
        if clicked[0] {
            self.archive_scratch_modal();
        } else if clicked[1] {
            self.open_scratch_entries_modal();
        } else if clicked[2] {
            self.cancel_scratch_modal();
        }
    }
    pub(super) fn render_capture_form(&mut self, ctx: &egui::Context) {
        if !self.capture_modal_open {
            return;
        }
        let clicked = dialog_ui::show(
            ctx,
            "capture_dialog".into(),
            "Capture",
            "Selection/current line → Scratch",
            &[("Enter", "archive"), ("Esc", "cancel")],
            |ui| {
                let height = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_salt("capture_content")
                    .auto_shrink([false, false])
                    .min_scrolled_height(1.0)
                    .max_height(height)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Title/context optional").color(notice_ui::DIM));
                        let response = ui.add(
                            TextEdit::singleline(&mut self.capture_title)
                                .id_salt("capture_title_input")
                                .hint_text("press Enter empty for no title")
                                .desired_width(f32::INFINITY)
                                .font(FontId::monospace(13.0))
                                .text_color(notice_ui::TEXT)
                                .frame(egui::Frame::NONE),
                        );
                        if self.capture_title_focus_once {
                            response.request_focus();
                            self.capture_title_focus_once = false;
                            ctx.request_repaint();
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("Capturing").color(notice_ui::DIM));
                        let lines: Vec<&str> = self.capture_text.lines().collect();
                        let row_height =
                            ui.fonts_mut(|fonts| fonts.row_height(&FontId::monospace(13.0)));
                        egui::Frame::new()
                            .fill(Color32::from_rgb(30, 36, 48))
                            .inner_margin(8)
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("capture_preview")
                                    .auto_shrink([false, false])
                                    .min_scrolled_height(1.0)
                                    .max_height((height - 84.0).max(40.0))
                                    .show_rows(ui, row_height, lines.len(), |ui, range| {
                                        for index in range {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(lines[index])
                                                        .font(FontId::monospace(13.0)),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_ui(|ui| {
                                                footer_ui::style_hints(ui);
                                                ui.label(lines[index]);
                                            });
                                        }
                                    });
                            });
                    });
            },
        );
        if clicked[0] {
            self.confirm_capture_modal();
        } else if clicked[1] {
            self.capture_modal_open = false;
        }
    }
    pub(super) fn render_modal_form(&mut self, ctx: &egui::Context) {
        if !self.link_preview_open {
            return;
        }
        let title = self
            .link_preview_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "untitled".into());
        let mut subtitle = format!(
            "{}{}",
            if self.link_preview_dirty {
                "Modified · "
            } else {
                ""
            },
            title
        );
        if let Some(fragment) = &self.link_preview_heading_fragment {
            subtitle.push_str(&format!(" · → #{fragment}"));
        }
        let clicked = dialog_ui::show(
            ctx,
            "link_preview_dialog".into(),
            "Modal buffer",
            &subtitle,
            &[
                ("Tab", "swap"),
                ("Ctrl+Enter", "promote"),
                ("Ctrl+S", "save"),
                ("Esc", "close"),
            ],
            |ui| {
                if self.focus_link_preview_once {
                    self.link_preview_view.request_keyboard_focus();
                }
                self.link_preview_view
                    .observe_buffer(&self.link_preview_buffer);
                let target_line = self
                    .link_preview_heading_fragment
                    .as_ref()
                    .map(|_| self.link_preview_buffer.cursor_line_col().0);
                let (_, changed) = self.link_preview_view.render(
                    ui,
                    &mut self.link_preview_buffer,
                    self.wrap,
                    None,
                    self.line_number_mode,
                    true,
                    true,
                    target_line,
                    self.markdown_live_rendering,
                    None,
                );
                self.focus_link_preview_once = false;
                if changed {
                    self.link_preview_dirty = true;
                }
            },
        );
        if clicked[0] {
            self.swap_link_preview_with_main();
        } else if clicked[1] {
            self.commit_link_preview();
        } else if clicked[2] {
            self.save_link_preview();
        } else if clicked[3] {
            self.close_link_preview();
        }
    }
}
