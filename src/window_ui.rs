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

#[cfg(test)]
pub fn titlebar(ui: &mut egui::Ui, title: &str) {
    titlebar_with_tabs(ui, title, &[], 0);
}

/// What the user did on the tab strip this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum TabAction {
    Activate(usize),
    Close(usize),
    Move {
        from: usize,
        to: usize,
    },
    /// Double click, type, Enter: the tab's alias (blank removes it).
    Rename {
        index: usize,
        alias: String,
    },
    /// The tab was dragged out of the window: make it a window that follows
    /// the cursor, if that can be done right away.
    TearOff {
        index: usize,
    },
    /// The tab was dropped outside the window, at this screen position
    /// (ui points): open it in a new window there.
    Detach {
        index: usize,
        at: egui::Pos2,
    },
    New,
}

const TAB_FONT: f32 = 12.0;
const TAB_CENTERS: &str = "slate_tab_centers";
const DROP_HOVER: &str = "slate_tab_drop_hover";
const DRAGGED_OUT: &str = "slate_tab_dragged_out";

/// A tab from another window is over this one: where it would land and its
/// label (None when no tab is).
pub(crate) fn set_drop_hover(ctx: &egui::Context, hover: Option<(usize, String)>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(DROP_HOVER), hover));
}

/// The label of a tab of ours dragged out of this window, while it is.
pub(crate) fn dragged_out_label(ctx: &egui::Context) -> Option<String> {
    ctx.data(|d| d.get_temp::<Option<String>>(egui::Id::new(DRAGGED_OUT)))
        .flatten()
}
const TITLEBAR_HEIGHT: f32 = 30.0;

/// Where a tab dropped at `screen` (physical pixels) goes among this
/// window's tabs: by position over the tab strip, last anywhere else.
pub(crate) fn tab_slot_at(ctx: &egui::Context, screen: (f32, f32)) -> usize {
    let centers: Vec<f32> = ctx
        .data(|d| d.get_temp(egui::Id::new(TAB_CENTERS)))
        .unwrap_or_default();
    let Some(inner) = ctx.input(|i| i.viewport().inner_rect) else {
        return centers.len();
    };
    let ppp = ctx.pixels_per_point();
    let local = egui::pos2(screen.0 / ppp, screen.1 / ppp) - inner.min.to_vec2();
    if local.y > TITLEBAR_HEIGHT + 4.0 {
        return centers.len();
    }
    centers.iter().filter(|x| **x < local.x).count()
}
const TAB_CORNERS: egui::CornerRadius = egui::CornerRadius {
    nw: 6,
    ne: 6,
    sw: 0,
    se: 0,
};
/// How far below the strip a dragged tab comes off it.
const LIFT_DISTANCE: f32 = 12.0;
/// Time constant of tabs sliding into their new places.
const SLIDE_SECONDS: f32 = 0.045;

/// Where a tab dragged over from another window would land: a bar between
/// the tabs with the tab's name beside it, and the window outlined.
fn paint_drop_marker(
    ui: &egui::Ui,
    strip: egui::Rect,
    rects: &[egui::Rect],
    slot: usize,
    label: &str,
    font: &FontId,
) {
    let green = Color32::from_rgb(163, 190, 140);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("slate_tab_drop_marker"),
    ));
    painter.rect_stroke(
        ui.ctx().content_rect(),
        0.0,
        egui::Stroke::new(2.0_f32, green),
        egui::StrokeKind::Inside,
    );
    let x = match slot.checked_sub(1).and_then(|before| rects.get(before)) {
        Some(before) => before.right() + 1.0,
        None => rects
            .first()
            .map_or(strip.left(), |first| first.left() - 1.0),
    };
    let x = x.clamp(strip.left() + 1.5, ui.ctx().content_rect().right() - 1.5);
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(x, strip.center().y + 2.0),
            Vec2::new(3.0, strip.height() - 6.0),
        ),
        1.5,
        green,
    );
    let galley = painter.layout_no_wrap(label.to_string(), font.clone(), green);
    let pill = egui::Rect::from_min_size(
        egui::pos2(x + 5.0, strip.center().y - galley.size().y / 2.0 - 1.0),
        galley.size() + Vec2::new(12.0, 4.0),
    );
    painter.rect_filled(pill, 4.0, Color32::from_rgb(30, 37, 48));
    painter.galley(pill.min + Vec2::new(6.0, 2.0), galley, green);
}

