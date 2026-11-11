use crate::history::{GapText, History, Kind};

struct LayoutChange {
    before: u64,
    after: u64,
    prefix_lines: usize,
    suffix_lines: usize,
}

const TYPING_GROUP_PAUSE: std::time::Duration = std::time::Duration::from_millis(1000);
const TYPING_GROUP_MAX_BYTES: usize = 256;
/// Jumps longer than this rebuild the target text once and change the
/// buffer in one splice, instead of one splice per step.
const DIRECT_STEPS: usize = 16;

static NEXT_BUFFER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) struct EditorBuffer {
    pub(crate) identity: u64,
    text: String,
    line_starts: Vec<usize>,
    layout_changes: std::collections::VecDeque<LayoutChange>,
    cursor: usize,
    selection: Option<(usize, usize)>,
    history: History,
    typing_revision: Option<u64>,
    pub(crate) revision: u64,
}

impl EditorBuffer {
    pub(crate) fn new() -> Self {
        Self::from_text(String::new())
    }

    /// A buffer whose text is what its file holds (or empty): its starting
    /// state counts as saved.
    pub(crate) fn from_text(text: String) -> Self {
        let mut buffer = Self {
            identity: NEXT_BUFFER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            text,
            line_starts: Vec::new(),
            layout_changes: std::collections::VecDeque::new(),
            cursor: 0,
            selection: None,
            history: History::new(true),
            typing_revision: None,
            revision: 0,
        };
        buffer.rebuild_line_index();
        buffer
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    /// Bytes held by the undo history (changes only, never whole documents).
    #[allow(dead_code)]
    pub(crate) fn undo_history_bytes(&self) -> usize {
        self.history.bytes()
    }

    pub(crate) fn history(&self) -> &History {
        &self.history
    }

    /// The text is exactly what was last saved (or opened).
    pub(crate) fn at_saved_state(&self) -> bool {
        self.history.at_saved()
    }

    /// The text now is on disk. Typing after this starts a new step, so the
    /// saved state stays exactly what was written.
    pub(crate) fn mark_saved(&mut self) {
        self.history.mark_saved();
        self.typing_revision = None;
    }

    /// A save of state `seq` (see [`Self::history_position`]) landed.
    pub(crate) fn mark_saved_at(&mut self, seq: u64) {
        self.history.mark_saved_seq(seq);
    }

    /// The current state, to mark it saved once a background write lands.
    /// Typing after this starts a new step.
    pub(crate) fn history_position(&mut self) -> u64 {
        self.typing_revision = None;
        self.history.current_seq()
    }

    /// Replaces `start..end` with `replacement`, records one undo delta and keeps
    /// the line index/layout tracking incremental. Cursor placement is the caller's job.
    fn edit(&mut self, start: usize, end: usize, replacement: &str) {
        self.edit_as(start, end, replacement, Kind::Edit);
    }

    fn edit_as(&mut self, start: usize, end: usize, replacement: &str, kind: Kind) {
        self.history.push(
            start,
            self.text[start..end].to_string(),
            replacement.to_string(),
            self.cursor,
            self.selection,
            kind,
        );
        self.typing_revision = None;
        self.splice_text(start, end, replacement);
    }

    /// Like [`Self::edit`] but derives the smallest changed range from a full new text.
    fn edit_to(&mut self, new_text: String) {
        self.edit_to_as(&new_text, Kind::Edit);
    }

    fn edit_to_as(&mut self, new_text: &str, kind: Kind) -> bool {
        if new_text == self.text {
            return false;
        }
        let (start, old_end) = Self::changed_old_byte_range(&self.text, new_text);
        let new_end = new_text.len() - (self.text.len() - old_end);
        self.edit_as(start, old_end, &new_text[start..new_end], kind);
        true
    }

    /// Makes the whole text `new_text` as one undoable step of `kind` (a
    /// reload from disk, lines brought back from the history). The cursor
    /// goes to `cursor`, or stays where it was.
    pub(crate) fn replace_text_as(
        &mut self,
        new_text: &str,
        kind: Kind,
        cursor: Option<usize>,
    ) -> bool {
        let previous = self.cursor;
        if !self.edit_to_as(new_text, kind) {
            return false;
        }
        self.selection = None;
        self.cursor = self.clamp_to_char_boundary(cursor.unwrap_or(previous));
        true
    }

    fn splice_text(&mut self, start: usize, end: usize, replacement: &str) {
        let first = self.line_index_for_byte(start);
        let last = self.line_index_for_byte(end);
        let suffix = self.line_count() - last - 1;
        let before = self.revision;
        let delta = replacement.len() as isize - (end - start) as isize;
        self.text.replace_range(start..end, replacement);
        let inserted: Vec<_> = replacement
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(offset, _)| start + offset + 1)
            .collect();
        let tail = first + 1 + inserted.len();
        self.line_starts.splice(first + 1..last + 1, inserted);
        for position in &mut self.line_starts[tail..] {
            *position = position
                .checked_add_signed(delta)
                .expect("valid edited line offset");
        }
        self.revision = self.revision.wrapping_add(1);
        self.record_layout_change(before, first, suffix);
    }

    fn restore_cursor_state(&mut self, cursor: usize, selection: Option<(usize, usize)>) {
        self.cursor = self.clamp_to_char_boundary(cursor);
        self.selection = selection.map(|(start, end)| {
            (
                self.clamp_to_char_boundary(start),
                self.clamp_to_char_boundary(end),
            )
        });
    }

    /// Undoes history node `index` on the text (node state → parent state).
    fn unapply(&mut self, index: u32) {
        let i = index as usize;
        let start = self.history.nodes[i].start;
        let end = start + self.history.nodes[i].inserted.len();
        let removed = std::mem::take(&mut self.history.nodes[i].removed);
        self.splice_text(start, end, &removed);
        self.history.nodes[i].removed = removed;
    }

