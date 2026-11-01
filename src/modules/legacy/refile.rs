// ---- from main.rs: fn start_refile_current_line
    fn start_refile_current_line(&mut self) {
        let Some(source_path) = self.path.clone() else {
            self.status = "Refile needs a saved source file".to_string();
            return;
        };
        if self.dirty {
            self.status = "Save current buffer before refile".to_string();
            return;
        }
        let line_index = self.buffer.line_index_for_byte(self.buffer.cursor());
        let text = self.buffer.line(line_index).trim().to_string();
        self.start_refile_draft(source_path, line_index, text);
    }


// ---- from main.rs: fn start_refile_selected_agenda_item
    fn start_refile_selected_agenda_item(&mut self) {
        let Some(index) = self.selected_agenda_task_index() else {
            self.status = "Agenda refile: no item selected".to_string();
            return;
        };
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "Agenda refile: no item selected".to_string();
            return;
        };
        if self.path.as_ref() == Some(&item.path) && self.dirty {
            self.status = "Save current source buffer before agenda refile".to_string();
            return;
        }
        let text = if self.path.as_ref() == Some(&item.path) {
            self.buffer.line(item.line_index).trim().to_string()
        } else {
            match fs::read_to_string(&item.path) {
                Ok(contents) => contents
                    .lines()
                    .nth(item.line_index)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string(),
                Err(err) => {
                    self.status = format!("Agenda refile failed reading source: {err}");
                    return;
                }
            }
        };
        self.start_refile_draft(item.path, item.line_index, text);
    }


// ---- from main.rs: fn start_refile_draft
    fn start_refile_draft(&mut self, source_path: PathBuf, line_index: usize, text: String) {
        if text.trim().is_empty() {
            self.status = "Refile needs a non-empty source line".to_string();
            return;
        }
        self.pending_refile = Some(RefileDraft {
            source_path,
            source_line_number: line_index + 1,
            source_line_index: line_index,
            text,
        });
        self.open_file_picker_at(self.project_root(), FilePickerMode::Refile);
        self.status = "Refile: choose destination Markdown file".to_string();
    }


// ---- from main.rs: fn open_refile_heading_picker_or_finish
    fn open_refile_heading_picker_or_finish(&mut self, destination: PathBuf) {
        let is_markdown = destination
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| matches!(extension.to_lowercase().as_str(), "md" | "markdown"))
            .unwrap_or(false);
        if !is_markdown {
            self.finish_refile_to_path(destination, None);
            return;
        }
        let headings = fs::read_to_string(&destination)
            .map(|contents| link_ui::markdown_heading_options(&contents))
            .unwrap_or_default();
        if headings.is_empty() {
            self.finish_refile_to_path(destination, None);
            return;
        }
        self.pending_refile_destination = Some(destination.clone());
        self.link_heading_picker_open = true;
        self.link_heading_picker_mode = LinkHeadingPickerMode::Refile;
        self.link_heading_base_target =
            link_ui::markdown_link_target_for_file(&destination, self.path.as_deref());
        self.link_heading_options = headings;
        self.selected_link_heading = 0;
        self.focus_editor_once = false;
        self.status = "Refile: choose heading or append to whole file".to_string();
    }


// ---- from main.rs: fn finish_refile_to_selected_target
    fn finish_refile_to_selected_target(&mut self, heading: Option<link_ui::LinkHeadingOption>) {
        let Some(destination) = self.pending_refile_destination.take() else {
            self.status = "No pending refile target".to_string();
            self.pending_refile = None;
            return;
        };
        self.finish_refile_to_path(destination, heading);
    }


