//! Ported from `pcbnew/drc/drc_test_provider_disallow.cpp`'s keepout half
//! (task item 3) -- the `DRCE_TEXT_ON_EDGECUTS` half of that file is a
//! separate, unrelated check (text/dimensions drawn on `Edge.Cuts`) and is
//! not ported here.
//!
//! Scope, forced by this model's single-layer `Zone`/keepout (see
//! `eda_model::ir::Zone`'s own doc on `is_rule_area`): a rule area is one
//! outline on one named layer, not a `LSET` of several. KiCad's real
//! `EvalRules(DISALLOW_CONSTRAINT, ...)` also covers arbitrary custom
//! `(disallow ...)` DRC rules on non-keepout items; this model has no
//! custom-rule language (`docs/parity/GAPS.md` #10), so only the explicit
//! keepout-zone path is ported.
//!
//! A footprint's "no footprints" test ignores the keepout's own layer --
//! a component either does or doesn't belong in a board area regardless
//! of which single layer its exclusion zone happened to be drawn on,
//! unlike a track/via/pad, which are genuinely layer-specific copper.

use crate::board::{DrcBoard, DrcKeepout};
use crate::fill::fill_all_zones;
use crate::item::{DrcRefItem, DrcViolation, ErrorType};
use crate::kimath::Shape;
use eda_model::ir::Point;
use eda_model::BoardRules;

fn layer_index(layers: &[String], name: &str) -> Option<usize> {
    layers.iter().position(|l| l == name)
}

/// Does a via spanning `from`..`to` (inclusive, either order) reach
/// `layer`? Falls back to an exact-name match if either name is missing
/// from the board's own layer list (shouldn't happen for a valid board,
/// but never panics on a stale/foreign layer name).
fn via_spans_layer(from: &str, to: &str, layer: &str, layers: &[String]) -> bool {
    match (layer_index(layers, from), layer_index(layers, to), layer_index(layers, layer)) {
        (Some(a), Some(b), Some(c)) => c >= a.min(b) && c <= a.max(b),
        _ => from == layer || to == layer,
    }
}

fn keepout_shape(k: &DrcKeepout) -> Shape {
    Shape::Polygon { pts: k.outline.clone() }
}

/// The keepout's outline, shrunk by a hairline epsilon --
/// `drc_test_provider_disallow.cpp`'s own `query_areas`: "Collisions
/// include touching, so we need to deflate outline by enough to exclude
/// it. This is particularly important for detecting copper fills as they
/// will be exactly touching along the entire exclusion border." The
/// filler cuts a copper-pour keepout out of a fill at exactly zero gap
/// (`FillKeepout`'s own doc), so the fill's own hole boundary is
/// literally coincident with this outline; without the deflate, every
/// correctly-excluded fill would still register as "touching" and false-
/// positive here.
fn deflated_keepout_outline(outline: &[Point]) -> Vec<Point> {
    use eda_shape_poly_set::{CornerStrategy, ShapePolySet};
    let chain: Vec<eda_clipper2::Point64> = outline.iter().map(|p| eda_clipper2::Point64::new(p.x, p.y)).collect();
    let mut sps = ShapePolySet::from_outline(chain);
    sps.deflate(1, CornerStrategy::ChamferAllCorners, 5);
    sps.polys.first().and_then(|poly| poly.first()).map(|c| c.iter().map(|p| Point { x: p.x, y: p.y }).collect()).unwrap_or_default()
}

