//! `POST /api/board_stats` (task item 8): port of `pcbnew/board_statistics_
//! report.cpp`'s `ComputeBoardStatistics` / `pcbnew/board_statistics.cpp`'s
//! `CollectDrillLineItems`. Read-only -- "Board Statistics..." never
//! mutates the board, so unlike every other item this task ported this is
//! a stateless computation with no `Cmd` at all, same shape as
//! `fab_api::bom`.
//!
//! Scope notes (this model's geometry is much simpler than source's real
//! polygon engine, so several metrics are honest approximations rather
//! than exact):
//! - Front/back copper area sums each item's own simple shape formula
//!   (track = capsule, via/round pad = circle, rect/round-rect/oval pad
//!   formulas below) on F.Cu/B.Cu only -- same two-layer scope source's
//!   own `IsOnLayer(F_Cu)`/`IsOnLayer(B_Cu)` calls have (inner layers
//!   were never included upstream either). Zone *fills* are not
//!   included (computing one requires the real zone-filler, out of
//!   proportion for a summary dialog) -- only tracks/vias/pads.
//! - Footprint courtyard area is the sum of each part's own bounding-box
//!   courtyard (`placed_courtyard` -- this model's courtyard is a bbox,
//!   not a polygon, an existing limitation predating this item), so
//!   overlapping courtyards double-count instead of union-ing away like
//!   source's real polygon `Simplify()`.
//! - Footprint type (THT/SMD/Unspecified) is derived from the footprint's
//!   own pad composition (any through-hole pad -> THT, else any SMD pad
//!   -> SMD, else Unspecified) rather than a stored attribute flag --
//!   robust regardless of whether a board's footprints were ever opened
//!   in the Footprint Editor (which is the only place this model's own
//!   `FootprintAttributes` actually lives).
//! - Pad categories are this model's three `PadKind`s (through-hole/SMD/
//!   NPTH) -- no separate "Connector" category, and no castellated/
//!   press-fit pad properties (not modeled).
//! - Vias are a single count -- no through/blind/buried/micro split
//!   (this model has only one via "type", the same limitation section 13
//!   of PARITY-pcb.md already documents for Global Edit Tracks & Vias).
//! - Min track-to-track clearance is tracks only (same layer, different
//!   net, closest approach via `eda_connectivity::geom::seg_seg_distance`)
//!   -- vias are excluded from this specific pairwise check, unlike
//!   source's own version, which also walks vias against tracks.

use crate::board;
use eda_connectivity::geom::seg_seg_distance;
use eda_model::footprint::placed_courtyard;
use eda_model::ir::{Design, Point, Side, Um};
use eda_model::{ConstraintModel, PadKind, PadShape};
use serde_json::{json, Value};
use std::path::Path;

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// Shoelace formula, um^2 -- same convention `polygonArea` in this
/// project's own frontend (`selectionCandidates.ts`) already uses for
/// exactly this (a simple, non-self-intersecting polygon, no holes).
fn polygon_area(pts: &[Point]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut sum = 0i64 as f64;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        sum += (a.x as f64) * (b.y as f64) - (b.x as f64) * (a.y as f64);
    }
    (sum / 2.0).abs()
}

