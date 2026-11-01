use super::*;

struct Fixture {
    app: SlateApp,
    ctx: egui::Context,
    frame: eframe::Frame,
    time: f64,
    root: PathBuf,
}

impl Fixture {
    /// Boxed: the editor's widget id comes from its address, so the fixture
    /// must not move after it has taken keyboard focus.
    fn new(text: &str) -> Box<Self> {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, false, None);
        app.scratch = false;
        let root = std::env::temp_dir().join(format!("slate-look-{}", crate::test_unique()));
        fs::create_dir_all(&root).unwrap();
        app.settings_path_override = Some(root.join("config.toml"));
        app.buffer.set_text(text.into());
        app.buffer.set_cursor(0);
        app.focus_editor_once = true;
        let mut f = Box::new(Self {
            app,
            ctx,
            frame: eframe::Frame::_new_kittest(),
            time: 0.0,
            root,
        });
        for _ in 0..3 {
            f.frame_after(0.016);
        }
        f
    }

    /// Runs one frame `seconds` after the previous one.
    fn frame_after(&mut self, seconds: f64) {
        self.time += seconds;
        let (app, frame) = (&mut self.app, &mut self.frame);
        let _ = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 560.0),
                )),
                time: Some(self.time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| eframe::App::ui(app, ui, frame));
            },
        );
    }

    fn cursor(&self) -> (Option<egui::Pos2>, Option<egui::Pos2>, bool) {
        self.app.editor_view.cursor_state()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn animated() -> editor_view::EditorLook {
    editor_view::EditorLook {
        animate_cursor: true,
        ..Default::default()
    }
}

#[test]
fn a_cursor_jump_glides_and_lands_while_typing_stays_instant() {
    let mut f = Fixture::new("uno\ndos\ntres\ncuatro\ncinco\n");
    f.app.apply_editor_look(animated());
    f.frame_after(0.016);
    let (start, _, _) = f.cursor();
    assert!(start.is_some(), "the cursor is drawn");

    f.app.buffer.move_to_bottom();
    f.frame_after(0.016);
    let (drawn, target, gliding) = f.cursor();
    assert!(gliding, "a jump glides");
    assert_ne!(drawn, target, "mid-glide it is between the two places");
    f.frame_after(0.2);
    let (drawn, target, gliding) = f.cursor();
    assert!(!gliding);
    assert_eq!(drawn, target, "it lands exactly on the new place");

    f.app.buffer.insert_text("x");
    f.frame_after(0.016);
    let (drawn, target, gliding) = f.cursor();
    assert!(!gliding, "typing a character never lags behind");
    assert_eq!(drawn, target);
}

#[test]
fn without_animation_the_cursor_jumps_at_once() {
    let mut f = Fixture::new("uno\ndos\ntres\n");
    f.app.buffer.move_to_bottom();
    f.frame_after(0.016);
    let (drawn, target, gliding) = f.cursor();
    assert!(drawn.is_some(), "the cursor is drawn");
    assert!(!gliding);
    assert_eq!(drawn, target);
}

#[test]
fn jetbrains_mono_changes_the_text_metrics_and_everything_persists() {
    let mut f = Fixture::new("una línea de texto");
    f.app.buffer.move_to_line_end();
    f.frame_after(0.016);
    let hack = f.cursor().1.unwrap();
    f.app.set_editor_look(editor_view::EditorLook {
        font: editor_view::EditorFont::JetBrainsMono,
        block_cursor: true,
        animate_cursor: false,
    });
    f.frame_after(0.016);
    let jetbrains = f.cursor().1.unwrap();
    assert_ne!(hack.x, jetbrains.x, "the editor now measures with JetBrains Mono");

    let config = fs::read_to_string(f.root.join("config.toml")).unwrap();
    assert!(config.contains("editor_font = \"jetbrains-mono\""), "{config}");
    assert!(config.contains("block_cursor = true"));
    assert!(config.contains("cursor_animation = false"));
    f.app.editor_look = Default::default();
    f.app.load_settings();
    assert_eq!(
        f.app.editor_look,
        editor_view::EditorLook {
            font: editor_view::EditorFont::JetBrainsMono,
            block_cursor: true,
            animate_cursor: false,
        }
    );
}
