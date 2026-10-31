//! Bounded derived height metadata; off-screen preview lines do not create widgets/galleys.
use crate::editor_buffer::EditorBuffer;
#[derive(Default)]
pub(crate) struct Heights {
    key: Option<(u64, u64, u32)>,
    lines: Vec<Line>,
}
#[derive(Default)]
struct Line {
    characters: usize,
    measured: Option<(f32, f32, f32, bool)>,
}
impl Heights {
    pub fn ensure(&mut self, buffer: &EditorBuffer, scale: f32) {
        let key = (buffer.identity, buffer.revision, scale.to_bits());
        if self.key != Some(key) {
            self.lines = buffer
                .as_str()
                .lines()
                .map(|text| Line {
                    characters: text.chars().count(),
                    measured: None,
                })
                .collect();
            self.key = Some(key);
        }
    }
    pub fn estimate(&self, index: usize, width: f32, size: f32, extra: f32, wrapped: bool) -> f32 {
        let Some(line) = self.lines.get(index) else {
            return size + extra;
        };
        if let Some((old_width, height, old_extra, old_wrapped)) = line.measured {
            if !old_wrapped || (old_width - width).abs() < 0.5 {
                return height;
            }
            return ((height - old_extra).max(size) * (old_width / width.max(40.0))).max(size)
                + old_extra;
        }
        if !wrapped {
            return size + extra;
        }
        ((line.characters as f32 * size * 0.5 / width.max(40.0))
            .ceil()
            .max(1.0))
            * size
            + extra
    }
    pub fn measured(&mut self, index: usize, width: f32, height: f32, extra: f32, wrapped: bool) {
        if let Some(line) = self.lines.get_mut(index) {
            line.measured = Some((width, height, extra, wrapped));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn height_metadata_reuses_measurements_and_invalidates_text_identity_scale() {
        let mut heights = Heights::default();
        let mut buffer = EditorBuffer::from_text("long text ".repeat(50));
        heights.ensure(&buffer, 1.0);
        heights.measured(0, 400.0, 60.0, 4.0, true);
        assert_eq!(heights.estimate(0, 400.0, 18.0, 4.0, true), 60.0);
        assert!(heights.estimate(0, 200.0, 18.0, 4.0, true) > 100.0);
        heights.ensure(&buffer, 1.0);
        assert_eq!(heights.estimate(0, 400.0, 18.0, 4.0, true), 60.0);
        buffer.insert_text("changed");
        heights.ensure(&buffer, 1.0);
        assert!(heights.lines[0].measured.is_none());
        heights.measured(0, 400.0, 60.0, 4.0, true);
        heights.ensure(&buffer, 1.5);
        assert!(heights.lines[0].measured.is_none());
    }
}
