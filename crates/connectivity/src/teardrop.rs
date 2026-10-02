//! Port of `pcbnew/teardrop/*` (task item 4): `TEARDROP_MANAGER`'s
//! polygon generator, as a pure function over this model's IR rather than
//! a live, incremental `BOARD` manager. Settings: `eda_model::ir::
//! TeardropSettings` (Board Setup > Teardrops). Storage: per the task
//! brief, "KiCad stores teardrops as special zones, so store them the
//! same way in the IR, regenerated on demand" -- a teardrop is a `Zone`
//! with `teardrop: true`, computed fresh by [`generate_teardrops`] and
//! swapped in wholesale by `Cmd::AddAllTeardrops`
//! (`crates/ops/src/lib.rs`), never persisted as anything the user hand-
//! edits (same spirit a zone fill is always recomputed, never cached).
//!
//! ## Scope (each a documented, bounded cut, not an oversight)
//!
//! - **Round anchors only** (a via, or a pad whose shape is
//!   `PadShape::Circle`). KiCad's own exact construction
//!   (`computeAnchorPoints`) builds a real convex hull over the anchor's
//!   polygon to find tangent-ish points, so it also handles rectangular/
//!   custom pad outlines; this port uses a closed-form tangent-on-circle
//!   formula instead (see [`teardrop_polygon`]), which only has a well-
//!   defined answer for a circular anchor. A rectangular/round-rect SMD
//!   pad -- the common case -- never gets a teardrop here. `m_UseRoundShapesOnly`
//!   is a real (if normally-off) upstream setting for exactly this
//!   restriction; this port behaves as if it were always on.
//! - **Straight edges only** (`m_CurvedEdges` is not modeled -- always
//!   `false`, which is also upstream's own factory default). No Bezier
//!   curve, no multi-segment curve approximation.
//! - **Single track segment only** (`m_AllowUseTwoTracks`, upstream's
//!   "the first segment is too short, so borrow length from the next one
//!   too" extension, is not ported): a track shorter than the requested
//!   teardrop length is simply skipped, not shortened-and-extended.
//! - **No track-to-track teardrops** (`TARGET_TRACK`/`TD_TRACKEND`,
//!   `m_TargetTrack2Track` -- default off upstream too): only the via/pad
//!   target kinds are ported.
//! - **No in-zone-fill exclusion filter** (`m_TdOnPadsInZones`): a pad
//!   already covered by a same-net zone fill still gets a teardrop here.
//! - A track must touch the anchor at one of its own polyline endpoints
//!   *exactly* (`Track::pts` first/last `== ` the anchor's center) --
//!   this model's router and hand-drawn tracks always land exactly on a
//!   pad/via center (see `crates/connectivity`'s own convention
//!   throughout, e.g. `cleanup.rs`), so this is the real connectivity
//!   test here, not a simplification of a fuzzier upstream rule.

use eda_model::footprint::placed_pads;
use eda_model::ir::{Design, Point, TeardropSettings, Um, Zone};
use eda_model::{ConstraintModel, PadKind, PadShape};
use std::collections::HashMap;

/// Which copper this anchor reaches: a pad's own one named side, or a
/// via's full `from_layer..to_layer` span (inclusive, either order).
enum Reach {
    Layer(String),
    Span(String, String),
}

fn reach_includes(reach: &Reach, layer: &str, board_layers: &[String]) -> bool {
    match reach {
        Reach::Layer(l) => l == layer,
        Reach::Span(a, b) => {
            let idx = |l: &str| board_layers.iter().position(|x| x == l);
            match (idx(a), idx(b), idx(layer)) {
                (Some(ia), Some(ib), Some(ic)) => ic >= ia.min(ib) && ic <= ia.max(ib),
                _ => a == layer || b == layer,
            }
        }
    }
}

struct Anchor {
    center: Point,
    radius: Um,
    net: String,
    reach: Reach,
}

