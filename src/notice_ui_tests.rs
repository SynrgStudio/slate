#![allow(deprecated)]
use super::*;
use crate::{
    PendingAction, SlateApp,
    document_safety::{Draft, Recovered, Slot},
};
fn app(ctx: &egui::Context) -> SlateApp {
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    SlateApp::new(&cc, None, true, None)
}
fn records(app: &mut SlateApp) {
    app.recovery_open = true;
    app.recovery_records = (0..60)
        .map(|n| Recovered {
            record: format!("unused/{n}/main.json").into(),
            draft: Draft {
                version: 1,
                slot: Slot::Main,
                path: None,
                base: None,
                text: format!("draft {n}"),
                cursor: 0,
                saved_ms: n,
            },
        })
        .collect();
}
fn draw(
    app: &mut SlateApp,
    ctx: &egui::Context,
    size: Vec2,
    id: &str,
    events: Vec<egui::Event>,
) -> Geometry {
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            if id == "confirm_close_prompt" {
                app.confirm_action_dialog(ctx);
            } else {
                app.document_safety_dialog(ctx);
            }
        },
    );
    for shape in output.shapes {
        if let egui::epaint::Shape::Text(text) = shape.shape {
            for section in &text.galley.job.sections {
                assert_eq!(section.format.font_id.family, FontFamily::Monospace);
            }
        }
    }
    ctx.data(|d| d.get_temp(egui::Id::new(id).with("geometry")))
        .unwrap()
}
#[test]
fn safety_notices_are_centered_eighty_percent_with_contained_fixed_chrome() {
    for size in [
        Vec2::new(360.0, 240.0),
        Vec2::new(420.0, 260.0),
        Vec2::new(900.0, 560.0),
        Vec2::new(1200.0, 800.0),
    ] {
        for id in [
            "recovery_dialog",
            "confirm_close_prompt",
            "external_change_dialog",
        ] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            match id {
                "recovery_dialog" => records(&mut app),
                "confirm_close_prompt" => {
                    app.dirty = true;
                    app.pending_action = Some(PendingAction::Quit);
                }
                _ => {
                    app.file_conflict = Some((Slot::Main, "unused/source.txt".into()));
                }
            }
            let mut geo = draw(&mut app, &ctx, size, id, vec![]);
            for _ in 0..5 {
                geo = draw(&mut app, &ctx, size, id, vec![]);
            }
            assert!(
                (geo.frame.width() - size.x * 0.8).abs() < 1.0,
                "{id}: {geo:?}"
            );
            assert!(
                (geo.frame.height() - (size.y - 30.0) * 0.8).abs() < 1.0,
                "{id}: {geo:?}"
            );
            assert!(
                (geo.frame.center().x - size.x * 0.5).abs() < 1.0,
                "{id}: {geo:?}"
            );
            assert!(
                (geo.frame.center().y - (size.y * 0.5 + 15.0)).abs() < 1.0,
                "{id}: {geo:?}"
            );
            assert!(geo.frame.contains_rect(geo.header));
            assert!(geo.frame.contains_rect(geo.footer));
            assert!(geo.body.top() >= geo.header.bottom());
            assert!(geo.body.bottom() <= geo.footer.top());
            assert!(geo.body.height() > 0.0);
            for rect in geo.actions {
                assert!(geo.footer.contains_rect(rect), "{id}: {rect:?}");
            }
        }
    }
}
#[test]
fn recovery_selection_scrolls_without_moving_header_or_footer() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    records(&mut app);
    let size = Vec2::new(900.0, 560.0);
    let mut before = draw(&mut app, &ctx, size, "recovery_dialog", vec![]);
    for _ in 0..5 {
        before = draw(&mut app, &ctx, size, "recovery_dialog", vec![]);
    }
    app.recovery_selected = 59;
    let mut after = before.clone();
    for _ in 0..15 {
        after = draw(&mut app, &ctx, size, "recovery_dialog", vec![]);
    }
    assert_eq!(before.frame, after.frame);
    assert_eq!(before.header, after.header);
    assert_eq!(before.footer, after.footer);
    let selected = ctx
        .data(|d| d.get_temp::<Rect>("recovery_selected_rect".into()))
        .unwrap();
    assert!(
        after.body.contains_rect(selected),
        "{after:?} selected={selected:?}"
    );
    assert!(selected.width() > after.body.width() * 0.9);
}
#[test]
fn return_footer_click_keeps_dirty_text_and_cancels_quit() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.buffer.set_text("keep unsaved text".into());
    app.dirty = true;
    app.pending_action = Some(PendingAction::Quit);
    let size = Vec2::new(900.0, 560.0);
    let mut geo = draw(&mut app, &ctx, size, "confirm_close_prompt", vec![]);
    for _ in 0..5 {
        geo = draw(&mut app, &ctx, size, "confirm_close_prompt", vec![]);
    }
    let pos = geo.actions[1].center();
    for pressed in [true, false] {
        draw(
            &mut app,
            &ctx,
            size,
            "confirm_close_prompt",
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.pending_action.is_none());
    assert!(app.dirty);
    assert_eq!(app.buffer.as_str(), "keep unsaved text");
}
