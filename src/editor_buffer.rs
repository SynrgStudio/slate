pub(crate) struct EditorBuffer {
    text: String,
    line_starts: Vec<usize>,
    cursor: usize,
    selection: Option<(usize, usize)>,
    pub(crate) revision: u64,
}

impl EditorBuffer {
    pub(crate) fn new() -> Self {
        Self::from_text(String::new())
    }

    pub(crate) fn from_text(text: String) -> Self {
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

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    #[allow(dead_code)]
    pub(crate) fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }

    pub(crate) fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.cursor.min(self.text.len());
        self.selection = None;
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
    }

    pub(crate) fn clear(&mut self) {
        self.set_text(String::new());
    }

    #[allow(dead_code)]
    pub(crate) fn mark_external_edit(&mut self) {
        self.cursor = self.cursor.min(self.text.len());
        self.selection = None;
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
    }

    pub(crate) fn line_count(&self) -> usize {
        self.line_starts.len().max(1)
    }

    #[allow(dead_code)]
    pub(crate) fn line(&self, line_index: usize) -> &str {
        let start = self.line_start(line_index);
        let end = self.line_end(line_index);
        &self.text[start..end]
    }

    pub(crate) fn line_start(&self, line_index: usize) -> usize {
        self.line_starts.get(line_index).copied().unwrap_or(0)
    }

    pub(crate) fn line_end(&self, line_index: usize) -> usize {
        self.line_starts
            .get(line_index + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(self.text.len())
    }

    pub(crate) fn line_index_for_byte(&self, byte: usize) -> usize {
        let byte = byte.min(self.text.len());
        match self.line_starts.binary_search(&byte) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        }
    }

    pub(crate) fn cursor_line_col(&self) -> (usize, usize) {
        let line_index = self.line_index_for_byte(self.cursor);
        let column = self.text[self.line_start(line_index)..self.cursor]
            .chars()
            .count();
        (line_index, column)
    }

    #[allow(dead_code)]
    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    #[allow(dead_code)]
    pub(crate) fn selection(&self) -> Option<(usize, usize)> {
        self.selection
    }

    #[allow(dead_code)]
    pub(crate) fn set_cursor(&mut self, byte: usize) {
        self.cursor = self.clamp_to_char_boundary(byte);
        self.selection = None;
    }

    #[allow(dead_code)]
    pub(crate) fn set_selection(&mut self, start: usize, end: usize) {
        let start = self.clamp_to_char_boundary(start);
        let end = self.clamp_to_char_boundary(end);
        self.selection = (start != end).then_some((start.min(end), start.max(end)));
        self.cursor = end;
    }

    #[allow(dead_code)]
    pub(crate) fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub(crate) fn move_left(&mut self) {
        if let Some((start, _)) = self.selection.take() {
            self.cursor = start;
        } else {
            self.cursor = self.previous_char_boundary(self.cursor);
        }
    }

    pub(crate) fn move_right(&mut self) {
        if let Some((_, end)) = self.selection.take() {
            self.cursor = end;
        } else {
            self.cursor = self.next_char_boundary(self.cursor);
        }
    }

    pub(crate) fn move_to_line_start(&mut self) {
        let line_index = self.line_index_for_byte(self.cursor);
        self.set_cursor(self.line_start(line_index));
    }

    pub(crate) fn move_to_line_end(&mut self) {
        let line_index = self.line_index_for_byte(self.cursor);
        self.set_cursor(self.line_end(line_index));
    }

    pub(crate) fn move_up(&mut self) {
        let (line_index, column) = self.cursor_line_col();
        if line_index == 0 {
            self.move_to_line_start();
            return;
        }
        let byte = self.line_col_to_byte(line_index, column + 1);
        self.set_cursor(byte);
    }

    pub(crate) fn move_down(&mut self) {
        let (line_index, column) = self.cursor_line_col();
        if line_index + 1 >= self.line_count() {
            self.move_to_line_end();
            return;
        }
        let byte = self.line_col_to_byte(line_index + 2, column + 1);
        self.set_cursor(byte);
    }

    #[allow(dead_code)]
    pub(crate) fn insert_text(&mut self, text: &str) {
        self.replace_selection_or_range(self.cursor, self.cursor, text);
    }

    #[allow(dead_code)]
    pub(crate) fn insert_newline(&mut self) {
        self.insert_text("\n");
    }

    #[allow(dead_code)]
    pub(crate) fn backspace(&mut self) {
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
    pub(crate) fn delete(&mut self) {
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

    pub(crate) fn delete_current_line(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }

        if self.line_count() == 1 {
            self.clear();
            return true;
        }

        let line_index = self.line_index_for_byte(self.cursor);
        let (start, end) = if line_index + 1 >= self.line_count() {
            (
                self.line_start(line_index).saturating_sub(1),
                self.text.len(),
            )
        } else {
            (self.line_start(line_index), self.line_start(line_index + 1))
        };

        self.selection = None;
        self.text.replace_range(start..end, "");
        self.cursor = self.clamp_to_char_boundary(start.min(self.text.len()));
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
        true
    }

    pub(crate) fn replace_selection_or_range(
        &mut self,
        start: usize,
        end: usize,
        replacement: &str,
    ) {
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

    pub(crate) fn clamp_to_char_boundary(&self, byte: usize) -> usize {
        let mut byte = byte.min(self.text.len());
        while byte > 0 && !self.text.is_char_boundary(byte) {
            byte -= 1;
        }
        byte
    }

    pub(crate) fn previous_char_boundary(&self, byte: usize) -> usize {
        let mut byte = self.clamp_to_char_boundary(byte).saturating_sub(1);
        while byte > 0 && !self.text.is_char_boundary(byte) {
            byte -= 1;
        }
        byte
    }

    pub(crate) fn next_char_boundary(&self, byte: usize) -> usize {
        let mut byte = (self.clamp_to_char_boundary(byte) + 1).min(self.text.len());
        while byte < self.text.len() && !self.text.is_char_boundary(byte) {
            byte += 1;
        }
        byte
    }

    #[allow(dead_code)]
    pub(crate) fn byte_to_line_col(&self, byte: usize) -> (usize, usize) {
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
    pub(crate) fn line_col_to_byte(&self, line: usize, column: usize) -> usize {
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

    pub(crate) fn rebuild_line_index(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        for (index, byte) in self.text.bytes().enumerate() {
            if byte == b'\n' {
                self.line_starts.push(index + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EditorBuffer;

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
    fn editor_buffer_deletes_current_middle_line() {
        let mut buffer = EditorBuffer::from_text("one\ntwo\nthree".to_string());
        buffer.set_cursor(5);

        assert!(buffer.delete_current_line());

        assert_eq!(buffer.as_str(), "one\nthree");
        assert_eq!(buffer.cursor(), 4);
    }

    #[test]
    fn editor_buffer_deletes_current_last_line_without_leaving_trailing_blank() {
        let mut buffer = EditorBuffer::from_text("one\ntwo\nthree".to_string());
        buffer.set_cursor(buffer.as_str().len());

        assert!(buffer.delete_current_line());

        assert_eq!(buffer.as_str(), "one\ntwo");
    }
}
