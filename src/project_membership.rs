//! Explicit document/folder membership. Never creates/moves/saves project documents.
use super::*;
use std::collections::{BTreeMap, HashMap};
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    version: u32,
    roots: Vec<PathBuf>,
    files: BTreeMap<String, PathBuf>,
}
impl Default for Data {
    fn default() -> Self {
        Self {
            version: 1,
            roots: vec![],
            files: BTreeMap::new(),
        }
    }
}
#[derive(Default)]
pub(super) struct State {
    data: Data,
    buffers: HashMap<u64, PathBuf>,
    loaded: Option<PathBuf>,
    /// Modification time and size when loaded: another window may change
    /// the file, and then it is read again.
    stamp: Option<(Option<std::time::SystemTime>, u64)>,
    base: Option<safe_file::Fingerprint>,
    error: Option<String>,
}
fn key(path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned();
    #[cfg(windows)]
    {
        text.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        text
    }
}
impl State {
    fn load(&mut self, path: &Path) {
        let stamp = fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.modified().ok(), metadata.len()));
        if self.loaded.as_deref() == Some(path) && self.stamp == stamp {
            return;
        }
        self.loaded = Some(path.to_path_buf());
        self.stamp = stamp;
        self.error = None;
        self.base = None;
        self.data = Data::default();
        if let Ok(metadata) = fs::metadata(path) {
            if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 {
                self.error = Some("Project metadata is not a small regular file; retained".into());
                return;
            }
        }
        match fs::read(path) {
            Ok(bytes) => {
                self.base = Some(safe_file::fingerprint(&bytes));
                match serde_json::from_slice::<Data>(&bytes) {
                    Ok(data) if data.version == 1 => self.data = data,
                    _ => {
                        self.error =
                            Some("Invalid/unsupported project metadata; file retained".into())
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    fn project(&self, id: u64, path: Option<&Path>) -> Option<PathBuf> {
        let Some(path) = path else {
            return self.buffers.get(&id).cloned();
        };
        let value = key(path);
        if let Some(root) = self.data.files.get(&value) {
            return Some(root.clone());
        }
        self.data
            .roots
            .iter()
            .filter(|root| value.starts_with(&format!("{}/", key(root))))
            .max_by_key(|root| key(root).len())
            .cloned()
    }
    fn save(&mut self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let path = self.loaded.as_ref().ok_or("No project metadata location")?;
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(|error| error.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Project metadata exceeds size limit; existing file retained".into());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        self.base = Some(
            safe_file::checked_save(path, &bytes, self.base)
                .map_err(|error| format!("{error:?}"))?,
        );
        self.stamp = fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.modified().ok(), metadata.len()));
        Ok(())
    }
    /// Records `root` as the project of buffer `id` / document `path`.
    /// Starts from the file as it is on disk, so another window's changes
    /// are kept, and tries once more if it changed again meanwhile.
    fn associate(&mut self, id: u64, path: Option<&Path>, root: PathBuf) -> Result<(), String> {
        if path.is_none() {
            self.buffers.insert(id, root.clone());
        } else {
            self.buffers.remove(&id);
        }
        let file = self.loaded.clone().ok_or("No project metadata location")?;
        let mut attempt = 0;
        loop {
            self.loaded = None;
            self.load(&file);
            let mut changed = false;
            if !self.data.roots.iter().any(|other| key(other) == key(&root)) {
                self.data.roots.push(root.clone());
                changed = true;
            }
            if let Some(path) = path {
                // A document inside the root needs no entry of its own.
                let explicit = self.data.files.get(&key(path));
                let covered =
                    explicit.is_none() && self.project(id, Some(path)) == Some(root.clone());
                if !covered && explicit.is_none_or(|old| key(old) != key(&root)) {
                    self.data.files.insert(key(path), root.clone());
                    changed = true;
                }
            }
            if !changed && self.error.is_none() {
                return Ok(());
            }
            match self.save() {
                Err(_) if attempt == 0 && self.error.is_none() => attempt += 1,
                result => return result,
            }
        }
    }
}
impl SlateApp {
    fn project_metadata_path(&self) -> Option<PathBuf> {
        #[cfg(test)]
        if let Some(path) = &self.settings_path_override {
            return path.parent().map(|path| path.join("projects.json"));
        }
        Self::settings_path()?
            .parent()
            .map(|path| path.join("projects.json"))
    }
    pub(super) fn document_project(&mut self) -> Option<PathBuf> {
        if let Some(path) = self.project_metadata_path() {
            self.project_membership.load(&path);
        }
        self.project_membership
            .project(self.buffer.identity, self.path.as_deref())
    }
    pub(super) fn create_document_project(&mut self) {
        if self.recovery_open
            || self.file_conflict.is_some()
            || self.pending_action.is_some()
            || self.link_preview_open
            || self.scratch_modal_open
            || self.capture_modal_open
            || self.link_assist_open
        {
            self.status = "Close the current dialog before creating a project".into();
            return;
        }
        self.cancel_file_open();
        self.periscope_open = false;
        self.periscope_prompt_open = false;
        self.periscope_engine.cancel();
        self.periscope_request = None;
        self.settings_open = false;
        self.find_note_open = false;
        self.doc_tasks_open = false;
        self.link_heading_picker_open = false;
        self.scratch_entries_open = false;
        self.open_file_picker_at(self.project_root(), FilePickerMode::SetPeriscopeProject);
        self.status="Create project · Enter chooses selected folder, → navigates, Ctrl+Enter uses current folder · Esc cancels".into();
    }
    pub(super) fn assign_document_project(&mut self, root: PathBuf) {
        if !root.is_dir() {
            self.status = "Project folder unavailable; document unchanged".into();
            return;
        }
        let root = std::path::absolute(&root).unwrap_or(root);
        if let Some(path) = self.project_metadata_path() {
            self.project_membership.load(&path);
        }
        let result = self.project_membership.associate(
            self.buffer.identity,
            self.path.as_deref(),
            root.clone(),
        );
        self.file_picker_open = false;
        self.focus_editor_once = true;
        self.periscope_project_root = Some(root.clone());
        self.status = match result {
            Ok(()) => format!(
                "Document project: {} · document text unchanged",
                root.display()
            ),
            Err(error) => {
                format!("Project assigned for this session · metadata not saved: {error}")
            }
        };
    }
    pub(super) fn persist_saved_project(
        &mut self,
        path: &Path,
        root: Option<PathBuf>,
    ) -> Result<(), String> {
        if let Some(root) = root {
            self.project_membership
                .associate(self.buffer.identity, Some(path), root)?;
        }
        Ok(())
    }
    pub(super) fn forget_project_buffer(&mut self, id: u64) {
        self.project_membership.buffers.remove(&id);
    }
    pub(super) fn clear_buffer_project(&mut self) {
        self.project_membership
            .buffers
            .remove(&self.buffer.identity);
        self.periscope_project_root = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_membership_survives_restart_and_does_not_leak_to_unrelated_documents() {
        let base = std::env::temp_dir().join(format!(
            "slate-project-membership-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let metadata = base.join("projects.json");
        let root = base.join("project");
        let outside = base.join("outside.md");
        let mut state = State::default();
        state.load(&metadata);
        state.associate(1, None, root.clone()).unwrap();
        state.associate(2, Some(&outside), root.clone()).unwrap();
        assert_eq!(state.project(1, None), Some(root.clone()));
        assert_eq!(
            state.project(2, Some(&root.join("child.md"))),
            Some(root.clone())
        );
        assert_eq!(state.project(2, Some(&base.join("unrelated.md"))), None);
        let mut restarted = State::default();
        restarted.load(&metadata);
        assert_eq!(restarted.project(3, Some(&outside)), Some(root));
        assert_eq!(restarted.project(3, None), None);
        assert!(!outside.exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn concurrent_project_metadata_change_is_not_overwritten() {
        let base = std::env::temp_dir().join(format!(
            "slate-project-conflict-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let path = base.join("projects.json");
        let mut first = State::default();
        let mut second = State::default();
        first.load(&path);
        second.load(&path);
        first.associate(1, None, base.join("one")).unwrap();
        // The second window starts from the file as the first one left it.
        second.associate(2, None, base.join("two")).unwrap();
        let mut reread = State::default();
        reread.load(&path);
        assert_eq!(reread.data.roots, vec![base.join("one"), base.join("two")]);
        // And the first one sees the second's change without a restart.
        let doc = base.join("two").join("a.md");
        first.load(&path);
        assert_eq!(first.project(3, Some(&doc)), Some(base.join("two")));
        // A document already inside its root gets no entry of its own.
        first.associate(3, Some(&doc), base.join("two")).unwrap();
        reread.load(&path);
        assert!(reread.data.files.is_empty());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn corrupted_project_metadata_is_not_overwritten() {
        let base = std::env::temp_dir().join(format!(
            "slate-project-corrupt-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&base).unwrap();
        let path = base.join("projects.json");
        fs::write(&path, b"bad json").unwrap();
        let mut state = State::default();
        state.load(&path);
        assert!(state.associate(1, None, base.clone()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"bad json");
        fs::remove_dir_all(base).unwrap();
    }
}
