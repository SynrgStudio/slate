// ---- from main.rs: fn show_agenda_updated_toast
    fn show_agenda_updated_toast(&mut self, ctx: &egui::Context) {
        self.agenda_updated_toast_until = Some(ctx.input(|input| input.time) + 0.4);
        self.feedback_toast_message = "Agenda updated";
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }


// ---- from main.rs: fn open_agenda
    fn open_agenda(&mut self) {
        self.open_agenda_with_filter(String::new());
    }


// ---- from main.rs: fn open_agenda_with_filter
    fn open_agenda_with_filter(&mut self, filter: String) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.capture_modal_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.ctrl_layer_active = false;
        self.ctrl_layer_sequence.clear();
        self.agenda_filter = filter.trim().to_string();
        self.agenda_start_offset_days = 0;
        self.agenda_tab = AgendaTab::Agenda;
        self.agenda_focus = AgendaFocus::Dated;
        self.agenda_selected_dated_row =
            Local::now().date_naive().weekday().num_days_from_monday() as usize;
        self.agenda_selected_inbox_item = 0;
        self.agenda_selected_tab_items = [0; 5];
        self.agenda_org_mode = None;
        self.agenda_org_query.clear();
        self.selected_agenda_org_suggestion = 0;
        self.flow_item_edit = None;
        self.agenda_scroll_to_selected = false;

        match self.load_agenda_items() {
            Ok(mut items) => {
                self.overlay_current_buffer_agenda_items(&mut items);
                self.agenda_items = items;
                self.clamp_agenda_selection();
                self.agenda_open = true;
                self.focus_editor_once = false;
                let visible_count = self.agenda_visible_item_indices().len();
                self.status = if self.agenda_filter.is_empty() {
                    format!("{} Flow agenda tasks", visible_count)
                } else {
                    format!(
                        "{} Flow agenda tasks matching {}",
                        visible_count, self.agenda_filter
                    )
                };
            }
            Err(err) => {
                let mut items = Vec::new();
                self.overlay_current_buffer_agenda_items(&mut items);
                if items.is_empty() {
                    self.agenda_open = false;
                    self.agenda_items.clear();
                    self.status = err;
                    self.focus_editor_once = true;
                } else {
                    self.agenda_items = items;
                    self.clamp_agenda_selection();
                    self.agenda_open = true;
                    self.focus_editor_once = false;
                    self.status = "Flow agenda from current buffer".to_string();
                }
            }
        }
    }


// ---- from main.rs: fn load_agenda_items
    fn load_agenda_items(&self) -> Result<Vec<AgendaItem>, String> {
        agenda::load_items_from_cache(&self.flow_sqlite_path()?, &self.flow_scope_root())
    }


// ---- from main.rs: fn overlay_current_buffer_agenda_items
    fn overlay_current_buffer_agenda_items(&self, items: &mut Vec<AgendaItem>) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if !flow::should_index_flow_path(path) {
            return;
        }

        let root = Self::flow_project_root_for_path(path);
        agenda::overlay_live_buffer_items(items, path, &root, self.buffer.as_str());
        self.overlay_inactive_tab_agenda_items(items);
    }


// ---- from main.rs: fn agenda_visible_item_indices
    fn agenda_visible_item_indices(&self) -> Vec<usize> {
        agenda::visible_item_indices(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
        )
    }


// ---- from main.rs: fn agenda_week_start
    fn agenda_week_start(&self) -> NaiveDate {
        agenda::week_start(self.agenda_start_offset_days)
    }


// ---- from main.rs: fn agenda_items_for_date
    fn agenda_items_for_date(&self, date: NaiveDate) -> Vec<usize> {
        agenda::item_indices_for_date(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
            date,
        )
    }


// ---- from main.rs: fn agenda_inbox_item_indices
    fn agenda_inbox_item_indices(&self) -> Vec<usize> {
        agenda::inbox_item_indices(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
        )
    }


// ---- from main.rs: fn agenda_flat_item_indices
    fn agenda_flat_item_indices(&self) -> Vec<usize> {
        self.agenda_visible_item_indices()
    }


// ---- from main.rs: fn agenda_dated_rows
    fn agenda_dated_rows(&self) -> Vec<AgendaDatedRow> {
        agenda::dated_rows(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
        )
    }


