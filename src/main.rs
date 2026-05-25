mod editor_buffer;
mod editor_view;
mod goto;
mod search;

use std::{fs, io::Write, path::PathBuf};

use editor_buffer::EditorBuffer;
use editor_view::{EditorView, LineNumberMode};
use eframe::egui::{self, Color32, FontFamily, FontId, Key, RichText, Stroke, TextEdit, Vec2};
use goto::GotoTarget;
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
        [980.0, 700.0]
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
    TogglePreview,
    ToggleWrap,
    Settings,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    New,
    Open,
    Quit,
}

impl PendingAction {
    fn prompt(self) -> &'static str {
        match self {
            PendingAction::New => "buffer has unsaved changes; start a new buffer anyway?",
            PendingAction::Open => "buffer has unsaved changes; open another file anyway?",
            PendingAction::Quit => "buffer has unsaved changes; close anyway?",
        }
    }
}

impl Command {
    fn label(self) -> &'static str {
        match self {
            Command::New => "New buffer",
            Command::Open => "Open file",
            Command::Save => "Save",
            Command::TogglePreview => "Toggle Markdown preview",
            Command::ToggleWrap => "Toggle word wrap",
            Command::Settings => "Settings",
            Command::Quit => "Quit",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Command::New => "Ctrl+N",
            Command::Open => "Ctrl+O",
            Command::Save => "Ctrl+S",
            Command::TogglePreview => "Ctrl+M",
            Command::ToggleWrap => "",
            Command::Settings => ":settings",
            Command::Quit => "Ctrl+Q",
        }
    }
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
    command_line_focused: bool,
    focus_command_line_once: bool,
    command_history: Vec<String>,
    command_history_index: Option<usize>,
    command_history_limit: usize,
    line_number_mode: LineNumberMode,
    search_state: Option<SearchState>,
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
            command_line_focused: false,
            focus_command_line_once: false,
            command_history: Vec::new(),
            command_history_index: None,
            command_history_limit: 5,
            line_number_mode: LineNumberMode::Absolute,
            search_state: None,
        };

        app.load_settings();

        if let Some(path) = path {
            app.open_path(path);
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

    fn append_to_scratch_archive(&mut self) {
        if !self.scratch
            || self.path.is_some()
            || !self.dirty
            || self.buffer.as_str().trim().is_empty()
        {
            return;
        }

        let Some(mut dir) = dirs_next::data_dir() else {
            self.status = "Scratch append failed: no data dir".to_string();
            return;
        };
        dir.push("slate");

        if let Err(err) = fs::create_dir_all(&dir) {
            self.status = format!("Scratch append failed: {err}");
            return;
        }

        let path = dir.join("scratch.md");
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let needs_header =
            !path.exists() || fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);
        let entry = if needs_header {
            format!(
                "# Scratch\n\n## {now}\n\n{}\n",
                self.buffer.as_str().trim_end()
            )
        } else {
            format!("\n\n## {now}\n\n{}\n", self.buffer.as_str().trim_end())
        };

        match fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => match file.write_all(entry.as_bytes()) {
                Ok(_) => {
                    self.dirty = false;
                    self.status = format!("Appended to {}", path.display());
                }
                Err(err) => self.status = format!("Scratch append failed: {err}"),
            },
            Err(err) => self.status = format!("Scratch append failed: {err}"),
        }
    }

    fn save_path(&mut self, path: PathBuf) {
        match fs::write(&path, self.buffer.as_str()) {
            Ok(_) => {
                self.path = Some(path.clone());
                self.dirty = false;
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
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            self.open_path(path);
        }
    }

    fn save_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new().save_file() {
            self.save_path(path);
        }
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
                _ => {}
            }
        }
    }

    fn save_settings(&self) -> Result<(), String> {
        let Some(path) = Self::settings_path() else {
            return Err("no config dir".to_string());
        };
        let parent = path
            .parent()
            .ok_or_else(|| "invalid config path".to_string())?;
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        fs::write(
            path,
            format!(
                "command_history_limit = {}\nline_number_mode = \"{}\"\n",
                self.command_history_limit,
                self.line_number_mode.config_value()
            ),
        )
        .map_err(|err| err.to_string())
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

    fn run_command(&mut self, command: Command, ctx: &egui::Context) {
        self.palette_open = false;
        self.palette_query.clear();
        self.selected_command = 0;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
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
            Command::TogglePreview => {
                self.preview = !self.preview;
                self.status = if self.preview {
                    "Preview on"
                } else {
                    "Preview off"
                }
                .to_string();
            }
            Command::ToggleWrap => {
                self.wrap = !self.wrap;
                self.status = if self.wrap {
                    "Word wrap on"
                } else {
                    "Word wrap off"
                }
                .to_string();
            }
            Command::Settings => {
                self.settings_open = true;
                self.selected_setting = 0;
                self.focus_editor_once = false;
            }
            Command::Quit => self.request_close(ctx),
        }
    }

    fn run_command_line(&mut self, ctx: &egui::Context) {
        let raw = self.command_line.trim().to_string();
        self.command_line.clear();
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        self.focus_editor_once = true;
        self.command_history_index = None;

        let input = raw.strip_prefix(':').unwrap_or(&raw).trim();
        if input.is_empty() {
            self.status = "Command cancelled".to_string();
            return;
        }

        if self.command_history.last().is_none_or(|last| last != input) {
            self.command_history.push(input.to_string());
        }

        let mut parts = input.split_whitespace();
        let Some(command) = parts.next() else {
            self.status = "Command cancelled".to_string();
            return;
        };

        match command {
            "w" | "write" | "save" => self.run_command(Command::Save, ctx),
            "q" | "quit" | "exit" => self.run_command(Command::Quit, ctx),
            "wq" | "x" => {
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
                    let expanded = path
                        .strip_prefix("~/")
                        .and_then(|rest| dirs_next::home_dir().map(|home| home.join(rest)))
                        .unwrap_or_else(|| PathBuf::from(path));
                    self.open_path(expanded);
                }
            }
            "preview" | "md" => self.run_command(Command::TogglePreview, ctx),
            "wrap" => self.run_command(Command::ToggleWrap, ctx),
            "find" | "f" => {
                let query = parts.collect::<Vec<_>>().join(" ");
                self.start_search(query);
            }
            "goto" | "g" | "line" | "l" => {
                let target = parts.collect::<Vec<_>>().join(" ");
                self.goto_target(&target);
            }
            "settings" | "set" | "prefs" | "preferences" => {
                self.run_command(Command::Settings, ctx)
            }
            _ => self.status = format!("Unknown command: {input}"),
        }
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

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let mut command = None;
        let mut execute_command_line = false;
        let mut previous_command = false;
        let mut next_command = false;
        let mut settings_decrement = false;
        let mut settings_increment = false;
        let mut settings_previous = false;
        let mut settings_next = false;
        let mut settings_activate = false;
        let mut search_next = false;
        let mut search_previous = false;
        let mut search_accept = false;
        let mut search_cancel = false;
        let mut search_cursor_after = false;
        let mut search_cursor_before = false;
        let search_active = self.search_state.is_some()
            && !self.command_line_focused
            && !self.focus_command_line_once
            && !self.palette_open
            && !self.settings_open
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
            if i.consume_key(egui::Modifiers::CTRL, Key::P) {
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
                self.command_history_index = None;
                self.command_line_focused = true;
                self.focus_command_line_once = true;
                self.focus_editor_once = false;
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::Period) {
                self.palette_open = false;
                self.command_line.clear();
                self.command_history_index = None;
                self.command_line_focused = true;
                self.focus_command_line_once = true;
                self.focus_editor_once = false;
            }
            if self.command_line_focused || self.focus_command_line_once {
                execute_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                execute_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                previous_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                next_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            } else if search_active {
                search_next |= i.consume_key(egui::Modifiers::NONE, Key::F);
                search_previous |= i.consume_key(egui::Modifiers::NONE, Key::B);
                search_accept |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                search_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::N) {
                command = Some(Command::New);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::O) {
                command = Some(Command::Open);
            }
            let save_pressed = i.events.iter().any(|event| {
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
            if save_pressed {
                command = Some(Command::Save);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::M) {
                command = Some(Command::TogglePreview);
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::Q) {
                command = Some(Command::Quit);
            }
            if !search_cancel && i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                if self.settings_open {
                    self.settings_open = false;
                    self.focus_editor_once = true;
                } else if self.command_line_focused || self.focus_command_line_once {
                    self.command_line.clear();
                    self.command_line_focused = false;
                    self.focus_command_line_once = false;
                    self.command_history_index = None;
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

        if settings_previous {
            self.selected_setting = self.selected_setting.saturating_sub(1);
            return;
        }

        if settings_next {
            self.selected_setting = (self.selected_setting + 1).min(1);
            return;
        }

        if settings_decrement {
            match self.selected_setting {
                0 => self.set_command_history_limit(self.command_history_limit.saturating_sub(1)),
                1 => self.set_line_number_mode(LineNumberMode::Absolute),
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
                _ => {}
            }
            return;
        }

        if previous_command && !self.command_history.is_empty() {
            let index = self
                .command_history_index
                .unwrap_or(self.command_history.len())
                .saturating_sub(1);
            self.command_history_index = Some(index);
            self.command_line = self.command_history[index].clone();
            self.focus_command_line_once = true;
            return;
        }

        if next_command {
            if let Some(index) = self.command_history_index {
                if index + 1 < self.command_history.len() {
                    let index = index + 1;
                    self.command_history_index = Some(index);
                    self.command_line = self.command_history[index].clone();
                } else {
                    self.command_history_index = None;
                    self.command_line.clear();
                }
                self.focus_command_line_once = true;
            }
            return;
        }

        if execute_command_line {
            self.run_command_line(ctx);
            return;
        }

        if let Some(command) = command {
            self.run_command(command, ctx);
        }
    }

    fn filtered_commands(&self) -> Vec<Command> {
        let all = [
            Command::New,
            Command::Open,
            Command::Save,
            Command::TogglePreview,
            Command::ToggleWrap,
            Command::Settings,
            Command::Quit,
        ];
        let q = self.palette_query.to_lowercase();
        all.into_iter()
            .filter(|c| c.label().to_lowercase().contains(&q))
            .collect()
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
                                        RichText::new(command.label())
                                            .font(FontId::new(14.0, FontFamily::Monospace))
                                            .color(label_color),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(command.hint())
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
                            self.run_command(*command, ctx);
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
                            self.run_command(command, ctx);
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

    fn preview_ui(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut in_code = false;
            for line in self.buffer.as_str().lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("```") {
                    in_code = !in_code;
                    continue;
                }

                if in_code {
                    ui.label(
                        RichText::new(line)
                            .font(FontId::new(14.0, FontFamily::Monospace))
                            .background_color(Color32::from_rgb(25, 31, 40)),
                    );
                } else if let Some(h) = trimmed.strip_prefix("### ") {
                    ui.label(RichText::new(h).size(18.0).strong());
                } else if let Some(h) = trimmed.strip_prefix("## ") {
                    ui.label(RichText::new(h).size(22.0).strong());
                } else if let Some(h) = trimmed.strip_prefix("# ") {
                    ui.label(RichText::new(h).size(28.0).strong());
                } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                    ui.label(format!("• {}", &trimmed[2..]));
                } else if trimmed.is_empty() {
                    ui.add_space(8.0);
                } else {
                    ui.label(RichText::new(line).size(15.0));
                }
            }
        });
    }
}

