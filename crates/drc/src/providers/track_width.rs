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
use crate::providers::copper_clearance::{facts_of_track, CourtyardMembership};
use eda_model::BoardRules;

/// `TRACK_WIDTH_CONSTRAINT`: the board's real minimum (never the net
/// class's nominal width -- see [`constraints::track_width_min`]'s doc
/// comment for GAPS.md #10), then any matching `.kicad_dru` `track_width`
/// rule applied on top of the min/max independently
/// ([`constraints::track_width_bounds`]). A board with no such rules
/// behaves exactly as before: every track checked against one board-wide
/// floor, no max.
pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let compiled = constraints::CompiledWidthRules::new(rules);
    // Same reasoning as `copper_clearance::CourtyardMembership`'s own doc
    // comment: compiled once per board, not per track, even though this
    // provider only needs it for `A.insideCourtyard(...)`-conditioned
    // `track_width` rules (none sampled yet, but the condition subset is
    // shared and must not silently stop supporting the function here).
    let courtyards = CourtyardMembership::new(board);
    for t in &board.tracks {
        let facts = facts_of_track(rules, t, &courtyards);
        let (min, max) = constraints::track_width_bounds(rules, &t.layer, &facts, &compiled);
        let item = || DrcRefItem { description: format!("Track [{}] on {}", t.net.as_deref().unwrap_or("<no net>"), t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() };
        if t.width < min {
            out.push(DrcViolation::new(ErrorType::TrackWidth, format!("(board setup constraints min width {}; actual {})", format_um(min), format_um(t.width)), vec![item()]));
        } else if let Some(max) = max {
            if t.width > max {
                out.push(DrcViolation::new(ErrorType::TrackWidth, format!("(constraint max width {}; actual {})", format_um(max), format_um(t.width)), vec![item()]));
            }
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

    fn board_with_track(width: i64, net_class: Option<&str>) -> (DrcBoard, BoardRules) {
        let mut rules = BoardRules::default();
        if let Some(name) = net_class {
            rules.net_classes.push(eda_model::NetClass { name: name.into(), nets: vec!["A".into()], track_width: Some(9_999), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 });
        }
        let board = DrcBoard {
            layers: vec![],
            outline: vec![],
            pads: vec![],
            tracks: vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width, a: Point { x: 0, y: 0 }, b: Point { x: 1000, y: 0 } }],
            vias: vec![],
            zones: vec![],
            keepouts: vec![],
            footprints: vec![],
            shapes: vec![],
            texts: vec![],
            silk_items: vec![],
        };
        (board, rules)
    }

    #[test]
    fn narrower_than_the_board_minimum_fails() {
        let (board, rules) = board_with_track(BoardRules::default().track_width_min_um - 10, None);
        let v = check(&board, &rules);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].error_type, "track_width");
    }

    /// GAPS.md #10's regression: a net class's own (much larger) nominal
    /// width must never be used as the enforced floor -- a track routed
    /// comfortably above the real board minimum but below its net class's
    /// nominal width is not a DRC violation (that nominal value is only
    /// ever the `Opt` KiCad's router aims for).
    #[test]
    fn width_above_the_board_minimum_but_below_the_netclass_nominal_passes() {
        let min = BoardRules::default().track_width_min_um;
        let (board, rules) = board_with_track(min + 10, Some("power")); // net class's own nominal is 9999, far above this
        let v = check(&board, &rules);
        assert!(v.is_empty(), "{v:#?}");
    }

    /// Real QA-corpus shape (`multinetclasses_drc.kicad_dru`): a custom
    /// rule caps one net class's width via `(constraint track_width (max
    /// ...))`, which this check must enforce as a `DRCE_TRACK_WIDTH`
    /// violation too, not just the board's minimum floor.
    #[test]
    fn custom_rule_max_width_is_enforced() {
        let (board, mut rules) = board_with_track(1000, None);
        rules.custom_rules.push(eda_model::CustomRule { name: "cap".into(), constraint_type: "track_width".into(), min: None, max: Some(500), opt: None, layer: None, severity: None, condition: None });
        let v = check(&board, &rules);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].error_type, "track_width");
    }
}
