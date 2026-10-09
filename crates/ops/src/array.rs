//! Create Array: grid and circular arrays of footprints and of every other board item, in one undo step.
//!
//! Ports `ARRAY_TOOL::CreateArray` (`pcbnew/tools/array_tool.cpp` at KiCad 8303b2ad), `ARRAY_GRID_OPTIONS::GetTransform` and
//! `ARRAY_CIRCULAR_OPTIONS::GetTransform` (`common/array_options.cpp`) and the dialog's own checks
//! (`DIALOG_CREATE_ARRAY::TransferDataFromWindow`, `dialogs/dialog_create_array.cpp`).
//!
//! * **Duplicate** (the dialog's default): every point of the array but one gets a copy of the whole selection -- footprints as new
//!   parts of the board ([`Board::collect_copies`], the machinery of Duplicate and Paste), tracks, vias, zones, graphics, text,
//!   dimensions, a group with copies of its members -- and each copy is moved to its point (`TransformItem`: `Move` by the offset,
//!   then `Rotate` about the item's own new position when the circular array turns its items). The selection itself takes the other
//!   point. KiCad's reverse loop leaves the original in the *last* point and a copy in the first; here the original keeps the first, so
//!   a footprint keeps its reference and what is routed to it, and the points taken (and the references on them) are the same.
//! * **Arrange selection**: nothing is made; the selected items, in the order given, are put on the points one after the other -- all
//!   at the first item's position first, then offset by the point's transform (`m_arrangeSelection`).
//! * **Footprint references**: "Assign unique reference designators" (the dialog's default, `ShouldReannotateFootprints`) gives each
//!   copy the next free number of its reference (`BOARD_REANNOTATE_TOOL::ReannotateDuplicates`, [`unique_reference`]), in the order of
//!   the points; "Keep original reference designators" leaves the text of every copy as the original's -- the copy's id stays unique,
//!   the Reference field shows the original's text.
//! * A pad or a field in the selection stands for its footprint, once (`GetParentFootprint`, `fpDeDupe`).
//!
//! The footprint editor's half of the dialog -- numbering the pads of the new copies (`ARRAY_AXIS`, `ARRAY_PAD_NUMBER_PROVIDER`) -- is
//! not ported: the library editor has no array tool, and the board editor never shows those controls
//! (`enableArrayNumbering = m_isFootprintEditor`). The grid's numbering direction and the serpentine (`horizontal_then_vertical`,
//! `reverse_alternate`) are here because they decide which point is which; the board dialog leaves them at KiCad's defaults.

use super::pcb_paste::{Copies, Member};
use super::pcb_transform::{part_pose, transform_dimension, transform_shape, transform_text, transform_track, transform_via, transform_zone, Xform};
use super::{rotate_point_about, Board};
use eda_model::fp_edit::{parse_field_id, parse_pad_id};
use eda_model::ir::{FootprintInstance, Point, Shape, Um};
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn d_true() -> bool {
    true
}

/// `Cmd::CreateArray`'s geometry -- `ARRAY_GRID_OPTIONS`/`ARRAY_CIRCULAR_OPTIONS` (`include/array_options.h`). A dialog-session object in
/// source too (`ARRAY_OPTIONS` is never written to the board file), so this lives here as a `Cmd` payload, not on the IR.
///
/// Angles are millidegrees in this crate's own `rotate_point_about` convention (positive = clockwise in this app's Y-down board
/// coordinates). `Circular::clockwise` picks the sign applied to the computed angle before rotating, the same final step source's own
/// `GetTransform` takes (`if (m_clockwise) angle = -angle;`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArrayGeometry {
    Grid {
        nx: i64,
        ny: i64,
        dx: Um,
        dy: Um,
        #[serde(default)]
        offset_x: Um,
        #[serde(default)]
        offset_y: Um,
        #[serde(default)]
        centred: bool,
        /// `ARRAY_GRID_OPTIONS::m_stagger` -- a brick/honeycomb offset every `stagger`-th row (or column, see `stagger_rows`). 0 or 1
        /// disables it (source: `std::abs(m_stagger) > 1`); a negative one staggers to the left or up.
        #[serde(default)]
        stagger: i64,
        #[serde(default = "d_true")]
        stagger_rows: bool,
        /// `ARRAY_GRID_OPTIONS::m_horizontalThenVertical` -- the order the points are numbered in: a row before the next row (true), or a
        /// column before the next column.
        #[serde(default = "d_true")]
        horizontal_then_vertical: bool,
        /// `m_reverseNumberingAlternate` -- every other row (or column) runs the other way, so the order of the points is a serpentine.
        #[serde(default)]
        reverse_alternate: bool,
    },
    Circular {
        center: Point,
        count: i64,
        /// Angle between consecutive points; 0 divides the circle evenly between the `count` points.
        angle_millideg: i64,
        #[serde(default)]
        angle_offset_millideg: i64,
        #[serde(default = "d_true")]
        clockwise: bool,
        /// `ARRAY_CIRCULAR_OPTIONS::m_rotateItems` -- turn each item about its own position by the angle it moved by.
        #[serde(default)]
        rotate_items: bool,
    },
}

