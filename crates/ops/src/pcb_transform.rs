//! Move, Rotate and Flip for every kind of PCB item, as three undoable verbs.
//!
//! Ports `EDIT_TOOL::Move`, `EDIT_TOOL::Rotate` and `EDIT_TOOL::Flip`
//! (`pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp` at KiCad 8303b2ad) down to what each item class does
//! with the transform: `BOARD_ITEM::Move`, `::Rotate`, `::Flip` of `PCB_TRACK`/`PCB_ARC`/`PCB_VIA`, `ZONE`,
//! `PCB_SHAPE` (`EDA_SHAPE::rotate`/`::flip`), `PCB_TEXT`, `PCB_DIMENSION_BASE` and its ortho/aligned subclasses,
//! `FOOTPRINT` and `PCB_GROUP`. The tool part of the real command (which items, about which point, which items a
//! lock keeps out) is the caller's: the studio reads KiCad's rules for those in
//! `web/studio/src/kicad-port/pcbTransform.ts` and sends the result here as one command, so one undo step covers
//! the whole selection (one `BOARD_COMMIT::Push` in source).
//!
//! # Conventions
//!
//! * Angles are millidegrees, **positive turns clockwise on the screen** (Y grows downward), the sense of
//!   `rotate_point_about` and of a footprint's `rot`. KiCad's own angles run the other way (`RotatePoint( +90 )`
//!   is counter-clockwise on the screen); the studio negates where it ports a KiCad angle.
//! * An item keeps its id: ids are assigned once, never recomputed from the new geometry (see `Track::id`).
//! * A group moves, turns and flips as its members do; the members that are footprints included.
//! * Placed parts land on the placement grid like every other pose here (`Board::set_pose`).
//! * Locks are the caller's business, as they are in source: `FilterCollectorForLockedItems` keeps locked items out
//!   of the selection the tool then works on; the item's own `Move`/`Rotate`/`Flip` never look at them.

use super::{rotate_point_about, Board};
use eda_model::footprint::PlacedPad;
use eda_model::ir::{tessellate_arc, Design, Dimension, DimensionKind, FootprintInstance, LabelSide, Millideg, Point, RoutingSection, Shape, Side, Text, Track, Um, Via, Zone, TRACK_ARC_SEGMENTS};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `FLIP_DIRECTION` (`include/geometry/flip_direction.h`): which way an item is turned over. `LeftRight` mirrors
/// across a vertical axis (x changes), KiCad's default for `Change Side / Flip` (`m_FlipDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlipDirection {
    LeftRight,
    TopBottom,
}

/// One rigid transform of the board plane, applied to the points, the vectors and the orientations of an item.
#[derive(Debug, Clone, Copy)]
enum Xform {
    Move { dx: Um, dy: Um },
    /// Clockwise-positive millidegrees about `pivot`.
    Rotate { pivot: Point, angle: i64 },
    /// `Flip( aCentre, aFlipDirection )`: mirror across the axis through `pivot`, and turn the item over to the other
    /// side of the board.
    Flip { pivot: Point, dir: FlipDirection },
}

impl Xform {
    fn point(self, p: Point) -> Point {
        match self {
            Xform::Move { dx, dy } => Point { x: p.x + dx, y: p.y + dy },
            Xform::Rotate { pivot, angle } => rotate_point_about(p, pivot, angle),
            Xform::Flip { pivot, dir: FlipDirection::LeftRight } => Point { x: 2 * pivot.x - p.x, y: p.y },
            Xform::Flip { pivot, dir: FlipDirection::TopBottom } => Point { x: p.x, y: 2 * pivot.y - p.y },
        }
    }

    /// The same transform on an offset between two points of one item (the mid point of an arc relative to its
    /// start): a translation leaves it alone.
    fn vector(self, v: Point) -> Point {
        match self {
            Xform::Move { .. } => v,
            Xform::Rotate { angle, .. } => rotate_point_about(v, Point { x: 0, y: 0 }, angle),
            Xform::Flip { dir: FlipDirection::LeftRight, .. } => Point { x: -v.x, y: v.y },
            Xform::Flip { dir: FlipDirection::TopBottom, .. } => Point { x: v.x, y: -v.y },
        }
    }

    fn is_flip(self) -> bool {
        matches!(self, Xform::Flip { .. })
    }
}

