//! A simplified, self-contained port of the dangling-item half of
//! `pcbnew/drc/drc_test_provider_connectivity.cpp` (`DRCE_DANGLING_TRACK`/
//! `DRCE_DANGLING_VIA`, via `CONNECTIVITY_DATA::TestTrackEndpointDangling`).
//!
//! KiCad's own version answers this from the board's full ratsnest graph
//! (`CONNECTIVITY_DATA`); connectivity is being ported separately in this
//! workspace (see the task brief), so this is a direct geometric stand-in
//! rather than a port of that graph: a track end (or a via) counts as
//! connected when some *other*, same-net pad/track/via/zone actually
//! touches it there. This is exactly what the real algorithm reduces to
//! for the common case (`SHAPE::Collide` at each endpoint, same-net
//! filtering already done by the caller) -- it just does the lookup
//! directly instead of through a persistent connectivity index.
//! `DRCE_UNCONNECTED_ITEMS` (cross-net ratsnest, needing the full netlist
//! graph) is out of scope per the brief and not attempted here.
//!
//! Generated: `DRCE_DANGLING_TRACK`, `DRCE_DANGLING_VIA`.

use crate::board::DrcBoard;
use crate::item::{DrcRefItem, DrcViolation, ErrorType};
use crate::kimath::Shape;
use eda_model::ir::Point;

/// Whether `p` touches `shape` within `tol` -- a zero-radius probe through
/// the same `collides` used everywhere else, so "inside a same-net zone"
/// (via its containment shortcut) counts as touching, exactly as
/// `TestTrackEndpointDangling`'s "both start and end in a zone" case does.
fn touches(p: Point, shape: &Shape, tol: eda_model::ir::Um) -> bool {
    Shape::Circle { c: p, r: 0 }.collides(shape, tol.max(1)).is_some()
}

fn anything_touches(board: &DrcBoard, p: Point, net: &str, tol: eda_model::ir::Um, skip_track_idx: Option<usize>) -> bool {
    for pad in &board.pads {
        if pad.net.as_deref() == Some(net) && touches(p, &pad.copper, tol) {
            return true;
        }
    }
    for (i, t) in board.tracks.iter().enumerate() {
        if Some(i) == skip_track_idx {
            continue;
        }
        if t.net.as_deref() == Some(net) && touches(p, &t.shape(), tol) {
            return true;
        }
    }
    for v in &board.vias {
        if v.net.as_deref() == Some(net) && touches(p, &v.shape(), tol) {
            return true;
        }
    }
    for z in &board.zones {
        if z.net.as_deref() == Some(net) && touches(p, &z.shape(), tol) {
            return true;
        }
    }
    false
}

pub fn check(board: &DrcBoard) -> Vec<DrcViolation> {
    let mut out = Vec::new();

    for (i, t) in board.tracks.iter().enumerate() {
        let Some(net) = t.net.as_deref() else { continue };
        let tol = (t.width / 2).max(1);
        let start_ok = anything_touches(board, t.a, net, tol, Some(i));
        let end_ok = anything_touches(board, t.b, net, tol, Some(i));
        if !start_ok || !end_ok {
            let item = DrcRefItem { description: format!("Track [{net}] on {}", t.layer), pos: (t.a.x, t.a.y), id: t.id.clone() };
            out.push(DrcViolation::new(ErrorType::TrackDangling, "", vec![item]));
        }
    }

    'via: for v in &board.vias {
        let Some(net) = v.net.as_deref() else { continue };
        let tol = (v.diameter / 2).max(1);
        for pad in &board.pads {
            if pad.net.as_deref() == Some(net) && touches(v.at, &pad.copper, tol) {
                continue 'via;
            }
        }
        for t in &board.tracks {
            if t.net.as_deref() == Some(net) && touches(v.at, &t.shape(), tol) {
                continue 'via;
            }
        }
        for other in &board.vias {
            if std::ptr::eq(other, v) {
                continue;
            }
            if other.net.as_deref() == Some(net) && touches(v.at, &other.shape(), tol) {
                continue 'via;
            }
        }
        for z in &board.zones {
            if z.net.as_deref() == Some(net) && touches(v.at, &z.shape(), tol) {
                continue 'via;
            }
        }
        let item = DrcRefItem { description: format!("Via [{net}] on {}-{}", v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() };
        out.push(DrcViolation::new(ErrorType::ViaDangling, "", vec![item]));
    }

    out
}