// ---- from main.rs: fn clamp_agenda_selection
    fn clamp_agenda_selection(&mut self) {
        agenda::clamp_selection(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
            &mut self.agenda_selected_dated_row,
            &mut self.agenda_selected_inbox_item,
            &mut self.agenda_selected_tab_items,
        );
    }


// ---- from main.rs: fn move_agenda_selection
    fn move_agenda_selection(&mut self, delta: isize) {
        self.agenda_scroll_to_selected = agenda::move_selection(
            &self.agenda_items,
            self.agenda_tab,
            self.agenda_focus,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
            &mut self.agenda_selected_dated_row,
            &mut self.agenda_selected_inbox_item,
            &mut self.agenda_selected_tab_items,
            delta,
        );
    }


// ---- from main.rs: fn move_agenda_week_window
    fn move_agenda_week_window(&mut self, delta_weeks: i64) {
        self.agenda_start_offset_days = self
            .agenda_start_offset_days
            .saturating_add(delta_weeks.saturating_mul(7));
        self.agenda_expanded_dates.clear();
        self.agenda_selected_dated_row = 0;
        self.agenda_focus = AgendaFocus::Dated;
        let start_date = self.agenda_week_start();
        self.status = format!("Agenda week starts {start_date}");
        self.agenda_scroll_to_selected = false;
    }


// ---- from main.rs: fn jump_agenda_to_today
    fn jump_agenda_to_today(&mut self) {
        self.agenda_start_offset_days = 0;
        self.agenda_selected_dated_row =
            Local::now().date_naive().weekday().num_days_from_monday() as usize;
        self.agenda_focus = AgendaFocus::Dated;
        self.status = "Agenda week: current".to_string();
        self.agenda_scroll_to_selected = false;
    }


// ---- from main.rs: fn toggle_agenda_focus
    fn toggle_agenda_focus(&mut self) {
        if self.agenda_tab != AgendaTab::Agenda {
            return;
        }
        self.agenda_focus = match self.agenda_focus {
            AgendaFocus::Dated => AgendaFocus::Inbox,
            AgendaFocus::Inbox => AgendaFocus::Dated,
        };
        self.clamp_agenda_selection();
        self.agenda_scroll_to_selected = true;
        self.status = match self.agenda_focus {
            AgendaFocus::Dated => "Agenda focus: dated week".to_string(),
            AgendaFocus::Inbox => "Agenda focus: no-date items".to_string(),
        };
    }


// ---- from main.rs: fn cycle_agenda_tab
    fn cycle_agenda_tab(&mut self) {
        let current = AgendaTab::ALL
            .iter()
            .position(|tab| *tab == self.agenda_tab)
            .unwrap_or(0);
        self.agenda_tab = AgendaTab::ALL[(current + 1) % AgendaTab::ALL.len()];
        self.clamp_agenda_selection();
        self.agenda_scroll_to_selected = true;
        self.status = format!("Agenda tab: {}", self.agenda_tab.label());
    }


// ---- from main.rs: fn toggle_agenda_done_visibility
    fn toggle_agenda_done_visibility(&mut self) {
        self.agenda_show_done = !self.agenda_show_done;
        self.clamp_agenda_selection();
        self.agenda_scroll_to_selected = true;
        self.status = if self.agenda_show_done {
            "Agenda: showing done tasks".to_string()
        } else {
            "Agenda: hiding done tasks".to_string()
        };
    }


