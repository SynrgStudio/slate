// ---- editor_view.rs: fn take_flow_agenda_update_request
    pub(crate) fn take_flow_agenda_update_request(&mut self) -> bool {
        let requested = self.flow_agenda_update_requested;
        self.flow_agenda_update_requested = false;
        requested
    }


// ---- editor_view.rs: fn cursor_near_flow_item_context
    fn cursor_near_flow_item_context(buffer: &EditorBuffer) -> bool {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        line_index
            .checked_sub(1)
            .into_iter()
            .chain(std::iter::once(line_index))
            .chain(std::iter::once(line_index + 1))
            .any(|index| {
                let line = buffer.line(index);
                parse_checkbox_line(line).is_some()
                    || parse_flow_note_or_idea_line(line, index, None).is_some()
            })
    }


// ---- editor_view.rs: fn ensure_current_flow_line_default_tag
    fn ensure_current_flow_line_default_tag(buffer: &mut EditorBuffer) -> bool {
        if buffer.selection().is_some() {
            return false;
        }
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let line_start = buffer.line_start(line_index);
        let line = buffer.line(line_index).to_string();
        if !line[buffer.cursor().saturating_sub(line_start)..]
            .trim()
            .is_empty()
        {
            return false;
        }
        Self::ensure_flow_line_default_tags_at_line(buffer, line_index)
    }


