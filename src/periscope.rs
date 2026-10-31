use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::commands::fuzzy_score_lower;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeriscopeMode {
    Project,
    Global,
}

#[derive(Clone)]
pub(crate) enum PeriscopeRow {
    Folder {
        path: PathBuf,
        expanded: bool,
    },
    File {
        path: PathBuf,
        group: Option<PathBuf>,
    },
}

pub(crate) fn query_tokens(query: &str) -> Vec<&str> {
    query
        .split(|ch: char| ch.is_whitespace() || ch == '/' || ch == '\\')
        .filter(|token| !token.is_empty())
        .collect()
}

pub(crate) fn path_score_lower(
    path_lower: &str,
    name_lower: &str,
    query_lower: &str,
) -> Option<usize> {
    let tokens = query_tokens(query_lower);
    if tokens.len() <= 1 {
        let query = query_lower;
        let components = path_lower
            .split('/')
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();

        if name_lower == query {
            return Some(0);
        }

        let mut best_component = None;
        for (component_index, component) in components.iter().enumerate() {
            if *component == query {
                let tail_depth = components.len().saturating_sub(component_index + 1);
                best_component = Some(
                    best_component
                        .unwrap_or(usize::MAX)
                        .min(20 + component_index * 16 + tail_depth * 4 + name_lower.len()),
                );
            }
        }
        if best_component.is_some() {
            return best_component;
        }

        if name_lower.starts_with(query) {
            return Some(120 + name_lower.len().saturating_sub(query.len()));
        }

        for (component_index, component) in components.iter().enumerate() {
            if component.starts_with(query) {
                let tail_depth = components.len().saturating_sub(component_index + 1);
                best_component = Some(best_component.unwrap_or(usize::MAX).min(
                    220 + component_index * 16
                        + tail_depth * 4
                        + component.len().saturating_sub(query.len()),
                ));
            }
        }
        if best_component.is_some() {
            return best_component;
        }

        if name_lower.contains(query) {
            return Some(420 + name_lower.find(query).unwrap_or(0));
        }
        if let Some(position) = path_lower.find(query) {
            return Some(620 + position + path_lower.len().saturating_sub(query.len()));
        }

        let path_score = fuzzy_score_lower(path_lower, query).map(|score| score + 2_000);
        let name_score = fuzzy_score_lower(name_lower, query).map(|score| score + 1_500);
        return path_score.into_iter().chain(name_score).min();
    }

    if name_lower.starts_with(query_lower) {
        return Some(120 + name_lower.len().saturating_sub(query_lower.len()));
    }
    if tokens.iter().all(|token| name_lower.contains(token)) {
        return Some(200 + name_lower.len().saturating_sub(query_lower.len()));
    }
    if tokens.iter().all(|token| path_lower.contains(token)) {
        return Some(600 + path_lower.len().saturating_sub(query_lower.len()));
    }
    let joined = tokens.concat();
    let fuzzy_fallback = || {
        fuzzy_score_lower(name_lower, &joined)
            .map(|score| score + 1500)
            .into_iter()
            .chain(fuzzy_score_lower(path_lower, &joined).map(|score| score + 2000))
            .min()
    };
    let mut search_start = 0usize;
    let mut previous_end = 0usize;
    let mut score = 0usize;
    for token in tokens {
        let mut found_position = None;
        let mut offset = search_start;
        while let Some(relative) = path_lower[offset..].find(token) {
            let position = offset + relative;
            let at_boundary = position == 0
                || matches!(
                    path_lower.as_bytes().get(position.saturating_sub(1)),
                    Some(b'/') | Some(b'-') | Some(b'_') | Some(b'.')
                )
                || path_lower[..position]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace);
            if at_boundary {
                found_position = Some(position);
                break;
            }
            offset = position + path_lower[position..].chars().next().unwrap().len_utf8();
        }
        let Some(position) = found_position else {
            return fuzzy_fallback();
        };
        let gap = position.saturating_sub(previous_end);
        score += gap;
        search_start = position + token.len();
        previous_end = search_start;
    }
    Some(score + path_lower.len().saturating_sub(query_lower.len()))
}

pub(crate) fn rank_native_paths(
    mut paths: Vec<PathBuf>,
    query: &str,
    limit: usize,
) -> Vec<PathBuf> {
    let query = query.trim().to_lowercase();
    let terms = query.split_whitespace().collect::<Vec<_>>();
    paths.sort_by_cached_key(|path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let score = if name == query {
            0
        } else if name.starts_with(&query) {
            1
        } else if terms.iter().all(|term| name.contains(term)) {
            2
        } else {
            3
        };
        (score, name, path.clone())
    });
    paths.truncate(limit);
    paths
}
pub(crate) fn rank_paths(
    mut paths: Vec<PathBuf>,
    root: &Path,
    query: &str,
    limit: usize,
) -> Vec<PathBuf> {
    let query = query.trim().to_lowercase().replace('\\', "/");
    if query.is_empty() {
        paths.sort();
        paths.truncate(limit);
        return paths;
    }
    let mut scored = paths
        .into_iter()
        .filter_map(|path| {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let relative_lower = relative.to_lowercase().replace('\\', "/");
            let name_lower = name.to_lowercase();
            path_score_lower(&relative_lower, &name_lower, &query).map(|score| (score, path))
        })
        .collect::<Vec<_>>();
    scored.sort_by_key(|(score, path)| (*score, path.clone()));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, path)| path)
        .collect()
}