// ---- from main.rs: fn activate_selected_agenda_row
    fn activate_selected_agenda_row(&mut self) {
        if self.agenda_tab != AgendaTab::Agenda {
            let indices = self.agenda_flat_item_indices();
            let tab_index = self.agenda_tab.index();
            let Some(index) = indices
                .get(self.agenda_selected_tab_items[tab_index])
                .copied()
            else {
                self.status = format!("No {} item selected", self.agenda_tab.label());
                return;
            };
            self.jump_to_agenda_item_index(index);
            return;
        }

        match self.agenda_focus {
            AgendaFocus::Dated => {
                let rows = self.agenda_dated_rows();
                let Some(row) = rows.get(self.agenda_selected_dated_row).copied() else {
                    self.status = "No dated agenda row".to_string();
                    return;
                };
                match row {
                    AgendaDatedRow::Day(date) => {
                        if !self.agenda_expanded_dates.insert(date) {
                            self.agenda_expanded_dates.remove(&date);
                            self.status = format!("Collapsed {}", date.format("%A %-d %B"));
                        } else {
                            let count = self.agenda_items_for_date(date).len();
                            self.status = format!(
                                "Expanded {} · {} task{}",
                                date.format("%A %-d %B"),
                                count,
                                if count == 1 { "" } else { "s" }
                            );
                        }
                        self.clamp_agenda_selection();
                    }
                    AgendaDatedRow::Task(index) => self.jump_to_agenda_item_index(index),
                }
            }
            AgendaFocus::Inbox => {
                let indices = self.agenda_inbox_item_indices();
                let Some(index) = indices.get(self.agenda_selected_inbox_item).copied() else {
                    self.status = "No no-date task selected".to_string();
                    return;
                };
                self.jump_to_agenda_item_index(index);
            }
        }
    }


// ---- from main.rs: fn selected_agenda_task_index
    fn selected_agenda_task_index(&self) -> Option<usize> {
        agenda::selected_item_index(
            &self.agenda_items,
            self.agenda_tab,
            self.agenda_focus,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
            self.agenda_selected_dated_row,
            self.agenda_selected_inbox_item,
            &self.agenda_selected_tab_items,
        )
    }


// ---- from main.rs: fn agenda_org_suggestions
    fn agenda_org_suggestions(&self) -> Vec<String> {
        let values = self
            .agenda_items
            .iter()
            .flat_map(|item| match self.agenda_org_mode {
                Some(AgendaOrgMode::Tag) => item.tags.clone(),
                Some(AgendaOrgMode::Project) => item.projects.clone(),
                None => Vec::new(),
            });
        organization_suggestions(values, &self.agenda_org_query)
    }


// ---- from main.rs: fn all_flow_org_values
    fn all_flow_org_values(&self, mode: AgendaOrgMode) -> Vec<String> {
        let current_path = self.path.as_ref();
        let mut values = self
            .agenda_items
            .iter()
            .filter(|item| current_path.map(|path| item.path != *path).unwrap_or(true))
            .flat_map(|item| match mode {
                AgendaOrgMode::Tag => item.tags.clone(),
                AgendaOrgMode::Project => item.projects.clone(),
            })
            .collect::<Vec<_>>();
        values.extend(self.current_buffer_flow_org_values(
            mode,
            self.current_flow_inline_org_token_global_range(mode),
        ));
        values
    }


// ---- from main.rs: fn current_buffer_flow_org_values
    fn current_buffer_flow_org_values(
        &self,
        mode: AgendaOrgMode,
        exclude: Option<std::ops::Range<usize>>,
    ) -> Vec<String> {
        let prefix = match mode {
            AgendaOrgMode::Tag => '#',
            AgendaOrgMode::Project => '@',
        };
        let mut values = Vec::new();
        let mut line_start = 0;
        for line in self.buffer.as_str().split('\n') {
            if Self::line_supports_flow_inline_org(line) {
                let mut index = 0;
                while index < line.len() {
                    let Some(ch) = line[index..].chars().next() else {
                        break;
                    };
                    if ch != prefix {
                        index += ch.len_utf8();
                        continue;
                    }
                    if index > 0 {
                        if let Some((_, before)) = line[..index].char_indices().next_back() {
                            if !before.is_whitespace() && !matches!(before, '(' | '[' | '{') {
                                index += ch.len_utf8();
                                continue;
                            }
                        }
                    }
                    let value_start = index + ch.len_utf8();
                    let mut end = value_start;
                    while end < line.len() {
                        let Some(next) = line[end..].chars().next() else {
                            break;
                        };
                        if next.is_alphanumeric() || matches!(next, '-' | '_' | '/' | '.') {
                            end += next.len_utf8();
                        } else {
                            break;
                        }
                    }
                    let global_range = line_start + index..line_start + end;
                    let overlaps_excluded = exclude
                        .as_ref()
                        .map(|excluded| {
                            global_range.start < excluded.end && excluded.start < global_range.end
                        })
                        .unwrap_or(false);
                    if !overlaps_excluded && end > value_start {
                        values.push(line[value_start..end].to_string());
                    }
                    index = end.max(index + ch.len_utf8());
                }
            }
            line_start += line.len() + 1;
        }
        values
    }


