//! Fixed dialog chrome for forms and embedded editors; the body owns its scrolling.
use super::*;
#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct Geometry {
    pub frame: egui::Rect,
    pub header: egui::Rect,
    pub body: egui::Rect,
    pub footer: egui::Rect,
    pub actions: Vec<egui::Rect>,
}
fn positions(widths: &[f32], width: f32) -> (Vec<(f32, usize)>, usize) {
    let mut result = Vec::new();
    let mut x = 0.0;
    let mut row = 0;
    for item in widths {
        if x > 0.0 && x + item > width {
            row += 1;
            x = 0.0;
        }
        result.push((x, row));
        x += item + 8.0;
    }
    (result, row + 1)
}
pub(super) fn show(
    ctx: &egui::Context,
    id: egui::Id,
    title: &str,
    subtitle: &str,
    actions: &[(&str, &str)],
    body: impl FnOnce(&mut egui::Ui),
) -> Vec<bool> {
    let mut clicked = vec![false; actions.len()];
    let font = FontId::monospace(13.0);
    #[cfg(test)]
    let mut geometry = Geometry {
        frame: egui::Rect::NOTHING,
        header: egui::Rect::NOTHING,
        body: egui::Rect::NOTHING,
        footer: egui::Rect::NOTHING,
        actions: Vec::new(),
    };
    let frame = window_ui::DialogArea::new(id)
        .fixed_chrome()
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let panel = ui.max_rect();
            let inner = panel.shrink(14.0);
            let width = inner.width().max(1.0);
            let measure = |text: String| {
                ctx.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(text, font.clone(), notice_ui::DIM)
                        .size()
                        .x
                        + 12.0
                })
            };
            let mut compact = false;
            let mut widths: Vec<f32> = actions
                .iter()
                .map(|(key, label)| measure(format!("[{key}] {label}")).min(width))
                .collect();
            let (_, rows) = positions(&widths, width);
            if rows > 2 {
                compact = true;
                widths = actions
                    .iter()
                    .map(|(key, _)| measure(format!("[{key}]")).min(width))
                    .collect();
            }
            let (slots, rows) = positions(&widths, width);
            let footer_height = rows as f32 * 28.0;
            let header = egui::Rect::from_min_size(inner.min, Vec2::new(width, 48.0));
            let footer = egui::Rect::from_min_max(
                egui::pos2(inner.left(), inner.bottom() - footer_height),
                inner.max,
            );
            let content = egui::Rect::from_min_max(
                egui::pos2(inner.left(), header.bottom() + 8.0),
                egui::pos2(
                    inner.right(),
                    (footer.top() - 8.0).max(header.bottom() + 9.0),
                ),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(header), |ui| {
                ui.set_clip_rect(header.intersect(ui.clip_rect()));
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(FontId::monospace(16.0))
                            .color(notice_ui::ACCENT),
                    )
                    .truncate(),
                )
                .on_hover_ui(|ui| {
                    footer_ui::style_hints(ui);
                    ui.label(title);
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(subtitle)
                            .font(font.clone())
                            .color(notice_ui::DIM),
                    )
                    .truncate(),
                )
                .on_hover_ui(|ui| {
                    footer_ui::style_hints(ui);
                    ui.label(subtitle);
                });
            });
            ui.scope_builder(
                egui::UiBuilder::new().id_salt("body").max_rect(content),
                |ui| {
                    ui.set_clip_rect(content.intersect(ui.clip_rect()));
                    ui.style_mut().override_font_id = Some(font.clone());
                    ui.visuals_mut().override_text_color = Some(notice_ui::TEXT);
                    body(ui);
                },
            );
            for (index, ((key, label), (x, row))) in actions.iter().zip(slots).enumerate() {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(footer.left() + x, footer.top() + row as f32 * 28.0),
                    Vec2::new(widths[index], 26.0),
                );
                let mut job = LayoutJob::default();
                job.append(
                    &format!("[{key}]"),
                    0.0,
                    TextFormat::simple(font.clone(), notice_ui::WARN),
                );
                if !compact {
                    job.append(
                        &format!(" {label}"),
                        0.0,
                        TextFormat::simple(font.clone(), notice_ui::DIM),
                    );
                }
                clicked[index] = ui
                    .put(rect, egui::Button::new(job).frame(false))
                    .on_hover_ui(|ui| {
                        footer_ui::style_hints(ui);
                        ui.label(format!("[{key}] {label}"));
                    })
                    .clicked();
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
        ctx.data_mut(|data| data.insert_temp(id.with("geometry"), geometry));
    }
    #[cfg(not(test))]
    let _ = frame;
    clicked
}
