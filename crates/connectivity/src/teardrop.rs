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
//! - **Vias and every pad shape but a custom one** (round 7, UI-LEFT P1). A via and a pad `TEARDROP_MANAGER::IsRound` calls round (a circle, or an oval as wide
//!   as it is high) get KiCad's construction for a round anchor, point for point ([`compute_teardrop_polygon`]; round 4's C-1). A rectangle, a rounded or
//!   chamfered rectangle, a trapezoid and a long oval -- the common SMD pads -- get the construction for the others, as `m_UseRoundShapesOnly` is off by
//!   default (`TEARDROP_PARAMETERS_LIST()`): the pad's own polygon (`PAD::TransformShapeToPolygon`, [`pad_polygon`]) clipped to the teardrop's width
//!   (`computeAnchorPoints`' `force_clip`), A and B refused inside the pad, D kept inside the copper the track enters, no symmetric re-fit. Both are proven
//!   against kicad-cli's own teardrops by `crates/kicad/tests/teardrops_kicad_cli.rs`. Not ported: a custom pad (`MergePrimitivesAsPolygon`; skipped), a
//!   padstack's other layers (the pad's main shape stands for every layer).
//! - **The anchors a track meets, found as `UpdateTeardrops` finds them** (25ec2335; round 7 R7-1). Each straight segment of a track's polyline is a
//!   `PCB_TRACK` of its own (the writer writes each as a `(segment ..)`), and each of its ends is hit-tested against the anchor (`PCB_VIA::HitTest`: the circle;
//!   `PAD::HitTest`: the pad's polygon), not compared with its centre: an end inside the anchor but off its centre gets its teardrop, and a track crossing the
//!   anchor's centre (`PCB_TRACK::HitTest( pos )`) gets one on each side (`buildCrossingStub`). A track that touches the anchor with neither end in it nor
//!   over its centre goes through `findAnchorPointsOnTrack` as it does in KiCad (a teardrop where its centreline crosses the outline). The same-net test stands
//!   for KiCad's connectivity (`GetConnectedPadsAndVias`, whose items' copper collides).
//! - **Arc tracks** (`PCB_ARC_T`, round 5's A5-1) follow the real arc: the crossing with its polyline, the reference segment found on it, and 25ec2335's
//!   two refinements (the polyline's points moved onto the arc, the corners kept `maxError` inside an arc's edge), as the kicad-cli 25ec2335 this is proven
//!   against does (`crates/kicad/tests/teardrops_kicad_cli.rs` has arc cases both ways round).
//! - **One KiCad version: 25ec2335** (round 6 D6-1), the kicad-cli the proof runs against, for every rule: `UpdateTeardrops`' filters (a Max width of 0 is no
//!   limit; `computeChordThroughShape`'s graze test on the extended centreline, arcs exempt, in place of 8303b2ad's `computeEmergingTrackLength`) as well as
//!   the arc geometry. Not ported from 25ec2335: `computeFittedTeardropPolygon` (a teardrop shrunk or dropped where it would come within clearance of
//!   another net's copper, `collidesWithOtherNets`) and the solder-mask teardrops (`createAndAddTeardropWithMask`).
//! - **One parameter block for every anchor**: Board Setup's ([`TeardropSettings`]). KiCad reads each via's and pad's own `TEARDROP_PARAMETERS`
//!   (`via->GetTeardropParams()`, `pad->GetTeardropParams()`, which Board Setup's round and rect blocks only seed, `DIALOG_GLOBAL_EDIT_TEARDROPS::processItem`);
//!   this model keeps an item's `(teardrops ..)` only as the file's tokens (`ViaExtra::tokens`, a pad's `ext.tokens`), unread.
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

use eda_model::footprint::to_board;
use eda_model::ir::{Design, FootprintInstance, Point, Side, TeardropSettings, Nm, Zone};
use eda_model::{ConstraintModel, Pad, PadKind, PadShape};
use eda_clipper2::Point64;
use eda_shape_poly_set::basic_shapes::{circle_polygon, oval_polygon, round_chamfered_rect_polygon, trapezoid_polygon, ErrorLoc};
use eda_shape_poly_set::hatch::point_inside;
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

/// What `TEARDROP_MANAGER` reads of the via or pad a teardrop is built on (`aOther` / `aCandidate`).
struct Anchor {
    /// `GetPosition()`: a via's centre, a pad's position (its shape offset not added).
    center: Point64,
    /// `TEARDROP_MANAGER::GetWidth`: a via's diameter, a pad's smaller size (`std::min( pad->GetSize( aLayer ).x, .y )`, the size of the pad's own frame).
    width: i64,
    net: String,
    reach: Reach,
    kind: AnchorKind,
    /// The outline `findAnchorPointsOnTrack`, `computeChordThroughShape` and `computeAnchorPoints` build on: a round anchor's circle about its position
    /// (`TransformCircleToPolygon( .., aOtherPos, GetWidth( .. ) / 2, maxError, ERROR_INSIDE, 16 )`), any other pad's own polygon.
    outline: Vec<Point64>,
}

enum AnchorKind {
    /// `PCB_VIA`: round, and hit by a point within `GetWidth() / 2` of its centre (`PCB_VIA::HitTest`).
    Via,
    /// `PAD`: `round` is `TEARDROP_MANAGER::IsRound`; `poly` the pad's copper (`PAD::GetEffectivePolygon( layer, ERROR_INSIDE )`, which `PAD::HitTest` asks),
    /// `bounding_radius` `PAD::GetBoundingRadius()`.
    Pad { round: bool, poly: Vec<Point64>, bounding_radius: i64 },
}

impl Anchor {
    fn is_round(&self) -> bool {
        match self.kind {
            AnchorKind::Via => true,
            AnchorKind::Pad { round, .. } => round,
        }
    }