// ---- editor_view.rs: fn ensure_flow_line_default_tags_at_line
    fn ensure_flow_line_default_tags_at_line(buffer: &mut EditorBuffer, line_index: usize) -> bool {
        let line_end = buffer.line_end(line_index);
        let line = buffer.line(line_index).to_string();
        let tags = Self::default_flow_tags_for_line(&line);
        let missing_tags = tags
            .into_iter()
            .filter(|tag| {
                !line
                    .split_whitespace()
                    .any(|word| word.eq_ignore_ascii_case(&format!("#{tag}")))
            })
            .collect::<Vec<_>>();
        if missing_tags.is_empty() {
            return false;
        }
        buffer.set_cursor(line_end);
        buffer.insert_text(&format!(
            " {}",
            missing_tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        true
    }


// ---- editor_view.rs: fn default_flow_tags_for_line
    fn default_flow_tags_for_line(line: &str) -> Vec<&'static str> {
        let trimmed = line.trim_start();
        if let Some(checkbox) = parse_checkbox_line(line) {
            if checkbox.text.trim().is_empty() {
                return Vec::new();
            }
            let has_extra = checkbox.text.split_whitespace().any(|word| {
                word.starts_with('#')
                    || word.starts_with('@')
                    || matches!(
                        word.to_ascii_uppercase().as_str(),
                        "ALTA" | "MEDIA" | "BAJA"
                    )
                    || word.starts_with("due:")
                    || word.starts_with("scheduled:")
            });
            return if has_extra {
                vec!["task"]
            } else {
                vec!["task", "inbox"]
            };
        }
        if trimmed.starts_with(": ") && trimmed[2..].trim().is_empty() {
            return Vec::new();
        }
        if trimmed.starts_with(": ") {
            return vec!["inbox"];
        }
        let Some(parsed) = parse_flow_note_or_idea_line(line, 0, None) else {
            return Vec::new();
        };
        match parsed.kind {
            FlowItemKind::Task => vec!["task"],
            FlowItemKind::Note => vec!["note"],
            FlowItemKind::Idea => vec!["idea"],
            FlowItemKind::Inbox => vec!["inbox"],
        }
    }


// ---- editor_view.rs: fn insert_flow_task_metadata_start
    fn insert_flow_task_metadata_start(&mut self, buffer: &mut EditorBuffer) -> bool {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let line = buffer.line(line_index);
        let Some(task) = parse_checkbox_line(line) else {
            return false;
        };
        let child_indent = format!("{}  ", task.indent);
        Self::ensure_flow_line_default_tags_at_line(buffer, line_index);
        let insertion = format!("\n{child_indent}- scheduled: ");
        buffer.set_cursor(buffer.line_end(line_index));
        buffer.insert_text(&insertion);
        self.flow_metadata_month_offset = 0;
        true
    }


// ---- editor_view.rs: fn handle_flow_metadata_enter
    fn handle_flow_metadata_enter(&mut self, buffer: &mut EditorBuffer) -> bool {
        let Some(line) =
            flow_metadata_line(buffer.line(buffer.line_index_for_byte(buffer.cursor())))
        else {
            return false;
        };
        let accepted = self.accept_flow_metadata_suggestion(buffer);
        let line = flow_metadata_line(buffer.line(buffer.line_index_for_byte(buffer.cursor())))
            .unwrap_or(line);
        match line.key {
            FlowMetadataKey::Scheduled => {
                let child_prefix = flow_metadata_child_prefix(&line);
                if line.value.trim().is_empty() && !accepted {
                    let replacement = format!("{child_prefix}due: ");
                    buffer.replace_current_line(&replacement, replacement.chars().count());
                } else {
                    buffer.set_cursor(buffer.line_end(buffer.line_index_for_byte(buffer.cursor())));
                    buffer.insert_text(&format!("\n{child_prefix}due: "));
                }
                self.flow_metadata_month_offset = 0;
                true
            }
            FlowMetadataKey::Due => {
                if line.value.trim().is_empty() && !accepted {
                    buffer.replace_current_line("", 0);
                } else {
                    buffer.set_cursor(buffer.line_end(buffer.line_index_for_byte(buffer.cursor())));
                    buffer.insert_text("\n");
                }
                self.flow_metadata_month_offset = 0;
                true
            }
        }
    }


// ---- editor_view.rs: fn accept_flow_metadata_suggestion
    fn accept_flow_metadata_suggestion(&mut self, buffer: &mut EditorBuffer) -> bool {
        let Some(suggestion) = self.current_flow_metadata_suggestion(buffer) else {
            return false;
        };
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let Some(line) = flow_metadata_line(buffer.line(line_index)) else {
            return false;
        };
        let replacement = format!("{}{}", line.prefix, suggestion.format("%Y-%m-%d"));
        buffer.replace_current_line(&replacement, replacement.chars().count());
        true
    }


// ---- editor_view.rs: fn adjust_flow_metadata_month
    fn adjust_flow_metadata_month(&mut self, buffer: &EditorBuffer, delta: i32) -> bool {
        if self.current_flow_metadata_suggestion(buffer).is_none() {
            return false;
        }
        self.flow_metadata_month_offset = self.flow_metadata_month_offset.saturating_add(delta);
        true
    }


// ---- editor_view.rs: fn current_flow_metadata_suggestion
    fn current_flow_metadata_suggestion(&self, buffer: &EditorBuffer) -> Option<NaiveDate> {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let line = flow_metadata_line(buffer.line(line_index))?;
        suggest_flow_metadata_date(&line.value, self.flow_metadata_month_offset)
    }


// ---- editor_view.rs: fn paint_flow_metadata_suggestion
    fn paint_flow_metadata_suggestion(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        render_markdown: bool,
    ) {
        if !render_markdown || row.start != buffer.line_start(row.line_index) {
            return;
        }
        if buffer.line_index_for_byte(buffer.cursor()) != row.line_index {
            return;
        }
        let Some(line) = flow_metadata_line(buffer.line(row.line_index)) else {
            return;
        };
        if line.value.trim().is_empty() {
            return;
        }
        let Some(suggestion) = self.current_flow_metadata_suggestion(buffer) else {
            return;
        };
        let ghost = format!(" → {}", suggestion.format("%Y-%m-%d"));
        let x = text_x + self.text_width(painter, buffer.line(row.line_index), font) + 8.0;
        painter.text(
            egui::pos2(x, y + line_height * 0.5),
            egui::Align2::LEFT_CENTER,
            ghost,
            font.clone(),
            Color32::from_rgb(94, 105, 126),
        );
    }


// ---- editor_view.rs: fn flow_metadata_line
fn flow_metadata_line(line: &str) -> Option<FlowMetadataLine> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    let rest = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    if let Some(value) = rest.strip_prefix("scheduled:") {
        return Some(FlowMetadataLine {
            key: FlowMetadataKey::Scheduled,
            prefix: format!("{indent}- scheduled: "),
            value: value.trim_start().to_string(),
        });
    }
    if let Some(value) = rest.strip_prefix("due:") {
        return Some(FlowMetadataLine {
            key: FlowMetadataKey::Due,
            prefix: format!("{indent}- due: "),
            value: value.trim_start().to_string(),
        });
    }
    None
}


// ---- editor_view.rs: fn flow_metadata_child_prefix
fn flow_metadata_child_prefix(line: &FlowMetadataLine) -> String {
    line.prefix
        .strip_suffix("scheduled: ")
        .or_else(|| line.prefix.strip_suffix("due: "))
        .unwrap_or(&line.prefix)
        .to_string()
}


