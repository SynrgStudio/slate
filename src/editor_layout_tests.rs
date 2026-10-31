#![allow(deprecated)]
use super::*;
fn frame(ctx: &egui::Context, run: impl FnOnce(&egui::Painter)) {
    let mut run = Some(run);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            run.take().unwrap()(ui.painter());
        });
    });
}
#[test]
fn cached_rows_reuse_scroll_and_cursor_but_not_different_documents_or_edits() {
    let ctx = egui::Context::default();
    let view = EditorView::new();
    let font = FontId::monospace(12.0);
    let mut a = EditorBuffer::from_text("abcdef\nsecond line".into());
    let b = EditorBuffer::from_text("different\nnew document".into());
    assert_eq!(a.revision, b.revision);
    frame(&ctx, |p| {
        let first = view.cached_visual_rows(p, &a, &font, 40.0, true);
        a.set_cursor(3);
        let moved = view.cached_visual_rows(p, &a, &font, 40.0, true);
        assert!(Arc::ptr_eq(&first, &moved));
        let other = view.cached_visual_rows(p, &b, &font, 40.0, true);
        assert!(!Arc::ptr_eq(&first, &other));
        let original = view.cached_visual_rows(p, &a, &font, 40.0, true);
        a.insert_text("雪");
        let edited = view.cached_visual_rows(p, &a, &font, 40.0, true);
        assert!(!Arc::ptr_eq(&original, &edited));
        a.undo();
        let undone = view.cached_visual_rows(p, &a, &font, 40.0, true);
        assert!(!Arc::ptr_eq(&edited, &undone));
        assert_eq!(a.as_str(), "abcdef\nsecond line");
    });
}
#[test]
fn row_cache_invalidates_width_font_scale_and_wrapping() {
    let ctx = egui::Context::default();
    let view = EditorView::new();
    let buffer = EditorBuffer::from_text("0123456789".repeat(20));
    let font = FontId::monospace(12.0);
    let mut before = None;
    frame(&ctx, |p| {
        let a = view.cached_visual_rows(p, &buffer, &font, 80.0, true);
        let b = view.cached_visual_rows(p, &buffer, &font, 120.0, true);
        assert!(!Arc::ptr_eq(&a, &b));
        assert!(a.len() > b.len());
        let c = view.cached_visual_rows(p, &buffer, &FontId::monospace(24.0), 120.0, true);
        assert!(!Arc::ptr_eq(&b, &c));
        assert!(c.len() > b.len());
        let d = view.cached_visual_rows(p, &buffer, &font, 120.0, false);
        assert_eq!(d.len(), 1);
        assert!(!Arc::ptr_eq(&c, &d));
        before = Some(d);
    });
    ctx.set_pixels_per_point(1.5);
    frame(&ctx, |p| {
        let after = view.cached_visual_rows(p, &buffer, &font, 120.0, false);
        assert!(!Arc::ptr_eq(before.as_ref().unwrap(), &after));
    });
}
#[test]
fn wrapping_preserves_every_utf8_byte_empty_line_and_long_line() {
    let ctx = egui::Context::default();
    let view = EditorView::new();
    let buffer =
        EditorBuffer::from_text(format!("café 雪 λ\t{}\n\nend", "wide → text ".repeat(1000)));
    let font = FontId::monospace(12.0);
    frame(&ctx, |p| {
        for width in [1.0, 40.0, 600.0] {
            let rows = view.visual_rows(p, &buffer, &font, width, true);
            for index in 0..buffer.line_count() {
                let mut reconstructed = String::new();
                for row in rows.iter().filter(|row| row.line_index == index) {
                    assert!(buffer.as_str().is_char_boundary(row.start));
                    assert!(buffer.as_str().is_char_boundary(row.end));
                    reconstructed.push_str(&buffer.as_str()[row.start..row.end]);
                }
                assert_eq!(reconstructed, buffer.line(index));
            }
            assert!(rows.iter().any(|row| row.start == row.end));
        }
    });
}
#[test]
fn monospace_wrap_width_agrees_with_painted_text() {
    let ctx = egui::Context::default();
    let view = EditorView::new();
    let buffer =
        EditorBuffer::from_text("ASCII café λ 雪\twords and numbers 1234567890 ".repeat(20));
    frame(&ctx, |p| {
        for size in [8.0, 12.0, 24.0, 32.0] {
            let font = FontId::monospace(size);
            for width in [40.0, 137.0, 600.0] {
                for row in view.visual_rows(p, &buffer, &font, width, true) {
                    let slice = &buffer.as_str()[row.start..row.end];
                    let actual = view.text_width(p, slice, &font);
                    assert!(
                        actual <= width + 1.0 || slice.chars().count() == 1,
                        "width={width} measured={actual} text={slice:?}"
                    );
                }
            }
        }
    });
}
#[test]
fn fence_cache_tracks_edit_undo_and_document_identity() {
    let view = EditorView::new();
    let mut a = EditorBuffer::from_text("```rust\ncode\n```\nplain".into());
    assert_eq!(view.code_block_range(&a, 1), Some((0, 2)));
    assert!(view.code_line_kind(&a, 3).is_none());
    let b = EditorBuffer::from_text("ordinary\ntext".into());
    assert_eq!(view.code_block_range(&b, 1), None);
    a.set_text("plain\n```\nopen code".into());
    assert_eq!(view.code_block_range(&a, 2), Some((1, 2)));
    assert!(matches!(
        view.code_line_kind(&a, 2),
        Some(CodeLineKind::Body)
    ));
    a.move_to_bottom();
    a.insert_text("\n```\nplain");
    assert_eq!(view.code_block_range(&a, 4), None);
    a.undo();
    assert_eq!(view.code_block_range(&a, 2), Some((1, 2)));
}
#[test]
fn wheel_scroll_only_moves_hovered_pane_even_when_editor_has_keyboard_focus() {
    for pointer_in_editor in [false, true] {
        let ctx = egui::Context::default();
        let mut view = EditorView::new();
        let mut buffer = EditorBuffer::from_text("editor line\n".repeat(300));
        let mut preview_y = 0.0;
        for pass in 0..12 {
            let pos = egui::pos2(if pointer_in_editor { 150.0 } else { 700.0 }, 200.0);
            let mut events = vec![egui::Event::PointerMoved(pos)];
            if pass == 4 {
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
                        egui::vec2(900.0, 560.0),
                    )),
                    time: Some(pass as f64 * 0.02),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.columns(2, |columns| {
                            view.render(
                                &mut columns[0],
                                &mut buffer,
                                true,
                                None,
                                LineNumberMode::Absolute,
                                false,
                                true,
                                None,
                                false,
                                None,
                            );
                            let output = egui::ScrollArea::vertical()
                                .id_salt("preview_wheel_regression")
                                .show(&mut columns[1], |ui| {
                                    ui.allocate_exact_size(
                                        egui::vec2(200.0, 4000.0),
                                        egui::Sense::hover(),
                                    );
                                });
                            preview_y = output.state.offset.y;
                        });
                    });
                },
            );
        }
        if pointer_in_editor {
            assert!(view.scroll_y > 0.0);
            assert_eq!(preview_y, 0.0);
        } else {
            assert_eq!(view.scroll_y, 0.0, "preview wheel leaked to focused editor");
            assert!(preview_y > 0.0);
        }
    }
}
#[test]
fn idle_focused_editor_does_not_schedule_frames() {
    let ctx = egui::Context::default();
    let mut view = EditorView::new();
    let mut buffer = EditorBuffer::from_text("idle text\nsecond line".into());
    let mut repaint_delay = std::time::Duration::ZERO;
    for frame in 0..6 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 500.0),
                )),
                time: Some(frame as f64 * 0.5),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.render(
                        ui,
                        &mut buffer,
                        true,
                        None,
                        LineNumberMode::Absolute,
                        true,
                        true,
                        None,
                        false,
                        None,
                    );
                });
            },
        );
        repaint_delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    }
    assert!(
        repaint_delay > std::time::Duration::from_secs(60),
        "idle editor asked for another frame after {repaint_delay:?}"
    );
}