/// A tab dragged off the strip: a card under the pointer that says what
/// letting go does.
struct Lifted {
    card: egui::Rect,
    text: String,
    detach_ready: bool,
    /// Over another Slate window: letting go joins the tab to it.
    joins_window: bool,
}

impl Lifted {
    fn paint(&self, ctx: &egui::Context, font: &FontId) {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("slate_tab_lifted"),
        ));
        let accent = Color32::from_rgb(136, 192, 208);
        let border = if self.joins_window {
            Color32::from_rgb(163, 190, 140)
        } else if self.detach_ready {
            accent
        } else {
            Color32::from_rgb(76, 86, 106)
        };
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 4],
                blur: 14,
                spread: 0,
                color: Color32::from_black_alpha(140),
            }
            .as_shape(self.card, 6),
        );
        painter.rect_filled(self.card, 6.0, Color32::from_rgb(36, 44, 58));
        painter.rect_stroke(
            self.card,
            6.0,
            egui::Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        );
        let mut job = egui::text::LayoutJob::simple_singleline(
            self.text.clone(),
            font.clone(),
            Color32::from_rgb(216, 222, 233),
        );
        job.wrap.max_width = (self.card.width() - 20.0).max(8.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        let galley = painter.layout_job(job);
        painter.galley(
            egui::pos2(
                self.card.left() + 10.0,
                self.card.center().y - galley.size().y / 2.0,
            ),
            galley,
            Color32::WHITE,
        );
        let (hint, color) = if self.joins_window {
            (
                "Release: move to that window",
                Color32::from_rgb(163, 190, 140),
            )
        } else if self.detach_ready {
            ("Release: new window", accent)
        } else {
            (
                "Drag out for a new window",
                Color32::from_rgb(136, 154, 176),
            )
        };
        painter.text(
            egui::pos2(self.card.left() + 2.0, self.card.bottom() + 6.0),
            egui::Align2::LEFT_TOP,
            hint,
            FontId::proportional(11.0),
            color,
        );
    }
}
const RENAMING_TAB: &str = "slate_tab_renaming";

/// True while a tab's alias is being typed: app shortcuts must not take its keys.
pub fn renaming_tab(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<(u64, bool)>(egui::Id::new(RENAMING_TAB)))
        .is_some()
}