// ---- editor_view.rs: fn suggest_flow_metadata_date
fn suggest_flow_metadata_date(value: &str, month_offset: i32) -> Option<NaiveDate> {
    let value = value.trim();
    if value.is_empty() || NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() {
        return None;
    }
    let numbers = value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect::<Vec<_>>();
    let today = Local::now().date_naive();
    match numbers.as_slice() {
        [day] if (1..=31).contains(day) => {
            let mut year = today.year();
            let mut month = today.month();
            let max_day = days_in_month(year, month);
            let candidate = NaiveDate::from_ymd_opt(year, month, (*day).min(max_day))?;
            if candidate < today {
                (year, month) = add_months(year, month, 1);
            }
            let (year, month) = add_months(year, month, month_offset);
            let day = (*day).min(days_in_month(year, month));
            NaiveDate::from_ymd_opt(year, month, day)
        }
        [day, month] if (1..=31).contains(day) && (1..=12).contains(month) => {
            let mut year = today.year();
            let max_day = days_in_month(year, *month);
            let candidate = NaiveDate::from_ymd_opt(year, *month, (*day).min(max_day))?;
            if candidate < today {
                year += 1;
            }
            let (year, month) = add_months(year, *month, month_offset);
            let day = (*day).min(days_in_month(year, month));
            NaiveDate::from_ymd_opt(year, month, day)
        }
        [day, month, year] if (1..=31).contains(day) && (1..=12).contains(month) => {
            let year = if *year < 100 {
                2000 + *year as i32
            } else {
                *year as i32
            };
            let (year, month) = add_months(year, *month, month_offset);
            let day = (*day).min(days_in_month(year, month));
            NaiveDate::from_ymd_opt(year, month, day)
        }
        _ => None,
    }
}


// ---- editor_view.rs: fn flow_task_token_spans
fn flow_task_token_spans(
    text: &str,
    text_start: usize,
    text_end: usize,
) -> Vec<(usize, usize, Color32)> {
    let mut spans = Vec::new();
    let slice = &text[text_start..text_end];
    let mut token_start = None;
    for (offset, ch) in slice
        .char_indices()
        .chain(std::iter::once((slice.len(), ' ')))
    {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                let end = offset;
                let token = &slice[start..end];
                let trimmed = token.trim_matches(|ch: char| matches!(ch, ',' | ';' | ')' | '('));
                let color = if trimmed.starts_with('#') && trimmed.len() > 1 {
                    Some(Color32::from_rgb(136, 192, 208))
                } else if trimmed.starts_with('@') && trimmed.len() > 1 {
                    Some(Color32::from_rgb(180, 142, 173))
                } else if matches!(trimmed, "ALTA" | "MEDIA" | "BAJA") {
                    Some(match trimmed {
                        "ALTA" => Color32::from_rgb(235, 203, 139),
                        "MEDIA" => Color32::from_rgb(163, 190, 140),
                        _ => Color32::from_rgb(136, 154, 176),
                    })
                } else if matches!(trimmed, "due:" | "scheduled:" | "related:" | "note:") {
                    Some(Color32::from_rgb(94, 105, 126))
                } else {
                    None
                };
                if let Some(color) = color {
                    spans.push((text_start + start, text_start + end, color));
                }
            }
        } else if token_start.is_none() {
            token_start = Some(offset);
        }
    }
    spans
}


// ---- editor_view.rs: enum FlowMetadataKey
#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowMetadataKey {
    Scheduled,
    Due,
}


// ---- editor_view.rs: struct FlowMetadataLine
struct FlowMetadataLine {
    key: FlowMetadataKey,
    prefix: String,
    value: String,
}


// ---- editor_view.rs: fn starting_flow_metadata_adds_default_task_tags_first
    #[test]
    fn starting_flow_metadata_adds_default_task_tags_first() {
        let mut buffer = EditorBuffer::from_text("[] nombre del task".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.insert_flow_task_metadata_start(&mut buffer));
        assert_eq!(
            buffer.as_str(),
            "[] nombre del task #task #inbox\n  - scheduled: "
        );
    }


// ---- editor_view.rs: fn flow_metadata_empty_scheduled_becomes_due_without_nesting_key
    #[test]
    fn flow_metadata_empty_scheduled_becomes_due_without_nesting_key() {
        let mut buffer = EditorBuffer::from_text("- [ ] task\n  - scheduled: ".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.handle_flow_metadata_enter(&mut buffer));

        assert_eq!(buffer.as_str(), "- [ ] task\n  - due: ");
        assert!(!buffer.as_str().contains("scheduled: due:"));
    }


// ---- editor_view.rs: fn flow_metadata_scheduled_date_inserts_due_sibling
    #[test]
    fn flow_metadata_scheduled_date_inserts_due_sibling() {
        let mut buffer = EditorBuffer::from_text("- [ ] task\n  - scheduled: 10".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.handle_flow_metadata_enter(&mut buffer));

        assert!(buffer.as_str().contains("\n  - due: "));
        assert!(!buffer.as_str().contains("scheduled: due:"));
    }