/// `::FlipLayer( aLayerId, aCopperLayersCount )` (`common/layer_id.cpp`): the layer on the other side of the board.
/// Front and back pairs swap; with four or more copper layers the inner ones mirror through the middle of the stack
/// (`In1` <-> `In2` on four layers, `In1` <-> `In4` and `In2` <-> `In3` on six); every other layer stays.
pub fn flip_layer(layer: &str, copper_layers: usize) -> String {
    const PAIRS: [(&str, &str); 7] = [("F.Cu", "B.Cu"), ("F.SilkS", "B.SilkS"), ("F.Adhes", "B.Adhes"), ("F.Mask", "B.Mask"), ("F.Paste", "B.Paste"), ("F.CrtYd", "B.CrtYd"), ("F.Fab", "B.Fab")];
    for (front, back) in PAIRS {
        if layer == front {
            return back.to_string();
        }
        if layer == back {
            return front.to_string();
        }
    }
    if copper_layers >= 4 {
        if let Some(n) = layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|d| d.parse::<i64>().ok()) {
            let inner_index = n - 1;
            let max_index = copper_layers as i64 - 3;
            let flipped = (max_index - inner_index).clamp(0, max_index);
            return format!("In{}.Cu", flipped + 1);
        }
    }
    layer.to_string()
}

/// `BOARD_ITEM::IsSideSpecific`: a layer that belongs to one side of the board, so text on it reads mirrored from the
/// back (`LSET::SideSpecificMask`: the front and back technical layers and every copper layer).
fn side_specific(layer: &str) -> bool {
    layer.starts_with("F.") || layer.starts_with("B.") || (layer.starts_with("In") && layer.ends_with(".Cu"))
}

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// The items a verb names, by kind, with groups already replaced by their members.
#[derive(Default)]
struct Targets {
    parts: BTreeSet<String>,
    tracks: BTreeSet<String>,
    vias: BTreeSet<String>,
    zones: BTreeSet<String>,
    shapes: BTreeSet<String>,
    texts: BTreeSet<String>,
    dimensions: BTreeSet<String>,
}

impl Targets {
    fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.tracks.is_empty() && self.vias.is_empty() && self.zones.is_empty() && self.shapes.is_empty() && self.texts.is_empty() && self.dimensions.is_empty()
    }
}

/// The placed footprints `ids` name, directly or as members of a group they name.
fn named_footprints<'d>(design: &'d Design, ids: &[String]) -> Vec<&'d FootprintInstance> {
    let Some(placement) = design.placement.as_ref() else { return vec![] };
    let mut named: BTreeSet<&str> = BTreeSet::new();
    for id in ids {
        match design.drawings.as_ref().and_then(|d| d.groups.iter().find(|g| &g.id == id)) {
            Some(g) => named.extend(g.member_ids.iter().map(String::as_str)),
            None => {
                named.insert(id.as_str());
            }
        }
    }
    placement.footprints.iter().filter(|f| named.contains(f.id.as_str())).collect()
}

/// Whether `p` lies in the box `(x0, y0, x1, y1)`.
fn in_box(p: Point, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
    (x0..=x1).contains(&(p.x as f64)) && (y0..=y1).contains(&(p.y as f64))
}

/// A piece of routed copper, by where it is in the routing.
#[derive(PartialEq, Clone, Copy)]
enum Copper {
    Track(usize),
    Via(usize),
}

/// The routed copper held by `pad`: a track that starts or ends on it (a router's track runs from pad to pad) and a via on it. A track
/// that merely passes over a pad is not routed to it.
fn copper_on(routing: &RoutingSection, pad: &PlacedPad) -> Vec<Copper> {
    let (hw, hh) = (pad.size.0 as f64 / 2.0, pad.size.1 as f64 / 2.0);
    let (cx, cy) = (pad.center.x as f64, pad.center.y as f64);
    let reaches = |half_width: Um| (cx - hw - half_width as f64, cy - hh - half_width as f64, cx + hw + half_width as f64, cy + hh + half_width as f64);
    let mut out = vec![];
    for (i, t) in routing.tracks.iter().enumerate() {
        let near = reaches(t.width / 2);
        if [t.pts.first(), t.pts.last()].into_iter().flatten().any(|&p| in_box(p, near)) {
            out.push(Copper::Track(i));
        }
    }
    for (i, v) in routing.vias.iter().enumerate() {
        if in_box(v.at, reaches(v.diameter / 2)) {
            out.push(Copper::Via(i));
        }
    }
    out
}

