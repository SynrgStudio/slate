#![allow(deprecated)]
use super::*;
use std::time::{Duration, Instant};
struct Fixture {
    root: PathBuf,
    cache: PathBuf,
    app: SlateApp,
    ctx: egui::Context,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "slate-preparation-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(root.join("files")).unwrap();
        fs::write(root.join("files/café.md"), "private content stays on disk").unwrap();
        let cache = root.join("cache/paths.sqlite3");
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, false, None);
        app.periscope_engine = periscope_backend::Manager::new(cache.clone());
        app.periscope_roots = vec![root.join("files")];
        app.periscope_everything = false;
        app.buffer.set_text("visible unsaved text".into());
        app.dirty = true;
        Self {
            root,
            cache,
            app,
            ctx,
        }
    }
    fn poll_until(&mut self, condition: impl Fn(&SlateApp) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            self.app.poll_periscope_setup(&self.ctx);
            if condition(&self.app) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("setup timeout: {}", self.app.status);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.cancel_periscope_setup();
        self.app.periscope_engine.cancel();
        thread::sleep(Duration::from_millis(150));
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn first_use_requires_consent_then_completion_before_global_search_and_persists() {
    let mut f = Fixture::new();
    f.app.open_periscope();
    f.poll_until(|app| app.periscope_setup.awaiting_consent());
    assert!(!f.cache.exists());
    assert!(!f.app.periscope_open);
    assert!(f.app.periscope_setup.visible);
    f.app.open_global_periscope();
    f.app.choose_periscope_project();
    assert!(!f.app.periscope_open);
    assert!(f.app.periscope_request.is_none());
    f.app.accept_periscope_indexing();
    assert!(!f.app.periscope_open);
    assert!(f.app.periscope_setup.visible);
    f.app.open_global_periscope();
    assert!(!f.app.periscope_open);
    f.poll_until(|app| !app.periscope_setup.visible);
    assert!(!f.app.periscope_prompt_open);
    assert!(f.app.periscope_open);
    assert!(f.app.periscope_mode == PeriscopeMode::Global);
    assert!(periscope_store::Store::prepared(&f.cache, &f.app.periscope_roots).unwrap());
    assert_eq!(f.app.buffer.as_str(), "visible unsaved text");
    assert!(f.app.dirty);
    assert_eq!(
        fs::read_to_string(f.root.join("files/café.md")).unwrap(),
        "private content stays on disk"
    );
    f.app.periscope_prompt_open = false;
    f.app.open_periscope();
    f.poll_until(|app| !app.periscope_setup.visible);
    assert!(!f.app.periscope_prompt_open);
    assert!(f.app.periscope_open);
}
#[test]
fn declining_cancels_without_scan_and_new_roots_require_new_consent() {
    let mut f = Fixture::new();
    f.app.open_periscope();
    f.poll_until(|app| app.periscope_setup.awaiting_consent());
    f.app.cancel_periscope_setup();
    assert!(!f.cache.exists());
    assert!(!f.app.periscope_prompt_open);
    assert!(!f.app.periscope_open);
    f.app.open_periscope();
    f.poll_until(|app| app.periscope_setup.awaiting_consent());
    f.app.accept_periscope_indexing();
    f.poll_until(|app| !app.periscope_setup.visible);
    fs::create_dir_all(f.root.join("other")).unwrap();
    f.app.periscope_roots = vec![f.root.join("other")];
    f.app.open_periscope();
    f.poll_until(|app| app.periscope_setup.awaiting_consent());
    assert!(!f.app.periscope_open);
}
#[test]
fn immediate_scan_cancel_never_opens_search_or_marks_an_interrupted_scan_ready() {
    let mut f = Fixture::new();
    for index in 0..400 {
        fs::write(f.root.join(format!("files/file-{index}.md")), "content").unwrap();
    }
    f.app.open_periscope();
    f.poll_until(|app| app.periscope_setup.awaiting_consent());
    f.app.accept_periscope_indexing();
    f.app.cancel_periscope_setup();
    thread::sleep(Duration::from_millis(150));
    f.app.poll_periscope_setup(&f.ctx);
    assert!(!f.app.periscope_open);
    assert!(!f.app.periscope_prompt_open);
    assert!(!periscope_store::Store::prepared(&f.cache, &f.app.periscope_roots).unwrap());
    assert!(f.root.join("files/café.md").exists());
}
#[test]
fn unavailable_root_and_database_failure_do_not_mark_prepared() {
    let f = Fixture::new();
    let roots = vec![f.root.join("missing")];
    let warm = f.app.periscope_engine.prepare(roots.clone());
    let end = Instant::now() + Duration::from_secs(5);
    while warm.result().is_none() && Instant::now() < end {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(warm.result().unwrap().is_err());
    assert!(!periscope_store::Store::prepared(&f.cache, &roots).unwrap());
    let corrupt = f.root.join("broken.sqlite3");
    fs::write(&corrupt, "not SQLite").unwrap();
    let manager = periscope_backend::Manager::new(corrupt.clone());
    let warm = manager.prepare(f.app.periscope_roots.clone());
    let end = Instant::now() + Duration::from_secs(5);
    while warm.result().is_none() && Instant::now() < end {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(warm.result().unwrap().is_err());
    assert_eq!(fs::read_to_string(corrupt).unwrap(), "not SQLite");
}
#[test]
fn prep_dialog_is_centered_with_fixed_chrome_and_escape_blocks_search() {
    let mut f = Fixture::new();
    f.app.open_periscope();
    f.poll_until(|app| app.periscope_setup.awaiting_consent());
    for size in [Vec2::new(420.0, 260.0), Vec2::new(900.0, 560.0)] {
        for _ in 0..3 {
            let _ = f.ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| f.app.periscope_setup_dialog(ctx),
            );
        }
        let geo = f
            .ctx
            .data(|data| {
                data.get_temp::<dialog_ui::Geometry>(
                    egui::Id::new("periscope_setup_dialog").with("geometry"),
                )
            })
            .unwrap();
        assert!(
            (geo.frame.center() - egui::pos2(size.x * 0.5, size.y * 0.5 + 15.0)).length() < 1.0
        );
        assert!(geo.frame.contains_rect(geo.header));
        assert!(geo.frame.contains_rect(geo.footer));
        assert!(geo.body.bottom() < geo.footer.top());
    }
    let _ = f.ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: Some(Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| f.app.shortcuts(ctx),
    );
    assert!(!f.app.periscope_setup.visible);
    assert!(!f.app.periscope_open);
    assert!(!f.cache.exists());
}
