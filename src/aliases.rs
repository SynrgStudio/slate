//! Aliases: display names for files, shown only inside Slate.
//!
//! The file keeps its real name on disk. An alias is stored per absolute path
//! in `aliases.json` next to the settings; renaming or moving the file outside
//! Slate drops it. Tabs, the window title, Lance and recent files show the
//! alias, and Periscope finds files by alias.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    path: PathBuf,
    alias: String,
}

#[derive(Default)]
pub(crate) struct Aliases {
    entries: BTreeMap<String, Entry>,
}

/// Same identity rule as tabs: absolute, `/`, case-insensitive on Windows.
fn key(path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

impl Aliases {
    pub(crate) fn load(file: Option<&Path>) -> Self {
        let entries = file
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice::<Vec<Entry>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| !entry.alias.trim().is_empty())
            .map(|entry| (key(&entry.path), entry))
            .collect();
        Self { entries }
    }

    pub(crate) fn get(&self, path: &Path) -> Option<&str> {
        self.entries
            .get(&key(path))
            .map(|entry| entry.alias.as_str())
    }

    /// Sets (or with `None`/blank, removes) the alias and saves the list.
    pub(crate) fn set(
        &mut self,
        file: Option<&Path>,
        path: &Path,
        alias: Option<&str>,
    ) -> Result<(), String> {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        match alias.map(str::trim).filter(|alias| !alias.is_empty()) {
            Some(alias) => {
                self.entries.insert(
                    key(&path),
                    Entry {
                        path,
                        alias: alias.to_string(),
                    },
                );
            }
            None => {
                self.entries.remove(&key(&path));
            }
        }
        let Some(file) = file else {
            return Err("no settings folder".into());
        };
        let list: Vec<&Entry> = self.entries.values().collect();
        let json = serde_json::to_vec_pretty(&list).map_err(|err| err.to_string())?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        crate::safe_file::atomic_replace(file, &json)
    }

    /// Existing files whose alias contains every word of `query`.
    pub(crate) fn matching(&self, query: &str) -> Vec<PathBuf> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(|word| word.to_lowercase())
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        self.entries
            .values()
            .filter(|entry| {
                let alias = entry.alias.to_lowercase();
                words.iter().all(|word| alias.contains(word.as_str()))
            })
            .filter(|entry| entry.path.is_file())
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// `alias · file.md`, or just the file name when there is no alias.
    pub(crate) fn labeled(&self, path: &Path) -> String {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        match self.get(path) {
            Some(alias) => format!("{alias} · {name}"),
            None => name.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_persist_match_by_words_and_are_removed_when_blank() {
        let dir = std::env::temp_dir().join(format!("slate-aliases-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("aliases.json");
        let note = dir.join("2026-notas-reunion.md");
        std::fs::write(&note, "x").unwrap();

        let mut aliases = Aliases::default();
        aliases
            .set(Some(&file), &note, Some("  Reunión semanal  "))
            .unwrap();
        assert_eq!(aliases.get(&note), Some("Reunión semanal"));
        assert_eq!(
            aliases.labeled(&note),
            "Reunión semanal · 2026-notas-reunion.md"
        );

        let loaded = Aliases::load(Some(&file));
        assert_eq!(loaded.get(&note), Some("Reunión semanal"));
        assert_eq!(
            loaded.matching("semanal reu"),
            vec![std::path::absolute(&note).unwrap()]
        );
        assert!(loaded.matching("mensual").is_empty());
        assert!(loaded.matching("").is_empty());

        aliases.set(Some(&file), &note, Some("   ")).unwrap();
        assert_eq!(Aliases::load(Some(&file)).get(&note), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
