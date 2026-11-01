//! A result list the core shows for modules (backlinks, …): type to filter,
//! ↑↓ or the wheel to select, Enter or a double click opens the file at the
//! line, Esc closes.
use super::*;
use module_api::ResultItem;

#[derive(Default)]
pub(super) struct Results {
    pub(super) open: bool,
    title: String,
    items: Vec<ResultItem>,
    query: String,
    /// Indices of `items` matching `query`, best first.
    shown: Vec<usize>,
    /// Position in `shown`.
    selected: usize,
}

impl Results {
    pub(super) fn show(&mut self, title: String, items: Vec<ResultItem>) {
        self.open = true;
        self.title = title;
        self.items = items;
        self.query.clear();
        self.refilter();
    }

    fn refilter(&mut self) {
        let query = self.query.trim().to_lowercase();
        let mut scored: Vec<(usize, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if query.is_empty() {
                    return Some((0, index));
                }
                let text = format!("{} {}", item.label, item.detail).to_lowercase();
                commands::fuzzy_score(&text, &query).map(|score| (score, index))
            })
            .collect();
        scored.sort();
        self.shown = scored.into_iter().map(|(_, index)| index).collect();
        self.selected = 0;
    }

    fn selected_item(&self) -> Option<&ResultItem> {
        self.shown
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
    }

    fn step(&mut self, delta: isize) {
        if !self.shown.is_empty() {
            self.selected =
                (self.selected as isize + delta).clamp(0, self.shown.len() as isize - 1) as usize;
        }
    }
}

impl SlateApp {
    /// Keys for the open result list; true when it took them.
    pub(super) fn handle_results_keys(&mut self, ctx: &egui::Context) -> bool {
        if !self.results.open {
            return false;
        }
        let mut open = false;
        let mut changed = false;
        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, Key::Escape) {
                self.results.open = false;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                self.results.step(-1);
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                self.results.step(1);
            }
            if input.consume_key(egui::Modifiers::NONE, Key::PageUp) {
                self.results.step(-10);
            }
            if input.consume_key(egui::Modifiers::NONE, Key::PageDown) {
                self.results.step(10);
            }
            open |= input.consume_key(egui::Modifiers::NONE, Key::Enter);
            if input.consume_key(egui::Modifiers::NONE, Key::Backspace) {
                changed |= self.results.query.pop().is_some();
            }
            input.events.retain(|event| match event {
                egui::Event::Text(text) if !text.chars().any(char::is_control) => {
                    self.results.query.push_str(text);
                    changed = true;
                    false
                }
                _ => true,
            });
        });
        if changed {
            self.results.refilter();
        }
        if open {
            self.open_selected_result();
        }
        if !self.results.open {
            self.focus_editor_once = true;
        }
        true
    }

    fn open_selected_result(&mut self) {
        let Some(item) = self.results.selected_item().cloned() else {
            return;
        };
        self.results.open = false;
        self.open_module_target(item.path, item.line);
    }

    pub(super) fn results_dialog(&mut self, ctx: &egui::Context) {
        if !self.results.open {
            return;
        }
        let subtitle = if self.results.query.is_empty() {
            format!("{} results · type to filter", self.results.items.len())
        } else {
            format!(
                "{} of {} · {}",
                self.results.shown.len(),
                self.results.items.len(),
                self.results.query
            )
        };
        let title = self.results.title.clone();
        let mut double_clicked = false;
        let clicked = dialog_ui::show(
            ctx,
            "module_results_dialog".into(),
            &title,
            &subtitle,
            &[("Esc", "close"), ("↑↓", "select"), ("Enter", "open")],
            |ui| {
                let font = FontId::monospace(13.0);
                let list = ui.max_rect();
                let painter = ui.painter_at(list);
                painter.rect_filled(list, 0.0, Color32::from_rgb(22, 28, 37));
                let row_height = 24.0;
                let capacity = (list.height() / row_height).floor().max(1.0) as usize;
                if ctx
                    .pointer_hover_pos()
                    .is_some_and(|pos| list.contains(pos))
                {
                    let delta = ctx.input(|input| input.smooth_scroll_delta.y);
                    if delta.abs() > 0.5 {
                        self.results.step(if delta > 0.0 { -1 } else { 1 });
                    }
                }
                if self.results.shown.is_empty() {
                    painter.text(
                        list.center(),
                        egui::Align2::CENTER_CENTER,
                        if self.results.items.is_empty() {
                            "no results"
                        } else {
                            "nothing matches the filter"
                        },
                        font.clone(),
                        notice_ui::DIM,
                    );
                    return;
                }
                let start = file_dialogs::centered_window_start(
                    self.results.selected,
                    capacity,
                    self.results.shown.len(),
                );
                let end = (start + capacity).min(self.results.shown.len());
                let detail_width = if list.width() >= 480.0 {
                    (list.width() * 0.45).min(420.0)
                } else {
                    0.0
                };
                for (row, position) in (start..end).enumerate() {
                    let item = &self.results.items[self.results.shown[position]];
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(list.left() + 4.0, list.top() + row as f32 * row_height),
                        Vec2::new((list.width() - 8.0).max(0.0), row_height),
                    );
                    let selected = position == self.results.selected;
                    if selected {
                        painter.rect_filled(rect, 0.0, Color32::from_rgb(38, 47, 61));
                    }
                    let response = ui.interact(
                        rect,
                        ui.id().with(("result", position)),
                        egui::Sense::click(),
                    );
                    if response.clicked() {
                        self.results.selected = position;
                    }
                    if response.double_clicked() {
                        self.results.selected = position;
                        double_clicked = true;
                    }
                    painter.text(
                        egui::pos2(rect.left() + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        if selected { ">" } else { " " },
                        font.clone(),
                        notice_ui::ACCENT,
                    );
                    let label_right = rect.right() - detail_width - 12.0;
                    painter
                        .with_clip_rect(egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(label_right, rect.bottom()),
                        ))
                        .text(
                            egui::pos2(rect.left() + 28.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            Self::text_for_width(
                                &item.label,
                                label_right - rect.left() - 28.0,
                                13.0,
                            ),
                            font.clone(),
                            if selected {
                                notice_ui::TEXT
                            } else {
                                notice_ui::ACCENT
                            },
                        );
                    if detail_width > 0.0 {
                        painter.text(
                            egui::pos2(rect.right() - detail_width, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            Self::text_for_width(&item.detail, detail_width, 13.0),
                            font.clone(),
                            notice_ui::DIM,
                        );
                    }
                }
            },
        );
        if clicked.first().copied().unwrap_or(false) {
            self.results.open = false;
            self.focus_editor_once = true;
        }
        if double_clicked || clicked.get(2).copied().unwrap_or(false) {
            self.open_selected_result();
        }
    }
}
