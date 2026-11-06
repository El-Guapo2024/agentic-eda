//! The text boxes, reference images, points and barcodes of a footprint on the board (UI-LEFT P17), and its dimensions and tables (UI-LEFT PP14): moved, turned and
//! flipped with the footprint.
//!
//! KiCad keeps them on the `FOOTPRINT` (`m_drawings` holds the text boxes, images and barcodes, `m_points` the points), in board coordinates, and every
//! transform of the footprint is passed on to each of them: `FOOTPRINT::SetPosition` moves them, `FOOTPRINT::SetOrientation` turns them about the footprint's
//! position by the change of angle (`item->Rotate( rotationCenter, angleChange )`), and `FOOTPRINT::Flip` flips each one top to bottom about the footprint's
//! position (`item->Flip( m_pos, FLIP_DIRECTION::TOP_BOTTOM )`) and then turns the whole footprint half a turn for a left-right flip.
//!
//! Here a placed footprint is a pose ([`FootprintInstance`]) and its items are held in the frame of its pads -- the footprint as seen from the top, un-rotated,
//! as [`crate::fp_art`] holds the graphics -- in [`crate::ir::FootprintExtra::items`], the footprint's own (each footprint on a board owns its items, as
//! KiCad's does). [`placed`] takes them to the board for the pose: a left-right flip about the origin when the footprint is on the back (`to_board` mirrors x),
//! the turn of the pose about the origin, the move to its position -- each step the item's own `Flip`, `Rotate` and `Move`.
//!
//! The transforms themselves are the board's, ported once here and used by the board's verbs too (`eda_ops::pcb_items` hands its `Xform` over):
//! `PCB_TEXTBOX::Move` / `Rotate` / `Flip` (with `EDA_SHAPE::rotate` / `flip`), `PCB_REFERENCE_IMAGE::Move` / `Rotate` / `Flip`, `PCB_POINT::Move` / `Rotate` /
//! `Flip`, `PCB_BARCODE::Move` / `Rotate` / `Flip`, `PCB_DIMENSION_BASE::Move` / `Rotate` / `Flip` (with `PCB_DIM_ORTHOGONAL::Rotate` and the kinds' `Mirror`) and
//! `PCB_TABLE::Move` / `Rotate` / `Flip` at KiCad 8303b2ad. What they do to an image's pixels (`REFERENCE_IMAGE::Rotate` turns them a quarter
//! at a time, `Flip` mirrors them: `BITMAP_BASE::Rotate`, `Mirror`) takes a decoder, which the workspace has not: [`image_quarter_turns`] says how many
//! quarter turns a turn makes and the browser turns the pixels (see the doc of [`ImageXform`]).

use crate::barcode::Barcode;
use crate::fp_items::FootprintItems;
use crate::ir::{Dimension, DimensionKind, FootprintInstance, Millideg, Nm, Point, Side};
use crate::pcb_items::{PcbPoint, RefImage, TextBox};
use crate::table::Table;
use serde::{Deserialize, Serialize};

/// A transform of a board item: a move, a turn about `pivot` (clockwise-positive millidegrees on the screen, the board verbs' sense: KiCad's angle negated), or a flip about the
/// axis through `pivot` (`FLIP_DIRECTION::LEFT_RIGHT`, x mirrored, or `TOP_BOTTOM`, y mirrored) that puts the item on the other side of the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemXform {
    Move { dx: Nm, dy: Nm },
    Rotate { pivot: Point, angle: i64 },
    Flip { pivot: Point, top_bottom: bool },
}

impl ItemXform {
    /// Where the transform takes `p` (`RotatePoint` rounds each coordinate, as `rotate_point_about` and `to_board` do).
    pub fn point(self, p: Point) -> Point {
        match self {
            ItemXform::Move { dx, dy } => Point { x: p.x + dx, y: p.y + dy },
            ItemXform::Rotate { pivot, angle } => {
                let rad = (angle as f64) / 1000.0 * std::f64::consts::PI / 180.0;
                let (sin, cos) = rad.sin_cos();
                let (dx, dy) = ((p.x - pivot.x).as_f64(), (p.y - pivot.y).as_f64());
                Point { x: pivot.x + Nm::kiround(dx * cos - dy * sin), y: pivot.y + Nm::kiround(dx * sin + dy * cos) }
            }
            ItemXform::Flip { pivot, top_bottom: false } => Point { x: 2 * pivot.x - p.x, y: p.y },
            ItemXform::Flip { pivot, top_bottom: true } => Point { x: p.x, y: 2 * pivot.y - p.y },
        }
    }
}

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// Whether a text angle in millidegrees is a multiple of 90 degrees (`EDA_ANGLE::IsCardinal`).
fn is_cardinal(angle: Millideg) -> bool {
    angle % 90_000 == 0
}

