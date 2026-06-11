mod agenda;
mod commands;
mod daily;
mod editor_buffer;
mod editor_view;
mod file_dialogs;
mod flow;
mod flow_edit;
mod flow_runtime;
mod goto;
mod link_ui;
mod markdown;
mod periscope;
mod scratch;
mod search;

use std::{
    collections::HashSet,
    fs,
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use agenda::{AgendaDatedRow, AgendaFocus, AgendaItem, AgendaTab};
use chrono::{Datelike, Duration, Local, NaiveDate};
use commands::{
    COMMAND_SPECS, Command, CommandLineAction, CommandLineNumberMode, CommandSpec, CommandUsage,
    best_command_token, clamp_history_limit, command_name, command_prefix, fuzzy_score,
    parse_command_line,
};
use daily::{DailyEvent, DailyEventKind};
use editor_buffer::EditorBuffer;
use editor_view::{EditorView, LineNumberMode};
use eframe::egui::{
    self, Color32, FontFamily, FontId, Key, RichText, Stroke, TextEdit, Vec2,
    text::{LayoutJob, LayoutSection, TextFormat},
};
use file_dialogs::FilePickerMode;
use flow_edit::{
    FlowItemEditState, FlowOrgMode as AgendaOrgMode, org_query_at_cursor, org_token_span_at_cursor,
    organization_suggestions, organization_token, replace_org_token_at_cursor,
};
use goto::GotoTarget;
use link_ui::{LinkAssistChoice, LinkHeadingOption};
use markdown::{
    CheckboxState, TableAlignment, is_markdown_separator, is_markdown_table_start,
    markdown_link_target_at_byte, parse_blockquote_line, parse_checkbox_line,
    parse_fenced_code_marker, parse_heading_line, parse_inline_code_spans, parse_list_line,
    parse_markdown_link_spans, parse_markdown_table_separator, split_markdown_table_row,
};
use periscope::{PeriscopeIndexedPath, PeriscopeMode, PeriscopeRow};
use scratch::ScratchEntry;
use search::SearchState;

const PERISCOPE_RESULT_LIMIT: usize = 100;
const DEFAULT_CURSOR_SIZE: &str = "24";

fn normalize_cursor_environment() {
    if std::env::var_os("XCURSOR_SIZE").is_none() {
        // eframe/winit runs Slate through X11/XWayland in this build. Without an
        // explicit cursor size, some desktops/themes pick a huge client-side
        // cursor only inside the app window.
        unsafe {
            std::env::set_var("XCURSOR_SIZE", DEFAULT_CURSOR_SIZE);
        }
    }
    if std::env::var_os("HYPRCURSOR_SIZE").is_none() {
        unsafe {
            std::env::set_var("HYPRCURSOR_SIZE", DEFAULT_CURSOR_SIZE);
        }
    }
}

fn main() -> eframe::Result {
    normalize_cursor_environment();

    let raw_args = std::env::args().skip(1).collect::<Vec<_>>();
    if raw_args.first().map(String::as_str) == Some("flow-api") {
        if let Err(err) = run_flow_api(&raw_args[1..]) {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut scratch = false;
    let mut standalone_agenda = false;
    let mut path = None;
    let mut startup_line = None;
    let mut args = raw_args.into_iter().peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scratch" | "-s" => scratch = true,
            "--agenda" | "--ag" => standalone_agenda = true,
            "--line" | "-l" => {
                startup_line = args.next().and_then(|value| value.parse::<usize>().ok());
            }
            _ => path = Some(PathBuf::from(arg)),
        }
    }

    let title = if scratch {
        "Slate Scratch"
    } else if standalone_agenda {
        "Slate Agenda"
    } else {
        "Slate"
    };
    let size = if scratch {
        [760.0, 460.0]
    } else if standalone_agenda {
        [900.0, 620.0]
    } else {
        [900.0, 560.0]
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size(if standalone_agenda {
                [620.0, 360.0]
            } else {
                [420.0, 260.0]
            }),
        vsync: false,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(|cc| {
            Ok(Box::new(SlateApp::new(
                cc,
                path,
                scratch,
                standalone_agenda,
                startup_line,
            )))
        }),
    )
}

#[derive(Clone, Debug)]
struct FlowApiTask {
    kind: String,
    id: String,
    path: PathBuf,
    relative_path: String,
    line_number: usize,
    line_index: usize,
    state: String,
    title: String,
    tags: Vec<String>,
    projects: Vec<String>,
    priority: Option<String>,
    due: Option<String>,
    scheduled: Option<String>,
    heading: Option<String>,
    source_snippet: String,
}

fn run_flow_api(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err("Usage: slate flow-api list|complete|open|rescan".to_string());
    };
    match command {
        "list" => {
            let scope = flow_api_scope(args)?;
            let days = flow_api_usize_arg(args, "--days").unwrap_or(45);
            flow_api_rescan_if_empty(&scope)?;
            let tasks = flow_api_load_tasks(&scope)?;
            println!("{}", flow_api_tasks_json(&tasks, days));
            Ok(())
        }
        "complete" => {
            let id = args
                .get(1)
                .ok_or_else(|| "Usage: slate flow-api complete <id> [--scope PATH]".to_string())?;
            let scope = flow_api_scope(args)?;
            flow_api_complete(&scope, id)?;
            println!("{{\"ok\":true}}");
            Ok(())
        }
        "open" => {
            let id = args
                .get(1)
                .ok_or_else(|| "Usage: slate flow-api open <id> [--scope PATH]".to_string())?;
            let scope = flow_api_scope(args)?;
            let task = flow_api_load_tasks(&scope)?
                .into_iter()
                .find(|task| task.id == *id)
                .ok_or_else(|| format!("Slate Flow API task not found: {id}"))?;
            ProcessCommand::new(std::env::current_exe().map_err(|err| err.to_string())?)
                .arg(&task.path)
                .arg("--line")
                .arg(task.line_number.to_string())
                .spawn()
                .map_err(|err| format!("Slate Flow API open failed: {err}"))?;
            println!("{{\"ok\":true}}");
            Ok(())
        }
        "rescan" => {
            let scope = flow_api_scope(args)?;
            let (files, tasks) = flow_api_rescan(&scope)?;
            println!(
                "{{\"ok\":true,\"indexedFiles\":{},\"indexedTasks\":{}}}",
                files, tasks
            );
            Ok(())
        }
        _ => Err(format!("Unknown Slate Flow API command: {command}")),
    }
}

fn flow_api_scope(args: &[String]) -> Result<PathBuf, String> {
    if let Some(scope) = flow_api_string_arg(args, "--scope") {
        return Ok(flow_api_normalize_scope(PathBuf::from(scope))?);
    }
    if let Ok(scope) = std::env::var("SLATE_FLOW_SCOPE") {
        if !scope.trim().is_empty() {
            return Ok(flow_api_normalize_scope(PathBuf::from(scope))?);
        }
    }
    let cwd = std::env::current_dir().map_err(|err| format!("Flow API scope failed: {err}"))?;
    Ok(flow_api_normalize_scope(cwd)?)
}

fn flow_api_normalize_scope(path: PathBuf) -> Result<PathBuf, String> {
    let path = if path.exists() {
        fs::canonicalize(&path).map_err(|err| format!("Flow API scope failed: {err}"))?
    } else {
        path
    };
    if path.is_file() {
        Ok(SlateApp::flow_project_root_for_path(&path))
    } else if path.join(".git").is_dir() || path.join(".slate").is_dir() {
        Ok(path)
    } else {
        Ok(SlateApp::flow_project_root_for_path(&path.join("inbox.md")))
    }
}

fn flow_api_string_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find_map(|pair| (pair[0] == flag).then(|| pair[1].clone()))
}

fn flow_api_usize_arg(args: &[String], flag: &str) -> Option<usize> {
    flow_api_string_arg(args, flag).and_then(|value| value.parse().ok())
}

fn flow_api_rescan_if_empty(scope: &Path) -> Result<(), String> {
    let sqlite_path = SlateApp::flow_sqlite_path_for_root(scope)?;
    if !sqlite_path.exists() {
        let _ = flow_api_rescan(scope)?;
    }
    Ok(())
}

fn flow_api_rescan(scope: &Path) -> Result<(usize, usize), String> {
    let sqlite_path = SlateApp::flow_sqlite_path_for_root(scope)?;
    let mut files = Vec::new();
    SlateApp::collect_flow_markdown_files(scope, &mut files, 20_000);
    let mut indexed_files = 0usize;
    let mut indexed_tasks = 0usize;
    for path in files {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let summary = flow::update_flow_index_for_file(scope, &sqlite_path, &path, &contents)?;
        indexed_files += 1;
        indexed_tasks += summary.tasks.len();
    }
    Ok((indexed_files, indexed_tasks))
}

fn flow_api_load_tasks(scope: &Path) -> Result<Vec<FlowApiTask>, String> {
    let sqlite_path = SlateApp::flow_sqlite_path_for_root(scope)?;
    if !sqlite_path.exists() {
        return Ok(Vec::new());
    }
    let query = "SELECT path, line_number, line_index, COALESCE(kind,'task'), state, title, tags, projects, COALESCE(priority,''), COALESCE(due,''), COALESCE(scheduled,''), COALESCE(heading,''), source_snippet FROM tasks ORDER BY COALESCE(scheduled, due, ''), path, line_number;";
    let output = ProcessCommand::new("sqlite3")
        .arg("-separator")
        .arg("\t")
        .arg(&sqlite_path)
        .arg(query)
        .output()
        .map_err(|err| format!("Slate Flow API failed: sqlite3 unavailable: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| flow_api_parse_row(scope, line))
        .collect())
}

fn flow_api_parse_row(scope: &Path, line: &str) -> Option<FlowApiTask> {
    let mut fields = line.split('\t');
    let relative_path = fields.next()?.to_string();
    let line_number = fields.next()?.parse().ok()?;
    let line_index = fields.next()?.parse().ok()?;
    let kind = fields.next()?.to_string();
    let state = fields.next()?.to_string();
    let title = fields.next()?.to_string();
    let tags = split_cache_list(fields.next()?);
    let projects = split_cache_list(fields.next()?);
    let priority = non_empty_string(fields.next()?);
    let due = non_empty_string(fields.next()?);
    let scheduled = non_empty_string(fields.next()?);
    let heading = non_empty_string(fields.next().unwrap_or(""));
    let source_snippet = fields.next().unwrap_or("").to_string();
    let id = flow_api_task_id(scope, &relative_path, line_number, &title);
    Some(FlowApiTask {
        kind,
        id,
        path: scope.join(&relative_path),
        relative_path,
        line_number,
        line_index,
        state,
        title,
        tags,
        projects,
        priority,
        due,
        scheduled,
        heading,
        source_snippet,
    })
}

fn flow_api_task_id(scope: &Path, relative_path: &str, line_number: usize, title: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    scope.display().to_string().hash(&mut hasher);
    let scope_hash = hasher.finish();
    let mut title_hasher = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut title_hasher);
    format!(
        "slate:{scope_hash:016x}:{}:{line_number}:{:016x}",
        relative_path.replace(':', "%3A"),
        title_hasher.finish()
    )
}

fn flow_api_complete(scope: &Path, id: &str) -> Result<(), String> {
    let task = flow_api_load_tasks(scope)?
        .into_iter()
        .find(|task| task.id == id)
        .ok_or_else(|| format!("Slate Flow API task not found: {id}"))?;
    if task.kind != "task" {
        return Err(format!(
            "Slate Flow API cannot complete {} items",
            task.kind
        ));
    }
    if task.state == "done" {
        return Ok(());
    }

    // Always leave a tiny local completion request for any open Slate window.
    // If the task is in a dirty in-memory buffer, the API process cannot safely
    // edit that memory directly, so the running editor applies this request on
    // its next UI tick and refreshes the derived agenda cache.
    append_flow_completion_request(scope, &task)?;

    let contents = fs::read_to_string(&task.path)
        .map_err(|err| format!("Slate Flow API complete failed reading source: {err}"))?;
    let mut lines = contents.lines().map(str::to_string).collect::<Vec<_>>();
    let Some(line) = lines.get_mut(task.line_index) else {
        return Ok(());
    };
    if line.trim_end() != task.source_snippet.trim_end() {
        return Ok(());
    }
    let Some(parsed) = parse_checkbox_line(line) else {
        return Ok(());
    };
    let marker_start = parsed.indent.len() + parsed.task_prefix.len();
    let marker_end = marker_start + parsed.marker.len();
    line.replace_range(marker_start..marker_end, "[x]");
    let mut next = lines.join("\n");
    if contents.ends_with('\n') {
        next.push('\n');
    }
    let tmp = task.path.with_extension(format!(
        "{}.tmp",
        task.path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("md")
    ));
    fs::write(&tmp, next).map_err(|err| format!("Slate Flow API complete write failed: {err}"))?;
    fs::rename(&tmp, &task.path)
        .map_err(|err| format!("Slate Flow API complete replace failed: {err}"))?;
    let updated = fs::read_to_string(&task.path)
        .map_err(|err| format!("Slate Flow API reindex read failed: {err}"))?;
    flow::update_flow_index_for_file(
        scope,
        &SlateApp::flow_sqlite_path_for_root(scope)?,
        &task.path,
        &updated,
    )?;
    Ok(())
}

fn append_flow_completion_request(scope: &Path, task: &FlowApiTask) -> Result<(), String> {
    flow_runtime::append_completion_request(
        &SlateApp::flow_sqlite_path_for_root(scope)?,
        &task.id,
        &task.relative_path,
        task.line_index,
        &task.title,
    )
}

fn append_flow_organization_request(
    scope: &Path,
    item: &AgendaItem,
    token: &str,
) -> Result<(), String> {
    flow_runtime::append_organization_request(
        &SlateApp::flow_sqlite_path_for_root(scope)?,
        &item.relative_path,
        item.line_index,
        token,
    )
}

fn append_flow_edit_request(
    scope: &Path,
    item: &AgendaItem,
    replacement: &str,
) -> Result<(), String> {
    flow_runtime::append_edit_request(
        &SlateApp::flow_sqlite_path_for_root(scope)?,
        &item.relative_path,
        item.line_index,
        replacement,
    )
}

fn append_flow_open_request(
    scope: &Path,
    relative_path: &str,
    line_index: usize,
) -> Result<(), String> {
    flow_runtime::append_open_request(
        &SlateApp::flow_sqlite_path_for_root(scope)?,
        relative_path,
        line_index,
    )
}

fn flow_api_tasks_json(tasks: &[FlowApiTask], days: usize) -> String {
    let today = Local::now().date_naive();
    let horizon = today + Duration::days(days as i64);
    let mut out = String::from("[");
    let mut first = true;
    for task in tasks {
        let due = task.due.as_deref().and_then(parse_cache_date);
        let scheduled = task.scheduled.as_deref().and_then(parse_cache_date);
        let dated = due.or(scheduled);
        if dated.is_some_and(|date| date > horizon) {
            continue;
        }
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&flow_api_task_json(task));
    }
    out.push(']');
    out
}

fn flow_api_task_json(task: &FlowApiTask) -> String {
    let project = task.projects.first().cloned();
    let priority = task.priority.as_deref().and_then(|value| match value {
        "ALTA" => Some("high"),
        "MEDIA" => Some("medium"),
        "BAJA" => Some("low"),
        _ => None,
    });
    format!(
        "{{\"id\":{},\"source\":\"slate\",\"kind\":{},\"title\":{},\"status\":{},\"priority\":{},\"project\":{},\"tags\":{},\"dueIso\":{},\"scheduledAtIso\":{},\"path\":{},\"lineNumber\":{},\"heading\":{}}}",
        json_string(&task.id),
        json_string(&task.kind),
        json_string(&task.title),
        json_string(&task.state),
        json_option(priority),
        json_option(project.as_deref()),
        json_string_array(&task.tags),
        json_option(task.due.as_deref()),
        json_option(task.scheduled.as_deref()),
        json_string(&task.relative_path),
        task.line_number,
        json_option(task.heading.as_deref()),
    )
}

fn json_option(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_string())
}

fn json_string_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn collect_link_target_files(root: &Path, out: &mut Vec<PathBuf>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
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
            if file_dialogs::default_should_skip_dir(name) {
                continue;
            }
            collect_link_target_files(&path, out, limit);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn is_markdown_like_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

fn absolutize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        }
    })
}

fn line_has_backlink_to_current(
    line: &str,
    source_file: &Path,
    current_path: &Path,
    current_abs: &Path,
) -> bool {
    parse_markdown_link_spans(line).into_iter().any(|span| {
        let target = &line[span.target_start..span.target_end];
        if link_ui::is_http_url(target) {
            return false;
        }
        let path_part = target
            .split_once('#')
            .map(|(path, _)| path)
            .unwrap_or(target);
        if path_part.trim().is_empty() {
            return false;
        }
        let resolved = link_ui::resolve_markdown_link_path(path_part, Some(source_file));
        let resolved_abs = absolutize_path(&resolved);
        resolved_abs == current_abs || resolved_abs == absolutize_path(current_path)
    })
}

fn line_mentions_current(line: &str, current_path: &Path) -> bool {
    let lower = line.to_ascii_lowercase();
    let file_name = current_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = current_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let path = current_path.display().to_string().to_ascii_lowercase();
    (!file_name.is_empty() && lower.contains(&file_name))
        || (!stem.is_empty() && lower.contains(&stem))
        || lower.contains(&path)
}

fn line_start_offsets(contents: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, ch) in contents.char_indices() {
        if ch == '\n' && index + 1 < contents.len() {
            offsets.push(index + 1);
        }
    }
    offsets
}

fn append_markdown_block(path: &Path, block: &str) -> Result<(), String> {
    let mut contents = fs::read_to_string(path).unwrap_or_default();
    append_markdown_block_to_string(&mut contents, block);
    fs::write(path, contents).map_err(|err| err.to_string())
}

fn append_markdown_block_to_heading(
    path: &Path,
    heading: &link_ui::LinkHeadingOption,
    block: &str,
) -> Result<(), String> {
    let mut contents = fs::read_to_string(path).unwrap_or_default();
    let insert_at =
        markdown_heading_insert_offset(&contents, heading).unwrap_or_else(|| contents.len());
    let mut insertion = String::new();
    if insert_at > 0 && !contents[..insert_at].ends_with('\n') {
        insertion.push('\n');
    }
    insertion.push('\n');
    insertion.push_str(block.trim_end());
    insertion.push('\n');
    contents.insert_str(insert_at, &insertion);
    fs::write(path, contents).map_err(|err| err.to_string())
}

fn append_markdown_block_to_string(contents: &mut String, block: &str) {
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    if !contents.is_empty() {
        contents.push('\n');
    }
    contents.push_str(block.trim_end());
    contents.push('\n');
}

fn markdown_heading_insert_offset(
    contents: &str,
    heading: &link_ui::LinkHeadingOption,
) -> Option<usize> {
    let heading_line_start = contents
        .lines()
        .take(heading.line_index)
        .map(|line| line.len() + 1)
        .sum::<usize>();
    let search_start = contents[heading_line_start..]
        .find('\n')
        .map(|offset| heading_line_start + offset + 1)
        .unwrap_or(contents.len());
    let mut byte = search_start;
    for line in contents[search_start..].split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches('\n');
        if let Some(next_heading) = parse_heading_line(line_without_newline) {
            if next_heading.level <= heading.level {
                return Some(byte);
            }
        }
        byte += line.len();
    }
    Some(contents.len())
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkHeadingPickerMode {
    InsertLink,
    BidirectionalLink,
    Refile,
}

#[derive(Clone)]
enum FindNoteRow {
    File {
        path: PathBuf,
        match_hint: String,
    },
    Heading {
        path: PathBuf,
        line: usize,
        title: String,
        level: usize,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    New,
    Open,
    OpenLast,
    OpenRecent,
    OpenProjectFile,
    OpenDaily,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CtrlShiftMoveMode {
    Vim,
    Slate,
}

impl CtrlShiftMoveMode {
    fn label(self) -> &'static str {
        match self {
            CtrlShiftMoveMode::Vim => "Vim hjkl",
            CtrlShiftMoveMode::Slate => "Slate ijkl",
        }
    }

    fn config_value(self) -> &'static str {
        match self {
            CtrlShiftMoveMode::Vim => "vim",
            CtrlShiftMoveMode::Slate => "slate",
        }
    }

    fn from_config_value(value: &str) -> Option<Self> {
        match value.trim().trim_matches('"') {
            "vim" | "hjkl" => Some(Self::Vim),
            "slate" | "ijkl" => Some(Self::Slate),
            _ => None,
        }
    }

    fn next(self) -> Self {
        match self {
            CtrlShiftMoveMode::Vim => Self::Slate,
            CtrlShiftMoveMode::Slate => Self::Vim,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            CtrlShiftMoveMode::Vim => "Ctrl+Shift: h left, j down, k up, l right",
            CtrlShiftMoveMode::Slate => "Ctrl+Shift: i up, j left, k down, l right",
        }
    }
}

impl PendingAction {
    fn prompt(self) -> &'static str {
        match self {
            PendingAction::New => "buffer has unsaved changes; start a new buffer anyway?",
            PendingAction::Open => "buffer has unsaved changes; open another file anyway?",
            PendingAction::OpenLast => "buffer has unsaved changes; open last file anyway?",
            PendingAction::OpenRecent => "buffer has unsaved changes; open recent file anyway?",
            PendingAction::OpenProjectFile => {
                "buffer has unsaved changes; open selected file anyway?"
            }
            PendingAction::OpenDaily => "buffer has unsaved changes; open Daily anyway?",
            PendingAction::Quit => "buffer has unsaved changes; close anyway?",
        }
    }
}

struct DuplicatePlacement {
    snapshot: String,
    was_dirty: bool,
}

#[derive(Clone)]
struct RefileDraft {
    source_path: PathBuf,
    source_line_number: usize,
    source_line_index: usize,
    text: String,
}

#[derive(Clone)]
struct DocTask {
    line_index: usize,
    state: CheckboxState,
    task_prefix: String,
    text: String,
}

struct FlowBackgroundResult {
    root: PathBuf,
    indexed_files: usize,
    indexed_tasks: usize,
    errors: usize,
}

struct FileCursorPosition {
    path: PathBuf,
    line_index: usize,
    column: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReferenceResultKind {
    Backlinks,
    Mentions,
}

struct SlateApp {
    buffer: EditorBuffer,
    editor_view: EditorView,
    path: Option<PathBuf>,
    dirty: bool,
    status: String,
    palette_open: bool,
    palette_query: String,
    selected_command: usize,
    preview: bool,
    wrap: bool,
    focus_editor_once: bool,
    scratch: bool,
    standalone_agenda: bool,
    scratch_close_after: Option<f64>,
    agenda_updated_toast_until: Option<f64>,
    pending_flow_agenda_sync_at: Option<f64>,
    pending_action: Option<PendingAction>,
    settings_open: bool,
    selected_setting: usize,
    command_line: String,
    command_line_cursor: usize,
    command_line_focused: bool,
    focus_command_line_once: bool,
    selected_command_line_suggestion: usize,
    command_history: Vec<String>,
    command_history_index: Option<usize>,
    command_history_limit: usize,
    command_usage: Vec<CommandUsage>,
    shortcut_help_open: bool,
    line_number_mode: LineNumberMode,
    ctrl_shift_move_mode: CtrlShiftMoveMode,
    reopen_last_file_on_startup: bool,
    markdown_live_rendering: bool,
    virtual_references_enabled: bool,
    virtual_references_collapsed: bool,
    virtual_backlinks_collapsed: bool,
    virtual_mentions_collapsed: bool,
    last_opened_path: Option<PathBuf>,
    last_open_dir: Option<PathBuf>,
    recent_files: Vec<PathBuf>,
    file_cursor_positions: Vec<FileCursorPosition>,
    recent_picker_open: bool,
    recent_query: String,
    selected_recent_file: usize,
    pending_recent_path: Option<PathBuf>,
    lance_open: bool,
    lance_query: String,
    lance_files: Vec<PathBuf>,
    selected_lance_file: usize,
    doc_tasks_open: bool,
    doc_task_query: String,
    selected_doc_task_line: usize,
    agenda_open: bool,
    agenda_items: Vec<AgendaItem>,
    selected_agenda_item: usize,
    agenda_filter: String,
    agenda_start_offset_days: i64,
    agenda_focus: AgendaFocus,
    agenda_tab: AgendaTab,
    agenda_selected_dated_row: usize,
    agenda_selected_inbox_item: usize,
    agenda_selected_tab_items: [usize; 5],
    agenda_expanded_dates: HashSet<NaiveDate>,
    agenda_show_done: bool,
    agenda_scroll_to_selected: bool,
    agenda_org_mode: Option<AgendaOrgMode>,
    agenda_org_query: String,
    selected_agenda_org_suggestion: usize,
    flow_inline_org_mode: Option<AgendaOrgMode>,
    flow_inline_org_anchor: Option<usize>,
    selected_flow_inline_org_suggestion: usize,
    link_inline_anchor: Option<usize>,
    link_inline_files: Vec<PathBuf>,
    selected_link_inline_suggestion: usize,
    pending_bidirectional_link_source_line: Option<usize>,
    link_heading_target_path: Option<PathBuf>,
    flow_item_edit: Option<FlowItemEditState>,
    flow_background_rx: Option<Receiver<FlowBackgroundResult>>,
    flow_background_root: Option<PathBuf>,
    flow_background_loading: bool,
    file_picker_open: bool,
    file_picker_mode: FilePickerMode,
    file_picker_dir: PathBuf,
    file_query: String,
    project_files: Vec<PathBuf>,
    selected_project_file: usize,
    periscope_open: bool,
    periscope_prompt_open: bool,
    periscope_mode: PeriscopeMode,
    periscope_project_root: Option<PathBuf>,
    periscope_query: String,
    periscope_results: Vec<PathBuf>,
    selected_periscope_result: usize,
    periscope_expanded_folders: HashSet<PathBuf>,
    periscope_backend_status: String,
    periscope_global_paths: Vec<PeriscopeIndexedPath>,
    periscope_global_rx: Option<Receiver<Vec<PeriscopeIndexedPath>>>,
    periscope_global_loading: bool,
    find_note_open: bool,
    find_note_query: String,
    find_note_files: Vec<PathBuf>,
    find_note_rows: Vec<FindNoteRow>,
    selected_find_note_row: usize,
    find_note_expanded_files: HashSet<PathBuf>,
    pending_project_file_path: Option<PathBuf>,
    pending_open_heading_fragment: Option<String>,
    pending_daily_target: Option<String>,
    pending_refile: Option<RefileDraft>,
    pending_refile_destination: Option<PathBuf>,
    link_assist_open: bool,
    link_assist_choice: LinkAssistChoice,
    link_assist_trigger_start: Option<usize>,
    link_assist_web_open: bool,
    link_assist_web_url: String,
    link_heading_picker_open: bool,
    link_heading_picker_mode: LinkHeadingPickerMode,
    link_heading_base_target: String,
    link_heading_options: Vec<LinkHeadingOption>,
    selected_link_heading: usize,
    link_preview_open: bool,
    link_preview_buffer: EditorBuffer,
    link_preview_view: EditorView,
    link_preview_path: Option<PathBuf>,
    link_preview_dirty: bool,
    focus_link_preview_once: bool,
    link_preview_heading_fragment: Option<String>,
    link_preview_heading_text: Option<String>,
    save_as_open: bool,
    save_as_dir: PathBuf,
    save_as_filename: String,
    save_as_entries: Vec<PathBuf>,
    selected_save_as_entry: usize,
    scratch_modal_open: bool,
    scratch_buffer: EditorBuffer,
    scratch_view: EditorView,
    scratch_entries_open: bool,
    scratch_entries: Vec<ScratchEntry>,
    selected_scratch_entry: usize,
    capture_modal_open: bool,
    capture_title: String,
    capture_text: String,
    capture_title_focus_once: bool,
    search_state: Option<SearchState>,
    ctrl_layer_active: bool,
    ctrl_layer_sequence: String,
    alt_layer_active: bool,
    alt_layer_sequence: String,
    alt_layer_last_key: Option<char>,
    alt_layer_last_key_time: f64,
    ctrl_alt_layer_active: bool,
    ctrl_alt_layer_sequence: String,
    ctrl_alt_layer_last_key: Option<char>,
    ctrl_alt_layer_last_key_time: f64,
    duplicate_placement: Option<DuplicatePlacement>,
    suppress_editor_keyboard_once: bool,
}

impl SlateApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        scratch: bool,
        standalone_agenda: bool,
        startup_line: Option<usize>,
    ) -> Self {
        setup_style(&cc.egui_ctx);

        let mut app = Self {
            buffer: EditorBuffer::new(),
            editor_view: EditorView::new(),
            path: None,
            dirty: false,
            status: "Ready".to_string(),
            palette_open: false,
            palette_query: String::new(),
            selected_command: 0,
            preview: false,
            wrap: true,
            focus_editor_once: true,
            scratch,
            standalone_agenda,
            scratch_close_after: None,
            agenda_updated_toast_until: None,
            pending_flow_agenda_sync_at: None,
            pending_action: None,
            settings_open: false,
            selected_setting: 0,
            command_line: String::new(),
            command_line_cursor: 0,
            command_line_focused: false,
            focus_command_line_once: false,
            selected_command_line_suggestion: 0,
            command_history: Vec::new(),
            command_history_index: None,
            command_history_limit: 5,
            command_usage: Vec::new(),
            shortcut_help_open: false,
            line_number_mode: LineNumberMode::Absolute,
            ctrl_shift_move_mode: CtrlShiftMoveMode::Vim,
            reopen_last_file_on_startup: false,
            markdown_live_rendering: true,
            virtual_references_enabled: true,
            virtual_references_collapsed: false,
            virtual_backlinks_collapsed: false,
            virtual_mentions_collapsed: false,
            last_opened_path: None,
            last_open_dir: None,
            recent_files: Vec::new(),
            file_cursor_positions: Vec::new(),
            recent_picker_open: false,
            recent_query: String::new(),
            selected_recent_file: 0,
            pending_recent_path: None,
            lance_open: false,
            lance_query: String::new(),
            lance_files: Vec::new(),
            selected_lance_file: 0,
            doc_tasks_open: false,
            doc_task_query: String::new(),
            selected_doc_task_line: 0,
            agenda_open: false,
            agenda_items: Vec::new(),
            selected_agenda_item: 0,
            agenda_filter: String::new(),
            agenda_start_offset_days: 0,
            agenda_focus: AgendaFocus::Dated,
            agenda_tab: AgendaTab::Agenda,
            agenda_selected_dated_row: 0,
            agenda_selected_inbox_item: 0,
            agenda_selected_tab_items: [0; 5],
            agenda_expanded_dates: HashSet::new(),
            agenda_show_done: false,
            agenda_scroll_to_selected: false,
            agenda_org_mode: None,
            agenda_org_query: String::new(),
            selected_agenda_org_suggestion: 0,
            flow_inline_org_mode: None,
            flow_inline_org_anchor: None,
            selected_flow_inline_org_suggestion: 0,
            link_inline_anchor: None,
            link_inline_files: Vec::new(),
            selected_link_inline_suggestion: 0,
            pending_bidirectional_link_source_line: None,
            link_heading_target_path: None,
            flow_item_edit: None,
            flow_background_rx: None,
            flow_background_root: None,
            flow_background_loading: false,
            file_picker_open: false,
            file_picker_mode: FilePickerMode::Browse,
            file_picker_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            file_query: String::new(),
            project_files: Vec::new(),
            selected_project_file: 0,
            periscope_open: false,
            periscope_prompt_open: false,
            periscope_mode: PeriscopeMode::Project,
            periscope_project_root: None,
            periscope_query: String::new(),
            periscope_results: Vec::new(),
            selected_periscope_result: 0,
            periscope_expanded_folders: HashSet::new(),
            periscope_backend_status: String::new(),
            periscope_global_paths: Vec::new(),
            periscope_global_rx: None,
            periscope_global_loading: false,
            find_note_open: false,
            find_note_query: String::new(),
            find_note_files: Vec::new(),
            find_note_rows: Vec::new(),
            selected_find_note_row: 0,
            find_note_expanded_files: HashSet::new(),
            pending_project_file_path: None,
            pending_open_heading_fragment: None,
            pending_daily_target: None,
            pending_refile: None,
            pending_refile_destination: None,
            link_assist_open: false,
            link_assist_choice: LinkAssistChoice::File,
            link_assist_trigger_start: None,
            link_assist_web_open: false,
            link_assist_web_url: String::new(),
            link_heading_picker_open: false,
            link_heading_picker_mode: LinkHeadingPickerMode::InsertLink,
            link_heading_base_target: String::new(),
            link_heading_options: Vec::new(),
            selected_link_heading: 0,
            link_preview_open: false,
            link_preview_buffer: EditorBuffer::new(),
            link_preview_view: EditorView::new(),
            link_preview_path: None,
            link_preview_dirty: false,
            focus_link_preview_once: false,
            link_preview_heading_fragment: None,
            link_preview_heading_text: None,
            save_as_open: false,
            save_as_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            save_as_filename: String::new(),
            save_as_entries: Vec::new(),
            selected_save_as_entry: 0,
            scratch_modal_open: false,
            scratch_buffer: EditorBuffer::new(),
            scratch_view: EditorView::new(),
            scratch_entries_open: false,
            scratch_entries: Vec::new(),
            selected_scratch_entry: 0,
            capture_modal_open: false,
            capture_title: String::new(),
            capture_text: String::new(),
            capture_title_focus_once: false,
            search_state: None,
            ctrl_layer_active: false,
            ctrl_layer_sequence: String::new(),
            alt_layer_active: false,
            alt_layer_sequence: String::new(),
            alt_layer_last_key: None,
            alt_layer_last_key_time: 0.0,
            ctrl_alt_layer_active: false,
            ctrl_alt_layer_sequence: String::new(),
            ctrl_alt_layer_last_key: None,
            ctrl_alt_layer_last_key_time: 0.0,
            duplicate_placement: None,
            suppress_editor_keyboard_once: false,
        };

        app.load_settings();

        if let Some(path) = path {
            app.open_path(path);
            if let Some(line) = startup_line {
                app.goto_startup_line(line);
            }
        } else if standalone_agenda {
            // Standalone agenda should be a view over the latest published Flow
            // cache. Do not immediately rescan from disk here: an open Slate
            // editor may have just published unsaved in-memory Flow items, and
            // a disk rescan would make those live agenda rows disappear.
            app.open_agenda();
        } else if !scratch && app.reopen_last_file_on_startup {
            if let Some(last_path) = app.last_opened_path.clone() {
                app.open_path(last_path);
            }
        }

        app
    }

    fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");
        if self.scratch && self.path.is_none() {
            format!("{}Slate Scratch", if self.dirty { "*" } else { "" })
        } else if self.standalone_agenda && self.path.is_none() {
            "Slate Agenda".to_string()
        } else {
            format!("{}{} — Slate", if self.dirty { "*" } else { "" }, name)
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        self.remember_current_cursor_position();
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.buffer.set_text(text);
                self.path = Some(path.clone());
                self.last_open_dir = path.parent().map(PathBuf::from);
                self.restore_cursor_position_for_path(&path);
                self.dirty = false;
                self.search_state = None;
                self.remember_recent_file(path.clone());
                let flow_summary = self
                    .refresh_flow_index_for_path(&path, self.buffer.as_str())
                    .ok()
                    .flatten();
                let flow_root = Self::flow_project_root_for_path(&path);
                self.start_flow_background_rescan(flow_root);
                let _ = self.save_settings();
                self.status = if let Some(summary) = flow_summary {
                    format!(
                        "Opened {} · indexed {} Flow tasks · scanning folder…",
                        path.display(),
                        summary.tasks.len()
                    )
                } else {
                    format!("Opened {} · scanning folder…", path.display())
                };
            }
            Err(err) => self.status = format!("Open failed: {err}"),
        }
    }

    fn start_refile_current_line(&mut self) {
        let Some(source_path) = self.path.clone() else {
            self.status = "Refile needs a saved source file".to_string();
            return;
        };
        if self.dirty {
            self.status = "Save current buffer before refile".to_string();
            return;
        }
        let line_index = self.buffer.line_index_for_byte(self.buffer.cursor());
        let text = self.buffer.line(line_index).trim().to_string();
        self.start_refile_draft(source_path, line_index, text);
    }

    fn start_refile_selected_agenda_item(&mut self) {
        let Some(index) = self.selected_agenda_task_index() else {
            self.status = "Agenda refile: no item selected".to_string();
            return;
        };
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "Agenda refile: no item selected".to_string();
            return;
        };
        if self.path.as_ref() == Some(&item.path) && self.dirty {
            self.status = "Save current source buffer before agenda refile".to_string();
            return;
        }
        let text = if self.path.as_ref() == Some(&item.path) {
            self.buffer.line(item.line_index).trim().to_string()
        } else {
            match fs::read_to_string(&item.path) {
                Ok(contents) => contents
                    .lines()
                    .nth(item.line_index)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string(),
                Err(err) => {
                    self.status = format!("Agenda refile failed reading source: {err}");
                    return;
                }
            }
        };
        self.start_refile_draft(item.path, item.line_index, text);
    }

    fn start_refile_draft(&mut self, source_path: PathBuf, line_index: usize, text: String) {
        if text.trim().is_empty() {
            self.status = "Refile needs a non-empty source line".to_string();
            return;
        }
        self.pending_refile = Some(RefileDraft {
            source_path,
            source_line_number: line_index + 1,
            source_line_index: line_index,
            text,
        });
        self.open_file_picker_at(self.project_root(), FilePickerMode::Refile);
        self.status = "Refile: choose destination Markdown file".to_string();
    }

    fn open_refile_heading_picker_or_finish(&mut self, destination: PathBuf) {
        let is_markdown = destination
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| matches!(extension.to_lowercase().as_str(), "md" | "markdown"))
            .unwrap_or(false);
        if !is_markdown {
            self.finish_refile_to_path(destination, None);
            return;
        }
        let headings = fs::read_to_string(&destination)
            .map(|contents| link_ui::markdown_heading_options(&contents))
            .unwrap_or_default();
        if headings.is_empty() {
            self.finish_refile_to_path(destination, None);
            return;
        }
        self.pending_refile_destination = Some(destination.clone());
        self.link_heading_picker_open = true;
        self.link_heading_picker_mode = LinkHeadingPickerMode::Refile;
        self.link_heading_base_target =
            link_ui::markdown_link_target_for_file(&destination, self.path.as_deref());
        self.link_heading_options = headings;
        self.selected_link_heading = 0;
        self.focus_editor_once = false;
        self.status = "Refile: choose heading or append to whole file".to_string();
    }

    fn finish_refile_to_selected_target(&mut self, heading: Option<link_ui::LinkHeadingOption>) {
        let Some(destination) = self.pending_refile_destination.take() else {
            self.status = "No pending refile target".to_string();
            self.pending_refile = None;
            return;
        };
        self.finish_refile_to_path(destination, heading);
    }

    fn finish_refile_to_path(
        &mut self,
        destination: PathBuf,
        heading: Option<link_ui::LinkHeadingOption>,
    ) {
        let Some(draft) = self.pending_refile.take() else {
            self.status = "No pending refile".to_string();
            return;
        };
        if destination == draft.source_path {
            self.status = "Refile target is the same file".to_string();
            return;
        }
        let source_label = draft
            .source_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("{name}:{}", draft.source_line_number))
            .unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    draft.source_path.display(),
                    draft.source_line_number
                )
            });
        let source_target_for_destination = format!(
            "{}#L{}",
            link_ui::markdown_link_target_for_file(&draft.source_path, Some(&destination)),
            draft.source_line_number
        );
        let source_target_for_daily = format!(
            "{}#L{}",
            draft.source_path.display(),
            draft.source_line_number
        );
        let mut target_for_source =
            link_ui::markdown_link_target_for_file(&destination, Some(&draft.source_path));
        let mut target_for_daily = destination.display().to_string();
        if let Some(heading) = heading.as_ref() {
            target_for_source.push('#');
            target_for_source.push_str(&heading.anchor);
            target_for_daily.push('#');
            target_for_daily.push_str(&heading.anchor);
        }
        let base_target_label = destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("refile target");
        let target_label = heading
            .as_ref()
            .map(|heading| format!("{base_target_label}#{}", heading.text))
            .unwrap_or_else(|| base_target_label.to_string());
        let block = format!(
            "{}\n  - source: [{}]({})\n",
            draft.text.trim(),
            source_label,
            source_target_for_destination
        );
        let append_result = if let Some(heading) = heading.as_ref() {
            append_markdown_block_to_heading(&destination, heading, &block)
        } else {
            append_markdown_block(&destination, &block)
        };
        if let Err(err) = append_result {
            self.status = format!("Refile failed: {err}");
            return;
        }

        let backlink = format!(
            "  - refiled to: [{}]({})\n",
            target_label, target_for_source
        );
        if let Err(err) = self.insert_refile_backlink(&draft, &backlink) {
            self.status = format!("Refile backlink failed: {err}");
            return;
        }
        let _ = self.refresh_flow_index_for_path(
            &destination,
            &fs::read_to_string(&destination).unwrap_or_default(),
        );
        self.schedule_current_flow_buffer_agenda_sync_later();
        let daily_event = DailyEvent::now(
            DailyEventKind::Organized,
            draft.text.trim().to_string(),
            Vec::new(),
            Vec::new(),
            source_label,
            Some(source_target_for_daily),
        )
        .with_target(target_label, Some(target_for_daily));
        let _ = daily::append_event(&daily_event);
        if self.agenda_open {
            if let Ok(mut items) = self.load_agenda_items() {
                self.overlay_current_buffer_agenda_items(&mut items);
                self.agenda_items = items;
                self.clamp_agenda_selection();
            }
        }
        self.status = format!("Refiled non-destructively to {}", destination.display());
    }

    fn insert_refile_backlink(
        &mut self,
        draft: &RefileDraft,
        backlink: &str,
    ) -> Result<(), String> {
        if self.path.as_ref() == Some(&draft.source_path) {
            let insert_at = if draft.source_line_index + 1 < self.buffer.line_count() {
                self.buffer.line_start(draft.source_line_index + 1)
            } else {
                self.buffer.as_str().len()
            };
            self.buffer.set_cursor(insert_at);
            let needs_leading_newline = insert_at == self.buffer.as_str().len()
                && !self.buffer.as_str().is_empty()
                && !self.buffer.as_str().ends_with('\n');
            let source_note = if needs_leading_newline {
                format!("\n{backlink}")
            } else {
                backlink.to_string()
            };
            self.buffer.insert_text(&source_note);
            self.dirty = true;
            self.schedule_current_flow_buffer_agenda_sync_later();
            return Ok(());
        }

        let mut contents = fs::read_to_string(&draft.source_path).map_err(|err| err.to_string())?;
        let line_starts = line_start_offsets(&contents);
        let insert_at = line_starts
            .get(draft.source_line_index + 1)
            .copied()
            .unwrap_or(contents.len());
        let needs_leading_newline =
            insert_at == contents.len() && !contents.is_empty() && !contents.ends_with('\n');
        let source_note = if needs_leading_newline {
            format!("\n{backlink}")
        } else {
            backlink.to_string()
        };
        contents.insert_str(insert_at, &source_note);
        fs::write(&draft.source_path, contents.clone()).map_err(|err| err.to_string())?;
        let _ = self.refresh_flow_index_for_path(&draft.source_path, &contents);
        Ok(())
    }

    fn schedule_current_flow_buffer_agenda_sync_later(&mut self) {
        self.pending_flow_agenda_sync_at = Some(0.0);
    }

    fn open_daily_with_target(&mut self, raw_target: &str) {
        if self.dirty {
            self.pending_daily_target = Some(raw_target.to_string());
            self.confirm(PendingAction::OpenDaily);
            return;
        }
        let target = match daily::parse_open_target(raw_target) {
            Ok(target) => target,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let date = daily::resolve_open_date(&target);
        let path = match daily::ensure_monthly_document(date) {
            Ok(path) => path,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let refreshing = matches!(target, daily::DailyOpenTarget::Refresh);
        self.open_path(path.clone());
        self.jump_to_daily_day(date);
        self.status = if refreshing {
            format!("Daily refreshed/opened at {date}: {}", path.display())
        } else {
            format!("Daily opened at {date}: {}", path.display())
        };
    }

    fn jump_to_daily_day(&mut self, date: chrono::NaiveDate) {
        let Some(byte) = daily::day_heading_byte(self.buffer.as_str(), date) else {
            return;
        };
        self.buffer.set_cursor(byte);
        self.editor_view.request_scroll_to_cursor(&self.buffer);
    }

    fn goto_startup_line(&mut self, line_number: usize) {
        if line_number == 0 || self.buffer.line_count() == 0 {
            return;
        }
        let line = (line_number - 1).min(self.buffer.line_count().saturating_sub(1));
        self.buffer.set_cursor(self.buffer.line_end(line));
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.status = format!("Opened at line {}", line + 1);
    }

    fn remember_current_cursor_position(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let (line_index, column) = self.buffer.cursor_line_col();
        self.file_cursor_positions
            .retain(|entry| entry.path != path);
        self.file_cursor_positions.insert(
            0,
            FileCursorPosition {
                path,
                line_index,
                column,
            },
        );
        self.file_cursor_positions.truncate(100);
    }

    fn restore_cursor_position_for_path(&mut self, path: &Path) {
        let Some(entry) = self
            .file_cursor_positions
            .iter()
            .find(|entry| entry.path == path)
        else {
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            return;
        };
        let byte = self
            .buffer
            .line_col_to_byte(entry.line_index + 1, entry.column + 1);
        self.buffer.set_cursor(byte);
        self.editor_view.request_scroll_to_cursor(&self.buffer);
    }

    fn refresh_flow_index_for_path(
        &self,
        path: &Path,
        contents: &str,
    ) -> Result<Option<flow::FlowIndexSummary>, String> {
        if !flow::should_index_flow_path(path) {
            return Ok(None);
        }
        let project_root = Self::flow_project_root_for_path(path);
        let sqlite_path = Self::flow_sqlite_path_for_root(&project_root)?;
        flow::update_flow_index_for_file(&project_root, &sqlite_path, path, contents).map(Some)
    }

    fn record_daily_events_for_flow_update(
        &mut self,
        path: &Path,
        old_items: &[AgendaItem],
        summary: &flow::FlowIndexSummary,
    ) {
        let relative_path = summary.known_file.path.clone();
        let old_for_path = old_items
            .iter()
            .filter(|item| item.path == path)
            .map(|item| (item.line_index, item))
            .collect::<std::collections::HashMap<_, _>>();

        for task in &summary.tasks {
            let source = format!("{}:{}", relative_path, task.line_number);
            let source_link = Some(format!("{}#L{}", path.display(), task.line_number));
            let old = old_for_path.get(&task.line_index).copied();
            let event_kind = if old.is_none() {
                Some(DailyEventKind::Captured)
            } else if task.kind == flow::FlowItemKind::Task
                && old.map(|item| item.state.as_str()) != Some("done")
                && task.state == CheckboxState::Done
            {
                Some(DailyEventKind::Completed)
            } else {
                None
            };

            let Some(kind) = event_kind else {
                continue;
            };
            let event = DailyEvent::now(
                kind,
                task.title.clone(),
                task.tags.clone(),
                task.projects.clone(),
                source,
                source_link,
            );
            if let Err(err) = daily::append_event(&event) {
                self.status = err;
                return;
            }
        }
    }

    fn flow_project_root_for_path(path: &Path) -> PathBuf {
        let mut dir = path
            .parent()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        loop {
            if dir.join(".git").is_dir() {
                return dir;
            }
            if !dir.pop() {
                return path
                    .parent()
                    .map(PathBuf::from)
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("."));
            }
        }
    }

    fn flow_sqlite_path_for_root(root: &Path) -> Result<PathBuf, String> {
        let Some(mut sqlite_path) = dirs_next::data_dir() else {
            return Err("Flow index failed: no data dir".to_string());
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        root.display().to_string().hash(&mut hasher);
        sqlite_path.push("slate");
        sqlite_path.push("flow");
        sqlite_path.push(format!("scope-{:016x}", hasher.finish()));
        sqlite_path.push("agenda.db");
        Ok(sqlite_path)
    }

    fn flow_sqlite_path(&self) -> Result<PathBuf, String> {
        Self::flow_sqlite_path_for_root(&self.flow_scope_root())
    }

    fn flow_scope_root(&self) -> PathBuf {
        self.path
            .as_ref()
            .map(|path| Self::flow_project_root_for_path(path))
            .or_else(|| self.detect_project_root())
            .unwrap_or_else(|| self.project_root())
    }

    fn collect_flow_markdown_files(root: &Path, out: &mut Vec<PathBuf>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        let Ok(entries) = fs::read_dir(root) else {
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
                if file_dialogs::default_should_skip_dir(name) {
                    continue;
                }
                Self::collect_flow_markdown_files(&path, out, limit);
            } else if path.is_file() && flow::should_index_flow_path(&path) {
                out.push(path);
            }
        }
    }

    fn start_flow_background_rescan(&mut self, root: PathBuf) {
        if self.flow_background_loading && self.flow_background_root.as_ref() == Some(&root) {
            return;
        }
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&root) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.flow_background_rx = Some(rx);
        self.flow_background_root = Some(root.clone());
        self.flow_background_loading = true;
        thread::spawn(move || {
            let mut files = Vec::new();
            Self::collect_flow_markdown_files(&root, &mut files, 20_000);
            let mut indexed_files = 0usize;
            let mut indexed_tasks = 0usize;
            let mut errors = 0usize;
            for path in files {
                let Ok(contents) = fs::read_to_string(&path) else {
                    errors += 1;
                    continue;
                };
                match flow::update_flow_index_for_file(&root, &sqlite_path, &path, &contents) {
                    Ok(summary) => {
                        indexed_files += 1;
                        indexed_tasks += summary.tasks.len();
                    }
                    Err(_) => errors += 1,
                }
            }
            let _ = tx.send(FlowBackgroundResult {
                root,
                indexed_files,
                indexed_tasks,
                errors,
            });
        });
    }

    fn poll_flow_background_rescan(&mut self) {
        let Some(rx) = &self.flow_background_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.flow_background_rx = None;
                self.flow_background_root = None;
                self.flow_background_loading = false;
                if self.agenda_open || self.standalone_agenda {
                    if let Ok(items) = self.load_agenda_items() {
                        self.agenda_items = items;
                        self.clamp_agenda_selection();
                        if self.standalone_agenda {
                            self.agenda_open = true;
                            self.focus_editor_once = false;
                        }
                    }
                }
                self.status = format!(
                    "Flow ready: indexed {} files, {} tasks in {}{}",
                    result.indexed_files,
                    result.indexed_tasks,
                    result.root.display(),
                    if result.errors > 0 {
                        format!(", {} errors", result.errors)
                    } else {
                        String::new()
                    }
                );
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.flow_background_rx = None;
                self.flow_background_root = None;
                self.flow_background_loading = false;
                self.status = "Flow background scan failed".to_string();
            }
        }
    }

    fn flow_rescan_current_scope(&mut self, rebuild: bool) {
        if self.dirty {
            self.status = "Save current buffer before Flow rescan".to_string();
            return;
        }
        let Ok(sqlite_path) = self.flow_sqlite_path() else {
            self.status = "Flow rescan failed: no data dir".to_string();
            return;
        };
        let root = self.flow_scope_root();
        if rebuild {
            let _ = fs::remove_file(root.join(".slate").join("index.tsv"));
            let _ = fs::remove_file(&sqlite_path);
        }

        let mut files = Vec::new();
        Self::collect_flow_markdown_files(&root, &mut files, 20_000);
        if files.is_empty() {
            self.status = format!(
                "Flow {} found no Markdown files in {}",
                if rebuild { "rebuild" } else { "rescan" },
                root.display()
            );
            return;
        }

        let mut indexed_files = 0usize;
        let mut indexed_tasks = 0usize;
        let mut errors = 0usize;
        for path in files {
            let Ok(contents) = fs::read_to_string(&path) else {
                errors += 1;
                continue;
            };
            match flow::update_flow_index_for_file(&root, &sqlite_path, &path, &contents) {
                Ok(summary) => {
                    indexed_files += 1;
                    indexed_tasks += summary.tasks.len();
                }
                Err(_) => errors += 1,
            }
        }

        self.status = format!(
            "Flow {}: indexed {} files, {} tasks{}",
            if rebuild { "rebuild" } else { "rescan" },
            indexed_files,
            indexed_tasks,
            if errors > 0 {
                format!(", {errors} errors")
            } else {
                String::new()
            }
        );
    }

    fn flow_inbox_path(&self) -> PathBuf {
        self.flow_scope_root().join("inbox.md")
    }

    fn is_current_path(&self, path: &Path) -> bool {
        let Some(current) = &self.path else {
            return false;
        };
        if current == path {
            return true;
        }
        match (current.canonicalize(), path.canonicalize()) {
            (Ok(current), Ok(path)) => current == path,
            _ => false,
        }
    }

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
        let entry = if needs_header {
            format!("# Inbox\n\n{task_line}\n")
        } else {
            format!("{task_line}\n")
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

    fn open_find_note_results(&mut self, query: &str) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.lance_open = false;
        self.doc_tasks_open = false;
        self.file_picker_open = false;
        self.periscope_open = false;
        self.save_as_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.find_note_query = query.trim().to_string();
        self.find_note_expanded_files.clear();
        self.selected_find_note_row = 0;
        self.find_note_open = true;
        self.focus_editor_once = false;
        self.refresh_find_note_results();
        self.status =
            "Find note: type to filter · Enter open · Shift+Enter front modal".to_string();
    }

    fn refresh_find_note_results(&mut self) {
        let root = self.project_root();
        let mut files = Vec::new();
        collect_link_target_files(&root, &mut files, 20_000);
        files.retain(|path| is_markdown_like_path(path));
        files.sort();
        files.dedup();
        let query = self.find_note_query.trim().to_ascii_lowercase();
        self.find_note_files = files
            .into_iter()
            .filter(|path| self.find_note_file_matches(path, &query))
            .take(200)
            .collect();
        self.rebuild_find_note_rows();
    }

    fn find_note_file_matches(&self, path: &Path, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let root = self.project_root();
        let label = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        if label.to_ascii_lowercase().contains(query)
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.to_ascii_lowercase().contains(query))
                .unwrap_or(false)
        {
            return true;
        }
        fs::read_to_string(path)
            .map(|contents| contents.to_ascii_lowercase().contains(query))
            .unwrap_or(false)
    }

    fn rebuild_find_note_rows(&mut self) {
        let query = self.find_note_query.trim().to_ascii_lowercase();
        let mut rows = Vec::new();
        for path in &self.find_note_files {
            let hint = if query.is_empty() {
                "note".to_string()
            } else {
                self.find_note_match_hint(path, &query)
            };
            rows.push(FindNoteRow::File {
                path: path.clone(),
                match_hint: hint,
            });
            if self.find_note_expanded_files.contains(path) {
                if let Ok(contents) = fs::read_to_string(path) {
                    for (line_index, line) in contents.lines().enumerate() {
                        if let Some(heading) = parse_heading_line(line) {
                            rows.push(FindNoteRow::Heading {
                                path: path.clone(),
                                line: line_index + 1,
                                title: heading.text.trim().to_string(),
                                level: heading.level,
                            });
                        }
                    }
                }
            }
        }
        self.find_note_rows = rows;
        self.selected_find_note_row = self
            .selected_find_note_row
            .min(self.find_note_rows.len().saturating_sub(1));
    }

    fn find_note_match_hint(&self, path: &Path, query: &str) -> String {
        let root = self.project_root();
        let label = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        if label.to_ascii_lowercase().contains(query) {
            return "file".to_string();
        }
        let Ok(contents) = fs::read_to_string(path) else {
            return "note".to_string();
        };
        for line in contents.lines() {
            let lower = line.to_ascii_lowercase();
            if let Some(heading) = parse_heading_line(line) {
                if heading.text.to_ascii_lowercase().contains(query) {
                    return format!("heading: {}", heading.text.trim());
                }
            } else if lower.contains(query) {
                return line.trim().chars().take(80).collect();
            }
        }
        "note".to_string()
    }

    fn move_find_note_selection(&mut self, delta: isize) {
        self.selected_find_note_row = self
            .selected_find_note_row
            .saturating_add_signed(delta)
            .min(self.find_note_rows.len().saturating_sub(1));
    }

    fn selected_find_note_target(&self) -> Option<(PathBuf, Option<String>)> {
        match self.find_note_rows.get(self.selected_find_note_row)? {
            FindNoteRow::File { path, .. } => Some((path.clone(), None)),
            FindNoteRow::Heading { path, line, .. } => {
                Some((path.clone(), Some(format!("L{line}"))))
            }
        }
    }

    fn selected_find_note_file_path(&self) -> Option<PathBuf> {
        match self.find_note_rows.get(self.selected_find_note_row)? {
            FindNoteRow::File { path, .. } | FindNoteRow::Heading { path, .. } => {
                Some(path.clone())
            }
        }
    }

    fn toggle_selected_find_note_file(&mut self) {
        let Some(path) = self.selected_find_note_file_path() else {
            return;
        };
        if self.find_note_expanded_files.remove(&path) {
            self.rebuild_find_note_rows();
            return;
        }
        self.find_note_expanded_files.insert(path);
        self.rebuild_find_note_rows();
    }

    fn open_selected_find_note(&mut self, as_modal: bool) {
        let Some((path, fragment)) = self.selected_find_note_target() else {
            self.status = "No find-note result selected".to_string();
            return;
        };
        self.find_note_open = false;
        if as_modal {
            self.open_link_preview(path, fragment);
        } else if self.dirty {
            self.status = "Save or discard changes before opening find-note result".to_string();
        } else {
            self.open_path(path);
            if let Some(fragment) = fragment {
                self.jump_to_markdown_fragment(&fragment);
            }
            self.focus_editor_once = true;
        }
    }

    fn handle_find_note_text_input(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        ctx.input_mut(|i| {
            let mut consumed_text = false;
            for event in &i.events {
                if let egui::Event::Text(text) = event {
                    if !text.chars().any(char::is_control) {
                        self.find_note_query.push_str(text);
                        changed = true;
                        consumed_text = true;
                    }
                }
            }
            if consumed_text {
                i.events
                    .retain(|event| !matches!(event, egui::Event::Text(_)));
            }
        });
        if changed {
            self.find_note_expanded_files.clear();
            self.selected_find_note_row = 0;
            self.refresh_find_note_results();
        }
    }

    fn open_reference_results(&mut self, kind: ReferenceResultKind) {
        let Some(current_path) = self.path.clone() else {
            self.status = "References need a saved current file".to_string();
            return;
        };
        let title = match kind {
            ReferenceResultKind::Backlinks => "Backlinks",
            ReferenceResultKind::Mentions => "Mentions",
        };
        let document = self.reference_results_document(kind, &current_path);
        self.link_preview_buffer.set_text(document);
        self.link_preview_path = None;
        self.link_preview_dirty = false;
        self.link_preview_heading_fragment = None;
        self.link_preview_heading_text = Some(title.to_string());
        self.link_preview_open = true;
        self.focus_link_preview_once = true;
        self.focus_editor_once = false;
        self.status = format!("{title} for {}", current_path.display());
    }

    fn reference_results_document(&self, kind: ReferenceResultKind, current_path: &Path) -> String {
        let root = self.project_root();
        let current_name = current_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("current file");
        let title = match kind {
            ReferenceResultKind::Backlinks => "Backlinks",
            ReferenceResultKind::Mentions => "Mentions",
        };
        let rows = self.reference_rows(kind, current_path);

        let mut doc = format!("# {title} — {current_name}\n\n");
        if rows.is_empty() {
            doc.push_str("No results.\n");
            return doc;
        }
        for (file, line_number, line) in rows {
            let label = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string();
            let target = format!("{}#L{}", file.display(), line_number);
            doc.push_str(&format!("- [{}:{}]({})\n", label, line_number, target));
            doc.push_str(&format!("  - {}\n", line));
        }
        doc
    }

    fn reference_rows(
        &self,
        kind: ReferenceResultKind,
        current_path: &Path,
    ) -> Vec<(PathBuf, usize, String)> {
        let root = self.project_root();
        let current_abs = absolutize_path(current_path);
        let mut files = Vec::new();
        collect_link_target_files(&root, &mut files, 20_000);
        files.retain(|path| is_markdown_like_path(path) && absolutize_path(path) != current_abs);
        files.sort();
        files.dedup();

        let mut rows = Vec::new();
        for file in files {
            let Ok(contents) = fs::read_to_string(&file) else {
                continue;
            };
            for (line_index, line) in contents.lines().enumerate() {
                let matched = match kind {
                    ReferenceResultKind::Backlinks => {
                        line_has_backlink_to_current(line, &file, current_path, &current_abs)
                    }
                    ReferenceResultKind::Mentions => line_mentions_current(line, current_path),
                };
                if matched {
                    rows.push((file.clone(), line_index + 1, line.trim().to_string()));
                }
            }
        }
        rows
    }

    fn markdown_link_target_at_byte(&self, byte: usize) -> Option<&str> {
        let line_index = self.buffer.line_index_for_byte(byte);
        let line_start = self.buffer.line_start(line_index);
        markdown_link_target_at_byte(self.buffer.line(line_index), line_start, byte)
    }

    fn open_link_under_cursor(&mut self) {
        self.open_markdown_link_at_byte(self.buffer.cursor());
    }

    fn open_markdown_link_at_byte(&mut self, byte: usize) {
        let Some(target) = self.markdown_link_target_at_byte(byte).map(str::to_string) else {
            self.status = "No Markdown link under cursor".to_string();
            return;
        };
        self.open_markdown_link_target(&target);
    }

    fn open_markdown_link_target(&mut self, target: &str) {
        let target = target.trim();
        if link_ui::is_http_url(target) {
            self.open_web_link(target, target);
            return;
        }

        let (path_part, heading_fragment) = target
            .split_once('#')
            .map(|(path, fragment)| (path, Some(fragment)))
            .unwrap_or((target, None));
        if link_ui::looks_like_bare_web_target(target) {
            let path = link_ui::resolve_markdown_link_path(path_part, self.path.as_deref());
            if !path.exists() {
                let browser_target = format!("https://{target}");
                self.open_web_link(&browser_target, target);
                return;
            }
        }
        if path_part.trim().is_empty() {
            self.status = "Link has no local file target".to_string();
            return;
        }
        let path = link_ui::resolve_markdown_link_path(path_part, self.path.as_deref());
        if self.dirty {
            self.pending_project_file_path = Some(path.clone());
            self.pending_open_heading_fragment = heading_fragment.map(str::to_string);
            self.confirm(PendingAction::OpenProjectFile);
            return;
        }

        if path.exists() {
            self.open_link_preview(path, heading_fragment.map(str::to_string));
        } else {
            self.status = format!("Link target not found: {}", path.display());
        }
    }

    fn open_link_preview(&mut self, path: PathBuf, heading_fragment: Option<String>) {
        if self.link_preview_open && self.link_preview_dirty {
            self.status = "Save modal buffer before opening another modal buffer".to_string();
            self.focus_link_preview_once = true;
            return;
        }
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.link_preview_buffer.set_text(text);
                let heading_text = if let Some(fragment) = heading_fragment.as_deref() {
                    Self::jump_buffer_to_markdown_fragment(
                        &mut self.link_preview_buffer,
                        &mut self.link_preview_view,
                        fragment,
                    )
                } else {
                    None
                };
                if heading_fragment.is_none() {
                    self.link_preview_view
                        .request_scroll_to_cursor(&self.link_preview_buffer);
                }
                self.link_preview_path = Some(path.clone());
                self.link_preview_dirty = false;
                self.focus_link_preview_once = true;
                self.link_preview_heading_fragment = heading_fragment;
                self.link_preview_heading_text = heading_text;
                self.link_preview_open = true;
                self.focus_editor_once = false;
                self.status = format!("Preview link {}", path.display());
            }
            Err(err) => self.status = format!("Open link preview failed: {err}"),
        }
    }

    fn commit_link_preview(&mut self) {
        let Some(path) = self.link_preview_path.clone() else {
            self.close_link_preview();
            return;
        };
        let heading_fragment = self.link_preview_heading_fragment.clone();
        let preview_dirty = self.link_preview_dirty;
        let preview_text = self.link_preview_buffer.as_str().to_string();
        self.close_link_preview_force();
        if preview_dirty {
            self.buffer.set_text(preview_text);
            self.path = Some(path.clone());
            self.dirty = true;
            self.last_open_dir = path.parent().map(PathBuf::from);
            self.remember_recent_file(path);
            self.focus_editor_once = true;
            self.status = "Promoted modified modal buffer".to_string();
            if let Some(fragment) = heading_fragment {
                self.jump_to_markdown_fragment(&fragment);
            }
            return;
        }
        if self.dirty {
            self.pending_project_file_path = Some(path);
            self.pending_open_heading_fragment = heading_fragment;
            self.confirm(PendingAction::OpenProjectFile);
            return;
        }
        self.open_path(path);
        if let Some(fragment) = heading_fragment {
            self.jump_to_markdown_fragment(&fragment);
        }
    }

    fn save_link_preview(&mut self) {
        let Some(path) = self.link_preview_path.clone() else {
            self.status = "Modal buffer has no file path".to_string();
            return;
        };
        match fs::write(&path, self.link_preview_buffer.as_str()) {
            Ok(_) => {
                self.link_preview_dirty = false;
                self.last_open_dir = path.parent().map(PathBuf::from);
                self.remember_recent_file(path.clone());
                self.status = format!("Saved modal buffer {}", path.display());
            }
            Err(err) => self.status = format!("Save modal buffer failed: {err}"),
        }
    }

    fn swap_link_preview_with_main(&mut self) {
        self.remember_current_cursor_position();
        let main_cursor = self.buffer.cursor_line_col();
        let modal_cursor = self.link_preview_buffer.cursor_line_col();
        std::mem::swap(&mut self.buffer, &mut self.link_preview_buffer);
        std::mem::swap(&mut self.path, &mut self.link_preview_path);
        std::mem::swap(&mut self.dirty, &mut self.link_preview_dirty);

        let main_byte = self
            .buffer
            .line_col_to_byte(main_cursor.0 + 1, main_cursor.1 + 1);
        let modal_byte = self
            .link_preview_buffer
            .line_col_to_byte(modal_cursor.0 + 1, modal_cursor.1 + 1);
        self.buffer.set_cursor(main_byte);
        self.link_preview_buffer.set_cursor(modal_byte);
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.link_preview_view
            .request_scroll_to_cursor(&self.link_preview_buffer);

        self.search_state = None;
        self.link_preview_heading_fragment = None;
        self.link_preview_heading_text = None;
        self.focus_link_preview_once = true;
        self.link_preview_view.request_keyboard_focus();
        if let Some(path) = self.path.clone() {
            self.last_open_dir = path.parent().map(PathBuf::from);
            self.remember_recent_file(path);
        }
        self.status = "Swapped main and modal buffers".to_string();
    }

    fn close_link_preview_force(&mut self) {
        self.link_preview_open = false;
        self.link_preview_path = None;
        self.link_preview_dirty = false;
        self.focus_link_preview_once = false;
        self.link_preview_heading_fragment = None;
        self.link_preview_heading_text = None;
        self.link_preview_buffer.clear();
        self.focus_editor_once = true;
    }

    fn close_link_preview(&mut self) {
        if self.link_preview_dirty {
            self.status = "Save modal buffer before closing".to_string();
            self.focus_link_preview_once = true;
            return;
        }
        self.link_preview_open = false;
        self.link_preview_path = None;
        self.link_preview_dirty = false;
        self.focus_link_preview_once = false;
        self.link_preview_heading_fragment = None;
        self.link_preview_heading_text = None;
        self.link_preview_buffer.clear();
        self.focus_editor_once = true;
    }

    fn jump_to_markdown_fragment(&mut self, fragment: &str) {
        if let Some(target) = Self::jump_buffer_to_markdown_fragment(
            &mut self.buffer,
            &mut self.editor_view,
            fragment,
        ) {
            self.status = format!("Opened {target}");
        } else {
            self.status = format!("Link target not found: #{fragment}");
        }
    }

    fn jump_buffer_to_markdown_fragment(
        buffer: &mut EditorBuffer,
        view: &mut EditorView,
        fragment: &str,
    ) -> Option<String> {
        let fragment = fragment.trim();
        if fragment.is_empty() {
            return None;
        }
        if let Some(line_number) = fragment
            .strip_prefix('L')
            .or_else(|| fragment.strip_prefix("line-"))
            .and_then(|value| value.parse::<usize>().ok())
        {
            if line_number == 0 || buffer.line_count() == 0 {
                return None;
            }
            let line_index = (line_number - 1).min(buffer.line_count().saturating_sub(1));
            buffer.set_cursor(buffer.line_start(line_index));
            view.request_scroll_to_cursor(buffer);
            return Some(format!("line {line_number}"));
        }
        for line_index in 0..buffer.line_count() {
            let Some(heading) = parse_heading_line(buffer.line(line_index)) else {
                continue;
            };
            let heading_text = heading.text.trim().to_string();
            if link_ui::markdown_heading_anchor(&heading_text) == fragment {
                buffer.set_cursor(buffer.line_start(line_index));
                view.request_scroll_to_cursor(buffer);
                return Some(format!("heading {heading_text}"));
            }
        }
        None
    }

    fn open_web_link(&mut self, browser_target: &str, display_target: &str) {
        match ProcessCommand::new("xdg-open").arg(browser_target).spawn() {
            Ok(_) => self.status = format!("Opened link {display_target}"),
            Err(err) => self.status = format!("Open link failed: {err}"),
        }
    }

    fn open_link_assist(&mut self, trigger_start: usize) {
        self.pending_bidirectional_link_source_line = None;
        self.link_inline_anchor = Some(trigger_start);
        self.link_inline_files = self.collect_link_inline_files();
        self.selected_link_inline_suggestion = 0;
        self.link_assist_open = false;
        self.link_assist_choice = LinkAssistChoice::File;
        self.link_assist_trigger_start = Some(trigger_start);
        self.link_assist_web_open = false;
        self.link_assist_web_url.clear();
        self.focus_editor_once = true;
        self.status = "Link: type to fuzzy-find files · Enter inserts Markdown link".to_string();
    }

    fn close_link_inline_autocomplete(&mut self) {
        self.link_inline_anchor = None;
        self.link_inline_files.clear();
        self.selected_link_inline_suggestion = 0;
    }

    fn detect_inline_bidirectional_link_command(&mut self, typed: Option<&str>) -> bool {
        if typed != Some(" ")
            || self.link_inline_anchor.is_some()
            || self.buffer.selection().is_some()
        {
            return false;
        }
        let cursor = self.buffer.cursor();
        let line_index = self.buffer.line_index_for_byte(cursor);
        let line_start = self.buffer.line_start(line_index);
        let Some(prefix) = self.buffer.as_str().get(line_start..cursor) else {
            return false;
        };
        if prefix != ":link " {
            return false;
        }
        self.buffer
            .replace_selection_or_range(line_start, cursor, "[");
        self.buffer.set_cursor(line_start + 1);
        self.pending_bidirectional_link_source_line = Some(line_index + 1);
        self.link_inline_anchor = Some(line_start);
        self.link_inline_files = self.collect_link_inline_files();
        self.selected_link_inline_suggestion = 0;
        self.link_assist_trigger_start = Some(line_start);
        self.focus_editor_once = true;
        self.status =
            ":link: choose target; Slate will write the reciprocal Markdown link".to_string();
        true
    }

    fn collect_link_inline_files(&self) -> Vec<PathBuf> {
        let root = self.project_root();
        let mut out = Vec::new();
        collect_link_target_files(&root, &mut out, 20_000);
        out.sort();
        out.dedup();
        out
    }

    fn current_link_inline_query(&self) -> Option<String> {
        let anchor = self.link_inline_anchor?;
        let cursor = self.buffer.cursor();
        let text = self.buffer.as_str();
        if self.buffer.selection().is_some()
            || anchor >= text.len()
            || cursor < anchor + 1
            || text.as_bytes().get(anchor) != Some(&b'[')
        {
            return None;
        }
        let query = text.get(anchor + 1..cursor)?;
        if query.contains('\n') || query.contains(']') || query.contains('(') || query.contains(')')
        {
            return None;
        }
        Some(query.to_string())
    }

    fn link_inline_suggestions(&self) -> Vec<PathBuf> {
        let Some(query) = self.current_link_inline_query() else {
            return Vec::new();
        };
        let root = self.project_root();
        if query.trim().is_empty() {
            let mut seen = HashSet::new();
            let mut results = Vec::new();
            for path in self
                .recent_files
                .iter()
                .chain(self.link_inline_files.iter())
            {
                if path.is_file()
                    && path.starts_with(&root)
                    && seen.insert(path.display().to_string())
                {
                    results.push(path.clone());
                    if results.len() >= 20 {
                        break;
                    }
                }
            }
            return results;
        }
        periscope::rank_paths(self.link_inline_files.clone(), &root, &query, 20)
    }

    fn refresh_link_inline_state(&mut self, typed: Option<&str>) {
        if self.link_inline_anchor.is_none() {
            return;
        }
        if typed
            .map(|text| text.contains(']') || text.contains('\n'))
            .unwrap_or(false)
        {
            self.close_link_inline_autocomplete();
            return;
        }
        if self.current_link_inline_query().is_none() {
            self.close_link_inline_autocomplete();
            return;
        }
        let suggestions = self.link_inline_suggestions();
        if suggestions.is_empty() {
            self.selected_link_inline_suggestion = 0;
        } else {
            self.selected_link_inline_suggestion = self
                .selected_link_inline_suggestion
                .min(suggestions.len().saturating_sub(1));
        }
    }

    fn selected_link_inline_path(&self) -> Option<PathBuf> {
        let suggestions = self.link_inline_suggestions();
        suggestions
            .get(self.selected_link_inline_suggestion)
            .cloned()
            .or_else(|| suggestions.first().cloned())
    }

    fn confirm_link_inline_autocomplete(&mut self) {
        let Some(anchor) = self.link_inline_anchor else {
            return;
        };
        let Some(path) = self.selected_link_inline_path() else {
            self.close_link_inline_autocomplete();
            return;
        };
        let target = link_ui::markdown_link_target_for_file(&path, self.path.as_deref());
        let cursor = self.buffer.cursor();
        self.buffer
            .replace_selection_or_range(anchor, cursor, &format!("[]({target})"));
        self.buffer.set_cursor(anchor + 1);
        self.dirty = true;
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.close_link_inline_autocomplete();
        self.focus_editor_once = true;
        if self.pending_bidirectional_link_source_line.is_some() {
            self.write_reciprocal_markdown_link(&path, None);
        } else {
            self.status =
                "Inserted Markdown link; type label or Enter to use file name".to_string();
        }
    }

    fn link_target_has_headings(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| matches!(extension.to_lowercase().as_str(), "md" | "markdown"))
            .unwrap_or(false)
            && fs::read_to_string(path)
                .map(|contents| !link_ui::markdown_heading_options(&contents).is_empty())
                .unwrap_or(false)
    }

    fn open_selected_link_inline_headings(&mut self) {
        let Some(path) = self.selected_link_inline_path() else {
            self.close_link_inline_autocomplete();
            return;
        };
        let target = link_ui::markdown_link_target_for_file(&path, self.path.as_deref());
        let bidirectional = self.pending_bidirectional_link_source_line.is_some();
        self.close_link_inline_autocomplete();
        self.open_link_heading_picker_or_insert(&path, target);
        if bidirectional && self.link_heading_picker_open {
            self.link_heading_picker_mode = LinkHeadingPickerMode::BidirectionalLink;
        }
    }

    fn cancel_link_assist(&mut self) {
        self.close_link_inline_autocomplete();
        self.link_assist_open = false;
        self.link_assist_web_open = false;
        self.link_heading_picker_open = false;
        self.link_heading_picker_mode = LinkHeadingPickerMode::InsertLink;
        self.pending_refile = None;
        self.pending_refile_destination = None;
        self.pending_bidirectional_link_source_line = None;
        self.link_heading_target_path = None;
        self.link_assist_web_url.clear();
        self.link_heading_base_target.clear();
        self.link_heading_options.clear();
        self.selected_link_heading = 0;
        self.link_assist_trigger_start = None;
        self.focus_editor_once = true;
    }

    fn confirm_link_assist_choice(&mut self) {
        match self.link_assist_choice {
            LinkAssistChoice::File => {
                self.link_assist_open = false;
                let dir = self
                    .path
                    .as_ref()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
                    .unwrap_or_else(|| self.project_root());
                self.open_file_picker_at(dir, FilePickerMode::InsertMarkdownLink);
            }
            LinkAssistChoice::Web => {
                self.link_assist_open = false;
                self.link_assist_web_open = true;
                self.link_assist_web_url.clear();
                self.focus_editor_once = false;
            }
        }
    }

    fn open_link_heading_picker_or_insert(&mut self, path: &Path, base_target: String) {
        self.link_heading_target_path = Some(path.to_path_buf());
        let is_markdown = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| matches!(extension.to_lowercase().as_str(), "md" | "markdown"))
            .unwrap_or(false);
        if !is_markdown {
            self.insert_assisted_markdown_link(&base_target);
            return;
        }

        let headings = fs::read_to_string(path)
            .map(|contents| link_ui::markdown_heading_options(&contents))
            .unwrap_or_default();
        if headings.is_empty() {
            self.insert_assisted_markdown_link(&base_target);
            return;
        }

        self.link_heading_picker_open = true;
        self.link_heading_picker_mode = LinkHeadingPickerMode::InsertLink;
        self.link_heading_base_target = base_target;
        self.link_heading_options = headings;
        self.selected_link_heading = 0;
        self.focus_editor_once = false;
        self.status = "Insert link: choose heading or whole file".to_string();
    }

    fn confirm_link_heading_picker(&mut self) {
        let target = if self.selected_link_heading == 0 {
            self.link_heading_base_target.clone()
        } else {
            let Some(heading) = self
                .link_heading_options
                .get(self.selected_link_heading - 1)
            else {
                return;
            };
            format!("{}#{}", self.link_heading_base_target, heading.anchor)
        };
        let selected_heading = (self.selected_link_heading > 0)
            .then(|| {
                self.link_heading_options
                    .get(self.selected_link_heading - 1)
                    .cloned()
            })
            .flatten();
        let mode = self.link_heading_picker_mode;
        self.link_heading_picker_open = false;
        self.link_heading_picker_mode = LinkHeadingPickerMode::InsertLink;
        self.link_heading_base_target.clear();
        self.link_heading_options.clear();
        self.selected_link_heading = 0;
        match mode {
            LinkHeadingPickerMode::InsertLink => self.insert_assisted_markdown_link(&target),
            LinkHeadingPickerMode::BidirectionalLink => {
                self.insert_assisted_markdown_link(&target);
                if let Some(path) = self.link_heading_target_path.take() {
                    self.write_reciprocal_markdown_link(&path, selected_heading.as_ref());
                }
            }
            LinkHeadingPickerMode::Refile => {
                self.finish_refile_to_selected_target(selected_heading)
            }
        }
    }

    fn move_link_heading_selection(&mut self, delta: isize) {
        let total = self.link_heading_options.len() + 1;
        if total == 0 {
            self.selected_link_heading = 0;
            return;
        }
        self.selected_link_heading = self
            .selected_link_heading
            .saturating_add_signed(delta)
            .min(total.saturating_sub(1));
    }

    fn insert_assisted_markdown_link(&mut self, target: &str) {
        let Some(trigger_start) = self.link_assist_trigger_start.take() else {
            self.status = "Insert link failed: missing trigger".to_string();
            return;
        };
        let text = self.buffer.as_str();
        let cursor = self.buffer.cursor();
        let trigger_end = if text.as_bytes().get(trigger_start) == Some(&b'[')
            && cursor >= trigger_start + 1
            && text
                .get(trigger_start + 1..cursor)
                .map(|query| {
                    !query.contains('\n')
                        && !query.contains(']')
                        && !query.contains('(')
                        && !query.contains(')')
                })
                .unwrap_or(false)
        {
            cursor
        } else {
            (trigger_start + 1).min(text.len())
        };
        let insertion = format!("]({target})");
        self.buffer.replace_selection_or_range(
            trigger_start,
            trigger_end,
            &format!("[{}", insertion),
        );
        self.buffer.set_cursor(trigger_start + 1);
        self.dirty = true;
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.focus_editor_once = true;
        if self.pending_bidirectional_link_source_line.is_some() {
            if let Some(path) = self.link_heading_target_path.take() {
                self.write_reciprocal_markdown_link(&path, None);
            }
        } else {
            self.status = "Inserted Markdown link; type the label".to_string();
        }
    }

    fn write_reciprocal_markdown_link(
        &mut self,
        target_path: &Path,
        heading: Option<&link_ui::LinkHeadingOption>,
    ) {
        let Some(source_line) = self.pending_bidirectional_link_source_line.take() else {
            return;
        };
        let Some(source_path) = self.path.clone() else {
            self.status = "Bidirectional link needs a saved source file".to_string();
            return;
        };
        let source_target = format!(
            "{}#L{}",
            link_ui::markdown_link_target_for_file(&source_path, Some(target_path)),
            source_line
        );
        let source_label = source_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("source");
        let block = format!("- linked from: [{source_label}:{source_line}]({source_target})");
        let result = if let Some(heading) = heading {
            append_markdown_block_to_heading(target_path, heading, &block)
        } else {
            append_markdown_block(target_path, &block)
        };
        match result {
            Ok(()) => {
                self.status = format!("Inserted bidirectional link to {}", target_path.display())
            }
            Err(err) => self.status = format!("Inserted link, reciprocal failed: {err}"),
        }
    }

    fn save(&mut self) {
        self.remember_current_cursor_position();
        if let Some(path) = self.path.clone() {
            self.save_path(path);
        } else {
            self.save_as();
        }
    }

    fn scratch_archive_path(&mut self) -> Option<PathBuf> {
        let path = scratch::archive_path();
        if path.is_none() {
            self.status = "Scratch failed: no data dir".to_string();
        }
        path
    }

    fn append_text_to_scratch_archive(&mut self, text: &str) -> bool {
        let Some(path) = self.scratch_archive_path() else {
            self.status = "Scratch append failed: no data dir".to_string();
            return false;
        };
        match scratch::append_text_to_archive(&path, text) {
            Ok(()) => {
                self.status = format!("Appended to {}", path.display());
                true
            }
            Err(err) => {
                self.status = err;
                false
            }
        }
    }

    fn append_to_scratch_archive(&mut self) -> bool {
        if !self.scratch
            || self.path.is_some()
            || !self.dirty
            || self.buffer.as_str().trim().is_empty()
        {
            return false;
        }

        let text = self.buffer.as_str().to_string();
        let (quickadds, remaining) = scratch::split_quickadds(&text);
        let mut quickadd_count = 0usize;
        for task in quickadds {
            if self.quickadd_flow_task(&task) {
                quickadd_count += 1;
            }
        }

        let archived = if remaining.trim().is_empty() {
            false
        } else {
            self.append_text_to_scratch_archive(&remaining)
        };

        let saved = archived || quickadd_count > 0;
        if saved {
            self.dirty = false;
            if quickadd_count > 0 && archived {
                self.status = format!("Scratch archived · quickadded {quickadd_count} Flow tasks");
            } else if quickadd_count > 0 {
                self.status = format!("Quickadded {quickadd_count} Flow tasks");
            }
        }
        saved
    }

    fn close_scratch_with_feedback(&mut self, ctx: &egui::Context) {
        let previous_status = self.status.clone();
        if self.append_to_scratch_archive() {
            self.status = previous_status;
            let until = ctx.input(|input| input.time) + 0.4;
            self.scratch_close_after = Some(until);
            self.agenda_updated_toast_until = Some(until);
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn show_agenda_updated_toast(&mut self, ctx: &egui::Context) {
        self.agenda_updated_toast_until = Some(ctx.input(|input| input.time) + 0.4);
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    fn schedule_current_flow_buffer_agenda_sync(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&path) {
            return;
        }
        self.pending_flow_agenda_sync_at = Some(ctx.input(|input| input.time) + 0.8);
        ctx.request_repaint_after(std::time::Duration::from_millis(850));
    }

    fn poll_pending_flow_agenda_sync(&mut self, ctx: &egui::Context) {
        let Some(sync_at) = self.pending_flow_agenda_sync_at else {
            return;
        };
        let now = ctx.input(|input| input.time);
        if now < sync_at {
            let remaining_ms = ((sync_at - now) * 1000.0).max(25.0) as u64;
            ctx.request_repaint_after(std::time::Duration::from_millis(remaining_ms.min(850)));
            return;
        }
        self.pending_flow_agenda_sync_at = None;
        self.sync_current_flow_buffer_to_agenda(ctx);
    }

    fn sync_current_flow_buffer_to_agenda(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&path) {
            return;
        }
        self.publish_current_flow_buffer_to_agenda(ctx);
    }

    fn publish_current_flow_buffer_to_agenda(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let old_items = self.load_agenda_items().unwrap_or_default();
        let summary = match self.refresh_flow_index_for_path(&path, self.buffer.as_str()) {
            Ok(summary) => summary,
            Err(err) => {
                self.status = format!("Flow agenda update failed: {err}");
                return;
            }
        };
        if let Some(summary) = summary.as_ref() {
            self.record_daily_events_for_flow_update(&path, &old_items, summary);
        }
        if self.agenda_open {
            if let Ok(mut items) = self.load_agenda_items() {
                self.overlay_current_buffer_agenda_items(&mut items);
                self.agenda_items = items;
                self.clamp_agenda_selection();
            }
        }
        self.show_agenda_updated_toast(ctx);
    }

    fn publish_active_flow_buffer_heartbeat(&self) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if self.standalone_agenda || !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let _ = flow_runtime::publish_active_buffer_heartbeat(&sqlite_path, &scope, &current_path);
    }

    fn flow_item_is_active_in_live_editor(&self, item: &AgendaItem) -> bool {
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&self.flow_scope_root()) else {
            return false;
        };
        flow_runtime::active_buffer_contains(&sqlite_path, &item.relative_path)
    }

    fn poll_flow_open_requests(&mut self) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, remaining)) =
            flow_runtime::drain_open_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let opened_line = requests.last().map(|request| {
            request
                .line_index
                .min(self.buffer.line_count().saturating_sub(1))
        });
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if let Some(line) = opened_line {
            self.buffer.set_cursor(self.buffer.line_end(line));
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.focus_editor_once = true;
            self.status = format!("Agenda jumped to open buffer line {}", line + 1);
        }
    }

    fn poll_flow_completion_requests(&mut self, ctx: &egui::Context) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, mut remaining)) =
            flow_runtime::drain_completion_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let mut applied = false;
        for request in requests {
            if self.apply_flow_completion_to_buffer(request.line_index, &request.title) {
                applied = true;
            } else {
                remaining.push(request.raw);
            }
        }
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if applied {
            self.dirty = true;
            self.search_state = None;
            self.publish_current_flow_buffer_to_agenda(ctx);
        }
    }

    fn poll_flow_organization_requests(&mut self, ctx: &egui::Context) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, mut remaining)) =
            flow_runtime::drain_organization_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let mut applied = false;
        for request in requests {
            if self.apply_flow_token_to_buffer(request.line_index, &request.value) {
                applied = true;
            } else {
                remaining.push(request.raw);
            }
        }
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if applied {
            self.dirty = true;
            self.search_state = None;
            self.publish_current_flow_buffer_to_agenda(ctx);
        }
    }

    fn poll_flow_edit_requests(&mut self, ctx: &egui::Context) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, mut remaining)) =
            flow_runtime::drain_edit_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let mut applied = false;
        for request in requests {
            if self.apply_flow_line_edit_to_buffer(request.line_index, &request.value) {
                applied = true;
            } else {
                remaining.push(request.raw);
            }
        }
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if applied {
            self.dirty = true;
            self.search_state = None;
            self.publish_current_flow_buffer_to_agenda(ctx);
        }
    }

    fn apply_flow_line_edit_to_buffer(&mut self, line_index: usize, replacement: &str) -> bool {
        if line_index >= self.buffer.line_count() {
            return false;
        }
        let line_start = self.buffer.line_start(line_index);
        let line_end = self.buffer.line_end(line_index);
        self.buffer
            .replace_selection_or_range(line_start, line_end, replacement);
        true
    }

    fn apply_flow_completion_to_buffer(&mut self, line_index: usize, title: &str) -> bool {
        let candidate_indices = [line_index, line_index.saturating_sub(1), line_index + 1];
        for index in candidate_indices {
            let line = self.buffer.line(index);
            let Some(parsed) = parse_checkbox_line(line) else {
                continue;
            };
            if parsed.state == CheckboxState::Done {
                return true;
            }
            let Some(task) = flow::parse_flow_task_line(line, index, None) else {
                continue;
            };
            if task.title != title {
                continue;
            }
            let line_start = self.buffer.line_start(index);
            let marker_start = line_start + parsed.indent.len() + parsed.task_prefix.len();
            let marker_end = marker_start + parsed.marker.len();
            self.buffer
                .replace_selection_or_range(marker_start, marker_end, "[x]");
            return true;
        }
        false
    }

    fn scratch_feedback_toast(&self, ctx: &egui::Context) {
        scratch::feedback_toast(ctx, self.agenda_updated_toast_until);
    }

    fn archive_scratch_modal(&mut self) {
        if self.scratch_buffer.as_str().trim().is_empty() {
            self.scratch_modal_open = false;
            self.scratch_buffer.clear();
            self.status = "Scratch cancelled".to_string();
            self.focus_editor_once = true;
            return;
        }
        let text = self.scratch_buffer.as_str().to_string();
        let (quickadds, remaining) = scratch::split_quickadds(&text);
        let mut quickadd_count = 0usize;
        for task in quickadds {
            if self.quickadd_flow_task(&task) {
                quickadd_count += 1;
            }
        }

        let archived = if remaining.trim().is_empty() {
            false
        } else {
            self.append_text_to_scratch_archive(&remaining)
        };

        if archived || quickadd_count > 0 {
            self.scratch_buffer.clear();
            self.scratch_modal_open = false;
            self.focus_editor_once = true;
            if quickadd_count > 0 && archived {
                self.status = format!("Scratch archived · quickadded {quickadd_count} Flow tasks");
            } else if quickadd_count > 0 {
                self.status = format!("Quickadded {quickadd_count} Flow tasks");
            }
        }
    }

    fn cancel_scratch_modal(&mut self) {
        self.scratch_modal_open = false;
        self.focus_editor_once = true;
        self.status = if self.scratch_buffer.as_str().trim().is_empty() {
            self.scratch_buffer.clear();
            "Scratch cancelled".to_string()
        } else {
            "Scratch hidden; run :scratch to resume".to_string()
        };
    }

    fn open_scratch_modal(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_entries_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.scratch_modal_open = true;
        self.focus_editor_once = false;
        self.status = "Scratch capture".to_string();
    }

    fn load_scratch_entries(&mut self) -> bool {
        let Some(path) = self.scratch_archive_path() else {
            return false;
        };
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.scratch_entries = scratch::parse_entries(&text);
                if self.scratch_entries.is_empty() {
                    self.selected_scratch_entry = 0;
                } else {
                    self.selected_scratch_entry = self
                        .selected_scratch_entry
                        .min(self.scratch_entries.len().saturating_sub(1));
                }
                true
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.scratch_entries.clear();
                self.selected_scratch_entry = 0;
                true
            }
            Err(err) => {
                self.status = format!("Scratch entries failed: {err}");
                false
            }
        }
    }

    fn open_scratch_entries_modal(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        if self.load_scratch_entries() {
            self.scratch_entries_open = true;
            self.focus_editor_once = false;
            self.status = format!("Scratch entries · {} entries", self.scratch_entries.len());
        }
    }

    fn capture_source_text(&self) -> String {
        if let Some((start, end)) = self.buffer.selection() {
            return self.buffer.as_str()[start..end]
                .trim_matches('\n')
                .to_string();
        }
        let (line_index, _) = self.buffer.cursor_line_col();
        self.buffer.line(line_index).trim_matches('\n').to_string()
    }

    fn open_capture_modal(&mut self) {
        let text = self.capture_source_text();
        if text.trim().is_empty() {
            self.status = "Nothing to capture".to_string();
            return;
        }
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.capture_title.clear();
        self.capture_text = text;
        self.capture_modal_open = true;
        self.capture_title_focus_once = true;
        self.focus_editor_once = false;
        self.status = "Capture to scratch".to_string();
    }

    fn confirm_capture_modal(&mut self) {
        if self.capture_text.trim().is_empty() {
            self.capture_modal_open = false;
            self.status = "Capture cancelled".to_string();
            self.focus_editor_once = true;
            return;
        }
        let entry = scratch::format_capture_entry(&self.capture_title, &self.capture_text);
        if self.append_text_to_scratch_archive(&entry) {
            self.capture_modal_open = false;
            self.capture_title.clear();
            self.capture_text.clear();
            self.focus_editor_once = true;
        }
    }

    fn delete_selected_scratch_entry(&mut self) {
        if self.scratch_entries.is_empty() {
            self.status = "No scratch entry to delete".to_string();
            return;
        }
        let index = self
            .selected_scratch_entry
            .min(self.scratch_entries.len() - 1);
        let deleted = self.scratch_entries.remove(index);
        self.selected_scratch_entry = self
            .selected_scratch_entry
            .min(self.scratch_entries.len().saturating_sub(1));
        let Some(path) = self.scratch_archive_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            if let Err(err) = fs::create_dir_all(dir) {
                self.status = format!("Scratch delete failed: {err}");
                return;
            }
        }
        match fs::write(&path, scratch::serialize_entries(&self.scratch_entries)) {
            Ok(_) => self.status = format!("Deleted scratch entry {}", deleted.title()),
            Err(err) => self.status = format!("Scratch delete failed: {err}"),
        }
    }

    fn save_path(&mut self, path: PathBuf) {
        match fs::write(&path, self.buffer.as_str()) {
            Ok(_) => {
                self.path = Some(path.clone());
                self.last_open_dir = path.parent().map(PathBuf::from);
                self.dirty = false;
                self.remember_recent_file(path.clone());
                let flow_summary = self
                    .refresh_flow_index_for_path(&path, self.buffer.as_str())
                    .ok()
                    .flatten();
                let _ = self.save_settings();
                self.status = if let Some(summary) = flow_summary {
                    format!(
                        "Saved {} · indexed {} Flow tasks",
                        path.display(),
                        summary.tasks.len()
                    )
                } else {
                    format!("Saved {}", path.display())
                };
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn new_buffer(&mut self) {
        self.remember_current_cursor_position();
        self.buffer.clear();
        self.path = None;
        self.dirty = false;
        self.search_state = None;
        self.focus_editor_once = true;
        self.status = "New buffer".to_string();
    }

    fn open_dialog(&mut self) {
        self.open_file_picker_for_open();
    }

    fn open_buffer_dialog(&mut self) {
        if self.link_preview_open && self.link_preview_dirty {
            self.status = "Save modal buffer before opening another modal buffer".to_string();
            self.focus_link_preview_once = true;
            return;
        }
        self.open_file_picker_for_buffer();
    }

    fn open_last(&mut self) {
        let Some(path) = self.last_opened_path.clone() else {
            self.status = "No last file".to_string();
            return;
        };
        self.open_path(path);
    }

    fn remember_recent_file(&mut self, path: PathBuf) {
        self.last_opened_path = Some(path.clone());
        self.recent_files.retain(|recent| recent != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(20);
    }

    fn open_recent_picker(&mut self) {
        self.open_recent_picker_with_query(String::new());
    }

    fn open_recent_picker_with_query(&mut self, query: String) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.recent_query = query;
        if self.recent_files.is_empty() {
            self.status = "No recent files".to_string();
            self.focus_editor_once = true;
            return;
        }
        self.recent_picker_open = true;
        self.selected_recent_file = self.recent_file_indices().first().copied().unwrap_or(0);
        self.status = "Recent files".to_string();
        self.focus_editor_once = false;
    }

    fn recent_file_indices(&self) -> Vec<usize> {
        file_dialogs::path_list_indices(&self.recent_files, &self.recent_query)
    }

    fn move_recent_selection(&mut self, delta: isize) {
        let indices = self.recent_file_indices();
        self.selected_recent_file =
            file_dialogs::move_selected_index(&indices, self.selected_recent_file, delta);
    }

    fn open_selected_recent_file(&mut self) {
        if !self
            .recent_file_indices()
            .contains(&self.selected_recent_file)
        {
            self.status = "No matching recent file".to_string();
            return;
        }
        let Some(path) = self.recent_files.get(self.selected_recent_file).cloned() else {
            self.status = "No recent file selected".to_string();
            return;
        };
        if self.dirty {
            self.pending_recent_path = Some(path);
            self.confirm(PendingAction::OpenRecent);
        } else {
            self.recent_picker_open = false;
            self.open_path(path);
            self.focus_editor_once = true;
        }
    }

    fn resolve_command_path(input: &str) -> PathBuf {
        let path = input.trim();
        path.strip_prefix("~/")
            .and_then(|rest| dirs_next::home_dir().map(|home| home.join(rest)))
            .unwrap_or_else(|| PathBuf::from(path))
    }

    fn add_lance_file(&mut self, path: PathBuf) {
        const LANCE_FILE_LIMIT: usize = 10;
        if self.lance_files.contains(&path) {
            self.status = format!("Already lanced {}", path.display());
            return;
        }
        if self.lance_files.len() >= LANCE_FILE_LIMIT {
            self.status = format!("Lance is full ({LANCE_FILE_LIMIT} files)");
            return;
        }
        self.lance_files.push(path.clone());
        self.selected_lance_file = self.lance_files.len().saturating_sub(1);
        let _ = self.save_settings();
        self.status = format!("Lanced {}", path.display());
    }

    fn lance_add_current(&mut self) {
        let Some(path) = self.path.clone() else {
            self.status = "Lance needs a saved file".to_string();
            return;
        };
        self.add_lance_file(path);
    }

    fn open_lance(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.doc_tasks_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.capture_modal_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.lance_open = true;
        if !self
            .lance_file_indices()
            .contains(&self.selected_lance_file)
        {
            self.selected_lance_file = self.lance_file_indices().first().copied().unwrap_or(0);
        }
        self.focus_editor_once = false;
        self.status = if self.lance_files.is_empty() {
            "Lance empty — Ctrl+A add file".to_string()
        } else {
            format!("{} lance marks", self.lance_files.len())
        };
    }

    fn open_lance_file_picker(&mut self) {
        self.open_file_picker_at(self.project_root(), FilePickerMode::AddLance);
    }

    fn lance_file_indices(&self) -> Vec<usize> {
        file_dialogs::path_list_indices(&self.lance_files, &self.lance_query)
    }

    fn move_lance_selection(&mut self, delta: isize) {
        let indices = self.lance_file_indices();
        self.selected_lance_file =
            file_dialogs::move_selected_index(&indices, self.selected_lance_file, delta);
    }

    fn remove_selected_lance_file(&mut self) {
        if self.selected_lance_file >= self.lance_files.len() {
            return;
        }
        let removed = self.lance_files.remove(self.selected_lance_file);
        self.selected_lance_file = self
            .selected_lance_file
            .min(self.lance_files.len().saturating_sub(1));
        let _ = self.save_settings();
        self.status = format!("Removed lance mark {}", removed.display());
    }

    fn open_selected_lance_file(&mut self) {
        if !self
            .lance_file_indices()
            .contains(&self.selected_lance_file)
        {
            self.status = "No matching lance file".to_string();
            return;
        }
        let Some(path) = self.lance_files.get(self.selected_lance_file).cloned() else {
            self.status = "No lance file selected".to_string();
            return;
        };
        self.lance_open = false;
        if self.dirty {
            self.pending_project_file_path = Some(path);
            self.pending_open_heading_fragment = None;
            self.confirm(PendingAction::OpenProjectFile);
        } else {
            self.open_path(path);
            self.focus_editor_once = true;
        }
    }

    fn open_selected_lance_file_as_modal(&mut self) {
        if !self
            .lance_file_indices()
            .contains(&self.selected_lance_file)
        {
            self.status = "No matching lance file".to_string();
            return;
        }
        let Some(path) = self.lance_files.get(self.selected_lance_file).cloned() else {
            self.status = "No lance file selected".to_string();
            return;
        };
        self.lance_open = false;
        self.open_link_preview(path, None);
    }

    fn open_lance_slot(&mut self, slot: usize) {
        if slot == 0 || slot > self.lance_files.len() {
            self.status = format!("No lance mark {slot}");
            return;
        }
        self.selected_lance_file = slot - 1;
        self.open_selected_lance_file();
    }

    fn open_next_lance_file(&mut self, delta: isize) {
        if self.lance_files.is_empty() {
            self.status = "Lance empty".to_string();
            return;
        }
        self.selected_lance_file = self
            .selected_lance_file
            .saturating_add_signed(delta)
            .min(self.lance_files.len().saturating_sub(1));
        self.open_selected_lance_file();
    }

    fn open_doc_tasks(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.capture_modal_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.ctrl_layer_active = false;
        self.ctrl_layer_sequence.clear();
        self.doc_task_query.clear();
        let tasks = self.doc_tasks();
        if tasks.is_empty() {
            self.status = "No document tasks".to_string();
            self.focus_editor_once = true;
            return;
        }
        self.selected_doc_task_line = tasks.first().map(|task| task.line_index).unwrap_or(0);
        self.doc_tasks_open = true;
        self.focus_editor_once = false;
        self.status = format!("{} document tasks", tasks.len());
    }

    fn doc_tasks(&self) -> Vec<DocTask> {
        self.buffer
            .as_str()
            .lines()
            .enumerate()
            .filter_map(|(line_index, line)| {
                let parsed = parse_checkbox_line(line)?;
                Some(DocTask {
                    line_index,
                    state: parsed.state,
                    task_prefix: parsed.task_prefix.to_string(),
                    text: parsed.text.to_string(),
                })
            })
            .collect()
    }

    fn doc_task_indices(&self) -> Vec<usize> {
        let query = self.doc_task_query.trim().to_lowercase();
        let tasks = self.doc_tasks();
        if query.is_empty() {
            return (0..tasks.len()).collect();
        }
        let mut scored = tasks
            .iter()
            .enumerate()
            .filter_map(|(index, task)| {
                let state = match task.state {
                    CheckboxState::Empty => "empty",
                    CheckboxState::Doing => "doing",
                    CheckboxState::Done => "done",
                };
                let laystack = format!("{} {} {}", task.line_index + 1, state, task.text);
                fuzzy_score(&laystack, &query).map(|score| (score, index))
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|(score, index)| (*score, *index));
        scored.into_iter().map(|(_, index)| index).collect()
    }

    fn move_doc_task_selection(&mut self, delta: isize) {
        let tasks = self.doc_tasks();
        let indices = self.doc_task_indices();
        if indices.is_empty() {
            self.selected_doc_task_line = 0;
            return;
        }
        let current_position = indices
            .iter()
            .position(|index| {
                tasks
                    .get(*index)
                    .map(|task| task.line_index == self.selected_doc_task_line)
                    .unwrap_or(false)
            })
            .unwrap_or(0);
        let next_position = current_position
            .saturating_add_signed(delta)
            .min(indices.len().saturating_sub(1));
        if let Some(task) = tasks.get(indices[next_position]) {
            self.selected_doc_task_line = task.line_index;
        }
    }

    fn jump_to_selected_doc_task(&mut self) {
        let tasks = self.doc_tasks();
        if !self.doc_task_indices().iter().any(|index| {
            tasks
                .get(*index)
                .map(|task| task.line_index == self.selected_doc_task_line)
                .unwrap_or(false)
        }) {
            self.status = "No matching document task".to_string();
            return;
        }
        let line = self
            .selected_doc_task_line
            .min(self.buffer.line_count().saturating_sub(1));
        self.buffer.set_cursor(self.buffer.line_end(line));
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.doc_tasks_open = false;
        self.focus_editor_once = true;
        self.status = format!("Jumped to task on line {}", line + 1);
    }

    fn cycle_selected_doc_task(&mut self) {
        let tasks = self.doc_tasks();
        if !self.doc_task_indices().iter().any(|index| {
            tasks
                .get(*index)
                .map(|task| task.line_index == self.selected_doc_task_line)
                .unwrap_or(false)
        }) {
            self.status = "No matching document task".to_string();
            return;
        }
        if EditorView::cycle_checkbox_at_line(&mut self.buffer, self.selected_doc_task_line) {
            self.dirty = true;
            self.search_state = None;
            self.status = format!("Cycled task on line {}", self.selected_doc_task_line + 1);
        }
    }

    fn open_agenda(&mut self) {
        self.open_agenda_with_filter(String::new());
    }

    fn open_agenda_with_filter(&mut self, filter: String) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.capture_modal_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.ctrl_layer_active = false;
        self.ctrl_layer_sequence.clear();
        self.agenda_filter = filter.trim().to_string();
        self.agenda_start_offset_days = 0;
        self.agenda_tab = AgendaTab::Agenda;
        self.agenda_focus = AgendaFocus::Dated;
        self.agenda_selected_dated_row =
            Local::now().date_naive().weekday().num_days_from_monday() as usize;
        self.agenda_selected_inbox_item = 0;
        self.agenda_selected_tab_items = [0; 5];
        self.agenda_org_mode = None;
        self.agenda_org_query.clear();
        self.selected_agenda_org_suggestion = 0;
        self.flow_item_edit = None;
        self.agenda_scroll_to_selected = false;

        match self.load_agenda_items() {
            Ok(mut items) => {
                self.overlay_current_buffer_agenda_items(&mut items);
                self.agenda_items = items;
                self.clamp_agenda_selection();
                self.agenda_open = true;
                self.focus_editor_once = false;
                let visible_count = self.agenda_visible_item_indices().len();
                self.status = if self.agenda_filter.is_empty() {
                    format!("{} Flow agenda tasks", visible_count)
                } else {
                    format!(
                        "{} Flow agenda tasks matching {}",
                        visible_count, self.agenda_filter
                    )
                };
            }
            Err(err) => {
                let mut items = Vec::new();
                self.overlay_current_buffer_agenda_items(&mut items);
                if items.is_empty() {
                    self.agenda_open = false;
                    self.agenda_items.clear();
                    self.status = err;
                    self.focus_editor_once = true;
                } else {
                    self.agenda_items = items;
                    self.clamp_agenda_selection();
                    self.agenda_open = true;
                    self.focus_editor_once = false;
                    self.status = "Flow agenda from current buffer".to_string();
                }
            }
        }
    }

    fn load_agenda_items(&self) -> Result<Vec<AgendaItem>, String> {
        agenda::load_items_from_cache(&self.flow_sqlite_path()?, &self.flow_scope_root())
    }

    fn overlay_current_buffer_agenda_items(&self, items: &mut Vec<AgendaItem>) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if !flow::should_index_flow_path(path) {
            return;
        }

        let root = Self::flow_project_root_for_path(path);
        agenda::overlay_live_buffer_items(items, path, &root, self.buffer.as_str());
    }

    fn agenda_visible_item_indices(&self) -> Vec<usize> {
        agenda::visible_item_indices(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
        )
    }

    fn agenda_week_start(&self) -> NaiveDate {
        agenda::week_start(self.agenda_start_offset_days)
    }

    fn agenda_items_for_date(&self, date: NaiveDate) -> Vec<usize> {
        agenda::item_indices_for_date(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
            date,
        )
    }

    fn agenda_inbox_item_indices(&self) -> Vec<usize> {
        agenda::inbox_item_indices(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
        )
    }

    fn agenda_flat_item_indices(&self) -> Vec<usize> {
        self.agenda_visible_item_indices()
    }

    fn agenda_dated_rows(&self) -> Vec<AgendaDatedRow> {
        agenda::dated_rows(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
        )
    }

    fn clamp_agenda_selection(&mut self) {
        agenda::clamp_selection(
            &self.agenda_items,
            self.agenda_tab,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
            &mut self.agenda_selected_dated_row,
            &mut self.agenda_selected_inbox_item,
            &mut self.agenda_selected_tab_items,
        );
    }

    fn move_agenda_selection(&mut self, delta: isize) {
        self.agenda_scroll_to_selected = agenda::move_selection(
            &self.agenda_items,
            self.agenda_tab,
            self.agenda_focus,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
            &mut self.agenda_selected_dated_row,
            &mut self.agenda_selected_inbox_item,
            &mut self.agenda_selected_tab_items,
            delta,
        );
    }

    fn move_agenda_week_window(&mut self, delta_weeks: i64) {
        self.agenda_start_offset_days = self
            .agenda_start_offset_days
            .saturating_add(delta_weeks.saturating_mul(7));
        self.agenda_expanded_dates.clear();
        self.agenda_selected_dated_row = 0;
        self.agenda_focus = AgendaFocus::Dated;
        let start_date = self.agenda_week_start();
        self.status = format!("Agenda week starts {start_date}");
        self.agenda_scroll_to_selected = false;
    }

    fn jump_agenda_to_today(&mut self) {
        self.agenda_start_offset_days = 0;
        self.agenda_selected_dated_row =
            Local::now().date_naive().weekday().num_days_from_monday() as usize;
        self.agenda_focus = AgendaFocus::Dated;
        self.status = "Agenda week: current".to_string();
        self.agenda_scroll_to_selected = false;
    }

    fn toggle_agenda_focus(&mut self) {
        if self.agenda_tab != AgendaTab::Agenda {
            return;
        }
        self.agenda_focus = match self.agenda_focus {
            AgendaFocus::Dated => AgendaFocus::Inbox,
            AgendaFocus::Inbox => AgendaFocus::Dated,
        };
        self.clamp_agenda_selection();
        self.agenda_scroll_to_selected = true;
        self.status = match self.agenda_focus {
            AgendaFocus::Dated => "Agenda focus: dated week".to_string(),
            AgendaFocus::Inbox => "Agenda focus: no-date items".to_string(),
        };
    }

    fn cycle_agenda_tab(&mut self) {
        let current = AgendaTab::ALL
            .iter()
            .position(|tab| *tab == self.agenda_tab)
            .unwrap_or(0);
        self.agenda_tab = AgendaTab::ALL[(current + 1) % AgendaTab::ALL.len()];
        self.clamp_agenda_selection();
        self.agenda_scroll_to_selected = true;
        self.status = format!("Agenda tab: {}", self.agenda_tab.label());
    }

    fn toggle_agenda_done_visibility(&mut self) {
        self.agenda_show_done = !self.agenda_show_done;
        self.clamp_agenda_selection();
        self.agenda_scroll_to_selected = true;
        self.status = if self.agenda_show_done {
            "Agenda: showing done tasks".to_string()
        } else {
            "Agenda: hiding done tasks".to_string()
        };
    }

    fn activate_selected_agenda_row(&mut self) {
        if self.agenda_tab != AgendaTab::Agenda {
            let indices = self.agenda_flat_item_indices();
            let tab_index = self.agenda_tab.index();
            let Some(index) = indices
                .get(self.agenda_selected_tab_items[tab_index])
                .copied()
            else {
                self.status = format!("No {} item selected", self.agenda_tab.label());
                return;
            };
            self.jump_to_agenda_item_index(index);
            return;
        }

        match self.agenda_focus {
            AgendaFocus::Dated => {
                let rows = self.agenda_dated_rows();
                let Some(row) = rows.get(self.agenda_selected_dated_row).copied() else {
                    self.status = "No dated agenda row".to_string();
                    return;
                };
                match row {
                    AgendaDatedRow::Day(date) => {
                        if !self.agenda_expanded_dates.insert(date) {
                            self.agenda_expanded_dates.remove(&date);
                            self.status = format!("Collapsed {}", date.format("%A %-d %B"));
                        } else {
                            let count = self.agenda_items_for_date(date).len();
                            self.status = format!(
                                "Expanded {} · {} task{}",
                                date.format("%A %-d %B"),
                                count,
                                if count == 1 { "" } else { "s" }
                            );
                        }
                        self.clamp_agenda_selection();
                    }
                    AgendaDatedRow::Task(index) => self.jump_to_agenda_item_index(index),
                }
            }
            AgendaFocus::Inbox => {
                let indices = self.agenda_inbox_item_indices();
                let Some(index) = indices.get(self.agenda_selected_inbox_item).copied() else {
                    self.status = "No no-date task selected".to_string();
                    return;
                };
                self.jump_to_agenda_item_index(index);
            }
        }
    }

    fn selected_agenda_task_index(&self) -> Option<usize> {
        agenda::selected_item_index(
            &self.agenda_items,
            self.agenda_tab,
            self.agenda_focus,
            &self.agenda_filter,
            self.agenda_show_done,
            self.agenda_start_offset_days,
            &self.agenda_expanded_dates,
            self.agenda_selected_dated_row,
            self.agenda_selected_inbox_item,
            &self.agenda_selected_tab_items,
        )
    }

    fn agenda_org_suggestions(&self) -> Vec<String> {
        let values = self
            .agenda_items
            .iter()
            .flat_map(|item| match self.agenda_org_mode {
                Some(AgendaOrgMode::Tag) => item.tags.clone(),
                Some(AgendaOrgMode::Project) => item.projects.clone(),
                None => Vec::new(),
            });
        organization_suggestions(values, &self.agenda_org_query)
    }

    fn all_flow_org_values(&self, mode: AgendaOrgMode) -> Vec<String> {
        let current_path = self.path.as_ref();
        let mut values = self
            .agenda_items
            .iter()
            .filter(|item| current_path.map(|path| item.path != *path).unwrap_or(true))
            .flat_map(|item| match mode {
                AgendaOrgMode::Tag => item.tags.clone(),
                AgendaOrgMode::Project => item.projects.clone(),
            })
            .collect::<Vec<_>>();
        values.extend(self.current_buffer_flow_org_values(
            mode,
            self.current_flow_inline_org_token_global_range(mode),
        ));
        values
    }

    fn current_buffer_flow_org_values(
        &self,
        mode: AgendaOrgMode,
        exclude: Option<std::ops::Range<usize>>,
    ) -> Vec<String> {
        let prefix = match mode {
            AgendaOrgMode::Tag => '#',
            AgendaOrgMode::Project => '@',
        };
        let mut values = Vec::new();
        let mut line_start = 0;
        for line in self.buffer.as_str().split('\n') {
            if Self::line_supports_flow_inline_org(line) {
                let mut index = 0;
                while index < line.len() {
                    let Some(ch) = line[index..].chars().next() else {
                        break;
                    };
                    if ch != prefix {
                        index += ch.len_utf8();
                        continue;
                    }
                    if index > 0 {
                        if let Some((_, before)) = line[..index].char_indices().next_back() {
                            if !before.is_whitespace() && !matches!(before, '(' | '[' | '{') {
                                index += ch.len_utf8();
                                continue;
                            }
                        }
                    }
                    let value_start = index + ch.len_utf8();
                    let mut end = value_start;
                    while end < line.len() {
                        let Some(next) = line[end..].chars().next() else {
                            break;
                        };
                        if next.is_alphanumeric() || matches!(next, '-' | '_' | '/' | '.') {
                            end += next.len_utf8();
                        } else {
                            break;
                        }
                    }
                    let global_range = line_start + index..line_start + end;
                    let overlaps_excluded = exclude
                        .as_ref()
                        .map(|excluded| {
                            global_range.start < excluded.end && excluded.start < global_range.end
                        })
                        .unwrap_or(false);
                    if !overlaps_excluded && end > value_start {
                        values.push(line[value_start..end].to_string());
                    }
                    index = end.max(index + ch.len_utf8());
                }
            }
            line_start += line.len() + 1;
        }
        values
    }

    fn flow_inline_org_suggestions(&self) -> Vec<String> {
        let Some((mode, query)) = self.current_flow_inline_org_token() else {
            return Vec::new();
        };
        if self.current_flow_inline_org_token_is_complete_existing(mode) {
            return Vec::new();
        }
        let query = query.trim().to_ascii_lowercase();
        let mut values = self.all_flow_org_values(mode);
        values.sort_by_key(|value| value.to_ascii_lowercase());
        values.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        values
            .into_iter()
            .filter(|value| query.is_empty() || value.to_ascii_lowercase().contains(&query))
            .filter(|value| !self.current_line_has_org_value(mode, value))
            .collect()
    }

    fn current_flow_inline_org_token_span(
        &self,
        mode: AgendaOrgMode,
    ) -> Option<(std::ops::Range<usize>, usize)> {
        let (line_index, column) = self.buffer.cursor_line_col();
        let line = self.buffer.line(line_index);
        let (range, _) = org_token_span_at_cursor(line, column, mode)?;
        let cursor_byte = line
            .char_indices()
            .nth(column)
            .map(|(index, _)| index)
            .unwrap_or(line.len());
        Some((range, cursor_byte))
    }

    fn current_flow_inline_org_token_global_start(&self, mode: AgendaOrgMode) -> Option<usize> {
        self.current_flow_inline_org_token_global_range(mode)
            .map(|range| range.start)
    }

    fn current_flow_inline_org_token_global_range(
        &self,
        mode: AgendaOrgMode,
    ) -> Option<std::ops::Range<usize>> {
        let (line_index, _) = self.buffer.cursor_line_col();
        let (range, _) = self.current_flow_inline_org_token_span(mode)?;
        let line_start = self.buffer.line_start(line_index);
        Some(line_start + range.start..line_start + range.end)
    }

    fn current_flow_inline_org_token_is_complete_existing(&self, mode: AgendaOrgMode) -> bool {
        let (line_index, _) = self.buffer.cursor_line_col();
        let line = self.buffer.line(line_index);
        let Some((range, _)) = self.current_flow_inline_org_token_span(mode) else {
            return false;
        };
        let prefix_len = match mode {
            AgendaOrgMode::Tag => '#'.len_utf8(),
            AgendaOrgMode::Project => '@'.len_utf8(),
        };
        let value = line[range.start + prefix_len..range.end].trim();
        !value.is_empty()
            && self
                .all_flow_org_values(mode)
                .iter()
                .any(|known| known.eq_ignore_ascii_case(value))
    }

    fn current_line_has_org_value(&self, mode: AgendaOrgMode, value: &str) -> bool {
        if value.trim().is_empty() {
            return false;
        }
        let (line_index, _) = self.buffer.cursor_line_col();
        let prefix = match mode {
            AgendaOrgMode::Tag => '#',
            AgendaOrgMode::Project => '@',
        };
        self.buffer.line(line_index).split_whitespace().any(|word| {
            word.strip_prefix(prefix)
                .map(|candidate| {
                    let candidate = candidate.trim_end_matches(|ch: char| {
                        !ch.is_alphanumeric() && !matches!(ch, '-' | '_' | '/' | '.')
                    });
                    candidate.eq_ignore_ascii_case(value.trim())
                })
                .unwrap_or(false)
        })
    }

    fn current_flow_inline_org_token(&self) -> Option<(AgendaOrgMode, String)> {
        let (line_index, column) = self.buffer.cursor_line_col();
        let line = self.buffer.line(line_index);
        if !Self::line_supports_flow_inline_org(line) {
            return None;
        }
        for mode in [AgendaOrgMode::Tag, AgendaOrgMode::Project] {
            if let Some(query) = org_query_at_cursor(line, column, mode) {
                return Some((mode, query));
            }
        }
        None
    }

    fn line_supports_flow_inline_org(line: &str) -> bool {
        let trimmed = line.trim_start();
        parse_checkbox_line(line).is_some()
            || trimmed.starts_with(":note")
            || trimmed.starts_with(":nota")
            || trimmed.starts_with(":idea")
            || trimmed.starts_with(": ")
    }

    fn refresh_flow_inline_org_state(&mut self, typed: Option<&str>) {
        let current = self.current_flow_inline_org_token();
        let current_query_empty = current
            .as_ref()
            .map(|(_, query)| query.trim().is_empty())
            .unwrap_or(true);
        let explicit_trigger = typed.and_then(|text| {
            text.chars().rev().find_map(|ch| match ch {
                '#' => Some(AgendaOrgMode::Tag),
                '@' => Some(AgendaOrgMode::Project),
                _ => None,
            })
        });
        let mut next_anchor = self.flow_inline_org_anchor;
        let next_mode = match (
            self.flow_inline_org_mode,
            explicit_trigger,
            current.as_ref(),
        ) {
            (_, Some(trigger), Some((current_mode, _))) if trigger == *current_mode => {
                next_anchor = self.current_flow_inline_org_token_global_start(trigger);
                Some(trigger)
            }
            (Some(active), _, Some((current_mode, _))) if active == *current_mode => {
                let current_anchor = self.current_flow_inline_org_token_global_start(active);
                if current_anchor.is_some() && current_anchor == self.flow_inline_org_anchor {
                    Some(active)
                } else {
                    None
                }
            }
            _ => None,
        };
        if next_mode != self.flow_inline_org_mode {
            self.selected_flow_inline_org_suggestion = 0;
        }
        self.flow_inline_org_mode = next_mode;
        self.flow_inline_org_anchor = next_mode.and(next_anchor);
        if next_mode.is_none() {
            self.close_flow_inline_org_autocomplete();
            return;
        }
        let suggestions = self.flow_inline_org_suggestions();
        if suggestions.is_empty() {
            if current_query_empty {
                return;
            }
            self.close_flow_inline_org_autocomplete();
            return;
        }
        let len = suggestions.len();
        self.selected_flow_inline_org_suggestion = self
            .selected_flow_inline_org_suggestion
            .min(len.saturating_sub(1));
    }

    fn close_flow_inline_org_autocomplete(&mut self) {
        self.flow_inline_org_mode = None;
        self.flow_inline_org_anchor = None;
        self.selected_flow_inline_org_suggestion = 0;
    }

    fn confirm_flow_inline_org_autocomplete(&mut self, ctx: &egui::Context) {
        let Some((mode, query)) = self.current_flow_inline_org_token() else {
            self.close_flow_inline_org_autocomplete();
            return;
        };
        let suggestions = self.flow_inline_org_suggestions();
        let value = suggestions
            .get(self.selected_flow_inline_org_suggestion)
            .cloned()
            .unwrap_or(query);
        let Some(token) = organization_token(mode, &value) else {
            self.close_flow_inline_org_autocomplete();
            return;
        };
        let (line_index, column) = self.buffer.cursor_line_col();
        let line_start = self.buffer.line_start(line_index);
        let mut line = self.buffer.line(line_index).to_string();
        let next_column = replace_org_token_at_cursor(&mut line, column, mode, &token);
        let line_end = self.buffer.line_end(line_index);
        self.buffer
            .replace_selection_or_range(line_start, line_end, &line);
        let next_byte = line_start
            + line
                .char_indices()
                .nth(next_column)
                .map(|(index, _)| index)
                .unwrap_or(line.len());
        self.buffer.set_cursor(next_byte);
        self.dirty = true;
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.sync_current_flow_buffer_to_agenda(ctx);
        self.close_flow_inline_org_autocomplete();
        self.status = format!("Inserted {token}");
    }

    fn open_agenda_org_autocomplete(&mut self, mode: AgendaOrgMode) {
        if self.selected_agenda_task_index().is_none() {
            self.status = "No agenda item selected".to_string();
            return;
        }
        self.agenda_org_mode = Some(mode);
        self.agenda_org_query.clear();
        self.selected_agenda_org_suggestion = 0;
        self.status = match mode {
            AgendaOrgMode::Tag => "Agenda tag autocomplete (#)".to_string(),
            AgendaOrgMode::Project => "Agenda project autocomplete (@)".to_string(),
        };
    }

    fn close_agenda_org_autocomplete(&mut self) {
        self.agenda_org_mode = None;
        self.agenda_org_query.clear();
        self.selected_agenda_org_suggestion = 0;
    }

    fn confirm_agenda_org_autocomplete(&mut self, ctx: &egui::Context) {
        let Some(mode) = self.agenda_org_mode else {
            return;
        };
        let Some(index) = self.selected_agenda_task_index() else {
            self.close_agenda_org_autocomplete();
            return;
        };
        let suggestions = self.agenda_org_suggestions();
        let value = suggestions
            .get(self.selected_agenda_org_suggestion)
            .cloned()
            .unwrap_or_else(|| self.agenda_org_query.trim().to_string());
        let Some(token) = organization_token(mode, &value) else {
            self.close_agenda_org_autocomplete();
            return;
        };
        self.close_agenda_org_autocomplete();
        self.append_token_to_agenda_item(index, &token, ctx);
    }

    fn open_selected_flow_item_edit(&mut self) {
        let Some(index) = self.selected_agenda_task_index() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        match self.agenda_item_source_line(&item) {
            Ok(line) => {
                self.flow_item_edit = Some(FlowItemEditState::new(index, line));
                self.close_agenda_org_autocomplete();
                self.status = "Editing Flow item · Ctrl+Enter save · Esc cancel".to_string();
            }
            Err(err) => self.status = err,
        }
    }

    fn agenda_item_source_line(&self, item: &AgendaItem) -> Result<String, String> {
        if self.path.as_ref() == Some(&item.path) {
            return Ok(self.buffer.line(item.line_index).to_string());
        }
        let contents = fs::read_to_string(&item.path)
            .map_err(|err| format!("Flow edit read failed: {err}"))?;
        contents
            .lines()
            .nth(item.line_index)
            .map(str::to_string)
            .ok_or_else(|| "Flow edit line not found".to_string())
    }

    fn confirm_flow_item_edit(&mut self, index: usize, replacement: String, ctx: &egui::Context) {
        self.flow_item_edit = None;
        self.replace_agenda_item_source_line(index, &replacement, ctx);
    }

    fn replace_agenda_item_source_line(
        &mut self,
        index: usize,
        replacement: &str,
        ctx: &egui::Context,
    ) {
        let Some(item) = self.agenda_items.get(index).cloned() else {
            return;
        };
        if self.standalone_agenda && self.flow_item_is_active_in_live_editor(&item) {
            match append_flow_edit_request(&self.flow_scope_root(), &item, replacement) {
                Ok(()) => {
                    self.status = format!(
                        "Requested edit for {}:{}",
                        item.relative_path, item.line_number
                    );
                    return;
                }
                Err(err) => {
                    self.status = err;
                    return;
                }
            }
        }
        if self.path.as_ref() == Some(&item.path) {
            if self.apply_flow_line_edit_to_buffer(item.line_index, replacement) {
                self.dirty = true;
                self.publish_current_flow_buffer_to_agenda(ctx);
                self.status = "Flow item edited".to_string();
            }
            return;
        }
        match Self::replace_flow_line_in_file(&self.flow_scope_root(), &item, replacement) {
            Ok(()) => {
                self.status = format!("Edited {}:{}", item.relative_path, item.line_number);
                if let Ok(mut items) = self.load_agenda_items() {
                    self.overlay_current_buffer_agenda_items(&mut items);
                    self.agenda_items = items;
                    self.clamp_agenda_selection();
                }
            }
            Err(err) => self.status = err,
        }
    }

    fn append_token_to_agenda_item(&mut self, index: usize, token: &str, ctx: &egui::Context) {
        let Some(item) = self.agenda_items.get(index).cloned() else {
            return;
        };
        if self.standalone_agenda && self.flow_item_is_active_in_live_editor(&item) {
            match append_flow_organization_request(&self.flow_scope_root(), &item, token) {
                Ok(()) => {
                    self.status = format!(
                        "Requested {token} for {}:{}",
                        item.relative_path, item.line_number
                    );
                    return;
                }
                Err(err) => {
                    self.status = err;
                    return;
                }
            }
        }
        if self.path.as_ref() == Some(&item.path) {
            if self.apply_flow_token_to_buffer(item.line_index, token) {
                self.dirty = true;
                self.publish_current_flow_buffer_to_agenda(ctx);
                self.status = format!("Added {token} to agenda item");
            }
            return;
        }
        match Self::append_flow_token_to_file(&self.flow_scope_root(), &item, token) {
            Ok(()) => {
                self.status = format!(
                    "Added {token} to {}:{}",
                    item.relative_path, item.line_number
                );
                if let Ok(mut items) = self.load_agenda_items() {
                    self.overlay_current_buffer_agenda_items(&mut items);
                    self.agenda_items = items;
                    self.clamp_agenda_selection();
                }
            }
            Err(err) => self.status = err,
        }
    }

    fn apply_flow_token_to_buffer(&mut self, line_index: usize, token: &str) -> bool {
        let candidate_indices = [line_index, line_index.saturating_sub(1), line_index + 1];
        for index in candidate_indices {
            if index >= self.buffer.line_count() {
                continue;
            }
            let line = self.buffer.line(index).to_string();
            if line
                .split_whitespace()
                .any(|word| word.eq_ignore_ascii_case(token))
            {
                return true;
            }
            if parse_checkbox_line(&line).is_none()
                && flow::parse_flow_note_or_idea_line(&line, index, None).is_none()
            {
                continue;
            }
            let line_end = self.buffer.line_end(index);
            self.buffer.set_cursor(line_end);
            self.buffer.insert_text(&format!(" {token}"));
            return true;
        }
        false
    }

    fn replace_flow_line_in_file(
        scope: &Path,
        item: &AgendaItem,
        replacement: &str,
    ) -> Result<(), String> {
        let contents = fs::read_to_string(&item.path)
            .map_err(|err| format!("Flow edit read failed: {err}"))?;
        let mut lines = contents.lines().map(str::to_string).collect::<Vec<_>>();
        let Some(line) = lines.get_mut(item.line_index) else {
            return Err("Flow edit line not found".to_string());
        };
        *line = replacement.to_string();
        let mut next = lines.join("\n");
        if contents.ends_with('\n') {
            next.push('\n');
        }
        let tmp = item.path.with_extension(format!(
            "{}.tmp",
            item.path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("md")
        ));
        fs::write(&tmp, next).map_err(|err| format!("Flow edit write failed: {err}"))?;
        fs::rename(&tmp, &item.path).map_err(|err| format!("Flow edit replace failed: {err}"))?;
        let updated = fs::read_to_string(&item.path)
            .map_err(|err| format!("Flow edit reindex read failed: {err}"))?;
        flow::update_flow_index_for_file(
            scope,
            &Self::flow_sqlite_path_for_root(scope)?,
            &item.path,
            &updated,
        )?;
        Ok(())
    }

    fn append_flow_token_to_file(
        scope: &Path,
        item: &AgendaItem,
        token: &str,
    ) -> Result<(), String> {
        let contents = fs::read_to_string(&item.path)
            .map_err(|err| format!("Flow organization read failed: {err}"))?;
        let mut lines = contents.lines().map(str::to_string).collect::<Vec<_>>();
        let Some(line) = lines.get_mut(item.line_index) else {
            return Err("Flow organization line not found".to_string());
        };
        if line
            .split_whitespace()
            .any(|word| word.eq_ignore_ascii_case(token))
        {
            return Ok(());
        }
        line.push(' ');
        line.push_str(token);
        let mut next = lines.join("\n");
        if contents.ends_with('\n') {
            next.push('\n');
        }
        let tmp = item.path.with_extension(format!(
            "{}.tmp",
            item.path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("md")
        ));
        fs::write(&tmp, next).map_err(|err| format!("Flow organization write failed: {err}"))?;
        fs::rename(&tmp, &item.path)
            .map_err(|err| format!("Flow organization replace failed: {err}"))?;
        let updated = fs::read_to_string(&item.path)
            .map_err(|err| format!("Flow organization reindex read failed: {err}"))?;
        flow::update_flow_index_for_file(
            scope,
            &Self::flow_sqlite_path_for_root(scope)?,
            &item.path,
            &updated,
        )?;
        Ok(())
    }

    fn jump_to_agenda_item_index(&mut self, index: usize) {
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        if self.standalone_agenda {
            self.open_agenda_item_in_main_slate(&item);
            return;
        }
        if self.path.as_ref() != Some(&item.path) {
            if self.dirty {
                self.status = "Save current buffer before agenda jump".to_string();
                return;
            }
            self.open_path(item.path.clone());
        }
        let line = item
            .line_index
            .min(self.buffer.line_count().saturating_sub(1));
        self.buffer.set_cursor(self.buffer.line_end(line));
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.agenda_open = false;
        self.focus_editor_once = true;
        self.status = format!("Jumped to agenda item on line {}", line + 1);
    }

    fn open_agenda_item_in_main_slate(&mut self, item: &AgendaItem) {
        if self.flow_item_is_active_in_live_editor(item) {
            if append_flow_open_request(
                &self.flow_scope_root(),
                &item.relative_path,
                item.line_index,
            )
            .is_ok()
            {
                self.status = format!(
                    "Jumping existing Slate window to {}:{}",
                    item.relative_path, item.line_number
                );
                self.agenda_open = false;
                self.focus_editor_once = false;
                return;
            }
        }

        let line_number = item.line_index.saturating_add(1).to_string();
        let slate_bin = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".local/bin/slate"))
            .filter(|path| path.exists())
            .unwrap_or_else(|| PathBuf::from("slate"));
        match ProcessCommand::new(&slate_bin)
            .arg(&item.path)
            .arg("--line")
            .arg(&line_number)
            .spawn()
        {
            Ok(_) => {
                self.status = format!("Opening {}:{}", item.path.display(), line_number);
                self.agenda_open = false;
                self.focus_editor_once = false;
            }
            Err(err) => {
                self.status = format!("Agenda open failed: {err}");
            }
        }
    }

    fn cycle_selected_agenda_item(&mut self) {
        let Some(index) = self.selected_agenda_task_index() else {
            self.status = "Select an agenda task before cycling it".to_string();
            return;
        };
        let Some(item) = self.agenda_items.get(index).cloned() else {
            self.status = "No agenda item selected".to_string();
            return;
        };
        if item.kind != "task" {
            self.status = format!("Agenda cycle only applies to tasks, not {}", item.kind);
            return;
        }
        if self.dirty {
            self.status = "Save current buffer before agenda cycle".to_string();
            return;
        }

        if self.path.as_ref() == Some(&item.path) {
            let line = item
                .line_index
                .min(self.buffer.line_count().saturating_sub(1));
            if !Self::cycle_flow_buffer_line(&mut self.buffer, line, item.line_number) {
                self.status = format!("Agenda cycle failed: no checkbox on line {}", line + 1);
                return;
            }
            self.search_state = None;
            self.dirty = true;
            self.save_path(item.path.clone());
        } else {
            let text = match fs::read_to_string(&item.path) {
                Ok(text) => text,
                Err(err) => {
                    self.status = format!("Agenda cycle failed: {err}");
                    return;
                }
            };
            let mut buffer = EditorBuffer::from_text(text);
            let line = item.line_index.min(buffer.line_count().saturating_sub(1));
            if !Self::cycle_flow_buffer_line(&mut buffer, line, item.line_number) {
                self.status = format!("Agenda cycle failed: no checkbox on line {}", line + 1);
                return;
            }
            if let Err(err) = fs::write(&item.path, buffer.as_str()) {
                self.status = format!("Agenda cycle failed: {err}");
                return;
            }
            if let Err(err) = self.refresh_flow_index_for_path(&item.path, buffer.as_str()) {
                self.status = err;
                return;
            }
        }

        match self.load_agenda_items() {
            Ok(items) => {
                self.agenda_items = items;
                self.clamp_agenda_selection();
                self.agenda_scroll_to_selected = true;
                let state = self
                    .agenda_items
                    .iter()
                    .find(|candidate| {
                        candidate.path == item.path && candidate.line_index == item.line_index
                    })
                    .map(|item| agenda::state_marker(&item.state))
                    .unwrap_or("[ ]");
                self.status = format!(
                    "Cycled agenda task to {state} on {}:{}",
                    item.relative_path, item.line_number
                );
            }
            Err(err) => {
                self.status = format!("Agenda cycle updated source but reload failed: {err}");
            }
        }
    }

    fn cycle_flow_buffer_line(
        buffer: &mut EditorBuffer,
        line_index: usize,
        line_number: usize,
    ) -> bool {
        if EditorView::cycle_checkbox_at_line(buffer, line_index) {
            return true;
        }
        let fallback = line_number.saturating_sub(1);
        fallback != line_index && EditorView::cycle_checkbox_at_line(buffer, fallback)
    }

    fn project_root(&self) -> PathBuf {
        self.path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or_else(|| self.last_open_dir.clone())
            .or_else(|| {
                self.last_opened_path
                    .as_ref()
                    .and_then(|path| path.parent().map(PathBuf::from))
            })
            .or_else(|| {
                self.recent_files
                    .first()
                    .and_then(|path| path.parent().map(PathBuf::from))
            })
            .or_else(|| std::env::current_dir().ok())
            .or_else(dirs_next::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn open_file_picker_for_open(&mut self) {
        self.open_file_picker_at(self.project_root(), FilePickerMode::Open);
    }

    fn open_file_picker_for_buffer(&mut self) {
        self.open_file_picker_at(self.project_root(), FilePickerMode::OpenBuffer);
    }

    fn open_file_picker_at(&mut self, dir: PathBuf, mode: FilePickerMode) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.lance_open = false;
        self.save_as_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.file_picker_open = true;
        self.file_picker_mode = mode;
        self.file_picker_dir = dir;
        self.file_query.clear();
        self.project_files = file_dialogs::scan_directory_entries(
            &self.file_picker_dir,
            file_dialogs::default_should_skip_dir,
        );
        self.selected_project_file = self.project_file_indices().first().copied().unwrap_or(0);
        self.status = format!("Files: {}", self.file_picker_dir.display());
        self.focus_editor_once = false;
    }

    fn project_file_indices(&self) -> Vec<usize> {
        file_dialogs::picker_entry_indices(
            &self.project_files,
            &self.file_picker_dir,
            &self.file_query,
        )
    }

    fn move_project_file_selection(&mut self, delta: isize) {
        let indices = self.project_file_indices();
        self.selected_project_file =
            file_dialogs::move_selected_index(&indices, self.selected_project_file, delta);
    }

    fn refresh_file_picker_entries(&mut self) {
        self.project_files = file_dialogs::scan_directory_entries(
            &self.file_picker_dir,
            file_dialogs::default_should_skip_dir,
        );
        self.selected_project_file = self.project_file_indices().first().copied().unwrap_or(0);
        self.status = format!("Files: {}", self.file_picker_dir.display());
    }

    fn file_picker_enter_selected_dir(&mut self) {
        let Some(path) = self.project_files.get(self.selected_project_file).cloned() else {
            return;
        };
        if path.is_dir() {
            self.file_picker_dir = path;
            self.file_query.clear();
            self.refresh_file_picker_entries();
        }
    }

    fn file_picker_go_parent(&mut self) {
        let Some(child) = self.file_picker_dir.file_name().map(|name| name.to_owned()) else {
            return;
        };
        if let Some(parent) = self.file_picker_dir.parent().map(PathBuf::from) {
            self.file_picker_dir = parent;
            self.file_query.clear();
            self.refresh_file_picker_entries();
            if let Some(index) = self
                .project_files
                .iter()
                .position(|path| path.file_name() == Some(child.as_os_str()))
            {
                self.selected_project_file = index;
            }
        }
    }

    fn open_selected_project_file(&mut self) {
        if !self
            .project_file_indices()
            .contains(&self.selected_project_file)
        {
            self.status = "No matching file".to_string();
            return;
        }
        let Some(path) = self.project_files.get(self.selected_project_file).cloned() else {
            self.status = "No file selected".to_string();
            return;
        };
        if path.is_dir() {
            if self.file_picker_mode == FilePickerMode::SetPeriscopeProject {
                self.file_picker_open = false;
                self.periscope_project_root = Some(path.clone());
                self.periscope_mode = PeriscopeMode::Project;
                self.periscope_open = true;
                self.periscope_query.clear();
                self.periscope_results.clear();
                self.periscope_expanded_folders.clear();
                self.selected_periscope_result = 0;
                self.refresh_periscope_results();
                self.status = format!("Periscope project: {}", path.display());
                return;
            }
            self.file_picker_dir = path;
            self.file_query.clear();
            self.refresh_file_picker_entries();
            return;
        }
        match self.file_picker_mode {
            FilePickerMode::InsertMarkdownLink => {
                let target = link_ui::markdown_link_target_for_file(&path, self.path.as_deref());
                self.file_picker_open = false;
                self.open_link_heading_picker_or_insert(&path, target);
            }
            FilePickerMode::Refile => {
                self.file_picker_open = false;
                self.open_refile_heading_picker_or_finish(path);
            }
            FilePickerMode::OpenBuffer => {
                self.file_picker_open = false;
                self.open_link_preview(path, None);
            }
            FilePickerMode::SetPeriscopeProject => {
                self.status = "Choose a folder for Periscope project".to_string();
            }
            FilePickerMode::AddLance => {
                self.file_picker_open = false;
                self.add_lance_file(path);
                self.open_lance();
            }
            FilePickerMode::Open if self.dirty => {
                self.pending_project_file_path = Some(path);
                self.confirm(PendingAction::OpenProjectFile);
            }
            FilePickerMode::Open | FilePickerMode::Browse => {
                self.file_picker_open = false;
                self.open_path(path);
                self.focus_editor_once = true;
            }
        }
    }

    fn open_periscope(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.lance_open = false;
        self.doc_tasks_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.periscope_query.clear();
        self.periscope_results.clear();
        self.periscope_expanded_folders.clear();
        self.selected_periscope_result = 0;
        self.periscope_backend_status.clear();
        self.focus_editor_once = false;

        if let Some(root) = self.detect_project_root() {
            self.periscope_project_root = Some(root.clone());
            self.periscope_mode = PeriscopeMode::Project;
            self.periscope_open = true;
            self.periscope_prompt_open = false;
            self.refresh_periscope_results();
            self.status = format!("Periscope project: {}", root.display());
        } else {
            self.periscope_open = false;
            self.periscope_prompt_open = true;
            self.status = "No .git project root found".to_string();
        }
    }

    fn detect_project_root(&self) -> Option<PathBuf> {
        let mut dir = self
            .path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or_else(|| self.last_open_dir.clone())
            .or_else(|| std::env::current_dir().ok())?;
        loop {
            if dir.join(".git").is_dir() {
                return Some(dir);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    fn open_periscope_project_folder_picker(&mut self) {
        self.periscope_prompt_open = false;
        self.open_file_picker_at(self.project_root(), FilePickerMode::SetPeriscopeProject);
        self.status = "Choose Periscope project folder with Enter".to_string();
    }

    fn open_global_periscope(&mut self) {
        self.periscope_prompt_open = false;
        self.periscope_open = true;
        self.periscope_mode = PeriscopeMode::Global;
        self.periscope_query.clear();
        self.periscope_results.clear();
        self.periscope_expanded_folders.clear();
        self.selected_periscope_result = 0;
        self.refresh_periscope_results();
        self.status = "Periscope global".to_string();
        self.focus_editor_once = false;
    }

    fn toggle_periscope_mode(&mut self) {
        if !self.periscope_open {
            return;
        }
        match self.periscope_mode {
            PeriscopeMode::Project => {
                self.periscope_mode = PeriscopeMode::Global;
                self.status = "Periscope global".to_string();
            }
            PeriscopeMode::Global => {
                if self.periscope_project_root.is_some() {
                    self.periscope_mode = PeriscopeMode::Project;
                    self.status = "Periscope project".to_string();
                } else {
                    self.status = "No Periscope project root set".to_string();
                }
            }
        }
        self.periscope_expanded_folders.clear();
        self.selected_periscope_result = 0;
        self.refresh_periscope_results();
    }

    fn start_periscope_global_index_load(&mut self) {
        if self.periscope_global_loading || !self.periscope_global_paths.is_empty() {
            return;
        }
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            self.periscope_backend_status = "Home Periscope needs $HOME".to_string();
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.periscope_global_rx = Some(rx);
        self.periscope_global_loading = true;
        self.periscope_backend_status = format!("Home index: {} · loading…", home.display());
        thread::spawn(move || {
            let _ = tx.send(periscope::load_global_index(&home));
        });
    }

    fn poll_periscope_global_index(&mut self) {
        let Some(rx) = &self.periscope_global_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(paths) => {
                self.periscope_global_paths = paths;
                self.periscope_global_rx = None;
                self.periscope_global_loading = false;
                self.periscope_backend_status = format!(
                    "Home index loaded: {} files · in-memory fuzzy · capped at 100 results",
                    self.periscope_global_paths.len()
                );
                if self.periscope_mode == PeriscopeMode::Global {
                    self.periscope_results = periscope::cached_results(
                        &self.periscope_global_paths,
                        &self.periscope_query,
                        PERISCOPE_RESULT_LIMIT,
                    );
                    self.selected_periscope_result = 0;
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.periscope_global_rx = None;
                self.periscope_global_loading = false;
                self.periscope_backend_status = "Home index load failed".to_string();
            }
        }
    }

    fn periscope_global_results(&mut self, query: &str) -> Vec<PathBuf> {
        self.start_periscope_global_index_load();
        if self.periscope_global_paths.is_empty() {
            self.periscope_backend_status = if self.periscope_global_loading {
                "Home index loading… type now, results will appear when ready".to_string()
            } else {
                "Home index unavailable".to_string()
            };
            return Vec::new();
        }
        let query = query.trim();
        if query.is_empty() {
            self.periscope_backend_status = format!(
                "Home index: {} files · type to search · capped at 100 results",
                self.periscope_global_paths.len()
            );
            return Vec::new();
        }
        self.periscope_backend_status = format!(
            "Home index: {} files · in-memory fuzzy · capped at 100 results",
            self.periscope_global_paths.len()
        );
        periscope::cached_results(&self.periscope_global_paths, query, PERISCOPE_RESULT_LIMIT)
    }

    fn periscope_rows(&self) -> Vec<PeriscopeRow> {
        periscope::rows(
            &self.periscope_results,
            &self.periscope_query,
            &self.periscope_expanded_folders,
        )
    }

    fn selected_periscope_row(&self) -> Option<PeriscopeRow> {
        periscope::selected_row(
            &self.periscope_results,
            &self.periscope_query,
            &self.periscope_expanded_folders,
            self.selected_periscope_result,
        )
    }

    fn toggle_selected_periscope_folder(&mut self) -> bool {
        let row = self.selected_periscope_row();
        periscope::toggle_folder(&mut self.periscope_expanded_folders, row)
    }

    fn refresh_periscope_results(&mut self) {
        self.periscope_results = match self.periscope_mode {
            PeriscopeMode::Project => self
                .periscope_project_root
                .clone()
                .map(|root| {
                    periscope::project_results(&root, &self.periscope_query, PERISCOPE_RESULT_LIMIT)
                })
                .unwrap_or_default(),
            PeriscopeMode::Global => {
                let query = self.periscope_query.clone();
                self.periscope_global_results(&query)
            }
        };
        self.selected_periscope_result = self
            .selected_periscope_result
            .min(self.periscope_rows().len().saturating_sub(1));
    }

    fn move_periscope_selection(&mut self, delta: isize) {
        self.selected_periscope_result = periscope::move_selection(
            self.periscope_rows().len(),
            self.selected_periscope_result,
            delta,
        );
    }

    fn open_selected_periscope_result(&mut self) {
        if self.toggle_selected_periscope_folder() {
            return;
        }
        let Some(PeriscopeRow::File { path, .. }) = self.selected_periscope_row() else {
            self.status = "No Periscope result selected".to_string();
            return;
        };
        if self.dirty {
            self.status = "Save or discard changes before opening Periscope result".to_string();
            return;
        }
        self.periscope_open = false;
        self.open_path(path);
        self.focus_editor_once = true;
    }

    fn open_selected_periscope_result_as_modal(&mut self) {
        if self.toggle_selected_periscope_folder() {
            return;
        }
        let Some(PeriscopeRow::File { path, .. }) = self.selected_periscope_row() else {
            self.status = "No Periscope result selected".to_string();
            return;
        };
        self.periscope_open = false;
        self.open_link_preview(path, None);
    }

    fn handle_periscope_text_input(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Text(text) = event {
                    if !text.chars().any(char::is_control) {
                        self.periscope_query.push_str(text);
                        changed = true;
                    }
                }
            }
        });
        if changed {
            self.periscope_expanded_folders.clear();
            self.selected_periscope_result = 0;
            self.refresh_periscope_results();
        }
    }

    fn save_as(&mut self) {
        self.open_save_as_modal();
    }

    fn open_save_as_modal(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.save_as_dir = self
            .path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or_else(|| self.last_open_dir.clone())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        self.save_as_filename = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("untitled.md")
            .to_string();
        self.save_as_entries = file_dialogs::scan_directory_entries(
            &self.save_as_dir,
            file_dialogs::default_should_skip_dir,
        );
        self.selected_save_as_entry = self.save_as_entry_indices().first().copied().unwrap_or(0);
        self.save_as_open = true;
        self.focus_editor_once = false;
        self.status = format!("Save as: {}", self.save_as_dir.display());
    }

    fn save_as_entry_indices(&self) -> Vec<usize> {
        file_dialogs::named_entry_indices(&self.save_as_entries, &self.save_as_filename)
    }

    fn refresh_save_as_entries(&mut self) {
        self.save_as_entries = file_dialogs::scan_directory_entries(
            &self.save_as_dir,
            file_dialogs::default_should_skip_dir,
        );
        self.selected_save_as_entry = self.save_as_entry_indices().first().copied().unwrap_or(0);
        self.status = format!("Save as: {}", self.save_as_dir.display());
    }

    fn move_save_as_selection(&mut self, delta: isize) {
        let indices = self.save_as_entry_indices();
        self.selected_save_as_entry =
            file_dialogs::move_selected_index(&indices, self.selected_save_as_entry, delta);
    }

    fn save_as_enter_selected_dir(&mut self) {
        let Some(path) = self
            .save_as_entries
            .get(self.selected_save_as_entry)
            .cloned()
        else {
            return;
        };
        if path.is_dir() {
            self.save_as_dir = path;
            self.save_as_filename.clear();
            self.refresh_save_as_entries();
        }
    }

    fn save_as_go_parent(&mut self) {
        let Some(child) = self.save_as_dir.file_name().map(|name| name.to_owned()) else {
            return;
        };
        if let Some(parent) = self.save_as_dir.parent().map(PathBuf::from) {
            self.save_as_dir = parent;
            self.save_as_filename.clear();
            self.refresh_save_as_entries();
            if let Some(index) = self
                .save_as_entries
                .iter()
                .position(|path| path.file_name() == Some(child.as_os_str()))
            {
                self.selected_save_as_entry = index;
            }
        }
    }

    fn confirm_save_as(&mut self) {
        let filename = self.save_as_filename.trim();
        let path = if filename.is_empty() {
            let Some(selected) = self
                .save_as_entries
                .get(self.selected_save_as_entry)
                .cloned()
            else {
                self.status = "Save as needs a file name".to_string();
                return;
            };
            if selected.is_dir() {
                self.save_as_dir = selected;
                self.refresh_save_as_entries();
                return;
            }
            selected
        } else {
            let input = PathBuf::from(filename);
            if input.is_absolute() {
                input
            } else {
                self.save_as_dir.join(input)
            }
        };
        self.save_as_open = false;
        self.save_path(path);
        self.focus_editor_once = true;
    }

    fn settings_path() -> Option<PathBuf> {
        let mut dir = dirs_next::config_dir()?;
        dir.push("slate");
        Some(dir.join("config.toml"))
    }

    fn load_settings(&mut self) {
        let Some(path) = Self::settings_path() else {
            return;
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };

        for line in contents.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "command_history_limit" => {
                    if let Ok(limit) = value.trim().parse::<usize>() {
                        self.command_history_limit = clamp_history_limit(limit);
                    }
                }
                "line_number_mode" => {
                    if let Some(mode) = LineNumberMode::from_config_value(value) {
                        self.line_number_mode = mode;
                    }
                }
                "word_wrap" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.wrap = enabled;
                    }
                }
                "preview_mode" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.preview = enabled;
                    }
                }
                "ctrl_shift_move_mode" => {
                    if let Some(mode) = CtrlShiftMoveMode::from_config_value(value) {
                        self.ctrl_shift_move_mode = mode;
                    }
                }
                "reopen_last_file_on_startup" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.reopen_last_file_on_startup = enabled;
                    }
                }
                "markdown_live_rendering" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.markdown_live_rendering = enabled;
                    }
                }
                "virtual_references_enabled" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.virtual_references_enabled = enabled;
                    }
                }
                "virtual_references_collapsed" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.virtual_references_collapsed = enabled;
                    }
                }
                "virtual_backlinks_collapsed" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.virtual_backlinks_collapsed = enabled;
                    }
                }
                "virtual_mentions_collapsed" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.virtual_mentions_collapsed = enabled;
                    }
                }
                "last_opened_path" => {
                    let value = Self::parse_config_string(value);
                    if !value.is_empty() {
                        self.last_opened_path = Some(PathBuf::from(value));
                    }
                }
                "last_open_dir" => {
                    let value = Self::parse_config_string(value);
                    if !value.is_empty() {
                        self.last_open_dir = Some(PathBuf::from(value));
                    }
                }
                "command_history" => {
                    let value = Self::parse_config_string(value);
                    if !value.is_empty() {
                        commands::push_command_history(
                            &mut self.command_history,
                            &value,
                            self.command_history_limit,
                        );
                    }
                }
                "recent_file" => {
                    let value = Self::parse_config_string(value);
                    if !value.is_empty() {
                        let path = PathBuf::from(value);
                        if !self.recent_files.contains(&path) {
                            self.recent_files.push(path);
                        }
                    }
                }
                "lance_file" | "harpoon_file" => {
                    let value = Self::parse_config_string(value);
                    if !value.is_empty() {
                        let path = PathBuf::from(value);
                        if !self.lance_files.contains(&path) {
                            self.lance_files.push(path);
                        }
                    }
                }
                "file_cursor" => {
                    let value = Self::parse_config_string(value);
                    if let Some((path, line_index, column)) =
                        Self::parse_file_cursor_position(&value)
                    {
                        if !self
                            .file_cursor_positions
                            .iter()
                            .any(|entry| entry.path == path)
                        {
                            self.file_cursor_positions.push(FileCursorPosition {
                                path,
                                line_index,
                                column,
                            });
                        }
                    }
                }
                "command_usage" => {
                    let value = Self::parse_config_string(value);
                    if let Some(usage) = commands::parse_command_usage(&value) {
                        commands::merge_command_usage(&mut self.command_usage, usage);
                    }
                }
                _ => {}
            }
        }
        commands::trim_command_history(&mut self.command_history, self.command_history_limit);
        self.recent_files.truncate(20);
        self.lance_files.truncate(10);
        self.file_cursor_positions.truncate(100);
        commands::sort_command_usage(&mut self.command_usage);
    }

    fn parse_config_bool(value: &str) -> Option<bool> {
        match value.trim().trim_matches('"').to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        }
    }

    fn parse_file_cursor_position(value: &str) -> Option<(PathBuf, usize, usize)> {
        let mut parts = value.rsplitn(3, '|');
        let column = parts.next()?.parse::<usize>().ok()?;
        let line_index = parts.next()?.parse::<usize>().ok()?;
        let path = PathBuf::from(parts.next()?);
        Some((path, line_index, column))
    }

    fn parse_config_string(value: &str) -> String {
        let value = value.trim().trim_matches('"');
        let mut parsed = String::new();
        let mut escaped = false;
        for ch in value.chars() {
            if escaped {
                parsed.push(match ch {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => other,
                });
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                parsed.push(ch);
            }
        }
        parsed
    }

    fn escape_config_string(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    fn save_settings(&self) -> Result<(), String> {
        let Some(path) = Self::settings_path() else {
            return Err("no config dir".to_string());
        };
        let parent = path
            .parent()
            .ok_or_else(|| "invalid config path".to_string())?;
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        let mut contents = format!(
            "command_history_limit = {}\nline_number_mode = \"{}\"\nword_wrap = {}\npreview_mode = {}\nctrl_shift_move_mode = \"{}\"\nreopen_last_file_on_startup = {}\nmarkdown_live_rendering = {}\nvirtual_references_enabled = {}\nvirtual_references_collapsed = {}\nvirtual_backlinks_collapsed = {}\nvirtual_mentions_collapsed = {}\nlast_opened_path = \"{}\"\nlast_open_dir = \"{}\"\n",
            self.command_history_limit,
            self.line_number_mode.config_value(),
            self.wrap,
            self.preview,
            self.ctrl_shift_move_mode.config_value(),
            self.reopen_last_file_on_startup,
            self.markdown_live_rendering,
            self.virtual_references_enabled,
            self.virtual_references_collapsed,
            self.virtual_backlinks_collapsed,
            self.virtual_mentions_collapsed,
            Self::escape_config_string(
                &self
                    .last_opened_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            ),
            Self::escape_config_string(
                &self
                    .last_open_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            )
        );
        let history_start = self
            .command_history
            .len()
            .saturating_sub(self.command_history_limit);
        for command in &self.command_history[history_start..] {
            contents.push_str(&format!(
                "command_history = \"{}\"\n",
                Self::escape_config_string(command)
            ));
        }
        for recent in &self.recent_files {
            contents.push_str(&format!(
                "recent_file = \"{}\"\n",
                Self::escape_config_string(&recent.display().to_string())
            ));
        }
        for path in &self.lance_files {
            contents.push_str(&format!(
                "lance_file = \"{}\"\n",
                Self::escape_config_string(&path.display().to_string())
            ));
        }
        let current_cursor = self.path.as_ref().map(|path| {
            let (line_index, column) = self.buffer.cursor_line_col();
            (path, line_index, column)
        });
        if let Some((path, line_index, column)) = current_cursor {
            contents.push_str(&format!(
                "file_cursor = \"{}|{}|{}\"\n",
                Self::escape_config_string(&path.display().to_string()),
                line_index,
                column
            ));
        }
        for entry in self.file_cursor_positions.iter().take(100) {
            if current_cursor
                .as_ref()
                .map(|(path, _, _)| *path == &entry.path)
                .unwrap_or(false)
            {
                continue;
            }
            contents.push_str(&format!(
                "file_cursor = \"{}|{}|{}\"\n",
                Self::escape_config_string(&entry.path.display().to_string()),
                entry.line_index,
                entry.column
            ));
        }
        for usage in self.command_usage.iter().take(100) {
            contents.push_str(&format!(
                "command_usage = \"{}|{}|{}\"\n",
                usage.name, usage.count, usage.last_used
            ));
        }
        let tmp_path = path.with_extension("toml.tmp");
        fs::write(&tmp_path, contents).map_err(|err| err.to_string())?;
        fs::rename(&tmp_path, &path).map_err(|err| err.to_string())
    }

    fn set_command_history_limit(&mut self, limit: usize) {
        self.command_history_limit = clamp_history_limit(limit);
        match self.save_settings() {
            Ok(_) => self.status = format!("History length: {}", self.command_history_limit),
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn set_line_number_mode(&mut self, mode: LineNumberMode) {
        self.line_number_mode = mode;
        match self.save_settings() {
            Ok(_) => self.status = format!("Line numbers: {}", self.line_number_mode.label()),
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn set_ctrl_shift_move_mode(&mut self, mode: CtrlShiftMoveMode) {
        self.ctrl_shift_move_mode = mode;
        match self.save_settings() {
            Ok(_) => {
                self.status = format!("Ctrl+Shift movement: {}", self.ctrl_shift_move_mode.label())
            }
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn set_wrap_mode(&mut self, enabled: bool) {
        self.wrap = enabled;
        match self.save_settings() {
            Ok(_) => {
                self.status = if self.wrap {
                    "Word wrap on"
                } else {
                    "Word wrap off"
                }
                .to_string()
            }
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn set_preview_mode(&mut self, enabled: bool) {
        self.preview = enabled;
        match self.save_settings() {
            Ok(_) => {
                self.status = if self.preview {
                    "Preview on"
                } else {
                    "Preview off"
                }
                .to_string()
            }
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn set_reopen_last_file_on_startup(&mut self, enabled: bool) {
        self.reopen_last_file_on_startup = enabled;
        match self.save_settings() {
            Ok(_) => {
                self.status = if self.reopen_last_file_on_startup {
                    "Reopen last file on startup on"
                } else {
                    "Reopen last file on startup off"
                }
                .to_string()
            }
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn set_markdown_live_rendering(&mut self, enabled: bool) {
        self.markdown_live_rendering = enabled;
        match self.save_settings() {
            Ok(_) => {
                self.status = if self.markdown_live_rendering {
                    "Markdown live rendering on"
                } else {
                    "Markdown plain source mode"
                }
                .to_string()
            }
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn set_virtual_references_enabled(&mut self, enabled: bool) {
        self.virtual_references_enabled = enabled;
        match self.save_settings() {
            Ok(_) => {
                self.status = if self.virtual_references_enabled {
                    "Virtual backlinks/mentions on"
                } else {
                    "Virtual backlinks/mentions hidden"
                }
                .to_string()
            }
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn save_virtual_reference_collapse_state(&mut self, status: &str) {
        match self.save_settings() {
            Ok(_) => self.status = status.to_string(),
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn handle_virtual_reference_click(&mut self, line: &str) {
        if line.contains("Backlinks") {
            self.virtual_backlinks_collapsed = !self.virtual_backlinks_collapsed;
            self.save_virtual_reference_collapse_state(if self.virtual_backlinks_collapsed {
                "Backlinks collapsed"
            } else {
                "Backlinks expanded"
            });
        } else if line.contains("Mentions") {
            self.virtual_mentions_collapsed = !self.virtual_mentions_collapsed;
            self.save_virtual_reference_collapse_state(if self.virtual_mentions_collapsed {
                "Mentions collapsed"
            } else {
                "Mentions expanded"
            });
        } else {
            self.virtual_references_collapsed = !self.virtual_references_collapsed;
            self.save_virtual_reference_collapse_state(if self.virtual_references_collapsed {
                "Virtual references collapsed"
            } else {
                "Virtual references expanded"
            });
        }
    }

    fn now_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default()
    }

    fn record_command_usage(&mut self, token: &str) {
        if commands::record_command_usage(&mut self.command_usage, token, Self::now_timestamp()) {
            let _ = self.save_settings();
        }
    }

    fn command_usage_boost(&self, name: &str) -> usize {
        commands::command_usage_boost(&self.command_usage, name, Self::now_timestamp())
    }

    fn run_command(&mut self, command: Command, ctx: &egui::Context) {
        self.record_command_usage(command_name(command));
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        if command != Command::Periscope {
            self.periscope_open = false;
            self.periscope_prompt_open = false;
        }
        if command != Command::FindNote {
            self.find_note_open = false;
        }
        self.save_as_open = false;
        if command != Command::Scratch {
            self.scratch_modal_open = false;
        }
        if command != Command::ScratchEntries {
            self.scratch_entries_open = false;
        }
        if command != Command::Capture {
            self.capture_modal_open = false;
        }
        if command != Command::DocTasks {
            self.doc_tasks_open = false;
        }
        if command != Command::Agenda {
            self.agenda_open = false;
        }
        if command != Command::Lance {
            self.lance_open = false;
        }
        self.palette_query.clear();
        self.selected_command = 0;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.shortcut_help_open = false;
        self.focus_editor_once = true;

        match command {
            Command::New => {
                if self.dirty {
                    self.confirm(PendingAction::New);
                } else {
                    self.new_buffer();
                }
            }
            Command::Open => {
                if self.dirty {
                    self.confirm(PendingAction::Open);
                } else {
                    self.open_dialog();
                }
            }
            Command::OpenBuffer => self.open_buffer_dialog(),
            Command::Periscope => self.open_periscope(),
            Command::Lance => self.open_lance(),
            Command::LanceAdd => self.lance_add_current(),
            Command::LanceNext => self.open_next_lance_file(1),
            Command::LancePrev => self.open_next_lance_file(-1),
            Command::FlowRescan => self.flow_rescan_current_scope(false),
            Command::FlowRebuild => self.flow_rescan_current_scope(true),
            Command::Daily => self.open_daily_with_target(""),
            Command::Refile => self.start_refile_current_line(),
            Command::QuickAdd => self.open_scratch_modal(),
            Command::Save => self.save(),
            Command::SaveAs => self.open_save_as_modal(),
            Command::Scratch => self.open_scratch_modal(),
            Command::ScratchEntries => self.open_scratch_entries_modal(),
            Command::Capture => self.open_capture_modal(),
            Command::TogglePreview => self.set_preview_mode(!self.preview),
            Command::ToggleWrap => self.set_wrap_mode(!self.wrap),
            Command::WrapOn => self.set_wrap_mode(true),
            Command::WrapOff => self.set_wrap_mode(false),
            Command::PreviewOn => self.set_preview_mode(true),
            Command::PreviewOff => self.set_preview_mode(false),
            Command::DocTasks => self.open_doc_tasks(),
            Command::Agenda => self.open_agenda(),
            Command::Backlinks => self.open_reference_results(ReferenceResultKind::Backlinks),
            Command::Mentions => self.open_reference_results(ReferenceResultKind::Mentions),
            Command::FindNote => self.open_find_note_results(""),
            Command::OpenLink => self.open_link_under_cursor(),
            Command::LineNumbersAbsolute => self.set_line_number_mode(LineNumberMode::Absolute),
            Command::LineNumbersRelative => self.set_line_number_mode(LineNumberMode::Relative),
            Command::Settings => {
                self.settings_open = true;
                self.selected_setting = 0;
                self.focus_editor_once = false;
            }
            Command::Quit => self.request_close(ctx),
        }
    }

    fn command_line_command_prefix(&self) -> Option<&str> {
        command_prefix(&self.command_line)
    }

    fn command_line_suggestions(&self) -> Vec<&'static CommandSpec> {
        let Some(prefix) = self.command_line_command_prefix() else {
            return Vec::new();
        };
        self.matching_command_specs(prefix, 5)
    }

    fn command_line_completion(&self) -> Option<&'static str> {
        let prefix = self.command_line_command_prefix()?;
        let spec = self.matching_command_specs(prefix, 1).into_iter().next()?;
        best_command_token(spec, prefix)
            .filter(|candidate| candidate.len() > prefix.len())
            .map(|candidate| &candidate[prefix.len()..])
    }

    fn accept_command_line_completion(&mut self) -> bool {
        let Some(prefix) = self.command_line_command_prefix().map(str::to_string) else {
            return false;
        };
        let suggestions = self.matching_command_specs(&prefix, 5);
        let Some(spec) = suggestions
            .get(
                self.selected_command_line_suggestion
                    .min(suggestions.len().saturating_sub(1)),
            )
            .copied()
        else {
            return false;
        };
        let Some(candidate) = best_command_token(spec, &prefix) else {
            return false;
        };
        if candidate.len() <= prefix.len() {
            return false;
        }
        self.command_line = candidate.to_string();
        self.command_line_cursor = self.command_line.len();
        self.focus_command_line_once = true;
        true
    }

    fn matching_command_specs(&self, prefix: &str, limit: usize) -> Vec<&'static CommandSpec> {
        commands::matching_command_specs(prefix, limit, |name| self.command_usage_boost(name))
    }

    fn run_command_line(&mut self, ctx: &egui::Context) {
        let raw = self.command_line.trim().to_string();
        self.command_line.clear();
        self.command_line_cursor = 0;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.shortcut_help_open = false;
        self.focus_editor_once = true;
        self.command_history_index = None;

        let input = raw.strip_prefix(':').unwrap_or(&raw).trim();
        if input.is_empty() {
            self.status = "Command cancelled".to_string();
            return;
        }

        if commands::push_command_history(
            &mut self.command_history,
            input,
            self.command_history_limit,
        ) {
            let _ = self.save_settings();
        }

        match parse_command_line(input) {
            CommandLineAction::Empty => self.status = "Command cancelled".to_string(),
            CommandLineAction::Run(command) => self.run_command(command, ctx),
            CommandLineAction::SaveQuit => {
                self.record_command_usage("wq");
                self.save();
                if !self.dirty {
                    self.run_command(Command::Quit, ctx);
                }
            }
            CommandLineAction::OpenPath(path) => {
                if self.dirty {
                    self.status = "Save or discard changes before opening another file".to_string();
                } else {
                    self.record_command_usage("open");
                    let expanded = Self::resolve_command_path(&path);
                    self.open_path(expanded);
                }
            }
            CommandLineAction::OpenBufferPath(path) => {
                self.record_command_usage("open-buffer");
                let expanded = Self::resolve_command_path(&path);
                self.open_link_preview(expanded, None);
            }
            CommandLineAction::QuickAdd(text) => {
                self.record_command_usage("quickadd");
                if let Some(text) = text {
                    self.quickadd_flow_task(&text);
                } else {
                    self.open_scratch_modal();
                    self.status = "Scratch quickadd: start a line with :task text".to_string();
                }
            }
            CommandLineAction::Daily { target } => {
                self.record_command_usage("daily");
                self.open_daily_with_target(&target);
            }
            CommandLineAction::LanceSlot(slot) => {
                self.record_command_usage("lance");
                self.open_lance_slot(slot);
            }
            CommandLineAction::LanceAddPath(path) => {
                self.record_command_usage("lance-add");
                self.add_lance_file(Self::resolve_command_path(&path));
            }
            CommandLineAction::OpenLast => {
                self.record_command_usage("open-last");
                if self.dirty {
                    self.confirm(PendingAction::OpenLast);
                } else {
                    self.open_last();
                }
            }
            CommandLineAction::Recent { query } => {
                self.record_command_usage("recent");
                self.open_recent_picker_with_query(query);
            }
            CommandLineAction::SetPreview(enabled) => match enabled {
                Some(enabled) => {
                    self.record_command_usage(if enabled { "preview-on" } else { "preview-off" });
                    self.set_preview_mode(enabled);
                }
                None => self.run_command(Command::TogglePreview, ctx),
            },
            CommandLineAction::Agenda { filter } => {
                self.record_command_usage("agenda");
                self.open_agenda_with_filter(filter);
            }
            CommandLineAction::FindNote { query } => {
                self.record_command_usage("find-note");
                self.open_find_note_results(&query);
            }
            CommandLineAction::SetWrap(enabled) => match enabled {
                Some(enabled) => {
                    self.record_command_usage(if enabled { "wrap-on" } else { "wrap-off" });
                    self.set_wrap_mode(enabled);
                }
                None => self.run_command(Command::ToggleWrap, ctx),
            },
            CommandLineAction::Find { query } => {
                self.record_command_usage("find");
                self.start_search(query);
            }
            CommandLineAction::Goto { target } => {
                self.record_command_usage("goto");
                self.goto_target(&target);
            }
            CommandLineAction::SelectWord => {
                self.record_command_usage("select-word");
                self.select_word();
            }
            CommandLineAction::SelectLine => {
                self.record_command_usage("select-line");
                self.select_line();
            }
            CommandLineAction::DeleteWord => {
                self.record_command_usage("delete-word");
                self.delete_word();
            }
            CommandLineAction::DeleteLine => {
                self.record_command_usage("delete-line");
                self.delete_current_line();
            }
            CommandLineAction::DuplicateLine => {
                self.record_command_usage("duplicate-line");
                self.duplicate_current_line();
            }
            CommandLineAction::DuplicatePlace => {
                self.record_command_usage("duplicate-place");
                self.start_duplicate_placement();
            }
            CommandLineAction::MoveLineUp => {
                self.record_command_usage("move-line-up");
                self.move_current_line_up();
            }
            CommandLineAction::MoveLineDown => {
                self.record_command_usage("move-line-down");
                self.move_current_line_down();
            }
            CommandLineAction::MoveLineToParagraphStart => {
                self.record_command_usage("move-line-to-paragraph-start");
                self.move_current_line_to_paragraph_start();
            }
            CommandLineAction::MoveLineToParagraphEnd => {
                self.record_command_usage("move-line-to-paragraph-end");
                self.move_current_line_to_paragraph_end();
            }
            CommandLineAction::Top => {
                self.record_command_usage("top");
                self.go_to_top();
            }
            CommandLineAction::Bottom => {
                self.record_command_usage("bottom");
                self.go_to_bottom();
            }
            CommandLineAction::LineNumbers { mode, had_arg } => match mode {
                Some(CommandLineNumberMode::Absolute) => {
                    self.record_command_usage("line-numbers-absolute");
                    self.set_line_number_mode(LineNumberMode::Absolute);
                }
                Some(CommandLineNumberMode::Relative) => {
                    self.record_command_usage("line-numbers-relative");
                    self.set_line_number_mode(LineNumberMode::Relative);
                }
                None if had_arg => {
                    self.status = "Usage: :line-numbers absolute|relative".to_string();
                }
                None => {
                    self.status = format!("Line numbers: {}", self.line_number_mode.label());
                }
            },
            CommandLineAction::Unknown(input) => self.status = format!("Unknown command: {input}"),
        }
    }
    fn select_word(&mut self) {
        if self.buffer.select_word() {
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Selected word".to_string();
            self.focus_editor_once = true;
        } else {
            self.status = "No word to select".to_string();
        }
    }

    fn select_line(&mut self) {
        if self.buffer.select_current_line() {
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Selected line".to_string();
            self.focus_editor_once = true;
        } else {
            self.status = "No line to select".to_string();
        }
    }

    fn delete_word(&mut self) {
        if self.buffer.delete_word() {
            self.dirty = true;
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Deleted word".to_string();
            self.focus_editor_once = true;
        } else {
            self.status = "No word to delete".to_string();
        }
    }

    fn select_word_left_extend(&mut self) {
        if self.buffer.select_word_left_extend() {
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Selected word left".to_string();
            self.focus_editor_once = true;
        } else {
            self.status = "No word to select left".to_string();
        }
    }

    fn select_word_right_extend(&mut self) {
        if self.buffer.select_word_right_extend() {
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Selected word right".to_string();
            self.focus_editor_once = true;
        } else {
            self.status = "No word to select right".to_string();
        }
    }

    fn delete_current_line(&mut self) {
        if self.buffer.delete_current_line() {
            self.dirty = true;
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Deleted line".to_string();
            self.focus_editor_once = true;
        }
    }

    fn duplicate_current_line(&mut self) {
        if self.buffer.duplicate_current_line() {
            self.dirty = true;
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Duplicated line".to_string();
            self.focus_editor_once = true;
        }
    }

    fn start_duplicate_placement(&mut self) {
        let snapshot = self.buffer.as_str().to_string();
        let was_dirty = self.dirty;
        if self.buffer.duplicate_current_line() {
            self.duplicate_placement = Some(DuplicatePlacement {
                snapshot,
                was_dirty,
            });
            self.dirty = true;
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Duplicate placement: move · Enter/Space place · Esc cancel".to_string();
            self.focus_editor_once = true;
        }
    }

    fn accept_duplicate_placement(&mut self) {
        if self.duplicate_placement.take().is_some() {
            self.dirty = true;
            self.status = "Duplicate placed".to_string();
            self.focus_editor_once = true;
            self.suppress_editor_keyboard_once = true;
        }
    }

    fn cancel_duplicate_placement(&mut self) {
        if let Some(placement) = self.duplicate_placement.take() {
            self.buffer.set_text(placement.snapshot);
            self.dirty = placement.was_dirty;
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Duplicate placement cancelled".to_string();
            self.focus_editor_once = true;
            self.suppress_editor_keyboard_once = true;
        }
    }

    fn move_duplicate_placement_up(&mut self) {
        if self.buffer.move_current_line_up() {
            self.dirty = true;
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Duplicate placement: moved up".to_string();
            self.focus_editor_once = true;
        } else {
            self.status = "Duplicate already at top".to_string();
        }
    }

    fn move_duplicate_placement_down(&mut self) {
        if self.buffer.move_current_line_down() {
            self.dirty = true;
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Duplicate placement: moved down".to_string();
            self.focus_editor_once = true;
        } else {
            self.status = "Duplicate already at bottom".to_string();
        }
    }

    fn move_current_line_up(&mut self) {
        if self.buffer.move_current_line_up() {
            self.after_line_move("Moved line up");
        } else {
            self.status = "Line already at top".to_string();
        }
    }

    fn move_current_line_down(&mut self) {
        if self.buffer.move_current_line_down() {
            self.after_line_move("Moved line down");
        } else {
            self.status = "Line already at bottom".to_string();
        }
    }

    fn move_current_line_to_paragraph_start(&mut self) {
        if self.buffer.move_current_line_to_paragraph_start() {
            self.after_line_move("Moved line to paragraph start");
        } else {
            self.status = "Line already at paragraph start".to_string();
        }
    }

    fn move_current_line_to_paragraph_end(&mut self) {
        if self.buffer.move_current_line_to_paragraph_end() {
            self.after_line_move("Moved line to paragraph end");
        } else {
            self.status = "Line already at paragraph end".to_string();
        }
    }

    fn after_line_move(&mut self, status: &str) {
        self.dirty = true;
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.status = status.to_string();
        self.focus_editor_once = true;
    }

    fn go_to_top(&mut self) {
        self.buffer.move_to_top();
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.status = "Top".to_string();
        self.focus_editor_once = true;
    }

    fn go_to_bottom(&mut self) {
        self.buffer.move_to_bottom();
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.status = "Bottom".to_string();
        self.focus_editor_once = true;
    }

    fn goto_target(&mut self, target: &str) {
        let Some(target) = GotoTarget::parse(target) else {
            self.status = "Usage: :g 10, :g 10:4, :g +5, :g -5".to_string();
            return;
        };

        let line_count = self.buffer.line_count();
        let (line, column, status) = match target {
            GotoTarget::Absolute { line, column } => {
                let line = line.clamp(1, line_count);
                let column = column.unwrap_or(1);
                (line, column, format!("Goto line {line}, col {column}"))
            }
            GotoTarget::Relative { offset, column } => {
                let current_line = self.buffer.cursor_line_col().0 + 1;
                let line = current_line
                    .saturating_add_signed(offset)
                    .clamp(1, line_count);
                let column = column.unwrap_or(1);
                (
                    line,
                    column,
                    format!("Goto {offset:+} lines → line {line}, col {column}"),
                )
            }
        };

        let byte = self.buffer.line_col_to_byte(line, column);
        self.buffer.set_cursor(byte);
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.search_state = None;
        self.status = status;
        self.focus_editor_once = true;
    }

    fn start_search(&mut self, query: String) {
        if query.is_empty() {
            self.search_state = None;
            self.status = "Find cancelled".to_string();
            return;
        }

        let state = SearchState::new(query, self.buffer.as_str(), self.buffer.revision);
        self.search_state = Some(state);
        self.apply_selected_search_match();
        self.focus_editor_once = true;
    }

    fn apply_selected_search_match(&mut self) {
        let Some(state) = self.search_state.as_ref() else {
            return;
        };

        let match_count = state.matches.len();
        if let Some((start, end)) = state.selected_match() {
            self.buffer.set_selection(start, end);
            self.editor_view.request_scroll_to_cursor(&self.buffer);
        }
        self.status = if match_count == 0 {
            format!("No matches for {}", state.query)
        } else {
            format!(
                "Find: {} ({}/{match_count}) · f next · b prev · ctrl+f after · ctrl+b before · enter accept · esc cancel",
                state.query,
                state.selected + 1
            )
        };
    }

    fn move_search_match(&mut self, forward: bool) {
        let Some(state) = self.search_state.as_mut() else {
            return;
        };
        if state.matches.is_empty() {
            return;
        }

        if forward {
            state.selected = (state.selected + 1) % state.matches.len();
        } else {
            state.selected = if state.selected == 0 {
                state.matches.len() - 1
            } else {
                state.selected - 1
            };
        }
        self.apply_selected_search_match();
        self.focus_editor_once = true;
    }

    fn accept_search(&mut self) {
        if self.search_state.is_some() {
            self.search_state = None;
            self.status = "Find accepted".to_string();
            self.focus_editor_once = true;
        }
    }

    fn place_cursor_at_search_edge(&mut self, after_match: bool) {
        let Some((start, end)) = self
            .search_state
            .as_ref()
            .and_then(|state| state.selected_match())
        else {
            return;
        };

        self.buffer
            .set_cursor(if after_match { end } else { start });
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.search_state = None;
        self.status = if after_match {
            "Find accepted: cursor after match".to_string()
        } else {
            "Find accepted: cursor before match".to_string()
        };
        self.focus_editor_once = true;
    }

    fn cancel_search(&mut self) {
        if self.search_state.is_some() {
            self.search_state = None;
            self.buffer.clear_selection();
            self.status = "Find cancelled".to_string();
            self.focus_editor_once = true;
        }
    }

    fn confirm(&mut self, action: PendingAction) {
        self.pending_action = Some(action);
        self.focus_editor_once = false;
    }

    fn finish_pending_action(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::New => self.new_buffer(),
            PendingAction::Open => self.open_dialog(),
            PendingAction::OpenLast => self.open_last(),
            PendingAction::OpenRecent => {
                if let Some(path) = self.pending_recent_path.take() {
                    self.recent_picker_open = false;
                    self.open_path(path);
                }
            }
            PendingAction::OpenProjectFile => {
                if let Some(path) = self.pending_project_file_path.take() {
                    self.file_picker_open = false;
                    let heading_fragment = self.pending_open_heading_fragment.take();
                    self.open_path(path);
                    if let Some(fragment) = heading_fragment {
                        self.jump_to_markdown_fragment(&fragment);
                    }
                }
            }
            PendingAction::OpenDaily => {
                let target = self.pending_daily_target.take().unwrap_or_default();
                self.open_daily_with_target(&target);
            }
            PendingAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn request_close(&mut self, ctx: &egui::Context) {
        if self.scratch {
            self.close_scratch_with_feedback(ctx);
        } else if self.dirty {
            self.confirm(PendingAction::Quit);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.standalone_agenda {
            self.agenda_open = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.scratch_close_after.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        if self.scratch && self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_scratch_with_feedback(ctx);
        } else if self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm(PendingAction::Quit);
        }
    }

    fn close_standalone_agenda_if_requested(&mut self, ctx: &egui::Context) -> bool {
        if !self.standalone_agenda {
            return false;
        }
        let should_close = ctx.input_mut(|i| {
            if self.flow_item_edit.is_some() && i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                self.flow_item_edit = None;
                return false;
            }
            if self.agenda_org_mode.is_some() && i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                self.close_agenda_org_autocomplete();
                return false;
            }
            i.consume_key(egui::Modifiers::NONE, Key::Escape)
                || i.consume_key(egui::Modifiers::CTRL, Key::Q)
                || i.viewport().close_requested()
        });
        if should_close {
            self.agenda_open = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return true;
        }
        false
    }

    fn layer_allowed(&self) -> bool {
        !self.command_line_focused
            && !self.focus_command_line_once
            && !self.palette_open
            && !self.settings_open
            && !self.recent_picker_open
            && !self.lance_open
            && !self.doc_tasks_open
            && !self.agenda_open
            && !self.file_picker_open
            && !self.periscope_open
            && !self.periscope_prompt_open
            && !self.find_note_open
            && !self.link_heading_picker_open
            && !self.link_preview_open
            && !self.save_as_open
            && !self.scratch_modal_open
            && !self.scratch_entries_open
            && !self.capture_modal_open
            && self.pending_action.is_none()
    }

    fn handle_duplicate_placement(&mut self, ctx: &egui::Context) -> bool {
        if self.duplicate_placement.is_none() {
            return false;
        }

        let mut accept = false;
        let mut cancel = false;
        let mut move_up = false;
        let mut move_down = false;

        ctx.input(|input| {
            for event in &input.events {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                else {
                    continue;
                };

                if modifiers.ctrl && modifiers.shift && !modifiers.alt {
                    match (self.ctrl_shift_move_mode, key) {
                        (CtrlShiftMoveMode::Vim, Key::K) | (CtrlShiftMoveMode::Slate, Key::I) => {
                            move_up = true;
                        }
                        (CtrlShiftMoveMode::Vim, Key::J) | (CtrlShiftMoveMode::Slate, Key::K) => {
                            move_down = true;
                        }
                        _ => {}
                    }
                    continue;
                }

                if modifiers.alt && !modifiers.ctrl && !modifiers.shift {
                    match (self.ctrl_shift_move_mode, key) {
                        (_, Key::ArrowUp)
                        | (CtrlShiftMoveMode::Vim, Key::K)
                        | (CtrlShiftMoveMode::Slate, Key::I) => {
                            move_up = true;
                        }
                        (_, Key::ArrowDown)
                        | (CtrlShiftMoveMode::Vim, Key::J)
                        | (CtrlShiftMoveMode::Slate, Key::K) => {
                            move_down = true;
                        }
                        _ => {}
                    }
                    continue;
                }

                if modifiers.is_none() {
                    match key {
                        Key::Enter | Key::Space => accept = true,
                        Key::Escape => cancel = true,
                        Key::ArrowUp => move_up = true,
                        Key::ArrowDown => move_down = true,
                        _ => {}
                    }
                }
            }
        });

        if cancel {
            self.cancel_duplicate_placement();
            return true;
        }
        if accept {
            self.accept_duplicate_placement();
            return true;
        }
        if move_up {
            self.move_duplicate_placement_up();
            return true;
        }
        if move_down {
            self.move_duplicate_placement_down();
            return true;
        }

        self.status = "Duplicate placement: move · Enter/Space place · Esc cancel".to_string();
        true
    }

    fn handle_shift_alt_layer(&mut self, ctx: &egui::Context) -> bool {
        if !self.layer_allowed() {
            self.ctrl_alt_layer_active = false;
            self.ctrl_alt_layer_sequence.clear();
            self.ctrl_alt_layer_last_key = None;
            return false;
        }

        let shift_alt_down = ctx
            .input(|input| input.modifiers.shift && input.modifiers.alt && !input.modifiers.ctrl);
        if shift_alt_down && !self.ctrl_alt_layer_active {
            self.ctrl_alt_layer_active = true;
            self.ctrl_alt_layer_sequence.clear();
            self.ctrl_alt_layer_last_key = None;
        }

        if self.ctrl_alt_layer_active && shift_alt_down {
            let now = ctx.input(|input| input.time);
            let keys = ctx.input(|input| input.events.clone());
            let mut handled = false;
            for event in keys {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                else {
                    continue;
                };
                if !modifiers.shift || !modifiers.alt || modifiers.ctrl {
                    continue;
                }

                let Some(ch) = Self::ctrl_layer_key(key) else {
                    continue;
                };
                if self.dispatch_ctrl_alt_layer_key(ch, now) {
                    handled = true;
                }
            }

            if !handled {
                self.status = "Shift+Alt movement layer".to_string();
            }
            return true;
        }

        if self.ctrl_alt_layer_active && !shift_alt_down {
            self.ctrl_alt_layer_active = false;
            self.ctrl_alt_layer_sequence.clear();
            self.ctrl_alt_layer_last_key = None;
            return true;
        }

        false
    }

    fn dispatch_ctrl_alt_layer_key(&mut self, ch: char, now: f64) -> bool {
        let up_key = match self.ctrl_shift_move_mode {
            CtrlShiftMoveMode::Vim => 'k',
            CtrlShiftMoveMode::Slate => 'i',
        };
        let down_key = match self.ctrl_shift_move_mode {
            CtrlShiftMoveMode::Vim => 'j',
            CtrlShiftMoveMode::Slate => 'k',
        };
        let left_key = match self.ctrl_shift_move_mode {
            CtrlShiftMoveMode::Vim => 'h',
            CtrlShiftMoveMode::Slate => 'j',
        };
        let right_key = 'l';
        const CTRL_ALT_SEQUENCE_SECONDS: f64 = 0.12;

        let within_sequence = self.ctrl_alt_layer_last_key.is_some()
            && now - self.ctrl_alt_layer_last_key_time <= CTRL_ALT_SEQUENCE_SECONDS;
        if !within_sequence {
            self.ctrl_alt_layer_sequence.clear();
        }

        self.ctrl_alt_layer_sequence.push(ch);
        self.ctrl_alt_layer_last_key = Some(ch);
        self.ctrl_alt_layer_last_key_time = now;

        if self.ctrl_alt_layer_sequence.chars().count() < 2 {
            return true;
        }

        let sequence = std::mem::take(&mut self.ctrl_alt_layer_sequence);
        self.ctrl_alt_layer_last_key = None;
        let mut chars = sequence.chars();
        let first = chars.next().unwrap_or_default();
        let second = chars.next().unwrap_or_default();

        if first == up_key && second == up_key {
            if self.buffer.move_to_paragraph_start() {
                self.after_cursor_jump("Paragraph start");
            } else {
                self.status = "No paragraph start".to_string();
            }
            return true;
        }
        if first == down_key && second == down_key {
            if self.buffer.move_to_paragraph_end() {
                self.after_cursor_jump("Paragraph end");
            } else {
                self.status = "No paragraph end".to_string();
            }
            return true;
        }
        if first == left_key && second == left_key {
            if self.buffer.move_to_word_start_left() {
                self.after_cursor_jump("Word start");
            } else {
                self.status = "No word start".to_string();
            }
            return true;
        }
        if first == right_key && second == right_key {
            if self.buffer.move_to_word_end_right() {
                self.after_cursor_jump("Word end");
            } else {
                self.status = "No word end".to_string();
            }
            return true;
        }
        if first == left_key && second == right_key {
            self.buffer.move_to_line_end();
            self.after_cursor_jump("Line end");
            return true;
        }
        if first == right_key && second == left_key {
            self.buffer.move_to_line_start();
            self.after_cursor_jump("Line start");
            return true;
        }

        false
    }

    fn after_cursor_jump(&mut self, status: &str) {
        self.search_state = None;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.status = status.to_string();
        self.focus_editor_once = true;
    }

    fn handle_alt_layer(&mut self, ctx: &egui::Context) -> bool {
        if !self.layer_allowed() {
            self.alt_layer_active = false;
            self.alt_layer_sequence.clear();
            self.alt_layer_last_key = None;
            return false;
        }

        let alt_down = ctx
            .input(|input| input.modifiers.alt && !input.modifiers.ctrl && !input.modifiers.shift);
        if alt_down && !self.alt_layer_active {
            self.alt_layer_active = true;
            self.alt_layer_sequence.clear();
            self.alt_layer_last_key = None;
        }

        if self.alt_layer_active && alt_down {
            let now = ctx.input(|input| input.time);
            let keys = ctx.input(|input| input.events.clone());
            let mut handled = false;
            for event in keys {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                else {
                    continue;
                };
                if !modifiers.alt || modifiers.ctrl || modifiers.shift {
                    continue;
                }

                if key == Key::ArrowUp {
                    self.alt_layer_sequence.push('↑');
                    self.move_current_line_up();
                    handled = true;
                    continue;
                }
                if key == Key::ArrowDown {
                    self.alt_layer_sequence.push('↓');
                    self.move_current_line_down();
                    handled = true;
                    continue;
                }

                if let Some(slot) = Self::lance_slot_key(key) {
                    self.alt_layer_sequence.clear();
                    self.alt_layer_last_key = None;
                    self.open_lance_slot(slot);
                    handled = true;
                    continue;
                }

                let Some(ch) = Self::ctrl_layer_key(key) else {
                    continue;
                };
                if self.dispatch_alt_layer_key(ch, now) {
                    handled = true;
                }
            }

            if !handled {
                self.status = "Alt structural layer".to_string();
            }
            return true;
        }

        if self.alt_layer_active && !alt_down {
            self.alt_layer_active = false;
            self.alt_layer_sequence.clear();
            self.alt_layer_last_key = None;
            return true;
        }

        false
    }

    fn dispatch_alt_layer_key(&mut self, ch: char, now: f64) -> bool {
        let up_key = match self.ctrl_shift_move_mode {
            CtrlShiftMoveMode::Vim => 'k',
            CtrlShiftMoveMode::Slate => 'i',
        };
        let down_key = match self.ctrl_shift_move_mode {
            CtrlShiftMoveMode::Vim => 'j',
            CtrlShiftMoveMode::Slate => 'k',
        };
        let left_key = match self.ctrl_shift_move_mode {
            CtrlShiftMoveMode::Vim => 'h',
            CtrlShiftMoveMode::Slate => 'j',
        };
        let right_key = 'l';

        const ALT_DOUBLE_TAP_SECONDS: f64 = 0.12;

        if ch == up_key {
            let is_double_tap = self.alt_layer_last_key == Some(up_key)
                && now - self.alt_layer_last_key_time <= ALT_DOUBLE_TAP_SECONDS;
            self.alt_layer_sequence.push(ch);
            self.alt_layer_last_key = Some(ch);
            self.alt_layer_last_key_time = now;
            if is_double_tap {
                self.move_current_line_to_paragraph_start();
                self.alt_layer_sequence.clear();
                self.alt_layer_last_key = None;
            } else {
                self.move_current_line_up();
            }
            return true;
        }

        if ch == down_key {
            let is_double_tap = self.alt_layer_last_key == Some(down_key)
                && now - self.alt_layer_last_key_time <= ALT_DOUBLE_TAP_SECONDS;
            self.alt_layer_sequence.push(ch);
            self.alt_layer_last_key = Some(ch);
            self.alt_layer_last_key_time = now;
            if is_double_tap {
                self.move_current_line_to_paragraph_end();
                self.alt_layer_sequence.clear();
                self.alt_layer_last_key = None;
            } else {
                self.move_current_line_down();
            }
            return true;
        }

        if ch == left_key {
            self.alt_layer_sequence.push(ch);
            self.alt_layer_last_key = None;
            self.select_word_left_extend();
            return true;
        }

        if ch == right_key {
            self.alt_layer_sequence.push(ch);
            self.alt_layer_last_key = None;
            self.select_word_right_extend();
            return true;
        }

        false
    }

    fn editor_shortcuts_allowed(&self) -> bool {
        !self.command_line_focused
            && !self.focus_command_line_once
            && !self.palette_open
            && !self.settings_open
            && !self.recent_picker_open
            && !self.lance_open
            && !self.doc_tasks_open
            && !self.agenda_open
            && !self.file_picker_open
            && !self.periscope_open
            && !self.periscope_prompt_open
            && !self.link_assist_open
            && !self.link_assist_web_open
            && !self.link_heading_picker_open
            && !self.link_preview_open
            && !self.save_as_open
            && !self.scratch_modal_open
            && !self.scratch_entries_open
            && !self.capture_modal_open
            && self.pending_action.is_none()
    }

    fn handle_immediate_ctrl_edit(&mut self, ctx: &egui::Context) -> bool {
        if !self.editor_shortcuts_allowed() {
            return false;
        }

        let mut undo = false;
        let mut redo = false;
        ctx.input_mut(|input| {
            for event in &input.events {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    continue;
                };
                if modifiers.alt || modifiers.shift || !(modifiers.ctrl || modifiers.command) {
                    continue;
                }
                match key {
                    Key::Z => undo = true,
                    Key::Y => redo = true,
                    _ => {}
                }
            }
            if undo || redo {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Z | Key::Y,
                            pressed: true,
                            modifiers,
                            ..
                        } if !modifiers.alt && !modifiers.shift && (modifiers.ctrl || modifiers.command)
                    )
                });
            }
        });

        if undo {
            if self.buffer.undo() {
                self.dirty = true;
                self.editor_view.request_scroll_to_cursor(&self.buffer);
                self.status = "Undo".to_string();
            } else {
                self.status = "Nothing to undo".to_string();
            }
            self.ctrl_layer_active = false;
            self.ctrl_layer_sequence.clear();
            return true;
        }

        if redo {
            if self.buffer.redo() {
                self.dirty = true;
                self.editor_view.request_scroll_to_cursor(&self.buffer);
                self.status = "Redo".to_string();
            } else {
                self.status = "Nothing to redo".to_string();
            }
            self.ctrl_layer_active = false;
            self.ctrl_layer_sequence.clear();
            return true;
        }

        false
    }

    fn handle_ctrl_layer(&mut self, ctx: &egui::Context) -> bool {
        let layer_allowed = self.editor_shortcuts_allowed();

        if !layer_allowed {
            self.ctrl_layer_active = false;
            self.ctrl_layer_sequence.clear();
            return false;
        }

        let ctrl_down = ctx.input(|input| input.modifiers.ctrl && !input.modifiers.alt);
        if ctrl_down && !self.ctrl_layer_active {
            self.ctrl_layer_active = true;
            self.ctrl_layer_sequence.clear();
        }

        if self.ctrl_layer_active && ctrl_down {
            let keys = ctx.input(|input| input.events.clone());
            if self.handle_ctrl_shift_navigation(&keys) {
                return true;
            }

            for event in keys {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                else {
                    continue;
                };
                if !modifiers.ctrl || modifiers.alt || modifiers.shift {
                    continue;
                }
                if let Some(ch) = Self::ctrl_layer_key(key) {
                    self.ctrl_layer_sequence.push(ch);
                }
            }
            self.status = "Ctrl layer".to_string();
            return true;
        }

        if self.ctrl_layer_active && !ctrl_down {
            let sequence = std::mem::take(&mut self.ctrl_layer_sequence);
            self.ctrl_layer_active = false;
            if !sequence.is_empty() {
                self.dispatch_ctrl_layer(&sequence, ctx);
            }
            return true;
        }

        false
    }

    fn handle_ctrl_shift_navigation(&mut self, events: &[egui::Event]) -> bool {
        let mut moved = false;
        for event in events {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            if !modifiers.ctrl || !modifiers.shift || modifiers.alt {
                continue;
            }

            match (self.ctrl_shift_move_mode, key) {
                (CtrlShiftMoveMode::Vim, Key::H) | (CtrlShiftMoveMode::Slate, Key::J) => {
                    self.buffer.move_left();
                    moved = true;
                }
                (CtrlShiftMoveMode::Vim, Key::J) | (CtrlShiftMoveMode::Slate, Key::K) => {
                    self.buffer.move_down();
                    moved = true;
                }
                (CtrlShiftMoveMode::Vim, Key::K) | (CtrlShiftMoveMode::Slate, Key::I) => {
                    self.buffer.move_up();
                    moved = true;
                }
                (CtrlShiftMoveMode::Vim, Key::L) | (CtrlShiftMoveMode::Slate, Key::L) => {
                    self.buffer.move_right();
                    moved = true;
                }
                _ => {}
            }
        }

        if moved {
            self.search_state = None;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = format!("Ctrl+Shift nav: {}", self.ctrl_shift_move_mode.label());
            self.focus_editor_once = true;
        }
        moved
    }

    fn lance_slot_key(key: Key) -> Option<usize> {
        match key {
            Key::Num1 => Some(1),
            Key::Num2 => Some(2),
            Key::Num3 => Some(3),
            Key::Num4 => Some(4),
            Key::Num5 => Some(5),
            Key::Num6 => Some(6),
            Key::Num7 => Some(7),
            Key::Num8 => Some(8),
            Key::Num9 => Some(9),
            Key::Num0 => Some(10),
            _ => None,
        }
    }

    fn ctrl_layer_key(key: Key) -> Option<char> {
        match key {
            Key::A => Some('a'),
            Key::B => Some('b'),
            Key::C => Some('c'),
            Key::D => Some('d'),
            Key::E => Some('e'),
            Key::F => Some('f'),
            Key::G => Some('g'),
            Key::H => Some('h'),
            Key::I => Some('i'),
            Key::J => Some('j'),
            Key::K => Some('k'),
            Key::L => Some('l'),
            Key::M => Some('m'),
            Key::N => Some('n'),
            Key::O => Some('o'),
            Key::P => Some('p'),
            Key::Q => Some('q'),
            Key::R => Some('r'),
            Key::S => Some('s'),
            Key::T => Some('t'),
            Key::U => Some('u'),
            Key::V => Some('v'),
            Key::W => Some('w'),
            Key::X => Some('x'),
            Key::Y => Some('y'),
            Key::Z => Some('z'),
            Key::Period => Some('.'),
            _ => None,
        }
    }

    fn dispatch_ctrl_layer(&mut self, sequence: &str, ctx: &egui::Context) {
        match sequence {
            "s" => self.run_command(Command::Save, ctx),
            "o" => self.run_command(Command::Open, ctx),
            "ol" => {
                self.record_command_usage("open-last");
                if self.dirty {
                    self.confirm(PendingAction::OpenLast);
                } else {
                    self.open_last();
                }
            }
            "r" => {
                self.record_command_usage("recent");
                self.open_recent_picker();
            }
            "n" => self.run_command(Command::New, ctx),
            "p" => {
                self.palette_open = true;
                self.palette_query.clear();
                self.selected_command = 0;
                self.focus_editor_once = false;
            }
            "q" => self.run_command(Command::Quit, ctx),
            "m" => self.run_command(Command::TogglePreview, ctx),
            "." => self.focus_command_line(),
            "h" => self.open_shortcut_help(),
            "f" if self.search_state.is_some() => self.place_cursor_at_search_edge(true),
            "f" => {
                self.record_command_usage("find");
                self.focus_find_command_line();
            }
            "b" if self.search_state.is_some() => self.place_cursor_at_search_edge(false),
            "sw" => {
                self.record_command_usage("select-word");
                self.select_word();
            }
            "sl" => {
                self.record_command_usage("select-line");
                self.select_line();
            }
            "dw" => {
                self.record_command_usage("delete-word");
                self.delete_word();
            }
            "dl" => {
                self.record_command_usage("delete-line");
                self.delete_current_line();
            }
            "dup" => {
                self.record_command_usage("duplicate-line");
                self.duplicate_current_line();
            }
            "dupp" => {
                self.record_command_usage("duplicate-place");
                self.start_duplicate_placement();
            }
            "gt" => {
                self.record_command_usage("top");
                self.go_to_top();
            }
            "gb" => {
                self.record_command_usage("bottom");
                self.go_to_bottom();
            }
            _ => self.status = format!("Unknown ctrl command: {sequence}"),
        }
    }

    fn open_shortcut_help(&mut self) {
        self.palette_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.shortcut_help_open = true;
        self.status = "Shortcut help".to_string();
        self.focus_editor_once = true;
    }

    fn focus_command_line(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.command_line.clear();
        self.command_line_cursor = 0;
        self.selected_command_line_suggestion = 0;
        self.command_history_index = None;
        self.command_line_focused = true;
        self.focus_command_line_once = true;
        self.focus_editor_once = false;
    }

    fn handle_link_inline_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if self.link_inline_anchor.is_none() {
            return false;
        }
        let mut handled = false;
        let mut cancel = false;
        let mut confirm = false;
        let mut previous = false;
        let mut next = false;
        let mut open_headings = false;
        ctx.input_mut(|i| {
            previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
            next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            open_headings |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
            open_headings |= i.consume_key(egui::Modifiers::SHIFT, Key::Enter);
            confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
            cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
        });
        let len = self.link_inline_suggestions().len().max(1);
        if previous {
            self.selected_link_inline_suggestion =
                self.selected_link_inline_suggestion.saturating_sub(1);
            handled = true;
        }
        if next {
            self.selected_link_inline_suggestion =
                (self.selected_link_inline_suggestion + 1).min(len.saturating_sub(1));
            handled = true;
        }
        if open_headings {
            self.open_selected_link_inline_headings();
            handled = true;
        }
        if cancel {
            self.close_link_inline_autocomplete();
            self.pending_bidirectional_link_source_line = None;
            self.link_heading_target_path = None;
            handled = true;
        }
        if confirm {
            self.confirm_link_inline_autocomplete();
            handled = true;
        }
        handled
    }

    fn handle_flow_inline_org_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if self.flow_inline_org_mode.is_none() {
            return false;
        }
        let mut handled = false;
        let mut previous = false;
        let mut next = false;
        let mut tab_next = false;
        let mut tab_previous = false;
        let mut confirm = false;
        let mut cancel = false;
        ctx.input_mut(|input| {
            previous |= input.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
            next |= input.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            tab_next |= input.consume_key(egui::Modifiers::NONE, Key::Tab);
            tab_previous |= input.consume_key(egui::Modifiers::SHIFT, Key::Tab);
            confirm |= input.consume_key(egui::Modifiers::NONE, Key::Enter);
            cancel |= input.consume_key(egui::Modifiers::NONE, Key::Escape);
        });
        let len = self.flow_inline_org_suggestions().len().max(1);
        if previous {
            self.selected_flow_inline_org_suggestion =
                self.selected_flow_inline_org_suggestion.saturating_sub(1);
            handled = true;
        }
        if next {
            self.selected_flow_inline_org_suggestion =
                (self.selected_flow_inline_org_suggestion + 1).min(len.saturating_sub(1));
            handled = true;
        }
        if tab_next {
            self.selected_flow_inline_org_suggestion =
                (self.selected_flow_inline_org_suggestion + 1) % len;
            handled = true;
        }
        if tab_previous {
            self.selected_flow_inline_org_suggestion =
                if self.selected_flow_inline_org_suggestion == 0 {
                    len.saturating_sub(1)
                } else {
                    self.selected_flow_inline_org_suggestion.saturating_sub(1)
                };
            handled = true;
        }
        if cancel {
            self.close_flow_inline_org_autocomplete();
            handled = true;
        }
        if confirm {
            self.confirm_flow_inline_org_autocomplete(ctx);
            handled = true;
        }
        handled
    }

    fn handle_agenda_org_keyboard(&mut self, ctx: &egui::Context) -> bool {
        if !self.agenda_open || self.agenda_org_mode.is_none() {
            return false;
        }
        let mut handled = false;
        let mut confirm = false;
        let mut cancel = false;
        let mut previous = false;
        let mut next = false;
        let mut tab_next = false;
        let mut tab_previous = false;
        let mut backspace = false;
        let mut typed = String::new();
        ctx.input_mut(|input| {
            previous |= input.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
            next |= input.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            tab_next |= input.consume_key(egui::Modifiers::NONE, Key::Tab);
            tab_previous |= input.consume_key(egui::Modifiers::SHIFT, Key::Tab);
            confirm |= input.consume_key(egui::Modifiers::NONE, Key::Enter);
            confirm |= input.consume_key(egui::Modifiers::NONE, Key::Space);
            cancel |= input.consume_key(egui::Modifiers::NONE, Key::Escape);
            backspace |= input.consume_key(egui::Modifiers::NONE, Key::Backspace);
            for event in &input.events {
                if let egui::Event::Text(text) = event {
                    typed.push_str(text);
                }
            }
            if !typed.is_empty() {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Text(_)));
            }
        });
        if previous {
            self.selected_agenda_org_suggestion =
                self.selected_agenda_org_suggestion.saturating_sub(1);
            handled = true;
        }
        if next {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion =
                (self.selected_agenda_org_suggestion + 1).min(len.saturating_sub(1));
            handled = true;
        }
        if tab_next {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion = (self.selected_agenda_org_suggestion + 1) % len;
            handled = true;
        }
        if tab_previous {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion = if self.selected_agenda_org_suggestion == 0 {
                len.saturating_sub(1)
            } else {
                self.selected_agenda_org_suggestion.saturating_sub(1)
            };
            handled = true;
        }
        if backspace {
            self.agenda_org_query.pop();
            self.selected_agenda_org_suggestion = 0;
            handled = true;
        }
        if !typed.is_empty() {
            for ch in typed.chars() {
                if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.') {
                    self.agenda_org_query.push(ch);
                }
            }
            self.selected_agenda_org_suggestion = 0;
            handled = true;
        }
        if cancel {
            self.close_agenda_org_autocomplete();
            handled = true;
        }
        if confirm {
            self.confirm_agenda_org_autocomplete(ctx);
            handled = true;
        }
        if self.agenda_org_mode.is_some() {
            let len = self.agenda_org_suggestions().len().max(1);
            self.selected_agenda_org_suggestion = self
                .selected_agenda_org_suggestion
                .min(len.saturating_sub(1));
        }
        handled
    }

    fn focus_find_command_line(&mut self) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.scratch_modal_open = false;
        self.scratch_entries_open = false;
        self.command_line = "find ".to_string();
        self.command_line_cursor = self.command_line.len();
        self.selected_command_line_suggestion = 0;
        self.command_history_index = None;
        self.command_line_focused = true;
        self.focus_command_line_once = true;
        self.focus_editor_once = false;
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.scratch_close_after.is_some() {
            return;
        }

        if self.handle_agenda_org_keyboard(ctx) {
            return;
        }

        if self.handle_link_inline_keyboard(ctx) {
            return;
        }

        if self.flow_item_edit.is_some() {
            return;
        }

        if self.handle_duplicate_placement(ctx) {
            return;
        }

        if self.handle_shift_alt_layer(ctx) {
            return;
        }

        if self.handle_alt_layer(ctx) {
            return;
        }

        if self.handle_immediate_ctrl_edit(ctx) {
            return;
        }

        if self.handle_ctrl_layer(ctx) {
            return;
        }

        let mut command = None;
        let mut execute_command_line = false;
        let mut complete_command_line = false;
        let mut previous_command = false;
        let mut next_command = false;
        let mut settings_decrement = false;
        let mut settings_increment = false;
        let mut settings_previous = false;
        let mut settings_next = false;
        let mut settings_activate = false;
        let mut recent_previous = false;
        let mut recent_next = false;
        let mut recent_open = false;
        let mut recent_backspace = false;
        let mut lance_previous = false;
        let mut lance_next = false;
        let mut lance_open = false;
        let mut lance_open_modal = false;
        let mut lance_add = false;
        let mut lance_delete = false;
        let mut lance_backspace = false;
        let mut doc_task_previous = false;
        let mut doc_task_next = false;
        let mut doc_task_open = false;
        let mut doc_task_cycle = false;
        let mut doc_task_backspace = false;
        let mut agenda_previous = false;
        let mut agenda_next = false;
        let mut agenda_open = false;
        let mut agenda_cycle = false;
        let mut agenda_edit = false;
        let mut agenda_refile = false;
        let mut agenda_previous_day = false;
        let mut agenda_next_day = false;
        let mut agenda_previous_week = false;
        let mut agenda_next_week = false;
        let mut agenda_today = false;
        let mut agenda_toggle_focus = false;
        let mut agenda_cycle_tab = false;
        let mut agenda_toggle_done = false;
        let mut agenda_org_tag = false;
        let mut agenda_org_project = false;
        let mut file_previous = false;
        let mut file_next = false;
        let mut file_open = false;
        let mut file_enter_dir = false;
        let mut file_parent = false;
        let mut file_backspace = false;
        let mut periscope_previous = false;
        let mut periscope_next = false;
        let mut periscope_open = false;
        let mut periscope_open_modal = false;
        let mut periscope_toggle_mode = false;
        let mut periscope_expand_folder = false;
        let mut periscope_backspace = false;
        let mut find_note_previous = false;
        let mut find_note_next = false;
        let mut find_note_open = false;
        let mut find_note_open_modal = false;
        let mut find_note_toggle = false;
        let mut find_note_backspace = false;
        let mut find_note_cancel = false;
        let mut periscope_prompt_yes = false;
        let mut periscope_prompt_no = false;
        let mut save_as_previous = false;
        let mut save_as_next = false;
        let mut save_as_enter = false;
        let mut save_as_enter_dir = false;
        let mut save_as_parent = false;
        let mut save_as_backspace = false;
        let mut link_assist_previous = false;
        let mut link_assist_next = false;
        let mut link_assist_confirm = false;
        let mut link_assist_cancel = false;
        let mut link_web_confirm = false;
        let mut link_web_backspace = false;
        let mut link_web_cancel = false;
        let mut link_heading_previous = false;
        let mut link_heading_next = false;
        let mut link_heading_confirm = false;
        let mut link_heading_cancel = false;
        let mut link_preview_confirm = false;
        let mut link_preview_cancel = false;
        let mut link_preview_save = false;
        let mut link_preview_swap = false;
        let mut scratch_archive = false;
        let mut scratch_open_entries = false;
        let mut scratch_entries_previous = false;
        let mut scratch_entries_next = false;
        let mut scratch_entries_delete = false;
        let mut capture_confirm = false;
        let mut search_next = false;
        let mut search_previous = false;
        let mut search_accept = false;
        let mut search_cancel = false;
        let mut search_cursor_after = false;
        let mut search_cursor_before = false;
        let mut command_line_backspace = false;
        let mut command_line_delete = false;
        let mut command_line_left = false;
        let mut command_line_right = false;
        let mut command_line_home = false;
        let mut command_line_end = false;
        let search_active = self.search_state.is_some()
            && !self.command_line_focused
            && !self.focus_command_line_once
            && !self.palette_open
            && !self.settings_open
            && !self.recent_picker_open
            && !self.lance_open
            && !self.doc_tasks_open
            && !self.agenda_open
            && !self.file_picker_open
            && !self.find_note_open
            && !self.link_heading_picker_open
            && !self.link_preview_open
            && !self.save_as_open
            && !self.scratch_modal_open
            && !self.scratch_entries_open
            && !self.capture_modal_open
            && self.pending_action.is_none();
        ctx.input_mut(|i| {
            if self.settings_open {
                settings_decrement |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                settings_increment |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                settings_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                settings_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                settings_activate |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                settings_activate |= i.consume_key(egui::Modifiers::NONE, Key::Space);
            }
            if self.recent_picker_open {
                recent_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                recent_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                recent_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                recent_open |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                recent_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.lance_open {
                lance_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                lance_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                lance_open_modal |= i.consume_key(egui::Modifiers::CTRL, Key::Enter);
                lance_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                lance_add |= i.consume_key(egui::Modifiers::CTRL, Key::A);
                lance_delete |= i.consume_key(egui::Modifiers::NONE, Key::Delete);
                lance_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.doc_tasks_open {
                doc_task_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                doc_task_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                let ctrl_held = i.modifiers.ctrl || i.modifiers.command;
                doc_task_cycle |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if ctrl_held || modifiers.ctrl || modifiers.command
                    )
                });
                doc_task_open |= !doc_task_cycle
                    && i.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                modifiers,
                                ..
                            } if !ctrl_held && !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                        )
                    });
                if doc_task_cycle || doc_task_open {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                ..
                            }
                        )
                    });
                }
                doc_task_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.agenda_open && !self.file_picker_open && !self.link_heading_picker_open {
                agenda_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                agenda_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                agenda_previous_day |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                agenda_next_day |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                agenda_previous_week |= i.consume_key(egui::Modifiers::NONE, Key::PageUp);
                agenda_next_week |= i.consume_key(egui::Modifiers::NONE, Key::PageDown);
                agenda_today |= i.consume_key(egui::Modifiers::NONE, Key::Home);
                agenda_toggle_done |= i.consume_key(egui::Modifiers::NONE, Key::X);
                agenda_refile |= i.consume_key(egui::Modifiers::NONE, Key::R);
                agenda_org_tag |= i.events.iter().any(|event| {
                    matches!(event, egui::Event::Text(text) if text == "#")
                });
                agenda_org_project |= i.events.iter().any(|event| {
                    matches!(event, egui::Event::Text(text) if text == "@")
                });
                if agenda_org_tag || agenda_org_project {
                    i.events.retain(|event| {
                        !matches!(event, egui::Event::Text(text) if text == "#" || text == "@")
                    });
                }
                agenda_cycle_tab |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && modifiers.shift
                    )
                });
                agenda_toggle_focus |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                    )
                });
                if agenda_toggle_focus || agenda_cycle_tab {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Tab,
                                pressed: true,
                                ..
                            }
                        )
                    });
                }
                let ctrl_held = i.modifiers.ctrl || i.modifiers.command;
                agenda_cycle |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if ctrl_held || modifiers.ctrl || modifiers.command
                    )
                });
                agenda_edit |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && modifiers.shift
                    )
                });
                agenda_open |= !agenda_cycle && !agenda_edit
                    && i.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                modifiers,
                                ..
                            } if !ctrl_held && !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                        )
                    });
                agenda_open |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                if agenda_cycle || agenda_edit || agenda_open {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                ..
                            }
                        )
                    });
                }
            }
            if self.file_picker_open {
                file_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                file_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                file_enter_dir |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                file_parent |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                file_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                file_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.periscope_open {
                periscope_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                periscope_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                periscope_open_modal |= i.consume_key(egui::Modifiers::CTRL, Key::Enter);
                periscope_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                periscope_toggle_mode |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                periscope_expand_folder |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                periscope_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.find_note_open {
                find_note_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                find_note_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                find_note_open_modal |= i.consume_key(egui::Modifiers::SHIFT, Key::Enter);
                find_note_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                find_note_toggle |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                find_note_toggle |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                find_note_toggle |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                find_note_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
                find_note_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.periscope_prompt_open {
                periscope_prompt_yes |= i.consume_key(egui::Modifiers::NONE, Key::Y);
                periscope_prompt_yes |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                periscope_prompt_no |= i.consume_key(egui::Modifiers::NONE, Key::N);
            }
            if self.link_assist_open {
                link_assist_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                link_assist_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                link_assist_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                link_assist_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                link_assist_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.link_assist_web_open {
                link_web_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                link_web_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
                link_web_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.link_heading_picker_open {
                link_heading_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                link_heading_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                link_heading_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                link_heading_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                link_heading_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.link_preview_open {
                link_preview_confirm |= i.consume_key(egui::Modifiers::CTRL, Key::Enter);
                link_preview_save |= i.consume_key(egui::Modifiers::CTRL, Key::S);
                link_preview_swap |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                    )
                });
                if link_preview_swap {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Tab,
                                pressed: true,
                                ..
                            }
                        )
                    });
                }
                link_preview_cancel |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Escape,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                });
                if link_preview_cancel {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Escape,
                                pressed: true,
                                ..
                            }
                        )
                    });
                }
            }
            if self.save_as_open {
                save_as_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                save_as_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                save_as_enter_dir |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                save_as_parent |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                save_as_enter |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                save_as_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.scratch_modal_open {
                scratch_archive |= i.consume_key(egui::Modifiers::CTRL, Key::S);
                scratch_open_entries |= i.consume_key(egui::Modifiers::CTRL, Key::E);
            }
            if self.capture_modal_open {
                capture_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
            }
            if self.scratch_entries_open {
                scratch_entries_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                scratch_entries_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                scratch_entries_delete |= i.consume_key(egui::Modifiers::NONE, Key::Delete);
                scratch_entries_delete |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::D,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if modifiers.ctrl && !modifiers.alt
                    )
                });
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::P) {
                self.recent_picker_open = false;
                self.lance_open = false;
                self.doc_tasks_open = false;
                self.file_picker_open = false;
                self.periscope_open = false;
                self.periscope_prompt_open = false;
                self.find_note_open = false;
                self.link_heading_picker_open = false;
                if !self.link_preview_dirty {
                    self.link_preview_open = false;
                }
                self.save_as_open = false;
                self.scratch_modal_open = false;
                self.scratch_entries_open = false;
                self.capture_modal_open = false;
                self.palette_open = true;
                self.palette_query.clear();
                self.selected_command = 0;
            }
            if search_active {
                search_cursor_after |= i.consume_key(egui::Modifiers::CTRL, Key::F);
                search_cursor_before |= i.consume_key(egui::Modifiers::CTRL, Key::B);
            } else if i.consume_key(egui::Modifiers::CTRL, Key::F) {
                self.palette_open = false;
                self.lance_open = false;
                self.command_line = "find ".to_string();
                self.command_line_cursor = self.command_line.len();
                self.command_history_index = None;
                self.command_line_focused = true;
                self.focus_command_line_once = true;
                self.focus_editor_once = false;
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::Period) {
                self.palette_open = false;
                self.recent_picker_open = false;
                self.lance_open = false;
                self.doc_tasks_open = false;
                self.file_picker_open = false;
                self.periscope_open = false;
                self.periscope_prompt_open = false;
                self.find_note_open = false;
                self.link_heading_picker_open = false;
                if !self.link_preview_dirty {
                    self.link_preview_open = false;
                }
                self.save_as_open = false;
                self.scratch_modal_open = false;
                self.scratch_entries_open = false;
                self.capture_modal_open = false;
                self.command_line.clear();
                self.command_line_cursor = 0;
                self.command_history_index = None;
                self.command_line_focused = true;
                self.focus_command_line_once = true;
                self.focus_editor_once = false;
            }
            if self.command_line_focused || self.focus_command_line_once {
                execute_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                complete_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                previous_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                next_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                command_line_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
                command_line_delete |= i.consume_key(egui::Modifiers::NONE, Key::Delete);
                command_line_left |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                command_line_right |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                command_line_home |= i.consume_key(egui::Modifiers::NONE, Key::Home);
                command_line_end |= i.consume_key(egui::Modifiers::NONE, Key::End);
            } else if search_active {
                search_next |= i.consume_key(egui::Modifiers::NONE, Key::F);
                search_previous |= i.consume_key(egui::Modifiers::NONE, Key::B);
                search_accept |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                search_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.consume_key(egui::Modifiers::CTRL, Key::N)
            {
                command = Some(Command::New);
            }
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.consume_key(egui::Modifiers::CTRL, Key::O)
            {
                command = Some(Command::Open);
            }
            let save_pressed = !self.link_preview_open
                && !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::S,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if modifiers.ctrl && !modifiers.alt && !modifiers.shift
                    )
                });
            let save_as_pressed = !self.link_preview_open
                && !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::S,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if modifiers.ctrl && modifiers.alt && !modifiers.shift
                    )
                });
            if save_as_pressed {
                command = Some(Command::SaveAs);
            } else if save_pressed {
                command = Some(Command::Save);
            }
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.consume_key(egui::Modifiers::CTRL, Key::M)
            {
                command = Some(Command::TogglePreview);
            }
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.consume_key(egui::Modifiers::CTRL, Key::Q)
            {
                command = Some(Command::Quit);
            }
            if !search_cancel && i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                if self.settings_open {
                    self.settings_open = false;
                    self.focus_editor_once = true;
                } else if self.command_line_focused || self.focus_command_line_once {
                    self.command_line.clear();
                    self.command_line_cursor = 0;
                    self.command_line_focused = false;
                    self.focus_command_line_once = false;
                    self.command_history_index = None;
                    self.focus_editor_once = true;
                } else if self.shortcut_help_open {
                    self.shortcut_help_open = false;
                    self.focus_editor_once = true;
                } else if self.recent_picker_open {
                    self.recent_picker_open = false;
                    self.pending_recent_path = None;
                    self.focus_editor_once = true;
                } else if self.lance_open {
                    self.lance_open = false;
                    self.focus_editor_once = true;
                } else if self.doc_tasks_open {
                    self.doc_tasks_open = false;
                    self.focus_editor_once = true;
                } else if self.agenda_open {
                    if self.standalone_agenda {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                    self.agenda_open = false;
                    self.focus_editor_once = true;
                } else if self.file_picker_open {
                    self.file_picker_open = false;
                    self.pending_project_file_path = None;
                    self.pending_open_heading_fragment = None;
                    if self.file_picker_mode == FilePickerMode::InsertMarkdownLink {
                        self.cancel_link_assist();
                    }
                    if self.file_picker_mode == FilePickerMode::Refile {
                        self.pending_refile = None;
                        self.pending_refile_destination = None;
                    }
                    self.focus_editor_once = true;
                } else if self.periscope_open {
                    self.periscope_open = false;
                    self.focus_editor_once = true;
                } else if self.periscope_prompt_open {
                    self.periscope_prompt_open = false;
                    self.focus_editor_once = true;
                } else if self.find_note_open {
                    self.find_note_open = false;
                    self.focus_editor_once = true;
                } else if self.link_heading_picker_open {
                    self.cancel_link_assist();
                } else if self.link_preview_open {
                    self.close_link_preview();
                } else if self.save_as_open {
                    self.save_as_open = false;
                    self.focus_editor_once = true;
                } else if self.scratch_modal_open {
                    self.cancel_scratch_modal();
                } else if self.scratch_entries_open {
                    self.scratch_entries_open = false;
                    self.focus_editor_once = true;
                } else if self.capture_modal_open {
                    self.capture_modal_open = false;
                    self.capture_title.clear();
                    self.capture_text.clear();
                    self.focus_editor_once = true;
                } else if self.palette_open {
                    self.palette_open = false;
                    self.focus_editor_once = true;
                } else if self.pending_action.is_some() {
                    self.pending_action = None;
                    self.pending_daily_target = None;
                    self.focus_editor_once = true;
                } else if self.scratch {
                    self.close_scratch_with_feedback(ctx);
                    return;
                }
            }
        });

        if scratch_archive {
            self.archive_scratch_modal();
            return;
        }

        if scratch_open_entries {
            self.open_scratch_entries_modal();
            return;
        }

        if scratch_entries_previous {
            self.selected_scratch_entry = self.selected_scratch_entry.saturating_sub(1);
            return;
        }

        if scratch_entries_next {
            self.selected_scratch_entry =
                (self.selected_scratch_entry + 1).min(self.scratch_entries.len().saturating_sub(1));
            return;
        }

        if scratch_entries_delete {
            self.delete_selected_scratch_entry();
            return;
        }

        if capture_confirm {
            self.confirm_capture_modal();
            return;
        }

        if search_cursor_after {
            self.place_cursor_at_search_edge(true);
            return;
        }

        if search_cursor_before {
            self.place_cursor_at_search_edge(false);
            return;
        }

        if search_cancel {
            self.cancel_search();
            return;
        }

        if search_accept {
            self.accept_search();
            return;
        }

        if search_next {
            self.move_search_match(true);
            return;
        }

        if search_previous {
            self.move_search_match(false);
            return;
        }

        if self.recent_picker_open {
            self.handle_recent_picker_text_input(ctx);
        }

        if self.lance_open {
            self.handle_lance_text_input(ctx);
        }

        if self.doc_tasks_open {
            self.handle_doc_tasks_text_input(ctx);
        }

        if self.file_picker_open {
            self.handle_file_picker_text_input(ctx);
        }

        if self.periscope_open {
            self.handle_periscope_text_input(ctx);
        }

        if self.find_note_open {
            self.handle_find_note_text_input(ctx);
        }

        if self.link_assist_web_open {
            self.handle_link_assist_web_text_input(ctx);
        }

        if self.save_as_open {
            self.handle_save_as_text_input(ctx);
        }

        if self.handle_flow_inline_org_keyboard(ctx) {
            return;
        }

        if link_assist_cancel || link_web_cancel || link_heading_cancel {
            self.cancel_link_assist();
            return;
        }

        if link_preview_cancel {
            self.close_link_preview();
            return;
        }

        if link_preview_save {
            self.save_link_preview();
            return;
        }

        if link_preview_swap {
            self.swap_link_preview_with_main();
            return;
        }

        if link_preview_confirm {
            self.commit_link_preview();
            return;
        }

        if link_heading_previous {
            self.move_link_heading_selection(-1);
            return;
        }

        if link_heading_next {
            self.move_link_heading_selection(1);
            return;
        }

        if link_heading_confirm {
            self.confirm_link_heading_picker();
            return;
        }

        if link_assist_previous || link_assist_next {
            self.link_assist_choice = match self.link_assist_choice {
                LinkAssistChoice::File => LinkAssistChoice::Web,
                LinkAssistChoice::Web => LinkAssistChoice::File,
            };
            return;
        }

        if link_assist_confirm {
            self.confirm_link_assist_choice();
            return;
        }

        if link_web_backspace {
            self.link_assist_web_url.pop();
            return;
        }

        if link_web_confirm {
            let url = self.link_assist_web_url.trim().to_string();
            if url.is_empty() {
                self.status = "Insert link: URL is empty".to_string();
            } else {
                self.link_assist_web_open = false;
                self.insert_assisted_markdown_link(&url);
            }
            return;
        }

        if save_as_backspace {
            self.save_as_filename.pop();
            self.selected_save_as_entry =
                self.save_as_entry_indices().first().copied().unwrap_or(0);
            return;
        }

        if save_as_previous {
            self.move_save_as_selection(-1);
            return;
        }

        if save_as_next {
            self.move_save_as_selection(1);
            return;
        }

        if save_as_enter_dir {
            self.save_as_enter_selected_dir();
            return;
        }

        if save_as_parent {
            self.save_as_go_parent();
            return;
        }

        if save_as_enter {
            self.confirm_save_as();
            return;
        }

        if periscope_prompt_yes {
            self.open_periscope_project_folder_picker();
            return;
        }

        if periscope_prompt_no {
            self.open_global_periscope();
            return;
        }

        if find_note_cancel {
            self.find_note_open = false;
            self.focus_editor_once = true;
            return;
        }

        if find_note_backspace {
            self.find_note_query.pop();
            self.find_note_expanded_files.clear();
            self.selected_find_note_row = 0;
            self.refresh_find_note_results();
            return;
        }

        if find_note_previous {
            self.move_find_note_selection(-1);
            return;
        }

        if find_note_next {
            self.move_find_note_selection(1);
            return;
        }

        if find_note_toggle {
            self.toggle_selected_find_note_file();
            return;
        }

        if find_note_open_modal {
            self.open_selected_find_note(true);
            return;
        }

        if find_note_open {
            self.open_selected_find_note(false);
            return;
        }

        if periscope_backspace {
            self.periscope_query.pop();
            self.periscope_expanded_folders.clear();
            self.selected_periscope_result = 0;
            self.refresh_periscope_results();
            return;
        }

        if periscope_previous {
            self.move_periscope_selection(-1);
            return;
        }

        if periscope_next {
            self.move_periscope_selection(1);
            return;
        }

        if periscope_toggle_mode {
            self.toggle_periscope_mode();
            return;
        }

        if periscope_expand_folder && self.toggle_selected_periscope_folder() {
            return;
        }

        if periscope_open_modal {
            self.open_selected_periscope_result_as_modal();
            return;
        }

        if periscope_open {
            self.open_selected_periscope_result();
            return;
        }

        if file_backspace {
            self.file_query.pop();
            self.selected_project_file = self.project_file_indices().first().copied().unwrap_or(0);
            return;
        }

        if file_previous {
            self.move_project_file_selection(-1);
            return;
        }

        if file_next {
            self.move_project_file_selection(1);
            return;
        }

        if file_enter_dir {
            self.file_picker_enter_selected_dir();
            return;
        }

        if file_parent {
            self.file_picker_go_parent();
            return;
        }

        if file_open {
            self.open_selected_project_file();
            return;
        }

        if recent_backspace {
            self.recent_query.pop();
            self.selected_recent_file = self.recent_file_indices().first().copied().unwrap_or(0);
            return;
        }

        if recent_previous {
            self.move_recent_selection(-1);
            return;
        }

        if recent_next {
            self.move_recent_selection(1);
            return;
        }

        if recent_open {
            self.open_selected_recent_file();
            return;
        }

        if lance_backspace {
            self.lance_query.pop();
            self.selected_lance_file = self.lance_file_indices().first().copied().unwrap_or(0);
            return;
        }

        if lance_previous {
            self.move_lance_selection(-1);
            return;
        }

        if lance_next {
            self.move_lance_selection(1);
            return;
        }

        if lance_add {
            self.open_lance_file_picker();
            return;
        }

        if lance_delete {
            self.remove_selected_lance_file();
            return;
        }

        if lance_open_modal {
            self.open_selected_lance_file_as_modal();
            return;
        }

        if lance_open {
            self.open_selected_lance_file();
            return;
        }

        if doc_task_backspace {
            self.doc_task_query.pop();
            let tasks = self.doc_tasks();
            self.selected_doc_task_line = self
                .doc_task_indices()
                .first()
                .and_then(|index| tasks.get(*index))
                .map(|task| task.line_index)
                .unwrap_or(0);
            return;
        }

        if doc_task_previous {
            self.move_doc_task_selection(-1);
            return;
        }

        if doc_task_next {
            self.move_doc_task_selection(1);
            return;
        }

        if doc_task_cycle {
            self.cycle_selected_doc_task();
            return;
        }

        if doc_task_open {
            self.jump_to_selected_doc_task();
            return;
        }

        if agenda_org_tag {
            self.open_agenda_org_autocomplete(AgendaOrgMode::Tag);
            return;
        }

        if agenda_org_project {
            self.open_agenda_org_autocomplete(AgendaOrgMode::Project);
            return;
        }

        if agenda_previous {
            self.move_agenda_selection(-1);
            return;
        }

        if agenda_next {
            self.move_agenda_selection(1);
            return;
        }

        if agenda_previous_day {
            self.move_agenda_week_window(-1);
            return;
        }

        if agenda_next_day {
            self.move_agenda_week_window(1);
            return;
        }

        if agenda_previous_week {
            self.move_agenda_week_window(-1);
            return;
        }

        if agenda_next_week {
            self.move_agenda_week_window(1);
            return;
        }

        if agenda_today {
            self.jump_agenda_to_today();
            return;
        }

        if agenda_cycle_tab {
            self.cycle_agenda_tab();
            return;
        }

        if agenda_toggle_focus {
            self.toggle_agenda_focus();
            return;
        }

        if agenda_toggle_done {
            self.toggle_agenda_done_visibility();
            return;
        }

        if agenda_edit {
            self.open_selected_flow_item_edit();
            return;
        }

        if agenda_refile {
            self.start_refile_selected_agenda_item();
            return;
        }

        if agenda_cycle {
            self.cycle_selected_agenda_item();
            return;
        }

        if agenda_open {
            self.activate_selected_agenda_row();
            if self.standalone_agenda && !self.agenda_open {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        if settings_previous {
            self.selected_setting = self.selected_setting.saturating_sub(1);
            return;
        }

        if settings_next {
            self.selected_setting = (self.selected_setting + 1).min(7);
            return;
        }

        if settings_decrement {
            match self.selected_setting {
                0 => self.set_command_history_limit(self.command_history_limit.saturating_sub(1)),
                1 => self.set_line_number_mode(LineNumberMode::Absolute),
                2 => self.set_ctrl_shift_move_mode(CtrlShiftMoveMode::Vim),
                3 => self.set_wrap_mode(false),
                4 => self.set_preview_mode(false),
                5 => self.set_markdown_live_rendering(false),
                6 => self.set_virtual_references_enabled(false),
                7 => self.set_reopen_last_file_on_startup(false),
                _ => {}
            }
            return;
        }

        if settings_increment || settings_activate {
            match self.selected_setting {
                0 => self.set_command_history_limit(self.command_history_limit + 1),
                1 => {
                    let next_mode = match self.line_number_mode {
                        LineNumberMode::Absolute => LineNumberMode::Relative,
                        LineNumberMode::Relative => LineNumberMode::Absolute,
                    };
                    self.set_line_number_mode(next_mode);
                }
                2 => self.set_ctrl_shift_move_mode(self.ctrl_shift_move_mode.next()),
                3 => self.set_wrap_mode(!self.wrap),
                4 => self.set_preview_mode(!self.preview),
                5 => self.set_markdown_live_rendering(!self.markdown_live_rendering),
                6 => self.set_virtual_references_enabled(!self.virtual_references_enabled),
                7 => self.set_reopen_last_file_on_startup(!self.reopen_last_file_on_startup),
                _ => {}
            }
            return;
        }

        if self.command_line_focused || self.focus_command_line_once {
            self.handle_command_line_text_input(ctx);
            if command_line_backspace {
                self.command_line_backspace();
                return;
            }
            if command_line_delete {
                self.command_line_delete();
                return;
            }
            if command_line_left {
                self.command_line_cursor = self.previous_command_line_boundary();
                return;
            }
            if command_line_right {
                self.command_line_cursor = self.next_command_line_boundary();
                return;
            }
            if command_line_home {
                self.command_line_cursor = 0;
                return;
            }
            if command_line_end {
                self.command_line_cursor = self.command_line.len();
                return;
            }
        }

        let navigating_command_history = self.command_history_index.is_some();
        let command_suggestion_count = if (self.command_line_focused
            || self.focus_command_line_once)
            && !navigating_command_history
        {
            self.command_line_suggestions().len()
        } else {
            0
        };

        if previous_command {
            if command_suggestion_count > 0 {
                self.selected_command_line_suggestion =
                    self.selected_command_line_suggestion.saturating_sub(1);
                self.focus_command_line_once = true;
                return;
            }
            if !self.command_history.is_empty() {
                let index = self
                    .command_history_index
                    .unwrap_or(self.command_history.len())
                    .saturating_sub(1);
                self.command_history_index = Some(index);
                self.command_line = self.command_history[index].clone();
                self.command_line_cursor = self.command_line.len();
                self.selected_command_line_suggestion = 0;
                self.focus_command_line_once = true;
                return;
            }
        }

        if next_command {
            if command_suggestion_count > 0 {
                self.selected_command_line_suggestion = (self.selected_command_line_suggestion + 1)
                    .min(command_suggestion_count.saturating_sub(1));
                self.focus_command_line_once = true;
                return;
            }
            if let Some(index) = self.command_history_index {
                if index + 1 < self.command_history.len() {
                    let index = index + 1;
                    self.command_history_index = Some(index);
                    self.command_line = self.command_history[index].clone();
                    self.command_line_cursor = self.command_line.len();
                } else {
                    self.command_history_index = None;
                    self.command_line.clear();
                    self.command_line_cursor = 0;
                }
                self.selected_command_line_suggestion = 0;
                self.focus_command_line_once = true;
            }
            return;
        }

        if complete_command_line {
            if !self.accept_command_line_completion() {
                self.run_command_line(ctx);
            }
            return;
        }

        if execute_command_line {
            if command_suggestion_count > 0 {
                let suggestions = self.command_line_suggestions();
                if let Some(spec) = suggestions.get(
                    self.selected_command_line_suggestion
                        .min(suggestions.len().saturating_sub(1)),
                ) {
                    self.command_line = spec.name.to_string();
                    self.command_line_cursor = self.command_line.len();
                }
            }
            self.run_command_line(ctx);
            return;
        }

        if let Some(command) = command {
            self.run_command(command, ctx);
        }
    }

    fn filtered_commands(&self) -> Vec<&'static CommandSpec> {
        self.matching_command_specs(&self.palette_query, 12)
    }

    fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }

        let commands = self.filtered_commands();
        if self.selected_command >= commands.len() {
            self.selected_command = commands.len().saturating_sub(1);
        }

        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                self.selected_command =
                    (self.selected_command + 1).min(commands.len().saturating_sub(1));
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                self.selected_command = self.selected_command.saturating_sub(1);
            }
        });

        let frame = egui::Frame::new()
            .fill(Color32::from_rgb(25, 31, 40))
            .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
            .corner_radius(0.0)
            .inner_margin(14.0)
            .shadow(egui::epaint::Shadow {
                offset: [0, 10],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(140),
            });

        egui::Area::new("command_palette".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(
                        RichText::new("command palette")
                            .font(FontId::new(16.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(136, 192, 208)),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("slate:~$")
                                .font(FontId::new(15.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(163, 190, 140)),
                        );
                        let response = ui.add(
                            TextEdit::singleline(&mut self.palette_query)
                                .hint_text("type a command")
                                .desired_width(f32::INFINITY)
                                .font(FontId::new(15.0, FontFamily::Monospace))
                                .text_color(Color32::from_rgb(216, 222, 233))
                                .frame(egui::Frame::NONE),
                        );
                        response.request_focus();
                    });
                    ui.add_space(8.0);
                    ui.painter().hline(
                        ui.available_rect_before_wrap().x_range(),
                        ui.cursor().top(),
                        Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                    );
                    ui.add_space(8.0);

                    if commands.is_empty() {
                        ui.label(
                            RichText::new("no matching commands")
                                .font(FontId::new(14.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(94, 105, 126)),
                        );
                    }

                    for (idx, command) in commands.iter().enumerate() {
                        let selected = idx == self.selected_command;
                        let fill = if selected {
                            Color32::from_rgb(46, 56, 72)
                        } else {
                            Color32::TRANSPARENT
                        };
                        let label_color = if selected {
                            Color32::from_rgb(236, 239, 244)
                        } else {
                            Color32::from_rgb(216, 222, 233)
                        };
                        let row = egui::Frame::new()
                            .fill(fill)
                            .corner_radius(0.0)
                            .inner_margin(6.0);
                        let clicked = row
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(if selected { ">" } else { " " })
                                            .font(FontId::new(14.0, FontFamily::Monospace))
                                            .color(Color32::from_rgb(136, 192, 208)),
                                    );
                                    ui.label(
                                        RichText::new(command.name)
                                            .font(FontId::new(14.0, FontFamily::Monospace))
                                            .color(label_color),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(command.hint)
                                                    .font(FontId::new(13.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 154, 176)),
                                            );
                                        },
                                    );
                                })
                            })
                            .response
                            .clicked();
                        if clicked {
                            if let Some(command) = command.palette_command {
                                self.run_command(command, ctx);
                            } else {
                                self.palette_open = false;
                                self.command_line = command.name.to_string();
                                self.command_line_cursor = self.command_line.len();
                                self.command_line_focused = true;
                                self.focus_command_line_once = true;
                                self.focus_editor_once = false;
                            }
                            return;
                        }
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        for (key, label) in [("↑↓", "move"), ("enter", "run"), ("esc", "close")]
                        {
                            ui.label(
                                RichText::new(format!("[{key}]"))
                                    .font(FontId::new(13.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(235, 203, 139)),
                            );
                            ui.label(
                                RichText::new(label)
                                    .font(FontId::new(13.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(136, 154, 176)),
                            );
                            ui.add_space(10.0);
                        }
                    });

                    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
                    if enter {
                        if let Some(command) = commands.get(self.selected_command).copied() {
                            if let Some(palette_command) = command.palette_command {
                                self.run_command(palette_command, ctx);
                            } else {
                                self.palette_open = false;
                                self.command_line = command.name.to_string();
                                self.command_line_cursor = self.command_line.len();
                                self.command_line_focused = true;
                                self.focus_command_line_once = true;
                                self.focus_editor_once = false;
                            }
                        }
                    }
                });
            });
    }

    fn confirm_action_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_action else {
            return;
        };

        let mut discard = false;
        let mut go_back = false;
        let mut save = false;
        ctx.input_mut(|i| {
            discard |= i.consume_key(egui::Modifiers::NONE, Key::Y);
            go_back |= i.consume_key(egui::Modifiers::NONE, Key::N);
            save |= i.consume_key(egui::Modifiers::NONE, Key::S);
            go_back |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
        });

        if discard {
            self.dirty = false;
            self.pending_action = None;
            self.finish_pending_action(action, ctx);
            return;
        }
        if go_back {
            self.pending_action = None;
            self.pending_daily_target = None;
            self.focus_editor_once = true;
            return;
        }
        if save {
            self.save();
            if !self.dirty {
                self.pending_action = None;
                self.finish_pending_action(action, ctx);
            }
            return;
        }

        egui::Area::new("confirm_close_prompt".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    })
                    .show(ui, |ui| {
                        ui.set_width(520.0);
                        ui.label(
                            RichText::new("unsaved changes")
                                .font(FontId::new(16.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(136, 192, 208)),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(action.prompt())
                                .font(FontId::new(14.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(216, 222, 233)),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            for (key, label, color) in [
                                ("y", "yes / discard", Color32::from_rgb(191, 97, 106)),
                                ("n", "no / return", Color32::from_rgb(163, 190, 140)),
                                ("s", "save…", Color32::from_rgb(235, 203, 139)),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(FontId::new(14.0, FontFamily::Monospace))
                                        .color(color),
                                );
                                ui.label(
                                    RichText::new(label)
                                        .font(FontId::new(14.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(136, 154, 176)),
                                );
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn handle_recent_picker_text_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        if ctx.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            return;
        }
        let Some((_, text)) = Self::normalized_text_input(&events) else {
            return;
        };
        if text.chars().any(|ch| ch.is_control()) {
            return;
        }
        self.recent_query.push_str(&text);
        self.selected_recent_file = self.recent_file_indices().first().copied().unwrap_or(0);
    }

    fn handle_lance_text_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        if ctx.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            return;
        }
        let Some((_, text)) = Self::normalized_text_input(&events) else {
            return;
        };
        if text.chars().any(|ch| ch.is_control()) {
            return;
        }
        self.lance_query.push_str(&text);
        self.selected_lance_file = self.lance_file_indices().first().copied().unwrap_or(0);
    }

    fn handle_doc_tasks_text_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        if ctx.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            return;
        }
        let Some((_, text)) = Self::normalized_text_input(&events) else {
            return;
        };
        if text.chars().any(|ch| ch.is_control()) {
            return;
        }
        self.doc_task_query.push_str(&text);
        let tasks = self.doc_tasks();
        self.selected_doc_task_line = self
            .doc_task_indices()
            .first()
            .and_then(|index| tasks.get(*index))
            .map(|task| task.line_index)
            .unwrap_or(0);
    }

    fn handle_file_picker_text_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        if ctx.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            return;
        }
        let Some((_, text)) = Self::normalized_text_input(&events) else {
            return;
        };
        if text.chars().any(|ch| ch.is_control()) {
            return;
        }
        self.file_query.push_str(&text);
        self.selected_project_file = self.project_file_indices().first().copied().unwrap_or(0);
    }

    fn handle_link_assist_web_text_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        if ctx.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            return;
        }
        let Some((_, text)) = Self::normalized_text_input(&events) else {
            return;
        };
        if text.chars().any(|ch| ch.is_control()) {
            return;
        }
        self.link_assist_web_url.push_str(&text);
    }

    fn handle_save_as_text_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        if ctx.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            return;
        }
        let Some((_, text)) = Self::normalized_text_input(&events) else {
            return;
        };
        if text.chars().any(|ch| ch.is_control()) {
            return;
        }
        self.save_as_filename.push_str(&text);
        self.selected_save_as_entry = self.save_as_entry_indices().first().copied().unwrap_or(0);
    }

    fn handle_command_line_text_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        for event in &events {
            if let egui::Event::Paste(text) = event {
                self.insert_command_line_text(text);
            }
        }
        if ctx.input(|input| input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt)
        {
            return;
        }
        if let Some((_, text)) = Self::normalized_text_input(&events) {
            self.insert_command_line_text(&text);
        }
    }

    fn insert_command_line_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.command_line_cursor = self.command_line_cursor.min(self.command_line.len());
        while self.command_line_cursor > 0
            && !self.command_line.is_char_boundary(self.command_line_cursor)
        {
            self.command_line_cursor -= 1;
        }
        self.command_line.insert_str(self.command_line_cursor, text);
        self.command_line_cursor += text.len();
        self.selected_command_line_suggestion = 0;
        self.command_history_index = None;
    }

    fn command_line_backspace(&mut self) {
        if self.command_line_cursor == 0 {
            return;
        }
        let previous = self.previous_command_line_boundary();
        self.command_line
            .replace_range(previous..self.command_line_cursor, "");
        self.command_line_cursor = previous;
        self.selected_command_line_suggestion = 0;
        self.command_history_index = None;
    }

    fn command_line_delete(&mut self) {
        if self.command_line_cursor >= self.command_line.len() {
            return;
        }
        let next = self.next_command_line_boundary();
        self.command_line
            .replace_range(self.command_line_cursor..next, "");
        self.selected_command_line_suggestion = 0;
        self.command_history_index = None;
    }

    fn previous_command_line_boundary(&self) -> usize {
        let mut byte = self
            .command_line_cursor
            .min(self.command_line.len())
            .saturating_sub(1);
        while byte > 0 && !self.command_line.is_char_boundary(byte) {
            byte -= 1;
        }
        byte
    }

    fn next_command_line_boundary(&self) -> usize {
        let mut byte = (self.command_line_cursor.min(self.command_line.len()) + 1)
            .min(self.command_line.len());
        while byte < self.command_line.len() && !self.command_line.is_char_boundary(byte) {
            byte += 1;
        }
        byte
    }

    fn normalized_text_input(events: &[egui::Event]) -> Option<(String, String)> {
        let texts = events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Text(text) if !text.is_empty() => Some(text.as_str()),
                egui::Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty() => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        let raw = texts.concat();
        let normalized = match texts.as_slice() {
            [] => return None,
            [text] => (*text).to_string(),
            many => {
                let last = many.last().copied().unwrap_or_default();
                if last.chars().count() == 1 && last.chars().any(|ch| !ch.is_ascii()) {
                    last.to_string()
                } else {
                    raw.clone()
                }
            }
        };

        Some((raw, normalized))
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        egui::Area::new("settings_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -30.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    })
                    .show(ui, |ui| {
                        ui.set_width(520.0);
                        ui.label(
                            RichText::new("settings")
                                .font(FontId::new(16.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(136, 192, 208)),
                        );
                        ui.add_space(10.0);

                        egui::Frame::new()
                            .fill(Color32::from_rgb(30, 36, 48))
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                let selected_fill = Color32::from_rgb(46, 56, 72);
                                let normal_fill = Color32::from_rgb(30, 36, 48);

                                let history_selected = self.selected_setting == 0;
                                egui::Frame::new()
                                    .fill(if history_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if history_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("History length")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let response = ui.add(
                                                        egui::DragValue::new(&mut self.command_history_limit)
                                                            .range(1..=50)
                                                            .speed(1),
                                                    );
                                                    if response.changed() {
                                                        self.set_command_history_limit(
                                                            self.command_history_limit,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Visible command history rows when Ctrl+. opens the commandline.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                let line_numbers_selected = self.selected_setting == 1;
                                egui::Frame::new()
                                    .fill(if line_numbers_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if line_numbers_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Line numbers")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let next_mode = match self.line_number_mode {
                                                        LineNumberMode::Absolute => LineNumberMode::Relative,
                                                        LineNumberMode::Relative => LineNumberMode::Absolute,
                                                    };
                                                    if ui
                                                        .button(self.line_number_mode.label())
                                                        .on_hover_text("Toggle absolute/relative line numbers")
                                                        .clicked()
                                                    {
                                                        self.set_line_number_mode(next_mode);
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Absolute shows file line numbers. Relative treats the cursor line as 1 and counts distance above/below.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                let movement_selected = self.selected_setting == 2;
                                egui::Frame::new()
                                    .fill(if movement_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if movement_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Ctrl+Shift movement")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(self.ctrl_shift_move_mode.label())
                                                        .on_hover_text("Toggle Vim hjkl / Slate ijkl live movement")
                                                        .clicked()
                                                    {
                                                        self.set_ctrl_shift_move_mode(
                                                            self.ctrl_shift_move_mode.next(),
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(self.ctrl_shift_move_mode.hint())
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                let wrap_selected = self.selected_setting == 3;
                                egui::Frame::new()
                                    .fill(if wrap_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if wrap_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Word wrap")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.wrap { "on" } else { "off" })
                                                        .on_hover_text("Toggle editor word wrap")
                                                        .clicked()
                                                    {
                                                        self.set_wrap_mode(!self.wrap);
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Persisted editor wrapping preference. Command: :wrap on|off")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                let preview_selected = self.selected_setting == 4;
                                egui::Frame::new()
                                    .fill(if preview_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if preview_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Markdown preview")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.preview { "on" } else { "off" })
                                                        .on_hover_text("Toggle Markdown preview split")
                                                        .clicked()
                                                    {
                                                        self.set_preview_mode(!self.preview);
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Persisted preview/split preference. Command: :preview on|off")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                let markdown_live_selected = self.selected_setting == 5;
                                egui::Frame::new()
                                    .fill(if markdown_live_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if markdown_live_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Markdown live rendering")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.markdown_live_rendering { "live" } else { "plain" })
                                                        .on_hover_text("Choose plain source text or live inline Markdown affordances while editing")
                                                        .clicked()
                                                    {
                                                        self.set_markdown_live_rendering(
                                                            !self.markdown_live_rendering,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Preview split always keeps the editor side plain and renders Markdown on the right.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                let virtual_refs_selected = self.selected_setting == 6;
                                egui::Frame::new()
                                    .fill(if virtual_refs_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if virtual_refs_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Virtual backlinks/mentions")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.virtual_references_enabled { "show" } else { "hide" })
                                                        .on_hover_text("Render the virtual Backlinks/Mentions section at the end of saved Markdown files")
                                                        .clicked()
                                                    {
                                                        self.set_virtual_references_enabled(
                                                            !self.virtual_references_enabled,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Shows a non-saved end-of-file references section. Click the virtual header, Backlinks, or Mentions to collapse/expand.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });

                                ui.add_space(6.0);
                                let reopen_selected = self.selected_setting == 7;
                                egui::Frame::new()
                                    .fill(if reopen_selected { selected_fill } else { normal_fill })
                                    .inner_margin(6.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if reopen_selected { ">" } else { " " })
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(136, 192, 208)),
                                            );
                                            ui.label(
                                                RichText::new("Reopen last file on startup")
                                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                                    .color(Color32::from_rgb(216, 222, 233)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .button(if self.reopen_last_file_on_startup { "on" } else { "off" })
                                                        .on_hover_text("Open Slate with the last opened file when no file path is provided")
                                                        .clicked()
                                                    {
                                                        self.set_reopen_last_file_on_startup(
                                                            !self.reopen_last_file_on_startup,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Off by default. Ctrl+O L / open-last remains the intentional manual workflow.")
                                                .font(FontId::new(13.0, FontFamily::Monospace))
                                                .color(Color32::from_rgb(136, 154, 176)),
                                        );
                                    });
                            });

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [("↑↓", "select"), ("←→", "adjust"), ("enter", "toggle"), ("esc", "close")] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(FontId::new(13.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(235, 203, 139)),
                                );
                                ui.label(
                                    RichText::new(label)
                                        .font(FontId::new(13.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(136, 154, 176)),
                                );
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn text_for_width(text: &str, width: f32, font_size: f32) -> String {
        let max_chars = ((width / (font_size * 0.62)).floor() as usize).max(1);
        if text.chars().count() <= max_chars {
            return text.to_string();
        }
        if max_chars <= 2 {
            return "…".to_string();
        }
        let mut trimmed: String = text.chars().take(max_chars - 1).collect();
        trimmed.push('…');
        trimmed
    }

    fn compact_shortcut_desc(desc: &str) -> &str {
        match desc {
            "open commandline" => "commandline",
            "open command palette/browser" => "palette",
            "open this help" => "help",
            "close modal / cancel active mode" => "close/cancel",
            "toggle Markdown preview" => "preview",
            "open last file" => "open last",
            "find next / previous" => "find next/prev",
            "select word / line" => "select word/line",
            "delete word / line" => "delete word/line",
            "duplicate and place" => "dup+place",
            "go top / bottom" => "top/bottom",
            "live cursor movement" => "cursor move",
            "system nav layer: ijkl arrows" => "ijkl arrows",
            "move to paragraph edge" => "paragraph edge",
            "paragraph / word / line-edge jumps" => "jump edges",
            "move · Enter/Space place · Esc cancel" => "move/place/cancel",
            "history / picker selection" => "history/picker",
            "run command / accept picker item" => "run/accept",
            "filter recent files" => "filter recent",
            "absolute / relative goto" => "goto abs/rel",
            _ => desc,
        }
    }

    fn link_assist_dialog(&mut self, ctx: &egui::Context) {
        if !self.link_assist_open && !self.link_assist_web_open && !self.link_heading_picker_open {
            return;
        }

        egui::Area::new("link_assist_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(12.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    })
                    .show(ui, |ui| {
                        ui.set_width(360.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(15.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let warn = Color32::from_rgb(235, 203, 139);

                        if self.link_heading_picker_open {
                            ui.set_width(520.0);
                            let title = match self.link_heading_picker_mode {
                                LinkHeadingPickerMode::InsertLink => "insert link heading",
                                LinkHeadingPickerMode::BidirectionalLink => {
                                    "bidirectional link heading"
                                }
                                LinkHeadingPickerMode::Refile => "refile heading",
                            };
                            let hint = match self.link_heading_picker_mode {
                                LinkHeadingPickerMode::InsertLink => {
                                    "Enter on first option links the whole file"
                                }
                                LinkHeadingPickerMode::BidirectionalLink => {
                                    "Enter chooses where to write the reciprocal link"
                                }
                                LinkHeadingPickerMode::Refile => {
                                    "Enter on first option appends to the end of the file"
                                }
                            };
                            ui.label(RichText::new(title).font(title_font).color(accent));
                            ui.label(RichText::new(hint).font(font.clone()).color(dim));
                            ui.add_space(8.0);

                            let total = self.link_heading_options.len() + 1;
                            let visible_rows = 12usize;
                            let start = file_dialogs::centered_window_start(
                                self.selected_link_heading,
                                visible_rows,
                                total,
                            );
                            let end = (start + visible_rows).min(total);
                            for index in start..end {
                                let selected = index == self.selected_link_heading;
                                let label = if index == 0 {
                                    match self.link_heading_picker_mode {
                                        LinkHeadingPickerMode::InsertLink => {
                                            "No usar headers · linkear archivo entero".to_string()
                                        }
                                        LinkHeadingPickerMode::BidirectionalLink => {
                                            "No usar headers · backlink al final".to_string()
                                        }
                                        LinkHeadingPickerMode::Refile => {
                                            "No usar headers · append al final".to_string()
                                        }
                                    }
                                } else {
                                    let heading = &self.link_heading_options[index - 1];
                                    format!(
                                        "{}{}  · line {}",
                                        "  ".repeat(heading.level.saturating_sub(1)),
                                        heading.text,
                                        heading.line_index + 1
                                    )
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "{} {}",
                                        if selected { ">" } else { " " },
                                        label
                                    ))
                                    .font(font.clone())
                                    .color(if selected {
                                        accent
                                    } else {
                                        text
                                    }),
                                );
                            }
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                for (key, label) in
                                    [("↑↓", "choose"), ("enter", "insert"), ("esc", "cancel")]
                                {
                                    ui.label(
                                        RichText::new(format!("[{key}]"))
                                            .font(font.clone())
                                            .color(warn),
                                    );
                                    ui.label(RichText::new(label).font(font.clone()).color(dim));
                                    ui.add_space(8.0);
                                }
                            });
                            return;
                        }

                        if self.link_assist_web_open {
                            ui.label(
                                RichText::new("insert web link")
                                    .font(title_font)
                                    .color(accent),
                            );
                            ui.add_space(8.0);
                            let display = if self.link_assist_web_url.is_empty() {
                                "https://...".to_string()
                            } else {
                                self.link_assist_web_url.clone()
                            };
                            ui.label(
                                RichText::new(format!("url: {display}"))
                                    .font(font.clone())
                                    .color(if self.link_assist_web_url.is_empty() {
                                        dim
                                    } else {
                                        text
                                    }),
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                for (key, label) in [("enter", "insert"), ("esc", "cancel")] {
                                    ui.label(
                                        RichText::new(format!("[{key}]"))
                                            .font(font.clone())
                                            .color(warn),
                                    );
                                    ui.label(RichText::new(label).font(font.clone()).color(dim));
                                    ui.add_space(8.0);
                                }
                            });
                            return;
                        }

                        ui.label(RichText::new("insert link").font(title_font).color(accent));
                        ui.add_space(8.0);
                        for choice in [LinkAssistChoice::File, LinkAssistChoice::Web] {
                            let selected = choice == self.link_assist_choice;
                            let label = match choice {
                                LinkAssistChoice::File => "Archivo",
                                LinkAssistChoice::Web => "Web",
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{} {label}",
                                    if selected { ">" } else { " " }
                                ))
                                .font(font.clone())
                                .color(if selected {
                                    accent
                                } else {
                                    text
                                }),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            for (key, label) in
                                [("↑↓", "choose"), ("enter", "accept"), ("esc", "cancel")]
                            {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(8.0);
                            }
                        });
                    });
            });
    }

    fn link_preview_dialog(&mut self, ctx: &egui::Context) {
        if !self.link_preview_open {
            return;
        }

        egui::Area::new("link_preview_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let screen = ctx.content_rect();
                let size = Vec2::new(screen.width() * 0.86, screen.height() * 0.82);
                egui::Frame::new()
                    .fill(Color32::from_rgb(22, 28, 37))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(12.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(170),
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(size);
                        ui.set_max_size(size);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(15.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let warn = Color32::from_rgb(235, 203, 139);
                        let title = self
                            .link_preview_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "link preview".to_string());
                        let dirty_marker = if self.link_preview_dirty { "*" } else { "" };

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("modal buffer").font(title_font).color(accent));
                            ui.label(RichText::new(format!("{dirty_marker}{title}")).font(font.clone()).color(dim));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[tab] swap  [ctrl+enter] promote  [ctrl+s] save  [esc] close")
                                            .font(font.clone())
                                            .color(warn),
                                    );
                                },
                            );
                        });
                        if let Some(fragment) = self.link_preview_heading_fragment.as_deref() {
                            let target = self
                                .link_preview_heading_text
                                .as_deref()
                                .unwrap_or(fragment);
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("→ target heading")
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(
                                    RichText::new(format!("#{fragment}"))
                                        .font(font.clone())
                                        .color(Color32::from_rgb(235, 203, 139)),
                                );
                                ui.label(
                                    RichText::new(format!("· {target}"))
                                        .font(font.clone())
                                        .color(accent),
                                );
                            });
                        }
                        ui.add_space(8.0);
                        let editor_height = (ui.available_height() - 4.0).max(80.0);
                        ui.allocate_ui(Vec2::new(ui.available_width(), editor_height), |ui| {
                            if self.focus_link_preview_once {
                                self.link_preview_view.request_keyboard_focus();
                            }
                            self.link_preview_view
                                .observe_buffer(&self.link_preview_buffer);
                            let target_line = self
                                .link_preview_heading_fragment
                                .as_ref()
                                .map(|_| self.link_preview_buffer.cursor_line_col().0);
                            let (_, changed) = self.link_preview_view.render(
                                ui,
                                &mut self.link_preview_buffer,
                                self.wrap,
                                None,
                                self.line_number_mode,
                                true,
                                true,
                                target_line,
                                self.markdown_live_rendering,
                                None,
                            );
                            if self.focus_link_preview_once {
                                self.focus_link_preview_once = false;
                            }
                            if changed {
                                self.link_preview_dirty = true;
                            }
                        });
                    });
            });
    }

    #[allow(dead_code)]
    fn lance_dialog(&mut self, ctx: &egui::Context) {
        if !self.lance_open {
            return;
        }

        egui::Area::new("lance_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let screen = ctx.content_rect();
                let row_count = self.lance_file_indices().len().clamp(3, 10) as f32;
                let height = (132.0 + row_count * 24.0).clamp(210.0, 380.0);
                let size = Vec2::new(
                    (screen.width() * 0.52).clamp(420.0, 720.0),
                    height.min(screen.height() * 0.7),
                );
                egui::Frame::new()
                    .fill(Color32::from_rgb(22, 28, 37))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(12.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(170),
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(size);
                        ui.set_max_size(size);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(15.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let warn = Color32::from_rgb(235, 203, 139);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("lance").font(title_font).color(accent));
                            ui.label(
                                RichText::new(format!("{} marks", self.lance_files.len()))
                                    .font(font.clone())
                                    .color(dim),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(
                                            "↑↓ select  Enter open  Ctrl+Enter modal  Ctrl+A add  Del remove  Esc close",
                                        )
                                        .font(font.clone())
                                        .color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(">").font(font.clone()).color(accent));
                            let query = if self.lance_query.is_empty() {
                                "filter marks".to_string()
                            } else {
                                self.lance_query.clone()
                            };
                            ui.label(
                                RichText::new(query)
                                    .font(font.clone())
                                    .color(if self.lance_query.is_empty() { dim } else { Color32::from_rgb(216, 222, 233) }),
                            );
                        });
                        ui.add_space(8.0);
                        let indices = self.lance_file_indices();
                        let row_height = 24.0;
                        let visible_rows = ((ui.available_height() - 10.0) / row_height).floor().max(1.0) as usize;
                        let selected_position = indices
                            .iter()
                            .position(|index| *index == self.selected_lance_file)
                            .unwrap_or(0);
                        let start = file_dialogs::centered_window_start(selected_position, visible_rows, indices.len());
                        let end = (start + visible_rows).min(indices.len());
                        if indices.is_empty() {
                            ui.add_space(20.0);
                            ui.label(
                                RichText::new("No lance marks yet. Ctrl+A adds a file manually; :lance-add marks the current file.")
                                    .font(font.clone())
                                    .color(dim),
                            );
                        }
                        for index in indices[start..end].iter().copied() {
                            let Some(path) = self.lance_files.get(index) else {
                                continue;
                            };
                            let selected = index == self.selected_lance_file;
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), row_height),
                                egui::Sense::click(),
                            );
                            let painter = ui.painter_at(rect);
                            if selected {
                                painter.rect_filled(rect, 0.0, Color32::from_rgb(38, 47, 61));
                            }
                            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("unknown");
                            painter.text(
                                egui::pos2(rect.left() + 8.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                if selected { ">" } else { " " },
                                font.clone(),
                                accent,
                            );
                            painter.text(
                                egui::pos2(rect.left() + 28.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{} {}", index + 1, name),
                                font.clone(),
                                if selected { Color32::from_rgb(216, 222, 233) } else { Color32::from_rgb(190, 200, 216) },
                            );
                            painter.text(
                                egui::pos2(rect.left() + 220.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                path.display().to_string(),
                                font.clone(),
                                dim,
                            );
                            if response.clicked() {
                                self.selected_lance_file = index;
                            }
                        }
                    });
            });
    }

    fn slate_picker_dialog(&mut self, ctx: &egui::Context) {
        enum ActivePicker {
            File,
            Lance,
            Recent,
        }

        let active = if self.file_picker_open {
            ActivePicker::File
        } else if self.lance_open {
            ActivePicker::Lance
        } else if self.recent_picker_open {
            ActivePicker::Recent
        } else {
            return;
        };

        let file_matches = if matches!(active, ActivePicker::File) {
            self.project_file_indices()
        } else {
            Vec::new()
        };
        let lance_matches = if matches!(active, ActivePicker::Lance) {
            self.lance_file_indices()
        } else {
            Vec::new()
        };
        let recent_matches = if matches!(active, ActivePicker::Recent) {
            self.recent_file_indices()
        } else {
            Vec::new()
        };

        let (title, subtitle, prompt, query, selected_index, total_len, visible_rows) = match active
        {
            ActivePicker::File => {
                let title = match self.file_picker_mode {
                    FilePickerMode::Open => "open",
                    FilePickerMode::OpenBuffer => "open buffer",
                    FilePickerMode::AddLance => "add lance",
                    FilePickerMode::Browse => "files",
                    FilePickerMode::InsertMarkdownLink => "insert link file",
                    FilePickerMode::Refile => "refile target",
                    FilePickerMode::SetPeriscopeProject => "set periscope project",
                };
                (
                    title.to_string(),
                    format!(
                        "{} entries · {}",
                        self.project_files.len(),
                        self.file_picker_dir.display()
                    ),
                    "type to fuzzy-find files and folders".to_string(),
                    self.file_query.clone(),
                    file_matches
                        .iter()
                        .position(|index| *index == self.selected_project_file)
                        .unwrap_or(0),
                    file_matches.len(),
                    16usize,
                )
            }
            ActivePicker::Lance => (
                "lance".to_string(),
                format!("{} marks", self.lance_files.len()),
                "filter marks".to_string(),
                self.lance_query.clone(),
                lance_matches
                    .iter()
                    .position(|index| *index == self.selected_lance_file)
                    .unwrap_or(0),
                lance_matches.len(),
                10usize,
            ),
            ActivePicker::Recent => (
                "recent".to_string(),
                format!("{} files", self.recent_files.len()),
                "filter recent files".to_string(),
                self.recent_query.clone(),
                recent_matches
                    .iter()
                    .position(|index| *index == self.selected_recent_file)
                    .unwrap_or(0),
                recent_matches.len(),
                10usize,
            ),
        };

        let start = file_dialogs::centered_window_start(selected_index, visible_rows, total_len);
        let end = (start + visible_rows).min(total_len);

        egui::Area::new("slate_picker_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -16.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(match active {
                            ActivePicker::File => 820.0,
                            ActivePicker::Lance | ActivePicker::Recent => 760.0,
                        });
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(title).font(title_font).color(accent));
                            ui.label(RichText::new(subtitle).font(font.clone()).color(faint));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);

                        let input_height = 30.0;
                        let (input_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), input_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(input_rect);
                        painter.rect_filled(input_rect, 0.0, Color32::from_rgb(30, 36, 48));
                        painter.rect_stroke(
                            input_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let query_text = if query.is_empty() {
                            prompt
                        } else {
                            query.clone()
                        };
                        let query_color = if query.is_empty() { faint } else { text };
                        painter.text(
                            egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            ">",
                            font.clone(),
                            accent,
                        );
                        let query_rect = painter.text(
                            egui::pos2(input_rect.left() + 32.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            query_text,
                            font.clone(),
                            query_color,
                        );
                        let cursor_x = if query.is_empty() {
                            input_rect.left() + 32.0
                        } else {
                            query_rect.right() + 2.0
                        };
                        painter.line_segment(
                            [
                                egui::pos2(cursor_x, input_rect.top() + 7.0),
                                egui::pos2(cursor_x, input_rect.bottom() - 7.0),
                            ],
                            Stroke::new(1.0, accent),
                        );

                        ui.add_space(8.0);
                        let row_height = 24.0;
                        let list_height = (visible_rows.max(1) as f32 + 1.0) * row_height;
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(list_rect).with_clip_rect(list_rect);
                        painter.rect_filled(list_rect, 0.0, Color32::from_rgb(22, 28, 37));
                        painter.rect_stroke(
                            list_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let header_y = list_rect.top() + row_height * 0.5;
                        painter.text(
                            egui::pos2(list_rect.left() + 32.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "name",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 240.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "path / detail",
                            font.clone(),
                            faint,
                        );
                        if matches!(active, ActivePicker::File) {
                            painter.text(
                                egui::pos2(list_rect.right() - 180.0, header_y),
                                egui::Align2::LEFT_CENTER,
                                "size",
                                font.clone(),
                                faint,
                            );
                            painter.text(
                                egui::pos2(list_rect.right() - 92.0, header_y),
                                egui::Align2::LEFT_CENTER,
                                "modified",
                                font.clone(),
                                faint,
                            );
                        }

                        if total_len == 0 {
                            painter.text(
                                list_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "no matches",
                                font.clone(),
                                faint,
                            );
                        }

                        for (row, match_position) in (start..end).enumerate() {
                            let row_top = list_rect.top() + (row as f32 + 1.0) * row_height;
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(list_rect.left() + 4.0, row_top),
                                Vec2::new(list_rect.width() - 8.0, row_height),
                            );
                            let selected = match_position == selected_index;
                            if selected {
                                painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                            }

                            let (item_index, name, detail, size_label, modified_label, is_dir) =
                                match active {
                                    ActivePicker::File => {
                                        let index = file_matches[match_position];
                                        let path = &self.project_files[index];
                                        let name = path
                                            .file_name()
                                            .and_then(|name| name.to_str())
                                            .unwrap_or("unknown");
                                        let relative = path
                                            .strip_prefix(&self.file_picker_dir)
                                            .unwrap_or(path)
                                            .display()
                                            .to_string();
                                        let (size, modified) = if path.is_dir() {
                                            ("dir".to_string(), "".to_string())
                                        } else {
                                            file_dialogs::file_metadata_labels(path)
                                        };
                                        (
                                            index,
                                            name.to_string(),
                                            relative,
                                            size,
                                            modified,
                                            path.is_dir(),
                                        )
                                    }
                                    ActivePicker::Lance => {
                                        let index = lance_matches[match_position];
                                        let path = &self.lance_files[index];
                                        let name = path
                                            .file_name()
                                            .and_then(|name| name.to_str())
                                            .unwrap_or("unknown");
                                        (
                                            index,
                                            format!("{} {}", index + 1, name),
                                            path.display().to_string(),
                                            String::new(),
                                            String::new(),
                                            false,
                                        )
                                    }
                                    ActivePicker::Recent => {
                                        let index = recent_matches[match_position];
                                        let path = &self.recent_files[index];
                                        let name = path
                                            .file_name()
                                            .and_then(|name| name.to_str())
                                            .unwrap_or("unknown");
                                        (
                                            index,
                                            name.to_string(),
                                            path.display().to_string(),
                                            String::new(),
                                            String::new(),
                                            false,
                                        )
                                    }
                                };

                            let y = row_rect.center().y - 0.5;
                            painter.text(
                                egui::pos2(row_rect.left() + 8.0, y),
                                egui::Align2::LEFT_CENTER,
                                if selected { ">" } else { " " },
                                font.clone(),
                                accent,
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 28.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(
                                    &format!("{}{}", if is_dir { "▸ " } else { "" }, name),
                                    190.0,
                                    13.0,
                                ),
                                font.clone(),
                                if selected { text } else { accent },
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 236.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(&detail, list_rect.width() - 430.0, 13.0),
                                font.clone(),
                                if selected { dim } else { faint },
                            );
                            if matches!(active, ActivePicker::File) {
                                painter.text(
                                    egui::pos2(row_rect.right() - 176.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    size_label,
                                    font.clone(),
                                    if selected { dim } else { faint },
                                );
                                painter.text(
                                    egui::pos2(row_rect.right() - 88.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    modified_label,
                                    font.clone(),
                                    if selected { dim } else { faint },
                                );
                            }

                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("slate_picker", match_position)),
                                egui::Sense::click(),
                            );
                            if response.clicked() || response.double_clicked() {
                                match active {
                                    ActivePicker::File => self.selected_project_file = item_index,
                                    ActivePicker::Lance => self.selected_lance_file = item_index,
                                    ActivePicker::Recent => self.selected_recent_file = item_index,
                                }
                            }
                            if response.double_clicked() {
                                match active {
                                    ActivePicker::File => self.open_selected_project_file(),
                                    ActivePicker::Lance => self.open_selected_lance_file(),
                                    ActivePicker::Recent => self.open_selected_recent_file(),
                                }
                            }
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            let actions: &[(&str, &str)] = match active {
                                ActivePicker::File => &[
                                    ("↑↓", "select"),
                                    ("type", "filter"),
                                    ("→", "enter dir"),
                                    ("←", "parent"),
                                    ("enter", "open"),
                                    ("esc", "close"),
                                ],
                                ActivePicker::Lance => &[
                                    ("↑↓", "select"),
                                    ("type", "filter"),
                                    ("enter", "open"),
                                    ("Ctrl+Enter", "modal"),
                                    ("Ctrl+A", "add"),
                                    ("Del", "remove"),
                                ],
                                ActivePicker::Recent => &[
                                    ("↑↓", "select"),
                                    ("type", "filter"),
                                    ("enter", "open"),
                                    ("esc", "close"),
                                ],
                            };
                            for (key, label) in actions {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(*label).font(font.clone()).color(dim));
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn periscope_prompt_dialog(&mut self, ctx: &egui::Context) {
        if !self.periscope_prompt_open {
            return;
        }
        egui::Area::new("periscope_prompt_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -16.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.set_width(620.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let warn = Color32::from_rgb(235, 203, 139);
                        ui.label(RichText::new("Periscope").font(title_font).color(accent));
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "No .git project root found for the current file/context.",
                            )
                            .font(font.clone())
                            .color(text),
                        );
                        ui.label(
                            RichText::new(
                                "Define a project folder, or jump straight to Home search?",
                            )
                            .font(font.clone())
                            .color(dim),
                        );
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("[Y/Enter]").font(font.clone()).color(warn));
                            ui.label(
                                RichText::new("choose project folder")
                                    .font(font.clone())
                                    .color(dim),
                            );
                            ui.add_space(16.0);
                            ui.label(RichText::new("[N]").font(font.clone()).color(warn));
                            ui.label(
                                RichText::new("Home Periscope")
                                    .font(font.clone())
                                    .color(dim),
                            );
                            ui.add_space(16.0);
                            ui.label(RichText::new("[Esc]").font(font.clone()).color(warn));
                            ui.label(RichText::new("cancel").font(font.clone()).color(dim));
                        });
                    });
            });
    }

    fn periscope_dialog(&mut self, ctx: &egui::Context) {
        if !self.periscope_open {
            return;
        }
        self.poll_periscope_global_index();
        let rows = self.periscope_rows();
        let visible_rows = 16usize;
        let selected = self
            .selected_periscope_result
            .min(rows.len().saturating_sub(1));
        let start = file_dialogs::centered_window_start(selected, visible_rows, rows.len());
        let end = (start + visible_rows).min(rows.len());
        let mode_label = match self.periscope_mode {
            PeriscopeMode::Project => "project",
            PeriscopeMode::Global => "home",
        };
        let scope = match self.periscope_mode {
            PeriscopeMode::Project => self
                .periscope_project_root
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "no project".to_string()),
            PeriscopeMode::Global => std::env::var("HOME").unwrap_or_else(|_| "~".to_string()),
        };

        egui::Area::new("periscope_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -16.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(860.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);
                        let danger = Color32::from_rgb(191, 97, 106);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Periscope · {mode_label}"))
                                    .font(title_font)
                                    .color(accent),
                            );
                            ui.label(RichText::new(scope.clone()).font(font.clone()).color(faint));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        if !self.periscope_backend_status.is_empty() {
                            ui.label(
                                RichText::new(self.periscope_backend_status.clone())
                                    .font(font.clone())
                                    .color(faint),
                            );
                        }
                        ui.add_space(8.0);

                        let input_height = 30.0;
                        let (input_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), input_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(input_rect);
                        painter.rect_filled(input_rect, 0.0, Color32::from_rgb(30, 36, 48));
                        painter.rect_stroke(
                            input_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let query = if self.periscope_query.is_empty() {
                            "type to search files".to_string()
                        } else {
                            self.periscope_query.clone()
                        };
                        let query_color = if self.periscope_query.is_empty() {
                            faint
                        } else {
                            text
                        };
                        painter.text(
                            egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            ">",
                            font.clone(),
                            accent,
                        );
                        let query_rect = painter.text(
                            egui::pos2(input_rect.left() + 32.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            query,
                            font.clone(),
                            query_color,
                        );
                        let cursor_x = if self.periscope_query.is_empty() {
                            input_rect.left() + 32.0
                        } else {
                            query_rect.right() + 2.0
                        };
                        painter.line_segment(
                            [
                                egui::pos2(cursor_x, input_rect.top() + 7.0),
                                egui::pos2(cursor_x, input_rect.bottom() - 7.0),
                            ],
                            Stroke::new(1.0, accent),
                        );

                        ui.add_space(8.0);
                        let row_height = 24.0;
                        let list_height = (visible_rows as f32 + 1.0) * row_height;
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(list_rect).with_clip_rect(list_rect);
                        painter.rect_filled(list_rect, 0.0, Color32::from_rgb(22, 28, 37));
                        painter.rect_stroke(
                            list_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 32.0, list_rect.top() + row_height * 0.5),
                            egui::Align2::LEFT_CENTER,
                            "path",
                            font.clone(),
                            faint,
                        );

                        if rows.is_empty() {
                            let message = if self.periscope_mode == PeriscopeMode::Global
                                && self.periscope_global_loading
                            {
                                "loading home index…"
                            } else if self.periscope_mode == PeriscopeMode::Global
                                && self.periscope_query.trim().is_empty()
                            {
                                "type to search $HOME · capped at 100 results"
                            } else if self.periscope_query.trim().is_empty() {
                                "type a query to search this project"
                            } else {
                                "no matches"
                            };
                            painter.text(
                                list_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                message,
                                font.clone(),
                                faint,
                            );
                        }

                        for (row, index) in (start..end).enumerate() {
                            let row_top = list_rect.top() + (row as f32 + 1.0) * row_height;
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(list_rect.left() + 4.0, row_top),
                                Vec2::new(list_rect.width() - 8.0, row_height),
                            );
                            let selected = index == self.selected_periscope_result;
                            if selected {
                                painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                            }
                            let row = &rows[index];
                            let (path, path_text, is_folder) = match row {
                                PeriscopeRow::Folder { path, expanded } => {
                                    let mut label = format!("{}/", path.display());
                                    if *expanded {
                                        label.push_str("  expanded");
                                    }
                                    (path, label, true)
                                }
                                PeriscopeRow::File { path, group } => {
                                    let label = if let Some(group) = group {
                                        format!(
                                            "---/{}",
                                            path.strip_prefix(group).unwrap_or(path).display()
                                        )
                                    } else if self.periscope_mode == PeriscopeMode::Project {
                                        if let Some(root) = &self.periscope_project_root {
                                            path.strip_prefix(root)
                                                .unwrap_or(path)
                                                .display()
                                                .to_string()
                                        } else {
                                            path.display().to_string()
                                        }
                                    } else {
                                        path.display().to_string()
                                    };
                                    (path, label, false)
                                }
                            };
                            let full = path.display().to_string();
                            let sensitive = full.starts_with("/etc/")
                                || full.starts_with("/boot/")
                                || full.starts_with("/usr/")
                                || full.starts_with("/var/")
                                || full.contains("/.ssh/");
                            let y = row_rect.center().y - 0.5;
                            painter.text(
                                egui::pos2(row_rect.left() + 8.0, y),
                                egui::Align2::LEFT_CENTER,
                                if selected { ">" } else { " " },
                                font.clone(),
                                accent,
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 28.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(&path_text, list_rect.width() - 56.0, 13.0),
                                font.clone(),
                                if sensitive {
                                    danger
                                } else if is_folder {
                                    warn
                                } else if selected {
                                    text
                                } else {
                                    dim
                                },
                            );
                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("periscope", index)),
                                egui::Sense::click(),
                            );
                            if response.clicked() || response.double_clicked() {
                                self.selected_periscope_result = index;
                            }
                            if response.double_clicked() {
                                self.open_selected_periscope_result();
                            }
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [
                                ("↑↓", "select"),
                                ("type", "search"),
                                ("Tab", "project/home"),
                                ("→", "expand"),
                                ("Enter", "open/expand"),
                                ("Ctrl+Enter", "modal"),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(8.0);
                            }
                        });
                    });
            });
    }

    fn find_note_dialog(&mut self, ctx: &egui::Context) {
        if !self.find_note_open {
            return;
        }
        let root = self.project_root();
        let visible_rows = 16usize;
        let selected = self
            .selected_find_note_row
            .min(self.find_note_rows.len().saturating_sub(1));
        let start =
            file_dialogs::centered_window_start(selected, visible_rows, self.find_note_rows.len());
        let end = (start + visible_rows).min(self.find_note_rows.len());

        egui::Area::new("find_note_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -16.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(860.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Find note").font(title_font).color(accent));
                            ui.label(
                                RichText::new(root.display().to_string())
                                    .font(font.clone())
                                    .color(faint),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(8.0);

                        let input_height = 30.0;
                        let (input_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), input_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(input_rect);
                        painter.rect_filled(input_rect, 0.0, Color32::from_rgb(30, 36, 48));
                        painter.rect_stroke(
                            input_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let query = if self.find_note_query.is_empty() {
                            "type to filter notes".to_string()
                        } else {
                            self.find_note_query.clone()
                        };
                        let query_color = if self.find_note_query.is_empty() {
                            faint
                        } else {
                            text
                        };
                        painter.text(
                            egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            ">",
                            font.clone(),
                            accent,
                        );
                        let query_rect = painter.text(
                            egui::pos2(input_rect.left() + 32.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            query,
                            font.clone(),
                            query_color,
                        );
                        let cursor_x = if self.find_note_query.is_empty() {
                            input_rect.left() + 32.0
                        } else {
                            query_rect.right() + 2.0
                        };
                        painter.line_segment(
                            [
                                egui::pos2(cursor_x, input_rect.top() + 7.0),
                                egui::pos2(cursor_x, input_rect.bottom() - 7.0),
                            ],
                            Stroke::new(1.0, accent),
                        );

                        ui.add_space(8.0);
                        let row_height = 24.0;
                        let list_height = (visible_rows as f32 + 1.0) * row_height;
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(list_rect).with_clip_rect(list_rect);
                        painter.rect_filled(list_rect, 0.0, Color32::from_rgb(22, 28, 37));
                        painter.rect_stroke(
                            list_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 32.0, list_rect.top() + row_height * 0.5),
                            egui::Align2::LEFT_CENTER,
                            "note",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(
                                list_rect.right() - 260.0,
                                list_rect.top() + row_height * 0.5,
                            ),
                            egui::Align2::LEFT_CENTER,
                            "match",
                            font.clone(),
                            faint,
                        );

                        if self.find_note_rows.is_empty() {
                            painter.text(
                                list_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "no notes found",
                                font.clone(),
                                faint,
                            );
                        }

                        for (row, index) in (start..end).enumerate() {
                            let row_top = list_rect.top() + (row as f32 + 1.0) * row_height;
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(list_rect.left() + 4.0, row_top),
                                Vec2::new(list_rect.width() - 8.0, row_height),
                            );
                            let selected = index == self.selected_find_note_row;
                            if selected {
                                painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                            }
                            let (label, hint, has_headings, expanded, is_heading) = match &self
                                .find_note_rows[index]
                            {
                                FindNoteRow::File { path, match_hint } => {
                                    let label = path
                                        .strip_prefix(&root)
                                        .unwrap_or(path)
                                        .display()
                                        .to_string();
                                    let has_headings = fs::read_to_string(path)
                                        .map(|contents| {
                                            contents
                                                .lines()
                                                .any(|line| parse_heading_line(line).is_some())
                                        })
                                        .unwrap_or(false);
                                    (
                                        label,
                                        match_hint.clone(),
                                        has_headings,
                                        self.find_note_expanded_files.contains(path),
                                        false,
                                    )
                                }
                                FindNoteRow::Heading {
                                    line, title, level, ..
                                } => (
                                    format!("{}# {}", "  ".repeat(level.saturating_sub(1)), title),
                                    format!("line {line}"),
                                    false,
                                    false,
                                    true,
                                ),
                            };
                            let prefix = if is_heading {
                                "  "
                            } else if has_headings && expanded {
                                "▾ "
                            } else if has_headings {
                                "▸ "
                            } else {
                                "  "
                            };
                            let y = row_rect.center().y - 0.5;
                            painter.text(
                                egui::pos2(row_rect.left() + 8.0, y),
                                egui::Align2::LEFT_CENTER,
                                if selected { ">" } else { " " },
                                font.clone(),
                                accent,
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 28.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(
                                    &format!("{prefix}{label}"),
                                    list_rect.width() - 340.0,
                                    13.0,
                                ),
                                font.clone(),
                                if selected {
                                    text
                                } else if is_heading {
                                    dim
                                } else {
                                    accent
                                },
                            );
                            painter.text(
                                egui::pos2(list_rect.right() - 260.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(&hint, 245.0, 13.0),
                                font.clone(),
                                faint,
                            );
                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("find_note", index)),
                                egui::Sense::click(),
                            );
                            if response.clicked() || response.double_clicked() {
                                self.selected_find_note_row = index;
                            }
                            if response.double_clicked() {
                                self.open_selected_find_note(false);
                            }
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [
                                ("↑↓", "select"),
                                ("type", "filter"),
                                ("Tab/→", "expand"),
                                ("←", "collapse"),
                                ("Enter", "open main"),
                                ("Shift+Enter", "front modal"),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(8.0);
                            }
                        });
                    });
            });
    }

    #[allow(dead_code)]
    fn file_picker_dialog(&mut self, ctx: &egui::Context) {
        if !self.file_picker_open {
            return;
        }

        let root = self.file_picker_dir.clone();
        let matches = self.project_file_indices();
        let visible_rows = 16usize;
        let selected_position = matches
            .iter()
            .position(|index| *index == self.selected_project_file)
            .unwrap_or(0);
        let start =
            file_dialogs::centered_window_start(selected_position, visible_rows, matches.len());
        let end = (start + visible_rows).min(matches.len());

        egui::Area::new("file_picker_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(820.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);

                        ui.horizontal(|ui| {
                            let title = match self.file_picker_mode {
                                FilePickerMode::Open => "open",
                                FilePickerMode::OpenBuffer => "open buffer",
                                FilePickerMode::AddLance => "add lance",
                                FilePickerMode::Browse => "files",
                                FilePickerMode::InsertMarkdownLink => "insert link file",
                                FilePickerMode::Refile => "refile target",
                                FilePickerMode::SetPeriscopeProject => "set periscope project",
                            };
                            ui.label(RichText::new(title).font(title_font).color(accent));
                            ui.label(
                                RichText::new(format!(
                                    "{} files · {}",
                                    self.project_files.len(),
                                    root.display()
                                ))
                                .font(font.clone())
                                .color(faint),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);

                        let input_height = 30.0;
                        let (input_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), input_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(input_rect);
                        painter.rect_filled(input_rect, 0.0, Color32::from_rgb(30, 36, 48));
                        painter.rect_stroke(
                            input_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let query = if self.file_query.is_empty() {
                            "type to fuzzy-find files and folders".to_string()
                        } else {
                            self.file_query.clone()
                        };
                        let query_color = if self.file_query.is_empty() {
                            faint
                        } else {
                            text
                        };
                        painter.text(
                            egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            "files: ",
                            font.clone(),
                            accent,
                        );
                        let query_rect = painter.text(
                            egui::pos2(input_rect.left() + 66.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            query,
                            font.clone(),
                            query_color,
                        );
                        let cursor_x = if self.file_query.is_empty() {
                            input_rect.left() + 66.0
                        } else {
                            query_rect.right() + 2.0
                        };
                        painter.line_segment(
                            [
                                egui::pos2(cursor_x, input_rect.top() + 7.0),
                                egui::pos2(cursor_x, input_rect.bottom() - 7.0),
                            ],
                            Stroke::new(1.0, accent),
                        );

                        ui.add_space(8.0);
                        let row_height = 24.0;
                        let list_height = (visible_rows.max(1) as f32 + 1.0) * row_height;
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(list_rect).with_clip_rect(list_rect);
                        painter.rect_filled(list_rect, 0.0, Color32::from_rgb(22, 28, 37));
                        painter.rect_stroke(
                            list_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let header_y = list_rect.top() + row_height * 0.5;
                        painter.text(
                            egui::pos2(list_rect.left() + 32.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "name",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 240.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "path",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.right() - 180.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "size",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.right() - 92.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "modified",
                            font.clone(),
                            faint,
                        );

                        if matches.is_empty() {
                            painter.text(
                                list_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "no matching files",
                                font.clone(),
                                faint,
                            );
                        }

                        for (row, index) in matches[start..end].iter().copied().enumerate() {
                            let row_top = list_rect.top() + (row as f32 + 1.0) * row_height;
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(list_rect.left() + 4.0, row_top),
                                Vec2::new(list_rect.width() - 8.0, row_height),
                            );
                            let selected = index == self.selected_project_file;
                            if selected {
                                painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                            }
                            let path = &self.project_files[index];
                            let name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("unknown");
                            let relative = path
                                .strip_prefix(&root)
                                .unwrap_or(path)
                                .display()
                                .to_string();
                            let (size_label, modified_label) = if path.is_dir() {
                                ("dir".to_string(), "".to_string())
                            } else {
                                file_dialogs::file_metadata_labels(path)
                            };
                            let y = row_rect.center().y - 0.5;
                            painter.text(
                                egui::pos2(row_rect.left() + 8.0, y),
                                egui::Align2::LEFT_CENTER,
                                if selected { ">" } else { " " },
                                font.clone(),
                                accent,
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 28.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(
                                    &format!("{}{}", if path.is_dir() { "▸ " } else { "  " }, name),
                                    190.0,
                                    13.0,
                                ),
                                font.clone(),
                                if selected { text } else { accent },
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 236.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(&relative, list_rect.width() - 430.0, 13.0),
                                font.clone(),
                                if selected { dim } else { faint },
                            );
                            painter.text(
                                egui::pos2(row_rect.right() - 176.0, y),
                                egui::Align2::LEFT_CENTER,
                                size_label,
                                font.clone(),
                                if selected { dim } else { faint },
                            );
                            painter.text(
                                egui::pos2(row_rect.right() - 88.0, y),
                                egui::Align2::LEFT_CENTER,
                                modified_label,
                                font.clone(),
                                if selected { dim } else { faint },
                            );

                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("project_file", index)),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_project_file = index;
                            }
                            if response.double_clicked() {
                                self.selected_project_file = index;
                                self.open_selected_project_file();
                            }
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [
                                ("↑↓", "select"),
                                ("type", "filter"),
                                ("→", "enter dir"),
                                ("←", "parent"),
                                ("enter", "open"),
                                ("esc", "close"),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn scratch_modal_dialog(&mut self, ctx: &egui::Context) {
        if !self.scratch_modal_open {
            return;
        }

        egui::Area::new("scratch_modal_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(760.0);
                        ui.set_height(430.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("scratch").font(title_font).color(accent));
                            ui.label(
                                RichText::new(
                                    "quick capture · archives to ~/.local/share/slate/scratch.md",
                                )
                                .font(font.clone())
                                .color(faint),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] hide").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);

                        let editor_height = 330.0;
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), editor_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                let (response, changed) = self.scratch_view.render(
                                    ui,
                                    &mut self.scratch_buffer,
                                    self.wrap,
                                    None,
                                    self.line_number_mode,
                                    true,
                                    true,
                                    None,
                                    false,
                                    None,
                                );
                                response.request_focus();
                                if changed {
                                    self.status = "Scratch capture modified".to_string();
                                }
                            },
                        );

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [
                                ("Ctrl+S", "archive"),
                                ("Esc", "hide"),
                                (":scratch", "resume"),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn capture_dialog(&mut self, ctx: &egui::Context) {
        if !self.capture_modal_open {
            return;
        }

        egui::Area::new("capture_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(720.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let text = Color32::from_rgb(216, 222, 233);
                        let warn = Color32::from_rgb(235, 203, 139);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("capture").font(title_font).color(accent));
                            ui.label(
                                RichText::new("selection/current line → scratch")
                                    .font(font.clone())
                                    .color(faint),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] cancel")
                                            .font(font.clone())
                                            .color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);

                        ui.label(
                            RichText::new("title/context optional")
                                .font(font.clone())
                                .color(faint),
                        );
                        let title_input_id = ui.make_persistent_id("capture_title_input");
                        let response = ui.add(
                            TextEdit::singleline(&mut self.capture_title)
                                .id_salt(title_input_id)
                                .hint_text("press Enter empty for no title")
                                .desired_width(f32::INFINITY)
                                .font(font.clone())
                                .text_color(text)
                                .frame(egui::Frame::NONE),
                        );
                        if self.capture_title_focus_once {
                            response.request_focus();
                            ui.memory_mut(|memory| memory.request_focus(response.id));
                            ctx.request_repaint();
                            self.capture_title_focus_once = false;
                        }

                        ui.add_space(10.0);
                        ui.label(RichText::new("capturing").font(font.clone()).color(faint));
                        let preview_height = 220.0;
                        let (preview_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), preview_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(preview_rect);
                        painter.rect_filled(preview_rect, 0.0, Color32::from_rgb(30, 36, 48));
                        painter.rect_stroke(
                            preview_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(59, 69, 89)),
                            egui::StrokeKind::Inside,
                        );
                        let inner = preview_rect.shrink(10.0);
                        let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
                        let clip = painter.with_clip_rect(inner);
                        let mut y = inner.top();
                        for line in self.capture_text.lines() {
                            if y + line_height > inner.bottom() {
                                break;
                            }
                            clip.text(
                                egui::pos2(inner.left(), y),
                                egui::Align2::LEFT_TOP,
                                line,
                                font.clone(),
                                text,
                            );
                            y += line_height;
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [("Enter", "archive"), ("Esc", "cancel")] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn scratch_entries_dialog(&mut self, ctx: &egui::Context) {
        if !self.scratch_entries_open {
            return;
        }

        egui::Area::new("scratch_entries_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(820.0);
                        ui.set_height(470.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let text = Color32::from_rgb(216, 222, 233);
                        let warn = Color32::from_rgb(235, 203, 139);
                        let danger = Color32::from_rgb(191, 97, 106);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("scratch entries")
                                    .font(title_font)
                                    .color(accent),
                            );
                            ui.label(
                                RichText::new(format!("{} entries", self.scratch_entries.len()))
                                    .font(font.clone())
                                    .color(faint),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);

                        if self.scratch_entries.is_empty() {
                            ui.label(
                                RichText::new("Scratch archive is empty.")
                                    .font(font.clone())
                                    .color(dim),
                            );
                        } else {
                            let row_height = 44.0;
                            let visible_rows = 8usize;
                            let selected = self
                                .selected_scratch_entry
                                .min(self.scratch_entries.len().saturating_sub(1));
                            let start = file_dialogs::centered_window_start(
                                selected,
                                visible_rows,
                                self.scratch_entries.len(),
                            );
                            let end = (start + visible_rows).min(self.scratch_entries.len());
                            let list_height = row_height * visible_rows as f32;
                            let total_width = ui.available_width();
                            let column_gap = 8.0;
                            let left_width = (total_width * 0.5 - column_gap).min(420.0);
                            let right_width = (total_width - left_width - column_gap).max(120.0);
                            let label_height = ui.fonts_mut(|fonts| fonts.row_height(&font));

                            let (label_rect, _) = ui.allocate_exact_size(
                                Vec2::new(total_width, label_height),
                                egui::Sense::hover(),
                            );
                            let label_painter = ui.painter_at(label_rect);
                            let left_label_pos =
                                egui::pos2(label_rect.left(), label_rect.center().y);
                            let right_label_pos = egui::pos2(
                                label_rect.left() + left_width + column_gap,
                                label_rect.center().y,
                            );
                            label_painter.text(
                                left_label_pos,
                                egui::Align2::LEFT_CENTER,
                                "entries",
                                font.clone(),
                                faint,
                            );
                            label_painter.text(
                                right_label_pos,
                                egui::Align2::LEFT_CENTER,
                                "preview",
                                font.clone(),
                                faint,
                            );
                            ui.add_space(6.0);

                            let (body_rect, _) = ui.allocate_exact_size(
                                Vec2::new(total_width, list_height),
                                egui::Sense::hover(),
                            );
                            let list_rect = egui::Rect::from_min_size(
                                body_rect.left_top(),
                                Vec2::new(left_width, list_height),
                            );
                            let preview_rect = egui::Rect::from_min_size(
                                egui::pos2(list_rect.right() + column_gap, body_rect.top()),
                                Vec2::new(right_width, list_height),
                            );

                            let list_painter = ui.painter_at(list_rect);
                            for (visible_row, index) in (start..end).enumerate() {
                                let entry = &self.scratch_entries[index];
                                let selected = index == self.selected_scratch_entry;
                                let rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        list_rect.left(),
                                        list_rect.top() + visible_row as f32 * row_height,
                                    ),
                                    Vec2::new(list_rect.width(), row_height),
                                );
                                let response = ui.interact(
                                    rect,
                                    ui.id().with(("scratch_entry", index)),
                                    egui::Sense::click(),
                                );
                                if selected {
                                    list_painter.rect_filled(
                                        rect,
                                        0.0,
                                        Color32::from_rgb(38, 47, 61),
                                    );
                                }
                                list_painter.text(
                                    egui::pos2(rect.left() + 8.0, rect.top() + 12.0),
                                    egui::Align2::LEFT_CENTER,
                                    if selected { ">" } else { " " },
                                    font.clone(),
                                    accent,
                                );
                                list_painter.text(
                                    egui::pos2(rect.left() + 26.0, rect.top() + 12.0),
                                    egui::Align2::LEFT_CENTER,
                                    entry.title(),
                                    font.clone(),
                                    if selected { text } else { dim },
                                );
                                list_painter.text(
                                    egui::pos2(rect.left() + 26.0, rect.top() + 31.0),
                                    egui::Align2::LEFT_CENTER,
                                    entry.preview(),
                                    font.clone(),
                                    faint,
                                );
                                if response.clicked() {
                                    self.selected_scratch_entry = index;
                                }
                            }

                            let selected_entry = self
                                .scratch_entries
                                .get(self.selected_scratch_entry)
                                .or_else(|| self.scratch_entries.first());
                            if let Some(entry) = selected_entry {
                                let painter = ui.painter_at(preview_rect);
                                painter.rect_filled(
                                    preview_rect,
                                    0.0,
                                    Color32::from_rgb(30, 36, 48),
                                );
                                painter.rect_stroke(
                                    preview_rect,
                                    0.0,
                                    Stroke::new(1.0, Color32::from_rgb(59, 69, 89)),
                                    egui::StrokeKind::Inside,
                                );

                                let inner_rect = preview_rect.shrink(10.0);
                                let preview_painter =
                                    ui.painter_at(preview_rect).with_clip_rect(inner_rect);
                                let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
                                preview_painter.text(
                                    inner_rect.left_top(),
                                    egui::Align2::LEFT_TOP,
                                    entry.title(),
                                    font.clone(),
                                    accent,
                                );
                                let mut body_y = inner_rect.top() + line_height + 12.0;
                                for line in entry.body.lines() {
                                    if body_y + line_height > inner_rect.bottom() {
                                        break;
                                    }
                                    preview_painter.text(
                                        egui::pos2(inner_rect.left(), body_y),
                                        egui::Align2::LEFT_TOP,
                                        line,
                                        font.clone(),
                                        text,
                                    );
                                    body_y += line_height;
                                }
                            }
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label, color) in [
                                ("↑↓", "select", warn),
                                ("Ctrl+D", "delete entry", danger),
                                ("Esc", "close", warn),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(color),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn save_as_dialog(&mut self, ctx: &egui::Context) {
        if !self.save_as_open {
            return;
        }

        let matches = self.save_as_entry_indices();
        let visible_rows = 12usize;
        let selected_position = matches
            .iter()
            .position(|index| *index == self.selected_save_as_entry)
            .unwrap_or(0);
        let start =
            file_dialogs::centered_window_start(selected_position, visible_rows, matches.len());
        let end = (start + visible_rows).min(matches.len());

        egui::Area::new("save_as_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(820.0);
                        let font = FontId::new(13.0, FontFamily::Monospace);
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(136, 192, 208);
                        let text = Color32::from_rgb(216, 222, 233);
                        let dim = Color32::from_rgb(136, 154, 176);
                        let faint = Color32::from_rgb(94, 105, 126);
                        let warn = Color32::from_rgb(235, 203, 139);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("save as").font(title_font).color(accent));
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} entries",
                                    self.save_as_dir.display(),
                                    self.save_as_entries.len()
                                ))
                                .font(font.clone())
                                .color(faint),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close").font(font.clone()).color(warn),
                                    );
                                },
                            );
                        });
                        ui.add_space(10.0);

                        let input_height = 30.0;
                        let (input_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), input_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(input_rect);
                        painter.rect_filled(input_rect, 0.0, Color32::from_rgb(30, 36, 48));
                        painter.rect_stroke(
                            input_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let filename = if self.save_as_filename.is_empty() {
                            "type file name".to_string()
                        } else {
                            self.save_as_filename.clone()
                        };
                        let filename_color = if self.save_as_filename.is_empty() {
                            faint
                        } else {
                            text
                        };
                        painter.text(
                            egui::pos2(input_rect.left() + 10.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            "save as: ",
                            font.clone(),
                            accent,
                        );
                        let filename_rect = painter.text(
                            egui::pos2(input_rect.left() + 90.0, input_rect.center().y - 0.5),
                            egui::Align2::LEFT_CENTER,
                            filename,
                            font.clone(),
                            filename_color,
                        );
                        let cursor_x = if self.save_as_filename.is_empty() {
                            input_rect.left() + 90.0
                        } else {
                            filename_rect.right() + 2.0
                        };
                        painter.line_segment(
                            [
                                egui::pos2(cursor_x, input_rect.top() + 7.0),
                                egui::pos2(cursor_x, input_rect.bottom() - 7.0),
                            ],
                            Stroke::new(1.0, accent),
                        );

                        ui.add_space(8.0);
                        let row_height = 24.0;
                        let list_height = (visible_rows.max(1) as f32 + 1.0) * row_height;
                        let (list_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), list_height),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(list_rect).with_clip_rect(list_rect);
                        painter.rect_filled(list_rect, 0.0, Color32::from_rgb(22, 28, 37));
                        painter.rect_stroke(
                            list_rect,
                            0.0,
                            Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                            egui::StrokeKind::Outside,
                        );
                        let header_y = list_rect.top() + row_height * 0.5;
                        painter.text(
                            egui::pos2(list_rect.left() + 32.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "name",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 300.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "size",
                            font.clone(),
                            faint,
                        );
                        painter.text(
                            egui::pos2(list_rect.left() + 390.0, header_y),
                            egui::Align2::LEFT_CENTER,
                            "modified",
                            font.clone(),
                            faint,
                        );

                        if matches.is_empty() {
                            painter.text(
                                list_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "no matching entries",
                                font.clone(),
                                faint,
                            );
                        }

                        for (row, index) in matches[start..end].iter().copied().enumerate() {
                            let row_top = list_rect.top() + (row as f32 + 1.0) * row_height;
                            let row_rect = egui::Rect::from_min_size(
                                egui::pos2(list_rect.left() + 4.0, row_top),
                                Vec2::new(list_rect.width() - 8.0, row_height),
                            );
                            let selected = index == self.selected_save_as_entry;
                            if selected {
                                painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                            }
                            let path = &self.save_as_entries[index];
                            let name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("unknown");
                            let (size_label, modified_label) = if path.is_dir() {
                                ("dir".to_string(), "".to_string())
                            } else {
                                file_dialogs::file_metadata_labels(path)
                            };
                            let y = row_rect.center().y - 0.5;
                            painter.text(
                                egui::pos2(row_rect.left() + 8.0, y),
                                egui::Align2::LEFT_CENTER,
                                if selected { ">" } else { " " },
                                font.clone(),
                                accent,
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 28.0, y),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(
                                    &format!("{}{}", if path.is_dir() { "▸ " } else { "  " }, name),
                                    250.0,
                                    13.0,
                                ),
                                font.clone(),
                                if selected { text } else { accent },
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 300.0, y),
                                egui::Align2::LEFT_CENTER,
                                size_label,
                                font.clone(),
                                if selected { dim } else { faint },
                            );
                            painter.text(
                                egui::pos2(row_rect.left() + 390.0, y),
                                egui::Align2::LEFT_CENTER,
                                modified_label,
                                font.clone(),
                                if selected { dim } else { faint },
                            );

                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("save_as_entry", index)),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_save_as_entry = index;
                                if path.is_file() {
                                    self.save_as_filename = name.to_string();
                                }
                            }
                            if response.double_clicked() {
                                self.selected_save_as_entry = index;
                                if path.is_dir() {
                                    self.save_as_enter_selected_dir();
                                } else {
                                    self.save_as_filename = name.to_string();
                                    self.confirm_save_as();
                                }
                            }
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            for (key, label) in [
                                ("↑↓", "select"),
                                ("type", "name/filter"),
                                ("→", "enter dir"),
                                ("←", "parent"),
                                ("enter", "save"),
                                ("esc", "close"),
                            ] {
                                ui.label(
                                    RichText::new(format!("[{key}]"))
                                        .font(font.clone())
                                        .color(warn),
                                );
                                ui.label(RichText::new(label).font(font.clone()).color(dim));
                                ui.add_space(10.0);
                            }
                        });
                    });
            });
    }

    fn shortcut_help_dialog(&mut self, ctx: &egui::Context) {
        if !self.shortcut_help_open {
            return;
        }

        let shortcut_groups: &[(&str, &[(&str, &str)])] = &[
            (
                "global",
                &[
                    ("Ctrl+.", "open commandline"),
                    ("Ctrl+P", "open command palette/browser"),
                    ("Ctrl+H", "open this help"),
                    ("Esc", "close modal / cancel active mode"),
                    ("Ctrl+S", "save"),
                    ("Ctrl+Alt+S", "save as"),
                    ("Ctrl+O", "open file"),
                    ("Ctrl+M", "toggle Markdown preview"),
                    ("Ctrl+Q", "quit"),
                ],
            ),
            (
                "ctrl layer",
                &[
                    ("Ctrl: s", "save"),
                    ("Ctrl: o", "open file"),
                    ("Ctrl: ol", "open last file"),
                    ("Ctrl: r", "recent files"),
                    ("Ctrl: f/b", "find next / previous"),
                    ("Ctrl: sw/sl", "select word / line"),
                    ("Ctrl: dw/dl", "delete word / line"),
                    ("Ctrl: dup", "duplicate line"),
                    ("Ctrl: dupp", "duplicate and place"),
                    ("Ctrl: gt/gb", "go top / bottom"),
                ],
            ),
            (
                "movement layers",
                &[
                    ("Ctrl+Shift", "live cursor movement"),
                    ("CapsLock", "system nav layer: ijkl arrows"),
                    ("Alt up/down", "move line/block"),
                    ("Alt double", "move to paragraph edge"),
                    ("Alt left/right", "word selection"),
                    ("Shift+Alt", "paragraph / word / line-edge jumps"),
                    ("duplicate mode", "move · Enter/Space place · Esc cancel"),
                ],
            ),
            (
                "commandline",
                &[
                    ("Tab", "accept completion"),
                    ("↑/↓", "history / picker selection"),
                    ("Enter", "run command / accept picker item"),
                    (":recent query", "filter recent files"),
                    (":find text", "find in file"),
                    (":g 10 / :g +5", "absolute / relative goto"),
                ],
            ),
        ];

        let viewport_width = ctx.input(|i| {
            i.viewport()
                .inner_rect
                .map(|rect| rect.width())
                .unwrap_or_else(|| i.content_rect().width())
        });
        // This is the frame *content* width. The frame itself adds margin/stroke,
        // so keep extra room for tiled/WM-small windows instead of filling edge-to-edge.
        let modal_width = (viewport_width - 80.0).clamp(420.0, 1320.0);
        let shortcut_width = (modal_width * 0.32).clamp(300.0, 380.0);
        let gutter_width = 18.0;
        let command_width = (modal_width - shortcut_width - gutter_width - 30.0).max(260.0);
        let key_x = (shortcut_width * 0.28).clamp(84.0, 112.0);
        let arrow_x = key_x + 22.0;
        let desc_x = arrow_x + 24.0;
        let compact_help = modal_width < 900.0;
        let command_table_width = (command_width - 44.0).max(240.0);
        let command_col = if compact_help {
            (command_table_width * 0.72).max(160.0)
        } else {
            (command_table_width * 0.30).clamp(150.0, 240.0)
        };
        let aliases_col = if compact_help {
            0.0
        } else {
            (command_table_width * 0.17).clamp(90.0, 140.0)
        };
        let summary_col = if compact_help {
            0.0
        } else {
            (command_table_width * 0.37).clamp(220.0, 330.0)
        };
        let hint_col = if compact_help {
            (command_table_width - command_col - 10.0).max(110.0)
        } else {
            (command_table_width * 0.16).clamp(110.0, 150.0)
        };

        egui::Area::new("shortcut_help_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(76, 86, 106)))
                    .corner_radius(0.0)
                    .inner_margin(14.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(modal_width);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("shortcuts + commands")
                                    .font(FontId::new(16.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(136, 192, 208)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("[esc] close")
                                            .font(FontId::new(13.0, FontFamily::Monospace))
                                            .color(Color32::from_rgb(235, 203, 139)),
                                    );
                                },
                            );
                        });
                        ui.add_space(8.0);

                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(shortcut_width);
                                for (title, entries) in shortcut_groups {
                                    ui.label(
                                        RichText::new(*title)
                                            .font(FontId::new(14.0, FontFamily::Monospace))
                                            .color(Color32::from_rgb(163, 190, 140)),
                                    );
                                    ui.add_space(3.0);
                                    for (key, desc) in *entries {
                                        let (row_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(shortcut_width, 15.0),
                                            egui::Sense::hover(),
                                        );
                                        let painter = ui.painter_at(row_rect);
                                        let y = row_rect.center().y - 0.5;
                                        painter.text(
                                            egui::pos2(row_rect.left() + key_x, y),
                                            egui::Align2::RIGHT_CENTER,
                                            *key,
                                            FontId::new(12.5, FontFamily::Monospace),
                                            Color32::from_rgb(235, 203, 139),
                                        );
                                        painter.text(
                                            egui::pos2(row_rect.left() + arrow_x, y),
                                            egui::Align2::CENTER_CENTER,
                                            "→",
                                            FontId::new(12.5, FontFamily::Monospace),
                                            Color32::from_rgb(94, 105, 126),
                                        );
                                        painter.text(
                                            egui::pos2(row_rect.left() + desc_x, y),
                                            egui::Align2::LEFT_CENTER,
                                            Self::text_for_width(
                                                Self::compact_shortcut_desc(desc),
                                                (shortcut_width - desc_x - 4.0).max(24.0),
                                                12.5,
                                            ),
                                            FontId::new(12.5, FontFamily::Monospace),
                                            Color32::from_rgb(216, 222, 233),
                                        );
                                    }
                                    ui.add_space(7.0);
                                }
                            });

                            ui.add_space(gutter_width);
                            ui.vertical(|ui| {
                                ui.set_width(command_width);
                                ui.label(
                                    RichText::new("available commands")
                                        .font(FontId::new(14.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(163, 190, 140)),
                                );
                                ui.add_space(3.0);
                                let header_height = 16.0;
                                let row_height = 15.0;
                                let table_height =
                                    header_height + COMMAND_SPECS.len() as f32 * row_height + 16.0;
                                let (table_rect, _) = ui.allocate_exact_size(
                                    Vec2::new(command_width, table_height),
                                    egui::Sense::hover(),
                                );
                                let painter = ui.painter_at(table_rect).with_clip_rect(table_rect);
                                painter.rect_filled(table_rect, 0.0, Color32::from_rgb(22, 28, 37));
                                painter.rect_stroke(
                                    table_rect,
                                    0.0,
                                    Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
                                    egui::StrokeKind::Outside,
                                );

                                let left = table_rect.left() + 8.0;
                                let top = table_rect.top() + 8.0;
                                let text_y_offset = 0.5;
                                if compact_help {
                                    let cols = [command_col, hint_col];
                                    let col_lefts = [left, left + command_col + 10.0];
                                    for (idx, header) in ["command", "hint"].iter().enumerate() {
                                        painter.text(
                                            egui::pos2(
                                                col_lefts[idx] + cols[idx] * 0.5,
                                                top + header_height * 0.5,
                                            ),
                                            egui::Align2::CENTER_CENTER,
                                            *header,
                                            FontId::new(11.5, FontFamily::Monospace),
                                            Color32::from_rgb(136, 154, 176),
                                        );
                                    }

                                    for (row, spec) in COMMAND_SPECS.iter().enumerate() {
                                        let row_top = top + header_height + row as f32 * row_height;
                                        let row_rect = egui::Rect::from_min_size(
                                            egui::pos2(table_rect.left() + 4.0, row_top),
                                            Vec2::new(command_width - 8.0, row_height),
                                        );
                                        if row % 2 == 0 {
                                            painter.rect_filled(
                                                row_rect,
                                                0.0,
                                                Color32::from_rgb(38, 47, 61),
                                            );
                                        }
                                        let y = row_rect.center().y - text_y_offset;
                                        let command = Self::text_for_width(
                                            &format!(":{}", spec.name),
                                            command_col - 6.0,
                                            12.5,
                                        );
                                        let hint =
                                            Self::text_for_width(spec.hint, hint_col - 6.0, 11.5);
                                        for (idx, (text, color, size)) in [
                                            (command, Color32::from_rgb(136, 192, 208), 12.5),
                                            (hint, Color32::from_rgb(235, 203, 139), 11.5),
                                        ]
                                        .into_iter()
                                        .enumerate()
                                        {
                                            painter.text(
                                                egui::pos2(col_lefts[idx] + cols[idx] * 0.5, y),
                                                egui::Align2::CENTER_CENTER,
                                                text,
                                                FontId::new(size, FontFamily::Monospace),
                                                color,
                                            );
                                        }
                                    }
                                } else {
                                    let cols = [command_col, aliases_col, summary_col, hint_col];
                                    let mut col_lefts = [left, left, left, left];
                                    for idx in 1..cols.len() {
                                        col_lefts[idx] = col_lefts[idx - 1] + cols[idx - 1] + 10.0;
                                    }
                                    for (idx, header) in
                                        ["command", "aliases", "summary", "hint"].iter().enumerate()
                                    {
                                        painter.text(
                                            egui::pos2(
                                                col_lefts[idx] + cols[idx] * 0.5,
                                                top + header_height * 0.5,
                                            ),
                                            egui::Align2::CENTER_CENTER,
                                            *header,
                                            FontId::new(11.5, FontFamily::Monospace),
                                            Color32::from_rgb(136, 154, 176),
                                        );
                                    }

                                    for (row, spec) in COMMAND_SPECS.iter().enumerate() {
                                        let row_top = top + header_height + row as f32 * row_height;
                                        let row_rect = egui::Rect::from_min_size(
                                            egui::pos2(table_rect.left() + 4.0, row_top),
                                            Vec2::new(command_width - 8.0, row_height),
                                        );
                                        if row % 2 == 0 {
                                            painter.rect_filled(
                                                row_rect,
                                                0.0,
                                                Color32::from_rgb(38, 47, 61),
                                            );
                                        }
                                        let y = row_rect.center().y - text_y_offset;
                                        let aliases = if spec.aliases.is_empty() {
                                            "".to_string()
                                        } else {
                                            spec.aliases.join(", ")
                                        };
                                        let command = Self::text_for_width(
                                            &format!(":{}", spec.name),
                                            command_col - 6.0,
                                            12.5,
                                        );
                                        let aliases =
                                            Self::text_for_width(&aliases, aliases_col - 6.0, 11.5);
                                        let summary = Self::text_for_width(
                                            spec.summary,
                                            summary_col - 6.0,
                                            11.5,
                                        );
                                        let hint =
                                            Self::text_for_width(spec.hint, hint_col - 6.0, 11.5);
                                        for (idx, (text, color, size)) in [
                                            (command, Color32::from_rgb(136, 192, 208), 12.5),
                                            (aliases, Color32::from_rgb(94, 105, 126), 11.5),
                                            (summary, Color32::from_rgb(136, 154, 176), 11.5),
                                            (hint, Color32::from_rgb(235, 203, 139), 11.5),
                                        ]
                                        .into_iter()
                                        .enumerate()
                                        {
                                            painter.text(
                                                egui::pos2(col_lefts[idx] + cols[idx] * 0.5, y),
                                                egui::Align2::CENTER_CENTER,
                                                text,
                                                FontId::new(size, FontFamily::Monospace),
                                                color,
                                            );
                                        }
                                    }
                                }
                            });
                        });
                    });
            });
    }

    fn checkbox_preview_icon(ui: &mut egui::Ui, state: CheckboxState) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 13.0), egui::Sense::hover());
        let (fill, stroke) = match state {
            CheckboxState::Empty => (
                Color32::from_rgb(30, 36, 48),
                Color32::from_rgb(136, 154, 176),
            ),
            CheckboxState::Doing => (
                Color32::from_rgb(59, 66, 82),
                Color32::from_rgb(235, 203, 139),
            ),
            CheckboxState::Done => (
                Color32::from_rgb(49, 70, 60),
                Color32::from_rgb(163, 190, 140),
            ),
        };
        ui.painter().rect_filled(rect, 2.0, fill);
        ui.painter().rect_stroke(
            rect,
            2.0,
            Stroke::new(1.2, stroke),
            egui::StrokeKind::Outside,
        );
        let glyph_rect = rect.shrink(3.2);
        match state {
            CheckboxState::Empty => {}
            CheckboxState::Doing => {
                ui.painter().line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5, stroke),
                );
            }
            CheckboxState::Done => {
                ui.painter().line_segment(
                    [glyph_rect.left_top(), glyph_rect.right_bottom()],
                    Stroke::new(1.5, stroke),
                );
                ui.painter().line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5, stroke),
                );
            }
        }
    }

    fn markdown_link_preview_label(ui: &mut egui::Ui, line: &str) -> bool {
        let spans = parse_markdown_link_spans(line);
        if spans.is_empty() {
            return false;
        }

        let font = FontId::new(15.0, FontFamily::Monospace);
        let mut sections = Vec::new();
        let mut byte = 0;
        for span in spans {
            let fragment_start = line[span.target_start..span.target_end]
                .find('#')
                .map(|offset| span.target_start + offset);
            let target_path_end = fragment_start.unwrap_or(span.target_end);
            let link_dim = Color32::from_rgb(94, 105, 126);
            let link_text = Color32::from_rgb(136, 192, 208);
            let link_target = Color32::from_rgb(163, 190, 140);
            let link_heading = Color32::from_rgb(235, 203, 139);
            let label_line_start = line[span.text_start..span.text_end]
                .rfind(':')
                .map(|offset| span.text_start + offset)
                .filter(|line_start| {
                    line[line_start + 1..span.text_end]
                        .chars()
                        .all(|ch| ch.is_ascii_digit())
                });
            let mut link_sections = vec![
                (
                    byte,
                    span.marker_start,
                    Color32::from_rgb(216, 222, 233),
                    false,
                ),
                (span.marker_start, span.text_start, link_dim, false),
            ];
            if let Some(line_start) = label_line_start {
                link_sections.push((span.text_start, line_start, link_target, true));
                link_sections.push((line_start, span.text_end, link_heading, true));
            } else {
                link_sections.push((span.text_start, span.text_end, link_text, true));
            }
            link_sections.extend([
                (span.text_end, span.target_start, link_dim, false),
                (span.target_start, target_path_end, link_target, false),
            ]);
            if let Some(fragment_start) = fragment_start {
                link_sections.push((fragment_start, span.target_end, link_heading, false));
            }
            link_sections.push((span.target_end, span.marker_end, link_dim, false));

            for (start, end, color, underline) in link_sections {
                if start < end {
                    let mut format = TextFormat::simple(font.clone(), color);
                    if underline {
                        format.underline = Stroke::new(1.0, color);
                    }
                    sections.push(LayoutSection {
                        leading_space: 0.0,
                        byte_range: start..end,
                        format,
                    });
                }
            }
            byte = span.marker_end;
        }
        if byte < line.len() {
            sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: byte..line.len(),
                format: TextFormat::simple(font, Color32::from_rgb(216, 222, 233)),
            });
        }

        ui.label(LayoutJob {
            text: line.to_string(),
            sections,
            break_on_newline: false,
            ..Default::default()
        });
        true
    }

    fn markdown_table_preview(
        ui: &mut egui::Ui,
        rows: &[Vec<String>],
        alignments: &[TableAlignment],
    ) {
        if rows.is_empty() {
            return;
        }

        let column_count = rows[0].len();
        let char_width = 8.4;
        let mut column_widths = vec![72.0_f32; column_count];
        for row in rows {
            for column in 0..column_count {
                let len = row
                    .get(column)
                    .map(|text| text.chars().count())
                    .unwrap_or(0);
                column_widths[column] = column_widths[column].max(len as f32 * char_width + 28.0);
            }
        }

        let row_height = 26.0;
        let table_width = column_widths.iter().sum::<f32>();
        let table_height = row_height * rows.len() as f32;
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(table_width.min(ui.available_width()), table_height),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        let border = Color32::from_rgb(59, 70, 90);
        let strong_border = Color32::from_rgb(76, 86, 106);
        let font = FontId::new(14.0, FontFamily::Monospace);

        painter.rect_filled(rect, 3.0, Color32::from_rgb(24, 30, 40));
        for (row_index, row) in rows.iter().enumerate() {
            let y = rect.top() + row_index as f32 * row_height;
            let row_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), y),
                egui::pos2(rect.right(), y + row_height),
            );
            let row_fill = if row_index == 0 {
                Color32::from_rgb(31, 38, 50)
            } else if row_index % 2 == 0 {
                Color32::from_rgb(28, 35, 46)
            } else {
                Color32::from_rgb(24, 30, 40)
            };
            painter.rect_filled(row_rect, 0.0, row_fill);

            let mut x = rect.left();
            for column in 0..column_count {
                let width = column_widths[column];
                let text = row.get(column).map(String::as_str).unwrap_or("");
                let alignment = alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableAlignment::Left);
                let color = if row_index == 0 {
                    Color32::from_rgb(235, 203, 139)
                } else {
                    Color32::from_rgb(216, 222, 233)
                };
                let (pos, align) = match alignment {
                    TableAlignment::Left => (
                        egui::pos2(x + 10.0, y + row_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                    ),
                    TableAlignment::Center => (
                        egui::pos2(x + width * 0.5, y + row_height * 0.5),
                        egui::Align2::CENTER_CENTER,
                    ),
                    TableAlignment::Right => (
                        egui::pos2(x + width - 10.0, y + row_height * 0.5),
                        egui::Align2::RIGHT_CENTER,
                    ),
                };
                painter.text(pos, align, text, font.clone(), color);
                x += width;
            }
        }

        painter.rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        let mut x = rect.left();
        for width in column_widths.iter().take(column_count.saturating_sub(1)) {
            x += *width;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_rgb(46, 56, 72)),
            );
        }
        if rows.len() > 1 {
            let header_y = rect.top() + row_height;
            painter.line_segment(
                [
                    egui::pos2(rect.left(), header_y),
                    egui::pos2(rect.right(), header_y),
                ],
                Stroke::new(1.0, strong_border),
            );
        }
        for row_index in 2..rows.len() {
            let y = rect.top() + row_index as f32 * row_height;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0, Color32::from_rgb(36, 45, 58)),
            );
        }
        ui.add_space(6.0);
    }

    fn inline_code_preview_label(ui: &mut egui::Ui, line: &str) -> bool {
        let spans = parse_inline_code_spans(line);
        if spans.is_empty() {
            return false;
        }

        let font = FontId::new(15.0, FontFamily::Monospace);
        let mut sections = Vec::new();
        let mut byte = 0;
        for span in spans {
            for (start, end, color, background) in [
                (
                    byte,
                    span.marker_start,
                    Color32::from_rgb(216, 222, 233),
                    Color32::TRANSPARENT,
                ),
                (
                    span.marker_start,
                    span.code_start,
                    Color32::from_rgb(94, 105, 126),
                    Color32::from_rgb(31, 38, 50),
                ),
                (
                    span.code_start,
                    span.code_end,
                    Color32::from_rgb(235, 203, 139),
                    Color32::from_rgb(31, 38, 50),
                ),
                (
                    span.code_end,
                    span.marker_end,
                    Color32::from_rgb(94, 105, 126),
                    Color32::from_rgb(31, 38, 50),
                ),
            ] {
                if start < end {
                    let mut format = TextFormat::simple(font.clone(), color);
                    format.background = background;
                    format.expand_bg = 2.0;
                    sections.push(LayoutSection {
                        leading_space: 0.0,
                        byte_range: start..end,
                        format,
                    });
                }
            }
            byte = span.marker_end;
        }
        if byte < line.len() {
            sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: byte..line.len(),
                format: TextFormat::simple(font, Color32::from_rgb(216, 222, 233)),
            });
        }

        ui.label(LayoutJob {
            text: line.to_string(),
            sections,
            break_on_newline: false,
            ..Default::default()
        });
        true
    }

    fn preview_ui(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let lines = self.buffer.as_str().lines().collect::<Vec<_>>();
            let mut index = 0;
            let mut in_code = false;
            let mut code_language = String::new();
            while index < lines.len() {
                let line = lines[index];
                let trimmed = line.trim_start();
                if let Some(fence) = parse_fenced_code_marker(line) {
                    if in_code {
                        in_code = false;
                        code_language.clear();
                    } else {
                        in_code = true;
                        code_language = fence.language.to_string();
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("code")
                                    .font(FontId::new(12.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(136, 154, 176)),
                            );
                            if !code_language.is_empty() {
                                ui.label(
                                    RichText::new(&code_language)
                                        .font(FontId::new(12.0, FontFamily::Monospace))
                                        .color(Color32::from_rgb(235, 203, 139)),
                                );
                            }
                        });
                    }
                    index += 1;
                    continue;
                }

                if in_code {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(25, 31, 40))
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(line)
                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(216, 222, 233)),
                            );
                        });
                } else if is_markdown_table_start(line, lines.get(index + 1).copied()) {
                    let mut rows = vec![split_markdown_table_row(line).unwrap_or_default()];
                    let alignments = parse_markdown_table_separator(lines[index + 1])
                        .unwrap_or_else(|| vec![TableAlignment::Left; rows[0].len()]);
                    index += 2;
                    while let Some(next_line) = lines.get(index).copied() {
                        let Some(cells) = split_markdown_table_row(next_line) else {
                            break;
                        };
                        if cells.len() != rows[0].len() {
                            break;
                        }
                        rows.push(cells);
                        index += 1;
                    }
                    Self::markdown_table_preview(ui, &rows, &alignments);
                    continue;
                } else if let Some(heading) = parse_heading_line(line) {
                    let size = match heading.level {
                        1 => 28.0,
                        2 => 22.0,
                        3 => 18.0,
                        _ => 16.0,
                    };
                    let color = match heading.level {
                        1 => Color32::from_rgb(235, 203, 139),
                        2 => Color32::from_rgb(180, 142, 173),
                        3 => Color32::from_rgb(136, 192, 208),
                        _ => Color32::from_rgb(216, 222, 233),
                    };
                    ui.add_space(if heading.level <= 2 { 4.0 } else { 2.0 });
                    ui.label(RichText::new(heading.text).size(size).strong().color(color));
                    if heading.level <= 2 {
                        ui.separator();
                    }
                } else if is_markdown_separator(line) {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                } else if let Some(blockquote) = parse_blockquote_line(line) {
                    ui.horizontal(|ui| {
                        ui.add_space((blockquote.depth.saturating_sub(1) as f32) * 8.0);
                        let (bar_rect, _) =
                            ui.allocate_exact_size(Vec2::new(3.0, 20.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(bar_rect, 1.5, Color32::from_rgb(136, 192, 208));
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(blockquote.text)
                                .size(15.0)
                                .color(Color32::from_rgb(190, 200, 216)),
                        );
                    });
                } else if let Some(checkbox) = parse_checkbox_line(line) {
                    ui.horizontal(|ui| {
                        if !checkbox.indent.is_empty() {
                            ui.label(
                                RichText::new(checkbox.indent)
                                    .font(FontId::new(15.0, FontFamily::Monospace)),
                            );
                        }
                        if !checkbox.task_prefix.is_empty() {
                            ui.label(
                                RichText::new(checkbox.task_prefix)
                                    .font(FontId::new(15.0, FontFamily::Monospace))
                                    .color(Color32::from_rgb(136, 154, 176)),
                            );
                        }
                        Self::checkbox_preview_icon(ui, checkbox.state);
                        ui.label(RichText::new(checkbox.text).size(15.0));
                    });
                } else if let Some(list) = parse_list_line(line) {
                    ui.horizontal(|ui| {
                        if !list.indent.is_empty() {
                            ui.label(
                                RichText::new(list.indent)
                                    .font(FontId::new(15.0, FontFamily::Monospace)),
                            );
                        }
                        let marker = if list.ordered { list.marker } else { "•" };
                        ui.label(
                            RichText::new(marker)
                                .font(FontId::new(15.0, FontFamily::Monospace))
                                .color(if list.ordered {
                                    Color32::from_rgb(235, 203, 139)
                                } else {
                                    Color32::from_rgb(136, 192, 208)
                                }),
                        );
                        ui.label(RichText::new(list.text).size(15.0));
                    });
                } else if trimmed.is_empty() {
                    ui.add_space(8.0);
                } else if !Self::markdown_link_preview_label(ui, line)
                    && !Self::inline_code_preview_label(ui, line)
                {
                    ui.label(RichText::new(line).size(15.0));
                }
                index += 1;
            }
        });
    }
}

impl SlateApp {
    fn link_inline_suggestion_popup(&mut self, ctx: &egui::Context) {
        if self.link_inline_anchor.is_none() {
            return;
        }
        let Some(query) = self.current_link_inline_query() else {
            return;
        };
        let suggestions = self.link_inline_suggestions();
        if suggestions.is_empty() {
            return;
        }
        let root = self.project_root();
        let row_font = FontId::new(13.0, FontFamily::Monospace);
        let small_font = FontId::new(11.0, FontFamily::Monospace);
        let text = Color32::from_rgb(218, 224, 235);
        let muted = Color32::from_rgb(126, 136, 157);
        let accent = Color32::from_rgb(163, 190, 140);
        let path_color = Color32::from_rgb(136, 192, 208);
        let screen = ctx.content_rect();
        let popup_width = (screen.width() * 0.34).clamp(260.0, 460.0);
        let row_count = suggestions.len().min(8) as f32;
        let popup_height = 36.0 + row_count * 24.0;
        let cursor_pos = self
            .editor_view
            .last_cursor_pos()
            .unwrap_or_else(|| egui::pos2(screen.left() + 44.0, screen.bottom() - 54.0));
        let mut popup_pos = cursor_pos + Vec2::new(10.0, 8.0);
        if popup_pos.x + popup_width > screen.right() - 12.0 {
            popup_pos.x = (screen.right() - popup_width - 12.0).max(screen.left() + 12.0);
        }
        if popup_pos.y + popup_height > screen.bottom() - 34.0 {
            popup_pos.y = (cursor_pos.y - popup_height - 8.0).max(screen.top() + 12.0);
        }
        egui::Area::new("link_inline_suggestions".into())
            .fixed_pos(popup_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 17, 24))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(53, 66, 86)))
                    .corner_radius(7.0)
                    .inner_margin(egui::Margin::symmetric(7, 6))
                    .show(ui, |ui| {
                        ui.set_width(popup_width);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("link").font(small_font.clone()).color(muted));
                            ui.label(
                                RichText::new(format!("[{query}"))
                                    .font(small_font.clone())
                                    .color(accent),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("↑↓ · Enter file · Tab headings")
                                            .font(small_font.clone())
                                            .color(muted),
                                    );
                                },
                            );
                        });
                        ui.add_space(4.0);
                        for (index, path) in suggestions.iter().take(8).enumerate() {
                            let selected = index == self.selected_link_inline_suggestion;
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 24.0),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_link_inline_suggestion = index;
                                self.confirm_link_inline_autocomplete();
                                break;
                            }
                            let painter = ui.painter_at(rect);
                            if selected {
                                painter.rect_filled(rect, 4.0, Color32::from_rgb(42, 56, 76));
                            }
                            let name = path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("untitled");
                            let relative = path
                                .strip_prefix(&root)
                                .unwrap_or(path)
                                .display()
                                .to_string();
                            let has_headings = Self::link_target_has_headings(path);
                            painter.text(
                                egui::pos2(rect.left() + 7.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                if has_headings { "▸" } else { " " },
                                row_font.clone(),
                                if has_headings { accent } else { muted },
                            );
                            painter.text(
                                egui::pos2(rect.left() + 24.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                name,
                                row_font.clone(),
                                if selected { text } else { path_color },
                            );
                            painter.text(
                                egui::pos2(rect.left() + 190.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                relative,
                                small_font.clone(),
                                muted,
                            );
                        }
                    });
            });
    }

    fn flow_inline_org_suggestion_popup(&mut self, ctx: &egui::Context) {
        let Some(active_mode) = self.flow_inline_org_mode else {
            return;
        };
        let Some((mode, query)) = self.current_flow_inline_org_token() else {
            return;
        };
        if mode != active_mode
            || self.current_flow_inline_org_token_global_start(mode) != self.flow_inline_org_anchor
        {
            return;
        }
        let suggestions = self.flow_inline_org_suggestions();
        if suggestions.is_empty() {
            return;
        }
        let prefix = match mode {
            AgendaOrgMode::Tag => "#",
            AgendaOrgMode::Project => "@",
        };
        let title = match mode {
            AgendaOrgMode::Tag => "tag",
            AgendaOrgMode::Project => "project",
        };
        let row_font = FontId::new(13.0, FontFamily::Monospace);
        let small_font = FontId::new(11.0, FontFamily::Monospace);
        let text = Color32::from_rgb(218, 224, 235);
        let muted = Color32::from_rgb(126, 136, 157);
        let accent = Color32::from_rgb(154, 185, 235);
        let screen = ctx.content_rect();
        let popup_width = (screen.width() * 0.28).clamp(220.0, 340.0);
        let row_count = suggestions.len().min(7) as f32;
        let popup_height = 36.0 + row_count * 22.0;
        let cursor_pos = self
            .editor_view
            .last_cursor_pos()
            .unwrap_or_else(|| egui::pos2(screen.left() + 44.0, screen.bottom() - 54.0));
        let mut popup_pos = cursor_pos + Vec2::new(10.0, 8.0);
        if popup_pos.x + popup_width > screen.right() - 12.0 {
            popup_pos.x = (screen.right() - popup_width - 12.0).max(screen.left() + 12.0);
        }
        if popup_pos.y + popup_height > screen.bottom() - 34.0 {
            popup_pos.y = (cursor_pos.y - popup_height - 8.0).max(screen.top() + 12.0);
        }
        egui::Area::new("flow_inline_org_suggestions".into())
            .fixed_pos(popup_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 17, 24))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(53, 66, 86)))
                    .corner_radius(7.0)
                    .inner_margin(egui::Margin::symmetric(7, 6))
                    .show(ui, |ui| {
                        ui.set_width(popup_width);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(title).font(small_font.clone()).color(muted));
                            ui.label(
                                RichText::new(format!("{prefix}{query}"))
                                    .font(small_font.clone())
                                    .color(accent),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new("↑↓/Tab · Enter")
                                            .font(small_font.clone())
                                            .color(muted),
                                    );
                                },
                            );
                        });
                        ui.add_space(4.0);
                        for (index, suggestion) in suggestions.iter().take(7).enumerate() {
                            let selected = index == self.selected_flow_inline_org_suggestion;
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 22.0),
                                egui::Sense::click(),
                            );
                            if response.clicked() {
                                self.selected_flow_inline_org_suggestion = index;
                                self.confirm_flow_inline_org_autocomplete(ctx);
                                break;
                            }
                            let painter = ui.painter_at(rect);
                            if selected {
                                painter.rect_filled(rect, 4.0, Color32::from_rgb(42, 56, 76));
                            }
                            painter.text(
                                egui::pos2(rect.left() + 7.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!("{prefix}{suggestion}"),
                                row_font.clone(),
                                if selected { text } else { muted },
                            );
                        }
                    });
            });
    }

    fn flow_item_edit_dialog(&mut self, ctx: &egui::Context) {
        if self.flow_item_edit.is_none() {
            return;
        }

        let mut save = false;
        let mut cancel = false;
        let mut open_org = None;
        let mut confirm_org = false;
        let mut org_next = false;
        let mut org_previous = false;
        let mut org_backspace = false;
        let mut org_typed = String::new();

        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::CTRL, Key::Enter) {
                save = true;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Escape) {
                if self
                    .flow_item_edit
                    .as_ref()
                    .is_some_and(|edit| edit.org_mode.is_some())
                {
                    if let Some(edit) = self.flow_item_edit.as_mut() {
                        edit.close_org();
                    }
                } else {
                    cancel = true;
                }
            }
            let org_open = self
                .flow_item_edit
                .as_ref()
                .is_some_and(|edit| edit.org_mode.is_some());
            if org_open && input.consume_key(egui::Modifiers::NONE, Key::Tab) {
                org_next = true;
            }
            if org_open && input.consume_key(egui::Modifiers::SHIFT, Key::Tab) {
                org_previous = true;
            }
            if org_open
                && (input.consume_key(egui::Modifiers::NONE, Key::Enter)
                    || input.consume_key(egui::Modifiers::NONE, Key::Space))
            {
                confirm_org = true;
            }
            if org_open && input.consume_key(egui::Modifiers::NONE, Key::Backspace) {
                org_backspace = true;
            }
            for event in &input.events {
                if let egui::Event::Text(text) = event {
                    if text == "#" {
                        open_org = Some(AgendaOrgMode::Tag);
                    } else if text == "@" {
                        open_org = Some(AgendaOrgMode::Project);
                    } else if self
                        .flow_item_edit
                        .as_ref()
                        .is_some_and(|edit| edit.org_mode.is_some())
                    {
                        org_typed.push_str(text);
                    }
                }
            }
        });

        let all_values = |mode: AgendaOrgMode, items: &[AgendaItem]| -> Vec<String> {
            items
                .iter()
                .flat_map(move |item| match mode {
                    AgendaOrgMode::Tag => item.tags.clone(),
                    AgendaOrgMode::Project => item.projects.clone(),
                })
                .collect()
        };

        if let Some(edit) = self.flow_item_edit.as_mut() {
            if let Some(mode) = open_org {
                edit.open_org(mode);
            }
            if let Some(mode) = edit.org_mode {
                if org_backspace {
                    edit.org_query.pop();
                    edit.selected_org_suggestion = 0;
                }
                if !org_typed.is_empty() {
                    for ch in org_typed.chars() {
                        if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.') {
                            edit.org_query.push(ch);
                        }
                    }
                    edit.selected_org_suggestion = 0;
                }
                let suggestions =
                    organization_suggestions(all_values(mode, &self.agenda_items), &edit.org_query);
                let len = suggestions.len().max(1);
                if org_next {
                    edit.selected_org_suggestion = (edit.selected_org_suggestion + 1) % len;
                }
                if org_previous {
                    edit.selected_org_suggestion = if edit.selected_org_suggestion == 0 {
                        len.saturating_sub(1)
                    } else {
                        edit.selected_org_suggestion.saturating_sub(1)
                    };
                }
                edit.selected_org_suggestion =
                    edit.selected_org_suggestion.min(len.saturating_sub(1));
                if confirm_org {
                    let value = suggestions
                        .get(edit.selected_org_suggestion)
                        .cloned()
                        .unwrap_or_else(|| edit.org_query.trim().to_string());
                    if let Some(token) = organization_token(mode, &value) {
                        edit.cursor_char_index = replace_org_token_at_cursor(
                            &mut edit.text,
                            edit.cursor_char_index,
                            mode,
                            &token,
                        );
                    }
                    edit.close_org();
                }
            }
        }

        let screen_rect = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            "flow_item_edit_scrim".into(),
        ))
        .rect_filled(screen_rect, 0.0, Color32::from_black_alpha(90));

        let mut commit_payload = None;
        egui::Area::new("flow_item_edit_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 17, 24))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(105, 117, 148)))
                    .corner_radius(9.0)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_min_width(720.0);
                        let row_font = FontId::new(14.0, FontFamily::Monospace);
                        let small_font = FontId::new(12.0, FontFamily::Monospace);
                        let title_font = FontId::new(15.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(154, 185, 235);
                        let accent_soft = Color32::from_rgb(42, 56, 76);
                        let text = Color32::from_rgb(218, 224, 235);
                        let muted = Color32::from_rgb(126, 136, 157);
                        let panel = Color32::from_rgb(17, 22, 31);
                        let field = Color32::from_rgb(10, 15, 22);
                        let line = Color32::from_rgb(43, 53, 68);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Flow item")
                                    .font(title_font.clone())
                                    .color(text),
                            );
                            ui.add_space(8.0);
                            egui::Frame::new()
                                .fill(Color32::from_rgb(24, 30, 42))
                                .corner_radius(999.0)
                                .inner_margin(egui::Margin::symmetric(7, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("Ctrl+Enter save")
                                            .font(small_font.clone())
                                            .color(accent),
                                    );
                                });
                            ui.label(
                                RichText::new("Esc cancel · #/@ autocomplete")
                                    .font(small_font.clone())
                                    .color(muted),
                            );
                        });
                        ui.add_space(10.0);
                        if let Some(edit) = self.flow_item_edit.as_mut() {
                            let output = egui::Frame::new()
                                .fill(field)
                                .stroke(Stroke::new(1.0, line))
                                .corner_radius(5.0)
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .show(ui, |ui| {
                                    TextEdit::singleline(&mut edit.text)
                                        .font(row_font.clone())
                                        .desired_width(ui.available_width())
                                        .show(ui)
                                })
                                .inner;
                            if let Some(cursor) =
                                output.cursor_range.and_then(|range| range.single())
                            {
                                edit.cursor_char_index = cursor.index;
                            }
                            if edit.focus_once {
                                output.response.request_focus();
                                edit.focus_once = false;
                            }
                            if let Some(mode) = edit.org_mode {
                                if let Some(query) =
                                    org_query_at_cursor(&edit.text, edit.cursor_char_index, mode)
                                {
                                    edit.org_query = query;
                                }
                            }
                            if let Some(mode) = edit.org_mode {
                                ui.add_space(9.0);
                                let prefix = match mode {
                                    AgendaOrgMode::Tag => "#",
                                    AgendaOrgMode::Project => "@",
                                };
                                let mode_label = match mode {
                                    AgendaOrgMode::Tag => "tags",
                                    AgendaOrgMode::Project => "projects",
                                };
                                let suggestions = organization_suggestions(
                                    self.agenda_items.iter().flat_map(|item| match mode {
                                        AgendaOrgMode::Tag => item.tags.clone(),
                                        AgendaOrgMode::Project => item.projects.clone(),
                                    }),
                                    &edit.org_query,
                                );
                                egui::Frame::new()
                                    .fill(panel)
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(29, 37, 50)))
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::symmetric(9, 7))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(mode_label)
                                                    .font(small_font.clone())
                                                    .color(muted),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "{prefix}{}",
                                                    edit.org_query
                                                ))
                                                .font(row_font.clone())
                                                .color(accent),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(
                                                            "Tab cycle · Enter add · Esc close",
                                                        )
                                                        .font(small_font.clone())
                                                        .color(muted),
                                                    );
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);
                                        ui.horizontal_wrapped(|ui| {
                                            for (index, suggestion) in
                                                suggestions.iter().take(8).enumerate()
                                            {
                                                let selected =
                                                    index == edit.selected_org_suggestion;
                                                egui::Frame::new()
                                                    .fill(if selected {
                                                        accent_soft
                                                    } else {
                                                        Color32::from_rgb(23, 29, 39)
                                                    })
                                                    .stroke(Stroke::new(
                                                        1.0,
                                                        if selected {
                                                            Color32::from_rgb(91, 124, 170)
                                                        } else {
                                                            Color32::from_rgb(28, 35, 47)
                                                        },
                                                    ))
                                                    .corner_radius(999.0)
                                                    .inner_margin(egui::Margin::symmetric(8, 3))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "{prefix}{suggestion}"
                                                            ))
                                                            .font(small_font.clone())
                                                            .color(if selected {
                                                                text
                                                            } else {
                                                                muted
                                                            }),
                                                        );
                                                    });
                                            }
                                        });
                                    });
                            }
                            if save {
                                commit_payload = Some((edit.item_index, edit.text.clone()));
                            }
                        }
                    });
            });

        if cancel {
            self.flow_item_edit = None;
            self.status = "Flow item edit cancelled".to_string();
        }
        if let Some((index, replacement)) = commit_payload {
            self.confirm_flow_item_edit(index, replacement, ctx);
        }
    }

    fn agenda_dialog(&mut self, ctx: &egui::Context) {
        if !self.agenda_open {
            return;
        }

        let today = Local::now().date_naive();
        let week_start = self.agenda_week_start();
        let week_end = week_start + Duration::days(6);
        let title = format!("week-agenda (W{}):", week_start.iso_week().week());
        let mut clicked_dated_row = None;
        let mut clicked_inbox_row = None;
        let mut clicked_flat_row = None;
        let mut clicked_task_index = None;
        let mut open_clicked = false;
        let mut toggle_clicked_day = None;
        let mut org_anchor_rect = None;
        let scroll_to_selected = self.agenda_scroll_to_selected;

        egui::Area::new("agenda_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let rect = ui.ctx().content_rect();
                let (width, height) = if self.standalone_agenda {
                    (
                        (rect.width() * 0.96).clamp(620.0, 1200.0),
                        (rect.height() * 0.94).clamp(420.0, 900.0),
                    )
                } else {
                    (
                        (rect.width() * 0.78).clamp(560.0, 920.0),
                        (rect.height() * 0.82).clamp(420.0, 760.0),
                    )
                };
                egui::Frame::new()
                    .fill(Color32::from_rgb(19, 23, 31))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(67, 76, 94)))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_min_size(Vec2::new(width, height));
                        ui.set_max_size(Vec2::new(width, height));
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let row_font = FontId::new(14.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(180, 160, 235);
                        let dim = Color32::from_rgb(118, 112, 150);
                        let text = Color32::from_rgb(216, 222, 233);
                        let muted = Color32::from_rgb(143, 151, 170);
                        let today_bg = Color32::from_rgb(38, 47, 61);
                        let selected_bg = Color32::from_rgb(49, 61, 79);
                        let inactive_bg = Color32::from_rgb(27, 32, 42);
                        let separator = Color32::from_rgb(49, 56, 72);

                        ui.horizontal(|ui| {
                            for tab in AgendaTab::ALL {
                                let active = self.agenda_tab == tab;
                                let label = format!(" {} ", tab.label());
                                let tab_text = RichText::new(label)
                                    .font(row_font.clone())
                                    .color(if active { text } else { muted });
                                let response = egui::Frame::new()
                                    .fill(if active { selected_bg } else { inactive_bg })
                                    .stroke(Stroke::new(
                                        1.0,
                                        if active {
                                            accent
                                        } else {
                                            Color32::from_rgb(37, 44, 58)
                                        },
                                    ))
                                    .corner_radius(4.0)
                                    .inner_margin(egui::Margin::symmetric(8, 3))
                                    .show(ui, |ui| ui.label(tab_text))
                                    .response;
                                let _ = response;
                                ui.add_space(4.0);
                            }
                            ui.label(
                                RichText::new("Shift+Tab cambia vista")
                                    .font(row_font.clone())
                                    .color(dim),
                            );
                        });
                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(title).font(title_font).color(text));
                            ui.label(
                                RichText::new(format!(
                                    "{}–{}",
                                    week_start.format("%-d %B"),
                                    week_end.format("%-d %B %Y")
                                ))
                                .font(row_font.clone())
                                .color(dim),
                            );
                            ui.label(
                                RichText::new(match self.agenda_focus {
                                    AgendaFocus::Dated => "focus: dated week",
                                    AgendaFocus::Inbox => "focus: no-date",
                                })
                                .font(row_font.clone())
                                .color(accent),
                            );
                            ui.label(
                                RichText::new(if self.agenda_show_done {
                                    "done: shown"
                                } else {
                                    "done: hidden"
                                })
                                .font(row_font.clone())
                                .color(muted),
                            );
                            if !self.agenda_filter.is_empty() {
                                ui.label(
                                    RichText::new(format!("filter: {}", self.agenda_filter))
                                        .font(row_font.clone())
                                        .color(accent),
                                );
                            }
                        });
                        ui.add_space(8.0);

                        if self.agenda_tab == AgendaTab::Agenda {
                            let dated_rows = self.agenda_dated_rows();
                            let dated_panel_height = (height * 0.42).clamp(160.0, 310.0);
                            let dated_title_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), 22.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            let dated_active = self.agenda_focus == AgendaFocus::Dated;
                            ui.painter().rect_filled(
                                dated_title_rect,
                                0.0,
                                if dated_active { today_bg } else { inactive_bg },
                            );
                            ui.painter().text(
                                egui::pos2(
                                    dated_title_rect.left() + 6.0,
                                    dated_title_rect.center().y,
                                ),
                                egui::Align2::LEFT_CENTER,
                                "Dated tasks · ↑↓ row · Enter expand/open · ←→ week",
                                row_font.clone(),
                                if dated_active { text } else { muted },
                            );

                            egui::ScrollArea::vertical()
                                .id_salt("agenda_dated_scroll")
                                .max_height(dated_panel_height)
                                .show(ui, |ui| {
                                    for (row_index, row) in dated_rows.iter().copied().enumerate() {
                                        let selected = self.agenda_focus == AgendaFocus::Dated
                                            && self.agenda_selected_dated_row == row_index;
                                        match row {
                                            AgendaDatedRow::Day(date) => {
                                                let count = self.agenda_items_for_date(date).len();
                                                let response = agenda::day_row(
                                                    ui,
                                                    row_index,
                                                    date,
                                                    count,
                                                    self.agenda_expanded_dates.contains(&date),
                                                    date == today,
                                                    selected,
                                                    scroll_to_selected && selected,
                                                    selected_bg,
                                                    today_bg,
                                                    row_font.clone(),
                                                    text,
                                                    muted,
                                                    accent,
                                                );
                                                if response.clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    open_clicked = false;
                                                }
                                                if response.double_clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    toggle_clicked_day = Some(date);
                                                }
                                            }
                                            AgendaDatedRow::Task(index) => {
                                                let response = agenda::task_row(
                                                    ui,
                                                    index,
                                                    selected,
                                                    scroll_to_selected && selected,
                                                    selected_bg,
                                                    row_font.clone(),
                                                    text,
                                                    accent,
                                                    &self.agenda_items[index],
                                                );
                                                if selected && self.agenda_org_mode.is_some() {
                                                    org_anchor_rect = Some(response.rect);
                                                }
                                                if response.clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    clicked_task_index = Some(index);
                                                    open_clicked = false;
                                                }
                                                if response.double_clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    clicked_task_index = Some(index);
                                                    open_clicked = true;
                                                }
                                            }
                                        }
                                    }
                                });

                            ui.add_space(8.0);
                            let (rule_rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 1.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(rule_rect, 0.0, separator);
                            ui.add_space(8.0);

                            let inbox_indices = self.agenda_inbox_item_indices();
                            let inbox_title_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), 22.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            let inbox_active = self.agenda_focus == AgendaFocus::Inbox;
                            ui.painter().rect_filled(
                                inbox_title_rect,
                                0.0,
                                if inbox_active { today_bg } else { inactive_bg },
                            );
                            ui.painter().text(
                            egui::pos2(inbox_title_rect.left() + 6.0, inbox_title_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!(
                                "No-date tasks · {} item{} · Tab switches segment · x toggles done",
                                inbox_indices.len(),
                                if inbox_indices.len() == 1 { "" } else { "s" }
                            ),
                            row_font.clone(),
                            if inbox_active { text } else { muted },
                        );

                            let inbox_height = (ui.available_height() - 4.0).max(80.0);
                            egui::ScrollArea::vertical()
                                .id_salt("agenda_inbox_scroll")
                                .max_height(inbox_height)
                                .show(ui, |ui| {
                                    if inbox_indices.is_empty() {
                                        ui.add_space(14.0);
                                        ui.label(
                                            RichText::new("No tasks without date.")
                                                .font(row_font.clone())
                                                .color(dim),
                                        );
                                    }
                                    for (position, index) in
                                        inbox_indices.iter().copied().enumerate()
                                    {
                                        let selected = self.agenda_focus == AgendaFocus::Inbox
                                            && self.agenda_selected_inbox_item == position;
                                        let response = agenda::task_row(
                                            ui,
                                            index,
                                            selected,
                                            scroll_to_selected && selected,
                                            selected_bg,
                                            row_font.clone(),
                                            text,
                                            accent,
                                            &self.agenda_items[index],
                                        );
                                        if selected && self.agenda_org_mode.is_some() {
                                            org_anchor_rect = Some(response.rect);
                                        }
                                        if response.clicked() {
                                            clicked_inbox_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = false;
                                        }
                                        if response.double_clicked() {
                                            clicked_inbox_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = true;
                                        }
                                    }
                                });
                        } else {
                            let flat_indices = self.agenda_flat_item_indices();
                            let tab_index = self.agenda_tab.index();
                            let title_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), 22.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            ui.painter().rect_filled(title_rect, 0.0, today_bg);
                            ui.painter().text(
                                egui::pos2(title_rect.left() + 6.0, title_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!(
                                    "{} · {} item{} · ↑↓ row · Enter open · Shift+Tab tab",
                                    self.agenda_tab.label(),
                                    flat_indices.len(),
                                    if flat_indices.len() == 1 { "" } else { "s" }
                                ),
                                row_font.clone(),
                                text,
                            );

                            let flat_height = (ui.available_height() - 4.0).max(120.0);
                            egui::ScrollArea::vertical()
                                .id_salt(format!("agenda_{}_scroll", self.agenda_tab.label()))
                                .max_height(flat_height)
                                .show(ui, |ui| {
                                    if flat_indices.is_empty() {
                                        ui.add_space(14.0);
                                        ui.label(
                                            RichText::new(format!(
                                                "No {} items.",
                                                self.agenda_tab.label().to_ascii_lowercase()
                                            ))
                                            .font(row_font.clone())
                                            .color(dim),
                                        );
                                    }
                                    for (position, index) in
                                        flat_indices.iter().copied().enumerate()
                                    {
                                        let selected =
                                            self.agenda_selected_tab_items[tab_index] == position;
                                        let response = agenda::task_row(
                                            ui,
                                            index,
                                            selected,
                                            scroll_to_selected && selected,
                                            selected_bg,
                                            row_font.clone(),
                                            text,
                                            accent,
                                            &self.agenda_items[index],
                                        );
                                        if selected && self.agenda_org_mode.is_some() {
                                            org_anchor_rect = Some(response.rect);
                                        }
                                        if response.clicked() {
                                            clicked_flat_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = false;
                                        }
                                        if response.double_clicked() {
                                            clicked_flat_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = true;
                                        }
                                    }
                                });
                        }
                    });
            });

        if let (Some(mode), Some(anchor_rect)) = (self.agenda_org_mode, org_anchor_rect) {
            let screen_rect = ctx.content_rect();
            let popup_width = 380.0;
            let popup_height = 82.0;
            let x = (anchor_rect.left() + anchor_rect.width() * 0.48)
                .min(screen_rect.right() - popup_width - 18.0)
                .max(screen_rect.left() + 18.0);
            let mut y = anchor_rect.bottom() + 4.0;
            if y + popup_height > screen_rect.bottom() - 18.0 {
                y = (anchor_rect.top() - popup_height - 4.0).max(screen_rect.top() + 18.0);
            }
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                "agenda_org_scrim".into(),
            ))
            .rect_filled(screen_rect, 0.0, Color32::from_black_alpha(72));

            let prefix = match mode {
                AgendaOrgMode::Tag => "#",
                AgendaOrgMode::Project => "@",
            };
            let suggestions = self.agenda_org_suggestions();
            egui::Area::new("agenda_org_autocomplete".into())
                .fixed_pos(egui::pos2(x, y))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let row_font = FontId::new(14.0, FontFamily::Monospace);
                    let accent = Color32::from_rgb(180, 160, 235);
                    let text = Color32::from_rgb(216, 222, 233);
                    let muted = Color32::from_rgb(143, 151, 170);
                    let dim = Color32::from_rgb(118, 112, 150);
                    let selected_bg = Color32::from_rgb(49, 61, 79);
                    let inactive_bg = Color32::from_rgb(27, 32, 42);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(15, 19, 27))
                        .stroke(Stroke::new(1.0, accent))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_min_width(popup_width);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{prefix}{}", self.agenda_org_query))
                                        .font(row_font.clone())
                                        .color(accent),
                                );
                                ui.label(
                                    RichText::new("Tab cycle · ↑↓ select · Enter add · Esc cancel")
                                        .font(row_font.clone())
                                        .color(dim),
                                );
                            });
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                for (index, suggestion) in suggestions.iter().take(8).enumerate() {
                                    let selected = index == self.selected_agenda_org_suggestion;
                                    let chip = RichText::new(format!(" {prefix}{suggestion} "))
                                        .font(row_font.clone())
                                        .color(if selected { text } else { muted });
                                    egui::Frame::new()
                                        .fill(if selected { selected_bg } else { inactive_bg })
                                        .corner_radius(4.0)
                                        .inner_margin(egui::Margin::symmetric(4, 2))
                                        .show(ui, |ui| {
                                            ui.label(chip);
                                        });
                                    ui.add_space(4.0);
                                }
                            });
                        });
                });
        }

        self.flow_item_edit_dialog(ctx);

        if let Some(row) = clicked_dated_row {
            self.agenda_focus = AgendaFocus::Dated;
            self.agenda_selected_dated_row = row;
            self.agenda_scroll_to_selected = false;
        }
        if let Some(row) = clicked_inbox_row {
            self.agenda_focus = AgendaFocus::Inbox;
            self.agenda_selected_inbox_item = row;
            self.agenda_scroll_to_selected = false;
        }
        if let Some(row) = clicked_flat_row {
            self.agenda_selected_tab_items[self.agenda_tab.index()] = row;
            self.agenda_scroll_to_selected = false;
        }
        if let Some(index) = clicked_task_index {
            self.selected_agenda_item = index;
            if open_clicked {
                self.jump_to_agenda_item_index(index);
                if self.standalone_agenda && !self.agenda_open {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Some(date) = toggle_clicked_day {
            if !self.agenda_expanded_dates.insert(date) {
                self.agenda_expanded_dates.remove(&date);
            }
            self.clamp_agenda_selection();
        }
        self.agenda_scroll_to_selected = false;
    }
}

impl eframe::App for SlateApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.remember_current_cursor_position();
        if self.scratch_modal_open || !self.scratch_buffer.as_str().trim().is_empty() {
            self.archive_scratch_modal();
        }
        self.append_to_scratch_archive();
        let _ = self.save_settings();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));
        self.publish_active_flow_buffer_heartbeat();
        self.poll_flow_background_rescan();
        self.poll_flow_open_requests();
        self.poll_flow_completion_requests(&ctx);
        self.poll_flow_organization_requests(&ctx);
        self.poll_flow_edit_requests(&ctx);
        self.poll_pending_flow_agenda_sync(&ctx);
        if self.flow_background_loading {
            ctx.request_repaint();
        }
        if let Some(close_after) = self.scratch_close_after {
            let now = ctx.input(|input| input.time);
            if now >= close_after {
                self.scratch_close_after = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        if let Some(toast_until) = self.agenda_updated_toast_until {
            let now = ctx.input(|input| input.time);
            if now >= toast_until {
                self.agenda_updated_toast_until = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        self.handle_window_close_request(&ctx);
        if self.close_standalone_agenda_if_requested(&ctx) {
            return;
        }
        self.shortcuts(&ctx);

        if self.standalone_agenda {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(30, 36, 48))
                        .inner_margin(0.0),
                )
                .show_inside(ui, |_ui| {});
            self.agenda_dialog(&ctx);
            return;
        }

        if self.pending_action.is_some() {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(Color32::from_rgb(30, 36, 48)))
                .show_inside(ui, |_ui| {});
            self.confirm_action_dialog(&ctx);
            return;
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(30, 36, 48))
                    .inner_margin(0.0),
            )
            .show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let footer_font = FontId::new(13.0, FontFamily::Monospace);
                let footer_color = Color32::from_rgb(136, 154, 176);
                let footer_dim = Color32::from_rgb(94, 105, 126);
                let footer_accent = Color32::from_rgb(136, 192, 208);
                let footer_ok = Color32::from_rgb(163, 190, 140);
                let footer_warn = Color32::from_rgb(235, 203, 139);
                let status_height = 30.0;
                let command_height = 30.0;
                let history_row_height = 22.0;

                let command_line_active = self.command_line_focused || self.focus_command_line_once;
                let command_suggestions = if command_line_active && self.command_history_index.is_none() {
                    self.command_line_suggestions()
                } else {
                    Vec::new()
                };
                let visible_suggestion_rows = command_suggestions.len();
                if visible_suggestion_rows > 0 {
                    self.selected_command_line_suggestion = self
                        .selected_command_line_suggestion
                        .min(visible_suggestion_rows.saturating_sub(1));
                } else {
                    self.selected_command_line_suggestion = 0;
                }
                let recent_file_indices = Vec::<usize>::new();
                let visible_recent_rows = 0;
                let doc_tasks = if self.doc_tasks_open {
                    self.doc_tasks()
                } else {
                    Vec::new()
                };
                let doc_task_indices = if self.doc_tasks_open {
                    self.doc_task_indices()
                } else {
                    Vec::new()
                };
                let visible_doc_task_rows = if self.doc_tasks_open {
                    doc_task_indices.len().min(8)
                } else {
                    0
                };
                let command_history_active = command_line_active
                    && command_suggestions.is_empty()
                    && !self.command_history.is_empty();
                let visible_history_rows = if command_history_active {
                    self.command_history.len().min(self.command_history_limit)
                } else {
                    0
                };
                let suggestion_height = visible_suggestion_rows as f32 * history_row_height;
                let recent_height = visible_recent_rows as f32 * history_row_height;
                let doc_tasks_height = visible_doc_task_rows as f32 * history_row_height;
                let history_height = visible_history_rows as f32 * history_row_height;
                let footer_height = status_height
                    + recent_height
                    + doc_tasks_height
                    + suggestion_height
                    + history_height
                    + command_height;
                let editor_size = Vec2::new(
                    ui.available_width(),
                    (ui.available_height() - footer_height).max(80.0),
                );

                self.editor_view.observe_buffer(&self.buffer);
                let editor_keyboard_enabled = self.duplicate_placement.is_none()
                    && !self.suppress_editor_keyboard_once
                    && !command_line_active
                    && !self.palette_open
                    && !self.settings_open
                    && !self.recent_picker_open
                    && !self.lance_open
                    && !self.doc_tasks_open
                    && !self.agenda_open
                    && !self.file_picker_open
                    && !self.periscope_open
                    && !self.periscope_prompt_open
                    && !self.find_note_open
                    && !self.link_assist_open
                    && !self.link_assist_web_open
                    && !self.link_heading_picker_open
                    && !self.link_preview_open
                    && !self.save_as_open
                    && !self.scratch_modal_open
                    && !self.scratch_entries_open
                    && !self.capture_modal_open
                    && !self.shortcut_help_open
                    && self.search_state.is_none();
                let active_line_text_highlight = self
                    .duplicate_placement
                    .as_ref()
                    .map(|_| self.buffer.cursor_line_col().0);
                self.suppress_editor_keyboard_once = false;

                ui.allocate_ui_with_layout(
                    editor_size,
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if self.preview {
                            ui.columns(2, |columns| {
                                let (response, changed) = self.editor_view.render(
                                    &mut columns[0],
                                    &mut self.buffer,
                                    self.wrap,
                                    self.search_state.as_ref(),
                                    self.line_number_mode,
                                    editor_keyboard_enabled,
                                    false,
                                    active_line_text_highlight,
                                    false,
                                    None,
                                );
                                if self.focus_editor_once
                                    && !self.palette_open
                                    && !self.settings_open
                                    && !self.recent_picker_open
                                    && !self.lance_open
                                    && !self.doc_tasks_open
                                    && !self.agenda_open
                                    && !self.file_picker_open
                                    && !self.periscope_open
                                    && !self.periscope_prompt_open
                                    && !self.find_note_open
                                    && !self.link_heading_picker_open
                                    && !self.link_preview_open
                                    && !self.save_as_open
                                    && !self.scratch_modal_open
                                    && !self.scratch_entries_open
                                    && !self.capture_modal_open
                                    && !self.command_line_focused
                                {
                                    response.request_focus();
                                    self.focus_editor_once = false;
                                }
                                if changed {
                                    self.dirty = true;
                                    self.search_state = None;
                                    if self.editor_view.take_flow_agenda_update_request() {
                                        self.schedule_current_flow_buffer_agenda_sync(&ctx);
                                    }
                                }
                                if let Some(byte) = self.editor_view.take_link_click_byte() {
                                    self.open_markdown_link_at_byte(byte);
                                }
                                if let Some(target) = self.editor_view.take_link_click_target() {
                                    self.open_markdown_link_target(&target);
                                }
                                if let Some(line) = self.editor_view.take_virtual_line_click() {
                                    self.handle_virtual_reference_click(&line);
                                }
                                if let Some(trigger_start) = self.editor_view.take_link_assist_trigger_start() {
                                    self.open_link_assist(trigger_start);
                                }
                                let typed = self.editor_view.take_last_text_input();
                                if !self.detect_inline_bidirectional_link_command(typed.as_deref()) {
                                    self.refresh_link_inline_state(typed.as_deref());
                                }
                                self.refresh_flow_inline_org_state(typed.as_deref());
                                columns[1].vertical(|ui| self.preview_ui(ui));
                            });
                        } else {
                            let (response, changed) = self.editor_view.render(
                                ui,
                                &mut self.buffer,
                                self.wrap,
                                self.search_state.as_ref(),
                                self.line_number_mode,
                                editor_keyboard_enabled,
                                false,
                                active_line_text_highlight,
                                self.markdown_live_rendering,
                                None,
                            );
                            if self.focus_editor_once
                                && !self.palette_open
                                && !self.settings_open
                                && !self.recent_picker_open
                                && !self.lance_open
                                && !self.doc_tasks_open
                                && !self.agenda_open
                                && !self.file_picker_open
                                && !self.periscope_open
                                && !self.periscope_prompt_open
                                && !self.find_note_open
                                && !self.link_heading_picker_open
                                && !self.link_preview_open
                                && !self.save_as_open
                                && !self.scratch_modal_open
                                && !self.scratch_entries_open
                                && !self.capture_modal_open
                                && !self.command_line_focused
                            {
                                response.request_focus();
                                self.focus_editor_once = false;
                            }
                            if changed {
                                self.dirty = true;
                                self.search_state = None;
                                if self.editor_view.take_flow_agenda_update_request() {
                                    self.schedule_current_flow_buffer_agenda_sync(&ctx);
                                }
                            }
                            if let Some(byte) = self.editor_view.take_link_click_byte() {
                                self.open_markdown_link_at_byte(byte);
                            }
                            if let Some(target) = self.editor_view.take_link_click_target() {
                                self.open_markdown_link_target(&target);
                            }
                            if let Some(line) = self.editor_view.take_virtual_line_click() {
                                self.handle_virtual_reference_click(&line);
                            }
                            if let Some(trigger_start) = self.editor_view.take_link_assist_trigger_start() {
                                self.open_link_assist(trigger_start);
                            }
                            let typed = self.editor_view.take_last_text_input();
                            if !self.detect_inline_bidirectional_link_command(typed.as_deref()) {
                                self.refresh_link_inline_state(typed.as_deref());
                            }
                            self.refresh_flow_inline_org_state(typed.as_deref());
                        }
                    },
                );

                self.flow_inline_org_suggestion_popup(&ctx);
                self.link_inline_suggestion_popup(&ctx);

                let filename = self
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "untitled".to_string());
                let dirty_label = if self.dirty { "modified" } else { "saved" };
                let dirty_color = if self.dirty { footer_warn } else { footer_ok };
                let lines = self.buffer.line_count();
                let chars = self.buffer.as_str().chars().count();
                let words = self.buffer.as_str().split_whitespace().count();
                let (cursor_line, cursor_col) = self.buffer.cursor_line_col();
                let cursor_line = cursor_line + 1;
                let cursor_col = cursor_col + 1;
                let base_mode = if self.preview { "preview" } else { "edit" };
                let active_mode = if self.shortcut_help_open {
                    "help"
                } else if self.duplicate_placement.is_some() {
                    "dup"
                } else if self.ctrl_alt_layer_active {
                    "shift-alt"
                } else if self.alt_layer_active {
                    "alt"
                } else if self.ctrl_layer_active {
                    "ctrl"
                } else if self.command_line_focused || self.focus_command_line_once {
                    "command"
                } else if self.recent_picker_open {
                    "recent"
                } else if self.lance_open {
                    "lance"
                } else if self.doc_tasks_open {
                    "doc tasks"
                } else if self.file_picker_open {
                    "files"
                } else if self.periscope_open {
                    "periscope"
                } else if self.periscope_prompt_open {
                    "periscope?"
                } else if self.link_heading_picker_open {
                    "link heading"
                } else if self.link_preview_open {
                    "link preview"
                } else if self.save_as_open {
                    "save as"
                } else if self.scratch_modal_open {
                    "scratch"
                } else if self.scratch_entries_open {
                    "scratch entries"
                } else if self.capture_modal_open {
                    "capture"
                } else if self.search_state.is_some() {
                    "find"
                } else if self.settings_open {
                    "settings"
                } else if self.palette_open {
                    "palette"
                } else {
                    base_mode
                };
                let wrap = if self.wrap { "wrap" } else { "nowrap" };

                let (status_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), status_height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(status_rect);
                painter.rect_filled(status_rect, 0.0, Color32::from_rgb(25, 31, 40));

                // Raw-painted monospace text needs only a tiny optical correction here.
                let status_y = status_rect.center().y - 0.5;
                let mut status_x = status_rect.left() + 10.0;
                for (text, color) in [
                    ("slate".to_string(), footer_accent),
                    ("::".to_string(), footer_dim),
                    (filename, footer_color),
                    (format!("[{dirty_label}]"), dirty_color),
                    (format!("— {}", self.status), footer_dim),
                ] {
                    let text_rect = painter.text(
                        egui::pos2(status_x, status_y),
                        egui::Align2::LEFT_CENTER,
                        text,
                        footer_font.clone(),
                        color,
                    );
                    status_x = text_rect.right() + 8.0;
                }

                let mut status_right = status_rect.right() - 10.0;
                let shortcut_rect = painter.text(
                    egui::pos2(status_right, status_y),
                    egui::Align2::RIGHT_CENTER,
                    "[Ctrl+P] [Ctrl+H]",
                    footer_font.clone(),
                    footer_accent,
                );
                status_right = shortcut_rect.left() - 12.0;
                painter.text(
                    egui::pos2(status_right, status_y),
                    egui::Align2::RIGHT_CENTER,
                    format!(
                        "{active_mode} · {wrap} · ln {cursor_line}, col {cursor_col} · {lines}l · {words}w · {chars}c"
                    ),
                    footer_font.clone(),
                    footer_dim,
                );

                if visible_recent_rows > 0 {
                    let (recent_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), recent_height),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(recent_rect);
                    painter.rect_filled(recent_rect, 0.0, Color32::from_rgb(25, 31, 40));

                    let selected_position = recent_file_indices
                        .iter()
                        .position(|index| *index == self.selected_recent_file)
                        .unwrap_or(0);
                    let start = selected_position
                        .min(recent_file_indices.len().saturating_sub(visible_recent_rows));
                    let end = (start + visible_recent_rows).min(recent_file_indices.len());
                    for (row, index) in recent_file_indices[start..end].iter().copied().enumerate() {
                        let row_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                recent_rect.left(),
                                recent_rect.top() + row as f32 * history_row_height,
                            ),
                            Vec2::new(recent_rect.width(), history_row_height),
                        );
                        let selected = index == self.selected_recent_file;
                        if selected {
                            painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                        }
                        let path = &self.recent_files[index];
                        let name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("unknown");
                        painter.text(
                            egui::pos2(row_rect.left() + 10.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            if selected { ">" } else { " " },
                            footer_font.clone(),
                            footer_accent,
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 28.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            name,
                            footer_font.clone(),
                            if selected { footer_color } else { footer_dim },
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 190.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            path.display().to_string(),
                            footer_font.clone(),
                            footer_dim,
                        );
                        let response = ui.interact(
                            row_rect,
                            ui.id().with(("recent_file", index)),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            self.selected_recent_file = index;
                            self.open_selected_recent_file();
                        }
                    }
                }

                if visible_doc_task_rows > 0 {
                    let (doc_tasks_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), doc_tasks_height),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(doc_tasks_rect);
                    painter.rect_filled(doc_tasks_rect, 0.0, Color32::from_rgb(25, 31, 40));

                    let selected_position = doc_task_indices
                        .iter()
                        .position(|index| {
                            doc_tasks
                                .get(*index)
                                .map(|task| task.line_index == self.selected_doc_task_line)
                                .unwrap_or(false)
                        })
                        .unwrap_or(0);
                    let start = selected_position
                        .min(doc_task_indices.len().saturating_sub(visible_doc_task_rows));
                    let end = (start + visible_doc_task_rows).min(doc_task_indices.len());
                    for (row, index) in doc_task_indices[start..end].iter().copied().enumerate() {
                        let Some(task) = doc_tasks.get(index) else {
                            continue;
                        };
                        let row_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                doc_tasks_rect.left(),
                                doc_tasks_rect.top() + row as f32 * history_row_height,
                            ),
                            Vec2::new(doc_tasks_rect.width(), history_row_height),
                        );
                        let selected = task.line_index == self.selected_doc_task_line;
                        if selected {
                            painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                        }
                        let marker = match task.state {
                            CheckboxState::Empty => "[ ]",
                            CheckboxState::Doing => "[/]",
                            CheckboxState::Done => "[x]",
                        };
                        painter.text(
                            egui::pos2(row_rect.left() + 10.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            if selected { ">" } else { " " },
                            footer_font.clone(),
                            footer_accent,
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 28.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            marker,
                            footer_font.clone(),
                            match task.state {
                                CheckboxState::Empty => footer_dim,
                                CheckboxState::Doing => footer_warn,
                                CheckboxState::Done => footer_ok,
                            },
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 66.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{}{}", task.task_prefix, task.text),
                            footer_font.clone(),
                            if selected { footer_color } else { footer_dim },
                        );
                        painter.text(
                            egui::pos2(row_rect.right() - 10.0, row_rect.center().y - 1.0),
                            egui::Align2::RIGHT_CENTER,
                            format!("line {}", task.line_index + 1),
                            footer_font.clone(),
                            footer_dim,
                        );
                        let response = ui.interact(
                            row_rect,
                            ui.id().with(("doc_task", task.line_index)),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            self.selected_doc_task_line = task.line_index;
                            self.jump_to_selected_doc_task();
                        }
                    }
                }

                if visible_suggestion_rows > 0 {
                    let (suggestions_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), suggestion_height),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(suggestions_rect);
                    painter.rect_filled(suggestions_rect, 0.0, Color32::from_rgb(25, 31, 40));

                    for (row, command) in command_suggestions.iter().enumerate() {
                        let selected = row == self.selected_command_line_suggestion;
                        let row_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                suggestions_rect.left(),
                                suggestions_rect.top() + row as f32 * history_row_height,
                            ),
                            Vec2::new(suggestions_rect.width(), history_row_height),
                        );
                        if selected {
                            painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                        }

                        painter.text(
                            egui::pos2(row_rect.left() + 10.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            if selected { ">" } else { " " },
                            footer_font.clone(),
                            footer_accent,
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 28.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            command.name,
                            footer_font.clone(),
                            if selected { footer_color } else { footer_dim },
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 190.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            command.summary,
                            footer_font.clone(),
                            footer_dim,
                        );
                        painter.text(
                            egui::pos2(row_rect.right() - 10.0, row_rect.center().y - 1.0),
                            egui::Align2::RIGHT_CENTER,
                            command.hint,
                            footer_font.clone(),
                            footer_warn,
                        );
                    }
                }

                if visible_history_rows > 0 {
                    let (history_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), history_height),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(history_rect);
                    painter.rect_filled(history_rect, 0.0, Color32::from_rgb(25, 31, 40));

                    let len = self.command_history.len();
                    let rows = visible_history_rows;
                    let selected_index = self.command_history_index;
                    let start = selected_index
                        .map(|idx| idx.min(len.saturating_sub(rows)))
                        .unwrap_or_else(|| len.saturating_sub(rows));
                    let end = (start + rows).min(len);

                    for (row, index) in (start..end).enumerate() {
                        let row_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                history_rect.left(),
                                history_rect.top() + row as f32 * history_row_height,
                            ),
                            Vec2::new(history_rect.width(), history_row_height),
                        );
                        let selected = selected_index == Some(index);
                        if selected {
                            painter.rect_filled(row_rect, 0.0, Color32::from_rgb(38, 47, 61));
                        }

                        let marker = if selected { ">" } else { " " };
                        painter.text(
                            egui::pos2(row_rect.left() + 10.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            marker,
                            footer_font.clone(),
                            footer_accent,
                        );
                        painter.text(
                            egui::pos2(row_rect.left() + 28.0, row_rect.center().y - 1.0),
                            egui::Align2::LEFT_CENTER,
                            &self.command_history[index],
                            footer_font.clone(),
                            if selected { footer_color } else { footer_dim },
                        );

                        let response = ui.interact(
                            row_rect,
                            ui.id().with(("command_history", index)),
                            egui::Sense::click(),
                        );
                        if response.clicked() {
                            self.command_history_index = Some(index);
                            self.command_line = self.command_history[index].clone();
                            self.command_line_cursor = self.command_line.len();
                            self.focus_command_line_once = true;
                        }
                    }
                }

                let (command_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), command_height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(command_rect);
                painter.rect_filled(command_rect, 0.0, Color32::from_rgb(25, 31, 40));
                let command_center_y = command_rect.center().y;
                let input_rect = egui::Rect::from_min_max(
                    egui::pos2(command_rect.left() + 19.0, command_rect.top() + 4.0),
                    egui::pos2(command_rect.right() - 10.0, command_rect.bottom() - 4.0),
                );
                let paint_command_line = |segments: Vec<(String, Color32)>| {
                    let mut text = String::new();
                    let mut sections = Vec::new();
                    for (segment, color) in segments {
                        let start = text.len();
                        text.push_str(&segment);
                        let end = text.len();
                        sections.push(LayoutSection {
                            leading_space: 0.0,
                            byte_range: start..end,
                            format: TextFormat::simple(footer_font.clone(), color),
                        });
                    }
                    let galley = painter.layout_job(LayoutJob {
                        text,
                        sections,
                        break_on_newline: false,
                        ..Default::default()
                    });
                    painter.galley(
                        egui::pos2(
                            command_rect.left() + 10.0,
                            command_center_y - galley.size().y * 0.5,
                        ),
                        galley,
                        footer_color,
                    );
                };
                let command_line_active = self.command_line_focused || self.focus_command_line_once;
                if command_line_active {
                    if self.focus_command_line_once {
                        self.focus_command_line_once = false;
                    }
                    self.command_line_focused = true;
                    if self.command_line.is_empty() {
                        paint_command_line(vec![
                            (":".to_string(), footer_accent),
                            (
                                "command  Ctrl+. enter · Tab complete · Ctrl+H help · Ctrl+P palette"
                                    .to_string(),
                                footer_dim,
                            ),
                        ]);
                    } else {
                        let mut segments = vec![
                            (":".to_string(), footer_accent),
                            (self.command_line.clone(), footer_color),
                        ];
                        if self.command_line_cursor == self.command_line.len() {
                            if let Some(completion) = self.command_line_completion() {
                                segments.push((completion.to_string(), Color32::from_rgb(76, 86, 106)));
                            }
                        }
                        paint_command_line(segments);
                    }
                    let cursor_chars = self.command_line[..self.command_line_cursor.min(self.command_line.len())]
                        .chars()
                        .count() as f32;
                    let cursor_x = input_rect.left() + cursor_chars * 8.0;
                    painter.line_segment(
                        [
                            egui::pos2(cursor_x, input_rect.top() + 3.0),
                            egui::pos2(cursor_x, input_rect.bottom() - 3.0),
                        ],
                        Stroke::new(1.0, footer_accent),
                    );
                } else {
                    let (text, color) = if self.duplicate_placement.is_some() {
                        (
                            "duplicate placement  Alt movement or Ctrl+Shift movement · Enter/Space place · Esc cancel".to_string(),
                            footer_accent,
                        )
                    } else if self.ctrl_alt_layer_active {
                        (format!("shift+alt:{}", self.ctrl_alt_layer_sequence), footer_accent)
                    } else if self.alt_layer_active {
                        (format!("alt:{}", self.alt_layer_sequence), footer_accent)
                    } else if self.ctrl_layer_active {
                        (format!("ctrl:{}", self.ctrl_layer_sequence), footer_accent)
                    } else if self.shortcut_help_open {
                        ("shortcuts  [esc] close".to_string(), footer_accent)
                    } else if self.recent_picker_open {
                        (
                            format!("recent files {} ↑↓ select · type filter · Enter open · Esc close", if self.recent_query.is_empty() { "".to_string() } else { format!("/{} ", self.recent_query) }),
                            footer_accent,
                        )
                    } else if self.lance_open {
                        (
                            format!("lance {} ↑↓ select · type filter · Enter open · Ctrl+Enter modal · Ctrl+A add · Del remove", if self.lance_query.is_empty() { "".to_string() } else { format!("/{} ", self.lance_query) }),
                            footer_accent,
                        )
                    } else if self.file_picker_open {
                        ("files  type filter · ↑↓ select · → enter folder · ← parent · Enter open · Esc close".to_string(), footer_accent)
                    } else if self.periscope_open {
                        ("periscope  type search · ↑↓ select · Tab project/home · Enter open · Ctrl+Enter modal".to_string(), footer_accent)
                    } else if self.find_note_open {
                        ("find-note  type filter · ↑↓ select · Tab/←/→ headings · Enter main · Shift+Enter front modal".to_string(), footer_accent)
                    } else if self.periscope_prompt_open {
                        ("periscope  Y/Enter project folder · N global · Esc cancel".to_string(), footer_accent)
                    } else if self.link_inline_anchor.is_some() {
                        ("link  type fuzzy · ↑↓ select · Enter file · Tab/Shift+Enter headings · ] cancel/manual".to_string(), footer_accent)
                    } else if self.link_heading_picker_open {
                        let action = match self.link_heading_picker_mode {
                            LinkHeadingPickerMode::InsertLink => "insert",
                            LinkHeadingPickerMode::BidirectionalLink => "link both ways",
                            LinkHeadingPickerMode::Refile => "refile",
                        };
                        (format!("link heading  ↑↓ select · Enter {action} · Esc cancel · first option = whole file/end"), footer_accent)
                    } else if self.link_preview_open {
                        ("modal buffer  Tab swap · Ctrl+Enter promote · Ctrl+S save · Esc close".to_string(), footer_accent)
                    } else if self.save_as_open {
                        ("save as  type file name · ↑↓ select · → enter folder · ← parent · Enter save · Esc close".to_string(), footer_accent)
                    } else if self.scratch_modal_open {
                        ("scratch  Ctrl+S archive · Ctrl+E entries · Esc hide · :scratch resume".to_string(), footer_accent)
                    } else if self.scratch_entries_open {
                        ("scratch entries  ↑↓ select · Ctrl+D/Delete delete · Esc close".to_string(), footer_accent)
                    } else if self.capture_modal_open {
                        ("capture  type optional title · Enter archive · Esc cancel".to_string(), footer_accent)
                    } else if self.agenda_open {
                        ("agenda  Tab segment · ↑↓ move · ←→ week · Enter open · Ctrl+Enter cycle · r refile · x show done · Esc close".to_string(), footer_accent)
                    } else {
                        (
                            "command  Ctrl+. enter · Ctrl+H help · Ctrl+P palette · w · q · wq"
                                .to_string(),
                            footer_dim,
                        )
                    };
                    paint_command_line(vec![(":".to_string(), footer_accent), (text, color)]);
                    self.command_line_focused = false;
                }
            });

        self.command_palette(&ctx);
        self.settings_dialog(&ctx);
        self.shortcut_help_dialog(&ctx);
        self.link_assist_dialog(&ctx);
        self.link_preview_dialog(&ctx);
        self.slate_picker_dialog(&ctx);
        self.periscope_prompt_dialog(&ctx);
        self.periscope_dialog(&ctx);
        self.find_note_dialog(&ctx);
        self.save_as_dialog(&ctx);
        self.scratch_modal_dialog(&ctx);
        self.capture_dialog(&ctx);
        self.scratch_entries_dialog(&ctx);
        self.agenda_dialog(&ctx);
        self.scratch_feedback_toast(&ctx);
    }
}

fn split_cache_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_cache_date(value: &str) -> Option<NaiveDate> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Ok(bytes) = fs::read("/usr/share/fonts/noto/NotoSansMono-Regular.ttf") {
        fonts.font_data.insert(
            "noto_mono".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "noto_mono".to_string());
    }

    if let Ok(bytes) = fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf") {
        fonts.font_data.insert(
            "noto_sans".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "noto_sans".to_string());
    }

    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = Color32::from_rgb(30, 36, 48);
    style.visuals.panel_fill = Color32::from_rgb(30, 36, 48);
    style.visuals.extreme_bg_color = Color32::from_rgb(25, 31, 40);
    style.visuals.faint_bg_color = Color32::from_rgb(38, 47, 61);
    style.visuals.selection.bg_fill = Color32::from_rgb(67, 76, 94);
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(136, 192, 208));
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 41, 54);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 56, 72);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(59, 66, 82);
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    ctx.set_global_style(style);
}
