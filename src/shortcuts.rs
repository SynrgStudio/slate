//! Global keyboard shortcuts and the Ctrl/Alt hold layers.
use super::*;

impl SlateApp {
    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.periscope_setup.visible {
            self.handle_periscope_setup_input(ctx);
            return;
        }
        if flow_error::handle_input(ctx) {
            self.suppress_editor_keyboard_once = true;
            return;
        }
        if self.recovery_open || self.file_conflict.is_some() {
            return;
        }
        if self.file_picker_open {
            if drive_picker::is_open(ctx) {
                if let Some(dir) = drive_picker::handle_input(ctx) {
                    self.file_picker_dir = dir;
                    self.file_query.clear();
                    self.refresh_file_picker_entries();
                }
                return;
            }
            let choose_drive = ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL, Key::L)
                    || (self.file_picker_dir.parent().is_none()
                        && input.consume_key(egui::Modifiers::NONE, Key::ArrowLeft))
            });
            if choose_drive {
                drive_picker::open(ctx, &self.file_picker_dir);
                return;
            }
        }
        if self.scratch_close_after.is_some() {
            return;
        }

        if self.handle_agenda_org_keyboard(ctx) {
            return;
        }

        if self.handle_link_inline_keyboard(ctx) {
            return;
        }

        if self.flow_item_edit.is_some() {
            return;
        }

        if self.handle_duplicate_placement(ctx) {
            return;
        }

        if self.handle_shift_alt_layer(ctx) {
            return;
        }

        if self.handle_alt_layer(ctx) {
            return;
        }

        if self.handle_immediate_ctrl_edit(ctx) {
            return;
        }

        if self.handle_ctrl_layer(ctx) {
            return;
        }

        let mut command = None;
        let mut execute_command_line = false;
        let mut complete_command_line = false;
        let mut previous_command = false;
        let mut next_command = false;
        let mut settings_decrement = false;
        let mut settings_increment = false;
        let mut settings_previous = false;
        let mut settings_next = false;
        let mut settings_activate = false;
        let mut recent_previous = false;
        let mut recent_next = false;
        let mut recent_open = false;
        let mut recent_backspace = false;
        let mut lance_previous = false;
        let mut lance_next = false;
        let mut lance_open = false;
        let mut lance_open_modal = false;
        let mut lance_add = false;
        let mut lance_delete = false;
        let mut lance_backspace = false;
        let mut doc_task_previous = false;
        let mut doc_task_next = false;
        let mut doc_task_open = false;
        let mut doc_task_cycle = false;
        let mut doc_task_backspace = false;
        let mut agenda_previous = false;
        let mut agenda_next = false;
        let mut agenda_open = false;
        let mut agenda_cycle = false;
        let mut agenda_edit = false;
        let mut agenda_refile = false;
        let mut agenda_previous_day = false;
        let mut agenda_next_day = false;
        let mut agenda_previous_week = false;
        let mut agenda_next_week = false;
        let mut agenda_today = false;
        let mut agenda_toggle_focus = false;
        let mut agenda_cycle_tab = false;
        let mut agenda_toggle_done = false;
        let mut agenda_org_tag = false;
        let mut agenda_org_project = false;
        let mut file_previous = false;
        let mut file_next = false;
        let mut file_open = false;
        let mut file_assign_current = false;
        let mut file_enter_dir = false;
        let mut file_parent = false;
        let mut file_backspace = false;
        let mut periscope_previous = false;
        let mut periscope_next = false;
        let mut periscope_open = false;
        let mut periscope_open_modal = false;
        let mut periscope_toggle_mode = false;
        let mut periscope_expand_folder = false;
        let mut periscope_backspace = false;
        let mut find_note_previous = false;
        let mut find_note_next = false;
        let mut find_note_open = false;
        let mut find_note_open_modal = false;
        let mut find_note_toggle = false;
        let mut find_note_backspace = false;
        let mut find_note_cancel = false;
        let mut periscope_prompt_yes = false;
        let mut periscope_prompt_no = false;
        let mut save_as_previous = false;
        let mut save_as_next = false;
        let mut save_as_enter = false;
        let mut save_as_enter_dir = false;
        let mut save_as_parent = false;
        let mut save_as_backspace = false;
        let mut link_assist_previous = false;
        let mut link_assist_next = false;
        let mut link_assist_confirm = false;
        let mut link_assist_cancel = false;
        let mut link_web_confirm = false;
        let mut link_web_backspace = false;
        let mut link_web_cancel = false;
        let mut link_heading_previous = false;
        let mut link_heading_next = false;
        let mut link_heading_confirm = false;
        let mut link_heading_cancel = false;
        let mut link_preview_confirm = false;
        let mut link_preview_cancel = false;
        let mut link_preview_save = false;
        let mut link_preview_swap = false;
        let mut scratch_archive = false;
        let mut scratch_open_entries = false;
        let mut scratch_entries_previous = false;
        let mut scratch_entries_next = false;
        let mut scratch_entries_delete = false;
        let mut capture_confirm = false;
        let mut search_next = false;
        let mut search_previous = false;
        let mut search_accept = false;
        let mut search_cancel = false;
        let mut search_cursor_after = false;
        let mut search_cursor_before = false;
        let mut command_line_backspace = false;
        let mut command_line_delete = false;
        let mut command_line_left = false;
        let mut command_line_right = false;
        let mut command_line_home = false;
        let mut command_line_end = false;
        let search_active = self.search_state.is_some()
            && !self.command_line_focused
            && !self.focus_command_line_once
            && !self.palette_open
            && !self.settings_open
            && !self.recent_picker_open
            && !self.lance_open
            && !self.doc_tasks_open
            && !self.agenda_open
            && !self.file_picker_open
            && !self.find_note_open
            && !self.link_heading_picker_open
            && !self.link_preview_open
            && !self.save_as_open
            && !self.scratch_modal_open
            && !self.scratch_entries_open
            && !self.capture_modal_open
            && self.pending_action.is_none();
        ctx.input_mut(|i| {
            if self.settings_open {
                settings_decrement |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                settings_increment |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                settings_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                settings_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                settings_activate |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                settings_activate |= i.consume_key(egui::Modifiers::NONE, Key::Space);
            }
            if self.recent_picker_open {
                recent_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                recent_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                recent_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                recent_open |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                recent_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.lance_open {
                lance_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                lance_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                lance_open_modal |= i.consume_key(egui::Modifiers::CTRL, Key::Enter);
                lance_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                lance_add |= i.consume_key(egui::Modifiers::CTRL, Key::A);
                lance_delete |= i.consume_key(egui::Modifiers::NONE, Key::Delete);
                lance_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.doc_tasks_open {
                doc_task_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                doc_task_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                let ctrl_held = i.modifiers.ctrl || i.modifiers.command;
                doc_task_cycle |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if ctrl_held || modifiers.ctrl || modifiers.command
                    )
                });
                doc_task_open |= !doc_task_cycle
                    && i.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                modifiers,
                                ..
                            } if !ctrl_held && !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                        )
                    });
                if doc_task_cycle || doc_task_open {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                ..
                            }
                        )
                    });
                }
                doc_task_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.agenda_open && !self.file_picker_open && !self.link_heading_picker_open {
                agenda_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                agenda_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                agenda_previous_day |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                agenda_next_day |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                agenda_previous_week |= i.consume_key(egui::Modifiers::NONE, Key::PageUp);
                agenda_next_week |= i.consume_key(egui::Modifiers::NONE, Key::PageDown);
                agenda_today |= i.consume_key(egui::Modifiers::NONE, Key::Home);
                agenda_toggle_done |= i.consume_key(egui::Modifiers::NONE, Key::X);
                agenda_refile |= i.consume_key(egui::Modifiers::NONE, Key::R);
                agenda_org_tag |= i.events.iter().any(|event| {
                    matches!(event, egui::Event::Text(text) if text == "#")
                });
                agenda_org_project |= i.events.iter().any(|event| {
                    matches!(event, egui::Event::Text(text) if text == "@")
                });
                if agenda_org_tag || agenda_org_project {
                    i.events.retain(|event| {
                        !matches!(event, egui::Event::Text(text) if text == "#" || text == "@")
                    });
                }
                agenda_cycle_tab |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && modifiers.shift
                    )
                });
                agenda_toggle_focus |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                    )
                });
                if agenda_toggle_focus || agenda_cycle_tab {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Tab,
                                pressed: true,
                                ..
                            }
                        )
                    });
                }
                let ctrl_held = i.modifiers.ctrl || i.modifiers.command;
                agenda_cycle |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if ctrl_held || modifiers.ctrl || modifiers.command
                    )
                });
                agenda_edit |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && modifiers.shift
                    )
                });
                agenda_open |= !agenda_cycle && !agenda_edit
                    && i.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                modifiers,
                                ..
                            } if !ctrl_held && !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                        )
                    });
                agenda_open |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                if agenda_cycle || agenda_edit || agenda_open {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Enter,
                                pressed: true,
                                repeat: false,
                                ..
                            }
                        )
                    });
                }
            }
            if self.file_picker_open {
                file_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                file_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                file_enter_dir |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                file_parent |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                file_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                if self.file_picker_mode==FilePickerMode::SetPeriscopeProject {file_assign_current|=i.consume_key(egui::Modifiers::CTRL,Key::Enter);}
                file_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.periscope_open {
                periscope_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                periscope_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                periscope_open_modal |= i.consume_key(egui::Modifiers::CTRL, Key::Enter);
                periscope_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                periscope_toggle_mode |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                periscope_expand_folder |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                periscope_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.find_note_open {
                find_note_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                find_note_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                find_note_open_modal |= i.consume_key(egui::Modifiers::SHIFT, Key::Enter);
                find_note_open |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                find_note_toggle |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                find_note_toggle |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                find_note_toggle |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                find_note_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
                find_note_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.periscope_prompt_open {
                periscope_prompt_yes |= i.consume_key(egui::Modifiers::NONE, Key::Y);
                periscope_prompt_yes |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                periscope_prompt_no |= i.consume_key(egui::Modifiers::NONE, Key::N);
            }
            if self.link_assist_open {
                link_assist_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                link_assist_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                link_assist_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                link_assist_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                link_assist_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.link_assist_web_open {
                link_web_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                link_web_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
                link_web_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.link_heading_picker_open {
                link_heading_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                link_heading_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                link_heading_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                link_heading_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Space);
                link_heading_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if self.link_preview_open {
                link_preview_confirm |= i.consume_key(egui::Modifiers::CTRL, Key::Enter);
                link_preview_save |= i.consume_key(egui::Modifiers::CTRL, Key::S);
                link_preview_swap |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift
                    )
                });
                if link_preview_swap {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Tab,
                                pressed: true,
                                ..
                            }
                        )
                    });
                }
                link_preview_cancel |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::Escape,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                });
                if link_preview_cancel {
                    i.events.retain(|event| {
                        !matches!(
                            event,
                            egui::Event::Key {
                                key: Key::Escape,
                                pressed: true,
                                ..
                            }
                        )
                    });
                }
            }
            if self.save_as_open {
                save_as_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                save_as_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                save_as_enter_dir |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                save_as_parent |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                save_as_enter |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                save_as_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
            }
            if self.scratch_modal_open {
                scratch_archive |= i.consume_key(egui::Modifiers::CTRL, Key::S);
                scratch_open_entries |= i.consume_key(egui::Modifiers::CTRL, Key::E);
            }
            if self.capture_modal_open {
                capture_confirm |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
            }
            if self.scratch_entries_open {
                scratch_entries_previous |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                scratch_entries_next |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                scratch_entries_delete |= i.consume_key(egui::Modifiers::NONE, Key::Delete);
                scratch_entries_delete |= i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::D,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if modifiers.ctrl && !modifiers.alt
                    )
                });
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::P) {
                self.open_command_palette();
            }
            if search_active {
                search_cursor_after |= i.consume_key(egui::Modifiers::CTRL, Key::F);
                search_cursor_before |= i.consume_key(egui::Modifiers::CTRL, Key::B);
            } else if i.consume_key(egui::Modifiers::CTRL, Key::F) {
                self.palette_open = false;
                self.lance_open = false;
                self.command_line = "find ".to_string();
                self.command_line_cursor = self.command_line.len();
                self.command_history_index = None;
                self.command_line_focused = true;
                self.focus_command_line_once = true;
                self.focus_editor_once = false;
            }
            if i.consume_key(egui::Modifiers::CTRL, Key::Period) {
                self.palette_open = false;
                self.recent_picker_open = false;
                self.lance_open = false;
                self.doc_tasks_open = false;
                self.file_picker_open = false;
                self.periscope_open = false;
                self.periscope_prompt_open = false;
                self.find_note_open = false;
                self.link_heading_picker_open = false;
                if !self.link_preview_dirty {
                    self.link_preview_open = false;
                }
                self.save_as_open = false;
                self.scratch_modal_open = false;
                self.scratch_entries_open = false;
                self.capture_modal_open = false;
                self.command_line.clear();
                self.command_line_cursor = 0;
                self.command_history_index = None;
                self.command_line_focused = true;
                self.focus_command_line_once = true;
                self.focus_editor_once = false;
            }
            if self.command_line_focused || self.focus_command_line_once {
                execute_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                complete_command_line |= i.consume_key(egui::Modifiers::NONE, Key::Tab);
                previous_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                next_command |= i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
                command_line_backspace |= i.consume_key(egui::Modifiers::NONE, Key::Backspace);
                command_line_delete |= i.consume_key(egui::Modifiers::NONE, Key::Delete);
                command_line_left |= i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft);
                command_line_right |= i.consume_key(egui::Modifiers::NONE, Key::ArrowRight);
                command_line_home |= i.consume_key(egui::Modifiers::NONE, Key::Home);
                command_line_end |= i.consume_key(egui::Modifiers::NONE, Key::End);
            } else if search_active {
                search_next |= i.consume_key(egui::Modifiers::NONE, Key::F);
                search_previous |= i.consume_key(egui::Modifiers::NONE, Key::B);
                search_accept |= i.consume_key(egui::Modifiers::NONE, Key::Enter);
                search_cancel |= i.consume_key(egui::Modifiers::NONE, Key::Escape);
            }
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.consume_key(egui::Modifiers::CTRL, Key::N)
            {
                command = Some(Command::New);
            }
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.consume_key(egui::Modifiers::CTRL, Key::O)
            {
                command = Some(Command::Open);
            }
            let save_pressed = !self.link_preview_open
                && !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::S,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if modifiers.ctrl && !modifiers.alt && !modifiers.shift
                    )
                });
            let save_as_pressed = !self.link_preview_open
                && !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: Key::S,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if modifiers.ctrl && modifiers.alt && !modifiers.shift
                    )
                });
            if save_as_pressed {
                command = Some(Command::SaveAs);
            } else if save_pressed {
                command = Some(Command::Save);
            }
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
                && i.consume_key(egui::Modifiers::CTRL, Key::M)
            {
                command = Some(Command::TogglePreview);
            }
            // Tab-style keys: Ctrl+T new document, Ctrl+W close the document
            // (a tab once Slate has tabs), Ctrl+Q close the window (Slate
            // stays in the tray; :quit-all or the tray quit completely).
            if !self.scratch_modal_open
                && !self.scratch_entries_open
                && !self.capture_modal_open
            {
                if i.consume_key(egui::Modifiers::CTRL, Key::T) {
                    command = Some(Command::New);
                } else if i.consume_key(egui::Modifiers::CTRL, Key::W) {
                    command = Some(Command::CloseBuffer);
                } else if i.consume_key(egui::Modifiers::CTRL, Key::Q) {
                    command = Some(Command::Quit);
                }
            }
            if !search_cancel && i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                if self.settings_open {
                    self.settings_open = false;
                    self.focus_editor_once = true;
                } else if self.command_line_focused || self.focus_command_line_once {
                    self.command_line.clear();
                    self.command_line_cursor = 0;
                    self.command_line_focused = false;
                    self.focus_command_line_once = false;
                    self.command_history_index = None;
                    self.focus_editor_once = true;
                } else if self.shortcut_help_open {
                    self.shortcut_help_open = false;
                    self.focus_editor_once = true;
                } else if self.recent_picker_open {
                    self.recent_picker_open = false;
                    self.pending_recent_path = None;
                    self.focus_editor_once = true;
                } else if self.lance_open {
                    self.lance_open = false;
                    self.focus_editor_once = true;
                } else if self.doc_tasks_open {
                    self.doc_tasks_open = false;
                    self.focus_editor_once = true;
                } else if self.agenda_open {
                    if self.standalone_agenda {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                    self.agenda_open = false;
                    self.focus_editor_once = true;
                } else if self.file_picker_open {
                    self.file_picker_open = false;
                    self.pending_project_file_path = None;
                    self.pending_open_heading_fragment = None;
                    if self.file_picker_mode == FilePickerMode::InsertMarkdownLink {
                        self.cancel_link_assist();
                    }
                    if self.file_picker_mode == FilePickerMode::Refile {
                        self.pending_refile = None;
                        self.pending_refile_destination = None;
                    }
                    self.focus_editor_once = true;
                } else if self.periscope_open {
                    self.periscope_open = false;
                    self.focus_editor_once = true;
                } else if self.periscope_prompt_open {
                    self.periscope_prompt_open = false;
                    self.focus_editor_once = true;
                } else if self.find_note_open {
                    self.find_note_open = false;
                    self.focus_editor_once = true;
                } else if self.link_heading_picker_open {
                    self.cancel_link_assist();
                } else if self.link_preview_open {
                    self.close_link_preview();
                } else if self.save_as_open {
                    self.save_as_open = false;
                    self.focus_editor_once = true;
                } else if self.scratch_modal_open {
                    self.cancel_scratch_modal();
                } else if self.scratch_entries_open {
                    self.scratch_entries_open = false;
                    self.focus_editor_once = true;
                } else if self.capture_modal_open {
                    self.capture_modal_open = false;
                    self.capture_title.clear();
                    self.capture_text.clear();
                    self.focus_editor_once = true;
                } else if self.palette_open {
                    self.palette_open = false;
                    self.focus_editor_once = true;
                } else if self.pending_action.is_some() {
                    self.pending_action = None;
                    self.pending_daily_target = None;
                    self.focus_editor_once = true;
                } else if self.pending_file_open.is_some() {
                    self.cancel_file_open();
                    self.status = "Open cancelled".to_string();
                    self.focus_editor_once = true;
                } else if self.scratch {
                    self.close_scratch_with_feedback(ctx);
                    return;
                }
            }
        });

        if scratch_archive {
            self.archive_scratch_modal();
            return;
        }

        if scratch_open_entries {
            self.open_scratch_entries_modal();
            return;
        }

        if scratch_entries_previous {
            self.selected_scratch_entry = self.selected_scratch_entry.saturating_sub(1);
            return;
        }

        if scratch_entries_next {
            self.selected_scratch_entry =
                (self.selected_scratch_entry + 1).min(self.scratch_entries.len().saturating_sub(1));
            return;
        }

        if scratch_entries_delete {
            self.delete_selected_scratch_entry();
            return;
        }

        if capture_confirm {
            self.confirm_capture_modal();
            return;
        }

        if search_cursor_after {
            self.place_cursor_at_search_edge(true);
            return;
        }

        if search_cursor_before {
            self.place_cursor_at_search_edge(false);
            return;
        }

        if search_cancel {
            self.cancel_search();
            return;
        }

        if search_accept {
            self.accept_search();
            return;
        }

        if search_next {
            self.move_search_match(true);
            return;
        }

        if search_previous {
            self.move_search_match(false);
            return;
        }

        if self.recent_picker_open {
            self.handle_recent_picker_text_input(ctx);
        }

        if self.lance_open {
            self.handle_lance_text_input(ctx);
        }

        if self.doc_tasks_open {
            self.handle_doc_tasks_text_input(ctx);
        }

        if self.file_picker_open {
            self.handle_file_picker_text_input(ctx);
        }

        if self.periscope_open {
            self.handle_periscope_text_input(ctx);
        }

        if self.find_note_open {
            self.handle_find_note_text_input(ctx);
        }

        if self.link_assist_web_open {
            self.handle_link_assist_web_text_input(ctx);
        }

        if self.save_as_open {
            self.handle_save_as_text_input(ctx);
        }

        if self.handle_flow_inline_org_keyboard(ctx) {
            return;
        }

        if link_assist_cancel || link_web_cancel || link_heading_cancel {
            self.cancel_link_assist();
            return;
        }

        if link_preview_cancel {
            self.close_link_preview();
            return;
        }

        if link_preview_save {
            self.save_link_preview();
            return;
        }

        if link_preview_swap {
            self.swap_link_preview_with_main();
            return;
        }

        if link_preview_confirm {
            self.commit_link_preview();
            return;
        }

        if link_heading_previous {
            self.move_link_heading_selection(-1);
            return;
        }

        if link_heading_next {
            self.move_link_heading_selection(1);
            return;
        }

        if link_heading_confirm {
            self.confirm_link_heading_picker();
            return;
        }

        if link_assist_previous || link_assist_next {
            self.link_assist_choice = match self.link_assist_choice {
                LinkAssistChoice::File => LinkAssistChoice::Web,
                LinkAssistChoice::Web => LinkAssistChoice::File,
            };
            return;
        }

        if link_assist_confirm {
            self.confirm_link_assist_choice();
            return;
        }

        if link_web_backspace {
            self.link_assist_web_url.pop();
            return;
        }

        if link_web_confirm {
            let url = self.link_assist_web_url.trim().to_string();
            if url.is_empty() {
                self.status = "Insert link: URL is empty".to_string();
            } else {
                self.link_assist_web_open = false;
                self.insert_assisted_markdown_link(&url);
            }
            return;
        }

        if save_as_backspace {
            self.save_as_filename.pop();
            self.selected_save_as_entry =
                self.save_as_entry_indices().first().copied().unwrap_or(0);
            return;
        }

        if save_as_previous {
            self.move_save_as_selection(-1);
            return;
        }

        if save_as_next {
            self.move_save_as_selection(1);
            return;
        }

        if save_as_enter_dir {
            self.save_as_enter_selected_dir();
            return;
        }

        if save_as_parent {
            self.save_as_go_parent();
            return;
        }

        if save_as_enter {
            self.confirm_save_as();
            return;
        }

        if periscope_prompt_yes {
            self.choose_periscope_project();
            return;
        }

        if periscope_prompt_no {
            self.open_global_periscope();
            return;
        }

        if find_note_cancel {
            self.find_note_open = false;
            self.focus_editor_once = true;
            return;
        }

        if find_note_backspace {
            self.find_note_query.pop();
            self.find_note_expanded_files.clear();
            self.selected_find_note_row = 0;
            self.refresh_find_note_results();
            return;
        }

        if find_note_previous {
            self.move_find_note_selection(-1);
            return;
        }

        if find_note_next {
            self.move_find_note_selection(1);
            return;
        }

        if find_note_toggle {
            self.toggle_selected_find_note_file();
            return;
        }

        if find_note_open_modal {
            self.open_selected_find_note(true);
            return;
        }

        if find_note_open {
            self.open_selected_find_note(false);
            return;
        }

        if periscope_backspace {
            self.periscope_query.pop();
            self.periscope_expanded_folders.clear();
            self.selected_periscope_result = 0;
            self.refresh_periscope_results();
            return;
        }

        if periscope_previous {
            self.move_periscope_selection(-1);
            return;
        }

        if periscope_next {
            self.move_periscope_selection(1);
            return;
        }

        if periscope_toggle_mode {
            self.toggle_periscope_mode();
            return;
        }

        if periscope_expand_folder && self.toggle_selected_periscope_folder() {
            return;
        }

        if periscope_open_modal {
            self.open_selected_periscope_result_as_modal();
            return;
        }

        if periscope_open {
            self.open_selected_periscope_result();
            return;
        }

        if file_backspace {
            self.file_query.pop();
            self.selected_project_file = self.project_file_indices().first().copied().unwrap_or(0);
            return;
        }

        if file_previous {
            self.move_project_file_selection(-1);
            return;
        }

        if file_next {
            self.move_project_file_selection(1);
            return;
        }

        if file_enter_dir {
            self.file_picker_enter_selected_dir();
            return;
        }

        if file_parent {
            self.file_picker_go_parent();
            return;
        }

        if file_assign_current {
            self.assign_document_project(self.file_picker_dir.clone());
            return;
        }
        if file_open {
            self.open_selected_project_file();
            return;
        }

        if recent_backspace {
            self.recent_query.pop();
            self.selected_recent_file = self.recent_file_indices().first().copied().unwrap_or(0);
            return;
        }

        if recent_previous {
            self.move_recent_selection(-1);
            return;
        }

        if recent_next {
            self.move_recent_selection(1);
            return;
        }

        if recent_open {
            self.open_selected_recent_file();
            return;
        }

        if lance_backspace {
            self.lance_query.pop();
            self.selected_lance_file = self.lance_file_indices().first().copied().unwrap_or(0);
            return;
        }

        if lance_previous {
            self.move_lance_selection(-1);
            return;
        }

        if lance_next {
            self.move_lance_selection(1);
            return;
        }

        if lance_add {
            self.open_lance_file_picker();
            return;
        }

        if lance_delete {
            self.remove_selected_lance_file();
            return;
        }

        if lance_open_modal {
            self.open_selected_lance_file_as_modal();
            return;
        }

        if lance_open {
            self.open_selected_lance_file();
            return;
        }

        if doc_task_backspace {
            self.doc_task_query.pop();
            let tasks = self.doc_tasks();
            self.selected_doc_task_line = self
                .doc_task_indices()
                .first()
                .and_then(|index| tasks.get(*index))
                .map(|task| task.line_index)
                .unwrap_or(0);
            return;
        }

        if doc_task_previous {
            self.move_doc_task_selection(-1);
            return;
        }

        if doc_task_next {
            self.move_doc_task_selection(1);
            return;
        }

        if doc_task_cycle {
            self.cycle_selected_doc_task();
            return;
        }

        if doc_task_open {
            self.jump_to_selected_doc_task();
            return;
        }

        if agenda_org_tag {
            self.open_agenda_org_autocomplete(AgendaOrgMode::Tag);
            return;
        }

        if agenda_org_project {
            self.open_agenda_org_autocomplete(AgendaOrgMode::Project);
            return;
        }

        if agenda_previous {
            self.move_agenda_selection(-1);
            return;
        }

        if agenda_next {
            self.move_agenda_selection(1);
            return;
        }

        if agenda_previous_day {
            self.move_agenda_week_window(-1);
            return;
        }

        if agenda_next_day {
            self.move_agenda_week_window(1);
            return;
        }

        if agenda_previous_week {
            self.move_agenda_week_window(-1);
            return;
        }

        if agenda_next_week {
            self.move_agenda_week_window(1);
            return;
        }

        if agenda_today {
            self.jump_agenda_to_today();
            return;
        }

        if agenda_cycle_tab {
            self.cycle_agenda_tab();
            return;
        }

        if agenda_toggle_focus {
            self.toggle_agenda_focus();
            return;
        }

        if agenda_toggle_done {
            self.toggle_agenda_done_visibility();
            return;
        }

        if agenda_edit {
            self.open_selected_flow_item_edit();
            return;
        }

        if agenda_refile {
            self.start_refile_selected_agenda_item();
            return;
        }

        if agenda_cycle {
            self.cycle_selected_agenda_item();
            return;
        }

        if agenda_open {
            self.activate_selected_agenda_row();
            if self.standalone_agenda && !self.agenda_open {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        if settings_previous {
            self.selected_setting = self.selected_setting.saturating_sub(1);
            return;
        }

        if settings_next {
            self.selected_setting = (self.selected_setting + 1).min(14 + footer_ui::FIELDS.len());
            return;
        }

        if settings_decrement {
            match self.selected_setting {
                0 => self.set_command_history_limit(self.command_history_limit.saturating_sub(1)),
                1 => self.set_line_number_mode(LineNumberMode::Absolute),
                2 => self.set_ctrl_shift_move_mode(CtrlShiftMoveMode::Vim),
                3 => self.set_wrap_mode(false),
                4 => self.set_preview_mode(false),
                5 => self.set_markdown_live_rendering(false),
                6 => self.set_virtual_references_enabled(false),
                7 => self.set_reopen_last_file_on_startup(false),
                8 => self.set_editor_font_size(self.editor_font_size - 1.0),
                9..=18 => self.set_footer_option(self.selected_setting - 9, false),
                19 => self.set_periscope_everything(false),
                20 => self.set_resident(false),
                21 => self.set_start_with_windows(false),
                22 => self.set_editor_look(editor_view::EditorLook {
                    font: self.editor_look.font.next(),
                    ..self.editor_look
                }),
                23 => self.set_editor_look(editor_view::EditorLook {
                    block_cursor: false,
                    ..self.editor_look
                }),
                24 => self.set_editor_look(editor_view::EditorLook {
                    animate_cursor: false,
                    ..self.editor_look
                }),
                _ => {}
            }
            return;
        }

        if settings_increment || settings_activate {
            match self.selected_setting {
                0 => self.set_command_history_limit(self.command_history_limit + 1),
                1 => {
                    let next_mode = match self.line_number_mode {
                        LineNumberMode::Absolute => LineNumberMode::Relative,
                        LineNumberMode::Relative => LineNumberMode::Absolute,
                    };
                    self.set_line_number_mode(next_mode);
                }
                2 => self.set_ctrl_shift_move_mode(self.ctrl_shift_move_mode.next()),
                3 => self.set_wrap_mode(!self.wrap),
                4 => self.set_preview_mode(!self.preview),
                5 => self.set_markdown_live_rendering(!self.markdown_live_rendering),
                6 => self.set_virtual_references_enabled(!self.virtual_references_enabled),
                7 => self.set_reopen_last_file_on_startup(!self.reopen_last_file_on_startup),
                8 => self.set_editor_font_size(if settings_activate {
                    editor_view::DEFAULT_EDITOR_FONT_SIZE
                } else {
                    self.editor_font_size + 1.0
                }),
                9..=18 => {
                    let index = self.selected_setting - 9;
                    self.set_footer_option(
                        index,
                        if settings_activate {
                            !self.footer_options.enabled[index]
                        } else {
                            true
                        },
                    );
                }
                19 => self.set_periscope_everything(if settings_activate {
                    !self.periscope_everything
                } else {
                    true
                }),
                20 => self.set_resident(if settings_activate { !self.resident } else { true }),
                21 => self.set_start_with_windows(if settings_activate {
                    !self.start_with_windows
                } else {
                    true
                }),
                22 => self.set_editor_look(editor_view::EditorLook {
                    font: self.editor_look.font.next(),
                    ..self.editor_look
                }),
                23 => self.set_editor_look(editor_view::EditorLook {
                    block_cursor: if settings_activate {
                        !self.editor_look.block_cursor
                    } else {
                        true
                    },
                    ..self.editor_look
                }),
                24 => self.set_editor_look(editor_view::EditorLook {
                    animate_cursor: if settings_activate {
                        !self.editor_look.animate_cursor
                    } else {
                        true
                    },
                    ..self.editor_look
                }),
                _ => {}
            }
            return;
        }

        if self.command_line_focused || self.focus_command_line_once {
            self.handle_command_line_text_input(ctx);
            if command_line_backspace {
                self.command_line_backspace();
                return;
            }
            if command_line_delete {
                self.command_line_delete();
                return;
            }
            if command_line_left {
                self.command_line_cursor = self.previous_command_line_boundary();
                return;
            }
            if command_line_right {
                self.command_line_cursor = self.next_command_line_boundary();
                return;
            }
            if command_line_home {
                self.command_line_cursor = 0;
                return;
            }
            if command_line_end {
                self.command_line_cursor = self.command_line.len();
                return;
            }
        }

        let navigating_command_history = self.command_history_index.is_some();
        let command_suggestion_count = if (self.command_line_focused
            || self.focus_command_line_once)
            && !navigating_command_history
        {
            self.command_line_suggestions().len()
        } else {
            0
        };

        if previous_command {
            if command_suggestion_count > 0 {
                self.selected_command_line_suggestion =
                    self.selected_command_line_suggestion.saturating_sub(1);
                self.focus_command_line_once = true;
                return;
            }
            if !self.command_history.is_empty() {
                let index = self
                    .command_history_index
                    .unwrap_or(self.command_history.len())
                    .saturating_sub(1);
                self.command_history_index = Some(index);
                self.command_line = self.command_history[index].clone();
                self.command_line_cursor = self.command_line.len();
                self.selected_command_line_suggestion = 0;
                self.focus_command_line_once = true;
                return;
            }
        }

        if next_command {
            if command_suggestion_count > 0 {
                self.selected_command_line_suggestion = (self.selected_command_line_suggestion + 1)
                    .min(command_suggestion_count.saturating_sub(1));
                self.focus_command_line_once = true;
                return;
            }
            if let Some(index) = self.command_history_index {
                if index + 1 < self.command_history.len() {
                    let index = index + 1;
                    self.command_history_index = Some(index);
                    self.command_line = self.command_history[index].clone();
                    self.command_line_cursor = self.command_line.len();
                } else {
                    self.command_history_index = None;
                    self.command_line.clear();
                    self.command_line_cursor = 0;
                }
                self.selected_command_line_suggestion = 0;
                self.focus_command_line_once = true;
            }
            return;
        }

        if complete_command_line {
            if !self.accept_command_line_completion() {
                self.run_command_line(ctx);
            }
            return;
        }

        if execute_command_line {
            if command_suggestion_count > 0 {
                let suggestions = self.command_line_suggestions();
                if let Some(spec) = suggestions.get(
                    self.selected_command_line_suggestion
                        .min(suggestions.len().saturating_sub(1)),
                ) {
                    self.command_line = spec.name.to_string();
                    self.command_line_cursor = self.command_line.len();
                }
            }
            self.run_command_line(ctx);
            return;
        }

        if let Some(command) = command {
            self.run_command(command, ctx);
        }
    }
}
