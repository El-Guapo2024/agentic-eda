//! Ported from `pcbnew/drc/drc_test_provider_track_width.cpp`.
//! Generated: `DRCE_TRACK_WIDTH`.

use crate::board::DrcBoard;
use crate::constraints;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use eda_model::BoardRules;

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let min = constraints::track_width_min(rules);
    let mut out = Vec::new();
    for t in &board.tracks {
        if t.width < min {
            let item = DrcRefItem { description: format!("Track [{}] on {}", t.net.as_deref().unwrap_or("<no net>"), t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() };
            out.push(DrcViolation::new(ErrorType::TrackWidth, format!("(board setup constraints min width {}; actual {})", format_um(min), format_um(t.width)), vec![item]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::DrcTrackSeg;
    use eda_model::ir::Point;

    #[test]
    fn narrower_than_the_board_minimum_fails() {
        let rules = BoardRules::default();
        let board = DrcBoard {
            layers: vec![],
            outline: vec![],
            pads: vec![],
            tracks: vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width: rules.track_width - 10, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 } }],
            vias: vec![],
            zones: vec![],
            footprints: vec![],
            shapes: vec![],
            texts: vec![],
            silk_items: vec![],
        };
        let v = check(&board, &rules);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].error_type, "track_width");
    }
}