fn bbox_of(pts: &[Point]) -> Option<(Um, Um, Um, Um)> {
    if pts.is_empty() {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some((x0, y0, x1, y1))
}

/// `PAD::GetEffectiveShape`-adjacent area, um^2 -- see this module's own
/// header for the per-shape formulas and why these four (the only ones
/// this model's resolved `PadShape` has -- trapezoid/chamfered-rect
/// footprint pads resolve to one of these before reaching here).
fn pad_area(shape: PadShape, size: (Um, Um), roundrect_ratio: Option<f64>) -> f64 {
    let (w, h) = (size.0 as f64, size.1 as f64);
    match shape {
        PadShape::Rect => w * h,
        PadShape::Circle => std::f64::consts::PI * (w / 2.0).powi(2),
        PadShape::Oval => {
            let (short, long) = if w < h { (w, h) } else { (h, w) };
            (long - short) * short + std::f64::consts::PI * (short / 2.0).powi(2)
        }
        PadShape::RoundRect => {
            let ratio = roundrect_ratio.unwrap_or(0.25);
            let r = ratio * w.min(h);
            w * h - (4.0 - std::f64::consts::PI) * r * r
        }
    }
}

/// One row of the drill table (`DRILL_LINE_ITEM`) -- grouped by every
/// field but the count, same `addOrIncrement` shape as source.
#[derive(PartialEq, Eq, Hash, Clone)]
struct DrillKey {
    x_um: Um,
    y_um: Um,
    oval: bool,
    plated: bool,
    is_pad: bool,
    layer_span: String,
}

pub fn compute(dir: &Path) -> Value {
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    compute_from(&design, &model)
}

/// The real computation, split out from [`compute`] purely so it can be
/// exercised directly with a hand-built fixture instead of a real board
/// directory on disk.
fn compute_from(design: &Design, model: &ConstraintModel) -> Value {
    let outer_first = model.board.layers.first().cloned().unwrap_or_else(|| "F.Cu".to_string());
    let outer_last = model.board.layers.last().cloned().unwrap_or_else(|| "B.Cu".to_string());

    let (mut tht_front, mut tht_back) = (0i64, 0i64);
    let (mut smd_front, mut smd_back) = (0i64, 0i64);
    let (mut unspec_front, mut unspec_back) = (0i64, 0i64);
    let (mut pad_pth, mut pad_smd, mut pad_npth) = (0i64, 0i64, 0i64);
    let (mut front_courtyard_area, mut back_courtyard_area) = (0.0, 0.0);
    let (mut front_copper_area, mut back_copper_area) = (0.0, 0.0);
    let mut min_drill_um: Option<Um> = None;
    let mut drills: std::collections::HashMap<DrillKey, i64> = std::collections::HashMap::new();

    let pl = design.placement.as_ref();

    for part in &model.parts {
        let Some(fpi) = pl.and_then(|pl| pl.footprints.iter().find(|f| f.id == part.reference)) else {
            continue; // Unplaced: source's own stats only ever walk BOARD::Footprints(), i.e. placed ones.
        };
        let Some(footprint) = model.footprint_of(part) else { continue };

        let has_tht = footprint.pads.iter().any(|p| p.kind == PadKind::ThroughHole);
        let has_smd = footprint.pads.iter().any(|p| p.kind == PadKind::Smd);
        let front = fpi.side == Side::Top;

        match (has_tht, has_smd, front) {
            (true, _, true) => tht_front += 1,
            (true, _, false) => tht_back += 1,
            (false, true, true) => smd_front += 1,
            (false, true, false) => smd_back += 1,
            (false, false, true) => unspec_front += 1,
            (false, false, false) => unspec_back += 1,
        }

        for p in &footprint.pads {
            match p.kind {
                PadKind::ThroughHole => pad_pth += 1,
                PadKind::Smd => pad_smd += 1,
                PadKind::NonPlatedHole => pad_npth += 1,
            }

            if p.kind != PadKind::NonPlatedHole {
                let area = pad_area(p.shape, p.size, p.roundrect_ratio);
                if p.kind == PadKind::ThroughHole {
                    // A plated through-hole pad's copper exists on every layer it spans -- at minimum both outer ones.
                    front_copper_area += area;
                    back_copper_area += area;
                } else if front {
                    front_copper_area += area;
                } else {
                    back_copper_area += area;
                }
            }

            if let Some(d) = p.drill {
                if d > 0 {
                    min_drill_um = Some(min_drill_um.map_or(d, |m| m.min(d)));
                    *drills
                        .entry(DrillKey { x_um: d, y_um: d, oval: false, plated: p.kind != PadKind::NonPlatedHole, is_pad: true, layer_span: format!("{outer_first}-{outer_last}") })
                        .or_insert(0) += 1;
                }
            } else if let Some((dx, dy)) = p.drill_slot {
                if dx > 0 && dy > 0 {
                    *drills
                        .entry(DrillKey { x_um: dx, y_um: dy, oval: true, plated: p.kind != PadKind::NonPlatedHole, is_pad: true, layer_span: format!("{outer_first}-{outer_last}") })
                        .or_insert(0) += 1;
                }
            }
        }

        if let Some((x0, y0, x1, y1)) = placed_courtyard(model, part, fpi) {
            let area = ((x1 - x0) as f64) * ((y1 - y0) as f64);
            if front {
                front_courtyard_area += area;
            } else {
                back_courtyard_area += area;
            }
        }
    }

    let mut min_track_width: Option<Um> = None;
    let mut via_count = 0i64;
    let mut min_clearance_um: Option<f64> = None;

    if let Some(rt) = &design.routing {
        for t in &rt.tracks {
            min_track_width = Some(min_track_width.map_or(t.width, |m| m.min(t.width)));

            let mut len = 0.0;
            for w in t.pts.windows(2) {
                len += (((w[1].x - w[0].x).pow(2) + (w[1].y - w[0].y).pow(2)) as f64).sqrt();
            }
            let area = len * t.width as f64 + std::f64::consts::PI * (t.width as f64 / 2.0).powi(2);
            if t.layer == outer_first {
                front_copper_area += area;
            } else if t.layer == outer_last {
                back_copper_area += area;
            }
        }

        for v in &rt.vias {
            via_count += 1;
            if v.drill > 0 {
                min_drill_um = Some(min_drill_um.map_or(v.drill, |m| m.min(v.drill)));
                *drills
                    .entry(DrillKey { x_um: v.drill, y_um: v.drill, oval: false, plated: true, is_pad: false, layer_span: format!("{}-{}", v.from_layer, v.to_layer) })
                    .or_insert(0) += 1;
            }
            let area = std::f64::consts::PI * (v.diameter as f64 / 2.0).powi(2);
            if v.from_layer == outer_first || v.to_layer == outer_first {
                front_copper_area += area;
            }
            if v.from_layer == outer_last || v.to_layer == outer_last {
                back_copper_area += area;
            }
        }

        // Pairwise same-layer, different-net track clearance -- O(n^2),
        // acceptable for a one-shot report the same way source's own
        // version (also a full pairwise walk) is.
        for i in 0..rt.tracks.len() {
            for j in (i + 1)..rt.tracks.len() {
                let (a, b) = (&rt.tracks[i], &rt.tracks[j]);
                if a.layer != b.layer || a.net == b.net {
                    continue;
                }
                for wa in a.pts.windows(2) {
                    for wb in b.pts.windows(2) {
                        let d = seg_seg_distance(wa[0], wa[1], wb[0], wb[1]) - (a.width as f64 + b.width as f64) / 2.0;
                        min_clearance_um = Some(min_clearance_um.map_or(d, |m: f64| m.min(d)));
                    }
                }
            }
        }
    }

    let outline = pl.map(|pl| pl.outline.as_slice()).unwrap_or(&[]);
    let has_outline = outline.len() >= 3;
    let board_area = if has_outline { polygon_area(outline) } else { 0.0 };
    let board_bbox = bbox_of(outline);
    let (board_width, board_height) = board_bbox.map(|(x0, y0, x1, y1)| (x1 - x0, y1 - y0)).unwrap_or((0, 0));

    let board_thickness_mm = model.stackup.as_ref().map(|s| s.layers.iter().filter_map(|l| l.thickness_mm).sum::<f64>());

    let mut drill_rows: Vec<Value> = drills
        .into_iter()
        .map(|(k, qty)| {
            json!({
                "qty": qty,
                "shape": if k.oval { "oval" } else { "circle" },
                "x_um": k.x_um,
                "y_um": k.y_um,
                "plated": k.plated,
                "is_pad": k.is_pad,
                "layer_span": k.layer_span,
            })
        })
        .collect();
    drill_rows.sort_by(|a, b| b["qty"].as_i64().cmp(&a["qty"].as_i64()));

    json!({
        "ok": true,
        "board": {
            "has_outline": has_outline,
            "width_um": board_width,
            "height_um": board_height,
            "area_um2": board_area,
            "front_copper_area_um2": front_copper_area,
            "back_copper_area_um2": back_copper_area,
            "front_courtyard_area_um2": front_courtyard_area,
            "back_courtyard_area_um2": back_courtyard_area,
            "front_density_pct": if has_outline && board_area > 0.0 { Some(front_courtyard_area * 100.0 / board_area) } else { None },
            "back_density_pct": if has_outline && board_area > 0.0 { Some(back_courtyard_area * 100.0 / board_area) } else { None },
            "min_track_width_um": min_track_width,
            "min_clearance_um": min_clearance_um,
            "min_drill_um": min_drill_um,
            "thickness_mm": board_thickness_mm,
        },
        "footprints": {
            "tht_front": tht_front, "tht_back": tht_back,
            "smd_front": smd_front, "smd_back": smd_back,
            "unspecified_front": unspec_front, "unspecified_back": unspec_back,
            "total": tht_front + tht_back + smd_front + smd_back + unspec_front + unspec_back,
        },
        "pads": { "through_hole": pad_pth, "smd": pad_smd, "non_plated_hole": pad_npth },
        "vias": { "count": via_count },
        "drills": drill_rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{PlacementSection, Provenance};

    fn design_with_outline(outline: Vec<Point>) -> Design {
        Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![], symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            routing: None,
            placement: Some(PlacementSection { outline, footprints: Vec::new(), modules: Vec::new() }),
            drawings: None,
        }
    }

    #[test]
    fn compute_from_reports_board_dimensions_and_area_from_the_outline() {
        let design = design_with_outline(vec![Point { x: 0, y: 0 }, Point { x: 100_000, y: 0 }, Point { x: 100_000, y: 50_000 }, Point { x: 0, y: 50_000 }]);
        let stats = compute_from(&design, &ConstraintModel::default());
        assert_eq!(stats["ok"], true);
        assert_eq!(stats["board"]["has_outline"], true);
        assert_eq!(stats["board"]["width_um"], 100_000);
        assert_eq!(stats["board"]["height_um"], 50_000);
        assert_eq!(stats["board"]["area_um2"], 5_000_000_000.0);
    }

    #[test]
    fn compute_from_with_no_outline_reports_zeroed_board_geometry_without_panicking() {
        let design = design_with_outline(vec![]);
        let stats = compute_from(&design, &ConstraintModel::default());
        assert_eq!(stats["ok"], true);
        assert_eq!(stats["board"]["has_outline"], false);
        assert_eq!(stats["board"]["area_um2"], 0.0);
        assert!(stats["board"]["front_density_pct"].is_null(), "density needs a real board area to divide by");
    }

    #[test]
    fn polygon_area_of_a_rectangle() {
        let pts = [Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 5_000 }, Point { x: 0, y: 5_000 }];
        assert_eq!(polygon_area(&pts), 50_000_000.0);
    }

    #[test]
    fn polygon_area_is_winding_independent() {
        let cw = [Point { x: 0, y: 0 }, Point { x: 0, y: 5_000 }, Point { x: 10_000, y: 5_000 }, Point { x: 10_000, y: 0 }];
        assert_eq!(polygon_area(&cw), 50_000_000.0);
    }

    #[test]
    fn polygon_area_of_fewer_than_three_points_is_zero() {
        assert_eq!(polygon_area(&[Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }]), 0.0);
    }

    #[test]
    fn bbox_of_points() {
        let pts = [Point { x: -500, y: 1000 }, Point { x: 2000, y: -300 }, Point { x: 100, y: 50 }];
        assert_eq!(bbox_of(&pts), Some((-500, -300, 2000, 1000)));
        assert_eq!(bbox_of(&[]), None);
    }

    #[test]
    fn pad_area_rect_is_width_times_height() {
        assert_eq!(pad_area(PadShape::Rect, (2000, 1000), None), 2_000_000.0);
    }

    #[test]
    fn pad_area_circle_uses_the_width_as_diameter() {
        let a = pad_area(PadShape::Circle, (2000, 2000), None);
        assert!((a - std::f64::consts::PI * 1000.0 * 1000.0).abs() < 1e-6);
    }

    #[test]
    fn pad_area_oval_is_a_capsule_not_a_plain_rectangle() {
        let rect_area = 3000.0 * 1000.0;
        let oval_area = pad_area(PadShape::Oval, (3000, 1000), None);
        // A capsule always has strictly less area than its bounding rect
        // (the two corners are rounded off, not squared), and strictly
        // more than the inscribed circle alone.
        assert!(oval_area < rect_area);
        assert!(oval_area > std::f64::consts::PI * 500.0 * 500.0);
    }

    #[test]
    fn pad_area_round_rect_falls_between_a_plain_rect_and_its_corner_cut() {
        let rect_area = 2000.0 * 2000.0;
        let a_default = pad_area(PadShape::RoundRect, (2000, 2000), None); // ratio defaults to 0.25
        let a_sharp = pad_area(PadShape::RoundRect, (2000, 2000), Some(0.0));
        assert_eq!(a_sharp, rect_area, "a zero corner ratio is just the plain rectangle");
        assert!(a_default < rect_area, "a real corner radius must cut area off the plain rectangle");
    }
}
