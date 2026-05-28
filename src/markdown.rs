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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableAlignment {
    Left,
    Center,
    Right,
}

pub(crate) fn parse_markdown_table_separator(line: &str) -> Option<Vec<TableAlignment>> {
    let cells = split_markdown_table_row(line)?;
    if cells.is_empty() {
        return None;
    }

    let mut alignments = Vec::with_capacity(cells.len());
    for cell in cells {
        let trimmed = cell.trim();
        if trimmed.len() < 3 {
            return None;
        }
        let left = trimmed.starts_with(':');
        let right = trimmed.ends_with(':');
        let core = trimmed.trim_matches(':');
        if core.len() < 3 || !core.chars().all(|ch| ch == '-') {
            return None;
        }
        alignments.push(match (left, right) {
            (true, true) => TableAlignment::Center,
            (false, true) => TableAlignment::Right,
            _ => TableAlignment::Left,
        });
    }
    Some(alignments)
}

pub(crate) fn split_markdown_table_row(line: &str) -> Option<Vec<String>> {
    if !line.contains('|') {
        return None;
    }

    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    let cells = trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();
    (cells.len() >= 2).then_some(cells)
}

pub(crate) fn is_markdown_table_start(header: &str, separator: Option<&str>) -> bool {
    let Some(header_cells) = split_markdown_table_row(header) else {
        return false;
    };
    let Some(separator_cells) = separator.and_then(parse_markdown_table_separator) else {
        return false;
    };
    header_cells.len() == separator_cells.len()
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

pub(crate) struct ParsedHeadingLine<'a> {
    pub(crate) indent: &'a str,
    pub(crate) marker: &'a str,
    pub(crate) level: usize,
    pub(crate) text: &'a str,
}

pub(crate) struct ParsedListLine<'a> {
    pub(crate) indent: &'a str,
    pub(crate) marker: &'a str,
    pub(crate) text: &'a str,
    pub(crate) ordered: bool,
    pub(crate) number: Option<usize>,
    pub(crate) separator: Option<char>,
}

pub(crate) fn parse_list_line(line: &str) -> Option<ParsedListLine<'_>> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];

    if let Some(text) = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
        return Some(ParsedListLine {
            indent,
            marker: &rest[..2],
            text,
            ordered: false,
            number: None,
            separator: None,
        });
    }

    let digit_len = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_len == 0 || digit_len > 9 {
        return None;
    }
    let marker_len = digit_len + 2;
    let marker = rest.get(..marker_len)?;
    let separator = rest.as_bytes().get(digit_len)?;
    if (*separator == b'.' || *separator == b')')
        && rest.as_bytes().get(digit_len + 1) == Some(&b' ')
    {
        return Some(ParsedListLine {
            indent,
            marker,
            text: &rest[marker_len..],
            ordered: true,
            number: rest[..digit_len].parse().ok(),
            separator: Some(*separator as char),
        });
    }

    None
}

pub(crate) fn parse_heading_line(line: &str) -> Option<ParsedHeadingLine<'_>> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    let level = rest.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) || rest.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    Some(ParsedHeadingLine {
        indent,
        marker: &rest[..=level],
        level,
        text: &rest[level + 1..],
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MarkdownLinkSpan {
    pub(crate) marker_start: usize,
    pub(crate) text_start: usize,
    pub(crate) text_end: usize,
    pub(crate) target_start: usize,
    pub(crate) target_end: usize,
    pub(crate) marker_end: usize,
}

pub(crate) fn markdown_link_target_at_byte(
    line: &str,
    line_start: usize,
    byte: usize,
) -> Option<&str> {
    parse_markdown_link_spans(line)
        .into_iter()
        .find(|span| byte >= line_start + span.marker_start && byte <= line_start + span.marker_end)
        .map(|span| &line[span.target_start..span.target_end])
}

