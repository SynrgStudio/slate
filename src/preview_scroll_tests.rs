#![allow(deprecated)]
use super::*;
fn draw(app: &mut SlateApp, ctx: &egui::Context, pass: usize, pointer: egui::Pos2, wheel: bool) {
    let mut events = vec![egui::Event::PointerMoved(pointer)];
    if wheel {
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0.0, -120.0),
            modifiers: egui::Modifiers::NONE,
        });
    }
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 560.0),
            )),
            time: Some(pass as f64 * 0.02),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.columns(2, |columns| {
                    app.prepare_preview_wheel_sync(&columns[1]);
                    app.editor_view.render(
                        &mut columns[0],
                        &mut app.buffer,
                        true,
                        None,
                        editor_view::LineNumberMode::Absolute,
                        false,
                        true,
                        None,
                        false,
                        None,
                    );
                    columns[1].vertical(|ui| app.preview_ui(ui));
                });
            });
        },
    );
}
fn fixture(ctx: &egui::Context) -> SlateApp {
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, None);
    let mut text = String::from("# Heading\n\n| Column | Value |\n| --- | --- |\n");
    for n in 0..35 {
        text.push_str(&format!("| row {n} | table value |\n"));
    }
    text.push_str("\n");
    text.push_str(
        &"A long paragraph café 雪 with different wrapping in editor and preview. ".repeat(10),
    );
    text.push('\n');
    text.push_str(&"plain text\n".repeat(70));
    app.buffer.set_text(text);
    app
}
#[test]
fn scrolling_either_pane_matches_content_position_with_unequal_table_heights() {
    for from_preview in [false, true] {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        let pointer = egui::pos2(if from_preview { 700.0 } else { 150.0 }, 200.0);
        for pass in 0..20 {
            draw(&mut app, &ctx, pass, pointer, pass == 5);
        }
        let editor = app.editor_view.scroll_position();
        let preview = app.preview_sync.position_for(app.preview_sync.offset);
        assert!(editor.source > 0.0);
        assert!(app.preview_sync.offset > 0.0);
        assert!(
            (editor.source - preview.source).abs() < 0.1,
            "driver={from_preview} editor={editor:?} preview={preview:?}"
        );
        let points = &app.preview_sync.points;
        assert!(
            points
                .windows(2)
                .all(|p| p[0].0 <= p[1].0 && p[0].1 <= p[1].1)
        );
        // Table separator has zero preview height; body rows use the actual 26pt table geometry.
        let top = scroll_sync::interpolate(points, 4.0, false);
        let bottom = scroll_sync::interpolate(points, 5.0, false);
        assert!((bottom - top - 26.0).abs() < 1.0);
    }
}
#[test]
fn endpoints_resize_and_edit_keep_sync_finite_without_feedback_drift() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    let pointer = egui::pos2(150.0, 200.0);
    for pass in 0..6 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    app.editor_view.set_scroll_position(scroll_sync::Position {
        source: 0.0,
        at_end: true,
    });
    for pass in 6..12 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    assert!(
        app.preview_sync
            .position_for(app.preview_sync.offset)
            .at_end
    );
    assert!(app.editor_view.scroll_position().at_end);
    app.buffer.move_to_top();
    app.buffer.insert_text("new line\n");
    ctx.set_pixels_per_point(1.5);
    for pass in 12..22 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    let offset = app.preview_sync.offset;
    for pass in 22..32 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    assert!(app.preview_sync.offset.is_finite());
    assert!((offset - app.preview_sync.offset).abs() < 0.1);
    assert_eq!(app.preview_sync.key.unwrap().1, app.buffer.revision);
    app.editor_view.set_scroll_position(Default::default());
    for pass in 32..40 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    assert!(app.preview_sync.offset < 0.5);
}
#[test]
fn large_preview_materializes_visible_lines_and_preserves_bilateral_anchors_after_resize() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    app.buffer.set_text(
        "Wrapped paragraph café with many words repeated for viewport layout. "
            .repeat(5)
            .lines()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
            .repeat(3000),
    );
    let pointer = egui::pos2(150.0, 200.0);
    for pass in 0..5 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    let (rendered, skipped) = ctx
        .data(|d| d.get_temp::<(usize, usize)>("preview_layout_counts".into()))
        .unwrap();
    assert!(rendered < 50, "{rendered}");
    assert!(skipped > 2900);
    app.editor_view.set_scroll_position(scroll_sync::Position {
        source: 1500.4,
        at_end: false,
    });
    for pass in 5..15 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    let a = app.editor_view.scroll_position();
    let b = app.preview_sync.position_for(app.preview_sync.offset);
    assert!((a.source - b.source).abs() < 0.15, "{a:?} {b:?}");
    ctx.set_pixels_per_point(1.5);
    for pass in 15..30 {
        draw(&mut app, &ctx, pass, pointer, false);
    }
    let a = app.editor_view.scroll_position();
    let b = app.preview_sync.position_for(app.preview_sync.offset);
    assert!((a.source - b.source).abs() < 0.15, "{a:?} {b:?}");
    let (rendered, skipped) = ctx
        .data(|d| d.get_temp::<(usize, usize)>("preview_layout_counts".into()))
        .unwrap();
    assert!(rendered < 50);
    assert!(skipped > 2900);
}
#[test]
fn fenced_code_in_the_preview_uses_the_editor_colours() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    app.apply_editor_look(editor_view::EditorLook::default());
    app.buffer
        .set_text("# Notes\n\n```rust\nfn main() { let x = 1; }\n```\nplain text\n".into());
    let colours_drawn = |ctx: &egui::Context, app: &mut SlateApp| {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 560.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.preview_ui(ui));
            },
        );
        let mut colours = Vec::new();
        for clipped in output.shapes {
            if let egui::Shape::Text(text) = clipped.shape {
                for section in &text.galley.job.sections {
                    colours.push((
                        text.galley.job.text[section.byte_range.clone()].to_string(),
                        section.format.color,
                    ));
                }
            }
        }
        colours
    };
    let colours = colours_drawn(&ctx, &mut app);
    let keyword = highlight::Token::Keyword.color();
    assert!(
        colours
            .iter()
            .any(|(text, colour)| text == "fn" && *colour == keyword),
        "{colours:?}"
    );
    assert!(colours.iter().any(|(text, _)| text.contains("plain text")));
    app.apply_editor_look(editor_view::EditorLook {
        syntax_highlighting: false,
        ..Default::default()
    });
    let colours = colours_drawn(&ctx, &mut app);
    assert!(!colours.iter().any(|(_, colour)| *colour == keyword));
}
