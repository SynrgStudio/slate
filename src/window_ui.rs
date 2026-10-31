//! Shared window chrome and bounded overlay layout (logical points, not pixels).
use eframe::egui::{self, Color32, FontId, Vec2};

pub struct DialogArea(egui::Area, bool);

impl DialogArea {
    pub fn new(id: egui::Id) -> Self {
        Self(
            egui::Area::new(id).anchor(egui::Align2::CENTER_CENTER, [0.0, 15.0]),
            true,
        )
    }

    pub fn anchor(self, _align: egui::Align2, _offset: impl Into<Vec2>) -> Self {
        // Every dialog shares the same centered, bounded viewport.
        Self(
            self.0.anchor(egui::Align2::CENTER_CENTER, [0.0, 15.0]),
            self.1,
        )
    }

    pub fn order(self, order: egui::Order) -> Self {
        Self(self.0.order(order), self.1)
    }

    /// The dialog supplies its own scroll region inside stationary chrome.
    pub fn fixed_chrome(mut self) -> Self {
        self.1 = false;
        self
    }

    pub fn show<R>(
        self,
        ctx: &egui::Context,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::Rect {
        let size = dialog_size(ctx.content_rect().size());
        self.0
            .show(ctx, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                let rect = egui::Rect::from_min_size(ui.cursor().min, size);
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_rgb(25, 31, 40));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)),
                    egui::StrokeKind::Inside,
                );
                if self.1 {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .max_width(size.x)
                        .max_height(size.y)
                        .show(ui, |ui| {
                            // Finite available height is important for embedded editors.
                            ui.set_max_size(size);
                            content(ui);
                        });
                } else {
                    content(ui);
                }
            })
            .response
            .rect
    }
}

fn dialog_size(window: Vec2) -> Vec2 {
    Vec2::new(window.x * 0.8, (window.y - 30.0).max(1.0) * 0.8)
}

pub fn titlebar(ui: &mut egui::Ui, title: &str) {
    let ctx = ui.ctx().clone();
    resize_edges(&ctx);
    egui::Panel::top("slate_titlebar")
        .exact_size(30.0)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(22, 28, 37))
                .inner_margin(0.0),
        )
        .show_inside(ui, |ui| {
            let rect = ui.max_rect();
            let drag_rect = egui::Rect::from_min_max(
                rect.min + Vec2::new(5.0, 5.0),
                rect.max - Vec2::new(120.0, 0.0),
            );
            let drag = ui.interact(
                drag_rect,
                ui.id().with("window_drag"),
                egui::Sense::click_and_drag(),
            );
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if drag.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            } else if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            // Symmetric reservation keeps the title centered on the whole window,
            // not merely on the remaining space to the left of the controls.
            let title_width = (rect.width() - 2.0 * (3.0 * 38.0 + 10.0)).max(0.0);
            #[cfg(test)]
            let mut recorded_title_rect = egui::Rect::from_center_size(rect.center(), Vec2::ZERO);
            if title_width > 0.0 {
                let mut job = egui::text::LayoutJob::simple_singleline(
                    title.into(),
                    FontId::monospace(12.0),
                    Color32::from_rgb(136, 192, 208),
                );
                job.wrap.max_width = title_width;
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                job.wrap.overflow_character = Some('…');
                let galley = ui.painter().layout_job(job);
                let title_rect = egui::Rect::from_center_size(rect.center(), galley.size());
                #[cfg(test)]
                {
                    recorded_title_rect = title_rect;
                }
                ui.painter_at(egui::Rect::from_center_size(
                    rect.center(),
                    Vec2::new(title_width, rect.height()),
                ))
                .galley(title_rect.min, galley, Color32::from_rgb(136, 192, 208));
            }
            #[cfg(test)]
            let mut buttons = Vec::new();
            for (index, icon, tooltip, command) in [
                (0, WindowIcon::Close, "Close", egui::ViewportCommand::Close),
                (
                    1,
                    if maximized {
                        WindowIcon::Restore
                    } else {
                        WindowIcon::Maximize
                    },
                    if maximized { "Restore" } else { "Maximize" },
                    egui::ViewportCommand::Maximized(!maximized),
                ),
                (
                    2,
                    WindowIcon::Minimize,
                    "Minimize",
                    egui::ViewportCommand::Minimized(true),
                ),
            ] {
                let button_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.right() - (index + 1) as f32 * 38.0, rect.top()),
                    egui::pos2(rect.right() - index as f32 * 38.0, rect.bottom() - 1.0),
                );
                let response = ui.put(button_rect, egui::Button::new("").frame(false));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip)
                });
                let hovered = response.hovered() || response.has_focus();
                if hovered {
                    ui.painter().rect_filled(
                        button_rect,
                        0.0,
                        if matches!(icon, WindowIcon::Close) {
                            Color32::from_rgb(180, 65, 75)
                        } else {
                            Color32::from_rgb(46, 56, 72)
                        },
                    );
                }
                paint_window_icon(
                    &ui.painter_at(button_rect),
                    button_rect,
                    icon,
                    if hovered {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(216, 222, 233)
                    },
                );
                #[cfg(test)]
                buttons.push(button_rect);
                if response
                    .on_hover_ui(|ui| {
                        crate::footer_ui::style_hints(ui);
                        ui.label(tooltip);
                    })
                    .clicked()
                {
                    ctx.send_viewport_cmd(command);
                }
            }
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    "titlebar_geometry".into(),
                    (rect, recorded_title_rect, buttons),
                )
            });
            ui.painter().hline(
                rect.x_range(),
                rect.bottom(),
                egui::Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
            );
        });
}

