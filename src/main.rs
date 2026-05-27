mod editor_buffer;
mod editor_view;
mod goto;
mod markdown;
mod search;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use editor_buffer::EditorBuffer;
use editor_view::{EditorView, LineNumberMode};
use eframe::egui::{
    self, Color32, FontFamily, FontId, Key, RichText, Stroke, TextEdit, Vec2,
    text::{LayoutJob, LayoutSection, TextFormat},
};
use goto::GotoTarget;
use markdown::{
    CheckboxState, is_markdown_separator, parse_blockquote_line, parse_checkbox_line,
    parse_fenced_code_marker, parse_heading_line, parse_inline_code_spans, parse_list_line,
};
use search::SearchState;

fn main() -> eframe::Result {
    let mut scratch = false;
    let mut path = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--scratch" | "-s" => scratch = true,
            _ => path = Some(PathBuf::from(arg)),
        }
    }

    let title = if scratch { "Slate Scratch" } else { "Slate" };
    let size = if scratch {
        [760.0, 460.0]
    } else {
        [900.0, 560.0]
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size([420.0, 260.0]),
        vsync: false,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(|cc| Ok(Box::new(SlateApp::new(cc, path, scratch)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Scratch,
    ScratchEntries,
    Capture,
    TogglePreview,
    ToggleWrap,
    Settings,
    LineNumbersAbsolute,
    LineNumbersRelative,
    WrapOn,
    WrapOff,
    PreviewOn,
    PreviewOff,
    DocTasks,
    Quit,
}

#[derive(Clone)]
struct ScratchEntry {
    heading: Option<String>,
    body: String,
}

impl ScratchEntry {
    fn title(&self) -> String {
        self.heading
            .clone()
            .unwrap_or_else(|| "untitled scratch".to_string())
    }

    fn preview(&self) -> String {
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

struct CommandSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    summary: &'static str,
    hint: &'static str,
    palette_command: Option<Command>,
}

const COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "save",
        aliases: &["w", "write"],
        summary: "Save current buffer",
        hint: "Ctrl+S",
        palette_command: Some(Command::Save),
    },
    CommandSpec {
        name: "quit",
        aliases: &["q", "exit"],
        summary: "Quit Slate",
        hint: "Ctrl+Q",
        palette_command: Some(Command::Quit),
    },
    CommandSpec {
        name: "wq",
        aliases: &["x"],
        summary: "Save and quit",
        hint: ":wq",
        palette_command: None,
    },
    CommandSpec {
        name: "new",
        aliases: &["enew"],
        summary: "New buffer",
        hint: "Ctrl+N",
        palette_command: Some(Command::New),
    },
    CommandSpec {
        name: "open",
        aliases: &["edit", "e"],
        summary: "Open file or path",
        hint: "Ctrl+O",
        palette_command: Some(Command::Open),
    },
    CommandSpec {
        name: "open-last",
        aliases: &["last", "ol"],
        summary: "Open last file",
        hint: "Ctrl+O L",
        palette_command: None,
    },
    CommandSpec {
        name: "recent",
        aliases: &["rec"],
        summary: "Open recent files picker",
        hint: ":recent",
        palette_command: None,
    },
    CommandSpec {
        name: "scratch",
        aliases: &["sc"],
        summary: "Open quick scratch capture modal",
        hint: ":scratch",
        palette_command: Some(Command::Scratch),
    },
    CommandSpec {
        name: "capture",
        aliases: &["cap"],
        summary: "Capture selection or current line to scratch",
        hint: ":capture",
        palette_command: Some(Command::Capture),
    },
    CommandSpec {
        name: "scratch-entries",
        aliases: &["scratch-log", "scl"],
        summary: "Review and clean scratch archive entries",
        hint: ":scratch-entries",
        palette_command: Some(Command::ScratchEntries),
    },
    CommandSpec {
        name: "save-as",
        aliases: &["saveas", "write-as"],
        summary: "Save current buffer with a new path",
        hint: "Ctrl+Alt+S",
        palette_command: Some(Command::SaveAs),
    },
    CommandSpec {
        name: "preview",
        aliases: &["md"],
        summary: "Toggle Markdown preview",
        hint: "Ctrl+M",
        palette_command: Some(Command::TogglePreview),
    },
    CommandSpec {
        name: "wrap",
        aliases: &[],
        summary: "Toggle word wrap",
        hint: ":wrap",
        palette_command: Some(Command::ToggleWrap),
    },
    CommandSpec {
        name: "wrap-on",
        aliases: &["wrap-enable"],
        summary: "Enable word wrap",
        hint: ":wrap on",
        palette_command: Some(Command::WrapOn),
    },
    CommandSpec {
        name: "wrap-off",
        aliases: &["nowrap", "wrap-disable"],
        summary: "Disable word wrap",
        hint: ":wrap off",
        palette_command: Some(Command::WrapOff),
    },
    CommandSpec {
        name: "preview-on",
        aliases: &["md-on"],
        summary: "Enable Markdown preview",
        hint: ":preview on",
        palette_command: Some(Command::PreviewOn),
    },
    CommandSpec {
        name: "preview-off",
        aliases: &["md-off"],
        summary: "Disable Markdown preview",
        hint: ":preview off",
        palette_command: Some(Command::PreviewOff),
    },
    CommandSpec {
        name: "doc-tasks",
        aliases: &["tasks"],
        summary: "Browse current-document checkboxes",
        hint: ":doc-tasks",
        palette_command: Some(Command::DocTasks),
    },
    CommandSpec {
        name: "find",
        aliases: &["f"],
        summary: "Find text",
        hint: "Ctrl+F",
        palette_command: None,
    },
    CommandSpec {
        name: "goto",
        aliases: &["g", "line", "l"],
        summary: "Go to line/column",
        hint: ":g 10",
        palette_command: None,
    },
    CommandSpec {
        name: "select-word",
        aliases: &["sw"],
        summary: "Select word under cursor",
        hint: "Ctrl S W",
        palette_command: None,
    },
    CommandSpec {
        name: "select-line",
        aliases: &["sl"],
        summary: "Select current line",
        hint: "Ctrl S L",
        palette_command: None,
    },
    CommandSpec {
        name: "delete-word",
        aliases: &["dw"],
        summary: "Delete word under cursor",
        hint: "Ctrl D W",
        palette_command: None,
    },
    CommandSpec {
        name: "delete-line",
        aliases: &["dl"],
        summary: "Delete current line",
        hint: "Ctrl D L",
        palette_command: None,
    },
    CommandSpec {
        name: "duplicate-line",
        aliases: &["dup"],
        summary: "Duplicate current line",
        hint: "Ctrl D U P",
        palette_command: None,
    },
    CommandSpec {
        name: "duplicate-place",
        aliases: &["dupp"],
        summary: "Duplicate line then place it",
        hint: "Ctrl D U P P",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-up",
        aliases: &["mlu"],
        summary: "Move current line up",
        hint: "Alt+Up",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-down",
        aliases: &["mld"],
        summary: "Move current line down",
        hint: "Alt+Down",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-to-paragraph-start",
        aliases: &["mlps"],
        summary: "Move line to paragraph start",
        hint: "Alt double-up",
        palette_command: None,
    },
    CommandSpec {
        name: "move-line-to-paragraph-end",
        aliases: &["mlpe"],
        summary: "Move line to paragraph end",
        hint: "Alt double-down",
        palette_command: None,
    },
    CommandSpec {
        name: "top",
        aliases: &["go-top", "gt"],
        summary: "Go to top of file",
        hint: "Ctrl G T",
        palette_command: None,
    },
    CommandSpec {
        name: "bottom",
        aliases: &["go-bottom", "gb"],
        summary: "Go to bottom of file",
        hint: "Ctrl G B",
        palette_command: None,
    },
    CommandSpec {
        name: "line-numbers",
        aliases: &["ln", "linenumbers"],
        summary: "Set line number mode",
        hint: ":line-numbers relative",
        palette_command: None,
    },
    CommandSpec {
        name: "line-numbers-absolute",
        aliases: &["ln-abs"],
        summary: "Use absolute line numbers",
        hint: ":ln absolute",
        palette_command: Some(Command::LineNumbersAbsolute),
    },
    CommandSpec {
        name: "line-numbers-relative",
        aliases: &["ln-rel"],
        summary: "Use relative line numbers",
        hint: ":ln relative",
        palette_command: Some(Command::LineNumbersRelative),
    },
    CommandSpec {
        name: "settings",
        aliases: &["set", "prefs", "preferences"],
        summary: "Open settings",
        hint: ":settings",
        palette_command: Some(Command::Settings),
    },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    New,
    Open,
    OpenLast,
    OpenRecent,
    OpenProjectFile,
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
            PendingAction::Quit => "buffer has unsaved changes; close anyway?",
        }
    }
}

