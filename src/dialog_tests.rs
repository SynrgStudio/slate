#![allow(deprecated)]
use super::*;
fn fixture(ctx: &egui::Context) -> SlateApp {
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, None);
    app.apply_editor_font_size(editor_view::DEFAULT_EDITOR_FONT_SIZE);
    app.buffer.set_text("untouched main text".into());
    app.scratch_buffer.set_text("scratch text\n".repeat(300));
    app.link_preview_buffer.set_text("modal text\n".repeat(300));
    app.link_preview_dirty = true;
    app.capture_text = "captured text\n".repeat(1000);
    app.capture_title = "keep this title".into();
    app.link_preview_path = Some(PathBuf::from(format!(
        "D:/{}document.md",
        "long directory/".repeat(50)
    )));
    app
}
fn geometry(ctx: &egui::Context, id: &str) -> dialog_ui::Geometry {
    ctx.data(|d| d.get_temp(egui::Id::new(id).with("geometry")))
        .unwrap()
}
fn draw(
    app: &mut SlateApp,
    ctx: &egui::Context,
    size: Vec2,
    id: &str,
    time: f64,
    events: Vec<egui::Event>,
) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ctx| match id {
            "periscope_prompt_dialog" => app.periscope_prompt_dialog(ctx),
            "periscope_dialog" => app.periscope_dialog(ctx),
            "find_note_dialog" => app.find_note_dialog(ctx),
            "capture_dialog" => app.capture_dialog(ctx),
            "scratch_modal_dialog" => app.scratch_modal_dialog(ctx),
            "link_preview_dialog" => app.link_preview_dialog(ctx),
            _ => unreachable!(),
        },
    );
}
fn open(app: &mut SlateApp, id: &str) {
    match id {
        "periscope_prompt_dialog" => app.periscope_prompt_open = true,
        "periscope_dialog" => {
            app.periscope_open = true;
            app.periscope_mode = PeriscopeMode::Global;
            app.periscope_query = "entry".into();
            app.periscope_results = (0..100)
                .map(|index| PathBuf::from(format!("C:/folder/entry-{index:03}.cmd")))
                .collect();
        }
        "find_note_dialog" => app.find_note_open = true,
        "capture_dialog" => app.capture_modal_open = true,
        "scratch_modal_dialog" => app.scratch_modal_open = true,
        "link_preview_dialog" => app.link_preview_open = true,
        _ => unreachable!(),
    }
}
#[test]
fn forms_are_eighty_percent_with_contained_stationary_header_body_footer_and_actions() {
    for size in [
        Vec2::new(420.0, 260.0),
        Vec2::new(900.0, 560.0),
        Vec2::new(1200.0, 800.0),
    ] {
        for id in [
            "periscope_prompt_dialog",
            "periscope_dialog",
            "find_note_dialog",
            "capture_dialog",
            "scratch_modal_dialog",
            "link_preview_dialog",
        ] {
            let ctx = egui::Context::default();
            let mut app = fixture(&ctx);
            open(&mut app, id);
            for pass in 0..4 {
                draw(&mut app, &ctx, size, id, pass as f64 * 0.05, vec![]);
            }
            let geo = geometry(&ctx, id);
            assert!(
                (geo.frame.width() - size.x * 0.8).abs() < 1.0,
                "{id} {geo:?}"
            );
            assert!(
                (geo.frame.height() - (size.y - 30.0) * 0.8).abs() < 1.0,
                "{id} {geo:?}"
            );
            assert!(
                (geo.frame.center() - egui::pos2(size.x * 0.5, size.y * 0.5 + 15.0)).length() < 1.0,
                "{id} not centered: {geo:?}"
            );
            assert!(geo.frame.contains_rect(geo.header));
            assert!(geo.frame.contains_rect(geo.body));
            assert!(geo.frame.contains_rect(geo.footer));
            assert!(geo.header.bottom() < geo.body.top());
            assert!(geo.body.bottom() < geo.footer.top());
            for (index, rect) in geo.actions.iter().enumerate() {
                assert!(geo.footer.contains_rect(*rect));
                for other in &geo.actions[index + 1..] {
                    assert!(!rect.intersects(*other));
                }
            }
            draw(
                &mut app,
                &ctx,
                size,
                id,
                0.3,
                vec![
                    egui::Event::PointerMoved(geo.body.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        phase: egui::TouchPhase::Move,
                        delta: Vec2::new(0.0, -200.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            let after = geometry(&ctx, id);
            assert_eq!(geo.frame, after.frame);
            assert_eq!(geo.header, after.header);
            assert_eq!(geo.footer, after.footer);
            assert_eq!(app.buffer.as_str(), "untouched main text");
            assert_eq!(app.scratch_buffer.as_str(), "scratch text\n".repeat(300));
            assert_eq!(app.link_preview_buffer.as_str(), "modal text\n".repeat(300));
            assert_eq!(app.capture_title, "keep this title");
            assert!(app.link_preview_dirty);
        }
    }
}
#[test]
fn forms_follow_available_window_center_after_resizing() {
    for id in [
        "capture_dialog",
        "scratch_modal_dialog",
        "link_preview_dialog",
        "periscope_prompt_dialog",
    ] {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        open(&mut app, id);
        for (step, size) in [
            Vec2::new(900.0, 560.0),
            Vec2::new(420.0, 260.0),
            Vec2::new(1200.0, 800.0),
        ]
        .into_iter()
        .enumerate()
        {
            for pass in 0..4 {
                draw(
                    &mut app,
                    &ctx,
                    size,
                    id,
                    (step * 4 + pass) as f64 * 0.05,
                    vec![],
                );
            }
            let geo = geometry(&ctx, id);
            assert!(
                (geo.frame.center() - egui::pos2(size.x * 0.5, size.y * 0.5 + 15.0)).length() < 1.0,
                "{id} not centered after resize: {geo:?}"
            );
            assert!((geo.frame.width() - size.x * 0.8).abs() < 1.0);
            assert!((geo.frame.height() - (size.y - 30.0) * 0.8).abs() < 1.0);
        }
    }
}
#[test]
fn clicking_modal_close_uses_dirty_guard_not_forced_discard() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    app.link_preview_open = true;
    let size = Vec2::new(900.0, 560.0);
    for pass in 0..3 {
        draw(
            &mut app,
            &ctx,
            size,
            "link_preview_dialog",
            pass as f64 * 0.05,
            vec![],
        );
    }
    let pos = geometry(&ctx, "link_preview_dialog").actions[3].center();
    draw(
        &mut app,
        &ctx,
        size,
        "link_preview_dialog",
        0.2,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(app.link_preview_open);
    assert!(app.link_preview_dirty);
    assert_eq!(app.status, "Save modal buffer before closing");
    assert_eq!(app.link_preview_buffer.as_str(), "modal text\n".repeat(300));
}
#[test]
fn editor_body_scrolls_and_scratch_hide_preserves_unsaved_text() {
    for id in ["scratch_modal_dialog", "link_preview_dialog"] {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        open(&mut app, id);
        let size = Vec2::new(900.0, 560.0);
        for pass in 0..4 {
            draw(&mut app, &ctx, size, id, pass as f64 * 0.05, vec![]);
        }
        let before = if id == "scratch_modal_dialog" {
            app.scratch_view.scroll_position()
        } else {
            app.link_preview_view.scroll_position()
        };
        let geo = geometry(&ctx, id);
        draw(
            &mut app,
            &ctx,
            size,
            id,
            0.3,
            vec![
                egui::Event::PointerMoved(geo.body.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -200.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let after = if id == "scratch_modal_dialog" {
            app.scratch_view.scroll_position()
        } else {
            app.link_preview_view.scroll_position()
        };
        assert!(after.source > before.source, "{id} {before:?} {after:?}");
        if id == "scratch_modal_dialog" {
            let pos = geo.actions[2].center();
            draw(
                &mut app,
                &ctx,
                size,
                id,
                0.4,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            assert!(!app.scratch_modal_open);
            assert_eq!(app.scratch_buffer.as_str(), "scratch text\n".repeat(300));
            assert_eq!(app.buffer.as_str(), "untouched main text");
        }
    }
}
#[test]
fn capture_initial_focus_accepts_typing_without_changing_captured_text() {
    let ctx = egui::Context::default();
    let mut app = fixture(&ctx);
    app.capture_modal_open = true;
    app.capture_title.clear();
    app.capture_title_focus_once = true;
    let before = app.capture_text.clone();
    let size = Vec2::new(900.0, 560.0);
    draw(&mut app, &ctx, size, "capture_dialog", 0.0, vec![]);
    draw(
        &mut app,
        &ctx,
        size,
        "capture_dialog",
        0.05,
        vec![egui::Event::Text("new title".into())],
    );
    assert_eq!(app.capture_title, "new title");
    assert_eq!(app.capture_text, before);
}
