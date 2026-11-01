#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// egui allocates and frees many small vectors every frame.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(test)]
mod async_open_tests;
#[cfg(test)]
mod bug_fix_tests;
#[cfg(test)]
mod module_tests;
mod commands;
#[cfg(test)]
mod dialog_tests;
mod dialog_ui;
mod document_safety;
mod editor_buffer;
#[cfg(test)]
mod editor_look_tests;
mod editor_view;
mod file_dialogs;
mod file_loader;
mod find_note_ui;
#[cfg(test)]
mod find_note_ui_tests;
#[cfg(test)]
mod footer_tests;
mod footer_ui;
mod form_dialogs;
#[cfg(test)]
mod frame_bench_tests;
mod highlight;
#[cfg(test)]
mod large_document_tests;
mod notice_ui;
mod preview_layout;
#[cfg(test)]
mod preview_scroll_tests;
#[cfg(test)]
mod resident_tests;
#[cfg(test)]
mod resize_tests;
mod safe_file;
#[cfg(test)]
mod safety_integration_tests;
mod safety_ui;
mod scroll_sync;
#[cfg(test)]
mod settings_tests;
mod settings_writer;
#[cfg(test)]
mod tabs_tests;
#[cfg(test)]
use settings_tests::SettingsDialogGeometry;
mod aliases;
mod drive_picker;
mod everything_inventory;
mod everything_ipc;
// Optional modules: dormant until they are reconnected through the core's
// hooks (see src/modules/README.md).
mod goto;
mod help_ui;
mod link_ui;
mod markdown;
mod markdown_files;
mod module_api;
#[allow(dead_code)]
mod modules;
mod results_ui;
mod vaults;
use markdown_files::{append_markdown_block, append_markdown_block_to_heading};
mod parallel;
mod perf_hud;
mod perf_log;
mod periscope;
mod periscope_backend;
#[cfg(test)]
mod periscope_results_tests;
mod periscope_setup;
#[cfg(test)]
mod periscope_setup_tests;
mod periscope_store;
mod periscope_ui;
#[cfg(test)]
mod picker_tests;
mod picker_ui;
mod preview_ui;
mod project_membership;
#[cfg(test)]
mod project_membership_tests;
mod resident;
mod save_as_ui;
mod scratch;
mod search;
mod settings_ui;
mod shortcuts;
mod single_instance;
mod spare;
mod tab_drop;
mod tabs;
mod window_drag;
mod window_ui;

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use commands::{
    Command, CommandLineAction, CommandLineNumberMode, CommandSpec, CommandUsage,
    best_command_token, clamp_history_limit, command_name, command_prefix, fuzzy_score,
    parse_command_line,
};
use editor_buffer::EditorBuffer;
use editor_view::{EditorView, LineNumberMode};
use eframe::egui::{
    self, Color32, FontFamily, FontId, Key, RichText, Stroke, TextEdit, Vec2,
    text::{LayoutJob, LayoutSection, TextFormat},
};
use file_dialogs::FilePickerMode;
use goto::GotoTarget;
use link_ui::{LinkAssistChoice, LinkHeadingOption};
use markdown::{
    CheckboxState, TableAlignment, is_markdown_separator, markdown_link_target_at_byte,
    parse_blockquote_line, parse_checkbox_line, parse_fenced_code_marker, parse_heading_line,
    parse_inline_code_spans, parse_list_line, parse_markdown_link_spans,
    parse_markdown_table_separator, split_markdown_table_row,
};
use periscope::{PeriscopeMode, PeriscopeRow};
use scratch::ScratchEntry;
use search::SearchState;

const PERISCOPE_RESULT_LIMIT: usize = 100;
const DEFAULT_CURSOR_SIZE: &str = "24";

fn open_url_in_browser(url: &str) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "windows")]
    {
        ProcessCommand::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()
    }
    #[cfg(target_os = "macos")]
    {
        ProcessCommand::new("open").arg(url).spawn()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        ProcessCommand::new("xdg-open").arg(url).spawn()
    }
}

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

/// `SLATE_RENDERER=wgpu` picks the wgpu backend when built with `--features wgpu`.
/// wgpu (DX12 flip model on Windows) by default; `SLATE_RENDERER=glow` forces
/// OpenGL. Builds without the `wgpu` feature always use glow.
fn selected_renderer() -> eframe::Renderer {
    #[cfg(feature = "wgpu")]
    if !std::env::var("SLATE_RENDERER").is_ok_and(|value| value.eq_ignore_ascii_case("glow")) {
        return eframe::Renderer::Wgpu;
    }
    eframe::Renderer::Glow
}

/// Low-latency wgpu presentation. On Windows this is a DXGI flip-model swapchain
/// (like Chromium/Thorium) instead of OpenGL's "copy with GPU GDI" composition,
/// which made every frame wait one extra display refresh.
#[cfg(feature = "wgpu")]
fn low_latency_wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, wgpu};
    let mut options = WgpuConfiguration::default();
    // Do not queue frames ahead of the display: the newest keystroke wins.
    options.desired_maximum_frame_latency = Some(1);
    options.present_mode = match std::env::var("SLATE_PRESENT").as_deref() {
        Ok("fifo") => wgpu::PresentMode::Fifo,
        Ok("mailbox") => wgpu::PresentMode::Mailbox,
        Ok("immediate") => wgpu::PresentMode::Immediate,
        _ => wgpu::PresentMode::AutoNoVsync,
    };
    if let WgpuSetup::CreateNew(create_new) = &mut options.wgpu_setup {
        // Only DX12 on Windows: skips Vulkan/GL instance creation at startup and
        // guarantees the DXGI flip-model path. WGPU_BACKEND still overrides.
        if cfg!(windows) && std::env::var_os("WGPU_BACKEND").is_none() {
            create_new.instance_descriptor.backends = wgpu::Backends::DX12;
        }
        // wgpu's default reserves GPU memory in 64–256 MB blocks; Slate's
        // buffers and font atlas need a few MB. Small blocks cut ~200 MB of
        // committed memory, which matters while resident in the tray.
        let base = create_new.device_descriptor.clone();
        create_new.device_descriptor = std::sync::Arc::new(move |adapter| wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..base(adapter)
        });
    }
    options
}

/// A save running on a helper thread; applied to the document it was taken
/// from (matched by buffer identity and path), even if another tab is active
/// by the time it lands.
struct PendingSave {
    path: PathBuf,
    identity: u64,
    revision: u64,
    project: Option<PathBuf>,
    result: std::sync::mpsc::Receiver<Result<safe_file::Fingerprint, safe_file::SaveError>>,
    started: std::time::Instant,
}

/// Direct3D 12 started on a helper thread while eframe creates the window.
/// D3D12 keeps one device per adapter per process, so when wgpu probes the
/// adapters and creates its device a moment later, the driver is already up.
#[cfg(all(windows, feature = "wgpu"))]
type WarmGpu = std::thread::JoinHandle<Box<dyn std::any::Any + Send>>;
#[cfg(all(windows, feature = "wgpu"))]
static WARM_GPU: std::sync::Mutex<Option<WarmGpu>> = std::sync::Mutex::new(None);

#[cfg(all(windows, feature = "wgpu"))]
fn prewarm_gpu() {
    use eframe::egui_wgpu::wgpu;
    if std::env::var_os("SLATE_PREWARM").is_some_and(|v| v == "0")
        || std::env::var_os("WGPU_BACKEND").is_some()
    {
        return;
    }
    let handle = std::thread::spawn(|| -> Box<dyn std::any::Any + Send> {
        let started = std::time::Instant::now();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
        perf_log::mark(
            "startup.gpu_prewarmed",
            &format!(
                "ms={:.1} adapters={}",
                started.elapsed().as_secs_f64() * 1000.0,
                adapters.len()
            ),
        );
        Box::new((instance, adapters))
    });
    *WARM_GPU.lock().unwrap() = Some(handle);
}

/// Called once the window has drawn: wgpu holds its own device by then.
fn release_prewarmed_gpu() {
    #[cfg(all(windows, feature = "wgpu"))]
    if let Some(handle) = WARM_GPU.lock().unwrap().take() {
        std::thread::spawn(move || drop(handle.join()));
    }
}

/// Window/taskbar icon (assets/slate-256.png, from tools/generate_icon.py).
/// A decode failure only costs the icon, never the start: egui-winit and
/// eframe treat an empty `IconData` as "no icon".
fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/slate-256.png")).unwrap_or_default()
}

fn main() -> eframe::Result {
    perf_log::init(std::time::Instant::now());
    normalize_cursor_environment();
    perf_log::mark("startup.environment_ready", "");

    let raw_args = std::env::args().skip(1).collect::<Vec<_>>();

    let mut scratch = false;
    let mut new_window = false;
    let mut background = false;
    let mut position: Option<egui::Pos2> = None;
    let mut handoff: Option<PathBuf> = None;
    let mut spare_owner: Option<u32> = None;
    let mut path = None;
    let mut startup_line = None;
    let mut args = raw_args.into_iter().peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scratch" | "-s" => scratch = true,
            "--new-window" | "-n" => new_window = true,
            "--background" => background = true,
            "--handoff" => handoff = args.next().map(PathBuf::from),
            "--spare" => {
                spare_owner = args.next().and_then(|value| value.parse().ok());
                new_window = true;
            }
            "--position" => {
                position = args.next().and_then(|value| {
                    let (x, y) = value.split_once(',')?;
                    Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
                });
            }
            "--line" | "-l" => {
                startup_line = args.next().and_then(|value| value.parse::<usize>().ok());
            }
            _ => path = Some(PathBuf::from(arg)),
        }
    }

    let use_single_instance = !scratch && !new_window && SlateApp::config_single_instance_enabled();
    if background {
        // Start-with-Windows entry: only useful as the resident primary, and
        // never shows anything when Slate is already running.
        let running =
            single_instance::instance_file().is_some_and(|file| single_instance::is_running(&file));
        if !use_single_instance || running {
            perf_log::mark(
                "startup.background_skipped",
                if running {
                    "running"
                } else {
                    "single instance off"
                },
            );
            perf_log::flush();
            return Ok(());
        }
    }
    if use_single_instance && !background {
        if let Some(file) = single_instance::instance_file() {
            let request = single_instance::OpenRequest {
                path: path
                    .as_deref()
                    .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())),
                line: startup_line,
                ..Default::default()
            };
            if single_instance::forward(&file, &request) {
                perf_log::mark("startup.forwarded_to_running_instance", "");
                perf_log::flush();
                return Ok(());
            }
        }
    }

    if spare_owner.is_some() {
        if spare::is_ready() {
            perf_log::mark("spare.skipped", "another spare is ready");
            perf_log::flush();
            return Ok(());
        }
        spare::set_starting_as_spare();
    } else if new_window && !scratch {
        // A ready hidden window shows itself in milliseconds.
        let request = single_instance::OpenRequest {
            path: path
                .as_deref()
                .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())),
            line: startup_line,
            handoff: handoff.clone(),
            position: position.map(|at| (at.x, at.y)),
            ..Default::default()
        };
        if spare::claim(&request).is_some() {
            perf_log::mark("startup.claimed_spare_window", "");
            perf_log::flush();
            return Ok(());
        }
    }

    #[cfg(all(windows, feature = "wgpu"))]
    if selected_renderer() == eframe::Renderer::Wgpu {
        prewarm_gpu();
    }

    let title = if scratch { "Slate Scratch" } else { "Slate" };
    let size = if scratch {
        [760.0, 460.0]
    } else {
        [900.0, 560.0]
    };

    tabs::set_owns_session(use_single_instance);
    let icon = std::sync::Arc::new(app_icon());
    let native_options = |renderer| eframe::NativeOptions {
        viewport: position
            .map_or_else(egui::ViewportBuilder::default, |at| {
                egui::ViewportBuilder::default().with_position(at)
            })
            .with_title(title)
            .with_icon(icon.clone())
            .with_decorations(false)
            .with_inner_size(size)
            .with_min_inner_size([420.0, 260.0]),
        vsync: false,
        renderer,
        #[cfg(feature = "wgpu")]
        wgpu_options: low_latency_wgpu_options(),
        ..Default::default()
    };

    let app_started = std::cell::Cell::new(false);
    let run = |renderer| {
        perf_log::mark(
            "startup.framework_begin",
            &format!(
                "window creation, renderer and fonts until app constructor renderer={renderer:?}"
            ),
        );
        let path = path.clone();
        let handoff = handoff.clone();
        let app_started = &app_started;
        eframe::run_native(
            title,
            native_options(renderer),
            Box::new(move |cc| {
                app_started.set(true);
                let mut app = SlateApp::new(cc, path, scratch, startup_line);
                if use_single_instance {
                    app.start_instance_server(&cc.egui_ctx);
                    app.enable_resident(cc, background);
                }
                if let Some(file) = &handoff {
                    app.apply_handoff(file);
                }
                if let Some(owner) = spare_owner {
                    app.become_spare(cc, owner);
                } else if !scratch {
                    app.tab_drop_server = tab_drop::listen(&cc.egui_ctx);
                }
                Ok(Box::new(app))
            }),
        )
    };
    let mut result = run(selected_renderer());
    if let Err(error) = &result
        && !app_started.get()
        && selected_renderer() != eframe::Renderer::Glow
    {
        // No usable DX12 adapter (old GPU, remote session...): OpenGL still works.
        perf_log::mark("startup.renderer_fallback", &format!("glow after: {error}"));
        result = run(eframe::Renderer::Glow);
    }
    settings_writer::flush();
    perf_log::mark(
        "process.ui_loop_exit",
        if result.is_ok() { "ok" } else { "error" },
    );
    perf_log::flush();
    result
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