// ---- from main.rs: fn open_agenda_org_autocomplete
    fn open_agenda_org_autocomplete(&mut self, mode: AgendaOrgMode) {
        if self.selected_agenda_task_index().is_none() {
            self.status = "No agenda item selected".to_string();
            return;
        }
        self.agenda_org_mode = Some(mode);
        self.agenda_org_query.clear();
        self.selected_agenda_org_suggestion = 0;
        self.status = match mode {
            AgendaOrgMode::Tag => "Agenda tag autocomplete (#)".to_string(),
            AgendaOrgMode::Project => "Agenda project autocomplete (@)".to_string(),
        };
    }


// ---- from main.rs: fn close_agenda_org_autocomplete
    fn close_agenda_org_autocomplete(&mut self) {
        self.agenda_org_mode = None;
        self.agenda_org_query.clear();
        self.selected_agenda_org_suggestion = 0;
    }


// ---- from main.rs: fn confirm_agenda_org_autocomplete
    fn confirm_agenda_org_autocomplete(&mut self, ctx: &egui::Context) {
        let Some(mode) = self.agenda_org_mode else {
            return;
        };
        let Some(index) = self.selected_agenda_task_index() else {
            self.close_agenda_org_autocomplete();
            return;
        };
        let suggestions = self.agenda_org_suggestions();
        let value = suggestions
            .get(self.selected_agenda_org_suggestion)
            .cloned()
            .unwrap_or_else(|| self.agenda_org_query.trim().to_string());
        let Some(token) = organization_token(mode, &value) else {
            self.close_agenda_org_autocomplete();
            return;
        };
        self.close_agenda_org_autocomplete();
        self.append_token_to_agenda_item(index, &token, ctx);
    }


// ---- from main.rs: fn open_selected_flow_item_edit
    fn open_selected_flow_item_edit(&mut self) {
        let Some(index) = self.selected_agenda_task_index() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        match self.agenda_item_source_line(&item) {
            Ok(line) => {
                self.flow_item_edit = Some(FlowItemEditState::new(index, line));
                self.close_agenda_org_autocomplete();
                self.status = "Editing Flow item · Ctrl+Enter save · Esc cancel".to_string();
            }
            Err(err) => self.status = err,
        }
    }


// ---- from main.rs: fn agenda_item_source_line
    fn agenda_item_source_line(&self, item: &AgendaItem) -> Result<String, String> {
        if self.path.as_ref() == Some(&item.path) {
            return Ok(self.buffer.line(item.line_index).to_string());
        }
        let contents = fs::read_to_string(&item.path)
            .map_err(|err| format!("Flow edit read failed: {err}"))?;
        contents
            .lines()
            .nth(item.line_index)
            .map(str::to_string)
            .ok_or_else(|| "Flow edit line not found".to_string())
    }


// ---- from main.rs: fn confirm_flow_item_edit
    fn confirm_flow_item_edit(&mut self, index: usize, replacement: String, ctx: &egui::Context) {
        self.flow_item_edit = None;
        self.replace_agenda_item_source_line(index, &replacement, ctx);
    }


