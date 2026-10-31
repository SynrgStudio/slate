//! Keyboard-first drive selector; no permanent controls in the file picker.
use eframe::egui::{self, Color32, FontFamily, FontId, Key, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct State {
    roots: Vec<PathBuf>,
    selected: usize,
    reveal_selection: bool,
}
fn id() -> egui::Id {
    egui::Id::new("slate_drive_picker_state")
}
fn state(ctx: &egui::Context) -> Option<State> {
    ctx.data(|data| data.get_temp::<State>(id()))
}
fn close(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<State>(id()));
}
pub fn is_open(ctx: &egui::Context) -> bool {
    state(ctx).is_some()
}
pub fn open(ctx: &egui::Context, current: &Path) {
    open_roots(ctx, current, crate::file_dialogs::available_roots());
}
fn open_roots(ctx: &egui::Context, current: &Path, roots: Vec<PathBuf>) {
    let selected = roots
        .iter()
        .position(|root| current.starts_with(root))
        .unwrap_or(0);
    ctx.data_mut(|data| {
        data.insert_temp(
            id(),
            State {
                roots,
                selected,
                reveal_selection: true,
            },
        )
    });
    ctx.request_repaint();
}

/// Consume input before the underlying file picker can use it.
pub fn handle_input(ctx: &egui::Context) -> Option<PathBuf> {
    let mut state = state(ctx)?;
    let (cancel, up, down, choose) = ctx.input_mut(|input| {
        (
            input.consume_key(egui::Modifiers::NONE, Key::Escape),
            input.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            input.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            input.consume_key(egui::Modifiers::NONE, Key::Enter)
                | input.consume_key(egui::Modifiers::NONE, Key::ArrowRight),
        )
    });
    if cancel {
        close(ctx);
        return None;
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    if down {
        state.selected = (state.selected + 1).min(state.roots.len().saturating_sub(1));
    }
    if up || down {
        state.reveal_selection = true;
    }
    if choose {
        let chosen = state.roots.get(state.selected).cloned();
        close(ctx);
        return chosen;
    }
    ctx.data_mut(|data| data.insert_temp(id(), state));
    None
}

pub fn show(ctx: &egui::Context) -> Option<PathBuf> {
    let mut state = state(ctx)?;
    let mut chosen = None;
    let reveal_selection = state.reveal_selection;
    state.reveal_selection = false;
    let font = FontId::new(13.0, FontFamily::Monospace);
    let accent = Color32::from_rgb(136, 192, 208);
    let dim = Color32::from_rgb(136, 154, 176);
    let response = egui::Modal::new("slate_drive_picker".into())
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(25, 31, 40))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)))
                .corner_radius(0.0)
                .inner_margin(14.0),
        )
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() * 0.8 - 30.0).clamp(1.0, 300.0));
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(
                egui::RichText::new("drives")
                    .font(FontId::new(16.0, FontFamily::Monospace))
                    .color(accent),
            );
            ui.add_space(6.0);
            let max_height = ((ctx.content_rect().height() - 30.0) * 0.8 - 90.0).max(28.0);
            egui::ScrollArea::vertical()
                .min_scrolled_height(0.0)
                .max_height(max_height)
                .show(ui, |ui| {
                    if state.roots.is_empty() {
                        ui.label(
                            egui::RichText::new("no available drives")
                                .font(font.clone())
                                .color(dim),
                        );
                    }
                    for (index, root) in state.roots.iter().enumerate() {
                        let selected = index == state.selected;
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 28.0),
                            egui::Sense::click(),
                        );
                        if selected {
                            ui.painter()
                                .rect_filled(rect, 0.0, Color32::from_rgb(38, 47, 61));
                            if reveal_selection {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                        ui.painter_at(rect).text(
                            rect.left_center() + Vec2::new(8.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("{}  {}", if selected { ">" } else { " " }, root.display()),
                            font.clone(),
                            if selected {
                                Color32::from_rgb(216, 222, 233)
                            } else {
                                accent
                            },
                        );
                        if response.clicked() {
                            state.selected = index;
                            state.reveal_selection = true;
                        }
                        if response.double_clicked() {
                            chosen = Some(root.clone());
                        }
                    }
                });
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new("[↑↓] select  [Enter/→] open  [Esc] back")
                        .font(FontId::new(11.0, FontFamily::Monospace))
                        .color(dim),
                )
                .truncate(),
            );
        });
    if chosen.is_some() || response.should_close() {
        close(ctx);
    } else {
        ctx.data_mut(|data| data.insert_temp(id(), state));
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(ctx: &egui::Context, key: Key) -> Option<PathBuf> {
        let mut chosen = None;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                chosen = handle_input(ctx);
            },
        );
        chosen
    }
    #[test]
    fn drive_selection_and_cancel_are_keyboard_only() {
        let ctx = egui::Context::default();
        let roots = vec![PathBuf::from("C:\\"), PathBuf::from("D:\\")];
        open_roots(&ctx, Path::new("D:\\"), roots.clone());
        assert_eq!(state(&ctx).unwrap().selected, 1);
        assert_eq!(key(&ctx, Key::ArrowUp), None);
        assert_eq!(key(&ctx, Key::Enter), Some(roots[0].clone()));
        assert!(!is_open(&ctx));
        open_roots(&ctx, Path::new("C:\\"), roots.clone());
        key(&ctx, Key::ArrowDown);
        assert_eq!(key(&ctx, Key::ArrowRight), Some(roots[1].clone()));
        open_roots(&ctx, Path::new("D:\\"), roots);
        assert_eq!(key(&ctx, Key::Escape), None);
        assert!(!is_open(&ctx));
    }
}