    /// `PCB_VIA::HitTest( aPosition, 0 )`: `rel_pos.x² + rel_pos.y² <= max_dist²` in doubles, `max_dist = GetWidth() / 2`; `PAD::HitTest( aPosition, 0, aLayer )`:
    /// within the bounding radius, then `GetEffectivePolygon( aLayer, ERROR_INSIDE )->Contains( aPosition )`.
    fn hit_test(&self, p: Point64) -> bool {
        let (dx, dy) = (p.x - self.center.x, p.y - self.center.y);
        match &self.kind {
            AnchorKind::Via => {
                let max_dist = (self.width / 2) as f64;
                (dx as f64) * (dx as f64) + (dy as f64) * (dy as f64) <= max_dist * max_dist
            }
            AnchorKind::Pad { poly, bounding_radius, .. } => {
                let d2 = dx as i128 * dx as i128 + dy as i128 * dy as i128;
                d2 <= *bounding_radius as i128 * *bounding_radius as i128 && point_inside(poly, p)
            }
        }
    }
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

/// How KiCad turns a pad's own frame onto the board: `PAD::GetOrientation()` in degrees, the file's `(at x y angle)` (`eda_kicad::pad_file_angle`: the model's
/// turns are clockwise on the screen, KiCad's counter-clockwise, and a bottom footprint's pad turns the other way under the mirror), and whether the pad's own
/// frame is mirrored first (a bottom footprint: the model keeps a footprint as seen from the top and mirrors x on placement, KiCad's file holds the pad flipped).
fn kicad_pad_frame(fp: &FootprintInstance, lp: &Pad) -> (f64, bool) {
    let bottom = fp.side == Side::Bottom;
    let pad_rot = if bottom { -(lp.rot as i64) } else { lp.rot as i64 };
    (-((fp.rot as i64 + pad_rot) as f64) / 1000.0, bottom)
}

/// A point of a pad's own frame, as the model holds it (its position the origin, before its rotation), turned as KiCad turns the pad's shape (`SHAPE_POLY_SET::
/// Rotate( GetOrientation() )`, `RotatePoint` on every vertex: exact on the quarter turns, KiCad's sine and cosine elsewhere).
fn pad_frame_turn(orientation: f64, mirrored: bool, p: Point64) -> Point64 {
    eda_shape_poly_set::basic_shapes::rotate_point_degrees(if mirrored { Point64::new(-p.x, p.y) } else { p }, orientation)
}

/// `PAD::TransformShapeToPolygon( aBuffer, aLayer, 0, maxError, ERROR_INSIDE )` (`pcbnew/pad.cpp`) of the pad `lp` of `fp`, whose position on the board is
/// `center`: a circle (and an oval as wide as it is high) `TransformCircleToPolygon` of half the width at `ShapePos`, a longer oval `TransformOvalToPolygon`
/// between its ends turned by the orientation, a rectangle and a trapezoid `TransformTrapezoidToPolygon`, a rounded or chamfered rectangle
/// `TransformRoundChamferedRectToPolygon` (`PAD_MIN_SEG_PER_CIRCLE_COUNT` 16), these built about the origin, turned by the pad's orientation and moved to
/// `ShapePos` (the position plus the turned shape offset). `None` for a custom pad (`MergePrimitivesAsPolygon` is not ported here).
fn pad_polygon(fp: &FootprintInstance, lp: &Pad, center: Point64) -> Option<Vec<Point64>> {
    let (orientation, mirrored) = kicad_pad_frame(fp, lp);
    let turn = |p: Point64| pad_frame_turn(orientation, mirrored, p);
    let (w, h) = (lp.size.0.get(), lp.size.1.get());
    let (dx, dy) = (w / 2, h / 2);
    let off = lp.ext.offset.map_or(Point64::new(0, 0), |(x, y)| turn(Point64::new(x.get(), y.get())));
    let shape_pos = Point64::new(center.x + off.x, center.y + off.y);
    let local = match lp.shape {
        // "Note: dx == dy is not guaranteed for circle pads in legacy boards".
        PadShape::Circle => return Some(circle_polygon(shape_pos, dx, MAX_ERROR, ErrorLoc::Inside, 16)),
        PadShape::Oval if dx == dy => return Some(circle_polygon(shape_pos, dx, MAX_ERROR, ErrorLoc::Inside, 16)),
        PadShape::Oval => {
            // `VECTOR2I delta( dx - half_width, dy - half_width ); RotatePoint( delta, GetOrientation() );` (an oval is its own mirror image).
            let half_width = dx.min(dy);
            let delta = eda_shape_poly_set::basic_shapes::rotate_point_degrees(Point64::new(dx - half_width, dy - half_width), orientation);
            let (a, b) = (Point64::new(shape_pos.x - delta.x, shape_pos.y - delta.y), Point64::new(shape_pos.x + delta.x, shape_pos.y + delta.y));
            return Some(oval_polygon(a, b, half_width * 2, MAX_ERROR, ErrorLoc::Inside, 16));
        }
        PadShape::Rect => trapezoid_polygon((w, h), 0, 0, MAX_ERROR),
        PadShape::Trapezoid => {
            let d = lp.ext.trapezoid_delta.unwrap_or((Nm::ZERO, Nm::ZERO));
            trapezoid_polygon((w, h), d.0.get() / 2, d.1.get() / 2, MAX_ERROR)
        }
        PadShape::RoundRect => round_chamfered_rect_polygon((w, h), eda_model::pad_geom::round_rect_radius(lp).get(), 0.0, 0, MAX_ERROR),
        PadShape::ChamferedRect => round_chamfered_rect_polygon((w, h), eda_model::pad_geom::round_rect_radius(lp).get(), lp.ext.chamfer_ratio.unwrap_or(0.0), lp.ext.chamfer_corners.bits(), MAX_ERROR),
        PadShape::Custom => return None,
    };
    Some(
        local
            .into_iter()
            .map(|p| {
                let q = turn(p);
                Point64::new(q.x + shape_pos.x, q.y + shape_pos.y)
            })
            .collect(),
    )
}

/// `PAD::GetBoundingRadius()` (`BuildEffectivePolygon( ERROR_OUTSIDE )`): the farthest vertex of the pad's `ERROR_OUTSIDE` polygon from its position, and half
/// the drill. Here the `ERROR_INSIDE` polygon's farthest vertex, grown by `maxError` when the pad has curves (a deliberate approximation: the radius only
/// bounds `PAD::HitTest` and sets how far `computeTeardropPolygon` casts its ray into the pad, which reaches the far side either way).
fn pad_bounding_radius(lp: &Pad, poly: &[Point64], center: Point64) -> i64 {
    let far = poly.iter().map(|p| euclidean_norm(p.x - center.x, p.y - center.y)).max().unwrap_or(0);
    let curved = !matches!(lp.shape, PadShape::Rect | PadShape::Trapezoid) && !(lp.shape == PadShape::ChamferedRect && eda_model::pad_geom::round_rect_radius(lp) == Nm::ZERO);
    let drill = lp.drill.map_or(0, |d| d.get()).max(lp.drill_slot.map_or(0, |(a, b)| a.get().max(b.get())));
    (far + if curved { MAX_ERROR } else { 0 }).max((drill as f64 / 2.0).round() as i64)
}

fn collect_anchors(design: &Design, model: &ConstraintModel, settings: &TeardropSettings) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    let round_outline = |c: Point64, w: i64| circle_polygon(c, w / 2, MAX_ERROR, ErrorLoc::Inside, 16);

    if settings.target_vias {
        if let Some(rt) = &design.routing {
            for v in &rt.vias {
                if v.net.is_empty() {
                    continue;
                }
                let center = p64(v.at);
                let width = v.diameter.get();
                anchors.push(Anchor { center, width, net: v.net.clone(), reach: Reach::Span(v.from_layer.clone(), v.to_layer.clone()), kind: AnchorKind::Via, outline: round_outline(center, width) });
            }
        }
    }

