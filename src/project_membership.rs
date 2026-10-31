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
        if self.loaded.as_deref() == Some(path) {
            return;
        }
        self.loaded = Some(path.to_path_buf());
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
        Ok(())
    }
    fn associate(&mut self, id: u64, path: Option<&Path>, root: PathBuf) -> Result<(), String> {
        if path.is_none() {
            self.buffers.insert(id, root.clone());
        } else {
            self.buffers.remove(&id);
        }
        if !self.data.roots.iter().any(|other| key(other) == key(&root)) {
            self.data.roots.push(root.clone());
        }
        if let Some(path) = path {
            self.data.files.insert(key(path), root);
        }
        self.save()
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
        if self.periscope_setup.visible
            || self.recovery_open
            || self.file_conflict.is_some()
            || self.pending_action.is_some()
            || self.link_preview_open
            || self.scratch_modal_open
            || self.capture_modal_open
            || self.flow_item_edit.is_some()
            || self.standalone_agenda
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
        self.agenda_open = false;
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
        let bytes = fs::read(&path).unwrap();
        assert!(second.associate(2, None, base.join("two")).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
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