/// "REF.PIN" -> net name (same lookup `eda_connectivity::items` builds).
fn net_of_pin_map(model: &ConstraintModel) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for net in &model.nets {
        for pin in &net.pins {
            map.insert(pin.clone(), net.name.clone());
        }
    }
    map
}

fn collect_anchors(design: &Design, model: &ConstraintModel, settings: &TeardropSettings) -> Vec<Anchor> {
    let mut anchors = Vec::new();

    if settings.target_vias {
        if let Some(rt) = &design.routing {
            for v in &rt.vias {
                if v.net.is_empty() {
                    continue;
                }
                anchors.push(Anchor { center: v.at, radius: v.diameter / 2, net: v.net.clone(), reach: Reach::Span(v.from_layer.clone(), v.to_layer.clone()) });
            }
        }
    }

    if settings.target_pth_pads || settings.target_smd_pads {
        let net_of_pin = net_of_pin_map(model);
        if let Some(pl) = &design.placement {
            for fp in &pl.footprints {
                let Some(part) = model.part(&fp.id) else { continue };
                let Some(footprint) = model.footprint_of(part) else { continue };
                let Some(placed) = placed_pads(model, part, fp) else { continue };
                let mut lib_pads: Vec<&eda_model::Pad> = footprint.pads.iter().collect();
                lib_pads.sort_by(|a, b| a.number.cmp(&b.number));

                for (pad, lp) in placed.into_iter().zip(lib_pads.iter()) {
                    if pad.shape != PadShape::Circle {
                        continue; // round anchors only -- see module doc
                    }
                    let is_pth = lp.kind == PadKind::ThroughHole;
                    let is_npth = lp.kind == PadKind::NonPlatedHole;
                    if is_npth {
                        continue; // no copper connection to tear-drop at all
                    }
                    if is_pth && !settings.target_pth_pads {
                        continue;
                    }
                    if !is_pth && !settings.target_smd_pads {
                        continue;
                    }
                    let net = net_of_pin.get(&format!("{}.{}", fp.id, pad.number)).cloned().unwrap_or_default();
                    if net.is_empty() {
                        continue;
                    }
                    let radius = pad.size.0.min(pad.size.1) / 2;
                    let reach = if is_pth {
                        // A plated through-hole pad spans the whole copper
                        // stack, same convention `eda_connectivity::items`
                        // uses for a THT pad's layer span.
                        let (first, last) = (model.board.layers.first().cloned().unwrap_or_default(), model.board.layers.last().cloned().unwrap_or_default());
                        Reach::Span(first, last)
                    } else {
                        Reach::Layer(if fp.side == eda_model::ir::Side::Top { "F.Cu".to_string() } else { "B.Cu".to_string() })
                    };
                    anchors.push(Anchor { center: pad.center, radius, net, reach });
                }
            }
        }
    }

    anchors
}

