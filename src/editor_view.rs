use crate::{editor_buffer::EditorBuffer, search::SearchState};

use eframe::egui::{self, Color32, FontFamily, FontId, Key, Stroke};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineNumberMode {
    Absolute,
    Relative,
}

impl LineNumberMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            LineNumberMode::Absolute => "absolute",
            LineNumberMode::Relative => "relative",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        self.label()
    }

    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value.trim().trim_matches('"') {
            "absolute" => Some(LineNumberMode::Absolute),
            "relative" => Some(LineNumberMode::Relative),
            _ => None,
        }
    }

    pub(crate) fn number_for_line(self, line_index: usize, current_line_index: usize) -> usize {
        match self {
            LineNumberMode::Absolute => line_index + 1,
            LineNumberMode::Relative => line_index.abs_diff(current_line_index) + 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckboxState {
    Empty,
    Doing,
    Done,
}

pub(crate) struct ParsedCheckboxLine<'a> {
    pub(crate) indent: &'a str,
    pub(crate) task_prefix: &'a str,
    pub(crate) marker: &'a str,
    pub(crate) state: CheckboxState,
    pub(crate) text: &'a str,
}

pub(crate) fn is_markdown_separator(line: &str) -> bool {
    line.trim() == "---"
}

pub(crate) struct ParsedBlockquoteLine<'a> {
    pub(crate) indent: &'a str,
    pub(crate) marker: &'a str,
    pub(crate) depth: usize,
    pub(crate) text: &'a str,
}

pub(crate) struct ParsedCodeFence<'a> {
    pub(crate) language: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InlineCodeSpan {
    pub(crate) marker_start: usize,
    pub(crate) code_start: usize,
    pub(crate) code_end: usize,
    pub(crate) marker_end: usize,
}

pub(crate) fn parse_inline_code_spans(line: &str) -> Vec<InlineCodeSpan> {
    let mut spans = Vec::new();
    let mut search_from = 0;
    while let Some(open_relative) = line[search_from..].find('`') {
        let marker_start = search_from + open_relative;
        let code_start = marker_start + 1;
        let Some(close_relative) = line[code_start..].find('`') else {
            break;
        };
        let code_end = code_start + close_relative;
        let marker_end = code_end + 1;
        if code_start < code_end {
            spans.push(InlineCodeSpan {
                marker_start,
                code_start,
                code_end,
                marker_end,
            });
        }
        search_from = marker_end;
    }
    spans
}

pub(crate) fn parse_fenced_code_marker(line: &str) -> Option<ParsedCodeFence<'_>> {
    let trimmed = line.trim_start();
    let language = trimmed.strip_prefix("```")?;
    if language.starts_with('`') || language.contains("```") {
        return None;
    }
    Some(ParsedCodeFence {
        language: language.trim(),
    })
}

pub(crate) fn parse_blockquote_line(line: &str) -> Option<ParsedBlockquoteLine<'_>> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    let depth = rest.chars().take_while(|ch| *ch == '>').count();
    if depth == 0 {
        return None;
    }

    let marker_without_space_len = depth;
    let has_space = rest.as_bytes().get(marker_without_space_len) == Some(&b' ');
    let marker_len = marker_without_space_len + usize::from(has_space);
    Some(ParsedBlockquoteLine {
        indent,
        marker: &rest[..marker_len],
        depth,
        text: &rest[marker_len..],
    })
}

pub(crate) fn parse_checkbox_line(line: &str) -> Option<ParsedCheckboxLine<'_>> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    let (task_prefix, rest) = if let Some(rest) = rest.strip_prefix("-- ") {
        ("-- ", rest)
    } else if let Some(rest) = rest.strip_prefix("- ") {
        ("- ", rest)
    } else {
        ("", rest)
    };
    let (marker, state, text) = if let Some(text) = rest.strip_prefix("[ ] ") {
        ("[ ]", CheckboxState::Empty, text)
    } else if let Some(text) = rest.strip_prefix("[] ") {
        ("[]", CheckboxState::Empty, text)
    } else if let Some(text) = rest.strip_prefix("[/] ") {
        ("[/]", CheckboxState::Doing, text)
    } else if let Some(text) = rest.strip_prefix("[x] ") {
        ("[x]", CheckboxState::Done, text)
    } else if let Some(text) = rest.strip_prefix("[X] ") {
        ("[X]", CheckboxState::Done, text)
    } else {
        return None;
    };

    Some(ParsedCheckboxLine {
        indent,
        task_prefix,
        marker,
        state,
        text,
    })
}

#[derive(Clone, Copy)]
pub(crate) struct VisualRow {
    line_index: usize,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CodeLineKind<'a> {
    Fence { language: &'a str },
    Body,
}

pub(crate) struct EditorView {
    scroll_y: f32,
    target_cursor: Option<usize>,
}

impl EditorView {
    pub(crate) fn new() -> Self {
        Self {
            scroll_y: 0.0,
            target_cursor: None,
        }
    }

    pub(crate) fn observe_buffer(&mut self, buffer: &EditorBuffer) {
        if self
            .target_cursor
            .map(|cursor| cursor > buffer.as_str().len())
            .unwrap_or(false)
        {
            self.target_cursor = Some(buffer.as_str().len());
        }
        self.scroll_y = self.scroll_y.max(0.0);
    }

    #[allow(dead_code)]
    pub(crate) fn visible_line_range(
        &self,
        buffer: &EditorBuffer,
        viewport_height: f32,
        line_height: f32,
    ) -> std::ops::Range<usize> {
        self.visible_row_range(buffer.line_count(), viewport_height, line_height)
    }

