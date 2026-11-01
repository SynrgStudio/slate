//! Per-line preview metadata kept in step with edits, so a frame only touches
//! the lines it shows: an edit re-reads the changed lines, and placing the
//! viewport walks plain numbers instead of re-parsing the whole document.
use crate::editor_buffer::EditorBuffer;
use crate::markdown::{
    is_markdown_separator, parse_fenced_code_marker, parse_heading_line,
    parse_markdown_table_separator, split_markdown_table_row,
};

#[derive(Default)]
pub(crate) struct Layout {
    key: Option<(u64, u64)>,
    scale: u32,
    lines: Vec<Line>,
}

#[derive(Clone, Default)]
struct Line {
    characters: u32,
    kind: Kind,
    /// Cells when the line reads as a table row, 0 otherwise.
    table_cells: u16,
    /// Cells when the line reads as a table separator (`| --- |`), 0 otherwise.
    separator_cells: u16,
    /// Derived: inside a code fence when the line starts.
    in_code: bool,
    /// Derived: lines a table starting here spans, 0 when it starts none.
    table_span: u32,
    /// Derived: part of a table started on an earlier line.
    in_table: bool,
    measured: Option<(f32, f32, f32, bool)>,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Kind {
    #[default]
    Text,
    Empty,
    Fence,
    Rule,
    Heading(u8),
}

/// How one block (a line, or a whole table) is sized before it is measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shape {
    pub size: f32,
    pub extra: f32,
    pub wrapped: bool,
}

impl Line {
    fn read(text: &str) -> Self {
        let text = text.strip_suffix('\r').unwrap_or(text);
        let kind = if parse_fenced_code_marker(text).is_some() {
            Kind::Fence
        } else if text.trim().is_empty() {
            Kind::Empty
        } else if is_markdown_separator(text) {
            Kind::Rule
        } else if let Some(heading) = parse_heading_line(text) {
            Kind::Heading(heading.level as u8)
        } else {
            Kind::Text
        };
        let table_cells = split_markdown_table_row(text).map_or(0, |cells| cells.len());
        let separator_cells = if table_cells > 0 {
            parse_markdown_table_separator(text).map_or(0, |cells| cells.len())
        } else {
            0
        };
        Self {
            characters: text.chars().count().min(u32::MAX as usize) as u32,
            kind,
            table_cells: table_cells.min(u16::MAX as usize) as u16,
            separator_cells: separator_cells.min(u16::MAX as usize) as u16,
            ..Self::default()
        }
    }
}

impl Layout {
    /// Brings the metadata up to the buffer's text, re-reading only the lines
    /// an edit touched when the buffer can say which those are.
    pub fn sync(&mut self, buffer: &EditorBuffer, scale: f32) {
        let key = (buffer.identity, buffer.revision);
        if self.key == Some(key) && self.scale == scale.to_bits() {
            return;
        }
        if self.scale != scale.to_bits() {
            self.scale = scale.to_bits();
            for line in &mut self.lines {
                line.measured = None;
            }
        }
        let count = buffer.line_count();
        let edges = match self.key {
            Some((identity, revision)) if identity == buffer.identity && revision != key.1 => {
                buffer.unchanged_layout_edges(revision)
            }
            Some(old) if old == key => Some((self.lines.len(), 0)),
            _ => None,
        };
        match edges {
            Some((prefix, suffix)) => {
                let old = self.lines.len();
                let prefix = prefix.min(old).min(count);
                let suffix = suffix.min(old - prefix).min(count - prefix);
                self.lines.splice(
                    prefix..old - suffix,
                    (prefix..count - suffix).map(|index| Line::read(buffer.line(index))),
                );
            }
            None => {
                self.lines = (0..count)
                    .map(|index| Line::read(buffer.line(index)))
                    .collect();
            }
        }
        self.key = Some(key);
        self.derive();
    }