/// Draws the tabs from the left edge of the title bar and returns where the
/// strip ends (the rest of the bar stays draggable).
///
/// Tabs shrink down to `MIN_TAB` and then the strip scrolls: the wheel over the
/// strip moves it, and the active tab is always brought into view. Widget ids
/// come from tab ids, so dragging a tab survives the reorders it causes.
fn draw_tabs(
    ui: &mut egui::Ui,
    bar: egui::Rect,
    tabs: &[crate::tabs::TabLabel],
    active: usize,
    action: &mut Option<TabAction>,
) -> f32 {
    const MIN_TAB: f32 = 72.0;
    const GAP: f32 = 2.0;
    let font = FontId::monospace(TAB_FONT);
    let window_buttons = 3.0 * 38.0;
    let plus_width = 26.0;
    // Leave some empty bar to grab and move the window.
    let available = (bar.width() - window_buttons - plus_width - 60.0).max(80.0);
    let natural: Vec<f32> = tabs
        .iter()
        .map(|tab| {
            let text = ui
                .painter()
                .layout_no_wrap(tab.text.clone(), font.clone(), Color32::WHITE)
                .size()
                .x;
            (text + 44.0).clamp(90.0, 220.0)
        })
        .collect();
    let room = (available - GAP * tabs.len() as f32).max(0.0);
    let natural_total: f32 = natural.iter().sum();
    let scale = if natural_total > room {
        room / natural_total
    } else {
        1.0
    };
    let widths: Vec<f32> = natural.iter().map(|w| (w * scale).max(MIN_TAB)).collect();
    let content: f32 = widths.iter().map(|w| w + GAP).sum();
    let strip = egui::Rect::from_min_size(
        egui::pos2(bar.left() + 6.0, bar.top()),
        Vec2::new(content.min(available), bar.height()),
    );

    // Horizontal scroll offset, kept per window.
    let scroll_id = ui.id().with("slate_tab_scroll");
    let max_offset = (content - strip.width()).max(0.0);
    let mut offset: f32 = ui.ctx().data(|d| d.get_temp(scroll_id)).unwrap_or(0.0);
    let hover = ui.rect_contains_pointer(strip);
    if hover && max_offset > 0.0 {
        let delta = ui.input(|i| {
            if i.modifiers.ctrl {
                0.0
            } else {
                i.smooth_scroll_delta.y + i.smooth_scroll_delta.x
            }
        });
        offset -= delta;
    }
    let active_id = tabs.get(active).map(|t| t.id);
    let last_active_id = ui.id().with("slate_tab_last_active");
    if ui.ctx().data(|d| d.get_temp::<u64>(last_active_id)) != active_id {
        // Bring a newly active tab into view.
        let start: f32 = widths[..active.min(widths.len())]
            .iter()
            .map(|w| w + GAP)
            .sum();
        let end = start + widths.get(active).copied().unwrap_or(0.0);
        if start < offset {
            offset = start;
        } else if end > offset + strip.width() {
            offset = end - strip.width();
        }
        if let Some(id) = active_id {
            ui.ctx().data_mut(|d| d.insert_temp(last_active_id, id));
        }
    }
    offset = offset.clamp(0.0, max_offset);
    ui.ctx().data_mut(|d| d.insert_temp(scroll_id, offset));

    let painter = ui.painter_at(strip);
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("tab_strip"), strip));
    let pointer = ui.ctx().pointer_interact_pos();
    let mut x = strip.left() - offset;
    let rects: Vec<egui::Rect> = widths
        .iter()
        .map(|width| {
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, bar.top() + 4.0),
                Vec2::new(*width, bar.height() - 4.0),
            );
            x += width + GAP;
            rect
        })
        .collect();
    // Where the tabs are, for a tab dropped here from another window.
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            egui::Id::new(TAB_CENTERS),
            rects.iter().map(|r| r.center().x).collect::<Vec<f32>>(),
        )
    });
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let latest = ui.input(|i| i.pointer.latest_pos());
    let screen = ui.ctx().content_rect();
    let mut sliding = false;
    // A tab pulled off the strip, drawn after the others as a floating card.
    let mut lifted: Option<Lifted> = None;
    let mut dragged_out: Option<String> = None;
    for (index, tab) in tabs.iter().enumerate() {
        let rect = rects[index];
        let visible = rect.intersect(strip);
        if visible.width() <= 0.0 {
            continue;
        }
        let id = ui.id().with(("slate_tab", tab.id));
        let response = ui
            .interact(visible, id, egui::Sense::click_and_drag())
            .on_hover_text(&tab.tooltip);
        let close_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 13.0, rect.center().y),
            Vec2::splat(16.0),
        );
        let close = if strip.contains_rect(close_rect) {
            Some(ui.interact(close_rect, id.with("close"), egui::Sense::click()))
        } else {
            None
        };
        let close_hovered = close.as_ref().is_some_and(|c| c.hovered());
        let is_active = index == active;
        let dragging = response.dragged();
        let hovered = response.hovered() || close_hovered;
        // Where the tab is drawn (strip coordinates, so scrolling does not
        // animate): it slides toward its slot, and a dragged tab is carried
        // by the pointer where it was grabbed.
        let slot_x = rect.left() - strip.left() + offset;
        let drawn_id = id.with("drawn_x");
        let grab_id = id.with("grab");
        if response.drag_started() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(id.with("tear_off_tried"), false));
        }
        if response.drag_started()
            && let Some(pos) = ui.input(|i| i.pointer.press_origin())
        {
            ui.ctx()
                .data_mut(|d| d.insert_temp(grab_id, pos - rect.min));
        }
        let grab: Vec2 = ui
            .ctx()
            .data(|d| d.get_temp(grab_id))
            .unwrap_or(rect.size() / 2.0);
        let mut drawn: f32 = ui.ctx().data(|d| d.get_temp(drawn_id)).unwrap_or(slot_x);
        let mut pulled_off = false;
        let mut detach_ready = false;
        match latest.filter(|_| dragging) {
            Some(pos) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                pulled_off = pos.y > bar.bottom() + LIFT_DISTANCE || !screen.contains(pos);
                detach_ready = !screen.expand(4.0).contains(pos);
                let carried = (pos.x - grab.x).clamp(
                    strip.left(),
                    (strip.right() - rect.width()).max(strip.left()),
                );
                drawn = carried - strip.left() + offset;
            }
            None => {
                drawn += (slot_x - drawn) * (1.0 - (-dt / SLIDE_SECONDS).exp());
                if (slot_x - drawn).abs() < 0.5 {
                    drawn = slot_x;
                } else {
                    sliding = true;
                }
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(drawn_id, drawn));
        let shift = Vec2::new(drawn - slot_x, 0.0);
        let shown = rect.translate(shift);
        if pulled_off && let Some(pos) = latest {
            // Its slot keeps an outline; the card follows the pointer, held
            // inside the window when the pointer leaves it.
            painter.rect_stroke(
                rect,
                TAB_CORNERS,
                egui::Stroke::new(1.0_f32, Color32::from_rgb(58, 68, 86)),
                egui::StrokeKind::Inside,
            );
            let card = egui::Rect::from_min_size(pos - grab, rect.size());
            let room = screen.shrink(6.0);
            let card = card.translate(Vec2::new(
                (room.left() - card.left()).max(0.0) + (room.right() - card.right()).min(0.0),
                (room.top() - card.top()).max(0.0)
                    + (room.bottom() - card.bottom() - 26.0).min(0.0),
            ));
            let joins_window = detach_ready
                && crate::tab_drop::target_under_cursor(&[std::process::id()]).is_some();
            if detach_ready {
                dragged_out = Some(tab.text.clone());
                // Out of the window and not over another one: become a
                // window that follows the cursor (asked once per drag).
                let tried = id.with("tear_off_tried");
                if !joins_window
                    && tabs.len() > 1
                    && !ui
                        .ctx()
                        .data(|d| d.get_temp::<bool>(tried))
                        .unwrap_or(false)
                {
                    ui.ctx().data_mut(|d| d.insert_temp(tried, true));
                    *action = Some(TabAction::TearOff { index });
                }
            }
            lifted = Some(Lifted {
                card,
                text: tab.text.clone(),
                detach_ready,
                joins_window,
            });
        } else if dragging {
            ui.ctx().request_repaint();
        }
        let fill = if is_active || dragging {
            Some(Color32::from_rgb(36, 44, 58))
        } else if hovered {
            Some(Color32::from_rgb(30, 37, 48))
        } else {
            None
        };
        // The dragged tab is drawn above its neighbours.
        let mut painter = if dragging {
            ui.ctx()
                .layer_painter(egui::LayerId::new(egui::Order::Foreground, id))
                .with_clip_rect(strip)
        } else {
            painter.clone()
        };
        if pulled_off {
            painter.set_invisible();
        }
        if let Some(fill) = fill {
            painter.rect_filled(shown, TAB_CORNERS, fill);
        }
        let text_color = if is_active {
            Color32::from_rgb(216, 222, 233)
        } else {
            Color32::from_rgb(136, 154, 176)
        };
        // Inline rename (alias): double click turns the label into a text field.
        let rename_id = egui::Id::new(RENAMING_TAB);
        let rename_text_id = egui::Id::new("slate_tab_rename_text");
        if response.double_clicked() {
            ui.ctx().data_mut(|d| {
                d.insert_temp(rename_id, (tab.id, true));
                d.insert_temp(rename_text_id, tab.rename_text.clone());
            });
        }
        if let Some((renaming, first_frame)) =
            ui.ctx().data(|d| d.get_temp::<(u64, bool)>(rename_id))
            && renaming == tab.id
        {
            let mut text: String = ui
                .ctx()
                .data(|d| d.get_temp(rename_text_id))
                .unwrap_or_else(|| tab.rename_text.clone());
            let field = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 6.0, rect.top() + 3.0),
                egui::pos2(rect.right() - 24.0, rect.bottom() - 3.0),
            );
            let edit = ui.put(
                field,
                egui::TextEdit::singleline(&mut text)
                    .font(font.clone())
                    .text_color(Color32::from_rgb(216, 222, 233))
                    .background_color(Color32::from_rgb(30, 37, 48))
                    .hint_text("alias"),
            );
            if first_frame {
                edit.request_focus();
                ui.ctx()
                    .data_mut(|d| d.insert_temp(rename_id, (tab.id, false)));
            }
            if edit.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    *action = Some(TabAction::Rename {
                        index,
                        alias: text.clone(),
                    });
                }
                ui.ctx().data_mut(|d| {
                    d.remove::<(u64, bool)>(rename_id);
                    d.remove::<String>(rename_text_id);
                });
            } else {
                ui.ctx().data_mut(|d| d.insert_temp(rename_text_id, text));
            }
            continue;
        }
        let mut job =
            egui::text::LayoutJob::simple_singleline(tab.text.clone(), font.clone(), text_color);
        job.wrap.max_width = (rect.width() - 34.0).max(8.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        let galley = ui.painter().layout_job(job);
        let text_pos = egui::pos2(
            shown.left() + 10.0,
            shown.center().y - galley.size().y / 2.0,
        );
        painter
            .with_clip_rect(shown.intersect(strip))
            .galley(text_pos, galley, text_color);
        let close_shown = close_rect.translate(shift);
        if close_hovered {
            painter.rect_filled(close_shown, 3.0, Color32::from_rgb(58, 68, 86));
        }
        if tab.dirty && !hovered {
            painter.circle_filled(close_shown.center(), 3.5, Color32::from_rgb(235, 203, 139));
        } else if is_active || hovered {
            let c = close_shown.center();
            let stroke = egui::Stroke::new(1.3_f32, text_color);
            painter.line_segment([c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)], stroke);
            painter.line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], stroke);
        }
        if response.drag_stopped() {
            // Dropped outside the window: move the tab to a new window.
            let screen = ui.ctx().content_rect();
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos())
                && !screen.expand(4.0).contains(pos)
            {
                let origin = ui
                    .input(|i| i.viewport().inner_rect)
                    .map_or(egui::Pos2::ZERO, |inner| inner.min);
                *action = Some(TabAction::Detach {
                    index,
                    at: origin + pos.to_vec2(),
                });
            }
        } else if dragging && !pulled_off {
            // Reorder live: the tab follows the pointer across its neighbours.
            if let Some(pos) = pointer {
                let target = drag_target(&rects, index, pos.x);
                if target != index {
                    *action = Some(TabAction::Move {
                        from: index,
                        to: target,
                    });
                }
            }
        } else if close.is_some_and(|c| c.clicked()) || response.middle_clicked() {
            *action = Some(TabAction::Close(index));
        } else if response.clicked() || response.drag_started() {
            *action = Some(TabAction::Activate(index));
        }
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(("tab_rect", index)), rect));
        #[cfg(test)]
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(("tab_shown", index)), shown));
    }
    if sliding {
        ui.ctx().request_repaint();
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(DRAGGED_OUT), dragged_out));
    // A tab from another window over this one: where it would land.
    if let Some((slot, label)) = ui
        .ctx()
        .data(|d| d.get_temp::<Option<(usize, String)>>(egui::Id::new(DROP_HOVER)))
        .flatten()
    {
        paint_drop_marker(ui, strip, &rects, slot, &label, &font);
    }
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("tab_lifted"),
            lifted.as_ref().map(|l| (l.card, l.detach_ready)),
        )
    });
    if let Some(lifted) = lifted {
        lifted.paint(ui.ctx(), &font);
    }
    // Scroll hints when tabs are hidden on either side.
    let fade = Color32::from_rgba_unmultiplied(22, 28, 37, 220);
    if offset > 0.5 {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(strip.left_top(), Vec2::new(10.0, strip.height())),
            0.0,
            fade,
        );
    }
    if offset < max_offset - 0.5 {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(strip.right() - 10.0, strip.top()),
                Vec2::new(10.0, strip.height()),
            ),
            0.0,
            fade,
        );
    }
    let plus_rect = egui::Rect::from_min_size(
        egui::pos2(strip.right() + 2.0, bar.top() + 6.0),
        Vec2::new(plus_width - 4.0, bar.height() - 10.0),
    );
    let plus = ui.interact(
        plus_rect,
        ui.id().with("slate_tab_new"),
        egui::Sense::click(),
    );
    if plus.hovered() {
        ui.painter()
            .rect_filled(plus_rect, 4.0, Color32::from_rgb(36, 44, 58));
    }
    let c = plus_rect.center();
    let stroke = egui::Stroke::new(1.3_f32, Color32::from_rgb(136, 154, 176));
    ui.painter()
        .line_segment([c + Vec2::new(-4.5, 0.0), c + Vec2::new(4.5, 0.0)], stroke);
    ui.painter()
        .line_segment([c + Vec2::new(0.0, -4.5), c + Vec2::new(0.0, 4.5)], stroke);
    if plus.on_hover_text("New tab (Ctrl+T)").clicked() {
        *action = Some(TabAction::New);
    }
    plus_rect.right()
}

