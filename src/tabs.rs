//! Editor tabs.
//!
//! `SlateApp` keeps working on "the active document" through its existing
//! fields (`buffer`, `path`, `dirty`, …). Inactive tabs park those fields in a
//! `DocumentState`; switching tabs swaps them, and crash recovery swaps the
//! `Main` checkpoint role with the tab's own `Slot::Tab(id)`, so every
//! unsaved document keeps a durable record of its own.
use super::*;

pub(crate) struct DocumentState {
    buffer: EditorBuffer,
    editor_view: EditorView,
    path: Option<PathBuf>,
    dirty: bool,
    disk_base: Option<safe_file::Fingerprint>,
    file_conflict: Option<(document_safety::Slot, PathBuf)>,
    preview_sync: scroll_sync::PreviewSync,
    preview_heights: preview_layout::Heights,
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
            preview_heights: preview_layout::Heights::default(),
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
}

pub(crate) struct Tabs {
    pub(crate) list: Vec<Tab>,
    pub(crate) active: usize,
    next_id: u64,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            list: vec![Tab { id: 1, doc: None }],
            active: 0,
            next_id: 2,
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
    pub(crate) text: String,
    pub(crate) dirty: bool,
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

fn file_label(path: Option<&Path>) -> String {
    path.and_then(|p| p.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("untitled")
        .to_string()
}

impl SlateApp {
    pub(super) fn tab_labels(&self) -> Vec<TabLabel> {
        self.tabs
            .list
            .iter()
            .enumerate()
            .map(|(index, tab)| TabLabel {
                // Includes a file that is still loading.
                text: file_label(self.tab_path(index)),
                dirty: tab.doc.as_ref().map_or(self.dirty, |doc| doc.dirty),
            })
            .collect()
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
        self.footer_counts_cache = None;
        self.flow_inline_org_mode = None;
        self.flow_inline_org_anchor = None;
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
            preview_heights: std::mem::take(&mut self.preview_heights),
            search_state: self.search_state.take(),
            recovery_origin: self.recovery_origins.remove(&document_safety::Slot::Main),
            periscope_project_root: self.periscope_project_root.take(),
            lazy_path: loading,
        }
    }

    fn restore_document(&mut self, mut doc: DocumentState) {
        doc.editor_view.set_font_size(self.editor_font_size);
        self.buffer = doc.buffer;
        self.editor_view = doc.editor_view;
        self.path = doc.path;
        self.dirty = doc.dirty;
        self.disk_base = doc.disk_base;
        self.file_conflict = doc.file_conflict;
        self.preview_sync = doc.preview_sync;
        self.preview_heights = doc.preview_heights;
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
            },
        );
        self.activate_tab(index);
        self.status = "New tab".into();
    }

    fn new_lazy_tab(&mut self, path: PathBuf) {
        let id = self.tabs.allocate_id();
        let mut doc = DocumentState::blank(self.editor_font_size);
        doc.lazy_path = Some(path);
        self.tabs.list.push(Tab { id, doc: Some(doc) });
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
    pub(super) fn close_active_tab_now(&mut self) {
        let closed = file_label(self.path.as_deref());
        self.cancel_file_open();
        self.remember_current_cursor_position();
        self.clear_recovery(document_safety::Slot::Main);
        self.forget_project_buffer(self.buffer.identity);
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
            let refresh_daily = matches!(destination, OpenDestination::Daily { .. }) && !self.dirty;
            let still_loading = self.pending_file_open.is_some();
            if !refresh_daily && !still_loading {
                self.apply_open_destination(destination.clone());
                return false;
            }
            return refresh_daily;
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
        if self.scratch || self.standalone_agenda {
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
            let _ = safe_file::atomic_replace(&file, &json);
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
        if self.scratch || self.standalone_agenda {
            return false;
        }
        let mut step = 0isize;
        let mut goto = None;
        let mut wheel = 0.0f32;
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
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