// ---- from main.rs: fn replace_agenda_item_source_line
    fn replace_agenda_item_source_line(
        &mut self,
        index: usize,
        replacement: &str,
        ctx: &egui::Context,
    ) {
        let Some(item) = self.agenda_items.get(index).cloned() else {
            return;
        };
        if self.standalone_agenda && self.flow_item_is_active_in_live_editor(&item) {
            match append_flow_edit_request(&self.flow_scope_root(), &item, replacement) {
                Ok(()) => {
                    self.status = format!(
                        "Requested edit for {}:{}",
                        item.relative_path, item.line_number
                    );
                    return;
                }
                Err(err) => {
                    self.status = err;
                    return;
                }
            }
        }
        if self.path.as_ref() == Some(&item.path) {
            if self.apply_flow_line_edit_to_buffer(item.line_index, replacement) {
                self.dirty = true;
                self.publish_current_flow_buffer_to_agenda(ctx);
                self.status = "Flow item edited".to_string();
            }
            return;
        }
        match Self::replace_flow_line_in_file(&self.flow_scope_root(), &item, replacement) {
            Ok(()) => {
                self.status = format!("Edited {}:{}", item.relative_path, item.line_number);
                if let Ok(mut items) = self.load_agenda_items() {
                    self.overlay_current_buffer_agenda_items(&mut items);
                    self.agenda_items = items;
                    self.clamp_agenda_selection();
                }
            }
            Err(err) => {
                self.status = err;
                self.reindex_agenda_item_file(&item);
            }
        }
    }


// ---- from main.rs: fn append_token_to_agenda_item
    fn append_token_to_agenda_item(&mut self, index: usize, token: &str, ctx: &egui::Context) {
        let Some(item) = self.agenda_items.get(index).cloned() else {
            return;
        };
        if self.standalone_agenda && self.flow_item_is_active_in_live_editor(&item) {
            match append_flow_organization_request(&self.flow_scope_root(), &item, token) {
                Ok(()) => {
                    self.status = format!(
                        "Requested {token} for {}:{}",
                        item.relative_path, item.line_number
                    );
                    return;
                }
                Err(err) => {
                    self.status = err;
                    return;
                }
            }
        }
        if self.path.as_ref() == Some(&item.path) {
            if self.apply_flow_token_to_buffer(item.line_index, token) {
                self.dirty = true;
                self.publish_current_flow_buffer_to_agenda(ctx);
                self.status = format!("Added {token} to agenda item");
            }
            return;
        }
        match Self::append_flow_token_to_file(&self.flow_scope_root(), &item, token) {
            Ok(()) => {
                self.status = format!(
                    "Added {token} to {}:{}",
                    item.relative_path, item.line_number
                );
                if let Ok(mut items) = self.load_agenda_items() {
                    self.overlay_current_buffer_agenda_items(&mut items);
                    self.agenda_items = items;
                    self.clamp_agenda_selection();
                }
            }
            Err(err) => {
                self.status = err;
                self.reindex_agenda_item_file(&item);
            }
        }
    }


// ---- from main.rs: fn reindex_agenda_item_file
    /// The file of `item` changed under the agenda: index it as it is now
    /// and show the agenda again.
    fn reindex_agenda_item_file(&mut self, item: &AgendaItem) {
        if let Ok(contents) = fs::read_to_string(&item.path) {
            let _ = self.refresh_flow_index_for_path(&item.path, &contents);
        }
        if let Ok(mut items) = self.load_agenda_items() {
            self.overlay_current_buffer_agenda_items(&mut items);
            self.agenda_items = items;
            self.clamp_agenda_selection();
        }
    }


// ---- from main.rs: fn apply_flow_token_to_buffer
    fn apply_flow_token_to_buffer(&mut self, line_index: usize, token: &str) -> bool {
        let candidate_indices = [line_index, line_index.saturating_sub(1), line_index + 1];
        for index in candidate_indices {
            if index >= self.buffer.line_count() {
                continue;
            }
            let line = self.buffer.line(index).to_string();
            if line
                .split_whitespace()
                .any(|word| word.eq_ignore_ascii_case(token))
            {
                return true;
            }
            if parse_checkbox_line(&line).is_none()
                && flow::parse_flow_note_or_idea_line(&line, index, None).is_none()
            {
                continue;
            }
            let line_end = self.buffer.line_end(index);
            self.buffer.set_cursor(line_end);
            self.buffer.insert_text(&format!(" {token}"));
            return true;
        }
        false
    }


// ---- from main.rs: fn replace_flow_line_in_file
    fn replace_flow_line_in_file(
        scope: &Path,
        item: &AgendaItem,
        replacement: &str,
    ) -> Result<(), String> {
        Self::rewrite_flow_item_line(scope, item, |line| {
            *line = replacement.to_string();
            true
        })
    }


