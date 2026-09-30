//! Ported from `pcbnew/drc/drc_test_provider_hole_size.cpp` and
//! `drc_test_provider_hole_to_hole.cpp`.
//!
//! Generated: `DRCE_DRILL_OUT_OF_RANGE`, `DRCE_DRILLED_HOLES_TOO_CLOSE`,
//! `DRCE_DRILLED_HOLES_COLOCATED`.

use crate::board::DrcBoard;
use crate::constraints;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;

/// One round-drilled hole, for the hole-size and hole-to-hole checks (slot
/// holes are excluded from both -- `checkPadHole` uses the *minor* axis of
/// an oblong drill for hole-size, which we could add, but hole-to-hole
/// explicitly skips slots: "Slots are generally milled _after_ drilling, so
/// we ignore them").
struct RoundHole<'a> {
    pos: Point,
    drill: Um,
    desc: String,
    id: &'a str,
}

fn round_holes(board: &DrcBoard) -> Vec<RoundHole<'_>> {
    let mut out = Vec::new();
    for v in &board.vias {
        out.push(RoundHole { pos: v.at, drill: v.drill, desc: format!("Via [{}] on {}-{}", v.net.as_deref().unwrap_or("<no net>"), v.from_layer, v.to_layer), id: &v.id });
    }
    for p in &board.pads {
        if let Some(d) = p.drill_round {
            out.push(RoundHole { pos: p.center, drill: d, desc: format!("Pad {} [{}] of {}", p.number, p.net.as_deref().unwrap_or("<no net>"), p.footprint_ref), id: &p.id });
        }
    }
    out
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let min_drill = constraints::hole_size_min(rules);

    // ---- hole size: DRCE_DRILL_OUT_OF_RANGE ----
    for h in round_holes(board) {
        if h.drill < min_drill {
            let item = DrcRefItem { description: h.desc.clone(), pos: (h.pos.x, h.pos.y), id: h.id.to_string() };
            out.push(DrcViolation::new(ErrorType::DrillOutOfRange, format!("(board setup constraints min hole {}; actual {})", format_um(min_drill), format_um(h.drill)), vec![item]));
        }
    }
    // Slot drills: hole-size still applies to the minor axis (`checkPadHole`
    // uses `min(drill.x, drill.y)` regardless of round/slot).
    for p in &board.pads {
        if let Some((w, h)) = p.drill_slot {
            let minor: Um = w.min(h);
            if minor < min_drill {
                let item = DrcRefItem { description: format!("Pad {} [{}] of {}", p.number, p.net.as_deref().unwrap_or("<no net>"), p.footprint_ref), pos: (p.center.x, p.center.y), id: p.id.clone() };
                out.push(DrcViolation::new(ErrorType::DrillOutOfRange, format!("(board setup constraints min hole {}; actual {})", format_um(min_drill), format_um(minor)), vec![item]));
            }
        }
    }

    // ---- hole to hole: mechanical, regardless of net ----
    let min_h2h = constraints::hole_to_hole_min(rules);
    let holes = round_holes(board);
    for i in 0..holes.len() {
        for j in (i + 1)..holes.len() {
            let (a, b) = (&holes[i], &holes[j]);
            let center_dist = crate::kimath::dist(a.pos, b.pos);
            if center_dist == 0 {
                let items = vec![DrcRefItem { description: a.desc.clone(), pos: (a.pos.x, a.pos.y), id: a.id.to_string() }, DrcRefItem { description: b.desc.clone(), pos: (b.pos.x, b.pos.y), id: b.id.to_string() }];
                out.push(DrcViolation::new(ErrorType::HolesCoLocated, "", items));
                continue;
            }
            let actual = (center_dist - a.drill / 2 - b.drill / 2).max(0);
            if actual < min_h2h {
                let items = vec![DrcRefItem { description: a.desc.clone(), pos: (a.pos.x, a.pos.y), id: a.id.to_string() }, DrcRefItem { description: b.desc.clone(), pos: (b.pos.x, b.pos.y), id: b.id.to_string() }];
                out.push(DrcViolation::new(ErrorType::HoleToHole, format!("(min {}; actual {})", format_um(min_h2h), format_um(actual)), items));
            }
        }
    }

    out
}