    if settings.target_pth_pads || settings.target_smd_pads {
        let net_of_pin = net_of_pin_map(model);
        if let Some(pl) = &design.placement {
            for fp in &pl.footprints {
                let Some(part) = model.part(&fp.id) else { continue };
                let Some(footprint) = model.footprint_of(part) else { continue };
                for lp in &footprint.pads {
                    let is_pth = lp.kind == PadKind::ThroughHole;
                    let is_npth = lp.kind == PadKind::NonPlatedHole;
                    if is_npth {
                        continue; // no copper connection to tear-drop at all
                    }
                    // `DIALOG_GLOBAL_EDIT_TEARDROPS::TransferDataFromWindow`: PTH pads by "PTH pads", SMD and edge connector pads by "SMD pads".
                    if is_pth && !settings.target_pth_pads {
                        continue;
                    }
                    if !is_pth && !settings.target_smd_pads {
                        continue;
                    }
                    let net = net_of_pin.get(&format!("{}.{}", fp.id, lp.number)).cloned().unwrap_or_default();
                    if net.is_empty() {
                        continue;
                    }
                    let center = p64(to_board(fp, lp.at));
                    let Some(poly) = pad_polygon(fp, lp, center) else { continue };
                    let width = lp.size.0.get().min(lp.size.1.get());
                    // `TEARDROP_MANAGER::IsRound`: a circle, or an oval as wide as it is high.
                    let round = lp.shape == PadShape::Circle || (lp.shape == PadShape::Oval && lp.size.0 == lp.size.1);
                    let reach = if is_pth {
                        // A plated through-hole pad spans the whole copper
                        // stack, same convention `eda_connectivity::items`
                        // uses for a THT pad's layer span.
                        let (first, last) = (model.board.layers.first().cloned().unwrap_or_default(), model.board.layers.last().cloned().unwrap_or_default());
                        Reach::Span(first, last)
                    } else {
                        // An SMD pad is on its footprint's side, or on the other one (`Pad::opposite_side`).
                        let front = (fp.side == Side::Top) != lp.opposite_side;
                        Reach::Layer(if front { "F.Cu".to_string() } else { "B.Cu".to_string() })
                    };
                    let bounding_radius = pad_bounding_radius(lp, &poly, center);
                    let outline = if round { round_outline(center, width) } else { poly.clone() };
                    anchors.push(Anchor { center, width, net, reach, kind: AnchorKind::Pad { round, poly, bounding_radius }, outline });
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

/// `( a + b ) / 2` of two `VECTOR2I`s: `VECTOR2<int>::operator/( double )` rounds each coordinate (`KiROUND( x / aFactor )`), half away from zero.
fn half_sum(a: Point64, b: Point64) -> Point64 {
    Point64::new(((a.x + b.x) as f64 / 2.0).round() as i64, ((a.y + b.y) as f64 / 2.0).round() as i64)
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


/// `TEARDROP_MANAGER::computeAnchorPoints` (`pcbnew/teardrop/teardrop_utils.cpp`, 25ec2335): the anchor's outline (a round anchor's circle, a pad's polygon),
/// clipped to a band `preferred_width` wide (or `m_TdMaxWidth` when that is narrower) along the teardrop's axis when the width is constrained -- always for a
/// pad that is not round ("the teardrop height use the smaller value of X and Y sizes") -- then C and E are the hull neighbours of B and A in the convex hull
/// of A, B and that polygon, swapped if that gives the larger pentagon. `pts` is `[A, B, _, D, _]`. ("A max width of 0 means no limit, so a min against it
/// would clip the shape to nothing.")
fn compute_anchor_points(settings: &TeardropSettings, anchor: &Anchor, pts: &mut [Point64; 5]) -> bool {
    let width = anchor.width;
    let preferred_width = (width as f64 * settings.best_width_ratio).round() as i64;
    let force_clip = settings.best_width_ratio < 1.0 || !anchor.is_round();
    let mut c_buffer = ShapePolySet::from_outline(anchor.outline.clone());

    let max_width = settings.max_width_nm.get();
    if force_clip || (max_width > 0 && max_width < preferred_width) {
        let halfsize = (if max_width > 0 { max_width.min(preferred_width) } else { preferred_width }) / 2;
        // `( aPts[0] + aPts[1] ) / 2`: `VECTOR2I::operator/( double )`, `KiROUND` of each half.
        let ref_on_track = half_sum(pts[0], pts[1]);
        let (ax, ay) = (pts[3].x - ref_on_track.x, pts[3].y - ref_on_track.y);
        // `EDA_ANGLE orient( teardrop_axis )`, degrees, and `clipping_rect.Rotate( -orient )` (`RotatePoint` on each corner) then `Move( ref_on_track )`.
        let orient = deg_angle(ax as f64, ay as f64);
        let len = euclidean_norm(ax, ay);
        let rect = [Point64::new(0, -halfsize), Point64::new(0, halfsize), Point64::new(len, halfsize), Point64::new(len, -halfsize)];
        let rect: Vec<Point64> = rect
            .iter()
            .map(|&p| {
                let r = eda_shape_poly_set::basic_shapes::rotate_point_degrees(p, -orient);
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

/// `EDA_ANGLE( const VECTOR2D& )` (`eda_angle.h`), degrees: the axes and the diagonals exact, else `atan2` in radians divided by `DEGREES_TO_RADIANS`.
fn deg_angle(x: f64, y: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        0.0
    } else if y == 0.0 {
        if x >= 0.0 { 0.0 } else { -180.0 }
    } else if x == 0.0 {
        if y >= 0.0 { 90.0 } else { -90.0 }
    } else if x == y {
        if x >= 0.0 { 45.0 } else { -180.0 + 45.0 }
    } else if x == -y {
        if x >= 0.0 { -45.0 } else { 180.0 - 45.0 }
    } else {
        y.atan2(x) / DEGREES_TO_RADIANS
    }
}

/// `EDA_ANGLE::DEGREES_TO_RADIANS`.
const DEGREES_TO_RADIANS: f64 = std::f64::consts::PI / 180.0;

/// `EDA_ANGLE::Normalize` for the small angles met here: into [0, 360).
fn deg_normalize(mut a: f64) -> f64 {
    while a < 0.0 {
        a += 360.0;
    }
    while a >= 360.0 {
        a -= 360.0;
    }
    a
}

/// `EDA_ANGLE::Cos`: exact at the multiples of 45 degrees (of the normalised angle), else `cos( AsRadians() )` of the angle as it is.
fn deg_cos(a: f64) -> f64 {
    let sqrt1_2 = std::f64::consts::FRAC_1_SQRT_2;
    match deg_normalize(a) {
        t if t == 0.0 => 1.0,
        t if t == 180.0 => -1.0,
        t if t == 90.0 || t == 270.0 => 0.0,
        t if t == 45.0 || t == 315.0 => sqrt1_2,
        t if t == 135.0 || t == 225.0 => -sqrt1_2,
        _ => (a * DEGREES_TO_RADIANS).cos(),
    }
}

/// `EDA_ANGLE::Sin`, as [`deg_cos`].
fn deg_sin(a: f64) -> f64 {
    let sqrt1_2 = std::f64::consts::FRAC_1_SQRT_2;
    match deg_normalize(a) {
        t if t == 0.0 || t == 180.0 => 0.0,
        t if t == 45.0 || t == 135.0 => sqrt1_2,
        t if t == 225.0 || t == 315.0 => -sqrt1_2,
        t if t == 90.0 => 1.0,
        t if t == 270.0 => -1.0,
        _ => (a * DEGREES_TO_RADIANS).sin(),
    }
}

/// `CalcArcCenter( const VECTOR2I& aStart, aMid, aEnd )` of 25ec2335 (`libs/kimath/src/trigo.cpp`): the circumcentre with the deltas exact in 64 bits and the
/// numerators in 128 ("only the final divisions round"), `snapArcCenter`'s round 100 nm or 10 nm centre when the three radii still agree to 1 nm,
/// `degenerateArcCenter`'s clusters and coincident pairs, `collinearArcCenter`, and `KiROUND`. (Round 6 D6-1: the plain circumcentre put a short arc's
/// teardrop 1 nm off kicad-cli's.)
fn calc_arc_center_i(start: Point64, mid: Point64, end: Point64) -> Point64 {
    let (ds, dm, de) = ((start.x as f64, start.y as f64), (mid.x as f64, mid.y as f64), (end.x as f64, end.y as f64));
    let norm = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1)).sqrt();
    let degenerate = || -> Option<(f64, f64)> {
        let (min_x, max_x) = (ds.0.min(dm.0).min(de.0), ds.0.max(dm.0).max(de.0));
        let (min_y, max_y) = (ds.1.min(dm.1).min(de.1), ds.1.max(dm.1).max(de.1));
        // kClusterExtent = 5, kCoincidentRadius = 2.
        if max_x - min_x < 5.0 && max_y - min_y < 5.0 {
            return Some(((ds.0 + dm.0 + de.0) / 3.0, (ds.1 + dm.1 + de.1) / 3.0));
        }
        let coincident = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1) < 4.0;
        if coincident(ds, dm) || coincident(dm, de) {
            return Some(((ds.0 + de.0) / 2.0, (ds.1 + de.1) / 2.0));
        }
        if coincident(ds, de) {
            return Some(((ds.0 + dm.0) / 2.0, (ds.1 + dm.1) / 2.0));
        }
        None
    };
    let center = degenerate().unwrap_or_else(|| {
        let (bx, by) = ((mid.x - start.x) as i128, (mid.y - start.y) as i128);
        let (cx, cy) = ((end.x - start.x) as i128, (end.y - start.y) as i128);
        let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
        let d = (bx * cy - by * cx) * 2;
        if d == 0 {
            // `collinearArcCenter`: the chord's middle, pushed 1e17 out along its normal.
            let (chx, chy) = (de.0 - ds.0, de.1 - ds.1);
            let len = chx.hypot(chy);
            let k = if len > 0.0 { 1e17 / len } else { 0.0 };
            return ((ds.0 + de.0) / 2.0 + chy * k, (ds.1 + de.1) / 2.0 - chx * k);
        }
        let dd = d as f64;
        let ux = (start.x as i128 * d + b2 * cy - c2 * by) as f64 / dd;
        let uy = (start.y as i128 * d + c2 * bx - b2 * cx) as f64 / dd;
        // `snapArcCenter`: kMaxExactInteger = 2^53, kSnapRadiusTolerance = 1.
        if !(ux.abs() < 9007199254740992.0) || !(uy.abs() < 9007199254740992.0) {
            return (ux, uy);
        }
        for grid in [100.0, 10.0] {
            let cand = ((ux / grid + 0.5).floor() * grid, (uy / grid + 0.5).floor() * grid);
            let (rs, rm, re) = (norm(cand, ds), norm(cand, dm), norm(cand, de));
            if rs.max(rm).max(re) - rs.min(rm).min(re) <= 1.0 {
                return cand;
            }
        }
        (ux, uy)
    });
    let clamp = |v: f64| v.clamp(i32::MIN as f64 + 100.0, i32::MAX as f64 - 100.0).round() as i64;
    Point64::new(clamp(center.0), clamp(center.1))
}

/// `SHAPE_ARC( aStart, aMid, aEnd, aWidth )` for a track arc (`libs/kimath/src/geometry/shape_arc.cpp`): the centre is `CalcArcCenter`'s ([`calc_arc_center_i`]),
/// an integer point, the radius a double from it (`sqrt( ( VECTOR2D( m_start ) - m_center ).SquaredEuclideanNorm() )`).
#[derive(Clone, Copy)]
struct TrackArc {
    start: Point64,
    mid: Point64,
    end: Point64,
    width: i64,
    center: (f64, f64),
    radius: f64,
}

impl TrackArc {
    fn new(start: Point, mid: Point, end: Point, width: Nm) -> Option<TrackArc> {
        let (s, m, e) = (p64(start), p64(mid), p64(end));
        // A straight "arc" is not one this generator follows (its centre would be `collinearArcCenter`'s, 1e17 away).
        if (m.x - s.x) as i128 * (e.y - s.y) as i128 - (m.y - s.y) as i128 * (e.x - s.x) as i128 == 0 {
            return None;
        }
        let c = calc_arc_center_i(s, m, e);
        let center = (c.x as f64, c.y as f64);
        let (dx, dy) = (s.x as f64 - center.0, s.y as f64 - center.1);
        Some(TrackArc { start: s, mid: m, end: e, width: width.get(), center, radius: (dx * dx + dy * dy).sqrt() })
    }

    /// `SHAPE_ARC::Reverse`: the ends swapped, the middle and the centre kept.
    fn reversed(&self) -> TrackArc {
        TrackArc { start: self.end, end: self.start, ..*self }
    }

    fn start_angle(&self) -> f64 {
        (self.start.y as f64 - self.center.1).atan2(self.start.x as f64 - self.center.0)
    }

    /// `SHAPE_ARC::GetCentralAngle`, radians: the middle point chooses between the two arcs (`IsCCW`).
    fn central_angle(&self) -> f64 {
        let (s, m, e) = (self.start, self.mid, self.end);
        let ccw = (e.x - m.x) as i128 * (s.y - m.y) as i128 - (e.y - m.y) as i128 * (s.x - m.x) as i128 > 0;
        let mut ca = (e.y as f64 - self.center.1).atan2(e.x as f64 - self.center.0) - self.start_angle();
        if ccw {
            if ca < 0.0 {
                ca += std::f64::consts::TAU;
            }
        } else if ca > 0.0 {
            ca -= std::f64::consts::TAU;
        }
        ca
    }

    /// `SHAPE_ARC::ConvertToPolyline( aMaxError )`: the segment count from the arc's *external* radius (`r + width / 2`, truncated to an `int` as
    /// `GetArcToSegmentCount` takes it), the points between the ends half the effective error outside the arc; repeated points dropped as
    /// `SHAPE_LINE_CHAIN::Append` does.
    fn polyline(&self, max_error: i64) -> Vec<Point64> {
        use eda_model::outline::{arc_to_segment_count, circle_to_end_segment_delta_radius};
        let (s, m, e) = (self.start, self.mid, self.end);
        let (cx, cy) = self.center;
        // `EDA_ANGLE`s, in degrees as KiCad keeps them (round 6 D6-1: the radians a vertex was computed in put it 1 nm off KiCad's): `GetStartAngle()`
        // normalised, `GetCentralAngle()` as `EDA_ANGLE( end - c ) - EDA_ANGLE( start - c )` turned by `IsCCW()`.
        let sa = deg_normalize(deg_angle(s.x as f64 - cx, s.y as f64 - cy));
        let mut ca = deg_angle(e.x as f64 - cx, e.y as f64 - cy) - deg_angle(s.x as f64 - cx, s.y as f64 - cy);
        if self.central_angle() > 0.0 {
            if ca < 0.0 {
                ca += 360.0;
            }
        } else if ca > 0.0 {
            ca -= 360.0;
        }
        let half_max_error = (max_error as f64 / 2.0).max(1.0);
        let external_radius = self.radius + self.width as f64 / 2.0;
        // `startToEnd.Distance( GetArcMid() )`: the middle's distance to the chord.
        let chord_dist = {
            let (dx, dy) = ((e.x - s.x) as f64, (e.y - s.y) as f64);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((m.x - s.x) as f64 * dx + (m.y - s.y) as f64 * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            ((m.x - s.x) as f64 - t * dx).hypot((m.y - s.y) as f64 - t * dy)
        };
        let (n, effective_error) = if external_radius < half_max_error || chord_dist < half_max_error {
            (0, external_radius)
        } else {
            let n = arc_to_segment_count((external_radius as i64) as f64, max_error as f64, ca.to_radians());
            let seg360 = (n as f64 * 360.0 / ca.abs()) as i64;
            // `CircleToEndSegmentDeltaRadius` returns an `int`, `KiROUND`ed (round 6 D6-1: unrounded, a short arc's vertex came 1 nm off KiCad's).
            (n, circle_to_end_segment_delta_radius((external_radius as i64) as f64, seg360).round())
        };
        let r = self.radius + effective_error / 2.0;
        let n2 = n * 2;
        let mut out = vec![s];
        let mut i = 1;
        while i < n2 {
            // `EDA_ANGLE a = sa; a += ( ca * i ) / n;` and `a.Cos()`, `a.Sin()`.
            let a = sa + ca * i as f64 / n2 as f64;
            let p = Point64::new((cx + r * deg_cos(a)).round() as i64, (cy + r * deg_sin(a)).round() as i64);
            if out.last() != Some(&p) {
                out.push(p);
            }
            i += 2;
        }
        if out.last() != Some(&e) {
            out.push(e);
        }
        out
    }

    /// `SHAPE_ARC::NearestPoint` (25ec2335): the full circle's nearest point (`CIRCLE( GetCenter(), GetRadius() )`, the radius truncated to an `int`, and
    /// `VECTOR2I::Resize`), snapped to an end within `sqrt( 8 )` nm, else the nearer end when it falls outside the arc's slice (`sliceContainsPoint`).
    fn nearest_point(&self, p: Point64) -> Point64 {
        let c = Point64::new(self.center.0 as i64, self.center.1 as i64);
        let (mut vx, vy) = (p.x - c.x, p.y - c.y);
        if vx == 0 && vy == 0 {
            vx = 1;
        }
        let (rx, ry) = resize(vx, vy, self.radius as i64);
        let nearest = Point64::new(rx + c.x, ry + c.y);
        let d2 = |a: Point64, b: Point64| ((a.x - b.x) as i128).pow(2) + ((a.y - b.y) as i128).pow(2);
        if d2(nearest, self.start) <= 8 {
            return self.start;
        }
        if d2(nearest, self.end) <= 8 {
            return self.end;
        }
        let tau = std::f64::consts::TAU;
        let sa = self.start_angle().rem_euclid(tau);
        let ca = self.central_angle();
        let ea = sa + ca;
        let mut phi = ((nearest.y - c.y) as f64).atan2((nearest.x - c.x) as f64).rem_euclid(tau);
        let inside = if ca >= 0.0 {
            while phi < sa {
                phi += tau;
            }
            phi >= sa && phi <= ea
        } else {
            while phi > sa {
                phi -= tau;
            }
            phi <= sa && phi >= ea
        };
        if inside {
            nearest
        } else if d2(p, self.start) <= d2(p, self.end) {
            self.start
        } else {
            self.end
        }
    }
}

/// `VECTOR2I::Resize( aNewLength )` (`libs/kimath/include/math/vector2d.h`): exact on the diagonals, else each coordinate `sqrt( rescale( len^2, c^2, l^2 ) )`,
/// rounded, with its sign.
fn resize(x: i64, y: i64, len: i64) -> (i64, i64) {
    if x == 0 && y == 0 {
        return (0, 0);
    }
    let (nx, ny) = if x.abs() == y.abs() {
        let v = len.abs() as f64 * std::f64::consts::FRAC_1_SQRT_2;
        (v, v)
    } else {
        let (xs, ys) = (x as i128 * x as i128, y as i128 * y as i128);
        let (l2, n2) = (xs + ys, len as i128 * len as i128);
        ((rescale(n2, xs, l2) as f64).sqrt(), (rescale(n2, ys, l2) as f64).sqrt())
    };
    let sign = len.signum();
    ((if x < 0 { -nx.round() } else { nx.round() }) as i64 * sign, (if y < 0 { -ny.round() } else { ny.round() }) as i64 * sign)
}

/// `SHAPE_LINE_CHAIN::Intersect( const SHAPE_LINE_CHAIN& aChain, INTERSECTIONS& )` of the closed `outline` with the open polyline `poly`, in KiCad's order:
/// the outline's edges in turn, and for each the polyline's segments by their smallest x. Collinear overlaps are not ported (see [`seg_intersect`]).
fn chain_chain_intersect(outline: &[Point64], poly: &[Point64]) -> Vec<Point64> {
    let mut theirs: Vec<(Point64, Point64)> = poly.windows(2).map(|w| (w[0], w[1])).collect();
    theirs.sort_by_key(|(a, b)| a.x.min(b.x));
    let mut hits = Vec::new();
    for s in 0..outline.len() {
        let (pa, pb) = (outline[s], outline[(s + 1) % outline.len()]);
        for &(c, d) in &theirs {
            if let Some(p) = seg_intersect(pa, pb, c, d) {
                hits.push(p);
            }
        }
    }
    hits
}


/// `TEARDROP_MANAGER::computeChordThroughShape` (`pcbnew/teardrop/teardrop_utils.cpp`, 25ec2335) for a straight track and the anchor's `outline` (a round
/// anchor's circle, `TransformCircleToPolygon( shapebuffer, pos, radius, maxError, ERROR_INSIDE, 16 )`, or a pad's polygon,
/// `PAD::TransformShapeToPolygon( shapebuffer, aLayer, 0, maxError, ERROR_INSIDE )`): the length of the copper span of the track's centreline, extended both ways
/// by the polygon's box diagonal plus the track's length, that holds `inside`. The crossings are projected on the direction, sorted, and the span between two
/// neighbours whose middle is inside the polygon and that brackets `inside` is the answer. A zero-length track, fewer than two crossings or no such span answer
/// `i64::MAX` ("keep the teardrop"). (The arc's `std::numeric_limits<int>::max()` is the caller's.)
fn chord_through_shape(outline: &[Point64], start: Point64, end: Point64, inside: Point64) -> i64 {
    let (dx, dy) = ((end.x - start.x) as f64, (end.y - start.y) as f64);
    let len = dx.hypot(dy);
    if len == 0.0 || outline.is_empty() {
        return i64::MAX;
    }
    let (dir_x, dir_y) = (dx / len, dy / len);
    // `VECTOR2I mid = ( start + end ) / 2`: `VECTOR2I::operator/( double )`, `KiROUND` of each half.
    let mid = half_sum(start, end);
    // `shapebuffer.BBox().Diagonal()`: the box's size, `VECTOR2I::EuclideanNorm`.
    let (min_x, max_x) = outline.iter().fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
    let (min_y, max_y) = outline.iter().fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let reach = (euclidean_norm(max_x - min_x, max_y - min_y) as f64 + len).round();
    let off = Point64::new((dir_x * reach).round() as i64, (dir_y * reach).round() as i64);
    let (ext_start, ext_end) = (Point64::new(mid.x - off.x, mid.y - off.y), Point64::new(mid.x + off.x, mid.y + off.y));
    let pts = chain_intersect(outline, ext_start, ext_end);
    // "Degenerate/tangent-only crossings should not drop the teardrop."
    if pts.len() < 2 {
        return i64::MAX;
    }
    let project = |x: f64, y: f64| (x - ext_start.x as f64) * dir_x + (y - ext_start.y as f64) * dir_y;
    let mut proj: Vec<f64> = pts.iter().map(|p| project(p.x as f64, p.y as f64)).collect();
    proj.sort_by(|a, b| a.total_cmp(b));
    let inside_proj = project(inside.x as f64, inside.y as f64);
    for ii in 0..proj.len() - 1 {
        let m = (proj[ii] + proj[ii + 1]) / 2.0;
        let span_mid = Point64::new(ext_start.x + (dir_x * m).round() as i64, ext_start.y + (dir_y * m).round() as i64);
        if !point_inside(outline, span_mid) {
            continue;
        }
        if inside_proj >= proj[ii] && inside_proj <= proj[ii + 1] {
            return (proj[ii + 1] - proj[ii]).round() as i64;
        }
    }
    // "Boundary-touch fallback: keep the teardrop."
    i64::MAX
}

/// `SEG( a, b ).NearestPoint( p )` (`seg.cpp`): the end when `p` projects outside the segment, else `A + rescale( t, d, l² )`.
fn seg_nearest_point(a: Point64, b: Point64, p: Point64) -> Point64 {
    let (dx, dy) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
    let l2 = dx * dx + dy * dy;
    if l2 == 0 {
        return a;
    }
    let t = dx * (p.x - a.x) as i128 + dy * (p.y - a.y) as i128;
    if t < 0 {
        return a;
    }
    if t > l2 {
        return b;
    }
    Point64::new(a.x + rescale(t, dx, l2) as i64, a.y + rescale(t, dy, l2) as i64)
}

/// `SEG( a, b ).SquaredDistance( p )` (`seg.cpp`, 25ec2335): an end's when `p` projects outside, else the Lagrange identity `cross² / |ab|²`, `KiROUND`ed.
fn seg_squared_distance(a: Point64, b: Point64, p: Point64) -> i128 {
    let (abx, aby) = ((b.x - a.x) as i128, (b.y - a.y) as i128);
    let (apx, apy) = ((p.x - a.x) as i128, (p.y - a.y) as i128);
    let e = apx * abx + apy * aby;
    if e <= 0 {
        return apx * apx + apy * apy;
    }
    let f = abx * abx + aby * aby;
    if e >= f {
        let (bpx, bpy) = ((p.x - b.x) as i128, (p.y - b.y) as i128);
        return bpx * bpx + bpy * bpy;
    }
    let cross = (abx * apy - aby * apx) as f64;
    let g = cross * cross / f as f64;
    if !(g >= 0.0) {
        return 0;
    }
    g.round() as i128
}

/// `TestSegmentHit( aRefPoint, aStart, aEnd, aDist )` (`trigo.cpp`): outside the box grown by `dist`, no; along a vertical or horizontal segment, the offset
/// across it; else `SquaredDistance < ( dist + 1 )²`.
fn test_segment_hit(p: Point64, start: Point64, end: Point64, dist: i64) -> bool {
    let (xmin, xmax) = (start.x.min(end.x), start.x.max(end.x));
    let (ymin, ymax) = (start.y.min(end.y), start.y.max(end.y));
    if ymin - p.y > dist || p.y - ymax > dist || xmin - p.x > dist || p.x - xmax > dist {
        return false;
    }
    if start.x == end.x && p.y > ymin && p.y < ymax {
        return (start.x - p.x).abs() <= dist;
    }
    if start.y == end.y && p.x > xmin && p.x < xmax {
        return (start.y - p.y).abs() <= dist;
    }
    seg_squared_distance(start, end, p) < (dist as i128 + 1) * (dist as i128 + 1)
}

/// One `PCB_TRACK` as `UpdateTeardrops` walks `m_board->Tracks()`: a straight segment (`PCB_TRACE_T`), or a track arc (`PCB_ARC_T`) when `arc` is set.
struct TrackItem {
    start: Point64,
    end: Point64,
    arc: Option<TrackArc>,
    width: i64,
}

impl TrackItem {
    /// The KiCad items of a track: an arc is one (`Track::arc`, round 5's A5-1), a polyline one segment per pair of points, as the writer writes it
    /// (`crates/kicad/src/pcb.rs`: a `(segment ..)` per pair). A straight "arc" is the segment between its ends.
    fn of(t: &eda_model::ir::Track) -> Vec<TrackItem> {
        let width = t.width.get();
        if let Some((s, m, e)) = t.arc() {
            return vec![TrackItem { start: p64(s), end: p64(e), arc: TrackArc::new(s, m, e, t.width), width }];
        }
        t.pts.windows(2).map(|w| TrackItem { start: p64(w[0]), end: p64(w[1]), arc: None, width }).collect()
    }

    /// `PCB_TRACK::HitTest( aPosition )` (`TestSegmentHit` within half the width) and `PCB_ARC::HitTest( aPosition )` (an end within half the width, else the
    /// distance to the circle within it and the point's angle inside the arc's), with no accuracy.
    fn hit_test(&self, p: Point64) -> bool {
        let Some(arc) = &self.arc else { return test_segment_hit(p, self.start, self.end, self.width / 2) };
        let max_dist = self.width as f64 / 2.0;
        let dist = |a: Point64| ((p.x - a.x) as f64).hypot((p.y - a.y) as f64);
        if dist(self.start) <= max_dist || dist(self.end) <= max_dist {
            return true;
        }
        let c = Point64::new(arc.center.0 as i64, arc.center.1 as i64);
        let rel = (p.x - c.x, p.y - c.y);
        if (euclidean_norm(rel.0, rel.1) as f64 - arc.radius).abs() > max_dist {
            return false;
        }
        // `GetAngle()` (signed), `GetArcAngleStart()` (0 .. 360) and `EDA_ANGLE( relpos )`, degrees.
        let (sa, ca) = (deg_normalize(deg_angle((arc.start.x - c.x) as f64, (arc.start.y - c.y) as f64)), arc.central_angle().to_degrees());
        let hit = deg_normalize(deg_angle(rel.0 as f64, rel.1 as f64) - sa);
        if ca < 0.0 {
            hit >= 360.0 + ca
        } else {
            hit <= ca
        }
    }

    /// `buildCrossingStub`'s end: the point of the track's centreline nearest `p` (`SEG::NearestPoint`, `SHAPE_ARC::NearestPoint`).
    fn nearest_point(&self, p: Point64) -> Point64 {
        match &self.arc {
            Some(arc) => arc.nearest_point(p),
            None => seg_nearest_point(self.start, self.end, p),
        }
    }
}

/// `TEARDROP_MANAGER::computeTeardropPolygon` with straight edges and one track (`pcbnew/teardrop/teardrop_utils.cpp`, 25ec2335): the pentagon `[A, B, C, D, E]`
/// of the teardrop of `item` (a segment, an arc, or a crossing's stub) at `anchor`. A and B are on the track, C and E on the anchor's outline, D just behind
/// the anchor's centre.
///
/// - `findAnchorPointsOnTrack`: the track's end inside the anchor is made its start (`if( !aOther->HitTest( start, 0 ) ) std::swap( start, end )`); the teardrop's
///   length is measured from where the track crosses the anchor's outline (`aIntersection = pts[0].p; start = aIntersection;`), `KiROUND( width *
///   m_BestLengthRatio )` capped by `m_TdMaxLen`, and by the chord past the outline (`actualTdLen = std::min( targetLength, SEG( start, end ).Length() )`).
/// - An arc (`PCB_ARC_T`, round 5's A5-1): the crossing is the outline's with the arc's polyline (`arc.ConvertToPolyline( maxError )`), and the reference
///   segment is the polyline's last one whose start is nearer the crossing than `actualTdLen`, walked from the far end ("find the best teardrop end point on
///   the arc"), with `actualTdLen` less that start's distance from the crossing.
/// - A and B: `start + vecT * len ± perp * width / 2`, the doubles truncated into a `VECTOR2I`. For a pad that is not round, either inside the pad refuses the
///   teardrop ("To build a polygonal valid shape pointA and point B must be outside the pad").
/// - D: `intersection - vecVia * ( effectiveDist + 1 um )`, `effectiveDist` the larger of the centre's projection on the axis and the radius, clamped for a
///   round anchor to the far side of its circle less 2 um, and for any other pad to where a ray cast into it from the crossing first leaves its copper, less
///   2 um ("so pointD stays inside the copper the track enters"). (`vecVia` is `vecT`: one track, and an arc keeps its own track.)
/// - C and E: [`compute_anchor_points`]; for a round anchor entered off its centre, made symmetric about the track's axis.
///
/// Returns `None` for the skips of `findAnchorPointsOnTrack` and `computeTeardropPolygon` (a skip, not an error). Not ported: `m_AllowUseTwoTracks` (the
/// teardrop is cut at the track's end), curved edges, and a custom pad's `effectiveDist`.
fn compute_teardrop_polygon(anchor: &Anchor, item: &TrackItem, settings: &TeardropSettings) -> Option<Vec<Point64>> {
    let (w, tw) = (anchor.width, item.width);
    let radius = w / 2;
    if radius <= 0 || tw <= 0 {
        return None;
    }
    let c = anchor.center;
    let outline = &anchor.outline;

    // `findAnchorPointsOnTrack`.
    let mut target_length = (w as f64 * settings.best_length_ratio).round() as i64;
    if settings.max_len_nm.get() > 0 {
        target_length = target_length.min(settings.max_len_nm.get());
    }
    let (mut start, mut end) = (item.start, item.end);
    // "aTrack is expected to have one end inside the via/pad and the other end outside so ensure the start point is inside the via/pad".
    let need_swap = !anchor.hit_test(start);
    if need_swap {
        std::mem::swap(&mut start, &mut end);
    }
    // The track's own polyline when it is an arc (`SHAPE_ARC( GetStart(), GetMid(), GetEnd(), GetWidth() ).ConvertToPolyline( maxError )`).
    let hits = match &item.arc {
        Some(arc) => chain_chain_intersect(outline, &arc.polyline(MAX_ERROR)).first().copied().into_iter().collect::<Vec<_>>(),
        None => chain_intersect(outline, start, end),
    };
    // "Ensure a intersection point was found, otherwise we cannot built the teardrop using this track (it is fully outside or inside the pad/via shape)".
    let intersection = *hits.first()?;
    start = intersection;
    let mut stub_len = target_length.min(euclidean_norm(end.x - start.x, end.y - start.y));
    if let Some(track_arc) = &item.arc {
        // "if aTrack is an arc, find the best teardrop end point on the arc": the arc from the anchor's end outwards (`arc.Reverse()` when the anchor is at
        // the arc's end). 25ec2335 (the kicad-cli this port is proven against) then moves the polyline's points onto the arc ("The conversion places its
        // corner points slightly outside the arc. Move them onto the arc so they can be used as points on the track centerline.").
        let oriented = if need_swap { track_arc.reversed() } else { *track_arc };
        let poly: Vec<Point64> = oriented.polyline(MAX_ERROR).into_iter().map(|p| oriented.nearest_point(p)).collect();
        let ref_length_point = start;
        if poly.len() > 2 {
            for ii in (0..poly.len()).rev() {
                let dist_from_start = euclidean_norm(poly[ii].x - start.x, poly[ii].y - start.y);
                if dist_from_start < stub_len || ii == 0 {
                    start = poly[ii];
                    if ii < poly.len() - 1 {
                        end = poly[ii + 1];
                    }
                    stub_len -= euclidean_norm(start.x - ref_length_point.x, start.y - ref_length_point.y);
                    stub_len = stub_len.max(0).min(euclidean_norm(end.x - start.x, end.y - start.y));
                    break;
                }
            }
        }
    }
    // "The start and end points must be different to calculate a valid polygon shape".
    if start == end {
        return None;
    }

    // `computeTeardropPolygon`.
    let (vx, vy) = ((end.x - start.x) as f64, (end.y - start.y) as f64);
    let norm = vx.hypot(vy);
    let (tx, ty) = (vx / norm, vy / norm);
    // 25ec2335: "The canvas draws an arc track as short straight lines that can sit up to the max deviation inside the true edge. Keep the corners inside the
    // track by that amount so they cannot show past the drawn edge." (`std::max( GetWidth() / 4, GetWidth() / 2 - maxError )` for an arc.)
    let track_halfwidth = if item.arc.is_some() { (tw / 4).max(tw / 2 - MAX_ERROR) } else { tw / 2 };
    let hw = track_halfwidth as f64;
    let len = stub_len as f64;
    let point_b = Point64::new(start.x + (tx * len + ty * hw) as i64, start.y + (ty * len - tx * hw) as i64);
    let point_a = Point64::new(start.x + (tx * len - ty * hw) as i64, start.y + (ty * len + tx * hw) as i64);
    // "To build a polygonal valid shape pointA and point B must be outside the pad. It can be inside with some pad shapes having very different X and X sizes".
    if !anchor.is_round() && (anchor.hit_test(point_a) || anchor.hit_test(point_b)) {
        return None;
    }

    let r = radius as f64;
    let (icx, icy) = ((c.x - intersection.x) as f64, (c.y - intersection.y) as f64);
    let proj_on_track = -(icx * tx + icy * ty);
    let mut effective_dist = proj_on_track.max(r);
    let offset = 1_000.0; // pcbIUScale.mmToIU( 0.001 )
    match &anchor.kind {
        AnchorKind::Pad { round: false, poly, bounding_radius } => {
            // "For non-round pads, clamp effectiveDist so pointD stays inside the copper the track enters rather than spiking out the far side on an oblique entry
            // that only grazes a corner": a ray from the crossing into the pad, as long as the pad's circumscribed radius allows, and the first crossing past
            // which a probe is outside the copper.
            let reach = effective_dist + 2.0 * *bounding_radius as f64 + offset;
            let ray_end = Point64::new(intersection.x + (-tx * reach).round() as i64, intersection.y + (-ty * reach).round() as i64);
            let mut crossings: Vec<f64> = chain_intersect(poly, intersection, ray_end)
                .into_iter()
                .map(|h| euclidean_norm(h.x - intersection.x, h.y - intersection.y) as f64)
                .filter(|&d| d > offset)
                .collect();
            crossings.sort_by(|a, b| a.total_cmp(b));
            let mut exit_edge = 0.0;
            for d in crossings {
                let probe = Point64::new(intersection.x + (-tx * (d + offset)).round() as i64, intersection.y + (-ty * (d + offset)).round() as i64);
                if !point_inside(poly, probe) {
                    exit_edge = d;
                    break;
                }
            }
            // "exitEdge == 0 means -vecVia does not penetrate the pad (a tangential graze); collapse pointD onto the entry".
            effective_dist = effective_dist.min((exit_edge - 2.0 * offset).max(0.0));
        }
        _ => {
            // "For round pads/vias, clamp effectiveDist so pointD stays inside the pad circle."
            let disc = proj_on_track * proj_on_track - (icx * icx + icy * icy - r * r);
            if disc >= 0.0 {
                let far_edge = proj_on_track + disc.sqrt();
                let max_allowed = (far_edge - 2.0 * offset).max(0.0);
                if effective_dist > max_allowed {
                    effective_dist = max_allowed;
                }
            }
        }
    }
    let point_d = Point64::new(intersection.x + (-tx * (effective_dist + offset)).round() as i64, intersection.y + (-ty * (effective_dist + offset)).round() as i64);

    let mut pts = [point_a, point_b, Point64::new(0, 0), point_d, Point64::new(0, 0)];
    if !compute_anchor_points(settings, anchor, &mut pts) {
        return None;
    }

    // "For off-center track connections, the convex hull produces asymmetric anchor points (C and E at different distances from the track axis). Recompute
    // them to be symmetric so the teardrop flares out evenly from the track on both sides." (`if( IsRound( aOther, layer ) )`.)
    let (pvx, pvy) = (-ty, tx);
    let (pox, poy) = ((c.x - intersection.x) as f64, (c.y - intersection.y) as f64);
    let perp_dist_to_center = pox * pvx + poy * pvy;
    if anchor.is_round() && perp_dist_to_center.abs() > radius as f64 * 0.1 {
        let d = perp_dist_to_center.abs();
        if d < radius as f64 {
            let max_symmetric = radius as f64 - d;
            let preferred_width = (w as f64 * settings.best_width_ratio).round() as i64;
            let mut max_half_width = preferred_width / 2;
            if settings.max_width_nm.get() > 0 {
                max_half_width = max_half_width.min(settings.max_width_nm.get() / 2);
            }
            let sym_half_width = max_symmetric.min(max_half_width as f64);
            if sym_half_width > track_halfwidth as f64 {
                let int_pt = (intersection.x as f64, intersection.y as f64);
                // `findCircleLineIntersection`: the circle's crossing with the axis' parallel at `perp_dist`, the one nearer the track's entry.
                let find = |perp_dist: f64| -> Point64 {
                    let proj = pox * tx + poy * ty;
                    let origin = (int_pt.0 + tx * proj + pvx * perp_dist, int_pt.1 + ty * proj + pvy * perp_dist);
                    let oc = (origin.0 - c.x as f64, origin.1 - c.y as f64);
                    let b = oc.0 * tx + oc.1 * ty;
                    let cc = oc.0 * oc.0 + oc.1 * oc.1 - r * r;
                    let disc = b * b - cc;
                    if disc < 0.0 {
                        return Point64::new(origin.0.round() as i64, origin.1.round() as i64);
                    }
                    let sq = disc.sqrt();
                    let p1 = (origin.0 + tx * (-b - sq), origin.1 + ty * (-b - sq));
                    let p2 = (origin.0 + tx * (-b + sq), origin.1 + ty * (-b + sq));
                    let dist = |p: (f64, f64)| (p.0 - int_pt.0).hypot(p.1 - int_pt.1);
                    let p = if dist(p1) < dist(p2) { p1 } else { p2 };
                    Point64::new(p.0.round() as i64, p.1.round() as i64)
                };
                pts[2] = find(-sym_half_width);
                pts[4] = find(sym_half_width);
            }
        }
    }
    Some(pts.to_vec())
}

/// `TEARDROP_MANAGER::UpdateTeardrops`' work for one track item and one anchor it is connected to (25ec2335, the pad and the via loops alike): the width
/// filters ("track width must be < teardrop width and filter width. A max width of 0 means no limit"), the ends hit-tested ("The track is entirely inside the
/// pad"), `computeChordThroughShape`'s graze filter for a track with one end inside ("Reject tangential grazes, but keep short radial entries"), then the
/// teardrop -- or, for a track crossing the anchor's centre with neither end inside, one teardrop per side, each on a stub from one of the track's ends to
/// the point of the track nearest the centre (`buildCrossingStub`; round 7 R7-1).
fn teardrops_for(anchor: &Anchor, item: &TrackItem, settings: &TeardropSettings) -> Vec<Vec<Point64>> {
    let (w, tw) = (anchor.width as f64, item.width);
    let max_width = settings.max_width_nm.get();
    if (max_width > 0 && tw >= max_width) || tw as f64 >= w * settings.best_width_ratio || tw as f64 >= w * settings.width_to_size_filter_ratio {
        return Vec::new();
    }
    let (start_hits, end_hits) = (anchor.hit_test(item.start), anchor.hit_test(item.end));
    if start_hits && end_hits {
        return Vec::new();
    }
    // Arcs are exempt ("Arcs are genuine entries, not the short straight grazes this filter targets").
    if start_hits != end_hits && item.arc.is_none() && chord_through_shape(&anchor.outline, item.start, item.end, if start_hits { item.start } else { item.end }) < tw {
        return Vec::new();
    }
    if !start_hits && !end_hits && item.hit_test(anchor.center) {
        // "A track crossing the pad earns one teardrop per side, not one over the whole track." The stub is a straight `PCB_TRACK` of the track's width, aimed
        // along its centreline at the crossed centre.
        let crossed = item.nearest_point(anchor.center);
        return [item.end, item.start]
            .into_iter()
            .filter_map(|from| compute_teardrop_polygon(anchor, &TrackItem { start: from, end: crossed, arc: None, width: tw }, settings))
            .collect();
    }
    compute_teardrop_polygon(anchor, item, settings).into_iter().collect()
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
            for item in TrackItem::of(t) {
                for outline in teardrops_for(anchor, &item, settings) {
                    out.push(Zone { id: String::new(), net: anchor.net.clone(), layer: t.layer.clone(), outline: outline.into_iter().map(pt).collect(), teardrop: true, ..Default::default() });
                }
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
    fn a_track_ending_inside_the_via_is_skipped() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + mm(3.0), y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        // `UpdateTeardrops`' "The track is entirely inside the via": this 50 um track ends inside the 0.8 mm via. (A track longer than the via's radius but
        // shorter than the requested 400 um gets a shorter teardrop, `actualTdLen`: `a_short_track_gets_a_clipped_teardrop`.)
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, Point { x: via_at.x + mm(0.05), y: via_at.y }], arc_mid_offset: None });

        assert!(generate_teardrops(&design, &model, &enabled_settings()).is_empty());
    }

    /// Round 6 D6-1, 25ec2335's `UpdateTeardrops`: a track leaving the via by less than its own width is a radial entry (the chord on its extended centreline
    /// is the via's diameter), not a graze, and gets its teardrop; a Max width of 0 is "no limit, not nothing fits".
    #[test]
    fn a_short_radial_track_and_a_max_width_of_zero_still_get_teardrops() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + mm(3.0), y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, Point { x: via_at.x + mm(0.35), y: via_at.y }], arc_mid_offset: None });
        assert_eq!(generate_teardrops(&design, &model, &enabled_settings()).len(), 1, "0.05 mm out of the via, under the track's 0.2 mm width");
        let no_limit = TeardropSettings { enabled: true, max_width_nm: Nm::ZERO, ..Default::default() };
        assert_eq!(generate_teardrops(&design, &model, &no_limit).len(), 1, "max width 0 = no limit");
    }

