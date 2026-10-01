//! Ported from `pcbnew/drc/drc_test_provider_track_width.cpp`.
//! Generated: `DRCE_TRACK_WIDTH`.
//!
//! [`check_netclass_conformance`] is not from KiCad: it is a port of
//! `eda_gates::pcb`'s own `routing_track_width` (a *different* question --
//! "did the router use the width the intent's net class asked for", not
//! KiCad's manufacturability floor). No KiCad equivalent -- KiCad has no
//! notion of "the intent assigned this net a class" -- so it stays under
//! its original name/error code rather than folding into `DRCE_TRACK_WIDTH`
//! above. `eda_gates::pcb` still carries its own copy too (see this crate's
//! top-level doc comment on integration status); this is available to call
//! directly but not yet wired in as its replacement.

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

/// `eda_gates::pcb::check_routing`'s original `routing_track_width`: a net
/// belonging to a class must carry that class's assigned width, not merely
/// clear the board's manufacturability floor -- a power net routed at
/// signal width is exactly the failure the class exists to prevent, and it
/// looks clean by [`check`] alone.
pub fn check_netclass_conformance(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    for t in &board.tracks {
        let Some(net) = t.net.as_deref() else { continue };
        let want = rules.width_of(net);
        if t.width < want {
            let class = rules.class_of(net).map(|c| c.name.as_str()).unwrap_or("default");
            let item = DrcRefItem { description: format!("Track [{net}] on {}", t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() };
            out.push(DrcViolation::new(ErrorType::NetClassTrackWidth, format!("width {} < {} required by net class {class}", t.width, want), vec![item]));
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
            keepouts: vec![],
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
