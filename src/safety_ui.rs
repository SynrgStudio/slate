use super::*;
use document_safety::{Event, Slot};
impl SlateApp {
    pub(super) fn clear_recovery(&mut self, slot: Slot) {
        self.safety_notified.remove(&slot);
        let origin = self.recovery_origins.remove(&slot);
        self.document_safety.clear(slot, origin);
    }
    pub(super) fn observe_document_safety(&mut self, now: f64, force: bool) {
        self.document_safety.observe(
            Slot::Main,
            self.path.as_deref(),
            self.disk_base,
            self.buffer.as_str(),
            self.buffer.revision,
            self.buffer.cursor(),
            self.dirty,
            now,
            force,
            self.recovery_origins.get(&Slot::Main).cloned(),
        );
        self.document_safety.observe(
            Slot::Modal,
            self.link_preview_path.as_deref(),
            self.modal_disk_base,
            self.link_preview_buffer.as_str(),
            self.link_preview_buffer.revision,
            self.link_preview_buffer.cursor(),
            self.link_preview_dirty,
            now,
            force,
            self.recovery_origins.get(&Slot::Modal).cloned(),
        );
        self.document_safety.observe(
            Slot::Scratch,
            None,
            None,
            self.scratch_buffer.as_str(),
            self.scratch_buffer.revision,
            self.scratch_buffer.cursor(),
            !self.scratch_buffer.as_str().is_empty() && !self.discard_on_exit,
            now,
            force,
            self.recovery_origins.get(&Slot::Scratch).cloned(),
        );
    }
    pub(super) fn poll_document_safety(&mut self, ctx: &egui::Context) {
        self.observe_document_safety(ctx.input(|i| i.time), false);
        while let Some(event) = self.document_safety.poll() {
            match event {
                Event::Found(records) => {
                    self.recovery_records.extend(records);
                    self.recovery_open = true;
                    self.recovery_selected = 0;
                }
                Event::Changed {
                    slot,
                    path,
                    expected,
                } => {
                    let matches = match slot {
                        Slot::Main => {
                            self.path.as_ref() == Some(&path) && self.disk_base == expected
                        }
                        Slot::Modal => {
                            self.link_preview_path.as_ref() == Some(&path)
                                && self.modal_disk_base == expected
                        }
                        Slot::Scratch | Slot::Tab(_) => false,
                    };
                    if matches
                        && !self.save_as_open
                        && self.file_conflict.is_none()
                        && self.safety_notified.get(&slot) != Some(&(path.clone(), expected))
                    {
                        self.safety_notified.insert(slot, (path.clone(), expected));
                        self.file_conflict = Some((slot, path));
                        self.status = "External change detected; current text preserved".into();
                    }
                }
                Event::Error(error) => {
                    perf_log::mark("document.safety.failed", &error);
                    self.status = error;
                }
                Event::CheckpointFailed(_) => {}
            }
        }
    }
    pub(super) fn restore_selected_recovery(&mut self) {
        if self.dirty || self.link_preview_dirty || !self.scratch_buffer.as_str().is_empty() {
            self.status = "Save current buffers before restoring another draft".into();
            return;
        }
        let Some(record) = self.recovery_records.get(self.recovery_selected).cloned() else {
            return;
        };
        self.cancel_file_open();
        self.clear_recovery(Slot::Main);
        self.buffer.set_text(record.draft.text);
        self.buffer.set_cursor(record.draft.cursor);
        self.path = record.draft.path;
        self.disk_base = record.draft.base;
        self.dirty = true;
        self.scratch = false;
        self.search_state = None;
        self.file_conflict = None;
        self.recovery_origins.insert(Slot::Main, record.record);
        self.recovery_open = false;
        self.focus_editor_once = true;
        self.editor_view.request_scroll_to_cursor(&self.buffer);
        self.schedule_current_flow_buffer_agenda_sync_later();
        self.observe_document_safety(0.0, true); // Durable new checkpoint precedes retiring original.
        self.status = "Recovered unsaved text; source file not overwritten".into();
    }
    pub(super) fn document_safety_dialog(&mut self, ctx: &egui::Context) {
        if self.recovery_open {
            let mut restore = false;
            let mut skip = false;
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                    self.recovery_selected = self.recovery_selected.saturating_sub(1);
                }
                if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                    self.recovery_selected = (self.recovery_selected + 1)
                        .min(self.recovery_records.len().saturating_sub(1));
                }
                restore = i.consume_key(egui::Modifiers::NONE, Key::Enter);
                skip = i.consume_key(egui::Modifiers::NONE, Key::Escape);
            });
            let reveal = ctx.data(|d| d.get_temp::<usize>("recovery_selection".into()))
                != Some(self.recovery_selected);
            let clicked = notice_ui::show(
                ctx,
                "recovery_dialog".into(),
                "recover unsaved text",
                &[
                    ("enter", "restore", notice_ui::WARN),
                    ("esc", "later", notice_ui::GREEN),
                ],
                |ui| {
                    ui.label(RichText::new("Drafts from interrupted sessions. Restoring never overwrites the original file.").color(notice_ui::DIM));
                    ui.add_space(10.0);
                    for (index, record) in self.recovery_records.iter().enumerate() {
                        let name = record
                            .draft
                            .path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "untitled".into());
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 28.0),
                            egui::Sense::click(),
                        );
                        let selected = self.recovery_selected == index;
                        if selected || response.hovered() {
                            ui.painter()
                                .rect_filled(rect, 0.0, Color32::from_rgb(39, 52, 67));
                        }
                        let metadata = format!(
                            "{:?} · {} bytes",
                            record.draft.slot,
                            record.draft.text.len()
                        );
                        let meta_width =
                            (metadata.len() as f32 * 7.2 + 16.0).min(rect.width() * 0.48);
                        let name_rect = egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.right() - meta_width, rect.bottom()),
                        );
                        ui.painter_at(name_rect).text(
                            rect.left_center() + Vec2::new(8.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            &name,
                            FontId::new(14.0, FontFamily::Monospace),
                            if selected {
                                notice_ui::ACCENT
                            } else {
                                notice_ui::TEXT
                            },
                        );
                        let meta_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.right() - meta_width, rect.top()),
                            rect.max,
                        );
                        ui.painter_at(meta_rect).text(
                            rect.right_center() - Vec2::new(8.0, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            metadata,
                            FontId::new(12.0, FontFamily::Monospace),
                            notice_ui::DIM,
                        );
                        if response.clicked() {
                            self.recovery_selected = index;
                        }
                        if reveal && self.recovery_selected == index {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        #[cfg(test)]
                        if self.recovery_selected == index {
                            ctx.data_mut(|d| d.insert_temp("recovery_selected_rect".into(), rect));
                        }
                        response.on_hover_text(name);
                    }
                    if self.status.starts_with("Save current buffers") {
                        ui.add_space(8.0);
                        ui.label(RichText::new(&self.status).color(notice_ui::WARN));
                    }
                },
            );
            restore |= clicked[0];
            skip |= clicked[1];
            ctx.data_mut(|d| d.insert_temp("recovery_selection".into(), self.recovery_selected));
            if skip {
                self.recovery_open = false;
            } else if restore {
                self.restore_selected_recovery();
            }
            return;
        }
        let Some((slot, path)) = self.file_conflict.clone() else {
            return;
        };
        let mut keep = false;
        let mut copy = false;
        let mut reload = false;
        ctx.input_mut(|i| {
            keep = i.consume_key(egui::Modifiers::NONE, Key::Escape);
            copy = i.consume_key(egui::Modifiers::NONE, Key::C);
            reload = i.consume_key(egui::Modifiers::NONE, Key::R);
        });
        let clicked = notice_ui::show(
            ctx,
            "external_change_dialog".into(),
            "file changed / existing destination",
            &[
                ("c", "save copy", notice_ui::WARN),
                ("r", "reload", notice_ui::ACCENT),
                ("esc", "keep text", notice_ui::GREEN),
            ],
            |ui| {
                ui.label(RichText::new(path.display().to_string()).color(notice_ui::ACCENT));
                ui.add_space(10.0);
                ui.label(
                    "Your text is preserved. Slate will not silently overwrite external changes.",
                );
                ui.add_space(8.0);
                ui.label(RichText::new("Save a copy under a new name, or reload after confirming any unsaved changes.").color(notice_ui::DIM));
            },
        );
        copy |= clicked[0];
        reload |= clicked[1];
        keep |= clicked[2];
        if keep {
            self.file_conflict = None;
            self.status = "External change remains; save protection stays active".into();
        }
        if copy {
            if slot == Slot::Main {
                self.file_conflict = None;
                self.save_as();
            } else if !self.dirty {
                self.file_conflict = None;
                // Copy the visible modal text, even when it was clean before the external edit.
                self.link_preview_dirty = true;
                self.commit_link_preview();
                self.save_as();
            } else {
                self.status = "Save the main buffer first; modal text is still preserved".into();
            }
        }
        if reload {
            if slot == Slot::Main {
                if self.path.as_ref() != Some(&path) {
                    self.status = "Existing Save As destination kept; choose a new filename".into();
                    self.file_conflict = None;
                    self.save_as();
                } else {
                    self.file_conflict = None;
                    if self.dirty {
                        self.pending_project_file_path = Some(path);
                        self.confirm(PendingAction::OpenProjectFile);
                    } else {
                        self.open_path(path);
                    }
                }
            } else if self.link_preview_dirty {
                self.status = "Save a modal copy before reloading".into();
            } else {
                self.file_conflict = None;
                self.open_link_preview(path, None);
            }
        }
    }
}
