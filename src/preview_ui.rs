//! Rendered Markdown preview pane.
use super::*;

impl SlateApp {
    pub(super) fn preview_ui(&mut self, ui: &mut egui::Ui) {
        let preview_started = std::time::Instant::now();
        self.preview_layout
            .sync(&self.buffer, ui.ctx().pixels_per_point());
        let mut rendered_lines = 0_usize;
        let mut skipped_lines = 0_usize;
        let key = (
            self.buffer.identity,
            self.buffer.revision,
            ui.available_width().to_bits(),
            ui.ctx().pixels_per_point().to_bits(),
        );
        let valid = self.preview_sync.key == Some(key);
        let editor = self.editor_view.scroll_position();
        let preview_input = ui.rect_contains_pointer(ui.max_rect())
            && ui.input(|i| i.smooth_scroll_delta.y != 0.0 || i.pointer.primary_down());
        let from_editor = !preview_input
            && (!valid
                || self.preview_sync.pending_editor
                || editor.differs(self.preview_sync.editor));
        let old_offset = self.preview_sync.offset;
        let mut area = egui::ScrollArea::vertical().id_salt("synced_markdown_preview");
        if valid && from_editor {
            area = area.vertical_scroll_offset(self.preview_sync.offset_for(editor));
        }
        let mut points = std::mem::take(&mut self.preview_sync.points);
        let mut tops = std::mem::take(&mut self.preview_tops);
        let output = area.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let origin = ui.cursor().top();
            let width = ui.available_width();
            let spacing = ui.spacing().item_spacing.y;
            let layout = &mut self.preview_layout;
            let count = layout.len();
            // Estimated geometry for every line; only the visible stretch is
            // laid out for real, the rest is one empty allocation each side.
            layout.tops(width, &mut tops);
            let viewport = ui.clip_rect().expand2(Vec2::new(0.0, 120.0));
            let visible_top = viewport.top() - origin;
            let visible_bottom = viewport.bottom() - origin;
            let line = tops[..count]
                .partition_point(|top| *top <= visible_top)
                .saturating_sub(1);
            let first = layout.block_start(line);
            let advance = |ui: &mut egui::Ui, height: f32| {
                if height > 0.0 {
                    ui.allocate_space(Vec2::new(width, (height - spacing).max(0.0)));
                }
            };
            advance(ui, tops[first]);
            skipped_lines += first;
            points.clear();
            points.extend(
                tops[..first]
                    .iter()
                    .enumerate()
                    .map(|(index, top)| (index as f32, *top)),
            );
            let mut index = first;
            while index < count && ui.cursor().top() - origin < visible_bottom {
                let start_y = ui.cursor().top();
                points.push((index as f32, start_y - origin));
                let span = layout.block_len(index);
                rendered_lines += span;
                let line = self.buffer.line(index);
                let line = line.strip_suffix('\r').unwrap_or(line);
                if layout.is_table(index) {
                    let mut rows = vec![split_markdown_table_row(line).unwrap_or_default()];
                    let alignments = parse_markdown_table_separator(self.buffer.line(index + 1))
                        .unwrap_or_else(|| vec![TableAlignment::Left; rows[0].len()]);
                    for row in index + 2..index + span {
                        rows.push(
                            split_markdown_table_row(self.buffer.line(row)).unwrap_or_default(),
                        );
                    }
                    let rect = Self::markdown_table_preview(ui, &rows, &alignments);
                    let height = rect.height() / rows.len().max(1) as f32;
                    points.push(((index + 1) as f32, rect.top() + height - origin));
                    for row in 1..rows.len() {
                        points.push((
                            (index + row + 1) as f32,
                            rect.top() + row as f32 * height - origin,
                        ));
                    }
                } else {
                    Self::preview_line(ui, line, layout.in_code(index));
                }
                layout.measured(index, width, ui.cursor().top() - start_y);
                index += span;
            }
            // Everything below keeps its estimated spacing, moved by however
            // much the laid-out stretch differed from its estimate.
            let shift = ui.cursor().top() - origin - tops[index];
            points.extend(
                tops[index..]
                    .iter()
                    .enumerate()
                    .map(|(offset, top)| ((index + offset) as f32, top + shift)),
            );
            advance(ui, tops[count] - tops[index]);
            skipped_lines += count - index;
        });
        self.preview_tops = tops;
        if self.buffer.as_str().len() >= 1_000_000 && preview_started.elapsed().as_millis() >= 16 {
            perf_log::mark(
                "preview.render.slow",
                &format!(
                    "bytes={} rendered_lines={} skipped_lines={} cpu_ms={:.3}",
                    self.buffer.as_str().len(),
                    rendered_lines,
                    skipped_lines,
                    preview_started.elapsed().as_secs_f64() * 1000.0
                ),
            );
        }
        #[cfg(test)]
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                "preview_layout_counts".into(),
                (rendered_lines, skipped_lines),
            )
        });
        self.preview_sync.key = Some(key);
        self.preview_sync.points = points;
        self.preview_sync.max_scroll =
            (output.content_size.y - output.inner_rect.height()).max(0.0);
        self.preview_sync.offset = output.state.offset.y;
        self.preview_sync.editor = editor;
        self.preview_sync.pending_editor = from_editor
            && (self.preview_sync.offset_for(editor) - output.state.offset.y).abs() > 0.5;
        if self.preview_sync.pending_editor {
            ui.ctx().request_repaint();
        }
        if !from_editor && (output.state.offset.y - old_offset).abs() > 0.01 {
            let position = self.preview_sync.position_for(output.state.offset.y);
            if position.differs(editor) {
                self.editor_view.set_scroll_position(position);
                ui.ctx().request_repaint();
            }
        }
    }

    /// One preview line that is not part of a table.
    fn preview_line(ui: &mut egui::Ui, line: &str, in_code: bool) {
        let trimmed = line.trim_start();
        if let Some(fence) = parse_fenced_code_marker(line) {
            if !in_code {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("code")
                            .font(FontId::new(12.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(136, 154, 176)),
                    );
                    if !fence.language.is_empty() {
                        ui.label(
                            RichText::new(fence.language)
                                .font(FontId::new(12.0, FontFamily::Monospace))
                                .color(Color32::from_rgb(235, 203, 139)),
                        );
                    }
                });
            }
        } else if in_code {
            egui::Frame::new()
                .fill(Color32::from_rgb(25, 31, 40))
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(line)
                            .font(FontId::new(14.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(216, 222, 233)),
                    );
                });
        } else if let Some(heading) = parse_heading_line(line) {
            let size = match heading.level {
                1 => 28.0,
                2 => 22.0,
                3 => 18.0,
                _ => 16.0,
            };
            let color = match heading.level {
                1 => Color32::from_rgb(235, 203, 139),
                2 => Color32::from_rgb(180, 142, 173),
                3 => Color32::from_rgb(136, 192, 208),
                _ => Color32::from_rgb(216, 222, 233),
            };
            ui.add_space(if heading.level <= 2 { 4.0 } else { 2.0 });
            ui.label(RichText::new(heading.text).size(size).strong().color(color));
            if heading.level <= 2 {
                ui.separator();
            }
        } else if is_markdown_separator(line) {
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);
        } else if let Some(blockquote) = parse_blockquote_line(line) {
            ui.horizontal(|ui| {
                ui.add_space((blockquote.depth.saturating_sub(1) as f32) * 8.0);
                let (bar_rect, _) =
                    ui.allocate_exact_size(Vec2::new(3.0, 20.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(bar_rect, 1.5, Color32::from_rgb(136, 192, 208));
                ui.add_space(8.0);
                ui.label(
                    RichText::new(blockquote.text)
                        .size(15.0)
                        .color(Color32::from_rgb(190, 200, 216)),
                );
            });
        } else if let Some(checkbox) = parse_checkbox_line(line) {
            ui.horizontal(|ui| {
                if !checkbox.indent.is_empty() {
                    ui.label(
                        RichText::new(checkbox.indent)
                            .font(FontId::new(15.0, FontFamily::Monospace)),
                    );
                }
                if !checkbox.task_prefix.is_empty() {
                    ui.label(
                        RichText::new(checkbox.task_prefix)
                            .font(FontId::new(15.0, FontFamily::Monospace))
                            .color(Color32::from_rgb(136, 154, 176)),
                    );
                }
                Self::checkbox_preview_icon(ui, checkbox.state);
                ui.label(RichText::new(checkbox.text).size(15.0));
            });
        } else if let Some(list) = parse_list_line(line) {
            ui.horizontal(|ui| {
                if !list.indent.is_empty() {
                    ui.label(
                        RichText::new(list.indent).font(FontId::new(15.0, FontFamily::Monospace)),
                    );
                }
                let marker = if list.ordered { list.marker } else { "•" };
                ui.label(
                    RichText::new(marker)
                        .font(FontId::new(15.0, FontFamily::Monospace))
                        .color(if list.ordered {
                            Color32::from_rgb(235, 203, 139)
                        } else {
                            Color32::from_rgb(136, 192, 208)
                        }),
                );
                ui.label(RichText::new(list.text).size(15.0));
            });
        } else if trimmed.is_empty() {
            ui.add_space(8.0);
        } else if !Self::markdown_link_preview_label(ui, line)
            && !Self::inline_code_preview_label(ui, line)
        {
            ui.label(RichText::new(line).size(15.0));
        }
    }
}