pub(crate) fn group_folder_for_path(query: &str, path: &Path) -> Option<PathBuf> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let tokens = query_tokens(&query);
    let target = tokens.last().copied()?;
    if target.is_empty() {
        return None;
    }
    let components = path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let text = component.as_os_str().to_string_lossy().to_lowercase();
        if text == target || text.starts_with(target) {
            let mut folder = PathBuf::new();
            for component in components.iter().take(index + 1) {
                folder.push(component.as_os_str());
            }
            if folder != path && path.starts_with(&folder) {
                return Some(folder);
            }
        }
    }
    None
}

pub(crate) fn rows(
    results: &[PathBuf],
    query: &str,
    expanded_folders: &HashSet<PathBuf>,
) -> Vec<PeriscopeRow> {
    let mut rows = Vec::new();
    let mut seen_folders = HashSet::new();
    for path in results {
        if let Some(folder) = group_folder_for_path(query, path) {
            if seen_folders.insert(folder.clone()) {
                let expanded = expanded_folders.contains(&folder);
                rows.push(PeriscopeRow::Folder {
                    path: folder.clone(),
                    expanded,
                });
            }
            if expanded_folders.contains(&folder) {
                rows.push(PeriscopeRow::File {
                    path: path.clone(),
                    group: Some(folder),
                });
            }
        } else {
            rows.push(PeriscopeRow::File {
                path: path.clone(),
                group: None,
            });
        }
    }
    rows
}

pub(crate) fn selected_row(
    results: &[PathBuf],
    query: &str,
    expanded_folders: &HashSet<PathBuf>,
    selected: usize,
) -> Option<PeriscopeRow> {
    rows(results, query, expanded_folders)
        .get(selected)
        .cloned()
}

pub(crate) fn toggle_folder(
    expanded_folders: &mut HashSet<PathBuf>,
    row: Option<PeriscopeRow>,
) -> bool {
    let Some(PeriscopeRow::Folder { path, .. }) = row else {
        return false;
    };
    if !expanded_folders.remove(&path) {
        expanded_folders.insert(path);
    }
    true
}

pub(crate) fn move_selection(row_count: usize, selected: usize, delta: isize) -> usize {
    if row_count == 0 {
        return 0;
    }
    selected
        .saturating_add_signed(delta)
        .min(row_count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_word_order_is_not_filtered_out_and_filenames_rank_before_path_only_matches() {
        let paths = vec![
            PathBuf::from("C:/Desktop/Abrir Miniaturas Lychee.cmd"),
            PathBuf::from("C:/abrir/miniatures/other.cmd"),
        ];
        let ranked = rank_native_paths(paths, "mi abrir", 100);
        assert_eq!(ranked.len(), 2);
        assert!(ranked[0].to_string_lossy().contains("Lychee"));
        assert!(
            path_score_lower(
                "c:/desktop/abrir miniaturas lychee.cmd",
                "abrir miniaturas lychee.cmd",
                "mi abrir"
            )
            .is_some()
        );
    }
    #[test]
    fn spaced_and_abbreviated_command_names_match_without_executing_them() {
        assert!(
            path_score_lower(
                "c:/users/test/desktop/abrir minis.cmd",
                "abrir minis.cmd",
                "abrir minis"
            )
            .is_some()
        );
        assert!(
            path_score_lower(
                "c:/users/test/desktop/abrir miniaturas lychee.cmd",
                "abrir miniaturas lychee.cmd",
                "abrir minis"
            )
            .is_some()
        );
        assert!(
            path_score_lower(
                "c:/desktop/éé notes test.cmd",
                "éé notes test.cmd",
                "é notes"
            )
            .is_some()
        );
        let paths = rank_paths(
            vec![PathBuf::from("C:/Desktop/Abrir Miniaturas Lychee.cmd")],
            Path::new("C:/"),
            "Abrir minis",
            100,
        );
        assert_eq!(paths.len(), 1);
    }
    #[test]
    fn path_score_prefers_exact_file_name() {
        let exact = path_score_lower("notes/project/readme.md", "readme.md", "readme.md").unwrap();
        let fuzzy = path_score_lower("notes/project/roadmap.md", "roadmap.md", "rdm").unwrap();
        assert!(exact < fuzzy);
    }

    #[test]
    fn rows_group_matching_folders_until_expanded() {
        let path = PathBuf::from("/home/me/projects/slate/src/main.rs");
        let collapsed = rows(std::slice::from_ref(&path), "proj", &HashSet::new());
        assert!(matches!(collapsed[0], PeriscopeRow::Folder { .. }));
        let mut expanded = HashSet::new();
        expanded.insert(PathBuf::from("/home/me/projects"));
        let expanded_rows = rows(&[path], "proj", &expanded);
        assert_eq!(expanded_rows.len(), 2);
    }
}
