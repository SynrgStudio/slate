#![allow(deprecated)] // Match the existing headless egui test harness.
use super::*;
#[derive(Clone, Debug)]
pub(super) struct SettingsDialogGeometry {
    pub frame: egui::Rect,
    pub header: egui::Rect,
    pub footer: egui::Rect,
    pub body: egui::Rect,
    pub rows: Vec<egui::Rect>,
}
impl Default for SettingsDialogGeometry {
    fn default() -> Self {
        Self {
            frame: egui::Rect::NOTHING,
            header: egui::Rect::NOTHING,
            footer: egui::Rect::NOTHING,
            body: egui::Rect::NOTHING,
            rows: Vec::new(),
        }
    }
}
fn app(ctx: &egui::Context) -> SlateApp {
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, None);
    app.settings_open = true;
    app
}
fn draw(
    app: &mut SlateApp,
    ctx: &egui::Context,
    size: Vec2,
    time: f64,
    events: Vec<egui::Event>,
) -> SettingsDialogGeometry {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| app.settings_dialog(ctx),
    );
    ctx.data(|d| d.get_temp("settings_geometry".into()))
        .unwrap()
}
#[test]
fn settings_bounds_fixed_chrome_and_selection_reveal_at_small_and_large_sizes() {
    for size in [
        Vec2::new(420.0, 260.0),
        Vec2::new(900.0, 560.0),
        Vec2::new(1200.0, 800.0),
    ] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let mut before = SettingsDialogGeometry::default();
        for n in 0..10 {
            before = draw(&mut app, &ctx, size, n as f64 * 0.05, vec![]);
        }
        let expected = Vec2::new(size.x * 0.8, (size.y - 30.0) * 0.8);
        assert!(
            (before.frame.width() - expected.x).abs() < 1.0,
            "{:?} {:?}",
            before.frame,
            expected
        );
        assert!(
            (before.frame.height() - expected.y).abs() <= 1.0,
            "{:?} {:?}",
            before.frame,
            expected
        );
        assert!(before.frame.contains_rect(before.header));
        assert!(before.frame.contains_rect(before.footer));
        assert!(before.body.top() >= before.header.bottom());
        assert!(before.body.bottom() <= before.footer.top());
        assert_eq!(before.rows.len(), app.settings_last_row() + 1);
        assert!(
            before.rows[0].width() >= before.body.width() - 2.0,
            "unused right column: {:?}",
            before
        );
        app.selected_setting = 7;
        let mut after = before.clone();
        for n in 10..25 {
            after = draw(&mut app, &ctx, size, n as f64 * 0.05, vec![]);
        }
        assert_eq!(before.frame, after.frame);
        assert_eq!(before.header, after.header);
        assert_eq!(before.footer, after.footer);
        assert!(
            after.body.intersects(after.rows[7]),
            "selected row invisible: {:?}",
            after
        );
        assert!(
            after.rows[7].top() >= after.body.top() - 1.0,
            "selected controls clipped: {:?}",
            after
        );
        // The last row (start with Windows) is reachable too.
        app.selected_setting = 20;
        for n in 25..40 {
            after = draw(&mut app, &ctx, size, n as f64 * 0.05, vec![]);
        }
        assert!(
            after.body.contains_rect(after.rows[20]),
            "last row invisible: {:?}",
            after
        );
    }
}
#[test]
fn wheel_moves_settings_content_but_not_header_or_footer() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    // The footer tab has enough rows to scroll.
    app.selected_setting = 8;
    let size = Vec2::new(900.0, 560.0);
    let mut before = SettingsDialogGeometry::default();
    for n in 0..10 {
        before = draw(&mut app, &ctx, size, n as f64 * 0.05, vec![]);
    }
    let mut after = draw(
        &mut app,
        &ctx,
        size,
        0.5,
        vec![
            egui::Event::PointerMoved(before.body.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: Vec2::new(0.0, -180.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for n in 11..25 {
        after = draw(&mut app, &ctx, size, n as f64 * 0.05, vec![]);
    }
    assert_eq!(before.frame, after.frame);
    assert_eq!(before.header, after.header);
    assert_eq!(before.footer, after.footer);
    assert!(after.rows[8].top() < before.rows[8].top() - 20.0);
}
#[test]
fn palette_header_and_footer_stay_fixed_while_commands_scroll() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.settings_open = false;
    app.palette_open = true;
    let size = Vec2::new(900.0, 560.0);
    let mut positions = Vec::new();
    for n in 0..20 {
        let events = if n == 10 {
            vec![
                egui::Event::PointerMoved(egui::pos2(450.0, 280.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -200.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        } else {
            vec![]
        };
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(n as f64 * 0.05),
                events,
                ..Default::default()
            },
            |ctx| app.command_palette(ctx),
        );
        if n == 9 || n == 19 {
            let text = |needle: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.job.text == needle => Some(t.pos),
                        _ => None,
                    })
                    .unwrap()
            };
            positions.push((text("command palette"), text("[esc]")));
        }
    }
    assert_eq!(positions[0], positions[1]);
    let rect = ctx.memory(|m| m.area_rect("command_palette")).unwrap();
    assert!((rect.width() - size.x * 0.8).abs() <= 1.0);
    assert!((rect.height() - (size.y - 30.0) * 0.8).abs() <= 1.0);
}

