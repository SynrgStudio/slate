#![allow(deprecated)]
use super::*;
#[test]
fn result_dialog_list_is_contained_at_small_and_screenshot_sizes_and_wheel_preserves_text() {
    for size in [
        Vec2::new(420.0, 260.0),
        Vec2::new(900.0, 600.0),
        Vec2::new(1200.0, 800.0),
    ] {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = SlateApp::new(&cc, None, true, false, None);
        app.buffer.set_text("unsaved text must remain".into());
        app.dirty = true;
        app.periscope_open = true;
        app.periscope_mode = PeriscopeMode::Global;
        app.periscope_query = "entry".into();
        app.periscope_backend_status = "Very long backend status ".repeat(20);
        app.periscope_results = (0..100)
            .map(|index| PathBuf::from(format!("C:/folder/entry-{index:03}.cmd")))
            .collect();
        app.selected_periscope_result = 99;
        for pass in 0..4 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(pass as f64 * 0.1),
                    ..Default::default()
                },
                |ctx| app.periscope_dialog(ctx),
            );
        }
        let geo: dialog_ui::Geometry = ctx
            .data(|data| data.get_temp(egui::Id::new("periscope_dialog").with("geometry")))
            .unwrap();
        let list: egui::Rect = ctx
            .data(|data| data.get_temp(egui::Id::new("periscope_list_rect")))
            .unwrap();
        assert!(
            geo.body.contains_rect(list),
            "size={size:?} list={list:?} body={:?}",
            geo.body
        );
        assert!(list.bottom() < geo.footer.top());
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(1.0),
                events: vec![
                    egui::Event::PointerMoved(list.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        phase: egui::TouchPhase::Move,
                        delta: Vec2::new(0.0, 48.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| app.periscope_dialog(ctx),
        );
        assert_eq!(app.selected_periscope_result, 98);
        assert_eq!(app.buffer.as_str(), "unsaved text must remain");
        assert!(app.dirty);
    }
}