// ---- from main.rs: fn append_flow_token_to_file
    fn append_flow_token_to_file(
        scope: &Path,
        item: &AgendaItem,
        token: &str,
    ) -> Result<(), String> {
        Self::rewrite_flow_item_line(scope, item, |line| {
            if line
                .split_whitespace()
                .any(|word| word.eq_ignore_ascii_case(token))
            {
                return false;
            }
            line.push(' ');
            line.push_str(token);
            true
        })
    }


// ---- from main.rs: fn rewrite_flow_item_line
    /// Changes the line of a Flow item in its file (not open here) and
    /// reindexes it. The item is looked for where the index last saw it and
    /// then nearby, by title: the file may have changed since, and another
    /// line must never be overwritten. Line endings stay as they were and
    /// the file is replaced atomically.
    fn rewrite_flow_item_line(
        scope: &Path,
        item: &AgendaItem,
        edit: impl FnOnce(&mut String) -> bool,
    ) -> Result<(), String> {
        let contents = fs::read_to_string(&item.path)
            .map_err(|err| format!("Flow edit read failed: {err}"))?;
        let Some(next) = Self::edit_flow_item_text(&contents, item, edit)? else {
            return Ok(());
        };
        safe_file::atomic_replace(&item.path, next.as_bytes())
            .map_err(|err| format!("Flow edit write failed: {err}"))?;
        flow::update_flow_index_for_file(
            scope,
            &Self::flow_sqlite_path_for_root(scope)?,
            &item.path,
            &next,
        )?;
        Ok(())
    }


// ---- from main.rs: fn edit_flow_item_text
    /// `contents` with the line of `item` edited (None when `edit` changed
    /// nothing), keeping its line endings.
    fn edit_flow_item_text(
        contents: &str,
        item: &AgendaItem,
        edit: impl FnOnce(&mut String) -> bool,
    ) -> Result<Option<String>, String> {
        let eol = if contents.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let body = contents.strip_suffix(eol).unwrap_or(contents);
        let mut lines: Vec<String> = body.split(eol).map(str::to_string).collect();
        let Some(index) = Self::find_flow_item_line(&lines, item) else {
            return Err(format!(
                "{}:{} changed on disk; agenda refreshed, try again",
                item.relative_path, item.line_number
            ));
        };
        if !edit(&mut lines[index]) {
            return Ok(None);
        }
        let mut next = lines.join(eol);
        if contents.ends_with(eol) {
            next.push_str(eol);
        }
        Ok(Some(next))
    }


// ---- from main.rs: fn find_flow_item_line
    /// The line that holds `item` now: where it was indexed, or the closest
    /// line with the same title.
    fn find_flow_item_line(lines: &[String], item: &AgendaItem) -> Option<usize> {
        let title_at = |index: usize| {
            let line = lines.get(index)?;
            flow::parse_flow_task_line(line, index, None)
                .or_else(|| flow::parse_flow_note_or_idea_line(line, index, None))
                .map(|parsed| parsed.title)
        };
        let matches = |index: usize| title_at(index).is_some_and(|title| title == item.title);
        (0..=lines.len()).find_map(|distance| {
            [
                item.line_index.checked_sub(distance),
                Some(item.line_index + distance).filter(|_| distance > 0),
            ]
            .into_iter()
            .flatten()
            .find(|index| matches(*index))
        })
    }


// ---- from main.rs: fn jump_to_agenda_item_index
    fn jump_to_agenda_item_index(&mut self, index: usize) {
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        if self.standalone_agenda {
            self.open_agenda_item_in_main_slate(&item);
            return;
        }
        if self.path.as_ref() != Some(&item.path) {
            if self.dirty {
                self.status = "Save current buffer before agenda jump".to_string();
                return;
            }
            self.request_file_open(item.path.clone(), OpenDestination::Agenda(item.line_index));
            self.agenda_open = false;
            self.focus_editor_once = true;
            return;
        }
        self.cancel_file_open();
        let line = item
            .line_index
            .min(self.buffer.line_count().saturating_sub(1));
        self.buffer.set_cursor(self.buffer.line_end(line));
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.agenda_open = false;
        self.focus_editor_once = true;
        self.status = format!("Jumped to agenda item on line {}", line + 1);
    }


