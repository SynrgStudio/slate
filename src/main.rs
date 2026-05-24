use std::{fs, io::Write, path::PathBuf};

use eframe::egui::{self, Color32, FontFamily, FontId, Key, RichText, Stroke, TextEdit, Vec2};

fn main() -> eframe::Result {
    let mut scratch = false;
    let mut path = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--scratch" | "-s" => scratch = true,
            _ => path = Some(PathBuf::from(arg)),
        }
    }

    let title = if scratch { "Slate Scratch" } else { "Slate" };
    let size = if scratch {
        [760.0, 460.0]
    } else {
        [980.0, 700.0]
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size([420.0, 260.0]),
        vsync: false,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(|cc| Ok(Box::new(SlateApp::new(cc, path, scratch)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    New,
    Open,
    Save,
    TogglePreview,
    ToggleWrap,
    Settings,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    New,
    Open,
    Quit,
}

impl PendingAction {
    fn prompt(self) -> &'static str {
        match self {
            PendingAction::New => "buffer has unsaved changes; start a new buffer anyway?",
            PendingAction::Open => "buffer has unsaved changes; open another file anyway?",
            PendingAction::Quit => "buffer has unsaved changes; close anyway?",
        }
    }
}

struct EditorBuffer {
    text: String,
    line_starts: Vec<usize>,
    cursor: usize,
    selection: Option<(usize, usize)>,
    revision: u64,
}

impl EditorBuffer {
    fn new() -> Self {
        Self::from_text(String::new())
    }

    fn from_text(text: String) -> Self {
        let mut buffer = Self {
            text,
            line_starts: Vec::new(),
            cursor: 0,
            selection: None,
            revision: 0,
        };
        buffer.rebuild_line_index();
        buffer
    }

    fn as_str(&self) -> &str {
        &self.text
    }

    #[allow(dead_code)]
    fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }

    fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.cursor.min(self.text.len());
        self.selection = None;
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
    }

    fn clear(&mut self) {
        self.set_text(String::new());
    }

    #[allow(dead_code)]
    fn mark_external_edit(&mut self) {
        self.cursor = self.cursor.min(self.text.len());
        self.selection = None;
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
    }

    fn line_count(&self) -> usize {
        self.line_starts.len().max(1)
    }

    #[allow(dead_code)]
    fn line(&self, line_index: usize) -> &str {
        let start = self.line_start(line_index);
        let end = self.line_end(line_index);
        &self.text[start..end]
    }

    fn line_start(&self, line_index: usize) -> usize {
        self.line_starts.get(line_index).copied().unwrap_or(0)
    }

    fn line_end(&self, line_index: usize) -> usize {
        self.line_starts
            .get(line_index + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(self.text.len())
    }

    fn line_index_for_byte(&self, byte: usize) -> usize {
        let byte = byte.min(self.text.len());
        match self.line_starts.binary_search(&byte) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        }
    }

    fn cursor_line_col(&self) -> (usize, usize) {
        let line_index = self.line_index_for_byte(self.cursor);
        let column = self.text[self.line_start(line_index)..self.cursor]
            .chars()
            .count();
        (line_index, column)
    }

    #[allow(dead_code)]
    fn cursor(&self) -> usize {
        self.cursor
    }

    #[allow(dead_code)]
    fn selection(&self) -> Option<(usize, usize)> {
        self.selection
    }

    #[allow(dead_code)]
    fn set_cursor(&mut self, byte: usize) {
        self.cursor = self.clamp_to_char_boundary(byte);
        self.selection = None;
    }

    #[allow(dead_code)]
    fn set_selection(&mut self, start: usize, end: usize) {
        let start = self.clamp_to_char_boundary(start);
        let end = self.clamp_to_char_boundary(end);
        self.selection = (start != end).then_some((start.min(end), start.max(end)));
        self.cursor = end;
    }

    #[allow(dead_code)]
    fn clear_selection(&mut self) {
        self.selection = None;
    }

    fn move_left(&mut self) {
        if let Some((start, _)) = self.selection.take() {
            self.cursor = start;
        } else {
            self.cursor = self.previous_char_boundary(self.cursor);
        }
    }

    fn move_right(&mut self) {
        if let Some((_, end)) = self.selection.take() {
            self.cursor = end;
        } else {
            self.cursor = self.next_char_boundary(self.cursor);
        }
    }

    fn move_to_line_start(&mut self) {
        let line_index = self.line_index_for_byte(self.cursor);
        self.set_cursor(self.line_start(line_index));
    }

    fn move_to_line_end(&mut self) {
        let line_index = self.line_index_for_byte(self.cursor);
        self.set_cursor(self.line_end(line_index));
    }

    fn move_up(&mut self) {
        let (line_index, column) = self.cursor_line_col();
        if line_index == 0 {
            self.move_to_line_start();
            return;
        }
        let byte = self.line_col_to_byte(line_index, column + 1);
        self.set_cursor(byte);
    }

    fn move_down(&mut self) {
        let (line_index, column) = self.cursor_line_col();
        if line_index + 1 >= self.line_count() {
            self.move_to_line_end();
            return;
        }
        let byte = self.line_col_to_byte(line_index + 2, column + 1);
        self.set_cursor(byte);
    }

    #[allow(dead_code)]
    fn insert_text(&mut self, text: &str) {
        self.replace_selection_or_range(self.cursor, self.cursor, text);
    }

    #[allow(dead_code)]
    fn insert_newline(&mut self) {
        self.insert_text("\n");
    }

    #[allow(dead_code)]
    fn backspace(&mut self) {
        if let Some((start, end)) = self.selection {
            self.replace_selection_or_range(start, end, "");
            return;
        }

        if self.cursor == 0 {
            return;
        }

        let previous = self.previous_char_boundary(self.cursor);
        self.replace_selection_or_range(previous, self.cursor, "");
    }

    #[allow(dead_code)]
    fn delete(&mut self) {
        if let Some((start, end)) = self.selection {
            self.replace_selection_or_range(start, end, "");
            return;
        }

        if self.cursor >= self.text.len() {
            return;
        }

        let next = self.next_char_boundary(self.cursor);
        self.replace_selection_or_range(self.cursor, next, "");
    }

    fn replace_selection_or_range(&mut self, start: usize, end: usize, replacement: &str) {
        let mut start = self.clamp_to_char_boundary(start);
        let mut end = self.clamp_to_char_boundary(end);
        if let Some((selection_start, selection_end)) = self.selection.take() {
            start = selection_start;
            end = selection_end;
        }

        if start > end {
            std::mem::swap(&mut start, &mut end);
        }

        self.text.replace_range(start..end, replacement);
        self.cursor = start + replacement.len();
        self.selection = None;
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
    }

    fn clamp_to_char_boundary(&self, byte: usize) -> usize {
        let mut byte = byte.min(self.text.len());
        while byte > 0 && !self.text.is_char_boundary(byte) {
            byte -= 1;
        }
        byte
    }

    fn previous_char_boundary(&self, byte: usize) -> usize {
        let mut byte = self.clamp_to_char_boundary(byte).saturating_sub(1);
        while byte > 0 && !self.text.is_char_boundary(byte) {
            byte -= 1;
        }
        byte
    }

    fn next_char_boundary(&self, byte: usize) -> usize {
        let mut byte = (self.clamp_to_char_boundary(byte) + 1).min(self.text.len());
        while byte < self.text.len() && !self.text.is_char_boundary(byte) {
            byte += 1;
        }
        byte
    }

    #[allow(dead_code)]
    fn byte_to_line_col(&self, byte: usize) -> (usize, usize) {
        let byte = byte.min(self.text.len());
        let line_index = match self.line_starts.binary_search(&byte) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
        let column = self.text[line_start..byte].chars().count() + 1;
        (line_index + 1, column)
    }

    #[allow(dead_code)]
    fn line_col_to_byte(&self, line: usize, column: usize) -> usize {
        let line_index = line
            .saturating_sub(1)
            .min(self.line_starts.len().saturating_sub(1));
        let line_start = self.line_starts.get(line_index).copied().unwrap_or(0);
        let line_end = self
            .line_starts
            .get(line_index + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(self.text.len());
        let target_column = column.saturating_sub(1);

        self.text[line_start..line_end]
            .char_indices()
            .nth(target_column)
            .map(|(offset, _)| line_start + offset)
            .unwrap_or(line_end)
    }

    fn rebuild_line_index(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        for (index, byte) in self.text.bytes().enumerate() {
            if byte == b'\n' {
                self.line_starts.push(index + 1);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct VisualRow {
    line_index: usize,
    start: usize,
    end: usize,
}

struct EditorView {
    scroll_y: f32,
    target_cursor: Option<usize>,
}

impl EditorView {
    fn new() -> Self {
        Self {
            scroll_y: 0.0,
            target_cursor: None,
        }
    }

    fn observe_buffer(&mut self, buffer: &EditorBuffer) {
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
    fn visible_line_range(
        &self,
        buffer: &EditorBuffer,
        viewport_height: f32,
        line_height: f32,
    ) -> std::ops::Range<usize> {
        self.visible_row_range(buffer.line_count(), viewport_height, line_height)
    }

    fn visible_row_range(
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

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        buffer: &mut EditorBuffer,
        wrap: bool,
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

        if response.has_focus() {
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
                    format!("{}", row.line_index + 1)
                } else {
                    "·".to_string()
                },
                FontId::new(12.0, FontFamily::Monospace),
                Color32::from_rgb(94, 105, 126),
            );
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[row.start..row.end],
                font.clone(),
                Color32::from_rgb(216, 222, 233),
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

    fn visual_rows(
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

    fn row_index_for_byte(&self, rows: &[VisualRow], byte: usize) -> usize {
        rows.iter()
            .position(|row| byte >= row.start && byte <= row.end)
            .unwrap_or_else(|| rows.len().saturating_sub(1))
    }

    fn handle_keyboard(&mut self, ui: &egui::Ui, buffer: &mut EditorBuffer) -> bool {
        let mut changed = false;
        let mut moved = false;
        let events = ui.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    if !text.is_empty()
                        && !ui.input(|input| input.modifiers.ctrl || input.modifiers.command)
                    {
                        buffer.insert_text(&text);
                        self.request_scroll_to_cursor(buffer);
                        changed = true;
                    }
                }
                egui::Event::Paste(text) => {
                    buffer.insert_text(&text);
                    self.request_scroll_to_cursor(buffer);
                    changed = true;
                }
                _ => {}
            }
        }

        ui.input_mut(|input| {
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

    fn selection_anchor(buffer: &EditorBuffer) -> usize {
        buffer
            .selection()
            .map(|(start, end)| if buffer.cursor() == start { end } else { start })
            .unwrap_or_else(|| buffer.cursor())
    }

    fn extend_selection(buffer: &mut EditorBuffer, anchor: usize, extend: bool) {
        if extend {
            buffer.set_selection(anchor, buffer.cursor());
        }
    }

    fn paint_selection(
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

    fn paint_cursor(
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

    fn text_width(&self, painter: &egui::Painter, text: &str, font: &FontId) -> f32 {
        if text.is_empty() {
            0.0
        } else {
            painter
                .layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        }
    }

    fn byte_for_x(
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

    fn request_scroll_to_cursor(&mut self, buffer: &EditorBuffer) {
        self.target_cursor = Some(buffer.cursor());
    }

    #[allow(dead_code)]
    fn clear_scroll_target(&mut self) {
        self.target_cursor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorBuffer, EditorView};

    #[test]
    fn editor_buffer_indexes_lines() {
        let buffer = EditorBuffer::from_text("one\ntwo\nthree".to_string());

        assert_eq!(buffer.line_count(), 3);
        assert_eq!(buffer.byte_to_line_col(0), (1, 1));
        assert_eq!(buffer.byte_to_line_col(4), (2, 1));
        assert_eq!(buffer.byte_to_line_col(8), (3, 1));
    }

    #[test]
    fn editor_buffer_converts_line_col_to_byte() {
        let buffer = EditorBuffer::from_text("one\ntwö\nthree".to_string());

        assert_eq!(buffer.line_col_to_byte(1, 2), 1);
        assert_eq!(buffer.line_col_to_byte(2, 1), 4);
        assert_eq!(buffer.line_col_to_byte(2, 3), 6);
        assert_eq!(buffer.line_col_to_byte(99, 99), buffer.as_str().len());
    }

    #[test]
    fn editor_buffer_returns_lines_without_newlines() {
        let buffer = EditorBuffer::from_text("one\ntwo\nthree".to_string());

        assert_eq!(buffer.line(0), "one");
        assert_eq!(buffer.line(1), "two");
        assert_eq!(buffer.line(2), "three");
    }

    #[test]
    fn editor_buffer_tracks_revision_on_external_edit() {
        let mut buffer = EditorBuffer::new();
        let revision = buffer.revision;

        buffer.text_mut().push_str("hello");
        buffer.mark_external_edit();

        assert!(buffer.revision > revision);
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.as_str(), "hello");
    }

    #[test]
    fn editor_buffer_inserts_text_and_newlines() {
        let mut buffer = EditorBuffer::new();

        buffer.insert_text("hello");
        buffer.insert_newline();
        buffer.insert_text("world");

        assert_eq!(buffer.as_str(), "hello\nworld");
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.cursor(), buffer.as_str().len());
    }

    #[test]
    fn editor_buffer_deletes_unicode_safely() {
        let mut buffer = EditorBuffer::from_text("aé日b".to_string());
        buffer.set_cursor("aé日".len());

        buffer.backspace();
        assert_eq!(buffer.as_str(), "aéb");
        assert_eq!(buffer.cursor(), "aé".len());

        buffer.delete();
        assert_eq!(buffer.as_str(), "aé");
    }

    #[test]
    fn editor_buffer_replaces_selection() {
        let mut buffer = EditorBuffer::from_text("hello world".to_string());
        buffer.set_selection(6, 11);

        buffer.insert_text("slate");

        assert_eq!(buffer.as_str(), "hello slate");
        assert_eq!(buffer.selection(), None);
        assert_eq!(buffer.cursor(), "hello slate".len());
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

impl Command {
    fn label(self) -> &'static str {
        match self {
            Command::New => "New buffer",
            Command::Open => "Open file",
            Command::Save => "Save",
            Command::TogglePreview => "Toggle Markdown preview",
            Command::ToggleWrap => "Toggle word wrap",
            Command::Settings => "Settings",
            Command::Quit => "Quit",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Command::New => "Ctrl+N",
            Command::Open => "Ctrl+O",
            Command::Save => "Ctrl+S",
            Command::TogglePreview => "Ctrl+M",
            Command::ToggleWrap => "",
            Command::Settings => ":settings",
            Command::Quit => "Ctrl+Q",
        }
    }
}

struct SlateApp {
    buffer: EditorBuffer,
    editor_view: EditorView,
    path: Option<PathBuf>,
    dirty: bool,
    status: String,
    palette_open: bool,
    palette_query: String,
    selected_command: usize,
    preview: bool,
    wrap: bool,
    focus_editor_once: bool,
    scratch: bool,
    pending_action: Option<PendingAction>,
    settings_open: bool,
    command_line: String,
    command_line_focused: bool,
    focus_command_line_once: bool,
    command_history: Vec<String>,
    command_history_index: Option<usize>,
    command_history_limit: usize,
}

impl SlateApp {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>, scratch: bool) -> Self {
        setup_style(&cc.egui_ctx);

        let mut app = Self {
            buffer: EditorBuffer::new(),
            editor_view: EditorView::new(),
            path: None,
            dirty: false,
            status: "Ready".to_string(),
            palette_open: false,
            palette_query: String::new(),
            selected_command: 0,
            preview: false,
            wrap: true,
            focus_editor_once: true,
            scratch,
            pending_action: None,
            settings_open: false,
            command_line: String::new(),
            command_line_focused: false,
            focus_command_line_once: false,
            command_history: Vec::new(),
            command_history_index: None,
            command_history_limit: 5,
        };

        app.load_settings();

        if let Some(path) = path {
            app.open_path(path);
        }

        app
    }

    fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");
        if self.scratch && self.path.is_none() {
            format!("{}Slate Scratch", if self.dirty { "*" } else { "" })
        } else {
            format!("{}{} — Slate", if self.dirty { "*" } else { "" }, name)
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.buffer.set_text(text);
                self.path = Some(path.clone());
                self.dirty = false;
                self.status = format!("Opened {}", path.display());
            }
            Err(err) => self.status = format!("Open failed: {err}"),
        }
    }

    fn save(&mut self) {
        if let Some(path) = self.path.clone() {
            self.save_path(path);
        } else {
            self.save_as();
        }
    }

    fn append_to_scratch_archive(&mut self) {
        if !self.scratch
            || self.path.is_some()
            || !self.dirty
            || self.buffer.as_str().trim().is_empty()
        {
            return;
        }

        let Some(mut dir) = dirs_next::data_dir() else {
            self.status = "Scratch append failed: no data dir".to_string();
            return;
        };
        dir.push("slate");

        if let Err(err) = fs::create_dir_all(&dir) {
            self.status = format!("Scratch append failed: {err}");
            return;
        }

        let path = dir.join("scratch.md");
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let needs_header =
            !path.exists() || fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);
        let entry = if needs_header {
            format!(
                "# Scratch\n\n## {now}\n\n{}\n",
                self.buffer.as_str().trim_end()
            )
        } else {
            format!("\n\n## {now}\n\n{}\n", self.buffer.as_str().trim_end())
        };

        match fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => match file.write_all(entry.as_bytes()) {
                Ok(_) => {
                    self.dirty = false;
                    self.status = format!("Appended to {}", path.display());
                }
                Err(err) => self.status = format!("Scratch append failed: {err}"),
            },
            Err(err) => self.status = format!("Scratch append failed: {err}"),
        }
    }

    fn save_path(&mut self, path: PathBuf) {
        match fs::write(&path, self.buffer.as_str()) {
            Ok(_) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.status = format!("Saved {}", path.display());
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn new_buffer(&mut self) {
        self.buffer.clear();
        self.path = None;
        self.dirty = false;
        self.status = "New buffer".to_string();
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.open_path(path);
        }
    }

    fn save_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new().save_file() {
            self.save_path(path);
        }
    }

    fn settings_path() -> Option<PathBuf> {
        let mut dir = dirs_next::config_dir()?;
        dir.push("slate");
        Some(dir.join("config.toml"))
    }

    fn load_settings(&mut self) {
        let Some(path) = Self::settings_path() else {
            return;
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };

        for line in contents.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == "command_history_limit" {
                if let Ok(limit) = value.trim().parse::<usize>() {
                    self.command_history_limit = limit.clamp(1, 50);
                }
            }
        }
    }

    fn save_settings(&self) -> Result<(), String> {
        let Some(path) = Self::settings_path() else {
            return Err("no config dir".to_string());
        };
        let parent = path
            .parent()
            .ok_or_else(|| "invalid config path".to_string())?;
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        fs::write(
            path,
            format!("command_history_limit = {}\n", self.command_history_limit),
        )
        .map_err(|err| err.to_string())
    }

    fn set_command_history_limit(&mut self, limit: usize) {
        self.command_history_limit = limit.clamp(1, 50);
        match self.save_settings() {
            Ok(_) => self.status = format!("History length: {}", self.command_history_limit),
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn run_command(&mut self, command: Command, ctx: &egui::Context) {
        self.palette_open = false;
        self.palette_query.clear();
        self.selected_command = 0;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.focus_editor_once = true;

        match command {
            Command::New => {
                if self.dirty {
                    self.confirm(PendingAction::New);
                } else {
                    self.new_buffer();
                }
            }
            Command::Open => {
                if self.dirty {
                    self.confirm(PendingAction::Open);
                } else {
                    self.open_dialog();
                }
            }
            Command::Save => self.save(),
            Command::TogglePreview => {
                self.preview = !self.preview;
                self.status = if self.preview {
                    "Preview on"
                } else {
                    "Preview off"
                }
                .to_string();
            }
            Command::ToggleWrap => {
                self.wrap = !self.wrap;
                self.status = if self.wrap {
                    "Word wrap on"
                } else {
                    "Word wrap off"
                }
                .to_string();
            }
            Command::Settings => {
                self.settings_open = true;
                self.focus_editor_once = false;
            }
            Command::Quit => self.request_close(ctx),
        }
    }

    fn run_command_line(&mut self, ctx: &egui::Context) {
        let raw = self.command_line.trim().to_string();
        self.command_line.clear();
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.focus_editor_once = true;
        self.command_history_index = None;

        let input = raw.strip_prefix(':').unwrap_or(&raw).trim();
        if input.is_empty() {
            self.status = "Command cancelled".to_string();
            return;
        }

        if self.command_history.last().is_none_or(|last| last != input) {
            self.command_history.push(input.to_string());
        }

        let mut parts = input.split_whitespace();
        let Some(command) = parts.next() else {
            self.status = "Command cancelled".to_string();
            return;
        };

        match command {
            "w" | "write" | "save" => self.run_command(Command::Save, ctx),
            "q" | "quit" | "exit" => self.run_command(Command::Quit, ctx),
            "wq" | "x" => {
                self.save();
                if !self.dirty {
                    self.run_command(Command::Quit, ctx);
                }
            }
            "new" | "enew" => self.run_command(Command::New, ctx),
            "open" | "edit" | "e" => {
                let path = parts.collect::<Vec<_>>().join(" ");
                if path.is_empty() {
                    self.run_command(Command::Open, ctx);
                } else if self.dirty {
                    self.status = "Save or discard changes before opening another file".to_string();
                } else {
                    let expanded = path
                        .strip_prefix("~/")
                        .and_then(|rest| dirs_next::home_dir().map(|home| home.join(rest)))
                        .unwrap_or_else(|| PathBuf::from(path));
                    self.open_path(expanded);
                }
            }
            "preview" | "md" => self.run_command(Command::TogglePreview, ctx),
            "wrap" => self.run_command(Command::ToggleWrap, ctx),
            "settings" | "set" | "prefs" | "preferences" => {
                self.run_command(Command::Settings, ctx)
            }
            _ => self.status = format!("Unknown command: {input}"),
        }
    }

    fn confirm(&mut self, action: PendingAction) {
        self.pending_action = Some(action);
        self.focus_editor_once = false;
    }

    fn finish_pending_action(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::New => self.new_buffer(),
            PendingAction::Open => self.open_dialog(),
            PendingAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn request_close(&mut self, ctx: &egui::Context) {
        if self.dirty && !self.scratch {
            self.confirm(PendingAction::Quit);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.scratch {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm(PendingAction::Quit);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let mut command = None;
        let mut execute_command_line = false;
        let mut previous_command = false;
        let mut next_command = false;
        let mut settings_decrement = false;
        let mut settings_increment = false;
        ctx.input_mut(|i| {
            if self.settings_open {
                settings_decrement |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                settings_increment |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::P) {
                self.palette_open = true;
                self.palette_query.clear();
                self.selected_command = 0;
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::Period) {
                self.palette_open = false;
                self.command_line.clear();
                self.command_history_index = None;
                self.command_line_focused = true;
                self.focus_command_line_once = true;
                self.focus_editor_once = false;
            }
            if self.command_line_focused || self.focus_command_line_once {
                execute_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                execute_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                previous_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                next_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::N) {
                command = Some(Command::New);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::O) {
                command = Some(Command::Open);
            }
            let save_pressed = i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: Key::S,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.ctrl && !modifiers.alt && !modifiers.shift
                )
            });
            if save_pressed {
                command = Some(Command::Save);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::M) {
                command = Some(Command::TogglePreview);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::Q) {
                command = Some(Command::Quit);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                if self.settings_open {
                    self.settings_open = false;
                    self.focus_editor_once = true;
                } else if self.command_line_focused || self.focus_command_line_once {
                    self.command_line.clear();
                    self.command_line_focused = false;
                    self.focus_command_line_once = false;
                    self.command_history_index = None;
                    self.focus_editor_once = true;
                } else if self.palette_open {
                    self.palette_open = false;
                    self.focus_editor_once = true;
                } else if self.pending_action.is_some() {
                    self.pending_action = None;
                    self.focus_editor_once = true;
                } else if self.scratch {
                    command = Some(Command::Quit);
                }
            }
        });

        if settings_decrement {
            self.set_command_history_limit(self.command_history_limit.saturating_sub(1));
            return;
        }

        if settings_increment {
            self.set_command_history_limit(self.command_history_limit + 1);
            return;
        }

        if previous_command && !self.command_history.is_empty() {
            let index = self
                .command_history_index
                .unwrap_or(self.command_history.len())
                .saturating_sub(1);
            self.command_history_index = Some(index);
            self.command_line = self.command_history[index].clone();
            self.focus_command_line_once = true;
            return;
        }

        if next_command {
            if let Some(index) = self.command_history_index {
                if index + 1 < self.command_history.len() {
                    let index = index + 1;
                    self.command_history_index = Some(index);
                    self.command_line = self.command_history[index].clone();
                } else {
                    self.command_history_index = None;
                    self.command_line.clear();
                }
                self.focus_command_line_once = true;
            }
            return;
        }

        if execute_command_line {
            self.run_command_line(ctx);
            return;
        }

        if let Some(command) = command {
            self.run_command(command, ctx);
        }
    }

    fn filtered_commands(&self) -> Vec<Command> {
        let all = [
            Command::New,
            Command::Open,
            Command::Save,
            Command::TogglePreview,
            Command::ToggleWrap,
            Command::Settings,
            Command::Quit,
        ];
        let q = self.palette_query.to_lowercase();
        all.into_iter()
            .filter(|c| c.label().to_lowercase().contains(&q))
            .collect()
    }

    fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }

        let commands = self.filtered_commands();
        if self.selected_command >= commands.len() {
            self.selected_command = commands.len().saturating_sub(1);
        }

        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                self.selected_command =
                    (self.selected_command + 1).min(commands.len().saturating_sub(1));
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                self.selected_command = self.selected_command.saturating_sub(1);
            }
        });

        let frame = egui::Frame::new()
            .fill(Color32::from_rgb(25, 31, 40))
            .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
            .corner_radius(0.0)
            .inner_margin(14.0)
            .shadow(egui::epaint::Shadow {
                offset: [0, 10],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(140),
            });

        egui::Area::new("command_palette".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(
                        RichText::new("command palette")
                            .font(FontId::new(16.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(136, 192, 208)),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("slate:~$")
                                .font(FontId::new(15.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(163, 190, 140)),
                        );
                        let response = ui.add(
                            TextEdit::singleline(&mut self.palette_query)
                                .hint_text("type a command")
                                .desired_width(f32::INFINITY)
                                .font(FontId::new(15.0, FontFamily::Monospace))
                                .text_color(Color32::from_rgb(216, 222, 233))
                                .frame(egui::Frame::NONE),
                        );
                        response.request_focus();
                    });
                    ui.add_space(8.0);
                    ui.painter().hline(
                        ui.available_rect_before_wrap().x_range(),
                        ui.cursor().top(),
                        Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                    );
                    ui.add_space(8.0);

                    if commands.is_empty() {
                        ui.label(
                            RichText::new("no matching commands")
                                .font(FontId::new(14.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(94, 105, 126)),
                        );
                    }

                    for (idx, command) in commands.iter().enumerate() {
                        let selected = idx == self.selected_command;
                        let fill = if selected {
                            Color32::from_rgb(46, 56, 72)
                        } else {
                            Color32::TRANSPARENT
                        };
                        let label_color = if selected {
                            Color32::from_rgb(236, 239, 244)
                        } else {
                            Color32::from_rgb(216, 222, 233)
                        };
                        let row = egui::Frame::new()
                            .fill(fill)
                            .corner_radius(0.0)
                            .inner_margin(6.0);
                        let clicked = row
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(if selected { ">" } else { " " })
                                            .font(FontId::new(14.0, FontFamily::Monospace))
                                            .color(Color32::from_rgb(136, 192, 208)),
                                    );
                                    ui.label(
                                        RichText::new(command.label())
                                            .font(FontId::new(14.0, FontFamily::Monospace))
                                            .color(label_color),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(command.hint())
                                                    .font(FontId::new(13.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 154, 176)),
                                            );
                                        },
                                    );
                                })
                            })
                            .response
                            .clicked();
                        if clicked {
                            self.run_command(*command, ctx);
                            return;
                        }
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        for (key, label) in [("↑↓", "move"), ("enter", "run"), ("esc", "close")]
                        {
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

                    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
                    if enter {
                        if let Some(command) = commands.get(self.selected_command).copied() {
                            self.run_command(command, ctx);
                        }
                    }
                });
            });
    }

    fn confirm_action_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_action else {
            return;
        };

        let mut discard = false;
        let mut go_back = false;
        let mut save = false;
        ctx.input_mut(|i| {
            discard |= i.consume_key(egui::Modifiers::NONE, Key::Y);
            go_back |= i.consume_key(egui::Modifiers::NONE, Key::N);
            save |= i.consume_key(egui::Modifiers::NONE, Key::S);
            go_back |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
        });

        if discard {
            self.dirty = false;
            self.pending_action = None;
            self.finish_pending_action(action, ctx);
            return;
        }
        if go_back {
            self.pending_action = None;
            self.focus_editor_once = true;
            return;
        }
        if save {
            self.save();
            if !self.dirty {
                self.pending_action = None;
                self.finish_pending_action(action, ctx);
            }
            return;
        }

        egui::Area::new("confirm_close_prompt".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    })
                    .show(ui, |ui| {
                        ui.set_width(520.0);
                        ui.label(
                            RichText::new("unsaved changes")
                                .font(FontId::new(16.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(136, 192, 208)),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(action.prompt())
                                .font(FontId::new(14.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(216, 222, 233)),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            for (key, label, color) in [
                                ("y", "yes / discard", Color32::from_rgb(191, 97, 106)),
                                ("n", "no / return", Color32::from_rgb(163, 190, 140)),
                                ("s", "save…", Color32::from_rgb(235, 203, 139)),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(FontId::new(14.0, FontFamily::Monospace))
                                        .color(color),
                                );
                                ui.label(
                                    RichText::new(label)
                                        .font(FontId::new(14.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(136, 154, 176)),
                                );
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        egui::Area::new("settings_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -30.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    })
                    .show(ui, |ui| {
                        ui.set_width(520.0);
                        ui.label(
                            RichText::new("settings")
                                .font(FontId::new(16.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(136, 192, 208)),
                        );
                        ui.add_space(10.0);

                        egui::Frame::new()
                            .fill(Color32::from_rgb(30, 36, 48))
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
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

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [("←→", "adjust"), ("esc", "close")] {
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
                    });
            });
    }

    fn preview_ui(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut in_code = false;
            for line in self.buffer.as_str().lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("```") {
                    in_code = !in_code;
                    continue;
                }

                if in_code {
                    ui.label(
                        RichText::new(line)
                            .font(FontId::new(14.0, FontFamily::Monospace))
                            .background_color(Color32::from_rgb(25, 31, 40)),
                    );
                } else if let Some(h) = trimmed.strip_prefix("### ") {
                    ui.label(RichText::new(h).size(18.0).strong());
                } else if let Some(h) = trimmed.strip_prefix("## ") {
                    ui.label(RichText::new(h).size(22.0).strong());
                } else if let Some(h) = trimmed.strip_prefix("# ") {
                    ui.label(RichText::new(h).size(28.0).strong());
                } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                    ui.label(format!("• {}", &trimmed[2..]));
                } else if trimmed.is_empty() {
                    ui.add_space(8.0);
                } else {
                    ui.label(RichText::new(line).size(15.0));
                }
            }
        });
    }
}

impl eframe::App for SlateApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.append_to_scratch_archive();
        let _ = self.save_settings();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));
        self.handle_window_close_request(&ctx);
        self.shortcuts(&ctx);

        if self.pending_action.is_some() {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(Color32::from_rgb(30, 36, 48)))
                .show_inside(ui, |_ui| {});
            self.confirm_action_dialog(&ctx);
            return;
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(30, 36, 48))
                    .inner_margin(0.0),
            )
            .show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let footer_font = FontId::new(13.0, FontFamily::Monospace);
                let footer_color = Color32::from_rgb(136, 154, 176);
                let footer_dim = Color32::from_rgb(94, 105, 126);
                let footer_accent = Color32::from_rgb(136, 192, 208);
                let footer_ok = Color32::from_rgb(163, 190, 140);
                let footer_warn = Color32::from_rgb(235, 203, 139);
                let status_height = 30.0;
                let command_height = 30.0;
                let history_row_height = 22.0;
                let command_history_active = (self.command_line_focused
                    || self.focus_command_line_once)
                    && !self.command_history.is_empty();
                let visible_history_rows = if command_history_active {
                    self.command_history.len().min(self.command_history_limit)
                } else {
                    0
                };
                let history_height = visible_history_rows as f32 * history_row_height;
                let footer_height = status_height + history_height + command_height;
                let editor_size = Vec2::new(
                    ui.available_width(),
                    (ui.available_height() - footer_height).max(80.0),
                );

                self.editor_view.observe_buffer(&self.buffer);

                ui.allocate_ui_with_layout(
                    editor_size,
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if self.preview {
                            ui.columns(2, |columns| {
                                let (response, changed) = self.editor_view.render(
                                    &mut columns[0],
                                    &mut self.buffer,
                                    self.wrap,
                                );
                                if self.focus_editor_once
                                    && !self.palette_open
                                    && !self.settings_open
                                    && !self.command_line_focused
                                {
                                    response.request_focus();
                                    self.focus_editor_once = false;
                                }
                                if changed {
                                    self.dirty = true;
                                }
                                columns[1].vertical(|ui| self.preview_ui(ui));
                            });
                        } else {
                            let (response, changed) =
                                self.editor_view.render(ui, &mut self.buffer, self.wrap);
                            if self.focus_editor_once
                                && !self.palette_open
                                && !self.settings_open
                                && !self.command_line_focused
                            {
                                response.request_focus();
                                self.focus_editor_once = false;
                            }
                            if changed {
                                self.dirty = true;
                            }
                        }
                    },
                );

                let filename = self
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "untitled".to_string());
                let dirty_label = if self.dirty { "modified" } else { "saved" };
                let dirty_color = if self.dirty { footer_warn } else { footer_ok };
                let lines = self.buffer.line_count();
                let chars = self.buffer.as_str().chars().count();
                let words = self.buffer.as_str().split_whitespace().count();
                let mode = if self.preview { "preview" } else { "edit" };
                let wrap = if self.wrap { "wrap" } else { "nowrap" };

                let (status_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), status_height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(status_rect);
                painter.rect_filled(status_rect, 0.0, Color32::from_rgb(25, 31, 40));

                // Raw-painted monospace text needs only a tiny optical correction here.
                let status_y = status_rect.center().y - 0.5;
                let mut status_x = status_rect.left() + 10.0;
                for (text, color) in [
                    ("slate".to_string(), footer_accent),
                    ("::".to_string(), footer_dim),
                    (filename, footer_color),
                    (format!("[{dirty_label}]"), dirty_color),
                    (format!("— {}", self.status), footer_dim),
                ] {
                    let text_rect = painter.text(
                        egui::pos2(status_x, status_y),
                        egui::Align2::LEFT_CENTER,
                        text,
                        footer_font.clone(),
                        color,
                    );
                    status_x = text_rect.right() + 8.0;
                }

                let mut status_right = status_rect.right() - 10.0;
                let shortcut_rect = painter.text(
                    egui::pos2(status_right, status_y),
                    egui::Align2::RIGHT_CENTER,
                    "[Ctrl+P]",
                    footer_font.clone(),
                    footer_accent,
                );
                status_right = shortcut_rect.left() - 12.0;
                painter.text(
                    egui::pos2(status_right, status_y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{mode} · {wrap} · {lines}l · {words}w · {chars}c"),
                    footer_font.clone(),
                    footer_dim,
                );

                if visible_history_rows > 0 {
                    let (history_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), history_height),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(history_rect);
                    painter.rect_filled(history_rect, 0.0, Color32::from_rgb(25, 31, 40));

                    let len = self.command_history.len();
                    let rows = visible_history_rows;
                    let selected_index = self.command_history_index;
                    let start = selected_index
                        .map(|idx| idx.min(len.saturating_sub(rows)))
                        .unwrap_or_else(|| len.saturating_sub(rows));
                    let end = (start + rows).min(len);

                    for (row, index) in (start..end).enumerate() {
                        let row_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                history_rect.left(),
                                history_rect.top() + row as f32 * history_row_height,
                            ),
                            Vec2::new(history_rect.width(), history_row_height),
                        );
                        let selected = selected_index == Some(index);
                        if selected {
                            painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                        }

                        let marker = if selected { ">" } else { " " };
                        painter.text(
                            egui::pos2(row_rect.left() + 10.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            marker,
                            footer_font.clone(),
                            footer_accent,
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 28.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            &self.command_history[index],
                            footer_font.clone(),
                            if selected { footer_color } else { footer_dim },
                        );

                        let response = ui.interact(
                            row_rect,
                            ui.id().with(("command_history", index)),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            self.command_history_index = Some(index);
                            self.command_line = self.command_history[index].clone();
                            self.focus_command_line_once = true;
                        }
                    }
                }

                let (command_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), command_height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(command_rect);
                painter.rect_filled(command_rect, 0.0, Color32::from_rgb(25, 31, 40));
                let command_y = command_rect.center().y - 2.0;
                painter.text(
                    egui::pos2(command_rect.left() + 10.0, command_y),
                    egui::Align2::LEFT_CENTER,
                    ":",
                    footer_font.clone(),
                    footer_accent,
                );

                let input_rect = egui::Rect::from_min_max(
                    egui::pos2(command_rect.left() + 19.0, command_rect.top() + 4.0),
                    egui::pos2(command_rect.right() - 10.0, command_rect.bottom() - 4.0),
                );
                let command_line_active = self.command_line_focused || self.focus_command_line_once;
                if command_line_active {
                    let response = ui.put(
                        input_rect,
                        TextEdit::singleline(&mut self.command_line)
                            .hint_text(
                                RichText::new("command  w · q · wq · open <file> · preview · wrap")
                                    .font(footer_font.clone())
                                    .color(footer_dim),
                            )
                            .desired_width(f32::INFINITY)
                            .font(footer_font.clone())
                            .text_color(footer_color)
                            .frame(egui::Frame::NONE),
                    );
                    if self.focus_command_line_once {
                        response.request_focus();
                        self.focus_command_line_once = false;
                    }
                    self.command_line_focused = response.has_focus();
                } else {
                    painter.text(
                        egui::pos2(input_rect.left(), input_rect.center().y - 0.5),
                        egui::Align2::LEFT_CENTER,
                        "command  w · q · wq · open <file> · preview · wrap",
                        footer_font.clone(),
                        footer_dim,
                    );
                    self.command_line_focused = false;
                }
            });

        self.command_palette(&ctx);
        self.settings_dialog(&ctx);
    }
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Ok(bytes) = fs::read("/usr/share/fonts/noto/NotoSansMono-Regular.ttf") {
        fonts.font_data.insert(
            "noto_mono".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "noto_mono".to_string());
    }

    if let Ok(bytes) = fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf") {
        fonts.font_data.insert(
            "noto_sans".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "noto_sans".to_string());
    }

    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = Color32::from_rgb(30, 36, 48);
    style.visuals.panel_fill = Color32::from_rgb(30, 36, 48);
    style.visuals.extreme_bg_color = Color32::from_rgb(25, 31, 40);
    style.visuals.faint_bg_color = Color32::from_rgb(38, 47, 61);
    style.visuals.selection.bg_fill = Color32::from_rgb(67, 76, 94);
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(136, 192, 208));
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 41, 54);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 56, 72);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(59, 66, 82);
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    ctx.set_global_style(style);
}
