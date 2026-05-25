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

    pub(crate) fn move_to_top(&mut self) {
        self.set_cursor(0);
    }

    pub(crate) fn move_to_bottom(&mut self) {
        self.set_cursor(self.text.len());
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

    pub(crate) fn move_current_line_up(&mut self) -> bool {
        let line_index = self.line_index_for_byte(self.cursor);
        if line_index == 0 {
            return false;
        }
        self.move_current_line_to(line_index - 1)
    }

    pub(crate) fn move_current_line_down(&mut self) -> bool {
        let line_index = self.line_index_for_byte(self.cursor);
        if line_index + 1 >= self.line_count() {
            return false;
        }
        self.move_current_line_to(line_index + 1)
    }

    pub(crate) fn move_current_line_to_paragraph_start(&mut self) -> bool {
        let line_index = self.line_index_for_byte(self.cursor);
        if self.line(line_index).trim().is_empty() {
            return false;
        }

        let mut target = line_index;
        while target > 0 && !self.line(target - 1).trim().is_empty() {
            target -= 1;
        }
        self.move_current_line_to(target)
    }

    pub(crate) fn move_current_line_to_paragraph_end(&mut self) -> bool {
        let line_index = self.line_index_for_byte(self.cursor);
        if self.line(line_index).trim().is_empty() {
            return false;
        }

        let mut target = line_index;
        while target + 1 < self.line_count() && !self.line(target + 1).trim().is_empty() {
            target += 1;
        }
        self.move_current_line_to(target)
    }

    pub(crate) fn select_current_line(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }

        let line_index = self.line_index_for_byte(self.cursor);
        self.set_selection(self.line_start(line_index), self.line_end(line_index));
        true
    }

    pub(crate) fn select_word(&mut self) -> bool {
        let Some((start, end)) = self.word_range_at_cursor() else {
            return false;
        };
        self.set_selection(start, end);
        true
    }

    pub(crate) fn select_word_left_extend(&mut self) -> bool {
        if let Some((selection_start, selection_end)) = self.selection {
            if self.cursor == selection_end {
                let Some((removed_start, _)) = self.word_before(selection_end) else {
                    return false;
                };
                let new_end = self.trim_non_word_left(removed_start);
                if new_end <= selection_start {
                    self.selection = None;
                    self.cursor = selection_start;
                } else {
                    self.selection = Some((selection_start, new_end));
                    self.cursor = new_end;
                }
                return true;
            }

            let Some((start, _)) = self.word_before(selection_start) else {
                return false;
            };
            self.selection = Some((start, selection_end));
            self.cursor = start;
            return true;
        }

        let Some((start, end)) = self.word_at_or_before(self.cursor) else {
            return false;
        };
        self.selection = Some((start, end));
        self.cursor = start;
        true
    }

    pub(crate) fn select_word_right_extend(&mut self) -> bool {
        if let Some((selection_start, selection_end)) = self.selection {
            if self.cursor == selection_start {
                let Some((_, removed_end)) = self.word_at_or_after(selection_start) else {
                    return false;
                };
                let new_start = self.trim_non_word_right(removed_end);
                if new_start >= selection_end {
                    self.selection = None;
                    self.cursor = selection_end;
                } else {
                    self.selection = Some((new_start, selection_end));
                    self.cursor = new_start;
                }
                return true;
            }

            let Some((_, end)) = self.word_after(selection_end) else {
                return false;
            };
            self.selection = Some((selection_start, end));
            self.cursor = end;
            return true;
        }

        let Some((start, end)) = self.word_at_or_after(self.cursor) else {
            return false;
        };
        self.selection = Some((start, end));
        self.cursor = end;
        true
    }

    pub(crate) fn delete_word(&mut self) -> bool {
        let Some((start, end)) = self.word_range_at_cursor() else {
            return false;
        };
        self.replace_selection_or_range(start, end, "");
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

    fn move_current_line_to(&mut self, target_index: usize) -> bool {
        if self.text.is_empty() {
            return false;
        }

        let line_index = self.line_index_for_byte(self.cursor);
        let target_index = target_index.min(self.line_count().saturating_sub(1));
        if line_index == target_index {
            return false;
        }

        let (_, column) = self.cursor_line_col();
        let mut lines = self.lines_with_endings();
        let line = lines.remove(line_index);
        lines.insert(target_index, line);
        self.text = lines.concat();
        self.selection = None;
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
        self.cursor = self.line_col_to_byte(target_index + 1, column + 1);
        true
    }

    fn lines_with_endings(&self) -> Vec<String> {
        let mut lines = self
            .text
            .split_inclusive('\n')
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if self.text.ends_with('\n') {
            lines.push(String::new());
        }
        lines
    }

    fn word_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut start = None;
        for (byte, ch) in self.text.char_indices() {
            if Self::is_word_char(ch) {
                start.get_or_insert(byte);
            } else if let Some(start) = start.take() {
                ranges.push((start, byte));
            }
        }
        if let Some(start) = start {
            ranges.push((start, self.text.len()));
        }
        ranges
    }

    fn word_at_or_before(&self, byte: usize) -> Option<(usize, usize)> {
        let byte = self.clamp_to_char_boundary(byte);
        self.word_ranges()
            .into_iter()
            .take_while(|(start, _)| *start <= byte)
            .last()
    }

    fn word_before(&self, byte: usize) -> Option<(usize, usize)> {
        let byte = self.clamp_to_char_boundary(byte);
        self.word_ranges()
            .into_iter()
            .take_while(|(_, end)| *end <= byte)
            .last()
    }

    fn word_at_or_after(&self, byte: usize) -> Option<(usize, usize)> {
        let byte = self.clamp_to_char_boundary(byte);
        self.word_ranges()
            .into_iter()
            .find(|(start, end)| (*start <= byte && byte < *end) || *start >= byte)
    }

    fn word_after(&self, byte: usize) -> Option<(usize, usize)> {
        let byte = self.clamp_to_char_boundary(byte);
        self.word_ranges()
            .into_iter()
            .find(|(start, _)| *start >= byte)
    }

    fn trim_non_word_left(&self, byte: usize) -> usize {
        let mut byte = self.clamp_to_char_boundary(byte);
        while byte > 0 {
            let previous = self.previous_char_boundary(byte);
            let Some(ch) = self.text[previous..byte].chars().next() else {
                break;
            };
            if Self::is_word_char(ch) {
                break;
            }
            byte = previous;
        }
        byte
    }

    fn trim_non_word_right(&self, byte: usize) -> usize {
        let mut byte = self.clamp_to_char_boundary(byte);
        while byte < self.text.len() {
            let next = self.next_char_boundary(byte);
            let Some(ch) = self.text[byte..next].chars().next() else {
                break;
            };
            if Self::is_word_char(ch) {
                break;
            }
            byte = next;
        }
        byte
    }

    fn word_range_at_cursor(&self) -> Option<(usize, usize)> {
        if self.text.is_empty() {
            return None;
        }

        let cursor = self.clamp_to_char_boundary(self.cursor);
        let word_byte = self.word_char_at_or_near(cursor)?;
        let mut start = word_byte;
        while start > 0 {
            let previous = self.previous_char_boundary(start);
            let Some(ch) = self.text[previous..start].chars().next() else {
                break;
            };
            if !Self::is_word_char(ch) {
                break;
            }
            start = previous;
        }

        let mut end = word_byte;
        while end < self.text.len() {
            let next = self.next_char_boundary(end);
            let Some(ch) = self.text[end..next].chars().next() else {
                break;
            };
            if !Self::is_word_char(ch) {
                break;
            }
            end = next;
        }

        (start < end).then_some((start, end))
    }

    fn word_char_at_or_near(&self, cursor: usize) -> Option<usize> {
        if cursor < self.text.len() {
            let next = self.next_char_boundary(cursor);
            if self.text[cursor..next]
                .chars()
                .next()
                .is_some_and(Self::is_word_char)
            {
                return Some(cursor);
            }
        }

        if cursor > 0 {
            let previous = self.previous_char_boundary(cursor);
            if self.text[previous..cursor]
                .chars()
                .next()
                .is_some_and(Self::is_word_char)
            {
                return Some(previous);
            }
        }

        self.text[cursor..]
            .char_indices()
            .find(|(_, ch)| Self::is_word_char(*ch))
            .map(|(offset, _)| cursor + offset)
    }

    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
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

    #[test]
    fn editor_buffer_moves_current_line_up_and_down() {
        let mut buffer = EditorBuffer::from_text("one\ntwö\nthree".to_string());
        buffer.set_cursor("one\nt".len());

        assert!(buffer.move_current_line_up());
        assert_eq!(buffer.as_str(), "twö\none\nthree");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (1, 2));

        assert!(buffer.move_current_line_down());
        assert_eq!(buffer.as_str(), "one\ntwö\nthree");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (2, 2));
    }

    #[test]
    fn editor_buffer_does_not_move_past_file_edges() {
        let mut buffer = EditorBuffer::from_text("one\ntwo".to_string());
        buffer.set_cursor(0);
        assert!(!buffer.move_current_line_up());

        buffer.set_cursor(buffer.as_str().len());
        assert!(!buffer.move_current_line_down());
    }

    #[test]
    fn editor_buffer_moves_current_line_to_paragraph_boundaries() {
        let mut buffer = EditorBuffer::from_text("one\ntwo\nthree\n\nfour".to_string());
        buffer.set_cursor("one\ntwo\nth".len());

        assert!(buffer.move_current_line_to_paragraph_start());
        assert_eq!(buffer.as_str(), "three\none\ntwo\n\nfour");

        assert!(buffer.move_current_line_to_paragraph_end());
        assert_eq!(buffer.as_str(), "one\ntwo\nthree\n\nfour");
    }

    #[test]
    fn editor_buffer_does_not_move_blank_line_to_paragraph_boundary() {
        let mut buffer = EditorBuffer::from_text("one\n\ntwo".to_string());
        buffer.set_cursor(4);

        assert!(!buffer.move_current_line_to_paragraph_start());
        assert!(!buffer.move_current_line_to_paragraph_end());
        assert_eq!(buffer.as_str(), "one\n\ntwo");
    }

    #[test]
    fn editor_buffer_selects_word_under_cursor() {
        let mut buffer = EditorBuffer::from_text("hello brave_world".to_string());
        buffer.set_cursor(8);

        assert!(buffer.select_word());

        assert_eq!(buffer.selection(), Some((6, 17)));
    }

    #[test]
    fn editor_buffer_extends_word_selection_left() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor("hello brave world".len());

        assert!(buffer.select_word_left_extend());
        assert_eq!(buffer.selection(), Some((12, 17)));
        assert_eq!(buffer.cursor(), 12);

        assert!(buffer.select_word_left_extend());
        assert_eq!(buffer.selection(), Some((6, 17)));
        assert_eq!(buffer.cursor(), 6);

        assert!(buffer.select_word_left_extend());
        assert_eq!(buffer.selection(), Some((0, 17)));
        assert_eq!(buffer.cursor(), 0);
    }

    #[test]
    fn editor_buffer_extends_word_selection_right() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor(0);

        assert!(buffer.select_word_right_extend());
        assert_eq!(buffer.selection(), Some((0, 5)));
        assert_eq!(buffer.cursor(), 5);

        assert!(buffer.select_word_right_extend());
        assert_eq!(buffer.selection(), Some((0, 11)));
        assert_eq!(buffer.cursor(), 11);

        assert!(buffer.select_word_right_extend());
        assert_eq!(buffer.selection(), Some((0, 17)));
        assert_eq!(buffer.cursor(), 17);
    }

    #[test]
    fn editor_buffer_extends_word_selection_with_unicode() {
        let mut buffer = EditorBuffer::from_text("café mañana world".to_string());
        buffer.set_cursor(0);

        assert!(buffer.select_word_right_extend());
        assert_eq!(buffer.selection(), Some((0, "café".len())));

        assert!(buffer.select_word_right_extend());
        assert_eq!(buffer.selection(), Some((0, "café mañana".len())));
    }

    #[test]
    fn editor_buffer_shrinks_word_selection_from_active_edge() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor("hello ".len());

        assert!(buffer.select_word_right_extend());
        assert!(buffer.select_word_right_extend());
        assert_eq!(buffer.selection(), Some((6, 17)));
        assert_eq!(buffer.cursor(), 17);

        assert!(buffer.select_word_left_extend());
        assert_eq!(buffer.selection(), Some((6, 11)));
        assert_eq!(buffer.cursor(), 11);

        assert!(buffer.select_word_left_extend());
        assert_eq!(buffer.selection(), None);
        assert_eq!(buffer.cursor(), 6);
    }

    #[test]
    fn editor_buffer_shrinks_word_selection_from_left_edge() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor("hello brave".len());

        assert!(buffer.select_word_left_extend());
        assert!(buffer.select_word_left_extend());
        assert_eq!(buffer.selection(), Some((0, 11)));
        assert_eq!(buffer.cursor(), 0);

        assert!(buffer.select_word_right_extend());
        assert_eq!(buffer.selection(), Some((6, 11)));
        assert_eq!(buffer.cursor(), 6);
    }

    #[test]
    fn editor_buffer_selects_current_line() {
        let mut buffer = EditorBuffer::from_text("one\ntwo\nthree".to_string());
        buffer.set_cursor(5);

        assert!(buffer.select_current_line());

        assert_eq!(buffer.selection(), Some((4, 7)));
    }

    #[test]
    fn editor_buffer_deletes_word_under_cursor() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor(8);

        assert!(buffer.delete_word());

        assert_eq!(buffer.as_str(), "hello  world");
        assert_eq!(buffer.cursor(), 6);
    }

    #[test]
    fn editor_buffer_moves_to_top_and_bottom() {
        let mut buffer = EditorBuffer::from_text("one\ntwo".to_string());
        buffer.move_to_bottom();
        assert_eq!(buffer.cursor(), buffer.as_str().len());

        buffer.move_to_top();
        assert_eq!(buffer.cursor(), 0);
    }
}
