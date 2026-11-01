#![allow(deprecated)]
use super::*;
struct Fixture {
    root: PathBuf,
    app: SlateApp,
    ctx: egui::Context,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("slate-project-ui-{}", crate::test_unique()));
        fs::create_dir_all(root.join("project")).unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, false, None);
        app.settings_path_override = Some(root.join("config.toml"));
        app.buffer.set_text("happy unsaved document".into());
        app.dirty = true;
        Self { root, app, ctx }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.periscope_engine.cancel();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn without_explicit_membership_git_and_previous_scope_do_not_prompt_or_leak() {
    let mut f = Fixture::new();
    fs::create_dir_all(f.root.join("project/.git")).unwrap();
    f.app.path = Some(f.root.join("project/note.md"));
    f.app.periscope_project_root = Some(f.root.join("stale-root"));
    f.app.choose_periscope_scope();
    assert!(f.app.periscope_open);
    assert!(!f.app.periscope_prompt_open);
    assert!(f.app.periscope_project_root.is_none());
    assert!(f.app.periscope_mode == PeriscopeMode::Global);
    assert_eq!(f.app.buffer.as_str(), "happy unsaved document");
    assert!(f.app.dirty);
}
#[test]
fn choosing_a_project_preserves_unsaved_text_and_only_then_enables_scope_prompt() {
    let mut f = Fixture::new();
    let identity = f.app.buffer.identity;
    let revision = f.app.buffer.revision;
    f.app.create_document_project();
    assert!(f.app.file_picker_open);
    assert!(!f.app.periscope_open);
    f.app.assign_document_project(f.root.join("project"));
    assert!(!f.app.file_picker_open);
    assert!(!f.app.periscope_open);
    assert_eq!(f.app.buffer.identity, identity);
    assert_eq!(f.app.buffer.revision, revision);
    assert_eq!(f.app.buffer.as_str(), "happy unsaved document");
    assert!(f.app.path.is_none());
    assert!(f.app.dirty);
    assert_eq!(fs::read_dir(f.root.join("project")).unwrap().count(), 0);
    f.app.open_save_as_modal();
    assert_eq!(f.app.save_as_dir, f.root.join("project"));
    assert!(f.app.dirty);
    assert!(f.app.path.is_none());
    f.app.save_as_open = false;
    f.app.choose_periscope_scope();
    assert!(f.app.periscope_prompt_open);
    assert!(!f.app.periscope_open);
    f.app.choose_periscope_project();
    assert!(f.app.periscope_open);
    assert!(f.app.periscope_mode == PeriscopeMode::Project);
    f.app.dirty = false;
    f.app.new_buffer();
    assert!(f.app.document_project().is_none());
    f.app.choose_periscope_scope();
    assert!(!f.app.periscope_prompt_open);
    assert!(f.app.periscope_mode == PeriscopeMode::Global);
}
#[test]
fn explicit_save_as_carries_membership_across_restart_even_outside_the_folder() {
    let mut f = Fixture::new();
    f.app.assign_document_project(f.root.join("project"));
    let destination = f.root.join("outside.md");
    f.app.save_path(destination.clone());
    assert_eq!(
        fs::read_to_string(&destination).unwrap(),
        "happy unsaved document"
    );
    assert!(!f.app.dirty);
    let cc = eframe::CreationContext::_new_kittest(f.ctx.clone());
    let mut other = SlateApp::new(&cc, None, true, false, None);
    other.settings_path_override = Some(f.root.join("config.toml"));
    other.path = Some(destination);
    assert_eq!(other.document_project(), Some(f.root.join("project")));
    other.path = Some(f.root.join("unrelated.md"));
    assert!(other.document_project().is_none());
}
#[test]
fn membership_follows_a_swapped_untitled_document_not_the_window() {
    let mut f = Fixture::new();
    let project = f.root.join("project");
    f.app.assign_document_project(project.clone());
    f.app.link_preview_buffer.set_text("unrelated modal".into());
    f.app.link_preview_path = Some(f.root.join("unrelated.md"));
    f.app.link_preview_dirty = false;
    f.app.swap_link_preview_with_main();
    assert!(f.app.document_project().is_none());
    assert_eq!(f.app.buffer.as_str(), "unrelated modal");
    f.app.swap_link_preview_with_main();
    assert_eq!(f.app.document_project(), Some(project));
    assert_eq!(f.app.buffer.as_str(), "happy unsaved document");
    assert!(f.app.dirty);
}
#[test]
fn cancelled_picker_and_current_folder_shortcut_do_not_save_the_document() {
    let mut f = Fixture::new();
    f.app.create_document_project();
    f.app.file_picker_dir = f.root.join("project");
    let _ = f.ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| f.app.shortcuts(ctx),
    );
    assert!(!f.app.file_picker_open);
    assert!(f.app.document_project().is_none());
    assert!(!f.root.join("projects.json").exists());
    f.app.create_document_project();
    f.app.file_picker_dir = f.root.join("project");
    let _ = f.ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
            ..Default::default()
        },
        |ctx| f.app.shortcuts(ctx),
    );
    assert!(!f.app.file_picker_open);
    assert_eq!(f.app.document_project(), Some(f.root.join("project")));
    assert!(f.app.dirty);
    assert!(f.app.path.is_none());
}