    /// `computeChordThroughShape`: the copper span of the extended centreline that holds the inside end; arcs and a line missing the shape answer "no limit".
    #[test]
    fn the_chord_through_a_round_anchor_is_measured_on_the_extended_centreline() {
        let circle = circle_polygon(Point64::new(0, 0), 300_000, MAX_ERROR, ErrorLoc::Inside, 16);
        // Through the centre: the inscribed polygon's width along x, within maxError of the diameter (ERROR_INSIDE).
        let d = chord_through_shape(&circle, Point64::new(0, 0), Point64::new(350_000, 0), Point64::new(0, 0));
        assert!((600_000 - 2 * MAX_ERROR..=600_000).contains(&d), "{d}");
        // A graze: a short track whose inside end is near the rim, off the centre line.
        let g = chord_through_shape(&circle, Point64::new(-50_000, 290_000), Point64::new(50_000, 290_000), Point64::new(0, 290_000));
        assert!(g < 200_000, "{g}");
        assert_eq!(chord_through_shape(&circle, Point64::new(5, 5), Point64::new(5, 5), Point64::new(5, 5)), i64::MAX, "a zero-length track");
        // A rectangle's polygon: the span across it along the track (a 1.5 x 1 mm pad entered along its length).
        let rect = round_chamfered_rect_polygon((1_500_000, 1_000_000), 0, 0.0, 0, MAX_ERROR);
        assert_eq!(chord_through_shape(&rect, Point64::new(0, 0), Point64::new(5_000_000, 0), Point64::new(0, 0)), 1_500_000);
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

    /// The board point (x, y) in mm.
    fn at(x: f64, y: f64) -> Point {
        Point { x: mm(x), y: mm(y) }
    }

    /// A kicad-cli outline, nanometres.
    fn outline(pts: &[(i64, i64)]) -> Vec<Point> {
        pts.iter().map(|&(x, y)| Point { x: Nm::new(x), y: Nm::new(y) }).collect()
    }

    /// `two_pad_model` with R1 an SMD pad of `shape` and `size` (mm) at (100, 145) mm, for the rectangle cases kicad-cli was run on.
    fn smd_pad_model(shape: PadShape, size: (f64, f64), roundrect_ratio: Option<f64>) -> (Design, ConstraintModel) {
        let (mut design, mut model) = two_pad_model();
        model.footprints.push(Footprint {
            name: "SMDPAD".into(),
            pads: vec![Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (mm(size.0), mm(size.1)), shape, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio }],
            courtyard: None,
            model: None,
            courtyard_outlines: vec![],
            models3d: vec![],
            art: Default::default(),
        });
        model.parts[0].footprint = Some("SMDPAD".into());
        model.parts[0].package = Some("SMDPAD".into());
        design.placement.as_mut().unwrap().footprints[0].at = at(100.0, 145.0);
        (design, model)
    }