#[derive(Clone, Copy)]
enum WindowIcon {
    Minimize,
    Maximize,
    Restore,
    Close,
}
fn paint_window_icon(painter: &egui::Painter, rect: egui::Rect, icon: WindowIcon, color: Color32) {
    let center = rect.center();
    let stroke = egui::Stroke::new(1.3_f32, color);
    let point = |x, y| center + Vec2::new(x, y);
    match icon {
        WindowIcon::Minimize => {
            painter.line_segment([point(-5.0, 2.0), point(5.0, 2.0)], stroke);
        }
        WindowIcon::Maximize => {
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-5.0, -5.0), point(5.0, 5.0)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        WindowIcon::Restore => {
            painter.add(egui::Shape::line(
                vec![
                    point(-3.0, -3.0),
                    point(-3.0, -5.0),
                    point(5.0, -5.0),
                    point(5.0, 3.0),
                    point(3.0, 3.0),
                ],
                stroke,
            ));
            painter.rect_stroke(
                egui::Rect::from_min_max(point(-5.0, -3.0), point(3.0, 5.0)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        WindowIcon::Close => {
            painter.line_segment([point(-5.0, -5.0), point(5.0, 5.0)], stroke);
            painter.line_segment([point(-5.0, 5.0), point(5.0, -5.0)], stroke);
        }
    }
}
fn resize_edges(ctx: &egui::Context) {
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let rect = ctx.content_rect();
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let left = pos.x < rect.left() + 5.0;
    let right = pos.x > rect.right() - 5.0;
    let top = pos.y < rect.top() + 5.0;
    let bottom = pos.y > rect.bottom() - 5.0;
    use egui::ResizeDirection::*;
    let (direction, cursor) = match (left, right, top, bottom) {
        (true, _, true, _) => (NorthWest, egui::CursorIcon::ResizeNwSe),
        (_, true, true, _) => (NorthEast, egui::CursorIcon::ResizeNeSw),
        (true, _, _, true) => (SouthWest, egui::CursorIcon::ResizeNeSw),
        (_, true, _, true) => (SouthEast, egui::CursorIcon::ResizeNwSe),
        (true, _, _, _) => (West, egui::CursorIcon::ResizeHorizontal),
        (_, true, _, _) => (East, egui::CursorIcon::ResizeHorizontal),
        (_, _, true, _) => (North, egui::CursorIcon::ResizeVertical),
        (_, _, _, true) => (South, egui::CursorIcon::ResizeVertical),
        _ => return,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_is_centered_on_window_and_never_overlaps_controls() {
        for width in [180.0, 300.0, 420.0, 900.0, 1600.0] {
            let ctx = egui::Context::default();
            for title in [
                "untitled – Slate",
                "a very long document name ".repeat(30).as_str(),
            ] {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 560.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| titlebar(ui, title));
                    },
                );
                let (frame, title, buttons) = ctx
                    .data(|data| {
                        data.get_temp::<(egui::Rect, egui::Rect, Vec<egui::Rect>)>(
                            "titlebar_geometry".into(),
                        )
                    })
                    .unwrap();
                assert!((frame.center().x - title.center().x).abs() < 0.01);
                assert!(frame.contains_rect(title));
                assert_eq!(buttons.len(), 3);
                for (index, button) in buttons.iter().enumerate() {
                    assert!(frame.contains_rect(*button));
                    if title.width() > 0.0 {
                        assert!(!title.intersects(*button));
                    }
                    for other in &buttons[index + 1..] {
                        assert!(button.intersect(*other).width() <= 0.0);
                    }
                }
            }
        }
    }
    #[test]
    fn titlebar_buttons_send_close_minimize_maximize_and_restore_commands() {
        for (index, maximized) in [(0, false), (1, false), (1, true), (2, false)] {
            let ctx = egui::Context::default();
            for pass in 0..2 {
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(900.0, 560.0),
                    )),
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .maximized = Some(maximized);
                if pass == 1 {
                    let pos = egui::pos2(900.0 - index as f32 * 38.0 - 19.0, 15.0);
                    input.events = vec![
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
                    ];
                }
                let output = ctx.run(input, |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| titlebar(ui, "untitled – Slate"));
                });
                if pass == 1 {
                    assert!(
                        output.viewport_output[&egui::ViewportId::ROOT]
                            .commands
                            .iter()
                            .any(|command| match (index, command) {
                                (0, egui::ViewportCommand::Close) => true,
                                (1, egui::ViewportCommand::Maximized(value)) => *value != maximized,
                                (2, egui::ViewportCommand::Minimized(true)) => true,
                                _ => false,
                            })
                    );
                }
            }
        }
    }
    #[test]
    fn window_icons_are_vector_shapes_contained_in_buttons_at_multiple_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(38.0, 29.0));
            let output = ctx.run(Default::default(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    "icon_test".into(),
                ));
                for icon in [
                    WindowIcon::Minimize,
                    WindowIcon::Maximize,
                    WindowIcon::Restore,
                    WindowIcon::Close,
                ] {
                    paint_window_icon(&painter, rect, icon, Color32::WHITE);
                }
            });
            assert_eq!(output.shapes.len(), 6);
            for shape in output.shapes {
                assert!(!matches!(shape.shape, egui::Shape::Text(_)));
                assert!(rect.contains_rect(shape.shape.visual_bounding_rect()));
            }
        }
    }
    #[test]
    fn oversized_and_small_content_keep_the_same_bounded_viewport() {
        for window in [Vec2::new(420.0, 260.0), Vec2::new(900.0, 560.0)] {
            for content_size in [Vec2::new(50.0, 50.0), Vec2::new(1600.0, 2000.0)] {
                let ctx = egui::Context::default();
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, window)),
                        ..Default::default()
                    };
                    let _ = ctx.run(input, |ctx| {
                        let rect = DialogArea::new("test_dialog".into())
                            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                            .show(ctx, |ui| {
                                ui.allocate_exact_size(content_size, egui::Sense::hover());
                            });
                        let expected = dialog_size(window);
                        assert!((rect.width() - expected.x).abs() < 1.0, "{rect:?}");
                        assert!((rect.height() - expected.y).abs() < 1.0, "{rect:?}");
                    });
                }
            }
        }
    }

    #[test]
    fn fixed_chrome_stays_put_while_body_scrolls() {
        let ctx = egui::Context::default();
        let window = Vec2::new(900.0, 560.0);
        let mut positions = Vec::new();
        for offset in [0.0, 100.0] {
            for pass in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, window)),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    let mut header = egui::Rect::NOTHING;
                    let mut first_row = egui::Rect::NOTHING;
                    let rect = DialogArea::new("fixed_header_test".into())
                        .fixed_chrome()
                        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                        .show(ctx, |ui| {
                            egui::Frame::new().inner_margin(14.0).show(ui, |ui| {
                                ui.set_width(dialog_size(window).x - 28.0);
                                ui.set_height(dialog_size(window).y - 28.0);
                                header = ui.label("shortcuts + commands").rect;
                                egui::ScrollArea::both()
                                    .auto_shrink([false, false])
                                    .max_height(ui.available_height())
                                    .vertical_scroll_offset(offset)
                                    .show(ui, |ui| {
                                        first_row = ui
                                            .allocate_exact_size(
                                                Vec2::new(600.0, 2000.0),
                                                egui::Sense::hover(),
                                            )
                                            .0;
                                    });
                            });
                        });
                    if pass == 2 {
                        positions.push((rect, header, first_row));
                    }
                });
            }
        }
        assert_eq!(positions[0].0, positions[1].0, "outer frame moved");
        assert_eq!(positions[0].1, positions[1].1, "header moved");
        assert!((positions[0].2.top() - positions[1].2.top() - 100.0).abs() < 1.0);
    }

    #[test]
    fn dialogs_use_eighty_percent_at_all_window_sizes() {
        for window in [
            Vec2::new(420.0, 260.0),
            Vec2::new(900.0, 560.0),
            Vec2::new(1920.0, 1080.0),
        ] {
            let size = dialog_size(window);
            assert!((size.x / window.x - 0.8).abs() < 0.0001);
            assert!((size.y / (window.y - 30.0) - 0.8).abs() < 0.0001);
        }
    }
}