fn violation(kind: &str, keepout_id: &str, item_desc: String, item_pos: Point, item_id: String) -> DrcViolation {
    DrcViolation::new(
        ErrorType::ItemsNotAllowed,
        format!("{kind} not allowed in rule area"),
        vec![DrcRefItem { description: item_desc, pos: (item_pos.x, item_pos.y), id: item_id }, DrcRefItem { description: "Rule area".into(), pos: (0, 0), id: keepout_id.to_string() }],
    )
}

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    if board.keepouts.is_empty() {
        return out;
    }

    for k in &board.keepouts {
        let kshape = keepout_shape(k);

        if k.no_tracks {
            for t in &board.tracks {
                if t.layer != k.layer {
                    continue;
                }
                if kshape.collides(&t.shape(), 0).is_some() {
                    out.push(violation("Track", &k.id, format!("Track on {}", t.layer), t.a, t.id.clone()));
                }
            }
        }

        if k.no_vias {
            for v in &board.vias {
                if !via_spans_layer(&v.from_layer, &v.to_layer, &k.layer, &board.layers) {
                    continue;
                }
                if kshape.collides(&v.shape(), 0).is_some() {
                    out.push(violation("Via", &k.id, "Via".into(), v.at, v.id.clone()));
                }
            }
        }

        if k.no_pads {
            for p in &board.pads {
                if !p.layers.iter().any(|l| l == &k.layer) {
                    continue;
                }
                if kshape.collides(&p.copper, 0).is_some() {
                    out.push(violation("Pad", &k.id, format!("Pad {} of {}", p.number, p.footprint_ref), p.center, p.id.clone()));
                }
            }
        }

        if k.no_footprints {
            for f in &board.footprints {
                let (x0, y0, x1, y1) = f.courtyard;
                let fshape = Shape::Rect { x0, y0, x1, y1 };
                if kshape.collides(&fshape, 0).is_some() {
                    let center = Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 };
                    out.push(violation("Footprint", &k.id, format!("Footprint {}", f.id), center, f.id.clone()));
                }
            }
        }
    }

    // Copper-pour keepouts: the filler (`eda_zone_filler`) already excludes
    // keepout area from every fill it computes (task item 3's other
    // deliverable) -- this is the DRC-side sanity check that it actually
    // did, same belt-and-suspenders role `drc_test_provider_disallow.cpp`
    // itself plays upstream (it tests the *already-filled* zone, not the
    // filler's own internal state).
    if board.keepouts.iter().any(|k| k.no_copper_pour) {
        let fills = fill_all_zones(board, rules);
        for k in board.keepouts.iter().filter(|k| k.no_copper_pour) {
            let deflated = deflated_keepout_outline(&k.outline);
            if deflated.len() < 3 {
                continue; // a sliver keepout that vanishes under deflate has nothing left to test
            }
            let kshape = Shape::Polygon { pts: deflated };
            for z in board.zones.iter().filter(|z| z.layer == k.layer) {
                let Some(fill) = fills.zones.get(&z.id) else { continue };
                for poly in &fill.fill.polys {
                    let Some(outline) = poly.first() else { continue };
                    let pts: Vec<Point> = outline.iter().map(|p| Point { x: p.x, y: p.y }).collect();
                    if pts.len() < 3 {
                        continue;
                    }
                    if kshape.collides(&Shape::Polygon { pts }, 0).is_some() {
                        out.push(violation("Copper pour", &k.id, format!("Zone on {}", z.layer), k.outline.first().copied().unwrap_or(Point { x: 0, y: 0 }), z.id.clone()));
                        break;
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{DrcPad, DrcTrackSeg, DrcVia};
    use eda_model::PadKind;

    fn empty_board() -> DrcBoard {
        DrcBoard { layers: vec!["F.Cu".into(), "B.Cu".into()], outline: vec![], pads: vec![], tracks: vec![], vias: vec![], zones: vec![], keepouts: vec![], footprints: vec![], shapes: vec![], texts: vec![], silk_items: vec![] }
    }

    fn rect_keepout(id: &str, layer: &str, x0: i64, y0: i64, x1: i64, y1: i64) -> DrcKeepout {
        DrcKeepout { id: id.into(), layer: layer.into(), outline: vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }], no_tracks: false, no_vias: false, no_pads: false, no_copper_pour: false, no_footprints: false }
    }

    #[test]
    fn a_track_crossing_a_no_tracks_keepout_is_reported() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_tracks: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width: 100, a: Point { x: -500, y: 500 }, b: Point { x: 500, y: 500 } }];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].error_type, "items_not_allowed");
    }

    #[test]
    fn a_track_outside_the_keepout_is_clean() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_tracks: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width: 100, a: Point { x: 2000, y: 2000 }, b: Point { x: 3000, y: 2000 } }];
        assert!(check(&b, &BoardRules::default()).is_empty());
    }

    #[test]
    fn a_track_on_a_different_layer_than_the_keepout_is_not_reported() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_tracks: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "B.Cu".into(), width: 100, a: Point { x: 500, y: 500 }, b: Point { x: 600, y: 500 } }];
        assert!(check(&b, &BoardRules::default()).is_empty());
    }

    #[test]
    fn a_track_crossing_is_fine_when_no_tracks_is_not_set() {
        let mut b = empty_board();
        b.keepouts = vec![rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000)]; // every flag false
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width: 100, a: Point { x: -500, y: 500 }, b: Point { x: 500, y: 500 } }];
        assert!(check(&b, &BoardRules::default()).is_empty());
    }

    #[test]
    fn a_via_spanning_the_keepout_layer_is_reported_even_if_its_span_is_named_differently() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_vias: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.vias = vec![DrcVia { id: "v1".into(), net: Some("A".into()), at: Point { x: 500, y: 500 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn a_pad_on_the_keepout_layer_inside_it_is_reported() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_pads: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.pads = vec![DrcPad {
            id: "U1.1".into(),
            footprint_ref: "U1".into(),
            number: "1".into(),
            net: None,
            center: Point { x: 500, y: 500 },
            side: eda_model::ir::Side::Top,
            kind: PadKind::Smd,
            layers: vec!["F.Cu".into()],
            copper: Shape::Rect { x0: 400, y0: 400, x1: 600, y1: 600 },
            hole: None,
            drill_round: None,
            drill_slot: None,
        }];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn a_footprint_courtyard_overlapping_a_no_footprints_keepout_is_reported_regardless_of_keepout_layer() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_footprints: true, ..rect_keepout("k1", "F.SilkS", 0, 0, 1000, 1000) }];
        b.footprints = vec![crate::board::DrcFootprint { id: "U1".into(), side: eda_model::ir::Side::Top, courtyard: (500, 500, 1500, 1500) }];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn a_zone_the_filler_already_excluded_from_the_keepout_is_not_reported_again() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_copper_pour: true, ..rect_keepout("k1", "F.Cu", 200, 200, 800, 800) }];
        b.zones = vec![crate::board::DrcZone {
            id: "z1".into(),
            net: Some("GND".into()),
            layer: "F.Cu".into(),
            outline: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 1000 }, Point { x: 0, y: 1000 }],
            priority: 0,
            clearance: 50,
            min_thickness: 50,
            thermal_gap: 100,
            thermal_spoke_width: 100,
            pad_connection: eda_model::ir::PadConnection::Full,
            island_removal_mode: eda_model::ir::IslandRemovalMode::Never,
            min_island_area: 0,
        }];
        // The filler itself already excludes the keepout from this fill
        // (crates/zone-filler's own test covers that in isolation); this
        // asserts the DRC-side cross-check does NOT fire a false positive
        // once the filler has done its job.
        let v = check(&b, &BoardRules::default());
        assert!(v.is_empty(), "the filler already carved the keepout out of the fill, so nothing should be left overlapping it: {v:?}");
    }
}
