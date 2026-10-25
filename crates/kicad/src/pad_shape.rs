//! The shape tokens of a `(pad ...)`, read and written the way `PCB_IO_KICAD_SEXPR` does (`parsePAD`, `parsePAD_option` and
//! `format( const PAD* )`, `pcbnew/pcb_io/kicad_sexpr`): `rect_delta` for a trapezoid, `roundrect_rratio`, `chamfer_ratio` and `chamfer`
//! for a rounded or chamfered rectangle, the shape offset inside `drill`, and `options` (the clearance mode and the anchor) with
//! `primitives` for a custom pad. The board writer, the library writer and the two readers all go through here.
//!
//! A bottom-side footprint's pads are in the file's mirrored frame (`PAD::Flip`): the offset's and the delta's x are negated, the
//! chamfered corners swap sides and every primitive's x is negated. The engine keeps the footprint as seen from the top, so reading
//! and writing both apply the same mirror.

use crate::import::{graphic_shape, mm_to_nm};
use crate::sexpr::{self, Sexpr};
use crate::mm;
use eda_model::footprint::{CustomZoneMode, PadAnchor, PadCustom, PadPrimitive, PadShape, PadShapeExt};
use eda_model::ir::{ChamferCorners, Point, Shape, Side, Nm};
use std::fmt::Write as _;

/// What the shape of a `(pad ..)` is: the shape itself, the ratio of its rounded corners and the rest of its padstack layer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PadShapeRead {
    pub shape: PadShape,
    pub roundrect_ratio: Option<f64>,
    pub ext: PadShapeExt,
}

fn mirror_x(p: Point, side: Side) -> Point {
    if side == Side::Bottom {
        Point { x: -p.x, y: p.y }
    } else {
        p
    }
}

/// The `(offset x y)` inside a pad's `(drill ..)`, in the engine's frame (`PAD::SetOffset`).
pub(crate) fn read_offset(pad: &[Sexpr], side: Side) -> Option<(Nm, Nm)> {
    let o = sexpr::find(pad, "drill").and_then(|d| sexpr::find(d, "offset"))?;
    let p = mirror_x(Point { x: mm_to_nm(sexpr::num(o, 1)?), y: mm_to_nm(sexpr::num(o, 2)?) }, side);
    (p.x != Nm::ZERO || p.y != Nm::ZERO).then_some((p.x, p.y))
}

/// The shape of `pad` (`parsePAD`): the shape token, `rect_delta`, `roundrect_rratio`, `chamfer_ratio`, `chamfer` and `options` and
/// `primitives`. `side` is the side of the pad's footprint (the file's frame is mirrored on the bottom).
pub(crate) fn read_pad_shape(pad: &[Sexpr], side: Side) -> PadShapeRead {
    read_shape_node(pad, sexpr::txt(pad, 3).unwrap_or(""), read_offset(pad, side), side)
}

