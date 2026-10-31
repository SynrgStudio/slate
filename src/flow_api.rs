//! `slate --flow-api ...` command-line interface used by external tools.
use super::*;

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

pub(super) fn run_flow_api(args: &[String]) -> Result<(), String> {
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
    let summary = flow_batch::index_files(scope, &sqlite_path, &files, &[])?;
    if let Some(err) = summary.first_error {
        return Err(err);
    }
    Ok((summary.indexed_files, summary.indexed_tasks))
}

fn flow_api_load_tasks(scope: &Path) -> Result<Vec<FlowApiTask>, String> {
    let sqlite_path = SlateApp::flow_sqlite_path_for_root(scope)?;
    if !sqlite_path.exists() {
        return Ok(Vec::new());
    }
    Ok(flow_store::read_tasks(&sqlite_path)?
        .into_iter()
        .map(|row| {
            let id = flow_api_task_id(scope, &row.path, row.line_number, &row.title);
            FlowApiTask {
                id,
                path: scope.join(&row.path),
                relative_path: row.path,
                line_number: row.line_number,
                line_index: row.line_index,
                kind: row.kind,
                state: row.state,
                title: row.title,
                tags: split_cache_list(&row.tags),
                projects: split_cache_list(&row.projects),
                priority: non_empty_string(&row.priority),
                due: non_empty_string(&row.due),
                scheduled: non_empty_string(&row.scheduled),
                heading: non_empty_string(&row.heading),
                source_snippet: row.source_snippet,
            }
        })
        .collect())
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

pub(super) fn append_flow_organization_request(
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

pub(super) fn append_flow_edit_request(
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

pub(super) fn append_flow_open_request(
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
