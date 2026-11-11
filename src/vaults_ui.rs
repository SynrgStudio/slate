//! `:vaults`: the vaults at a glance. Each row shows a vault's name, its
//! folder and whether the open note is in it; add a folder, stop using one
//! (its files are never touched), browse its notes or open it in Explorer.
use super::*;

#[derive(Default)]
pub(super) struct VaultsDialog {
    pub(super) open: bool,
    pub(super) selected: usize,
    /// The folder picker was opened from here: come back when it closes.
    pub(super) return_after_pick: bool,
}

/// Footer actions, in the order `dialog_ui::show` reports clicks.
const ACTIONS: &[(&str, &str)] = &[
    ("Esc", "close"),
    ("↑↓", "select"),
    ("Enter", "browse notes"),
    ("A", "add folder"),
    ("Del", "remove from Slate"),
    ("E", "open in Explorer"),
];

impl SlateApp {
    pub(super) fn open_vaults_dialog(&mut self) {
        self.vaults_dialog.open = true;
        self.vaults_dialog.selected = self
            .path
            .as_deref()
            .and_then(|path| vaults::declared(&self.vaults, path))
            .and_then(|vault| self.vaults.iter().position(|v| *v == vault))
            .unwrap_or(0);
        self.focus_editor_once = false;
    }

