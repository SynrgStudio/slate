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
        // SlateApp::new read the user's real config; start from the defaults.
        app.apply_editor_look(editor_view::EditorLook::default());
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
        cursor_trail: true,
        cursor_trail_weight: 5,
        cursor_trail_typing: true,
        cursor_stretch: true,
        cursor_pulse: true,
        syntax_highlighting: true,
        cursor_color: editor_view::CURSOR_COLORS[3].1,
    });
    f.frame_after(0.016);
    let jetbrains = f.cursor().1.unwrap();
    assert_ne!(
        hack.x, jetbrains.x,
        "the editor now measures with JetBrains Mono"
    );

    let config = fs::read_to_string(f.root.join("config.toml")).unwrap();
    assert!(
        config.contains("editor_font = \"jetbrains-mono\""),
        "{config}"
    );
    assert!(config.contains("block_cursor = true"));
    assert!(config.contains("cursor_animation = false"));
    assert!(config.contains("cursor_trail = true"));
    assert!(config.contains("cursor_trail_weight = 5"));
    assert!(config.contains("cursor_trail_typing = true"));
    assert!(config.contains("cursor_stretch = true"));
    assert!(config.contains("cursor_pulse = true"));
    assert!(config.contains("cursor_color = \"#ebcb8b\""), "{config}");
    f.app.editor_look = Default::default();
    f.app.load_settings();
    assert_eq!(
        f.app.editor_look,
        editor_view::EditorLook {
            font: editor_view::EditorFont::JetBrainsMono,
            block_cursor: true,
            animate_cursor: false,
            cursor_trail: true,
            cursor_trail_weight: 5,
            cursor_trail_typing: true,
            cursor_stretch: true,
            cursor_pulse: true,
            syntax_highlighting: true,
            cursor_color: editor_view::CURSOR_COLORS[3].1,
        }
    );
}

#[test]
fn with_a_trail_the_cursor_lands_on_time_and_the_trail_catches_up() {
    let mut f = Fixture::new(
        "uno
dos
tres
cuatro
cinco
",
    );
    f.app.apply_editor_look(editor_view::EditorLook {
        cursor_trail: true,
        ..animated()
    });
    f.frame_after(0.016);
    f.app.buffer.move_to_bottom();
    f.frame_after(0.0);
    f.frame_after(0.1);
    let (drawn, target, animating) = f.cursor();
    assert_eq!(drawn, target, "the cursor itself has landed after 90 ms");
    assert!(animating, "the trail is still catching up");
    f.frame_after(0.1);
    assert!(!f.cursor().2, "then everything rests");
}

#[test]
fn cursor_colors_round_trip_and_cycle() {
    for (_, color) in editor_view::CURSOR_COLORS {
        let hex = editor_view::color_to_hex(color);
        assert_eq!(editor_view::color_from_hex(&hex), Some(color));
    }
    assert_eq!(
        editor_view::color_from_hex("\"#12aBcd\""),
        Some(egui::Color32::from_rgb(0x12, 0xab, 0xcd))
    );
    assert_eq!(editor_view::color_from_hex("nope"), None);
    let first = editor_view::CURSOR_COLORS[0].1;
    let last = editor_view::CURSOR_COLORS[editor_view::CURSOR_COLORS.len() - 1].1;
    assert_eq!(editor_view::cycle_cursor_color(first, false), last);
    assert_eq!(editor_view::cycle_cursor_color(last, true), first);
    let custom = egui::Color32::from_rgb(1, 2, 3);
    assert_eq!(editor_view::cycle_cursor_color(custom, true), first);
}

#[test]
fn typing_can_leave_a_short_trail_without_moving_the_cursor_late() {
    let mut f = Fixture::new("hola\n");
    f.app.apply_editor_look(editor_view::EditorLook {
        cursor_trail: true,
        cursor_trail_typing: true,
        ..animated()
    });
    f.frame_after(0.016);
    f.app.buffer.insert_text("x");
    f.frame_after(0.016);
    let (drawn, target, animating) = f.cursor();
    assert_eq!(drawn, target, "the cursor is already after the new letter");
    assert!(animating, "a short trail follows it");
    f.frame_after(0.4);
    assert!(!f.cursor().2);

    // Off: typing draws no trail and schedules nothing.
    f.app.apply_editor_look(editor_view::EditorLook {
        cursor_trail: true,
        ..animated()
    });
    f.app.buffer.insert_text("y");
    f.frame_after(0.016);
    assert!(!f.cursor().2);
}

