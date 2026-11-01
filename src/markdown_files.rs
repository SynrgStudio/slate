//! Small edits to Markdown files on disk: appending a block at the end or
//! under a heading, keeping the file's line endings, written atomically.
use crate::{link_ui, markdown::parse_heading_line, safe_file};
use std::{fs, path::Path};

/// Edits a text file (missing: empty) with `\n` line endings, writes it
/// back with the endings it had, atomically, and returns what was written.
pub(crate) fn edit_text_file(
    path: &Path,
    edit: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<String, String> {
    let original = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.to_string()),
    };
    let crlf = original.contains("\r\n");
    let mut text = if crlf {
        original.replace("\r\n", "\n")
    } else {
        original
    };
    edit(&mut text)?;
    if crlf {
        text = text.replace('\n', "\r\n");
    }
    safe_file::atomic_replace(path, text.as_bytes())?;
    Ok(text)
}

pub(crate) fn append_markdown_block(path: &Path, block: &str) -> Result<(), String> {
    edit_text_file(path, |contents| {
        append_markdown_block_to_string(contents, block);
        Ok(())
    })
    .map(drop)
}

pub(crate) fn append_markdown_block_to_heading(
    path: &Path,
    heading: &link_ui::LinkHeadingOption,
    block: &str,
) -> Result<(), String> {
    edit_text_file(path, |contents| {
        let insert_at = markdown_heading_insert_offset(contents, heading).unwrap_or(contents.len());
        let mut insertion = String::new();
        if insert_at > 0 && !contents[..insert_at].ends_with('\n') {
            insertion.push('\n');
        }
        insertion.push('\n');
        insertion.push_str(block.trim_end());
        insertion.push('\n');
        contents.insert_str(insert_at, &insertion);
        Ok(())
    })
    .map(drop)
}

pub(crate) fn append_markdown_block_to_string(contents: &mut String, block: &str) {
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    if !contents.is_empty() {
        contents.push('\n');
    }
    contents.push_str(block.trim_end());
    contents.push('\n');
}

pub(crate) fn markdown_heading_insert_offset(
    contents: &str,
    heading: &link_ui::LinkHeadingOption,
) -> Option<usize> {
    let heading_line_start = contents
        .lines()
        .take(heading.line_index)
        .map(|line| line.len() + 1)
        .sum::<usize>();
    let search_start = contents[heading_line_start..]
        .find('\n')
        .map(|offset| heading_line_start + offset + 1)
        .unwrap_or(contents.len());
    let mut byte = search_start;
    for line in contents[search_start..].split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches('\n');
        if let Some(next_heading) = parse_heading_line(line_without_newline) {
            if next_heading.level <= heading.level {
                return Some(byte);
            }
        }
        byte += line.len();
    }
    Some(contents.len())
}
