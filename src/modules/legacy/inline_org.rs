// ---- from main.rs: fn flow_inline_org_suggestions
    fn flow_inline_org_suggestions(&self) -> Vec<String> {
        let Some((mode, query)) = self.current_flow_inline_org_token() else {
            return Vec::new();
        };
        if self.current_flow_inline_org_token_is_complete_existing(mode) {
            return Vec::new();
        }
        let query = query.trim().to_ascii_lowercase();
        let mut values = self.all_flow_org_values(mode);
        values.sort_by_key(|value| value.to_ascii_lowercase());
        values.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        values
            .into_iter()
            .filter(|value| query.is_empty() || value.to_ascii_lowercase().contains(&query))
            .filter(|value| !self.current_line_has_org_value(mode, value))
            .collect()
    }


// ---- from main.rs: fn current_flow_inline_org_token_span
    fn current_flow_inline_org_token_span(
        &self,
        mode: AgendaOrgMode,
    ) -> Option<(std::ops::Range<usize>, usize)> {
        let (line_index, column) = self.buffer.cursor_line_col();
        let line = self.buffer.line(line_index);
        let (range, _) = org_token_span_at_cursor(line, column, mode)?;
        let cursor_byte = line
            .char_indices()
            .nth(column)
            .map(|(index, _)| index)
            .unwrap_or(line.len());
        Some((range, cursor_byte))
    }


// ---- from main.rs: fn current_flow_inline_org_token_global_start
    fn current_flow_inline_org_token_global_start(&self, mode: AgendaOrgMode) -> Option<usize> {
        self.current_flow_inline_org_token_global_range(mode)
            .map(|range| range.start)
    }


// ---- from main.rs: fn current_flow_inline_org_token_global_range
    fn current_flow_inline_org_token_global_range(
        &self,
        mode: AgendaOrgMode,
    ) -> Option<std::ops::Range<usize>> {
        let (line_index, _) = self.buffer.cursor_line_col();
        let (range, _) = self.current_flow_inline_org_token_span(mode)?;
        let line_start = self.buffer.line_start(line_index);
        Some(line_start + range.start..line_start + range.end)
    }


// ---- from main.rs: fn current_flow_inline_org_token_is_complete_existing
    fn current_flow_inline_org_token_is_complete_existing(&self, mode: AgendaOrgMode) -> bool {
        let (line_index, _) = self.buffer.cursor_line_col();
        let line = self.buffer.line(line_index);
        let Some((range, _)) = self.current_flow_inline_org_token_span(mode) else {
            return false;
        };
        let prefix_len = match mode {
            AgendaOrgMode::Tag => '#'.len_utf8(),
            AgendaOrgMode::Project => '@'.len_utf8(),
        };
        let value = line[range.start + prefix_len..range.end].trim();
        !value.is_empty()
            && self
                .all_flow_org_values(mode)
                .iter()
                .any(|known| known.eq_ignore_ascii_case(value))
    }


// ---- from main.rs: fn current_flow_inline_org_token
    fn current_flow_inline_org_token(&self) -> Option<(AgendaOrgMode, String)> {
        let (line_index, column) = self.buffer.cursor_line_col();
        let line = self.buffer.line(line_index);
        if !Self::line_supports_flow_inline_org(line) {
            return None;
        }
        for mode in [AgendaOrgMode::Tag, AgendaOrgMode::Project] {
            if let Some(query) = org_query_at_cursor(line, column, mode) {
                return Some((mode, query));
            }
        }
        None
    }


// ---- from main.rs: fn line_supports_flow_inline_org
    fn line_supports_flow_inline_org(line: &str) -> bool {
        let trimmed = line.trim_start();
        parse_checkbox_line(line).is_some()
            || trimmed.starts_with(":note")
            || trimmed.starts_with(":nota")
            || trimmed.starts_with(":idea")
            || trimmed.starts_with(": ")
    }


