use std::{fs, path::PathBuf};

use chrono::{Datelike, Local, NaiveDate, Timelike};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DailyOpenTarget {
    Today,
    Yesterday,
    Date(NaiveDate),
    Refresh,
}

pub(crate) fn parse_open_target(raw: &str) -> Result<DailyOpenTarget, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("today")
        || trimmed.eq_ignore_ascii_case("hoy")
    {
        return Ok(DailyOpenTarget::Today);
    }
    if trimmed.eq_ignore_ascii_case("yesterday") || trimmed.eq_ignore_ascii_case("ayer") {
        return Ok(DailyOpenTarget::Yesterday);
    }
    if trimmed.eq_ignore_ascii_case("refresh") || trimmed.eq_ignore_ascii_case("rebuild") {
        return Ok(DailyOpenTarget::Refresh);
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(DailyOpenTarget::Date)
        .map_err(|_| format!("Unknown daily target: {trimmed}"))
}

pub(crate) fn resolve_open_date(target: &DailyOpenTarget) -> NaiveDate {
    let today = Local::now().date_naive();
    match target {
        DailyOpenTarget::Today | DailyOpenTarget::Refresh => today,
        DailyOpenTarget::Yesterday => today.pred_opt().unwrap_or(today),
        DailyOpenTarget::Date(date) => *date,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum DailyEventKind {
    Captured,
    Completed,
    Organized,
}

impl DailyEventKind {
    fn section(self) -> &'static str {
        match self {
            Self::Captured => "Captured",
            Self::Completed => "Completed",
            Self::Organized => "Organized",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Captured => "captured",
            Self::Completed => "done",
            Self::Organized => "organized",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DailyEvent {
    pub(crate) kind: DailyEventKind,
    pub(crate) date: NaiveDate,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) projects: Vec<String>,
    pub(crate) source: String,
    pub(crate) source_link: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) target_link: Option<String>,
}

impl DailyEvent {
    pub(crate) fn now(
        kind: DailyEventKind,
        title: String,
        tags: Vec<String>,
        projects: Vec<String>,
        source: String,
        source_link: Option<String>,
    ) -> Self {
        let now = Local::now();
        Self {
            kind,
            date: now.date_naive(),
            hour: now.hour(),
            minute: now.minute(),
            title,
            tags,
            projects,
            source,
            source_link,
            target: None,
            target_link: None,
        }
    }

    pub(crate) fn with_target(mut self, target: String, target_link: Option<String>) -> Self {
        self.target = Some(target);
        self.target_link = target_link;
        self
    }
}

pub(crate) fn daily_dir() -> Result<PathBuf, String> {
    let Some(mut dir) = dirs_next::data_dir() else {
        return Err("Daily log failed: no data dir".to_string());
    };
    dir.push("slate");
    dir.push("daily");
    Ok(dir)
}

pub(crate) fn monthly_path_for(date: NaiveDate) -> Result<PathBuf, String> {
    let mut path = daily_dir()?;
    path.push(format!("{:04}-{:02}.md", date.year(), date.month()));
    Ok(path)
}

pub(crate) fn ensure_monthly_document(date: NaiveDate) -> Result<PathBuf, String> {
    let path = monthly_path_for(date)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("Daily log mkdir failed: {err}"))?;
    }
    let current = fs::read_to_string(&path).unwrap_or_default();
    let next = ensure_day_in_document(&current, date);
    fs::write(&path, next).map_err(|err| format!("Daily log write failed: {err}"))?;
    Ok(path)
}

pub(crate) fn append_event(event: &DailyEvent) -> Result<PathBuf, String> {
    let path = ensure_monthly_document(event.date)?;
    let current = fs::read_to_string(&path).unwrap_or_default();
    let next = append_event_to_document(&current, event);
    fs::write(&path, next).map_err(|err| format!("Daily log write failed: {err}"))?;
    Ok(path)
}

pub(crate) fn ensure_day_in_document(document: &str, date: NaiveDate) -> String {
    let month_title = format!("# Slate Daily Log — {:04}-{:02}", date.year(), date.month());
    let day_heading = format!("## {date}");
    let mut doc = document.trim_end().to_string();
    if doc.is_empty() {
        doc.push_str(&month_title);
        doc.push_str("\n\n");
        doc.push_str(&day_heading);
        doc.push('\n');
        return doc;
    }
    if !doc.starts_with("# Slate Daily Log") {
        doc = format!("{month_title}\n\n{doc}");
    }
    if !doc.contains(&day_heading) {
        doc.push_str("\n\n");
        doc.push_str(&day_heading);
        doc.push('\n');
    }
    doc
}

pub(crate) fn day_heading_byte(document: &str, date: NaiveDate) -> Option<usize> {
    document.find(&format!("## {date}"))
}

pub(crate) fn append_event_to_document(document: &str, event: &DailyEvent) -> String {
    let month_title = format!(
        "# Slate Daily Log — {:04}-{:02}",
        event.date.year(),
        event.date.month()
    );
    let day_heading = format!("## {}", event.date);
    let section_heading = format!("### {}", event.kind.section());
    let entry = render_event(event);

    if document.contains(&entry) {
        return document.to_string();
    }

    let mut doc = document.trim_end().to_string();
    if doc.is_empty() {
        doc.push_str(&month_title);
        doc.push('\n');
    } else if !doc.starts_with("# Slate Daily Log") {
        doc = format!("{month_title}\n\n{doc}");
    }

    if !doc.contains(&day_heading) {
        doc.push_str("\n\n");
        doc.push_str(&day_heading);
        doc.push_str("\n\n");
        doc.push_str(&section_heading);
        doc.push('\n');
        doc.push_str(&entry);
        return doc;
    }

    if let Some(insert_at) = section_insert_offset(&doc, &day_heading, &section_heading) {
        let prefix = if insert_at > 0 && !doc[..insert_at].ends_with('\n') {
            "\n"
        } else {
            ""
        };
        doc.insert_str(insert_at, &format!("{prefix}{entry}"));
        return doc;
    }

    if let Some(insert_at) = day_insert_offset(&doc, &day_heading) {
        let block = format!("\n{section_heading}\n{entry}");
        doc.insert_str(insert_at, &block);
        return doc;
    }

    doc.push_str("\n\n");
    doc.push_str(&day_heading);
    doc.push_str("\n\n");
    doc.push_str(&section_heading);
    doc.push('\n');
    doc.push_str(&entry);
    doc
}