/// The shape a node holds: `shape_token` (`rect`, `circle`, ..), the copper's `offset` and the tokens `rect_delta`, `roundrect_rratio`, `chamfer_ratio`,
/// `chamfer`, `options` and `primitives` among its children. A pad is such a node (`parsePAD`), and so is each `(layer ..)` of its padstack (`parsePadstack`).
pub(crate) fn read_shape_node(pad: &[Sexpr], shape_token: &str, offset: Option<(Nm, Nm)>, side: Side) -> PadShapeRead {
    let mut shape = match shape_token {
        "circle" => PadShape::Circle,
        "oval" => PadShape::Oval,
        "roundrect" => PadShape::RoundRect,
        "trapezoid" => PadShape::Trapezoid,
        "custom" => PadShape::Custom,
        _ => PadShape::Rect,
    };
    let mut ext = PadShapeExt { offset, ..Default::default() };

    // `(rect_delta dx dy)`, `PAD::SetDelta`; the delta's x is mirrored with the pad.
    if let Some(d) = sexpr::find(pad, "rect_delta") {
        let dx = mm_to_nm(sexpr::num(d, 1).unwrap_or(0.0));
        let dy = mm_to_nm(sexpr::num(d, 2).unwrap_or(0.0));
        let dx = if side == Side::Bottom { -dx } else { dx };
        if dx != Nm::ZERO || dy != Nm::ZERO {
            ext.trapezoid_delta = Some((dx, dy));
        }
    }

    // `(chamfer_ratio r)` and `(chamfer top_left ..)` turn the shape into a chamfered rectangle, whatever the shape token was.
    let ratio = sexpr::find(pad, "chamfer_ratio").and_then(|r| sexpr::num(r, 1));
    if let Some(r) = ratio {
        ext.chamfer_ratio = Some(r);
        if r > 0.0 {
            shape = PadShape::ChamferedRect;
        }
    }
    if let Some(c) = sexpr::find(pad, "chamfer") {
        let toks: Vec<&str> = c.iter().skip(1).filter_map(Sexpr::text).collect();
        let corners = ChamferCorners { top_left: toks.contains(&"top_left"), top_right: toks.contains(&"top_right"), bottom_left: toks.contains(&"bottom_left"), bottom_right: toks.contains(&"bottom_right") };
        ext.chamfer_corners = if side == Side::Bottom { corners.mirrored_x() } else { corners };
        if !corners.is_none() {
            shape = PadShape::ChamferedRect;
        }
    }
    if shape != PadShape::ChamferedRect {
        ext.chamfer_ratio = None;
        ext.chamfer_corners = ChamferCorners::default();
    }
    if shape != PadShape::Trapezoid {
        ext.trapezoid_delta = None;
    }
    let roundrect_ratio = if matches!(shape, PadShape::RoundRect | PadShape::ChamferedRect) { sexpr::find(pad, "roundrect_rratio").and_then(|r| sexpr::num(r, 1)) } else { None };

    if shape == PadShape::Custom {
        let mut custom = PadCustom::default();
        if let Some(o) = sexpr::find(pad, "options") {
            // `(anchor circle|rect)`, `(clearance outline|convexhull)`
            if let Some(a) = sexpr::find(o, "anchor") {
                custom.anchor = if sexpr::txt(a, 1) == Some("rect") { PadAnchor::Rect } else { PadAnchor::Circle };
            }
            if let Some(c) = sexpr::find(o, "clearance") {
                custom.zone = if sexpr::txt(c, 1) == Some("convexhull") { CustomZoneMode::ConvexHull } else { CustomZoneMode::Outline };
            }
        }
        if let Some(prims) = sexpr::find(pad, "primitives") {
            custom.primitives = read_primitives(prims, side);
        }
        ext.custom = Some(custom);
    }
    PadShapeRead { shape, roundrect_ratio, ext }
}

/// `(primitives (gr_line ..) (gr_poly ..) ...)`: the basic shapes of a custom pad, in the pad's own frame.
fn read_primitives(prims: &[Sexpr], side: Side) -> Vec<PadPrimitive> {
    let mut out = Vec::new();
    for child in prims.iter().skip(1) {
        let Some(item) = child.as_list() else { continue };
        // `gr_bbox` is the box a pad's number is placed in and `gr_vector` a thermal spoke template: proxy items, not copper.
        let (tag, proxy) = match sexpr::tag(item) {
            Some(t @ ("gr_line" | "gr_arc" | "gr_circle" | "gr_rect" | "gr_poly" | "gr_curve")) => (t, false),
            Some("gr_bbox") => ("gr_rect", true),
            Some("gr_vector") => ("gr_line", true),
            _ => continue,
        };
        let Some(mut shape) = graphic_shape(item, tag, |p| mirror_x(p, side)) else { continue };
        shape.set_layer(String::new());
        if !proxy {
            // `parsePCB_SHAPE`: a file without a `fill` token filled the shapes that had a zero width (a rectangle, a circle) and every
            // polygon; and only a filled shape may have no width -- any other gets the default line, 0.1 mm.
            if sexpr::find(item, "fill").is_none() && (matches!(shape, Shape::Polygon { .. }) || (matches!(shape, Shape::Rect { .. } | Shape::Circle { .. }) && shape.stroke_width() == Nm::ZERO)) {
                shape.set_filled(true);
            }
            if shape.stroke_width() <= Nm::ZERO && !shape.is_filled() {
                shape.set_stroke_width(Nm::from_raw_shim(100_000));
            }
        }
        out.push(PadPrimitive { shape, proxy });
    }
    out
}

