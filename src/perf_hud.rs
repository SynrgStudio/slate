//! On-screen timing overlay (`F12` / `:perf`). Shows what the editor actually
//! spends per frame, per keystroke and per search so speed work is measured,
//! not guessed. Recording is a few `Instant` reads per frame; drawing only
//! happens while the overlay is visible.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, RichText};

const WINDOW: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct FrameSample {
    ended: Instant,
    cpu_ms: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct TimedResult {
    pub(crate) ms: f32,
    pub(crate) count: usize,
}

#[derive(Default)]
pub(crate) struct PerfHud {
    pub(crate) visible: bool,
    frames: VecDeque<FrameSample>,
    last_input_frame_ms: Option<f32>,
    previous_total_cpu_ms: Option<f32>,
    startup_ms: Option<f32>,
    last_find: Option<TimedResult>,
    last_note_search: Option<TimedResult>,
    last_open_ms: Option<f32>,
    /// Frames to repaint immediately after opening, while egui measures the area.
    settle_frames: u8,
}

pub(crate) struct DocumentStats {
    pub(crate) bytes: usize,
    pub(crate) lines: usize,
    pub(crate) undo_bytes: usize,
}

impl PerfHud {
    pub(crate) fn toggle(&mut self) {
        self.visible = !self.visible;
        self.frames.clear();
        self.settle_frames = 3;
    }

    /// Called once per frame with the CPU time Slate spent building it.
    pub(crate) fn record_frame(
        &mut self,
        cpu: Duration,
        had_input: bool,
        previous_total_cpu: Option<f32>,
    ) {
        let now = Instant::now();
        let cpu_ms = cpu.as_secs_f32() * 1000.0;
        if self.startup_ms.is_none() {
            self.startup_ms = crate::perf_log::since_start().map(|d| d.as_secs_f32() * 1000.0);
        }
        if had_input {
            self.last_input_frame_ms = Some(cpu_ms);
        }
        self.previous_total_cpu_ms = previous_total_cpu.map(|seconds| seconds * 1000.0);
        self.frames.push_back(FrameSample { ended: now, cpu_ms });
        while self
            .frames
            .front()
            .is_some_and(|frame| now.duration_since(frame.ended) > WINDOW)
        {
            self.frames.pop_front();
        }
    }

    pub(crate) fn record_find(&mut self, elapsed: Duration, count: usize) {
        self.last_find = Some(TimedResult {
            ms: elapsed.as_secs_f32() * 1000.0,
            count,
        });
    }

    pub(crate) fn record_note_search(&mut self, elapsed: Duration, count: usize) {
        self.last_note_search = Some(TimedResult {
            ms: elapsed.as_secs_f32() * 1000.0,
            count,
        });
    }

    pub(crate) fn record_open(&mut self, elapsed: Duration) {
        self.last_open_ms = Some(elapsed.as_secs_f32() * 1000.0);
    }

    fn frame_stats(&self) -> Option<(f32, f32, f32, usize)> {
        let last = self.frames.back()?.cpu_ms;
        let mut sorted = self.frames.iter().map(|f| f.cpu_ms).collect::<Vec<_>>();
        sorted.sort_by(f32::total_cmp);
        let median = sorted[sorted.len() / 2];
        let max = *sorted.last()?;
        Some((last, median, max, self.frames.len()))
    }

    pub(crate) fn lines(&self, doc: &DocumentStats) -> Vec<(String, String)> {
        let ms = |value: Option<f32>| value.map_or("—".to_string(), |v| format!("{v:.2} ms"));
        let mut rows = Vec::new();
        if let Some((last, median, max, frames)) = self.frame_stats() {
            rows.push(("frame cpu".into(), format!("{last:.2} ms")));
            rows.push((
                "  2 s window".into(),
                format!("p50 {median:.2} · max {max:.2} · {frames} frames"),
            ));
        }
        rows.push(("total cpu (prev)".into(), ms(self.previous_total_cpu_ms)));
        rows.push(("keystroke frame".into(), ms(self.last_input_frame_ms)));
        if let Some(find) = self.last_find {
            rows.push((
                "find".into(),
                format!("{:.2} ms · {} matches", find.ms, find.count),
            ));
        }
        if let Some(search) = self.last_note_search {
            rows.push((
                "find-note".into(),
                format!("{:.2} ms · {} files", search.ms, search.count),
            ));
        }
        if let Some(open) = self.last_open_ms {
            rows.push(("last open".into(), format!("{open:.2} ms")));
        }
        rows.push(("startup".into(), ms(self.startup_ms)));
        rows.push((
            "document".into(),
            format!("{} · {} lines", human_bytes(doc.bytes), doc.lines),
        ));
        rows.push(("undo history".into(), human_bytes(doc.undo_bytes)));
        rows.push((
            "build".into(),
            if cfg!(debug_assertions) {
                "debug".into()
            } else {
                "release".into()
            },
        ));
        rows
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, doc: &DocumentStats) {
        if !self.visible {
            return;
        }
        let rows = self.lines(doc);
        egui::Area::new(egui::Id::new("slate_perf_hud"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_TOP, egui::vec2(-12.0, 40.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(20, 24, 32, 230))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(76, 86, 106)))
                    .inner_margin(egui::Margin::same(8))
                    .corner_radius(4.0_f32)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("perf · F12")
                                .font(FontId::monospace(12.0))
                                .color(Color32::from_rgb(136, 192, 208)),
                        );
                        egui::Grid::new("slate_perf_hud_grid")
                            .num_columns(2)
                            .spacing(egui::vec2(12.0, 2.0))
                            .show(ui, |ui| {
                                for (label, value) in rows {
                                    ui.label(
                                        RichText::new(label)
                                            .font(FontId::monospace(12.0))
                                            .color(Color32::from_rgb(129, 140, 160)),
                                    );
                                    ui.label(
                                        RichText::new(value)
                                            .font(FontId::monospace(12.0))
                                            .color(Color32::from_rgb(216, 222, 233)),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            });
        // Keep the numbers fresh without forcing a continuous render loop; the
        // first frames repaint at once so the anchored area settles immediately.
        if self.settle_frames > 0 {
            self.settle_frames -= 1;
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

fn human_bytes(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes < KIB {
        format!("{bytes:.0} B")
    } else if bytes < KIB * KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{:.1} MiB", bytes / KIB / KIB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_reports_frame_and_search_timings() {
        let mut hud = PerfHud::default();
        hud.record_frame(Duration::from_micros(1500), true, Some(0.004));
        hud.record_frame(Duration::from_micros(500), false, None);
        hud.record_find(Duration::from_micros(2100), 42);
        let rows = hud.lines(&DocumentStats {
            bytes: 5 * 1024 * 1024,
            lines: 27_343,
            undo_bytes: 12,
        });
        let find = |key: &str| {
            rows.iter()
                .find(|(label, _)| label == key)
                .map(|(_, value)| value.clone())
                .unwrap()
        };
        assert_eq!(find("frame cpu"), "0.50 ms");
        assert_eq!(find("keystroke frame"), "1.50 ms");
        assert_eq!(find("find"), "2.10 ms · 42 matches");
        assert_eq!(find("document"), "5.0 MiB · 27343 lines");
        assert_eq!(find("undo history"), "12 B");
    }

    #[test]
    #[allow(deprecated)]
    fn visible_hud_renders_inside_the_window() {
        let ctx = egui::Context::default();
        let mut hud = PerfHud::default();
        hud.toggle();
        hud.record_frame(Duration::from_micros(800), true, Some(0.002));
        let stats = DocumentStats {
            bytes: 1024,
            lines: 10,
            undo_bytes: 0,
        };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 560.0));
        // Opening repaints immediately for a few frames; by then it is in bounds.
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| hud.show(ctx, &stats),
            );
        }
        let rect = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("slate_perf_hud")))
            .expect("hud area painted");
        assert!(screen.contains_rect(rect), "{rect:?} inside {screen:?}");
        assert!(rect.width() > 100.0 && rect.height() > 100.0);
    }

    #[test]
    fn toggle_flips_visibility() {
        let mut hud = PerfHud::default();
        hud.toggle();
        assert!(hud.visible);
        hud.toggle();
        assert!(!hud.visible);
    }
}