fn find_note_file_matches(root: &Path, path: &Path, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let label = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();
    if label.to_lowercase().contains(query)
        || path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.to_lowercase().contains(query))
            .unwrap_or(false)
    {
        return true;
    }
    fs::read_to_string(path)
        .map(|contents| contents.to_lowercase().contains(query))
        .unwrap_or(false)
}

fn find_note_match_hint(root: &Path, path: &Path, query: &str) -> String {
    let label = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();
    if label.to_lowercase().contains(query) {
        return "file".to_string();
    }
    let Ok(contents) = fs::read_to_string(path) else {
        return "note".to_string();
    };
    for line in contents.lines() {
        let lower = line.to_lowercase();
        if let Some(heading) = parse_heading_line(line) {
            if heading.text.to_lowercase().contains(query) {
                return format!("heading: {}", heading.text.trim());
            }
        } else if lower.contains(query) {
            return line.trim().chars().take(80).collect();
        }
    }
    "note".to_string()
}

fn is_markdown_like_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkHeadingPickerMode {
    InsertLink,
    BidirectionalLink,
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
    CloseTab,
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
            PendingAction::CloseTab => "tab has unsaved changes; close it anyway?",
            PendingAction::Quit => "buffer has unsaved changes; close anyway?",
        }
    }
}

struct DuplicatePlacement {
    snapshot: String,
    was_dirty: bool,
}

#[derive(Clone)]
struct DocTask {
    line_index: usize,
    state: CheckboxState,
    task_prefix: String,
    text: String,
}

struct FileCursorPosition {
    path: PathBuf,
    line_index: usize,
    column: usize,
}

#[derive(Clone)]
enum OpenDestination {
    Restore,
    Line(usize),
    Fragment(String),
}

struct PendingFileOpen {
    id: u64,
    path: PathBuf,
    origin_path: Option<PathBuf>,
    origin_revision: u64,
    destination: OpenDestination,
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
    scratch_close_after: Option<f64>,
    /// "Scratch archived" toast shown while the scratch window closes.
    scratch_toast_until: Option<f64>,
    pending_action: Option<PendingAction>,
    file_loader: file_loader::Loader,
    preview_sync: scroll_sync::PreviewSync,
    preview_layout: preview_layout::Layout,
    preview_tops: Vec<f32>,
    document_safety: document_safety::Manager,
    disk_base: Option<safe_file::Fingerprint>,
    modal_disk_base: Option<safe_file::Fingerprint>,
    recovery_origins: std::collections::HashMap<document_safety::Slot, PathBuf>,
    recovery_records: Vec<document_safety::Recovered>,
    recovery_open: bool,
    recovery_selected: usize,
    file_conflict: Option<(document_safety::Slot, PathBuf)>,
    safety_notified:
        std::collections::HashMap<document_safety::Slot, (PathBuf, Option<safe_file::Fingerprint>)>,
    discard_on_exit: bool,
    pending_file_open: Option<PendingFileOpen>,
    #[cfg(test)]
    settings_path_override: Option<PathBuf>,
    settings_open: bool,
    selected_setting: usize,
    settings_last_selected: Option<usize>,
    editor_font_size: f32,
    editor_look: editor_view::EditorLook,
    footer_options: footer_ui::Options,
    footer_counts: footer_ui::DocumentCounts,
    footer_status_seen: String,
    footer_status_since: Option<std::time::Instant>,
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
    single_instance: bool,
    instance_server: Option<single_instance::Server>,
    /// Waiting hidden as the ready window (`--spare`) until claimed.
    spare_server: Option<single_instance::Server>,
    /// config.toml lines the core does not know (modules' settings), written
    /// back unchanged.
    config_passthrough: Vec<String>,
    /// Optional modules of this window and their on/off state.
    module_host: module_api::Host,
    module_ctx: egui::Context,
    /// Revision each open document had when modules last heard of it.
    module_seen: std::collections::HashMap<u64, u64>,
    /// Result list shown for a module.
    results: results_ui::Results,
    /// Note folders (Settings → Modules & vaults).
    vaults: Vec<PathBuf>,
    spare_owner: Option<u32>,
    /// Receives tabs dropped onto this window from other Slate windows.
    tab_drop_server: Option<single_instance::Server>,
    /// A tab from another window is being dragged over this one: where it
    /// would land, its label, when that was last said.
    tab_drop_hover: Option<(usize, String, std::time::Instant)>,
    /// The window last told a tab of ours is over it, and where.
    drag_hover: Option<(PathBuf, (f32, f32))>,
    /// A tab torn off into a new window that follows the cursor.
    torn_tab: Option<tabs::TornTab>,
    /// Last time the resident primary made sure a spare exists.
    spare_checked_at: Option<std::time::Instant>,
    /// Config: closing the primary window hides it in the tray.
    resident: bool,
    /// Resident mode is running in this process (tray icon installed).
    resident_active: bool,
    window_hidden: bool,
    /// A Ctrl+S whose synced write is still running on a helper thread.
    pending_save: Option<PendingSave>,
    /// Ctrl+S pressed again while saving: save once more when it lands.
    save_again: bool,
    /// Tests save synchronously unless they exercise the background path.
    sync_saves: bool,
    /// When the window was last shown from the tray: the next frame's start
    /// logs how long the first visible frame took to build and paint.
    shown_at: Option<std::time::Instant>,
    hide_on_first_frame: bool,
    /// Set by :quit-all / the tray menu: the next close really exits.
    quit_for_real: bool,
    start_with_windows: bool,
    tabs: tabs::Tabs,
    aliases: aliases::Aliases,
    /// An open action that continues in a freshly opened tab this frame.
    pending_tab_action: Option<PendingAction>,
    /// Ctrl+wheel accumulator so touchpads switch one tab per gesture step.
    tab_wheel: f32,
    markdown_live_rendering: bool,
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
    link_inline_anchor: Option<usize>,
    link_inline_files: Vec<PathBuf>,
    selected_link_inline_suggestion: usize,
    pending_bidirectional_link_source_line: Option<usize>,
    link_heading_target_path: Option<PathBuf>,
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
    project_membership: project_membership::State,
    periscope_query: String,
    periscope_results: Vec<PathBuf>,
    selected_periscope_result: usize,
    periscope_expanded_folders: HashSet<PathBuf>,
    periscope_backend_status: String,
    periscope_engine: periscope_backend::Manager,
    periscope_setup: periscope_setup::Setup,
    periscope_request: Option<u64>,
    periscope_roots: Vec<PathBuf>,
    periscope_everything: bool,
    periscope_global_loading: bool,
    find_note_open: bool,
    find_note_query: String,
    find_note_files: Vec<PathBuf>,
    find_note_rows: Vec<FindNoteRow>,
    selected_find_note_row: usize,
    find_note_expanded_files: HashSet<PathBuf>,
    pending_project_file_path: Option<PathBuf>,
    pending_open_heading_fragment: Option<String>,
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
    perf_hud: perf_hud::PerfHud,
    last_window_title: String,
}