// ---- from main.rs: fn open_agenda_item_in_main_slate
    fn open_agenda_item_in_main_slate(&mut self, item: &AgendaItem) {
        if self.flow_item_is_active_in_live_editor(item) {
            if append_flow_open_request(
                &self.flow_scope_root(),
                &item.relative_path,
                item.line_index,
            )
            .is_ok()
            {
                self.status = format!(
                    "Jumping existing Slate window to {}:{}",
                    item.relative_path, item.line_number
                );
                self.agenda_open = false;
                self.focus_editor_once = false;
                return;
            }
        }

        let line_number = item.line_index.saturating_add(1).to_string();
        #[cfg(target_os = "windows")]
        let slate_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("slate.exe"));
        #[cfg(not(target_os = "windows"))]
        let slate_bin = dirs_next::home_dir()
            .map(|home| home.join(".local/bin/slate"))
            .filter(|path| path.exists())
            .unwrap_or_else(|| PathBuf::from("slate"));
        match ProcessCommand::new(&slate_bin)
            .arg(&item.path)
            .arg("--line")
            .arg(&line_number)
            .spawn()
        {
            Ok(_) => {
                self.status = format!("Opening {}:{}", item.path.display(), line_number);
                self.agenda_open = false;
                self.focus_editor_once = false;
            }
            Err(err) => {
                self.status = format!("Agenda open failed: {err}");
            }
        }
    }


// ---- from main.rs: fn cycle_selected_agenda_item
    fn cycle_selected_agenda_item(&mut self) {
        let Some(index) = self.selected_agenda_task_index() else {
            self.status = "Select an agenda task before cycling it".to_string();
            return;
        };
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        if item.kind != "task" {
            self.status = format!("Agenda cycle only applies to tasks, not {}", item.kind);
            return;
        }
        if self.path.as_ref() == Some(&item.path) {
            // Unsaved edits may have moved it: find it by its title.
            let lines: Vec<String> = (0..self.buffer.line_count())
                .map(|index| self.buffer.line(index).to_string())
                .collect();
            let Some(line) = Self::find_flow_item_line(&lines, &item) else {
                self.status = format!(
                    "Agenda cycle failed: {} is no longer in this buffer",
                    item.title
                );
                return;
            };
            if !EditorView::cycle_checkbox_at_line(&mut self.buffer, line) {
                self.status = format!("Agenda cycle failed: no checkbox on line {}", line + 1);
                return;
            }
            self.search_state = None;
            // Unsaved changes are the user's to save: only a clean file is
            // saved right away.
            let was_dirty = self.dirty;
            self.dirty = true;
            if !was_dirty {
                self.save_path(item.path.clone());
            }
        } else {
            let mut cycled = false;
            let result = Self::rewrite_flow_item_line(&self.flow_scope_root(), &item, |line| {
                let mut buffer = EditorBuffer::from_text(line.clone());
                cycled = EditorView::cycle_checkbox_at_line(&mut buffer, 0);
                if cycled {
                    *line = buffer.as_str().to_string();
                }
                cycled
            });
            if let Err(err) = result {
                self.status = format!("Agenda cycle failed: {err}");
                self.reindex_agenda_item_file(&item);
                return;
            }
            if !cycled {
                self.status = format!(
                    "Agenda cycle failed: no checkbox on {}:{}",
                    item.relative_path, item.line_number
                );
                return;
            }
        }

        match self.load_agenda_items() {
            Ok(mut items) => {
                self.overlay_current_buffer_agenda_items(&mut items);
                self.agenda_items = items;
                self.clamp_agenda_selection();
                self.agenda_scroll_to_selected = true;
                let state = self
                    .agenda_items
                    .iter()
                    .find(|candidate| {
                        candidate.path == item.path && candidate.line_index == item.line_index
                    })
                    .map(|item| agenda::state_marker(&item.state))
                    .unwrap_or("[ ]");
                self.status = format!(
                    "Cycled agenda task to {state} on {}:{}",
                    item.relative_path, item.line_number
                );
            }
            Err(err) => {
                self.status = format!("Agenda cycle updated source but reload failed: {err}");
            }
        }
    }


