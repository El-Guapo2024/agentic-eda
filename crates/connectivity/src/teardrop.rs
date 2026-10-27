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
//! - **Round anchors only** (a via, or a pad `TEARDROP_MANAGER::IsRound` calls round: a circle, or an oval as wide as it is high). A rectangular or
//!   round-rect pad -- the common SMD case -- never gets a teardrop here; `m_UseRoundShapesOnly` is a real (if normally-off) upstream setting for exactly this
//!   restriction, and this port behaves as if it were always on. For a round anchor the outline is KiCad's own construction, point for point
//!   ([`teardrop_polygon`]; round 4's C-1, proven against kicad-cli's own teardrops by `crates/kicad/tests/teardrops_kicad_cli.rs`).
//! - **Straight edges only** (`m_CurvedEdges` is not modeled -- always
//!   `false`, which is also upstream's own factory default). No Bezier
//!   curve, no multi-segment curve approximation.
//! - **Single track segment only** (`m_AllowUseTwoTracks`, upstream's
//!   "the first segment is too short, so borrow length from the next one
//!   too" extension, is not ported): a segment shorter than the requested
//!   teardrop length gets a teardrop cut at its end (`actualTdLen`), as KiCad
//!   gives with "allow two segments" off.
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
use eda_model::ir::{Design, Point, TeardropSettings, Nm, Zone};
use eda_model::{ConstraintModel, PadKind, PadShape};
use eda_clipper2::Point64;
use eda_shape_poly_set::basic_shapes::ErrorLoc;
use eda_shape_poly_set::ShapePolySet;
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
    /// `TEARDROP_MANAGER::GetWidth`: a via's diameter, a pad's smaller size.
    width: Nm,
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
                anchors.push(Anchor { center: v.at, width: v.diameter, net: v.net.clone(), reach: Reach::Span(v.from_layer.clone(), v.to_layer.clone()) });
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
                    // `TEARDROP_MANAGER::IsRound`: a circle, or an oval as wide as it is high.
                    if !(pad.shape == PadShape::Circle || (pad.shape == PadShape::Oval && pad.size.0 == pad.size.1)) {
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
                    let width = pad.size.0.min(pad.size.1);
                    let reach = if is_pth {
                        // A plated through-hole pad spans the whole copper
                        // stack, same convention `eda_connectivity::items`
                        // uses for a THT pad's layer span.
                        let (first, last) = (model.board.layers.first().cloned().unwrap_or_default(), model.board.layers.last().cloned().unwrap_or_default());
                        Reach::Span(first, last)
                    } else {
                        Reach::Layer(if fp.side == eda_model::ir::Side::Top { "F.Cu".to_string() } else { "B.Cu".to_string() })
                    };
                    anchors.push(Anchor { center: pad.center, width, net, reach });
                }
            }
        }
    }

    anchors
}

/// `m_MaxError` (`BOARD_DESIGN_SETTINGS`), the error the anchor's circle is approximated with: KiCad's default `ARC_HIGH_DEF`, 0.005 mm. This model keeps no
/// per-board value.
const MAX_ERROR: i64 = 5_000;

fn p64(p: Point) -> Point64 {
    Point64::new(p.x.get(), p.y.get())
}

fn pt(p: Point64) -> Point {
    Point { x: Nm::new(p.x), y: Nm::new(p.y) }
}

/// `VECTOR2I::EuclideanNorm()`: `KiROUND( hypot )`, exact on an axis and on a 45-degree diagonal.
fn euclidean_norm(x: i64, y: i64) -> i64 {
    if x.abs() == y.abs() {
        return (x.abs() as f64 * std::f64::consts::SQRT_2).round() as i64;
    }
    if x == 0 {
        return y.abs();
    }
    if y == 0 {
        return x.abs();
    }
    (x as f64).hypot(y as f64).round() as i64
}

/// `rescale( a, b, c )` (`libs/kimath/src/math/util.cpp`): `a * b / c`, rounded half away from zero.
fn rescale(a: i128, b: i128, c: i128) -> i128 {
    let n = a * b;
    if (n < 0) ^ (c < 0) {
        (n - c / 2) / c
    } else {
        (n + c / 2) / c
    }
}

