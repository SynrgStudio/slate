//! Slate Flow agenda modal and Flow item edit form.
use super::*;

impl SlateApp {
    pub(super) fn agenda_dialog(&mut self, ctx: &egui::Context) {
        if !self.agenda_open {
            return;
        }

        let today = Local::now().date_naive();
        let week_start = self.agenda_week_start();
        let week_end = week_start + Duration::days(6);
        let title = format!("week-agenda (W{}):", week_start.iso_week().week());
        let mut clicked_dated_row = None;
        let mut clicked_inbox_row = None;
        let mut clicked_flat_row = None;
        let mut clicked_task_index = None;
        let mut open_clicked = false;
        let mut toggle_clicked_day = None;
        let mut org_anchor_rect = None;
        let scroll_to_selected = self.agenda_scroll_to_selected;

        window_ui::DialogArea::new("agenda_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let rect = ui.ctx().content_rect();
                let (width, height) = if self.standalone_agenda {
                    (
                        (rect.width() * 0.96).clamp(620.0, 1200.0),
                        (rect.height() * 0.94).clamp(420.0, 900.0),
                    )
                } else {
                    (
                        (rect.width() * 0.78).clamp(560.0, 920.0),
                        (rect.height() * 0.82).clamp(420.0, 760.0),
                    )
                };
                egui::Frame::new()
                    .fill(Color32::from_rgb(19, 23, 31))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(67, 76, 94)))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_min_size(Vec2::new(width, height));
                        ui.set_max_size(Vec2::new(width, height));
                        let title_font = FontId::new(16.0, FontFamily::Monospace);
                        let row_font = FontId::new(14.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(180, 160, 235);
                        let dim = Color32::from_rgb(118, 112, 150);
                        let text = Color32::from_rgb(216, 222, 233);
                        let muted = Color32::from_rgb(143, 151, 170);
                        let today_bg = Color32::from_rgb(38, 47, 61);
                        let selected_bg = Color32::from_rgb(49, 61, 79);
                        let inactive_bg = Color32::from_rgb(27, 32, 42);
                        let separator = Color32::from_rgb(49, 56, 72);

                        ui.horizontal(|ui| {
                            for tab in AgendaTab::ALL {
                                let active = self.agenda_tab == tab;
                                let label = format!(" {} ", tab.label());
                                let tab_text = RichText::new(label)
                                    .font(row_font.clone())
                                    .color(if active { text } else { muted });
                                let response = egui::Frame::new()
                                    .fill(if active { selected_bg } else { inactive_bg })
                                    .stroke(Stroke::new(
                                        1.0_f32,
                                        if active {
                                            accent
                                        } else {
                                            Color32::from_rgb(37, 44, 58)
                                        },
                                    ))
                                    .corner_radius(4.0)
                                    .inner_margin(egui::Margin::symmetric(8, 3))
                                    .show(ui, |ui| ui.label(tab_text))
                                    .response;
                                let _ = response;
                                ui.add_space(4.0);
                            }
                            ui.label(
                                RichText::new("Shift+Tab cambia vista")
                                    .font(row_font.clone())
                                    .color(dim),
                            );
                        });
                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(title).font(title_font).color(text));
                            ui.label(
                                RichText::new(format!(
                                    "{}–{}",
                                    week_start.format("%-d %B"),
                                    week_end.format("%-d %B %Y")
                                ))
                                .font(row_font.clone())
                                .color(dim),
                            );
                            ui.label(
                                RichText::new(match self.agenda_focus {
                                    AgendaFocus::Dated => "focus: dated week",
                                    AgendaFocus::Inbox => "focus: no-date",
                                })
                                .font(row_font.clone())
                                .color(accent),
                            );
                            ui.label(
                                RichText::new(if self.agenda_show_done {
                                    "done: shown"
                                } else {
                                    "done: hidden"
                                })
                                .font(row_font.clone())
                                .color(muted),
                            );
                            if !self.agenda_filter.is_empty() {
                                ui.label(
                                    RichText::new(format!("filter: {}", self.agenda_filter))
                                        .font(row_font.clone())
                                        .color(accent),
                                );
                            }
                        });
                        ui.add_space(8.0);

                        if self.agenda_tab == AgendaTab::Agenda {
                            let dated_rows = self.agenda_dated_rows();
                            let dated_panel_height = (height * 0.42).clamp(160.0, 310.0);
                            let dated_title_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), 22.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            let dated_active = self.agenda_focus == AgendaFocus::Dated;
                            ui.painter().rect_filled(
                                dated_title_rect,
                                0.0,
                                if dated_active { today_bg } else { inactive_bg },
                            );
                            ui.painter().text(
                                egui::pos2(
                                    dated_title_rect.left() + 6.0,
                                    dated_title_rect.center().y,
                                ),
                                egui::Align2::LEFT_CENTER,
                                "Dated tasks · ↑↓ row · Enter expand/open · ←→ week",
                                row_font.clone(),
                                if dated_active { text } else { muted },
                            );

                            egui::ScrollArea::vertical()
                                .id_salt("agenda_dated_scroll")
                                .max_height(dated_panel_height)
                                .show(ui, |ui| {
                                    for (row_index, row) in dated_rows.iter().copied().enumerate() {
                                        let selected = self.agenda_focus == AgendaFocus::Dated
                                            && self.agenda_selected_dated_row == row_index;
                                        match row {
                                            AgendaDatedRow::Day(date) => {
                                                let count = self.agenda_items_for_date(date).len();
                                                let response = agenda::day_row(
                                                    ui,
                                                    row_index,
                                                    date,
                                                    count,
                                                    self.agenda_expanded_dates.contains(&date),
                                                    date == today,
                                                    selected,
                                                    scroll_to_selected && selected,
                                                    selected_bg,
                                                    today_bg,
                                                    row_font.clone(),
                                                    text,
                                                    muted,
                                                    accent,
                                                );
                                                if response.clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    open_clicked = false;
                                                }
                                                if response.double_clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    toggle_clicked_day = Some(date);
                                                }
                                            }
                                            AgendaDatedRow::Task(index) => {
                                                let response = agenda::task_row(
                                                    ui,
                                                    index,
                                                    selected,
                                                    scroll_to_selected && selected,
                                                    selected_bg,
                                                    row_font.clone(),
                                                    text,
                                                    accent,
                                                    &self.agenda_items[index],
                                                );
                                                if selected && self.agenda_org_mode.is_some() {
                                                    org_anchor_rect = Some(response.rect);
                                                }
                                                if response.clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    clicked_task_index = Some(index);
                                                    open_clicked = false;
                                                }
                                                if response.double_clicked() {
                                                    clicked_dated_row = Some(row_index);
                                                    clicked_task_index = Some(index);
                                                    open_clicked = true;
                                                }
                                            }
                                        }
                                    }
                                });

                            ui.add_space(8.0);
                            let (rule_rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 1.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(rule_rect, 0.0, separator);
                            ui.add_space(8.0);

                            let inbox_indices = self.agenda_inbox_item_indices();
                            let inbox_title_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), 22.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            let inbox_active = self.agenda_focus == AgendaFocus::Inbox;
                            ui.painter().rect_filled(
                                inbox_title_rect,
                                0.0,
                                if inbox_active { today_bg } else { inactive_bg },
                            );
                            ui.painter().text(
                            egui::pos2(inbox_title_rect.left() + 6.0, inbox_title_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!(
                                "No-date tasks · {} item{} · Tab switches segment · x toggles done",
                                inbox_indices.len(),
                                if inbox_indices.len() == 1 { "" } else { "s" }
                            ),
                            row_font.clone(),
                            if inbox_active { text } else { muted },
                        );

                            let inbox_height = (ui.available_height() - 4.0).max(80.0);
                            egui::ScrollArea::vertical()
                                .id_salt("agenda_inbox_scroll")
                                .max_height(inbox_height)
                                .show(ui, |ui| {
                                    if inbox_indices.is_empty() {
                                        ui.add_space(14.0);
                                        ui.label(
                                            RichText::new("No tasks without date.")
                                                .font(row_font.clone())
                                                .color(dim),
                                        );
                                    }
                                    for (position, index) in
                                        inbox_indices.iter().copied().enumerate()
                                    {
                                        let selected = self.agenda_focus == AgendaFocus::Inbox
                                            && self.agenda_selected_inbox_item == position;
                                        let response = agenda::task_row(
                                            ui,
                                            index,
                                            selected,
                                            scroll_to_selected && selected,
                                            selected_bg,
                                            row_font.clone(),
                                            text,
                                            accent,
                                            &self.agenda_items[index],
                                        );
                                        if selected && self.agenda_org_mode.is_some() {
                                            org_anchor_rect = Some(response.rect);
                                        }
                                        if response.clicked() {
                                            clicked_inbox_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = false;
                                        }
                                        if response.double_clicked() {
                                            clicked_inbox_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = true;
                                        }
                                    }
                                });
                        } else {
                            let flat_indices = self.agenda_flat_item_indices();
                            let tab_index = self.agenda_tab.index();
                            let title_rect = ui
                                .allocate_exact_size(
                                    Vec2::new(ui.available_width(), 22.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            ui.painter().rect_filled(title_rect, 0.0, today_bg);
                            ui.painter().text(
                                egui::pos2(title_rect.left() + 6.0, title_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                format!(
                                    "{} · {} item{} · ↑↓ row · Enter open · Shift+Tab tab",
                                    self.agenda_tab.label(),
                                    flat_indices.len(),
                                    if flat_indices.len() == 1 { "" } else { "s" }
                                ),
                                row_font.clone(),
                                text,
                            );

                            let flat_height = (ui.available_height() - 4.0).max(120.0);
                            egui::ScrollArea::vertical()
                                .id_salt(format!("agenda_{}_scroll", self.agenda_tab.label()))
                                .max_height(flat_height)
                                .show(ui, |ui| {
                                    if flat_indices.is_empty() {
                                        ui.add_space(14.0);
                                        ui.label(
                                            RichText::new(format!(
                                                "No {} items.",
                                                self.agenda_tab.label().to_ascii_lowercase()
                                            ))
                                            .font(row_font.clone())
                                            .color(dim),
                                        );
                                    }
                                    for (position, index) in
                                        flat_indices.iter().copied().enumerate()
                                    {
                                        let selected =
                                            self.agenda_selected_tab_items[tab_index] == position;
                                        let response = agenda::task_row(
                                            ui,
                                            index,
                                            selected,
                                            scroll_to_selected && selected,
                                            selected_bg,
                                            row_font.clone(),
                                            text,
                                            accent,
                                            &self.agenda_items[index],
                                        );
                                        if selected && self.agenda_org_mode.is_some() {
                                            org_anchor_rect = Some(response.rect);
                                        }
                                        if response.clicked() {
                                            clicked_flat_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = false;
                                        }
                                        if response.double_clicked() {
                                            clicked_flat_row = Some(position);
                                            clicked_task_index = Some(index);
                                            open_clicked = true;
                                        }
                                    }
                                });
                        }
                    });
            });

        if let (Some(mode), Some(anchor_rect)) = (self.agenda_org_mode, org_anchor_rect) {
            let screen_rect = ctx.content_rect();
            let popup_width = 380.0;
            let popup_height = 82.0;
            let x = (anchor_rect.left() + anchor_rect.width() * 0.48)
                .min(screen_rect.right() - popup_width - 18.0)
                .max(screen_rect.left() + 18.0);
            let mut y = anchor_rect.bottom() + 4.0;
            if y + popup_height > screen_rect.bottom() - 18.0 {
                y = (anchor_rect.top() - popup_height - 4.0).max(screen_rect.top() + 18.0);
            }
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                "agenda_org_scrim".into(),
            ))
            .rect_filled(screen_rect, 0.0, Color32::from_black_alpha(72));

            let prefix = match mode {
                AgendaOrgMode::Tag => "#",
                AgendaOrgMode::Project => "@",
            };
            let suggestions = self.agenda_org_suggestions();
            egui::Area::new("agenda_org_autocomplete".into())
                .fixed_pos(egui::pos2(x, y))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let row_font = FontId::new(14.0, FontFamily::Monospace);
                    let accent = Color32::from_rgb(180, 160, 235);
                    let text = Color32::from_rgb(216, 222, 233);
                    let muted = Color32::from_rgb(143, 151, 170);
                    let dim = Color32::from_rgb(118, 112, 150);
                    let selected_bg = Color32::from_rgb(49, 61, 79);
                    let inactive_bg = Color32::from_rgb(27, 32, 42);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(15, 19, 27))
                        .stroke(Stroke::new(1.0_f32, accent))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_min_width(popup_width);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{prefix}{}", self.agenda_org_query))
                                        .font(row_font.clone())
                                        .color(accent),
                                );
                                ui.label(
                                    RichText::new("Tab cycle · ↑↓ select · Enter add · Esc cancel")
                                        .font(row_font.clone())
                                        .color(dim),
                                );
                            });
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                for (index, suggestion) in suggestions.iter().take(8).enumerate() {
                                    let selected = index == self.selected_agenda_org_suggestion;
                                    let chip = RichText::new(format!(" {prefix}{suggestion} "))
                                        .font(row_font.clone())
                                        .color(if selected { text } else { muted });
                                    egui::Frame::new()
                                        .fill(if selected { selected_bg } else { inactive_bg })
                                        .corner_radius(4.0)
                                        .inner_margin(egui::Margin::symmetric(4, 2))
                                        .show(ui, |ui| {
                                            ui.label(chip);
                                        });
                                    ui.add_space(4.0);
                                }
                            });
                        });
                });
        }

        self.flow_item_edit_dialog(ctx);

        if let Some(row) = clicked_dated_row {
            self.agenda_focus = AgendaFocus::Dated;
            self.agenda_selected_dated_row = row;
            self.agenda_scroll_to_selected = false;
        }
        if let Some(row) = clicked_inbox_row {
            self.agenda_focus = AgendaFocus::Inbox;
            self.agenda_selected_inbox_item = row;
            self.agenda_scroll_to_selected = false;
        }
        if let Some(row) = clicked_flat_row {
            self.agenda_selected_tab_items[self.agenda_tab.index()] = row;
            self.agenda_scroll_to_selected = false;
        }
        if let Some(index) = clicked_task_index {
            self.selected_agenda_item = index;
            if open_clicked {
                self.jump_to_agenda_item_index(index);
                if self.standalone_agenda && !self.agenda_open {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if let Some(date) = toggle_clicked_day {
            if !self.agenda_expanded_dates.insert(date) {
                self.agenda_expanded_dates.remove(&date);
            }
            self.clamp_agenda_selection();
        }
        self.agenda_scroll_to_selected = false;
    }

    pub(super) fn flow_item_edit_dialog(&mut self, ctx: &egui::Context) {
        if self.flow_item_edit.is_none() {
            return;
        }

        let mut save = false;
        let mut cancel = false;
        let mut open_org = None;
        let mut confirm_org = false;
        let mut org_next = false;
        let mut org_previous = false;
        let mut org_backspace = false;
        let mut org_typed = String::new();

        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::CTRL, Key::Enter) {
                save = true;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Escape) {
                if self
                    .flow_item_edit
                    .as_ref()
                    .is_some_and(|edit| edit.org_mode.is_some())
                {
                    if let Some(edit) = self.flow_item_edit.as_mut() {
                        edit.close_org();
                    }
                } else {
                    cancel = true;
                }
            }
            let org_open = self
                .flow_item_edit
                .as_ref()
                .is_some_and(|edit| edit.org_mode.is_some());
            if org_open && input.consume_key(egui::Modifiers::NONE, Key::Tab) {
                org_next = true;
            }
            if org_open && input.consume_key(egui::Modifiers::SHIFT, Key::Tab) {
                org_previous = true;
            }
            if org_open
                && (input.consume_key(egui::Modifiers::NONE, Key::Enter)
                    || input.consume_key(egui::Modifiers::NONE, Key::Space))
            {
                confirm_org = true;
            }
            if org_open && input.consume_key(egui::Modifiers::NONE, Key::Backspace) {
                org_backspace = true;
            }
            for event in &input.events {
                if let egui::Event::Text(text) = event {
                    if text == "#" {
                        open_org = Some(AgendaOrgMode::Tag);
                    } else if text == "@" {
                        open_org = Some(AgendaOrgMode::Project);
                    } else if self
                        .flow_item_edit
                        .as_ref()
                        .is_some_and(|edit| edit.org_mode.is_some())
                    {
                        org_typed.push_str(text);
                    }
                }
            }
        });

        let all_values = |mode: AgendaOrgMode, items: &[AgendaItem]| -> Vec<String> {
            items
                .iter()
                .flat_map(move |item| match mode {
                    AgendaOrgMode::Tag => item.tags.clone(),
                    AgendaOrgMode::Project => item.projects.clone(),
                })
                .collect()
        };

        if let Some(edit) = self.flow_item_edit.as_mut() {
            if let Some(mode) = open_org {
                edit.open_org(mode);
            }
            if let Some(mode) = edit.org_mode {
                if org_backspace {
                    edit.org_query.pop();
                    edit.selected_org_suggestion = 0;
                }
                if !org_typed.is_empty() {
                    for ch in org_typed.chars() {
                        if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.') {
                            edit.org_query.push(ch);
                        }
                    }
                    edit.selected_org_suggestion = 0;
                }
                let suggestions =
                    organization_suggestions(all_values(mode, &self.agenda_items), &edit.org_query);
                let len = suggestions.len().max(1);
                if org_next {
                    edit.selected_org_suggestion = (edit.selected_org_suggestion + 1) % len;
                }
                if org_previous {
                    edit.selected_org_suggestion = if edit.selected_org_suggestion == 0 {
                        len.saturating_sub(1)
                    } else {
                        edit.selected_org_suggestion.saturating_sub(1)
                    };
                }
                edit.selected_org_suggestion =
                    edit.selected_org_suggestion.min(len.saturating_sub(1));
                if confirm_org {
                    let value = suggestions
                        .get(edit.selected_org_suggestion)
                        .cloned()
                        .unwrap_or_else(|| edit.org_query.trim().to_string());
                    if let Some(token) = organization_token(mode, &value) {
                        edit.cursor_char_index = replace_org_token_at_cursor(
                            &mut edit.text,
                            edit.cursor_char_index,
                            mode,
                            &token,
                        );
                    }
                    edit.close_org();
                }
            }
        }

        let screen_rect = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            "flow_item_edit_scrim".into(),
        ))
        .rect_filled(screen_rect, 0.0, Color32::from_black_alpha(90));

        let mut commit_payload = None;
        window_ui::DialogArea::new("flow_item_edit_dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 17, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(105, 117, 148)))
                    .corner_radius(9.0)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_min_width(720.0);
                        let row_font = FontId::new(14.0, FontFamily::Monospace);
                        let small_font = FontId::new(12.0, FontFamily::Monospace);
                        let title_font = FontId::new(15.0, FontFamily::Monospace);
                        let accent = Color32::from_rgb(154, 185, 235);
                        let accent_soft = Color32::from_rgb(42, 56, 76);
                        let text = Color32::from_rgb(218, 224, 235);
                        let muted = Color32::from_rgb(126, 136, 157);
                        let panel = Color32::from_rgb(17, 22, 31);
                        let field = Color32::from_rgb(10, 15, 22);
                        let line = Color32::from_rgb(43, 53, 68);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Flow item")
                                    .font(title_font.clone())
                                    .color(text),
                            );
                            ui.add_space(8.0);
                            egui::Frame::new()
                                .fill(Color32::from_rgb(24, 30, 42))
                                .corner_radius(999.0)
                                .inner_margin(egui::Margin::symmetric(7, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("Ctrl+Enter save")
                                            .font(small_font.clone())
                                            .color(accent),
                                    );
                                });
                            ui.label(
                                RichText::new("Esc cancel · #/@ autocomplete")
                                    .font(small_font.clone())
                                    .color(muted),
                            );
                        });
                        ui.add_space(10.0);
                        if let Some(edit) = self.flow_item_edit.as_mut() {
                            let output = egui::Frame::new()
                                .fill(field)
                                .stroke(Stroke::new(1.0_f32, line))
                                .corner_radius(5.0)
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .show(ui, |ui| {
                                    TextEdit::singleline(&mut edit.text)
                                        .font(row_font.clone())
                                        .desired_width(ui.available_width())
                                        .show(ui)
                                })
                                .inner;
                            if let Some(cursor) =
                                output.cursor_range.and_then(|range| range.single())
                            {
                                edit.cursor_char_index = cursor.index;
                            }
                            if edit.focus_once {
                                output.response.request_focus();
                                edit.focus_once = false;
                            }
                            if let Some(mode) = edit.org_mode {
                                if let Some(query) =
                                    org_query_at_cursor(&edit.text, edit.cursor_char_index, mode)
                                {
                                    edit.org_query = query;
                                }
                            }
                            if let Some(mode) = edit.org_mode {
                                ui.add_space(9.0);
                                let prefix = match mode {
                                    AgendaOrgMode::Tag => "#",
                                    AgendaOrgMode::Project => "@",
                                };
                                let mode_label = match mode {
                                    AgendaOrgMode::Tag => "tags",
                                    AgendaOrgMode::Project => "projects",
                                };
                                let suggestions = organization_suggestions(
                                    self.agenda_items.iter().flat_map(|item| match mode {
                                        AgendaOrgMode::Tag => item.tags.clone(),
                                        AgendaOrgMode::Project => item.projects.clone(),
                                    }),
                                    &edit.org_query,
                                );
                                egui::Frame::new()
                                    .fill(panel)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(29, 37, 50)))
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::symmetric(9, 7))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(mode_label)
                                                    .font(small_font.clone())
                                                    .color(muted),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "{prefix}{}",
                                                    edit.org_query
                                                ))
                                                .font(row_font.clone())
                                                .color(accent),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(
                                                            "Tab cycle · Enter add · Esc close",
                                                        )
                                                        .font(small_font.clone())
                                                        .color(muted),
                                                    );
                                                },
                                            );
                                        });
                                        ui.add_space(6.0);
                                        ui.horizontal_wrapped(|ui| {
                                            for (index, suggestion) in
                                                suggestions.iter().take(8).enumerate()
                                            {
                                                let selected =
                                                    index == edit.selected_org_suggestion;
                                                egui::Frame::new()
                                                    .fill(if selected {
                                                        accent_soft
                                                    } else {
                                                        Color32::from_rgb(23, 29, 39)
                                                    })
                                                    .stroke(Stroke::new(
                                                        1.0_f32,
                                                        if selected {
                                                            Color32::from_rgb(91, 124, 170)
                                                        } else {
                                                            Color32::from_rgb(28, 35, 47)
                                                        },
                                                    ))
                                                    .corner_radius(999.0)
                                                    .inner_margin(egui::Margin::symmetric(8, 3))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "{prefix}{suggestion}"
                                                            ))
                                                            .font(small_font.clone())
                                                            .color(if selected {
                                                                text
                                                            } else {
                                                                muted
                                                            }),
                                                        );
                                                    });
                                            }
                                        });
                                    });
                            }
                            if save {
                                commit_payload = Some((edit.item_index, edit.text.clone()));
                            }
                        }
                    });
            });

        if cancel {
            self.flow_item_edit = None;
            self.status = "Flow item edit cancelled".to_string();
        }
        if let Some((index, replacement)) = commit_payload {
            self.confirm_flow_item_edit(index, replacement, ctx);
        }
    }
}
