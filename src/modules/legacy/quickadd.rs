// ---- from main.rs: fn flow_inbox_path
    fn flow_inbox_path(&self) -> PathBuf {
        self.flow_scope_root().join("inbox.md")
    }


// ---- from main.rs: fn append_flow_task_to_current_buffer
    fn append_flow_task_to_current_buffer(&mut self, task_line: &str) {
        let needs_leading_newline =
            !self.buffer.as_str().is_empty() && !self.buffer.as_str().ends_with('\n');
        let prefix = if self.buffer.as_str().trim().is_empty() {
            "# Inbox\n\n"
        } else if needs_leading_newline {
            "\n"
        } else {
            ""
        };
        let insertion = format!("{prefix}{task_line}\n");
        self.buffer.set_cursor(self.buffer.as_str().len());
        self.buffer.insert_text(&insertion);
        self.dirty = true;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
    }


// ---- from main.rs: fn quickadd_flow_task
    fn quickadd_flow_task(&mut self, text: &str) -> bool {
        let Some(task_line) = scratch::normalize_quickadd_task_line(text) else {
            self.status = "Quickadd cancelled".to_string();
            return false;
        };
        let path = self.flow_inbox_path();

        if self.is_current_path(&path) {
            self.append_flow_task_to_current_buffer(&task_line);
            if self.agenda_open {
                if let Ok(items) = self.load_agenda_items() {
                    self.agenda_items = items;
                    self.clamp_agenda_selection();
                }
            }
            self.status = format!("Quickadded to current inbox buffer: {task_line}");
            return true;
        }

        if let Some(dir) = path.parent() {
            if let Err(err) = fs::create_dir_all(dir) {
                self.status = format!("Quickadd failed: {err}");
                return false;
            }
        }

        let needs_header = !path.exists()
            || fs::metadata(&path)
                .map(|metadata| metadata.len() == 0)
                .unwrap_or(true);
        // A file that does not end in a newline would glue the task onto
        // its last line.
        let ends_with_newline = || -> std::io::Result<bool> {
            use std::io::{Read, Seek};
            let mut file = fs::File::open(&path)?;
            file.seek(std::io::SeekFrom::End(-1))?;
            let mut last = [0u8];
            file.read_exact(&mut last)?;
            Ok(last[0] == b'\n')
        };
        let entry = if needs_header {
            format!("# Inbox\n\n{task_line}\n")
        } else if ends_with_newline().unwrap_or(true) {
            format!("{task_line}\n")
        } else {
            format!("\n{task_line}\n")
        };

        match fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => {
                if let Err(err) = file.write_all(entry.as_bytes()) {
                    self.status = format!("Quickadd failed: {err}");
                    return false;
                }
            }
            Err(err) => {
                self.status = format!("Quickadd failed: {err}");
                return false;
            }
        }

        match fs::read_to_string(&path)
            .map_err(|err| err.to_string())
            .and_then(|contents| {
                self.refresh_flow_index_for_path(&path, &contents)
                    .map(|_| ())
            }) {
            Ok(()) => {
                if self.agenda_open {
                    if let Ok(items) = self.load_agenda_items() {
                        self.agenda_items = items;
                        self.clamp_agenda_selection();
                    }
                }
                self.status = format!("Quickadded to {}", path.display());
                true
            }
            Err(err) => {
                self.status = format!("Quickadded, Flow index failed: {err}");
                true
            }
        }
    }

// ---- from scratch.rs: fn normalize_quickadd_task_line
pub(crate) fn normalize_quickadd_task_line(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(parsed) = parse_checkbox_line(text) {
        if parsed.text.trim().is_empty() {
            return None;
        }
        let mut line = text.to_string();
        append_default_task_tags(&mut line, parsed.text);
        return Some(line);
    }
    let mut line = format!("- [ ] {text}");
    append_default_task_tags(&mut line, text);
    Some(line)
}