pub(crate) fn parse_markdown_link_spans(line: &str) -> Vec<MarkdownLinkSpan> {
    let mut spans = Vec::new();
    let mut search_from = 0;
    while let Some(open_relative) = line[search_from..].find('[') {
        let marker_start = search_from + open_relative;
        let text_start = marker_start + 1;
        let Some(close_text_relative) = line[text_start..].find(']') else {
            break;
        };
        let text_end = text_start + close_text_relative;
        if text_start == text_end || line.as_bytes().get(text_end + 1) != Some(&b'(') {
            search_from = text_end.saturating_add(1);
            continue;
        }
        let target_start = text_end + 2;
        let Some(close_target_relative) = line[target_start..].find(')') else {
            break;
        };
        let target_end = target_start + close_target_relative;
        let marker_end = target_end + 1;
        if target_start < target_end {
            spans.push(MarkdownLinkSpan {
                marker_start,
                text_start,
                text_end,
                target_start,
                target_end,
                marker_end,
            });
        }
        search_from = marker_end;
    }
    spans
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slate_checkbox_lines() {
        let empty = parse_checkbox_line("[ ] todo").unwrap();
        assert_eq!(empty.marker, "[ ]");
        assert_eq!(empty.state, CheckboxState::Empty);
        assert_eq!(empty.text, "todo");

        let compact_empty = parse_checkbox_line("[] todo").unwrap();
        assert_eq!(compact_empty.marker, "[]");
        assert_eq!(compact_empty.state, CheckboxState::Empty);

        let subtask = parse_checkbox_line("- [ ] subtask").unwrap();
        assert_eq!(subtask.task_prefix, "- ");
        assert_eq!(subtask.marker, "[ ]");
        assert_eq!(subtask.text, "subtask");

        let subsubtask = parse_checkbox_line("-- [/] subsubtask").unwrap();
        assert_eq!(subsubtask.task_prefix, "-- ");
        assert_eq!(subsubtask.state, CheckboxState::Doing);

        let doing = parse_checkbox_line("  [/] doing").unwrap();
        assert_eq!(doing.indent, "  ");
        assert_eq!(doing.state, CheckboxState::Doing);
        assert_eq!(doing.text, "doing");

        let done = parse_checkbox_line("[x] done").unwrap();
        assert_eq!(done.state, CheckboxState::Done);
        assert!(parse_checkbox_line("[]todo").is_none());
        assert!(parse_checkbox_line("text [] todo").is_none());
    }

    #[test]
    fn parses_slate_markdown_separator() {
        assert!(is_markdown_separator("---"));
        assert!(is_markdown_separator("  ---  "));
        assert!(!is_markdown_separator("----"));
        assert!(!is_markdown_separator("text ---"));
    }

    #[test]
    fn parses_markdown_table_rows_and_separators() {
        assert_eq!(
            split_markdown_table_row("| Name | Status | Notes |").unwrap(),
            vec!["Name", "Status", "Notes"]
        );
        assert_eq!(
            split_markdown_table_row("Name | Status").unwrap(),
            vec!["Name", "Status"]
        );
        assert!(split_markdown_table_row("not a table").is_none());

        assert_eq!(
            parse_markdown_table_separator("| :--- | :---: | ---: |").unwrap(),
            vec![
                TableAlignment::Left,
                TableAlignment::Center,
                TableAlignment::Right
            ]
        );
        assert!(parse_markdown_table_separator("| --- | nope |").is_none());
        assert!(is_markdown_table_start(
            "| Name | Status |",
            Some("| --- | :---: |")
        ));
        assert!(!is_markdown_table_start("| Name |", Some("| --- | --- |")));
    }

    #[test]
    fn parses_list_lines() {
        let bullet = parse_list_line("  - item").unwrap();
        assert_eq!(bullet.indent, "  ");
        assert_eq!(bullet.marker, "- ");
        assert_eq!(bullet.text, "item");
        assert!(!bullet.ordered);
        assert_eq!(bullet.number, None);
        assert_eq!(bullet.separator, None);

        let star = parse_list_line("* item").unwrap();
        assert_eq!(star.marker, "* ");
        assert_eq!(star.text, "item");
        assert!(!star.ordered);

        let ordered = parse_list_line("12. item").unwrap();
        assert_eq!(ordered.marker, "12. ");
        assert_eq!(ordered.text, "item");
        assert!(ordered.ordered);
        assert_eq!(ordered.number, Some(12));
        assert_eq!(ordered.separator, Some('.'));

        let paren = parse_list_line("3) item").unwrap();
        assert_eq!(paren.marker, "3) ");
        assert_eq!(paren.text, "item");
        assert!(paren.ordered);
        assert_eq!(paren.number, Some(3));
        assert_eq!(paren.separator, Some(')'));

        assert!(parse_list_line("-not list").is_none());
        assert!(parse_list_line("1.not list").is_none());
    }

    #[test]
    fn parses_heading_lines() {
        let h1 = parse_heading_line("# Title").unwrap();
        assert_eq!(h1.level, 1);
        assert_eq!(h1.marker, "# ");
        assert_eq!(h1.text, "Title");

        let h3 = parse_heading_line("  ### Section").unwrap();
        assert_eq!(h3.indent, "  ");
        assert_eq!(h3.level, 3);
        assert_eq!(h3.marker, "### ");
        assert_eq!(h3.text, "Section");

        assert!(parse_heading_line("#Nope").is_none());
        assert!(parse_heading_line("####### Too much").is_none());
    }

    #[test]
    fn finds_markdown_link_target_at_byte() {
        let line = "see [note](./note.md)";

        assert_eq!(
            markdown_link_target_at_byte(line, 10, 15),
            Some("./note.md")
        );
        assert_eq!(
            markdown_link_target_at_byte(line, 10, 31),
            Some("./note.md")
        );
        assert_eq!(markdown_link_target_at_byte(line, 10, 10), None);
    }

    #[test]
    fn parses_markdown_link_spans() {
        assert_eq!(
            parse_markdown_link_spans("see [note](./note.md) and [web](https://example.com)"),
            vec![
                MarkdownLinkSpan {
                    marker_start: 4,
                    text_start: 5,
                    text_end: 9,
                    target_start: 11,
                    target_end: 20,
                    marker_end: 21,
                },
                MarkdownLinkSpan {
                    marker_start: 26,
                    text_start: 27,
                    text_end: 30,
                    target_start: 32,
                    target_end: 51,
                    marker_end: 52,
                },
            ]
        );
        assert!(parse_markdown_link_spans("[empty]()").is_empty());
        assert!(parse_markdown_link_spans("[](./note.md)").is_empty());
        assert!(parse_markdown_link_spans("[broken](./note.md").is_empty());
    }

    #[test]
    fn parses_inline_code_spans() {
        assert_eq!(
            parse_inline_code_spans("use `code` here and `more`"),
            vec![
                InlineCodeSpan {
                    marker_start: 4,
                    code_start: 5,
                    code_end: 9,
                    marker_end: 10,
                },
                InlineCodeSpan {
                    marker_start: 20,
                    code_start: 21,
                    code_end: 25,
                    marker_end: 26,
                },
            ]
        );
        assert!(parse_inline_code_spans("no code").is_empty());
        assert!(parse_inline_code_spans("open `code").is_empty());
        assert!(parse_inline_code_spans("empty `` span").is_empty());
    }

    #[test]
    fn parses_fenced_code_markers() {
        let rust = parse_fenced_code_marker("```rust").unwrap();
        assert_eq!(rust.language, "rust");

        let plain = parse_fenced_code_marker("  ```").unwrap();
        assert_eq!(plain.language, "");

        assert!(parse_fenced_code_marker("text ```rust").is_none());
        assert!(parse_fenced_code_marker("````rust").is_none());
    }

    #[test]
    fn parses_blockquote_lines() {
        let quote = parse_blockquote_line("> quote").unwrap();
        assert_eq!(quote.marker, "> ");
        assert_eq!(quote.depth, 1);
        assert_eq!(quote.text, "quote");

        let nested = parse_blockquote_line("  >> nested").unwrap();
        assert_eq!(nested.indent, "  ");
        assert_eq!(nested.marker, ">> ");
        assert_eq!(nested.depth, 2);
        assert_eq!(nested.text, "nested");

        let empty = parse_blockquote_line(">").unwrap();
        assert_eq!(empty.marker, ">");
        assert_eq!(empty.text, "");
        assert!(parse_blockquote_line("text > quote").is_none());
    }
}