impl SlateApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        scratch: bool,
        startup_line: Option<usize>,
    ) -> Self {
        let mut timing = perf_log::Span::new("startup.app_constructor", "");
        setup_style(&cc.egui_ctx);
        timing.step("setup_style");
        window_drag::install(cc);

        let mut app = Self {
            perf_hud: perf_hud::PerfHud::default(),
            last_window_title: String::new(),
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
            scratch_close_after: None,
            scratch_toast_until: None,
            pending_action: None,
            file_loader: file_loader::Loader::new(cc.egui_ctx.clone()),
            preview_sync: scroll_sync::PreviewSync::default(),
            preview_layout: preview_layout::Layout::default(),
            preview_tops: Vec::new(),
            document_safety: {
                #[cfg(test)]
                {
                    document_safety::Manager::disabled()
                }
                #[cfg(not(test))]
                {
                    document_safety::Manager::new(
                        cc.egui_ctx.clone(),
                        document_safety::default_root(),
                    )
                }
            },
            disk_base: None,
            modal_disk_base: None,
            recovery_origins: std::collections::HashMap::new(),
            recovery_records: Vec::new(),
            recovery_open: false,
            recovery_selected: 0,
            file_conflict: None,
            safety_notified: std::collections::HashMap::new(),
            discard_on_exit: false,
            pending_file_open: None,
            #[cfg(test)]
            settings_path_override: None,
            settings_open: false,
            selected_setting: 0,
            settings_last_selected: None,
            editor_font_size: editor_view::DEFAULT_EDITOR_FONT_SIZE,
            editor_look: editor_view::EditorLook::default(),
            footer_options: footer_ui::Options::default(),
            footer_counts: Default::default(),
            footer_status_seen: "Ready".into(),
            footer_status_since: None,
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
            single_instance: true,
            instance_server: None,
            spare_server: None,
            config_passthrough: Vec::new(),
            module_host: module_api::Host::new(modules::all()),
            module_ctx: cc.egui_ctx.clone(),
            module_seen: std::collections::HashMap::new(),
            results: results_ui::Results::default(),
            vaults: Vec::new(),
            spare_owner: None,
            tab_drop_server: None,
            spare_checked_at: None,
            tab_drop_hover: None,
            drag_hover: None,
            torn_tab: None,
            resident: true,
            resident_active: false,
            window_hidden: false,
            pending_save: None,
            save_again: false,
            sync_saves: cfg!(test),
            shown_at: None,
            hide_on_first_frame: false,
            quit_for_real: false,
            start_with_windows: resident::autostart_enabled(),
            tabs: tabs::Tabs::default(),
            // Tests never read the user's aliases.
            aliases: if cfg!(test) {
                aliases::Aliases::default()
            } else {
                aliases::Aliases::load(
                    Self::settings_path()
                        .and_then(|p| p.parent().map(|dir| dir.join("aliases.json")))
                        .as_deref(),
                )
            },
            pending_tab_action: None,
            tab_wheel: 0.0,
            markdown_live_rendering: true,
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
            link_inline_anchor: None,
            link_inline_files: Vec::new(),
            selected_link_inline_suggestion: 0,
            pending_bidirectional_link_source_line: None,
            link_heading_target_path: None,
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
            project_membership: Default::default(),
            periscope_query: String::new(),
            periscope_results: Vec::new(),
            selected_periscope_result: 0,
            periscope_expanded_folders: HashSet::new(),
            periscope_backend_status: String::new(),
            periscope_engine: periscope_backend::Manager::default(),
            periscope_setup: periscope_setup::Setup::default(),
            periscope_request: None,
            periscope_roots: Vec::new(),
            periscope_everything: true,
            periscope_global_loading: false,
            find_note_open: false,
            find_note_query: String::new(),
            find_note_files: Vec::new(),
            find_note_rows: Vec::new(),
            selected_find_note_row: 0,
            find_note_expanded_files: HashSet::new(),
            pending_project_file_path: None,
            pending_open_heading_fragment: None,
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

        timing.step("initial_state");
        app.load_settings();
        timing.step("load_settings");

        if let Some(path) = path {
            if !scratch && tabs::owns_session() {
                app.restore_session();
            }
            app.request_file_open(
                path,
                startup_line
                    .map(OpenDestination::Line)
                    .unwrap_or(OpenDestination::Restore),
            );
        } else if !scratch
            && !spare::starting_as_spare()
            && !(tabs::owns_session() && app.restore_session())
            && app.reopen_last_file_on_startup
        {
            if let Some(last_path) = app.last_opened_path.clone() {
                app.open_path(last_path);
            }
        }

        timing.step("initial_document_or_agenda");
        app
    }

    fn title(&self) -> String {
        let name = self
            .path
            .as_deref()
            .and_then(|p| self.aliases.get(p).or_else(|| p.file_name()?.to_str()))
            .or_else(|| self.active_tab_alias())
            .unwrap_or("untitled");
        if self.scratch && self.path.is_none() {
            format!("{}Slate Scratch", if self.dirty { "*" } else { "" })
        } else {
            format!("{}{} — Slate", if self.dirty { "*" } else { "" }, name)
        }
    }

    fn start_instance_server(&mut self, ctx: &egui::Context) {
        if !self.single_instance {
            return;
        }
        let Some(file) = single_instance::instance_file() else {
            return;
        };
        let wake_ctx = ctx.clone();
        match single_instance::Server::start(file, move |_| {
            // A hidden window would only notice the request on eframe's 100 ms
            // throttled repaint; show it now, egui syncs on the next frame.
            resident::show_now();
            wake_ctx.request_repaint();
        }) {
            Ok(server) => self.instance_server = Some(server),
            Err(err) => perf_log::mark("single_instance.unavailable", &err.to_string()),
        }
    }

    /// Opens files handed over by `slate <file>` launches while this window runs.
    fn enable_resident(&mut self, cc: &eframe::CreationContext<'_>, background: bool) {
        if !(self.resident || background) || self.instance_server.is_none() {
            return;
        }
        resident::install(cc);
        self.resident_active = true;
        spare::prepare_soon(std::process::id(), spare::PREPARE_DELAY);
        if background {
            // eframe shows the window after its first paint; hide it right away.
            self.window_hidden = true;
            self.hide_on_first_frame = true;
        }
    }

    /// `--spare`: wait hidden, fully started, until a new window claims
    /// this one (see `spare`).
    fn become_spare(&mut self, cc: &eframe::CreationContext<'_>, owner: u32) {
        let Some(file) = single_instance::spare_file() else {
            std::process::exit(0);
        };
        resident::remember_window(cc);
        // eframe shows the window after its first paint; nothing may flash.
        resident::cloak(true);
        let wake_ctx = cc.egui_ctx.clone();
        let server = match single_instance::Server::start_one_shot(file.clone(), move |request| {
            spare::claimed();
            resident::show_at(request.position, !request.follow);
            wake_ctx.request_repaint();
        }) {
            Ok(server) => server,
            Err(err) => {
                perf_log::mark("spare.unavailable", &err.to_string());
                std::process::exit(0);
            }
        };
        let written = fs::read_to_string(&file).unwrap_or_default();
        resident::when_process_exits(owner, move || {
            // Nothing was opened yet: no settings or session to write.
            if !spare::is_claimed() {
                single_instance::remove_if_ours(&file, &written);
                perf_log::mark("spare.owner_gone", "");
                perf_log::flush();
                std::process::exit(0);
            }
        });
        self.spare_server = Some(server);
        self.spare_owner = Some(owner);
        self.window_hidden = true;
        self.hide_on_first_frame = true;
        resident::trim_memory_soon();
        perf_log::mark("spare.ready", &format!("owner={owner}"));
    }

    /// Tabs dropped onto this window from other Slate windows.
    fn poll_dropped_tabs(&mut self, ctx: &egui::Context) {
        let Some(server) = self.tab_drop_server.as_ref() else {
            return;
        };
        for request in server.take_requests() {
            if let Some(label) = request.hover {
                self.tab_drop_hover = request.tab_at.map(|at| {
                    (
                        window_ui::tab_slot_at(ctx, at),
                        label,
                        std::time::Instant::now(),
                    )
                });
            } else if let (Some(file), Some(at)) = (request.send_to.clone(), request.tab_at) {
                self.send_tab_on(file, at, ctx);
            } else {
                self.tab_drop_hover = None;
                self.receive_tab(request, ctx);
            }
        }
        // A drag that went away without saying so.
        if self
            .tab_drop_hover
            .as_ref()
            .is_some_and(|(_, _, at)| at.elapsed() > std::time::Duration::from_millis(600))
        {
            self.tab_drop_hover = None;
        }
        let shown = self
            .tab_drop_hover
            .as_ref()
            .map(|(slot, label, _)| (*slot, label.clone()));
        if shown.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        window_ui::set_drop_hover(ctx, shown);
    }

    /// The ready window was claimed: become the requested window.
    fn poll_spare(&mut self, ctx: &egui::Context) {
        let Some(request) = self
            .spare_server
            .as_ref()
            .and_then(|server| server.take_requests().pop())
        else {
            return;
        };
        self.spare_server = None;
        perf_log::mark("spare.claimed", "");
        self.tab_drop_server = tab_drop::listen(ctx);
        // Settings may have changed since this window started.
        self.load_settings();
        if request.follow {
            // Following a tab still being dragged: the dragging window keeps
            // the focus until the drop.
            self.window_hidden = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.shown_at = Some(std::time::Instant::now());
        } else {
            self.show_window(ctx);
        }
        if let Some(file) = request.handoff {
            self.apply_handoff(&file);
        } else if let Some(path) = request.path {
            self.request_file_open(
                path,
                request
                    .line
                    .map(OpenDestination::Line)
                    .unwrap_or(OpenDestination::Restore),
            );
        } else if self.reopen_last_file_on_startup
            && let Some(last_path) = self.last_opened_path.clone()
        {
            self.open_path(last_path);
        }
        if let Some(owner) = self.spare_owner {
            spare::prepare_soon(owner, spare::PREPARE_DELAY);
        }
    }

    fn poll_resident(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.hide_on_first_frame) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // Keep a spare while visible (one that was closed or crashed is
        // replaced); nothing runs while hidden.
        if self.resident_active
            && !self.window_hidden
            && self
                .spare_checked_at
                .is_none_or(|at| at.elapsed() >= spare::CHECK_INTERVAL)
        {
            let first = self.spare_checked_at.is_none();
            self.spare_checked_at = Some(std::time::Instant::now());
            if !first {
                spare::prepare_soon(std::process::id(), std::time::Duration::ZERO);
            }
        }
        if resident::take_quit_request() {
            self.quit_for_real = true;
            if self.window_hidden {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.request_close(ctx);
            }
        }
        if resident::take_show_request() {
            self.show_window(ctx);
        }
    }

    /// Closing in resident mode: persist what a real exit persists (unsaved
    /// text included, as recovery checkpoints) and hide; tabs stay as they are.
    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        self.observe_document_safety(0.0, true);
        self.remember_current_cursor_position();
        if !self.discard_on_exit
            && (self.scratch_modal_open || !self.scratch_buffer.as_str().is_empty())
        {
            self.archive_scratch_modal();
        }
        self.discard_on_exit = false;
        self.document_safety.flush();
        let _ = self.save_settings();
        for open in [
            &mut self.palette_open,
            &mut self.settings_open,
            &mut self.shortcut_help_open,
            &mut self.recent_picker_open,
            &mut self.lance_open,
            &mut self.doc_tasks_open,
            &mut self.file_picker_open,
            &mut self.periscope_open,
            &mut self.periscope_prompt_open,
            &mut self.find_note_open,
            &mut self.link_assist_open,
            &mut self.link_assist_web_open,
            &mut self.link_heading_picker_open,
            &mut self.link_preview_open,
            &mut self.save_as_open,
            &mut self.scratch_modal_open,
            &mut self.scratch_entries_open,
            &mut self.capture_modal_open,
        ] {
            *open = false;
        }
        self.window_hidden = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        perf_log::mark("resident.hidden", "");
        resident::trim_memory_soon();
    }

    /// Shows the window (un-hides, un-minimizes, focuses) with its tabs.
    fn show_window(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.window_hidden) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            perf_log::mark("resident.shown", "");
            self.shown_at = Some(std::time::Instant::now());
            ctx.request_repaint();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.focus_editor_once = true;
    }

    fn set_resident(&mut self, enabled: bool) {
        self.resident = enabled;
        if !enabled && self.resident_active {
            resident::remove_tray();
            self.resident_active = false;
        }
        self.status = match self.save_settings() {
            Ok(_) if enabled && !self.resident_active => {
                "Resident mode on (from the next start)".to_string()
            }
            Ok(_) if enabled => "Resident mode on".to_string(),
            Ok(_) => "Resident mode off: closing quits Slate".to_string(),
            Err(err) => format!("Settings save failed: {err}"),
        };
    }

    fn set_start_with_windows(&mut self, enabled: bool) {
        self.status = match resident::set_autostart(enabled) {
            Ok(()) => {
                self.start_with_windows = enabled;
                if enabled {
                    "Slate starts hidden with Windows".to_string()
                } else {
                    "Slate no longer starts with Windows".to_string()
                }
            }
            Err(err) => format!("Start with Windows failed: {err}"),
        };
    }

    fn poll_instance_requests(&mut self, ctx: &egui::Context) {
        let Some(server) = self.instance_server.as_ref() else {
            return;
        };
        let Some(request) = server.take_requests().into_iter().last() else {
            return;
        };
        self.show_window(ctx);
        let Some(path) = request.path else {
            return;
        };
        if self.path.as_ref() == Some(&path) {
            if let Some(line) = request.line {
                self.goto_startup_line(line);
            }
        } else {
            // Same guard as :open — dirty text is never replaced.
            self.request_file_open(
                path,
                request
                    .line
                    .map(OpenDestination::Line)
                    .unwrap_or(OpenDestination::Restore),
            );
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        self.request_file_open(path, OpenDestination::Restore);
    }

    fn request_file_open(&mut self, path: PathBuf, destination: OpenDestination) {
        if !self.scratch && !self.prepare_tab_for_open(&path, &destination) {
            return;
        }
        self.start_file_load(path, destination);
    }

    fn apply_open_destination(&mut self, destination: OpenDestination) {
        match destination {
            OpenDestination::Restore => {}
            OpenDestination::Line(line) => self.goto_startup_line(line),
            OpenDestination::Fragment(fragment) => self.jump_to_markdown_fragment(&fragment),
        }
        self.focus_editor_once = true;
    }

    /// Loads `path` into the active tab (it must be clean).
    fn start_file_load(&mut self, path: PathBuf, destination: OpenDestination) {
        if self.dirty {
            self.cancel_file_open();
            self.status = "Save or discard changes before opening another file".to_string();
            return;
        }
        self.remember_current_cursor_position();
        match self.file_loader.request(path.clone()) {
            Ok(id) => {
                self.pending_file_open = Some(PendingFileOpen {
                    id,
                    path: path.clone(),
                    origin_path: self.path.clone(),
                    origin_revision: self.buffer.revision,
                    destination,
                });
                self.status = format!("Opening {}… · Esc cancels", path.display());
            }
            Err(err) => {
                self.cancel_file_open();
                self.status = format!("Open failed: {err}");
            }
        }
    }

    fn cancel_file_open(&mut self) {
        if let Some(pending) = self.pending_file_open.take() {
            perf_log::mark(
                "file_open.cancelled",
                &format!("request={} path={:?}", pending.id, pending.path),
            );
        }
        self.file_loader.cancel_queued();
    }

    fn poll_file_open(&mut self, ctx: &egui::Context) {
        if self.pending_file_open.as_ref().is_some_and(|p| {
            self.dirty || p.origin_path != self.path || p.origin_revision != self.buffer.revision
        }) {
            self.cancel_file_open();
            self.status = "Open cancelled: current buffer changed; retry when ready".to_string();
        }
        // Let this frame's keystrokes edit the document that was visible when typed.
        // Apply on the next frame, after revision/dirty guards can observe those edits.
        if self.pending_file_open.is_some()
            && ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Cut
                            | egui::Event::Key { pressed: true, .. }
                            | egui::Event::PointerButton { pressed: true, .. }
                    )
                })
            })
        {
            ctx.request_repaint();
            return;
        }
        while let Some(mut loaded) = self.file_loader.try_recv() {
            if !self
                .pending_file_open
                .as_ref()
                .is_some_and(|p| p.id == loaded.id && p.path == loaded.path)
            {
                perf_log::mark(
                    "file_open.discarded",
                    &format!("request={} path={:?}", loaded.id, loaded.path),
                );
                continue;
            }
            let pending = self.pending_file_open.take().expect("matching request");
            loaded.timing.step("dispatch_to_ui");
            match loaded.result {
                Ok(mut buffer) => {
                    let path = loaded.path;
                    self.remember_current_cursor_position();
                    buffer.set_cursor(self.buffer.cursor());
                    buffer.revision = self.buffer.revision.wrapping_add(1);
                    perf_log::mark(
                        "file_open.metadata",
                        &format!("path={path:?} bytes={}", buffer.as_str().len()),
                    );
                    self.clear_recovery(document_safety::Slot::Main);
                    self.buffer = buffer;
                    self.path = Some(path.clone());
                    self.disk_base = loaded.fingerprint;
                    self.file_conflict = None;
                    self.last_open_dir = path.parent().map(PathBuf::from);
                    self.dirty = false;
                    self.search_state = None;
                    self.restore_cursor_position_for_path(&path);
                    self.remember_recent_file(path.clone());
                    loaded.timing.step("apply_buffer_restore_cursor_and_recent");
                    self.perf_hud.record_open(loaded.timing.elapsed());
                    let _ = self.save_settings();
                    loaded.timing.step("save_settings");
                    self.focus_editor_once = true;
                    self.status = format!("Opened {}", path.display());
                    self.notify_modules_opened();
                    match pending.destination {
                        OpenDestination::Restore => {}
                        OpenDestination::Line(line) => self.goto_startup_line(line),
                        OpenDestination::Fragment(fragment) => {
                            self.jump_to_markdown_fragment(&fragment)
                        }
                    }
                    loaded.timing.step("apply_destination");
                    loaded.timing.wait_for_ui();
                    ctx.request_repaint();
                }
                Err(err) => {
                    perf_log::mark(
                        "file_open.error",
                        &format!("path={:?} error={err}", loaded.path),
                    );
                    self.status = format!("Open failed: {err}");
                }
            }
        }
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
        let started = std::time::Instant::now();
        let root = self.project_root();
        let mut files = Vec::new();
        collect_link_target_files(&root, &mut files, 20_000);
        files.retain(|path| is_markdown_like_path(path));
        files.sort();
        files.dedup();
        let query = self.find_note_query.trim().to_lowercase();
        const LIMIT: usize = 200;
        let mut matched = Vec::new();
        // Read and test files on all cores, batch by batch, so a common query still
        // stops early instead of reading the whole project.
        for batch in files.chunks(512) {
            let flags =
                parallel::par_map(batch, |path| find_note_file_matches(&root, path, &query));
            matched.extend(
                batch
                    .iter()
                    .zip(flags)
                    .filter_map(|(path, matches)| matches.then(|| path.clone())),
            );
            if matched.len() >= LIMIT {
                break;
            }
        }
        matched.truncate(LIMIT);
        self.find_note_files = matched;
        self.rebuild_find_note_rows();
        self.perf_hud
            .record_note_search(started.elapsed(), self.find_note_files.len());
    }

    fn rebuild_find_note_rows(&mut self) {
        let query = self.find_note_query.trim().to_lowercase();
        let root = self.project_root();
        let hints = if query.is_empty() {
            vec!["note".to_string(); self.find_note_files.len()]
        } else {
            parallel::par_map(&self.find_note_files, |path| {
                find_note_match_hint(&root, path, &query)
            })
        };
        let mut rows = Vec::new();
        for (path, hint) in self.find_note_files.iter().zip(hints) {
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
        } else {
            self.open_keeping_unsaved_text(path, fragment);
        }
    }

    /// Opens `path` (at `fragment`). Unsaved text here stays in its tab and
    /// the file opens in another one; windows without tabs ask first.
    fn open_keeping_unsaved_text(&mut self, path: PathBuf, fragment: Option<String>) {
        if self.dirty {
            self.pending_project_file_path = Some(path);
            self.pending_open_heading_fragment = fragment;
            self.confirm(PendingAction::OpenProjectFile);
        } else {
            self.request_file_open(
                path,
                fragment
                    .map(OpenDestination::Fragment)
                    .unwrap_or(OpenDestination::Restore),
            );
        }
        self.focus_editor_once = true;
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
                self.safety_notified.remove(&document_safety::Slot::Modal);
                self.modal_disk_base = Some(safe_file::fingerprint(text.as_bytes()));
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
        let preview_base = self.modal_disk_base;
        if preview_dirty && self.dirty {
            self.status = "Save current buffer before promoting modified modal text".into();
            return;
        }
        let recovery_origin = if preview_dirty {
            self.recovery_origins.remove(&document_safety::Slot::Modal)
        } else {
            None
        };
        if preview_dirty {
            self.clear_recovery(document_safety::Slot::Main);
            self.document_safety
                .swap_roles(document_safety::Slot::Main, document_safety::Slot::Modal);
        }
        self.close_link_preview_force();
        if preview_dirty {
            self.cancel_file_open();
            self.clear_buffer_project();
            self.buffer.set_text(preview_text);
            self.path = Some(path.clone());
            self.disk_base = preview_base;
            if let Some(origin) = recovery_origin {
                self.recovery_origins
                    .insert(document_safety::Slot::Main, origin);
            }
            self.dirty = true;
            self.last_open_dir = path.parent().map(PathBuf::from);
            self.remember_recent_file(path);
            self.focus_editor_once = true;
            self.status = "Promoted modified modal buffer".to_string();
            self.observe_document_safety(0.0, true);
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
        self.request_file_open(
            path,
            heading_fragment
                .map(OpenDestination::Fragment)
                .unwrap_or(OpenDestination::Restore),
        );
    }

    fn save_link_preview(&mut self) {
        let Some(path) = self.link_preview_path.clone() else {
            self.status = "Modal buffer has no file path".to_string();
            return;
        };
        match safe_file::checked_save(
            &path,
            self.link_preview_buffer.as_str().as_bytes(),
            self.modal_disk_base,
        ) {
            Ok(base) => {
                self.modal_disk_base = Some(base);
                self.clear_recovery(document_safety::Slot::Modal);
                self.file_conflict = None;
                self.link_preview_dirty = false;
                self.last_open_dir = path.parent().map(PathBuf::from);
                self.remember_recent_file(path.clone());
                self.status = format!("Saved modal buffer {}", path.display());
            }
            Err(safe_file::SaveError::Conflict) => {
                self.file_conflict = Some((document_safety::Slot::Modal, path));
                self.status = "Modal save blocked: external change; text preserved".into();
            }
            Err(err) => self.status = format!("Save modal buffer failed: {err}"),
        }
    }

    fn swap_link_preview_with_main(&mut self) {
        self.cancel_file_open();
        self.remember_current_cursor_position();
        let main_cursor = self.buffer.cursor_line_col();
        let modal_cursor = self.link_preview_buffer.cursor_line_col();
        std::mem::swap(&mut self.buffer, &mut self.link_preview_buffer);
        std::mem::swap(&mut self.path, &mut self.link_preview_path);
        std::mem::swap(&mut self.dirty, &mut self.link_preview_dirty);
        std::mem::swap(&mut self.disk_base, &mut self.modal_disk_base);
        self.document_safety
            .swap_roles(document_safety::Slot::Main, document_safety::Slot::Modal);
        let main_origin = self.recovery_origins.remove(&document_safety::Slot::Main);
        self.safety_notified.clear();
        let modal_origin = self.recovery_origins.remove(&document_safety::Slot::Modal);
        if let Some(origin) = main_origin {
            self.recovery_origins
                .insert(document_safety::Slot::Modal, origin);
        }
        if let Some(origin) = modal_origin {
            self.recovery_origins
                .insert(document_safety::Slot::Main, origin);
        }
        self.file_conflict = None;

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
        self.clear_recovery(document_safety::Slot::Modal);
        self.modal_disk_base = None;
        self.link_preview_open = false;
        self.link_preview_path = None;
        self.link_preview_dirty = false;
        self.focus_link_preview_once = false;
        self.link_preview_heading_fragment = None;
        self.link_preview_heading_text = None;
        self.forget_project_buffer(self.link_preview_buffer.identity);
        self.link_preview_buffer.clear();
        self.focus_editor_once = true;
    }

    fn close_link_preview(&mut self) {
        if self.link_preview_dirty {
            self.status = "Save modal buffer before closing".to_string();
            self.focus_link_preview_once = true;
            return;
        }
        self.close_link_preview_force();
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
        match open_url_in_browser(browser_target) {
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

        let text = self.buffer.as_str().trim_end().to_string();
        if self.append_text_to_scratch_archive(&text) {
            self.dirty = false;
            true
        } else {
            false
        }
    }

    fn close_scratch_with_feedback(&mut self, ctx: &egui::Context) {
        if self.path.is_some()
            || self.link_preview_dirty
            || (self.dirty && self.buffer.as_str().trim().is_empty())
        {
            if self.dirty || self.link_preview_dirty {
                self.confirm(PendingAction::Quit);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        let previous_status = self.status.clone();
        if self.append_to_scratch_archive() {
            self.status = previous_status;
            let until = ctx.input(|input| input.time) + 0.4;
            self.scratch_close_after = Some(until);
            self.scratch_toast_until = Some(until);
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if !self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.status = format!("Scratch not fully archived; text kept: {}", self.status);
        }
    }

    fn scratch_feedback_toast(&self, ctx: &egui::Context) {
        scratch::feedback_toast(ctx, self.scratch_toast_until, "Scratch archived");
    }

    fn archive_scratch_modal(&mut self) {
        if self.scratch_buffer.as_str().trim().is_empty() {
            self.scratch_modal_open = false;
            self.forget_project_buffer(self.scratch_buffer.identity);
            self.scratch_buffer.clear();
            self.status = "Scratch cancelled".to_string();
            self.focus_editor_once = true;
            return;
        }
        let text = self.scratch_buffer.as_str().trim_end().to_string();
        if self.append_text_to_scratch_archive(&text) {
            self.forget_project_buffer(self.scratch_buffer.identity);
            self.scratch_buffer.clear();
            self.scratch_modal_open = false;
            self.focus_editor_once = true;
        }
    }

    fn cancel_scratch_modal(&mut self) {
        self.scratch_modal_open = false;
        self.focus_editor_once = true;
        self.status = if self.scratch_buffer.as_str().trim().is_empty() {
            self.forget_project_buffer(self.scratch_buffer.identity);
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
        let selected = self.scratch_entries[index].clone();
        let Some(path) = self.scratch_archive_path() else {
            return;
        };
        // Entries may have been archived since the list was read: delete
        // from the file as it is now, never from the old list.
        let current = match fs::read_to_string(&path) {
            Ok(text) => scratch::parse_entries(&text),
            Err(err) => {
                self.status = format!("Scratch delete failed: {err}");
                return;
            }
        };
        let Some(position) = current.iter().position(|entry| *entry == selected) else {
            self.load_scratch_entries();
            self.status = "That scratch entry changed on disk; list refreshed".to_string();
            return;
        };
        let mut entries = current;
        let deleted = entries.remove(position);
        match safe_file::atomic_replace(&path, scratch::serialize_entries(&entries).as_bytes()) {
            Ok(_) => {
                self.scratch_entries = entries;
                self.selected_scratch_entry =
                    index.min(self.scratch_entries.len().saturating_sub(1));
                self.status = format!("Deleted scratch entry {}", deleted.title());
            }
            Err(err) => self.status = format!("Scratch delete failed: {err}"),
        }
    }

    fn save_path(&mut self, path: PathBuf) {
        // Never two writers on a file: a background save lands first.
        self.finish_background_save();
        let project = self.document_project();
        let expected = if self.path.as_ref() == Some(&path) {
            self.disk_base
        } else {
            None
        };
        let result = safe_file::checked_save(&path, self.buffer.as_str().as_bytes(), expected);
        self.apply_save_result(path, project, result);
    }

    /// Ctrl+S / `:w`: the synced write (13–60 ms on this machine, ~400 ms for
    /// 5 MB on a slow disk) runs on a helper thread so typing never waits for
    /// the disk. The tab stays marked unsaved until the write is confirmed.
    fn save_in_background(&mut self, ctx: &egui::Context) {
        self.remember_current_cursor_position();
        let Some(path) = self.path.clone() else {
            self.save_as();
            return;
        };
        if self.sync_saves {
            self.save_path(path);
            return;
        }
        if self.pending_save.is_some() {
            self.save_again = true;
            return;
        }
        let project = self.document_project();
        let expected = self.disk_base;
        let text = self.buffer.as_str().to_owned();
        let (sender, result) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        let target = path.clone();
        std::thread::spawn(move || {
            let _ = sender.send(safe_file::checked_save(&target, text.as_bytes(), expected));
            wake.request_repaint();
        });
        self.pending_save = Some(PendingSave {
            path,
            identity: self.buffer.identity,
            revision: self.buffer.revision,
            project,
            result,
            started: std::time::Instant::now(),
        });
        self.status = "Saving…".into();
    }

    /// Applies a background save once its write has finished.
    fn poll_background_save(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_save else {
            return;
        };
        match pending.result.try_recv() {
            Ok(result) => {
                let pending = self.pending_save.take().unwrap();
                self.land_background_save(pending, result);
                if std::mem::take(&mut self.save_again) {
                    self.save_in_background(ctx);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_save = None;
                self.status = "Save failed: writer stopped; text kept unsaved".into();
            }
        }
    }

    /// Waits for a background save: before closing, quitting, moving a tab or
    /// any other save, so they see the file as it really is.
    fn finish_background_save(&mut self) {
        if let Some(pending) = self.pending_save.take() {
            let result = pending
                .result
                .recv()
                .unwrap_or_else(|_| Err(safe_file::SaveError::Io("writer stopped".into())));
            self.land_background_save(pending, result);
            self.save_again = false;
        }
    }

    fn land_background_save(
        &mut self,
        pending: PendingSave,
        result: Result<safe_file::Fingerprint, safe_file::SaveError>,
    ) {
        perf_log::mark(
            "save.background",
            &format!(
                "ms={:.1} ok={}",
                pending.started.elapsed().as_secs_f64() * 1000.0,
                result.is_ok()
            ),
        );
        if self.buffer.identity == pending.identity && self.path.as_ref() == Some(&pending.path) {
            let typed_since = self.buffer.revision != pending.revision;
            let saved = result.is_ok();
            self.apply_save_result(pending.path, pending.project, result);
            if saved && typed_since {
                // Saved what was there at Ctrl+S; newer typing stays unsaved
                // and gets its recovery checkpoint right away.
                self.dirty = true;
                self.observe_document_safety(0.0, true);
            }
            return;
        }
        // The saved document is in a background tab now.
        if !self.land_save_in_background_tab(&pending, &result) && result.is_ok() {
            // Its tab was closed meanwhile; the file on disk is what was saved.
            self.status = format!("Saved {}", pending.path.display());
        }
    }

    fn apply_save_result(
        &mut self,
        path: PathBuf,
        project: Option<PathBuf>,
        result: Result<safe_file::Fingerprint, safe_file::SaveError>,
    ) {
        match result {
            Ok(base) => {
                self.disk_base = Some(base);
                self.clear_recovery(document_safety::Slot::Main);
                self.file_conflict = None;
                self.path = Some(path.clone());
                self.last_open_dir = path.parent().map(PathBuf::from);
                self.dirty = false;
                self.remember_recent_file(path.clone());
                self.adopt_tab_alias(&path);
                let _ = self.save_settings();
                self.status = format!("Saved {}", path.display());
                self.notify_modules_saved();
                if let Err(error) = self.persist_saved_project(&path, project) {
                    self.status
                        .push_str(&format!(" · project metadata not saved: {error}"));
                }
            }
            Err(safe_file::SaveError::Conflict) => {
                self.file_conflict = Some((document_safety::Slot::Main, path));
                self.status =
                    "Save blocked: external change / existing destination; text preserved".into();
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn new_buffer(&mut self) {
        if self.dirty {
            self.status = "Save or discard changes before creating a new buffer".into();
            return;
        }
        self.clear_recovery(document_safety::Slot::Main);
        self.disk_base = None;
        self.file_conflict = None;
        self.cancel_file_open();
        self.remember_current_cursor_position();
        self.clear_buffer_project();
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
        // One picker at a time: the others would cover this one.
        self.file_picker_open = false;
        self.lance_open = false;
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
        // Only a mark the filter shows: a hidden one is not "selected".
        if !self
            .lance_file_indices()
            .contains(&self.selected_lance_file)
        {
            self.status = "No matching lance file".to_string();
            return;
        }
        let removed = self.lance_files.remove(self.selected_lance_file);
        let visible = self.lance_file_indices();
        self.selected_lance_file = visible
            .iter()
            .copied()
            .find(|index| *index >= self.selected_lance_file)
            .or_else(|| visible.last().copied())
            .unwrap_or(0);
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
                self.assign_document_project(path);
                return;
            }
            if self.file_picker_mode == FilePickerMode::AddVault {
                self.file_picker_open = false;
                self.add_vault(path);
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
            FilePickerMode::OpenBuffer => {
                self.file_picker_open = false;
                self.open_link_preview(path, None);
            }
            FilePickerMode::SetPeriscopeProject => {
                self.status = "Choose a folder for this document's project".to_string();
            }
            FilePickerMode::AddVault => {
                self.status =
                    "Choose a folder: Enter on it, or Ctrl+Enter for the current one".to_string();
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
        self.begin_periscope_setup();
    }
    fn choose_periscope_scope(&mut self) {
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

        self.periscope_project_root = self.document_project();
        if self.periscope_project_root.is_none() {
            self.open_global_periscope();
            return;
        }
        self.periscope_open = false;
        self.periscope_prompt_open = true;
        self.status = "Document has a project · choose project or global".into();
    }

    fn open_global_periscope(&mut self) {
        if self.periscope_setup.visible {
            return;
        }
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

    fn poll_periscope_global_index(&mut self) {
        if !self.periscope_open {
            if self.periscope_request.take().is_some() {
                self.periscope_engine.cancel();
            }
            return;
        }
        if let Some(result) = self.periscope_engine.poll() {
            if self.periscope_request != Some(result.id) {
                return;
            }
            let selected_path = self.selected_periscope_row();
            self.periscope_results = result.paths;
            self.add_alias_matches_to_periscope();
            self.periscope_backend_status = result.status;
            self.periscope_global_loading = false;
            self.selected_periscope_result = selected_path
                .and_then(|selected| {
                    self.periscope_rows()
                        .iter()
                        .position(|row| match (&selected, row) {
                            (
                                PeriscopeRow::File { path: a, .. },
                                PeriscopeRow::File { path: b, .. },
                            )
                            | (
                                PeriscopeRow::Folder { path: a, .. },
                                PeriscopeRow::Folder { path: b, .. },
                            ) => a == b,
                            _ => false,
                        })
                })
                .unwrap_or(0);
        }
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
        if self.periscope_setup.visible {
            return;
        }
        self.periscope_results.clear();
        self.periscope_global_loading = true;
        self.periscope_backend_status = "Searching index… · latest query wins".into();
        self.periscope_request = Some(self.periscope_engine.request(periscope_backend::Request {
            query: self.periscope_query.clone(),
            project: if self.periscope_mode == PeriscopeMode::Project {
                self.periscope_project_root.clone()
            } else {
                None
            },
            roots: self.periscope_roots.clone(),
            everything: self.periscope_everything,
        }));
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
        self.periscope_open = false;
        self.open_keeping_unsaved_text(path, None);
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
        let project = self.document_project();
        self.save_as_dir = self
            .path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or(project)
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

    /// Reads only `single_instance` so `main` can decide before creating a window.
    fn config_single_instance_enabled() -> bool {
        Self::settings_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    let (key, value) = line.split_once('=')?;
                    (key.trim() == "single_instance")
                        .then(|| Self::parse_config_bool(value.trim()))
                        .flatten()
                })
            })
            .unwrap_or(true)
    }

    fn settings_path() -> Option<PathBuf> {
        let mut dir = dirs_next::config_dir()?;
        dir.push("slate");
        Some(dir.join("config.toml"))
    }

    fn load_settings(&mut self) {
        let path = Self::settings_path();
        #[cfg(test)]
        let path = self.settings_path_override.clone().or(path);
        let Some(path) = path else {
            return;
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };

        self.periscope_roots.clear();
        self.config_passthrough.clear();
        self.vaults.clear();
        for line in contents.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                key if footer_ui::Options::key_index(key).is_some() => {
                    if let Ok(enabled) = value.trim().parse::<bool>() {
                        self.footer_options.enabled[footer_ui::Options::key_index(key).unwrap()] =
                            enabled;
                    }
                }
                "periscope_everything" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.periscope_everything = enabled;
                    }
                }
                "periscope_root" => {
                    let path = PathBuf::from(Self::parse_config_string(value));
                    if path.is_absolute() && !self.periscope_roots.contains(&path) {
                        self.periscope_roots.push(path);
                    }
                }
                "editor_font_size" => {
                    if let Ok(size) = value.trim().parse::<f32>() {
                        self.apply_editor_font_size(size);
                    }
                }
                "editor_font" => {
                    if let Some(font) = editor_view::EditorFont::from_config(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            font,
                            ..self.editor_look
                        });
                    }
                }
                "block_cursor" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            block_cursor: enabled,
                            ..self.editor_look
                        });
                    }
                }
                "cursor_animation" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            animate_cursor: enabled,
                            ..self.editor_look
                        });
                    }
                }
                "cursor_trail" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            cursor_trail: enabled,
                            ..self.editor_look
                        });
                    }
                }
                "cursor_trail_weight" => {
                    if let Ok(weight) = value.trim().parse::<u8>() {
                        self.apply_editor_look(editor_view::EditorLook {
                            cursor_trail_weight: editor_view::clamp_trail_weight(weight),
                            ..self.editor_look
                        });
                    }
                }
                "cursor_trail_typing" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            cursor_trail_typing: enabled,
                            ..self.editor_look
                        });
                    }
                }
                "cursor_stretch" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            cursor_stretch: enabled,
                            ..self.editor_look
                        });
                    }
                }
                "cursor_pulse" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            cursor_pulse: enabled,
                            ..self.editor_look
                        });
                    }
                }
                "syntax_highlighting" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            syntax_highlighting: enabled,
                            ..self.editor_look
                        });
                    }
                }
                "cursor_color" => {
                    if let Some(color) = editor_view::color_from_hex(value) {
                        self.apply_editor_look(editor_view::EditorLook {
                            cursor_color: color,
                            ..self.editor_look
                        });
                    }
                }
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
                "single_instance" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.single_instance = enabled;
                    }
                }
                "resident" => {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.resident = enabled;
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
                // Settings of modules (or newer versions) are kept as they
                // are, so saving here never loses them.
                "vault" => {
                    let folder = Self::parse_config_string(value);
                    if !folder.is_empty() {
                        vaults::add(&mut self.vaults, PathBuf::from(folder));
                    }
                }
                key if key.starts_with("module.")
                    && self
                        .module_host
                        .list()
                        .iter()
                        .any(|(id, ..)| *id == &key[7..]) =>
                {
                    if let Some(enabled) = Self::parse_config_bool(value) {
                        self.module_host.set_enabled(&key[7..], enabled);
                    }
                }
                _ => self.config_passthrough.push(line.to_string()),
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
        let path = Self::settings_path();
        #[cfg(test)]
        let path = self.settings_path_override.clone().or(path);
        let Some(path) = path else {
            return Err("no config dir".to_string());
        };
        let parent = path
            .parent()
            .ok_or_else(|| "invalid config path".to_string())?;
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        self.save_session();
        let mut contents = format!(
            "command_history_limit = {}\nline_number_mode = \"{}\"\nword_wrap = {}\npreview_mode = {}\nctrl_shift_move_mode = \"{}\"\nreopen_last_file_on_startup = {}\nsingle_instance = {}\nresident = {}\nmarkdown_live_rendering = {}\nlast_opened_path = \"{}\"\nlast_open_dir = \"{}\"\n",
            self.command_history_limit,
            self.line_number_mode.config_value(),
            self.wrap,
            self.preview,
            self.ctrl_shift_move_mode.config_value(),
            self.reopen_last_file_on_startup,
            self.single_instance,
            self.resident,
            self.markdown_live_rendering,
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
        contents.push_str(&format!("editor_font_size = {}\n", self.editor_font_size));
        contents.push_str(&format!(
            "editor_font = \"{}\"\nblock_cursor = {}\ncursor_animation = {}\ncursor_trail = {}\ncursor_trail_weight = {}\ncursor_trail_typing = {}\ncursor_stretch = {}\ncursor_pulse = {}\nsyntax_highlighting = {}\ncursor_color = \"{}\"\n",
            self.editor_look.font.config_value(),
            self.editor_look.block_cursor,
            self.editor_look.animate_cursor,
            self.editor_look.cursor_trail,
            self.editor_look.cursor_trail_weight,
            self.editor_look.cursor_trail_typing,
            self.editor_look.cursor_stretch,
            self.editor_look.cursor_pulse,
            self.editor_look.syntax_highlighting,
            editor_view::color_to_hex(self.editor_look.cursor_color)
        ));
        contents.push_str(&format!(
            "periscope_everything = {}\n",
            self.periscope_everything
        ));
        for root in &self.periscope_roots {
            contents.push_str(&format!(
                "periscope_root = \"{}\"\n",
                Self::escape_config_string(&root.display().to_string())
            ));
        }
        for (index, (key, _)) in footer_ui::FIELDS.iter().enumerate() {
            contents.push_str(&format!("{key} = {}\n", self.footer_options.enabled[index]));
        }
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
        for (id, _, _, enabled) in self.module_host.list() {
            contents.push_str(&format!("module.{id} = {enabled}\n"));
        }
        for vault in &self.vaults {
            contents.push_str(&format!(
                "vault = \"{}\"\n",
                Self::escape_config_string(&vault.display().to_string())
            ));
        }
        for line in &self.config_passthrough {
            contents.push_str(line);
            contents.push('\n');
        }
        // Off the UI thread; written before the process exits.
        settings_writer::write(
            path,
            contents.into_bytes(),
            settings_writer::Durability::Quick,
        );
        Ok(())
    }

    fn set_command_history_limit(&mut self, limit: usize) {
        self.command_history_limit = clamp_history_limit(limit);
        match self.save_settings() {
            Ok(_) => self.status = format!("History length: {}", self.command_history_limit),
            Err(err) => self.status = format!("Settings save failed: {err}"),
        }
    }

    fn apply_editor_font_size(&mut self, size: f32) {
        self.editor_font_size = editor_view::clamp_editor_font_size(size);
        self.editor_view.set_font_size(self.editor_font_size);
        self.link_preview_view.set_font_size(self.editor_font_size);
        self.scratch_view.set_font_size(self.editor_font_size);
    }

    fn apply_editor_look(&mut self, look: editor_view::EditorLook) {
        self.editor_look = look;
        self.editor_view.set_look(look);
        self.link_preview_view.set_look(look);
        self.scratch_view.set_look(look);
    }

    fn set_editor_look(&mut self, look: editor_view::EditorLook) {
        self.apply_editor_look(look);
        let summary = format!(
            "Editor: {} · {} {} cursor{}{}{}{}",
            look.font.label(),
            editor_view::CURSOR_COLORS
                .iter()
                .find(|(_, color)| *color == look.cursor_color)
                .map_or_else(
                    || editor_view::color_to_hex(look.cursor_color),
                    |(name, _)| (*name).to_string()
                ),
            if look.block_cursor { "block" } else { "line" },
            if look.animate_cursor {
                " · animated"
            } else {
                ""
            },
            if look.animate_cursor && look.cursor_stretch {
                " · stretch"
            } else {
                ""
            },
            if look.cursor_pulse { " · pulse" } else { "" },
            if look.cursor_trail {
                format!(
                    " · trail {}{}",
                    look.cursor_trail_weight,
                    if look.cursor_trail_typing {
                        " (also typing)"
                    } else {
                        ""
                    }
                )
            } else {
                String::new()
            }
        );
        self.status = match self.save_settings() {
            Ok(()) => summary,
            Err(err) => format!("{summary}; settings save failed: {err}"),
        };
    }

    fn set_editor_font_size(&mut self, size: f32) {
        self.apply_editor_font_size(size);
        self.status = match self.save_settings() {
            Ok(()) => format!("Editor font: {} pt", self.editor_font_size),
            Err(err) => format!("Editor font changed; settings save failed: {err}"),
        };
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
        if command != Command::Save {
            // Closing, quitting, opening...: decide on the file as it is.
            self.finish_background_save();
        }
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
            Command::Module(name) => self.run_module_command(name, "", ctx),
            Command::New => {
                if self.scratch {
                    if self.dirty {
                        self.confirm(PendingAction::New);
                    } else {
                        self.new_buffer();
                    }
                } else {
                    self.new_tab();
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
            Command::CreateProject => self.create_document_project(),
            Command::Lance => self.open_lance(),
            Command::LanceAdd => self.lance_add_current(),
            Command::LanceNext => self.open_next_lance_file(1),
            Command::LancePrev => self.open_next_lance_file(-1),
            Command::Save => self.save_in_background(ctx),
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
            Command::FindNote => self.open_find_note_results(""),
            Command::OpenLink => self.open_link_under_cursor(),
            Command::LineNumbersAbsolute => self.set_line_number_mode(LineNumberMode::Absolute),
            Command::LineNumbersRelative => self.set_line_number_mode(LineNumberMode::Relative),
            Command::PerfHud => self.perf_hud.toggle(),
            Command::Settings => {
                self.settings_open = true;
                self.selected_setting = 0;
                self.settings_last_selected = None;
                self.focus_editor_once = false;
            }
            Command::Quit => self.request_close(ctx),
            Command::QuitAll => {
                self.quit_for_real = true;
                self.request_close(ctx);
            }
            Command::CloseBuffer => self.close_active_tab(),
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
                let expanded =
                    Self::resolve_command_path(&file_dialogs::normalize_drive_path(&path));
                if expanded.is_dir() {
                    self.record_command_usage("open");
                    self.open_file_picker_at(expanded, FilePickerMode::Open);
                } else {
                    self.record_command_usage("open");
                    self.open_keeping_unsaved_text(expanded, None);
                }
            }
            CommandLineAction::Alias(alias) => {
                self.record_command_usage("alias");
                self.rename_tab(self.tabs.active, Some(&alias));
            }
            CommandLineAction::BrowsePath(path) => {
                self.record_command_usage("cd");
                let dir = Self::resolve_command_path(&file_dialogs::normalize_drive_path(&path));
                if path.trim().is_empty() {
                    self.open_file_picker_for_open();
                } else if dir.is_dir() {
                    self.open_file_picker_at(dir, FilePickerMode::Browse);
                } else {
                    self.status = format!("Not an accessible directory: {}", dir.display());
                }
            }
            CommandLineAction::OpenBufferPath(path) => {
                self.record_command_usage("open-buffer");
                let expanded = Self::resolve_command_path(&path);
                self.open_link_preview(expanded, None);
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
            CommandLineAction::Unknown(input) => {
                let (token, args) = input.split_once(' ').unwrap_or((input.as_str(), ""));
                match commands::canonical_command_name(token)
                    .filter(|name| self.module_host.command_owner(name).is_some())
                {
                    Some(name) => {
                        self.record_command_usage(name);
                        self.run_module_command(name, args.trim(), ctx);
                    }
                    None => self.status = format!("Unknown command: {input}"),
                }
            }
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

        let started = std::time::Instant::now();
        let state = SearchState::new(query, self.buffer.as_str(), self.buffer.revision);
        self.perf_hud
            .record_find(started.elapsed(), state.matches.len());
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
        if !self.scratch
            && matches!(
                action,
                PendingAction::Open
                    | PendingAction::OpenLast
                    | PendingAction::OpenRecent
                    | PendingAction::OpenProjectFile
            )
        {
            // The unsaved document stays in its tab; continue in a new one.
            self.new_tab();
            self.pending_tab_action = Some(action);
            return;
        }
        self.cancel_file_open();
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
                    self.request_file_open(
                        path,
                        heading_fragment
                            .map(OpenDestination::Fragment)
                            .unwrap_or(OpenDestination::Restore),
                    );
                }
            }
            PendingAction::CloseTab => self.close_active_tab_now(),
            PendingAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn request_close(&mut self, ctx: &egui::Context) {
        self.finish_background_save();
        if self.resident_active && !self.quit_for_real {
            // Hiding keeps every tab, saved or not: nothing to ask.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.dirty && self.activate_first_dirty_tab() {
            self.confirm(PendingAction::Quit);
            return;
        }
        if self.scratch && self.path.is_none() && !self.link_preview_dirty {
            self.close_scratch_with_feedback(ctx);
        } else if self.dirty || self.link_preview_dirty {
            self.confirm(PendingAction::Quit);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_window_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        self.finish_background_save();
        if self.scratch_close_after.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        if self.resident_active && !self.quit_for_real {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
            return;
        }
        if !self.dirty && self.activate_first_dirty_tab() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm(PendingAction::Quit);
            return;
        }
        if self.scratch && self.path.is_none() && self.dirty && !self.link_preview_dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_scratch_with_feedback(ctx);
        } else if self.dirty || self.link_preview_dirty {
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
            && !self.lance_open
            && !self.doc_tasks_open
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
        let mut select_all = false;
        let mut copy = false;
        let mut paste = None;
        let mut cut = false;

        ctx.input_mut(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.alt
                        && !modifiers.shift
                        && (modifiers.ctrl || modifiers.command) =>
                    {
                        match key {
                            Key::Z => undo = true,
                            Key::Y => redo = true,
                            Key::A => select_all = true,
                            Key::C => copy = true,
                            Key::X => cut = true,
                            _ => {}
                        }
                    }
                    egui::Event::Copy => copy = true,
                    egui::Event::Paste(text) => paste = Some(text.clone()),
                    egui::Event::Cut => cut = true,
                    _ => {}
                }
            }

            let any_action = undo || redo || select_all || copy || paste.is_some() || cut;
            if any_action {
                input.events.retain(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.alt
                        && !modifiers.shift
                        && (modifiers.ctrl || modifiers.command) =>
                    {
                        !matches!(key, Key::Z | Key::Y | Key::A | Key::C | Key::V | Key::X)
                    }
                    egui::Event::Copy | egui::Event::Paste(_) | egui::Event::Cut => false,
                    _ => true,
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

        if select_all {
            self.buffer.set_selection(0, self.buffer.as_str().len());
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Select all".to_string();
            self.ctrl_layer_active = false;
            self.ctrl_layer_sequence.clear();
            return true;
        }

        if copy {
            if let Some((start, end)) = self.buffer.selection() {
                ctx.copy_text(self.buffer.as_str()[start..end].to_string());
                self.status = "Copied selection".to_string();
            } else {
                self.status = "No selection to copy".to_string();
            }
            self.ctrl_layer_active = false;
            self.ctrl_layer_sequence.clear();
            return true;
        }

        if cut {
            if let Some((start, end)) = self.buffer.selection() {
                ctx.copy_text(self.buffer.as_str()[start..end].to_string());
                self.buffer.replace_selection_or_range(start, end, "");
                self.dirty = true;
                self.editor_view.request_scroll_to_cursor(&self.buffer);
                self.status = "Cut selection".to_string();
            } else {
                self.status = "No selection to cut".to_string();
            }
            self.ctrl_layer_active = false;
            self.ctrl_layer_sequence.clear();
            return true;
        }

        if let Some(text) = paste {
            let normalized = EditorView::normalize_paste_text(&text);
            self.buffer.insert_text(&normalized);
            self.dirty = true;
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.status = "Pasted".to_string();
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
            if self.handle_tab_switch_input(ctx) {
                return true;
            }
            let keys = ctx.input(|input| input.events.clone());
            if self.handle_ctrl_shift_navigation(&keys) {
                return true;
            }

            let mut acted = false;
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
                    // Tab keys act at once, like Ctrl+1…9, so holding Ctrl and
                    // tapping W closes one tab per tap. No sequence starts with
                    // t, w or q, so `dw`, `gt`, … keep working.
                    if self.ctrl_layer_sequence.is_empty() && matches!(ch, 't' | 'w' | 'q') {
                        acted = true;
                        self.dispatch_ctrl_layer(&ch.to_string(), ctx);
                        if ch == 'q' {
                            return true;
                        }
                        continue;
                    }
                    self.ctrl_layer_sequence.push(ch);
                }
            }
            if acted && self.ctrl_layer_sequence.is_empty() {
                // Keep the result of an immediate action visible.
                return true;
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
            "t" => self.run_command(Command::New, ctx),
            "w" => self.run_command(Command::CloseBuffer, ctx),
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
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)))
            .corner_radius(0.0)
            .inner_margin(14.0)
            .shadow(egui::epaint::Shadow {
                offset: [0, 10],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(140),
            });

        let selection = (self.selected_command, self.palette_query.clone());
        let selection_id = egui::Id::new("palette_scroll_selection");
        let reveal_selection =
            ctx.data(|d| d.get_temp::<(usize, String)>(selection_id)) != Some(selection.clone());
        ctx.data_mut(|d| d.insert_temp(selection_id, selection));
        window_ui::DialogArea::new("command_palette".into())
            .fixed_chrome()
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                frame.show(ui, |ui| {
                    ui.set_width((ctx.content_rect().width() * 0.8 - 30.0).max(1.0));
                    ui.set_height(((ctx.content_rect().height() - 30.0) * 0.8 - 30.0).max(1.0));
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
                        Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
                    );
                    ui.add_space(8.0);

                    egui::ScrollArea::vertical()
                        .id_salt("command_palette_body")
                        .auto_shrink([false, false])
                        .min_scrolled_height(1.0)
                        .max_height((ui.available_height() - 48.0).max(1.0))
                        .show(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
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
                                let response = row
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
                                                            .font(FontId::new(
                                                                13.0,
                                                                FontFamily::Monospace,
                                                            ))
                                                            .color(Color32::from_rgb(
                                                                136, 154, 176,
                                                            )),
                                                    );
                                                },
                                            );
                                        })
                                    })
                                    .response;
                                if selected && reveal_selection {
                                    response.scroll_to_me(Some(egui::Align::Min));
                                }
                                if response.clicked() {
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
                        });
                    if !self.palette_open {
                        return;
                    }
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
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

        if !discard && !go_back && !save {
            let clicked = notice_ui::show(
                ctx,
                "confirm_close_prompt".into(),
                "unsaved changes",
                &[
                    ("y", "discard", notice_ui::RED),
                    ("n / esc", "return", notice_ui::GREEN),
                    ("s", "save…", notice_ui::WARN),
                ],
                |ui| {
                    ui.label(action.prompt());
                    ui.add_space(10.0);
                    ui.label(RichText::new("Save your text before continuing, or explicitly discard the unsaved changes.").color(notice_ui::DIM));
                    if action == PendingAction::Quit && self.link_preview_dirty {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("The preview also has unsaved changes.")
                                .color(notice_ui::WARN),
                        );
                    }
                },
            );
            discard |= clicked[0];
            go_back |= clicked[1];
            save |= clicked[2];
        }
        if discard {
            if action == PendingAction::CloseTab {
                // Discarded text stays reachable with Ctrl+Shift+T.
                self.tabs.discarded_close = true;
            }
            self.dirty = false;
            self.clear_recovery(document_safety::Slot::Main);
            if action == PendingAction::Quit {
                self.discard_on_exit = true;
                self.link_preview_dirty = false;
                self.clear_recovery(document_safety::Slot::Modal);
                self.clear_recovery(document_safety::Slot::Scratch);
            }
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
            if self.dirty {
                self.save();
            }
            if action == PendingAction::Quit && self.link_preview_dirty {
                self.save_link_preview();
            }
            if !self.dirty && (action != PendingAction::Quit || !self.link_preview_dirty) {
                self.pending_action = None;
                self.finish_pending_action(action, ctx);
            }
            return;
        }
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

        window_ui::DialogArea::new("link_assist_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)))
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
                            };
                            let hint = match self.link_heading_picker_mode {
                                LinkHeadingPickerMode::InsertLink => {
                                    "Enter on first option links the whole file"
                                }
                                LinkHeadingPickerMode::BidirectionalLink => {
                                    "Enter chooses where to write the reciprocal link"
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
        self.render_modal_form(ctx);
    }

    fn periscope_prompt_dialog(&mut self, ctx: &egui::Context) {
        if self.periscope_setup.visible {
            self.periscope_setup_dialog(ctx);
            return;
        }
        self.render_periscope_prompt(ctx);
    }

    fn scratch_modal_dialog(&mut self, ctx: &egui::Context) {
        self.render_scratch_form(ctx);
    }

    fn capture_dialog(&mut self, ctx: &egui::Context) {
        self.render_capture_form(ctx);
    }

    fn scratch_entries_dialog(&mut self, ctx: &egui::Context) {
        if !self.scratch_entries_open {
            return;
        }

        window_ui::DialogArea::new("scratch_entries_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 31, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)))
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
                                    Stroke::new(1.0_f32, Color32::from_rgb(59, 69, 89)),
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
            Stroke::new(1.2_f32, stroke),
            egui::StrokeKind::Outside,
        );
        let glyph_rect = rect.shrink(3.2);
        match state {
            CheckboxState::Empty => {}
            CheckboxState::Doing => {
                ui.painter().line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5_f32, stroke),
                );
            }
            CheckboxState::Done => {
                ui.painter().line_segment(
                    [glyph_rect.left_top(), glyph_rect.right_bottom()],
                    Stroke::new(1.5_f32, stroke),
                );
                ui.painter().line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5_f32, stroke),
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
                        format.underline = Stroke::new(1.0_f32, color);
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
    ) -> egui::Rect {
        if rows.is_empty() {
            return egui::Rect::NOTHING;
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
        if !ui.is_rect_visible(rect) {
            ui.add_space(6.0);
            return rect;
        }
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
            Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        );
        let mut x = rect.left();
        for width in column_widths.iter().take(column_count.saturating_sub(1)) {
            x += *width;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
            );
        }
        if rows.len() > 1 {
            let header_y = rect.top() + row_height;
            painter.line_segment(
                [
                    egui::pos2(rect.left(), header_y),
                    egui::pos2(rect.right(), header_y),
                ],
                Stroke::new(1.0_f32, strong_border),
            );
        }
        for row_index in 2..rows.len() {
            let y = rect.top() + row_index as f32 * row_height;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0_f32, Color32::from_rgb(36, 45, 58)),
            );
        }
        ui.add_space(6.0);
        rect
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

    fn prepare_preview_wheel_sync(&mut self, ui: &egui::Ui) {
        let valid = self.preview_sync.key.is_some_and(|(id, revision, _, _)| {
            id == self.buffer.identity && revision == self.buffer.revision
        });
        let delta = ui.input(|i| i.smooth_scroll_delta.y);
        if valid && delta != 0.0 && ui.rect_contains_pointer(ui.max_rect()) {
            let offset =
                (self.preview_sync.offset - delta).clamp(0.0, self.preview_sync.max_scroll);
            self.editor_view
                .set_scroll_position(self.preview_sync.position_for(offset));
        }
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
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(53, 66, 86)))
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
}

