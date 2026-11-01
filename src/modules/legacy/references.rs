// ---- from main.rs: fn open_reference_results
    fn open_reference_results(&mut self, kind: ReferenceResultKind) {
        let Some(current_path) = self.path.clone() else {
            self.status = "References need a saved current file".to_string();
            return;
        };
        let title = match kind {
            ReferenceResultKind::Backlinks => "Backlinks",
            ReferenceResultKind::Mentions => "Mentions",
        };
        let document = self.reference_results_document(kind, &current_path);
        self.link_preview_buffer.set_text(document);
        self.link_preview_path = None;
        self.link_preview_dirty = false;
        self.link_preview_heading_fragment = None;
        self.link_preview_heading_text = Some(title.to_string());
        self.link_preview_open = true;
        self.focus_link_preview_once = true;
        self.focus_editor_once = false;
        self.status = format!("{title} for {}", current_path.display());
    }


// ---- from main.rs: fn set_virtual_references_enabled
    fn set_virtual_references_enabled(&mut self, enabled: bool) {
        self.virtual_references_enabled = enabled;
        match self.save_settings() {
            Ok(_) => {
                self.status = if self.virtual_references_enabled {
                    "Virtual backlinks/mentions on"
                } else {
                    "Virtual backlinks/mentions hidden"
                }
                .to_string()
            }
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }


// ---- from main.rs: fn save_virtual_reference_collapse_state
    fn save_virtual_reference_collapse_state(&mut self, status: &str) {
        match self.save_settings() {
            Ok(_) => self.status = status.to_string(),
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }


// ---- from main.rs: fn handle_virtual_reference_click
    fn handle_virtual_reference_click(&mut self, line: &str) {
        if line.contains("Backlinks") {
            self.virtual_backlinks_collapsed = !self.virtual_backlinks_collapsed;
            self.save_virtual_reference_collapse_state(if self.virtual_backlinks_collapsed {
                "Backlinks collapsed"
            } else {
                "Backlinks expanded"
            });
        } else if line.contains("Mentions") {
            self.virtual_mentions_collapsed = !self.virtual_mentions_collapsed;
            self.save_virtual_reference_collapse_state(if self.virtual_mentions_collapsed {
                "Mentions collapsed"
            } else {
                "Mentions expanded"
            });
        } else {
            self.virtual_references_collapsed = !self.virtual_references_collapsed;
            self.save_virtual_reference_collapse_state(if self.virtual_references_collapsed {
                "Virtual references collapsed"
            } else {
                "Virtual references expanded"
            });
        }
    }


// ---- from main.rs: fn line_has_backlink_to_current
fn line_has_backlink_to_current(
    line: &str,
    source_file: &Path,
    current_path: &Path,
    current_abs: &Path,
) -> bool {
    parse_markdown_link_spans(line).into_iter().any(|span| {
        let target = &line[span.target_start..span.target_end];
        if link_ui::is_http_url(target) {
            return false;
        }
        let path_part = target
            .split_once('#')
            .map(|(path, _)| path)
            .unwrap_or(target);
        if path_part.trim().is_empty() {
            return false;
        }
        let resolved = link_ui::resolve_markdown_link_path(path_part, Some(source_file));
        let resolved_abs = absolutize_path(&resolved);
        resolved_abs == current_abs || resolved_abs == absolutize_path(current_path)
    })
}


// ---- from main.rs: fn line_mentions_current
fn line_mentions_current(line: &str, current_path: &Path) -> bool {
    let lower = line.to_ascii_lowercase();
    let file_name = current_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = current_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let path = current_path.display().to_string().to_ascii_lowercase();
    (!file_name.is_empty() && lower.contains(&file_name))
        || (!stem.is_empty() && lower.contains(&stem))
        || lower.contains(&path)
}


// ---- from main.rs: enum ReferenceResultKind
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReferenceResultKind {
    Backlinks,
    Mentions,
}

// ---- from main.rs: fn reference_results_document
    fn reference_results_document(&self, kind: ReferenceResultKind, current_path: &Path) -> String {
        let root = self.project_root();
        let current_name = current_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("current file");
        let title = match kind {
            ReferenceResultKind::Backlinks => "Backlinks",
            ReferenceResultKind::Mentions => "Mentions",
        };
        let rows = self.reference_rows(kind, current_path);

        let mut doc = format!("# {title} — {current_name}\n\n");
        if rows.is_empty() {
            doc.push_str("No results.\n");
            return doc;
        }
        for (file, line_number, line) in rows {
            let label = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string();
            let target = format!("{}#L{}", file.display(), line_number);
            doc.push_str(&format!("- [{}:{}]({})\n", label, line_number, target));
            doc.push_str(&format!("  - {}\n", line));
        }
        doc
    }


// ---- from main.rs: fn reference_rows
    fn reference_rows(
        &self,
        kind: ReferenceResultKind,
        current_path: &Path,
    ) -> Vec<(PathBuf, usize, String)> {
        let root = self.project_root();
        let current_abs = absolutize_path(current_path);
        let mut files = Vec::new();
        collect_link_target_files(&root, &mut files, 20_000);
        files.retain(|path| is_markdown_like_path(path) && absolutize_path(path) != current_abs);
        files.sort();
        files.dedup();

        let mut rows = Vec::new();
        for file in files {
            let Ok(contents) = fs::read_to_string(&file) else {
                continue;
            };
            for (line_index, line) in contents.lines().enumerate() {
                let matched = match kind {
                    ReferenceResultKind::Backlinks => {
                        line_has_backlink_to_current(line, &file, current_path, &current_abs)
                    }
                    ReferenceResultKind::Mentions => line_mentions_current(line, current_path),
                };
                if matched {
                    rows.push((file.clone(), line_index + 1, line.trim().to_string()));
                }
            }
        }
        rows
    }

// ---- settings_ui.rs: Virtual backlinks/mentions row (was row 6)
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