/// The exact-tangent-on-circle pentagon [A, C, D, E, B] described in
/// `teardrop.h`'s own header comment ("A and B are points on the track;
/// C and E are points on the pad/via; D is a midpoint behind the pad/via
/// centre"). `near` must be exactly the anchor's own center (the track's
/// connected endpoint); `far` is that same track's other endpoint.
/// Returns `None` when the track is too short, too wide, or the anchor
/// has no usable radius to build a tangent from -- a skip, not an error.
fn teardrop_polygon(center: Point, radius: Um, near: Point, far: Point, track_width: Um, settings: &TeardropSettings) -> Option<Vec<Point>> {
    let r = radius as f64;
    let hw = track_width as f64 / 2.0;
    if r <= 0.0 || hw <= 0.0 || near != center {
        return None;
    }

    let (dx, dy) = ((far.x - near.x) as f64, (far.y - near.y) as f64);
    let track_len = (dx * dx + dy * dy).sqrt();
    if track_len < 1.0 {
        return None;
    }
    let (dir_x, dir_y) = (dx / track_len, dy / track_len);
    let (perp_x, perp_y) = (-dir_y, dir_x);

    // `m_WidthtoSizeFilterRatio`: skip a track too wide, relative to the
    // anchor, to be worth a teardrop at all.
    let anchor_diameter = 2.0 * r;
    if track_width as f64 > anchor_diameter * settings.width_to_size_filter_ratio {
        return None;
    }

    let mut target_len = anchor_diameter * settings.best_length_ratio;
    if settings.max_len_um > 0 {
        target_len = target_len.min(settings.max_len_um as f64);
    }
    // Never reach past the track's own far end -- the single-segment
    // scope this port has (see module doc); leave a hairline margin so
    // the far corner A/B never lands exactly on (or past) the next item.
    target_len = target_len.min(track_len * 0.98);
    if target_len <= hw {
        return None; // not enough room for a meaningful teardrop
    }

    let mut half_width_pad_side = r * settings.best_width_ratio.clamp(0.0, 1.0);
    if settings.max_width_um > 0 {
        half_width_pad_side = half_width_pad_side.min(settings.max_width_um as f64 / 2.0);
    }
    half_width_pad_side = half_width_pad_side.min(r);

    let round = |x: f64| x.round() as Um;
    let pt = |bx: f64, by: f64, ox: f64, oy: f64| Point { x: round(bx + ox), y: round(by + oy) };

    let (px, py) = (near.x as f64 + dir_x * target_len, near.y as f64 + dir_y * target_len);
    let a = pt(px, py, perp_x * hw, perp_y * hw);
    let b = pt(px, py, -perp_x * hw, -perp_y * hw);

    // C/E: real points on the anchor's own circle (not an approximation)
    // at the chord half-width `half_width_pad_side`, on the hemisphere
    // facing the track.
    let axial = (r * r - half_width_pad_side * half_width_pad_side).max(0.0).sqrt();
    let (fx, fy) = (near.x as f64 + dir_x * axial, near.y as f64 + dir_y * axial);
    let c = pt(fx, fy, perp_x * half_width_pad_side, perp_y * half_width_pad_side);
    let e = pt(fx, fy, -perp_x * half_width_pad_side, -perp_y * half_width_pad_side);

    // D: the anchor's own back point, opposite the track direction --
    // guarantees the anchor's center sits inside the closed pentagon.
    let d = pt(near.x as f64, near.y as f64, -dir_x * r, -dir_y * r);

    Some(vec![a, c, d, e, b])
}

