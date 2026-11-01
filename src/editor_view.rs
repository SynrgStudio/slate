use crate::{
    editor_buffer::EditorBuffer,
    flow::{FlowItemKind, parse_flow_note_or_idea_line},
    markdown::{
        CheckboxState, TableAlignment, is_markdown_separator, is_markdown_table_start,
        markdown_link_target_at_byte, parse_blockquote_line, parse_checkbox_line,
        parse_fenced_code_marker, parse_heading_line, parse_inline_code_spans, parse_list_line,
        parse_markdown_link_spans, parse_markdown_table_separator, split_markdown_table_row,
    },
    search::SearchState,
};

use std::{cell::RefCell, collections::HashMap, path::Path, sync::Arc};

#[derive(Clone, Copy, PartialEq, Eq)]
struct RowLayoutKey {
    identity: u64,
    revision: u64,
    font_size: u32,
    width: u32,
    scale: u32,
    wrap: bool,
}
struct RowLayout {
    key: RowLayoutKey,
    rows: Arc<Vec<VisualRow>>,
    line_count: usize,
    text_len: usize,
    #[cfg(test)]
    recalculated_lines: usize,
}
struct FenceLayout {
    identity: u64,
    revision: u64,
    ranges: Vec<(usize, usize)>,
}

#[cfg(test)]
#[path = "editor_incremental_tests.rs"]
mod incremental_tests;
#[cfg(test)]
#[path = "editor_layout_tests.rs"]
mod layout_tests;

use chrono::{Datelike, Local, NaiveDate};
use eframe::egui::{self, Color32, FontFamily, FontId, Key, Pos2, Stroke};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineNumberMode {
    Absolute,
    Relative,
}

fn source_label_line_fragment(text: &str, start: usize, end: usize) -> Option<usize> {
    let label = text.get(start..end)?;
    let colon = label.rfind(':')?;
    let line = label.get(colon + 1..)?;
    if !line.is_empty() && line.chars().all(|ch| ch.is_ascii_digit()) {
        Some(start + colon)
    } else {
        None
    }
}

