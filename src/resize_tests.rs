#![allow(deprecated)]
use super::*;
#[test]
#[ignore = "manual release resize baseline; reads owned fixture, does not write documents"]
fn benchmark_large_preview_resize() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = SlateApp::new(&cc, None, true, false, None);
    app.buffer
        .set_text(fs::read_to_string("D:/slate-baselines/large-document-5m.md").unwrap());
    for (pass, width) in [900.0, 900.0, 880.0, 860.0, 840.0, 820.0, 800.0, 800.0]
        .into_iter()
        .enumerate()
    {
        let start = std::time::Instant::now();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 560.0),
                )),
                time: Some(pass as f64 * 0.05),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.columns(2, |cols| {
                        app.editor_view.render(
                            &mut cols[0],
                            &mut app.buffer,
                            true,
                            None,
                            editor_view::LineNumberMode::Absolute,
                            false,
                            false,
                            None,
                            true,
                            None,
                        );
                        cols[1].vertical(|ui| app.preview_ui(ui));
                    });
                });
            },
        );
        println!(
            "RESIZE_BASELINE width={width} cpu_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
