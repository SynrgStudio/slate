//! Editor tabs.
//!
//! `SlateApp` keeps working on "the active document" through its existing
//! fields (`buffer`, `path`, `dirty`, …). Inactive tabs park those fields in a
//! `DocumentState`; switching tabs swaps them, and crash recovery swaps the
//! `Main` checkpoint role with the tab's own `Slot::Tab(id)`, so every
//! unsaved document keeps a durable record of its own.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// Only the primary window (the single-instance owner) reads and writes the
/// session; `--new-window` and detached windows must not clobber it.
static OWNS_SESSION: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_owns_session(owns: bool) {
    OWNS_SESSION.store(owns, Ordering::Relaxed);
}

pub(crate) fn owns_session() -> bool {
    OWNS_SESSION.load(Ordering::Relaxed)
}

pub(crate) struct DocumentState {
    buffer: EditorBuffer,
    editor_view: EditorView,
    path: Option<PathBuf>,
    dirty: bool,
    disk_base: Option<safe_file::Fingerprint>,
    file_conflict: Option<(document_safety::Slot, PathBuf)>,
    preview_sync: scroll_sync::PreviewSync,
    preview_layout: preview_layout::Layout,
    search_state: Option<SearchState>,
    recovery_origin: Option<PathBuf>,
    periscope_project_root: Option<PathBuf>,
    /// Restored from the last session and not read yet: loads on activation.
    lazy_path: Option<PathBuf>,
}

impl DocumentState {
    fn blank(font_size: f32) -> Self {
        let mut editor_view = EditorView::new();
        editor_view.set_font_size(font_size);
        Self {
            buffer: EditorBuffer::new(),
            editor_view,
            path: None,
            dirty: false,
            disk_base: None,
            file_conflict: None,
            preview_sync: scroll_sync::PreviewSync::default(),
            preview_layout: preview_layout::Layout::default(),
            search_state: None,
            recovery_origin: None,
            periscope_project_root: None,
            lazy_path: None,
        }
    }
}

pub(crate) struct Tab {
    pub(crate) id: u64,
    /// `None` for the active tab, whose document lives in `SlateApp`.
    doc: Option<DocumentState>,
    /// Alias of an untitled tab; it becomes the file's alias when saved.
    alias: Option<String>,
}

pub(crate) struct Tabs {
    pub(crate) list: Vec<Tab>,
    pub(crate) active: usize,
    next_id: u64,
    /// Recently closed tabs, newest last (Ctrl+Shift+T).
    closed: Vec<ClosedTab>,
    /// Set when the user chose "discard" to close the active tab: its text
    /// still goes to `closed`, so Ctrl+Shift+T can bring it back.
    pub(crate) discarded_close: bool,
}

const MAX_CLOSED_TABS: usize = 25;
/// Unsaved documents kept for Ctrl+Shift+T, in bytes of text.
const MAX_CLOSED_TEXT: usize = 64 * 1024 * 1024;

/// A closed tab: a clean file reopens from disk; unsaved (or untitled) text is
/// kept whole — text, undo history, cursor and alias.
pub(crate) enum ClosedTab {
    File(PathBuf),
    Document {
        doc: Box<DocumentState>,
        alias: Option<String>,
    },
}

impl ClosedTab {
    fn path(&self) -> Option<&Path> {
        match self {
            ClosedTab::File(path) => Some(path),
            ClosedTab::Document { doc, .. } => doc.path.as_deref(),
        }
    }

    fn text_len(&self) -> usize {
        match self {
            ClosedTab::File(_) => 0,
            ClosedTab::Document { doc, .. } => doc.buffer.as_str().len(),
        }
    }
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            list: vec![Tab {
                id: 1,
                doc: None,
                alias: None,
            }],
            active: 0,
            next_id: 2,
            closed: Vec::new(),
            discarded_close: false,
        }
    }
}

impl Tabs {
    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// What the tab strip shows for one tab.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TabLabel {
    /// Stable across reorders (widget identity while dragging).
    pub(crate) id: u64,
    pub(crate) text: String,
    pub(crate) dirty: bool,
    pub(crate) tooltip: String,
    /// What the rename field starts with: the alias, the file name, or
    /// nothing for an untitled tab.
    pub(crate) rename_text: String,
}

/// Where the cursor holds a torn-off tab's window (points from its top-left):
/// over its first tab.
const TORN_GRAB: (f32, f32) = (60.0, 14.0);

/// A tab torn off into a window that follows the cursor until the drop.
pub(crate) struct TornTab {
    process_id: u32,
    label: String,
}

/// A tab moving to a new window with unsaved text (or no file yet). Written
/// by the old window, read and deleted by the new one after it has its own
/// recovery checkpoint, so the text always exists in at least one place.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Handoff {
    path: Option<PathBuf>,
    disk_base: Option<safe_file::Fingerprint>,
    text: String,
    cursor: usize,
    dirty: bool,
    alias: Option<String>,
}

/// Saved between runs: the files of the open tabs and which one was active.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Session {
    tabs: Vec<PathBuf>,
    active: usize,
}

pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    let normalize = |path: &Path| {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let text = path.to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    a == b || normalize(a) == normalize(b)
}