impl ArrayGeometry {
    /// `ARRAY_OPTIONS::GetArraySize`.
    pub fn size(&self) -> i64 {
        match self {
            ArrayGeometry::Grid { nx, ny, .. } => nx * ny,
            ArrayGeometry::Circular { count, .. } => *count,
        }
    }

    /// `ARRAY_GRID_OPTIONS::getGridCoords`: the column and row of the `n`-th point, in the order the grid is numbered.
    fn grid_coords(n: i64, nx: i64, ny: i64, horizontal_then_vertical: bool, reverse_alternate: bool) -> (i64, i64) {
        let axis_size = (if horizontal_then_vertical { nx } else { ny }).max(1);
        let mut x = n % axis_size;
        let y = n / axis_size;
        if reverse_alternate && y % 2 == 1 {
            x = axis_size - x - 1;
        }
        (x, y)
    }

    /// `ARRAY_OPTIONS::GetTransform`: where the `n`-th point is, as an offset from the item (`pos` is its position), and how far the
    /// item turns there (clockwise millidegrees, 0 unless the array turns its items).
    pub fn transform(&self, n: i64, pos: Point) -> (Point, i64) {
        match *self {
            ArrayGeometry::Grid { nx, ny, dx, dy, offset_x, offset_y, centred, stagger, stagger_rows, horizontal_then_vertical, reverse_alternate } => {
                let (mut cx, mut cy) = Self::grid_coords(n, nx, ny, horizontal_then_vertical, reverse_alternate);
                // swap axes if needed
                if !horizontal_then_vertical {
                    std::mem::swap(&mut cx, &mut cy);
                }
                let mut x = cx * dx + cy * offset_x;
                let mut y = cy * dy + cx * offset_y;
                if stagger.abs() > 1 {
                    let s = stagger.abs();
                    let idx = (if stagger_rows { cy } else { cx }) % s;
                    let (sdx, sdy) = if stagger_rows { (dx, offset_y) } else { (offset_x, dy) };
                    // `stagger_delta * copysign( stagger_idx, m_stagger ) / stagger`, the vector truncated back to integers.
                    let signed = idx * stagger.signum();
                    x += (sdx * signed) / s;
                    y += (sdy * signed) / s;
                }
                // Bump the item by half the array size.
                if centred {
                    let extent_x = (nx - 1) * dx + (ny - 1) * offset_x;
                    let extent_y = (ny - 1) * dy + (nx - 1) * offset_y;
                    x -= extent_x / 2;
                    y -= extent_y / 2;
                }
                (Point { x, y }, 0)
            }
            ArrayGeometry::Circular { center, count, angle_millideg, angle_offset_millideg, clockwise, rotate_items } => {
                // An angle of zero divides the circle evenly between the points.
                let step = if angle_millideg == 0 { 360_000.0 * n as f64 / count.max(1) as f64 } else { (angle_millideg * n) as f64 };
                let total = step.round() as i64 + angle_offset_millideg;
                let signed = if clockwise { total } else { -total };
                let moved = rotate_point_about(pos, center, signed);
                (Point { x: moved.x - pos.x, y: moved.y - pos.y }, if rotate_items { signed } else { 0 })
            }
        }
    }

