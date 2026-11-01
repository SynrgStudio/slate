use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::commands::fuzzy_score;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilePickerMode {
    Open,
    OpenBuffer,
    AddLance,
    Browse,
    InsertMarkdownLink,
    SetPeriscopeProject,
    AddVault,
}

pub(crate) fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GB {
        format!("{:.1}G", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1}M", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1}K", bytes_f / KB)
    } else {
        format!("{}B", bytes)
    }
}

pub(crate) fn format_modified_time(modified: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return "now".to_string();
    };
    let seconds = elapsed.as_secs();
    let minute = 60;
    let hour = minute * 60;
    let day = hour * 24;
    let week = day * 7;
    let month = day * 30;
    let year = day * 365;

    if seconds < minute {
        "now".to_string()
    } else if seconds < hour {
        format!("{}m ago", seconds / minute)
    } else if seconds < day {
        format!("{}h ago", seconds / hour)
    } else if seconds < week {
        format!("{}d ago", seconds / day)
    } else if seconds < month {
        format!("{}w ago", seconds / week)
    } else if seconds < year {
        format!("{}mo ago", seconds / month)
    } else {
        format!("{}y ago", seconds / year)
    }
}

pub(crate) fn file_metadata_labels(path: &Path) -> (String, String) {
    let Ok(metadata) = fs::metadata(path) else {
        return ("?".to_string(), "?".to_string());
    };
    let size = format_file_size(metadata.len());
    let modified = metadata
        .modified()
        .map(format_modified_time)
        .unwrap_or_else(|_| "?".to_string());
    (size, modified)
}

pub(crate) fn default_should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | "node_modules"
            | ".threadwell"
            | ".slate"
            | ".idea"
            | ".vscode"
            | "dist"
            | "build"
    )
}

pub(crate) fn scan_directory_entries<F>(dir: &Path, should_skip_dir: F) -> Vec<PathBuf>
where
    F: Fn(&str) -> bool,
{
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                !should_skip_dir(name)
            } else {
                path.is_file()
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|path| (!path.is_dir(), path.file_name().map(|name| name.to_owned())));
    entries
}

pub(crate) fn path_list_indices(paths: &[PathBuf], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0..paths.len()).collect();
    }

    let mut scored = paths
        .iter()
        .enumerate()
        .filter_map(|(index, path)| {
            let display = path.display().to_string();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let display_score = fuzzy_score(&display, &query);
            let name_score = fuzzy_score(file_name, &query).map(|score| score / 2);
            display_score
                .into_iter()
                .chain(name_score)
                .min()
                .map(|score| (score, index))
        })
        .collect::<Vec<_>>();
    scored.sort_by_key(|(score, index)| (*score, *index));
    scored.into_iter().map(|(_, index)| index).collect()
}