/// `EDA_SHAPE::GetCornersInSequence( angle )` of a text box: the four corners in the order the text's frame has them (top left, top right, bottom right, bottom left of the
/// text as it reads), for a rectangle (its bounding box turned to the angle's quadrant) and for a polygon (the corners that sit furthest left, up, right and down).
pub fn corners_in_sequence(t: &TextBox, angle: Millideg) -> [Point; 4] {
    let angle = angle % 360_000;
    if t.pts.is_empty() || is_cardinal(angle) {
        let [x0, y0, x1, y1] = t.bounds();
        let (lt, rt, rb, lb) = (Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 });
        return match angle {
            90_000 => [lb, lt, rt, rb],
            180_000 => [rb, lb, lt, rt],
            270_000 => [rt, rb, lb, lt],
            _ => [lt, rt, rb, lb],
        };
    }
    let mut corners = t.pts.clone();
    while corners.len() < 4 {
        let last = *corners.last().unwrap_or(&Point { x: Nm::ZERO, y: Nm::ZERO });
        corners.push(Point { x: last.x + Nm::new(10), y: last.y + Nm::new(10) });
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (corners[0], corners[0], corners[0], corners[0]);
    for c in &corners {
        if c.x < min_x.x {
            min_x = *c;
        }
        if c.x > max_x.x {
            max_x = *c;
        }
        if c.y < min_y.y {
            min_y = *c;
        }
        if c.y > max_y.y {
            max_y = *c;
        }
    }
    if angle < 90_000 {
        [min_x, min_y, max_x, max_y]
    } else if angle < 180_000 {
        [max_y, min_x, min_y, max_x]
    } else if angle < 270_000 {
        [max_x, max_y, min_x, min_y]
    } else {
        [min_y, max_x, max_y, min_x]
    }
}

/// `PCB_TEXTBOX::Move` / `::Rotate` / `::Flip` (with `EDA_SHAPE::rotate` / `::flip`). `ItemXform::Rotate`'s angle is clockwise on the screen, so the text's own angle (KiCad's,
/// counter-clockwise) loses it. A rectangle turned by anything but a quarter turn becomes the polygon of its four corners; a polygon whose text angle is again a multiple
/// of 90 degrees becomes a rectangle again, started at the first corner of the text's frame. A flip puts the box on the other side's layer (`flip_layer`: the board's
/// `FlipLayer`), negates (left-right) or reflects (top-bottom) the text angle, and mirrors the glyphs of a box on a layer that belongs to a side (`IsSideSpecific`).
pub fn transform_text_box(t: &mut TextBox, x: ItemXform, flip_layer: &dyn Fn(&str) -> String) {
    match x {
        ItemXform::Move { .. } => {
            t.start = x.point(t.start);
            t.end = x.point(t.end);
            for p in t.pts.iter_mut() {
                *p = x.point(*p);
            }
        }
        ItemXform::Rotate { angle, .. } => {
            if t.pts.is_empty() {
                if angle.rem_euclid(90_000) == 0 {
                    t.start = x.point(t.start);
                    t.end = x.point(t.end);
                } else {
                    // "Convert non-cardinally-rotated rect to a diamond"
                    let (s, e) = (t.start, t.end);
                    let corners = [s, Point { x: e.x, y: s.y }, e, Point { x: s.x, y: e.y }];
                    t.pts = corners.iter().map(|c| x.point(*c)).collect();
                    t.start = t.pts[0];
                    t.end = t.pts[2];
                }
            } else {
                for p in t.pts.iter_mut() {
                    *p = x.point(*p);
                }
                t.start = t.pts[0];
                t.end = t.pts[2];
            }
            t.angle = norm360(t.angle as i64 - angle) as Millideg;
            if is_cardinal(t.angle) && !t.pts.is_empty() {
                let corners = corners_in_sequence(t, t.angle);
                let diag = Point { x: corners[2].x - corners[0].x, y: corners[2].y - corners[0].y };
                let (dx, dy) = (diag.x.abs(), diag.y.abs());
                let c0 = corners[0];
                t.start = c0;
                t.end = match t.angle {
                    90_000 => Point { x: c0.x + dx, y: c0.y - dy },
                    180_000 => Point { x: c0.x - dx, y: c0.y - dy },
                    270_000 => Point { x: c0.x - dx, y: c0.y + dy },
                    _ => Point { x: c0.x + dx, y: c0.y + dy },
                };
                t.pts.clear();
            }
        }
        ItemXform::Flip { top_bottom, .. } => {
            t.start = x.point(t.start);
            t.end = x.point(t.end);
            for p in t.pts.iter_mut() {
                *p = x.point(*p);
            }
            t.angle = if top_bottom { norm360(180_000 - t.angle as i64) } else { norm360(-(t.angle as i64)) } as Millideg;
            t.layer = flip_layer(&t.layer);
            if crate::fp_art::is_side_specific(&t.layer) {
                t.mirror = !t.mirror;
            }
        }
    }
}