impl eframe::App for SlateApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.spare_server.is_some() {
            // An unclaimed spare holds nothing, and its settings may be stale.
            return;
        }
        self.observe_document_safety(0.0, true);
        self.remember_current_cursor_position();
        if !self.discard_on_exit {
            if self.scratch_modal_open || !self.scratch_buffer.as_str().is_empty() {
                self.archive_scratch_modal();
            }
            self.append_to_scratch_archive();
        }
        self.observe_document_safety(0.0, true);
        self.document_safety.flush();
        let _ = self.save_settings();
        if self.resident_active {
            resident::remove_tray();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let started = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        window_drag::note_frame();
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F12)) {
            self.perf_hud.toggle();
        }
        let had_input = ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Text(_)
                        | egui::Event::Paste(_)
                        | egui::Event::Key { pressed: true, .. }
                )
            })
        });
        self.ui_frame(ui);
        self.perf_hud
            .record_frame(started.elapsed(), had_input, frame.info().cpu_usage);
        if self.perf_hud.visible {
            let stats = perf_hud::DocumentStats {
                bytes: self.buffer.as_str().len(),
                lines: self.buffer.line_count(),
                undo_bytes: self.buffer.undo_history_bytes(),
            };
            self.perf_hud.show(&ctx, &stats);
        }
    }
}

