//! Source-position anchors mapped to actual preview geometry (not pixel/line deltas).
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Position {
    pub source: f32,
    pub at_end: bool,
}
impl Position {
    pub fn differs(self, other: Self) -> bool {
        self.at_end != other.at_end || (self.source - other.source).abs() > 0.0001
    }
}
#[derive(Default)]
pub(crate) struct PreviewSync {
    pub key: Option<(u64, u64, u32, u32)>,
    pub points: Vec<(f32, f32)>,
    pub editor: Position,
    pub offset: f32,
    pub max_scroll: f32,
    pub pending_editor: bool,
}
pub(crate) fn interpolate(points: &[(f32, f32)], value: f32, inverse: bool) -> f32 {
    let axis = |p: (f32, f32)| if inverse { (p.1, p.0) } else { p };
    if points.is_empty() {
        return 0.0;
    }
    let index = points.partition_point(|p| axis(*p).0 <= value);
    if index == 0 {
        return axis(points[0]).1;
    }
    if index == points.len() {
        return axis(*points.last().unwrap()).1;
    }
    let a = axis(points[index - 1]);
    let b = axis(points[index]);
    if b.0 <= a.0 {
        return b.1;
    }
    a.1 + (b.1 - a.1) * ((value - a.0) / (b.0 - a.0)).clamp(0.0, 1.0)
}
impl PreviewSync {
    pub fn offset_for(&self, position: Position) -> f32 {
        if position.at_end {
            self.max_scroll
        } else {
            interpolate(&self.points, position.source, false).clamp(0.0, self.max_scroll)
        }
    }
    pub fn position_for(&self, offset: f32) -> Position {
        Position {
            source: interpolate(&self.points, offset, true),
            at_end: self.max_scroll > 0.0 && offset >= self.max_scroll - 0.5,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unequal_table_heights_map_both_directions_without_equal_pixel_steps() {
        let points = vec![
            (0.0, 0.0),
            (1.0, 20.0),
            (2.0, 80.0),
            (3.0, 80.0),
            (4.0, 180.0),
            (5.0, 200.0),
        ];
        assert_eq!(interpolate(&points, 3.5, false), 130.0);
        assert_eq!(interpolate(&points, 130.0, true), 3.5);
        assert_eq!(interpolate(&points, 80.0, true), 3.0);
        assert_eq!(interpolate(&points, 1000.0, true), 5.0);
    }
    #[test]
    fn endpoints_clamp_and_short_documents_are_safe() {
        let sync = PreviewSync {
            points: vec![(0.0, 0.0), (10.0, 600.0)],
            max_scroll: 300.0,
            ..Default::default()
        };
        assert_eq!(
            sync.offset_for(Position {
                source: 9.0,
                at_end: false
            }),
            300.0
        );
        assert!(sync.position_for(300.0).at_end);
        assert_eq!(
            PreviewSync::default().offset_for(Position {
                source: 100.0,
                at_end: true
            }),
            0.0
        );
    }
}