/// `PCB_REFERENCE_IMAGE::Move` / `::Rotate` / `::Flip` as far as the position and the layer go: the centre goes where the transform puts it, and a flip puts the image on
/// the other side's layer (`SetLayer( GetBoard()->FlipLayer( GetLayer() ) )`). What `REFERENCE_IMAGE::Rotate` and `Flip` do to the pixels is [`ImageXform`]'s.
pub fn transform_ref_image(i: &mut RefImage, x: ItemXform, flip_layer: &dyn Fn(&str) -> String) {
    i.at = x.point(i.at);
    if let ItemXform::Flip { .. } = x {
        i.layer = flip_layer(&i.layer);
    }
}

/// `PCB_POINT::Move` / `::Rotate` / `::Flip`: the position, and a flip's layer (`SetLayer( GetBoard()->FlipLayer( GetLayer() ) )`; `FOOTPRINT::Flip` says "Points move but
/// don't flip layer" over a call to the point's own `Flip`, which does).
pub fn transform_point(p: &mut PcbPoint, x: ItemXform, flip_layer: &dyn Fn(&str) -> String) {
    p.at = x.point(p.at);
    if let ItemXform::Flip { .. } = x {
        p.layer = flip_layer(&p.layer);
    }
}

/// `PCB_BARCODE::Move` / `::Rotate` / `::Flip`: the centre goes where the transform puts it, a turn adds to the barcode's own angle (`ItemXform::Rotate`'s is clockwise, the
/// file's counter-clockwise, so it is taken off), a flip puts the barcode on the other side's layer and a top-bottom one turns it half a turn
/// (`if( aFlipDirection == FLIP_DIRECTION::TOP_BOTTOM ) m_angle += ANGLE_180`); the symbol is mirrored on a back layer when it is drawn.
pub fn transform_barcode(b: &mut Barcode, x: ItemXform, flip_layer: &dyn Fn(&str) -> String) {
    b.at = x.point(b.at);
    match x {
        ItemXform::Move { .. } => {}
        ItemXform::Rotate { angle, .. } => b.angle = norm360(b.angle as i64 - angle) as Millideg,
        ItemXform::Flip { top_bottom, .. } => {
            if top_bottom {
                b.angle = norm360(b.angle as i64 + 180_000) as Millideg;
            }
            b.layer = flip_layer(&b.layer);
        }
    }
}