    /// Code fences and tables depend on the lines around them.
    fn derive(&mut self) {
        let mut in_code = false;
        let mut index = 0;
        while index < self.lines.len() {
            let start = index;
            let mut span = 0;
            let line = &self.lines[index];
            if line.kind != Kind::Fence
                && !in_code
                && line.table_cells > 0
                && self
                    .lines
                    .get(index + 1)
                    .is_some_and(|next| next.separator_cells == line.table_cells)
            {
                let cells = line.table_cells;
                let mut end = index + 2;
                while self
                    .lines
                    .get(end)
                    .is_some_and(|row| row.table_cells == cells)
                {
                    end += 1;
                }
                span = (end - index) as u32;
            }
            let line = &mut self.lines[start];
            if line.in_code != in_code || line.table_span != span || line.in_table {
                line.measured = None;
            }
            line.in_code = in_code;
            line.table_span = span;
            line.in_table = false;
            if line.kind == Kind::Fence {
                in_code = !in_code;
            }
            index += 1;
            if span > 0 {
                for row in &mut self.lines[start + 1..start + span as usize] {
                    row.in_code = false;
                    row.table_span = 0;
                    row.in_table = true;
                    row.measured = None;
                }
                index = start + span as usize;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Lines the block starting at `index` covers (1, or a whole table).
    pub fn block_len(&self, index: usize) -> usize {
        self.lines
            .get(index)
            .map_or(1, |line| (line.table_span as usize).max(1))
    }

    /// First line of the block holding `index`.
    pub fn block_start(&self, mut index: usize) -> usize {
        while index > 0 && self.lines.get(index).is_some_and(|line| line.in_table) {
            index -= 1;
        }
        index
    }

    pub fn is_table(&self, index: usize) -> bool {
        self.lines
            .get(index)
            .is_some_and(|line| line.table_span > 0)
    }

    pub fn in_code(&self, index: usize) -> bool {
        self.lines.get(index).is_some_and(|line| line.in_code)
    }

    pub fn shape(&self, index: usize) -> Shape {
        let Some(line) = self.lines.get(index) else {
            return Shape {
                size: 0.0,
                extra: 8.0,
                wrapped: false,
            };
        };
        let (size, extra, wrapped) = if line.table_span > 0 {
            // Rows after the separator, each a fixed-height table row.
            ((line.table_span - 1) as f32 * 26.0, 0.0, false)
        } else if line.kind == Kind::Fence {
            if line.in_code {
                (0.0, 0.0, false)
            } else {
                (16.0, 8.0, false)
            }
        } else if line.kind == Kind::Empty {
            (0.0, 8.0, false)
        } else if line.in_code {
            (18.0, 10.0, true)
        } else {
            match line.kind {
                Kind::Rule => (18.0, 12.0, false),
                Kind::Heading(level) => (
                    match level {
                        1 => 32.0,
                        2 => 26.0,
                        3 => 22.0,
                        _ => 20.0,
                    },
                    12.0,
                    true,
                ),
                _ => (18.0, 4.0, true),
            }
        };
        Shape {
            size,
            extra,
            wrapped,
        }
    }

    pub fn estimate(&self, index: usize, width: f32) -> f32 {
        let shape = self.shape(index);
        let (size, extra) = (shape.size, shape.extra);
        let Some(line) = self.lines.get(index) else {
            return size + extra;
        };
        if let Some((old_width, height, old_extra, old_wrapped)) = line.measured {
            if !old_wrapped || (old_width - width).abs() < 0.5 {
                return height;
            }
            return ((height - old_extra).max(size) * (old_width / width.max(40.0))).max(size)
                + old_extra;
        }
        if !shape.wrapped {
            return size + extra;
        }
        ((line.characters as f32 * size * 0.5 / width.max(40.0))
            .ceil()
            .max(1.0))
            * size
            + extra
    }

    pub fn measured(&mut self, index: usize, width: f32, height: f32) {
        let shape = self.shape(index);
        if let Some(line) = self.lines.get_mut(index) {
            line.measured = Some((width, height, shape.extra, shape.wrapped));
        }
    }

    /// Estimated top of every line plus the end, relative to the first line.
    /// Lines inside a table share its height evenly after the separator.
    pub fn tops(&self, width: f32, tops: &mut Vec<f32>) {
        tops.clear();
        tops.reserve(self.lines.len() + 1);
        let mut y = 0.0;
        let mut index = 0;
        while index < self.lines.len() {
            let height = self.estimate(index, width);
            let span = self.block_len(index);
            if span > 1 {
                let row = height / (span - 1) as f32;
                tops.push(y);
                tops.push(y + row);
                for n in 2..span {
                    tops.push(y + (n - 1) as f32 * row);
                }
            } else {
                tops.push(y);
            }
            y += height;
            index += span;
        }
        tops.push(y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(text: &str) -> Layout {
        let mut layout = Layout::default();
        layout.sync(&EditorBuffer::from_text(text.to_string()), 1.0);
        layout
    }

    fn summary(layout: &Layout) -> Vec<(bool, u32, bool)> {
        layout
            .lines
            .iter()
            .map(|line| (line.in_code, line.table_span, line.in_table))
            .collect()
    }

    #[test]
    fn measurements_are_reused_and_dropped_on_edit_or_scale() {
        let mut layout = Layout::default();
        let mut buffer = EditorBuffer::from_text("long text ".repeat(50));
        layout.sync(&buffer, 1.0);
        layout.measured(0, 400.0, 60.0);
        assert_eq!(layout.estimate(0, 400.0), 60.0);
        assert!(layout.estimate(0, 200.0) > 100.0);
        layout.sync(&buffer, 1.0);
        assert_eq!(layout.estimate(0, 400.0), 60.0);
        buffer.insert_text("changed");
        layout.sync(&buffer, 1.0);
        assert!(layout.lines[0].measured.is_none());
        layout.measured(0, 400.0, 60.0);
        layout.sync(&buffer, 1.5);
        assert!(layout.lines[0].measured.is_none());
    }

    #[test]
    fn edits_update_only_touched_lines_and_match_a_fresh_read() {
        let text =
            "# Title\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn x() {}\n```\nend\n";
        let mut buffer = EditorBuffer::from_text(text.to_string());
        let mut layout = Layout::default();
        layout.sync(&buffer, 1.0);
        layout.measured(0, 400.0, 99.0);
        assert_eq!(
            summary(&layout),
            vec![
                (false, 0, false),
                (false, 0, false),
                (false, 3, false),
                (false, 0, true),
                (false, 0, true),
                (false, 0, false),
                (false, 0, false),
                (true, 0, false),
                (true, 0, false),
                (false, 0, false),
                (false, 0, false),
            ]
        );
        // Typing far from the title keeps its measurement.
        buffer.set_cursor(buffer.line_end(9));
        buffer.insert_text("ing");
        layout.sync(&buffer, 1.0);
        assert_eq!(layout.estimate(0, 400.0), 99.0);
        // Opening a fence above flips everything after it, like a fresh read.
        buffer.set_cursor(buffer.line_start(1));
        buffer.insert_text("```\n");
        layout.sync(&buffer, 1.0);
        assert_eq!(summary(&layout), summary(&fresh(buffer.as_str())));
        assert!(layout.in_code(2) && !layout.is_table(3));
        // Breaking the table separator ends the table.
        buffer.undo();
        let separator = buffer.line_start(3);
        buffer.set_selection(separator, separator + 1);
        buffer.insert_text("x");
        layout.sync(&buffer, 1.0);
        assert_eq!(summary(&layout), summary(&fresh(buffer.as_str())));
        assert!(!layout.is_table(2));
        assert_eq!(layout.len(), buffer.line_count());
    }

    #[test]
    fn tops_cover_every_line_and_tables_split_after_the_separator() {
        let layout = fresh("a\n| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |\nz");
        let mut tops = Vec::new();
        layout.tops(400.0, &mut tops);
        assert_eq!(tops.len(), layout.len() + 1);
        assert!(tops.windows(2).all(|pair| pair[0] <= pair[1]));
        // Header row, then a zero-height separator, then the body rows.
        assert_eq!(tops[2] - tops[1], 26.0);
        assert_eq!(tops[3], tops[2]);
        assert_eq!(tops[4] - tops[3], 26.0);
    }
}