    /// Redoes history node `index` on the text (parent state → node state).
    fn apply(&mut self, index: u32) {
        let i = index as usize;
        let start = self.history.nodes[i].start;
        let end = start + self.history.nodes[i].removed.len();
        let inserted = std::mem::take(&mut self.history.nodes[i].inserted);
        self.splice_text(start, end, &inserted);
        self.history.nodes[i].inserted = inserted;
    }

    /// Goes to history state `target`: undoing up to the state both share and
    /// redoing down the other branch. Redo then follows the branch taken.
    fn go_to(&mut self, target: u32) -> bool {
        let from = self.history.current;
        if target == from {
            return false;
        }
        let (ups, downs) = self.history.path(from, target);
        let leaving = (self.cursor, self.selection);
        if ups.len() + downs.len() <= DIRECT_STEPS {
            for &up in &ups {
                self.unapply(up);
            }
            for &down in &downs {
                self.apply(down);
            }
        } else {
            let mut text = GapText::new(&self.text);
            self.history.rewind(&mut text, &ups, &downs);
            let new_text = text.to_string();
            let (start, old_end) = Self::changed_old_byte_range(&self.text, &new_text);
            let new_end = new_text.len() - (self.text.len() - old_end);
            self.splice_text(start, old_end, &new_text[start..new_end]);
        }
        if let Some(&left) = ups.first() {
            self.history.nodes[left as usize].after = Some(leaving);
        }
        for &step in ups.iter().chain(&downs) {
            let parent = self.history.nodes[step as usize].parent;
            self.history.nodes[parent as usize].last_child = step;
        }
        self.history.current = target;
        if let Some(&down) = downs.last() {
            let node = &self.history.nodes[down as usize];
            let (cursor, selection) = node
                .after
                .unwrap_or((node.start + node.inserted.len(), None));
            self.restore_cursor_state(cursor, selection);
        } else if let Some(&up) = ups.last() {
            let node = &self.history.nodes[up as usize];
            let (cursor, selection) = (node.cursor_before, node.selection_before);
            self.restore_cursor_state(cursor, selection);
        }
        self.typing_revision = None;
        true
    }

    pub(crate) fn undo(&mut self) -> bool {
        let parent = self.history.current_node().parent;
        parent != crate::history::NONE && self.go_to(parent)
    }

    pub(crate) fn redo(&mut self) -> bool {
        let child = self.history.current_node().last_child;
        child != crate::history::NONE && self.go_to(child)
    }

    /// Goes to the history state with stable id `seq`.
    pub(crate) fn jump_to(&mut self, seq: u64) -> bool {
        self.history
            .index_of(seq)
            .is_some_and(|target| self.go_to(target))
    }