/// `PCB_DIMENSION_BASE::Move` / `::Rotate` / `::Flip` and the kinds' own: `PCB_DIM_ORTHOGONAL::Rotate` turns the crossbar to the nearest quarter turn first, and
/// `PCB_DIMENSION_BASE::Mirror` (with `PCB_DIM_ALIGNED::Mirror` and `PCB_DIM_ORTHOGONAL::Mirror`) mirrors the feature points and the kept text position, negates the text
/// angle and puts the crossbar on the mirrored side. The lines and the text are worked out again from the points (`Update()`), so only those, the kind's numbers, the text
/// angle and a radial's or a leader's kept text position are carried. The same as `eda_ops`' `transform_dimension` for the board's own (which hands its turns and moves to
/// `eda_connectivity::dimension`: the same rounding as [`ItemXform::point`]), here for a footprint's (`FOOTPRINT::SetOrientation`, `Flip` pass the transform to each).
pub fn transform_dimension(d: &mut Dimension, x: ItemXform, flip_layer: &dyn Fn(&str) -> String) {
    match x {
        ItemXform::Move { .. } => {
            d.start = x.point(d.start);
            d.end = x.point(d.end);
            if let Some(t) = &mut d.text_pos {
                *t = x.point(*t);
            }
        }
        ItemXform::Rotate { angle, .. } => {
            ortho_for_rotation(d, angle);
            d.start = x.point(d.start);
            d.end = x.point(d.end);
            if let Some(t) = &mut d.text_pos {
                *t = x.point(*t);
            }
            d.text_angle = norm360(d.text_angle as i64 - angle) as Millideg;
        }
        ItemXform::Flip { top_bottom, .. } => {
            if let Some(t) = &mut d.text_pos {
                *t = x.point(*t);
            }
            d.start = x.point(d.start);
            d.end = x.point(d.end);
            d.text_angle = norm360(-(d.text_angle as i64)) as Millideg;
            match &mut d.kind {
                DimensionKind::Aligned { height } => *height = -*height,
                // "Only reverse the height if the height is aligned with the flip"
                DimensionKind::Orthogonal { height, horizontal } => {
                    if *horizontal == top_bottom {
                        *height = -*height;
                    }
                }
                DimensionKind::Radial { .. } | DimensionKind::Leader | DimensionKind::Center => {}
            }
            d.flip_text_mirror();
            d.layer = flip_layer(&d.layer);
        }
    }
}

/// `PCB_DIM_ORTHOGONAL::Rotate`'s adjustment before the points turn: the crossbar is axis-locked, so a turn of about 90 degrees makes a horizontal one vertical and the other
/// way round, the height changing sign where the side it is on swaps; about 180 degrees only the height changes sign. `angle_cw` is clockwise (KiCad's is counter-clockwise).
fn ortho_for_rotation(d: &mut Dimension, angle_cw: i64) {
    let DimensionKind::Orthogonal { height, horizontal } = &mut d.kind else { return };
    // `angle.Normalize180()`
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
    } else if (-135_000..-45_000).contains(&a) {
        if *horizontal {
            *horizontal = false;
            *height = -*height;
        } else {
            *horizontal = true;
        }
    } else if !(-135_000..=135_000).contains(&a) {
        *height = -*height;
    }
}