// ---- editor_view.rs: block
if let Some(item) = parse_flow_note_or_idea_line(line, row.line_index, None) {
                let item_color = match item.kind {
                    FlowItemKind::Task => Color32::from_rgb(136, 192, 208),
                    FlowItemKind::Note => Color32::from_rgb(180, 142, 173),
                    FlowItemKind::Idea => Color32::from_rgb(235, 203, 139),
                    FlowItemKind::Inbox => Color32::from_rgb(136, 154, 176),
                };
                let bar_rect = egui::Rect::from_min_max(
                    egui::pos2(text_x - 7.0, y + 4.0),
                    egui::pos2(text_x - 4.0, y + line_height - 4.0),
                );
                painter.rect_filled(bar_rect, 1.5, item_color);
            }

// ---- editor_view.rs: block
        if let Some(item) = parse_flow_note_or_idea_line(line, row.line_index, None) {
            let trimmed = line.trim_start();
            let indent_len = line.len().saturating_sub(trimmed.len());
            let marker_len = match item.kind {
                FlowItemKind::Task => 0,
                FlowItemKind::Note if trimmed.starts_with(":nota") => ":nota".len(),
                FlowItemKind::Note => ":note".len(),
                FlowItemKind::Idea => ":idea".len(),
                FlowItemKind::Inbox => ":".len(),
            };
            let marker_start = line_start + indent_len;
            let marker_end = marker_start + marker_len;
            let text_start = marker_end;
            let cursor_in_marker = buffer.cursor() >= marker_start && buffer.cursor() <= marker_end;
            if !cursor_in_marker && row.start == line_start && row.end > marker_start {
                let indent = &line[..indent_len];
                let marker = &line[indent_len..indent_len + marker_len];
                let indent_width = self.text_width(painter, indent, font);
                let marker_width = self.text_width(painter, marker, font);
                if !indent.is_empty() {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        indent,
                        font.clone(),
                        text_color,
                    );
                }
                let marker_color = match item.kind {
                    FlowItemKind::Task => Color32::from_rgb(136, 192, 208),
                    FlowItemKind::Note => Color32::from_rgb(180, 142, 173),
                    FlowItemKind::Idea => Color32::from_rgb(235, 203, 139),
                    FlowItemKind::Inbox => Color32::from_rgb(136, 154, 176),
                };
                painter.text(
                    egui::pos2(text_x + indent_width, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    marker,
                    font.clone(),
                    marker_color,
                );
                if row.end > text_start {
                    self.paint_flow_task_text(
                        painter,
                        buffer,
                        text_start,
                        row.end,
                        text_x + indent_width + marker_width,
                        y,
                        line_height,
                        font,
                        text_color,
                    );
                }
                return;
            }
        }

// ---- editor_view.rs: fn paint_flow_task_text
    fn paint_flow_task_text(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        text_start: usize,
        text_end: usize,
        x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        text_color: Color32,
    ) {
        let mut cursor = text_start;
        for (token_start, token_end, color) in
            flow_task_token_spans(buffer.as_str(), text_start, text_end)
        {
            if cursor < token_start {
                let segment_x =
                    x + self.text_width(painter, &buffer.as_str()[text_start..cursor], font);
                painter.text(
                    egui::pos2(segment_x, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    &buffer.as_str()[cursor..token_start],
                    font.clone(),
                    text_color,
                );
            }
            let token_x =
                x + self.text_width(painter, &buffer.as_str()[text_start..token_start], font);
            painter.text(
                egui::pos2(token_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[token_start..token_end],
                font.clone(),
                color,
            );
            cursor = token_end;
        }
        if cursor < text_end {
            let segment_x =
                x + self.text_width(painter, &buffer.as_str()[text_start..cursor], font);
            painter.text(
                egui::pos2(segment_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[cursor..text_end],
                font.clone(),
                text_color,
            );
        }
    }

// ---- from editor_view.rs: fn add_months
fn add_months(year: i32, month: u32, offset: i32) -> (i32, u32) {
    let total = year * 12 + month as i32 - 1 + offset;
    (total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
}


// ---- from editor_view.rs: fn days_in_month
fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = add_months(year, month, 1);
    let Some(first_next) = NaiveDate::from_ymd_opt(next_year, next_month, 1) else {
        return 28;
    };
    first_next.pred_opt().map(|date| date.day()).unwrap_or(28)
}