// ---- from main.rs: fn close_standalone_agenda_if_requested
    fn close_standalone_agenda_if_requested(&mut self, ctx: &egui::Context) -> bool {
        if !self.standalone_agenda {
            return false;
        }
        let should_close = ctx.input_mut(|i| {
            if self.flow_item_edit.is_some() && i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                self.flow_item_edit = None;
                return false;
            }
            if self.agenda_org_mode.is_some() && i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                self.close_agenda_org_autocomplete();
                return false;
            }
            i.consume_key(egui::Modifiers::NONE, Key::Escape)
                || i.consume_key(egui::Modifiers::CTRL, Key::W)
                || i.consume_key(egui::Modifiers::CTRL, Key::Q)
                || i.viewport().close_requested()
        });
        if should_close {
            self.agenda_open = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return true;
        }
        false
    }


// ---- from main.rs: fn handle_agenda_org_keyboard
    fn handle_agenda_org_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if !self.agenda_open || self.agenda_org_mode.is_none() {
            return false;
        }
        let mut handled = false;
        let mut confirm = false;
        let mut cancel = false;
        let mut previous = false;
        let mut next = false;
        let mut tab_next = false;
        let mut tab_previous = false;
        let mut backspace = false;
        let mut typed = String::new();
        ctx.input_mut(|input| {
            previous |= input.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
            next |= input.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            tab_next |= input.consume_key(egui::Modifiers::NONE, Key::Tab);
            tab_previous |= input.consume_key(egui::Modifiers::SHIFT, Key::Tab);
            confirm |= input.consume_key(egui::Modifiers::NONE, Key::Enter);
            confirm |= input.consume_key(egui::Modifiers::NONE, Key::Space);
            cancel |= input.consume_key(egui::Modifiers::NONE, Key::Escape);
            backspace |= input.consume_key(egui::Modifiers::NONE, Key::Backspace);
            for event in &input.events {
                if let egui::Event::Text(text) = event {
                    typed.push_str(text);
                }
            }
            if !typed.is_empty() {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Text(_)));
            }
        });
        if previous {
            self.selected_agenda_org_suggestion =
                self.selected_agenda_org_suggestion.saturating_sub(1);
            handled = true;
        }
        if next {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion =
                (self.selected_agenda_org_suggestion + 1).min(len.saturating_sub(1));
            handled = true;
        }
        if tab_next {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion = (self.selected_agenda_org_suggestion + 1) % len;
            handled = true;
        }
        if tab_previous {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion = if self.selected_agenda_org_suggestion == 0 {
                len.saturating_sub(1)
            } else {
                self.selected_agenda_org_suggestion.saturating_sub(1)
            };
            handled = true;
        }
        if backspace {
            self.agenda_org_query.pop();
            self.selected_agenda_org_suggestion = 0;
            handled = true;
        }
        if !typed.is_empty() {
            for ch in typed.chars() {
                if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.') {
                    self.agenda_org_query.push(ch);
                }
            }
            self.selected_agenda_org_suggestion = 0;
            handled = true;
        }
        if cancel {
            self.close_agenda_org_autocomplete();
            handled = true;
        }
        if confirm {
            self.confirm_agenda_org_autocomplete(ctx);
            handled = true;
        }
        if self.agenda_org_mode.is_some() {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion = self
                .selected_agenda_org_suggestion
                .min(len.saturating_sub(1));
        }
        handled
    }


// ---- from main.rs: fn split_cache_list
fn split_cache_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}


// ---- from main.rs: fn non_empty_string
fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}


// ---- from main.rs: fn parse_cache_date
fn parse_cache_date(value: &str) -> Option<NaiveDate> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

// ---- from tabs.rs
    /// The agenda shows unsaved tasks of every tab, not only the active one.
    pub(super) fn overlay_inactive_tab_agenda_items(&self, items: &mut Vec<AgendaItem>) {
        for doc in self.tabs.list.iter().filter_map(|tab| tab.doc.as_ref()) {
            let Some(path) = doc.path.as_deref() else {
                continue;
            };
            if doc.dirty && flow::should_index_flow_path(path) {
                let root = Self::flow_project_root_for_path(path);
                agenda::overlay_live_buffer_items(items, path, &root, doc.buffer.as_str());
            }
        }
    }