/// `PCB_TABLE::Move` / `::Rotate` / `::Flip` ([`Table::translate`], [`Table::rotate`], [`Table::flip`]): a flip puts the table on the other side's layer, its cells' text
/// mirrored when that layer belongs to a side (`IsSideSpecific`). `ItemXform::Rotate`'s angle is clockwise, the table's counter-clockwise.
pub fn transform_table(t: &mut Table, x: ItemXform, flip_layer: &dyn Fn(&str) -> String) {
    match x {
        ItemXform::Move { dx, dy } => t.translate(dx, dy),
        ItemXform::Rotate { pivot, angle } => t.rotate(pivot, (-angle).rem_euclid(360_000) as Millideg),
        ItemXform::Flip { pivot, top_bottom } => {
            let layer = flip_layer(&t.layer);
            let side = crate::fp_art::is_side_specific(&layer);
            t.flip(pivot, !top_bottom, layer, side);
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the pixels of an image

/// `REFERENCE_IMAGE::Rotate`'s quarter turns: "each call to m_bitmapBase->Rotate() rotates 90 degrees", once for every `ang` of 45, 135, 225 and 315 below the angle
/// normalised to [0, 360) -- so a turn rounds to the nearest quarter, an eighth going down -- each counter-clockwise (`m_bitmapBase->Rotate( true )`). `angle_ccw` is KiCad's
/// angle in millidegrees (counter-clockwise on the screen; a board verb's clockwise angle negated).
pub fn image_quarter_turns(angle_ccw: i64) -> u8 {
    let norm = angle_ccw.rem_euclid(360_000);
    let mut n = 0u8;
    let mut ang = 45_000;
    while ang < norm {
        n += 1;
        ang += 90_000;
    }
    n % 4
}

/// What a transform does to an image's pixels (`BITMAP_BASE::Rotate`, `BITMAP_BASE::Mirror`): quarter turns counter-clockwise, or a mirror (`top_bottom`: upside down,
/// else left to right). The workspace has no image codec, so the server does not do it: the browser decodes the file, turns it on a canvas and sends the file it encodes
/// back with the verb (`set_ref_image_data` in the batch of the `rotate_items` or `flip_items`), as Convert to Greyscale does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageXform {
    TurnCcw(u8),
    Mirror { top_bottom: bool },
}

/// The pixel steps of `x` on an image that is a board item (`PCB_REFERENCE_IMAGE::Rotate` -> `REFERENCE_IMAGE::Rotate`, `PCB_REFERENCE_IMAGE::Flip` -> `REFERENCE_IMAGE::Flip`).
pub fn image_steps(x: ItemXform) -> Vec<ImageXform> {
    match x {
        ItemXform::Move { .. } => vec![],
        ItemXform::Rotate { angle, .. } => match image_quarter_turns(-angle) {
            0 => vec![],
            n => vec![ImageXform::TurnCcw(n)],
        },
        ItemXform::Flip { top_bottom, .. } => vec![ImageXform::Mirror { top_bottom }],
    }
}

/// The pixel steps of an image of a footprint when the footprint is turned or flipped: a turn is passed on as it is (`SetOrientation`: `item->Rotate( .., angleChange )`); a
/// flip is the item's top-bottom flip and, for a left-right flip, the half turn of the footprint after it (`FOOTPRINT::Flip`).
pub fn footprint_image_steps(x: ItemXform) -> Vec<ImageXform> {
    match x {
        ItemXform::Flip { top_bottom: true, .. } => vec![ImageXform::Mirror { top_bottom: true }],
        ItemXform::Flip { top_bottom: false, .. } => vec![ImageXform::Mirror { top_bottom: true }, ImageXform::TurnCcw(2)],
        other => image_steps(other),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------ the footprint's frame

/// The steps that take an item of a footprint's frame to the board for the pose `fp` (as `to_board` takes a pad): a left-right flip about the origin when the footprint is
/// on the back, the pose's turn about the origin, the move to its position.
pub fn placement_steps(fp: &FootprintInstance) -> Vec<ItemXform> {
    let origin = Point { x: Nm::ZERO, y: Nm::ZERO };
    let mut steps = Vec::with_capacity(3);
    if fp.side == Side::Bottom {
        steps.push(ItemXform::Flip { pivot: origin, top_bottom: false });
    }
    if fp.rot != 0 {
        steps.push(ItemXform::Rotate { pivot: origin, angle: fp.rot as i64 });
    }
    steps.push(ItemXform::Move { dx: fp.at.x, dy: fp.at.y });
    steps
}

/// The steps that take an item on the board back into the frame of the footprint at `fp`: the inverse of [`placement_steps`].
pub fn unplacement_steps(fp: &FootprintInstance) -> Vec<ItemXform> {
    let origin = Point { x: Nm::ZERO, y: Nm::ZERO };
    let mut steps = vec![ItemXform::Move { dx: -fp.at.x, dy: -fp.at.y }];
    if fp.rot != 0 {
        steps.push(ItemXform::Rotate { pivot: origin, angle: -(fp.rot as i64) });
    }
    if fp.side == Side::Bottom {
        steps.push(ItemXform::Flip { pivot: origin, top_bottom: false });
    }
    steps
}

/// The footprint's items transformed by `steps`, one after the other; layers flip as a footprint's do ([`crate::fp_art::flip_layer`]: an inner copper layer is its own).
pub fn transformed(items: &FootprintItems, steps: &[ItemXform]) -> FootprintItems {
    let flip = |l: &str| crate::fp_art::flip_layer(l);
    let mut out = items.clone();
    for &x in steps {
        out.text_boxes.iter_mut().for_each(|t| transform_text_box(t, x, &flip));
        out.images.iter_mut().for_each(|i| transform_ref_image(i, x, &flip));
        out.points.iter_mut().for_each(|p| transform_point(p, x, &flip));
        out.barcodes.iter_mut().for_each(|b| transform_barcode(b, x, &flip));
        out.dimensions.iter_mut().for_each(|d| transform_dimension(d, x, &flip));
        out.tables.iter_mut().for_each(|t| transform_table(t, x, &flip));
    }
    out
}

/// The footprint's items on the board for the pose `fp` (see the module doc). An image keeps its pixels: they are held as the board has them (see
/// [`crate::ir::FootprintExtra::items`]).
pub fn placed(items: &FootprintItems, fp: &FootprintInstance) -> FootprintItems {
    transformed(items, &placement_steps(fp))
}

/// The id the studio gives the item `id` of the footprint `reference` (an image's file, a barcode's shape and the pixels a turn sends are asked for by it): `U1~img_..`.
pub fn board_id(reference: &str, id: &str) -> String {
    format!("{reference}~{id}")
}

/// The reverse of [`board_id`]: the footprint's reference and the item's id, when `id` is one.
pub fn parse_board_id(id: &str) -> Option<(&str, &str)> {
    let (reference, item) = id.split_once('~')?;
    let kind = item.split('_').next()?;
    (!reference.is_empty() && matches!(kind, "tbx" | "img" | "pnt" | "bar" | "dim" | "tbl")).then_some((reference, item))
}

/// The name the board file gives the footprint `part` is on (`write_footprint`'s `lib_name_written`): the link the derived symbol names (`footprint_lib_id`), else the
/// definition's name, without the importer's `#2` slot.
pub fn written_name(model: &crate::ConstraintModel, part: &crate::Part) -> String {
    let name = Some(crate::footprint::footprint_lib_id(part)).filter(|n| !n.is_empty()).or_else(|| model.footprint_of(part).map(|f| f.name)).unwrap_or_default();
    crate::footprint::without_import_slot(&name, &model.footprints)
}

/// The items of the footprint `part` is on, as the board has them (`FootprintExtra::items`): none while the part is on another footprint than the one they were read from
/// (`FootprintExtra::source_name`), which the writer does not write them for either. The items the board put on a footprint made here are its own whatever it is on
/// ([`crate::ir::FootprintExtra::items_are_for`]).
pub fn items_of<'a>(design: &'a crate::ir::Design, model: &crate::ConstraintModel, part: &crate::Part) -> Option<&'a FootprintItems> {
    let e = design.drawings.as_ref()?.footprint_extras.iter().find(|e| e.id == part.reference && !e.items.is_empty())?;
    e.items_are_for(&written_name(model, part)).then_some(&e.items)
}

/// Whether the footprint's items are of the six kinds this module places (and nothing else of [`FootprintItems`]).
pub fn has_placed_kinds(items: &FootprintItems) -> bool {
    !placed_kinds(items).is_empty()
}

/// The six kinds of `items` this module places (the text boxes, images, points, barcodes, dimensions and tables): what a board footprint holds as its items
/// (`FootprintExtra::items`). A library footprint's zones are the board's zones of the footprint (`Zone::parent_footprint`, in the board's frame as KiCad keeps a
/// footprint's `Zones()`), its groups the footprint's groups of its record (`FootprintExtra::groups`): neither is among them.
pub fn placed_kinds(items: &FootprintItems) -> FootprintItems {
    FootprintItems {
        text_boxes: items.text_boxes.clone(),
        images: items.images.clone(),
        points: items.points.clone(),
        barcodes: items.barcodes.clone(),
        dimensions: items.dimensions.clone(),
        tables: items.tables.clone(),
        ..Default::default()
    }
}

/// Where a footprint was when its zones were put where they are on the board ([`crate::ir::FootprintExtra::zones_pose`]): the position, the orientation and the side
/// of its [`FootprintInstance`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZonesPose {
    pub at: Point,
    #[serde(default)]
    pub rot: Millideg,
    pub side: Side,
}

impl ZonesPose {
    /// The pose of `fp`.
    pub fn of(fp: &FootprintInstance) -> Self {
        ZonesPose { at: fp.at, rot: fp.rot, side: fp.side }
    }

    /// The footprint at this pose (its id and label are of no account to its zones).
    pub fn instance(&self) -> FootprintInstance {
        FootprintInstance { id: String::new(), at: self.at, rot: self.rot, side: self.side, label: Default::default() }
    }
}

/// The steps that take an item on the board from where the footprint at `from` had it to where the footprint at `to` has it: off the board for `from`, onto it for `to`.
pub fn repose_steps(from: &FootprintInstance, to: &FootprintInstance) -> Vec<ItemXform> {
    let mut steps = unplacement_steps(from);
    steps.extend(placement_steps(to));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::mm;

    fn p(x: f64, y: f64) -> Point {
        Point { x: mm(x), y: mm(y) }
    }

    fn pose(at: Point, rot: Millideg, side: Side) -> FootprintInstance {
        FootprintInstance { id: "U1".into(), at, rot, side, label: Default::default() }
    }

    fn items() -> FootprintItems {
        FootprintItems {
            text_boxes: vec![TextBox::new("F.SilkS", p(1.0, 1.0), p(3.0, 2.0), mm(1.0), mm(0.1))],
            images: vec![RefImage::new("F.SilkS", p(0.0, -2.0), "AAAA")],
            points: vec![PcbPoint::new("F.Fab", p(2.0, 0.0))],
            barcodes: vec![Barcode { id: String::new(), at: p(-2.0, 0.0), angle: 0, layer: "F.SilkS".into(), width_nm: mm(3.0), height_nm: mm(3.0), text: "x".into(), text_height_nm: mm(1.0), kind: Default::default(), ecc: Default::default(), show_text: true, knockout: false, margin_x_nm: Nm::ZERO, margin_y_nm: Nm::ZERO }],
            ..Default::default()
        }
    }

    #[test]
    fn the_quarter_turns_of_an_image_are_reference_images_loop() {
        // 45 < norm, 135 < norm, ...: a turn rounds to the nearest quarter, an eighth down
        assert_eq!([0, 45_000, 46_000, 90_000, 135_000, 136_000, 180_000, 270_000, 359_000].map(image_quarter_turns), [0, 0, 1, 1, 1, 2, 2, 3, 0]);
        assert_eq!(image_quarter_turns(-90_000), 3, "a clockwise quarter is three counter-clockwise ones");
        // a board verb's clockwise quarter
        assert_eq!(image_steps(ItemXform::Rotate { pivot: p(0.0, 0.0), angle: 90_000 }), vec![ImageXform::TurnCcw(3)]);
        assert_eq!(footprint_image_steps(ItemXform::Flip { pivot: p(0.0, 0.0), top_bottom: false }), vec![ImageXform::Mirror { top_bottom: true }, ImageXform::TurnCcw(2)]);
    }

    #[test]
    fn placing_is_the_items_own_flip_turn_and_move_and_unplacing_takes_them_back() {
        for (rot, side) in [(0, Side::Top), (90_000, Side::Top), (30_000, Side::Top), (0, Side::Bottom), (270_000, Side::Bottom)] {
            let fp = pose(p(50.0, 40.0), rot, side);
            let placed = placed(&items(), &fp);
            // every position is where `to_board` puts the frame's point
            assert_eq!(placed.points[0].at, crate::footprint::to_board(&fp, (mm(2.0), Nm::ZERO)));
            assert_eq!(placed.images[0].at, crate::footprint::to_board(&fp, (Nm::ZERO, mm(-2.0))));
            assert_eq!(placed.barcodes[0].at, crate::footprint::to_board(&fp, (mm(-2.0), Nm::ZERO)));
            let back = transformed(&placed, &unplacement_steps(&fp));
            assert_eq!(back.points, items().points, "{rot} {side:?}");
            assert_eq!(back.barcodes, items().barcodes, "{rot} {side:?}");
            if rot % 90_000 == 0 {
                assert_eq!(back.text_boxes, items().text_boxes, "{rot} {side:?}");
            }
        }
        let fp = pose(p(0.0, 0.0), 0, Side::Bottom);
        let on_back = placed(&items(), &fp);
        assert_eq!((on_back.text_boxes[0].layer.as_str(), on_back.text_boxes[0].mirror), ("B.SilkS", true), "a box on the back is on the back silkscreen, its text mirrored");
        assert_eq!((on_back.points[0].layer.as_str(), on_back.barcodes[0].layer.as_str(), on_back.images[0].layer.as_str()), ("B.Fab", "B.SilkS", "B.SilkS"));
        // a quarter turn clockwise on the screen takes 90 from the box's text angle and adds to nothing else's
        let turned = placed(&items(), &pose(p(0.0, 0.0), 90_000, Side::Top));
        assert_eq!((turned.text_boxes[0].angle, turned.barcodes[0].angle), (270_000, 270_000));
    }

    fn dimension(kind: DimensionKind) -> Dimension {
        use crate::ir::{ArrowDirection, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat};
        Dimension {
            id: "dim_1".into(),
            layer: "F.Fab".into(),
            kind,
            start: p(-1.0, 2.0),
            end: p(1.0, 2.0),
            prefix: String::new(),
            suffix: String::new(),
            override_text: None,
            units: DimensionUnits::Mm,
            units_format: DimensionUnitsFormat::NoSuffix,
            precision: 2,
            suppress_trailing_zeros: true,
            text_position: DimensionTextPosition::Outside,
            keep_text_aligned: true,
            text_angle: 0,
            text_size_nm: mm(1.0),
            stroke_width: mm(0.1),
            arrow_length: mm(1.27),
            extension_offset: mm(0.5),
            extension_height: mm(0.58),
            arrow_direction: ArrowDirection::Outward,
            text_pos: None,
            text_frame: Default::default(),
            text_effects: Default::default(),
            text_thickness_nm: None,
        }
    }

    /// UI-LEFT PP14: a footprint's dimension and table go where the footprint's pose puts them and come back exactly at a quarter turn: the board's own transforms
    /// (`PCB_DIMENSION_BASE::Rotate` with `PCB_DIM_ORTHOGONAL::Rotate`'s crossbar swap, `Mirror`; `PCB_TABLE::Rotate`, `Flip`).
    #[test]
    fn a_footprints_dimension_and_table_are_placed_and_unplaced_as_its_other_items() {
        let items = FootprintItems {
            dimensions: vec![dimension(DimensionKind::Aligned { height: mm(1.0) }), dimension(DimensionKind::Orthogonal { height: mm(1.0), horizontal: true })],
            tables: vec![Table::new_grid("F.Fab", p(-1.5, -2.0), 2, 1, mm(1.5), mm(1.0), mm(0.1))],
            ..Default::default()
        };
        for (rot, side) in [(0, Side::Top), (90_000, Side::Top), (180_000, Side::Top), (0, Side::Bottom), (270_000, Side::Bottom)] {
            let fp = pose(p(50.0, 40.0), rot, side);
            let on_board = placed(&items, &fp);
            assert_eq!(on_board.dimensions[0].start, crate::footprint::to_board(&fp, (mm(-1.0), mm(2.0))), "{rot} {side:?}");
            assert_eq!(transformed(&on_board, &unplacement_steps(&fp)), items, "{rot} {side:?}");
        }
        let turned = placed(&items, &pose(p(0.0, 0.0), 90_000, Side::Top));
        assert_eq!(turned.dimensions[1].kind, DimensionKind::Orthogonal { height: mm(-1.0), horizontal: false }, "a quarter turn makes the horizontal crossbar vertical");
        assert_eq!(turned.tables[0].angle, 270_000, "the table turns with the footprint (a clockwise quarter is KiCad's 270)");
        let flipped = placed(&items, &pose(p(0.0, 0.0), 0, Side::Bottom));
        assert_eq!((flipped.dimensions[0].layer.as_str(), flipped.tables[0].layer.as_str()), ("B.Fab", "B.Fab"));
        assert_eq!(flipped.dimensions[0].kind, DimensionKind::Aligned { height: mm(-1.0) }, "a mirror puts the crossbar on the other side");
        assert!(flipped.tables[0].cells.iter().all(|c| c.style.mirror), "the cells' text reads mirrored from the back");
        assert_eq!(repose_steps(&pose(p(1.0, 2.0), 90_000, Side::Top), &pose(p(1.0, 2.0), 90_000, Side::Top)).iter().fold(p(3.0, 4.0), |q, x| x.point(q)), p(3.0, 4.0));
    }

    #[test]
    fn a_board_id_names_the_footprint_and_the_item() {
        assert_eq!(board_id("U1", "img_0123456789ab"), "U1~img_0123456789ab");
        assert_eq!(parse_board_id("U1~img_0123456789ab"), Some(("U1", "img_0123456789ab")));
        assert_eq!(parse_board_id("U1~t0"), None, "a footprint's text is no item of these kinds");
        assert_eq!(parse_board_id("U1~dim_0123456789ab"), Some(("U1", "dim_0123456789ab")));
        assert_eq!(parse_board_id("U1~tbl_0123456789ab"), Some(("U1", "tbl_0123456789ab")));
        assert_eq!(parse_board_id("img_0123456789ab"), None);
    }
}
