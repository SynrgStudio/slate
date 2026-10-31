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
fn assert_layout(
    view: &EditorView,
    p: &egui::Painter,
    buffer: &EditorBuffer,
    font: &FontId,
    width: f32,
    wrap: bool,
) {
    let cached = view.cached_visual_rows(p, buffer, font, width, wrap);
    let full = view.visual_rows(p, buffer, font, width, wrap);
    let tuple = |r: &VisualRow| (r.line_index, r.start, r.end);
    assert_eq!(
        cached.iter().map(tuple).collect::<Vec<_>>(),
        full.iter().map(tuple).collect::<Vec<_>>()
    );
    let rebuilt = EditorBuffer::from_text(buffer.as_str().into());
    assert_eq!(buffer.line_count(), rebuilt.line_count());
    for line in 0..buffer.line_count() {
        assert_eq!(buffer.line_start(line), rebuilt.line_start(line));
        assert_eq!(buffer.line_end(line), rebuilt.line_end(line));
    }
}
#[test]
fn single_line_edit_only_rewraps_one_line_and_preserves_suffix_positions() {
    let ctx = egui::Context::default();
    let view = EditorView::new();
    let font = FontId::monospace(12.0);
    let mut buffer = EditorBuffer::from_text("repeat café 雪 words and text\n".repeat(1000));
    frame(&ctx, |p| {
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        buffer.set_cursor(buffer.line_start(500));
        buffer.insert_text("λ");
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        assert_eq!(
            view.row_layout
                .borrow()
                .as_ref()
                .unwrap()
                .recalculated_lines,
            1
        );
        assert!(buffer.undo());
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        assert_eq!(
            view.row_layout
                .borrow()
                .as_ref()
                .unwrap()
                .recalculated_lines,
            1
        );
        assert!(buffer.redo());
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        assert_eq!(
            view.row_layout
                .borrow()
                .as_ref()
                .unwrap()
                .recalculated_lines,
            1
        );
    });
}
#[test]
fn multiline_paste_join_delete_and_selection_match_full_layout() {
    let ctx = egui::Context::default();
    let view = EditorView::new();
    let font = FontId::monospace(12.0);
    let mut buffer = EditorBuffer::from_text("first\n雪 second\n\nthird\nlast\n".into());
    frame(&ctx, |p| {
        for wrap in [false, true] {
            assert_layout(&view, p, &buffer, &font, 40.0, wrap);
            buffer.set_cursor(buffer.line_start(1));
            buffer.insert_text("a\n\nβ\n");
            assert_layout(&view, p, &buffer, &font, 40.0, wrap);
            let start = buffer.line_start(1);
            let end = buffer.line_start(4);
            buffer.set_selection(start, end);
            buffer.insert_text("joined");
            assert_layout(&view, p, &buffer, &font, 40.0, wrap);
            buffer.move_to_bottom();
            buffer.backspace();
            assert_layout(&view, p, &buffer, &font, 40.0, wrap);
            buffer.set_selection(0, buffer.as_str().len());
            buffer.insert_text("");
            assert_layout(&view, p, &buffer, &font, 40.0, wrap);
            assert_eq!(buffer.line_count(), 1);
            buffer.undo();
            assert_layout(&view, p, &buffer, &font, 40.0, wrap);
        }
    });
}
#[test]
fn randomized_batched_edits_undo_redo_match_fresh_layout_and_line_index() {
    let ctx = egui::Context::default();
    let current = EditorView::new();
    let batched = EditorView::new();
    let font = FontId::monospace(12.0);
    let mut buffer = EditorBuffer::from_text("paragraph café 雪\n\n```\ncode\n```\n".repeat(20));
    let mut seed = 12345_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 32) as usize
    };
    frame(&ctx, |p| {
        assert_layout(&current, p, &buffer, &font, 83.0, true);
        assert_layout(&batched, p, &buffer, &font, 83.0, true);
        for step in 0..400 {
            match random() % 10 {
                0 => {
                    buffer.undo();
                }
                1 => {
                    buffer.redo();
                }
                _ => {
                    let boundaries: Vec<_> = buffer
                        .as_str()
                        .char_indices()
                        .map(|(n, _)| n)
                        .chain(std::iter::once(buffer.as_str().len()))
                        .collect();
                    let first = random() % boundaries.len();
                    let last = (first + random() % 8).min(boundaries.len() - 1);
                    let replacement = ["x", "雪", "é", "\n", "\n\n", "abc\nλ", ""][random() % 7];
                    if step % 3 == 0 {
                        buffer.set_selection(boundaries[first], boundaries[last]);
                        buffer.insert_text(replacement);
                    } else {
                        buffer.clear_selection();
                        buffer.replace_selection_or_range(
                            boundaries[first],
                            boundaries[last],
                            replacement,
                        );
                    }
                }
            }
            assert_layout(&current, p, &buffer, &font, 83.0, true);
            if step % 7 == 0 {
                assert_layout(&batched, p, &buffer, &font, 83.0, true);
            }
        }
        assert_layout(&batched, p, &buffer, &font, 83.0, true);
    });
}
#[test]
fn bounded_history_unknown_edit_and_replaced_document_fall_back_safely() {
    let ctx = egui::Context::default();
    let view = EditorView::new();
    let font = FontId::monospace(12.0);
    let mut buffer = EditorBuffer::from_text("a line\n".repeat(100));
    frame(&ctx, |p| {
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        buffer.move_to_top();
        for _ in 0..80 {
            buffer.insert_text("x");
        }
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        assert_eq!(
            view.row_layout
                .borrow()
                .as_ref()
                .unwrap()
                .recalculated_lines,
            buffer.line_count()
        );
        buffer.text_mut().push_str("external\n");
        buffer.mark_external_edit();
        buffer.move_to_top();
        buffer.insert_text("λ");
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        assert_eq!(
            view.row_layout
                .borrow()
                .as_ref()
                .unwrap()
                .recalculated_lines,
            buffer.line_count()
        );
        buffer.set_text("new\nreplacement".into());
        assert_layout(&view, p, &buffer, &font, 80.0, true);
        assert_eq!(
            view.row_layout
                .borrow()
                .as_ref()
                .unwrap()
                .recalculated_lines,
            buffer.line_count()
        );
    });
}