impl SlateApp {
    fn ui_frame(&mut self, ui: &mut egui::Ui) {
        let mut ui_timing = perf_log::ui_frame();
        release_prewarmed_gpu();
        let ctx = ui.ctx().clone();
        self.editor_view
            .set_file_language(self.path.as_deref().and_then(highlight::Lang::from_path));
        if let Some(shown) = self.shown_at.take() {
            perf_log::mark(
                "resident.first_frame_painted",
                &format!("ms={:.3}", shown.elapsed().as_secs_f64() * 1000.0),
            );
        }
        let title = self.title();
        if title != self.last_window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_window_title = title;
        }
        self.poll_background_save(&ctx);
        self.poll_modules(&ctx);
        self.poll_file_open(&ctx);
        self.poll_instance_requests(&ctx);
        self.poll_spare(&ctx);
        self.poll_dropped_tabs(&ctx);
        self.poll_tab_drag(&ctx);
        self.poll_resident(&ctx);
        if let Some(action) = self.pending_tab_action.take() {
            self.finish_pending_action(action, &ctx);
        }
        self.poll_periscope_setup(&ctx);
        self.poll_periscope_global_index();
        if self.periscope_open {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.poll_document_safety(&ctx);
        if !self.window_hidden {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        ui_timing.step("poll_background_and_requests");
        if let Some(close_after) = self.scratch_close_after {
            let now = ctx.input(|input| input.time);
            if now >= close_after {
                self.scratch_close_after = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        if let Some(toast_until) = self.scratch_toast_until {
            let now = ctx.input(|input| input.time);
            if now >= toast_until {
                self.scratch_toast_until = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        self.handle_window_close_request(&ctx);
        if !window_ui::renaming_tab(&ctx) {
            self.shortcuts(&ctx);
        }
        let labels = if self.scratch {
            Vec::new()
        } else {
            self.tab_labels()
        };
        match window_ui::titlebar_with_tabs(ui, &self.title(), &labels, self.tabs.active) {
            Some(window_ui::TabAction::Activate(index)) => self.activate_tab(index),
            Some(window_ui::TabAction::Close(index)) => {
                // Decide on "unsaved?" with the file as it really is.
                self.finish_background_save();
                self.close_tab(index)
            }
            Some(window_ui::TabAction::New) => self.new_tab(),
            Some(window_ui::TabAction::Move { from, to }) => self.move_tab(from, to),
            Some(window_ui::TabAction::Rename { index, alias }) => {
                self.rename_tab(index, Some(&alias))
            }
            Some(window_ui::TabAction::TearOff { index }) => self.tear_off_tab(index, &ctx),
            Some(window_ui::TabAction::Detach { index, at }) => {
                // Moving a window's only tab away closes (or hides) the window.
                let only_tab = self.tabs_len() == 1;
                if self.detach_tab(index, at) && only_tab {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            None => {}
        }
        if self.recovery_open || self.file_conflict.is_some() {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.label("Your current text is preserved while this notice is open.");
            });
            self.document_safety_dialog(&ctx);
            self.observe_document_safety(ctx.input(|i| i.time), false);
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
                // Code files are coloured as code, never rendered as Markdown.
                let is_code_file = self
                    .path
                    .as_deref()
                    .and_then(highlight::Lang::from_path)
                    .is_some();
                let editor_keyboard_enabled = self.duplicate_placement.is_none()
                    && !self.suppress_editor_keyboard_once
                    && !command_line_active
                    && !self.palette_open
                    && !self.settings_open
                    && !self.recent_picker_open
                    && !self.lance_open
                    && !self.doc_tasks_open
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
                                self.prepare_preview_wheel_sync(&columns[1]);
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
                                    self.notify_modules_edited();
                                }
                                if let Some(byte) = self.editor_view.take_link_click_byte() {
                                    self.open_markdown_link_at_byte(byte);
                                }
                                if let Some(target) = self.editor_view.take_link_click_target() {
                                    self.open_markdown_link_target(&target);
                                }
                                if let Some(trigger_start) = self.editor_view.take_link_assist_trigger_start() {
                                    self.open_link_assist(trigger_start);
                                }
                                let typed = self.editor_view.take_last_text_input();
                                if !self.detect_inline_bidirectional_link_command(typed.as_deref()) {
                                    self.refresh_link_inline_state(typed.as_deref());
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
                                false,
                                active_line_text_highlight,
                                self.markdown_live_rendering && !is_code_file,
                                None,
                            );
                            if self.focus_editor_once
                                && !self.palette_open
                                && !self.settings_open
                                && !self.recent_picker_open
                                && !self.lance_open
                                && !self.doc_tasks_open
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
                                self.notify_modules_edited();
                            }
                            if let Some(byte) = self.editor_view.take_link_click_byte() {
                                self.open_markdown_link_at_byte(byte);
                            }
                            if let Some(target) = self.editor_view.take_link_click_target() {
                                self.open_markdown_link_target(&target);
                            }
                            if let Some(trigger_start) = self.editor_view.take_link_assist_trigger_start() {
                                self.open_link_assist(trigger_start);
                            }
                            let typed = self.editor_view.take_last_text_input();
                            if !self.detect_inline_bidirectional_link_command(typed.as_deref()) {
                                self.refresh_link_inline_state(typed.as_deref());
                            }
                        }
                    },
                );

                self.link_inline_suggestion_popup(&ctx);

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
                } else if self.periscope_setup.visible {
                    "periscope setup"
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

                let (status_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), status_height),
                    egui::Sense::hover(),
                );
                self.compact_footer(ui,status_rect,active_mode);

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
                let active=self.command_line_focused || self.focus_command_line_once;
                let (command_text_rect,open_palette)=footer_ui::palette_launcher(ui,command_rect,!(active && !self.command_line.is_empty()));
                if open_palette {self.open_command_palette();}
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
                        let hint="command  Ctrl+. enter · Tab complete · Ctrl+H help";
                        paint_command_line(footer_ui::fitted_hints(&painter,&footer_font,(command_text_rect.width()-20.0).max(0.0),hint,footer_dim,footer_accent));
                        ui.interact(command_text_rect,ui.id().with("command_hints"),egui::Sense::hover()).on_hover_ui(|ui| {footer_ui::style_hints(ui);ui.label(hint);});
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
                        Stroke::new(1.0_f32, footer_accent),
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
                    } else if self.file_picker_open && matches!(self.file_picker_mode, FilePickerMode::SetPeriscopeProject | FilePickerMode::AddVault) {
                        ("create project  Enter choose folder · → navigate · Ctrl+Enter use current folder · Esc cancel".into(),footer_accent)
                    } else if self.file_picker_open {
                        ("files  type filter · ↑↓ select · → enter folder · ← parent · Enter open · Esc close".to_string(), footer_accent)
                    } else if self.periscope_open {
                        ("periscope  type search · ↑↓ select · Tab project/global · Enter open · Ctrl+Enter modal".to_string(), footer_accent)
                    } else if self.find_note_open {
                        ("find-note  type filter · ↑↓ select · Tab/←/→ headings · Enter main · Shift+Enter front modal".to_string(), footer_accent)
                    } else if self.periscope_setup.visible {
                        (self.periscope_setup.hint().to_string(),footer_accent)
                    } else if self.periscope_prompt_open {
                        ("periscope  Y/Enter project · N global · Esc cancel".to_string(), footer_accent)
                    } else if self.link_inline_anchor.is_some() {
                        ("link  type fuzzy · ↑↓ select · Enter file · Tab/Shift+Enter headings · ] cancel/manual".to_string(), footer_accent)
                    } else if self.link_heading_picker_open {
                        let action = match self.link_heading_picker_mode {
                            LinkHeadingPickerMode::InsertLink => "insert",
                            LinkHeadingPickerMode::BidirectionalLink => "link both ways",
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
                    } else {
                        (
                            "command  Ctrl+. enter · Ctrl+H help · w · q · wq"
                                .to_string(),
                            footer_dim,
                        )
                    };
                    paint_command_line(footer_ui::fitted_hints(&painter,&footer_font,(command_text_rect.width()-20.0).max(0.0),&text,color,footer_accent));
                    ui.interact(command_text_rect,ui.id().with("command_hints"),egui::Sense::hover()).on_hover_ui(|ui| {footer_ui::style_hints(ui);ui.label(text);});
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
        self.results_dialog(&ctx);
        self.scratch_feedback_toast(&ctx);
        self.observe_document_safety(ctx.input(|i| i.time), false);
    }
}