/// Whether two pads' boxes overlap (pads of a footprint and of the copy that sits on it).
fn pads_overlap(a: &PlacedPad, b: &PlacedPad) -> bool {
    (a.center.x - b.center.x).abs() * 2 < a.size.0 + b.size.0 && (a.center.y - b.center.y).abs() * 2 < a.size.1 + b.size.1
}

impl Board<'_> {
    /// Whether moving, turning, flipping or taking away the footprints `ids` name (directly, or as members of a group) would leave routed
    /// copper behind: a track that ends on, or a via that sits on, a pad of theirs and is not still held by a pad of a footprint that stays where it
    /// is *at that very spot* (the original under a fresh copy of it). This model answers it by clearing the routing
    /// (`Cmd::clears_routing`); copper, graphics and text on their own never do, and neither does a footprint nothing was routed to.
    /// A footprint whose pads cannot be worked out counts as routed -- the safe answer.
    pub(crate) fn names_routed_part(&self, ids: &[String]) -> bool {
        let Some(routing) = self.design.routing.as_ref() else { return false };
        let named = named_footprints(&self.design, ids);
        if named.is_empty() {
            return false;
        }
        let pads_of = |fp: &FootprintInstance| self.model.part(&fp.id).and_then(|part| eda_model::footprint::placed_pads(self.model, part, fp));
        let mut moving: Vec<PlacedPad> = vec![];
        for fp in &named {
            match pads_of(fp) {
                Some(pads) => moving.extend(pads),
                None => return true,
            }
        }
        let staying: Vec<PlacedPad> = self.design.placement.iter().flat_map(|p| p.footprints.iter()).filter(|f| !named.iter().any(|n| n.id == f.id)).filter_map(|f| pads_of(f)).flatten().collect();
        for pad in &moving {
            let held_by: Vec<Vec<Copper>> = staying.iter().filter(|q| pads_overlap(pad, q)).map(|q| copper_on(routing, q)).collect();
            if copper_on(routing, pad).iter().any(|c| !held_by.iter().any(|h| h.contains(c))) {
                return true;
            }
        }
        false
    }
}

impl Board<'_> {
    /// Files `id` under the collection it belongs to. `false` when nothing on the board has the id.
    fn file_item(&self, id: &str, out: &mut Targets) -> bool {
        if self.pose_of(id).is_some() {
            out.parts.insert(id.to_string());
            return true;
        }
        if let Some(rt) = &self.design.routing {
            if rt.tracks.iter().any(|t| t.id == id) {
                out.tracks.insert(id.to_string());
                return true;
            }
            if rt.vias.iter().any(|v| v.id == id) {
                out.vias.insert(id.to_string());
                return true;
            }
            if rt.zones.iter().any(|z| z.id == id) {
                out.zones.insert(id.to_string());
                return true;
            }
        }
        let Some(dr) = &self.design.drawings else { return false };
        if dr.shapes.iter().any(|s| s.id() == id) {
            out.shapes.insert(id.to_string());
        } else if dr.texts.iter().any(|t| t.id == id) {
            out.texts.insert(id.to_string());
        } else if dr.dimensions.iter().any(|d| d.id == id) {
            out.dimensions.insert(id.to_string());
        } else {
            return false;
        }
        true
    }

    /// Every item `ids` names. A group stands for its members (`PCB_GROUP::Move`/`Rotate`/`Flip` act on each); an id
    /// that is not on the board refuses the whole command, so nothing is half applied.
    fn resolve_targets(&self, verb: &str, ids: &[String]) -> Result<Targets, Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_transform", verb, "no items given")]);
        }
        let mut out = Targets::default();
        for id in ids {
            let group = self.design.drawings.as_ref().and_then(|d| d.groups.iter().find(|g| &g.id == id));
            if let Some(g) = group {
                // A member the board no longer has (a stale group) is skipped, as everywhere else groups are read.
                for m in &g.member_ids {
                    self.file_item(m, &mut out);
                }
            } else if !self.file_item(id, &mut out) {
                return Err(vec![CheckResult::fail("ops_unknown_item", id, "no placed part, track, via, zone, shape, text, dimension or group with this id")]);
            }
        }
        if out.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_transform", verb, "none of the items has anything on the board to transform")]);
        }
        Ok(out)
    }

    /// `Cmd::MoveItems`.
    pub(crate) fn move_items(&mut self, ids: &[String], dx: Um, dy: Um) -> Result<(), Vec<CheckResult>> {
        self.transform_items("move_items", ids, Xform::Move { dx, dy })
    }

    /// `Cmd::RotateItems`.
    pub(crate) fn rotate_items(&mut self, ids: &[String], pivot: Point, angle_millideg: i64) -> Result<(), Vec<CheckResult>> {
        self.transform_items("rotate_items", ids, Xform::Rotate { pivot, angle: angle_millideg })
    }

    /// `Cmd::FlipItems`.
    pub(crate) fn flip_items(&mut self, ids: &[String], pivot: Point, direction: FlipDirection) -> Result<(), Vec<CheckResult>> {
        self.transform_items("flip_items", ids, Xform::Flip { pivot, dir: direction })
    }

    fn transform_items(&mut self, verb: &str, ids: &[String], x: Xform) -> Result<(), Vec<CheckResult>> {
        let targets = self.resolve_targets(verb, ids)?;
        let copper = self.model.board.layers.len();
        let outer = (self.model.board.layers.first().cloned(), self.model.board.layers.last().cloned());

        // Footprints first: `set_pose` takes `&mut self`, and the collections below borrow the design.
        for r in &targets.parts {
            let fp = self.require_placed(r)?;
            let (at, rot, side, label) = part_pose(&fp, x);
            self.set_pose(r, at, rot)?;
            let list = &mut self.design.placement.as_mut().expect("a Board always carries a placement section").footprints;
            if let Some(f) = list.iter_mut().find(|f| &f.id == r) {
                f.side = side;
                f.label = label;
            }
        }

        if let Some(rt) = self.design.routing.as_mut() {
            for t in rt.tracks.iter_mut().filter(|t| targets.tracks.contains(&t.id)) {
                transform_track(t, x, copper);
            }
            for v in rt.vias.iter_mut().filter(|v| targets.vias.contains(&v.id)) {
                transform_via(v, x, copper, &outer);
            }
            for z in rt.zones.iter_mut().filter(|z| targets.zones.contains(&z.id)) {
                transform_zone(z, x, copper);
            }
        }
        if let Some(dr) = self.design.drawings.as_mut() {
            for s in dr.shapes.iter_mut().filter(|s| targets.shapes.contains(s.id())) {
                transform_shape(s, x, copper);
            }
            for t in dr.texts.iter_mut().filter(|t| targets.texts.contains(&t.id)) {
                transform_text(t, x, copper);
            }
            for d in dr.dimensions.iter_mut().filter(|d| targets.dimensions.contains(&d.id)) {
                transform_dimension(d, x, copper);
            }
        }
        if !targets.tracks.is_empty() || !targets.vias.is_empty() {
            self.sort_routing();
        }
        Ok(())
    }
}