// ---- from scratch.rs: fn append_default_task_tags
fn append_default_task_tags(line: &mut String, body: &str) {
    let has_extra = body.split_whitespace().any(|word| {
        word.starts_with('#')
            || word.starts_with('@')
            || matches!(
                word.to_ascii_uppercase().as_str(),
                "ALTA" | "MEDIA" | "BAJA"
            )
            || word.starts_with("due:")
            || word.starts_with("scheduled:")
    });
    let default_tags = if has_extra {
        ["task"].as_slice()
    } else {
        ["task", "inbox"].as_slice()
    };
    let missing_tags = default_tags
        .iter()
        .filter(|tag| {
            !line
                .split_whitespace()
                .any(|word| word.eq_ignore_ascii_case(&format!("#{tag}")))
        })
        .collect::<Vec<_>>();
    if missing_tags.is_empty() {
        return;
    }
    line.push(' ');
    line.push_str(
        &missing_tags
            .into_iter()
            .map(|tag| format!("#{tag}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
}


// ---- from scratch.rs: fn split_quickadds
/// Splits scratch text into Flow quickadds (`:qa`, `:quickadd` or `:inbox`
/// lines, and checkbox lines) and the rest. Each quickadd keeps its original
/// line, so one that fails can be put back.
pub(crate) fn split_quickadds(text: &str) -> (Vec<(String, String)>, String) {
    let mut quickadds = Vec::new();
    let mut remaining = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(':') {
            let rest = rest.trim_start();
            // Any other line starting with ':' is just text.
            let task = ["quickadd ", "qa ", "inbox "]
                .iter()
                .find_map(|command| rest.strip_prefix(command))
                .map(str::trim);
            if let Some(task) = task.filter(|task| !task.is_empty()) {
                quickadds.push((task.to_string(), line.to_string()));
                continue;
            }
        }
        if parse_checkbox_line(trimmed).is_some_and(|checkbox| !checkbox.text.trim().is_empty()) {
            quickadds.push((trimmed.to_string(), line.to_string()));
            continue;
        }
        remaining.push_str(line);
        remaining.push('\n');
    }
    (quickadds, remaining.trim_end().to_string())
}


// ---- from scratch.rs: fn scratch_split_quickadds_extracts_colon_commands
    #[test]
    fn scratch_split_quickadds_extracts_colon_commands() {
        let (quickadds, remaining) = split_quickadds(
            "nota\n:quickadd comprar pan #casa\n: inbox llamar\n[] testeo @proyectocasa\nfin",
        );
        assert_eq!(
            quickadds
                .iter()
                .map(|(task, _)| task.as_str())
                .collect::<Vec<_>>(),
            vec!["comprar pan #casa", "llamar", "[] testeo @proyectocasa"]
        );
        assert_eq!(
            quickadds[1].1, ": inbox llamar",
            "the original line is kept"
        );
        assert_eq!(remaining, "nota\nfin");
        // Other lines that start with ':' are text, not tasks.
        let (quickadds, remaining) = split_quickadds(":) hola\n:qa\n:note: idea\nhttp://x");
        assert!(quickadds.is_empty());
        assert_eq!(remaining, ":) hola\n:qa\n:note: idea\nhttp://x");
    }


// ---- from scratch.rs: fn quickadd_normalization_adds_flow_default_tags
    #[test]
    fn quickadd_normalization_adds_flow_default_tags() {
        assert_eq!(
            normalize_quickadd_task_line("comprar pan"),
            Some("- [ ] comprar pan #task #inbox".to_string())
        );
        assert_eq!(
            normalize_quickadd_task_line("comprar pan @casa"),
            Some("- [ ] comprar pan @casa #task".to_string())
        );
        assert_eq!(
            normalize_quickadd_task_line("- [ ] ya tagueada #task"),
            Some("- [ ] ya tagueada #task".to_string())
        );
        assert_eq!(
            normalize_quickadd_task_line("[] testeo @proyectocasa"),
            Some("[] testeo @proyectocasa #task".to_string())
        );
    }

