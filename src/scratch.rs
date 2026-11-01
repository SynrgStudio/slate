use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Local;
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke};

use crate::markdown::parse_checkbox_line;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScratchEntry {
    pub(crate) heading: Option<String>,
    pub(crate) body: String,
}

impl ScratchEntry {
    pub(crate) fn title(&self) -> String {
        self.heading
            .clone()
            .unwrap_or_else(|| "untitled scratch".to_string())
    }

    pub(crate) fn preview(&self) -> String {
        self.body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .chars()
            .take(80)
            .collect()
    }
}

pub(crate) fn archive_path() -> Option<PathBuf> {
    let mut dir = dirs_next::data_dir()?;
    dir.push("slate");
    Some(dir.join("scratch.md"))
}

pub(crate) fn feedback_toast(ctx: &egui::Context, until: Option<f64>, message: &str) {
    let Some(until) = until else {
        return;
    };
    if ctx.input(|input| input.time) >= until {
        return;
    }
    egui::Area::new("scratch_feedback_toast".into())
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(25, 31, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(136, 192, 208)))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(message)
                            .font(FontId::new(15.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(163, 190, 140)),
                    );
                });
        });
}

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

pub(crate) fn format_capture_entry(title: &str, text: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        text.trim_end().to_string()
    } else {
        format!("**{title}**\n\n{}", text.trim_end())
    }
}

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

pub(crate) fn append_text_to_archive(path: &Path, text: &str) -> Result<(), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("Scratch append failed: {err}"))?;
    }
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    let needs_header = !path.exists()
        || fs::metadata(path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(true);
    let entry = if needs_header {
        format!("# Scratch\n\n## {now}\n\n{trimmed}\n")
    } else {
        format!("\n\n## {now}\n\n{trimmed}\n")
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("Scratch append failed: {err}"))?;
    file.write_all(entry.as_bytes())
        .map_err(|err| format!("Scratch append failed: {err}"))
}

/// An entry starts at the `## <date time>` heading the archive writes (or
/// the "untitled scratch" one): a `## ` heading the user wrote inside an
/// entry is part of it.
fn is_entry_heading(text: &str) -> bool {
    let text = text.trim();
    text == "untitled scratch"
        || chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").is_ok()
}

pub(crate) fn parse_entries(text: &str) -> Vec<ScratchEntry> {
    let mut entries = Vec::new();
    let mut heading: Option<String> = None;
    let mut body = String::new();
    let mut seen_entry = false;

    let flush =
        |entries: &mut Vec<ScratchEntry>, heading: &mut Option<String>, body: &mut String| {
            if heading.is_some() || !body.trim().is_empty() {
                entries.push(ScratchEntry {
                    heading: heading.take(),
                    body: body.trim_matches('\n').to_string(),
                });
                body.clear();
            }
        };

    for line in text.lines() {
        if line.trim() == "# Scratch" && !seen_entry && heading.is_none() && body.trim().is_empty()
        {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("## ")
            .filter(|rest| is_entry_heading(rest))
        {
            flush(&mut entries, &mut heading, &mut body);
            heading = Some(rest.trim().to_string());
            seen_entry = true;
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&mut entries, &mut heading, &mut body);
    entries
}

pub(crate) fn serialize_entries(entries: &[ScratchEntry]) -> String {
    let mut text = "# Scratch\n".to_string();
    for entry in entries {
        text.push('\n');
        text.push_str("## ");
        text.push_str(entry.heading.as_deref().unwrap_or("untitled scratch"));
        text.push_str("\n\n");
        text.push_str(entry.body.trim_end());
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn scratch_entries_round_trip() {
        let entries = parse_entries(
            "# Scratch\n\n## 2026-11-01 10:00:00\n\ntexto\n## Mis ideas\nmas\n\n## 2026-11-01 11:30:05\n\notra",
        );
        assert_eq!(
            entries.len(),
            2,
            "a heading inside an entry does not split it"
        );
        assert_eq!(entries[0].title(), "2026-11-01 10:00:00");
        assert_eq!(entries[0].body, "texto\n## Mis ideas\nmas");
        assert_eq!(entries[1].body, "otra");
        assert_eq!(parse_entries(&serialize_entries(&entries)), entries);
    }

    #[test]
    fn capture_entry_uses_optional_title() {
        assert_eq!(format_capture_entry("", "texto\n"), "texto");
        assert_eq!(format_capture_entry("ctx", "texto\n"), "**ctx**\n\ntexto");
    }
}