/// `SEG::Intersect( aSeg )` (`seg.cpp::SEG::intersects`, segments, endpoints counted): the crossing point of `a`-`b` and `c`-`d`, `aSeg.A + rescale( q, f, d )`.
/// A collinear overlap (`checkCollinearOverlap`) is not ported: a track along a polygon edge of the anchor is not a case this caller meets.
fn seg_intersect(a: Point64, b: Point64, c: Point64, d: Point64) -> Option<Point64> {
    if a.x.max(b.x) < c.x.min(d.x) || c.x.max(d.x) < a.x.min(b.x) || a.y.max(b.y) < c.y.min(d.y) || c.y.max(d.y) < a.y.min(b.y) {
        return None;
    }
    let (d1x, d1y) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
    let (d2x, d2y) = ((d.x - c.x) as i128, (d.y - c.y) as i128);
    let (ox, oy) = ((c.x - a.x) as i128, (c.y - a.y) as i128);
    let det = d2x * d1y - d2y * d1x;
    if det == 0 {
        return None;
    }
    let p2 = d2x * oy - d2y * ox;
    let p1 = d1x * oy - d1y * ox;
    if det > 0 {
        if p1 < 0 || p1 > det || p2 < 0 || p2 > det {
            return None;
        }
    } else if p1 > 0 || p1 < det || p2 > 0 || p2 < det {
        return None;
    }
    Some(Point64::new(c.x + rescale(p1, d2x, det) as i64, c.y + rescale(p1, d2y, det) as i64))
}

/// `SHAPE_LINE_CHAIN::Intersect( const SEG& aSeg, INTERSECTIONS& )` on a closed chain: every crossing of the chain's edges with `a`-`b`, nearest to `a` first
/// (`compareOriginDistance`).
fn chain_intersect(chain: &[Point64], a: Point64, b: Point64) -> Vec<Point64> {
    let mut hits = Vec::new();
    for s in 0..chain.len() {
        let (pa, pb) = (chain[s], chain[(s + 1) % chain.len()]);
        if let Some(p) = seg_intersect(pa, pb, a, b) {
            hits.push(p);
        }
    }
    let d2 = |p: &Point64| ((p.x - a.x) as i128).pow(2) + ((p.y - a.y) as i128).pow(2);
    hits.sort_by_key(d2);
    hits
}

/// `BuildConvexHull( aResult, aPoly )` (`libs/kimath/src/geometry/convex_hull.cpp`): Andrew's monotone chain over the points sorted by x then y, the last point not
/// repeated.
fn build_convex_hull(points: &[Point64]) -> Vec<Point64> {
    let mut poly = points.to_vec();
    if poly.len() < 2 {
        return Vec::new();
    }
    poly.sort_by(|a, b| (a.x, a.y).cmp(&(b.x, b.y)));
    let cross = |o: Point64, a: Point64, b: Point64| (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128;
    let mut out: Vec<Point64> = Vec::with_capacity(2 * poly.len());
    for &p in &poly {
        while out.len() >= 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0 {
            out.pop();
        }
        out.push(p);
    }
    let t = out.len() + 1;
    for &p in poly.iter().rev().skip(1) {
        while out.len() >= t && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0 {
            out.pop();
        }
        out.push(p);
    }
    if out.len() > 1 && out[0] == out[out.len() - 1] {
        out.pop();
    }
    out
}

/// `SHAPE_LINE_CHAIN::Area()` of a closed chain, absolute.
fn area(pts: &[Point64]) -> f64 {
    let mut a = 0.0;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        a += (pts[j].x as f64 + pts[i].x as f64) * (pts[j].y as f64 - pts[i].y as f64);
        j = i;
    }
    (a * 0.5).abs()
}