impl eframe::App for SlateApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
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
                let command_history_active = (self.command_line_focused
                    || self.focus_command_line_once)
                    && !self.command_history.is_empty();
                let visible_history_rows = if command_history_active {
                    self.command_history.len().min(self.command_history_limit)
                } else {
                    0
                };
                let history_height = visible_history_rows as f32 * history_row_height;
                let footer_height = status_height + history_height + command_height;
                let editor_size = Vec2::new(
                    ui.available_width(),
                    (ui.available_height() - footer_height).max(80.0),
                );

                self.editor_view.observe_buffer(&self.buffer);

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
                                );
                                if self.focus_editor_once
                                    && !self.palette_open
                                    && !self.settings_open
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
                            );
                            if self.focus_editor_once
                                && !self.palette_open
                                && !self.settings_open
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
                let mode = if self.preview { "preview" } else { "edit" };
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
                    "[Ctrl+P]",
                    footer_font.clone(),
                    footer_accent,
                );
                status_right = shortcut_rect.left() - 12.0;
                painter.text(
                    egui::pos2(status_right, status_y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{mode} · {wrap} · {lines}l · {words}w · {chars}c"),
                    footer_font.clone(),
                    footer_dim,
                );

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
                let command_y = command_rect.center().y - 2.0;
                painter.text(
                    egui::pos2(command_rect.left() + 10.0, command_y),
                    egui::Align2::LEFT_CENTER,
                    ":",
                    footer_font.clone(),
                    footer_accent,
                );

                let input_rect = egui::Rect::from_min_max(
                    egui::pos2(command_rect.left() + 19.0, command_rect.top() + 4.0),
                    egui::pos2(command_rect.right() - 10.0, command_rect.bottom() - 4.0),
                );
                let command_line_active = self.command_line_focused || self.focus_command_line_once;
                if command_line_active {
                    let response = ui.put(
                        input_rect,
                        TextEdit::singleline(&mut self.command_line)
                            .hint_text(
                                RichText::new("command  w · q · wq · open <file> · preview · wrap")
                                    .font(footer_font.clone())
                                    .color(footer_dim),
                            )
                            .desired_width(f32::INFINITY)
                            .font(footer_font.clone())
                            .text_color(footer_color)
                            .frame(egui::Frame::NONE),
                    );
                    if self.focus_command_line_once {
                        response.request_focus();
                        self.focus_command_line_once = false;
                    }
                    self.command_line_focused = response.has_focus();
                } else {
                    painter.text(
                        egui::pos2(input_rect.left(), input_rect.center().y - 0.5),
                        egui::Align2::LEFT_CENTER,
                        "command  w · q · wq · open <file> · preview · wrap",
                        footer_font.clone(),
                        footer_dim,
                    );
                    self.command_line_focused = false;
                }
            });

        self.command_palette(&ctx);
        self.settings_dialog(&ctx);
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
