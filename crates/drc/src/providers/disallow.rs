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

/// One KiCad rule area: the IR splits a multi-layer `ZONE` into one
/// single-layer keepout per layer, so those are folded back into the
/// zone's `GetLayerSet()` here -- KiCad reports an item once per rule
/// area, not once per layer of it.
struct RuleArea<'a> {
    id: &'a str,
    layers: Vec<&'a str>,
    outline: &'a [Point],
    k: &'a DrcKeepout,
}

impl RuleArea<'_> {
    fn on(&self, layer: &str) -> bool {
        self.layers.contains(&layer)
    }
}

fn rule_areas(board: &DrcBoard) -> Vec<RuleArea<'_>> {
    let mut out: Vec<RuleArea> = Vec::new();
    for k in &board.keepouts {
        let flags = (k.no_tracks, k.no_vias, k.no_pads, k.no_copper_pour, k.no_footprints, &k.parent_footprint);
        if let Some(a) = out.iter_mut().find(|a| a.outline == k.outline.as_slice() && (a.k.no_tracks, a.k.no_vias, a.k.no_pads, a.k.no_copper_pour, a.k.no_footprints, &a.k.parent_footprint) == flags && !a.on(&k.layer)) {
            a.layers.push(&k.layer);
        } else {
            out.push(RuleArea { id: &k.id, layers: vec![&k.layer], outline: &k.outline, k });
        }
    }
    out
}

/// The copper layers a via spans (`PCB_VIA::GetLayerSet`).
fn via_layers<'a>(from: &str, to: &str, layers: &'a [String]) -> Vec<&'a str> {
    layers.iter().filter(|l| via_spans_layer(from, to, l, layers)).map(String::as_str).collect()
}

fn is_front(l: &str) -> bool {
    l.starts_with("F.")
}

fn is_back(l: &str) -> bool {
    l.starts_with("B.")
}