/// The file's token for a pad shape (`shapeName`): a chamfered rectangle is a `roundrect` with `chamfer_ratio` and `chamfer`.
pub(crate) fn shape_token(shape: PadShape) -> &'static str {
    match shape {
        PadShape::Rect => "rect",
        PadShape::RoundRect | PadShape::ChamferedRect => "roundrect",
        PadShape::Circle => "circle",
        PadShape::Oval => "oval",
        PadShape::Trapezoid => "trapezoid",
        PadShape::Custom => "custom",
    }
}

/// The pieces of a pad's shape text, each with the leading space it is written with, in the places `format( const PAD* )` puts them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PadShapeText {
    /// ` (rect_delta dx dy)`, right after `(size ..)`.
    pub delta: String,
    /// The copper's offset in the file's frame, written inside `(drill ..)`.
    pub offset: Option<(Nm, Nm)>,
    /// ` (roundrect_rratio r)` and, for a chamfered rectangle, ` (chamfer_ratio r) (chamfer ..)`.
    pub corners: String,
    /// A custom pad's ` (options ..) (primitives ..)`, after the thermal relief and before the uuid.
    pub custom: String,
}

/// `PCB_IO_KICAD_SEXPR::format( const PAD* )` for the shape of a pad of a footprint on `side`.
pub(crate) fn write_pad_shape(shape: PadShape, roundrect_ratio: Option<f64>, ext: &PadShapeExt, side: Side) -> PadShapeText {
    let mut t = PadShapeText::default();
    if let Some((dx, dy)) = ext.trapezoid_delta.filter(|d| *d != (Nm::ZERO, Nm::ZERO)) {
        let dx = if side == Side::Bottom { -dx } else { dx };
        write!(t.delta, " (rect_delta {} {})", mm(dx), mm(dy)).unwrap();
    }
    t.offset = ext.offset.filter(|o| *o != (Nm::ZERO, Nm::ZERO)).map(|(x, y)| (if side == Side::Bottom { -x } else { x }, y));
    if matches!(shape, PadShape::RoundRect | PadShape::ChamferedRect) {
        // a ratio is `FormatDouble2Str`'s, not a length: 0.038462 stays 0.038462
        write!(t.corners, " (roundrect_rratio {})", crate::fmt_double_2_str(roundrect_ratio.unwrap_or(0.25))).unwrap();
    }
    if shape == PadShape::ChamferedRect {
        // `PADSTACK` defaults: a chamfer of 0.2 of the shorter side.
        write!(t.corners, " (chamfer_ratio {})", crate::fmt_double_2_str(ext.chamfer_ratio.unwrap_or(0.2))).unwrap();
        let c = if side == Side::Bottom { ext.chamfer_corners.mirrored_x() } else { ext.chamfer_corners };
        t.corners.push_str(" (chamfer");
        for (on, name) in [(c.top_left, "top_left"), (c.top_right, "top_right"), (c.bottom_left, "bottom_left"), (c.bottom_right, "bottom_right")] {
            if on {
                write!(t.corners, " {name}").unwrap();
            }
        }
        t.corners.push(')');
    }
    if shape == PadShape::Custom {
        t.custom = write_custom_text(ext, side, true);
    }
    t
}

/// A custom pad's ` (options (clearance outline|convexhull) (anchor circle|rect)) (primitives ..)`; a padstack layer has no zone option of its own (it is the
/// pad's) and writes ` (options (anchor ..)) (primitives ..)` (`with_clearance` false).
pub(crate) fn write_custom_text(ext: &PadShapeExt, side: Side, with_clearance: bool) -> String {
    let mut text = String::new();
    let default = PadCustom::default();
    let custom = ext.custom.as_ref().unwrap_or(&default);
    let zone = if custom.zone == CustomZoneMode::ConvexHull { "convexhull" } else { "outline" };
    let anchor = if custom.anchor == PadAnchor::Rect { "rect" } else { "circle" };
    if with_clearance {
        write!(text, " (options (clearance {zone}) (anchor {anchor})) (primitives").unwrap();
    } else {
        write!(text, " (options (anchor {anchor})) (primitives").unwrap();
    }
    for prim in &custom.primitives {
        write_primitive(&mut text, prim, side);
    }
    text.push(')');
    text
}

