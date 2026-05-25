use crate::{editor_buffer::EditorBuffer, search::SearchState};

use eframe::egui::{self, Color32, FontFamily, FontId, Key, Stroke};

#[derive(Clone, Copy)]
pub(crate) struct VisualRow {
    line_index: usize,
    start: usize,
    end: usize,
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

        if response.has_focus() && search_state.is_none() {
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

#[cfg(test)]
mod tests {
    use super::EditorView;
    use crate::editor_buffer::EditorBuffer;

    #[test]
    fn editor_view_calculates_visible_line_range() {
        let buffer = EditorBuffer::from_text("1\n2\n3\n4\n5".to_string());
        let mut view = EditorView::new();

        assert_eq!(view.visible_line_range(&buffer, 20.0, 10.0), 0..3);

        view.scroll_y = 20.0;
        assert_eq!(view.visible_line_range(&buffer, 20.0, 10.0), 2..5);
    }
}
