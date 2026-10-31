#![allow(deprecated)]
use super::*;
#[test]
#[ignore = "manual release CPU baseline; reads the generated fixture, never writes source files"]
fn benchmark_large_document_editor() {
    let text = fs::read_to_string("D:/slate-baselines/large-document-5m.md").unwrap();
    assert_eq!(text.chars().count(), 5_000_000);
    let mut buffer = EditorBuffer::from_text(text);
    let mut view = EditorView::new();
    let ctx = egui::Context::default();
    for stage in [
        "first",
        "scroll",
        "end",
        "idle_end",
        "edit",
        "undo",
        "edit_middle",
        "undo_middle",
        "edit_start",
        "undo_start",
        "paste_middle",
        "undo_paste",
    ] {
        if stage == "end" {
            buffer.move_to_bottom();
            view.request_scroll_to_cursor(&buffer);
        }
        let events = if stage == "scroll" {
            vec![
                egui::Event::PointerMoved(egui::pos2(200.0, 200.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -120.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        } else {
            vec![]
        };
        let started = std::time::Instant::now();
        if stage == "edit" {
            buffer.insert_text("x");
        }
        if stage == "edit_middle" || stage == "paste_middle" {
            let byte = buffer
                .as_str()
                .find("SLATE_BASELINE_NEEDLE_MIDDLE")
                .unwrap();
            buffer.set_cursor(byte);
            buffer.insert_text(if stage == "paste_middle" {
                "pasted café\n雪\n\nnew line\n"
            } else {
                "x"
            });
            view.request_scroll_to_cursor(&buffer);
        }
        if stage == "edit_start" {
            buffer.move_to_top();
            buffer.insert_text("x");
            view.request_scroll_to_cursor(&buffer);
        }
        if stage.starts_with("undo") {
            assert!(buffer.undo());
            view.request_scroll_to_cursor(&buffer);
        }
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 560.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.render(
                        ui,
                        &mut buffer,
                        true,
                        None,
                        editor_view::LineNumberMode::Absolute,
                        false,
                        false,
                        None,
                        true,
                        None,
                    );
                });
            },
        );
        println!(
            "EDITOR_BASELINE stage={stage} cpu_ms={:.3}",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
}