    /// `DIALOG_CREATE_ARRAY::TransferDataFromWindow`'s checks of the numbers: at least one row and column, no zero spacing between
    /// several, no zero angle between several points. `Err` carries the dialog's message.
    pub fn validate(&self) -> Result<(), String> {
        match *self {
            ArrayGeometry::Grid { nx, ny, dx, dy, .. } => {
                if nx < 1 || ny < 1 {
                    return Err("a grid array needs at least one row and one column".into());
                }
                if nx > 1 && dx == 0 {
                    return Err(format!("horizontal delta of zero with {nx} objects"));
                }
                if ny > 1 && dy == 0 {
                    return Err(format!("vertical delta of zero with {ny} objects"));
                }
            }
            ArrayGeometry::Circular { count, angle_millideg, .. } => {
                if count < 1 {
                    return Err("a circular array needs at least one point".into());
                }
                if count > 1 && angle_millideg == 0 {
                    return Err(format!("angular delta of zero with {count} objects"));
                }
            }
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------------------------------- positions

/// `EDA_SHAPE::getPosition`: the centre of an arc or a circle, the first vertex of a polygon, the start of everything else.
fn shape_position(s: &Shape) -> Point {
    match s {
        Shape::Arc { start, mid, end, .. } => arc_centre(*start, *mid, *end).unwrap_or(*start),
        Shape::Circle { center, .. } => *center,
        other => other.points().first().copied().unwrap_or_default(),
    }
}

/// The centre of the circle through three points; `None` when they are in a line.
fn arc_centre(a: Point, b: Point, c: Point) -> Option<Point> {
    let (ax, ay, bx, by, cx, cy) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64, c.x as f64, c.y as f64);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (ax * ax + ay * ay, bx * bx + by * by, cx * cx + cy * cy);
    Some(Point { x: ((a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d).round() as Um, y: ((a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d).round() as Um })
}

/// The centre of the box the points span (`PCB_GROUP::GetPosition` is the middle of its bounding box).
fn middle(points: &[Point]) -> Option<Point> {
    let first = points.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some(Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 })
}

impl Board<'_> {
    /// `BOARD_ITEM::GetPosition()` of the item `id` names: a footprint's origin, a track's start, a via's centre, a zone's first corner, a
    /// shape's (see [`shape_position`]), a text's anchor, a dimension's first feature point, a group's middle. `None` when nothing on
    /// the board has the id.
    pub(crate) fn item_position(&self, id: &str) -> Option<Point> {
        if let Some(fp) = self.pose_of(id) {
            return Some(fp.at);
        }
        if let Some(rt) = &self.design.routing {
            if let Some(t) = rt.tracks.iter().find(|t| t.id == id) {
                return t.pts.first().copied();
            }
            if let Some(v) = rt.vias.iter().find(|v| v.id == id) {
                return Some(v.at);
            }
            if let Some(z) = rt.zones.iter().find(|z| z.id == id) {
                return z.outline.first().copied();
            }
        }
        let dr = self.design.drawings.as_ref()?;
        if let Some(s) = dr.shapes.iter().find(|s| s.id() == id) {
            return Some(shape_position(s));
        }
        if let Some(t) = dr.texts.iter().find(|t| t.id == id) {
            return Some(t.at);
        }
        if let Some(d) = dr.dimensions.iter().find(|d| d.id == id) {
            return Some(d.start);
        }
        if let Some(g) = dr.groups.iter().find(|g| g.id == id) {
            let members: Vec<Point> = g.member_ids.iter().filter_map(|m| self.item_position(m)).collect();
            return middle(&members);
        }
        None
    }

    /// The items an array works on: the ids given with a pad or a field standing for its footprint, once; an id that names nothing on
    /// the board dropped. Order is the order given (`GetItemsSortedBySelectionOrder`).
    pub(crate) fn array_selection(&self, ids: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for id in ids {
            let item = match (parse_pad_id(id), parse_field_id(id)) {
                (Some((r, number, nth)), _) if self.pose_of(&r).is_some() && self.pad_exists(&r, &number, nth) => r,
                (_, Some((r, _))) if self.resolve_field(id).is_some() => r.to_string(),
                _ => id.clone(),
            };
            if self.item_position(&item).is_some() && seen.insert(item.clone()) {
                out.push(item);
            }
        }
        out
    }

    fn pad_exists(&self, reference: &str, number: &str, nth: u32) -> bool {
        self.model.part(reference).and_then(|p| self.model.footprint_of(p)).is_some_and(|f| eda_model::fp_edit::pad_index(&f, number, nth).is_some())
    }

    /// `Cmd::CreateArray`. See the module doc.
    pub(crate) fn create_array(&mut self, ids: &[String], geometry: &ArrayGeometry, arrange: bool, reannotate: bool) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_array", "array", "no ids given")]);
        }
        geometry.validate().map_err(|m| vec![CheckResult::fail("ops_bad_array", "array", m)])?;
        let selection = self.array_selection(ids);
        if selection.is_empty() {
            return Err(vec![CheckResult::fail("ops_unknown_array", "array", "none of the given ids name a footprint, track, via, zone, shape, text, dimension or group on the board")]);
        }
        if arrange {
            self.arrange_into_array(&selection, geometry)
        } else {
            self.duplicate_into_array(&selection, geometry, reannotate)
        }
    }