pub(super) fn file_label(path: Option<&Path>) -> String {
    path.and_then(|p| p.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("untitled")
        .to_string()
}

/// The last `depth` folder names above `path`, e.g. `slate` or `src/slate`.
fn parent_suffix(path: &Path, depth: usize) -> String {
    let mut parts: Vec<String> = path
        .parent()
        .into_iter()
        .flat_map(|parent| parent.components().rev())
        .filter_map(|part| match part {
            std::path::Component::Normal(name) => name.to_str().map(str::to_owned),
            _ => None,
        })
        .take(depth)
        .collect();
    parts.reverse();
    parts.join("/")
}

/// Tab texts: the file name, plus as many parent folders as needed to tell
/// apart tabs whose file names are equal (`README.md · slate`).
pub(crate) fn disambiguated_names(paths: &[Option<&Path>]) -> Vec<String> {
    let names: Vec<String> = paths.iter().map(|p| file_label(*p)).collect();
    let key = |name: &str| name.to_lowercase();
    (0..paths.len())
        .map(|index| {
            let Some(path) = paths[index] else {
                return names[index].clone();
            };
            let clashes: Vec<&Path> = (0..paths.len())
                .filter(|&other| other != index && key(&names[other]) == key(&names[index]))
                .filter_map(|other| paths[other])
                .collect();
            if clashes.is_empty() {
                return names[index].clone();
            }
            for depth in 1..=4 {
                let mine = parent_suffix(path, depth);
                if clashes
                    .iter()
                    .all(|other| parent_suffix(other, depth).to_lowercase() != mine.to_lowercase())
                {
                    return format!("{} · {mine}", names[index]);
                }
            }
            format!("{} · {}", names[index], parent_suffix(path, 4))
        })
        .collect()
}

impl SlateApp {
    pub(super) fn tab_labels(&self) -> Vec<TabLabel> {
        // Paths include a file that is still loading.
        let paths: Vec<Option<&Path>> = (0..self.tabs.list.len())
            .map(|index| self.tab_path(index))
            .collect();
        let names = disambiguated_names(&paths);
        let aliases: Vec<Option<String>> = self
            .tabs
            .list
            .iter()
            .zip(&paths)
            .map(|(tab, path)| match path {
                Some(path) => self.aliases.get(path).map(str::to_owned),
                None => tab.alias.clone(),
            })
            .collect();
        let rename_texts: Vec<String> = aliases
            .iter()
            .zip(&paths)
            .map(|(alias, path)| {
                alias
                    .clone()
                    .or_else(|| path.map(|path| file_label(Some(path))))
                    .unwrap_or_default()
            })
            .collect();
        let names: Vec<String> = names
            .into_iter()
            .zip(&aliases)
            .map(|(name, alias)| alias.clone().unwrap_or(name))
            .collect();
        self.tabs
            .list
            .iter()
            .zip(names)
            .zip(&paths)
            .zip(rename_texts)
            .map(|(((tab, text), path), rename_text)| {
                let dirty = tab.doc.as_ref().map_or(self.dirty, |doc| doc.dirty);
                let tooltip = match path {
                    Some(path) => format!(
                        "{}{}{}",
                        self.aliases
                            .get(path)
                            .map(|alias| format!("{alias} → "))
                            .unwrap_or_default(),
                        path.display(),
                        if dirty { " · unsaved changes" } else { "" }
                    ),
                    None if dirty => "untitled · unsaved".to_string(),
                    None => "untitled".to_string(),
                };
                TabLabel {
                    id: tab.id,
                    text,
                    dirty,
                    tooltip,
                    rename_text,
                }
            })
            .collect()
    }

    fn aliases_path(&self) -> Option<PathBuf> {
        #[cfg(test)]
        return self
            .settings_path_override
            .as_ref()
            .and_then(|path| path.parent().map(|dir| dir.join("aliases.json")));
        #[cfg(not(test))]
        Self::settings_path()?
            .parent()
            .map(|dir| dir.join("aliases.json"))
    }

    /// Names the tab's file inside Slate (the file on disk keeps its name).
    /// A blank alias, or the file's own name, removes it.
    pub(super) fn rename_tab(&mut self, index: usize, alias: Option<&str>) {
        let Some(path) = self.tab_path(index).map(Path::to_path_buf) else {
            // Untitled: the alias lives on the tab until the file is saved.
            let alias = alias
                .map(str::trim)
                .filter(|alias| !alias.is_empty())
                .map(str::to_owned);
            self.status = match &alias {
                Some(alias) => format!("Alias: {alias} · kept for the file when you save it"),
                None => "Alias removed".into(),
            };
            if let Some(tab) = self.tabs.list.get_mut(index) {
                tab.alias = alias;
            }
            return;
        };
        let file_name = file_label(Some(&path));
        let alias = alias
            .map(str::trim)
            .filter(|alias| !alias.is_empty() && *alias != file_name);
        let file = self.aliases_path();
        self.status = match self.aliases.set(file.as_deref(), &path, alias) {
            Ok(()) => match alias {
                Some(alias) => format!("Alias: {alias} · {file_name} stays its real name"),
                None => format!("Alias removed · {file_name}"),
            },
            Err(err) => format!("Alias set for this session; not saved: {err}"),
        };
    }

    /// The active untitled tab's alias, if it has one.
    pub(super) fn active_tab_alias(&self) -> Option<&str> {
        self.tabs.list.get(self.tabs.active)?.alias.as_deref()
    }

    /// After the first save, an untitled tab's alias becomes the file's alias.
    pub(super) fn adopt_tab_alias(&mut self, path: &Path) {
        let Some(alias) = self
            .tabs
            .list
            .get_mut(self.tabs.active)
            .and_then(|tab| tab.alias.take())
        else {
            return;
        };
        let file = self.aliases_path();
        let _ = self.aliases.set(file.as_deref(), path, Some(&alias));
    }

    /// Periscope also finds files by alias; those matches come first.
    pub(super) fn add_alias_matches_to_periscope(&mut self) {
        let project = (self.periscope_mode == PeriscopeMode::Project)
            .then(|| self.periscope_project_root.clone())
            .flatten();
        let mut matches: Vec<PathBuf> = self
            .aliases
            .matching(&self.periscope_query)
            .into_iter()
            .filter(|path| project.as_ref().is_none_or(|root| path.starts_with(root)))
            .collect();
        if matches.is_empty() {
            return;
        }
        self.periscope_results
            .retain(|path| !matches.iter().any(|alias_path| same_file(alias_path, path)));
        matches.append(&mut self.periscope_results);
        self.periscope_results = matches;
    }

    /// A tab dropped outside the window opens in a new Slate window at `at`
    /// (screen points) and leaves this one. Unsaved text never moves between
    /// processes: such tabs are saved first.
    /// A background save landed after its document moved to a background
    /// tab: marks that tab saved (or conflicted). False if the tab is gone.
    pub(super) fn land_save_in_background_tab(
        &mut self,
        pending: &PendingSave,
        result: &Result<safe_file::Fingerprint, safe_file::SaveError>,
    ) -> bool {
        let Some(doc) = self
            .tabs
            .list
            .iter_mut()
            .filter_map(|tab| tab.doc.as_mut())
            .find(|doc| {
                doc.buffer.identity == pending.identity && doc.path.as_ref() == Some(&pending.path)
            })
        else {
            return false;
        };
        match result {
            Ok(base) => {
                doc.disk_base = Some(*base);
                doc.file_conflict = None;
                doc.buffer.mark_saved_at(pending.node);
                doc.dirty = !doc.buffer.at_saved_state();
                self.remember_recent_file(pending.path.clone());
                let _ = self.save_settings();
                self.status = format!("Saved {}", pending.path.display());
                if let Err(error) =
                    self.persist_saved_project(&pending.path, pending.project.clone())
                {
                    self.status
                        .push_str(&format!(" · project metadata not saved: {error}"));
                }
            }
            Err(safe_file::SaveError::Conflict) => {
                doc.file_conflict = Some((document_safety::Slot::Main, pending.path.clone()));
                self.status =
                    "Save blocked: external change / existing destination; text preserved".into();
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
        true
    }

    pub(super) fn tabs_len(&self) -> usize {
        self.tabs.list.len()
    }

    /// What carries tab `index` to another window: its path, or a handoff
    /// file with its unsaved text (written here). None (with a status) when
    /// the handoff cannot be written.
    fn moved_tab_request(&mut self, index: usize) -> Option<single_instance::OpenRequest> {
        // The moved tab must carry the file's real state.
        self.finish_background_save();
        if index >= self.tabs.list.len() {
            return None;
        }
        // Unsaved text (or a tab without a file) travels in a handoff file.
        let handoff = match self.tab_handoff(index) {
            Some(handoff) => match self.write_handoff(&handoff) {
                Ok(file) => Some(file),
                Err(err) => {
                    self.status = format!("Could not move the tab: {err}");
                    return None;
                }
            },
            None => None,
        };
        Some(single_instance::OpenRequest {
            path: if handoff.is_none() {
                self.tab_path(index).map(Path::to_path_buf)
            } else {
                None
            },
            handoff,
            ..Default::default()
        })
    }

    /// Moves tab `index` out of this window: onto another Slate window under
    /// the cursor, or else into a new window at `at`. True when it moved.
    pub(super) fn detach_tab(&mut self, index: usize, at: egui::Pos2) -> bool {
        let Some(moved) = self.moved_tab_request(index) else {
            return false;
        };
        let (path, handoff) = (moved.path.clone(), moved.handoff.clone());
        let label = self.tab_labels()[index].text.clone();
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let moved_tab = |_: Option<PathBuf>| moved.clone();
        if let Some(target) = tab_drop::target_under_cursor(&[std::process::id()])
            && tab_drop::send(&target, moved.clone())
        {
            perf_log::mark("tab.detach", &format!("joined={}", target.process_id));
            self.tab_moved_away(index);
            tab_drop::focus_window(target.process_id);
            self.status = format!("Moved {label} to the other window");
            return true;
        }
        // Put the new window's tab strip under the pointer.
        let position = format!("{},{}", (at.x - 60.0).round(), (at.y - 14.0).round());
        let command = {
            let mut command = std::process::Command::new(exe);
            command.arg("--new-window").arg("--position").arg(position);
            match (&handoff, &path) {
                (Some(file), _) => {
                    command.arg("--handoff").arg(file);
                }
                (None, Some(path)) => {
                    command.arg(path);
                }
                (None, None) => {}
            }
            command
        };
        // A ready hidden window takes the tab in milliseconds; otherwise a
        // new process starts.
        let request = single_instance::OpenRequest {
            position: Some(((at.x - 60.0).round(), (at.y - 14.0).round())),
            ..moved_tab(path.clone())
        };
        let claimed = spare::claim(&request).is_some();
        // Tests check the command instead of starting a window.
        #[cfg(test)]
        let spawned: std::io::Result<()> = {
            let _ = claimed;
            self.status = format!("{command:?}");
            Ok(())
        };
        #[cfg(not(test))]
        let spawned = if claimed {
            Ok(())
        } else {
            { command }.spawn().map(drop)
        };
        match spawned {
            Ok(_) => {
                perf_log::mark("tab.detach", &format!("new_window spare={claimed}"));
                #[cfg(test)]
                let launched = self.status.clone();
                self.tab_moved_away(index);
                self.status = format!("Moved {label} to a new window");
                #[cfg(test)]
                self.status.push_str(&format!(" · {launched}"));
                true
            }
            Err(err) => {
                if let Some(file) = handoff {
                    let _ = fs::remove_file(file);
                }
                self.status = format!("New window failed: {err}");
                false
            }
        }
    }

    /// Dragged out of the window: with a spare ready the tab becomes a new
    /// window at once, which follows the cursor until the drop. Without one
    /// nothing happens yet (the drop opens a window the usual way).
    pub(super) fn tear_off_tab(&mut self, index: usize, ctx: &egui::Context) {
        if self.torn_tab.is_some() || self.tabs.list.len() < 2 || !spare::is_ready() {
            return;
        }
        let Some(cursor) = tab_drop::cursor() else {
            return;
        };
        let label = self.tab_labels()[index].text.clone();
        let Some(moved) = self.moved_tab_request(index) else {
            return;
        };
        let scale = ctx.native_pixels_per_point().unwrap_or(1.0);
        let request = single_instance::OpenRequest {
            position: Some((
                cursor.0 / scale - TORN_GRAB.0,
                cursor.1 / scale - TORN_GRAB.1,
            )),
            follow: true,
            ..moved.clone()
        };
        match spare::claim(&request) {
            Some(process_id) => {
                perf_log::mark("tab.torn_off", &format!("window={process_id}"));
                self.tab_moved_away(index);
                self.torn_tab = Some(TornTab {
                    process_id,
                    label: label.clone(),
                });
                self.status = format!("Moved {label} to a new window");
            }
            None => {
                if let Some(file) = moved.handoff {
                    let _ = fs::remove_file(file);
                }
            }
        }
    }

    /// Each frame: a torn-off tab's window follows the cursor; a dragged
    /// tab (ours, torn off or not) tells the Slate window under it where it
    /// would land. At the drop the torn-off window joins the window under
    /// it, if any, or takes the focus.
    pub(super) fn poll_tab_drag(&mut self, ctx: &egui::Context) {
        if let Some(torn) = &self.torn_tab {
            let (process_id, label) = (torn.process_id, torn.label.clone());
            let target = tab_drop::target_under_cursor(&[process_id]);
            // The system knows the button even if this window lost the mouse.
            let down = tab_drop::button_down()
                .unwrap_or_else(|| ctx.input(|i| i.pointer.primary_down()));
            if down {
                if let Some(cursor) = tab_drop::cursor() {
                    tab_drop::move_window(process_id, cursor, TORN_GRAB);
                }
                self.tell_drag_hover(target.as_ref(), &label);
                ctx.request_repaint();
                return;
            }
            self.tell_drag_hover(None, &label);
            self.torn_tab = None;
            match target {
                Some(target) if tab_drop::send_on(process_id, &target) => {
                    perf_log::mark("tab.torn_drop", &format!("joined={}", target.process_id));
                    tab_drop::focus_window(target.process_id);
                }
                _ => {
                    perf_log::mark("tab.torn_drop", "joined=none");
                    tab_drop::focus_window(process_id);
                }
            }
            return;
        }
        let dragged = window_ui::dragged_out_label(ctx);
        let target = dragged
            .as_ref()
            .and_then(|_| tab_drop::target_under_cursor(&[std::process::id()]));
        self.tell_drag_hover(target.as_ref(), dragged.as_deref().unwrap_or_default());
    }

    fn tell_drag_hover(&mut self, target: Option<&tab_drop::Target>, label: &str) {
        let previous = self.drag_hover.take();
        if let Some((file, _)) = &previous
            && target.is_none_or(|target| &target.file != file)
        {
            tab_drop::leave(file);
        }
        if let Some(target) = target {
            if previous.as_ref() != Some(&(target.file.clone(), target.cursor)) {
                tab_drop::hover(target, label);
            }
            self.drag_hover = Some((target.file.clone(), target.cursor));
        }
    }

    /// End of a drag that tore our tab off into this window: hand it on to
    /// the window it was dropped on, and close if nothing is left here.
    pub(super) fn send_tab_on(&mut self, file: PathBuf, cursor: (f32, f32), ctx: &egui::Context) {
        let index = self.tabs.active;
        let only_tab = self.tabs.list.len() == 1;
        let Some(moved) = self.moved_tab_request(index) else {
            return;
        };
        let target = tab_drop::Target {
            process_id: 0,
            file,
            cursor,
        };
        if tab_drop::send(&target, moved.clone()) {
            self.tab_moved_away(index);
            if only_tab {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if let Some(file) = moved.handoff {
            let _ = fs::remove_file(file);
        }
    }

    /// The tab's text now lives in the handoff file and the other window.
    fn tab_moved_away(&mut self, index: usize) {
        if index != self.tabs.active {
            self.activate_tab(index);
        }
        self.dirty = false;
        self.close_active_tab_inner(false);
    }

    /// A tab dropped here from another Slate window: added where it was
    /// dropped (or last when dropped below the tab strip) and focused.
    pub(super) fn receive_tab(
        &mut self,
        request: single_instance::OpenRequest,
        ctx: &egui::Context,
    ) {
        self.finish_background_save();
        perf_log::mark("tab.received", "");
        let slot = request
            .tab_at
            .map_or(self.tabs.list.len(), |at| window_ui::tab_slot_at(ctx, at));
        let existing = request
            .path
            .as_deref()
            .and_then(|path| self.tab_index_for_path(path));
        if let Some(index) = existing {
            // Already open here: that tab is the one.
            self.activate_tab(index);
        } else {
            if !self.active_tab_is_blank() {
                self.new_tab();
            }
            if let Some(file) = &request.handoff {
                self.apply_handoff(file);
            } else if let Some(path) = request.path {
                self.request_file_open(path, OpenDestination::Restore);
            }
            let last = self.tabs.list.len() - 1;
            self.move_tab(self.tabs.active, slot.min(last));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.focus_editor_once = true;
    }

    /// What has to travel with a tab: `None` when the file on disk is enough.
    fn tab_handoff(&self, index: usize) -> Option<Handoff> {
        let tab = &self.tabs.list[index];
        let (path, disk_base, text, cursor, dirty) = match &tab.doc {
            None => (
                self.path.clone(),
                self.disk_base,
                self.buffer.as_str(),
                self.buffer.cursor(),
                self.dirty,
            ),
            Some(doc) => (
                doc.path.clone().or(doc.lazy_path.clone()),
                doc.disk_base,
                doc.buffer.as_str(),
                doc.buffer.cursor(),
                doc.dirty,
            ),
        };
        if !dirty && path.is_some() {
            return None;
        }
        Some(Handoff {
            alias: path.is_none().then(|| tab.alias.clone()).flatten(),
            path,
            disk_base,
            text: text.to_string(),
            cursor,
            dirty,
        })
    }

    fn handoff_dir(&self) -> Option<PathBuf> {
        #[cfg(test)]
        return self
            .settings_path_override
            .as_ref()
            .and_then(|path| path.parent().map(|dir| dir.join("handoff")));
        #[cfg(not(test))]
        dirs_next::data_local_dir().map(|dir| dir.join("slate").join("handoff"))
    }

    fn write_handoff(&self, handoff: &Handoff) -> Result<PathBuf, String> {
        let dir = self.handoff_dir().ok_or("no local data folder")?;
        fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let file = dir.join(format!("{}-{nanos}.json", std::process::id()));
        let json = serde_json::to_vec(handoff).map_err(|err| err.to_string())?;
        safe_file::atomic_replace(&file, &json)?;
        Ok(file)
    }

    /// New window side of a moved tab: takes the text, makes it durable in
    /// this process's recovery, then deletes the handoff file.
    pub(super) fn apply_handoff(&mut self, file: &Path) {
        let Some(handoff) = fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Handoff>(&bytes).ok())
        else {
            self.status = format!("Moved tab could not be read: {}", file.display());
            return;
        };
        self.cancel_file_open();
        let cursor = handoff.cursor.min(handoff.text.len());
        self.buffer.set_text(handoff.text);
        self.buffer.set_cursor(cursor);
        if !handoff.dirty {
            // The text is the file's: undoing back to it leaves nothing unsaved.
            self.buffer.mark_saved();
        }
        self.path = handoff.path;
        self.disk_base = handoff.disk_base;
        self.dirty = handoff.dirty;
        if let Some(tab) = self.tabs.list.get_mut(self.tabs.active) {
            tab.alias = handoff.alias;
        }
        self.focus_editor_once = true;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.observe_document_safety(0.0, true);
        self.document_safety.flush();
        let _ = fs::remove_file(file);
        self.status = if self.dirty {
            "Moved here with its unsaved changes".into()
        } else {
            "Moved here".into()
        };
    }

    /// Moves a tab in the strip; the active tab stays active.
    pub(super) fn move_tab(&mut self, from: usize, to: usize) {
        let count = self.tabs.list.len();
        if from >= count || to >= count || from == to {
            return;
        }
        let active_id = self.tabs.list[self.tabs.active].id;
        let tab = self.tabs.list.remove(from);
        self.tabs.list.insert(to, tab);
        self.tabs.active = self
            .tabs
            .list
            .iter()
            .position(|tab| tab.id == active_id)
            .unwrap_or(0);
    }

    /// Ctrl+Shift+T: reopens the most recently closed tab — unsaved text
    /// exactly as it was, a clean file from disk.
    pub(super) fn reopen_closed_tab(&mut self) {
        while let Some(closed) = self.tabs.closed.pop() {
            match closed {
                ClosedTab::File(path) => {
                    if path.is_file() {
                        self.request_file_open(path, OpenDestination::Restore);
                        return;
                    }
                }
                ClosedTab::Document { doc, alias } => {
                    let label = alias
                        .clone()
                        .unwrap_or_else(|| file_label(doc.path.as_deref()));
                    let id = self.tabs.allocate_id();
                    let index = if self.active_tab_is_blank() && self.tabs.list.len() == 1 {
                        // Replace the lone empty tab.
                        0
                    } else {
                        self.tabs.active + 1
                    };
                    self.tabs.list.insert(
                        index,
                        Tab {
                            id,
                            doc: Some(*doc),
                            alias,
                        },
                    );
                    if index == 0 {
                        // The empty tab shifted right: show the restored one, then drop it.
                        self.tabs.active = 1;
                        self.activate_tab(0);
                        self.tabs.list.remove(1);
                    } else {
                        self.activate_tab(index);
                    }
                    self.status = format!("Reopened {label} with its unsaved changes");
                    return;
                }
            }
        }
        self.status = "No closed tab to reopen".into();
    }

    fn tab_path(&self, index: usize) -> Option<&Path> {
        match &self.tabs.list.get(index)?.doc {
            None => self
                .path
                .as_deref()
                .or_else(|| self.pending_file_open.as_ref().map(|p| p.path.as_path())),
            Some(doc) => doc.path.as_deref().or(doc.lazy_path.as_deref()),
        }
    }

    pub(super) fn tab_index_for_path(&self, path: &Path) -> Option<usize> {
        (0..self.tabs.list.len())
            .find(|&index| self.tab_path(index).is_some_and(|tab| same_file(tab, path)))
    }

    /// An untitled, empty, unchanged tab: opening a file reuses it.
    pub(super) fn active_tab_is_blank(&self) -> bool {
        self.path.is_none()
            && !self.dirty
            && self.buffer.as_str().is_empty()
            && self.pending_file_open.is_none()
    }

    fn first_dirty_tab(&self) -> Option<usize> {
        (0..self.tabs.list.len()).find(|&index| match &self.tabs.list[index].doc {
            None => self.dirty,
            Some(doc) => doc.dirty,
        })
    }

    /// Moves the active document out of `SlateApp`.
    fn stash_active_document(&mut self) -> DocumentState {
        // A blank tab still loading its file keeps loading it when shown again.
        let loading = self
            .path
            .is_none()
            .then(|| self.pending_file_open.as_ref().map(|p| p.path.clone()))
            .flatten();
        self.cancel_file_open();
        self.remember_current_cursor_position();
        // Latest text is durable before the checkpoint changes role.
        self.observe_document_safety(0.0, true);
        let blank = DocumentState::blank(self.editor_font_size);
        self.footer_counts = Default::default();
        self.link_inline_anchor = None;
        self.duplicate_placement = None;
        self.pending_bidirectional_link_source_line = None;
        self.safety_notified.remove(&document_safety::Slot::Main);
        DocumentState {
            buffer: std::mem::replace(&mut self.buffer, blank.buffer),
            editor_view: std::mem::replace(&mut self.editor_view, blank.editor_view),
            path: self.path.take(),
            dirty: std::mem::take(&mut self.dirty),
            disk_base: self.disk_base.take(),
            file_conflict: self.file_conflict.take(),
            preview_sync: std::mem::take(&mut self.preview_sync),
            preview_layout: std::mem::take(&mut self.preview_layout),
            search_state: self.search_state.take(),
            recovery_origin: self.recovery_origins.remove(&document_safety::Slot::Main),
            periscope_project_root: self.periscope_project_root.take(),
            lazy_path: loading,
        }
    }

    fn restore_document(&mut self, mut doc: DocumentState) {
        doc.editor_view.set_font_size(self.editor_font_size);
        doc.editor_view.set_look(self.editor_look);
        self.buffer = doc.buffer;
        self.editor_view = doc.editor_view;
        self.path = doc.path;
        self.dirty = doc.dirty;
        self.disk_base = doc.disk_base;
        self.file_conflict = doc.file_conflict;
        self.preview_sync = doc.preview_sync;
        self.preview_layout = doc.preview_layout;
        self.search_state = doc.search_state;
        if let Some(origin) = doc.recovery_origin {
            self.recovery_origins
                .insert(document_safety::Slot::Main, origin);
        }
        self.periscope_project_root = doc.periscope_project_root;
        self.focus_editor_once = true;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        if let Some(path) = doc.lazy_path {
            self.start_file_load(path, OpenDestination::Restore);
        }
    }

    /// Makes `index` the active tab.
    pub(super) fn activate_tab(&mut self, index: usize) {
        if index == self.tabs.active || index >= self.tabs.list.len() {
            return;
        }
        // Modules waiting for a pause in this document hear it now.
        if self.module_idle_at.is_some() {
            let ctx = self.module_ctx.clone();
            self.notify_modules_idle(&ctx);
        }
        let old = self.tabs.active;
        let old_id = self.tabs.list[old].id;
        let new_id = self.tabs.list[index].id;
        let parked = self.stash_active_document();
        self.document_safety.swap_roles(
            document_safety::Slot::Main,
            document_safety::Slot::Tab(old_id),
        );
        self.tabs.list[old].doc = Some(parked);
        let incoming = self.tabs.list[index]
            .doc
            .take()
            .unwrap_or_else(|| DocumentState::blank(self.editor_font_size));
        self.document_safety.swap_roles(
            document_safety::Slot::Tab(new_id),
            document_safety::Slot::Main,
        );
        self.tabs.active = index;
        self.restore_document(incoming);
    }

    /// Opens an empty tab right after the active one.
    pub(super) fn new_tab(&mut self) {
        let id = self.tabs.allocate_id();
        let index = self.tabs.active + 1;
        self.tabs.list.insert(
            index,
            Tab {
                id,
                doc: Some(DocumentState::blank(self.editor_font_size)),
                alias: None,
            },
        );
        self.activate_tab(index);
        self.status = "New tab".into();
    }

    fn new_lazy_tab(&mut self, path: PathBuf) {
        let id = self.tabs.allocate_id();
        let mut doc = DocumentState::blank(self.editor_font_size);
        doc.lazy_path = Some(path);
        self.tabs.list.push(Tab {
            id,
            doc: Some(doc),
            alias: None,
        });
    }

    pub(super) fn cycle_tab(&mut self, step: isize) {
        let count = self.tabs.list.len() as isize;
        if count > 1 {
            let next = (self.tabs.active as isize + step).rem_euclid(count) as usize;
            self.activate_tab(next);
        }
    }

    /// Ctrl+W: closes the active tab, asking first when it has unsaved text.
    pub(super) fn close_active_tab(&mut self) {
        if self.dirty {
            self.confirm(PendingAction::CloseTab);
        } else {
            self.close_active_tab_now();
        }
    }

    pub(super) fn close_tab(&mut self, index: usize) {
        if index != self.tabs.active {
            self.activate_tab(index);
        }
        self.close_active_tab();
    }

    /// Closes the active tab without asking (clean or already discarded).
    /// Ctrl+Shift+T can bring it back, unsaved text included.
    pub(super) fn close_active_tab_now(&mut self) {
        self.close_active_tab_inner(true);
    }

    fn remember_closed(&mut self, closed: ClosedTab) {
        if let Some(path) = closed.path().map(Path::to_path_buf) {
            self.tabs
                .closed
                .retain(|old| !old.path().is_some_and(|old| same_file(old, &path)));
        }
        self.tabs.closed.push(closed);
        let excess = self.tabs.closed.len().saturating_sub(MAX_CLOSED_TABS);
        self.tabs.closed.drain(..excess);
        // Bound the memory held by closed unsaved documents (oldest go first).
        while self
            .tabs
            .closed
            .iter()
            .map(ClosedTab::text_len)
            .sum::<usize>()
            > MAX_CLOSED_TEXT
            && self.tabs.closed.len() > 1
        {
            self.tabs.closed.remove(0);
        }
    }

    /// `remember: false` when the tab moved to another window instead.
    fn close_active_tab_inner(&mut self, remember: bool) {
        let closed = file_label(self.path.as_deref());
        let unsaved = std::mem::take(&mut self.tabs.discarded_close)
            || self.dirty
            || (self.path.is_none() && !self.buffer.as_str().is_empty());
        let alias = self
            .tabs
            .list
            .get_mut(self.tabs.active)
            .and_then(|tab| tab.alias.take());
        self.cancel_file_open();
        self.remember_current_cursor_position();
        if remember && unsaved {
            let mut doc = self.stash_active_document();
            doc.dirty = true;
            self.remember_closed(ClosedTab::Document {
                doc: Box::new(doc),
                alias,
            });
        } else if let (true, Some(path)) = (remember, self.path.clone()) {
            self.remember_closed(ClosedTab::File(path));
        }
        self.clear_recovery(document_safety::Slot::Main);
        self.forget_project_buffer(self.buffer.identity);
        self.notify_modules_closed(self.buffer.identity);
        if self.tabs.list.len() == 1 {
            // The last tab stays, empty, like a fresh window.
            self.dirty = false;
            self.new_buffer();
        } else {
            let closing = self.tabs.active;
            let next = if closing + 1 < self.tabs.list.len() {
                closing + 1
            } else {
                closing - 1
            };
            let next_id = self.tabs.list[next].id;
            let incoming = self.tabs.list[next]
                .doc
                .take()
                .unwrap_or_else(|| DocumentState::blank(self.editor_font_size));
            self.document_safety.swap_roles(
                document_safety::Slot::Tab(next_id),
                document_safety::Slot::Main,
            );
            self.tabs.list.remove(closing);
            self.tabs.active = if next > closing { next - 1 } else { next };
            self.restore_document(incoming);
        }
        let _ = self.save_settings();
        self.status = format!("Closed {closed}");
    }

    /// Picks the tab a file should open in. Returns false when the file is
    /// already open and nothing has to be loaded.
    pub(super) fn prepare_tab_for_open(
        &mut self,
        path: &Path,
        destination: &OpenDestination,
    ) -> bool {
        if let Some(index) = self.tab_index_for_path(path) {
            let came_from_blank = self.active_tab_is_blank() && index != self.tabs.active;
            let left = self.tabs.active;
            self.activate_tab(index);
            if came_from_blank {
                // A tab opened just for this file is not needed any more.
                self.tabs.list.remove(left);
                if left < self.tabs.active {
                    self.tabs.active -= 1;
                }
            }
            if self.pending_file_open.is_none() {
                self.apply_open_destination(destination.clone());
            }
            return false;
        }
        if !self.active_tab_is_blank() {
            self.new_tab();
        }
        true
    }

    /// Ctrl+Q / close button with tabs: a real quit walks every unsaved tab.
    pub(super) fn activate_first_dirty_tab(&mut self) -> bool {
        match self.first_dirty_tab() {
            Some(index) => {
                self.activate_tab(index);
                true
            }
            None => false,
        }
    }

    fn session_path(&self) -> Option<PathBuf> {
        // Tests never touch the user's real session.
        #[cfg(test)]
        return self
            .settings_path_override
            .as_ref()
            .and_then(|path| path.parent().map(|dir| dir.join("session.json")));
        #[cfg(not(test))]
        Self::settings_path()?
            .parent()
            .map(|dir| dir.join("session.json"))
    }

    pub(super) fn save_session(&self) {
        if self.scratch || !(cfg!(test) || owns_session()) {
            return;
        }
        let Some(file) = self.session_path() else {
            return;
        };
        let mut session = Session::default();
        for index in 0..self.tabs.list.len() {
            if let Some(path) = self.tab_path(index) {
                if index == self.tabs.active {
                    session.active = session.tabs.len();
                }
                session.tabs.push(path.to_path_buf());
            }
        }
        if let Ok(json) = serde_json::to_vec_pretty(&session) {
            settings_writer::write(file, json, settings_writer::Durability::Synced);
        }
    }

    /// Reopens the last session's tabs. Only the active one is read now;
    /// the others load when first shown.
    pub(super) fn restore_session(&mut self) -> bool {
        let Some(session) = self
            .session_path()
            .and_then(|file| fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice::<Session>(&bytes).ok())
        else {
            return false;
        };
        let paths: Vec<PathBuf> = session.tabs.into_iter().filter(|p| p.is_file()).collect();
        if paths.is_empty() {
            return false;
        }
        let active = session.active.min(paths.len() - 1);
        for path in &paths {
            self.new_lazy_tab(path.clone());
        }
        // The initial empty tab gives way to the restored ones.
        let first_restored = 1;
        self.activate_tab(first_restored + active);
        self.tabs.list.remove(0);
        self.tabs.active -= 1;
        true
    }
}

fn tab_number(key: egui::Key) -> Option<usize> {
    use egui::Key::*;
    Some(match key {
        Num1 => 1,
        Num2 => 2,
        Num3 => 3,
        Num4 => 4,
        Num5 => 5,
        Num6 => 6,
        Num7 => 7,
        Num8 => 8,
        Num9 => 9,
        _ => return None,
    })
}

impl SlateApp {
    /// Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+1…9 (9 = last) and Ctrl+wheel.
    /// Called while Ctrl is held; consumes the events it uses.
    pub(super) fn handle_tab_switch_input(&mut self, ctx: &egui::Context) -> bool {
        if self.scratch {
            return false;
        }
        let mut step = 0isize;
        let mut goto = None;
        let mut wheel = 0.0f32;
        let mut reopen = false;
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Key {
                    key: egui::Key::T,
                    pressed,
                    repeat: false,
                    modifiers,
                    ..
                } if modifiers.ctrl && modifiers.shift && !modifiers.alt => {
                    reopen |= *pressed;
                    false
                }
                egui::Event::Key {
                    key: egui::Key::Tab,
                    pressed,
                    modifiers,
                    ..
                } if modifiers.ctrl && !modifiers.alt => {
                    if *pressed {
                        step += if modifiers.shift { -1 } else { 1 };
                    }
                    false
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.shift
                    && tab_number(*key).is_some() =>
                {
                    goto = tab_number(*key);
                    false
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if modifiers.ctrl => {
                    // Wheel up (content moves down, positive y) = next tab.
                    wheel += match unit {
                        egui::MouseWheelUnit::Point => delta.y / 50.0,
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * 3.0,
                    };
                    false
                }
                _ => true,
            });
        });
        if wheel != 0.0 {
            self.tab_wheel += wheel;
            while self.tab_wheel >= 1.0 {
                self.tab_wheel -= 1.0;
                step += 1;
            }
            while self.tab_wheel <= -1.0 {
                self.tab_wheel += 1.0;
                step -= 1;
            }
        }
        if reopen {
            self.reopen_closed_tab();
            return true;
        }
        if let Some(number) = goto {
            let last = self.tabs.list.len() - 1;
            self.activate_tab(if number == 9 {
                last
            } else {
                (number - 1).min(last)
            });
            return true;
        }
        if step != 0 {
            self.cycle_tab(step);
            return true;
        }
        wheel != 0.0
    }
}