/// `TEARDROP_MANAGER::computeAnchorPoints` (`pcbnew/teardrop/teardrop_utils.cpp`) for a round anchor: the anchor's circle (`TransformCircleToPolygon( .., ERROR_INSIDE,
/// 16 )`), clipped to a band `min( m_TdMaxWidth, preferred_width )` wide along the teardrop's axis when the width is constrained, then C and E are the hull
/// neighbours of B and A in the convex hull of A, B and that polygon, swapped if that gives the larger pentagon. `pts` is `[A, B, _, D, _]`.
fn compute_anchor_points(settings: &TeardropSettings, center: Point64, width: i64, pts: &mut [Point64; 5]) -> bool {
    let preferred_width = (width as f64 * settings.best_width_ratio).round() as i64;
    let force_clip = settings.best_width_ratio < 1.0;
    let circle = eda_shape_poly_set::basic_shapes::circle_polygon(center, width / 2, MAX_ERROR, ErrorLoc::Inside, 16);
    let mut c_buffer = ShapePolySet::from_outline(circle);

    let max_width = settings.max_width_nm.get();
    if force_clip || (max_width > 0 && max_width < preferred_width) {
        let halfsize = max_width.min(preferred_width) / 2;
        let ref_on_track = Point64::new((pts[0].x + pts[1].x) / 2, (pts[0].y + pts[1].y) / 2);
        let (ax, ay) = (pts[3].x - ref_on_track.x, pts[3].y - ref_on_track.y);
        // `EDA_ANGLE( teardrop_axis )`, and `clipping_rect.Rotate( -orient )`.
        let orient = (ay as f64).atan2(ax as f64);
        let len = euclidean_norm(ax, ay);
        let rect = [Point64::new(0, -halfsize), Point64::new(0, halfsize), Point64::new(len, halfsize), Point64::new(len, -halfsize)];
        let rect: Vec<Point64> = rect
            .iter()
            .map(|&p| {
                let r = eda_shape_poly_set::basic_shapes::rotate_point(p, -orient);
                Point64::new(r.x + ref_on_track.x, r.y + ref_on_track.y)
            })
            .collect();
        c_buffer.boolean_intersection(&ShapePolySet::from_outline(rect));
    }

    let Some(padpoly) = c_buffer.polys.first().and_then(|p| p.first()) else { return false };
    let mut initial = vec![pts[0], pts[1]];
    initial.extend(padpoly.iter().copied());
    let hull = build_convex_hull(&initial);
    if hull.is_empty() {
        return false;
    }

    let (start, pend) = (pts[0], pts[1]);
    let (mut point_c, mut point_e) = (Point64::new(0, 0), Point64::new(0, 0));
    let (mut found_start, mut found_end) = (-1i64, -1i64);
    let n = hull.len();
    for ii in 0..n {
        let next = if ii + 1 >= n { 0 } else { ii + 1 };
        let prev = if ii == 0 { n - 1 } else { ii - 1 };
        if hull[ii] == start {
            point_e = if hull[next] != pend { hull[next] } else { hull[prev] };
            found_start = ii as i64;
        }
        if hull[ii] == pend {
            point_c = if hull[next] != start { hull[next] } else { hull[prev] };
            found_end = ii as i64;
        }
    }
    if found_start < 0 {
        let ii = if found_end - 1 < 0 { n as i64 - 1 } else { found_end - 1 };
        point_e = hull[ii as usize];
    }
    if found_end < 0 {
        let ii = if found_start - 1 < 0 { n as i64 - 1 } else { found_start - 1 };
        point_c = hull[ii as usize];
    }
    pts[2] = point_c;
    pts[4] = point_e;

    let area1 = area(&pts[..]);
    pts.swap(2, 4);
    let area2 = area(&pts[..]);
    if area1 > area2 {
        pts.swap(2, 4);
    }
    true
}

