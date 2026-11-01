#![allow(dead_code)]

pub(crate) mod batch;
pub(crate) mod edit;
pub(crate) mod index_export;
pub(crate) mod runtime;
pub(crate) mod store;
pub(crate) mod worker;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use crate::markdown::{
    CheckboxState, parse_checkbox_line, parse_fenced_code_marker, parse_heading_line,
    parse_list_line, parse_markdown_link_spans,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlowPriority {
    Alta,
    Media,
    Baja,
}

impl FlowPriority {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FlowPriority::Alta => "ALTA",
            FlowPriority::Media => "MEDIA",
            FlowPriority::Baja => "BAJA",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlowItemKind {
    Task,
    Note,
    Idea,
    Inbox,
}

impl FlowItemKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FlowItemKind::Task => "task",
            FlowItemKind::Note => "note",
            FlowItemKind::Idea => "idea",
            FlowItemKind::Inbox => "inbox",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FlowTaskMetadata {
    pub(crate) due: Option<String>,
    pub(crate) scheduled: Option<String>,
    pub(crate) related: Vec<String>,
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlowTask {
    pub(crate) kind: FlowItemKind,
    pub(crate) line_index: usize,
    pub(crate) line_number: usize,
    pub(crate) indent: usize,
    pub(crate) state: CheckboxState,
    pub(crate) raw_text: String,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) projects: Vec<String>,
    pub(crate) priority: Option<FlowPriority>,
    pub(crate) heading: Option<String>,
    pub(crate) metadata: FlowTaskMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlowKnownFile {
    pub(crate) path: String,
    pub(crate) last_seen_unix: u64,
    pub(crate) mtime_unix: u64,
    pub(crate) task_count: usize,
    pub(crate) has_tasks: bool,
    pub(crate) has_links: bool,
    pub(crate) has_tags: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlowIndexSummary {
    pub(crate) project_root: PathBuf,
    pub(crate) index_path: PathBuf,
    pub(crate) sqlite_path: PathBuf,
    pub(crate) known_file: FlowKnownFile,
    pub(crate) tasks: Vec<FlowTask>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenTask {
    task_index: usize,
    indent: usize,
}

pub(crate) fn parse_flow_tasks(document: &str) -> Vec<FlowTask> {
    let mut tasks: Vec<FlowTask> = Vec::new();
    let mut open_tasks: Vec<OpenTask> = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut in_fenced_code = false;

    for (line_index, line) in document.lines().enumerate() {
        if parse_fenced_code_marker(line).is_some() {
            in_fenced_code = !in_fenced_code;
            continue;
        }
        if in_fenced_code {
            continue;
        }

        if let Some(heading) = parse_heading_line(line) {
            heading_stack.retain(|(level, _)| *level < heading.level);
            heading_stack.push((heading.level, heading.text.trim().to_string()));
        }

        let indent = leading_whitespace_len(line);
        while open_tasks
            .last()
            .is_some_and(|open_task| indent <= open_task.indent && !line.trim().is_empty())
        {
            open_tasks.pop();
        }

        if let Some((key, value)) = parse_task_metadata_line(line) {
            if let Some(parent) = open_tasks
                .iter()
                .rev()
                .find(|open_task| indent > open_task.indent)
            {
                apply_metadata(&mut tasks[parent.task_index].metadata, key, value);
                continue;
            }
        }

        if let Some(task) = parse_flow_task_line(
            line,
            line_index,
            heading_stack.last().map(|(_, heading)| heading.clone()),
        )
        .or_else(|| {
            parse_flow_note_or_idea_line(
                line,
                line_index,
                heading_stack.last().map(|(_, heading)| heading.clone()),
            )
        }) {
            let task_index = tasks.len();
            open_tasks.push(OpenTask {
                task_index,
                indent: task.indent,
            });
            tasks.push(task);
        }
    }

    tasks
}

pub(crate) fn parse_flow_task_line(
    line: &str,
    line_index: usize,
    heading: Option<String>,
) -> Option<FlowTask> {
    let checkbox = parse_checkbox_line(line)?;
    let parsed_text = parse_flow_item_text(checkbox.text);
    if parsed_text.title.trim().is_empty() {
        return None;
    }
    Some(FlowTask {
        kind: FlowItemKind::Task,
        line_index,
        line_number: line_index + 1,
        indent: checkbox.indent.len(),
        state: checkbox.state,
        raw_text: checkbox.text.to_string(),
        title: parsed_text.title,
        tags: parsed_text.tags,
        projects: parsed_text.projects,
        priority: parsed_text.priority,
        heading,
        metadata: FlowTaskMetadata::default(),
    })
}

pub(crate) fn parse_flow_note_or_idea_line(
    line: &str,
    line_index: usize,
    heading: Option<String>,
) -> Option<FlowTask> {
    let trimmed = line.trim_start();
    let indent = line.len().saturating_sub(trimmed.len());
    let (kind, text, marker_includes_space) = if let Some(text) = trimmed.strip_prefix(":note") {
        (FlowItemKind::Note, text, false)
    } else if let Some(text) = trimmed.strip_prefix(":nota") {
        (FlowItemKind::Note, text, false)
    } else if let Some(text) = trimmed.strip_prefix(":idea") {
        (FlowItemKind::Idea, text, false)
    } else if let Some(text) = trimmed.strip_prefix(": ") {
        (FlowItemKind::Inbox, text, true)
    } else {
        return None;
    };
    if !marker_includes_space && !text.is_empty() && !text.starts_with(char::is_whitespace) {
        return None;
    }
    let text = text.trim_start();
    let parsed_text = parse_flow_item_text(text);
    if parsed_text.title.trim().is_empty() {
        return None;
    }
    Some(FlowTask {
        kind,
        line_index,
        line_number: line_index + 1,
        indent,
        state: CheckboxState::Empty,
        raw_text: text.to_string(),
        title: parsed_text.title,
        tags: parsed_text.tags,
        projects: parsed_text.projects,
        priority: parsed_text.priority,
        heading,
        metadata: FlowTaskMetadata::default(),
    })
}

pub(crate) fn should_index_flow_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd"
            )
        })
        .unwrap_or(false)
}

static INDEX_WRITE_LOCK: Mutex<()> = Mutex::new(());
static NEXT_TEMP_INDEX: AtomicU64 = AtomicU64::new(1);

pub(crate) fn update_flow_index_for_file(
    project_root: &Path,
    sqlite_path: &Path,
    path: &Path,
    contents: &str,
) -> Result<FlowIndexSummary, String> {
    let tasks = parse_flow_tasks(contents);
    let known_file = build_known_file(project_root, path, contents, &tasks);
    write_sqlite_cache(sqlite_path, &known_file, &tasks, contents)?;
    let index_path = project_root.join(".slate").join("index.tsv");
    index_export::request(project_root, sqlite_path);

    Ok(FlowIndexSummary {
        project_root: project_root.to_path_buf(),
        index_path,
        sqlite_path: sqlite_path.to_path_buf(),
        known_file,
        tasks,
    })
}

pub(crate) fn build_known_file(
    project_root: &Path,
    path: &Path,
    contents: &str,
    tasks: &[FlowTask],
) -> FlowKnownFile {
    let relative_path = format_index_path(project_root, path);
    let last_seen_unix = unix_now();
    let mtime_unix = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(system_time_to_unix)
        .unwrap_or(last_seen_unix);
    let has_links = contents
        .lines()
        .any(|line| !parse_markdown_link_spans(line).is_empty());
    let has_tags = tasks.iter().any(|task| !task.tags.is_empty());

    FlowKnownFile {
        path: relative_path,
        last_seen_unix,
        mtime_unix,
        task_count: tasks.len(),
        has_tasks: !tasks.is_empty(),
        has_links,
        has_tags,
    }
}

#[cfg(test)]
fn write_project_index_entry(
    project_root: &Path,
    known_file: FlowKnownFile,
) -> Result<PathBuf, String> {
    let _guard = INDEX_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let slate_dir = project_root.join(".slate");
    fs::create_dir_all(&slate_dir).map_err(|err| err.to_string())?;
    let index_path = slate_dir.join("index.tsv");
    let mut entries = read_project_index_entries(&index_path);
    entries.insert(known_file.path.clone(), known_file);
    write_project_index_entries(&index_path, &entries)?;
    Ok(index_path)
}

#[cfg(test)]
fn read_project_index_entries(index_path: &Path) -> BTreeMap<String, FlowKnownFile> {
    let Ok(contents) = fs::read_to_string(index_path) else {
        return BTreeMap::new();
    };
    contents
        .lines()
        .filter_map(parse_project_index_entry)
        .map(|entry| (entry.path.clone(), entry))
        .collect()
}

fn write_project_index_entries(
    index_path: &Path,
    entries: &BTreeMap<String, FlowKnownFile>,
) -> Result<(), String> {
    let tmp_path = index_path.with_extension(format!(
        "tsv.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP_INDEX.fetch_add(1, Ordering::Relaxed)
    ));
    let mut contents = String::from(
        "# Slate Flow known files index\n\
         # Derived/rebuildable. Markdown files remain the source of truth.\n\
         # path\tlast_seen_unix\tmtime_unix\ttask_count\thas_tasks\thas_links\thas_tags\n",
    );
    for entry in entries.values() {
        contents.push_str(&format_project_index_entry(entry));
    }
    fs::write(&tmp_path, contents).map_err(|err| err.to_string())?;
    fs::rename(&tmp_path, index_path).map_err(|err| err.to_string())
}

#[cfg(test)]
fn parse_project_index_entry(line: &str) -> Option<FlowKnownFile> {
    if line.trim().is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split('\t');
    let path = fields.next()?.to_string();
    Some(FlowKnownFile {
        path,
        last_seen_unix: fields.next()?.parse().ok()?,
        mtime_unix: fields.next()?.parse().ok()?,
        task_count: fields.next()?.parse().ok()?,
        has_tasks: parse_bool_field(fields.next()?),
        has_links: parse_bool_field(fields.next()?),
        has_tags: parse_bool_field(fields.next()?),
    })
}

fn format_project_index_entry(entry: &FlowKnownFile) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        entry.path,
        entry.last_seen_unix,
        entry.mtime_unix,
        entry.task_count,
        bool_field(entry.has_tasks),
        bool_field(entry.has_links),
        bool_field(entry.has_tags),
    )
}