/// One primitive as `formatPrimitives` writes it: `(gr_line (start ..) (end ..) (width w))`, the filled shapes with `(fill yes|no)`.
fn write_primitive(out: &mut String, prim: &PadPrimitive, side: Side) {
    let p = |q: Point| {
        let q = mirror_x(q, side);
        format!("{} {}", mm(q.x), mm(q.y))
    };
    let fill = |f: bool| format!(" (fill {})", if f { "yes" } else { "no" });
    let width = |w: Nm| format!(" (width {})", mm(w.max(Nm::ZERO)));
    match (&prim.shape, prim.proxy) {
        (Shape::Segment { start, end, .. }, true) => write!(out, " (gr_vector (start {}) (end {}))", p(*start), p(*end)),
        (Shape::Segment { start, end, stroke_width, .. }, false) => write!(out, " (gr_line (start {}) (end {}){})", p(*start), p(*end), width(*stroke_width)),
        (Shape::Rect { start, end, .. }, true) => write!(out, " (gr_bbox (start {}) (end {}))", p(*start), p(*end)),
        (Shape::Rect { start, end, stroke_width, filled, .. }, false) => write!(out, " (gr_rect (start {}) (end {}){}{})", p(*start), p(*end), width(*stroke_width), fill(*filled)),
        (Shape::Arc { start, mid, end, stroke_width, .. }, _) => write!(out, " (gr_arc (start {}) (mid {}) (end {}){})", p(*start), p(*mid), p(*end), width(*stroke_width)),
        (Shape::Circle { center, end, stroke_width, filled, .. }, _) => write!(out, " (gr_circle (center {}) (end {}){}{})", p(*center), p(*end), width(*stroke_width), fill(*filled)),
        (Shape::Bezier { start, c1, c2, end, stroke_width, .. }, _) => {
            write!(out, " (gr_curve (pts (xy {}) (xy {}) (xy {}) (xy {})){})", p(*start), p(*c1), p(*c2), p(*end), width(*stroke_width))
        }
        (Shape::Polygon { pts, stroke_width, filled, .. }, _) => {
            let xy: Vec<String> = pts.iter().map(|q| format!("(xy {})", p(*q))).collect();
            write!(out, " (gr_poly (pts {}){}{})", xy.join(" "), width(*stroke_width), fill(*filled))
        }
    }
    .unwrap();
}