// ----------------------------------------------------------------- footprints

/// `FOOTPRINT::Move`/`::Rotate`/`::Flip` for this model's pose: the position follows the transform, and the
/// orientation and side follow the item.
///
/// A flip is KiCad's: `FOOTPRINT::Flip` mirrors across a horizontal axis (Y changes) and turns the part over, and for
/// `LEFT_RIGHT` then rotates it half a turn about the pivot, which in the end mirrors X and leaves Y alone. This
/// model draws a bottom-side part as its top-side pads with x negated, then turned by `rot` (`footprint::to_board`),
/// so the mirror of a part turned by `rot` is the other side's part turned by `-rot` (left-right) or `180 - rot`
/// (top-bottom). The refdes label sits on the mirrored side of the courtyard.
fn part_pose(fp: &FootprintInstance, x: Xform) -> (Point, u32, Side, LabelSide) {
    let at = x.point(fp.at);
    match x {
        Xform::Move { .. } => (at, fp.rot, fp.side, fp.label),
        Xform::Rotate { angle, .. } => (at, norm360(fp.rot as i64 + angle) as u32, fp.side, fp.label),
        Xform::Flip { dir, .. } => {
            let rot = match dir {
                FlipDirection::LeftRight => norm360(-(fp.rot as i64)),
                FlipDirection::TopBottom => norm360(180_000 - fp.rot as i64),
            };
            let side = if fp.side == Side::Top { Side::Bottom } else { Side::Top };
            let label = match (dir, fp.label) {
                (FlipDirection::LeftRight, LabelSide::Left) => LabelSide::Right,
                (FlipDirection::LeftRight, LabelSide::Right) => LabelSide::Left,
                (FlipDirection::TopBottom, LabelSide::Above) => LabelSide::Below,
                (FlipDirection::TopBottom, LabelSide::Below) => LabelSide::Above,
                (_, l) => l,
            };
            (at, rot as u32, side, label)
        }
    }
}

