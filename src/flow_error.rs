//! Cache errors remain readable in a small modal; embedded SQLite needs no installer.
use eframe::egui::{self, Color32, FontFamily, FontId, Key};
#[derive(Clone)]
struct State {
    error: String,
    shown: bool,
}
fn id() -> egui::Id {
    egui::Id::new("flow_cache_error_state")
}
fn state(ctx: &egui::Context) -> Option<State> {
    ctx.data(|d| d.get_temp(id()))
}
fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<State>(id()));
}
pub fn is_open(ctx: &egui::Context) -> bool {
    state(ctx).is_some()
}
pub fn offer(ctx: &egui::Context, error: &str, manual: bool) {
    let offered = egui::Id::new("flow_error_offered_this_session");
    if !manual && ctx.data(|d| d.get_temp::<bool>(offered).unwrap_or(false)) {
        return;
    }
    ctx.data_mut(|d| {
        d.insert_temp(offered, true);
        d.insert_temp(
            id(),
            State {
                error: error.to_string(),
                shown: false,
            },
        );
    });
    ctx.request_repaint();
}
pub fn handle_input(ctx: &egui::Context) -> bool {
    let Some(state) = state(ctx) else {
        return false;
    };
    let dismiss = ctx.input_mut(|input| {
        let escape = input.consume_key(egui::Modifiers::NONE, Key::Escape);
        let enter = input.consume_key(egui::Modifiers::NONE, Key::Enter);
        escape || (state.shown && enter)
    });
    if dismiss {
        close(ctx);
    }
    true
}
pub fn show(ctx: &egui::Context) {
    let Some(mut state) = state(ctx) else {
        return;
    };
    let mut dismiss = false;
    let response = egui::Modal::new("flow_cache_error_modal".into())
        .frame(egui::Frame::new().fill(Color32::from_rgb(25, 31, 40))
            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)))
            .corner_radius(0.0).inner_margin(14.0))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() * 0.8 - 30.0).clamp(1.0, 420.0));
            ui.label(egui::RichText::new("Flow cache error").font(FontId::new(16.0, FontFamily::Monospace)).color(Color32::from_rgb(136, 192, 208)));
            egui::ScrollArea::vertical().min_scrolled_height(0.0)
                .max_height(((ctx.content_rect().height() - 30.0) * 0.8 - 90.0).max(20.0))
                .show(ui, |ui| {
                    ui.add(egui::Label::new(&state.error).wrap());
                    ui.add(egui::Label::new("SQLite viene incluido en Slate. No necesitas instalar el CLI ni modificar el PATH. Tus textos siguen siendo la fuente de verdad.").wrap());
                });
            dismiss = ui.button("[Enter / Esc] Close").clicked();
        });
    if dismiss || response.should_close() {
        close(ctx);
    } else {
        state.shown = true;
        ctx.data_mut(|d| d.insert_temp(id(), state));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_error_fits_small_window_and_never_offers_installation() {
        let ctx = egui::Context::default();
        offer(&ctx, "SQLite cache is locked by another process", false);
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(420.0, 260.0),
                    )),
                    ..Default::default()
                },
                show,
            );
            assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.contains("Instalar SQLite"))));
        }
        let rect = ctx
            .memory(|m| m.area_rect("flow_cache_error_modal"))
            .unwrap();
        assert!(rect.width() <= 337.0 && rect.height() <= 208.0, "{rect:?}");
    }
}