    /// `TransformItem` for an item on the board: move it by the transform's offset, then turn it about where it now is.
    fn transform_on_board(&mut self, id: &str, geometry: &ArrayGeometry, index: i64) -> Result<(), Vec<CheckResult>> {
        let Some(pos) = self.item_position(id) else { return Ok(()) };
        let (offset, turn) = geometry.transform(index, pos);
        if offset != (Point { x: 0, y: 0 }) {
            self.transform_items("array", std::slice::from_ref(&id.to_string()), Xform::Move { dx: offset.x, dy: offset.y })?;
        }
        if turn != 0 {
            let at = self.item_position(id).unwrap_or(pos);
            self.transform_items("array", std::slice::from_ref(&id.to_string()), Xform::Rotate { pivot: at, angle: turn })?;
        }
        Ok(())
    }

    /// `ARRAY_TOOL::CreateArray`'s `ShouldArrangeSelection()` branch: the items, in order, onto the points. A footprint is moved whole.
    fn arrange_into_array(&mut self, selection: &[String], geometry: &ArrayGeometry) -> Result<(), Vec<CheckResult>> {
        let size = geometry.size();
        let mut first: Option<Point> = None;
        for (index, id) in selection.iter().enumerate() {
            if index as i64 >= size {
                break;
            }
            if let Some(origin) = first {
                // Every item after the first starts at the first item's position.
                let here = self.item_position(id).expect("the selection holds items on the board");
                if here != origin {
                    self.transform_items("array", std::slice::from_ref(id), Xform::Move { dx: origin.x - here.x, dy: origin.y - here.y })?;
                }
            }
            self.transform_on_board(id, geometry, index as i64)?;
            if first.is_none() {
                first = self.item_position(id);
            }
        }
        Ok(())
    }

    /// The default branch: one set of copies for every point after the first, each moved to its point, and the selection to the first.
    fn duplicate_into_array(&mut self, selection: &[String], geometry: &ArrayGeometry, reannotate: bool) -> Result<(), Vec<CheckResult>> {
        let size = geometry.size();
        let copper = self.model.board.layers.len();
        let outer = (self.model.board.layers.first().cloned(), self.model.board.layers.last().cloned());
        let mut all = Copies::default();
        for index in 1..size {
            let mut block = self.collect_copies(selection)?;
            if !reannotate {
                for f in block.footprints.iter_mut() {
                    f.shown_reference = Some(f.reference.clone());
                }
            }
            transform_block(&mut block, geometry, index, copper, &outer);
            all.absorb(block);
        }
        // Copies first, with unique ids and references in the order of the points; then the selection moves to the first point.
        if !all.is_empty() {
            self.insert_copies_all(all)?;
        }
        for id in selection {
            self.transform_on_board(id, geometry, 0)?;
        }
        Ok(())
    }
}