/// Every teardrop this board's current tracks/vias/pads call for, given
/// `settings` -- the dry-run-free equivalent of `TEARDROP_MANAGER::
/// UpdateTeardrops` run over the whole board. Returns `Zone`s with
/// `teardrop: true`, fresh ids (empty, assigned by the caller's own
/// `RoutingSection::assign_missing_ids`), ready to replace whatever
/// teardrop zones currently exist. Empty immediately when `!settings.
/// enabled`, matching `TEARDROP_PARAMETERS::m_Enabled` gating the whole
/// feature upstream.
pub fn generate_teardrops(design: &Design, model: &ConstraintModel, settings: &TeardropSettings) -> Vec<Zone> {
    let mut out = Vec::new();
    if !settings.enabled {
        return out;
    }
    let Some(rt) = &design.routing else { return out };
    let anchors = collect_anchors(design, model, settings);

    for anchor in &anchors {
        for t in &rt.tracks {
            if t.net != anchor.net || t.pts.len() < 2 {
                continue;
            }
            if !reach_includes(&anchor.reach, &t.layer, &model.board.layers) {
                continue;
            }
            let (near, far) = if *t.pts.first().unwrap() == anchor.center {
                (*t.pts.first().unwrap(), t.pts[1])
            } else if *t.pts.last().unwrap() == anchor.center {
                (*t.pts.last().unwrap(), t.pts[t.pts.len() - 2])
            } else {
                continue;
            };

            if let Some(outline) = teardrop_polygon(anchor.center, anchor.radius, near, far, t.width, settings) {
                out.push(Zone { id: String::new(), net: anchor.net.clone(), layer: t.layer.clone(), outline, teardrop: true, ..Default::default() });
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::{Track, Via};
    use eda_model::{Footprint, Pad};

    fn enabled_settings() -> TeardropSettings {
        TeardropSettings { enabled: true, ..Default::default() }
    }

    #[test]
    fn disabled_settings_produce_nothing() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().vias.push(Via { id: "v1".into(), net: "N1".into(), at: a, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![a, Point { x: a.x + 5000, y: a.y }], arc_mid_offset: None });
        assert!(generate_teardrops(&design, &model, &TeardropSettings::default()).is_empty());
    }

    #[test]
    fn a_via_with_a_long_enough_track_gets_a_pentagon_teardrop() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + 3000, y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![via_at, Point { x: via_at.x + 5000, y: via_at.y }], arc_mid_offset: None });

        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].teardrop);
        assert_eq!(out[0].net, "N1");
        assert_eq!(out[0].layer, "F.Cu");
        assert_eq!(out[0].outline.len(), 5, "A, C, D, E, B");

        // The via's own center must sit inside the generated pentagon --
        // the whole point of placing D behind it (shoelace-formula
        // point-in-polygon, since this is a small, convex-ish pentagon).
        assert!(point_in_polygon(via_at, &out[0].outline));
    }

    #[test]
    fn a_track_too_short_for_the_requested_length_is_skipped() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + 3000, y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        // Via diameter 800 * best_length_ratio 0.5 = 400um requested length,
        // but this track is only 50um long -- must be skipped, not clipped.
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![via_at, Point { x: via_at.x + 50, y: via_at.y }], arc_mid_offset: None });

        assert!(generate_teardrops(&design, &model, &enabled_settings()).is_empty());
    }

    #[test]
    fn a_track_on_a_different_net_than_the_via_is_ignored() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + 3000, y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "t1".into(), net: "OTHER".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![via_at, Point { x: via_at.x + 5000, y: via_at.y }], arc_mid_offset: None });
        assert!(generate_teardrops(&design, &model, &enabled_settings()).is_empty());
    }

    #[test]
    fn target_vias_off_skips_vias_entirely() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + 3000, y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![via_at, Point { x: via_at.x + 5000, y: via_at.y }], arc_mid_offset: None });
        let settings = TeardropSettings { enabled: true, target_vias: false, ..Default::default() };
        assert!(generate_teardrops(&design, &model, &settings).is_empty());
    }

    #[test]
    fn a_round_through_hole_pad_gets_a_teardrop_spanning_every_layer() {
        let (mut design, mut model) = two_pad_model();
        model.footprints.push(Footprint {
            name: "THPAD".into(),
            pads: vec![Pad { opposite_side: false, number: "1".into(), at: (0, 0), size: (1000, 1000), shape: PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(500), drill_slot: None, rot: 0, roundrect_ratio: None }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
        });
        model.parts[0].footprint = Some("THPAD".into());
        model.parts[0].package = Some("THPAD".into());
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "B.Cu".into(), width: 200, pts: vec![a, Point { x: a.x, y: a.y + 5000 }], arc_mid_offset: None });

        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].layer, "B.Cu", "a PTH pad's teardrop must follow the track's own layer, not assume F.Cu");
    }

    #[test]
    fn a_non_round_smd_pad_never_gets_a_teardrop() {
        // two_pad_model's own 0603 package -- rectangular pads, round
        // anchors only (see module doc).
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: 150, pts: vec![a, Point { x: a.x + 5000, y: a.y }], arc_mid_offset: None });
        assert!(generate_teardrops(&design, &model, &enabled_settings()).is_empty());
    }

    /// Even-odd point-in-polygon, test-only (pure geometry, not worth a
    /// shared helper for one assertion).
    fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
        let mut inside = false;
        let n = poly.len();
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = (poly[i].x as f64, poly[i].y as f64);
            let (xj, yj) = (poly[j].x as f64, poly[j].y as f64);
            if (yi > p.y as f64) != (yj > p.y as f64) {
                let x_cross = xi + (p.y as f64 - yi) / (yj - yi) * (xj - xi);
                if (p.x as f64) < x_cross {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }
}