/// `DRC_TEST_PROVIDER_DISALLOW::Run`'s keepout half. Tracks are collided
/// against every no-tracks rule area on their layer (`antiTrackKeepouts`,
/// one marker per crossing area); every other item goes through
/// `EvalRules( DISALLOW_CONSTRAINT, item )`, which resolves to a single
/// constraint, so it is reported at most once however many rule areas it
/// intersects. Membership is `intersectsArea` (`pcbexpr_functions.cpp`):
/// common layers with the area, then `collidesWithArea` per layer against
/// the area outline deflated by the DRC epsilon.
pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    if board.keepouts.is_empty() {
        return out;
    }
    let areas = rule_areas(board);
    let deflated: Vec<Option<Shape>> = areas
        .iter()
        .map(|a| {
            let d = deflated_keepout_outline(a.outline);
            (d.len() >= 3).then_some(Shape::Polygon { pts: d })
        })
        .collect();

    // antiTrackKeepouts->QueryColliding( track, layer, layer, ... ) against the undeflated outline.
    for t in &board.tracks {
        for a in areas.iter().filter(|a| a.k.no_tracks && a.on(&t.layer)) {
            if (Shape::Polygon { pts: a.outline.to_vec() }).collides(&t.shape(), 0).is_some() {
                out.push(violation("Track", a.id, format!("Track on {}", t.layer), t.a, t.id.clone()));
            }
        }
    }

    // The first rule area that disallows the item and intersects it.
    let first_hit = |allowed: &dyn Fn(&DrcKeepout) -> bool, item_layers: &[&str], collides: &dyn Fn(usize, &Shape, &[&str]) -> bool| -> Option<usize> {
        areas.iter().enumerate().find_map(|(i, a)| {
            if !allowed(a.k) {
                return None;
            }
            let common: Vec<&str> = item_layers.iter().copied().filter(|l| a.on(l)).collect();
            if common.is_empty() {
                return None;
            }
            let shape = deflated[i].as_ref()?;
            collides(i, shape, &common).then_some(i)
        })
    };

    for v in &board.vias {
        let layers = via_layers(&v.from_layer, &v.to_layer, &board.layers);
        let vshape = v.shape();
        if let Some(i) = first_hit(&|k| k.no_vias, &layers, &|_, s, _| s.collides(&vshape, 0).is_some()) {
            out.push(violation("Via", areas[i].id, "Via".into(), v.at, v.id.clone()));
        }
    }

    for p in &board.pads {
        let layers: Vec<&str> = p.layers.iter().map(String::as_str).collect();
        if let Some(i) = first_hit(&|k| k.no_pads, &layers, &|_, s, _| s.collides(&p.copper, 0).is_some()) {
            out.push(violation("Pad", areas[i].id, format!("Pad {} of {}", p.number, p.footprint_ref), p.center, p.id.clone()));
        }
    }

    for f in &board.footprints {
        // FOOTPRINT::GetLayerSet() is its own layer; the courtyard tested
        // depends on which side(s) the area covers.
        let own = if f.side == eda_model::ir::Side::Bottom { "B.Cu" } else { "F.Cu" };
        let court: Vec<Shape> = if f.outlines.is_empty() {
            let (x0, y0, x1, y1) = f.courtyard;
            vec![Shape::Rect { x0, y0, x1, y1 }]
        } else {
            f.outlines.iter().map(|o| Shape::Polygon { pts: o.clone() }).collect()
        };
        let collides = |i: usize, s: &Shape, _: &[&str]| {
            let a = &areas[i];
            let side_ok = if own == "B.Cu" { a.layers.iter().any(|l| is_back(l)) } else { a.layers.iter().any(|l| is_front(l)) };
            side_ok && court.first().is_some_and(|c| s.collides(c, 0).is_some())
        };
        if let Some(i) = first_hit(&|k| k.no_footprints && k.parent_footprint.as_deref() != Some(f.id.as_str()), &[own], &collides) {
            let (x0, y0, x1, y1) = f.courtyard;
            let center = Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 };
            out.push(violation("Footprint", areas[i].id, format!("Footprint {}", f.id), center, f.id.clone()));
        }
    }

    // Copper zones (and teardrops, tested as tracks) against the filled
    // copper on each common layer; a multi-layer zone is one KiCad item.
    let wants_zones = areas.iter().any(|a| a.k.no_copper_pour || a.k.no_tracks);
    if wants_zones {
        let fills = fill_all_zones(board, rules);
        let mut groups: Vec<(&crate::board::DrcZone, Vec<&str>)> = Vec::new();
        for z in &board.zones {
            if let Some(g) = groups.iter_mut().find(|(g, _)| g.outline == z.outline && g.net == z.net && g.priority == z.priority && g.teardrop == z.teardrop) {
                g.1.push(&z.layer);
            } else {
                groups.push((z, vec![&z.layer]));
            }
        }
        for (z, layers) in &groups {
            let teardrop = z.teardrop;
            let allowed = |k: &DrcKeepout| if teardrop { k.no_tracks } else { k.no_copper_pour };
            let collides = |_: usize, s: &Shape, common: &[&str]| {
                board.zones.iter().filter(|zz| zz.outline == z.outline && zz.net == z.net && zz.priority == z.priority && zz.teardrop == z.teardrop && common.contains(&zz.layer.as_str())).any(|zz| {
                    let Some(fill) = fills.zones.get(&zz.id) else { return false };
                    fill.fill.polys.iter().any(|poly| {
                        let Some(o) = poly.first() else { return false };
                        let pts: Vec<Point> = o.iter().map(|p| Point { x: p.x, y: p.y }).collect();
                        pts.len() >= 3 && s.collides(&Shape::Polygon { pts }, 0).is_some()
                    })
                })
            };
            if let Some(i) = first_hit(&allowed, layers, &collides) {
                let kind = if teardrop { "Track" } else { "Copper pour" };
                out.push(violation(kind, areas[i].id, format!("Zone on {}", z.layer), areas[i].outline.first().copied().unwrap_or(Point { x: 0, y: 0 }), z.id.clone()));
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
        DrcKeepout { id: id.into(), layer: layer.into(), outline: vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }], no_tracks: false, no_vias: false, no_pads: false, no_copper_pour: false, no_footprints: false, parent_footprint: None }
    }

    #[test]
    fn a_track_crossing_a_no_tracks_keepout_is_reported() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_tracks: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width: 100, a: Point { x: -500, y: 500 }, b: Point { x: 500, y: 500 }, arc_mid: None }];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].error_type, "items_not_allowed");
    }

    #[test]
    fn a_track_outside_the_keepout_is_clean() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_tracks: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width: 100, a: Point { x: 2000, y: 2000 }, b: Point { x: 3000, y: 2000 }, arc_mid: None }];
        assert!(check(&b, &BoardRules::default()).is_empty());
    }

    #[test]
    fn a_track_on_a_different_layer_than_the_keepout_is_not_reported() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_tracks: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "B.Cu".into(), width: 100, a: Point { x: 500, y: 500 }, b: Point { x: 600, y: 500 }, arc_mid: None }];
        assert!(check(&b, &BoardRules::default()).is_empty());
    }

    #[test]
    fn a_track_crossing_is_fine_when_no_tracks_is_not_set() {
        let mut b = empty_board();
        b.keepouts = vec![rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000)]; // every flag false
        b.tracks = vec![DrcTrackSeg { id: "t1".into(), net: Some("A".into()), layer: "F.Cu".into(), width: 100, a: Point { x: -500, y: 500 }, b: Point { x: 500, y: 500 }, arc_mid: None }];
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
    fn a_footprint_courtyard_overlapping_a_no_footprints_keepout_on_its_side_is_reported() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_footprints: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.footprints = vec![crate::board::DrcFootprint { id: "U1".into(), side: eda_model::ir::Side::Top, courtyard: (500, 500, 1500, 1500), outlines: vec![] }];
        let v = check(&b, &BoardRules::default());
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn a_footprint_is_reported_once_for_a_rule_area_on_several_layers() {
        let mut b = empty_board();
        b.keepouts = vec![
            DrcKeepout { no_footprints: true, no_pads: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) },
            DrcKeepout { no_footprints: true, no_pads: true, ..rect_keepout("k2", "B.Cu", 0, 0, 1000, 1000) },
        ];
        b.footprints = vec![crate::board::DrcFootprint { id: "U1".into(), side: eda_model::ir::Side::Top, courtyard: (500, 500, 1500, 1500), outlines: vec![] }];
        assert_eq!(check(&b, &BoardRules::default()).len(), 1);
    }

    #[test]
    fn a_back_footprint_ignores_a_front_only_rule_area() {
        let mut b = empty_board();
        b.keepouts = vec![DrcKeepout { no_footprints: true, ..rect_keepout("k1", "F.Cu", 0, 0, 1000, 1000) }];
        b.footprints = vec![crate::board::DrcFootprint { id: "U1".into(), side: eda_model::ir::Side::Bottom, courtyard: (500, 500, 1500, 1500), outlines: vec![] }];
        assert!(check(&b, &BoardRules::default()).is_empty());
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
            teardrop: false,
        }];
        // The filler itself already excludes the keepout from this fill
        // (crates/zone-filler's own test covers that in isolation); this
        // asserts the DRC-side cross-check does NOT fire a false positive
        // once the filler has done its job.
        let v = check(&b, &BoardRules::default());
        assert!(v.is_empty(), "the filler already carved the keepout out of the fill, so nothing should be left overlapping it: {v:?}");
    }
}