pub(crate) fn picker_entry_indices(entries: &[PathBuf], base: &Path, query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut indices = if query.is_empty() {
        (0..entries.len()).collect::<Vec<_>>()
    } else {
        let mut scored = entries
            .iter()
            .enumerate()
            .filter_map(|(index, path)| {
                let relative = path
                    .strip_prefix(base)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                let relative_score = fuzzy_score(&relative, &query);
                let name_score = fuzzy_score(name, &query).map(|score| score / 2);
                relative_score
                    .into_iter()
                    .chain(name_score)
                    .min()
                    .map(|score| (score, index))
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|(score, index)| (*score, *index));
        scored
            .into_iter()
            .map(|(_, index)| index)
            .collect::<Vec<_>>()
    };
    indices.sort_by_key(|index| !entries[*index].is_dir());
    indices
}

pub(crate) fn named_entry_indices(entries: &[PathBuf], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut indices = if query.is_empty() {
        (0..entries.len()).collect::<Vec<_>>()
    } else {
        let mut scored = entries
            .iter()
            .enumerate()
            .filter_map(|(index, path)| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                fuzzy_score(name, &query).map(|score| (score, index))
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|(score, index)| (*score, *index));
        scored
            .into_iter()
            .map(|(_, index)| index)
            .collect::<Vec<_>>()
    };
    indices.sort_by_key(|index| !entries[*index].is_dir());
    indices
}

pub(crate) fn move_selected_index(indices: &[usize], selected: usize, delta: isize) -> usize {
    if indices.is_empty() {
        return 0;
    }
    let current_position = indices
        .iter()
        .position(|index| *index == selected)
        .unwrap_or(0);
    let next_position = current_position
        .saturating_add_signed(delta)
        .min(indices.len().saturating_sub(1));
    indices[next_position]
}

pub(crate) fn centered_window_start(
    selected_position: usize,
    visible_rows: usize,
    total_rows: usize,
) -> usize {
    if total_rows <= visible_rows {
        return 0;
    }
    selected_position
        .saturating_sub(visible_rows / 2)
        .min(total_rows.saturating_sub(visible_rows))
}

/// Column positions relative to the left edge; optional columns disappear on narrow windows.
pub(crate) struct PickerColumns {
    pub name_width: f32,
    pub detail: Option<(f32, f32)>,
    pub size_right: Option<f32>,
    pub modified_right: Option<f32>,
}
impl PickerColumns {
    pub fn new(width: f32, files: bool) -> Self {
        let modified_right = (files && width >= 680.0).then_some(width - 12.0);
        let size_right = files.then_some(
            width
                - if modified_right.is_some() {
                    104.0
                } else {
                    12.0
                },
        );
        let name_end = size_right.map(|x| x - 64.0).unwrap_or(width - 12.0);
        let detail = (width >= 560.0).then(|| {
            let x = width * 0.35;
            (x, (name_end - x - 12.0).max(0.0))
        });
        let name_width = (detail.map(|(x, _)| x).unwrap_or(name_end) - 40.0).max(0.0);
        Self {
            name_width,
            detail,
            size_right,
            modified_right,
        }
    }
}

pub(crate) fn available_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|letter| PathBuf::from(format!("{}:\\", letter as char)))
            .filter(|root| root.is_dir())
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/")]
    }
}

/// Bare Windows drive names mean their root, not a drive-relative working directory.
pub(crate) fn normalize_drive_path(raw: &str) -> String {
    let raw = raw.trim().trim_matches('"');
    if raw.len() == 2 && raw.as_bytes()[0].is_ascii_alphabetic() && raw.as_bytes()[1] == b':' {
        format!("{raw}\\")
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_names_are_absolute_roots() {
        assert_eq!(normalize_drive_path("C:"), "C:\\");
        assert_eq!(
            normalize_drive_path("\"C:\\Program Files\""),
            "C:\\Program Files"
        );
        assert_eq!(normalize_drive_path("notes"), "notes");
    }

    #[test]
    fn picker_columns_fit_small_and_large_windows() {
        for width in [280.0, 336.0, 500.0, 720.0, 1200.0] {
            let cols = PickerColumns::new(width, true);
            assert!(cols.name_width >= 0.0);
            if let Some((x, w)) = cols.detail {
                assert!(x + w <= width);
            }
            assert!(cols.size_right.unwrap() <= width);
            if let Some(x) = cols.modified_right {
                assert!(x <= width);
            }
        }
    }

    #[test]
    fn centered_window_keeps_selection_near_middle() {
        assert_eq!(centered_window_start(0, 5, 20), 0);
        assert_eq!(centered_window_start(10, 5, 20), 8);
        assert_eq!(centered_window_start(19, 5, 20), 15);
    }

    #[test]
    fn move_selected_index_clamps_to_available_indices() {
        let indices = vec![2, 5, 9];
        assert_eq!(move_selected_index(&indices, 5, 1), 9);
        assert_eq!(move_selected_index(&indices, 5, -1), 2);
        assert_eq!(move_selected_index(&indices, 9, 1), 9);
    }
}
