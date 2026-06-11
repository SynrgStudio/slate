use std::{
    collections::{BinaryHeap, HashSet},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use crate::commands::fuzzy_score_lower;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeriscopeMode {
    Project,
    Global,
}

pub(crate) struct PeriscopeIndexedPath {
    pub(crate) path: PathBuf,
    pub(crate) lower: String,
    pub(crate) is_file: bool,
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

pub(crate) fn command_available(name: &str) -> bool {
    ProcessCommand::new("sh")
        .arg("-lc")
        .arg(format!("command -v {} >/dev/null 2>&1", name))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn collect_project_files(root: &Path, out: &mut Vec<PathBuf>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if out.len() >= limit {
            return;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if name == ".git" {
                continue;
            }
            collect_project_files(&path, out, limit);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

pub(crate) fn project_results(root: &Path, query: &str, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if command_available("fd") {
        if let Ok(output) = ProcessCommand::new("fd")
            .args(["--type", "f", "--hidden", "--no-ignore", "."])
            .arg(root)
            .output()
        {
            if output.status.success() {
                files = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .take(20_000)
                    .map(PathBuf::from)
                    .collect();
            }
        }
    }
    if files.is_empty() {
        collect_project_files(root, &mut files, 20_000);
    }
    rank_paths(files, root, query, limit)
}

pub(crate) fn load_global_index(home: &Path) -> Vec<PeriscopeIndexedPath> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    if command_available("fd") {
        if let Ok(output) = ProcessCommand::new("fd")
            .args(["--type", "f", "."])
            .arg(home)
            .output()
        {
            if output.status.success() {
                files = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(PathBuf::from)
                    .collect();
            }
        }
    }
    if files.is_empty() {
        collect_project_files(home, &mut files, 200_000);
    }
    files
        .into_iter()
        .filter_map(|path| {
            let display = path.display().to_string();
            if !seen.insert(display.clone()) {
                return None;
            }
            Some(PeriscopeIndexedPath {
                path,
                lower: display.to_lowercase(),
                is_file: true,
            })
        })
        .collect()
}

pub(crate) fn query_tokens(query: &str) -> Vec<&str> {
    query
        .split(|ch: char| ch.is_whitespace() || ch == '/')
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
                );
            if at_boundary {
                found_position = Some(position);
                break;
            }
            offset = position + 1;
        }
        let position = found_position?;
        let gap = position.saturating_sub(previous_end);
        score += gap;
        search_start = position + token.len();
        previous_end = search_start;
    }
    Some(score + path_lower.len().saturating_sub(query_lower.len()))
}

pub(crate) fn cached_results(
    paths: &[PeriscopeIndexedPath],
    query: &str,
    limit: usize,
) -> Vec<PathBuf> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut best: BinaryHeap<(usize, usize)> = BinaryHeap::new();
    for (index, path) in paths.iter().enumerate() {
        if !path.is_file {
            continue;
        }
        let name_lower = path
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_lowercase();
        let Some(score) = path_score_lower(&path.lower, &name_lower, &query) else {
            continue;
        };
        let candidate = (score, index);
        if best.len() < limit {
            best.push(candidate);
        } else if best.peek().is_some_and(|worst| candidate < *worst) {
            best.pop();
            best.push(candidate);
        }
    }
    let mut scored = best.into_vec();
    scored.sort_by_key(|(score, index)| (*score, *index));
    scored
        .into_iter()
        .map(|(_, index)| paths[index].path.clone())
        .collect()
}

pub(crate) fn rank_paths(
    mut paths: Vec<PathBuf>,
    root: &Path,
    query: &str,
    limit: usize,
) -> Vec<PathBuf> {
    let query = query.trim().to_lowercase();
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
            let relative_lower = relative.to_lowercase();
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