/// Where a dragged tab goes: it swaps with a neighbour once the pointer passes
/// that neighbour's centre, so tabs of different widths do not flicker.
pub(crate) fn drag_target(rects: &[egui::Rect], index: usize, x: f32) -> usize {
    let mut target = index;
    while target + 1 < rects.len() && x > rects[target + 1].center().x {
        target += 1;
    }
    while target > 0 && x < rects[target - 1].center().x {
        target -= 1;
    }
    target
}

/// Title bar; with `tabs` it shows the tab strip instead of the centered title.
pub fn titlebar_with_tabs(
    ui: &mut egui::Ui,
    title: &str,
    tabs: &[crate::tabs::TabLabel],
    active: usize,
) -> Option<TabAction> {
    let mut action = None;
    let ctx = ui.ctx().clone();
    resize_edges(&ctx);
    egui::Panel::top("slate_titlebar")
        .exact_size(TITLEBAR_HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(22, 28, 37))
                .inner_margin(0.0),
        )
        .show_inside(ui, |ui| {
            let rect = ui.max_rect();
            let tabs_end = if tabs.is_empty() {
                rect.left()
            } else {
                draw_tabs(ui, rect, tabs, active, &mut action)
            };
            let drag_rect = egui::Rect::from_min_max(
                egui::pos2((rect.min.x + 5.0).max(tabs_end + 6.0), rect.min.y + 5.0),
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
            if title_width > 0.0 && tabs.is_empty() {
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
    action
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
    fn tabs_never_overlap_window_buttons_and_leave_room_to_drag() {
        for (width, count) in [(420.0, 3), (900.0, 1), (900.0, 12), (1600.0, 30)] {
            let ctx = egui::Context::default();
            let tabs: Vec<_> = (0..count)
                .map(|n| crate::tabs::TabLabel {
                    id: n as u64,
                    text: format!("documento-{n}-con-nombre-largo.md"),
                    dirty: n % 2 == 0,
                    tooltip: String::new(),
                    rename_text: String::new(),
                })
                .collect();
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 560.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    titlebar_with_tabs(ui, "Slate", &tabs, 0);
                },
            );
            let (frame, _, buttons) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Rect, egui::Rect, Vec<egui::Rect>)>(
                        "titlebar_geometry".into(),
                    )
                })
                .unwrap();
            let first_button = buttons.iter().map(|b| b.left()).fold(f32::MAX, f32::min);
            // Tabs are drawn clipped to the strip (the rest scrolls).
            let last_tab = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("tab_strip")))
                .unwrap();
            assert!(frame.contains_rect(last_tab), "{width} {count}");
            assert!(
                last_tab.right() + 26.0 + 30.0 < first_button,
                "tabs + new-tab button leave drag room: {width} {count} {last_tab:?} {first_button}"
            );
        }
    }
    #[test]
    fn many_tabs_scroll_and_the_active_tab_is_always_visible() {
        let ctx = egui::Context::default();
        let tabs: Vec<_> = (0..40)
            .map(|n| crate::tabs::TabLabel {
                id: n as u64,
                text: format!("nota-{n}.md"),
                dirty: false,
                tooltip: String::new(),
                rename_text: String::new(),
            })
            .collect();
        for active in [39, 0, 20] {
            for _ in 0..2 {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(900.0, 560.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        titlebar_with_tabs(ui, "Slate", &tabs, active);
                    },
                );
            }
            let (frame, _, buttons) = ctx
                .data(|data| {
                    data.get_temp::<(egui::Rect, egui::Rect, Vec<egui::Rect>)>(
                        "titlebar_geometry".into(),
                    )
                })
                .unwrap();
            let first_button = buttons.iter().map(|b| b.left()).fold(f32::MAX, f32::min);
            let rect = ctx
                .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(("tab_rect", active))))
                .unwrap();
            assert!(rect.width() >= 72.0, "tabs stay readable: {rect:?}");
            assert!(
                rect.left() >= frame.left() && rect.right() + 26.0 < first_button,
                "active {active} visible: {rect:?}"
            );
        }
    }
    #[test]
    fn double_clicking_a_tab_edits_its_alias_and_enter_commits() {
        let ctx = egui::Context::default();
        let tabs = vec![crate::tabs::TabLabel {
            id: 7,
            text: "nota.md".into(),
            dirty: false,
            tooltip: String::new(),
            rename_text: "nota.md".into(),
        }];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 560.0));
        let mut frame = |time: f64, events: Vec<egui::Event>| {
            let mut action = None;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| action = titlebar_with_tabs(ui, "Slate", &tabs, 0),
            );
            action
        };
        frame(0.0, vec![]);
        let tab = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("tab_rect", 0))))
            .unwrap();
        let at = tab.left_center() + Vec2::new(20.0, 0.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(0.1, vec![egui::Event::PointerMoved(at)]);
        frame(0.2, vec![button(true)]);
        frame(0.25, vec![button(false)]);
        frame(0.3, vec![button(true)]);
        frame(0.35, vec![button(false)]);
        frame(0.4, vec![]);
        frame(0.5, vec![egui::Event::Text(" · plan".into())]);
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let action = frame(0.6, vec![enter]);
        assert_eq!(
            action,
            Some(TabAction::Rename {
                index: 0,
                alias: "nota.md · plan".into()
            })
        );
    }
    #[test]
    fn dropping_a_tab_outside_the_window_detaches_it() {
        let ctx = egui::Context::default();
        let tabs: Vec<_> = (0..2)
            .map(|n| crate::tabs::TabLabel {
                id: n,
                text: format!("nota-{n}.md"),
                dirty: false,
                tooltip: String::new(),
                rename_text: String::new(),
            })
            .collect();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 560.0));
        let mut frame = |time: f64, events: Vec<egui::Event>| {
            let mut action = None;
            let mut raw = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            };
            raw.viewports.insert(
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    inner_rect: Some(screen.translate(Vec2::new(100.0, 50.0))),
                    ..Default::default()
                },
            );
            let _ = ctx.run_ui(raw, |ui| action = titlebar_with_tabs(ui, "Slate", &tabs, 0));
            action
        };
        frame(0.0, vec![]);
        let tab = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("tab_rect", 1))))
            .unwrap();
        let start = tab.center();
        let outside = egui::pos2(start.x, 700.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(0.1, vec![egui::Event::PointerMoved(start)]);
        frame(0.2, vec![button(start, true)]);
        frame(
            0.3,
            vec![egui::Event::PointerMoved(start + Vec2::new(0.0, 40.0))],
        );
        frame(0.4, vec![egui::Event::PointerMoved(outside)]);
        let action = frame(0.5, vec![button(outside, false)]);
        assert_eq!(
            action,
            Some(TabAction::Detach {
                index: 1,
                at: egui::pos2(100.0 + outside.x, 750.0)
            })
        );
    }
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
