// ---- shortcuts.rs: flow error dialog keys
        if flow_error::handle_input(ctx) {
            self.suppress_editor_keyboard_once = true;
            return;
        }

// ---- shortcuts.rs: agenda org autocomplete keys
        if self.handle_agenda_org_keyboard(ctx) {
            return;
        }


// ---- shortcuts.rs: agenda key flags
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

// ---- shortcuts.rs: agenda key capture
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

// ---- shortcuts.rs: escape closes agenda
                } else if self.agenda_open {
                    if self.standalone_agenda {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                    self.agenda_open = false;
                    self.focus_editor_once = true;

// ---- shortcuts.rs: inline #/@ autocomplete keys
        if self.handle_flow_inline_org_keyboard(ctx) {
            return;
        }


// ---- shortcuts.rs: agenda key handlers
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