    /// Inserts keyboard-typed text. Consecutive typing of a word is grouped into one
    /// undo step; whitespace after a word, newlines, pauses, cursor moves or other
    /// edits start a new group.
    pub(crate) fn type_text(&mut self, text: &str) {
        let mut chars = text.chars();
        let single = match (chars.next(), chars.next()) {
            (Some(ch), None) if ch != '\n' => Some(ch),
            _ => None,
        };
        let can_group = single.is_some_and(|ch| {
            let top = self.history.current_node();
            let word_then_space =
                ch.is_whitespace() && !top.inserted.ends_with(char::is_whitespace);
            self.selection.is_none()
                && self.typing_revision == Some(self.revision)
                && top.kind == Kind::Typing
                // A state with branches below, or the saved one, stays as it is.
                && top.children == 0
                && !self.history.at_saved()
                && top.start + top.inserted.len() == self.cursor
                && top.inserted.len() < TYPING_GROUP_MAX_BYTES
                && top.recorded_at.elapsed() < TYPING_GROUP_PAUSE
                && !word_then_space
        });
        if can_group {
            let cursor = self.cursor;
            self.splice_text(cursor, cursor, text);
            self.cursor = cursor + text.len();
            self.history.extend_current(text);
            self.typing_revision = Some(self.revision);
            return;
        }
        let revision = self.revision;
        self.insert_text(text);
        if self.revision != revision && single.is_some() {
            self.history.set_current_kind(Kind::Typing);
            self.typing_revision = Some(self.revision);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }

    /// Replaces the text and starts the history over (not saved: callers that
    /// load a file mark it).
    pub(crate) fn set_text(&mut self, text: String) {
        self.layout_changes.clear();
        self.text = text;
        self.cursor = self.cursor.min(self.text.len());
        self.selection = None;
        self.history.reset(false);
        self.typing_revision = None;
        self.revision = self.revision.wrapping_add(1);
        self.rebuild_line_index();
    }

    pub(crate) fn clear(&mut self) {
        self.set_text(String::new());
    }

    #[allow(dead_code)]
    pub(crate) fn mark_external_edit(&mut self) {
        self.layout_changes.clear();
        self.cursor = self.cursor.min(self.text.len());
        self.selection = None;
        self.history.reset(false);
        self.typing_revision = None;
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

    pub(crate) fn move_to_paragraph_start(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        let line_index = self.line_index_for_byte(self.cursor);

        let mut paragraph_line = if self.line(line_index).trim().is_empty() {
            line_index.checked_sub(1)
        } else {
            Some(line_index)
        };
        while let Some(index) = paragraph_line {
            if !self.line(index).trim().is_empty() {
                break;
            }
            paragraph_line = index.checked_sub(1);
        }

        let Some(mut target) = paragraph_line else {
            return false;
        };
        while target > 0 && !self.line(target - 1).trim().is_empty() {
            target -= 1;
        }

        let target_byte = self.line_start(target);
        if self.cursor != target_byte {
            self.set_cursor(target_byte);
            return true;
        }

        if target == 0 {
            return false;
        }
        let mut previous = target - 1;
        while self.line(previous).trim().is_empty() {
            let Some(next_previous) = previous.checked_sub(1) else {
                return false;
            };
            previous = next_previous;
        }
        while previous > 0 && !self.line(previous - 1).trim().is_empty() {
            previous -= 1;
        }
        self.set_cursor(self.line_start(previous));
        true
    }

    pub(crate) fn move_to_paragraph_end(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        let line_index = self.line_index_for_byte(self.cursor);
        let line_count = self.line_count();

        let mut paragraph_line = if self.line(line_index).trim().is_empty() {
            (line_index + 1 < line_count).then_some(line_index + 1)
        } else {
            Some(line_index)
        };
        while let Some(index) = paragraph_line {
            if index >= line_count {
                return false;
            }
            if !self.line(index).trim().is_empty() {
                break;
            }
            paragraph_line = (index + 1 < line_count).then_some(index + 1);
        }

        let Some(mut target) = paragraph_line else {
            return false;
        };
        while target + 1 < line_count && !self.line(target + 1).trim().is_empty() {
            target += 1;
        }

        let target_byte = self.line_end(target);
        if self.cursor != target_byte {
            self.set_cursor(target_byte);
            return true;
        }

        if target + 1 >= line_count {
            return false;
        }
        let mut next = target + 1;
        while next < line_count && self.line(next).trim().is_empty() {
            next += 1;
        }
        if next >= line_count {
            return false;
        }
        while next + 1 < line_count && !self.line(next + 1).trim().is_empty() {
            next += 1;
        }
        self.set_cursor(self.line_end(next));
        true
    }

    pub(crate) fn move_to_word_start_left(&mut self) -> bool {
        let cursor = self.cursor;
        let Some((start, _)) = self
            .word_ranges()
            .into_iter()
            .take_while(|(start, end)| {
                if *start < cursor && cursor <= *end {
                    true
                } else {
                    *start < cursor
                }
            })
            .last()
        else {
            return false;
        };
        self.set_cursor(start);
        true
    }

    pub(crate) fn move_to_word_end_right(&mut self) -> bool {
        let cursor = self.cursor;
        let Some((_, end)) = self
            .word_ranges()
            .into_iter()
            .find(|(start, end)| (*start <= cursor && cursor < *end) || *start >= cursor)
        else {
            return false;
        };
        self.set_cursor(end);
        true
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

    pub(crate) fn indent_current_or_selected_lines(&mut self) -> bool {
        let (start_line, end_line) = self.selection_line_range().unwrap_or_else(|| {
            let line = self.line_index_for_byte(self.cursor);
            (line, line)
        });
        if start_line > end_line || end_line >= self.line_count() {
            return false;
        }

        let original_cursor = self.cursor;
        let had_selection = self.selection.is_some();
        let affected_starts = (start_line..=end_line)
            .map(|line| self.line_start(line))
            .collect::<Vec<_>>();

        let region_start = self.line_start(start_line);
        let region_end = self.line_end(end_line);
        let mut indented =
            String::with_capacity(region_end - region_start + affected_starts.len() * 4);
        for (index, line) in self.text[region_start..region_end].split('\n').enumerate() {
            if index > 0 {
                indented.push('\n');
            }
            indented.push_str("    ");
            indented.push_str(line);
        }
        self.edit(region_start, region_end, &indented);
        self.selection = None;
        let cursor_shift = affected_starts
            .iter()
            .filter(|start| **start <= original_cursor)
            .count()
            * 4;
        self.cursor = original_cursor + cursor_shift;

        if had_selection {
            let selection_start = self.line_start(start_line);
            let selection_end = self.line_end(end_line);
            self.set_selection(selection_start, selection_end);
        }
        true
    }

    pub(crate) fn dedent_current_or_selected_lines(&mut self) -> bool {
        let (start_line, end_line) = self.selection_line_range().unwrap_or_else(|| {
            let line = self.line_index_for_byte(self.cursor);
            (line, line)
        });
        if start_line > end_line || end_line >= self.line_count() {
            return false;
        }

        let removals = (start_line..=end_line)
            .filter_map(|line| {
                let start = self.line_start(line);
                let remove_len = self
                    .line(line)
                    .chars()
                    .take_while(|ch| *ch == ' ')
                    .take(4)
                    .count();
                (remove_len > 0).then_some((start, remove_len))
            })
            .collect::<Vec<_>>();
        if removals.is_empty() {
            return false;
        }

        let original_cursor = self.cursor;
        let had_selection = self.selection.is_some();
        let cursor_shift = removals
            .iter()
            .map(|(start, len)| {
                if original_cursor >= start + len {
                    *len
                } else if original_cursor > *start {
                    original_cursor - *start
                } else {
                    0
                }
            })
            .sum::<usize>();

        let region_start = removals[0].0;
        let (last_start, last_len) = removals[removals.len() - 1];
        let region_end = last_start + last_len;
        let mut dedented = String::with_capacity(region_end - region_start);
        let mut copied_until = region_start;
        for (start, len) in &removals {
            dedented.push_str(&self.text[copied_until..*start]);
            copied_until = start + len;
        }
        self.edit(region_start, region_end, &dedented);
        self.selection = None;
        self.cursor = self.clamp_to_char_boundary(original_cursor.saturating_sub(cursor_shift));

        if had_selection {
            let selection_start = self.line_start(start_line);
            let selection_end = self.line_end(end_line);
            self.set_selection(selection_start, selection_end);
        }
        true
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

    #[allow(dead_code)]
    pub(crate) fn replace_current_line(&mut self, replacement: &str, cursor_column: usize) -> bool {
        let line_index = self.line_index_for_byte(self.cursor);
        let start = self.line_start(line_index);
        let end = self.line_end(line_index);
        self.replace_selection_or_range(start, end, replacement);
        let cursor_column = cursor_column.min(replacement.chars().count());
        self.cursor = self.line_col_to_byte(line_index + 1, cursor_column + 1);
        true
    }

    pub(crate) fn delete_current_line(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }

        if self.line_count() == 1 {
            self.edit(0, self.text.len(), "");
            self.cursor = 0;
            self.selection = None;
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

        self.edit(start, end, "");
        self.selection = None;
        self.cursor = self.clamp_to_char_boundary(start.min(self.text.len()));
        true
    }

    pub(crate) fn duplicate_current_line(&mut self) -> bool {
        let line_index = self.line_index_for_byte(self.cursor);
        let (_, column) = self.cursor_line_col();
        let line = self.line(line_index).to_string();
        let insertion = if line_index + 1 < self.line_count() {
            format!("{line}\n")
        } else {
            format!("\n{line}")
        };
        let insert_at = if line_index + 1 < self.line_count() {
            self.line_start(line_index + 1)
        } else {
            self.text.len()
        };

        self.edit(insert_at, insert_at, &insertion);
        self.selection = None;
        self.cursor = self.line_col_to_byte(line_index + 2, column + 1);
        true
    }

    pub(crate) fn move_current_line_up(&mut self) -> bool {
        if self.selection_line_range().is_some() {
            return self.move_selected_lines_up();
        }

        let line_index = self.line_index_for_byte(self.cursor);
        if line_index == 0 {
            return false;
        }
        self.move_current_line_to(line_index - 1)
    }

    pub(crate) fn move_current_line_down(&mut self) -> bool {
        if self.selection_line_range().is_some() {
            return self.move_selected_lines_down();
        }

        let line_index = self.line_index_for_byte(self.cursor);
        if line_index + 1 >= self.lines_with_endings().len() {
            return false;
        }
        self.move_current_line_to(line_index + 1)
    }

    pub(crate) fn move_current_line_to_paragraph_start(&mut self) -> bool {
        if let Some((start_line, end_line)) = self.selection_line_range() {
            if self.line(start_line).trim().is_empty() {
                return false;
            }

            let mut target = start_line;
            while target > 0 && !self.line(target - 1).trim().is_empty() {
                target -= 1;
            }
            return self.move_line_range_to(start_line, end_line, target);
        }

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
        if let Some((start_line, end_line)) = self.selection_line_range() {
            if self.line(end_line).trim().is_empty() {
                return false;
            }

            let block_len = end_line - start_line + 1;
            let mut target_end = end_line;
            while target_end + 1 < self.lines_with_endings().len()
                && !self.line(target_end + 1).trim().is_empty()
            {
                target_end += 1;
            }
            let target_start = target_end.saturating_sub(block_len - 1);
            return self.move_line_range_to(start_line, end_line, target_start);
        }

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

    pub(crate) fn delete_word_before_cursor(&mut self) -> bool {
        if let Some((start, end)) = self.selection {
            if start != end {
                self.replace_selection_or_range(start, end, "");
                return true;
            }
        }
        let cursor = self.cursor;
        if cursor == 0 {
            return false;
        }
        let Some((start, _)) = self
            .word_ranges()
            .into_iter()
            .take_while(|(start, _)| *start < cursor)
            .last()
        else {
            return false;
        };
        self.replace_selection_or_range(start, cursor, "");
        true
    }

    pub(crate) fn delete_word_after_cursor(&mut self) -> bool {
        if let Some((start, end)) = self.selection {
            if start != end {
                self.replace_selection_or_range(start, end, "");
                return true;
            }
        }
        let cursor = self.cursor;
        if cursor >= self.text.len() {
            return false;
        }
        let Some((_, end)) = self
            .word_ranges()
            .into_iter()
            .find(|(start, end)| (*start <= cursor && cursor < *end) || *start >= cursor)
        else {
            return false;
        };
        self.replace_selection_or_range(cursor, end, "");
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
        if let Some((selection_start, selection_end)) = self.selection {
            start = selection_start;
            end = selection_end;
        }

        if start > end {
            std::mem::swap(&mut start, &mut end);
        }

        if self.text.get(start..end) == Some(replacement) {
            self.cursor = start + replacement.len();
            self.selection = None;
            return;
        }

        self.edit(start, end, replacement);
        self.cursor = start + replacement.len();
        self.selection = None;
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

    fn selection_line_range(&self) -> Option<(usize, usize)> {
        let (start, end) = self.selection?;
        if start == end {
            return None;
        }

        let start_line = self.line_index_for_byte(start);
        let mut end_line = self.line_index_for_byte(end);
        if end > start && end_line > start_line && self.line_start(end_line) == end {
            end_line -= 1;
        }

        let max_line = self.lines_with_endings().len().saturating_sub(1);
        let start_line = start_line.min(max_line);
        let end_line = end_line.min(max_line);
        (start_line <= end_line).then_some((start_line, end_line))
    }

    fn move_selected_lines_up(&mut self) -> bool {
        let Some((start_line, end_line)) = self.selection_line_range() else {
            return false;
        };
        if start_line == 0 {
            return false;
        }
        self.move_line_range_to(start_line, end_line, start_line - 1)
    }

    fn move_selected_lines_down(&mut self) -> bool {
        let Some((start_line, end_line)) = self.selection_line_range() else {
            return false;
        };
        let lines_len = self.lines_with_endings().len();
        if end_line + 1 >= lines_len {
            return false;
        }
        self.move_line_range_to(start_line, end_line, start_line + 1)
    }

    fn move_line_range_to(
        &mut self,
        start_line: usize,
        end_line: usize,
        target_start: usize,
    ) -> bool {
        if self.text.is_empty() || start_line > end_line {
            return false;
        }

        let mut lines = self.lines_with_endings();
        if start_line >= lines.len() || end_line >= lines.len() {
            return false;
        }

        let block_len = end_line - start_line + 1;
        let target_start = target_start.min(lines.len().saturating_sub(block_len));
        if target_start == start_line {
            return false;
        }

        let original_block_start_byte = self.line_start(start_line);
        let cursor_offset = self.cursor.saturating_sub(original_block_start_byte);
        let block = lines
            .splice(start_line..=end_line, std::iter::empty())
            .collect::<Vec<_>>();
        let insert_at = target_start.min(lines.len());
        lines.splice(insert_at..insert_at, block);

        self.edit_to(lines.concat());
        self.selection = None;

        let new_start = insert_at;
        let new_end = insert_at + block_len - 1;
        let selection_start = self.line_start(new_start);
        let selection_end = if new_end + 1 < self.line_count() {
            self.line_start(new_end + 1)
        } else {
            self.text.len()
        };
        self.selection = Some((selection_start, selection_end));
        self.cursor =
            self.clamp_to_char_boundary((selection_start + cursor_offset).min(selection_end));
        true
    }

    fn move_current_line_to(&mut self, target_index: usize) -> bool {
        if self.text.is_empty() {
            return false;
        }

        let line_index = self.line_index_for_byte(self.cursor);
        let mut lines = self.lines_with_endings();
        if line_index >= lines.len() {
            return false;
        }
        let target_index = target_index.min(lines.len().saturating_sub(1));
        if line_index == target_index {
            return false;
        }

        let (_, column) = self.cursor_line_col();
        let line = lines.remove(line_index);
        let target_index = target_index.min(lines.len());
        lines.insert(target_index, line);
        self.edit_to(lines.concat());
        self.selection = None;
        self.cursor = self.line_col_to_byte(target_index + 1, column + 1);
        true
    }

    fn lines_with_endings(&self) -> Vec<String> {
        let mut lines = self
            .text
            .split_inclusive('\n')
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            if let Some(last) = lines.last_mut() {
                last.push('\n');
            }
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

    fn record_layout_change(&mut self, before: u64, prefix_lines: usize, suffix_lines: usize) {
        const LIMIT: usize = 64;
        self.layout_changes.push_back(LayoutChange {
            before,
            after: self.revision,
            prefix_lines,
            suffix_lines,
        });
        while self.layout_changes.len() > LIMIT {
            self.layout_changes.pop_front();
        }
    }

    pub(crate) fn unchanged_layout_edges(&self, revision: u64) -> Option<(usize, usize)> {
        let mut expected = revision;
        let mut prefix = usize::MAX;
        let mut suffix = usize::MAX;
        for change in self
            .layout_changes
            .iter()
            .filter(|change| change.after > revision)
        {
            if change.before != expected {
                return None;
            }
            prefix = prefix.min(change.prefix_lines);
            suffix = suffix.min(change.suffix_lines);
            expected = change.after;
        }
        (expected == self.revision && expected != revision).then_some((prefix, suffix))
    }

    fn changed_old_byte_range(old: &str, new: &str) -> (usize, usize) {
        let mut prefix = old
            .bytes()
            .zip(new.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let mut suffix = old[prefix..]
            .bytes()
            .rev()
            .zip(new[prefix..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix)
        {
            suffix -= 1;
        }
        (prefix, old.len() - suffix)
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
    fn editor_buffer_undoes_and_redoes_text_edits() {
        let mut buffer = EditorBuffer::new();

        buffer.insert_text("hello");
        buffer.insert_text(" world");
        assert_eq!(buffer.as_str(), "hello world");

        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "hello");
        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "");
        assert!(!buffer.undo());

        assert!(buffer.redo());
        assert_eq!(buffer.as_str(), "hello");
        assert!(buffer.redo());
        assert_eq!(buffer.as_str(), "hello world");
        assert!(!buffer.redo());
    }

    #[test]
    fn editor_buffer_edit_after_undo_starts_a_branch_and_keeps_the_old_one() {
        let mut buffer = EditorBuffer::new();

        buffer.insert_text("hello");
        buffer.insert_text(" world");
        let world = buffer.history().current_seq();
        assert!(buffer.undo());
        buffer.insert_text(" slate");

        assert_eq!(buffer.as_str(), "hello slate");
        assert!(!buffer.redo(), "the new branch has nothing to redo");
        assert!(buffer.jump_to(world), "the undone branch is still there");
        assert_eq!(buffer.as_str(), "hello world");
        assert!(buffer.undo());
        assert!(buffer.redo(), "redo follows the branch visited last");
        assert_eq!(buffer.as_str(), "hello world");
    }

    #[test]
    fn editor_buffer_knows_when_undo_returns_to_the_saved_text() {
        let mut buffer = EditorBuffer::from_text("saved".to_string());
        assert!(buffer.at_saved_state());
        buffer.move_to_bottom();
        buffer.type_text("!");
        assert!(!buffer.at_saved_state());
        assert!(buffer.undo());
        assert!(buffer.at_saved_state());
        buffer.type_text("?");
        buffer.mark_saved();
        // Typing after a save is a new step: the saved state stays exact.
        buffer.type_text("?");
        assert!(!buffer.at_saved_state());
        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "saved?");
        assert!(buffer.at_saved_state());
        buffer.set_text("not from disk".to_string());
        assert!(!buffer.at_saved_state());
    }

    #[test]
    fn editor_buffer_history_tree_rebuilds_every_state_after_random_travel() {
        use std::collections::HashMap;
        for (max_nodes, max_bytes) in [(usize::MAX, usize::MAX), (60, usize::MAX), (400, 900)] {
            let mut buffer = EditorBuffer::from_text("café 雪
second line
".repeat(3));
            if max_nodes != usize::MAX {
                buffer.history =
                    crate::history::History::with_limits(true, max_nodes, max_bytes);
            }
            let mut seen: HashMap<u64, String> = HashMap::new();
            seen.insert(buffer.history().current_seq(), buffer.as_str().to_string());
            let mut seed = 2026_u64 ^ max_nodes as u64;
            let mut random = |limit: usize| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((seed >> 33) as usize) % limit.max(1)
            };
            for step in 0..4000 {
                let op = random(12);
                match op {
                    0..=3 => {
                        let bounds: Vec<usize> = buffer
                            .as_str()
                            .char_indices()
                            .map(|(index, _)| index)
                            .chain([buffer.as_str().len()])
                            .collect();
                        let first = bounds[random(bounds.len())];
                        let last = bounds[random(bounds.len())];
                        let text = ["x", "雪", "
", "line
", "", "abc"][random(6)];
                        buffer.set_selection(first.min(last), first.max(last));
                        buffer.insert_text(text);
                    }
                    4 | 5 => {
                        let bounds = buffer.as_str().len();
                        buffer.set_cursor(random(bounds + 1));
                        for ch in ["w", "o", "r", "d", " ", "z"].iter().take(1 + random(6)) {
                            buffer.type_text(ch);
                            seen.insert(
                                buffer.history().current_seq(),
                                buffer.as_str().to_string(),
                            );
                        }
                    }
                    6 | 7 => {
                        buffer.undo();
                    }
                    8 => {
                        buffer.redo();
                    }
                    _ => {
                        let reachable: Vec<u64> = seen
                            .keys()
                            .copied()
                            .filter(|seq| buffer.history().index_of(*seq).is_some())
                            .collect();
                        let target = reachable[random(reachable.len())];
                        buffer.jump_to(target);
                    }
                }
                let seq = buffer.history().current_seq();
                if op >= 6 {
                    assert_eq!(
                        Some(buffer.as_str()),
                        seen.get(&seq).map(String::as_str),
                        "step {step} op {op} limits {max_nodes}"
                    );
                }
                // Typing may have grown the current step: its text is the latest.
                seen.insert(seq, buffer.as_str().to_string());
                if step % 97 == 0 {
                    let history = buffer.history();
                    for (seq, text) in &seen {
                        if let Some(index) = history.index_of(*seq) {
                            assert_eq!(&history.text_of(buffer.as_str(), index), text);
                        }
                    }
                    let fresh = EditorBuffer::from_text(buffer.as_str().to_string());
                    assert_eq!(buffer.line_count(), fresh.line_count());
                    for line in 0..fresh.line_count() {
                        assert_eq!(buffer.line_start(line), fresh.line_start(line));
                    }
                }
            }
            assert!(buffer.history().len() <= max_nodes);
            if max_bytes != usize::MAX {
                assert!(buffer.undo_history_bytes() <= max_bytes);
            }
        }
    }

    #[test]
    fn editor_buffer_indents_current_line_with_four_spaces() {
        let mut buffer = EditorBuffer::from_text("one\ntwo".to_string());
        buffer.set_cursor(5);

        assert!(buffer.indent_current_or_selected_lines());

        assert_eq!(buffer.as_str(), "one\n    two");
        assert_eq!(buffer.cursor(), 9);
    }

    #[test]
    fn editor_buffer_indents_selected_lines() {
        let mut buffer = EditorBuffer::from_text("one\ntwo\nthree".to_string());
        buffer.set_selection(1, 8);

        assert!(buffer.indent_current_or_selected_lines());

        assert_eq!(buffer.as_str(), "    one\n    two\nthree");
        assert_eq!(buffer.selection(), Some((0, 15)));
    }

    #[test]
    fn editor_buffer_dedents_current_line_by_up_to_four_spaces() {
        let mut buffer = EditorBuffer::from_text("  one\n    two".to_string());
        buffer.set_cursor(9);

        assert!(buffer.dedent_current_or_selected_lines());

        assert_eq!(buffer.as_str(), "  one\ntwo");
        assert_eq!(buffer.cursor(), 6);
    }

    #[test]
    fn editor_buffer_dedents_selected_lines_without_crossing_text() {
        let mut buffer = EditorBuffer::from_text("  one\n    two\nthree".to_string());
        buffer.set_selection(0, buffer.as_str().len());

        assert!(buffer.dedent_current_or_selected_lines());

        assert_eq!(buffer.as_str(), "one\ntwo\nthree");
        assert_eq!(buffer.selection(), Some((0, buffer.as_str().len())));
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
    fn editor_buffer_duplicates_current_middle_line() {
        let mut buffer = EditorBuffer::from_text("one\ntwö\nthree".to_string());
        buffer.set_cursor("one\nt".len());

        assert!(buffer.duplicate_current_line());

        assert_eq!(buffer.as_str(), "one\ntwö\ntwö\nthree");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (3, 2));
    }

    #[test]
    fn editor_buffer_duplicates_current_last_line() {
        let mut buffer = EditorBuffer::from_text("one\ntwo".to_string());
        buffer.set_cursor(buffer.as_str().len());

        assert!(buffer.duplicate_current_line());

        assert_eq!(buffer.as_str(), "one\ntwo\ntwo");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (3, 4));
    }

    #[test]
    fn editor_buffer_duplicates_empty_buffer_as_empty_line() {
        let mut buffer = EditorBuffer::new();

        assert!(buffer.duplicate_current_line());

        assert_eq!(buffer.as_str(), "\n");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (2, 1));
    }

    #[test]
    fn editor_buffer_moves_current_line_up_and_down() {
        let mut buffer = EditorBuffer::from_text("one\ntwö\nthree".to_string());
        buffer.set_cursor("one\nt".len());

        assert!(buffer.move_current_line_up());
        assert_eq!(buffer.as_str(), "twö\none\nthree\n");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (1, 2));

        assert!(buffer.move_current_line_down());
        assert_eq!(buffer.as_str(), "one\ntwö\nthree\n");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (2, 2));
    }

    #[test]
    fn editor_buffer_moves_final_line_up_without_joining_lines() {
        let mut buffer = EditorBuffer::from_text("asd\nfgh\njkl".to_string());
        buffer.set_cursor(buffer.as_str().len());

        assert!(buffer.move_current_line_up());

        assert_eq!(buffer.as_str(), "asd\njkl\nfgh\n");
        assert_eq!(buffer.byte_to_line_col(buffer.cursor()), (2, 4));
    }

    #[test]
    fn editor_buffer_does_not_panic_when_repeatedly_moving_line_down_at_end() {
        let mut buffer = EditorBuffer::from_text("asd\nfgh\njkl".to_string());
        buffer.set_cursor(buffer.as_str().len());

        assert!(!buffer.move_current_line_down());
        assert_eq!(buffer.as_str(), "asd\nfgh\njkl");

        assert!(buffer.move_current_line_up());
        assert!(buffer.move_current_line_down());
        assert!(!buffer.move_current_line_down());
        assert!(!buffer.move_current_line_down());

        assert_eq!(buffer.as_str(), "asd\nfgh\njkl\n");
    }

    #[test]
    fn editor_buffer_moves_selected_line_block_up() {
        let mut buffer = EditorBuffer::from_text("asd\nfgh\njkl".to_string());
        let start = buffer.line_start(1);
        let end = buffer.as_str().len();
        buffer.set_selection(start, end);

        assert!(buffer.move_current_line_up());

        assert_eq!(buffer.as_str(), "fgh\njkl\nasd\n");
        assert_eq!(buffer.selection(), Some((0, "fgh\njkl\n".len())));
    }

    #[test]
    fn editor_buffer_moves_selected_line_block_down() {
        let mut buffer = EditorBuffer::from_text("asd\nfgh\njkl\nzzz".to_string());
        let start = buffer.line_start(1);
        let end = buffer.line_start(3);
        buffer.set_selection(start, end);

        assert!(buffer.move_current_line_down());

        assert_eq!(buffer.as_str(), "asd\nzzz\nfgh\njkl\n");
        assert_eq!(
            buffer.selection(),
            Some(("asd\nzzz\n".len(), "asd\nzzz\nfgh\njkl\n".len()))
        );
    }

    #[test]
    fn editor_buffer_does_not_move_selected_line_block_past_edges() {
        let mut buffer = EditorBuffer::from_text("asd\nfgh\njkl".to_string());
        buffer.set_selection(0, buffer.line_start(2));
        assert!(!buffer.move_current_line_up());

        let start = buffer.line_start(1);
        let end = buffer.as_str().len();
        buffer.set_selection(start, end);
        assert!(!buffer.move_current_line_down());
    }

    #[test]
    fn editor_buffer_moves_selected_line_block_to_paragraph_start() {
        let mut buffer = EditorBuffer::from_text("asd\nfgh\njkl\nqwe\n\nzxc".to_string());
        let start = buffer.line_start(2);
        let end = buffer.line_start(4);
        buffer.set_selection(start, end);

        assert!(buffer.move_current_line_to_paragraph_start());

        assert_eq!(buffer.as_str(), "jkl\nqwe\nasd\nfgh\n\nzxc\n");
        assert_eq!(buffer.selection(), Some((0, "jkl\nqwe\n".len())));
    }

    #[test]
    fn editor_buffer_moves_selected_line_block_to_paragraph_end() {
        let mut buffer = EditorBuffer::from_text("asd\nfgh\njkl\nqwe\n\nzxc".to_string());
        let start = buffer.line_start(0);
        let end = buffer.line_start(2);
        buffer.set_selection(start, end);

        assert!(buffer.move_current_line_to_paragraph_end());

        assert_eq!(buffer.as_str(), "jkl\nqwe\nasd\nfgh\n\nzxc\n");
        assert_eq!(
            buffer.selection(),
            Some(("jkl\nqwe\n".len(), "jkl\nqwe\nasd\nfgh\n".len()))
        );
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
        assert_eq!(buffer.as_str(), "three\none\ntwo\n\nfour\n");

        assert!(buffer.move_current_line_to_paragraph_end());
        assert_eq!(buffer.as_str(), "one\ntwo\nthree\n\nfour\n");
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
    fn editor_buffer_deletes_word_before_cursor() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor("hello brave".len());

        assert!(buffer.delete_word_before_cursor());

        assert_eq!(buffer.as_str(), "hello  world");
        assert_eq!(buffer.cursor(), "hello ".len());
    }

    #[test]
    fn editor_buffer_deletes_word_after_cursor() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor("hello ".len());

        assert!(buffer.delete_word_after_cursor());

        assert_eq!(buffer.as_str(), "hello  world");
        assert_eq!(buffer.cursor(), "hello ".len());
    }

    #[test]
    fn editor_buffer_deletes_word_across_surrounding_space() {
        let mut before = EditorBuffer::from_text("hello   brave".to_string());
        before.set_cursor("hello   ".len());
        assert!(before.delete_word_before_cursor());
        assert_eq!(before.as_str(), "brave");
        assert_eq!(before.cursor(), 0);

        let mut after = EditorBuffer::from_text("hello   brave".to_string());
        after.set_cursor("hello".len());
        assert!(after.delete_word_after_cursor());
        assert_eq!(after.as_str(), "hello");
        assert_eq!(after.cursor(), "hello".len());
    }

    #[test]
    fn editor_buffer_moves_to_top_and_bottom() {
        let mut buffer = EditorBuffer::from_text("one\ntwo".to_string());
        buffer.move_to_bottom();
        assert_eq!(buffer.cursor(), buffer.as_str().len());

        buffer.move_to_top();
        assert_eq!(buffer.cursor(), 0);
    }

    #[test]
    fn editor_buffer_moves_cursor_to_paragraph_boundaries() {
        let mut buffer = EditorBuffer::from_text("one\ntwo\nthree\n\nfour".to_string());
        buffer.set_cursor("one\ntwo\nth".len());

        assert!(buffer.move_to_paragraph_start());
        assert_eq!(buffer.cursor(), 0);

        buffer.set_cursor("one\ntwo".len());
        assert!(buffer.move_to_paragraph_end());
        assert_eq!(buffer.cursor(), "one\ntwo\nthree".len());
    }

    #[test]
    fn editor_buffer_repeats_paragraph_boundary_jumps() {
        let mut buffer = EditorBuffer::from_text("one\ntwo\n\nthree\nfour\n\nfive".to_string());
        buffer.set_cursor("one\ntwo\n\nthree".len());

        assert!(buffer.move_to_paragraph_start());
        assert_eq!(buffer.cursor(), "one\ntwo\n\n".len());

        assert!(buffer.move_to_paragraph_start());
        assert_eq!(buffer.cursor(), 0);

        assert!(buffer.move_to_paragraph_end());
        assert_eq!(buffer.cursor(), "one\ntwo".len());

        assert!(buffer.move_to_paragraph_end());
        assert_eq!(buffer.cursor(), "one\ntwo\n\nthree\nfour".len());
    }

    #[test]
    fn editor_buffer_moves_cursor_to_word_edges() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor("hello br".len());

        assert!(buffer.move_to_word_start_left());
        assert_eq!(buffer.cursor(), "hello ".len());

        buffer.set_cursor("hello brave w".len());
        assert!(buffer.move_to_word_end_right());
        assert_eq!(buffer.cursor(), "hello brave world".len());
    }

    #[test]
    fn editor_buffer_repeats_word_start_left_from_word_boundary() {
        let mut buffer = EditorBuffer::from_text("hello brave world".to_string());
        buffer.set_cursor("hello brave world".len());

        assert!(buffer.move_to_word_start_left());
        assert_eq!(buffer.cursor(), "hello brave ".len());

        assert!(buffer.move_to_word_start_left());
        assert_eq!(buffer.cursor(), "hello ".len());

        assert!(buffer.move_to_word_start_left());
        assert_eq!(buffer.cursor(), 0);
    }

    #[test]
    fn editor_buffer_groups_typed_word_into_one_undo_step() {
        let mut buffer = EditorBuffer::new();
        for ch in "hello world".chars() {
            buffer.type_text(&ch.to_string());
        }
        assert_eq!(buffer.as_str(), "hello world");

        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "hello");
        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "");
        assert!(!buffer.undo());

        assert!(buffer.redo());
        assert!(buffer.redo());
        assert_eq!(buffer.as_str(), "hello world");
        assert_eq!(buffer.cursor(), buffer.as_str().len());
    }

