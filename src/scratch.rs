use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Local;
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke};

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

pub(crate) fn format_capture_entry(title: &str, text: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        text.trim_end().to_string()
    } else {
        format!("**{title}**\n\n{}", text.trim_end())
    }
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
