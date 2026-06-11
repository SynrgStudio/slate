use std::path::{Path, PathBuf};

use crate::markdown::parse_heading_line;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkAssistChoice {
    File,
    Web,
}

#[derive(Clone)]
pub(crate) struct LinkHeadingOption {
    pub(crate) level: usize,
    pub(crate) text: String,
    pub(crate) anchor: String,
    pub(crate) line_index: usize,
}

pub(crate) fn is_http_url(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

pub(crate) fn looks_like_bare_web_target(target: &str) -> bool {
    if target.is_empty()
        || target.starts_with("./")
        || target.starts_with("../")
        || target.starts_with('/')
        || target.starts_with('~')
        || target.chars().any(char::is_whitespace)
        || target.contains("://")
    {
        return false;
    }

    let host = target
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_start_matches("www.");
    let Some(tld) = host.rsplit('.').next() else {
        return false;
    };
    host.contains('.')
        && tld.len() >= 2
        && tld.chars().all(|ch| ch.is_ascii_alphabetic())
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        })
}

pub(crate) fn markdown_heading_options(contents: &str) -> Vec<LinkHeadingOption> {
    contents
        .lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let heading = parse_heading_line(line)?;
            let text = heading.text.trim();
            if text.is_empty() {
                return None;
            }
            Some(LinkHeadingOption {
                level: heading.level,
                text: text.to_string(),
                anchor: markdown_heading_anchor(text),
                line_index,
            })
        })
        .collect()
}

pub(crate) fn markdown_heading_anchor(text: &str) -> String {
    let mut anchor = String::new();
    let mut last_was_dash = false;
    for ch in text.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            anchor.push(ch);
            last_was_dash = false;
        } else if ch.is_whitespace() || ch == '-' {
            if !last_was_dash && !anchor.is_empty() {
                anchor.push('-');
                last_was_dash = true;
            }
        }
    }
    anchor.trim_matches('-').to_string()
}

pub(crate) fn markdown_link_target_for_file(path: &Path, current_path: Option<&Path>) -> String {
    let base = current_path
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let raw = path
        .strip_prefix(&base)
        .map(|relative| {
            let relative = relative.display().to_string();
            if relative.starts_with("../") || relative.starts_with("./") {
                relative
            } else {
                format!("./{relative}")
            }
        })
        .unwrap_or_else(|_| path.display().to_string());
    raw.replace('\\', "/")
}

pub(crate) fn resolve_markdown_link_path(target: &str, current_path: Option<&Path>) -> PathBuf {
    let expanded = target
        .strip_prefix("~/")
        .and_then(|rest| dirs_next::home_dir().map(|home| home.join(rest)))
        .unwrap_or_else(|| PathBuf::from(target));
    if expanded.is_absolute() {
        return expanded;
    }

    let base = current_path
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    base.join(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_anchor_matches_markdown_style_slugs() {
        assert_eq!(
            markdown_heading_anchor(" Hello, Slate Flow! "),
            "hello-slate-flow"
        );
        assert_eq!(markdown_heading_anchor("A -- B"), "a-b");
    }

    #[test]
    fn bare_web_targets_are_conservative() {
        assert!(looks_like_bare_web_target("example.com/path"));
        assert!(!looks_like_bare_web_target("notes/today.md"));
        assert!(!looks_like_bare_web_target("http://example.com"));
    }
}