    pub(crate) fn visible_row_range(
        &self,
        row_count: usize,
        viewport_height: f32,
        line_height: f32,
    ) -> std::ops::Range<usize> {
        let line_height = line_height.max(1.0);
        let row_count = row_count.max(1);
        let first = (self.scroll_y / line_height).floor().max(0.0) as usize;
        let visible = (viewport_height / line_height).ceil().max(1.0) as usize + 1;
        let start = first.min(row_count.saturating_sub(1));
        let end = (start + visible).min(row_count);
        start..end
    }

    pub(crate) fn render(
        &mut self,
        ui: &mut egui::Ui,
        buffer: &mut EditorBuffer,
        wrap: bool,
        search_state: Option<&SearchState>,
        line_number_mode: LineNumberMode,
        keyboard_enabled: bool,
        active_line_text_highlight: Option<usize>,
        render_markdown: bool,
    ) -> (egui::Response, bool) {
        self.observe_buffer(buffer);

        let font = FontId::new(15.0, FontFamily::Monospace);
        let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 2.0;
        let available = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(available, egui::Sense::hover());
        let response = ui
            .interact(
                rect,
                ui.id().with("native_editor_view"),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Text);
        let painter = ui.painter_at(rect);
        let gutter_width = 22.0;
        let text_x = rect.left() + gutter_width + 6.0;
        let gutter_x = rect.left() + gutter_width - 4.0;
        let wrap_width = (rect.right() - text_x - 8.0).max(40.0);
        let mut rows = self.visual_rows(&painter, buffer, &font, wrap_width, wrap);
        let mut max_scroll = (rows.len() as f32 * line_height - rect.height()).max(0.0);
        let mut changed = false;

        if response.clicked() {
            response.request_focus();
            if let Some(pos) = response.interact_pointer_pos() {
                let row_index = ((self.scroll_y + pos.y - rect.top()) / line_height)
                    .floor()
                    .max(0.0) as usize;
                let row = rows
                    .get(row_index.min(rows.len().saturating_sub(1)))
                    .copied()
                    .unwrap_or(VisualRow {
                        line_index: 0,
                        start: 0,
                        end: 0,
                    });
                let byte = self.byte_for_x(&painter, buffer, row, pos.x - text_x, &font);
                buffer.set_cursor(byte);
                self.request_scroll_to_cursor(buffer);
            }
        }

        if response.hovered() || response.has_focus() {
            let scroll_delta = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll_delta != 0.0 {
                self.scroll_y = (self.scroll_y - scroll_delta).clamp(0.0, max_scroll);
            }
        }

        if keyboard_enabled && response.has_focus() && search_state.is_none() {
            changed = self.handle_keyboard(ui, buffer) || changed;
            if changed {
                rows = self.visual_rows(&painter, buffer, &font, wrap_width, wrap);
                max_scroll = (rows.len() as f32 * line_height - rect.height()).max(0.0);
            }
        }

        if let Some(target) = self.target_cursor.take() {
            let row_index = self.row_index_for_byte(&rows, target);
            let y = row_index as f32 * line_height;
            if y < self.scroll_y {
                self.scroll_y = y;
            } else if y + line_height > self.scroll_y + rect.height() {
                self.scroll_y = (y + line_height - rect.height()).max(0.0);
            }
        }
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);

        painter.rect_filled(rect, 0.0, Color32::from_rgb(30, 36, 48));