/// Read the `(xy x y)` pairs of a `(pts ..)` node -- exposed for the tests of this module.
#[cfg(test)]
pub(crate) fn first_xy(node: &[Sexpr]) -> Option<Point> {
    sexpr::find(node, "xy").and_then(crate::import::xy_point)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad_node(text: &str) -> Vec<Sexpr> {
        sexpr::parse(text).unwrap().as_list().unwrap().to_vec()
    }

    #[test]
    fn a_trapezoid_reads_its_delta_and_writes_it_back() {
        let node = pad_node("(pad \"1\" smd trapezoid (at 0 0) (size 2 1) (rect_delta 0.4 0) (layers \"F.Cu\"))");
        let r = read_pad_shape(&node, Side::Top);
        assert_eq!((r.shape, r.ext.trapezoid_delta), (PadShape::Trapezoid, Some((Nm::from_raw_shim(400_000), Nm::ZERO))));
        assert_eq!(write_pad_shape(r.shape, r.roundrect_ratio, &r.ext, Side::Top).delta, " (rect_delta 0.4 0)");
        // Seen from the bottom the file's frame is mirrored: the x delta changes sign each way.
        let b = read_pad_shape(&node, Side::Bottom);
        assert_eq!(b.ext.trapezoid_delta, Some((Nm::from_raw_shim(-400_000), Nm::ZERO)));
        assert_eq!(write_pad_shape(b.shape, b.roundrect_ratio, &b.ext, Side::Bottom).delta, " (rect_delta 0.4 0)");
    }

    #[test]
    fn chamfer_tokens_make_a_chamfered_rectangle_and_round_trip_on_either_side() {
        let node = pad_node("(pad \"1\" smd roundrect (at 0 0) (size 1 0.6) (roundrect_rratio 0.25) (chamfer_ratio 0.2) (chamfer top_left bottom_right) (layers \"F.Cu\"))");
        for side in [Side::Top, Side::Bottom] {
            let r = read_pad_shape(&node, side);
            assert_eq!((r.shape, r.roundrect_ratio, r.ext.chamfer_ratio), (PadShape::ChamferedRect, Some(0.25), Some(0.2)));
            let corners = if side == Side::Top { ChamferCorners { top_left: true, bottom_right: true, ..Default::default() } } else { ChamferCorners { top_right: true, bottom_left: true, ..Default::default() } };
            assert_eq!(r.ext.chamfer_corners, corners, "{side:?}");
            let t = write_pad_shape(r.shape, r.roundrect_ratio, &r.ext, side);
            assert_eq!(t.corners, " (roundrect_rratio 0.25) (chamfer_ratio 0.2) (chamfer top_left bottom_right)", "{side:?}");
        }
        // A plain roundrect stays one.
        let plain = read_pad_shape(&pad_node("(pad \"1\" smd roundrect (at 0 0) (size 1 0.6) (roundrect_rratio 0.3))"), Side::Top);
        assert_eq!((plain.shape, plain.roundrect_ratio, plain.ext.chamfer_ratio), (PadShape::RoundRect, Some(0.3), None));
    }

    #[test]
    fn a_custom_pad_reads_and_writes_its_options_and_primitives() {
        let text = "(pad \"1\" smd custom (at 0 0) (size 1.45 1.45) (layers \"B.Cu\") (options (clearance convexhull) (anchor rect)) \
            (primitives (gr_circle (center -2.975 0) (end -0.375 0) (width 2.2) (fill no)) (gr_poly (pts (xy 0 0) (xy 1 0) (xy 1 1)) (width 0) (fill yes)) \
            (gr_arc (start 1 0) (mid 0 1) (end -1 0) (width 0.2)) (gr_line (start 0 0) (end 1 2) (width 0.1)) (gr_bbox (start -1 -1) (end 1 1)) (gr_vector (start 0 0) (end 3 0))))";
        let node = pad_node(text);
        let r = read_pad_shape(&node, Side::Top);
        assert_eq!(r.shape, PadShape::Custom);
        let custom = r.ext.custom.clone().unwrap();
        assert_eq!((custom.anchor, custom.zone, custom.primitives.len()), (PadAnchor::Rect, CustomZoneMode::ConvexHull, 6));
        assert_eq!(custom.primitives.iter().filter(|p| p.proxy).count(), 2, "gr_bbox and gr_vector are proxy items");
        assert!(matches!(custom.primitives[0].shape, Shape::Circle { stroke_width, filled: false, center, .. } if stroke_width == Nm::from_raw_shim(2_200_000) && center == Point { x: Nm::from_raw_shim(-2_975_000), y: Nm::ZERO }));
        // The text it writes reads back to the same shape, on either side.
        for side in [Side::Top, Side::Bottom] {
            let r = read_pad_shape(&node, side);
            let t = write_pad_shape(r.shape, r.roundrect_ratio, &r.ext, side);
            let again = read_pad_shape(&pad_node(&format!("(pad \"1\" smd custom (at 0 0) (size 1.45 1.45){})", t.custom)), side);
            assert_eq!(again.ext.custom, r.ext.custom, "{side:?}");
        }
        let top = write_pad_shape(r.shape, r.roundrect_ratio, &r.ext, Side::Top).custom;
        assert!(top.starts_with(" (options (clearance convexhull) (anchor rect)) (primitives (gr_circle (center -2.975 0) (end -0.375 0) (width 2.2) (fill no))"), "{top}");
        assert!(top.contains("(gr_vector (start 0 0) (end 3 0))") && top.contains("(gr_bbox (start -1 -1) (end 1 1))"), "{top}");
    }

    #[test]
    fn the_shape_offset_is_read_from_inside_the_drill_and_mirrored_on_the_bottom() {
        let node = pad_node("(pad \"1\" thru_hole rect (at 0 0) (size 2 1) (drill 0.8 (offset 0.5 -0.25)) (layers \"*.Cu\"))");
        assert_eq!(read_offset(&node, Side::Top), Some((Nm::from_raw_shim(500_000), Nm::from_raw_shim(-250_000))));
        assert_eq!(read_offset(&node, Side::Bottom), Some((Nm::from_raw_shim(-500_000), Nm::from_raw_shim(-250_000))));
        assert_eq!(read_offset(&pad_node("(pad \"1\" smd rect (at 0 0) (size 2 1))"), Side::Top), None);
        assert_eq!(first_xy(&pad_node("(pts (xy 1 2))")), Some(Point { x: Nm::from_raw_shim(1_000_000), y: Nm::from_raw_shim(2_000_000) }));
    }
}