// ---- from main.rs: fn refresh_flow_inline_org_state
    fn refresh_flow_inline_org_state(&mut self, typed: Option<&str>) {
        let current = self.current_flow_inline_org_token();
        let current_query_empty = current
            .as_ref()
            .map(|(_, query)| query.trim().is_empty())
            .unwrap_or(true);
        let explicit_trigger = typed.and_then(|text| {
            text.chars().rev().find_map(|ch| match ch {
                '#' => Some(AgendaOrgMode::Tag),
                '@' => Some(AgendaOrgMode::Project),
                _ => None,
            })
        });
        let mut next_anchor = self.flow_inline_org_anchor;
        let next_mode = match (
            self.flow_inline_org_mode,
            explicit_trigger,
            current.as_ref(),
        ) {
            (_, Some(trigger), Some((current_mode, _))) if trigger == *current_mode => {
                next_anchor = self.current_flow_inline_org_token_global_start(trigger);
                Some(trigger)
            }
            (Some(active), _, Some((current_mode, _))) if active == *current_mode => {
                let current_anchor = self.current_flow_inline_org_token_global_start(active);
                if current_anchor.is_some() && current_anchor == self.flow_inline_org_anchor {
                    Some(active)
                } else {
                    None
                }
            }
            _ => None,
        };
        if next_mode != self.flow_inline_org_mode {
            self.selected_flow_inline_org_suggestion = 0;
        }
        self.flow_inline_org_mode = next_mode;
        self.flow_inline_org_anchor = next_mode.and(next_anchor);
        if next_mode.is_none() {
            self.close_flow_inline_org_autocomplete();
            return;
        }
        let suggestions = self.flow_inline_org_suggestions();
        if suggestions.is_empty() {
            if current_query_empty {
                return;
            }
            self.close_flow_inline_org_autocomplete();
            return;
        }
        let len = suggestions.len();
        self.selected_flow_inline_org_suggestion = self
            .selected_flow_inline_org_suggestion
            .min(len.saturating_sub(1));
    }


// ---- from main.rs: fn close_flow_inline_org_autocomplete
    fn close_flow_inline_org_autocomplete(&mut self) {
        self.flow_inline_org_mode = None;
        self.flow_inline_org_anchor = None;
        self.selected_flow_inline_org_suggestion = 0;
    }


// ---- from main.rs: fn confirm_flow_inline_org_autocomplete
    fn confirm_flow_inline_org_autocomplete(&mut self, ctx: &egui::Context) {
        let Some((mode, query)) = self.current_flow_inline_org_token() else {
            self.close_flow_inline_org_autocomplete();
            return;
        };
        let suggestions = self.flow_inline_org_suggestions();
        let value = suggestions
            .get(self.selected_flow_inline_org_suggestion)
            .cloned()
            .unwrap_or(query);
        let Some(token) = organization_token(mode, &value) else {
            self.close_flow_inline_org_autocomplete();
            return;
        };
        let (line_index, column) = self.buffer.cursor_line_col();
        let line_start = self.buffer.line_start(line_index);
        let mut line = self.buffer.line(line_index).to_string();
        let next_column = replace_org_token_at_cursor(&mut line, column, mode, &token);
        let line_end = self.buffer.line_end(line_index);
        self.buffer
            .replace_selection_or_range(line_start, line_end, &line);
        let next_byte = line_start
            + line
                .char_indices()
                .nth(next_column)
                .map(|(index, _)| index)
                .unwrap_or(line.len());
        self.buffer.set_cursor(next_byte);
        self.dirty = true;
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.sync_current_flow_buffer_to_agenda(ctx);
        self.close_flow_inline_org_autocomplete();
        self.status = format!("Inserted {token}");
    }


// ---- from main.rs: fn handle_flow_inline_org_keyboard
    fn handle_flow_inline_org_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if self.flow_inline_org_mode.is_none() {
            return false;
        }
        let mut handled = false;
        let mut previous = false;
        let mut next = false;
        let mut tab_next = false;
        let mut tab_previous = false;
        let mut confirm = false;
        let mut cancel = false;
        ctx.input_mut(|input| {
            previous |= input.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
            next |= input.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            tab_next |= input.consume_key(egui::Modifiers::NONE, Key::Tab);
            tab_previous |= input.consume_key(egui::Modifiers::SHIFT, Key::Tab);
            confirm |= input.consume_key(egui::Modifiers::NONE, Key::Enter);
            cancel |= input.consume_key(egui::Modifiers::NONE, Key::Escape);
        });
        let len = self.flow_inline_org_suggestions().len().max(1);
        if previous {
            self.selected_flow_inline_org_suggestion =
                self.selected_flow_inline_org_suggestion.saturating_sub(1);
            handled = true;
        }
        if next {
            self.selected_flow_inline_org_suggestion =
                (self.selected_flow_inline_org_suggestion + 1).min(len.saturating_sub(1));
            handled = true;
        }
        if tab_next {
            self.selected_flow_inline_org_suggestion =
                (self.selected_flow_inline_org_suggestion + 1) % len;
            handled = true;
        }
        if tab_previous {
            self.selected_flow_inline_org_suggestion =
                if self.selected_flow_inline_org_suggestion == 0 {
                    len.saturating_sub(1)
                } else {
                    self.selected_flow_inline_org_suggestion.saturating_sub(1)
                };
            handled = true;
        }
        if cancel {
            self.close_flow_inline_org_autocomplete();
            handled = true;
        }
        if confirm {
            self.confirm_flow_inline_org_autocomplete(ctx);
            handled = true;
        }
        handled
    }