// --------------------------------------------------------------------------------------------------- copies

fn move_by(o: Point) -> Xform {
    Xform::Move { dx: o.x, dy: o.y }
}

/// `TransformItem` over a set of not-yet-placed copies: each one (a group's members as one) moves to point `index` and turns there.
fn transform_block(block: &mut Copies, g: &ArrayGeometry, index: i64, copper: usize, outer: &(Option<String>, Option<String>)) {
    // Members of a copied group are transformed as the group is (`PCB_GROUP::Move`/`Rotate`), about the group's position.
    let mut in_group: BTreeSet<(u8, usize)> = BTreeSet::new();
    let key = |m: &Member| -> (u8, usize) {
        match *m {
            Member::Track(i) => (0, i),
            Member::Via(i) => (1, i),
            Member::Zone(i) => (2, i),
            Member::Shape(i) => (3, i),
            Member::Text(i) => (4, i),
            Member::Dimension(i) => (5, i),
            Member::Footprint(i) => (6, i),
        }
    };
    let position = |block: &Copies, m: &Member| -> Option<Point> {
        match *m {
            Member::Track(i) => block.tracks[i].pts.first().copied(),
            Member::Via(i) => Some(block.vias[i].at),
            Member::Zone(i) => block.zones[i].outline.first().copied(),
            Member::Shape(i) => Some(shape_position(&block.shapes[i])),
            Member::Text(i) => Some(block.texts[i].at),
            Member::Dimension(i) => Some(block.dimensions[i].start),
            Member::Footprint(i) => Some(block.footprints[i].at),
        }
    };
    let apply = |block: &mut Copies, m: &Member, x: Xform| match *m {
        Member::Track(i) => transform_track(&mut block.tracks[i], x, copper),
        Member::Via(i) => transform_via(&mut block.vias[i], x, copper, outer),
        Member::Zone(i) => transform_zone(&mut block.zones[i], x, copper),
        Member::Shape(i) => transform_shape(&mut block.shapes[i], x, copper),
        Member::Text(i) => transform_text(&mut block.texts[i], x, copper),
        Member::Dimension(i) => transform_dimension(&mut block.dimensions[i], x, copper),
        Member::Footprint(i) => {
            let f = &mut block.footprints[i];
            let pose = FootprintInstance { id: f.reference.clone(), at: f.at, rot: f.rot, side: f.side, label: f.label };
            let (at, rot, side, label) = part_pose(&pose, x);
            f.at = at;
            f.rot = rot;
            f.side = side;
            f.label = label;
        }
    };

    for gi in 0..block.groups.len() {
        let members = block.groups[gi].members.clone();
        for m in &members {
            in_group.insert(key(m));
        }
        let points: Vec<Point> = members.iter().filter_map(|m| position(block, m)).collect();
        let Some(pos) = middle(&points) else { continue };
        let (offset, turn) = g.transform(index, pos);
        for m in &members {
            apply(block, m, move_by(offset));
        }
        if turn != 0 {
            let pivot = Point { x: pos.x + offset.x, y: pos.y + offset.y };
            for m in &members {
                apply(block, m, Xform::Rotate { pivot, angle: turn });
            }
        }
    }

    let mut each: Vec<Member> = Vec::new();
    each.extend((0..block.tracks.len()).map(Member::Track));
    each.extend((0..block.vias.len()).map(Member::Via));
    each.extend((0..block.zones.len()).map(Member::Zone));
    each.extend((0..block.shapes.len()).map(Member::Shape));
    each.extend((0..block.texts.len()).map(Member::Text));
    each.extend((0..block.dimensions.len()).map(Member::Dimension));
    each.extend((0..block.footprints.len()).map(Member::Footprint));
    for m in each.into_iter().filter(|m| !in_group.contains(&key(m))) {
        let Some(pos) = position(block, &m) else { continue };
        let (offset, turn) = g.transform(index, pos);
        apply(block, &m, move_by(offset));
        if turn != 0 {
            let at = position(block, &m).unwrap_or(pos);
            apply(block, &m, Xform::Rotate { pivot: at, angle: turn });
        }
    }
}
