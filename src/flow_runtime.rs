use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn runtime_path(sqlite_path: &Path, name: &str) -> Result<PathBuf, String> {
    let Some(parent) = sqlite_path.parent() else {
        return Err("Slate Flow runtime path failed".to_string());
    };
    Ok(parent.join(name))
}

pub(crate) fn completion_requests_path(sqlite_path: &Path) -> Result<PathBuf, String> {
    runtime_path(sqlite_path, "completion-requests.tsv")
}

pub(crate) fn organization_requests_path(sqlite_path: &Path) -> Result<PathBuf, String> {
    runtime_path(sqlite_path, "organization-requests.tsv")
}

pub(crate) fn edit_requests_path(sqlite_path: &Path) -> Result<PathBuf, String> {
    runtime_path(sqlite_path, "edit-requests.tsv")
}

pub(crate) fn open_requests_path(sqlite_path: &Path) -> Result<PathBuf, String> {
    runtime_path(sqlite_path, "open-requests.tsv")
}

pub(crate) fn active_buffers_path(sqlite_path: &Path) -> Result<PathBuf, String> {
    runtime_path(sqlite_path, "active-buffers.tsv")
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn relative_path(scope: &Path, path: &Path) -> String {
    path.strip_prefix(scope)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

pub(crate) fn publish_active_buffer_heartbeat(
    sqlite_path: &Path,
    scope: &Path,
    current_path: &Path,
) -> Result<(), String> {
    let path = active_buffers_path(sqlite_path)?;
    let relative = relative_path(scope, current_path);
    let now = unix_now();
    let pid = std::process::id();
    let pid_string = pid.to_string();
    let mut rows = fs::read_to_string(&path)
        .ok()
        .map(|raw| {
            raw.lines()
                .filter(|line| {
                    let mut fields = line.split('\t');
                    let row_pid = fields.next().unwrap_or("");
                    let seen = fields
                        .next()
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(0);
                    let row_relative = fields.next().unwrap_or("");
                    row_pid != pid_string
                        && row_relative != relative
                        && now.saturating_sub(seen) <= 10
                })
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    rows.push(format!(
        "{}\t{}\t{}\t{}",
        pid,
        now,
        relative.replace('\t', " "),
        current_path.display().to_string().replace('\t', " ")
    ));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Slate Flow active buffer dir failed: {err}"))?;
    }
    fs::write(path, format!("{}\n", rows.join("\n")))
        .map_err(|err| format!("Slate Flow active buffer write failed: {err}"))
}

pub(crate) fn active_buffer_contains(sqlite_path: &Path, relative_path: &str) -> bool {
    let Ok(path) = active_buffers_path(sqlite_path) else {
        return false;
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let now = unix_now();
    raw.lines().any(|line| {
        let mut fields = line.split('\t');
        let _pid = fields.next();
        let seen = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let relative = fields.next().unwrap_or("");
        now.saturating_sub(seen) <= 10 && relative == relative_path
    })
}

fn append_line(path: PathBuf, line: String, context: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{context} dir failed: {err}"))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("{context} open failed: {err}"))?;
    writeln!(file, "{line}").map_err(|err| format!("{context} write failed: {err}"))
}

pub(crate) fn append_completion_request(
    sqlite_path: &Path,
    id: &str,
    relative_path: &str,
    line_index: usize,
    title: &str,
) -> Result<(), String> {
    append_line(
        completion_requests_path(sqlite_path)?,
        format!(
            "{}\t{}\t{}\t{}",
            id.replace('\t', " "),
            relative_path.replace('\t', " "),
            line_index,
            title.replace(['\t', '\n'], " ")
        ),
        "Slate Flow API request",
    )
}

pub(crate) fn append_organization_request(
    sqlite_path: &Path,
    relative_path: &str,
    line_index: usize,
    token: &str,
) -> Result<(), String> {
    append_line(
        organization_requests_path(sqlite_path)?,
        format!(
            "{}\t{}\t{}",
            relative_path.replace('\t', " "),
            line_index,
            token.replace(['\t', '\n'], " ")
        ),
        "Slate Flow organization request",
    )
}

pub(crate) fn append_edit_request(
    sqlite_path: &Path,
    relative_path: &str,
    line_index: usize,
    replacement: &str,
) -> Result<(), String> {
    append_line(
        edit_requests_path(sqlite_path)?,
        format!(
            "{}\t{}\t{}",
            relative_path.replace('\t', " "),
            line_index,
            replacement.replace(['\t', '\n'], " ")
        ),
        "Slate Flow edit request",
    )
}

pub(crate) fn append_open_request(
    sqlite_path: &Path,
    relative_path: &str,
    line_index: usize,
) -> Result<(), String> {
    append_line(
        open_requests_path(sqlite_path)?,
        format!("{}\t{}", relative_path.replace('\t', " "), line_index),
        "Slate Flow open request",
    )
}

pub(crate) struct OpenRequest {
    pub(crate) line_index: usize,
}

pub(crate) struct CompletionRequest {
    pub(crate) raw: String,
    pub(crate) line_index: usize,
    pub(crate) title: String,
}

pub(crate) struct LineValueRequest {
    pub(crate) raw: String,
    pub(crate) line_index: usize,
    pub(crate) value: String,
}

fn split_request_lines(path: &Path) -> Option<Vec<String>> {
    fs::read_to_string(path)
        .ok()
        .map(|raw| raw.lines().map(str::to_string).collect())
}

pub(crate) fn finish_request_drain(path: &Path, remaining: Vec<String>) {
    if remaining.is_empty() {
        let _ = fs::remove_file(path);
    } else {
        let _ = fs::write(path, format!("{}\n", remaining.join("\n")));
    }
}

pub(crate) fn drain_open_requests_for_path(
    sqlite_path: &Path,
    scope: &Path,
    current_path: &Path,
) -> Option<(PathBuf, Vec<OpenRequest>, Vec<String>)> {
    let path = open_requests_path(sqlite_path).ok()?;
    let current_relative = relative_path(scope, current_path);
    let mut matching = Vec::new();
    let mut remaining = Vec::new();
    for raw in split_request_lines(&path)? {
        let mut fields = raw.split('\t');
        let relative_path = fields.next().unwrap_or("");
        let line_index = fields
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        if relative_path == current_relative {
            matching.push(OpenRequest { line_index });
        } else {
            remaining.push(raw);
        }
    }
    Some((path, matching, remaining))
}

pub(crate) fn drain_completion_requests_for_path(
    sqlite_path: &Path,
    scope: &Path,
    current_path: &Path,
) -> Option<(PathBuf, Vec<CompletionRequest>, Vec<String>)> {
    let path = completion_requests_path(sqlite_path).ok()?;
    let current_relative = relative_path(scope, current_path);
    let mut matching = Vec::new();
    let mut remaining = Vec::new();
    for raw in split_request_lines(&path)? {
        let mut fields = raw.split('\t');
        let _id = fields.next().unwrap_or("");
        let relative_path = fields.next().unwrap_or("");
        let line_index = fields
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let title = fields.next().unwrap_or("").to_string();
        if relative_path == current_relative {
            matching.push(CompletionRequest {
                raw,
                line_index,
                title,
            });
        } else {
            remaining.push(raw);
        }
    }
    Some((path, matching, remaining))
}

pub(crate) fn drain_organization_requests_for_path(
    sqlite_path: &Path,
    scope: &Path,
    current_path: &Path,
) -> Option<(PathBuf, Vec<LineValueRequest>, Vec<String>)> {
    let path = organization_requests_path(sqlite_path).ok()?;
    drain_line_value_requests_for_path(path, scope, current_path)
}

pub(crate) fn drain_edit_requests_for_path(
    sqlite_path: &Path,
    scope: &Path,
    current_path: &Path,
) -> Option<(PathBuf, Vec<LineValueRequest>, Vec<String>)> {
    let path = edit_requests_path(sqlite_path).ok()?;
    drain_line_value_requests_for_path(path, scope, current_path)
}

fn drain_line_value_requests_for_path(
    path: PathBuf,
    scope: &Path,
    current_path: &Path,
) -> Option<(PathBuf, Vec<LineValueRequest>, Vec<String>)> {
    let current_relative = relative_path(scope, current_path);
    let mut matching = Vec::new();
    let mut remaining = Vec::new();
    for raw in split_request_lines(&path)? {
        let mut fields = raw.split('\t');
        let relative_path = fields.next().unwrap_or("");
        let line_index = fields
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let value = fields.next().unwrap_or("").to_string();
        if relative_path == current_relative {
            matching.push(LineValueRequest {
                raw,
                line_index,
                value,
            });
        } else {
            remaining.push(raw);
        }
    }
    Some((path, matching, remaining))
}