// ---- from main.rs: fn flow_inline_org_suggestion_popup
    fn flow_inline_org_suggestion_popup(&mut self, ctx: &egui::Context) {
        let Some(active_mode) = self.flow_inline_org_mode else {
            return;
        };
        let Some((mode, query)) = self.current_flow_inline_org_token() else {
            return;
        };
        if mode != active_mode
            || self.current_flow_inline_org_token_global_start(mode) != self.flow_inline_org_anchor
        {
            return;
        }
        let suggestions = self.flow_inline_org_suggestions();
        if suggestions.is_empty() {
            return;
        }
        let prefix = match mode {
            AgendaOrgMode::Tag => "#",
            AgendaOrgMode::Project => "@",
        };
        let title = match mode {
            AgendaOrgMode::Tag => "tag",
            AgendaOrgMode::Project => "project",
        };
        let row_font = FontId::new(13.0, FontFamily::Monospace);
        let small_font = FontId::new(11.0, FontFamily::Monospace);
        let text = Color32::from_rgb(218, 224, 235);
        let muted = Color32::from_rgb(126, 136, 157);
        let accent = Color32::from_rgb(154, 185, 235);
        let screen = ctx.content_rect();
        let popup_width = (screen.width() * 0.28).clamp(220.0, 340.0);
        let row_count = suggestions.len().min(7) as f32;
        let popup_height = 36.0 + row_count * 22.0;
        let cursor_pos = self
            .editor_view
            .last_cursor_pos()
            .unwrap_or_else(|| egui::pos2(screen.left() + 44.0, screen.bottom() - 54.0));
        let mut popup_pos = cursor_pos + Vec2::new(10.0, 8.0);
        if popup_pos.x + popup_width > screen.right() - 12.0 {
            popup_pos.x = (screen.right() - popup_width - 12.0).max(screen.left() + 12.0);
        }
        if popup_pos.y + popup_height > screen.bottom() - 34.0 {
            popup_pos.y = (cursor_pos.y - popup_height - 8.0).max(screen.top() + 12.0);
        }
        egui::Area::new("flow_inline_org_suggestions".into())
            .fixed_pos(popup_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 17, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(53, 66, 86)))
                    .corner_radius(7.0)
                    .inner_margin(egui::Margin::symmetric(7, 6))
                    .show(ui, |ui| {
                        ui.set_width(popup_width);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(title).font(small_font.clone()).color(muted));
                            ui.label(
                                RichText::new(format!("{prefix}{query}"))
                                    .font(small_font.clone())
                                    .color(accent),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("↑↓/Tab · Enter")
                                            .font(small_font.clone())
                                            .color(muted),
                                    );
                                },
                            );
                        });
                        ui.add_space(4.0);
                        for (index, suggestion) in suggestions.iter().take(7).enumerate() {
                            let selected = index == self.selected_flow_inline_org_suggestion;
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 22.0),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_flow_inline_org_suggestion = index;
                                self.confirm_flow_inline_org_autocomplete(ctx);
                                break;
                            }
                            let painter = ui.painter_at(rect);
                            if selected {
                                painter.rect_filled(rect, 4.0, Color32::from_rgb(42, 56, 76));
                            }
                            painter.text(
                                egui::pos2(rect.left() + 7.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{prefix}{suggestion}"),
                                row_font.clone(),
                                if selected { text } else { muted },
                            );
                        }
                    });
            });
    }
// ---- from main.rs: fn current_line_has_org_value
    fn current_line_has_org_value(&self, mode: AgendaOrgMode, value: &str) -> bool {
        if value.trim().is_empty() {
            return false;
        }
        let (line_index, _) = self.buffer.cursor_line_col();
        let prefix = match mode {
            AgendaOrgMode::Tag => '#',
            AgendaOrgMode::Project => '@',
        };
        self.buffer.line(line_index).split_whitespace().any(|word| {
            word.strip_prefix(prefix)
                .map(|candidate| {
                    let candidate = candidate.trim_end_matches(|ch: char| {
                        !ch.is_alphanumeric() && !matches!(ch, '-' | '_' | '/' | '.')
                    });
                    candidate.eq_ignore_ascii_case(value.trim())
                })
                .unwrap_or(false)
        })
    }


// ---- from main.rs: fn detect_project_root
    // Git detection remains for Flow's existing scope semantics, not Periscope membership.
    fn detect_project_root(&self) -> Option<PathBuf> {
        let mut dir = self
            .path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or_else(|| self.last_open_dir.clone())
            .or_else(|| std::env::current_dir().ok())?;
        loop {
            if dir.join(".git").is_dir() {
                return Some(dir);
            }
            if !dir.pop() {
                return None;
            }
        }
    }


// ---- from main.rs: fn is_current_path
    fn is_current_path(&self, path: &Path) -> bool {
        let Some(current) = &self.path else {
            return false;
        };
        if current == path {
            return true;
        }
        match (current.canonicalize(), path.canonicalize()) {
            (Ok(current), Ok(path)) => current == path,
            _ => false,
        }
    }

