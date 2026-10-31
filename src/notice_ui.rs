//! Shared fixed-chrome notices: one centered frame, Slate typography, no nested panels.
use eframe::egui::{self, Color32, FontFamily, FontId, Rect, RichText, Vec2};
pub(super) const ACCENT: Color32 = Color32::from_rgb(136, 192, 208);
pub(super) const TEXT: Color32 = Color32::from_rgb(216, 222, 233);
pub(super) const DIM: Color32 = Color32::from_rgb(136, 154, 176);
pub(super) const WARN: Color32 = Color32::from_rgb(235, 203, 139);
pub(super) const RED: Color32 = Color32::from_rgb(191, 97, 106);
pub(super) const GREEN: Color32 = Color32::from_rgb(163, 190, 140);
#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct Geometry {
    pub frame: Rect,
    pub header: Rect,
    pub body: Rect,
    pub footer: Rect,
    pub actions: Vec<Rect>,
}

pub(super) fn show(
    ctx: &egui::Context,
    id: egui::Id,
    title: &str,
    actions: &[(&str, &str, Color32)],
    body: impl FnOnce(&mut egui::Ui),
) -> Vec<bool> {
    let mut clicked = vec![false; actions.len()];
    let font = FontId::new(14.0, FontFamily::Monospace);
    let widths: Vec<f32> = actions
        .iter()
        .map(|(key, label, _)| {
            ctx.fonts_mut(|f| {
                f.layout_no_wrap(format!("[{key}] {label}"), font.clone(), DIM)
                    .size()
                    .x
                    + 16.0
            })
        })
        .collect();
    #[cfg(test)]
    let mut geometry = Geometry {
        frame: Rect::NOTHING,
        header: Rect::NOTHING,
        body: Rect::NOTHING,
        footer: Rect::NOTHING,
        actions: Vec::new(),
    };
    let frame = crate::window_ui::DialogArea::new(id)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_chrome()
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let size = Vec2::new(
                ctx.content_rect().width() * 0.8,
                (ctx.content_rect().height() - 30.0).max(1.0) * 0.8,
            );
            let panel = Rect::from_min_size(ui.cursor().min, size);
            let inner = panel.shrink(14.0);
            let stacked = widths.iter().sum::<f32>()
                + 12.0 * actions.len().saturating_sub(1) as f32
                > inner.width();
            let footer_height = if stacked {
                28.0 * actions.len() as f32
            } else {
                28.0
            };
            let header = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 28.0));
            let footer = Rect::from_min_max(
                egui::pos2(inner.left(), inner.bottom() - footer_height),
                inner.max,
            );
            let content = Rect::from_min_max(
                egui::pos2(inner.left(), header.bottom() + 8.0),
                egui::pos2(
                    inner.right(),
                    (footer.top() - 8.0).max(header.bottom() + 9.0),
                ),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(header), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(FontId::new(16.0, FontFamily::Monospace))
                            .color(ACCENT),
                    )
                    .truncate(),
                )
                .on_hover_text(title);
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                ui.style_mut().override_font_id = Some(font.clone());
                ui.visuals_mut().override_text_color = Some(TEXT);
                egui::ScrollArea::vertical()
                    .id_salt(id.with("body"))
                    .auto_shrink([false, false])
                    .min_scrolled_height(1.0)
                    .max_height(content.height())
                    .show(ui, body);
            });
            let mut x = footer.left();
            for (index, (key, label, color)) in actions.iter().enumerate() {
                let rect = if stacked {
                    Rect::from_min_size(
                        egui::pos2(footer.left(), footer.top() + 28.0 * index as f32),
                        Vec2::new(footer.width(), 26.0),
                    )
                } else {
                    let rect = Rect::from_min_size(
                        egui::pos2(x, footer.top()),
                        Vec2::new(widths[index], 26.0),
                    );
                    x += widths[index] + 12.0;
                    rect
                };
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &format!("[{key}] "),
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: *color,
                        ..Default::default()
                    },
                );
                job.append(
                    label,
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: DIM,
                        ..Default::default()
                    },
                );
                clicked[index] = ui.put(rect, egui::Button::new(job).frame(false)).clicked();
                #[cfg(test)]
                geometry.actions.push(rect);
            }
            #[cfg(test)]
            {
                geometry.header = header;
                geometry.body = content;
                geometry.footer = footer;
            }
        });
    #[cfg(test)]
    {
        geometry.frame = frame;
        ctx.data_mut(|d| d.insert_temp(id.with("geometry"), geometry));
    }
    #[cfg(not(test))]
    let _ = frame;
    clicked
}

#[cfg(test)]
#[path = "notice_ui_tests.rs"]
mod tests;