struct DuplicatePlacement {
    snapshot: String,
    was_dirty: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FilePickerMode {
    Open,
    Browse,
}

#[derive(Clone)]
struct DocTask {
    line_index: usize,
    state: CheckboxState,
    task_prefix: String,
    text: String,
}

struct CommandUsage {
    name: String,
    count: usize,
    last_used: i64,
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
    last_opened_path: Option<PathBuf>,
    recent_files: Vec<PathBuf>,
    recent_picker_open: bool,
    recent_query: String,
    selected_recent_file: usize,
    pending_recent_path: Option<PathBuf>,
    doc_tasks_open: bool,
    doc_task_query: String,
    selected_doc_task_line: usize,
    file_picker_open: bool,
    file_picker_mode: FilePickerMode,
    file_picker_dir: PathBuf,
    file_query: String,
    project_files: Vec<PathBuf>,
    selected_project_file: usize,
    pending_project_file_path: Option<PathBuf>,
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
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>, scratch: bool) -> Self {
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
            last_opened_path: None,
            recent_files: Vec::new(),
            recent_picker_open: false,
            recent_query: String::new(),
            selected_recent_file: 0,
            pending_recent_path: None,
            doc_tasks_open: false,
            doc_task_query: String::new(),
            selected_doc_task_line: 0,
            file_picker_open: false,
            file_picker_mode: FilePickerMode::Browse,
            file_picker_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            file_query: String::new(),
            project_files: Vec::new(),
            selected_project_file: 0,
            pending_project_file_path: None,
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
        } else {
            format!("{}{} — Slate", if self.dirty { "*" } else { "" }, name)
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.buffer.set_text(text);
                self.path = Some(path.clone());
                self.dirty = false;
                self.search_state = None;
                self.remember_recent_file(path.clone());
                let _ = self.save_settings();
                self.status = format!("Opened {}", path.display());
            }
            Err(err) => self.status = format!("Open failed: {err}"),
        }
    }

    fn save(&mut self) {
        if let Some(path) = self.path.clone() {
            self.save_path(path);
        } else {
            self.save_as();
        }
    }

    fn scratch_archive_path(&mut self) -> Option<PathBuf> {
        let Some(mut dir) = dirs_next::data_dir() else {
            self.status = "Scratch failed: no data dir".to_string();
            return None;
        };
        dir.push("slate");
        Some(dir.join("scratch.md"))
    }

    fn append_text_to_scratch_archive(&mut self, text: &str) -> bool {
        let text = text.trim_end();
        if text.trim().is_empty() {
            return false;
        }

        let Some(path) = self.scratch_archive_path() else {
            self.status = "Scratch append failed: no data dir".to_string();
            return false;
        };
        if let Some(dir) = path.parent() {
            if let Err(err) = fs::create_dir_all(dir) {
                self.status = format!("Scratch append failed: {err}");
                return false;
            }
        }

        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let needs_header =
            !path.exists() || fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);
        let entry = if needs_header {
            format!("# Scratch\n\n## {now}\n\n{text}\n")
        } else {
            format!("\n\n## {now}\n\n{text}\n")
        };

        match fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => match file.write_all(entry.as_bytes()) {
                Ok(_) => {
                    self.status = format!("Appended to {}", path.display());
                    true
                }
                Err(err) => {
                    self.status = format!("Scratch append failed: {err}");
                    false
                }
            },
            Err(err) => {
                self.status = format!("Scratch append failed: {err}");
                false
            }
        }
    }

    fn append_to_scratch_archive(&mut self) {
        if !self.scratch
            || self.path.is_some()
            || !self.dirty
            || self.buffer.as_str().trim().is_empty()
        {
            return;
        }

        let text = self.buffer.as_str().to_string();
        if self.append_text_to_scratch_archive(&text) {
            self.dirty = false;
        }
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
        if self.append_text_to_scratch_archive(&text) {
            self.scratch_buffer.clear();
            self.scratch_modal_open = false;
            self.focus_editor_once = true;
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

    fn parse_scratch_entries(text: &str) -> Vec<ScratchEntry> {
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
            if line.trim() == "# Scratch"
                && !seen_entry
                && heading.is_none()
                && body.trim().is_empty()
            {
                continue;
            }
            if let Some(rest) = line.strip_prefix("## ") {
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

    fn serialize_scratch_entries(entries: &[ScratchEntry]) -> String {
        let mut text = "# Scratch\n".to_string();
        for entry in entries {
            text.push_str("\n## ");
            text.push_str(entry.heading.as_deref().unwrap_or("untitled scratch"));
            text.push_str("\n\n");
            text.push_str(entry.body.trim_end());
            text.push('\n');
        }
        text
    }

    fn load_scratch_entries(&mut self) -> bool {
        let Some(path) = self.scratch_archive_path() else {
            return false;
        };
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.scratch_entries = Self::parse_scratch_entries(&text);
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
        let title = self.capture_title.trim();
        let entry = if title.is_empty() {
            self.capture_text.trim_end().to_string()
        } else {
            format!("**{title}**\n\n{}", self.capture_text.trim_end())
        };
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
        match fs::write(
            &path,
            Self::serialize_scratch_entries(&self.scratch_entries),
        ) {
            Ok(_) => self.status = format!("Deleted scratch entry {}", deleted.title()),
            Err(err) => self.status = format!("Scratch delete failed: {err}"),
        }
    }

    fn save_path(&mut self, path: PathBuf) {
        match fs::write(&path, self.buffer.as_str()) {
            Ok(_) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.remember_recent_file(path.clone());
                let _ = self.save_settings();
                self.status = format!("Saved {}", path.display());
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn new_buffer(&mut self) {
        self.buffer.clear();
        self.path = None;
        self.dirty = false;
        self.search_state = None;
        self.status = "New buffer".to_string();
    }

    fn open_dialog(&mut self) {
        self.open_file_picker_for_open();
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
        let query = self.recent_query.trim().to_lowercase();
        if query.is_empty() {
            return (0..self.recent_files.len()).collect();
        }

        let mut scored = self
            .recent_files
            .iter()
            .enumerate()
            .filter_map(|(index, path)| {
                let display = path.display().to_string();
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                let display_score = Self::fuzzy_score(&display, &query);
                let name_score = Self::fuzzy_score(file_name, &query).map(|score| score / 2);
                display_score
                    .into_iter()
                    .chain(name_score)
                    .min()
                    .map(|score| (score, index))
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|(score, index)| (*score, *index));
        scored.into_iter().map(|(_, index)| index).collect()
    }

    fn move_recent_selection(&mut self, delta: isize) {
        let indices = self.recent_file_indices();
        if indices.is_empty() {
            self.selected_recent_file = 0;
            return;
        }
        let current_position = indices
            .iter()
            .position(|index| *index == self.selected_recent_file)
            .unwrap_or(0);
        let next_position = current_position
            .saturating_add_signed(delta)
            .min(indices.len().saturating_sub(1));
        self.selected_recent_file = indices[next_position];
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
                let haystack = format!("{} {} {}", task.line_index + 1, state, task.text);
                Self::fuzzy_score(&haystack, &query).map(|score| (score, index))
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

    fn project_root(&self) -> PathBuf {
        self.path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or_else(dirs_next::home_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn should_skip_file_dir(name: &str) -> bool {
        matches!(
            name,
            ".git"
                | "target"
                | "node_modules"
                | ".threadwell"
                | ".idea"
                | ".vscode"
                | "dist"
                | "build"
        )
    }

    fn format_file_size(bytes: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;
        let bytes = bytes as f64;
        if bytes >= GB {
            format!("{:.1}G", bytes / GB)
        } else if bytes >= MB {
            format!("{:.1}M", bytes / MB)
        } else if bytes >= KB {
            format!("{:.1}K", bytes / KB)
        } else {
            format!("{}B", bytes as u64)
        }
    }

    fn format_modified_time(modified: SystemTime) -> String {
        let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
            return "now".to_string();
        };
        let seconds = elapsed.as_secs();
        let minute = 60;
        let hour = minute * 60;
        let day = hour * 24;
        let week = day * 7;
        let month = day * 30;
        let year = day * 365;

        if seconds < minute {
            "now".to_string()
        } else if seconds < hour {
            format!("{}m ago", seconds / minute)
        } else if seconds < day {
            format!("{}h ago", seconds / hour)
        } else if seconds < week {
            format!("{}d ago", seconds / day)
        } else if seconds < month {
            format!("{}w ago", seconds / week)
        } else if seconds < year {
            format!("{}mo ago", seconds / month)
        } else {
            format!("{}y ago", seconds / year)
        }
    }

    fn file_metadata_labels(path: &std::path::Path) -> (String, String) {
        let Ok(metadata) = fs::metadata(path) else {
            return ("?".to_string(), "?".to_string());
        };
        let size = Self::format_file_size(metadata.len());
        let modified = metadata
            .modified()
            .map(Self::format_modified_time)
            .unwrap_or_else(|_| "?".to_string());
        (size, modified)
    }

    fn scan_directory_entries(dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut entries = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if path.is_dir() {
                    !Self::should_skip_file_dir(name)
                } else {
                    path.is_file()
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|path| (!path.is_dir(), path.file_name().map(|name| name.to_owned())));
        entries
    }

    fn open_file_picker_for_open(&mut self) {
        self.open_file_picker_at(self.project_root(), FilePickerMode::Open);
    }

    fn open_file_picker_at(&mut self, dir: PathBuf, mode: FilePickerMode) {
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.save_as_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.command_history_index = None;
        self.file_picker_open = true;
        self.file_picker_mode = mode;
        self.file_picker_dir = dir;
        self.file_query.clear();
        self.project_files = Self::scan_directory_entries(&self.file_picker_dir);
        self.selected_project_file = self.project_file_indices().first().copied().unwrap_or(0);
        self.status = format!("Files: {}", self.file_picker_dir.display());
        self.focus_editor_once = false;
    }

    fn project_file_indices(&self) -> Vec<usize> {
        let query = self.file_query.trim().to_lowercase();
        let mut indices = if query.is_empty() {
            (0..self.project_files.len()).collect::<Vec<_>>()
        } else {
            let mut scored = self
                .project_files
                .iter()
                .enumerate()
                .filter_map(|(index, path)| {
                    let relative = path
                        .strip_prefix(&self.file_picker_dir)
                        .unwrap_or(path)
                        .display()
                        .to_string();
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("");
                    let relative_score = Self::fuzzy_score(&relative, &query);
                    let name_score = Self::fuzzy_score(name, &query).map(|score| score / 2);
                    relative_score
                        .into_iter()
                        .chain(name_score)
                        .min()
                        .map(|score| (score, index))
                })
                .collect::<Vec<_>>();
            scored.sort_by_key(|(score, index)| (*score, *index));
            scored
                .into_iter()
                .map(|(_, index)| index)
                .collect::<Vec<_>>()
        };
        indices.sort_by_key(|index| !self.project_files[*index].is_dir());
        indices
    }

    fn move_project_file_selection(&mut self, delta: isize) {
        let indices = self.project_file_indices();
        if indices.is_empty() {
            self.selected_project_file = 0;
            return;
        }
        let current_position = indices
            .iter()
            .position(|index| *index == self.selected_project_file)
            .unwrap_or(0);
        let next_position = current_position
            .saturating_add_signed(delta)
            .min(indices.len().saturating_sub(1));
        self.selected_project_file = indices[next_position];
    }

    fn centered_window_start(
        selected_position: usize,
        visible_rows: usize,
        total_rows: usize,
    ) -> usize {
        if total_rows <= visible_rows {
            return 0;
        }
        selected_position
            .saturating_sub(visible_rows / 2)
            .min(total_rows.saturating_sub(visible_rows))
    }

    fn refresh_file_picker_entries(&mut self) {
        self.project_files = Self::scan_directory_entries(&self.file_picker_dir);
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
            self.file_picker_dir = path;
            self.file_query.clear();
            self.refresh_file_picker_entries();
            return;
        }
        if self.dirty && self.file_picker_mode == FilePickerMode::Open {
            self.pending_project_file_path = Some(path);
            self.confirm(PendingAction::OpenProjectFile);
        } else {
            self.file_picker_open = false;
            self.open_path(path);
            self.focus_editor_once = true;
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
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        self.save_as_filename = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("untitled.md")
            .to_string();
        self.save_as_entries = Self::scan_directory_entries(&self.save_as_dir);
        self.selected_save_as_entry = self.save_as_entry_indices().first().copied().unwrap_or(0);
        self.save_as_open = true;
        self.focus_editor_once = false;
        self.status = format!("Save as: {}", self.save_as_dir.display());
    }

    fn save_as_entry_indices(&self) -> Vec<usize> {
        let query = self.save_as_filename.trim().to_lowercase();
        let mut indices = if query.is_empty() {
            (0..self.save_as_entries.len()).collect::<Vec<_>>()
        } else {
            let mut scored = self
                .save_as_entries
                .iter()
                .enumerate()
                .filter_map(|(index, path)| {
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("");
                    Self::fuzzy_score(name, &query).map(|score| (score, index))
                })
                .collect::<Vec<_>>();
            scored.sort_by_key(|(score, index)| (*score, *index));
            scored
                .into_iter()
                .map(|(_, index)| index)
                .collect::<Vec<_>>()
        };
        indices.sort_by_key(|index| !self.save_as_entries[*index].is_dir());
        indices
    }

    fn refresh_save_as_entries(&mut self) {
        self.save_as_entries = Self::scan_directory_entries(&self.save_as_dir);
        self.selected_save_as_entry = self.save_as_entry_indices().first().copied().unwrap_or(0);
        self.status = format!("Save as: {}", self.save_as_dir.display());
    }

    fn move_save_as_selection(&mut self, delta: isize) {
        let indices = self.save_as_entry_indices();
        if indices.is_empty() {
            self.selected_save_as_entry = 0;
            return;
        }
        let current_position = indices
            .iter()
            .position(|index| *index == self.selected_save_as_entry)
            .unwrap_or(0);
        let next_position = current_position
            .saturating_add_signed(delta)
            .min(indices.len().saturating_sub(1));
        self.selected_save_as_entry = indices[next_position];
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
                        self.command_history_limit = limit.clamp(1, 50);
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
                "last_opened_path" => {
                    let value = Self::parse_config_string(value);
                    if !value.is_empty() {
                        self.last_opened_path = Some(PathBuf::from(value));
                    }
                }
                "command_history" => {
                    let value = Self::parse_config_string(value);
                    if !value.is_empty() && self.command_history.last() != Some(&value) {
                        self.command_history.push(value);
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
                "command_usage" => {
                    let value = Self::parse_config_string(value);
                    let mut parts = value.split('|');
                    let Some(name) = parts.next() else {
                        continue;
                    };
                    let Some(count) = parts.next().and_then(|value| value.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    let Some(last_used) = parts.next().and_then(|value| value.parse::<i64>().ok())
                    else {
                        continue;
                    };
                    let Some(name) = Self::canonical_command_name(name) else {
                        continue;
                    };
                    if let Some(existing) = self
                        .command_usage
                        .iter_mut()
                        .find(|usage| usage.name == name)
                    {
                        existing.count = existing.count.max(count);
                        existing.last_used = existing.last_used.max(last_used);
                    } else {
                        self.command_usage.push(CommandUsage {
                            name: name.to_string(),
                            count,
                            last_used,
                        });
                    }
                }
                _ => {}
            }
        }
        if self.command_history.len() > self.command_history_limit {
            let keep_from = self.command_history.len() - self.command_history_limit;
            self.command_history.drain(0..keep_from);
        }
        self.recent_files.truncate(20);
        self.command_usage.sort_by_key(|usage| {
            (
                std::cmp::Reverse(usage.last_used),
                std::cmp::Reverse(usage.count),
                usage.name.clone(),
            )
        });
        self.command_usage.truncate(100);
    }

    fn parse_config_bool(value: &str) -> Option<bool> {
        match value.trim().trim_matches('"').to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        }
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
            "command_history_limit = {}\nline_number_mode = \"{}\"\nword_wrap = {}\npreview_mode = {}\nctrl_shift_move_mode = \"{}\"\nreopen_last_file_on_startup = {}\nmarkdown_live_rendering = {}\nlast_opened_path = \"{}\"\n",
            self.command_history_limit,
            self.line_number_mode.config_value(),
            self.wrap,
            self.preview,
            self.ctrl_shift_move_mode.config_value(),
            self.reopen_last_file_on_startup,
            self.markdown_live_rendering,
            Self::escape_config_string(
                &self
                    .last_opened_path
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
        self.command_history_limit = limit.clamp(1, 50);
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

    fn command_name(command: Command) -> &'static str {
        match command {
            Command::New => "new",
            Command::Open => "open",
            Command::Save => "save",
            Command::SaveAs => "save-as",
            Command::Scratch => "scratch",
            Command::ScratchEntries => "scratch-entries",
            Command::Capture => "capture",
            Command::TogglePreview => "preview",
            Command::ToggleWrap => "wrap",
            Command::Settings => "settings",
            Command::LineNumbersAbsolute => "line-numbers-absolute",
            Command::LineNumbersRelative => "line-numbers-relative",
            Command::WrapOn => "wrap-on",
            Command::WrapOff => "wrap-off",
            Command::PreviewOn => "preview-on",
            Command::PreviewOff => "preview-off",
            Command::DocTasks => "doc-tasks",
            Command::Quit => "quit",
        }
    }

    fn now_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default()
    }

    fn canonical_command_name(token: &str) -> Option<&'static str> {
        let token = token.trim_start_matches(':');
        COMMAND_SPECS
            .iter()
            .find(|spec| spec.name == token || spec.aliases.contains(&token))
            .map(|spec| spec.name)
    }

    fn record_command_usage(&mut self, token: &str) {
        let Some(name) = Self::canonical_command_name(token) else {
            return;
        };
        let now = Self::now_timestamp();
        if let Some(usage) = self
            .command_usage
            .iter_mut()
            .find(|usage| usage.name == name)
        {
            usage.count = usage.count.saturating_add(1);
            usage.last_used = now;
        } else {
            self.command_usage.push(CommandUsage {
                name: name.to_string(),
                count: 1,
                last_used: now,
            });
        }
        self.command_usage.sort_by_key(|usage| {
            (
                std::cmp::Reverse(usage.last_used),
                std::cmp::Reverse(usage.count),
                usage.name.clone(),
            )
        });
        self.command_usage.truncate(100);
        let _ = self.save_settings();
    }

    fn command_usage(&self, name: &str) -> Option<&CommandUsage> {
        self.command_usage.iter().find(|usage| usage.name == name)
    }

    fn command_usage_boost(&self, name: &str) -> usize {
        let Some(usage) = self.command_usage(name) else {
            return 0;
        };
        let count_boost = usage.count.min(30);
        let age_seconds = Self::now_timestamp().saturating_sub(usage.last_used);
        let recency_boost = if age_seconds <= 60 * 60 {
            30
        } else if age_seconds <= 24 * 60 * 60 {
            20
        } else if age_seconds <= 7 * 24 * 60 * 60 {
            10
        } else {
            0
        };
        count_boost + recency_boost
    }

    fn run_command(&mut self, command: Command, ctx: &egui::Context) {
        self.record_command_usage(Self::command_name(command));
        self.palette_open = false;
        self.recent_picker_open = false;
        self.file_picker_open = false;
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
        let input = self
            .command_line
            .trim_start()
            .strip_prefix(':')
            .unwrap_or(self.command_line.trim_start());
        if input.is_empty() || input.contains(char::is_whitespace) {
            return None;
        }
        Some(input)
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
        Self::best_command_token(spec, prefix)
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
        let Some(candidate) = Self::best_command_token(spec, &prefix) else {
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
        let query = prefix.trim_start_matches(':').to_lowercase();
        if query.is_empty() {
            let mut specs = COMMAND_SPECS.iter().collect::<Vec<_>>();
            specs.sort_by_key(|spec| {
                let boost = self.command_usage_boost(spec.name);
                (std::cmp::Reverse(boost), spec.name.len(), spec.name)
            });
            return specs.into_iter().take(limit).collect();
        }

        let mut scored = COMMAND_SPECS
            .iter()
            .filter_map(|spec| Self::command_spec_score(spec, &query).map(|score| (score, spec)))
            .collect::<Vec<_>>();
        scored.sort_by_key(|(score, spec)| {
            let boost = self.command_usage_boost(spec.name);
            let adjusted_score = (*score as isize * 100) - boost as isize;
            (*score / 100, adjusted_score, spec.name.len(), spec.name)
        });
        scored
            .into_iter()
            .map(|(_, spec)| spec)
            .take(limit)
            .collect()
    }

    fn command_spec_score(spec: &CommandSpec, query: &str) -> Option<usize> {
        let mut best = Self::fuzzy_score(spec.name, query);
        for alias in spec.aliases {
            best = best
                .into_iter()
                .chain(Self::fuzzy_score(alias, query))
                .min();
        }
        best
    }

    fn best_command_token(spec: &CommandSpec, prefix: &str) -> Option<&'static str> {
        let query = prefix.to_lowercase();
        std::iter::once(spec.name)
            .chain(spec.aliases.iter().copied())
            .filter_map(|token| Self::fuzzy_score(token, &query).map(|score| (score, token)))
            .min_by_key(|(score, token)| (*score, token.len()))
            .map(|(_, token)| token)
    }

    fn fuzzy_score(candidate: &str, query: &str) -> Option<usize> {
        if query.is_empty() {
            return Some(0);
        }
        let candidate = candidate.to_lowercase();
        if candidate == query {
            return Some(0);
        }
        if candidate.starts_with(query) {
            return Some(1 + candidate.len().saturating_sub(query.len()));
        }
        if candidate.contains(query) {
            return Some(100 + candidate.find(query).unwrap_or(0));
        }

        let mut score = 200usize;
        let mut last_match = None;
        let mut chars = candidate.char_indices();
        for query_ch in query.chars() {
            let Some((index, _)) = chars.find(|(_, candidate_ch)| *candidate_ch == query_ch) else {
                return None;
            };
            if let Some(last) = last_match {
                score += index.saturating_sub(last + 1);
            }
            last_match = Some(index);
        }
        Some(score + candidate.len().saturating_sub(query.len()))
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

        if self.command_history.last().is_none_or(|last| last != input) {
            self.command_history.push(input.to_string());
            if self.command_history.len() > self.command_history_limit {
                let keep_from = self.command_history.len() - self.command_history_limit;
                self.command_history.drain(0..keep_from);
            }
            let _ = self.save_settings();
        }

        let mut parts = input.split_whitespace();
        let Some(command) = parts.next() else {
            self.status = "Command cancelled".to_string();
            return;
        };

        match command {
            "w" | "write" | "save" => self.run_command(Command::Save, ctx),
            "save-as" | "saveas" | "write-as" => self.run_command(Command::SaveAs, ctx),
            "scratch" | "sc" => self.run_command(Command::Scratch, ctx),
            "scratch-entries" | "scratch-log" | "scl" => {
                self.run_command(Command::ScratchEntries, ctx)
            }
            "capture" | "cap" => self.run_command(Command::Capture, ctx),
            "q" | "quit" | "exit" => self.run_command(Command::Quit, ctx),
            "wq" | "x" => {
                self.record_command_usage("wq");
                self.save();
                if !self.dirty {
                    self.run_command(Command::Quit, ctx);
                }
            }
            "new" | "enew" => self.run_command(Command::New, ctx),
            "open" | "edit" | "e" => {
                let path = parts.collect::<Vec<_>>().join(" ");
                if path.is_empty() {
                    self.run_command(Command::Open, ctx);
                } else if self.dirty {
                    self.status = "Save or discard changes before opening another file".to_string();
                } else {
                    self.record_command_usage("open");
                    let expanded = path
                        .strip_prefix("~/")
                        .and_then(|rest| dirs_next::home_dir().map(|home| home.join(rest)))
                        .unwrap_or_else(|| PathBuf::from(path));
                    self.open_path(expanded);
                }
            }
            "open-last" | "last" | "ol" => {
                self.record_command_usage("open-last");
                if self.dirty {
                    self.confirm(PendingAction::OpenLast);
                } else {
                    self.open_last();
                }
            }
            "recent" | "rec" => {
                self.record_command_usage("recent");
                let query = parts.collect::<Vec<_>>().join(" ");
                self.open_recent_picker_with_query(query);
            }
            "preview" | "md" => {
                let arg = parts.next();
                match arg.and_then(Self::parse_config_bool) {
                    Some(enabled) => {
                        self.record_command_usage(if enabled {
                            "preview-on"
                        } else {
                            "preview-off"
                        });
                        self.set_preview_mode(enabled);
                    }
                    None => self.run_command(Command::TogglePreview, ctx),
                }
            }
            "preview-on" | "md-on" => self.run_command(Command::PreviewOn, ctx),
            "preview-off" | "md-off" => self.run_command(Command::PreviewOff, ctx),
            "doc-tasks" | "tasks" => self.run_command(Command::DocTasks, ctx),
            "wrap" => {
                let arg = parts.next();
                match arg.and_then(Self::parse_config_bool) {
                    Some(enabled) => {
                        self.record_command_usage(if enabled { "wrap-on" } else { "wrap-off" });
                        self.set_wrap_mode(enabled);
                    }
                    None => self.run_command(Command::ToggleWrap, ctx),
                }
            }
            "wrap-on" | "wrap-enable" => self.run_command(Command::WrapOn, ctx),
            "wrap-off" | "nowrap" | "wrap-disable" => self.run_command(Command::WrapOff, ctx),
            "find" | "f" => {
                self.record_command_usage("find");
                let query = parts.collect::<Vec<_>>().join(" ");
                self.start_search(query);
            }
            "goto" | "g" | "line" | "l" => {
                self.record_command_usage("goto");
                let target = parts.collect::<Vec<_>>().join(" ");
                self.goto_target(&target);
            }
            "select-word" | "sw" => {
                self.record_command_usage("select-word");
                self.select_word();
            }
            "select-line" | "sl" => {
                self.record_command_usage("select-line");
                self.select_line();
            }
            "delete-word" | "dw" => {
                self.record_command_usage("delete-word");
                self.delete_word();
            }
            "delete-line" | "dl" => {
                self.record_command_usage("delete-line");
                self.delete_current_line();
            }
            "duplicate-line" | "dup" => {
                self.record_command_usage("duplicate-line");
                self.duplicate_current_line();
            }
            "duplicate-place" | "dupp" => {
                self.record_command_usage("duplicate-place");
                self.start_duplicate_placement();
            }
            "move-line-up" | "mlu" => {
                self.record_command_usage("move-line-up");
                self.move_current_line_up();
            }
            "move-line-down" | "mld" => {
                self.record_command_usage("move-line-down");
                self.move_current_line_down();
            }
            "move-line-to-paragraph-start" | "mlps" => {
                self.record_command_usage("move-line-to-paragraph-start");
                self.move_current_line_to_paragraph_start();
            }
            "move-line-to-paragraph-end" | "mlpe" => {
                self.record_command_usage("move-line-to-paragraph-end");
                self.move_current_line_to_paragraph_end();
            }
            "top" | "go-top" | "gt" => {
                self.record_command_usage("top");
                self.go_to_top();
            }
            "bottom" | "go-bottom" | "gb" => {
                self.record_command_usage("bottom");
                self.go_to_bottom();
            }
            "line-numbers" | "ln" | "linenumbers" => {
                let Some(mode) = parts.next() else {
                    self.status = format!("Line numbers: {}", self.line_number_mode.label());
                    return;
                };
                match LineNumberMode::from_config_value(mode) {
                    Some(LineNumberMode::Absolute) => {
                        self.record_command_usage("line-numbers-absolute");
                        self.set_line_number_mode(LineNumberMode::Absolute);
                    }
                    Some(LineNumberMode::Relative) => {
                        self.record_command_usage("line-numbers-relative");
                        self.set_line_number_mode(LineNumberMode::Relative);
                    }
                    None => self.status = "Usage: :line-numbers absolute|relative".to_string(),
                }
            }
            "line-numbers-absolute" | "ln-abs" => {
                self.run_command(Command::LineNumbersAbsolute, ctx)
            }
            "line-numbers-relative" | "ln-rel" => {
                self.run_command(Command::LineNumbersRelative, ctx)
            }
            "settings" | "set" | "prefs" | "preferences" => {
                self.run_command(Command::Settings, ctx)
            }
            _ => self.status = format!("Unknown command: {input}"),
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
                    self.open_path(path);
                }
            }
            PendingAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn request_close(&mut self, ctx: &egui::Context) {
        if self.dirty && !self.scratch {
            self.confirm(PendingAction::Quit);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.scratch {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm(PendingAction::Quit);
        }
    }

    fn layer_allowed(&self) -> bool {
        !self.command_line_focused
            && !self.focus_command_line_once
            && !self.palette_open
            && !self.settings_open
            && !self.recent_picker_open
            && !self.doc_tasks_open
            && !self.file_picker_open
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
            && !self.doc_tasks_open
            && !self.file_picker_open
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
        let mut doc_task_previous = false;
        let mut doc_task_next = false;
        let mut doc_task_open = false;
        let mut doc_task_cycle = false;
        let mut doc_task_backspace = false;
        let mut file_previous = false;
        let mut file_next = false;
        let mut file_open = false;
        let mut file_enter_dir = false;
        let mut file_parent = false;
        let mut file_backspace = false;
        let mut save_as_previous = false;
        let mut save_as_next = false;
        let mut save_as_enter = false;
        let mut save_as_enter_dir = false;
        let mut save_as_parent = false;
        let mut save_as_backspace = false;
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
            && !self.doc_tasks_open
            && !self.file_picker_open
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
            if self.file_picker_open {
                file_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                file_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                file_enter_dir |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                file_parent |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                file_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                file_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
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
                self.doc_tasks_open = false;
                self.file_picker_open = false;
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
                self.doc_tasks_open = false;
                self.file_picker_open = false;
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
            let save_pressed = !self.scratch_modal_open
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
            let save_as_pressed = !self.scratch_modal_open
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
                } else if self.doc_tasks_open {
                    self.doc_tasks_open = false;
                    self.focus_editor_once = true;
                } else if self.file_picker_open {
                    self.file_picker_open = false;
                    self.pending_project_file_path = None;
                    self.focus_editor_once = true;
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
                    self.focus_editor_once = true;
                } else if self.scratch {
                    command = Some(Command::Quit);
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

        if self.doc_tasks_open {
            self.handle_doc_tasks_text_input(ctx);
        }

        if self.file_picker_open {
            self.handle_file_picker_text_input(ctx);
        }

        if self.save_as_open {
            self.handle_save_as_text_input(ctx);
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

        if settings_previous {
            self.selected_setting = self.selected_setting.saturating_sub(1);
            return;
        }

        if settings_next {
            self.selected_setting = (self.selected_setting + 1).min(6);
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
                6 => self.set_reopen_last_file_on_startup(false),
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
                6 => self.set_reopen_last_file_on_startup(!self.reopen_last_file_on_startup),
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
                                let reopen_selected = self.selected_setting == 6;
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
        let start = Self::centered_window_start(selected_position, visible_rows, matches.len());
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
                            let title = if self.file_picker_mode == FilePickerMode::Open {
                                "open"
                            } else {
                                "files"
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
                                Self::file_metadata_labels(path)
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
                                    None,
                                    false,
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
                            let start = Self::centered_window_start(
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
        let start = Self::centered_window_start(selected_position, visible_rows, matches.len());
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
                                Self::file_metadata_labels(path)
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
            let mut in_code = false;
            let mut code_language = String::new();
            for line in self.buffer.as_str().lines() {
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
                } else if !Self::inline_code_preview_label(ui, line) {
                    ui.label(RichText::new(line).size(15.0));
                }
            }
        });
    }
}

impl eframe::App for SlateApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.scratch_modal_open || !self.scratch_buffer.as_str().trim().is_empty() {
            self.archive_scratch_modal();
        }
        self.append_to_scratch_archive();
        let _ = self.save_settings();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));
        self.handle_window_close_request(&ctx);
        self.shortcuts(&ctx);

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
                let recent_file_indices = if self.recent_picker_open {
                    self.recent_file_indices()
                } else {
                    Vec::new()
                };
                let visible_recent_rows = if self.recent_picker_open {
                    recent_file_indices.len().min(8)
                } else {
                    0
                };
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
                    && !self.doc_tasks_open
                    && !self.file_picker_open
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
                                    active_line_text_highlight,
                                    false,
                                );
                                if self.focus_editor_once
                                    && !self.palette_open
                                    && !self.settings_open
                                    && !self.recent_picker_open
                                    && !self.doc_tasks_open
                                    && !self.file_picker_open
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
                                }
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
                                active_line_text_highlight,
                                self.markdown_live_rendering,
                            );
                            if self.focus_editor_once
                                && !self.palette_open
                                && !self.settings_open
                                && !self.recent_picker_open
                                && !self.doc_tasks_open
                                && !self.file_picker_open
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
                            }
                        }
                    },
                );

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
                } else if self.doc_tasks_open {
                    "doc tasks"
                } else if self.file_picker_open {
                    "files"
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
                    } else if self.file_picker_open {
                        ("files  type filter · ↑↓ select · → enter folder · ← parent · Enter open · Esc close".to_string(), footer_accent)
                    } else if self.save_as_open {
                        ("save as  type file name · ↑↓ select · → enter folder · ← parent · Enter save · Esc close".to_string(), footer_accent)
                    } else if self.scratch_modal_open {
                        ("scratch  Ctrl+S archive · Ctrl+E entries · Esc hide · :scratch resume".to_string(), footer_accent)
                    } else if self.scratch_entries_open {
                        ("scratch entries  ↑↓ select · Ctrl+D/Delete delete · Esc close".to_string(), footer_accent)
                    } else if self.capture_modal_open {
                        ("capture  type optional title · Enter archive · Esc cancel".to_string(), footer_accent)
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
        self.file_picker_dialog(&ctx);
        self.save_as_dialog(&ctx);
        self.scratch_modal_dialog(&ctx);
        self.capture_dialog(&ctx);
        self.scratch_entries_dialog(&ctx);
    }
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