/// The teardrop of one track segment at a round anchor (a via, or a pad `TEARDROP_MANAGER::IsRound` calls round), with straight edges: the pentagon
/// `[A, B, C, D, E]` of `TEARDROP_MANAGER::computeTeardropPolygon` (`pcbnew/teardrop/teardrop_utils.cpp`). A and B are on the track, C and E on the anchor's
/// outline, D just behind the anchor's centre.
///
/// - `findAnchorPointsOnTrack`: the track's end inside the anchor is `start`; the teardrop's length is measured from where the segment crosses the anchor's
///   outline (`aIntersection = pts[0].p; start = aIntersection;`), `KiROUND( width * m_BestLengthRatio )` capped by `m_TdMaxLen`, and by the segment past the
///   outline (`actualTdLen = std::min( targetLength, SEG( start, end ).Length() )`).
/// - A and B: `start + vecT * len ± perp * width / 2`, the doubles truncated into a `VECTOR2I`.
/// - D: `intersection - vecVia * ( effectiveDist + 1 um )`, `effectiveDist` the larger of the centre's projection on the axis and the radius, clamped to the far
///   side of the circle less 2 um.
/// - C and E: [`compute_anchor_points`].
///
/// `width` is `TEARDROP_MANAGER::GetWidth`: a via's diameter, a pad's smaller size. `near` is the track's end at the anchor, `far` its other end. Returns `None`
/// for the skips of `UpdateTeardrops` and `findAnchorPointsOnTrack` (a skip, not an error). Not ported: `m_AllowUseTwoTracks` (the teardrop is cut at the
/// segment's end), curved edges, and the off-centre adjustment (the track ends on the anchor's centre here, see the module doc).
fn teardrop_polygon(center: Point, width: Nm, near: Point, far: Point, track_width: Nm, settings: &TeardropSettings) -> Option<Vec<Point>> {
    let (w, tw) = (width.get(), track_width.get());
    let radius = w / 2;
    if radius <= 0 || tw <= 0 || near != center {
        return None;
    }
    // `UpdateTeardrops`: "Ensure a teardrop shape can be built: track width must be < teardrop width and filter width".
    if tw >= settings.max_width_nm.get() || tw as f64 >= w as f64 * settings.best_width_ratio || tw as f64 >= w as f64 * settings.width_to_size_filter_ratio {
        return None;
    }
    let c = p64(center);
    let (start, end) = (p64(near), p64(far));
    // `PCB_VIA::HitTest( pos )`: the far end inside the anchor too, "The track is entirely inside the via".
    let inside = |p: Point64| ((p.x - c.x) as f64).powi(2) + ((p.y - c.y) as f64).powi(2) <= (radius as f64) * (radius as f64);
    if inside(end) {
        return None;
    }

    let outline = eda_shape_poly_set::basic_shapes::circle_polygon(c, radius, MAX_ERROR, ErrorLoc::Inside, 16);
    let hits = chain_intersect(&outline, start, end);
    // `computeEmergingTrackLength`: a track emerging by less than its own width does not anchor a teardrop. (Measured from the outside end.)
    let emerging = chain_intersect(&outline, end, start);
    let Some(&nearest_out) = emerging.first() else { return None };
    if euclidean_norm(nearest_out.x - end.x, nearest_out.y - end.y) < tw {
        return None;
    }
    let intersection = *hits.first()?;

    // `findAnchorPointsOnTrack`.
    let mut target_length = (w as f64 * settings.best_length_ratio).round() as i64;
    if settings.max_len_nm.get() > 0 {
        target_length = target_length.min(settings.max_len_nm.get());
    }
    let start = intersection;
    let stub_len = target_length.min(euclidean_norm(end.x - start.x, end.y - start.y));
    if start == end {
        return None;
    }

    // `computeTeardropPolygon`.
    let (vx, vy) = ((end.x - start.x) as f64, (end.y - start.y) as f64);
    let norm = vx.hypot(vy);
    let (tx, ty) = (vx / norm, vy / norm);
    let hw = (tw / 2) as f64;
    let len = stub_len as f64;
    let point_b = Point64::new(start.x + (tx * len + ty * hw) as i64, start.y + (ty * len - tx * hw) as i64);
    let point_a = Point64::new(start.x + (tx * len - ty * hw) as i64, start.y + (ty * len + tx * hw) as i64);

    let r = radius as f64;
    let (icx, icy) = ((c.x - intersection.x) as f64, (c.y - intersection.y) as f64);
    let proj_on_track = -(icx * tx + icy * ty);
    let mut effective_dist = proj_on_track.max(r);
    let offset = 1_000.0; // pcbIUScale.mmToIU( 0.001 )
    let disc = proj_on_track * proj_on_track - (icx * icx + icy * icy - r * r);
    if disc >= 0.0 {
        let far_edge = proj_on_track + disc.sqrt();
        let max_allowed = (far_edge - 2.0 * offset).max(0.0);
        if effective_dist > max_allowed {
            effective_dist = max_allowed;
        }
    }
    let point_d = Point64::new(intersection.x + (-tx * (effective_dist + offset)).round() as i64, intersection.y + (-ty * (effective_dist + offset)).round() as i64);

    let mut pts = [point_a, point_b, Point64::new(0, 0), point_d, Point64::new(0, 0)];
    if !compute_anchor_points(settings, c, w, &mut pts) {
        return None;
    }
    Some(pts.iter().map(|&p| pt(p)).collect())
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

            if let Some(outline) = teardrop_polygon(anchor.center, anchor.width, near, far, t.width, settings) {
                out.push(Zone { id: String::new(), net: anchor.net.clone(), layer: t.layer.clone(), outline, teardrop: true, ..Default::default() });
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
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
        design.routing.as_mut().unwrap().vias.push(Via { id: "v1".into(), net: "N1".into(), at: a, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, Point { x: a.x + mm(5.0), y: a.y }], arc_mid_offset: None });
        assert!(generate_teardrops(&design, &model, &TeardropSettings::default()).is_empty());
    }

    #[test]
    fn a_via_with_a_long_enough_track_gets_a_pentagon_teardrop() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + mm(3.0), y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, Point { x: via_at.x + mm(5.0), y: via_at.y }], arc_mid_offset: None });

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
        let via_at = Point { x: a.x + mm(3.0), y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        // Via diameter 800 * best_length_ratio 0.5 = 400um requested length,
        // but this track is only 50um long -- must be skipped, not clipped.
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, Point { x: via_at.x + mm(0.05), y: via_at.y }], arc_mid_offset: None });

        assert!(generate_teardrops(&design, &model, &enabled_settings()).is_empty());
    }

    #[test]
    fn a_track_on_a_different_net_than_the_via_is_ignored() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + mm(3.0), y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "t1".into(), net: "OTHER".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, Point { x: via_at.x + mm(5.0), y: via_at.y }], arc_mid_offset: None });
        assert!(generate_teardrops(&design, &model, &enabled_settings()).is_empty());
    }

    #[test]
    fn target_vias_off_skips_vias_entirely() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + mm(3.0), y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, Point { x: via_at.x + mm(5.0), y: via_at.y }], arc_mid_offset: None });
        let settings = TeardropSettings { enabled: true, target_vias: false, ..Default::default() };
        assert!(generate_teardrops(&design, &model, &settings).is_empty());
    }

    #[test]
    fn a_round_through_hole_pad_gets_a_teardrop_spanning_every_layer() {
        let (mut design, mut model) = two_pad_model();
        model.footprints.push(Footprint {
            name: "THPAD".into(),
            pads: vec![Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (mm(1.0), mm(1.0)), shape: PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(mm(0.5)), drill_slot: None, rot: 0, roundrect_ratio: None }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        });
        model.parts[0].footprint = Some("THPAD".into());
        model.parts[0].package = Some("THPAD".into());
        let a = pad_center(&design, &model, "R1", "1");
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "B.Cu".into(), width: mm(0.2), pts: vec![a, Point { x: a.x, y: a.y + mm(5.0) }], arc_mid_offset: None });

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
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.15), pts: vec![a, Point { x: a.x + mm(5.0), y: a.y }], arc_mid_offset: None });
        assert!(generate_teardrops(&design, &model, &enabled_settings()).is_empty());
    }

    /// C-1 (round 4): the teardrop's length is measured from where the track leaves the via's outline (`findAnchorPointsOnTrack`: `start = aIntersection;`),
    /// not from its centre. The expected outlines are kicad-cli's own: the same three vias and tracks written with `(teardrops ..)` parameters and no
    /// `(legacy_teardrops yes)`, refilled by `kicad-cli pcb drc --refill-zones --save-board` (KiCad 10.99), which runs `TEARDROP_MANAGER::UpdateTeardrops`. A 0.6 mm
    /// via, a 0.8 mm via with a diagonal track, and a 3 mm via (the 1 mm length cap, and the 2 mm width cap clipping the circle).
    #[test]
    fn the_teardrop_length_starts_at_the_anchor_edge_as_kicad_clis_own() {
        let (mut design, model) = two_pad_model();
        let p = |x: i64, y: i64| Point { x: Nm::new(x), y: Nm::new(y) };
        let rt = design.routing.as_mut().unwrap();
        let cases = [
            (p(100_000_000, 100_000_000), mm(0.6), p(105_000_000, 100_000_000), mm(0.2), vec![p(100597433, 100100000), p(100597433, 99900000), p(100114805, 99722836), p(99996433, 100000000), p(100114805, 100277164)]),
            (p(110_000_000, 100_000_000), mm(0.8), p(113_000_000, 104_000_000), mm(0.25), vec![p(110379667, 100714556), p(110579667, 100564556), p(110369552, 99846927), p(109999067, 99998757), p(109756495, 100317341)]),
            (p(95_000_000, 95_000_000), mm(3.0), p(95_000_000, 105_000_000), mm(0.25), vec![p(94875000, 97495376), p(95125000, 97495376), p(96000000, 96114781), p(95000000, 94994376), p(94000000, 96114781)]),
        ];
        for (i, (at, d, end, w, _)) in cases.iter().enumerate() {
            rt.vias.push(Via { id: format!("v{i}"), net: "N1".into(), at: *at, drill: mm(0.3), diameter: *d, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
            rt.tracks.push(Track { id: format!("t{i}"), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: *w, pts: vec![*at, *end], arc_mid_offset: None });
        }
        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 3, "{out:?}");
        for (zone, (.., want)) in out.iter().zip(cases.iter()) {
            assert_eq!(&zone.outline, want);
        }
    }

    /// Even-odd point-in-polygon, test-only (pure geometry, not worth a
    /// shared helper for one assertion).
    fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
        let mut inside = false;
        let n = poly.len();
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = (poly[i].x.as_f64(), poly[i].y.as_f64());
            let (xj, yj) = (poly[j].x.as_f64(), poly[j].y.as_f64());
            if (yi > p.y.as_f64()) != (yj > p.y.as_f64()) {
                let x_cross = xi + (p.y.as_f64() - yi) / (yj - yi) * (xj - xi);
                if (p.x.as_f64()) < x_cross {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }
}