    fn track(pts: Vec<Point>, width: f64) -> Track {
        Track { id: String::new(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(width), pts, arc_mid_offset: None }
    }

    fn via(x: f64, y: f64, d: f64) -> Via {
        Via { id: String::new(), net: "N1".into(), at: at(x, y), drill: mm(0.3), diameter: mm(d), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }
    }

    /// UI-LEFT P1: `m_UseRoundShapesOnly` is off by default, so a rectangular SMD pad gets its teardrop, clipped to the pad's smaller side
    /// (`computeAnchorPoints`' `force_clip`). The outline is kicad-cli's own (`crates/kicad/tests/teardrops_kicad_cli.rs`, the first pad case).
    #[test]
    fn a_rectangular_smd_pad_gets_kicad_clis_teardrop() {
        let (mut design, model) = smd_pad_model(PadShape::Rect, (1.5, 1.0), None);
        design.routing.as_mut().unwrap().tracks.push(track(vec![at(100.0, 145.0), at(105.0, 145.0)], 0.25));
        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].outline, outline(&[(101250000, 145125000), (101250000, 144875000), (100750000, 144500000), (99999000, 145000000), (100750000, 145500000)]));
        // "SMD pads" off: none.
        let off = TeardropSettings { enabled: true, target_smd_pads: false, ..Default::default() };
        assert!(generate_teardrops(&design, &model, &off).is_empty());
    }

    /// A track crossing a rounded rectangle's centre gets one teardrop on each side (`buildCrossingStub`), as kicad-cli makes them.
    #[test]
    fn a_track_crossing_a_rounded_rectangle_gets_a_teardrop_on_each_side() {
        let (mut design, model) = smd_pad_model(PadShape::RoundRect, (1.5, 1.0), Some(0.25));
        design.routing.as_mut().unwrap().tracks.push(track(vec![at(97.0, 145.0), at(103.0, 145.0)], 0.2));
        let mut out: Vec<_> = generate_teardrops(&design, &model, &enabled_settings()).into_iter().map(|z| z.outline).collect();
        out.sort_by_key(|o| o[0].x);
        assert_eq!(
            out,
            vec![
                outline(&[(98750000, 144900000), (98750000, 145100000), (99404329, 145480970), (100001000, 145000000), (99404329, 144519030)]),
                outline(&[(101250000, 145100000), (101250000, 144900000), (100595671, 144519030), (99999000, 145000000), (100595671, 145480970)]),
            ]
        );
    }

    /// Round 7 R7-1: `UpdateTeardrops` hit-tests the track's ends against the via (`via->HitTest( track->GetStart() )`), so a track starting 0.1 mm off a
    /// 0.8 mm via's centre gets the teardrop kicad-cli gives it (the finding's case; C and E made symmetric about the track's axis).
    #[test]
    fn a_track_ending_off_centre_inside_the_via_gets_its_teardrop() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(via(100.0, 100.0, 0.8));
        rt.tracks.push(track(vec![at(100.1, 100.0), at(105.0, 100.0)], 0.2));
        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].outline, outline(&[(100796578, 100100000), (100796578, 99900000), (100153073, 99630448), (99995578, 100000000), (100153073, 100369552)]));
    }

    /// Round 7 R7-1: a track running through a via ("a track that merely crosses the via earns a teardrop on each side") gets two, kicad-cli's.
    #[test]
    fn a_track_crossing_the_via_gets_one_teardrop_on_each_side() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(via(120.0, 100.0, 0.8));
        rt.tracks.push(track(vec![at(115.0, 100.0), at(125.0, 100.0)], 0.2));
        let mut out: Vec<_> = generate_teardrops(&design, &model, &enabled_settings()).into_iter().map(|z| z.outline).collect();
        out.sort_by_key(|o| o[0].x);
        assert_eq!(
            out,
            vec![
                outline(&[(119203422, 99900000), (119203422, 100100000), (119846927, 100369552), (120004422, 100000000), (119846927, 99630448)]),
                outline(&[(120796578, 100100000), (120796578, 99900000), (120153073, 99630448), (119995578, 100000000), (120153073, 100369552)]),
            ]
        );
    }

    /// A polyline is a `(segment ..)` per pair of points: one bending on a via is two segments that each end on it, and each gets its teardrop (kicad-cli's).
    #[test]
    fn a_polyline_bending_on_a_via_gets_a_teardrop_for_each_segment() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(via(150.0, 100.0, 0.8));
        rt.tracks.push(track(vec![at(150.0, 95.0), at(150.0, 100.0), at(155.0, 100.0)], 0.2));
        let mut out: Vec<_> = generate_teardrops(&design, &model, &enabled_settings()).into_iter().map(|z| z.outline).collect();
        out.sort_by_key(|o| o[0].x);
        assert_eq!(
            out,
            vec![
                outline(&[(150100000, 99203422), (149900000, 99203422), (149630448, 99846927), (150000000, 100004422), (150369552, 99846927)]),
                outline(&[(150796578, 100100000), (150796578, 99900000), (150153073, 99630448), (149995578, 100000000), (150153073, 100369552)]),
            ]
        );
    }

    /// A custom pad is skipped (`MergePrimitivesAsPolygon` is not ported).
    #[test]
    fn a_custom_pad_gets_no_teardrop() {
        let (mut design, model) = smd_pad_model(PadShape::Custom, (1.0, 1.0), None);
        design.routing.as_mut().unwrap().tracks.push(track(vec![at(100.0, 145.0), at(105.0, 145.0)], 0.2));
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
    #[test]
    fn a_short_track_gets_a_clipped_teardrop() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x + mm(3.0), y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: via_at, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        // 0.65 mm long: 0.25 mm past the via's edge (more than its 0.2 mm width), less than the requested 0.4 mm, so the
        // teardrop ends at the track's end (`actualTdLen`).
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, Point { x: via_at.x + mm(0.65), y: via_at.y }], arc_mid_offset: None });
        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        let max_x = out[0].outline.iter().map(|p| p.x).max().unwrap();
        assert!(max_x <= via_at.x + mm(0.65), "the teardrop stops at the track's end: {:?}", out[0].outline);
    }

    /// Round 5's A5-1: a 0.6 mm via at (100, 100) mm and a 0.2 mm arc track from it through (101, 100.4) to (102, 100). The arc's first tessellation step
    /// ends inside the via; KiCad's `findAnchorPointsOnTrack` works on the arc itself and makes a teardrop, whose sharp end lies on the arc.
    #[test]
    fn an_arc_track_gets_a_teardrop_along_the_arc() {
        let (mut design, model) = two_pad_model();
        let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let (s, m, e) = (p(100.0, 100.0), p(101.0, 100.4), p(102.0, 100.0));
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(Via { id: "v1".into(), net: "N1".into(), at: s, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
        let pts = eda_model::ir::tessellate_arc(s, m, e, eda_model::ir::TRACK_ARC_SEGMENTS);
        rt.tracks.push(Track { id: "t1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts, arc_mid_offset: Some(Point { x: m.x - s.x, y: m.y - s.y }) });
        assert!(rt.tracks[0].arc().is_some());

        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        let o = &out[0].outline;
        assert_eq!(o.len(), 5);
        // (D is behind the crossing along the reference segment's direction, `vecVia = vecT`, so for an arc it is not exactly behind the centre.)
        // A and B straddle the arc 0.3 mm (0.6 * 0.5) from where it leaves the via: their middle is within a micrometre of the circle through the arc.
        let (a, b) = (o[0], o[1]);
        let mid = ((a.x.get() + b.x.get()) as f64 / 2.0, (a.y.get() + b.y.get()) as f64 / 2.0);
        // The arc's centre: (101 mm, y) with (1 mm)^2 + (y - 100)^2 = (y - 100.4)^2, y = 98.95 mm, radius 1.45 mm.
        let r = (mid.0 - 101e6).hypot(mid.1 - 98.95e6);
        assert!((r - 1.45e6).abs() < 3_000.0, "the sharp end is on the arc: r = {r}, outline {o:?}");
        assert!(mid.1 > 100.0e6 + 50_000.0, "it follows the arc's bend, not the chord: {mid:?}");
    }

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
