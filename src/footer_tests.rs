#![allow(deprecated)]
use super::*;
#[test]
fn command_hints_hide_complete_blocks_at_every_width() {
    let ctx = egui::Context::default();
    let _=ctx.run(Default::default(),|ctx|{egui::CentralPanel::default().show(ctx,|ui| {
        let painter=ui.painter();let font=FontId::monospace(12.0);
        for hint in ["command  Ctrl+. enter · Ctrl+H help · Ctrl+P palette · w · q · wq","command  Ctrl+. enter · Tab complete · Ctrl+H help · Ctrl+P palette","files  type filter · ↑↓ select · → enter folder · ← parent · Enter open · Esc close","shortcuts  [esc] close","alt:sequence"] {
            let (label,rest)=hint.split_once("  ").unwrap_or((hint,""));let mut prefixes=vec![format!(":{label}")];
            for (index,block) in rest.split(" · ").filter(|block|!block.is_empty()).enumerate() {prefixes.push(format!("{}{}{block}",prefixes.last().unwrap(),if index==0 {"  "} else {" · "}));}
            for width in (0..=1100).step_by(7) {
                let segments=footer_ui::fitted_hints(painter,&font,width as f32,hint,Color32::GRAY,Color32::WHITE);
                let displayed=segments.iter().map(|(text,_)|text.as_str()).collect::<String>();
                assert!(displayed.is_empty() || prefixes.contains(&displayed),"{displayed}");
                let measured=painter.layout_no_wrap(displayed,font.clone(),Color32::WHITE).size().x;assert!(measured<=width as f32,"{measured} > {width}");
            }
        }
    });});
}
#[test]
fn tooltip_style_matches_footer_without_changing_parent_style() {
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = ui.style().override_font_id.clone();
            let color = ui.visuals().override_text_color;
            ui.scope(|ui| {
                footer_ui::style_hints(ui);
                assert_eq!(ui.style().override_font_id, Some(FontId::monospace(12.0)));
                assert_eq!(
                    ui.visuals().override_text_color,
                    Some(Color32::from_rgb(136, 154, 176))
                );
                ui.label("Ln 1 · Col 1 · 1 lines · 0 words · 0 characters");
            });
            assert_eq!(ui.style().override_font_id, font);
            assert_eq!(ui.visuals().override_text_color, color);
        });
    });
}
struct Fixture {
    app: SlateApp,
    ctx: egui::Context,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, false, None);
        let root = std::env::temp_dir().join(format!(
            "slate-footer-{}-{}",
            std::process::id(),
            crate::test_unique()
        ));
        fs::create_dir_all(&root).unwrap();
        app.settings_path_override = Some(root.join("settings.toml"));
        app.footer_options = footer_ui::Options::default(); // Independent of the user's saved preferences.
        Self { app, ctx, root }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn draw(f: &mut Fixture, width: f32, mode: &str, events: Vec<egui::Event>) -> footer_ui::Geometry {
    let _ = f.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width, 300.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), 26.0),
                    egui::Sense::hover(),
                );
                f.app.compact_footer(ui, rect, mode);
            });
        },
    );
    f.ctx
        .data(|d| d.get_temp("compact_footer_geometry".into()))
        .unwrap()
}
#[test]
fn palette_launcher_is_right_aligned_bounded_and_hidden_whole_when_needed() {
    let ctx = egui::Context::default();
    for scale in [1.0, 1.5, 2.0] {
        ctx.set_pixels_per_point(scale);
        for width in [180.0, 260.0, 420.0, 900.0] {
            for visible in [false, true] {
                let row = egui::Rect::from_min_size(egui::pos2(0.0, 560.0), Vec2::new(width, 32.0));
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let (text, clicked) = footer_ui::palette_launcher(ui, row, visible);
                            assert!(!clicked);
                            assert!(row.contains_rect(text));
                            if text != row {
                                let button: egui::Rect = ctx
                                    .data(|data| {
                                        data.get_temp(egui::Id::new("palette_launcher_rect"))
                                    })
                                    .unwrap();
                                assert!(row.contains_rect(button));
                                assert!(text.right() < button.left());
                                assert_eq!(button.right(), row.right() - 10.0);
                            } else {
                                assert!(!visible || width == 180.0);
                            }
                        });
                    },
                );
            }
        }
    }
}
#[test]
fn footer_launcher_click_opens_palette_without_changing_document_or_command_text() {
    let mut f = Fixture::new();
    f.app.buffer.set_text("unsaved happy text".into());
    f.app.dirty = true;
    f.app.command_line = "open preserve this command".into();
    let revision = f.app.buffer.revision;
    let mut time = 0.0;
    let mut render = |f: &mut Fixture, events| {
        time += 0.1;
        let _ = f.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 600.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let row =
                        egui::Rect::from_min_size(egui::pos2(8.0, 560.0), Vec2::new(884.0, 32.0));
                    if footer_ui::palette_launcher(ui, row, true).1 {
                        f.app.open_command_palette();
                    }
                });
            },
        );
    };
    render(&mut f, vec![]);
    let rect: egui::Rect = f
        .ctx
        .data(|data| data.get_temp(egui::Id::new("palette_launcher_rect")))
        .unwrap();
    let event = |pressed| egui::Event::PointerButton {
        pos: rect.center(),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    render(
        &mut f,
        vec![egui::Event::PointerMoved(rect.center()), event(true)],
    );
    render(&mut f, vec![event(false)]);
    assert!(f.app.palette_open);
    assert_eq!(f.app.buffer.as_str(), "unsaved happy text");
    assert_eq!(f.app.buffer.revision, revision);
    assert!(f.app.dirty);
    assert_eq!(f.app.command_line, "open preserve this command");
}
#[test]
fn control_p_uses_the_same_palette_launcher_and_preserves_typed_command() {
    let mut f = Fixture::new();
    f.app.command_line = "preserve me".into();
    f.app.command_line_focused = true;
    let _ = f.ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::P,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
            ..Default::default()
        },
        |ctx| f.app.shortcuts(ctx),
    );
    assert!(f.app.palette_open);
    assert!(!f.app.command_line_focused);
    assert_eq!(f.app.command_line, "preserve me");
}
#[test]
fn footer_never_overlaps_with_long_path_messages_or_all_fields_enabled() {
    let mut f = Fixture::new();
    f.app.path = Some(PathBuf::from(format!(
        "D:/a/very/long/path/{}/large-document-5m - Copy.md",
        "directory/".repeat(30)
    )));
    f.app.status = "Opened a really long path and generated extra status information ".repeat(20);
    f.app.buffer.set_text("large document text\n".repeat(1000));
    f.app.footer_options.enabled = [true; 10];
    for width in [180.0, 260.0, 420.0, 900.0, 1600.0] {
        let geo = draw(&mut f, width, "preview", vec![]);
        assert!(geo.frame.contains_rect(geo.left));
        for (index, rect) in geo.right.iter().enumerate() {
            assert!(geo.frame.contains_rect(*rect));
            assert!(rect.left() >= geo.left.right());
            for other in &geo.right[index + 1..] {
                assert!(!rect.intersects(*other));
            }
        }
    }
    f.app.footer_options.enabled = [false; 10];
    let geo = draw(&mut f, 420.0, "edit", vec![]);
    assert!(geo.right.is_empty());
}
#[test]
fn footer_preferences_roundtrip_and_old_config_preserves_compact_defaults() {
    let mut f = Fixture::new();
    let file = f.app.settings_path_override.clone().unwrap();
    fs::write(&file, "word_wrap = true\n").unwrap();
    f.app.load_settings();
    assert_eq!(f.app.footer_options, footer_ui::Options::default());
    for index in 0..footer_ui::FIELDS.len() {
        f.app.set_footer_option(index, index % 2 == 0);
    }
    let expected = f.app.footer_options.clone();
    f.app.footer_options = footer_ui::Options::default();
    f.app.load_settings();
    assert_eq!(f.app.footer_options, expected);
    let saved = fs::read_to_string(file).unwrap();
    for (key, _) in footer_ui::FIELDS {
        assert!(saved.contains(&format!("{key} = ")));
    }
}
#[test]
fn default_footer_does_not_recount_large_document_each_frame() {
    let mut f = Fixture::new();
    f.app.buffer.set_text(
        "café words
"
        .repeat(100_000),
    );
    draw(&mut f, 900.0, "edit", vec![]);
    assert!(f.app.footer_counts.revision().is_none());
    f.app.footer_options.enabled[6] = true;
    draw(&mut f, 900.0, "edit", vec![]);
    let revision = f.app.footer_counts.revision();
    assert_eq!(f.app.footer_counts(), (200_000, 1_100_000));
    draw(&mut f, 900.0, "edit", vec![]);
    assert_eq!(f.app.footer_counts.revision(), revision);
    f.app.buffer.insert_text("new ");
    draw(&mut f, 900.0, "edit", vec![]);
    assert_ne!(f.app.footer_counts.revision(), revision);
}
#[test]
fn word_and_character_counts_follow_edits_like_a_full_recount() {
    let mut f = Fixture::new();
    f.app.buffer.set_text(
        "one two

three café
  four  "
            .into(),
    );
    let full = |text: &str| (text.split_whitespace().count(), text.chars().count());
    assert_eq!(f.app.footer_counts(), full(f.app.buffer.as_str()));
    f.app.buffer.set_cursor(4);
    f.app.buffer.insert_text(
        "big
new lines ",
    );
    assert_eq!(f.app.footer_counts(), full(f.app.buffer.as_str()));
    f.app.buffer.set_selection(0, 12);
    f.app.buffer.backspace();
    assert_eq!(f.app.footer_counts(), full(f.app.buffer.as_str()));
    f.app.buffer.undo();
    f.app.buffer.undo();
    assert_eq!(f.app.footer_counts(), full(f.app.buffer.as_str()));
    f.app.buffer.set_text("fresh text".into());
    assert_eq!(f.app.footer_counts(), (2, 10));
}
#[test]
fn context_and_message_expiry_do_not_change_command_buffer_or_document() {
    let mut f = Fixture::new();
    f.app.buffer.set_text("document".into());
    f.app.command_line = "open some path".into();
    f.app.status = "Saved document".into();
    draw(&mut f, 900.0, "command", vec![]);
    f.app.footer_status_since = Some(std::time::Instant::now() - std::time::Duration::from_secs(5));
    draw(&mut f, 900.0, "edit", vec![]);
    assert!(f.app.footer_status_since.unwrap().elapsed().as_secs() >= 5);
    assert_eq!(f.app.buffer.as_str(), "document");
    assert_eq!(f.app.command_line, "open some path");
}
#[test]
fn keyboard_toggles_footer_settings_and_last_row_is_reachable() {
    let mut f = Fixture::new();
    f.app.settings_open = true;
    f.app.selected_setting = 9;
    let key = |key| egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = f.ctx.run(
        egui::RawInput {
            events: vec![key(Key::Enter)],
            ..Default::default()
        },
        |ctx| f.app.shortcuts(ctx),
    );
    assert!(!f.app.footer_options.enabled[0]);
    f.app.selected_setting = 17;
    let _ = f.ctx.run(
        egui::RawInput {
            events: vec![key(Key::ArrowDown)],
            ..Default::default()
        },
        |ctx| f.app.shortcuts(ctx),
    );
    assert_eq!(f.app.selected_setting, 18);
}
#[test]
fn last_footer_checkbox_is_visible_in_small_settings_dialog() {
    let mut f = Fixture::new();
    f.app.settings_open = true;
    f.app.selected_setting = 18;
    for pass in 0..15 {
        let _ = f.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(420.0, 260.0),
                )),
                time: Some(pass as f64 * 0.05),
                ..Default::default()
            },
            |ctx| f.app.settings_dialog(ctx),
        );
    }
    let geo = f
        .ctx
        .data(|d| d.get_temp::<settings_tests::SettingsDialogGeometry>("settings_geometry".into()))
        .unwrap();
    assert_eq!(geo.rows.len(), 32);
    assert!(geo.body.contains_rect(geo.rows[18]), "{geo:?}");
    assert!(geo.frame.contains_rect(geo.footer));
}