impl LineNumberMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            LineNumberMode::Absolute => "absolute",
            LineNumberMode::Relative => "relative",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        self.label()
    }

    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value.trim().trim_matches('"') {
            "absolute" => Some(LineNumberMode::Absolute),
            "relative" => Some(LineNumberMode::Relative),
            _ => None,
        }
    }

    pub(crate) fn number_for_line(self, line_index: usize, current_line_index: usize) -> usize {
        match self {
            LineNumberMode::Absolute => line_index + 1,
            LineNumberMode::Relative => {
                if line_index == current_line_index {
                    line_index + 1
                } else {
                    line_index.abs_diff(current_line_index)
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct VisualRow {
    line_index: usize,
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CodeLineKind<'a> {
    Fence { language: &'a str },
    Body,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableLineKind {
    Header,
    Separator,
    Body,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowMetadataKey {
    Scheduled,
    Due,
}

struct FlowMetadataLine {
    key: FlowMetadataKey,
    prefix: String,
    value: String,
}

pub(crate) const DEFAULT_EDITOR_FONT_SIZE: f32 = LINE_NUMBER_FONT_SIZE;
pub(crate) const LINE_NUMBER_FONT_SIZE: f32 = 12.0;
pub(crate) fn clamp_editor_font_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(8.0, 32.0)
    } else {
        DEFAULT_EDITOR_FONT_SIZE
    }
}

/// Name of the embedded JetBrains Mono family (assets/fonts, SIL OFL).
pub(crate) const JETBRAINS_MONO: &str = "JetBrains Mono";

/// The editor's text font. Both are monospaced; Hack is egui's built-in one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum EditorFont {
    #[default]
    Hack,
    JetBrainsMono,
}

impl EditorFont {
    pub(crate) fn family(self) -> FontFamily {
        match self {
            Self::Hack => FontFamily::Monospace,
            Self::JetBrainsMono => FontFamily::Name(JETBRAINS_MONO.into()),
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Hack => "hack",
            Self::JetBrainsMono => "jetbrains-mono",
        }
    }

    pub(crate) fn from_config(value: &str) -> Option<Self> {
        match value.trim().trim_matches('"') {
            "hack" => Some(Self::Hack),
            "jetbrains-mono" => Some(Self::JetBrainsMono),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Hack => "Hack",
            Self::JetBrainsMono => "JetBrains Mono",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Hack => Self::JetBrainsMono,
            Self::JetBrainsMono => Self::Hack,
        }
    }
}

/// Cursor colors offered in Settings (←/→ cycles them); any other color
/// can be picked with the swatch button.
pub(crate) const CURSOR_COLORS: [(&str, Color32); 7] = [
    ("snow", Color32::from_rgb(236, 239, 244)),
    ("frost", Color32::from_rgb(136, 192, 208)),
    ("green", Color32::from_rgb(163, 190, 140)),
    ("yellow", Color32::from_rgb(235, 203, 139)),
    ("orange", Color32::from_rgb(208, 135, 112)),
    ("red", Color32::from_rgb(191, 97, 106)),
    ("purple", Color32::from_rgb(180, 142, 173)),
];

/// How the editor text and caret look; set from Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EditorLook {
    pub(crate) font: EditorFont,
    /// A full-cell block over the character instead of a thin line.
    pub(crate) block_cursor: bool,
    /// The caret glides to its new place instead of jumping.
    pub(crate) animate_cursor: bool,
    /// A fading smear follows the gliding caret.
    pub(crate) cursor_trail: bool,
    /// 1–5: how long and how opaque the trail is.
    pub(crate) cursor_trail_weight: u8,
    /// A short trail also follows the caret while typing.
    pub(crate) cursor_trail_typing: bool,
    /// While gliding, the caret's leading edge runs ahead and its trailing
    /// edge lags, so it stretches toward the jump and snaps back on landing.
    pub(crate) cursor_stretch: bool,
    /// A ring expands and fades where the caret lands after a jump.
    pub(crate) cursor_pulse: bool,
    pub(crate) cursor_color: Color32,
}

pub(crate) const DEFAULT_TRAIL_WEIGHT: u8 = 3;

pub(crate) fn clamp_trail_weight(weight: u8) -> u8 {
    weight.clamp(1, 5)
}

impl Default for EditorLook {
    fn default() -> Self {
        Self {
            font: EditorFont::default(),
            block_cursor: false,
            animate_cursor: false,
            cursor_trail: false,
            cursor_trail_weight: DEFAULT_TRAIL_WEIGHT,
            cursor_trail_typing: false,
            cursor_stretch: false,
            cursor_pulse: false,
            cursor_color: CURSOR_COLORS[0].1,
        }
    }
}

/// `#rrggbb`, as saved in config.toml.
pub(crate) fn color_to_hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

pub(crate) fn color_from_hex(value: &str) -> Option<Color32> {
    let hex = value.trim().trim_matches('"').strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

/// The next (or previous) preset after `color`; custom colors start at snow.
pub(crate) fn cycle_cursor_color(color: Color32, forward: bool) -> Color32 {
    let count = CURSOR_COLORS.len();
    let index = CURSOR_COLORS.iter().position(|(_, c)| *c == color);
    let next = match (index, forward) {
        (None, _) => 0,
        (Some(i), true) => (i + 1) % count,
        (Some(i), false) => (i + count - 1) % count,
    };
    CURSOR_COLORS[next].1
}

/// How long the caret takes to glide to a new place.
const CURSOR_GLIDE_SECONDS: f64 = 0.09;

/// With the trail on, the caret still lands after `CURSOR_GLIDE_SECONDS`;
/// the trail's tail catches up later, the heavier the trail the later.
fn trail_seconds(weight: u8) -> f64 {
    CURSOR_GLIDE_SECONDS + 0.035 * f64::from(clamp_trail_weight(weight))
}

/// With stretch on, the leading corners land at 0.75× the glide time and the
/// trailing ones at 1.5×.
const STRETCH_LEAD: f64 = 0.75;
const STRETCH_LAG: f64 = 1.5;
/// How long the landing ring takes to expand and fade.
const CURSOR_PULSE_SECONDS: f64 = 0.25;

/// Opacity of the trail right behind the caret.
fn trail_opacity(weight: u8) -> f32 {
    0.12 + 0.11 * f32::from(clamp_trail_weight(weight))
}

/// A caret animation in progress, in content coordinates.
#[derive(Clone, Copy)]
struct CursorGlide {
    head_from: Pos2,
    tail_from: Pos2,
    started: f64,
    /// Typing: the caret itself moves at once; only the trail follows.
    instant_head: bool,
}

pub(crate) struct EditorView {
    font_size: f32,
    font_size_changed: bool,
    look: EditorLook,
    /// Where the caret is going and where it was last drawn, in content
    /// coordinates (x from the text area's left edge, y from the document
    /// top), so scrolling never animates it.
    cursor_target: Option<Pos2>,
    cursor_drawn: Option<Pos2>,
    cursor_glide: Option<CursorGlide>,
    /// When the landing ring starts (it may be scheduled for the end of a glide).
    cursor_pulse_at: Option<f64>,
    /// Where the trail's tail was last drawn, so a new keystroke continues
    /// the trail instead of restarting it.
    cursor_tail: Option<Pos2>,
    row_layout: RefCell<Option<RowLayout>>,
    fence_layout: RefCell<Option<FenceLayout>>,
    scroll_y: f32,
    scroll_geometry: (f32, f32),
    pending_scroll_position: Option<crate::scroll_sync::Position>,
    target_cursor: Option<usize>,
    link_click_byte: Option<usize>,
    link_click_target: Option<String>,
    virtual_line_click: Option<String>,
    link_assist_trigger_start: Option<usize>,
    focus_next: bool,
    mouse_selection_anchor: Option<usize>,
    flow_metadata_month_offset: i32,
    flow_agenda_update_requested: bool,
    last_cursor_pos: Option<Pos2>,
    last_text_input: Option<String>,
}

fn virtual_tail_buffer(buffer: &EditorBuffer, tail: &str) -> EditorBuffer {
    let mut text = buffer.as_str().to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push('\n');
    text.push_str(tail.trim_start_matches('\n'));
    let cursor = buffer.cursor();
    let mut display = EditorBuffer::new();
    display.set_text(text);
    display.set_cursor(cursor);
    display
}

impl EditorView {
    pub(crate) fn new() -> Self {
        Self {
            font_size: DEFAULT_EDITOR_FONT_SIZE,
            font_size_changed: false,
            look: EditorLook::default(),
            cursor_target: None,
            cursor_drawn: None,
            cursor_glide: None,
            cursor_pulse_at: None,
            cursor_tail: None,
            row_layout: RefCell::new(None),
            fence_layout: RefCell::new(None),
            scroll_y: 0.0,
            scroll_geometry: (1.0, 0.0),
            pending_scroll_position: None,
            target_cursor: None,
            link_click_byte: None,
            link_click_target: None,
            virtual_line_click: None,
            link_assist_trigger_start: None,
            focus_next: false,
            mouse_selection_anchor: None,
            flow_metadata_month_offset: 0,
            flow_agenda_update_requested: false,
            last_cursor_pos: None,
            last_text_input: None,
        }
    }

    pub(crate) fn scroll_position(&self) -> crate::scroll_sync::Position {
        let cache = self.row_layout.borrow();
        let Some(layout) = cache.as_ref() else {
            return Default::default();
        };
        let height = self.scroll_geometry.0;
        let max = (layout.rows.len() as f32 * height - self.scroll_geometry.1).max(0.0);
        let index = (self.scroll_y / height).floor().max(0.0) as usize;
        let Some(row) = layout
            .rows
            .get(index.min(layout.rows.len().saturating_sub(1)))
        else {
            return Default::default();
        };
        let first = layout
            .rows
            .partition_point(|r| r.line_index < row.line_index);
        let end = layout
            .rows
            .partition_point(|r| r.line_index <= row.line_index);
        crate::scroll_sync::Position {
            source: row.line_index as f32
                + ((self.scroll_y / height - first as f32) / (end - first).max(1) as f32)
                    .clamp(0.0, 1.0),
            at_end: max > 0.0 && self.scroll_y >= max - 0.5,
        }
    }
    pub(crate) fn set_scroll_position(&mut self, position: crate::scroll_sync::Position) {
        self.pending_scroll_position = Some(position);
        self.target_cursor = None;
    }
    fn apply_scroll_position(
        &mut self,
        rows: &[VisualRow],
        position: crate::scroll_sync::Position,
        line_height: f32,
        max: f32,
    ) {
        if position.at_end {
            self.scroll_y = max;
            return;
        }
        let line = position.source.floor().max(0.0) as usize;
        let first = rows.partition_point(|r| r.line_index < line);
        let end = rows.partition_point(|r| r.line_index <= line);
        self.scroll_y = ((first as f32 + position.source.fract().max(0.0) * (end - first) as f32)
            * line_height)
            .clamp(0.0, max);
    }

    pub(crate) fn take_flow_agenda_update_request(&mut self) -> bool {
        let requested = self.flow_agenda_update_requested;
        self.flow_agenda_update_requested = false;
        requested
    }

    pub(crate) fn request_keyboard_focus(&mut self) {
        self.focus_next = true;
    }

    pub(crate) fn last_cursor_pos(&self) -> Option<Pos2> {
        self.last_cursor_pos
    }

    pub(crate) fn take_last_text_input(&mut self) -> Option<String> {
        self.last_text_input.take()
    }

    pub(crate) fn set_font_size(&mut self, size: f32) {
        let size = clamp_editor_font_size(size);
        if self.font_size != size {
            self.font_size = size;
            self.font_size_changed = true;
        }
    }

    pub(crate) fn set_look(&mut self, look: EditorLook) {
        if look.font != self.look.font {
            // Cached rows were measured with the other font.
            *self.row_layout.borrow_mut() = None;
            *self.fence_layout.borrow_mut() = None;
            self.font_size_changed = true;
        }
        if !look.animate_cursor {
            self.cursor_glide = None;
        }
        self.look = look;
    }

    pub(crate) fn observe_buffer(&mut self, buffer: &EditorBuffer) {
        if self
            .target_cursor
            .map(|cursor| cursor > buffer.as_str().len())
            .unwrap_or(false)
        {
            self.target_cursor = Some(buffer.as_str().len());
        }
        self.scroll_y = self.scroll_y.max(0.0);
    }

    #[allow(dead_code)]
    pub(crate) fn visible_line_range(
        &self,
        buffer: &EditorBuffer,
        viewport_height: f32,
        line_height: f32,
    ) -> std::ops::Range<usize> {
        self.visible_row_range(buffer.line_count(), viewport_height, line_height)
    }

    pub(crate) fn visible_row_range(
        &self,
        row_count: usize,
        viewport_height: f32,
        line_height: f32,
    ) -> std::ops::Range<usize> {
        let line_height = line_height.max(1.0);
        let row_count = row_count.max(1);
        let first = (self.scroll_y / line_height).floor().max(0.0) as usize;
        let visible = (viewport_height / line_height).ceil().max(1.0) as usize + 1;
        let start = first.min(row_count.saturating_sub(1));
        let end = (start + visible).min(row_count);
        start..end
    }

    pub(crate) fn render(
        &mut self,
        ui: &mut egui::Ui,
        buffer: &mut EditorBuffer,
        wrap: bool,
        search_state: Option<&SearchState>,
        line_number_mode: LineNumberMode,
        keyboard_enabled: bool,
        force_keyboard_focus: bool,
        active_line_text_highlight: Option<usize>,
        render_markdown: bool,
        virtual_tail: Option<&str>,
    ) -> (egui::Response, bool) {
        let render_started = (buffer.as_str().len() >= 1_000_000).then(std::time::Instant::now);
        self.observe_buffer(buffer);
        if self.font_size_changed {
            self.target_cursor = Some(buffer.cursor());
            self.font_size_changed = false;
        }
        self.last_cursor_pos = None;
        self.last_text_input = None;

        let font = FontId::new(self.font_size, self.look.font.family());
        let gutter_font = FontId::new(LINE_NUMBER_FONT_SIZE, FontFamily::Monospace);
        let line_height =
            ui.fonts_mut(|fonts| fonts.row_height(&font).max(fonts.row_height(&gutter_font))) + 2.0;
        let available = ui.available_size();
        let (rect, _) = ui.allocate_exact_size(available, egui::Sense::hover());
        let view_id = ui
            .id()
            .with(("native_editor_view", self as *const Self as usize));
        let response = ui
            .interact(rect, view_id, egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Text);
        let requested_focus = self.focus_next;
        if requested_focus {
            response.request_focus();
            ui.memory_mut(|memory| memory.request_focus(response.id));
            self.focus_next = false;
        }
        let mut has_keyboard_focus =
            force_keyboard_focus || response.has_focus() || requested_focus;
        let painter = ui.painter_at(rect);
        let real_line_count = buffer.line_count();
        let line_number_digits = real_line_count.max(1).to_string().len().max(2);
        let digit_width = self.text_width(&painter, "0", &gutter_font);
        let gutter_width = (line_number_digits as f32 * digit_width + 18.0).max(28.0);
        let scrollbar_reserved_width = 12.0;
        let content_right = rect.right() - scrollbar_reserved_width;
        let text_x = rect.left() + gutter_width + 6.0;
        let gutter_x = rect.left() + gutter_width - 6.0;
        let wrap_width = (content_right - text_x - 8.0).max(40.0);
        let mut display_buffer_storage = virtual_tail.map(|tail| virtual_tail_buffer(buffer, tail));
        let mut rows = self.cached_visual_rows(
            &painter,
            display_buffer_storage.as_ref().unwrap_or(buffer),
            &font,
            wrap_width,
            wrap,
        );
        let mut max_scroll = (rows.len() as f32 * line_height - rect.height()).max(0.0);
        self.scroll_geometry = (line_height, rect.height());
        if let Some(position) = self.pending_scroll_position.take() {
            self.apply_scroll_position(&rows, position, line_height, max_scroll);
        }
        let scrollbar_track_rect = egui::Rect::from_min_max(
            egui::pos2(rect.right() - 9.0, rect.top() + 4.0),
            egui::pos2(rect.right() - 4.0, rect.bottom() - 4.0),
        );
        let mut changed = false;

        if response.clicked() {
            response.request_focus();
            has_keyboard_focus = true;
            if let Some(pos) = response.interact_pointer_pos() {
                if max_scroll > 0.0 && scrollbar_track_rect.contains(pos) {
                    let ratio = ((pos.y - scrollbar_track_rect.top())
                        / scrollbar_track_rect.height())
                    .clamp(0.0, 1.0);
                    self.scroll_y = (ratio * max_scroll).clamp(0.0, max_scroll);
                } else {
                    let display_buffer = display_buffer_storage.as_ref().unwrap_or(buffer);
                    let row = self.row_for_pointer(display_buffer, &rows, rect, line_height, pos.y);
                    let byte =
                        self.byte_for_x(&painter, display_buffer, row, pos.x - text_x, &font);
                    let clicked_link = markdown_link_target_at_byte(
                        display_buffer.line(row.line_index),
                        display_buffer.line_start(row.line_index),
                        byte,
                    );
                    if let Some(target) = clicked_link {
                        if row.line_index >= real_line_count {
                            self.link_click_target = Some(target.to_string());
                        } else {
                            self.link_click_byte = Some(byte);
                        }
                    } else if row.line_index >= real_line_count {
                        let line = display_buffer.line(row.line_index).trim().to_string();
                        if line.contains("SLATE REFERENCES")
                            || line.contains("virtual references")
                            || line.contains("Backlinks")
                            || line.contains("Mentions")
                        {
                            self.virtual_line_click = Some(line);
                        }
                    } else {
                        buffer.set_cursor(byte);
                        self.request_scroll_to_cursor(buffer);
                    }
                }
            }
        }

        if response.drag_started() {
            response.request_focus();
            has_keyboard_focus = true;
            if let Some(pos) = response.interact_pointer_pos() {
                if max_scroll > 0.0 && scrollbar_track_rect.contains(pos) {
                    self.mouse_selection_anchor = None;
                    let ratio = ((pos.y - scrollbar_track_rect.top())
                        / scrollbar_track_rect.height())
                    .clamp(0.0, 1.0);
                    self.scroll_y = (ratio * max_scroll).clamp(0.0, max_scroll);
                } else {
                    let row = self.row_for_pointer(buffer, &rows, rect, line_height, pos.y);
                    let byte = self.byte_for_x(&painter, buffer, row, pos.x - text_x, &font);
                    self.mouse_selection_anchor = Some(byte);
                    buffer.set_cursor(byte);
                }
            }
        }

        if response.dragged() {
            has_keyboard_focus = true;
            if let Some(pos) = response.interact_pointer_pos() {
                if max_scroll > 0.0 && scrollbar_track_rect.contains(pos) {
                    self.mouse_selection_anchor = None;
                    let ratio = ((pos.y - scrollbar_track_rect.top())
                        / scrollbar_track_rect.height())
                    .clamp(0.0, 1.0);
                    self.scroll_y = (ratio * max_scroll).clamp(0.0, max_scroll);
                } else if let Some(anchor) = self.mouse_selection_anchor {
                    let row = self.row_for_pointer(buffer, &rows, rect, line_height, pos.y);
                    let byte = self.byte_for_x(&painter, buffer, row, pos.x - text_x, &font);
                    buffer.set_selection(anchor, byte);
                }
            }
        }

        if response.drag_stopped() {
            self.mouse_selection_anchor = None;
        }

        // Wheel ownership follows the pointer, not keyboard focus. Otherwise a
        // focused editor also scrolls when the pointer is over the preview.
        if response.hovered() {
            let scroll_delta = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll_delta != 0.0 {
                self.scroll_y = (self.scroll_y - scroll_delta).clamp(0.0, max_scroll);
            }
        }

        if has_keyboard_focus {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }

        if keyboard_enabled && has_keyboard_focus && search_state.is_none() {
            changed = self.handle_keyboard(ui, buffer) || changed;
            if changed {
                display_buffer_storage = virtual_tail.map(|tail| virtual_tail_buffer(buffer, tail));
                rows = self.cached_visual_rows(
                    &painter,
                    display_buffer_storage.as_ref().unwrap_or(buffer),
                    &font,
                    wrap_width,
                    wrap,
                );
                max_scroll = (rows.len() as f32 * line_height - rect.height()).max(0.0);
            }
        }

        if let Some(target) = self.target_cursor.take() {
            let row_index = self.row_index_for_byte(&rows, target);
            let y = row_index as f32 * line_height;
            if y < self.scroll_y {
                self.scroll_y = y;
            } else if y + line_height > self.scroll_y + rect.height() {
                self.scroll_y = (y + line_height - rect.height()).max(0.0);
            }
        }
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);

        painter.rect_filled(rect, 0.0, Color32::from_rgb(30, 36, 48));
        if max_scroll > 0.0 && scrollbar_track_rect.height() > 0.0 {
            painter.rect_filled(scrollbar_track_rect, 0.0, Color32::from_rgb(25, 31, 40));
            let content_height = rows.len() as f32 * line_height;
            let thumb_height = (rect.height() / content_height * scrollbar_track_rect.height())
                .clamp(24.0, scrollbar_track_rect.height());
            let thumb_range = (scrollbar_track_rect.height() - thumb_height).max(0.0);
            let thumb_top = scrollbar_track_rect.top()
                + if max_scroll > 0.0 {
                    (self.scroll_y / max_scroll).clamp(0.0, 1.0) * thumb_range
                } else {
                    0.0
                };
            let thumb_rect = egui::Rect::from_min_size(
                egui::pos2(scrollbar_track_rect.left(), thumb_top),
                egui::vec2(scrollbar_track_rect.width(), thumb_height),
            );
            painter.rect_filled(thumb_rect, 0.0, Color32::from_rgb(136, 154, 176));
        }

        let display_buffer = display_buffer_storage.as_ref().unwrap_or(buffer);
        let range = self.visible_row_range(rows.len(), rect.height(), line_height);
        let y_offset = self.scroll_y % line_height;
        let current_line_index = buffer.cursor_line_col().0;
        for (visible_index, row_index) in range.enumerate() {
            let Some(row) = rows.get(row_index).copied() else {
                continue;
            };
            let y = rect.top() + visible_index as f32 * line_height - y_offset;
            if y > rect.bottom() || y + line_height < rect.top() {
                continue;
            }

            let line_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), y),
                egui::vec2(content_right - rect.left(), line_height),
            );
            let is_virtual_row = row.line_index >= real_line_count;
            if is_virtual_row {
                let virtual_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 2.0, y),
                    egui::pos2(content_right - 2.0, y + line_height),
                );
                painter.rect_filled(virtual_rect, 0.0, Color32::from_rgb(25, 31, 42));
                painter.line_segment(
                    [
                        egui::pos2(virtual_rect.left(), virtual_rect.top()),
                        egui::pos2(virtual_rect.right(), virtual_rect.top()),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgb(74, 89, 113)),
                );
                painter.line_segment(
                    [
                        egui::pos2(virtual_rect.left(), virtual_rect.bottom()),
                        egui::pos2(virtual_rect.right(), virtual_rect.bottom()),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgb(44, 55, 72)),
                );
                painter.line_segment(
                    [
                        egui::pos2(virtual_rect.left(), virtual_rect.top()),
                        egui::pos2(virtual_rect.left(), virtual_rect.bottom()),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgb(74, 89, 113)),
                );
                painter.line_segment(
                    [
                        egui::pos2(virtual_rect.right(), virtual_rect.top()),
                        egui::pos2(virtual_rect.right(), virtual_rect.bottom()),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgb(74, 89, 113)),
                );
            } else if row.line_index % 2 == 0 {
                painter.rect_filled(line_rect, 0.0, Color32::from_rgb(29, 35, 46));
            }

            if !is_virtual_row {
                self.paint_markdown_line_background(
                    &painter,
                    display_buffer,
                    row,
                    text_x,
                    y,
                    line_height,
                    content_right,
                    render_markdown,
                    &font,
                );
            }
            if active_line_text_highlight == Some(row.line_index) && !is_virtual_row {
                let highlight_rect = egui::Rect::from_min_max(
                    egui::pos2(text_x - 8.0, y + 1.0),
                    egui::pos2(content_right - 6.0, y + line_height - 1.0),
                );
                painter.rect_stroke(
                    highlight_rect,
                    0.0,
                    Stroke::new(1.0_f32, Color32::from_rgb(235, 203, 139)),
                    egui::StrokeKind::Inside,
                );
            }

            if !is_virtual_row
                && let Some(search_state) =
                    search_state.filter(|state| state.buffer_revision == buffer.revision)
            {
                self.paint_search_matches(
                    &painter,
                    buffer,
                    row,
                    search_state,
                    text_x,
                    y,
                    line_height,
                    &font,
                    content_right,
                );
            }

            if !is_virtual_row && let Some((selection_start, selection_end)) = buffer.selection() {
                self.paint_selection(
                    &painter,
                    buffer,
                    row,
                    selection_start,
                    selection_end,
                    text_x,
                    y,
                    line_height,
                    &font,
                    content_right,
                );
            }

            painter.text(
                egui::pos2(gutter_x, y + line_height * 0.5),
                egui::Align2::RIGHT_CENTER,
                if is_virtual_row {
                    "·".to_string()
                } else if row.start == buffer.line_start(row.line_index) {
                    format!(
                        "{}",
                        line_number_mode.number_for_line(row.line_index, current_line_index)
                    )
                } else {
                    "·".to_string()
                },
                gutter_font.clone(),
                Color32::from_rgb(94, 105, 126),
            );
            let text_color = if is_virtual_row {
                Color32::from_rgb(160, 176, 198)
            } else if active_line_text_highlight == Some(row.line_index) {
                Color32::from_rgb(235, 203, 139)
            } else {
                Color32::from_rgb(216, 222, 233)
            };
            self.paint_line_text(
                &painter,
                display_buffer,
                row,
                text_x,
                y,
                line_height,
                &font,
                text_color,
                render_markdown,
            );
            if is_virtual_row {
                continue;
            }
            self.paint_flow_metadata_suggestion(
                &painter,
                buffer,
                row,
                text_x,
                y,
                line_height,
                &font,
                render_markdown,
            );
        }

        if keyboard_enabled && has_keyboard_focus {
            self.paint_cursor(
                &painter,
                buffer,
                &rows,
                rect,
                text_x,
                line_height,
                &font,
            );
        } else {
            self.forget_cursor_position();
        }

        if let Some(started) = render_started {
            let elapsed = started.elapsed();
            if elapsed.as_millis() >= 16 {
                crate::perf_log::mark(
                    "editor.render.slow",
                    &format!(
                        "bytes={} cpu_ms={:.3} wrap={wrap} changed={changed}",
                        buffer.as_str().len(),
                        elapsed.as_secs_f64() * 1000.0
                    ),
                );
            }
        }
        (response, changed)
    }

    fn cached_visual_rows(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        font: &FontId,
        wrap_width: f32,
        wrap: bool,
    ) -> Arc<Vec<VisualRow>> {
        let key = RowLayoutKey {
            identity: buffer.identity,
            revision: buffer.revision,
            font_size: font.size.to_bits(),
            width: wrap_width.to_bits(),
            scale: painter.ctx().pixels_per_point().to_bits(),
            wrap,
        };
        if let Some(layout) = self
            .row_layout
            .borrow()
            .as_ref()
            .filter(|layout| layout.key == key)
        {
            return layout.rows.clone();
        }
        let started = std::time::Instant::now();
        let previous = self.row_layout.borrow_mut().take();
        let compatible = previous.as_ref().is_some_and(|old| {
            old.key.identity == key.identity
                && old.key.font_size == key.font_size
                && old.key.width == key.width
                && old.key.scale == key.scale
                && old.key.wrap == key.wrap
        });
        let edges = if compatible {
            buffer.unchanged_layout_edges(previous.as_ref().unwrap().key.revision)
        } else {
            None
        };
        let (rows, recalculated_lines) = if let (Some(old), Some((prefix, suffix))) =
            (previous.as_ref(), edges)
        {
            let prefix = prefix.min(old.line_count).min(buffer.line_count());
            let suffix = suffix
                .min(old.line_count - prefix)
                .min(buffer.line_count() - prefix);
            let first = old.rows.partition_point(|row| row.line_index < prefix);
            let last = old
                .rows
                .partition_point(|row| row.line_index < old.line_count - suffix);
            let dirty = prefix..buffer.line_count() - suffix;
            let recalculated = dirty.len();
            let mut rows = Vec::with_capacity(old.rows.len());
            rows.extend_from_slice(&old.rows[..first]);
            rows.extend(self.visual_rows_for_lines(painter, buffer, font, wrap_width, wrap, dirty));
            let byte_delta = buffer.as_str().len() as isize - old.text_len as isize;
            let line_delta = buffer.line_count() as isize - old.line_count as isize;
            rows.extend(old.rows[last..].iter().map(|row| {
                VisualRow {
                    line_index: row
                        .line_index
                        .checked_add_signed(line_delta)
                        .expect("valid reused line"),
                    start: row
                        .start
                        .checked_add_signed(byte_delta)
                        .expect("valid reused row start"),
                    end: row
                        .end
                        .checked_add_signed(byte_delta)
                        .expect("valid reused row end"),
                }
            }));
            (Arc::new(rows), recalculated)
        } else {
            (
                Arc::new(self.visual_rows(painter, buffer, font, wrap_width, wrap)),
                buffer.line_count(),
            )
        };
        if buffer.as_str().len() >= 1_000_000 {
            crate::perf_log::mark(
                "editor.layout.rebuild",
                &format!(
                    "bytes={} lines={} rows={} recalculated_lines={} cpu_ms={:.3} wrap={wrap}",
                    buffer.as_str().len(),
                    buffer.line_count(),
                    rows.len(),
                    recalculated_lines,
                    started.elapsed().as_secs_f64() * 1000.0
                ),
            );
        }
        *self.row_layout.borrow_mut() = Some(RowLayout {
            key,
            rows: rows.clone(),
            line_count: buffer.line_count(),
            text_len: buffer.as_str().len(),
            #[cfg(test)]
            recalculated_lines,
        });
        rows
    }

    pub(crate) fn visual_rows(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        font: &FontId,
        wrap_width: f32,
        wrap: bool,
    ) -> Vec<VisualRow> {
        self.visual_rows_for_lines(
            painter,
            buffer,
            font,
            wrap_width,
            wrap,
            0..buffer.line_count(),
        )
    }

    fn visual_rows_for_lines(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        font: &FontId,
        wrap_width: f32,
        wrap: bool,
        lines: std::ops::Range<usize>,
    ) -> Vec<VisualRow> {
        let mut rows = Vec::with_capacity(lines.len());
        let mut widths = HashMap::new();
        let mut ascii_widths = [None; 128];
        painter.ctx().fonts_mut(|fonts| {
            for line_index in lines {
                let line_start = buffer.line_start(line_index);
                let line_end = buffer.line_end(line_index);
                let line = buffer.line(line_index);
                if !wrap || line.is_empty() {
                    rows.push(VisualRow {
                        line_index,
                        start: line_start,
                        end: line_end,
                    });
                    continue;
                }

                let mut start_offset = 0;
                let mut width = 0.0;
                for (offset, ch) in line.char_indices() {
                    let advance = if ch.is_ascii() {
                        *ascii_widths[ch as usize]
                            .get_or_insert_with(|| fonts.glyph_width(font, ch))
                    } else {
                        *widths
                            .entry(ch)
                            .or_insert_with(|| fonts.glyph_width(font, ch))
                    };
                    if offset > start_offset && width + advance > wrap_width {
                        rows.push(VisualRow {
                            line_index,
                            start: line_start + start_offset,
                            end: line_start + offset,
                        });
                        start_offset = offset;
                        width = 0.0;
                    }
                    width += advance;
                }
                rows.push(VisualRow {
                    line_index,
                    start: line_start + start_offset,
                    end: line_end,
                });
            }
        });
        rows
    }

    pub(crate) fn row_index_for_byte(&self, rows: &[VisualRow], byte: usize) -> usize {
        rows.iter()
            .position(|row| byte >= row.start && byte <= row.end)
            .unwrap_or_else(|| rows.len().saturating_sub(1))
    }

    fn cursor_near_flow_item_context(buffer: &EditorBuffer) -> bool {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        line_index
            .checked_sub(1)
            .into_iter()
            .chain(std::iter::once(line_index))
            .chain(std::iter::once(line_index + 1))
            .any(|index| {
                let line = buffer.line(index);
                parse_checkbox_line(line).is_some()
                    || parse_flow_note_or_idea_line(line, index, None).is_some()
            })
    }

    pub(crate) fn handle_keyboard(&mut self, ui: &egui::Ui, buffer: &mut EditorBuffer) -> bool {
        let mut changed = false;
        let mut moved = false;
        let events = ui.input(|input| input.events.clone());
        let tab_indent_action = events.iter().find_map(|event| match event {
            egui::Event::Key {
                key: Key::Tab,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if !modifiers.ctrl && !modifiers.command && !modifiers.alt => {
                Some(if modifiers.shift { -1 } else { 1 })
            }
            egui::Event::Text(text) if text == "\t" => Some(1),
            egui::Event::Ime(egui::ImeEvent::Commit(text)) if text == "\t" => Some(1),
            _ => None,
        });
        let mut handled_tab_indent = false;
        if let Some(tab_indent_action) = tab_indent_action {
            let was_near_flow_item = Self::cursor_near_flow_item_context(buffer);
            let indented = if tab_indent_action < 0 {
                buffer.dedent_current_or_selected_lines()
            } else {
                buffer.indent_current_or_selected_lines()
            };
            if indented && (was_near_flow_item || Self::cursor_near_flow_item_context(buffer)) {
                self.flow_agenda_update_requested = true;
            }
            if indented {
                self.request_scroll_to_cursor(buffer);
            }
            changed = indented || changed;
            handled_tab_indent = true;
        }
        if !handled_tab_indent
            && !ui.input(|input| {
                input.modifiers.ctrl || input.modifiers.command || input.modifiers.alt
            })
        {
            if let Some(text) = text_input_from_events(&events) {
                self.flow_metadata_month_offset = 0;
                self.last_text_input = Some(text.clone());
                let was_near_flow_item = Self::cursor_near_flow_item_context(buffer);
                self.insert_text_with_checkbox_expansion(buffer, &text);
                if was_near_flow_item || Self::cursor_near_flow_item_context(buffer) {
                    self.flow_agenda_update_requested = true;
                }
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }
        }
        for event in events {
            match event {
                egui::Event::Copy => {
                    if let Some((start, end)) = buffer.selection() {
                        ui.ctx().copy_text(buffer.as_str()[start..end].to_string());
                    }
                }
                egui::Event::Paste(text) => {
                    let text = Self::normalize_paste_text(&text);
                    buffer.insert_text(&text);
                    self.request_scroll_to_cursor(buffer);
                    changed = true;
                }
                _ => {}
            }
        }

        ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::CTRL, Key::Enter) {
                let cycled = Self::cycle_current_line_checkbox(buffer);
                if cycled {
                    self.flow_agenda_update_requested = true;
                }
                changed = cycled || changed;
            }
            if input.consume_key(egui::Modifiers::SHIFT, Key::Enter) {
                let inserted = self.insert_flow_task_metadata_start(buffer);
                if inserted {
                    self.flow_agenda_update_requested = true;
                }
                changed = inserted || changed;
                self.request_scroll_to_cursor(buffer);
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Enter) {
                let was_near_flow_task = Self::cursor_near_flow_item_context(buffer);
                let tagged_flow_line = Self::ensure_current_flow_line_default_tag(buffer);
                if !self.handle_flow_metadata_enter(buffer)
                    && !Self::insert_space_after_markdown_link_label(buffer)
                    && !self.insert_predictive_list_newline(buffer)
                {
                    buffer.insert_newline();
                }
                if tagged_flow_line
                    || was_near_flow_task
                    || Self::cursor_near_flow_item_context(buffer)
                {
                    self.flow_agenda_update_requested = true;
                }
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }
            if input.consume_key(egui::Modifiers::CTRL, Key::Backspace) {
                let was_near_flow_task = Self::cursor_near_flow_item_context(buffer);
                let deleted = buffer.delete_word_before_cursor();
                if deleted && (was_near_flow_task || Self::cursor_near_flow_item_context(buffer)) {
                    self.flow_agenda_update_requested = true;
                }
                self.request_scroll_to_cursor(buffer);
                changed = deleted || changed;
            }
            if input.consume_key(egui::Modifiers::CTRL, Key::Delete) {
                let was_near_flow_task = Self::cursor_near_flow_item_context(buffer);
                let deleted = buffer.delete_word_after_cursor();
                if deleted && (was_near_flow_task || Self::cursor_near_flow_item_context(buffer)) {
                    self.flow_agenda_update_requested = true;
                }
                self.request_scroll_to_cursor(buffer);
                changed = deleted || changed;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Backspace) {
                let was_near_flow_task = Self::cursor_near_flow_item_context(buffer);
                buffer.backspace();
                if was_near_flow_task || Self::cursor_near_flow_item_context(buffer) {
                    self.flow_agenda_update_requested = true;
                }
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Delete) {
                let was_near_flow_task = Self::cursor_near_flow_item_context(buffer);
                buffer.delete();
                if was_near_flow_task || Self::cursor_near_flow_item_context(buffer) {
                    self.flow_agenda_update_requested = true;
                }
                self.request_scroll_to_cursor(buffer);
                changed = true;
            }

            let ctrl_shift_left = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::ArrowLeft,
            );
            if ctrl_shift_left || input.consume_key(egui::Modifiers::CTRL, Key::ArrowLeft) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_left {
                    buffer.clear_selection();
                }
                buffer.move_to_word_start_left();
                Self::extend_selection(buffer, anchor, ctrl_shift_left);
                moved = true;
            }

            let ctrl_shift_right = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::ArrowRight,
            );
            if ctrl_shift_right || input.consume_key(egui::Modifiers::CTRL, Key::ArrowRight) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_right {
                    buffer.clear_selection();
                }
                buffer.move_to_word_end_right();
                Self::extend_selection(buffer, anchor, ctrl_shift_right);
                moved = true;
            }

            let shift_left = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowLeft);
            if shift_left || input.consume_key(egui::Modifiers::NONE, Key::ArrowLeft) {
                let anchor = Self::selection_anchor(buffer);
                if shift_left {
                    buffer.clear_selection();
                }
                buffer.move_left();
                Self::extend_selection(buffer, anchor, shift_left);
                moved = true;
            }

            let shift_right = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowRight);
            if shift_right || input.consume_key(egui::Modifiers::NONE, Key::ArrowRight) {
                let anchor = Self::selection_anchor(buffer);
                if shift_right {
                    buffer.clear_selection();
                }
                buffer.move_right();
                Self::extend_selection(buffer, anchor, shift_right);
                moved = true;
            }

            if self.current_flow_metadata_suggestion(buffer).is_some()
                && input.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
            {
                self.adjust_flow_metadata_month(buffer, 1);
                moved = true;
            }

            let ctrl_shift_up = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::ArrowUp,
            );
            if ctrl_shift_up || input.consume_key(egui::Modifiers::CTRL, Key::ArrowUp) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_up {
                    buffer.clear_selection();
                }
                let moved_to_paragraph = buffer.move_to_paragraph_start();
                Self::extend_selection(buffer, anchor, ctrl_shift_up);
                moved = moved || moved_to_paragraph;
            }

            let shift_up = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowUp);
            if shift_up || input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                let anchor = Self::selection_anchor(buffer);
                if shift_up {
                    buffer.clear_selection();
                }
                buffer.move_up();
                Self::extend_selection(buffer, anchor, shift_up);
                moved = true;
            }

            if self.current_flow_metadata_suggestion(buffer).is_some()
                && input.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
            {
                self.adjust_flow_metadata_month(buffer, -1);
                moved = true;
            }

            let ctrl_shift_down = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::ArrowDown,
            );
            if ctrl_shift_down || input.consume_key(egui::Modifiers::CTRL, Key::ArrowDown) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_down {
                    buffer.clear_selection();
                }
                let moved_to_paragraph = buffer.move_to_paragraph_end();
                Self::extend_selection(buffer, anchor, ctrl_shift_down);
                moved = moved || moved_to_paragraph;
            }

            let shift_down = input.consume_key(egui::Modifiers::SHIFT, Key::ArrowDown);
            if shift_down || input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                let anchor = Self::selection_anchor(buffer);
                if shift_down {
                    buffer.clear_selection();
                }
                buffer.move_down();
                Self::extend_selection(buffer, anchor, shift_down);
                moved = true;
            }

            let ctrl_shift_home = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::Home,
            );
            let ctrl_shift_end = input.consume_key(
                egui::Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: true,
                    mac_cmd: false,
                    command: false,
                },
                Key::End,
            );
            if ctrl_shift_home || input.consume_key(egui::Modifiers::CTRL, Key::Home) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_home {
                    buffer.clear_selection();
                }
                buffer.move_to_top();
                Self::extend_selection(buffer, anchor, ctrl_shift_home);
                moved = true;
            }
            if ctrl_shift_end || input.consume_key(egui::Modifiers::CTRL, Key::End) {
                let anchor = Self::selection_anchor(buffer);
                if ctrl_shift_end {
                    buffer.clear_selection();
                }
                buffer.move_to_bottom();
                Self::extend_selection(buffer, anchor, ctrl_shift_end);
                moved = true;
            }

            let shift_home = input.consume_key(egui::Modifiers::SHIFT, Key::Home);
            if shift_home || input.consume_key(egui::Modifiers::NONE, Key::Home) {
                let anchor = Self::selection_anchor(buffer);
                if shift_home {
                    buffer.clear_selection();
                }
                buffer.move_to_line_start();
                Self::extend_selection(buffer, anchor, shift_home);
                moved = true;
            }

            let shift_end = input.consume_key(egui::Modifiers::SHIFT, Key::End);
            if shift_end || input.consume_key(egui::Modifiers::NONE, Key::End) {
                let anchor = Self::selection_anchor(buffer);
                if shift_end {
                    buffer.clear_selection();
                }
                buffer.move_to_line_end();
                Self::extend_selection(buffer, anchor, shift_end);
                moved = true;
            }

            let shift_page_up = input.consume_key(egui::Modifiers::SHIFT, Key::PageUp);
            if shift_page_up || input.consume_key(egui::Modifiers::NONE, Key::PageUp) {
                let anchor = Self::selection_anchor(buffer);
                self.scroll_y = (self.scroll_y - 12.0 * 20.0).max(0.0);
                if shift_page_up {
                    buffer.clear_selection();
                }
                for _ in 0..12 {
                    buffer.move_up();
                }
                Self::extend_selection(buffer, anchor, shift_page_up);
                moved = true;
            }

            let shift_page_down = input.consume_key(egui::Modifiers::SHIFT, Key::PageDown);
            if shift_page_down || input.consume_key(egui::Modifiers::NONE, Key::PageDown) {
                let anchor = Self::selection_anchor(buffer);
                self.scroll_y += 12.0 * 20.0;
                if shift_page_down {
                    buffer.clear_selection();
                }
                for _ in 0..12 {
                    buffer.move_down();
                }
                Self::extend_selection(buffer, anchor, shift_page_down);
                moved = true;
            }
        });

        if moved {
            self.request_scroll_to_cursor(buffer);
        }

        changed
    }

    pub(crate) fn cycle_current_line_checkbox(buffer: &mut EditorBuffer) -> bool {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        Self::cycle_checkbox_at_line(buffer, line_index)
    }

    pub(crate) fn cycle_checkbox_at_line(buffer: &mut EditorBuffer, line_index: usize) -> bool {
        let cursor = buffer.cursor();
        let line_start = buffer.line_start(line_index);
        let line = buffer.line(line_index);
        let Some(parsed) = parse_checkbox_line(line) else {
            return false;
        };
        let marker_start = line_start + parsed.indent.len() + parsed.task_prefix.len();
        let marker_end = marker_start + parsed.marker.len();
        let next_marker = match parsed.state {
            CheckboxState::Empty => "[/]",
            CheckboxState::Doing => "[x]",
            CheckboxState::Done => "[]",
        };
        let next_cursor = if cursor <= marker_start {
            cursor
        } else if cursor <= marker_end {
            marker_start + cursor.saturating_sub(marker_start).min(next_marker.len())
        } else {
            cursor
                .saturating_add(next_marker.len())
                .saturating_sub(parsed.marker.len())
        };
        buffer.replace_selection_or_range(marker_start, marker_end, next_marker);
        buffer.set_cursor(next_cursor);
        true
    }

    pub(crate) fn insert_space_after_markdown_link_label(buffer: &mut EditorBuffer) -> bool {
        if buffer.selection().is_some() {
            return false;
        }

        let cursor = buffer.cursor();
        let line_index = buffer.line_index_for_byte(cursor);
        let line_start = buffer.line_start(line_index);
        let line = buffer.line(line_index);

        let mut search_from = 0;
        while let Some(open_relative) = line[search_from..].find('[') {
            let marker_start = search_from + open_relative;
            let text_start = marker_start + 1;
            let Some(close_text_relative) = line[text_start..].find(']') else {
                break;
            };
            let text_end = text_start + close_text_relative;
            if line.as_bytes().get(text_end + 1) != Some(&b'(') {
                search_from = text_end.saturating_add(1);
                continue;
            }
            let target_start = text_end + 2;
            let Some(close_target_relative) = line[target_start..].find(')') else {
                break;
            };
            let target_end = target_start + close_target_relative;
            let marker_end = target_end + 1;
            if target_start < target_end
                && cursor >= line_start + text_start
                && cursor <= line_start + text_end
            {
                if text_start == text_end {
                    let target = &line[target_start..target_end];
                    if let Some(label) = markdown_file_label_from_target(target) {
                        buffer.replace_selection_or_range(
                            line_start + text_start,
                            line_start + text_end,
                            &label,
                        );
                        buffer.set_cursor(line_start + marker_end + label.len());
                        return true;
                    }
                }
                let insert_at = line_start + marker_end;
                buffer.replace_selection_or_range(insert_at, insert_at, " ");
                return true;
            }
            search_from = marker_end;
        }

        false
    }

    pub(crate) fn insert_predictive_list_newline(&mut self, buffer: &mut EditorBuffer) -> bool {
        if buffer.selection().is_some() {
            return false;
        }

        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let line_start = buffer.line_start(line_index);
        let line_end = buffer.line_end(line_index);
        if buffer.cursor() != line_end {
            return false;
        }

        let line = buffer.line(line_index);
        let Some(list) = parse_list_line(line) else {
            return false;
        };

        if list.text.trim().is_empty() {
            buffer.replace_selection_or_range(line_start, line_end, "");
            return true;
        }

        let next_marker = if let (Some(number), Some(separator)) = (list.number, list.separator) {
            format!("{}{} ", number + 1, separator)
        } else {
            list.marker.to_string()
        };
        buffer.insert_text(&format!("\n{}{}", list.indent, next_marker));
        true
    }

    fn ensure_current_flow_line_default_tag(buffer: &mut EditorBuffer) -> bool {
        if buffer.selection().is_some() {
            return false;
        }
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let line_start = buffer.line_start(line_index);
        let line = buffer.line(line_index).to_string();
        if !line[buffer.cursor().saturating_sub(line_start)..]
            .trim()
            .is_empty()
        {
            return false;
        }
        Self::ensure_flow_line_default_tags_at_line(buffer, line_index)
    }

    fn ensure_flow_line_default_tags_at_line(buffer: &mut EditorBuffer, line_index: usize) -> bool {
        let line_end = buffer.line_end(line_index);
        let line = buffer.line(line_index).to_string();
        let tags = Self::default_flow_tags_for_line(&line);
        let missing_tags = tags
            .into_iter()
            .filter(|tag| {
                !line
                    .split_whitespace()
                    .any(|word| word.eq_ignore_ascii_case(&format!("#{tag}")))
            })
            .collect::<Vec<_>>();
        if missing_tags.is_empty() {
            return false;
        }
        buffer.set_cursor(line_end);
        buffer.insert_text(&format!(
            " {}",
            missing_tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        true
    }

    fn default_flow_tags_for_line(line: &str) -> Vec<&'static str> {
        let trimmed = line.trim_start();
        if let Some(checkbox) = parse_checkbox_line(line) {
            if checkbox.text.trim().is_empty() {
                return Vec::new();
            }
            let has_extra = checkbox.text.split_whitespace().any(|word| {
                word.starts_with('#')
                    || word.starts_with('@')
                    || matches!(
                        word.to_ascii_uppercase().as_str(),
                        "ALTA" | "MEDIA" | "BAJA"
                    )
                    || word.starts_with("due:")
                    || word.starts_with("scheduled:")
            });
            return if has_extra {
                vec!["task"]
            } else {
                vec!["task", "inbox"]
            };
        }
        if trimmed.starts_with(": ") && trimmed[2..].trim().is_empty() {
            return Vec::new();
        }
        if trimmed.starts_with(": ") {
            return vec!["inbox"];
        }
        let Some(parsed) = parse_flow_note_or_idea_line(line, 0, None) else {
            return Vec::new();
        };
        match parsed.kind {
            FlowItemKind::Task => vec!["task"],
            FlowItemKind::Note => vec!["note"],
            FlowItemKind::Idea => vec!["idea"],
            FlowItemKind::Inbox => vec!["inbox"],
        }
    }

    pub(crate) fn insert_text_with_checkbox_expansion(
        &mut self,
        buffer: &mut EditorBuffer,
        text: &str,
    ) {
        let inserted_at = buffer.cursor();
        buffer.type_text(text);
        if buffer.selection().is_none() && text == "[" {
            self.link_assist_trigger_start = Some(inserted_at);
        }
    }

    pub(crate) fn normalize_paste_text(text: &str) -> String {
        let mut normalized = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\u{00a0}', " ")
            .replace(['\u{200b}', '\u{200c}', '\u{200d}', '\u{feff}'], "");

        if normalized.lines().count() <= 1 {
            return normalized.trim_end_matches([' ', '\t']).to_string();
        }

        let mut lines = normalized
            .split('\n')
            .map(|line| line.trim_end_matches([' ', '\t']).to_string())
            .collect::<Vec<_>>();

        while lines
            .first()
            .map(|line| line.trim().is_empty())
            .unwrap_or(false)
        {
            lines.remove(0);
        }
        while lines
            .last()
            .map(|line| line.trim().is_empty())
            .unwrap_or(false)
        {
            lines.pop();
        }

        let common_indent = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                line.chars()
                    .take_while(|ch| *ch == ' ' || *ch == '\t')
                    .count()
            })
            .min()
            .unwrap_or(0);

        if common_indent > 0 {
            for line in &mut lines {
                if line.trim().is_empty() {
                    continue;
                }
                let byte_index = line
                    .char_indices()
                    .nth(common_indent)
                    .map(|(index, _)| index)
                    .unwrap_or_else(|| line.len());
                line.replace_range(..byte_index, "");
            }
        }

        normalized = lines.join("\n");
        normalized
    }

    pub(crate) fn selection_anchor(buffer: &EditorBuffer) -> usize {
        buffer
            .selection()
            .map(|(start, end)| if buffer.cursor() == start { end } else { start })
            .unwrap_or_else(|| buffer.cursor())
    }

    pub(crate) fn extend_selection(buffer: &mut EditorBuffer, anchor: usize, extend: bool) {
        if extend {
            buffer.set_selection(anchor, buffer.cursor());
        }
    }

    fn insert_flow_task_metadata_start(&mut self, buffer: &mut EditorBuffer) -> bool {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let line = buffer.line(line_index);
        let Some(task) = parse_checkbox_line(line) else {
            return false;
        };
        let child_indent = format!("{}  ", task.indent);
        Self::ensure_flow_line_default_tags_at_line(buffer, line_index);
        let insertion = format!("\n{child_indent}- scheduled: ");
        buffer.set_cursor(buffer.line_end(line_index));
        buffer.insert_text(&insertion);
        self.flow_metadata_month_offset = 0;
        true
    }

    fn handle_flow_metadata_enter(&mut self, buffer: &mut EditorBuffer) -> bool {
        let Some(line) =
            flow_metadata_line(buffer.line(buffer.line_index_for_byte(buffer.cursor())))
        else {
            return false;
        };
        let accepted = self.accept_flow_metadata_suggestion(buffer);
        let line = flow_metadata_line(buffer.line(buffer.line_index_for_byte(buffer.cursor())))
            .unwrap_or(line);
        match line.key {
            FlowMetadataKey::Scheduled => {
                let child_prefix = flow_metadata_child_prefix(&line);
                if line.value.trim().is_empty() && !accepted {
                    let replacement = format!("{child_prefix}due: ");
                    buffer.replace_current_line(&replacement, replacement.chars().count());
                } else {
                    buffer.set_cursor(buffer.line_end(buffer.line_index_for_byte(buffer.cursor())));
                    buffer.insert_text(&format!("\n{child_prefix}due: "));
                }
                self.flow_metadata_month_offset = 0;
                true
            }
            FlowMetadataKey::Due => {
                if line.value.trim().is_empty() && !accepted {
                    buffer.replace_current_line("", 0);
                } else {
                    buffer.set_cursor(buffer.line_end(buffer.line_index_for_byte(buffer.cursor())));
                    buffer.insert_text("\n");
                }
                self.flow_metadata_month_offset = 0;
                true
            }
        }
    }

    fn accept_flow_metadata_suggestion(&mut self, buffer: &mut EditorBuffer) -> bool {
        let Some(suggestion) = self.current_flow_metadata_suggestion(buffer) else {
            return false;
        };
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let Some(line) = flow_metadata_line(buffer.line(line_index)) else {
            return false;
        };
        let replacement = format!("{}{}", line.prefix, suggestion.format("%Y-%m-%d"));
        buffer.replace_current_line(&replacement, replacement.chars().count());
        true
    }

    fn adjust_flow_metadata_month(&mut self, buffer: &EditorBuffer, delta: i32) -> bool {
        if self.current_flow_metadata_suggestion(buffer).is_none() {
            return false;
        }
        self.flow_metadata_month_offset = self.flow_metadata_month_offset.saturating_add(delta);
        true
    }

    fn current_flow_metadata_suggestion(&self, buffer: &EditorBuffer) -> Option<NaiveDate> {
        let line_index = buffer.line_index_for_byte(buffer.cursor());
        let line = flow_metadata_line(buffer.line(line_index))?;
        suggest_flow_metadata_date(&line.value, self.flow_metadata_month_offset)
    }

    fn code_line_kind<'a>(
        &self,
        buffer: &'a EditorBuffer,
        line_index: usize,
    ) -> Option<CodeLineKind<'a>> {
        if let Some(fence) = parse_fenced_code_marker(buffer.line(line_index)) {
            return Some(CodeLineKind::Fence {
                language: fence.language,
            });
        }
        self.code_block_range(buffer, line_index)
            .map(|_| CodeLineKind::Body)
    }

    fn table_line_kind(&self, buffer: &EditorBuffer, line_index: usize) -> Option<TableLineKind> {
        let line = buffer.line(line_index);
        let cells = split_markdown_table_row(line)?;
        if cells.len() < 2 {
            return None;
        }
        if parse_markdown_table_separator(line).is_some() {
            return Some(TableLineKind::Separator);
        }
        let next = (line_index + 1 < buffer.line_count()).then(|| buffer.line(line_index + 1));
        if is_markdown_table_start(line, next) {
            return Some(TableLineKind::Header);
        }

        let mut index = line_index;
        while let Some(previous_index) = index.checked_sub(1) {
            let previous = buffer.line(previous_index);
            if parse_markdown_table_separator(previous).is_some() {
                return Some(TableLineKind::Body);
            }
            if split_markdown_table_row(previous).is_none() {
                break;
            }
            index = previous_index;
        }
        None
    }

    fn table_block_context(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        line_index: usize,
        font: &FontId,
    ) -> Option<(Vec<TableAlignment>, Vec<f32>)> {
        self.table_line_kind(buffer, line_index)?;
        let mut start = line_index;
        while start > 0 && split_markdown_table_row(buffer.line(start - 1)).is_some() {
            start -= 1;
        }
        let mut end = line_index;
        while end + 1 < buffer.line_count()
            && split_markdown_table_row(buffer.line(end + 1)).is_some()
        {
            end += 1;
        }

        let separator_index = (start..=end)
            .find(|index| parse_markdown_table_separator(buffer.line(*index)).is_some())?;
        let alignments = parse_markdown_table_separator(buffer.line(separator_index))?;
        let column_count = alignments.len();
        let mut widths = vec![56.0_f32; column_count];
        for index in start..=end {
            if index == separator_index {
                continue;
            }
            let Some(cells) = split_markdown_table_row(buffer.line(index)) else {
                continue;
            };
            if cells.len() != column_count {
                continue;
            }
            for (column, cell) in cells.iter().enumerate() {
                widths[column] = widths[column].max(self.text_width(painter, cell, font) + 22.0);
            }
        }
        Some((alignments, widths))
    }

    fn code_block_range(&self, buffer: &EditorBuffer, line_index: usize) -> Option<(usize, usize)> {
        let mut cache = self.fence_layout.borrow_mut();
        if cache.as_ref().is_none_or(|layout| {
            layout.identity != buffer.identity || layout.revision != buffer.revision
        }) {
            let mut ranges = Vec::new();
            let mut start = None;
            for index in 0..buffer.line_count() {
                if parse_fenced_code_marker(buffer.line(index)).is_some() {
                    if let Some(first) = start.take() {
                        ranges.push((first, index));
                    } else {
                        start = Some(index);
                    }
                }
            }
            if let Some(first) = start {
                ranges.push((first, buffer.line_count().saturating_sub(1)));
            }
            *cache = Some(FenceLayout {
                identity: buffer.identity,
                revision: buffer.revision,
                ranges,
            });
        }
        let ranges = &cache.as_ref().unwrap().ranges;
        let index = ranges.partition_point(|(start, _)| *start <= line_index);
        index
            .checked_sub(1)
            .and_then(|i| ranges.get(i).copied())
            .filter(|(_, end)| line_index <= *end)
    }

    fn paint_markdown_line_background(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        right: f32,
        render_markdown: bool,
        font: &FontId,
    ) {
        if !render_markdown {
            return;
        }

        if let Some(table_kind) = self.table_line_kind(buffer, row.line_index) {
            let table_width = self
                .table_block_context(painter, buffer, row.line_index, font)
                .map(|(_, widths)| widths.iter().sum::<f32>())
                .unwrap_or_else(|| (right - text_x - 8.0).min(520.0));
            let table_left = text_x - 4.0;
            let table_right = (text_x + table_width + 4.0).min(right - 8.0);
            let table_rect = egui::Rect::from_min_max(
                egui::pos2(table_left, y),
                egui::pos2(table_right, y + line_height),
            );
            let fill = match table_kind {
                TableLineKind::Header => Color32::from_rgb(31, 38, 50),
                TableLineKind::Separator => Color32::from_rgb(24, 30, 40),
                TableLineKind::Body if row.line_index % 2 == 0 => Color32::from_rgb(29, 36, 48),
                TableLineKind::Body => Color32::from_rgb(27, 33, 44),
            };
            let border = Color32::from_rgb(59, 70, 90);
            let strong_border = Color32::from_rgb(76, 86, 106);
            painter.rect_filled(table_rect, 0.0, fill);

            let previous_is_table = row
                .line_index
                .checked_sub(1)
                .and_then(|line_index| self.table_line_kind(buffer, line_index))
                .is_some();
            let next_is_table = (row.line_index + 1 < buffer.line_count())
                .then(|| self.table_line_kind(buffer, row.line_index + 1))
                .flatten()
                .is_some();
            let top_y = y;
            let bottom_y = y + line_height;

            painter.line_segment(
                [
                    egui::pos2(table_left, top_y),
                    egui::pos2(table_left, bottom_y),
                ],
                Stroke::new(1.0_f32, border),
            );
            painter.line_segment(
                [
                    egui::pos2(table_right, top_y),
                    egui::pos2(table_right, bottom_y),
                ],
                Stroke::new(1.0_f32, border),
            );
            if !previous_is_table {
                painter.line_segment(
                    [
                        egui::pos2(table_left, top_y),
                        egui::pos2(table_right, top_y),
                    ],
                    Stroke::new(1.0_f32, border),
                );
            }
            if !next_is_table {
                painter.line_segment(
                    [
                        egui::pos2(table_left, bottom_y),
                        egui::pos2(table_right, bottom_y),
                    ],
                    Stroke::new(1.0_f32, border),
                );
            }
            if let Some((_, widths)) =
                self.table_block_context(painter, buffer, row.line_index, font)
            {
                let mut x = text_x;
                for width in widths.iter().take(widths.len().saturating_sub(1)) {
                    x += *width;
                    painter.line_segment(
                        [egui::pos2(x, top_y), egui::pos2(x, bottom_y)],
                        Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
                    );
                }
            }
            if matches!(table_kind, TableLineKind::Header | TableLineKind::Separator) {
                painter.line_segment(
                    [
                        egui::pos2(table_left, bottom_y),
                        egui::pos2(table_right, bottom_y),
                    ],
                    Stroke::new(1.0_f32, strong_border),
                );
            }
        }

        if self.code_line_kind(buffer, row.line_index).is_some() {
            let cursor_line = buffer.cursor_line_col().0;
            let cursor_in_this_code_block = self
                .code_block_range(buffer, row.line_index)
                .is_some_and(|line_range| {
                    self.code_block_range(buffer, cursor_line) == Some(line_range)
                });
            if cursor_in_this_code_block {
                return;
            }

            let code_rect = egui::Rect::from_min_max(
                egui::pos2(text_x - 4.0, y + 1.0),
                egui::pos2(right - 8.0, y + line_height - 1.0),
            );
            painter.rect_filled(code_rect, 2.0, Color32::from_rgb(25, 31, 40));
            return;
        }

        let line = buffer.line(row.line_index);
        let line_start = buffer.line_start(row.line_index);
        if row.start == line_start {
            if let Some(task) = parse_checkbox_line(line) {
                let task_color = match task.state {
                    CheckboxState::Empty => Color32::from_rgb(136, 192, 208),
                    CheckboxState::Doing => Color32::from_rgb(235, 203, 139),
                    CheckboxState::Done => Color32::from_rgb(163, 190, 140),
                };
                let bar_rect = egui::Rect::from_min_max(
                    egui::pos2(text_x - 7.0, y + 4.0),
                    egui::pos2(text_x - 4.0, y + line_height - 4.0),
                );
                painter.rect_filled(bar_rect, 1.5, task_color);
            } else if let Some(item) = parse_flow_note_or_idea_line(line, row.line_index, None) {
                let item_color = match item.kind {
                    FlowItemKind::Task => Color32::from_rgb(136, 192, 208),
                    FlowItemKind::Note => Color32::from_rgb(180, 142, 173),
                    FlowItemKind::Idea => Color32::from_rgb(235, 203, 139),
                    FlowItemKind::Inbox => Color32::from_rgb(136, 154, 176),
                };
                let bar_rect = egui::Rect::from_min_max(
                    egui::pos2(text_x - 7.0, y + 4.0),
                    egui::pos2(text_x - 4.0, y + line_height - 4.0),
                );
                painter.rect_filled(bar_rect, 1.5, item_color);
            }
        }
        let cursor = buffer.cursor();
        let spans = parse_inline_code_spans(line);
        if spans.iter().any(|span| {
            cursor >= line_start + span.marker_start && cursor <= line_start + span.marker_end
        }) {
            return;
        }

        for span in spans {
            let start = (line_start + span.marker_start).max(row.start);
            let end = (line_start + span.marker_end).min(row.end);
            if start >= end {
                continue;
            }
            let start_x =
                text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
            let end_x = text_x + self.text_width(painter, &buffer.as_str()[row.start..end], font);
            let code_rect = egui::Rect::from_min_max(
                egui::pos2(start_x - 2.0, y + 2.0),
                egui::pos2(end_x + 2.0, y + line_height - 2.0),
            );
            painter.rect_filled(code_rect, 3.0, Color32::from_rgb(31, 38, 50));
        }
    }

    pub(crate) fn paint_line_text(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        text_color: Color32,
        render_markdown: bool,
    ) {
        let line_start = buffer.line_start(row.line_index);
        if !render_markdown {
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[row.start..row.end],
                font.clone(),
                text_color,
            );
            return;
        }
        let line = buffer.line(row.line_index);
        if let Some(code_kind) = self.code_line_kind(buffer, row.line_index) {
            let cursor_line = buffer.cursor_line_col().0;
            let cursor_in_this_code_block = self
                .code_block_range(buffer, row.line_index)
                .is_some_and(|line_range| {
                    self.code_block_range(buffer, cursor_line) == Some(line_range)
                });
            if cursor_in_this_code_block {
                painter.text(
                    egui::pos2(text_x, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    &buffer.as_str()[row.start..row.end],
                    font.clone(),
                    text_color,
                );
                return;
            }

            match code_kind {
                CodeLineKind::Fence { .. } if row.start == line_start => {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[row.start..row.end],
                        font.clone(),
                        Color32::from_rgb(136, 154, 176),
                    );
                    return;
                }
                CodeLineKind::Body => {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[row.start..row.end],
                        font.clone(),
                        Color32::from_rgb(216, 222, 233),
                    );
                    return;
                }
                _ => {}
            }
        }

        if let Some(table_kind) = self.table_line_kind(buffer, row.line_index) {
            let cursor_line = buffer.cursor_line_col().0;
            if row.start != line_start {
                painter.text(
                    egui::pos2(text_x, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    &buffer.as_str()[row.start..row.end],
                    font.clone(),
                    text_color,
                );
                return;
            }
            if table_kind == TableLineKind::Separator {
                if cursor_line == row.line_index {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        line,
                        font.clone(),
                        Color32::from_rgb(94, 105, 126),
                    );
                }
                return;
            }

            let Some(cells) = split_markdown_table_row(line) else {
                return;
            };
            let (alignments, widths) = self
                .table_block_context(painter, buffer, row.line_index, font)
                .unwrap_or_else(|| {
                    (
                        vec![TableAlignment::Left; cells.len()],
                        cells
                            .iter()
                            .map(|cell| self.text_width(painter, cell, font) + 22.0)
                            .collect(),
                    )
                });
            let table_color = match table_kind {
                TableLineKind::Header => Color32::from_rgb(235, 203, 139),
                TableLineKind::Separator => Color32::from_rgb(94, 105, 126),
                TableLineKind::Body => Color32::from_rgb(216, 222, 233),
            };
            let mut x = text_x;
            for (column, cell) in cells.iter().enumerate() {
                let width = widths.get(column).copied().unwrap_or(72.0);
                let cell_width = self.text_width(painter, cell, font);
                let alignment = alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableAlignment::Left);
                let cell_x = match alignment {
                    TableAlignment::Left => x + 8.0,
                    TableAlignment::Center => x + ((width - cell_width) * 0.5).max(4.0),
                    TableAlignment::Right => x + (width - cell_width - 8.0).max(4.0),
                };
                painter.text(
                    egui::pos2(cell_x, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    cell,
                    font.clone(),
                    table_color,
                );
                x += width;
            }
            return;
        }

        if let Some(heading) = parse_heading_line(line) {
            let marker_start = line_start + heading.indent.len();
            let marker_end = marker_start + heading.marker.len();
            let cursor_in_marker = buffer.cursor() >= marker_start && buffer.cursor() <= marker_end;
            let heading_color = match heading.level {
                1 => Color32::from_rgb(235, 203, 139),
                2 => Color32::from_rgb(180, 142, 173),
                3 => Color32::from_rgb(136, 192, 208),
                _ => Color32::from_rgb(216, 222, 233),
            };
            if !cursor_in_marker && row.start == line_start {
                let indent_width = self.text_width(painter, heading.indent, font);
                let marker_width = self.text_width(painter, heading.marker, font);
                if !heading.indent.is_empty() {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        heading.indent,
                        font.clone(),
                        text_color,
                    );
                }
                painter.text(
                    egui::pos2(text_x + indent_width, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    heading.marker,
                    font.clone(),
                    Color32::from_rgb(94, 105, 126),
                );
                if row.end > marker_end {
                    painter.text(
                        egui::pos2(text_x + indent_width + marker_width, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[marker_end..row.end],
                        font.clone(),
                        heading_color,
                    );
                }
                if heading.level <= 2 {
                    let underline_y = y + line_height - 2.0;
                    painter.line_segment(
                        [
                            egui::pos2(text_x + indent_width + marker_width, underline_y),
                            egui::pos2(painter.clip_rect().right() - 8.0, underline_y),
                        ],
                        Stroke::new(1.0_f32, Color32::from_rgb(46, 56, 72)),
                    );
                }
                return;
            } else if !cursor_in_marker {
                painter.text(
                    egui::pos2(text_x, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    &buffer.as_str()[row.start..row.end],
                    font.clone(),
                    heading_color,
                );
                return;
            }
        }

        if is_markdown_separator(line)
            && buffer.cursor_line_col().0 != row.line_index
            && row.start == line_start
        {
            let separator_y = y + line_height * 0.5;
            painter.line_segment(
                [
                    egui::pos2(text_x, separator_y),
                    egui::pos2(painter.clip_rect().right() - 8.0, separator_y),
                ],
                Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)),
            );
            return;
        }

        if let Some(blockquote) = parse_blockquote_line(line) {
            let marker_start = line_start + blockquote.indent.len();
            let marker_end = marker_start + blockquote.marker.len();
            let cursor_in_marker = buffer.cursor() >= marker_start && buffer.cursor() <= marker_end;
            if !cursor_in_marker && row.start == line_start {
                let indent_width = self.text_width(painter, blockquote.indent, font);
                let marker_width = self.text_width(painter, blockquote.marker, font);
                if !blockquote.indent.is_empty() {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        blockquote.indent,
                        font.clone(),
                        text_color,
                    );
                }
                let previous_quote_depth = row
                    .line_index
                    .checked_sub(1)
                    .and_then(|line_index| parse_blockquote_line(buffer.line(line_index)))
                    .map(|quote| quote.depth)
                    .unwrap_or(0);
                let next_quote_depth = (row.line_index + 1 < buffer.line_count())
                    .then(|| parse_blockquote_line(buffer.line(row.line_index + 1)))
                    .flatten()
                    .map(|quote| quote.depth)
                    .unwrap_or(0);
                for depth in 0..blockquote.depth {
                    let quote_level = depth + 1;
                    let x = text_x + indent_width + depth as f32 * 4.0 + 2.0;
                    let top = if previous_quote_depth >= quote_level {
                        y
                    } else {
                        y + 3.0
                    };
                    let bottom = if next_quote_depth >= quote_level {
                        y + line_height
                    } else {
                        y + line_height - 3.0
                    };
                    painter.line_segment(
                        [egui::pos2(x, top), egui::pos2(x, bottom)],
                        Stroke::new(1.0_f32, Color32::from_rgb(136, 192, 208)),
                    );
                }
                if row.end > marker_end {
                    painter.text(
                        egui::pos2(text_x + indent_width + marker_width, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[marker_end..row.end],
                        font.clone(),
                        Color32::from_rgb(190, 200, 216),
                    );
                }
                return;
            }
        }

        if let Some(item) = parse_flow_note_or_idea_line(line, row.line_index, None) {
            let trimmed = line.trim_start();
            let indent_len = line.len().saturating_sub(trimmed.len());
            let marker_len = match item.kind {
                FlowItemKind::Task => 0,
                FlowItemKind::Note if trimmed.starts_with(":nota") => ":nota".len(),
                FlowItemKind::Note => ":note".len(),
                FlowItemKind::Idea => ":idea".len(),
                FlowItemKind::Inbox => ":".len(),
            };
            let marker_start = line_start + indent_len;
            let marker_end = marker_start + marker_len;
            let text_start = marker_end;
            let cursor_in_marker = buffer.cursor() >= marker_start && buffer.cursor() <= marker_end;
            if !cursor_in_marker && row.start == line_start && row.end > marker_start {
                let indent = &line[..indent_len];
                let marker = &line[indent_len..indent_len + marker_len];
                let indent_width = self.text_width(painter, indent, font);
                let marker_width = self.text_width(painter, marker, font);
                if !indent.is_empty() {
                    painter.text(
                        egui::pos2(text_x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        indent,
                        font.clone(),
                        text_color,
                    );
                }
                let marker_color = match item.kind {
                    FlowItemKind::Task => Color32::from_rgb(136, 192, 208),
                    FlowItemKind::Note => Color32::from_rgb(180, 142, 173),
                    FlowItemKind::Idea => Color32::from_rgb(235, 203, 139),
                    FlowItemKind::Inbox => Color32::from_rgb(136, 154, 176),
                };
                painter.text(
                    egui::pos2(text_x + indent_width, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    marker,
                    font.clone(),
                    marker_color,
                );
                if row.end > text_start {
                    self.paint_flow_task_text(
                        painter,
                        buffer,
                        text_start,
                        row.end,
                        text_x + indent_width + marker_width,
                        y,
                        line_height,
                        font,
                        text_color,
                    );
                }
                return;
            }
        }

        let Some(parsed) = parse_checkbox_line(line) else {
            if let Some(list) = parse_list_line(line) {
                let marker_start = line_start + list.indent.len();
                let marker_end = marker_start + list.marker.len();
                let cursor_in_marker =
                    buffer.cursor() >= marker_start && buffer.cursor() <= marker_end;
                if !cursor_in_marker && row.start == line_start {
                    let indent_width = self.text_width(painter, list.indent, font);
                    let marker_width = self.text_width(painter, list.marker, font);
                    if !list.indent.is_empty() {
                        painter.text(
                            egui::pos2(text_x, y + line_height * 0.5),
                            egui::Align2::LEFT_CENTER,
                            list.indent,
                            font.clone(),
                            text_color,
                        );
                    }
                    let marker_color = if list.ordered {
                        Color32::from_rgb(235, 203, 139)
                    } else {
                        Color32::from_rgb(136, 192, 208)
                    };
                    painter.text(
                        egui::pos2(text_x + indent_width, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        list.marker,
                        font.clone(),
                        marker_color,
                    );
                    if row.end > marker_end {
                        let text_row = VisualRow {
                            line_index: row.line_index,
                            start: marker_end,
                            end: row.end,
                        };
                        if !self.paint_markdown_link_text(
                            painter,
                            buffer,
                            text_row,
                            text_x + indent_width + marker_width,
                            y,
                            line_height,
                            font,
                            text_color,
                        ) {
                            self.paint_flow_task_text(
                                painter,
                                buffer,
                                marker_end,
                                row.end,
                                text_x + indent_width + marker_width,
                                y,
                                line_height,
                                font,
                                text_color,
                            );
                        }
                    }
                    return;
                }
            }

            if self.paint_markdown_link_text(
                painter,
                buffer,
                row,
                text_x,
                y,
                line_height,
                font,
                text_color,
            ) {
                return;
            }

            if self.paint_inline_code_text(
                painter,
                buffer,
                row,
                text_x,
                y,
                line_height,
                font,
                text_color,
            ) {
                return;
            }
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[row.start..row.end],
                font.clone(),
                text_color,
            );
            return;
        };

        let marker_start = line_start + parsed.indent.len() + parsed.task_prefix.len();
        let marker_text_end = marker_start + parsed.marker.len();
        let text_start = marker_text_end;
        let cursor_in_checkbox =
            buffer.cursor() >= marker_start && buffer.cursor() <= marker_text_end;
        if cursor_in_checkbox
            || row.start != line_start
            || row.end <= marker_start
            || row.start >= text_start
        {
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[row.start..row.end],
                font.clone(),
                text_color,
            );
            return;
        }

        let indent_width = self.text_width(painter, parsed.indent, font);
        let prefix_width = self.text_width(painter, parsed.task_prefix, font);
        let checkbox_slot_width = self.text_width(painter, parsed.marker, font);
        if !parsed.indent.is_empty() {
            painter.text(
                egui::pos2(text_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                parsed.indent,
                font.clone(),
                text_color,
            );
        }

        if !parsed.task_prefix.is_empty() {
            painter.text(
                egui::pos2(text_x + indent_width, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                parsed.task_prefix,
                font.clone(),
                Color32::from_rgb(136, 154, 176),
            );
        }

        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(
                text_x + indent_width + prefix_width,
                y + (line_height - 13.0) * 0.5,
            ),
            egui::vec2(13.0, 13.0),
        );
        let (fill, stroke) = match parsed.state {
            CheckboxState::Empty => (
                Color32::from_rgb(30, 36, 48),
                Color32::from_rgb(136, 154, 176),
            ),
            CheckboxState::Doing => (
                Color32::from_rgb(59, 66, 82),
                Color32::from_rgb(235, 203, 139),
            ),
            CheckboxState::Done => (
                Color32::from_rgb(49, 70, 60),
                Color32::from_rgb(163, 190, 140),
            ),
        };
        painter.rect_filled(icon_rect, 2.0, fill);
        painter.rect_stroke(
            icon_rect,
            2.0,
            Stroke::new(1.2_f32, stroke),
            egui::StrokeKind::Outside,
        );
        let glyph_rect = icon_rect.shrink(3.2);
        match parsed.state {
            CheckboxState::Empty => {}
            CheckboxState::Doing => {
                painter.line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5_f32, stroke),
                );
            }
            CheckboxState::Done => {
                painter.line_segment(
                    [glyph_rect.left_top(), glyph_rect.right_bottom()],
                    Stroke::new(1.5_f32, stroke),
                );
                painter.line_segment(
                    [glyph_rect.left_bottom(), glyph_rect.right_top()],
                    Stroke::new(1.5_f32, stroke),
                );
            }
        }

        if row.end > text_start {
            self.paint_flow_task_text(
                painter,
                buffer,
                text_start,
                row.end,
                text_x + indent_width + prefix_width + checkbox_slot_width,
                y,
                line_height,
                font,
                text_color,
            );
        }
    }

    fn paint_flow_task_text(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        text_start: usize,
        text_end: usize,
        x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        text_color: Color32,
    ) {
        let mut cursor = text_start;
        for (token_start, token_end, color) in
            flow_task_token_spans(buffer.as_str(), text_start, text_end)
        {
            if cursor < token_start {
                let segment_x =
                    x + self.text_width(painter, &buffer.as_str()[text_start..cursor], font);
                painter.text(
                    egui::pos2(segment_x, y + line_height * 0.5),
                    egui::Align2::LEFT_CENTER,
                    &buffer.as_str()[cursor..token_start],
                    font.clone(),
                    text_color,
                );
            }
            let token_x =
                x + self.text_width(painter, &buffer.as_str()[text_start..token_start], font);
            painter.text(
                egui::pos2(token_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[token_start..token_end],
                font.clone(),
                color,
            );
            cursor = token_end;
        }
        if cursor < text_end {
            let segment_x =
                x + self.text_width(painter, &buffer.as_str()[text_start..cursor], font);
            painter.text(
                egui::pos2(segment_x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[cursor..text_end],
                font.clone(),
                text_color,
            );
        }
    }

    fn paint_flow_metadata_suggestion(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        render_markdown: bool,
    ) {
        if !render_markdown || row.start != buffer.line_start(row.line_index) {
            return;
        }
        if buffer.line_index_for_byte(buffer.cursor()) != row.line_index {
            return;
        }
        let Some(line) = flow_metadata_line(buffer.line(row.line_index)) else {
            return;
        };
        if line.value.trim().is_empty() {
            return;
        }
        let Some(suggestion) = self.current_flow_metadata_suggestion(buffer) else {
            return;
        };
        let ghost = format!(" → {}", suggestion.format("%Y-%m-%d"));
        let x = text_x + self.text_width(painter, buffer.line(row.line_index), font) + 8.0;
        painter.text(
            egui::pos2(x, y + line_height * 0.5),
            egui::Align2::LEFT_CENTER,
            ghost,
            font.clone(),
            Color32::from_rgb(94, 105, 126),
        );
    }

    fn paint_markdown_link_text(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        text_color: Color32,
    ) -> bool {
        let line = buffer.line(row.line_index);
        let line_start = buffer.line_start(row.line_index);
        let spans = parse_markdown_link_spans(line);
        if spans.is_empty()
            || spans.iter().any(|span| {
                buffer.cursor() >= line_start + span.marker_start
                    && buffer.cursor() <= line_start + span.marker_end
            })
        {
            return false;
        }

        let mut byte = row.start;
        for span in spans {
            let marker_start = line_start + span.marker_start;
            let text_start = line_start + span.text_start;
            let text_end = line_start + span.text_end;
            let target_start = line_start + span.target_start;
            let target_end = line_start + span.target_end;
            let marker_end = line_start + span.marker_end;
            if marker_end <= row.start || marker_start >= row.end {
                continue;
            }

            let target_fragment_start = buffer.as_str()[target_start..target_end]
                .find('#')
                .map(|offset| target_start + offset);
            let target_path_end = target_fragment_start.unwrap_or(target_end);
            let target_fragment_end = target_fragment_start
                .map(|_| target_end)
                .unwrap_or(target_end);
            let link_dim = Color32::from_rgb(94, 105, 126);
            let link_text = Color32::from_rgb(136, 192, 208);
            let link_target = Color32::from_rgb(163, 190, 140);
            let link_heading = Color32::from_rgb(235, 203, 139);
            let label_line_start =
                source_label_line_fragment(buffer.as_str(), text_start, text_end);
            let mut segments = vec![
                (byte, marker_start.min(row.end), text_color),
                (
                    marker_start.max(row.start),
                    text_start.min(row.end),
                    link_dim,
                ),
            ];
            if let Some(line_start) = label_line_start {
                segments.push((
                    text_start.max(row.start),
                    line_start.min(row.end),
                    link_target,
                ));
                segments.push((
                    line_start.max(row.start),
                    text_end.min(row.end),
                    link_heading,
                ));
            } else {
                segments.push((text_start.max(row.start), text_end.min(row.end), link_text));
            }
            segments.extend([
                (text_end.max(row.start), target_start.min(row.end), link_dim),
                (
                    target_start.max(row.start),
                    target_path_end.min(row.end),
                    link_target,
                ),
            ]);
            if let Some(fragment_start) = target_fragment_start {
                segments.push((
                    fragment_start.max(row.start),
                    target_fragment_end.min(row.end),
                    link_heading,
                ));
            }
            segments.push((target_end.max(row.start), marker_end.min(row.end), link_dim));

            for (start, end, color) in segments {
                if start < end {
                    let x =
                        text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
                    painter.text(
                        egui::pos2(x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[start..end],
                        font.clone(),
                        color,
                    );
                }
            }

            let underline_start_byte = text_start.max(row.start).min(row.end);
            let underline_end_byte = text_end.max(row.start).min(row.end);
            if underline_start_byte < underline_end_byte {
                let underline_start = text_x
                    + self.text_width(
                        painter,
                        &buffer.as_str()[row.start..underline_start_byte],
                        font,
                    );
                let underline_end = text_x
                    + self.text_width(
                        painter,
                        &buffer.as_str()[row.start..underline_end_byte],
                        font,
                    );
                let underline_y = y + line_height - 3.0;
                painter.line_segment(
                    [
                        egui::pos2(underline_start, underline_y),
                        egui::pos2(underline_end, underline_y),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgb(136, 192, 208)),
                );
            }
            byte = marker_end.max(row.start).min(row.end);
        }

        if byte < row.end {
            let x = text_x + self.text_width(painter, &buffer.as_str()[row.start..byte], font);
            painter.text(
                egui::pos2(x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[byte..row.end],
                font.clone(),
                text_color,
            );
        }

        true
    }

    fn paint_inline_code_text(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        text_color: Color32,
    ) -> bool {
        let line = buffer.line(row.line_index);
        let line_start = buffer.line_start(row.line_index);
        let spans = parse_inline_code_spans(line);
        if spans.is_empty()
            || spans.iter().any(|span| {
                buffer.cursor() >= line_start + span.marker_start
                    && buffer.cursor() <= line_start + span.marker_end
            })
        {
            return false;
        }

        let mut byte = row.start;
        for span in spans {
            let marker_start = line_start + span.marker_start;
            let code_start = line_start + span.code_start;
            let code_end = line_start + span.code_end;
            let marker_end = line_start + span.marker_end;
            if marker_end <= row.start || marker_start >= row.end {
                continue;
            }

            for (start, end, color) in [
                (byte, marker_start.min(row.end), text_color),
                (
                    marker_start.max(row.start),
                    code_start.min(row.end),
                    Color32::from_rgb(94, 105, 126),
                ),
                (
                    code_start.max(row.start),
                    code_end.min(row.end),
                    Color32::from_rgb(235, 203, 139),
                ),
                (
                    code_end.max(row.start),
                    marker_end.min(row.end),
                    Color32::from_rgb(94, 105, 126),
                ),
            ] {
                if start < end {
                    let x =
                        text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
                    painter.text(
                        egui::pos2(x, y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        &buffer.as_str()[start..end],
                        font.clone(),
                        color,
                    );
                }
            }
            byte = marker_end.max(row.start).min(row.end);
        }

        if byte < row.end {
            let x = text_x + self.text_width(painter, &buffer.as_str()[row.start..byte], font);
            painter.text(
                egui::pos2(x, y + line_height * 0.5),
                egui::Align2::LEFT_CENTER,
                &buffer.as_str()[byte..row.end],
                font.clone(),
                text_color,
            );
        }

        true
    }

    pub(crate) fn paint_search_matches(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        search_state: &SearchState,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        right: f32,
    ) {
        for (index, (match_start, match_end)) in search_state.matches.iter().copied().enumerate() {
            let start = match_start.max(row.start).min(row.end);
            let end = match_end.max(row.start).min(row.end);
            if start >= end {
                continue;
            }

            let start_x =
                text_x + self.text_width(painter, &buffer.as_str()[row.start..start], font);
            let end_x = text_x + self.text_width(painter, &buffer.as_str()[row.start..end], font);
            let color = if index == search_state.selected {
                Color32::from_rgb(235, 203, 139)
            } else {
                Color32::from_rgb(76, 86, 106)
            };
            let rect = egui::Rect::from_min_max(
                egui::pos2(start_x, y + 2.0),
                egui::pos2(end_x.min(right), y + line_height - 2.0),
            );
            painter.rect_filled(rect, 2.0, color);
        }
    }

    pub(crate) fn paint_selection(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        selection_start: usize,
        selection_end: usize,
        text_x: f32,
        y: f32,
        line_height: f32,
        font: &FontId,
        right: f32,
    ) {
        let start = selection_start.max(row.start).min(row.end);
        let end = selection_end.max(row.start).min(row.end);
        if start >= end {
            return;
        }
        let start_x = self.visual_x_for_byte(painter, buffer, row, start, text_x, font);
        let end_x = self.visual_x_for_byte(painter, buffer, row, end, text_x, font);
        let rect = egui::Rect::from_min_max(
            egui::pos2(start_x, y + 2.0),
            egui::pos2(end_x.min(right), y + line_height - 2.0),
        );
        painter.rect_filled(rect, 2.0, Color32::from_rgb(67, 86, 117));
    }

    pub(crate) fn paint_cursor(
        &mut self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        rows: &[VisualRow],
        rect: egui::Rect,
        text_x: f32,
        line_height: f32,
        font: &FontId,
    ) {
        let row_index = self.row_index_for_byte(rows, buffer.cursor());
        let Some(row) = rows.get(row_index).copied() else {
            self.forget_cursor_position();
            return;
        };
        let y = rect.top() + row_index as f32 * line_height - self.scroll_y;
        if y + line_height < rect.top() || y > rect.bottom() {
            // Off screen: when it comes back it appears in place, no glide.
            self.forget_cursor_position();
            return;
        }
        let cursor = buffer.cursor();
        let x = self.visual_x_for_byte(painter, buffer, row, cursor, text_x, font);
        self.last_cursor_pos = Some(egui::pos2(x, y + line_height));

        // The character under the caret (a space at the end of a line).
        let under = buffer.as_str()[cursor..row.end]
            .chars()
            .next()
            .filter(|c| *c != '\n' && *c != '\r');
        let cell_width = {
            let glyph = under.map_or_else(|| " ".to_string(), |c| c.to_string());
            self.text_width(painter, &glyph, font).max(1.0)
        };

        let target = egui::pos2(x - rect.left(), row_index as f32 * line_height);
        let now = painter.ctx().input(|input| input.time);
        if self.cursor_target != Some(target) {
            let from = self.cursor_drawn;
            // Typing moves one cell at a time: that stays instant and crisp.
            let typing_step = from.is_some_and(|from| {
                from.y == target.y && (target.x - from.x).abs() <= cell_width * 1.5
            });
            let look = self.look;
            self.cursor_glide = from.and_then(|from| {
                let tail_from = self.cursor_tail.unwrap_or(from);
                if typing_step {
                    (look.cursor_trail && look.cursor_trail_typing).then_some(CursorGlide {
                        head_from: target,
                        tail_from,
                        started: now,
                        instant_head: true,
                    })
                } else {
                    look.animate_cursor.then_some(CursorGlide {
                        head_from: from,
                        tail_from,
                        started: now,
                        instant_head: false,
                    })
                }
            });
            let jumped = from.is_some() && !typing_step;
            self.cursor_pulse_at = if jumped && look.cursor_pulse {
                let landing = match self.cursor_glide {
                    Some(_) if look.cursor_stretch => CURSOR_GLIDE_SECONDS * STRETCH_LAG,
                    Some(_) => CURSOR_GLIDE_SECONDS,
                    None => 0.0,
                };
                Some(now + landing)
            } else {
                None
            };
            self.cursor_target = Some(target);
        }
        // Ease-out cubic: fast start, gentle landing.
        let ease = |t: f64| 1.0 - (1.0 - t.clamp(0.0, 1.0) as f32).powi(3);
        let mut animating = false;
        let mut gliding = false;
        let mut tail = None;
        let drawn = match self.cursor_glide {
            Some(glide) => {
                let elapsed = now - glide.started;
                let head_seconds = if glide.instant_head {
                    0.0
                } else if self.look.cursor_stretch {
                    CURSOR_GLIDE_SECONDS * STRETCH_LAG
                } else {
                    CURSOR_GLIDE_SECONDS
                };
                let tail_seconds = if self.look.cursor_trail {
                    trail_seconds(self.look.cursor_trail_weight)
                } else {
                    head_seconds
                };
                if elapsed >= head_seconds.max(tail_seconds) {
                    self.cursor_glide = None;
                    target
                } else {
                    animating = true;
                    gliding = elapsed < head_seconds;
                    if self.look.cursor_trail {
                        tail = Some(
                            glide.tail_from
                                + (target - glide.tail_from) * ease(elapsed / tail_seconds),
                        );
                    }
                    if glide.instant_head {
                        target
                    } else {
                        glide.head_from
                            + (target - glide.head_from) * ease(elapsed / CURSOR_GLIDE_SECONDS)
                    }
                }
            }
            None => target,
        };
        self.cursor_drawn = Some(drawn);
        self.cursor_tail = tail;
        let to_screen = |p: Pos2| egui::pos2(rect.left() + p.x, rect.top() + p.y - self.scroll_y);
        let at = to_screen(drawn);
        let color = self.look.cursor_color;
        // The caret's shape at a given top-left corner.
        let shape = |at: Pos2| {
            if self.look.block_cursor {
                egui::Rect::from_min_size(
                    egui::pos2(at.x, at.y + 1.0),
                    egui::vec2(cell_width, line_height - 2.0),
                )
            } else {
                egui::Rect::from_min_size(
                    egui::pos2(at.x - 0.75, at.y + 3.0),
                    egui::vec2(1.5, line_height - 6.0),
                )
            }
        };
        if let Some(tail) = tail {
            paint_cursor_trail(
                painter,
                shape(to_screen(tail)),
                shape(at),
                color.gamma_multiply(trail_opacity(self.look.cursor_trail_weight)),
            );
        }
        // Stretch: each corner of the shape travels on its own clock, the
        // ones facing the jump fastest, so the caret leans into the move.
        let stretched = self.cursor_glide.filter(|glide| {
            gliding && self.look.cursor_stretch && !glide.instant_head
        });
        if let Some(glide) = stretched {
            let elapsed = now - glide.started;
            let travel = target - glide.head_from;
            let from_shape = shape(to_screen(glide.head_from));
            let corners = [
                from_shape.left_top(),
                from_shape.right_top(),
                from_shape.right_bottom(),
                from_shape.left_bottom(),
            ];
            let center = from_shape.center();
            let direction = travel.normalized();
            let reach = corners
                .iter()
                .map(|corner| (*corner - center).dot(direction).abs())
                .fold(0.0_f32, f32::max)
                .max(f32::EPSILON);
            let points = corners
                .iter()
                .map(|corner| {
                    let lead = f64::from(((*corner - center).dot(direction) / reach).clamp(-1.0, 1.0));
                    // lead 1 → STRETCH_LEAD, lead -1 → STRETCH_LAG.
                    let share = STRETCH_LAG - (STRETCH_LAG - STRETCH_LEAD) * (lead + 1.0) / 2.0;
                    *corner + travel * ease(elapsed / (CURSOR_GLIDE_SECONDS * share))
                })
                .collect::<Vec<_>>();
            let fill = if self.look.block_cursor {
                color.gamma_multiply(0.75)
            } else {
                color
            };
            painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
        } else if self.look.block_cursor {
            let block = shape(at);
            if gliding {
                painter.rect_filled(block, 1.0, color.gamma_multiply(0.75));
            } else {
                painter.rect_filled(block, 1.0, color);
                if let Some(c) = under.filter(|c| !c.is_whitespace()) {
                    painter.text(
                        egui::pos2(at.x, at.y + line_height * 0.5),
                        egui::Align2::LEFT_CENTER,
                        c,
                        font.clone(),
                        Color32::from_rgb(30, 36, 48),
                    );
                }
            }
        } else {
            painter.line_segment(
                [
                    egui::pos2(at.x, at.y + 3.0),
                    egui::pos2(at.x, at.y + line_height - 3.0),
                ],
                Stroke::new(1.5_f32, color),
            );
        }
        if let Some(start) = self.cursor_pulse_at {
            let progress = ((now - start) / CURSOR_PULSE_SECONDS) as f32;
            if progress >= 1.0 {
                self.cursor_pulse_at = None;
            } else {
                animating = true;
                if progress >= 0.0 {
                    let fade = (1.0 - progress).powi(2);
                    let ring = shape(at).expand(2.0 + 7.0 * ease(f64::from(progress)));
                    painter.rect_stroke(
                        ring,
                        3.0,
                        Stroke::new(1.5_f32, color.gamma_multiply(0.7 * fade)),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }
        // The caret does not blink: once it has landed, drawing it schedules
        // no frame, so an idle editor presents nothing until input arrives.
        if animating {
            painter.ctx().request_repaint();
        }
    }

    /// Whether the landing ring is scheduled or showing (tests).
    #[cfg(test)]
    pub(crate) fn cursor_pulsing(&self) -> bool {
        self.cursor_pulse_at.is_some()
    }

    /// (drawn, target, animating) for tests.
    #[cfg(test)]
    pub(crate) fn cursor_state(&self) -> (Option<Pos2>, Option<Pos2>, bool) {
        (self.cursor_drawn, self.cursor_target, self.cursor_glide.is_some())
    }

    fn forget_cursor_position(&mut self) {
        self.cursor_target = None;
        self.cursor_drawn = None;
        self.cursor_glide = None;
        self.cursor_pulse_at = None;
        self.cursor_tail = None;
    }

    pub(crate) fn text_width(&self, painter: &egui::Painter, text: &str, font: &FontId) -> f32 {
        if text.is_empty() {
            0.0
        } else {
            painter
                .layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        }
    }

    fn visual_x_for_byte(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        byte: usize,
        text_x: f32,
        font: &FontId,
    ) -> f32 {
        let byte = byte.max(row.start).min(row.end);
        if row.start != buffer.line_start(row.line_index) {
            return text_x + self.text_width(painter, &buffer.as_str()[row.start..byte], font);
        }
        let line = buffer.line(row.line_index);
        let Some(parsed) = parse_checkbox_line(line) else {
            return text_x + self.text_width(painter, &buffer.as_str()[row.start..byte], font);
        };
        let marker_start = row.start + parsed.indent.len() + parsed.task_prefix.len();
        let marker_text_end = marker_start + parsed.marker.len();
        if byte < marker_text_end {
            return text_x + self.text_width(painter, &buffer.as_str()[row.start..byte], font);
        }
        let indent_width = self.text_width(painter, parsed.indent, font);
        let prefix_width = self.text_width(painter, parsed.task_prefix, font);
        let checkbox_slot_width = self.text_width(painter, parsed.marker, font);
        text_x
            + indent_width
            + prefix_width
            + checkbox_slot_width
            + self.text_width(painter, &buffer.as_str()[marker_text_end..byte], font)
    }

    fn row_for_pointer(
        &self,
        buffer: &EditorBuffer,
        rows: &[VisualRow],
        rect: egui::Rect,
        line_height: f32,
        pointer_y: f32,
    ) -> VisualRow {
        let row_index = ((self.scroll_y + pointer_y - rect.top()) / line_height)
            .floor()
            .max(0.0) as usize;
        rows.get(row_index.min(rows.len().saturating_sub(1)))
            .copied()
            .unwrap_or(VisualRow {
                line_index: 0,
                start: 0,
                end: buffer.line_end(0),
            })
    }

    pub(crate) fn byte_for_x(
        &self,
        painter: &egui::Painter,
        buffer: &EditorBuffer,
        row: VisualRow,
        x: f32,
        font: &FontId,
    ) -> usize {
        let text = &buffer.as_str()[row.start..row.end];
        let mut closest = row.start;
        let mut closest_distance = x.abs();

        for (offset, _) in text.char_indices() {
            let width = self.text_width(painter, &text[..offset], font);
            let distance = (width - x).abs();
            if distance < closest_distance {
                closest = row.start + offset;
                closest_distance = distance;
            }
        }

        let end_distance = (self.text_width(painter, text, font) - x).abs();
        if end_distance < closest_distance {
            row.end
        } else {
            closest
        }
    }

    pub(crate) fn request_scroll_to_cursor(&mut self, buffer: &EditorBuffer) {
        self.target_cursor = Some(buffer.cursor());
    }

    pub(crate) fn take_link_click_byte(&mut self) -> Option<usize> {
        self.link_click_byte.take()
    }

    pub(crate) fn take_link_click_target(&mut self) -> Option<String> {
        self.link_click_target.take()
    }

    pub(crate) fn take_virtual_line_click(&mut self) -> Option<String> {
        self.virtual_line_click.take()
    }

    pub(crate) fn take_link_assist_trigger_start(&mut self) -> Option<usize> {
        self.link_assist_trigger_start.take()
    }

    #[allow(dead_code)]
    pub(crate) fn clear_scroll_target(&mut self) {
        self.target_cursor = None;
    }
}

fn flow_metadata_line(line: &str) -> Option<FlowMetadataLine> {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    let rest = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    if let Some(value) = rest.strip_prefix("scheduled:") {
        return Some(FlowMetadataLine {
            key: FlowMetadataKey::Scheduled,
            prefix: format!("{indent}- scheduled: "),
            value: value.trim_start().to_string(),
        });
    }
    if let Some(value) = rest.strip_prefix("due:") {
        return Some(FlowMetadataLine {
            key: FlowMetadataKey::Due,
            prefix: format!("{indent}- due: "),
            value: value.trim_start().to_string(),
        });
    }
    None
}

fn flow_metadata_child_prefix(line: &FlowMetadataLine) -> String {
    line.prefix
        .strip_suffix("scheduled: ")
        .or_else(|| line.prefix.strip_suffix("due: "))
        .unwrap_or(&line.prefix)
        .to_string()
}

fn suggest_flow_metadata_date(value: &str, month_offset: i32) -> Option<NaiveDate> {
    let value = value.trim();
    if value.is_empty() || NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() {
        return None;
    }
    let numbers = value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect::<Vec<_>>();
    let today = Local::now().date_naive();
    match numbers.as_slice() {
        [day] if (1..=31).contains(day) => {
            let mut year = today.year();
            let mut month = today.month();
            let max_day = days_in_month(year, month);
            let candidate = NaiveDate::from_ymd_opt(year, month, (*day).min(max_day))?;
            if candidate < today {
                (year, month) = add_months(year, month, 1);
            }
            let (year, month) = add_months(year, month, month_offset);
            let day = (*day).min(days_in_month(year, month));
            NaiveDate::from_ymd_opt(year, month, day)
        }
        [day, month] if (1..=31).contains(day) && (1..=12).contains(month) => {
            let mut year = today.year();
            let max_day = days_in_month(year, *month);
            let candidate = NaiveDate::from_ymd_opt(year, *month, (*day).min(max_day))?;
            if candidate < today {
                year += 1;
            }
            let (year, month) = add_months(year, *month, month_offset);
            let day = (*day).min(days_in_month(year, month));
            NaiveDate::from_ymd_opt(year, month, day)
        }
        [day, month, year] if (1..=31).contains(day) && (1..=12).contains(month) => {
            let year = if *year < 100 {
                2000 + *year as i32
            } else {
                *year as i32
            };
            let (year, month) = add_months(year, *month, month_offset);
            let day = (*day).min(days_in_month(year, month));
            NaiveDate::from_ymd_opt(year, month, day)
        }
        _ => None,
    }
}

fn add_months(year: i32, month: u32, offset: i32) -> (i32, u32) {
    let total = year * 12 + month as i32 - 1 + offset;
    (total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = add_months(year, month, 1);
    let Some(first_next) = NaiveDate::from_ymd_opt(next_year, next_month, 1) else {
        return 28;
    };
    first_next.pred_opt().map(|date| date.day()).unwrap_or(28)
}

fn flow_task_token_spans(
    text: &str,
    text_start: usize,
    text_end: usize,
) -> Vec<(usize, usize, Color32)> {
    let mut spans = Vec::new();
    let slice = &text[text_start..text_end];
    let mut token_start = None;
    for (offset, ch) in slice
        .char_indices()
        .chain(std::iter::once((slice.len(), ' ')))
    {
        if ch.is_whitespace() {
            if let Some(start) = token_start.take() {
                let end = offset;
                let token = &slice[start..end];
                let trimmed = token.trim_matches(|ch: char| matches!(ch, ',' | ';' | ')' | '('));
                let color = if trimmed.starts_with('#') && trimmed.len() > 1 {
                    Some(Color32::from_rgb(136, 192, 208))
                } else if trimmed.starts_with('@') && trimmed.len() > 1 {
                    Some(Color32::from_rgb(180, 142, 173))
                } else if matches!(trimmed, "ALTA" | "MEDIA" | "BAJA") {
                    Some(match trimmed {
                        "ALTA" => Color32::from_rgb(235, 203, 139),
                        "MEDIA" => Color32::from_rgb(163, 190, 140),
                        _ => Color32::from_rgb(136, 154, 176),
                    })
                } else if matches!(trimmed, "due:" | "scheduled:" | "related:" | "note:") {
                    Some(Color32::from_rgb(94, 105, 126))
                } else {
                    None
                };
                if let Some(color) = color {
                    spans.push((text_start + start, text_start + end, color));
                }
            }
        } else if token_start.is_none() {
            token_start = Some(offset);
        }
    }
    spans
}

fn markdown_file_label_from_target(target: &str) -> Option<String> {
    if target.contains("://") || target.starts_with("mailto:") {
        return None;
    }
    let without_fragment = target
        .split_once('#')
        .map(|(path, _)| path)
        .unwrap_or(target);
    let path_part = without_fragment
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(without_fragment);
    let extension = Path::new(path_part)
        .extension()
        .and_then(|extension| extension.to_str())?;
    if !matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown") {
        return None;
    }
    Path::new(path_part)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .map(|stem| stem.to_string())
}

fn text_input_from_events(events: &[egui::Event]) -> Option<String> {
    let texts = events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Text(text) if !text.is_empty() => Some(text.as_str()),
            egui::Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty() => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    match texts.as_slice() {
        [] => None,
        [text] => Some((*text).to_string()),
        many => {
            let last = many.last().copied().unwrap_or_default();
            if last.chars().count() == 1 && last.chars().any(|ch| !ch.is_ascii()) {
                Some(last.to_string())
            } else {
                Some(many.concat())
            }
        }
    }
}


/// A smear from where the caret was (transparent) to where it is (`strong`):
/// every edge of the caret's shape swept from tail to head.
fn paint_cursor_trail(painter: &egui::Painter, tail: egui::Rect, head: egui::Rect, strong: Color32) {
    let corners = |r: egui::Rect| [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    let (tail, head) = (corners(tail), corners(head));
    let faint = Color32::TRANSPARENT;
    let mut mesh = egui::Mesh::default();
    for edge in 0..4 {
        let next = (edge + 1) % 4;
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(tail[edge], faint);
        mesh.colored_vertex(tail[next], faint);
        mesh.colored_vertex(head[next], strong);
        mesh.colored_vertex(head[edge], strong);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::{EditorView, LineNumberMode};
    use crate::editor_buffer::EditorBuffer;

    #[test]
    fn normalizes_multiline_paste_text() {
        let pasted = "\r\n    fn main() {  \r\n        println!(\"hi\");\u{00a0}\r\n    }\r\n";

        assert_eq!(
            EditorView::normalize_paste_text(pasted),
            "fn main() {\n    println!(\"hi\");\n}"
        );
    }

    #[test]
    fn normalizes_single_line_paste_text_without_stripping_leading_spaces() {
        assert_eq!(EditorView::normalize_paste_text("  hello  "), "  hello");
    }

    #[test]
    fn predictive_list_newline_continues_unordered_lists() {
        let mut buffer = EditorBuffer::from_text("- hola".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.insert_predictive_list_newline(&mut buffer));
        assert_eq!(buffer.as_str(), "- hola\n- ");
    }

    #[test]
    fn predictive_list_newline_increments_ordered_lists() {
        let mut buffer = EditorBuffer::from_text("  1) hola".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.insert_predictive_list_newline(&mut buffer));
        assert_eq!(buffer.as_str(), "  1) hola\n  2) ");
    }

    #[test]
    fn predictive_list_newline_removes_empty_list_marker() {
        let mut buffer = EditorBuffer::from_text("- hola\n- ".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.insert_predictive_list_newline(&mut buffer));
        assert_eq!(buffer.as_str(), "- hola\n");
    }

    #[test]
    fn enter_inside_markdown_link_label_moves_after_link_with_space() {
        let mut buffer = EditorBuffer::from_text("[algo](./note.md)".to_string());
        buffer.set_cursor("[algo".len());

        assert!(EditorView::insert_space_after_markdown_link_label(
            &mut buffer
        ));
        assert_eq!(buffer.as_str(), "[algo](./note.md) ");
        assert_eq!(buffer.cursor(), buffer.as_str().len());
    }

    #[test]
    fn enter_inside_empty_markdown_file_link_uses_file_stem_as_label() {
        let mut buffer = EditorBuffer::from_text("[](docs/periscope.md#ideas)".to_string());
        buffer.set_cursor(1);

        assert!(EditorView::insert_space_after_markdown_link_label(
            &mut buffer
        ));
        assert_eq!(buffer.as_str(), "[periscope](docs/periscope.md#ideas)");
        assert_eq!(buffer.cursor(), buffer.as_str().len());
    }

    #[test]
    fn enter_inside_empty_markdown_link_label_moves_after_link_with_space() {
        let mut buffer = EditorBuffer::from_text("[](https://example.com)".to_string());
        buffer.set_cursor(1);

        assert!(EditorView::insert_space_after_markdown_link_label(
            &mut buffer
        ));
        assert_eq!(buffer.as_str(), "[](https://example.com) ");
        assert_eq!(buffer.cursor(), buffer.as_str().len());
    }

    #[test]
    fn detects_fenced_code_block_ranges() {
        let buffer = EditorBuffer::from_text("before\n```rust\ncode\n```\nafter".to_string());
        let view = EditorView::new();

        assert_eq!(view.code_block_range(&buffer, 0), None);
        assert_eq!(view.code_block_range(&buffer, 1), Some((1, 3)));
        assert_eq!(view.code_block_range(&buffer, 2), Some((1, 3)));
        assert_eq!(view.code_block_range(&buffer, 3), Some((1, 3)));
        assert_eq!(view.code_block_range(&buffer, 4), None);
    }

    #[test]
    fn keeps_compact_empty_checkbox_when_space_is_typed() {
        let mut buffer = EditorBuffer::from_text("[]".to_string());
        buffer.set_cursor(2);
        let mut view = EditorView::new();

        view.insert_text_with_checkbox_expansion(&mut buffer, " ");

        assert_eq!(buffer.as_str(), "[] ");
        assert_eq!(buffer.cursor(), 3);
    }

    #[test]
    fn keeps_compact_empty_subtask_when_space_is_typed() {
        let mut buffer = EditorBuffer::from_text("- []".to_string());
        buffer.set_cursor(4);
        let mut view = EditorView::new();

        view.insert_text_with_checkbox_expansion(&mut buffer, " ");

        assert_eq!(buffer.as_str(), "- [] ");
        assert_eq!(buffer.cursor(), 5);
    }

    #[test]
    fn keeps_compact_empty_subsubtask_when_space_is_typed() {
        let mut buffer = EditorBuffer::from_text("-- []".to_string());
        buffer.set_cursor(5);
        let mut view = EditorView::new();

        view.insert_text_with_checkbox_expansion(&mut buffer, " ");

        assert_eq!(buffer.as_str(), "-- [] ");
        assert_eq!(buffer.cursor(), 6);
    }

    #[test]
    fn cycles_checkbox_state_on_current_line() {
        let mut buffer = EditorBuffer::from_text("[]todo".to_string());
        buffer.set_cursor(1);

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));
        assert_eq!(buffer.as_str(), "[/]todo");

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));
        assert_eq!(buffer.as_str(), "[x]todo");

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));
        assert_eq!(buffer.as_str(), "[]todo");
    }

    #[test]
    fn cycling_checkbox_preserves_cursor_in_text() {
        let mut buffer = EditorBuffer::from_text("[ ] todo".to_string());
        buffer.set_cursor(8);

        assert!(EditorView::cycle_current_line_checkbox(&mut buffer));

        assert_eq!(buffer.as_str(), "[/] todo");
        assert_eq!(buffer.cursor(), 8);
    }

    #[test]
    fn starting_flow_metadata_adds_default_task_tags_first() {
        let mut buffer = EditorBuffer::from_text("[] nombre del task".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.insert_flow_task_metadata_start(&mut buffer));
        assert_eq!(
            buffer.as_str(),
            "[] nombre del task #task #inbox\n  - scheduled: "
        );
    }

    #[test]
    fn flow_metadata_empty_scheduled_becomes_due_without_nesting_key() {
        let mut buffer = EditorBuffer::from_text("- [ ] task\n  - scheduled: ".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.handle_flow_metadata_enter(&mut buffer));

        assert_eq!(buffer.as_str(), "- [ ] task\n  - due: ");
        assert!(!buffer.as_str().contains("scheduled: due:"));
    }

    #[test]
    fn flow_metadata_scheduled_date_inserts_due_sibling() {
        let mut buffer = EditorBuffer::from_text("- [ ] task\n  - scheduled: 10".to_string());
        buffer.set_cursor(buffer.as_str().len());
        let mut view = EditorView::new();

        assert!(view.handle_flow_metadata_enter(&mut buffer));

        assert!(buffer.as_str().contains("\n  - due: "));
        assert!(!buffer.as_str().contains("scheduled: due:"));
    }

    #[test]
    fn line_number_mode_calculates_relative_numbers() {
        assert_eq!(LineNumberMode::Absolute.number_for_line(9, 4), 10);
        assert_eq!(LineNumberMode::Relative.number_for_line(4, 4), 5);
        assert_eq!(LineNumberMode::Relative.number_for_line(3, 4), 1);
        assert_eq!(LineNumberMode::Relative.number_for_line(5, 4), 1);
    }

    #[test]
    fn editor_view_calculates_visible_line_range() {
        let buffer = EditorBuffer::from_text("1\n2\n3\n4\n5".to_string());
        let mut view = EditorView::new();

        assert_eq!(view.visible_line_range(&buffer, 20.0, 10.0), 0..3);

        view.scroll_y = 20.0;
        assert_eq!(view.visible_line_range(&buffer, 20.0, 10.0), 2..5);
    }
}