#[test]
fn font_setting_roundtrips_clamps_and_keyboard_adjusts_without_changing_text() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = std::env::temp_dir().join(format!(
        "slate-font-settings-{}-{}",
        std::process::id(),
        crate::test_unique()
    ));
    fs::create_dir_all(&root).unwrap();
    let config = root.join("config.toml");
    app.settings_path_override = Some(config.clone());
    app.buffer.set_text("keep me".into());
    let revision = app.buffer.revision;
    app.set_editor_font_size(18.0);
    assert_eq!(app.editor_font_size, 18.0);
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("editor_font_size = 18")
    );
    let mut restored = super::settings_tests::app(&ctx);
    restored.settings_path_override = Some(config.clone());
    restored.load_settings();
    assert_eq!(restored.editor_font_size, 18.0);
    for (size, expected) in [
        (100.0, 32.0),
        (-4.0, 8.0),
        (f32::NAN, 12.0),
        (f32::INFINITY, 12.0),
    ] {
        app.set_editor_font_size(size);
        assert_eq!(app.editor_font_size, expected);
    }
    app.selected_setting = 7;
    for (key, expected) in [
        (Key::ArrowRight, 13.0),
        (Key::ArrowLeft, 12.0),
        (Key::ArrowRight, 13.0),
        (Key::Enter, 12.0),
    ] {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| app.shortcuts(ctx),
        );
        assert_eq!(app.editor_font_size, expected);
    }
    assert_eq!(app.buffer.as_str(), "keep me");
    assert_eq!(app.buffer.revision, revision);
    let _ = fs::remove_dir_all(root);
}
#[test]
fn text_and_gutter_share_vertical_center_at_default_and_large_font_sizes() {
    for size in [8.0, 12.0, 24.0, 32.0] {
        let ctx = egui::Context::default();
        let mut view = EditorView::new();
        view.set_font_size(size);
        let mut buffer = EditorBuffer::from_text("abcdef\nnext".into());
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(800.0, 300.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.render(
                        ui,
                        &mut buffer,
                        false,
                        None,
                        LineNumberMode::Absolute,
                        false,
                        false,
                        None,
                        false,
                        None,
                    );
                });
            },
        );
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some(t)
                } else {
                    None
                }
            })
            .collect();
        let number = texts.iter().find(|t| t.galley.job.text == "1").unwrap();
        let text = texts
            .iter()
            .find(|t| t.galley.job.text == "abcdef")
            .unwrap();
        let center = |t: &egui::epaint::TextShape| t.pos.y + t.galley.size().y * 0.5;
        assert!((center(number) - center(text)).abs() < 0.5);
        assert_eq!(text.galley.job.sections[0].format.font_id.size, size);
        assert_eq!(number.galley.job.sections[0].format.font_id.size, 12.0);
    }
}

#[test]
fn settings_the_core_does_not_know_survive_a_save() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let root = std::env::temp_dir().join(format!(
        "slate-passthrough-settings-{}-{}",
        std::process::id(),
        crate::test_unique()
    ));
    fs::create_dir_all(&root).unwrap();
    let config = root.join("config.toml");
    fs::write(
        &config,
        "word_wrap = true\nflow_vaults = \"D:/notas\"\nagenda_week_start = monday\n",
    )
    .unwrap();
    app.settings_path_override = Some(config.clone());
    app.load_settings();
    app.set_editor_font_size(15.0);
    settings_writer::flush();
    let saved = fs::read_to_string(&config).unwrap();
    assert!(saved.contains("editor_font_size = 15"), "{saved}");
    assert!(saved.contains("flow_vaults = \"D:/notas\"\n"), "{saved}");
    assert!(saved.contains("agenda_week_start = monday\n"), "{saved}");
    // Loading again does not duplicate them.
    app.load_settings();
    app.set_editor_font_size(16.0);
    settings_writer::flush();
    let saved = fs::read_to_string(&config).unwrap();
    assert_eq!(saved.matches("flow_vaults").count(), 1, "{saved}");
    let _ = fs::remove_dir_all(root);
}
#[test]
fn tab_switches_settings_tabs_and_arrows_stay_inside_one() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let mut press = |app: &mut SlateApp, event| {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![event],
                ..Default::default()
            },
            |ctx| app.shortcuts(ctx),
        );
    };
    let names: Vec<_> = app.settings_tabs().iter().map(|(name, _)| *name).collect();
    assert_eq!(app.settings_tab(), 0);
    press(&mut app, key(Key::Tab, egui::Modifiers::NONE));
    assert_eq!(names[app.settings_tab()], "Editor");
    assert_eq!(app.selected_setting, 1);
    press(&mut app, key(Key::ArrowDown, egui::Modifiers::NONE));
    assert_eq!(app.selected_setting, 3, "next row of the same tab");
    press(&mut app, key(Key::ArrowUp, egui::Modifiers::NONE));
    press(&mut app, key(Key::ArrowUp, egui::Modifiers::NONE));
    assert_eq!(app.selected_setting, 1, "stops at the tab's first row");
    press(&mut app, key(Key::Tab, egui::Modifiers::SHIFT));
    press(&mut app, key(Key::Tab, egui::Modifiers::SHIFT));
    assert_eq!(names[app.settings_tab()], "Modules", "wraps around");
    // Every row belongs to exactly one tab.
    let mut rows: Vec<usize> = app.settings_tabs().into_iter().flat_map(|(_, rows)| rows).collect();
    rows.sort();
    assert_eq!(rows, (0..=app.settings_last_row()).collect::<Vec<_>>());
    // Only the current tab is drawn.
    app.selected_setting = 8;
    let geometry = draw(&mut app, &ctx, Vec2::new(900.0, 560.0), 0.0, vec![]);
    assert!(geometry.rows[8].is_positive());
    assert!(!geometry.rows[0].is_positive());
}
