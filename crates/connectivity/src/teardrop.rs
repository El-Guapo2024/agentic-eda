//! Port of `pcbnew/teardrop/*` (task item 4; KiCad 25ec2335, the box's kicad-cli, throughout): `TEARDROP_MANAGER`'s polygon builders and its two passes,
//! `UpdateTeardrops` (with `AddTeardropsOnTracks`) and `RemoveTeardrops`, over this model's IR rather than a live `BOARD`. Settings: `eda_model::ir::
//! TeardropSettings` (Board Setup > Teardrops) and each item's own `eda_model::teardrop_params`. Storage: a teardrop is a `Zone` with `teardrop: true` (KiCad's
//! teardrops are zones too), made by [`generate_teardrops`] (a full update: `Cmd::AddAllTeardrops`) or by [`update_after_edit`] (the update every board edit's
//! commit runs, `BOARD_COMMIT::Push`: `crates/ops/src/teardrop_upkeep.rs`).
//!
//! ## What is built, and how
//!
//! - **Vias and every pad shape** (round 7, UI-LEFT P1; UI-LEFT-2 PP7). A via and a pad `TEARDROP_MANAGER::IsRound` calls round (a circle, or an oval as wide as
//!   it is high) get KiCad's construction for a round anchor (`Manager::compute_teardrop_polygon`; round 4's C-1). A rectangle, a rounded or chamfered
//!   rectangle, a trapezoid, a long oval and a custom pad get the construction for the others, as `m_UseRoundShapesOnly` is off by default
//!   (`TEARDROP_PARAMETERS_LIST()`): the pad's own polygon (`PAD::TransformShapeToPolygon`, `pad_polygon`; a custom pad's `MergePrimitivesAsPolygon`, within
//!   `maxError` of KiCad's) clipped to the teardrop's width (`computeAnchorPoints`' `force_clip`), A and B refused inside the pad, D kept inside the copper the
//!   track enters (a custom pad's at its anchor's radius). **A padstack's every layer**: a pad (and a via) is its shape on the track's layer (`GetWidth( aItem,
//!   aLayer )`, `IsRound( aItem, aLayer )`, `PADSTACK::ForEachUniqueLayer`).
//! - **The anchors a track meets, found as `UpdateTeardrops` finds them** (round 7 R7-1). Each straight segment of a track's polyline is a `PCB_TRACK` of its
//!   own (the writer writes each as a `(segment ..)`). The anchors of a track are the vias and pads of its net whose copper on its layer touches it (KiCad's
//!   `GetConnectedPadsAndVias`, whose items' copper collides); each of the track's ends is hit-tested against the anchor (`PCB_VIA::HitTest`: the circle;
//!   `PAD::HitTest`: the pad's polygon): an end inside off the centre gets its teardrop, a track crossing the anchor's centre gets one on each side
//!   (`buildCrossingStub`), a track that touches it otherwise goes through `findAnchorPointsOnTrack` as in KiCad.
//! - **Arc tracks** (`PCB_ARC_T`, round 5's A5-1) follow the real arc, with 25ec2335's two refinements (the polyline's points moved onto the arc, the corners
//!   kept `maxError` inside an arc's edge).
//! - **Fitted to other nets' clearance** (UI-LEFT-2 PP6: `computeFittedTeardropPolygon`, `collidesWithOtherNets`, `pairClearance`). A teardrop that would come
//!   within clearance of another net's track, via, pad, graphic on copper or teardrop (one already made included: "nothing keeps two teardrops out of the same
//!   gap") is narrowed by bisection between the track's width and its own, or dropped when not even the track's width fits.
//! - **Curved edges** (`m_CurvedEdges`: `computeCurvedForRoundShape`, `computeCurvedForRectShape` with the rounded rectangle's and the oval's tangent control
//!   points) and **two segments** (`m_AllowUseTwoTracks`: `findTouchingTrack`, a first segment too short lends the teardrop the next one's length, the bend at
//!   most 60 degrees, the junction's points on both sides) (UI-LEFT-2 PP7).
//! - **Track to track** (`AddTeardropsOnTracks`, `TARGET_TRACK`'s block: [`TeardropSettings::track_end`]): a narrow track ending on a wider one of its net and
//!   layer, the teardrop's zone a `TD_TRACKEND` one (`Zone::teardrop_track_end`).
//! - **Solder-mask teardrops** (`createAndAddTeardropWithMask`, `createTeardropMask`): a track with a mask opening on an outer layer (`eda_model::track_mask`)
//!   gets a teardrop on F.Mask or B.Mask too, grown by the track's mask expansion.
//! - **Each anchor's own parameters** (UI-LEFT P3, `task-u5-pcbfp`): `via->GetTeardropParams()`, `pad->GetTeardropParams()` govern an anchor that has them;
//!   one without them is Board Setup's ([`TeardropSettings`], which stands for the Edit Teardrops dialog: its enable and its target kinds).
//! - **Kept current as the board is edited** (UI-LEFT-2 PP6: [`update_after_edit`], `BOARD_COMMIT::Push` and `RemoveTeardrops`): the teardrops an edit makes
//!   stale are made again, the others left as they are.
//!
//! All of it is proven against kicad-cli's own teardrops by `crates/kicad/tests/teardrops_kicad_cli.rs` (63 outlines).
//!
//! ## What differs
//!
//! - `areItemsInSameZone` (`m_TdOnPadsInZones` off, "Prefer zone connections": no teardrop on a pad a same-net pour joins to the track) is not asked: the fills
//!   are kicad-cli's, kept beside `design.json`, not part of this crate's input. Every pad gets its teardrop as if the option were off.
//! - `pairClearance` is the net classes' clearance and the board's minimum (the rule engine's fast path); a custom rule or an item's own clearance is not asked.
//!   The copper index leaves out texts on copper and footprints' own graphics on copper.
//! - `PAD::HitTest( aPosition, 0 )` asks the pad's copper only (KiCad's asks its hole too, which a plated pad's copper covers).
//! - A track without a net gets no teardrop (KiCad stands geometry in for net 0).
//! - The teardrops' priorities (`setTeardropPriorities`) are given by the writer (`eda_kicad::pcb::teardrop_priorities`), and their ids by the routing.

use eda_drc::kimath::Shape;
use eda_model::footprint::to_board;
use eda_model::ir::{Design, FootprintInstance, Point, Side, TeardropSettings, Nm, Zone};
use eda_model::teardrop_params::TeardropParams;
use eda_model::{ConstraintModel, Pad, PadKind, PadShape};
use eda_clipper2::Point64;
use eda_shape_poly_set::basic_shapes::{circle_polygon, oval_polygon, round_chamfered_rect_polygon, trapezoid_polygon, ErrorLoc};
use eda_shape_poly_set::hatch::point_inside;
use eda_shape_poly_set::ShapePolySet;
use std::collections::{HashMap, HashSet};

/// Which item an anchor is, across two states of the board (what an edit's commit names, `BOARD_COMMIT::Push`'s stale lists): a via by its id (or its place in the
/// routing when it has none), a pad by its footprint and its place in the footprint's pads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnchorKey {
    Via(String),
    Pad(String, usize),
}

/// The geometry of a via or pad on one copper layer (`GetWidth( aOther, aLayer )`, `IsRound( aOther, aLayer )`, `TransformShapeToPolygon( .., aLayer, .. )`).
#[derive(Debug, Clone, PartialEq)]
struct Geo {
    /// `TEARDROP_MANAGER::GetWidth`: a via's diameter on the layer (`via->GetWidth( aLayer )`), a pad's smaller size there (`std::min( pad->GetSize( aLayer ).x, .y )`).
    width: i64,
    /// `TEARDROP_MANAGER::IsRound`: a via, or a pad whose shape on the layer is a circle or an oval as wide as it is high.
    round: bool,
    /// The pad's copper on the layer (`PAD::TransformShapeToPolygon( .., aLayer, 0, maxError, ERROR_INSIDE )`): each polygon its outline and then its holes, the
    /// first polygon's outline being what `findAnchorPointsOnTrack` and `computeAnchorPoints` take (`Outline( 0 )`). Empty for a via (its circle is made where it
    /// is used).
    polys: Vec<Vec<Vec<Point64>>>,
    /// What `computeCurvedForRectShape` asks of a pad that is not round: its shape on the layer and its corner radius, its size and `GetOrientation()`.
    rect: RectInfo,
    /// `PAD_SHAPE::CUSTOM` on the layer.
    custom: bool,
}

/// See [`Geo::rect`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct RectInfo {
    /// `PAD_SHAPE::ROUNDRECT`'s `GetRoundRectCornerRadius( aLayer )` (a chamfered rectangle is not one).
    round_rect_radius: Option<i64>,
    oval: bool,
    size: (i64, i64),
    orientation: f64,
}

/// What `TEARDROP_MANAGER` reads of a via or pad a teardrop is built on (`aOther` / `aCandidate`).
#[derive(Debug, Clone, PartialEq)]
struct Anchor {
    key: AnchorKey,
    /// `GetPosition()`: a via's centre, a pad's position (its shape offset not added).
    center: Point64,
    net: String,
    kind: AnchorKind,
    /// The copper layers it is on, each group with its geometry there (a via's span, a pad's padstack: `PADSTACK::ForEachUniqueLayer`).
    groups: Vec<(Vec<String>, Geo)>,
    /// `pad->GetTeardropParams()` / `via->GetTeardropParams()`: the item's own parameters where it has them, else Board Setup's block when it targets the item;
    /// `None` when they are not enabled (`if( !tdParams.m_Enabled ) continue;`).
    params: Option<TeardropParams>,
}

#[derive(Debug, Clone, PartialEq)]
enum AnchorKind {
    /// `PCB_VIA`: hit by a point within half its front diameter (`PCB_VIA::HitTest`, `GetWidth( PADSTACK::ALL_LAYERS ) / 2`).
    Via { front_width: i64 },
    /// `PAD`: `bounding_radius` is `PAD::GetBoundingRadius()`.
    Pad { bounding_radius: i64 },
}

impl Anchor {
    fn geo(&self, layer: &str) -> Option<&Geo> {
        self.groups.iter().find(|(l, _)| l.iter().any(|x| x == layer)).map(|(_, g)| g)
    }

    fn is_pad(&self) -> bool {
        matches!(self.kind, AnchorKind::Pad { .. })
    }

    /// `PCB_VIA::HitTest( aPosition, 0 )`: `rel_pos.x² + rel_pos.y² <= max_dist²` in doubles; `PAD::HitTest( aPosition, 0, aLayer )`: within the bounding radius,
    /// then the layer's `GetEffectivePolygon( aLayer, ERROR_INSIDE )->Contains( aPosition )`. `layer` `None` is `PAD::HitTest( aPosition, 0 )`: any of the pad's
    /// layers (`Padstack().ForEachUniqueLayer`; the hole shape is not asked, see the module doc).
    fn hit_test(&self, p: Point64, layer: Option<&str>) -> bool {
        let (dx, dy) = (p.x - self.center.x, p.y - self.center.y);
        match &self.kind {
            AnchorKind::Via { front_width } => {
                let max_dist = (front_width / 2) as f64;
                (dx as f64) * (dx as f64) + (dy as f64) * (dy as f64) <= max_dist * max_dist
            }
            AnchorKind::Pad { bounding_radius } => {
                let d2 = dx as i128 * dx as i128 + dy as i128 * dy as i128;
                if d2 > *bounding_radius as i128 * *bounding_radius as i128 {
                    return false;
                }
                match layer {
                    Some(l) => self.geo(l).is_some_and(|g| g.contains(p)),
                    None => self.groups.iter().any(|(_, g)| g.contains(p)),
                }
            }
        }
    }

    /// The anchor's copper on `layer` as a DRC shape, for the touch test that stands for KiCad's connectivity (`GetConnectedPadsAndVias`) and for the
    /// collision test of [`Manager::collides_with_other_nets`].
    fn shape_on(&self, layer: &str) -> Option<Shape> {
        let g = self.geo(layer)?;
        Some(match &self.kind {
            AnchorKind::Via { .. } => Shape::Circle { c: pt(self.center), r: Nm::new(g.width / 2) },
            AnchorKind::Pad { .. } => {
                // The outlines (a hole in a custom pad's copper is not left out: the shape is a little larger than the copper there).
                let mut parts: Vec<Shape> = g.polys.iter().filter_map(|p| p.first()).filter(|c| c.len() >= 3).map(|c| Shape::Polygon { pts: c.iter().map(|&p| pt(p)).collect() }).collect();
                match parts.len() {
                    0 => return None,
                    1 => parts.remove(0),
                    _ => Shape::Compound { parts },
                }
            }
        })
    }
}

impl Geo {
    /// Every outline and hole.
    fn contours(&self) -> impl Iterator<Item = &Vec<Point64>> {
        self.polys.iter().flatten()
    }