/// First Settings row of the Modules & vaults section (one row per module,
/// then the vaults row).
const MODULE_ROWS_START: usize = 31;

/// Calling module hooks and doing what modules ask.
impl SlateApp {
    /// Last row of the Settings dialog.
    pub(crate) fn settings_last_row(&self) -> usize {
        MODULE_ROWS_START + self.module_host.list().len()
    }

    /// Keyboard on a Modules & vaults row: `value` sets a module (None
    /// toggles it); on the vaults row it opens the folder picker.
    pub(crate) fn settings_module_row(&mut self, row: usize, value: Option<bool>, activate: bool) {
        let modules = self.module_host.list();
        match modules.get(row.wrapping_sub(MODULE_ROWS_START)) {
            Some((id, _, _, enabled)) => {
                let enabled = value.unwrap_or(!enabled);
                self.set_module_enabled(id, enabled);
            }
            None if activate => self.open_vault_picker(),
            None => {}
        }
    }

    pub(crate) fn open_vault_picker(&mut self) {
        self.settings_open = false;
        let start = self
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.project_root());
        self.open_file_picker_at(start, FilePickerMode::AddVault);
        self.status = "Vault: Enter on a folder, or Ctrl+Enter for the current one".to_string();
    }

    fn apply_module_requests(&mut self, requests: Vec<module_api::Request>) {
        for request in requests {
            match request {
                module_api::Request::Status(message) => self.status = message,
                module_api::Request::Open { path, line } => self.open_module_target(path, line),
                module_api::Request::Results { title, items } => {
                    self.results.show(title, items);
                    self.focus_editor_once = false;
                }
            }
        }
    }

    /// Opens `path` (in a tab), at 0-based `line` if given.
    fn open_module_target(&mut self, path: PathBuf, line: Option<usize>) {
        if self
            .path
            .as_deref()
            .is_some_and(|current| tabs::same_file(current, &path))
        {
            if let Some(line) = line {
                self.goto_startup_line(line + 1);
            }
            self.focus_editor_once = true;
            return;
        }
        self.open_keeping_unsaved_text(path, line.map(|line| format!("L{}", line + 1)));
    }

    fn run_module_command(&mut self, name: &'static str, args: &str, ctx: &egui::Context) {
        let Some(owner) = self.module_host.command_owner(name) else {
            return;
        };
        let doc = module_api::DocumentRef {
            id: self.buffer.identity,
            path: self.path.as_deref(),
            revision: self.buffer.revision,
            text: self.buffer.as_str(),
        };
        let requests = self
            .module_host
            .call(ctx, &self.vaults, Some(owner), name, |module, cx| {
                module.run(cx, name, args, Some(doc))
            });
        self.apply_module_requests(requests);
    }

    fn notify_modules_opened(&mut self) {
        let doc = module_api::DocumentRef {
            id: self.buffer.identity,
            path: self.path.as_deref(),
            revision: self.buffer.revision,
            text: self.buffer.as_str(),
        };
        self.module_seen.insert(doc.id, doc.revision);
        let requests = self.module_host.call(
            &self.module_ctx,
            &self.vaults,
            None,
            "document_opened",
            |module, cx| module.document_opened(cx, doc),
        );
        self.apply_module_requests(requests);
    }

    fn notify_modules_saved(&mut self) {
        let doc = module_api::DocumentRef {
            id: self.buffer.identity,
            path: self.path.as_deref(),
            revision: self.buffer.revision,
            text: self.buffer.as_str(),
        };
        let requests = self.module_host.call(
            &self.module_ctx,
            &self.vaults,
            None,
            "document_saved",
            |module, cx| module.document_saved(cx, doc),
        );
        self.apply_module_requests(requests);
    }

    /// The active document changed: the lines that may differ since the
    /// modules last heard of it.
    fn notify_modules_edited(&mut self) {
        let seen = self
            .module_seen
            .insert(self.buffer.identity, self.buffer.revision);
        let count = self.buffer.line_count();
        let lines = match seen.and_then(|revision| self.buffer.unchanged_layout_edges(revision)) {
            Some((prefix, suffix)) => {
                prefix.min(count)..count.saturating_sub(suffix).max(prefix.min(count))
            }
            None => 0..count,
        };
        let doc = module_api::DocumentRef {
            id: self.buffer.identity,
            path: self.path.as_deref(),
            revision: self.buffer.revision,
            text: self.buffer.as_str(),
        };
        let requests = self.module_host.call(
            &self.module_ctx,
            &self.vaults,
            None,
            "document_edited",
            |module, cx| module.document_edited(cx, doc, lines.clone()),
        );
        self.apply_module_requests(requests);
    }

    fn notify_modules_closed(&mut self, id: u64) {
        self.module_seen.remove(&id);
        let requests = self.module_host.call(
            &self.module_ctx,
            &self.vaults,
            None,
            "document_closed",
            |module, cx| module.document_closed(cx, id),
        );
        self.apply_module_requests(requests);
    }

    /// Modules that asked to be woken.
    fn poll_modules(&mut self, ctx: &egui::Context) {
        for id in self.module_host.due() {
            let requests =
                self.module_host
                    .call(ctx, &self.vaults, Some(id), "wake", |module, cx| {
                        module.wake(cx)
                    });
            self.apply_module_requests(requests);
        }
    }

    pub(crate) fn set_module_enabled(&mut self, id: &str, enabled: bool) {
        self.module_host.set_enabled(id, enabled);
        self.status = match self.save_settings() {
            Ok(()) => format!("Module {id} {}", if enabled { "on" } else { "off" }),
            Err(err) => format!("Settings save failed: {err}"),
        };
    }

    pub(crate) fn add_vault(&mut self, folder: PathBuf) {
        if !folder.is_dir() {
            self.status = format!("Not a folder: {}", folder.display());
            return;
        }
        let added = vaults::add(&mut self.vaults, folder.clone());
        self.status = match (added, self.save_settings()) {
            (false, _) => format!("Already a vault: {}", folder.display()),
            (true, Ok(())) => format!("Vault added: {}", folder.display()),
            (true, Err(err)) => format!("Settings save failed: {err}"),
        };
    }

    pub(crate) fn remove_vault(&mut self, index: usize) {
        if index < self.vaults.len() {
            let removed = self.vaults.remove(index);
            self.status = match self.save_settings() {
                Ok(()) => format!("Vault removed: {}", removed.display()),
                Err(err) => format!("Settings save failed: {err}"),
            };
        }
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

    // JetBrains Mono for the editor when chosen in Settings; the usual
    // monospace fonts stay behind it for symbols and emoji it lacks.
    fonts.font_data.insert(
        "jetbrains_mono".to_string(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMonoNL-Regular.ttf"
        ))),
    );
    let mut editor_family = vec!["jetbrains_mono".to_string()];
    editor_family.extend(
        fonts
            .families
            .get(&FontFamily::Monospace)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(
        FontFamily::Name(editor_view::JETBRAINS_MONO.into()),
        editor_family,
    );

    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = Color32::from_rgb(30, 36, 48);
    style.visuals.panel_fill = Color32::from_rgb(30, 36, 48);
    style.visuals.extreme_bg_color = Color32::from_rgb(25, 31, 40);
    style.visuals.faint_bg_color = Color32::from_rgb(38, 47, 61);
    style.visuals.selection.bg_fill = Color32::from_rgb(67, 76, 94);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(136, 192, 208));
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 41, 54);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 56, 72);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(59, 66, 82);
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    ctx.set_global_style(style);
}

/// A name part unique within the test run. Windows clocks tick every 100 ns,
/// so parallel tests could otherwise share (and delete) a temp folder.
#[cfg(test)]
pub(crate) fn test_unique() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