// --------------------------------------------------------------------- copper

/// A true arc is rebuilt from its three points rather than carried sample by sample: its polyline is the arc's
/// tessellation, and a rotated or mirrored copy of the old samples can be a micrometre off the new arc's own
/// (`Track::arc` would then refuse it, and the writer would turn the arc into 32 straight segments).
fn transform_track(t: &mut Track, x: Xform, copper: usize) {
    if let Some((start, mid, end)) = t.arc() {
        let (s, m, e) = (x.point(start), x.point(mid), x.point(end));
        t.pts = tessellate_arc(s, m, e, TRACK_ARC_SEGMENTS);
        t.arc_mid_offset = Some(Point { x: m.x - s.x, y: m.y - s.y });
    } else {
        for p in t.pts.iter_mut() {
            *p = x.point(*p);
        }
        if let Some(off) = t.arc_mid_offset {
            t.arc_mid_offset = Some(x.vector(off));
        }
    }
    if x.is_flip() {
        t.layer = flip_layer(&t.layer, copper);
    }
}

/// `PCB_VIA::Flip`: the position mirrors; a blind or buried via changes its layer pair, a through via has none to
/// change (`GetViaType() != VIATYPE::THROUGH`). A through via spans the outer layers.
fn transform_via(v: &mut Via, x: Xform, copper: usize, outer: &(Option<String>, Option<String>)) {
    v.at = x.point(v.at);
    if x.is_flip() {
        let is_through = match outer {
            (Some(first), Some(last)) => (v.from_layer == *first && v.to_layer == *last) || (v.from_layer == *last && v.to_layer == *first),
            _ => false,
        };
        if !is_through {
            v.from_layer = flip_layer(&v.from_layer, copper);
            v.to_layer = flip_layer(&v.to_layer, copper);
        }
    }
}

/// `ZONE::Move`/`::Rotate`/`::Flip` (`Mirror` of the outline, then the layer flips). The fill is derived from the
/// outline every time it is wanted, so there is nothing else to carry.
fn transform_zone(z: &mut Zone, x: Xform, copper: usize) {
    for p in z.outline.iter_mut() {
        *p = x.point(*p);
    }
    if x.is_flip() {
        z.layer = flip_layer(&z.layer, copper);
    }
}

// ------------------------------------------------------------------- graphics

/// `EDA_SHAPE::Normalize` for a rectangle: start at the top left, end at the bottom right.
fn norm_rect(start: &mut Point, end: &mut Point) {
    let (x0, x1) = (start.x.min(end.x), start.x.max(end.x));
    let (y0, y1) = (start.y.min(end.y), start.y.max(end.y));
    *start = Point { x: x0, y: y0 };
    *end = Point { x: x1, y: y1 };
}

/// `EDA_SHAPE::rotate`/`::flip` and `PCB_SHAPE::Flip`. A rectangle turned by anything but a quarter turn becomes a
/// polygon (`m_shape = SHAPE_T::POLY`); every other rectangle is normalised after a turn or a flip (`Normalize`),
/// start at the top left. A mirrored arc swaps its ends, as in source, so it still runs counter-clockwise from start
/// to end; the three points describe the same arc either way.
fn transform_shape(s: &mut Shape, x: Xform, copper: usize) {
    let flip = x.is_flip();
    let moving = matches!(x, Xform::Move { .. });
    let mut as_polygon: Option<Shape> = None;
    match s {
        Shape::Segment { start, end, .. } | Shape::Circle { center: start, end, .. } => {
            *start = x.point(*start);
            *end = x.point(*end);
        }
        Shape::Arc { start, mid, end, .. } => {
            *start = x.point(*start);
            *mid = x.point(*mid);
            *end = x.point(*end);
            if flip {
                std::mem::swap(start, end);
            }
        }
        Shape::Rect { id, layer, stroke_width, filled, start, end } => {
            if let Xform::Rotate { angle, .. } = x {
                if angle.rem_euclid(90_000) != 0 {
                    let (x0, y0, x1, y1) = (start.x.min(end.x), start.y.min(end.y), start.x.max(end.x), start.y.max(end.y));
                    let corners = [Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }];
                    as_polygon = Some(Shape::Polygon { id: std::mem::take(id), layer: layer.clone(), stroke_width: *stroke_width, filled: *filled, pts: corners.iter().map(|c| x.point(*c)).collect() });
                }
            }
            if as_polygon.is_none() {
                *start = x.point(*start);
                *end = x.point(*end);
                if !moving {
                    norm_rect(start, end);
                }
            }
        }
        Shape::Polygon { pts, .. } => {
            for p in pts.iter_mut() {
                *p = x.point(*p);
            }
        }
        Shape::Bezier { start, c1, c2, end, .. } => {
            *start = x.point(*start);
            *c1 = x.point(*c1);
            *c2 = x.point(*c2);
            *end = x.point(*end);
        }
    }
    if let Some(poly) = as_polygon {
        *s = poly;
    }
    if flip {
        let flipped = flip_layer(s.layer(), copper);
        s.set_layer(flipped);
    }
}