fn write_sqlite_cache(
    sqlite_path: &Path,
    known_file: &FlowKnownFile,
    tasks: &[FlowTask],
    contents: &str,
) -> Result<(), String> {
    let document = crate::modules::flow::store::Document {
        known_file: known_file.clone(),
        tasks: tasks.to_vec(),
        contents: contents.to_string(),
    };
    crate::modules::flow::store::with_writer(sqlite_path, |writer| writer.write_batch(&[document]))
}

/// Export the rebuildable TSV once after a scan, from committed DB rows rather
/// than a stale snapshot. Serialize exporters with individual UI updates.
pub(crate) fn export_cache_index(
    root: &Path,
    writer: &crate::modules::flow::store::CacheWriter,
) -> Result<(), String> {
    let _guard = INDEX_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let dir = root.join(".slate");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let entries = writer
        .known_files()?
        .into_iter()
        .map(|file| (file.path.clone(), file))
        .collect();
    write_project_index_entries(&dir.join("index.tsv"), &entries)
}

pub(crate) fn export_database_index(root: &Path, database: &Path) -> Result<(), String> {
    let _guard = INDEX_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let entries = crate::modules::flow::store::read_known_files(database)?
        .into_iter()
        .map(|file| (file.path.clone(), file))
        .collect();
    let dir = root.join(".slate");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    write_project_index_entries(&dir.join("index.tsv"), &entries)
}