    #[test]
    fn editor_buffer_typing_group_breaks_on_cursor_move_and_newline() {
        let mut buffer = EditorBuffer::new();
        buffer.type_text("a");
        buffer.type_text("b");
        buffer.move_left();
        buffer.type_text("x");
        assert_eq!(buffer.as_str(), "axb");
        buffer.move_to_line_end();
        buffer.type_text("\n");
        buffer.type_text("c");
        assert_eq!(buffer.as_str(), "axb\nc");

        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "axb\n");
        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "axb");
        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "ab");
        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "");
    }

    #[test]
    fn editor_buffer_undo_history_stores_deltas_not_documents() {
        let large = "line of text\n".repeat(400_000);
        let mut buffer = EditorBuffer::from_text(large.clone());
        buffer.set_cursor(large.len() / 2);
        for _ in 0..300 {
            buffer.insert_text("x");
            buffer.move_right();
        }
        assert!(buffer.undo_history_bytes() < 4 * 1024);
        while buffer.undo() {}
        assert_eq!(buffer.as_str(), large);
    }

    #[test]
    fn editor_buffer_undo_restores_replaced_selection() {
        let mut buffer = EditorBuffer::from_text("alpha beta".to_string());
        buffer.set_selection(0, 5);
        buffer.insert_text("omega");
        assert_eq!(buffer.as_str(), "omega beta");

        assert!(buffer.undo());
        assert_eq!(buffer.as_str(), "alpha beta");
        assert_eq!(buffer.selection(), Some((0, 5)));
    }

    #[test]
    fn editor_buffer_line_operations_round_trip_through_undo_redo() {
        let original = "one\ntwo\nthree\nfour";
        let mut buffer = EditorBuffer::from_text(original.to_string());
        buffer.set_selection(4, 13);
        assert!(buffer.indent_current_or_selected_lines());
        let indented = buffer.as_str().to_string();
        assert!(buffer.move_current_line_up());
        let moved = buffer.as_str().to_string();
        assert!(buffer.duplicate_current_line());
        let duplicated = buffer.as_str().to_string();
        assert!(buffer.delete_current_line());
        let deleted = buffer.as_str().to_string();

        for expected in [&duplicated, &moved, &indented, &original.to_string()] {
            assert!(buffer.undo());
            assert_eq!(buffer.as_str(), expected.as_str());
            assert_eq!(buffer.line_count(), expected.split('\n').count());
        }
        for expected in [&indented, &moved, &duplicated, &deleted] {
            assert!(buffer.redo());
            assert_eq!(buffer.as_str(), expected.as_str());
        }
    }
}