#[test]
fn a_heavier_trail_lasts_longer() {
    let mut light = Fixture::new("uno\ndos\ntres\ncuatro\n");
    let mut heavy = Fixture::new("uno\ndos\ntres\ncuatro\n");
    for (f, weight) in [(&mut light, 1), (&mut heavy, 5)] {
        f.app.apply_editor_look(editor_view::EditorLook {
            cursor_trail: true,
            cursor_trail_weight: weight,
            ..animated()
        });
        f.frame_after(0.016);
        f.app.buffer.move_to_bottom();
        f.frame_after(0.0);
        f.frame_after(0.15);
    }
    assert!(!light.cursor().2, "weight 1 is done by 150 ms");
    assert!(heavy.cursor().2, "weight 5 is still trailing");
}

#[test]
fn a_stretched_jump_lands_exactly_and_then_rests() {
    let mut f = Fixture::new("uno\ndos\ntres\ncuatro\n");
    f.app.apply_editor_look(editor_view::EditorLook {
        cursor_stretch: true,
        block_cursor: true,
        ..animated()
    });
    f.frame_after(0.016);
    f.app.buffer.move_to_bottom();
    f.frame_after(0.0);
    f.frame_after(0.1);
    assert!(
        f.cursor().2,
        "the trailing edge is still catching up at 100 ms"
    );
    f.frame_after(0.1);
    let (drawn, target, animating) = f.cursor();
    assert!(!animating);
    assert_eq!(drawn, target);
}

#[test]
fn a_pulse_follows_a_jump_but_not_typing() {
    let mut f = Fixture::new("uno\ndos\ntres\n");
    f.app.apply_editor_look(editor_view::EditorLook {
        cursor_pulse: true,
        syntax_highlighting: true,
        ..Default::default()
    });
    f.frame_after(0.016);
    f.app.buffer.insert_text("x");
    f.frame_after(0.016);
    assert!(!f.app.editor_view.cursor_pulsing(), "typing does not pulse");
    f.app.buffer.move_to_bottom();
    f.frame_after(0.016);
    assert!(
        f.app.editor_view.cursor_pulsing(),
        "a jump pulses, even without the glide"
    );
    f.frame_after(0.3);
    f.frame_after(0.016);
    assert!(!f.app.editor_view.cursor_pulsing(), "and then it rests");
}

fn kinds(
    view: &editor_view::EditorView,
    buffer: &EditorBuffer,
    line: usize,
) -> Option<Vec<(String, highlight::Token)>> {
    let text = buffer.line(line).to_string();
    view.line_highlight(buffer, line).map(|tokens| {
        tokens
            .into_iter()
            .map(|(range, token)| (text[range].to_string(), token))
            .collect()
    })
}

#[test]
fn code_files_are_coloured_and_edits_recolour_the_lines_below() {
    let mut view = editor_view::EditorView::new();
    view.set_look(editor_view::EditorLook::default());
    view.set_file_language(Some(highlight::Lang::Rust));
    let mut buffer = EditorBuffer::from_text("fn a() {}\nlet x = 1;\n// end\n".into());
    assert_eq!(
        kinds(&view, &buffer, 1).unwrap()[0],
        ("let".to_string(), highlight::Token::Keyword)
    );
    // Opening a block comment on line 0 turns the following lines into comment.
    buffer.set_cursor(0);
    buffer.insert_text("/* ");
    assert_eq!(
        kinds(&view, &buffer, 1).unwrap(),
        vec![("let x = 1;".to_string(), highlight::Token::Comment)]
    );
    assert!(buffer.undo());
    assert_eq!(
        kinds(&view, &buffer, 1).unwrap()[0],
        ("let".to_string(), highlight::Token::Keyword)
    );
}

#[test]
fn notes_colour_only_fenced_blocks_with_a_known_language() {
    let mut view = editor_view::EditorView::new();
    view.set_look(editor_view::EditorLook::default());
    let buffer = EditorBuffer::from_text(
        "# def title\n```py\ndef go():\n```\n```brainfuck\n+[-]>.\n```\nlet after\n".into(),
    );
    assert_eq!(kinds(&view, &buffer, 0), None, "prose is not code");
    assert_eq!(kinds(&view, &buffer, 1), None, "the fence line itself");
    assert_eq!(
        kinds(&view, &buffer, 2).unwrap()[0],
        ("def".to_string(), highlight::Token::Keyword)
    );
    assert_eq!(
        kinds(&view, &buffer, 5),
        None,
        "unknown languages stay plain"
    );
    assert_eq!(
        kinds(&view, &buffer, 7),
        None,
        "after the block it is prose again"
    );

    view.set_look(editor_view::EditorLook {
        syntax_highlighting: false,
        ..Default::default()
    });
    assert_eq!(kinds(&view, &buffer, 2), None, "the toggle turns it off");
}
