//! Vaults: folders the user declares as note collections (Settings →
//! Modules & vaults, `config.toml` `vault = "..."` lines). One answer to
//! "which notes belong together" for every feature that looks beyond the
//! open file.
use std::path::{Path, PathBuf};

/// The vault holding `path`: the innermost declared vault containing it,
/// else the nearest folder with `.git`, else the file's own folder.
pub(crate) fn vault_for(vaults: &[PathBuf], path: &Path) -> PathBuf {
    if let Some(vault) = declared(vaults, path) {
        return vault;
    }
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let folder = path.parent().map(Path::to_path_buf).unwrap_or(path.clone());
    folder
        .ancestors()
        .find(|dir| dir.join(".git").is_dir())
        .map(Path::to_path_buf)
        .unwrap_or(folder)
}

/// The innermost declared vault holding `path`, if any.
pub(crate) fn declared(vaults: &[PathBuf], path: &Path) -> Option<PathBuf> {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let key = |p: &Path| {
        let text = p.to_string_lossy().replace('\\', "/");
        let text = text.trim_end_matches('/').to_string();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    let path_key = key(&path);
    vaults
        .iter()
        .filter(|vault| {
            let vault_key = key(vault);
            path_key == vault_key || path_key.starts_with(&format!("{vault_key}/"))
        })
        .max_by_key(|vault| key(vault).len())
        .cloned()
}

/// The Markdown notes under `dir` (skipping build and tool folders), at most
/// `max`. True as second value when the walk stopped at `max`.
pub(crate) fn notes(dir: &Path, max: usize) -> (Vec<PathBuf>, bool) {
    fn walk(dir: &Path, max: usize, out: &mut Vec<PathBuf>) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            if out.len() >= max {
                return true;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                let name = entry.file_name();
                if !crate::file_dialogs::default_should_skip_dir(&name.to_string_lossy())
                    && walk(&path, max, out)
                {
                    return true;
                }
            } else if kind.is_file() && is_note(&path) {
                out.push(path);
            }
        }
        false
    }
    let mut out = Vec::new();
    let truncated = walk(dir, max, &mut out);
    (out, truncated)
}

/// A Markdown file, by extension.
pub(crate) fn is_note(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd"
            )
        })
}

/// Adds `folder` unless it (or the same folder spelled differently) is
/// already there. True when added.
pub(crate) fn add(vaults: &mut Vec<PathBuf>, folder: PathBuf) -> bool {
    let folder = std::path::absolute(&folder).unwrap_or(folder);
    if vaults
        .iter()
        .any(|vault| crate::tabs::same_file(vault, &folder))
    {
        return false;
    }
    vaults.push(folder);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_innermost_declared_vault_wins_then_git_then_the_folder() {
        let root = std::env::temp_dir().join(format!("slate-vaults-{}", crate::test_unique()));
        let notes = root.join("notes");
        let work = notes.join("work");
        let repo = root.join("repo");
        for dir in [
            &work,
            &repo.join(".git"),
            &repo.join("src"),
            &root.join("loose"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let vaults = vec![notes.clone(), work.clone()];
        assert_eq!(vault_for(&vaults, &notes.join("a.md")), notes);
        assert_eq!(vault_for(&vaults, &work.join("deep/b.md")), work);
        assert_eq!(vault_for(&vaults, &repo.join("src/c.md")), repo);
        assert_eq!(
            vault_for(&vaults, &root.join("loose/d.md")),
            root.join("loose")
        );
        // "notes-old" is not inside "notes".
        assert_eq!(
            vault_for(&vaults, &root.join("notes-old/e.md")),
            root.join("notes-old")
        );
        let mut list = vaults.clone();
        assert!(!add(&mut list, notes.join(".")));
        assert!(add(&mut list, repo.clone()));
        let _ = std::fs::remove_dir_all(root);
    }
}
