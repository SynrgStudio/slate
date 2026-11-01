//! References: which notes link to the open file (`:backlinks`) or mention
//! its name (`:mentions`). The search runs on its own thread over the
//! file's vault; the results open in the core's result list.
use crate::commands::{Command, CommandSpec};
use crate::module_api::{Cx, DocumentRef, Module, ResultItem};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "backlinks",
        aliases: &["bl"],
        summary: "Notes that link to the current file",
        hint: ":backlinks",
        palette_command: Some(Command::Module("backlinks")),
    },
    CommandSpec {
        name: "mentions",
        aliases: &["refs"],
        summary: "Notes that mention the current file's name",
        hint: ":mentions",
        palette_command: Some(Command::Module("mentions")),
    },
];

/// Notes looked at in one search, at most.
const MAX_FILES: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Backlinks,
    Mentions,
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Kind::Backlinks => "Backlinks",
            Kind::Mentions => "Mentions",
        }
    }
}

struct Finished {
    search: u64,
    kind: Kind,
    target: PathBuf,
    items: Vec<ResultItem>,
}

#[derive(Default)]
pub(crate) struct References {
    /// The newest search; older results are dropped.
    search: u64,
    done: Arc<Mutex<Option<Finished>>>,
}

impl Module for References {
    fn id(&self) -> &'static str {
        "references"
    }

    fn name(&self) -> &'static str {
        "Backlinks & mentions"
    }

    fn description(&self) -> &'static str {
        ":backlinks and :mentions list the notes in the vault that link to or name the open file"
    }

    fn commands(&self) -> &'static [CommandSpec] {
        COMMANDS
    }

    fn run(&mut self, cx: &mut Cx, command: &str, _args: &str, doc: Option<DocumentRef>) {
        let kind = if command == "mentions" {
            Kind::Mentions
        } else {
            Kind::Backlinks
        };
        let Some(target) = doc.and_then(|doc| doc.path).map(Path::to_path_buf) else {
            cx.status(format!("{} need a saved file", kind.title()));
            return;
        };
        let vault = cx.vault_for(&target);
        self.search += 1;
        let search = self.search;
        let done = self.done.clone();
        let waker = cx.waker();
        cx.status(format!("{}: searching {}…", kind.title(), vault.display()));
        std::thread::spawn(move || {
            let items = find(kind, &vault, &target);
            if let Ok(mut slot) = done.lock() {
                *slot = Some(Finished {
                    search,
                    kind,
                    target,
                    items,
                });
            }
            waker.wake();
        });
    }

    fn wake(&mut self, cx: &mut Cx) {
        let Some(finished) = self.done.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        if finished.search != self.search {
            return;
        }
        let name = finished
            .target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        cx.status(format!(
            "{} of {name}: {}",
            finished.kind.title(),
            finished.items.len()
        ));
        cx.show_results(
            format!("{} · {name}", finished.kind.title()),
            finished.items,
        );
    }
}

/// Every line in the vault's notes that links to (or names) `target`.
fn find(kind: Kind, vault: &Path, target: &Path) -> Vec<ResultItem> {
    let mut files = Vec::new();
    collect_notes(vault, &mut files);
    let target_abs = normalize(target);
    let names = mention_names(target);
    let found = crate::parallel::par_map(&files, |file| {
        if normalize(file) == target_abs {
            return Vec::new();
        }
        let Ok(text) = fs::read_to_string(file) else {
            return Vec::new();
        };
        text.lines()
            .enumerate()
            .filter(|(_, line)| match kind {
                Kind::Backlinks => links_to(line, file, &target_abs),
                Kind::Mentions => mentions(line, &names),
            })
            .map(|(index, line)| ResultItem {
                path: file.clone(),
                line: Some(index),
                label: format!(
                    "{}:{}",
                    file.strip_prefix(vault).unwrap_or(file).display(),
                    index + 1
                ),
                detail: line.trim().chars().take(160).collect(),
            })
            .collect::<Vec<_>>()
    });
    found.into_iter().flatten().collect()
}

fn collect_notes(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let name = entry.file_name();
            if !crate::file_dialogs::default_should_skip_dir(&name.to_string_lossy()) {
                collect_notes(&path, out);
            }
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown"))
        {
            out.push(path);
        }
    }
}

fn normalize(path: &Path) -> String {
    let path = fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// A Markdown link `[..](path)` or a wiki link `[[name]]` to `target`.
fn links_to(line: &str, source: &Path, target_abs: &str) -> bool {
    let markdown = crate::markdown::parse_markdown_link_spans(line)
        .into_iter()
        .any(|span| {
            let link = &line[span.target_start..span.target_end];
            if crate::link_ui::is_http_url(link) {
                return false;
            }
            let file = link.split_once('#').map_or(link, |(file, _)| file);
            !file.trim().is_empty()
                && normalize(&crate::link_ui::resolve_markdown_link_path(
                    file,
                    Some(source),
                )) == target_abs
        });
    markdown || wiki_links(line).any(|name| target_abs.ends_with(&format!("/{name}.md")))
}

/// Lower-cased names inside `[[...]]` (without `|alias` or `#heading`).
fn wiki_links(line: &str) -> impl Iterator<Item = String> + '_ {
    line.split("[[").skip(1).filter_map(|rest| {
        let inner = rest.split_once("]]")?.0;
        let name = inner.split(['|', '#']).next()?.trim();
        (!name.is_empty()).then(|| name.to_lowercase())
    })
}

/// File name and stem of `target`, lower-cased, for mentions.
fn mention_names(target: &Path) -> Vec<String> {
    [target.file_name(), target.file_stem()]
        .into_iter()
        .flatten()
        .map(|name| name.to_string_lossy().to_lowercase())
        .filter(|name| name.chars().count() >= 3)
        .collect()
}

fn mentions(line: &str, names: &[String]) -> bool {
    let lower = line.to_lowercase();
    names.iter().any(|name| lower.contains(name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_markdown_and_wiki_links_and_mentions_in_the_vault() {
        let root = std::env::temp_dir().join(format!("slate-references-{}", crate::test_unique()));
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(root.join("node_modules")).unwrap();
        let target = root.join("Árbol.md");
        fs::write(&target, "# Árbol\nlinks to [self](Árbol.md)\n").unwrap();
        fs::write(
            root.join("a.md"),
            "intro\nsee [the tree](Árbol.md#raíz)\n[web](https://example.com/Árbol.md)\n",
        )
        .unwrap();
        fs::write(sub.join("b.md"), "x\n\n[[árbol|alias]] and more\n").unwrap();
        fs::write(sub.join("c.md"), "nothing here\nhablo del ÁRBOL sin link\n").unwrap();
        fs::write(root.join("node_modules/d.md"), "[x](../Árbol.md)\n").unwrap();

        let labels = |kind| {
            let mut found: Vec<_> = find(kind, &root, &target)
                .into_iter()
                .map(|item| {
                    (
                        item.path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                        item.line,
                    )
                })
                .collect();
            found.sort();
            found
        };
        assert_eq!(
            labels(Kind::Backlinks),
            vec![("a.md".into(), Some(1)), ("b.md".into(), Some(2))],
            "own file, web links and skipped folders are not backlinks"
        );
        assert_eq!(
            labels(Kind::Mentions),
            vec![
                ("a.md".into(), Some(1)),
                ("a.md".into(), Some(2)),
                ("b.md".into(), Some(2)),
                ("c.md".into(), Some(1)),
            ]
        );
        let _ = fs::remove_dir_all(root);
    }
}