    /// `SHAPE_POLY_SET::Contains`: inside a polygon's outline and none of its holes.
    fn contains(&self, p: Point64) -> bool {
        self.polys.iter().any(|poly| poly.first().is_some_and(|o| o.len() >= 3 && point_inside(o, p)) && !poly[1..].iter().any(|h| h.len() >= 3 && point_inside(h, p)))
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

/// `PAD::TransformShapeToPolygon( aBuffer, aLayer, 0, maxError, ERROR_INSIDE )` (`pcbnew/pad.cpp`) of the pad `lp` of `fp` (with the shape it has on the layer
/// laid on it, `PadLayerShape::lay_on`), whose position on the board is `center`: a circle (and an oval as wide as it is high) `TransformCircleToPolygon` of
/// half the width at `ShapePos`, a longer oval `TransformOvalToPolygon` between its ends turned by the orientation, a rectangle and a trapezoid
/// `TransformTrapezoidToPolygon`, a rounded or chamfered rectangle `TransformRoundChamferedRectToPolygon` (`PAD_MIN_SEG_PER_CIRCLE_COUNT` 16), these built about
/// the origin, turned by the pad's orientation and moved to `ShapePos` (the position plus the turned shape offset). A custom pad is `MergePrimitivesAsPolygon`
/// (its anchor shape and its primitives merged, turned and moved) as `eda_drc::pad_shape::pad_polygons` makes it: the union of the parts the DRC measures the
/// pad by. **Differs:** its circles and arcs are cut into segments by that module, not by `TransformCircleToPolygon` and `PCB_SHAPE::TransformShapeToPolygon`, so
/// a custom pad's polygon is within `maxError` of KiCad's, not point for point. Each polygon of the result is its outline and its holes.
fn pad_polygon(fp: &FootprintInstance, lp: &Pad, center: Point64) -> Vec<Vec<Vec<Point64>>> {
    let (orientation, mirrored) = kicad_pad_frame(fp, lp);
    let turn = |p: Point64| pad_frame_turn(orientation, mirrored, p);
    let (w, h) = (lp.size.0.get(), lp.size.1.get());
    let (dx, dy) = (w / 2, h / 2);
    let off = lp.ext.offset.map_or(Point64::new(0, 0), |(x, y)| turn(Point64::new(x.get(), y.get())));
    let shape_pos = Point64::new(center.x + off.x, center.y + off.y);
    let one = |c: Vec<Point64>| vec![vec![c]];
    let local = match lp.shape {
        // "Note: dx == dy is not guaranteed for circle pads in legacy boards".
        PadShape::Circle => return one(circle_polygon(shape_pos, dx, MAX_ERROR, ErrorLoc::Inside, 16)),
        PadShape::Oval if dx == dy => return one(circle_polygon(shape_pos, dx, MAX_ERROR, ErrorLoc::Inside, 16)),
        PadShape::Oval => {
            // `VECTOR2I delta( dx - half_width, dy - half_width ); RotatePoint( delta, GetOrientation() );` (an oval is its own mirror image).
            let half_width = dx.min(dy);
            let delta = eda_shape_poly_set::basic_shapes::rotate_point_degrees(Point64::new(dx - half_width, dy - half_width), orientation);
            let (a, b) = (Point64::new(shape_pos.x - delta.x, shape_pos.y - delta.y), Point64::new(shape_pos.x + delta.x, shape_pos.y + delta.y));
            return one(oval_polygon(a, b, half_width * 2, MAX_ERROR, ErrorLoc::Inside, 16));
        }
        PadShape::Rect => trapezoid_polygon((w, h), 0, 0, MAX_ERROR),
        PadShape::Trapezoid => {
            let d = lp.ext.trapezoid_delta.unwrap_or((Nm::ZERO, Nm::ZERO));
            trapezoid_polygon((w, h), d.0.get() / 2, d.1.get() / 2, MAX_ERROR)
        }
        PadShape::RoundRect => round_chamfered_rect_polygon((w, h), eda_model::pad_geom::round_rect_radius(lp).get(), 0.0, 0, MAX_ERROR),
        PadShape::ChamferedRect => round_chamfered_rect_polygon((w, h), eda_model::pad_geom::round_rect_radius(lp).get(), lp.ext.chamfer_ratio.unwrap_or(0.0), lp.ext.chamfer_corners.bits(), MAX_ERROR),
        PadShape::Custom => {
            let set = eda_drc::pad_shape::pad_polygons(fp, lp, pt(center), Nm::new(MAX_ERROR));
            return set.polys.into_iter().map(|poly| poly.into_iter().collect()).collect();
        }
    };
    one(local
        .into_iter()
        .map(|p| {
            let q = turn(p);
            Point64::new(q.x + shape_pos.x, q.y + shape_pos.y)
        })
        .collect())
}

/// `PAD::GetBoundingRadius()` (`BuildEffectivePolygon( ERROR_OUTSIDE )` over every layer): the farthest vertex of the pad's `ERROR_OUTSIDE` polygons from its
/// position, and half the drill. Here the `ERROR_INSIDE` polygons' farthest vertex, grown by `maxError` when the pad has curves (a deliberate approximation: the
/// radius only bounds `PAD::HitTest` and sets how far `computeTeardropPolygon` casts its ray into the pad, which reaches the far side either way).
fn pad_bounding_radius(lp: &Pad, groups: &[(Vec<String>, Geo)], center: Point64) -> i64 {
    let far = groups.iter().flat_map(|(_, g)| g.contours()).flatten().map(|p| euclidean_norm(p.x - center.x, p.y - center.y)).max().unwrap_or(0);
    let curved = !matches!(lp.shape, PadShape::Rect | PadShape::Trapezoid) && !(lp.shape == PadShape::ChamferedRect && eda_model::pad_geom::round_rect_radius(lp) == Nm::ZERO);
    let drill = lp.drill.map_or(0, |d| d.get()).max(lp.drill_slot.map_or(0, |(a, b)| a.get().max(b.get())));
    (far + if curved { MAX_ERROR } else { 0 }).max((drill as f64 / 2.0).round() as i64)
}

/// The parameters an anchor is built with: its own (`GetTeardropParams()`, which alone govern it, as in `UpdateTeardrops`), else Board Setup's block when the block is
/// on and targets the anchor's kind (`DIALOG_GLOBAL_EDIT_TEARDROPS`, which writes the block into every targeted item). `None`: the anchor gets no teardrop.
fn anchor_params(own: Option<TeardropParams>, board: &TeardropSettings, targeted: bool) -> Option<TeardropParams> {
    match own {
        Some(p) => p.enabled.then_some(p),
        None => (board.enabled && targeted).then(|| TeardropParams::of_settings(board)),
    }
}

/// The board's copper layers from `a` to `b` (either order, inclusive): a via's span.
fn span_layers(board_layers: &[String], a: &str, b: &str) -> Vec<String> {
    let idx = |l: &str| board_layers.iter().position(|x| x == l);
    match (idx(a), idx(b)) {
        (Some(ia), Some(ib)) => board_layers[ia.min(ib)..=ia.max(ib)].to_vec(),
        _ => [a, b].iter().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect(),
    }
}

/// Every via and pad of the board that a teardrop can be built on, in the board's order (the vias as the routing holds them, then each footprint's pads), each
/// with its geometry on each of its copper layers and the parameters it is built with.
fn collect_anchors(design: &Design, model: &ConstraintModel, settings: &TeardropSettings) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    let layers = &model.board.layers;

    if let Some(rt) = &design.routing {
        for (i, v) in rt.vias.iter().enumerate() {
            if v.net.is_empty() {
                continue;
            }
            let extra = design.drawings.as_ref().and_then(|d| d.via_extra(&v.id));
            let own = extra.and_then(|e| e.teardrops());
            let params = anchor_params(own, settings, settings.target_vias);
            // `via->GetWidth( aLayer )`: the padstack's diameter on the layer (`PADSTACK::Size( aLayer )`), the via's own on a normal stack.
            let padstack = extra.and_then(|e| e.props().padstack);
            let mut groups: Vec<(Vec<String>, Geo)> = Vec::new();
            for layer in span_layers(layers, &v.from_layer, &v.to_layer) {
                let width = padstack.as_ref().map_or(v.diameter, |p| p.size_on(&layer, v.diameter)).get();
                match groups.iter_mut().find(|(_, g)| g.width == width) {
                    Some((ls, _)) => ls.push(layer),
                    None => groups.push((vec![layer], Geo { width, round: true, polys: Vec::new(), rect: RectInfo::default(), custom: false })),
                }
            }
            let key = AnchorKey::Via(if v.id.is_empty() { format!("#{i}") } else { v.id.clone() });
            anchors.push(Anchor { key, center: p64(v.at), net: v.net.clone(), kind: AnchorKind::Via { front_width: v.diameter.get() }, groups, params });
        }
    }

    let net_of_pin = net_of_pin_map(model);
    if let Some(pl) = &design.placement {
        for fp in &pl.footprints {
            let Some(part) = model.part(&fp.id) else { continue };
            let Some(footprint) = model.footprint_of(part) else { continue };
            let mut seen: HashMap<&str, u32> = HashMap::new();
            for (pad_idx, lp) in footprint.pads.iter().enumerate() {
                let nth = {
                    let n = seen.entry(lp.number.as_str()).or_insert(0);
                    *n += 1;
                    *n
                };
                if lp.kind == PadKind::NonPlatedHole {
                    continue; // no copper connection to tear-drop at all
                }
                // `DIALOG_GLOBAL_EDIT_TEARDROPS::TransferDataFromWindow`: PTH pads by "PTH pads", SMD and edge connector pads by "SMD pads".
                let targeted = if lp.kind == PadKind::ThroughHole { settings.target_pth_pads } else { settings.target_smd_pads };
                let own = design.pad_teardrops(&fp.id, pad_idx, footprint.pads.len(), &lp.number, nth);
                let params = anchor_params(own, settings, targeted);
                let net = net_of_pin.get(&format!("{}.{}", fp.id, lp.number)).cloned().unwrap_or_default();
                if net.is_empty() {
                    continue;
                }
                let center = p64(to_board(fp, lp.at));
                // The padstack's shapes (`PADSTACK::ForEachUniqueLayer`), each on the board's copper layers it has: an SMD pad's one layer (its footprint's side or
                // the other, `Pad::opposite_side`), a through-hole pad's every layer, a front/inner/back or custom stack a shape per group.
                let mut groups: Vec<(Vec<String>, Geo)> = Vec::new();
                for g in lp.shape_groups(fp.side, layers) {
                    let laid = g.shape.lay_on(lp);
                    let polys = pad_polygon(fp, &laid, center);
                    if polys.is_empty() {
                        continue;
                    }
                    let (w, h) = (laid.size.0.get(), laid.size.1.get());
                    let rect = RectInfo {
                        round_rect_radius: (laid.shape == PadShape::RoundRect).then(|| eda_model::pad_geom::round_rect_radius(&laid).get()),
                        oval: laid.shape == PadShape::Oval,
                        size: (w, h),
                        orientation: kicad_pad_frame(fp, &laid).0,
                    };
                    let round = laid.shape == PadShape::Circle || (laid.shape == PadShape::Oval && laid.size.0 == laid.size.1);
                    groups.push((g.layers, Geo { width: w.min(h), round, polys, rect, custom: laid.shape == PadShape::Custom }));
                }
                if groups.is_empty() {
                    continue;
                }
                let bounding_radius = pad_bounding_radius(lp, &groups, center);
                anchors.push(Anchor { key: AnchorKey::Pad(fp.id.clone(), pad_idx), center, net, kind: AnchorKind::Pad { bounding_radius }, groups, params });
            }
        }
    }

    anchors
}

/// Whether anything on the board can have a teardrop or has one: a teardrop zone, Board Setup's block on (or its track-to-track block), or a via's or pad's own
/// parameters enabled. A board with none of these has nothing for [`update_after_edit`] to do.
pub fn teardrops_possible(design: &Design) -> bool {
    let Some(rt) = &design.routing else { return false };
    if rt.zones.iter().any(|z| z.teardrop) || rt.teardrop_settings.enabled || rt.teardrop_settings.track_end.enabled {
        return true;
    }
    let Some(d) = &design.drawings else { return false };
    d.via_extras.iter().any(|e| e.teardrops().is_some_and(|p| p.enabled))
        || d.footprint_edits.iter().any(|e| e.pads.iter().any(|p| p.teardrops.is_some_and(|t| t.enabled)))
        || d.footprint_extras.iter().any(|e| e.pads.iter().any(|p| TeardropParams::from_tokens(&p.tokens).is_some_and(|t| t.enabled)))
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
///
/// `shape` is the item's copper on the layer: a round item's circle at `aPos` (`TransformCircleToPolygon( c_buffer, aPos, GetWidth( aItem, aLayer ) / 2,
/// maxError, ERROR_INSIDE, 16 )`), a pad's polygon set (`pad->TransformShapeToPolygon( c_buffer, aLayer, 0, maxError, ERROR_INSIDE )`).
fn compute_anchor_points(params: &TeardropParams, width: i64, round: bool, shape: ShapePolySet, pts: &mut [Point64; 5]) -> bool {
    let preferred_width = (width as f64 * params.best_width_ratio).round() as i64;
    let force_clip = params.best_width_ratio < 1.0 || !round;
    let mut c_buffer = shape;

    let max_width = params.max_width.get();
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
#[derive(Debug, Clone, Copy)]
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


/// `TEARDROP_MANAGER::computeChordThroughShape` (`pcbnew/teardrop/teardrop_utils.cpp`, 25ec2335) for a straight track and the anchor's polygons `polys` (a
/// round anchor's circle, `TransformCircleToPolygon( shapebuffer, pos, radius, maxError, ERROR_INSIDE, 16 )`, or a pad's polygons,
/// `PAD::TransformShapeToPolygon( shapebuffer, aLayer, 0, maxError, ERROR_INSIDE )`, "every contour and hole" crossed): the length of the copper span of the track's centreline, extended both ways
/// by the polygon's box diagonal plus the track's length, that holds `inside`. The crossings are projected on the direction, sorted, and the span between two
/// neighbours whose middle is inside the polygon and that brackets `inside` is the answer. A zero-length track, fewer than two crossings or no such span answer
/// `i64::MAX` ("keep the teardrop"). (The arc's `std::numeric_limits<int>::max()` is the caller's.)
fn chord_through_shape(polys: &[Vec<Vec<Point64>>], start: Point64, end: Point64, inside: Point64) -> i64 {
    let (dx, dy) = ((end.x - start.x) as f64, (end.y - start.y) as f64);
    let len = dx.hypot(dy);
    let all: Vec<Point64> = polys.iter().flatten().flatten().copied().collect();
    if len == 0.0 || all.is_empty() {
        return i64::MAX;
    }
    let (dir_x, dir_y) = (dx / len, dy / len);
    // `VECTOR2I mid = ( start + end ) / 2`: `VECTOR2I::operator/( double )`, `KiROUND` of each half.
    let mid = half_sum(start, end);
    // `shapebuffer.BBox().Diagonal()`: the box's size, `VECTOR2I::EuclideanNorm`.
    let (min_x, max_x) = all.iter().fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
    let (min_y, max_y) = all.iter().fold((i64::MAX, i64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let reach = (euclidean_norm(max_x - min_x, max_y - min_y) as f64 + len).round();
    let off = Point64::new((dir_x * reach).round() as i64, (dir_y * reach).round() as i64);
    let (ext_start, ext_end) = (Point64::new(mid.x - off.x, mid.y - off.y), Point64::new(mid.x + off.x, mid.y + off.y));
    let pts: Vec<Point64> = polys.iter().flatten().flat_map(|c| chain_intersect(c, ext_start, ext_end)).collect();
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
        let contains = polys.iter().any(|poly| poly.first().is_some_and(|o| point_inside(o, span_mid)) && !poly[1..].iter().any(|h| point_inside(h, span_mid)));
        if !contains {
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

/// Which `PCB_TRACK` a `TrackItem` is across two states of the board (what an edit's commit names, `BOARD_COMMIT::Push`'s `staleTeardropTracks`): its copper,
/// net and mask opening. A studio track is a polyline, a run of KiCad's segments of which an edit may change one, so its id is not the segment's.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SegKey {
    layer: String,
    net: String,
    width: i64,
    start: (i64, i64),
    end: (i64, i64),
    mid: Option<(i64, i64)>,
    mask: Option<i64>,
}

/// One `PCB_TRACK` as `UpdateTeardrops` walks `m_board->Tracks()`: a straight segment (`PCB_TRACE_T`), or a track arc (`PCB_ARC_T`) when `arc` is set; or a
/// crossing's stub (`buildCrossingStub`, `key` `None`).
#[derive(Debug, Clone)]
struct TrackItem {
    start: Point64,
    end: Point64,
    arc: Option<TrackArc>,
    width: i64,
    net: String,
    layer: String,
    /// `HasSolderMask()`: the track's solder-mask opening, as its expansion (`GetSolderMaskExpansion()`).
    mask: Option<i64>,
    key: Option<SegKey>,
}

/// `PCB_TRACK::IsPointOnEnds`' `STARTPOINT` and `ENDPOINT`.
const STARTPOINT: u8 = 1;
const ENDPOINT: u8 = 2;

impl TrackItem {
    /// The KiCad items of a track: an arc is one (`Track::arc`, round 5's A5-1), a polyline one segment per pair of points, as the writer writes it
    /// (`crates/kicad/src/pcb.rs`: a `(segment ..)` per pair). A straight "arc" is the segment between its ends. `mask` is the track's solder-mask expansion when it
    /// has an opening.
    fn of(t: &eda_model::ir::Track, mask: Option<i64>) -> Vec<TrackItem> {
        let width = t.width.get();
        let item = |start: Point64, end: Point64, arc: Option<TrackArc>| {
            let key = SegKey { layer: t.layer.clone(), net: t.net.clone(), width, start: (start.x, start.y), end: (end.x, end.y), mid: arc.map(|a| (a.mid.x, a.mid.y)), mask };
            TrackItem { start, end, arc, width, net: t.net.clone(), layer: t.layer.clone(), mask, key: Some(key) }
        };
        if let Some((s, m, e)) = t.arc() {
            return vec![item(p64(s), p64(e), TrackArc::new(s, m, e, t.width))];
        }
        t.pts.windows(2).map(|w| item(p64(w[0]), p64(w[1]), None)).collect()
    }

    /// A straight stub of this track's width, net, layer and mask (`buildCrossingStub`'s `SetLayer`, `SetWidth`, `SetNet`, `SetHasSolderMask`,
    /// `SetLocalSolderMaskMargin`).
    fn stub(&self, start: Point64, end: Point64) -> TrackItem {
        TrackItem { start, end, arc: None, key: None, ..self.clone() }
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

    /// `PCB_TRACK::GetLength()` (`m_Start.Distance( m_End )`) and `PCB_ARC::GetLength()` (`GetRadius() * std::abs( GetAngle().AsRadians() )`).
    fn length(&self) -> f64 {
        match &self.arc {
            Some(arc) => arc.radius * arc.central_angle().abs(),
            None => ((self.end.x - self.start.x) as f64).hypot((self.end.y - self.start.y) as f64),
        }
    }

    /// `PCB_TRACK::IsPointOnEnds( point, min_dist )`: [`STARTPOINT`] and / or [`ENDPOINT`], an end within `min_dist` of the point (`min_dist >= dist`, the
    /// distance a double).
    fn is_point_on_ends(&self, p: Point64, min_dist: i64) -> u8 {
        let dist = |a: Point64| ((p.x - a.x) as f64).hypot((p.y - a.y) as f64);
        let mut result = 0;
        if min_dist == 0 {
            if self.start == p {
                result |= STARTPOINT;
            }
            if self.end == p {
                result |= ENDPOINT;
            }
        } else {
            if min_dist as f64 >= dist(self.start) {
                result |= STARTPOINT;
            }
            if min_dist as f64 >= dist(self.end) {
                result |= ENDPOINT;
            }
        }
        result
    }

    /// The track's copper as a DRC shape (`GetEffectiveShape`): a segment's stadium, an arc's polyline stroked at half its width.
    fn shape(&self) -> Shape {
        let r = Nm::new(self.width / 2);
        match &self.arc {
            None => Shape::Stadium { a: pt(self.start), b: pt(self.end), r },
            Some(arc) => {
                let pts = arc.polyline(MAX_ERROR);
                Shape::Strokes { segs: pts.windows(2).map(|w| eda_drc::kimath::Seg::new(pt(w[0]), pt(w[1]))).collect(), r }
            }
        }
    }
}

/// `PCB_TRACK::GetSolderMaskExpansion()` of a track with its own margin `own` (`m_solderMaskMargin`), else the board's (`m_SolderMaskExpansion`), kept from making
/// the opening narrower than nothing (`margin = std::max( margin, -m_width / 2 )`). **Differs:** a custom rule's `solder_mask_expansion` is not asked.
fn track_mask_expansion(own: Option<Nm>, board: Nm, width: i64) -> i64 {
    let margin = own.unwrap_or(board).get();
    if margin < 0 {
        margin.max(-width / 2)
    } else {
        margin
    }
}

/// `NormalizeVector`: a unit vector along `v`.
fn normalize(v: (i64, i64)) -> (f64, f64) {
    let (x, y) = (v.0 as f64, v.1 as f64);
    let n = x.hypot(y);
    (x / n, y / n)
}

/// `VECTOR2I( KiROUND( x ), KiROUND( y ) )`.
fn kiround_v(x: f64, y: f64) -> Point64 {
    Point64::new(x.round() as i64, y.round() as i64)
}

/// The via, pad or track a teardrop is built on (`BOARD_ITEM* aOther`): a via or pad of [`Manager::anchors`], or the wider track of a track-to-track teardrop.
#[derive(Clone, Copy)]
enum Other<'a> {
    Anchor(usize, &'a Anchor),
    Track(usize, &'a TrackItem),
}

impl<'a> Other<'a> {
    /// `TEARDROP_MANAGER::GetWidth( aItem, aLayer )`.
    fn width(&self, layer: &str) -> i64 {
        match self {
            Other::Anchor(_, a) => a.geo(layer).map_or(0, |g| g.width),
            Other::Track(_, t) => t.width,
        }
    }

    /// `TEARDROP_MANAGER::IsRound( aItem, aLayer )`: a via, a track, or a round pad.
    fn round(&self, layer: &str) -> bool {
        match self {
            Other::Anchor(_, a) => a.geo(layer).is_none_or(|g| g.round),
            Other::Track(..) => true,
        }
    }

    /// The pad's geometry on `layer`, when `aOther` is a pad that is not round there (`wxCHECK_MSG( aOther->Type() == PCB_PAD_T, .. )`).
    fn pad_geo(&self, layer: &str) -> Option<&'a Geo> {
        match self {
            Other::Anchor(_, a) if a.is_pad() => a.geo(layer).filter(|g| !g.round),
            _ => None,
        }
    }

    /// `aOther->HitTest( aPosition, 0 )`.
    fn hit_test(&self, p: Point64) -> bool {
        match self {
            Other::Anchor(_, a) => a.hit_test(p, None),
            Other::Track(_, t) => t.hit_test(p),
        }
    }

    /// The copper `findAnchorPointsOnTrack`, `computeChordThroughShape` and `computeAnchorPoints` build on: a round item's circle about `pos`
    /// (`TransformCircleToPolygon( .., aOtherPos, GetWidth( aOther, layer ) / 2, maxError, ERROR_INSIDE, 16 )`), a pad's polygons on the layer.
    fn shape_buffer(&self, layer: &str, pos: Point64) -> Vec<Vec<Vec<Point64>>> {
        match self.pad_geo(layer) {
            Some(g) => g.polys.clone(),
            None => vec![vec![circle_polygon(pos, self.width(layer) / 2, MAX_ERROR, ErrorLoc::Inside, 16)]],
        }
    }

    /// The collision test's name for the item (`aExempt`).
    fn obstacle(&self) -> ObsId {
        match self {
            Other::Anchor(i, _) => ObsId::Anchor(*i),
            Other::Track(i, _) => ObsId::Item(*i),
        }
    }
}

/// `TEARDROP_MANAGER::TEARDROP_VARIANT`: on a via or pad (`TD_TYPE_PADVIA`, `TEARDROP_TYPE::TD_VIAPAD`), or at the end of a track on a wider one
/// (`TD_TYPE_TRACKEND`, `TEARDROP_TYPE::TD_TRACKEND`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    PadVia,
    TrackEnd,
}

/// An item of the copper index `collidesWithOtherNets` asks (`m_copperRTree`, `m_createdTdRTree`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObsId {
    Item(usize),
    Anchor(usize),
    Zone(usize),
    Graphic(usize),
    Created(usize),
}

/// One item of the copper index on one layer.
#[derive(Debug, Clone)]
struct Obstacle {
    id: ObsId,
    layer: String,
    net: String,
    shape: Shape,
    bbox: (i64, i64, i64, i64),
}

fn bbox_of(shape: &Shape) -> (i64, i64, i64, i64) {
    let (a, b, c, d) = shape.bbox(Nm::ZERO);
    (a.get(), b.get(), c.get(), d.get())
}

fn boxes_meet(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

fn inflate_box(b: (i64, i64, i64, i64), by: i64) -> (i64, i64, i64, i64) {
    (b.0 - by, b.1 - by, b.2 + by, b.3 + by)
}

fn points_box(pts: &[Point64]) -> (i64, i64, i64, i64) {
    pts.iter().fold((i64::MAX, i64::MAX, i64::MIN, i64::MIN), |b, p| (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y)))
}

/// A spatial index of boxes on a 1 mm grid (standing for KiCad's `DRC_RTREE`s, `m_tracksRTree`, `m_copperRTree`, `m_createdTdRTree`): what may meet a box.
#[derive(Debug, Clone, Default)]
struct Grid {
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// Boxes too large to file cell by cell, asked by every query.
    big: Vec<usize>,
}

const GRID_CELL: i64 = 1_000_000;

impl Grid {
    fn span(b: (i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
        (b.0.div_euclid(GRID_CELL), b.1.div_euclid(GRID_CELL), b.2.div_euclid(GRID_CELL), b.3.div_euclid(GRID_CELL))
    }

    fn insert(&mut self, i: usize, b: (i64, i64, i64, i64)) {
        let (x0, y0, x1, y1) = Self::span(b);
        if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > 4096 {
            self.big.push(i);
            return;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(i);
            }
        }
    }

    /// The indices filed in a cell `b` reaches, each once, in increasing order (the board's order).
    fn query(&self, b: (i64, i64, i64, i64)) -> Vec<usize> {
        let (x0, y0, x1, y1) = Self::span(b);
        let mut out = self.big.clone();
        if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > 4096 {
            out.extend(self.cells.values().flatten().copied());
        } else {
            for x in x0..=x1 {
                for y in y0..=y1 {
                    if let Some(v) = self.cells.get(&(x, y)) {
                        out.extend_from_slice(v);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The board's items a teardrop is made from and measured against, and the parameters of its track-to-track teardrops.
pub struct TeardropWorld {
    anchors: Vec<Anchor>,
    /// Each anchor's copper on each of its layers, with its box: what the touch tests ask, made once.
    anchor_copper: Vec<Vec<(String, Shape, (i64, i64, i64, i64))>>,
    items: Vec<TrackItem>,
    /// Each item's copper and its box (`TrackItem::shape`, made once), and the items by layer in a grid.
    item_copper: Vec<(Shape, (i64, i64, i64, i64))>,
    item_grid: HashMap<String, Grid>,
    /// The anchors of each net.
    anchors_by_net: HashMap<String, Vec<usize>>,
    /// The board's graphics on copper layers (`m_board->Drawings()`, "Graphics, text and dimensions all plot as copper when they sit on a copper layer"), each
    /// with its layer and shape: obstacles of the fit, and copper an edit can move. The third member names the shape for the comparison of two states of the board.
    graphics: Vec<(String, Shape, String)>,
    /// `TARGET_TRACK`'s parameters (`m_prmsList->GetParameters( TARGET_TRACK )`).
    track_params: TeardropParams,
    /// The teardrop zones of the board, as they were.
    zones: Vec<Zone>,
}

/// Each anchor's copper on each of its layers, with its box.
fn copper_of(anchors: &[Anchor]) -> Vec<Vec<(String, Shape, (i64, i64, i64, i64))>> {
    anchors
        .iter()
        .map(|a| a.groups.iter().flat_map(|(layers, _)| layers.iter()).filter_map(|l| a.shape_on(l).map(|s| (l.clone(), bbox_of(&s), s))).map(|(l, b, s)| (l, s, b)).collect())
        .collect()
}

impl TeardropWorld {
    /// The board `design` as `TEARDROP_MANAGER` reads it.
    pub fn of(design: &Design, model: &ConstraintModel) -> TeardropWorld {
        let settings = design.routing.as_ref().map(|r| r.teardrop_settings).unwrap_or_default();
        let anchors = collect_anchors(design, model, &settings);
        let board_mask = model.board.solder_mask.expansion_nm;
        let mut items = Vec::new();
        if let Some(rt) = &design.routing {
            for t in &rt.tracks {
                if t.pts.len() < 2 {
                    continue;
                }
                // `HasSolderMask()` on an outer layer: `(layers "F.Cu" "F.Mask")` (`eda_model::track_mask`).
                let mask = design.drawings.as_ref().and_then(|d| d.track_mask(&t.id)).map(|m| track_mask_expansion(m.margin, board_mask, t.width.get()));
                items.extend(TrackItem::of(t, mask));
            }
        }
        let graphics = design
            .drawings
            .as_ref()
            .map(|d| d.shapes.iter().filter(|s| eda_model::layers::is_copper_layer_name(s.layer())).flat_map(|s| eda_drc::board::effective_shapes(s).into_iter().map(move |sh| (s.layer().to_string(), format!("{sh:?}"), sh)).map(|(l, k, sh)| (l, sh, k))).collect())
            .unwrap_or_default();
        let zones = design.routing.as_ref().map(|r| r.zones.iter().filter(|z| z.teardrop).cloned().collect()).unwrap_or_default();
        let mut world = TeardropWorld { anchors, anchor_copper: Vec::new(), items, item_copper: Vec::new(), item_grid: HashMap::new(), anchors_by_net: HashMap::new(), graphics, track_params: settings.track_end, zones };
        world.index();
        world
    }

    /// The caches of the touch tests: each anchor's and item's copper, the items' grid, the anchors by net.
    fn index(&mut self) {
        self.anchor_copper = copper_of(&self.anchors);
        self.anchors_by_net.clear();
        for (i, a) in self.anchors.iter().enumerate() {
            self.anchors_by_net.entry(a.net.clone()).or_default().push(i);
        }
        self.item_copper = self.items.iter().map(|t| {
            let s = t.shape();
            let b = bbox_of(&s);
            (s, b)
        }).collect();
        self.item_grid.clear();
        for (i, (t, (_, b))) in self.items.iter().zip(&self.item_copper).enumerate() {
            self.item_grid.entry(t.layer.clone()).or_default().insert(i, *b);
        }
    }

    /// The items on `layer` whose box meets `b`, in the board's order.
    fn items_near(&self, layer: &str, b: (i64, i64, i64, i64)) -> Vec<usize> {
        self.item_grid.get(layer).map(|g| g.query(b)).unwrap_or_default().into_iter().filter(|&i| boxes_meet(self.item_copper[i].1, b)).collect()
    }

    /// [`Self::connected`] of the world's `i`-th item, its copper as the cache has it.
    fn connected_item(&self, i: usize) -> (Vec<usize>, Vec<usize>) {
        let (shape, b) = &self.item_copper[i];
        self.connected_shape(&self.items[i], shape, *b)
    }

    /// `GetConnectedPadsAndVias( aTrack )`: the anchors of the track's net whose copper on its layer touches it, the pads first and then the vias, each in the
    /// board's order. (KiCad asks its connectivity, whose items' copper collides.)
    fn connected(&self, item: &TrackItem) -> (Vec<usize>, Vec<usize>) {
        let shape = item.shape();
        let b = bbox_of(&shape);
        self.connected_shape(item, &shape, b)
    }

    fn connected_shape(&self, item: &TrackItem, shape: &Shape, b: (i64, i64, i64, i64)) -> (Vec<usize>, Vec<usize>) {
        if item.net.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let (mut pads, mut vias) = (Vec::new(), Vec::new());
        for &i in self.anchors_by_net.get(&item.net).map_or(&[][..], |v| v.as_slice()) {
            let a = &self.anchors[i];
            let Some((_, s, _)) = self.anchor_copper[i].iter().find(|(l, _, ab)| *l == item.layer && boxes_meet(*ab, b)) else { continue };
            if s.collides(shape, Nm::ZERO).is_some() {
                if a.is_pad() {
                    pads.push(i);
                } else {
                    vias.push(i);
                }
            }
        }
        (pads, vias)
    }

    /// `connectivity->GetConnectedPadsAndVias( zone, .. )` and `GetConnectedTracks( zone )` of a teardrop: the anchors and tracks of its net on its layer that its
    /// outline touches.
    fn zone_anchors(&self, z: &Zone) -> (Vec<AnchorKey>, Vec<SegKey>) {
        if z.outline.len() < 3 || z.net.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let poly = Shape::Polygon { pts: z.outline.clone() };
        let b = bbox_of(&poly);
        let anchors = self
            .anchors_by_net
            .get(&z.net)
            .map_or(&[][..], |v| v.as_slice())
            .iter()
            .filter(|&&i| self.anchor_copper[i].iter().any(|(l, s, ab)| *l == z.layer && boxes_meet(*ab, b) && s.collides(&poly, Nm::ZERO).is_some()))
            .map(|&i| self.anchors[i].key.clone())
            .collect();
        let tracks = self
            .items_near(&z.layer, b)
            .into_iter()
            .filter(|&i| self.items[i].net == z.net && self.item_copper[i].0.collides(&poly, Nm::ZERO).is_some())
            .filter_map(|i| self.items[i].key.clone())
            .collect();
        (anchors, tracks)
    }
}

/// `TEARDROP_MANAGER` at work on a [`TeardropWorld`]: the teardrops it makes, and the copper index they are fitted against.
struct Manager<'a> {
    world: &'a TeardropWorld,
    rules: &'a eda_model::BoardRules,
    /// `ensureCopperIndex`: tracks, vias, pads, graphics on copper and the board's teardrops; no pour ("The filler's priority rules knock a different-net pour back
    /// around a teardrop, but nothing keeps two teardrops out of the same gap").
    copper: std::cell::OnceCell<(Vec<Obstacle>, HashMap<String, Grid>)>,
    /// The teardrops of the board the index holds.
    zones: Vec<Zone>,
    /// `m_createdTdRTree`: the teardrops made so far ("The next teardrop has to see this one, or two of them flare into the same gap").
    created: Vec<Obstacle>,
    created_grid: HashMap<String, Grid>,
    /// `BOARD::GetMaxClearanceValue()`.
    max_clearance: i64,
    out: Vec<Zone>,
}

/// `m_tolerance`: `pcbIUScale.mmToIU( 0.01 )`.
const TOLERANCE: i64 = 10_000;

/// `BOARD_DESIGN_SETTINGS::GetDRCEpsilon()`: `ADVANCED_CFG::m_DRCEpsilon`, 0.0005 mm.
const DRC_EPSILON: i64 = 500;

impl<'a> Manager<'a> {
    /// The manager on `world`, the teardrop zones `zones` (the board's that are left) part of its copper index.
    fn new(world: &'a TeardropWorld, rules: &'a eda_model::BoardRules, zones: &[Zone]) -> Manager<'a> {
        let zones = zones.iter().filter(|z| z.outline.len() >= 3 && eda_model::layers::is_copper_layer_name(&z.layer)).cloned().collect();
        Manager { world, rules, copper: std::cell::OnceCell::new(), zones, created: Vec::new(), created_grid: HashMap::new(), max_clearance: eda_drc::constraints::worst_case_clearance(rules).get(), out: Vec::new() }
    }

    /// `ensureCopperIndex`, made the first time it is asked: every track, via and pad on each of its layers, the graphics on copper and the board's teardrops.
    fn copper(&self) -> &(Vec<Obstacle>, HashMap<String, Grid>) {
        self.copper.get_or_init(|| {
            let world = self.world;
            let mut copper = Vec::new();
            for (i, (t, (shape, b))) in world.items.iter().zip(&world.item_copper).enumerate() {
                copper.push(Obstacle { id: ObsId::Item(i), layer: t.layer.clone(), net: t.net.clone(), bbox: *b, shape: shape.clone() });
            }
            for (i, (a, shapes)) in world.anchors.iter().zip(&world.anchor_copper).enumerate() {
                for (l, shape, b) in shapes {
                    copper.push(Obstacle { id: ObsId::Anchor(i), layer: l.clone(), net: a.net.clone(), bbox: *b, shape: shape.clone() });
                }
            }
            for (i, (l, shape, _)) in world.graphics.iter().enumerate() {
                copper.push(Obstacle { id: ObsId::Graphic(i), layer: l.clone(), net: String::new(), bbox: bbox_of(shape), shape: shape.clone() });
            }
            for (i, z) in self.zones.iter().enumerate() {
                let shape = Shape::Polygon { pts: z.outline.clone() };
                copper.push(Obstacle { id: ObsId::Zone(i), layer: z.layer.clone(), net: z.net.clone(), bbox: bbox_of(&shape), shape });
            }
            let mut grid: HashMap<String, Grid> = HashMap::new();
            for (i, o) in copper.iter().enumerate() {
                grid.entry(o.layer.clone()).or_default().insert(i, o.bbox);
            }
            (copper, grid)
        })
    }

    /// `TEARDROP_MANAGER::pairClearance`: the clearance the rules ask between the track and the item (`EvalRules( CLEARANCE_CONSTRAINT, .. )`), less the DRC's
    /// epsilon ("DRC allows geometry exactly at the limit; rejecting it here would silently delete a teardrop the rules permit"). **Differs:** the clearance is
    /// the net classes' and the board's minimum (`eda_drc::constraints::clearance`, the engine's fast path); a custom rule or an item's own clearance is not asked.
    fn pair_clearance(&self, net: &str, other: &str) -> i64 {
        let c = eda_drc::constraints::clearance(self.rules, (!net.is_empty()).then_some(net), (!other.is_empty()).then_some(other)).get();
        (c - DRC_EPSILON).max(0)
    }

    /// `TEARDROP_MANAGER::collidesWithOtherNets`: whether the polygon `points` (simplified: "The shape can self-intersect on awkward entries") comes within the
    /// pair clearance of copper of another net on the source track's layer, or of a teardrop made before it. `exempt` are the anchor and the tracks it is built
    /// from. Same-net copper is the same conductor; a track without a net is not built on here.
    fn collides_with_other_nets(&self, points: &[Point64], source: &TrackItem, exempt: &[ObsId]) -> bool {
        if points.len() < 3 {
            return false;
        }
        let mut set = ShapePolySet::from_outline(points.to_vec());
        set.simplify();
        let shapes: Vec<Shape> = set.polys.iter().filter_map(|p| p.first()).filter(|o| o.len() >= 3).map(|o| Shape::Polygon { pts: o.iter().map(|&p| pt(p)).collect() }).collect();
        if shapes.is_empty() {
            return false;
        }
        // "The radius has to cover the widest clearance anything can demand, or an obstacle with a generous one of its own is never tested."
        let reach = inflate_box(points_box(points), self.max_clearance);
        let collides = |o: &Obstacle| {
            if o.layer != source.layer || !boxes_meet(reach, o.bbox) || exempt.contains(&o.id) {
                return false;
            }
            // `isSameConductor`: the source track's net.
            if !source.net.is_empty() && o.net == source.net {
                return false;
            }
            let clearance = Nm::new(self.pair_clearance(&source.net, &o.net));
            shapes.iter().any(|s| s.collides(&o.shape, clearance).is_some())
        };
        let near = |grid: &HashMap<String, Grid>| grid.get(&source.layer).map(|g| g.query(reach)).unwrap_or_default();
        let (copper, grid) = self.copper();
        near(grid).into_iter().any(|i| collides(&copper[i])) || near(&self.created_grid).into_iter().any(|i| collides(&self.created[i]))
    }

    /// `TEARDROP_MANAGER::findTouchingTrack( aMatchType, aTrackRef, aSourceTrack, aEndPoint )`: the board track on the reference's layer, other than the reference
    /// and the source, whose copper touches the reference's (`m_tracksRTree.QueryColliding( aTrackRef, .. )`) and that has an end within `m_tolerance` of `end`
    /// (`IsPointOnEnds`); at a Y junction the longest ("if faced with a Y junction, choose the track longest segment as candidate").
    fn find_touching_track(&self, track_ref: &TrackItem, source: &TrackItem, end: Point64) -> Option<(usize, u8)> {
        let ref_shape = track_ref.shape();
        let mut candidate: Option<(usize, u8)> = None;
        for i in self.world.items_near(&track_ref.layer, bbox_of(&ref_shape)) {
            let t = &self.world.items[i];
            // "A stub shares an endpoint with the track it stands in for, and continuing back onto that is not a continuation."
            if t.key.is_some() && (t.key == track_ref.key || t.key == source.key) {
                continue;
            }
            if ref_shape.collides(&self.world.item_copper[i].0, Nm::ZERO).is_none() {
                continue;
            }
            let m = t.is_point_on_ends(end, TOLERANCE);
            if m == 0 {
                continue;
            }
            if let Some((c, _)) = candidate {
                if self.world.items[c].length() >= t.length() {
                    continue;
                }
            }
            candidate = Some((i, m));
        }
        candidate
    }

    /// `TEARDROP_MANAGER::computeTeardropPolygon( aParams, aCorners, aTrack, aSourceTrack, aOther, aOtherPos )` (25ec2335) with `findAnchorPointsOnTrack`: see
    /// the module doc. `track` is the track the teardrop is built along (the source track itself, or a crossing's stub), `other` the via, pad or wider track.
    #[allow(clippy::too_many_arguments)]
    fn compute_teardrop_polygon(&self, params: &TeardropParams, track: &TrackItem, source: &TrackItem, other: Other, other_pos: Point64) -> Option<Vec<Point64>> {
        let layer = track.layer.as_str();
        let w = other.width(layer);
        let radius = w / 2;
        if radius <= 0 || track.width <= 0 {
            return None;
        }

        // ---- `findAnchorPointsOnTrack`.
        let mut target_length = (w as f64 * params.best_length_ratio).round() as i64;
        if params.max_length.get() > 0 {
            target_length = target_length.min(params.max_length.get());
        }
        let (mut start, mut end) = (track.start, track.end);
        // "aTrack is expected to have one end inside the via/pad and the other end outside so ensure the start point is inside the via/pad".
        let mut need_swap = false;
        if !other.hit_test(start) {
            std::mem::swap(&mut start, &mut end);
            need_swap = true;
        }
        let shapebuffer = other.shape_buffer(layer, other_pos);
        let outline: &[Point64] = shapebuffer.first().and_then(|p| p.first()).map_or(&[], |c| c.as_slice());
        // "Search the intersection point between the pad/via shape and the current track"; an arc by its polyline.
        let hits = match &track.arc {
            Some(arc) => chain_chain_intersect(outline, &arc.polyline(MAX_ERROR)),
            None => chain_intersect(outline, start, end),
        };
        // "Ensure a intersection point was found, otherwise we cannot built the teardrop using this track (it is fully outside or inside the pad/via shape)".
        let intersection = *hits.first()?;
        start = intersection;
        let mut actual_td_len = target_length.min(euclidean_norm(end.x - start.x, end.y - start.y));
        let ref_length_point = start;
        // "If the first track is too short to allow a teardrop having the requested length explore the connected track(s), and try to find a anchor point at
        // targetLength from initial start".
        let mut connected: Option<usize> = None;
        if actual_td_len < target_length && params.allow_two_segments {
            let mut consumed = 0;
            if let Some((c, match_type)) = self.find_touching_track(track, source, end) {
                let ct = &self.world.items[c];
                // "Reject the extension if the angle between segments is too large" (`kMinCosForTwoSegmentExtension`, cos 60 degrees).
                let first_dir = normalize((end.x - ref_length_point.x, end.y - ref_length_point.y));
                let second_dir = if match_type == STARTPOINT { normalize((ct.end.x - ct.start.x, ct.end.y - ct.start.y)) } else { normalize((ct.start.x - ct.end.x, ct.start.y - ct.end.y)) };
                let cos_angle = first_dir.0 * second_dir.0 + first_dir.1 * second_dir.1;
                if cos_angle >= 0.5 {
                    consumed += actual_td_len;
                    actual_td_len = (target_length - consumed).min(ct.length() as i64);
                    connected = Some(c);
                    end = ct.end;
                    start = ct.start;
                    need_swap = false;
                    if match_type != STARTPOINT {
                        std::mem::swap(&mut start, &mut end);
                        need_swap = true;
                    }
                }
            }
        }
        let a_track: &TrackItem = connected.map_or(track, |c| &self.world.items[c]);
        // "if aTrack is an arc, find the best teardrop end point on the arc".
        if let Some(track_arc) = &a_track.arc {
            // 25ec2335: "The conversion places its corner points slightly outside the arc. Move them onto the arc so they can be used as points on the track
            // centerline."
            let oriented = if need_swap { track_arc.reversed() } else { *track_arc };
            let poly: Vec<Point64> = oriented.polyline(MAX_ERROR).into_iter().map(|p| oriented.nearest_point(p)).collect();
            if poly.len() > 2 {
                for ii in (0..poly.len()).rev() {
                    let dist_from_start = euclidean_norm(poly[ii].x - start.x, poly[ii].y - start.y);
                    if dist_from_start < actual_td_len || ii == 0 {
                        start = poly[ii];
                        if ii < poly.len() - 1 {
                            end = poly[ii + 1];
                        }
                        actual_td_len -= euclidean_norm(start.x - ref_length_point.x, start.y - ref_length_point.y);
                        actual_td_len = actual_td_len.max(0).min(euclidean_norm(end.x - start.x, end.y - start.y));
                        break;
                    }
                }
            }
        }

        // ---- `computeTeardropPolygon`.
        // "The start and end points must be different to calculate a valid polygon shape".
        if start == end {
            return None;
        }
        let vec_t = normalize((end.x - start.x, end.y - start.y));
        // "When spanning two segments, findAnchorPointsOnTrack replaces aTrack with the connected track ... Use the first segment's direction for all via-side
        // geometry"; "When the first segment is so short that the junction coincides with the pad edge intersection ... Fall back to the second segment's direction".
        let two_segments = connected.is_some();
        let vec_via = if two_segments && start != intersection { normalize((start.x - intersection.x, start.y - intersection.y)) } else { vec_t };
        // 25ec2335: "The canvas draws an arc track as short straight lines that can sit up to the max deviation inside the true edge. Keep the corners inside the
        // track by that amount so they cannot show past the drawn edge."
        let track_halfwidth = if a_track.arc.is_some() { (a_track.width / 4).max(a_track.width / 2 - MAX_ERROR) } else { a_track.width / 2 };
        let (hw, len) = (track_halfwidth as f64, actual_td_len as f64);
        let point_b = Point64::new(start.x + (vec_t.0 * len + vec_t.1 * hw) as i64, start.y + (vec_t.1 * len - vec_t.0 * hw) as i64);
        let point_a = Point64::new(start.x + (vec_t.0 * len - vec_t.1 * hw) as i64, start.y + (vec_t.1 * len + vec_t.0 * hw) as i64);
        // "To build a polygonal valid shape pointA and point B must be outside the pad. It can be inside with some pad shapes having very different X and X sizes".
        let pad_geo = other.pad_geo(layer);
        if let Other::Anchor(_, a) = other {
            if pad_geo.is_some() && (a.hit_test(point_a, Some(layer)) || a.hit_test(point_b, Some(layer))) {
                return None;
            }
        }

        // "Compute pointD, the "back" point of the teardrop behind the pad/via center."
        let pad_radius = w / 2;
        let int_to_pad = ((other_pos.x - intersection.x) as f64, (other_pos.y - intersection.y) as f64);
        let proj_on_track = -(int_to_pad.0 * vec_via.0 + int_to_pad.1 * vec_via.1);
        let offset = 1_000.0; // pcbIUScale.mmToIU( 0.001 )
        // "A custom pad's position is only its anchor, so projecting it yields a depth unrelated to the lobe the track enters".
        let is_custom_pad = matches!(other, Other::Anchor(_, a) if a.is_pad() && a.geo(layer).is_some_and(|g| g.custom));
        let mut effective_dist = if is_custom_pad { pad_radius as f64 } else { proj_on_track.max(pad_radius as f64) };
        match (pad_geo, &other) {
            (Some(g), Other::Anchor(_, a)) => {
                // "For non-round pads, clamp effectiveDist so pointD stays inside the copper the track enters rather than spiking out the far side on an oblique entry
                // that only grazes a corner": a ray from the crossing into the pad, as long as the pad's circumscribed radius allows; the crossings of every contour.
                let bounding_radius = match a.kind {
                    AnchorKind::Pad { bounding_radius } => bounding_radius,
                    AnchorKind::Via { .. } => 0,
                };
                let reach = effective_dist + 2.0 * bounding_radius as f64 + offset;
                let ray_end = Point64::new(intersection.x + (-vec_via.0 * reach).round() as i64, intersection.y + (-vec_via.1 * reach).round() as i64);
                let mut crossings: Vec<f64> = g
                    .contours()
                    .flat_map(|c| chain_intersect(c, intersection, ray_end))
                    .map(|h| euclidean_norm(h.x - intersection.x, h.y - intersection.y) as f64)
                    .filter(|&d| d > offset)
                    .collect();
                crossings.sort_by(|a, b| a.total_cmp(b));
                let mut exit_edge = 0.0;
                for d in crossings {
                    let probe = Point64::new(intersection.x + (-vec_via.0 * (d + offset)).round() as i64, intersection.y + (-vec_via.1 * (d + offset)).round() as i64);
                    if !g.contains(probe) {
                        exit_edge = d;
                        break;
                    }
                }
                // "exitEdge == 0 means -vecVia does not penetrate the pad (a tangential graze); collapse pointD onto the entry".
                effective_dist = effective_dist.min((exit_edge - 2.0 * offset).max(0.0));
            }
            _ => {
                // "For round pads/vias, clamp effectiveDist so pointD stays inside the pad circle."
                let r = pad_radius as f64;
                let dist_center_sq = int_to_pad.0 * int_to_pad.0 + int_to_pad.1 * int_to_pad.1;
                let disc = proj_on_track * proj_on_track - (dist_center_sq - r * r);
                if disc >= 0.0 {
                    let far_edge = proj_on_track + disc.sqrt();
                    let max_allowed = (far_edge - 2.0 * offset).max(0.0);
                    if effective_dist > max_allowed {
                        effective_dist = max_allowed;
                    }
                }
            }
        }
        let point_d = kiround_v(intersection.x as f64 - vec_via.0 * (effective_dist + offset), intersection.y as f64 - vec_via.1 * (effective_dist + offset));

        // "For two-segment teardrops, compute junction edge points where the track changes direction, and use the first segment side of the junction for the convex
        // hull anchor so that C and E are oriented to the via entry axis."
        let junction = |v: (f64, f64), sign: f64| Point64::new(start.x + (sign * v.1 * hw).round() as i64, start.y + (-sign * v.0 * hw).round() as i64);
        let (junction_b_seg2, junction_a_seg2) = (junction(vec_t, 1.0), junction(vec_t, -1.0));
        let (junction_b_seg1, junction_a_seg1) = (junction(vec_via, 1.0), junction(vec_via, -1.0));
        // "On the inside of a bend, the seg2 junction point backtracks relative to seg1 and causes self-intersection."
        let backtracks = |seg2: Point64, seg1: Point64, anchor: Point64| ((seg2.x - seg1.x) as f64 * (anchor.x - seg1.x) as f64 + (seg2.y - seg1.y) as f64 * (anchor.y - seg1.y) as f64) < 0.0;
        let skip_junction_a = two_segments && backtracks(junction_a_seg2, junction_a_seg1, point_a);
        let skip_junction_b = two_segments && backtracks(junction_b_seg2, junction_b_seg1, point_b);
        let (anchor_a, anchor_b) = if two_segments { (junction_a_seg1, junction_b_seg1) } else { (point_a, point_b) };

        let mut pts = [anchor_a, anchor_b, Point64::new(0, 0), point_d, Point64::new(0, 0)];
        // "On failure the pad-side anchors are left at the origin, stretching the shape to (0, 0)."
        let mut c_buffer = ShapePolySet::new();
        for poly in &shapebuffer {
            let mut it = poly.iter();
            if let Some(o) = it.next() {
                let i = c_buffer.add_outline(o.clone());
                for h in it {
                    c_buffer.add_hole(h.clone(), Some(i));
                }
            }
        }
        if !compute_anchor_points(params, w, other.round(layer), c_buffer, &mut pts) {
            return None;
        }

        // "For off-center track connections, the convex hull produces asymmetric anchor points (C and E at different distances from the track axis). Recompute
        // them to be symmetric so the teardrop flares out evenly from the track on both sides." (`if( IsRound( aOther, layer ) )`.)
        if other.round(layer) {
            let perp_via = (-vec_via.1, vec_via.0);
            let pad_offset = int_to_pad;
            let perp_dist_to_center = pad_offset.0 * perp_via.0 + pad_offset.1 * perp_via.1;
            if perp_dist_to_center.abs() > pad_radius as f64 * 0.1 {
                let d = perp_dist_to_center.abs();
                if d < pad_radius as f64 {
                    let max_symmetric = pad_radius as f64 - d;
                    let preferred_width = (w as f64 * params.best_width_ratio).round() as i64;
                    let mut max_half_width = preferred_width / 2;
                    if params.max_width.get() > 0 {
                        max_half_width = max_half_width.min(params.max_width.get() / 2);
                    }
                    let sym_half_width = max_symmetric.min(max_half_width as f64);
                    if sym_half_width > track_halfwidth as f64 {
                        let (cx, cy) = (other_pos.x as f64, other_pos.y as f64);
                        let r = pad_radius as f64;
                        let int_pt = (intersection.x as f64, intersection.y as f64);
                        // `findCircleLineIntersection`: the circle's crossing with the axis' parallel at `perp_dist`, the one nearer the track's entry.
                        let find = |perp_dist: f64| -> Point64 {
                            let proj_along_track = pad_offset.0 * vec_via.0 + pad_offset.1 * vec_via.1;
                            let origin = (int_pt.0 + vec_via.0 * proj_along_track + perp_via.0 * perp_dist, int_pt.1 + vec_via.1 * proj_along_track + perp_via.1 * perp_dist);
                            let oc = (origin.0 - cx, origin.1 - cy);
                            let b = oc.0 * vec_via.0 + oc.1 * vec_via.1;
                            let c = oc.0 * oc.0 + oc.1 * oc.1 - r * r;
                            let disc = b * b - c;
                            if disc < 0.0 {
                                return kiround_v(origin.0, origin.1);
                            }
                            let sq = disc.sqrt();
                            let p1 = (origin.0 + vec_via.0 * (-b - sq), origin.1 + vec_via.1 * (-b - sq));
                            let p2 = (origin.0 + vec_via.0 * (-b + sq), origin.1 + vec_via.1 * (-b + sq));
                            let dist = |p: (f64, f64)| (p.0 - int_pt.0).hypot(p.1 - int_pt.1);
                            if dist(p1) < dist(p2) {
                                kiround_v(p1.0, p1.1)
                            } else {
                                kiround_v(p2.0, p2.1)
                            }
                        };
                        pts[2] = find(-sym_half_width);
                        pts[4] = find(sym_half_width);
                    }
                }
            }
        }

        let mut corners: Vec<Point64> = Vec::new();
        if !params.curved_edges {
            if two_segments {
                corners.push(point_a);
                corners.push(point_b);
                if !skip_junction_b {
                    corners.push(junction_b_seg2);
                }
                corners.extend([pts[1], pts[2], pts[3], pts[4], pts[0]]);
                if !skip_junction_a {
                    corners.push(junction_a_seg2);
                }
            } else {
                corners = pts.to_vec();
            }
            return Some(corners);
        }

        // "See if we can use curved teardrop shape".
        let mut curve: Vec<Point64> = Vec::new();
        if other.round(layer) {
            compute_curved_for_round_shape(params, &mut curve, w, track_halfwidth, if two_segments { vec_via } else { vec_t }, other_pos, &mut pts);
        } else {
            // (`td_width`, `KiROUND( GetWidth( aOther, layer ) * m_BestWidthRatio )` clamped by `m_TdMaxWidth`, is handed to `computeCurvedForRectShape`, which
            // does not read it.)
            compute_curved_for_rect_shape(&mut curve, &pts, intersection, pad_geo.map(|g| g.rect), other_pos);
        }
        if two_segments {
            corners.push(point_b);
            if !skip_junction_b {
                corners.push(junction_b_seg2);
            }
            corners.extend(curve);
            if !skip_junction_a {
                corners.push(junction_a_seg2);
            }
            corners.push(point_a);
        } else {
            corners = curve;
        }
        Some(corners)
    }

    /// `TEARDROP_MANAGER::computeFittedTeardropPolygon`: the teardrop, narrowed where it would come within clearance of another net's copper ("Bisect between the
    /// requested width and the track width, the narrowest worth keeping": the track's width first, then four halvings), or none when not even the track's width
    /// fits.
    fn fitted_teardrop(&self, params: &TeardropParams, track: &TrackItem, source: &TrackItem, source_id: Option<usize>, other: Other, other_pos: Point64) -> Option<Vec<Point64>> {
        // "A teardrop overlaps its anchor and its track by construction."
        let mut exempt = vec![other.obstacle()];
        if let Some(i) = source_id {
            exempt.push(ObsId::Item(i));
        }
        let points = self.compute_teardrop_polygon(params, track, source, other, other_pos)?;
        if !self.collides_with_other_nets(&points, source, &exempt) {
            return Some(points);
        }
        let layer = track.layer.as_str();
        let preferred = (other.width(layer) as f64 * params.best_width_ratio).round() as i64;
        let mut hi = if params.max_width.get() > 0 { params.max_width.get().min(preferred) } else { preferred };
        let mut lo = track.width;
        // "Nothing narrower than the track is kept, so a track at or over the bound leaves nothing that fits."
        if lo >= hi {
            return None;
        }
        let try_width = |width: i64| -> Option<Vec<Point64>> {
            let p = TeardropParams { max_width: Nm::new(width), ..*params };
            self.compute_teardrop_polygon(&p, track, source, other, other_pos).filter(|c| !self.collides_with_other_nets(c, source, &exempt))
        };
        // "Midpoints alone never probe the bottom of the range, losing a teardrop that only fits close to the track width."
        let mut fitted = try_width(lo)?;
        for _ in 0..4 {
            if hi - lo <= 1 {
                break;
            }
            let mid = lo + (hi - lo) / 2;
            match try_width(mid) {
                Some(c) => {
                    lo = mid;
                    fitted = c;
                }
                None => hi = mid,
            }
        }
        (!fitted.is_empty()).then_some(fitted)
    }

    /// `TEARDROP_MANAGER::tryCreateTrackTeardrop` and `createAndAddTeardropWithMask`: the fitted teardrop, added with the mask teardrop over it when the source track
    /// has a solder-mask opening on an outer layer.
    #[allow(clippy::too_many_arguments)]
    fn try_create(&mut self, params: &TeardropParams, variant: Variant, track: &TrackItem, source: &TrackItem, source_id: Option<usize>, other: Other, other_pos: Point64) -> bool {
        let Some(mut points) = self.fitted_teardrop(params, track, source, source_id, other, other_pos) else { return false };
        // `outline->Append( pt.x, pt.y )`: `SHAPE_LINE_CHAIN::Append` drops a point equal to the last one (a straight two-segment teardrop's junction points).
        points.dedup();
        let net = source.net.clone();
        let layer = source.layer.clone();
        let outline: Vec<Point> = points.iter().map(|&p| pt(p)).collect();
        let shape = Shape::Polygon { pts: outline.clone() };
        let (ci, cb) = (self.created.len(), bbox_of(&shape));
        self.created_grid.entry(layer.clone()).or_default().insert(ci, cb);
        self.created.push(Obstacle { id: ObsId::Created(ci), layer: layer.clone(), net: net.clone(), bbox: cb, shape });
        let track_end = variant == Variant::TrackEnd;
        self.out.push(Zone { id: String::new(), net, layer: layer.clone(), outline, teardrop: true, teardrop_track_end: track_end, ..Default::default() });
        // `if( aSourceTrack->HasSolderMask() && IsExternalCopperLayer( aSourceTrack->GetLayer() ) )`: `createTeardropMask`, on F.Mask or B.Mask, the copper's
        // outline grown by the track's mask expansion (`Inflate( expansion, CORNER_STRATEGY::ALLOW_ACUTE_CORNERS, m_MaxError )`).
        if let (Some(expansion), true) = (source.mask, layer == "F.Cu" || layer == "B.Cu") {
            let mut mask = points.clone();
            if expansion != 0 {
                let mut set = ShapePolySet::from_outline(points.clone());
                set.inflate(expansion, eda_shape_poly_set::CornerStrategy::AllowAcuteCorners, MAX_ERROR as i32, false);
                if let Some(o) = set.polys.first().and_then(|p| p.first()) {
                    mask = o.clone();
                }
            }
            let mask_layer = if layer == "F.Cu" { "F.Mask" } else { "B.Mask" };
            // "The zone-min-thickness deflate/reinflate is going to round corners" (`SetMinThickness( std::max( GetMinThickness(), expansion ) )`).
            self.out.push(Zone {
                id: String::new(),
                net: String::new(),
                layer: mask_layer.to_string(),
                outline: mask.into_iter().map(pt).collect(),
                teardrop: true,
                teardrop_track_end: track_end,
                min_thickness: Nm::new(expansion.max(TEARDROP_MIN_THICKNESS)),
                ..Default::default()
            });
        }
        true
    }

    /// `UpdateTeardrops`' work on the board's tracks: for each track, the pads and vias it is connected to (`GetConnectedPadsAndVias`), when the track is dirty or
    /// the anchor is (or the update is a full one): each anchor's own parameters (`m_Enabled`), the width filters ("track width must be < teardrop width and
    /// filter width. A max width of 0 means no limit"), the ends hit-tested ("The track is entirely inside the pad"), `computeChordThroughShape`'s graze filter
    /// ("Reject tangential grazes, but keep short radial entries"), then the teardrop -- or, for a track crossing the anchor's centre with neither end inside, one
    /// per side, each on a stub from one of the track's ends to the point of the track nearest the centre (`buildCrossingStub`). Then the track-to-track teardrops
    /// when their block is on (`AddTeardropsOnTracks`). **Differs:** `areItemsInSameZone` (a pad and a track a same-net pour joins, skipped when
    /// `m_TdOnPadsInZones` is off) is not asked: the fills are kicad-cli's and not part of this crate's input.
    fn update(&mut self, dirty: Option<&Dirty>) {
        let world = self.world;
        // A track that is not stale is built on only at a stale anchor, which is of its net.
        let dirty_nets: HashSet<&str> = dirty.map(|d| world.anchors.iter().filter(|a| d.anchors.contains(&a.key)).map(|a| a.net.as_str()).collect()).unwrap_or_default();
        for (ti, item) in world.items.iter().enumerate() {
            let force_update = dirty.is_none_or(|d| item.key.as_ref().is_some_and(|k| d.tracks.contains(k)));
            if !force_update && !dirty_nets.contains(item.net.as_str()) {
                continue;
            }
            let (pads, vias) = world.connected_item(ti);
            for ai in pads.into_iter().chain(vias) {
                let anchor = &world.anchors[ai];
                if !force_update && !dirty.is_some_and(|d| d.anchors.contains(&anchor.key)) {
                    continue;
                }
                let Some(params) = anchor.params else { continue };
                let Some(geo) = anchor.geo(&item.layer) else { continue };
                let annular = geo.width as f64;
                let tw = item.width;
                let max_width = params.max_width.get();
                if (max_width > 0 && tw >= max_width) || tw as f64 >= annular * params.best_width_ratio || tw as f64 >= annular * params.filter_ratio {
                    continue;
                }
                // `pad->HitTest( track->GetStart(), 0, track->GetLayer() )`, `via->HitTest( track->GetStart() )`.
                let layer_hit = |p: Point64| if anchor.is_pad() { anchor.hit_test(p, Some(&item.layer)) } else { anchor.hit_test(p, None) };
                let (start_hits, end_hits) = (layer_hit(item.start), layer_hit(item.end));
                if start_hits && end_hits {
                    continue;
                }
                // Arcs are exempt ("Arcs are genuine entries, not the short straight grazes this filter targets").
                if start_hits != end_hits && item.arc.is_none() {
                    let polys = Other::Anchor(ai, anchor).shape_buffer(&item.layer, anchor.center);
                    if chord_through_shape(&polys, item.start, item.end, if start_hits { item.start } else { item.end }) < tw {
                        continue;
                    }
                }
                let other = Other::Anchor(ai, anchor);
                if !start_hits && !end_hits && item.hit_test(anchor.center) {
                    // "A track crossing the pad earns one teardrop per side, not one over the whole track."
                    let crossed = item.nearest_point(anchor.center);
                    for from in [item.end, item.start] {
                        let stub = item.stub(from, crossed);
                        self.try_create(&params, Variant::PadVia, &stub, item, Some(ti), other, anchor.center);
                    }
                } else {
                    self.try_create(&params, Variant::PadVia, item, item, Some(ti), other, anchor.center);
                }
            }
        }
        let tracks_dirty = dirty.is_none_or(|d| !d.tracks.is_empty());
        if tracks_dirty && world.track_params.enabled {
            self.add_teardrops_on_tracks(dirty);
        }
    }

    /// `TEARDROP_MANAGER::AddTeardropsOnTracks`: in each group of tracks of one net on one layer (`m_trackLookupList`, by net then layer) with two widths or
    /// more, a teardrop where a track ends on the end of a wider one (wider by more than the filter allows: `min_width = KiROUND( width / m_WidthtoSizeFilterRatio
    /// )`, the ratio at least 0.1), the track longer than half the wider one's width, unless a pad or via there takes it ("Pads/vias have priority for teardrops").
    /// An update that is not a full one builds only the pairs one of whose tracks is dirty ("An untouched pair's teardrop was not removed as stale").
    fn add_teardrops_on_tracks(&mut self, dirty: Option<&Dirty>) {
        let world = self.world;
        let mut params = world.track_params;
        let mut groups: std::collections::BTreeMap<(&str, &str), Vec<usize>> = std::collections::BTreeMap::new();
        for (i, t) in world.items.iter().enumerate() {
            if t.net.is_empty() {
                continue;
            }
            groups.entry((t.net.as_str(), t.layer.as_str())).or_default().push(i);
        }
        let is_dirty = |i: usize| dirty.is_none_or(|d| world.items[i].key.as_ref().is_some_and(|k| d.tracks.contains(k)));
        for (_, mut sublist) in groups {
            // "We need at least 2 track segments"
            if sublist.len() <= 1 {
                continue;
            }
            // "The sort function to sort by increasing track widths"
            sublist.sort_by_key(|&i| world.items[i].width);
            let (min_w, max_w) = (world.items[sublist[0]].width, world.items[*sublist.last().unwrap()].width);
            // "Skip groups having the same track thickness"
            if min_w == max_w {
                continue;
            }
            for ii in 0..sublist.len() - 1 {
                let ti = sublist[ii];
                let track = &world.items[ti];
                let track_len = track.length() as i64;
                let track_needs_update = is_dirty(ti);
                // "to avoid creating a teardrop between 2 tracks having similar widths give a threshold"
                params.filter_ratio = params.filter_ratio.max(0.1);
                let th = 1.0 / params.filter_ratio;
                let min_width = (track.width as f64 * th).round() as i64;
                for &ci in &sublist[ii + 1..] {
                    let candidate = &world.items[ci];
                    if min_width >= candidate.width {
                        continue;
                    }
                    // "Cannot build a teardrop on a too short track segment. The min len is > candidate radius"
                    if track_len <= candidate.width / 2 {
                        continue;
                    }
                    let mut pos = candidate.start;
                    let mut match_points = track.is_point_on_ends(pos, TOLERANCE);
                    if match_points == 0 {
                        pos = candidate.end;
                        match_points = track.is_point_on_ends(pos, TOLERANCE);
                    }
                    if match_points == 0 {
                        continue;
                    }
                    if !track_needs_update && !is_dirty(ci) {
                        continue;
                    }
                    let (pads, vias) = world.connected_item(ti);
                    if pads.into_iter().chain(vias).any(|a| world.anchors[a].hit_test(pos, None)) {
                        continue;
                    }
                    self.try_create(&params, Variant::TrackEnd, track, track, Some(ti), Other::Track(ci, candidate), pos);
                }
            }
        }
    }
}

/// `computeCurvedForRoundShape` (`teardrop_utils.cpp`, 25ec2335): the curved sides of a teardrop on a round item, two Bezier curves (`BEZIER_POLY::GetPoly` at
/// `maxError`) from B to C and from E to A with D between them, the control points along the track (`aTrackDir`) and along the circle's tangents at C and E,
/// weakened as the teardrop narrows; C and E first put back on the circle when the width clip moved them off it ("For large circles where teardrop width is
/// constrained"). `pts` is `[A, B, C, D, E]` (C and E may be moved).
#[allow(clippy::too_many_arguments)]
fn compute_curved_for_round_shape(params: &TeardropParams, poly: &mut Vec<Point64>, width: i64, track_halfwidth: i64, track_dir: (f64, f64), other_pos: Point64, pts: &mut [Point64; 5]) {
    let mut v_percent = params.best_width_ratio;
    let td_height = (width as f64 * v_percent).round() as i64;
    // "First, calculate a aVpercent equivalent to the td_height clamped by aTdMaxHeight".
    if params.max_width.get() > 0 && params.max_width.get() < td_height {
        v_percent *= params.max_width.get() as f64 / td_height as f64;
    }
    // "Don't divide by zero.  No good can come of that."
    let radius = (width / 2).max(1);
    let min_v_percent = track_halfwidth as f64 / radius as f64;
    let weaken = (v_percent - min_v_percent) / (1.0 - min_v_percent) / radius as f64;
    // "Project them onto the circle edge to ensure proper tangent calculation for smooth curves."
    let mut on_circle = |i: usize| -> (i64, i64) {
        let v = (pts[i].x - other_pos.x, pts[i].y - other_pos.y);
        let dist = euclidean_norm(v.0, v.1) as f64;
        if dist > 0.0 && (dist - radius as f64).abs() > MAX_ERROR as f64 {
            let (rx, ry) = resize(v.0, v.1, radius);
            pts[i] = Point64::new(other_pos.x + rx, other_pos.y + ry);
            return (rx, ry);
        }
        v
    };
    let vec_c = on_circle(2);
    let vec_e = on_circle(4);
    let seg_len = |a: Point64, b: Point64| euclidean_norm(b.x - a.x, b.y - a.y) as f64;
    let bias_bc = 0.5 * seg_len(pts[1], pts[2]);
    let bias_ae = 0.5 * seg_len(pts[4], pts[0]);
    // `VECTOR2I( double, double )`: truncated.
    let v2i = |x: f64, y: f64| Point64::new(x as i64, y as i64);
    let tangent_c = v2i(pts[2].x as f64 - vec_c.1 as f64 * bias_bc * weaken, pts[2].y as f64 + vec_c.0 as f64 * bias_bc * weaken);
    let tangent_e = v2i(pts[4].x as f64 + vec_e.1 as f64 * bias_ae * weaken, pts[4].y as f64 - vec_e.0 as f64 * bias_ae * weaken);
    let tangent_b = v2i(pts[1].x as f64 - track_dir.0 * bias_bc, pts[1].y as f64 - track_dir.1 * bias_bc);
    let tangent_a = v2i(pts[0].x as f64 - track_dir.0 * bias_ae, pts[0].y as f64 - track_dir.1 * bias_ae);
    poly.extend(bezier(pts[1], tangent_b, tangent_c, pts[2]));
    poly.push(pts[3]);
    poly.extend(bezier(pts[4], tangent_e, tangent_a, pts[0]));
}

/// `BEZIER_POLY( aStart, aCtrl1, aCtrl2, aEnd ).GetPoly( aOutput, maxError )` (`eda_model::bezier`).
fn bezier(a: Point64, c1: Point64, c2: Point64, b: Point64) -> Vec<Point64> {
    eda_model::bezier::bezier_polyline(pt(a), pt(c1), pt(c2), pt(b), Nm::new(MAX_ERROR)).into_iter().map(p64).collect()
}

/// `computeCornerTangentControlPoint`: the control point `aBias` from the anchor point along the tangent of the corner arc there, on the side that best follows
/// `aDesiredDir`.
fn corner_tangent_control_point(anchor: Point64, corner_center: Point64, bias: f64, desired: (i64, i64)) -> Point64 {
    let radial = (anchor.x - corner_center.x, anchor.y - corner_center.y);
    if euclidean_norm(radial.0, radial.1) == 0 {
        return anchor;
    }
    let (t1, t2) = ((radial.1, -radial.0), (-radial.1, radial.0));
    let dot = |t: (i64, i64)| t.0 as i128 * desired.0 as i128 + t.1 as i128 * desired.1 as i128;
    let t = if dot(t1) > dot(t2) { t1 } else { t2 };
    let (rx, ry) = resize(t.0, t.1, bias.round() as i64);
    Point64::new(anchor.x + rx, anchor.y + ry)
}

/// `isPointOnRoundedCorner` and `isPointOnOvalEnd` (`teardrop_utils.cpp`): the centre of the corner arc (or oval end) the point is on, if it is in one. The point is
/// taken into the pad's frame with `RotatePoint( localPt, aRotation )` and the centre back with `RotatePoint( aCornerCenter, -aRotation )`, as KiCad does.
fn curved_region_center(p: Point64, pad_pos: Point64, info: &RectInfo) -> Option<Point64> {
    let local = eda_shape_poly_set::basic_shapes::rotate_point_degrees(Point64::new(p.x - pad_pos.x, p.y - pad_pos.y), info.orientation);
    let (half_w, half_h) = (info.size.0 / 2, info.size.1 / 2);
    let center = if info.oval {
        // "Oval geometry: semicircle radius is min dimension / 2"
        let radius = half_w.min(half_h);
        if half_w > half_h {
            let off = half_w - radius;
            if local.x.abs() <= off {
                return None;
            }
            Point64::new(if local.x > 0 { off } else { -off }, 0)
        } else {
            let off = half_h - radius;
            if local.y.abs() <= off {
                return None;
            }
            Point64::new(0, if local.y > 0 { off } else { -off })
        }
    } else {
        let r = info.round_rect_radius?;
        let (inner_w, inner_h) = (half_w - r, half_h - r);
        if local.x.abs() <= inner_w || local.y.abs() <= inner_h {
            return None;
        }
        Point64::new(if local.x > 0 { inner_w } else { -inner_w }, if local.y > 0 { inner_h } else { -inner_h })
    };
    let c = eda_shape_poly_set::basic_shapes::rotate_point_degrees(center, -info.orientation);
    Some(Point64::new(c.x + pad_pos.x, c.y + pad_pos.y))
}

/// `computeCurvedForRectShape` (`teardrop_utils.cpp`, 25ec2335): the curved sides of a teardrop on a pad that is not round, two Bezier curves from B to C and from
/// E to A with D between them, the control points a quarter of each side along the track and halfway to the crossing -- or, where C or E is on a rounded
/// rectangle's corner arc or an oval's end, along that arc's tangent ("preventing the teardrop curve from intersecting the pad's corner radius").
fn compute_curved_for_rect_shape(poly: &mut Vec<Point64>, pts: &[Point64; 5], intersection: Point64, rect: Option<RectInfo>, other_pos: Point64) {
    let side1 = (pts[2].x - pts[1].x, pts[2].y - pts[1].y);
    let side2 = (pts[4].x - pts[0].x, pts[4].y - pts[0].y);
    let mid_ab = half_sum(pts[0], pts[1]);
    let track_dir = (intersection.x - mid_ab.x, intersection.y - mid_ab.y);
    let toward_track = (-track_dir.0, -track_dir.1);
    let curved = rect.filter(|r| r.oval || r.round_rect_radius.is_some_and(|c| c > 0));
    let along = |from: Point64, side: (i64, i64)| {
        let (rx, ry) = resize(track_dir.0, track_dir.1, euclidean_norm(side.0, side.1) / 4);
        Point64::new(from.x + rx, from.y + ry)
    };
    let control = |anchor: Point64, side: (i64, i64)| -> Point64 {
        // "Default control point - midpoint approach"
        let default = half_sum(anchor, intersection);
        match curved.and_then(|r| curved_region_center(anchor, other_pos, &r)) {
            Some(center) => corner_tangent_control_point(anchor, center, 0.5 * euclidean_norm(side.0, side.1) as f64, toward_track),
            None => default,
        }
    };
    let ctrl1 = along(pts[1], side1);
    let ctrl2 = control(pts[2], side1);
    poly.extend(bezier(pts[1], ctrl1, ctrl2, pts[2]));
    poly.push(pts[3]);
    let ctrl1 = control(pts[4], side2);
    let ctrl2 = along(pts[0], side2);
    poly.extend(bezier(pts[4], ctrl1, ctrl2, pts[0]));
}

/// `ZONE::SetMinThickness( pcbIUScale.mmToIU( 0.0254 ) )`: a teardrop's minimum thickness.
const TEARDROP_MIN_THICKNESS: i64 = 25_400;

/// What an edit made stale (`BOARD_COMMIT::Push`'s `staleTeardropPadsAndVias` and `staleTeardropTracks`, with what `RemoveTeardrops` adds for a fitted
/// teardrop whose neighbour moved).
#[derive(Debug, Clone, Default)]
struct Dirty {
    anchors: HashSet<AnchorKey>,
    tracks: HashSet<SegKey>,
}

/// Every teardrop this board's tracks, vias and pads call for, given `settings` (Board Setup's block, which serves the vias and pads that have no parameters of
/// their own) -- `TEARDROP_MANAGER::UpdateTeardrops( commit, nullptr, nullptr, true /* forceFullUpdate */ )`, what Edit Teardrops and every zone fill run: the
/// board's teardrops all taken out first, then made again. Returns `Zone`s with `teardrop: true` and no id (the caller's `RoutingSection::assign_missing_ids`),
/// ready to replace whatever teardrop zones exist; a track with a solder-mask opening on an outer layer gets its mask teardrop too (a zone on `F.Mask` or
/// `B.Mask`).
pub fn generate_teardrops(design: &Design, model: &ConstraintModel, settings: &TeardropSettings) -> Vec<Zone> {
    let mut world = TeardropWorld::of(design, model);
    if design.routing.as_ref().map(|r| r.teardrop_settings) != Some(*settings) {
        world.anchors = collect_anchors(design, model, settings);
        world.index();
        world.track_params = settings.track_end;
    }
    let mut manager = Manager::new(&world, &model.board, &[]);
    manager.update(None);
    manager.out
}

/// The board before an edit, for [`update_after_edit`]; `None` when nothing on it can have a teardrop ([`teardrops_possible`]).
pub fn snapshot(design: &Design, model: &ConstraintModel) -> Option<TeardropWorld> {
    teardrops_possible(design).then(|| TeardropWorld::of(design, model))
}

/// `BOARD_COMMIT::Push`'s teardrop update after an edit (25ec2335), `before` being the board as it was ([`snapshot`]): the tracks, vias and pads the edit added,
/// changed or took away are stale (a changed track's anchors as they were too), and so is every teardrop fitted near copper of another net the edit moved
/// (`RemoveTeardrops`' `foreignNeighbourMoved`, the teardrop's box grown by the largest clearance); the teardrops of stale items go (a mask teardrop with the
/// copper it covers), and `UpdateTeardrops( commit, dirtyPadsAndVias, dirtyTracks )` makes the teardrops of the stale items again on the board as it is now,
/// fitted to it, with the track-to-track ones of the stale tracks. All in the edit's own undo step. Returns whether the board's teardrops changed.
///
/// **Differs:** a mask teardrop is paired with the copper one it covers by their boxes (KiCad pairs them by uuid, the mask's the copper's plus one, and falls
/// back to the boxes for a board that predates that); the stale items are found by comparing the board before and after (KiCad's commit names them).
pub fn update_after_edit(design: &mut Design, model: &ConstraintModel, before: &TeardropWorld) -> bool {
    let after = TeardropWorld::of(design, model);
    let mut dirty = Dirty::default();
    // The tracks the edit added, changed or took away (a changed segment is its old copper gone and its new copper added).
    let before_keys: HashSet<&SegKey> = before.items.iter().filter_map(|t| t.key.as_ref()).collect();
    let after_keys: HashSet<&SegKey> = after.items.iter().filter_map(|t| t.key.as_ref()).collect();
    let mut dirty_copper: Vec<(String, String, (i64, i64, i64, i64))> = Vec::new();
    for t in &before.items {
        if t.key.as_ref().is_some_and(|k| !after_keys.contains(k)) {
            dirty.tracks.insert(t.key.clone().unwrap());
            dirty_copper.push((t.layer.clone(), t.net.clone(), bbox_of(&t.shape())));
            // `connectivity->GetConnectedPadsAndVias( track, .. )`: what the track was connected to.
            let (pads, vias) = before.connected(t);
            for a in pads.into_iter().chain(vias) {
                dirty.anchors.insert(before.anchors[a].key.clone());
            }
        }
    }
    for t in &after.items {
        if t.key.as_ref().is_some_and(|k| !before_keys.contains(k)) {
            dirty.tracks.insert(t.key.clone().unwrap());
            dirty_copper.push((t.layer.clone(), t.net.clone(), bbox_of(&t.shape())));
        }
    }
    // The vias and pads the edit added, changed or took away (a footprint's pads with it).
    let index = |w: &TeardropWorld| -> HashMap<AnchorKey, usize> { w.anchors.iter().enumerate().map(|(i, a)| (a.key.clone(), i)).collect() };
    let (bi, ai) = (index(before), index(&after));
    let changed_anchor = |a: &Anchor, out: &mut Vec<(String, String, (i64, i64, i64, i64))>| {
        for (layers, _) in &a.groups {
            for l in layers {
                if let Some(s) = a.shape_on(l) {
                    out.push((l.clone(), a.net.clone(), bbox_of(&s)));
                }
            }
        }
    };
    for a in &before.anchors {
        if ai.get(&a.key).is_none_or(|&j| after.anchors[j] != *a) {
            dirty.anchors.insert(a.key.clone());
            changed_anchor(a, &mut dirty_copper);
        }
    }
    for a in &after.anchors {
        if bi.get(&a.key).is_none_or(|&j| before.anchors[j] != *a) {
            dirty.anchors.insert(a.key.clone());
            changed_anchor(a, &mut dirty_copper);
        }
    }
    // The graphics on copper the edit moved (dirty copper only: nothing anchors on them).
    let keys = |w: &TeardropWorld| -> HashSet<(String, String)> { w.graphics.iter().map(|(l, _, k)| (l.clone(), k.clone())).collect() };
    let (bk, ak) = (keys(before), keys(&after));
    for (l, s, _) in before.graphics.iter().filter(|(l, _, k)| !ak.contains(&(l.clone(), k.clone()))).chain(after.graphics.iter().filter(|(l, _, k)| !bk.contains(&(l.clone(), k.clone())))) {
        dirty_copper.push((l.clone(), String::new(), bbox_of(s)));
    }
    if dirty.anchors.is_empty() && dirty.tracks.is_empty() && dirty_copper.is_empty() {
        return false;
    }

    // `RemoveTeardrops`: the anchors of each teardrop as the board had them before the edit (a teardrop the edit itself made has none yet).
    let Some(rt) = design.routing.as_mut() else { return false };
    let max_clearance = eda_drc::constraints::worst_case_clearance(&model.board).get();
    let copper_td: Vec<usize> = rt.zones.iter().enumerate().filter(|(_, z)| z.teardrop && eda_model::layers::is_copper_layer_name(&z.layer)).map(|(i, _)| i).collect();
    let key = |z: &Zone| (z.layer.clone(), z.net.clone(), z.outline.iter().map(|p| (p.x.get(), p.y.get())).collect::<Vec<_>>());
    let existed: HashSet<(String, String, Vec<(i64, i64)>)> = before.zones.iter().map(key).collect();
    let zone_box = |z: &Zone| points_box(&z.outline.iter().map(|&p| p64(p)).collect::<Vec<_>>());
    // Only a teardrop whose box meets what the edit touched can be stale (its anchors touch it): the copper the edit changed, and the copper of each stale anchor as
    // the board had it. The others are not looked at (KiCad asks every teardrop's anchors of its connectivity).
    let anchor_boxes = |keys: &HashSet<AnchorKey>| -> Vec<(String, (i64, i64, i64, i64))> {
        before.anchors.iter().enumerate().filter(|(_, a)| keys.contains(&a.key)).flat_map(|(i, _)| before.anchor_copper[i].iter().map(|(l, _, b)| (l.clone(), *b))).collect()
    };
    let mut touched: Vec<(String, (i64, i64, i64, i64))> = dirty_copper.iter().map(|(l, _, b)| (l.clone(), *b)).collect();
    touched.extend(anchor_boxes(&dirty.anchors));
    let candidates = |touched: &[(String, (i64, i64, i64, i64))], grow: i64, skip: &HashMap<usize, (Vec<AnchorKey>, Vec<SegKey>)>| -> Vec<usize> {
        copper_td
            .iter()
            .copied()
            .filter(|i| !skip.contains_key(i) && existed.contains(&key(&rt.zones[*i])))
            .filter(|&i| {
                let reach = inflate_box(zone_box(&rt.zones[i]), grow);
                touched.iter().any(|(l, b)| *l == rt.zones[i].layer && boxes_meet(reach, *b))
            })
            .collect()
    };
    let mut zone_anchors: HashMap<usize, (Vec<AnchorKey>, Vec<SegKey>)> = HashMap::new();
    for i in candidates(&touched, max_clearance, &zone_anchors) {
        zone_anchors.insert(i, before.zone_anchors(&rt.zones[i]));
    }
    // "A width fitted to the neighbours goes stale when one of them moves, though the teardrop anchors on neither. Pre- and post-edit geometry both count".
    let (mut more_anchors, mut more_tracks) = (HashSet::new(), HashSet::new());
    for (&i, (anchors, tracks)) in &zone_anchors {
        let z = &rt.zones[i];
        let reach = inflate_box(zone_box(z), max_clearance);
        // "Net 0 is "no net", not a net that every unassigned item shares."
        let moved = dirty_copper.iter().any(|(l, net, b)| *l == z.layer && !(!z.net.is_empty() && *net == z.net) && boxes_meet(reach, *b));
        if moved {
            more_anchors.extend(anchors.iter().filter(|a| !dirty.anchors.contains(*a)).cloned());
            more_tracks.extend(tracks.iter().filter(|t| !dirty.tracks.contains(*t)).cloned());
        }
    }
    // The teardrops that share an anchor or a track with one fitted again are stale too ("Dirty the anchors first, or the rebuild passes these teardrops by").
    if !more_anchors.is_empty() || !more_tracks.is_empty() {
        let mut more: Vec<(String, (i64, i64, i64, i64))> = anchor_boxes(&more_anchors);
        more.extend(before.items.iter().enumerate().filter(|(_, t)| t.key.as_ref().is_some_and(|k| more_tracks.contains(k))).map(|(i, t)| (t.layer.clone(), before.item_copper[i].1)));
        for i in candidates(&more, 0, &zone_anchors) {
            zone_anchors.insert(i, before.zone_anchors(&rt.zones[i]));
        }
        dirty.anchors.extend(more_anchors);
        dirty.tracks.extend(more_tracks);
    }
    let stale: HashSet<usize> = zone_anchors
        .iter()
        .filter(|(_, (anchors, tracks))| anchors.iter().any(|a| dirty.anchors.contains(a)) || tracks.iter().any(|t| dirty.tracks.contains(t)))
        .map(|(&i, _)| i)
        .collect();
    // A mask teardrop goes with the copper it covers: kept while a surviving copper teardrop on its side is concentric with it.
    let center = |z: &Zone| {
        let b = points_box(&z.outline.iter().map(|&p| p64(p)).collect::<Vec<_>>());
        (b, Point64::new((b.0 + b.2) / 2, (b.1 + b.3) / 2))
    };
    let inside = |b: (i64, i64, i64, i64), p: Point64| p.x >= b.0 && p.x <= b.2 && p.y >= b.1 && p.y <= b.3;
    let surviving: Vec<usize> = copper_td.iter().copied().filter(|i| !stale.contains(i)).collect();
    let mut remove: HashSet<usize> = stale.clone();
    for (i, z) in rt.zones.iter().enumerate() {
        if !z.teardrop || eda_model::layers::is_copper_layer_name(&z.layer) {
            continue;
        }
        let copper_layer = if z.layer == "F.Mask" { "F.Cu" } else { "B.Cu" };
        let (mb, mc) = center(z);
        let covers = surviving.iter().any(|&c| {
            let cz = &rt.zones[c];
            let (cb, cc) = center(cz);
            cz.layer == copper_layer && inside(mb, cc) && inside(cb, mc)
        });
        if !covers {
            remove.insert(i);
        }
    }
    let mut k = 0;
    rt.zones.retain(|_| {
        let keep = !remove.contains(&k);
        k += 1;
        keep
    });

    // `UpdateTeardrops( commit, dirtyPadsAndVias, dirtyTracks )` on the board as it is now, the teardrops left part of its copper.
    let survivors: Vec<Zone> = rt.zones.iter().filter(|z| z.teardrop).cloned().collect();
    let mut manager = Manager::new(&after, &model.board, &survivors);
    manager.update(Some(&dirty));
    let made = !manager.out.is_empty();
    rt.zones.extend(manager.out);
    made || !remove.is_empty()
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

    /// UI-LEFT P3: each via's and pad's own `TEARDROP_PARAMETERS` govern it (`UpdateTeardrops` reads `via->GetTeardropParams()`, `pad->GetTeardropParams()`);
    /// Board Setup's block serves only the anchors that have none.
    #[test]
    fn an_anchors_own_teardrop_parameters_govern_it_whatever_board_setup_says() {
        use eda_model::teardrop_params::TeardropParams;
        use eda_model::via_extra::ViaExtra;
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let (v1, v2) = (Point { x: a.x + mm(3.0), y: a.y }, Point { x: a.x + mm(3.0), y: a.y + mm(6.0) });
        let rt = design.routing.as_mut().unwrap();
        for (id, at) in [("v1", v1), ("v2", v2)] {
            rt.vias.push(Via { id: id.into(), net: "N1".into(), at, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
            rt.tracks.push(Track { id: format!("t{id}"), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![at, Point { x: at.x + mm(5.0), y: at.y }], arc_mid_offset: None });
        }
        let own = |p: TeardropParams| {
            let mut e = ViaExtra { id: "v1".into(), ..Default::default() };
            e.set_teardrops(Some(&p));
            e
        };
        // Board Setup off, v1's own on: v1 alone gets one.
        let on = TeardropParams { enabled: true, ..Default::default() };
        design.drawings.get_or_insert_with(Default::default).via_extras = vec![own(on)];
        let out = generate_teardrops(&design, &model, &TeardropSettings::default());
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(point_in_polygon(v1, &out[0].outline));
        // Board Setup on, v1's own off: v2 alone gets one.
        design.drawings.as_mut().unwrap().via_extras = vec![own(TeardropParams::default())];
        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(point_in_polygon(v2, &out[0].outline));
        // v1's own sizes: a longer teardrop than Board Setup's gives v2.
        design.drawings.as_mut().unwrap().via_extras = vec![own(TeardropParams { enabled: true, best_length_ratio: 1.0, max_length: mm(5.0), ..Default::default() })];
        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 2);
        let reach = |z: &Zone, from: Point| z.outline.iter().map(|p| p.x - from.x).max().unwrap();
        let (z1, z2) = if point_in_polygon(v1, &out[0].outline) { (&out[0], &out[1]) } else { (&out[1], &out[0]) };
        assert!(reach(z1, v1) > reach(z2, v2) + mm(0.3), "{:?} {:?}", reach(z1, v1), reach(z2, v2));
        // A pad's own parameters, from its edit: R1.1 turned on while Board Setup is off.
        design.drawings.as_mut().unwrap().via_extras.clear();
        design.routing.as_mut().unwrap().tracks.push(Track { id: "tp".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![a, Point { x: a.x, y: a.y - mm(5.0) }], arc_mid_offset: None });
        assert!(generate_teardrops(&design, &model, &TeardropSettings::default()).is_empty());
        let mut edit = eda_model::fp_edit::FootprintEdit::new("R1");
        edit.set_pad(eda_model::fp_edit::PadEdit { teardrops: Some(on), ..eda_model::fp_edit::PadEdit::none("1", 1) });
        design.drawings.as_mut().unwrap().footprint_edits = vec![edit];
        let out = generate_teardrops(&design, &model, &TeardropSettings::default());
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(point_in_polygon(Point { x: a.x, y: a.y - mm(0.1) }, &out[0].outline) || point_in_polygon(a, &out[0].outline));
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
        let d = chord_through_shape(&[vec![circle.clone()]], Point64::new(0, 0), Point64::new(350_000, 0), Point64::new(0, 0));
        assert!((600_000 - 2 * MAX_ERROR..=600_000).contains(&d), "{d}");
        // A graze: a short track whose inside end is near the rim, off the centre line.
        let g = chord_through_shape(&[vec![circle.clone()]], Point64::new(-50_000, 290_000), Point64::new(50_000, 290_000), Point64::new(0, 290_000));
        assert!(g < 200_000, "{g}");
        assert_eq!(chord_through_shape(&[vec![circle.clone()]], Point64::new(5, 5), Point64::new(5, 5), Point64::new(5, 5)), i64::MAX, "a zero-length track");
        // A rectangle's polygon: the span across it along the track (a 1.5 x 1 mm pad entered along its length).
        let rect = round_chamfered_rect_polygon((1_500_000, 1_000_000), 0, 0.0, 0, MAX_ERROR);
        assert_eq!(chord_through_shape(&[vec![rect.clone()]], Point64::new(0, 0), Point64::new(5_000_000, 0), Point64::new(0, 0)), 1_500_000);
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

    /// UI-LEFT-2 PP7: a custom pad gets its teardrop, on its merged polygon (`MergePrimitivesAsPolygon`), D at the anchor pad's radius ("A custom pad's position is
    /// only its anchor"). (Proven within 10 um of kicad-cli's by `crates/kicad/tests/teardrops_kicad_cli.rs`.)
    #[test]
    fn a_custom_pad_gets_its_teardrop() {
        let (mut design, model) = smd_pad_model(PadShape::Custom, (1.0, 1.0), None);
        design.routing.as_mut().unwrap().tracks.push(track(vec![at(100.0, 145.0), at(105.0, 145.0)], 0.2));
        let out = generate_teardrops(&design, &model, &enabled_settings());
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(point_in_polygon(at(100.6, 145.0), &out[0].outline), "{:?}", out[0].outline);
    }

    /// The settings of kicad-cli's cases (`crates/kicad/tests/teardrops_kicad_cli.rs`, `the_teardrops_round_7_left_out_are_kicad_clis_own`): Board Setup's block on
    /// with KiCad's sizes, one segment unless asked.
    fn kicad_case_settings(curved: bool, two: bool, max_len: f64) -> TeardropSettings {
        TeardropSettings { enabled: true, curved_edges: curved, allow_two_segments: two, max_len_nm: mm(max_len), ..Default::default() }
    }

    /// UI-LEFT-2 PP6: `computeFittedTeardropPolygon`. A track of another net near the teardrop's flank narrows it (bisected: the track's width, then four halvings
    /// towards the via's), to kicad-cli's own outline; two tracks too close for even the track's width leave none.
    #[test]
    fn a_teardrop_is_fitted_to_the_clearance_of_another_nets_copper() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(via(100.0, 100.0, 0.8));
        rt.tracks.push(track(vec![at(100.0, 100.0), at(105.0, 100.0)], 0.2));
        let settings = kicad_case_settings(false, false, 1.0);
        let wide = generate_teardrops(&design, &model, &settings);
        assert_eq!(wide[0].outline, outline(&[(100796578, 100100000), (100796578, 99900000), (100153073, 99630448), (99995578, 100000000), (100153073, 100369552)]));
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(Track { net: "N2".into(), ..track(vec![at(100.6, 100.48), at(100.6, 103.0)], 0.2) });
        let fitted = generate_teardrops(&design, &model, &settings);
        assert_eq!(fitted.len(), 1);
        assert_eq!(fitted[0].outline, outline(&[(100796578, 100100000), (100796578, 99900000), (100310846, 99750000), (99995578, 100000000), (100310846, 100250000)]));
        // Hemmed in on both sides: nothing fits.
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.pop();
        rt.tracks.push(Track { net: "N2".into(), ..track(vec![at(100.55, 99.62), at(103.0, 99.62)], 0.2) });
        rt.tracks.push(Track { net: "N2".into(), ..track(vec![at(100.55, 100.38), at(103.0, 100.38)], 0.2) });
        assert!(generate_teardrops(&design, &model, &settings).is_empty());
    }

    /// UI-LEFT-2 PP7: `m_CurvedEdges` on a via (`computeCurvedForRoundShape`): kicad-cli's outline, the two Bezier sides from B to C and E to A.
    #[test]
    fn curved_edges_give_kicad_clis_bezier_sides() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(via(100.0, 115.0, 0.8));
        rt.tracks.push(track(vec![at(100.0, 115.0), at(105.0, 115.0)], 0.2));
        let out = generate_teardrops(&design, &model, &kicad_case_settings(true, false, 1.0));
        assert_eq!(
            out[0].outline,
            outline(&[
                (100796578, 114900000), (100693601, 114895188), (100612384, 114881504), (100497840, 114835661), (100410798, 114777597), (100305227, 114706687),
                (100153073, 114630448), (99995578, 115000000), (100153073, 115369552), (100323432, 115282095), (100410797, 115222403), (100482597, 115173403),
                (100560228, 115134647), (100659588, 115109167), (100796578, 115100000),
            ])
        );
    }

    /// UI-LEFT-2 PP7: `m_AllowUseTwoTracks`. A segment leaving a 1.6 mm via by 0.1 mm lends the teardrop the length of the one it joins, bent by 18 degrees: the
    /// junction's points on both sides, kicad-cli's outline. One segment only: the teardrop is cut at the junction.
    #[test]
    fn a_short_first_segment_borrows_the_next_ones_length() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(via(100.0, 130.0, 1.6));
        rt.tracks.push(track(vec![at(100.0, 130.0), at(100.9, 130.0), at(104.0, 131.0)], 0.2));
        let out = generate_teardrops(&design, &model, &kicad_case_settings(false, true, 2.0));
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].outline, outline(&[(101531829, 130308890), (101593230, 130118548), (100930700, 129904829), (100900000, 129900000), (100618408, 129492485), (99995148, 130000000), (100618408, 130507515), (100900000, 130100000)]));
        let one = generate_teardrops(&design, &model, &kicad_case_settings(false, false, 2.0));
        assert_eq!(one[0].outline, outline(&[(100900000, 130100000), (100900000, 129900000), (100618408, 129492485), (99995148, 130000000), (100618408, 130507515)]));
    }

    /// UI-LEFT-2 PP7: `AddTeardropsOnTracks`. A 0.2 mm track ending on a 0.6 mm one gets a track-end teardrop (`TD_TRACKEND`) when the track-to-track block is on,
    /// kicad-cli's outline; none when it is off.
    #[test]
    fn a_narrow_track_on_a_wide_one_gets_a_track_end_teardrop() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track(vec![at(100.0, 150.0), at(105.0, 150.0)], 0.6));
        rt.tracks.push(track(vec![at(105.0, 150.0), at(110.0, 150.0)], 0.2));
        assert!(generate_teardrops(&design, &model, &TeardropSettings::default()).is_empty());
        let settings = TeardropSettings { track_end: TeardropParams { enabled: true, ..Default::default() }, ..Default::default() };
        let out = generate_teardrops(&design, &model, &settings);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].teardrop_track_end);
        assert_eq!(out[0].outline, outline(&[(105597433, 150100000), (105597433, 149900000), (105114805, 149722836), (104996433, 150000000), (105114805, 150277164)]));
    }

    /// UI-LEFT-2 PP7: `createTeardropMask`. A track with a solder-mask opening gets a mask teardrop on F.Mask over its copper one, grown by the board's expansion
    /// (`Inflate( 0.05 mm, ALLOW_ACUTE_CORNERS )`): kicad-cli's outline.
    #[test]
    fn a_track_with_a_mask_opening_gets_a_mask_teardrop() {
        let (mut design, mut model) = two_pad_model();
        model.board.solder_mask.expansion_nm = mm(0.05);
        let rt = design.routing.as_mut().unwrap();
        rt.vias.push(via(150.0, 100.0, 0.8));
        rt.tracks.push(Track { id: "tm".into(), ..track(vec![at(150.0, 100.0), at(155.0, 100.0)], 0.2) });
        design.drawings.get_or_insert_with(Default::default).track_masks = vec![eda_model::track_mask::TrackMask { id: "tm".into(), margin: None }];
        let out = generate_teardrops(&design, &model, &kicad_case_settings(false, false, 1.0));
        assert_eq!(out.len(), 2, "{out:?}");
        let mask = out.iter().find(|z| z.layer == "F.Mask").expect("a mask teardrop");
        assert!(mask.net.is_empty() && mask.teardrop);
        assert_eq!(mask.outline, outline(&[(150846578, 99866735), (150846578, 100133265), (150126558, 100434868), (149941227, 100000000), (150126558, 99565132)]));
    }

    /// The board's teardrops as `Cmd::AddAllTeardrops` leaves them, with ids.
    fn with_teardrops(design: &mut Design, model: &ConstraintModel) {
        let settings = design.routing.as_ref().unwrap().teardrop_settings;
        let fresh = generate_teardrops(design, model, &settings);
        let rt = design.routing.as_mut().unwrap();
        rt.zones.retain(|z| !z.teardrop);
        rt.zones.extend(fresh);
        for (i, z) in rt.zones.iter_mut().enumerate() {
            if z.id.is_empty() {
                z.id = format!("td{i}");
            }
        }
    }

    /// The outlines of the board's teardrops, sorted.
    fn teardrop_outlines(design: &Design) -> Vec<(String, Vec<Point>)> {
        let mut v: Vec<_> = design.routing.as_ref().unwrap().zones.iter().filter(|z| z.teardrop).map(|z| (z.layer.clone(), z.outline.clone())).collect();
        v.sort_by_key(|(l, o)| (l.clone(), o[0].x, o[0].y));
        v
    }

    /// UI-LEFT-2 PP6: `BOARD_COMMIT::Push`'s teardrop update. A track moved on its via has its teardrop made again (the board then has the teardrops a full update
    /// gives it), the other via's teardrop is left as it was (its id kept); a track deleted takes its teardrop with it; a track added to a via makes one.
    #[test]
    fn an_edit_updates_only_the_teardrops_it_makes_stale() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.teardrop_settings = kicad_case_settings(false, false, 1.0);
        rt.vias.push(Via { id: "v1".into(), ..via(100.0, 100.0, 0.8) });
        rt.vias.push(Via { id: "v2".into(), ..via(110.0, 100.0, 0.8) });
        rt.tracks.push(Track { id: "t1".into(), ..track(vec![at(100.0, 100.0), at(105.0, 100.0)], 0.2) });
        rt.tracks.push(Track { id: "t2".into(), ..track(vec![at(110.0, 100.0), at(115.0, 100.0)], 0.2) });
        with_teardrops(&mut design, &model);
        let v2_id = design.routing.as_ref().unwrap().zones.iter().find(|z| point_in_polygon(at(110.0, 100.0), &z.outline)).unwrap().id.clone();
        // Move t1's far end.
        let before = snapshot(&design, &model).expect("teardrops on the board");
        design.routing.as_mut().unwrap().tracks[0].pts[1] = at(104.0, 103.0);
        assert!(update_after_edit(&mut design, &model, &before));
        let mut full = design.clone();
        with_teardrops(&mut full, &model);
        assert_eq!(teardrop_outlines(&design), teardrop_outlines(&full), "the board's teardrops are a full update's");
        assert!(design.routing.as_ref().unwrap().zones.iter().any(|z| z.id == v2_id), "v2's teardrop is not touched");
        // Delete t2: its teardrop goes.
        let before = snapshot(&design, &model).unwrap();
        design.routing.as_mut().unwrap().tracks.retain(|t| t.id != "t2");
        assert!(update_after_edit(&mut design, &model, &before));
        assert_eq!(design.routing.as_ref().unwrap().zones.iter().filter(|z| z.teardrop).count(), 1);
        // Add it back: its teardrop is made.
        let before = snapshot(&design, &model).unwrap();
        design.routing.as_mut().unwrap().tracks.push(Track { id: "t3".into(), ..track(vec![at(110.0, 100.0), at(110.0, 95.0)], 0.2) });
        assert!(update_after_edit(&mut design, &model, &before));
        assert_eq!(design.routing.as_ref().unwrap().zones.iter().filter(|z| z.teardrop).count(), 2);
        // An edit far from every teardrop changes none.
        let before = snapshot(&design, &model).unwrap();
        design.routing.as_mut().unwrap().tracks.push(Track { id: "far".into(), net: "N2".into(), ..track(vec![at(150.0, 150.0), at(155.0, 150.0)], 0.2) });
        assert!(!update_after_edit(&mut design, &model, &before));
    }

    /// UI-LEFT-2 PP6: `RemoveTeardrops`' `foreignNeighbourMoved`. Another net's track moved near a teardrop makes it stale though it anchors on neither: it is fitted
    /// again, narrower, and widens back when the track moves away.
    #[test]
    fn a_teardrop_is_fitted_again_when_another_nets_copper_moves_near_it() {
        let (mut design, model) = two_pad_model();
        let rt = design.routing.as_mut().unwrap();
        rt.teardrop_settings = kicad_case_settings(false, false, 1.0);
        rt.vias.push(Via { id: "v1".into(), ..via(100.0, 100.0, 0.8) });
        rt.tracks.push(Track { id: "t1".into(), ..track(vec![at(100.0, 100.0), at(105.0, 100.0)], 0.2) });
        rt.tracks.push(Track { id: "n2".into(), net: "N2".into(), ..track(vec![at(100.6, 102.48), at(100.6, 105.0)], 0.2) });
        with_teardrops(&mut design, &model);
        let wide = teardrop_outlines(&design);
        let before = snapshot(&design, &model).unwrap();
        design.routing.as_mut().unwrap().tracks[1].pts = vec![at(100.6, 100.48), at(100.6, 103.0)];
        assert!(update_after_edit(&mut design, &model, &before));
        assert_eq!(teardrop_outlines(&design)[0].1, outline(&[(100796578, 100100000), (100796578, 99900000), (100310846, 99750000), (99995578, 100000000), (100310846, 100250000)]));
        let before = snapshot(&design, &model).unwrap();
        design.routing.as_mut().unwrap().tracks[1].pts = vec![at(100.6, 102.48), at(100.6, 105.0)];
        assert!(update_after_edit(&mut design, &model, &before));
        assert_eq!(teardrop_outlines(&design), wide);
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