/// `PCB_TEXT::Move`/`::Rotate`/`::Flip`. A text's angle is KiCad's (counter-clockwise on the screen), so a clockwise
/// turn takes from it. Flipped left-right the angle negates; flipped top-bottom it becomes `180 - angle`. A text on a
/// side-specific layer reads mirrored from the back, and turning it over toggles that.
fn transform_text(t: &mut Text, x: Xform, copper: usize) {
    t.at = x.point(t.at);
    match x {
        Xform::Move { .. } => {}
        Xform::Rotate { angle, .. } => t.angle = norm360(t.angle as i64 - angle) as Millideg,
        Xform::Flip { dir, .. } => {
            t.angle = match dir {
                FlipDirection::LeftRight => norm360(-(t.angle as i64)),
                FlipDirection::TopBottom => norm360(180_000 - t.angle as i64),
            } as Millideg;
            t.layer = flip_layer(&t.layer, copper);
            if side_specific(&t.layer) {
                t.mirror = !t.mirror;
            }
        }
    }
}

// ----------------------------------------------------------------- dimensions

/// `PCB_DIMENSION_BASE::Move`/`::Rotate`/`::Flip` and the kind-specific overrides: `PCB_DIM_ORTHOGONAL::Rotate`
/// turns the crossbar to the nearest quarter turn, `PCB_DIM_ALIGNED::Mirror` and `PCB_DIM_ORTHOGONAL::Mirror` move
/// the side the crossbar sits on with the mirror. The text and the lines are worked out again from the two feature
/// points, so only those, the kind's own numbers and the text angle are carried.
fn transform_dimension(d: &mut Dimension, x: Xform, copper: usize) {
    match x {
        Xform::Move { dx, dy } => eda_connectivity::dimension::translate_dimension(d, dx, dy),
        Xform::Rotate { pivot, angle } => {
            ortho_for_rotation(d, angle);
            eda_connectivity::dimension::rotate_dimension(d, pivot, angle);
        }
        Xform::Flip { dir, .. } => {
            d.start = x.point(d.start);
            d.end = x.point(d.end);
            d.text_angle = norm360(-(d.text_angle as i64)) as Millideg;
            match &mut d.kind {
                DimensionKind::Aligned { height } => *height = -*height,
                // "Only reverse the height if the height is aligned with the flip".
                DimensionKind::Orthogonal { height, horizontal } => {
                    if (*horizontal && dir == FlipDirection::TopBottom) || (!*horizontal && dir == FlipDirection::LeftRight) {
                        *height = -*height;
                    }
                }
                DimensionKind::Radial { .. } | DimensionKind::Leader | DimensionKind::Center => {}
            }
            d.layer = flip_layer(&d.layer, copper);
        }
    }
}

/// `PCB_DIM_ORTHOGONAL::Rotate`'s adjustment before the points turn: the crossbar is axis-locked, so a turn of about
/// 90 degrees makes a horizontal one vertical and the other way round, with the height changing sign where the side it
/// is on swaps; about 180 degrees only the height changes sign. The angle is KiCad's there (counter-clockwise), ours
/// is clockwise.
fn ortho_for_rotation(d: &mut Dimension, angle_cw: i64) {
    let DimensionKind::Orthogonal { height, horizontal } = &mut d.kind else { return };
    // `angle.Normalize180()`: -179.999 to 180 degrees, in KiCad's sense.
    let mut a = norm360(-angle_cw);
    if a > 180_000 {
        a -= 360_000;
    }
    if a > 45_000 && a <= 135_000 {
        if *horizontal {
            *horizontal = false;
        } else {
            *horizontal = true;
            *height = -*height;
        }
    } else if a < -45_000 && a >= -135_000 {
        if *horizontal {
            *horizontal = false;
            *height = -*height;
        } else {
            *horizontal = true;
        }
    } else if a > 135_000 || a < -135_000 {
        *height = -*height;
    }
}