pub(crate) fn flow_item_state_label(item: &FlowTask) -> &'static str {
    match item.kind {
        FlowItemKind::Task => checkbox_state_label(item.state),
        FlowItemKind::Note => "note",
        FlowItemKind::Idea => "idea",
        FlowItemKind::Inbox => "inbox",
    }
}

fn checkbox_state_label(state: CheckboxState) -> &'static str {
    match state {
        CheckboxState::Empty => "todo",
        CheckboxState::Doing => "doing",
        CheckboxState::Done => "done",
    }
}

fn bool_field(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
fn parse_bool_field(value: &str) -> bool {
    matches!(value, "yes" | "true" | "1")
}

fn format_index_path(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn unix_now() -> u64 {
    system_time_to_unix(SystemTime::now()).unwrap_or(0)
}

fn system_time_to_unix(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedFlowItemText {
    title: String,
    tags: Vec<String>,
    projects: Vec<String>,
    priority: Option<FlowPriority>,
}

fn parse_flow_item_text(text: &str) -> ParsedFlowItemText {
    let mut title_tokens = Vec::new();
    let mut tags = Vec::new();
    let mut projects = Vec::new();
    let mut priority = None;

    for token in text.split_whitespace() {
        let trimmed = trim_token_punctuation(token);
        if let Some(tag) = parse_tag_token(trimmed) {
            tags.push(tag.to_string());
        } else if let Some(project) = parse_project_token(trimmed) {
            projects.push(project.to_string());
        } else if let Some(parsed_priority) = parse_priority_token(trimmed) {
            priority = Some(parsed_priority);
        } else {
            title_tokens.push(token);
        }
    }

    ParsedFlowItemText {
        title: title_tokens.join(" "),
        tags,
        projects,
        priority,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MetadataKey {
    Due,
    Scheduled,
    Related,
    Note,
}

fn parse_task_metadata_line(line: &str) -> Option<(MetadataKey, String)> {
    let trimmed = line.trim_start();
    let metadata_text = if let Some(checkbox) = parse_checkbox_line(line) {
        checkbox.text
    } else if let Some(list) = parse_list_line(line) {
        list.text
    } else {
        trimmed
    };

    let (key, value) = metadata_text.split_once(':')?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let key = match key.trim().to_ascii_lowercase().as_str() {
        "due" => MetadataKey::Due,
        "scheduled" => MetadataKey::Scheduled,
        "related" => MetadataKey::Related,
        "note" | "notes" => MetadataKey::Note,
        _ => return None,
    };

    Some((key, value.to_string()))
}

fn apply_metadata(metadata: &mut FlowTaskMetadata, key: MetadataKey, value: String) {
    match key {
        MetadataKey::Due => metadata.due = Some(value),
        MetadataKey::Scheduled => metadata.scheduled = Some(value),
        MetadataKey::Related => metadata.related.push(value),
        MetadataKey::Note => metadata.notes.push(value),
    }
}

fn parse_tag_token(token: &str) -> Option<&str> {
    let tag = token.strip_prefix('#')?;
    is_identifier(tag).then_some(tag)
}

fn parse_project_token(token: &str) -> Option<&str> {
    let project = token.strip_prefix('@')?;
    is_identifier(project).then_some(project)
}

fn parse_priority_token(token: &str) -> Option<FlowPriority> {
    match token {
        "ALTA" => Some(FlowPriority::Alta),
        "MEDIA" => Some(FlowPriority::Media),
        "BAJA" => Some(FlowPriority::Baja),
        _ => None,
    }
}

fn trim_token_punctuation(token: &str) -> &str {
    token.trim_matches(|ch: char| matches!(ch, ',' | ';' | '.'))
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/'))
}

fn leading_whitespace_len(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_task_state_text_tags_project_and_priority() {
        let task = parse_flow_task_line(
            "- [ ] Arreglar selección con mouse #editor #input @slate ALTA",
            6,
            Some("Editor".to_string()),
        )
        .unwrap();

        assert_eq!(task.line_index, 6);
        assert_eq!(task.line_number, 7);
        assert_eq!(task.indent, 0);
        assert_eq!(task.state, CheckboxState::Empty);
        assert_eq!(
            task.raw_text,
            "Arreglar selección con mouse #editor #input @slate ALTA"
        );
        assert_eq!(task.title, "Arreglar selección con mouse");
        assert_eq!(task.tags, vec!["editor", "input"]);
        assert_eq!(task.projects, vec!["slate"]);
        assert_eq!(task.priority, Some(FlowPriority::Alta));
        assert_eq!(task.heading.as_deref(), Some("Editor"));
    }

    #[test]
    fn parses_doing_and_done_states() {
        let doing = parse_flow_task_line("  - [/] Revisar agenda @slate MEDIA", 0, None).unwrap();
        let done = parse_flow_task_line("- [x] Documentar Flow #flow BAJA", 1, None).unwrap();

        assert_eq!(doing.indent, 2);
        assert_eq!(doing.state, CheckboxState::Doing);
        assert_eq!(doing.projects, vec!["slate"]);
        assert_eq!(doing.priority, Some(FlowPriority::Media));
        assert_eq!(done.state, CheckboxState::Done);
        assert_eq!(done.tags, vec!["flow"]);
        assert_eq!(done.priority, Some(FlowPriority::Baja));
    }

    #[test]
    fn parses_note_and_idea_items_without_bullets() {
        let document = "# Inbox\n\n:note Revisar calendario #flow @slate\n:nota Nota en castellano #flow @slate\n:idea Modal de agenda con tabs #ui @slate MEDIA\n";
        let items = parse_flow_tasks(document);

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, FlowItemKind::Note);
        assert_eq!(items[0].title, "Revisar calendario");
        assert_eq!(items[0].tags, vec!["flow"]);
        assert_eq!(items[0].projects, vec!["slate"]);
        assert_eq!(items[0].heading.as_deref(), Some("Inbox"));
        assert_eq!(items[1].kind, FlowItemKind::Note);
        assert_eq!(items[1].title, "Nota en castellano");
        assert_eq!(items[2].kind, FlowItemKind::Idea);
        assert_eq!(items[2].title, "Modal de agenda con tabs");
        assert_eq!(items[2].priority, Some(FlowPriority::Media));
    }

    #[test]
    fn ignores_empty_checkbox_lines_as_agenda_tasks() {
        assert!(parse_flow_task_line("[]", 0, None).is_none());
        assert!(parse_flow_task_line("- [ ]", 1, None).is_none());
        assert!(parse_flow_task_line("  [x]   ", 2, None).is_none());

        let document = "- [ ] Comprar pan\n[]\n- [ ]   \n";
        let tasks = parse_flow_tasks(document);

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Comprar pan");
    }

    #[test]
    fn attaches_child_metadata_to_parent_task() {
        let document = "# Slate Flow\n\n- [ ] Arreglar selección con mouse #editor @slate ALTA\n  - due: 2026-06-10\n  - scheduled: 2026-06-08\n  - related: [Periscope](README.md#periscope)\n  - note: revisar Unicode\n- [ ] Otra tarea\n";

        let tasks = parse_flow_tasks(document);

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].heading.as_deref(), Some("Slate Flow"));
        assert_eq!(tasks[0].metadata.due.as_deref(), Some("2026-06-10"));
        assert_eq!(tasks[0].metadata.scheduled.as_deref(), Some("2026-06-08"));
        assert_eq!(
            tasks[0].metadata.related,
            vec!["[Periscope](README.md#periscope)"]
        );
        assert_eq!(tasks[0].metadata.notes, vec!["revisar Unicode"]);
        assert_eq!(tasks[1].title, "Otra tarea");
        assert_eq!(tasks[1].metadata, FlowTaskMetadata::default());
    }

    #[test]
    fn treats_checkbox_metadata_as_parent_metadata_not_agenda_task() {
        let document = "- [ ] Preparar release @slate ALTA\n  - [ ] due: 2026-06-10\n  - [ ] scheduled: 2026-06-08\n  - [ ] revisar changelog #release\n";

        let tasks = parse_flow_tasks(document);

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Preparar release");
        assert_eq!(tasks[0].metadata.due.as_deref(), Some("2026-06-10"));
        assert_eq!(tasks[0].metadata.scheduled.as_deref(), Some("2026-06-08"));
        assert_eq!(tasks[1].title, "revisar changelog");
        assert_eq!(tasks[1].tags, vec!["release"]);
    }

    #[test]
    fn attaches_metadata_to_nearest_lower_indent_task() {
        let document = "- [ ] Parent @slate\n  - [ ] Child #subtask\n    - due: 2026-06-12\n  - note: parent note\n";

        let tasks = parse_flow_tasks(document);

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Parent");
        assert_eq!(tasks[0].metadata.notes, vec!["parent note"]);
        assert_eq!(tasks[0].metadata.due, None);
        assert_eq!(tasks[1].title, "Child");
        assert_eq!(tasks[1].metadata.due.as_deref(), Some("2026-06-12"));
    }

    #[test]
    fn keeps_unknown_colon_lines_as_tasks_when_they_are_checkboxes() {
        let document = "- [ ] Parent\n  - [ ] blocker: revisar parser\n";

        let tasks = parse_flow_tasks(document);

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1].title, "blocker: revisar parser");
    }

    #[test]
    fn accepts_identifier_punctuation_without_leaking_it_into_tags() {
        let task = parse_flow_task_line("- [ ] Test parser #flow, @slate. MEDIA", 0, None).unwrap();

        assert_eq!(task.title, "Test parser");
        assert_eq!(task.tags, vec!["flow"]);
        assert_eq!(task.projects, vec!["slate"]);
        assert_eq!(task.priority, Some(FlowPriority::Media));
    }

    #[test]
    fn only_indexes_markdown_like_paths() {
        assert!(should_index_flow_path(Path::new("notes.md")));
        assert!(should_index_flow_path(Path::new("notes.markdown")));
        assert!(!should_index_flow_path(Path::new("main.rs")));
        assert!(!should_index_flow_path(Path::new("README")));
    }

    #[test]
    fn writes_readable_project_index_entries() {
        let root = temp_test_dir("slate-flow-index");
        let path = root.join("notes.md");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            &path,
            "- [ ] First task #flow @slate ALTA\n  - due: 2026-06-10\n[link](README.md)\n",
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let tasks = parse_flow_tasks(&contents);
        let known_file = build_known_file(&root, &path, &contents, &tasks);
        let index_path = write_project_index_entry(&root, known_file).unwrap();
        let index = fs::read_to_string(index_path).unwrap();

        assert!(index.contains("# Slate Flow known files index"));
        assert!(index.contains("notes.md\t"));
        assert!(index.contains("\t1\tyes\tyes\tyes"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn update_flow_index_writes_readable_index_and_sqlite_cache() {
        let root = temp_test_dir("slate-flow-cache");
        let path = root.join("notes.md");
        let sqlite_path = root.join("agenda.db");
        fs::create_dir_all(&root).unwrap();
        let contents =
            "# Project\n\n- [ ] Cache this task #flow @slate ALTA\n  - due: 2026-06-10\n";
        fs::write(&path, contents).unwrap();

        let summary = update_flow_index_for_file(&root, &sqlite_path, &path, contents).unwrap();

        assert_eq!(summary.known_file.path, "notes.md");
        assert_eq!(summary.tasks.len(), 1);
        index_export::flush();
        assert!(summary.index_path.exists());
        assert!(sqlite_path.exists());

        let rows = crate::modules::flow::store::read_tasks(&sqlite_path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            format!("{}|{}|{}", rows[0].title, rows[0].due, rows[0].priority),
            "Cache this task|2026-06-10|ALTA"
        );

        let _ = fs::remove_dir_all(root);
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let unique = format!("{}-{}-{}", name, std::process::id(), unix_now());
        std::env::temp_dir().join(unique)
    }
}