fn render_event(event: &DailyEvent) -> String {
    let mut metadata = String::new();
    for tag in &event.tags {
        metadata.push(' ');
        metadata.push('#');
        metadata.push_str(tag.trim_start_matches('#'));
    }
    for project in &event.projects {
        metadata.push(' ');
        metadata.push('@');
        metadata.push_str(project.trim_start_matches('@'));
    }
    let source = render_reference(&event.source, event.source_link.as_deref());
    if event.kind == DailyEventKind::Organized {
        if let Some(target) = event
            .target
            .as_deref()
            .filter(|target| !target.trim().is_empty())
        {
            let target = render_reference(target, event.target_link.as_deref());
            return format!(
                "- {:02}:{:02} refile — {}{}\n  - from: {}\n  - to: {}\n",
                event.hour,
                event.minute,
                event.title.trim(),
                metadata,
                source,
                target
            );
        }
    }
    format!(
        "- {:02}:{:02} {} — {}{}\n  - source: {}\n",
        event.hour,
        event.minute,
        event.kind.label(),
        event.title.trim(),
        metadata,
        source
    )
}

fn render_reference(label: &str, link: Option<&str>) -> String {
    if let Some(link) = link.filter(|link| !link.trim().is_empty()) {
        format!("[{}]({})", label, link)
    } else {
        format!("`{}`", label)
    }
}

fn section_insert_offset(
    document: &str,
    day_heading: &str,
    section_heading: &str,
) -> Option<usize> {
    let day_start = document.find(day_heading)?;
    let day_body_start = day_start + day_heading.len();
    let next_day = document[day_body_start..]
        .find("\n## ")
        .map(|offset| day_body_start + offset)
        .unwrap_or(document.len());
    let section_relative = document[day_body_start..next_day].find(section_heading)?;
    let section_start = day_body_start + section_relative;
    let section_body_start = section_start + section_heading.len();
    let next_section = document[section_body_start..next_day]
        .find("\n### ")
        .map(|offset| section_body_start + offset)
        .unwrap_or(next_day);
    Some(next_section)
}

fn day_insert_offset(document: &str, day_heading: &str) -> Option<usize> {
    let day_start = document.find(day_heading)?;
    let day_body_start = day_start + day_heading.len();
    Some(
        document[day_body_start..]
            .find("\n## ")
            .map(|offset| day_body_start + offset)
            .unwrap_or(document.len()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: DailyEventKind, title: &str) -> DailyEvent {
        DailyEvent {
            kind,
            date: NaiveDate::from_ymd_opt(2026, 6, 10).unwrap(),
            hour: 9,
            minute: 30,
            title: title.to_string(),
            tags: vec!["task".to_string()],
            projects: vec!["slate".to_string()],
            source: "inbox.md:4".to_string(),
            source_link: Some("/tmp/slate/inbox.md#L4".to_string()),
            target: None,
            target_link: None,
        }
    }

    #[test]
    fn appends_event_to_empty_monthly_document() {
        let doc = append_event_to_document("", &event(DailyEventKind::Captured, "probar daily"));
        assert!(doc.contains("# Slate Daily Log — 2026-06"));
        assert!(doc.contains("## 2026-06-10"));
        assert!(doc.contains("### Captured"));
        assert!(doc.contains("- 09:30 captured — probar daily #task @slate"));
    }

    #[test]
    fn groups_events_by_existing_day_and_section() {
        let doc = append_event_to_document("", &event(DailyEventKind::Captured, "uno"));
        let doc = append_event_to_document(&doc, &event(DailyEventKind::Completed, "uno"));
        assert!(doc.contains("### Captured"));
        assert!(doc.contains("### Completed"));
    }

    #[test]
    fn does_not_duplicate_same_event() {
        let doc = append_event_to_document("", &event(DailyEventKind::Captured, "uno"));
        let doc = append_event_to_document(&doc, &event(DailyEventKind::Captured, "uno"));
        assert_eq!(doc.matches("captured — uno").count(), 1);
    }

    #[test]
    fn ensures_day_without_creating_capture_surface() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 11).unwrap();
        let doc = ensure_day_in_document("", date);
        assert!(doc.contains("# Slate Daily Log — 2026-06"));
        assert!(doc.contains("## 2026-06-11"));
        assert!(!doc.contains("### Captured"));
    }

    #[test]
    fn parses_navigation_targets() {
        assert_eq!(parse_open_target("").unwrap(), DailyOpenTarget::Today);
        assert_eq!(parse_open_target("today").unwrap(), DailyOpenTarget::Today);
        assert_eq!(
            parse_open_target("yesterday").unwrap(),
            DailyOpenTarget::Yesterday
        );
        assert_eq!(
            parse_open_target("refresh").unwrap(),
            DailyOpenTarget::Refresh
        );
        assert_eq!(
            parse_open_target("2026-06-10").unwrap(),
            DailyOpenTarget::Date(NaiveDate::from_ymd_opt(2026, 6, 10).unwrap())
        );
        assert!(parse_open_target("nope").is_err());
    }
}