    /// Keys while `:vaults` is open; true when it took them.
    pub(super) fn handle_vaults_keys(&mut self, ctx: &egui::Context) -> bool {
        if !self.vaults_dialog.open {
            return false;
        }
        let (mut close, mut browse, mut add, mut remove, mut explore) =
            (false, false, false, false, false);
        let mut step = 0isize;
        ctx.input_mut(|input| {
            close |= input.consume_key(egui::Modifiers::NONE, Key::Escape);
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                step -= 1;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                step += 1;
            }
            browse |= input.consume_key(egui::Modifiers::NONE, Key::Enter);
            add |= input.consume_key(egui::Modifiers::NONE, Key::A);
            remove |= input.consume_key(egui::Modifiers::NONE, Key::Delete);
            explore |= input.consume_key(egui::Modifiers::NONE, Key::E);
            // Letters typed here are not text for the editor below.
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Text(_)));
        });
        self.vaults_step(step);
        if close {
            self.vaults_dialog.open = false;
            self.focus_editor_once = true;
        } else if browse {
            self.browse_selected_vault();
        } else if add {
            self.add_vault_from_dialog();
        } else if remove {
            self.remove_selected_vault();
        } else if explore {
            self.explore_selected_vault();
        }
        true
    }

    fn vaults_step(&mut self, step: isize) {
        if step != 0 && !self.vaults.is_empty() {
            self.vaults_dialog.selected = (self.vaults_dialog.selected as isize + step)
                .clamp(0, self.vaults.len() as isize - 1)
                as usize;
        }
    }

    fn browse_selected_vault(&mut self) {
        let Some(vault) = self.vaults.get(self.vaults_dialog.selected).cloned() else {
            return;
        };
        self.vaults_dialog.open = false;
        self.open_file_picker_at(vault, FilePickerMode::Open);
    }

    fn add_vault_from_dialog(&mut self) {
        self.vaults_dialog.open = false;
        self.vaults_dialog.return_after_pick = true;
        self.open_vault_picker();
    }

    fn remove_selected_vault(&mut self) {
        let index = self.vaults_dialog.selected;
        self.remove_vault(index);
        if self.vaults_dialog.selected >= self.vaults.len() {
            self.vaults_dialog.selected = self.vaults.len().saturating_sub(1);
        }
    }

    fn explore_selected_vault(&mut self) {
        let Some(vault) = self.vaults.get(self.vaults_dialog.selected) else {
            return;
        };
        #[cfg(all(windows, not(test)))]
        let opened = std::process::Command::new("explorer.exe").arg(vault).spawn().is_ok();
        #[cfg(not(all(windows, not(test))))]
        let opened = false;
        self.status = if opened {
            format!("Opened {} in Explorer", vault.display())
        } else {
            format!("Could not open {}", vault.display())
        };
    }

    pub(super) fn vaults_dialog(&mut self, ctx: &egui::Context) {
        // Back from the folder picker that `A` opened.
        if self.vaults_dialog.return_after_pick && !self.file_picker_open {
            self.vaults_dialog.return_after_pick = false;
            self.vaults_dialog.open = true;
            if let Some(last) = self.vaults.len().checked_sub(1) {
                self.vaults_dialog.selected = last;
            }
        }
        if !self.vaults_dialog.open {
            return;
        }
        let here = self
            .path
            .as_deref()
            .and_then(|path| vaults::declared(&self.vaults, path));
        let subtitle = match self.vaults.len() {
            0 => "no vaults yet · A adds a folder of notes".to_string(),
            1 => "1 vault · removing one never touches its files".to_string(),
            n => format!("{n} vaults · removing one never touches its files"),
        };
        let mut clicked_row = None;
        let mut double_clicked = false;
        let mut remove_row = None;
        let clicked = dialog_ui::show(
            ctx,
            "vaults_dialog".into(),
            "vaults",
            &subtitle,
            ACTIONS,
            |ui| {
                let list = ui.max_rect();
                ui.painter()
                    .rect_filled(list, 0.0, Color32::from_rgb(22, 28, 37));
                if self.vaults.is_empty() {
                    ui.painter().text(
                        list.center(),
                        egui::Align2::CENTER_CENTER,
                        "Vaults are your note folders: Flow, backlinks and mentions look across them.\nPress A to add one.",
                        FontId::monospace(13.0),
                        notice_ui::DIM,
                    );
                    return;
                }
                let row_height = 46.0;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, vault) in self.vaults.iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), row_height),
                                egui::Sense::click(),
                            );
                            let selected = index == self.vaults_dialog.selected;
                            if selected {
                                ui.painter()
                                    .rect_filled(rect, 0.0, Color32::from_rgb(38, 47, 61));
                                ui.scroll_to_rect(rect, None);
                            }
                            if response.clicked() {
                                clicked_row = Some(index);
                            }
                            if response.double_clicked() {
                                double_clicked = true;
                            }
                            let name = vault
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| vault.display().to_string());
                            let painter = ui.painter_at(rect);
                            painter.text(
                                egui::pos2(rect.left() + 10.0, rect.top() + 14.0),
                                egui::Align2::LEFT_CENTER,
                                if selected { ">" } else { " " },
                                FontId::monospace(14.0),
                                notice_ui::ACCENT,
                            );
                            let title = painter.text(
                                egui::pos2(rect.left() + 28.0, rect.top() + 14.0),
                                egui::Align2::LEFT_CENTER,
                                &name,
                                FontId::monospace(14.0),
                                notice_ui::TEXT,
                            );
                            if here.as_ref() == Some(vault) {
                                painter.text(
                                    egui::pos2(title.right() + 12.0, rect.top() + 14.0),
                                    egui::Align2::LEFT_CENTER,
                                    "● this note",
                                    FontId::monospace(12.0),
                                    Color32::from_rgb(163, 190, 140),
                                );
                            }
                            let (detail, color) = if vault.is_dir() {
                                (vault.display().to_string(), notice_ui::DIM)
                            } else {
                                (
                                    format!("{} · folder not found", vault.display()),
                                    Color32::from_rgb(191, 97, 106),
                                )
                            };
                            painter.text(
                                egui::pos2(rect.left() + 28.0, rect.top() + 33.0),
                                egui::Align2::LEFT_CENTER,
                                Self::text_for_width(&detail, rect.width() - 80.0, 12.0),
                                FontId::monospace(12.0),
                                color,
                            );
                            let remove = egui::Rect::from_center_size(
                                egui::pos2(rect.right() - 22.0, rect.center().y),
                                Vec2::splat(22.0),
                            );
                            if ui
                                .put(remove, egui::Button::new("✕").small())
                                .on_hover_text("Stop using this folder as a vault (files are not touched)")
                                .clicked()
                            {
                                remove_row = Some(index);
                            }
                        }
                    });
            },
        );
        if let Some(index) = clicked_row {
            self.vaults_dialog.selected = index;
        }
        if let Some(index) = remove_row {
            self.vaults_dialog.selected = index;
            self.remove_selected_vault();
            return;
        }
        let action = |n: usize| clicked.get(n).copied().unwrap_or(false);
        if action(0) {
            self.vaults_dialog.open = false;
            self.focus_editor_once = true;
        } else if double_clicked || action(2) {
            self.browse_selected_vault();
        } else if action(3) {
            self.add_vault_from_dialog();
        } else if action(4) {
            self.remove_selected_vault();
        } else if action(5) {
            self.explore_selected_vault();
        }
    }
}