// ---- from main.rs: fn finish_refile_to_path
    fn finish_refile_to_path(
        &mut self,
        destination: PathBuf,
        heading: Option<link_ui::LinkHeadingOption>,
    ) {
        let Some(draft) = self.pending_refile.take() else {
            self.status = "No pending refile".to_string();
            return;
        };
        if tabs::same_file(&destination, &draft.source_path) {
            self.status = "Refile target is the same file".to_string();
            return;
        }
        let source_label = draft
            .source_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("{name}:{}", draft.source_line_number))
            .unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    draft.source_path.display(),
                    draft.source_line_number
                )
            });
        let source_target_for_destination = format!(
            "{}#L{}",
            link_ui::markdown_link_target_for_file(&draft.source_path, Some(&destination)),
            draft.source_line_number
        );
        let source_target_for_daily = format!(
            "{}#L{}",
            draft.source_path.display(),
            draft.source_line_number
        );
        let mut target_for_source =
            link_ui::markdown_link_target_for_file(&destination, Some(&draft.source_path));
        let mut target_for_daily = destination.display().to_string();
        if let Some(heading) = heading.as_ref() {
            target_for_source.push('#');
            target_for_source.push_str(&heading.anchor);
            target_for_daily.push('#');
            target_for_daily.push_str(&heading.anchor);
        }
        let base_target_label = destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("refile target");
        let target_label = heading
            .as_ref()
            .map(|heading| format!("{base_target_label}#{}", heading.text))
            .unwrap_or_else(|| base_target_label.to_string());
        let block = format!(
            "{}\n  - source: [{}]({})\n",
            draft.text.trim(),
            source_label,
            source_target_for_destination
        );
        let append_result = if let Some(heading) = heading.as_ref() {
            append_markdown_block_to_heading(&destination, heading, &block)
        } else {
            append_markdown_block(&destination, &block)
        };
        if let Err(err) = append_result {
            self.status = format!("Refile failed: {err}");
            return;
        }

        let backlink = format!(
            "  - refiled to: [{}]({})\n",
            target_label, target_for_source
        );
        if let Err(err) = self.insert_refile_backlink(&draft, &backlink) {
            self.status = format!("Refile backlink failed: {err}");
            return;
        }
        let _ = self.refresh_flow_index_for_path(
            &destination,
            &fs::read_to_string(&destination).unwrap_or_default(),
        );
        self.schedule_current_flow_buffer_agenda_sync_later();
        let daily_event = DailyEvent::now(
            DailyEventKind::Organized,
            draft.text.trim().to_string(),
            Vec::new(),
            Vec::new(),
            source_label,
            Some(source_target_for_daily),
        )
        .with_target(target_label, Some(target_for_daily));
        let _ = daily::append_event(&daily_event);
        if self.agenda_open {
            if let Ok(mut items) = self.load_agenda_items() {
                self.overlay_current_buffer_agenda_items(&mut items);
                self.agenda_items = items;
                self.clamp_agenda_selection();
            }
        }
        self.status = format!("Refiled non-destructively to {}", destination.display());
    }


// ---- from main.rs: fn insert_refile_backlink
    fn insert_refile_backlink(
        &mut self,
        draft: &RefileDraft,
        backlink: &str,
    ) -> Result<(), String> {
        if self.path.as_ref() == Some(&draft.source_path) {
            let insert_at = if draft.source_line_index + 1 < self.buffer.line_count() {
                self.buffer.line_start(draft.source_line_index + 1)
            } else {
                self.buffer.as_str().len()
            };
            self.buffer.set_cursor(insert_at);
            let needs_leading_newline = insert_at == self.buffer.as_str().len()
                && !self.buffer.as_str().is_empty()
                && !self.buffer.as_str().ends_with('\n');
            let source_note = if needs_leading_newline {
                format!("\n{backlink}")
            } else {
                backlink.to_string()
            };
            self.buffer.insert_text(&source_note);
            self.dirty = true;
            // Refile asked for a saved source and wrote everything else to
            // disk: the backlink is saved too, not left as a new change.
            self.save_path(draft.source_path.clone());
            self.schedule_current_flow_buffer_agenda_sync_later();
            return Ok(());
        }

        let contents = edit_text_file(&draft.source_path, |contents| {
            // The line may have moved since the refile started.
            let lines: Vec<&str> = contents.lines().collect();
            let line = (0..=lines.len())
                .find_map(|distance| {
                    [
                        draft.source_line_index.checked_sub(distance),
                        Some(draft.source_line_index + distance).filter(|_| distance > 0),
                    ]
                    .into_iter()
                    .flatten()
                    .find(|index| {
                        lines
                            .get(*index)
                            .is_some_and(|line| line.trim() == draft.text)
                    })
                })
                .ok_or_else(|| "the source line changed on disk".to_string())?;
            let insert_at = line_start_offsets(contents)
                .get(line + 1)
                .copied()
                .unwrap_or(contents.len());
            let needs_leading_newline =
                insert_at == contents.len() && !contents.is_empty() && !contents.ends_with('\n');
            let source_note = if needs_leading_newline {
                format!("\n{backlink}")
            } else {
                backlink.to_string()
            };
            contents.insert_str(insert_at, &source_note);
            Ok(())
        })?;
        let _ = self.refresh_flow_index_for_path(&draft.source_path, &contents);
        Ok(())
    }


// ---- from main.rs: struct RefileDraft
#[derive(Clone)]
struct RefileDraft {
    source_path: PathBuf,
    source_line_number: usize,
    source_line_index: usize,
    text: String,
}


// ---- from main.rs: fn line_start_offsets
fn line_start_offsets(contents: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, ch) in contents.char_indices() {
        if ch == '\n' && index + 1 < contents.len() {
            offsets.push(index + 1);
        }
    }
    offsets
}