        let range = self.visible_row_range(rows.len(), rect.height(), line_height);
        let y_offset = self.scroll_y % line_height;
        let current_line_index = buffer.cursor_line_col().0;
        for (visible_index, row_index) in range.enumerate() {
            let Some(row) = rows.get(row_index).copied() else {
                continue;
            };
            let y = rect.top() + visible_index as f32 * line_height - y_offset;
            if y > rect.bottom() || y + line_height < rect.top() {
                continue;
            }

            let line_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), y),
                egui::vec2(rect.width(), line_height),
            );
            if row.line_index % 2 == 0 {
                painter.rect_filled(line_rect, 0.0, Color32::from_rgb(29, 35, 46));
            }

            self.paint_markdown_line_background(
                &painter,
                buffer,
                row,
                text_x,
                y,
                line_height,
                rect.right(),
                render_markdown,
                &font,
            );

            if let Some(search_state) =
                search_state.filter(|state| state.buffer_revision == buffer.revision)
            {
                self.paint_search_matches(
                    &painter,
                    buffer,
                    row,
                    search_state,
                    text_x,
                    y,
                    line_height,
                    &font,
                    rect.right(),
                );
            }

            if let Some((selection_start, selection_end)) = buffer.selection() {
                self.paint_selection(
                    &painter,
                    buffer,
                    row,
                    selection_start,
                    selection_end,
                    text_x,
                    y,
                    line_height,
                    &font,
                    rect.right(),
                );
            }

            painter.text(
                egui::pos2(gutter_x, y + line_height * 0.5 + 2.0),
                egui::Align2::RIGHT_CENTER,
                if row.start == buffer.line_start(row.line_index) {
                    format!(
                        "{}",
                        line_number_mode.number_for_line(row.line_index, current_line_index)
                    )
                } else {
                    "·".to_string()
                },
                FontId::new(12.0, FontFamily::Monospace),
                Color32::from_rgb(94, 105, 126),
            );
            let text_color = if active_line_text_highlight == Some(row.line_index) {
                Color32::from_rgb(235, 203, 139)
            } else {
                Color32::from_rgb(216, 222, 233)
            };
            self.paint_line_text(
                &painter,
                buffer,
                row,
                text_x,
                y,
                line_height,
                &font,
                text_color,
                render_markdown,
            );
        }

        if response.has_focus() {
            self.paint_cursor(
                ui,
                &painter,
                buffer,
                &rows,
                rect,
                text_x,
                line_height,
                &font,
            );
        }

        (response, changed)
    }

    pub(crate) fn visual_rows(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        font: &FontId,
        wrap_width: f32,
        wrap: bool,
    ) -> Vec<VisualRow> {
        let mut rows = Vec::new();
        for line_index in 0..buffer.line_count() {
            let line_start = buffer.line_start(line_index);
            let line_end = buffer.line_end(line_index);
            let line = buffer.line(line_index);
            if !wrap || line.is_empty() {
                rows.push(VisualRow {
                    line_index,
                    start: line_start,
                    end: line_end,
                });
                continue;
            }

            let mut start_offset = 0;
            while start_offset < line.len() {
                let mut best_end = line.len();
                let mut exceeded = false;

                for end_offset in line
                    .char_indices()
                    .map(|(offset, _)| offset)
                    .filter(|offset| *offset > start_offset)
                    .chain(std::iter::once(line.len()))
                {
                    let width = self.text_width(painter, &line[start_offset..end_offset], font);
                    if width > wrap_width {
                        exceeded = true;
                        break;
                    }
                    best_end = end_offset;
                }

                if exceeded && best_end == start_offset {
                    best_end = line[start_offset..]
                        .char_indices()
                        .nth(1)
                        .map(|(offset, _)| start_offset + offset)
                        .unwrap_or(line.len());
                }

                rows.push(VisualRow {
                    line_index,
                    start: line_start + start_offset,
                    end: line_start + best_end,
                });
                start_offset = best_end;
            }
        }
        rows
    }

    pub(crate) fn row_index_for_byte(&self, rows: &[VisualRow], byte: usize) -> usize {
        rows.iter()
            .position(|row| byte >= row.start && byte <= row.end)
            .unwrap_or_else(|| rows.len().saturating_sub(1))
    }

    pub(crate) fn handle_keyboard(&mut self, ui: &egui::Ui, buffer: &mut EditorBuffer) -> bool {
        let mut changed = false;
        let mut moved = false;
        let events = ui.input(|input| input.events.clone());
        if !ui.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            if let Some(text) = text_input_from_events(&events) {
                self.insert_text_with_checkbox_expansion(buffer, &text);
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }
        }
        for event in events {
            if let egui::Event::Paste(text) = event {
                let text = Self::normalize_paste_text(&text);
                buffer.insert_text(&text);
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }
        }

        ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::CTRL, Key::Enter) {
                changed = Self::cycle_current_line_checkbox(buffer) || changed;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Enter) {
                buffer.insert_newline();
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Backspace) {
                buffer.backspace();
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Delete) {
                buffer.delete();
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }

            let shift_left = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowLeft);
            if shift_left || input.consume_key(egui::Modifiers::NONE, Key::ArrowLeft) {
                let anchor = Self::selection_anchor(buffer);
                if shift_left {
                    buffer.clear_selection();
                }
                buffer.move_left();
                Self::extend_selection(buffer, anchor, shift_left);
                moved = true;
            }

            let shift_right = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowRight);
            if shift_right || input.consume_key(egui::Modifiers::NONE, Key::ArrowRight) {
                let anchor = Self::selection_anchor(buffer);
                if shift_right {
                    buffer.clear_selection();
                }
                buffer.move_right();
                Self::extend_selection(buffer, anchor, shift_right);
                moved = true;
            }

            let shift_up = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowUp);
            if shift_up || input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                let anchor = Self::selection_anchor(buffer);
                if shift_up {
                    buffer.clear_selection();
                }
                buffer.move_up();
                Self::extend_selection(buffer, anchor, shift_up);
                moved = true;
            }

            let shift_down = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowDown);
            if shift_down || input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                let anchor = Self::selection_anchor(buffer);
                if shift_down {
                    buffer.clear_selection();
                }
                buffer.move_down();
                Self::extend_selection(buffer, anchor, shift_down);
                moved = true;
            }

            let ctrl_shift_home = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::Home,
            );
            let ctrl_shift_end = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::End,
            );
            if ctrl_shift_home || input.consume_key(egui::Modifiers::CTRL, Key::Home) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_home {
                    buffer.clear_selection();
                }
                buffer.move_to_top();
                Self::extend_selection(buffer, anchor, ctrl_shift_home);
                moved = true;
            }
            if ctrl_shift_end || input.consume_key(egui::Modifiers::CTRL, Key::End) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_end {
                    buffer.clear_selection();
                }
                buffer.move_to_bottom();
                Self::extend_selection(buffer, anchor, ctrl_shift_end);
                moved = true;
            }

            let shift_home = input.consume_key(egui::Modifiers::SHIFT, Key::Home);
            if shift_home || input.consume_key(egui::Modifiers::NONE, Key::Home) {
                let anchor = Self::selection_anchor(buffer);
                if shift_home {
                    buffer.clear_selection();
                }
                buffer.move_to_line_start();
                Self::extend_selection(buffer, anchor, shift_home);
                moved = true;
            }

            let shift_end = input.consume_key(egui::Modifiers::SHIFT, Key::End);
            if shift_end || input.consume_key(egui::Modifiers::NONE, Key::End) {
                let anchor = Self::selection_anchor(buffer);
                if shift_end {
                    buffer.clear_selection();
                }
                buffer.move_to_line_end();
                Self::extend_selection(buffer, anchor, shift_end);
                moved = true;
            }

            let shift_page_up = input.consume_key(egui::Modifiers::SHIFT, Key::PageUp);
            if shift_page_up || input.consume_key(egui::Modifiers::NONE, Key::PageUp) {
                let anchor = Self::selection_anchor(buffer);
                self.scroll_y = (self.scroll_y - 12.0 * 20.0).max(0.0);
                if shift_page_up {
                    buffer.clear_selection();
                }
                for _ in 0..12 {
                    buffer.move_up();
                }
                Self::extend_selection(buffer, anchor, shift_page_up);
                moved = true;
            }

            let shift_page_down = input.consume_key(egui::Modifiers::SHIFT, Key::PageDown);
            if shift_page_down || input.consume_key(egui::Modifiers::NONE, Key::PageDown) {
                let anchor = Self::selection_anchor(buffer);
                self.scroll_y += 12.0 * 20.0;
                if shift_page_down {
                    buffer.clear_selection();
                }
                for _ in 0..12 {
                    buffer.move_down();
                }
                Self::extend_selection(buffer, anchor, shift_page_down);
                moved = true;
            }
        });

        if moved {
            self.request_scroll_to_cursor(buffer);
        }

        changed
    }

    pub(crate) fn cycle_current_line_checkbox(buffer: &mut EditorBuffer) -> bool {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        Self::cycle_checkbox_at_line(buffer, line_index)
    }

    pub(crate) fn cycle_checkbox_at_line(buffer: &mut EditorBuffer, line_index: usize) -> bool {
        let cursor = buffer.cursor();
        let line_start = buffer.line_start(line_index);
        let line = buffer.line(line_index);
        let Some(parsed) = parse_checkbox_line(line) else {
            return false;
        };
        let marker_start = line_start + parsed.indent.len() + parsed.task_prefix.len();
        let marker_end = marker_start + parsed.marker.len();
        let next_marker = match parsed.state {
            CheckboxState::Empty => "[/]",
            CheckboxState::Doing => "[x]",
            CheckboxState::Done => "[ ]",
        };
        let next_cursor = if cursor <= marker_start {
            cursor
        } else if cursor <= marker_end {
            marker_start + cursor.saturating_sub(marker_start).min(next_marker.len())
        } else {
            cursor
                .saturating_add(next_marker.len())
                .saturating_sub(parsed.marker.len())
        };
        buffer.replace_selection_or_range(marker_start, marker_end, next_marker);
        buffer.set_cursor(next_cursor);
        true
    }

    pub(crate) fn insert_text_with_checkbox_expansion(
        &mut self,
        buffer: &mut EditorBuffer,
        text: &str,
    ) {
        if text == " " && buffer.selection().is_none() && buffer.cursor() >= 2 {
            let cursor = buffer.cursor();
            let source = buffer.as_str();
            if source.get(cursor.saturating_sub(2)..cursor) == Some("[]") {
                let line_index = buffer.line_index_for_byte(cursor);
                let line_start = buffer.line_start(line_index);
                let before_marker = &source[line_start..cursor.saturating_sub(2)];
                let marker_allowed = before_marker.chars().all(char::is_whitespace)
                    || before_marker
                        .strip_suffix("- ")
                        .map(|before_prefix| before_prefix.chars().all(char::is_whitespace))
                        .unwrap_or(false)
                    || before_marker
                        .strip_suffix("-- ")
                        .map(|before_prefix| before_prefix.chars().all(char::is_whitespace))
                        .unwrap_or(false);
                if marker_allowed {
                    buffer.replace_selection_or_range(cursor - 2, cursor, "[ ] ");
                    return;
                }
            }
        }
        buffer.insert_text(text);
    }

    pub(crate) fn normalize_paste_text(text: &str) -> String {
        let mut normalized = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\u{00a0}', " ")
            .replace(['\u{200b}', '\u{200c}', '\u{200d}', '\u{feff}'], "");

        if normalized.lines().count() <= 1 {
            return normalized.trim_end_matches([' ', '\t']).to_string();
        }

        let mut lines = normalized
            .split('\n')
            .map(|line| line.trim_end_matches([' ', '\t']).to_string())
            .collect::<Vec<_>>();

        while lines
            .first()
            .map(|line| line.trim().is_empty())
            .unwrap_or(false)
        {
            lines.remove(0);
        }
        while lines
            .last()
            .map(|line| line.trim().is_empty())
            .unwrap_or(false)
        {
            lines.pop();
        }

        let common_indent = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                line.chars()
                    .take_while(|ch| *ch == ' ' || *ch == '\t')
                    .count()
            })
            .min()
            .unwrap_or(0);

        if common_indent > 0 {
            for line in &mut lines {
                if line.trim().is_empty() {
                    continue;
                }
                let byte_index = line
                    .char_indices()
                    .nth(common_indent)
                    .map(|(index, _)| index)
                    .unwrap_or_else(|| line.len());
                line.replace_range(..byte_index, "");
            }
        }

        normalized = lines.join("\n");
        normalized
    }

    pub(crate) fn selection_anchor(buffer: &EditorBuffer) -> usize {
        buffer
            .selection()
            .map(|(start, end)| if buffer.cursor() == start { end } else { start })
            .unwrap_or_else(|| buffer.cursor())
    }

    pub(crate) fn extend_selection(buffer: &mut EditorBuffer, anchor: usize, extend: bool) {
        if extend {
            buffer.set_selection(anchor, buffer.cursor());
        }
    }

    fn code_line_kind<'a>(
        &self,
        buffer: &'a EditorBuffer,
        line_index: usize,
    ) -> Option<CodeLineKind<'a>> {
        let mut in_code = false;
        for index in 0..=line_index {
            let line = buffer.line(index);
            if let Some(fence) = parse_fenced_code_marker(line) {
                if index == line_index {
                    return Some(CodeLineKind::Fence {
                        language: fence.language,
                    });
                }
                in_code = !in_code;
                continue;
            }
            if index == line_index && in_code {
                return Some(CodeLineKind::Body);
            }
        }
        None
    }

    fn code_block_range(&self, buffer: &EditorBuffer, line_index: usize) -> Option<(usize, usize)> {
        let mut block_start = None;
        for index in 0..buffer.line_count() {
            if parse_fenced_code_marker(buffer.line(index)).is_none() {
                continue;
            }

            if let Some(start) = block_start {
                let end = index;
                if line_index >= start && line_index <= end {
                    return Some((start, end));
                }
                block_start = None;
            } else {
                block_start = Some(index);
            }
        }

        block_start.and_then(|start| {
            (line_index >= start).then(|| (start, buffer.line_count().saturating_sub(1)))
        })
    }

    fn paint_markdown_line_background(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        right: f32,
        render_markdown: bool,
        font: &FontId,
    ) {
        if !render_markdown {
            return;
        }

        if self.code_line_kind(buffer, row.line_index).is_some() {
            let cursor_line = buffer.cursor_line_col().0;
            let cursor_in_this_code_block = self
                .code_block_range(buffer, row.line_index)
                .is_some_and(|line_range| {
                    self.code_block_range(buffer, cursor_line) == Some(line_range)
                });
            if cursor_in_this_code_block {
                return;
            }

            let code_rect = egui::Rect::from_min_max(
                egui::pos2(text_x - 4.0, y + 1.0),
                egui::pos2(right - 8.0, y + line_height - 1.0),
            );
            painter.rect_filled(code_rect, 2.0, Color32::from_rgb(25, 31, 40));
            return;
        }

        let line = buffer.line(row.line_index);
        let line_start = buffer.line_start(row.line_index);
        let cursor = buffer.cursor();
        let spans = parse_inline_code_spans(line);
        if spans.iter().any(|span| {
            cursor >= line_start + span.marker_start && cursor <= line_start + span.marker_end
        }) {
            return;
        }

        for span in spans {
            let start = (line_start + span.marker_start).max(row.start);
            let end = (line_start + span.marker_end).min(row.end);
            if start >= end {
                continue;
            }
            let start_x =
                text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
            let end_x = text_x + self.text_width(painter, &buffer.as_str()[row.start..end], font);
            let code_rect = egui::Rect::from_min_max(
                egui::pos2(start_x - 2.0, y + 2.0),
                egui::pos2(end_x + 2.0, y + line_height - 2.0),
            );
            painter.rect_filled(code_rect, 3.0, Color32::from_rgb(31, 38, 50));
        }
    }

    pub(crate) fn paint_line_text(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        text_color: Color32,
        render_markdown: bool,
    ) {
        let line_start = buffer.line_start(row.line_index);
        if !render_markdown {
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[row.start..row.end],
                font.clone(),
                text_color,
            );
            return;
        }
        let line = buffer.line(row.line_index);
        if let Some(code_kind) = self.code_line_kind(buffer, row.line_index) {
            let cursor_line = buffer.cursor_line_col().0;
            let cursor_in_this_code_block = self
                .code_block_range(buffer, row.line_index)
                .is_some_and(|line_range| {
                    self.code_block_range(buffer, cursor_line) == Some(line_range)
                });
            if cursor_in_this_code_block {
                painter.text(
                    egui::pos2(text_x, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    &buffer.as_str()[row.start..row.end],
                    font.clone(),
                    text_color,
                );
                return;
            }

            match code_kind {
                CodeLineKind::Fence { .. } if row.start == line_start => {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[row.start..row.end],
                        font.clone(),
                        Color32::from_rgb(136, 154, 176),
                    );
                    return;
                }
                CodeLineKind::Body => {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[row.start..row.end],
                        font.clone(),
                        Color32::from_rgb(216, 222, 233),
                    );
                    return;
                }
                _ => {}
            }
        }

        if is_markdown_separator(line)
            && buffer.cursor_line_col().0 != row.line_index
            && row.start == line_start
        {
            let separator_y = y + line_height * 0.5;
            painter.line_segment(
                [
                    egui::pos2(text_x, separator_y),
                    egui::pos2(painter.clip_rect().right() - 8.0, separator_y),
                ],
                Stroke::new(1.0, Color32::from_rgb(76, 86, 106)),
            );
            return;
        }

        if let Some(blockquote) = parse_blockquote_line(line) {
            let marker_start = line_start + blockquote.indent.len();
            let marker_end = marker_start + blockquote.marker.len();
            let cursor_in_marker = buffer.cursor() >= marker_start && buffer.cursor() <= marker_end;
            if !cursor_in_marker && row.start == line_start {
                let indent_width = self.text_width(painter, blockquote.indent, font);
                let marker_width = self.text_width(painter, blockquote.marker, font);
                if !blockquote.indent.is_empty() {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        blockquote.indent,
                        font.clone(),
                        text_color,
                    );
                }
                let previous_quote_depth = row
                    .line_index
                    .checked_sub(1)
                    .and_then(|line_index| parse_blockquote_line(buffer.line(line_index)))
                    .map(|quote| quote.depth)
                    .unwrap_or(0);
                let next_quote_depth = (row.line_index + 1 < buffer.line_count())
                    .then(|| parse_blockquote_line(buffer.line(row.line_index + 1)))
                    .flatten()
                    .map(|quote| quote.depth)
                    .unwrap_or(0);
                for depth in 0..blockquote.depth {
                    let quote_level = depth + 1;
                    let x = text_x + indent_width + depth as f32 * 4.0 + 2.0;
                    let top = if previous_quote_depth >= quote_level {
                        y
                    } else {
                        y + 3.0
                    };
                    let bottom = if next_quote_depth >= quote_level {
                        y + line_height
                    } else {
                        y + line_height - 3.0
                    };
                    painter.line_segment(
                        [egui::pos2(x, top), egui::pos2(x, bottom)],
                        Stroke::new(1.0, Color32::from_rgb(136, 192, 208)),
                    );
                }
                if row.end > marker_end {
                    painter.text(
                        egui::pos2(text_x + indent_width + marker_width, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[marker_end..row.end],
                        font.clone(),
                        Color32::from_rgb(190, 200, 216),
                    );
                }
                return;
            }
        }

        let Some(parsed) = parse_checkbox_line(line) else {
            if self.paint_inline_code_text(
                painter,
                buffer,
                row,
                text_x,
                y,
                line_height,
                font,
                text_color,
            ) {
                return;
            }
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[row.start..row.end],
                font.clone(),
                text_color,
            );
            return;
        };

        let marker_start = line_start + parsed.indent.len() + parsed.task_prefix.len();
        let marker_text_end = marker_start + parsed.marker.len();
        let marker_end = marker_text_end + 1;
        let cursor_in_checkbox =
            buffer.cursor() >= marker_start && buffer.cursor() <= marker_text_end;
        if cursor_in_checkbox
            || row.start != line_start
            || row.end <= marker_start
            || row.start >= marker_end
        {
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[row.start..row.end],
                font.clone(),
                text_color,
            );
            return;
        }

        let indent_width = self.text_width(painter, parsed.indent, font);
        let prefix_width = self.text_width(painter, parsed.task_prefix, font);
        let checkbox_slot = if parsed.marker == "[]" { "[] " } else { "[x] " };
        let checkbox_slot_width = self.text_width(painter, checkbox_slot, font);
        if !parsed.indent.is_empty() {
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                parsed.indent,
                font.clone(),
                text_color,
            );
        }

        if !parsed.task_prefix.is_empty() {
            painter.text(
                egui::pos2(text_x + indent_width, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                parsed.task_prefix,
                font.clone(),
                Color32::from_rgb(136, 154, 176),
            );
        }

        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(
                text_x + indent_width + prefix_width,
                y + (line_height - 13.0) * 0.5,
            ),
            egui::vec2(13.0, 13.0),
        );
        let (fill, stroke) = match parsed.state {
            CheckboxState::Empty => (
                Color32::from_rgb(30, 36, 48),
                Color32::from_rgb(136, 154, 176),
            ),
            CheckboxState::Doing => (
                Color32::from_rgb(59, 66, 82),
                Color32::from_rgb(235, 203, 139),
            ),
            CheckboxState::Done => (
                Color32::from_rgb(49, 70, 60),
                Color32::from_rgb(163, 190, 140),
            ),
        };
        painter.rect_filled(icon_rect, 2.0, fill);
        painter.rect_stroke(
            icon_rect,
            2.0,
            Stroke::new(1.2, stroke),
            egui::StrokeKind::Outside,
        );
        let glyph_rect = icon_rect.shrink(3.2);
        match parsed.state {
            CheckboxState::Empty => {}
            CheckboxState::Doing => {
                painter.line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5, stroke),
                );
            }
            CheckboxState::Done => {
                painter.line_segment(
                    [glyph_rect.left_top(), glyph_rect.right_bottom()],
                    Stroke::new(1.5, stroke),
                );
                painter.line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5, stroke),
                );
            }
        }

        let text_start = marker_end;
        if row.end > text_start {
            painter.text(
                egui::pos2(
                    text_x + indent_width + prefix_width + checkbox_slot_width,
                    y + line_height * 0.5,
                ),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[text_start..row.end],
                font.clone(),
                text_color,
            );
        }
    }

    fn paint_inline_code_text(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        text_color: Color32,
    ) -> bool {
        let line = buffer.line(row.line_index);
        let line_start = buffer.line_start(row.line_index);
        let spans = parse_inline_code_spans(line);
        if spans.is_empty()
            || spans.iter().any(|span| {
                buffer.cursor() >= line_start + span.marker_start
                    && buffer.cursor() <= line_start + span.marker_end
            })
        {
            return false;
        }

        let mut byte = row.start;
        for span in spans {
            let marker_start = line_start + span.marker_start;
            let code_start = line_start + span.code_start;
            let code_end = line_start + span.code_end;
            let marker_end = line_start + span.marker_end;
            if marker_end <= row.start || marker_start >= row.end {
                continue;
            }

            for (start, end, color) in [
                (byte, marker_start.min(row.end), text_color),
                (
                    marker_start.max(row.start),
                    code_start.min(row.end),
                    Color32::from_rgb(94, 105, 126),
                ),
                (
                    code_start.max(row.start),
                    code_end.min(row.end),
                    Color32::from_rgb(235, 203, 139),
                ),
                (
                    code_end.max(row.start),
                    marker_end.min(row.end),
                    Color32::from_rgb(94, 105, 126),
                ),
            ] {
                if start < end {
                    let x =
                        text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
                    painter.text(
                        egui::pos2(x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[start..end],
                        font.clone(),
                        color,
                    );
                }
            }
            byte = marker_end.max(row.start).min(row.end);
        }

        if byte < row.end {
            let x = text_x + self.text_width(painter, &buffer.as_str()[row.start..byte], font);
            painter.text(
                egui::pos2(x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[byte..row.end],
                font.clone(),
                text_color,
            );
        }

        true
    }

    pub(crate) fn paint_search_matches(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        search_state: &SearchState,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        right: f32,
    ) {
        for (index, (match_start, match_end)) in search_state.matches.iter().copied().enumerate() {
            let start = match_start.max(row.start).min(row.end);
            let end = match_end.max(row.start).min(row.end);
            if start >= end {
                continue;
            }

            let start_x =
                text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
            let end_x = text_x + self.text_width(painter, &buffer.as_str()[row.start..end], font);
            let color = if index == search_state.selected {
                Color32::from_rgb(235, 203, 139)
            } else {
                Color32::from_rgb(76, 86, 106)
            };
            let rect = egui::Rect::from_min_max(
                egui::pos2(start_x, y + 2.0),
                egui::pos2(end_x.min(right), y + line_height - 2.0),
            );
            painter.rect_filled(rect, 2.0, color);
        }
    }

    pub(crate) fn paint_selection(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        selection_start: usize,
        selection_end: usize,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        right: f32,
    ) {
        let start = selection_start.max(row.start).min(row.end);
        let end = selection_end.max(row.start).min(row.end);
        if start >= end {
            return;
        }
        let start_x = text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
        let end_x = text_x + self.text_width(painter, &buffer.as_str()[row.start..end], font);
        let rect = egui::Rect::from_min_max(
            egui::pos2(start_x, y + 2.0),
            egui::pos2(end_x.min(right), y + line_height - 2.0),
        );
        painter.rect_filled(rect, 2.0, Color32::from_rgb(67, 86, 117));
    }

    pub(crate) fn paint_cursor(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        rows: &[VisualRow],
        rect: egui::Rect,
        text_x: f32,
        line_height: f32,
        font: &FontId,
    ) {
        let row_index = self.row_index_for_byte(rows, buffer.cursor());
        let Some(row) = rows.get(row_index).copied() else {
            return;
        };
        let y = rect.top() + row_index as f32 * line_height - self.scroll_y;
        if y + line_height < rect.top() || y > rect.bottom() {
            return;
        }
        let x =
            text_x + self.text_width(painter, &buffer.as_str()[row.start..buffer.cursor()], font);
        painter.line_segment(
            [egui::pos2(x, y + 3.0), egui::pos2(x, y + line_height - 3.0)],
            Stroke::new(1.5, Color32::from_rgb(236, 239, 244)),
        );
        ui.ctx().request_repaint();
    }

    pub(crate) fn text_width(&self, painter: &egui::Painter, text: &str, font: &FontId) -> f32 {
        if text.is_empty() {
            0.0
        } else {
            painter
                .layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        }
    }

    pub(crate) fn byte_for_x(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        x: f32,
        font: &FontId,
    ) -> usize {
        let text = &buffer.as_str()[row.start..row.end];
        let mut closest = row.start;
        let mut closest_distance = x.abs();

        for (offset, _) in text.char_indices() {
            let width = self.text_width(painter, &text[..offset], font);
            let distance = (width - x).abs();
            if distance < closest_distance {
                closest = row.start + offset;
                closest_distance = distance;
            }
        }

        let end_distance = (self.text_width(painter, text, font) - x).abs();
        if end_distance < closest_distance {
            row.end
        } else {
            closest
        }
    }

    pub(crate) fn request_scroll_to_cursor(&mut self, buffer: &EditorBuffer) {
        self.target_cursor = Some(buffer.cursor());
    }

    #[allow(dead_code)]
    pub(crate) fn clear_scroll_target(&mut self) {
        self.target_cursor = None;
    }
}

fn text_input_from_events(events: &[egui::Event]) -> Option<String> {
    let texts = events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Text(text) if !text.is_empty() => Some(text.as_str()),
            egui::Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty() => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    match texts.as_slice() {
        [] => None,
        [text] => Some((*text).to_string()),
        many => {
            let last = many.last().copied().unwrap_or_default();
            if last.chars().count() == 1 && last.chars().any(|ch| !ch.is_ascii()) {
                Some(last.to_string())
            } else {
                Some(many.concat())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorView, LineNumberMode};
    use crate::editor_buffer::EditorBuffer;

    #[test]
    fn parses_slate_checkbox_lines() {
        let empty = super::parse_checkbox_line("[ ] todo").unwrap();
        assert_eq!(empty.marker, "[ ]");
        assert_eq!(empty.state, super::CheckboxState::Empty);
        assert_eq!(empty.text, "todo");

        let compact_empty = super::parse_checkbox_line("[] todo").unwrap();
        assert_eq!(compact_empty.marker, "[]");
        assert_eq!(compact_empty.state, super::CheckboxState::Empty);

        let subtask = super::parse_checkbox_line("- [ ] subtask").unwrap();
        assert_eq!(subtask.task_prefix, "- ");
        assert_eq!(subtask.marker, "[ ]");
        assert_eq!(subtask.text, "subtask");

        let subsubtask = super::parse_checkbox_line("-- [/] subsubtask").unwrap();
        assert_eq!(subsubtask.task_prefix, "-- ");
        assert_eq!(subsubtask.state, super::CheckboxState::Doing);

        let doing = super::parse_checkbox_line("  [/] doing").unwrap();
        assert_eq!(doing.indent, "  ");
        assert_eq!(doing.state, super::CheckboxState::Doing);
        assert_eq!(doing.text, "doing");

        let done = super::parse_checkbox_line("[x] done").unwrap();
        assert_eq!(done.state, super::CheckboxState::Done);
        assert!(super::parse_checkbox_line("[]todo").is_none());
        assert!(super::parse_checkbox_line("text [] todo").is_none());
    }

    #[test]
    fn normalizes_multiline_paste_text() {
        let pasted = "\r\n    fn main() {  \r\n        println!(\"hi\");\u{00a0}\r\n    }\r\n";

        assert_eq!(
            EditorView::normalize_paste_text(pasted),
            "fn main() {\n    println!(\"hi\");\n}"
        );
    }

    #[test]
    fn normalizes_single_line_paste_text_without_stripping_leading_spaces() {
        assert_eq!(EditorView::normalize_paste_text("  hello  "), "  hello");
    }

    #[test]
    fn parses_slate_markdown_separator() {
        assert!(super::is_markdown_separator("---"));
        assert!(super::is_markdown_separator("  ---  "));
        assert!(!super::is_markdown_separator("----"));
        assert!(!super::is_markdown_separator("text ---"));
    }

    #[test]
    fn parses_inline_code_spans() {
        assert_eq!(
            super::parse_inline_code_spans("use `code` here and `more`"),
            vec![
                super::InlineCodeSpan {
                    marker_start: 4,
                    code_start: 5,
                    code_end: 9,
                    marker_end: 10,
                },
                super::InlineCodeSpan {
                    marker_start: 20,
                    code_start: 21,
                    code_end: 25,
                    marker_end: 26,
                },
            ]
        );
        assert!(super::parse_inline_code_spans("no code").is_empty());
        assert!(super::parse_inline_code_spans("open `code").is_empty());
        assert!(super::parse_inline_code_spans("empty `` span").is_empty());
    }

    #[test]
    fn parses_fenced_code_markers() {
        let rust = super::parse_fenced_code_marker("```rust").unwrap();
        assert_eq!(rust.language, "rust");

        let plain = super::parse_fenced_code_marker("  ```").unwrap();
        assert_eq!(plain.language, "");

        assert!(super::parse_fenced_code_marker("text ```rust").is_none());
        assert!(super::parse_fenced_code_marker("````rust").is_none());
    }

    #[test]
    fn detects_fenced_code_block_ranges() {
        let buffer = EditorBuffer::from_text("before\n```rust\ncode\n```\nafter".to_string());
        let view = EditorView::new();

        assert_eq!(view.code_block_range(&buffer, 0), None);
        assert_eq!(view.code_block_range(&buffer, 1), Some((1, 3)));
        assert_eq!(view.code_block_range(&buffer, 2), Some((1, 3)));
        assert_eq!(view.code_block_range(&buffer, 3), Some((1, 3)));
        assert_eq!(view.code_block_range(&buffer, 4), None);
    }

    #[test]
    fn parses_blockquote_lines() {
        let quote = super::parse_blockquote_line("> quote").unwrap();
        assert_eq!(quote.marker, "> ");
        assert_eq!(quote.depth, 1);
        assert_eq!(quote.text, "quote");

        let nested = super::parse_blockquote_line("  >> nested").unwrap();
        assert_eq!(nested.indent, "  ");
        assert_eq!(nested.marker, ">> ");
        assert_eq!(nested.depth, 2);
        assert_eq!(nested.text, "nested");

        let empty = super::parse_blockquote_line(">").unwrap();
        assert_eq!(empty.marker, ">");
        assert_eq!(empty.text, "");
        assert!(super::parse_blockquote_line("text > quote").is_none());
    }

    #[test]
    fn expands_compact_empty_checkbox_when_space_is_typed() {
        let mut buffer = EditorBuffer::from_text("[]".to_string());
        buffer.set_cursor(2);
        let mut view = EditorView::new();

        view.insert_text_with_checkbox_expansion(&mut buffer, " ");

        assert_eq!(buffer.as_str(), "[ ] ");
        assert_eq!(buffer.cursor(), 4);
    }

    #[test]
    fn expands_compact_empty_subtask_when_space_is_typed() {
        let mut buffer = EditorBuffer::from_text("- []".to_string());
        buffer.set_cursor(4);
        let mut view = EditorView::new();

        view.insert_text_with_checkbox_expansion(&mut buffer, " ");

        assert_eq!(buffer.as_str(), "- [ ] ");
        assert_eq!(buffer.cursor(), 6);
    }

    #[test]
    fn expands_compact_empty_subsubtask_when_space_is_typed() {
        let mut buffer = EditorBuffer::from_text("-- []".to_string());
        buffer.set_cursor(5);
        let mut view = EditorView::new();

        view.insert_text_with_checkbox_expansion(&mut buffer, " ");

        assert_eq!(buffer.as_str(), "-- [ ] ");
        assert_eq!(buffer.cursor(), 7);
    }

    #[test]
    fn cycles_checkbox_state_on_current_line() {
        let mut buffer = EditorBuffer::from_text("[ ] todo".to_string());
        buffer.set_cursor(1);

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));
        assert_eq!(buffer.as_str(), "[/] todo");

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));
        assert_eq!(buffer.as_str(), "[x] todo");

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));
        assert_eq!(buffer.as_str(), "[ ] todo");
    }

    #[test]
    fn cycling_checkbox_preserves_cursor_in_text() {
        let mut buffer = EditorBuffer::from_text("[ ] todo".to_string());
        buffer.set_cursor(8);

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));

        assert_eq!(buffer.as_str(), "[/] todo");
        assert_eq!(buffer.cursor(), 8);
    }

    #[test]
    fn line_number_mode_calculates_relative_numbers() {
        assert_eq!(LineNumberMode::Absolute.number_for_line(9, 4), 10);
        assert_eq!(LineNumberMode::Relative.number_for_line(4, 4), 1);
        assert_eq!(LineNumberMode::Relative.number_for_line(3, 4), 2);
        assert_eq!(LineNumberMode::Relative.number_for_line(5, 4), 2);
    }

    #[test]
    fn editor_view_calculates_visible_line_range() {
        let buffer = EditorBuffer::from_text("1\n2\n3\n4\n5".to_string());
        let mut view = EditorView::new();

        assert_eq!(view.visible_line_range(&buffer, 20.0, 10.0), 0..3);

        view.scroll_y = 20.0;
        assert_eq!(view.visible_line_range(&buffer, 20.0, 10.0), 2..5);
    }
}
