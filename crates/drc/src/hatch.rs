//! The hatching of a board shape: `EDA_SHAPE::UpdateHatching` (`common/eda_shape.cpp`) with `PCB_SHAPE::getHatchingKnockouts` (`pcbnew/pcb_shape.cpp`).
//!
//! A rectangle, a circle or a polygon whose fill is a hatch (`(fill hatch)`, `(fill reverse_hatch)`, `(fill cross_hatch)`, [`HatchKind`]) is drawn by KiCad as its outline
//! and the lines this makes: spaced ten line widths apart (the spacing is cut down to a hundredth of the shape's longer side when it would give more than a hundred lines),
//! slanted at -1 (hatch), +1 (reverse hatch) or both (cross-hatch), each as wide as the shape's line. The lines stop where other things on the layer are: the shapes, the
//! texts and the text boxes of the board, each grown by half the spacing (`getHatchingKnockouts`). KiCad works them out again after every commit, every undo and every file
//! load (`PCB_ACTIONS::rehatchShapes`), so they are never older than the board; so are these, worked out when asked for.
//!
//! The plot has the lines as polygons, not as lines (`BRDITEMS_PLOTTER::PlotShape`): [`hatch_polygons`] makes them the way `UpdateHatching` fills `GetHatching()` -- a stadium
//! per line for a hatch or a reverse hatch, and for a cross-hatch the shape with a grid of squares cut out of it.
//!
//! The knockouts are KiCad's: the other shapes, texts and text boxes of the board on the layer, and the other footprints' Reference, Value and user fields, graphics and (on a courtyard
//! layer) courtyards. Left out: a footprint's own texts (`PCB_TEXT_T` is not among the things `getHatchingKnockouts` takes), the text variables in a text and the outline fonts.
//! Units are micrometres, KiCad's nanometres divided by a thousand; the errors of the curves are KiCad's (`ARC_HIGH_DEF` for the shape, `ARC_LOW_DEF` for the knockouts).

use crate::stroke_font::{effective_pen_width, kicad_text_segments, TextLayout};
use eda_clipper2::Point64;
use eda_model::footprint::to_board;
use eda_model::fp_art::draw_rotation;
use eda_model::fp_edit::{effective_reference, effective_value, FieldLayout};
use eda_model::ir::{Design, FootprintInstance, Point, Shape, Side, Text, TextJustify, TextVAlign, Um};
use eda_model::pcb_items::{HatchKind, TextBox};
use eda_model::ConstraintModel;
use eda_shape_poly_set::basic_shapes::{circle_polygon, oval_polygon, rotate_point, ErrorLoc};
use eda_shape_poly_set::{CornerStrategy, LineChain, ShapePolySet};

/// The board's `m_MaxError` (`ARC_HIGH_DEF`, 0.005 mm): the error of the polygon a circle is hatched as.
pub const MAX_ERROR_UM: i64 = 5;
/// `ARC_LOW_DEF` (0.02 mm): the error of the polygons the knockouts are made of.
pub const KNOCKOUT_ERROR_UM: i64 = 20;

/// The lines of a hatched shape and the width they are drawn with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hatching {
    pub width: Um,
    pub lines: Vec<(Point, Point)>,
}

fn p64(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

fn pt(p: Point64) -> Point {
    Point { x: p.x, y: p.y }
}

/// The outline a hatched shape is hatched inside: a rectangle's corners, a circle as the polygon of `TransformCircleToPolygon( .., ERROR_INSIDE )`, a polygon's points. `None`
/// for the shapes that have no inside (a line, an arc, a curve) and a polygon of fewer than three points.
fn outline_of(shape: &Shape) -> Option<LineChain> {
    match shape {
        Shape::Rect { start, end, .. } => {
            let (x0, x1) = (start.x.min(end.x), start.x.max(end.x));
            let (y0, y1) = (start.y.min(end.y), start.y.max(end.y));
            Some(vec![Point64::new(x0, y0), Point64::new(x1, y0), Point64::new(x1, y1), Point64::new(x0, y1)])
        }
        Shape::Circle { center, end, .. } => {
            let r = (((end.x - center.x) as f64).hypot((end.y - center.y) as f64)).round() as i64;
            (r > 0).then(|| circle_polygon(p64(*center), r, MAX_ERROR_UM, ErrorLoc::Inside, 0))
        }
        Shape::Polygon { pts, .. } if pts.len() >= 3 => Some(pts.iter().map(|p| p64(*p)).collect()),
        _ => None,
    }
}

/// `GetBoundingBox` of a shape: its points' box grown by half its line (a circle's by its radius).
fn shape_box(shape: &Shape) -> [i64; 4] {
    let half = shape.stroke_width().max(0) / 2;
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    let mut take = |p: Point| {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    };
    match shape {
        Shape::Circle { center, end, .. } => {
            let r = ((end.x - center.x) as f64).hypot((end.y - center.y) as f64).round() as i64;
            take(Point { x: center.x - r, y: center.y - r });
            take(Point { x: center.x + r, y: center.y + r });
        }
        Shape::Bezier { .. } => shape.bezier_points().unwrap_or_default().into_iter().for_each(&mut take),
        _ => shape.points().into_iter().for_each(&mut take),
    }
    [x0 - half, y0 - half, x1 + half, y1 + half]
}

fn boxes_meet(a: [i64; 4], b: [i64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// The ovals (`TransformOvalToPolygon`) along the closed or open polyline `pts`, `width` wide, into `out`.
fn ovals_along(out: &mut ShapePolySet, pts: &[Point], closed: bool, width: i64) {
    let n = pts.len();
    let count = if closed { n } else { n.saturating_sub(1) };
    for i in 0..count {
        out.add_outline(oval_polygon(p64(pts[i]), p64(pts[(i + 1) % n]), width, KNOCKOUT_ERROR_UM, ErrorLoc::Outside, 0));
    }
}

/// `PCB_SHAPE::TransformShapeToPolygon( knockouts, layer, margin, ARC_LOW_DEF, ERROR_OUTSIDE )` for another shape of the layer: its area, if it is filled or hatched (a hatched shape
/// is solid here: `includeFill` is false), and its line, both grown by `margin`.
fn shape_knockout(out: &mut ShapePolySet, shape: &Shape, solid: bool, margin: i64) {
    let width = shape.stroke_width().max(0) + 2 * margin;
    match shape {
        Shape::Segment { start, end, .. } => {
            out.add_outline(oval_polygon(p64(*start), p64(*end), width, KNOCKOUT_ERROR_UM, ErrorLoc::Outside, 0));
        }
        Shape::Arc { start, mid, end, .. } => {
            let pts = eda_model::outline::Arc::through(*start, *mid, *end).polyline(KNOCKOUT_ERROR_UM as f64);
            ovals_along(out, &pts, false, width);
        }
        Shape::Bezier { .. } => {
            let pts = shape.bezier_points().unwrap_or_default();
            ovals_along(out, &pts, false, width);
        }
        Shape::Rect { .. } => {
            let Some(corners) = outline_of(shape) else { return };
            let pts: Vec<Point> = corners.iter().map(|p| pt(*p)).collect();
            if solid {
                out.add_outline(corners);
            }
            if width > 0 || !solid {
                ovals_along(out, &pts, true, width);
            }
        }
        Shape::Circle { center, end, .. } => {
            let r = (((end.x - center.x) as f64).hypot((end.y - center.y) as f64)).round() as i64;
            if solid {
                out.add_outline(circle_polygon(p64(*center), r + width / 2, KNOCKOUT_ERROR_UM, ErrorLoc::Outside, 0));
            } else {
                // `TransformRingToPolygon`: a ring, or a disc when the line is wider than the radius.
                let inner = r - width / 2;
                if inner <= 0 {
                    out.add_outline(circle_polygon(p64(*center), r + width / 2, KNOCKOUT_ERROR_UM, ErrorLoc::Outside, 0));
                } else {
                    let idx = out.add_outline(circle_polygon(p64(*center), inner + width, KNOCKOUT_ERROR_UM, ErrorLoc::Outside, 0));
                    out.add_hole(circle_polygon(p64(*center), inner, KNOCKOUT_ERROR_UM, ErrorLoc::Inside, 0), Some(idx));
                }
            }
        }
        Shape::Polygon { pts, .. } if pts.len() >= 2 => {
            if solid && pts.len() >= 3 {
                let mut tmp = ShapePolySet::from_outline(pts.iter().map(|p| p64(*p)).collect());
                if width > 0 {
                    tmp.inflate(width / 2 + KNOCKOUT_ERROR_UM, CornerStrategy::RoundAllCorners, KNOCKOUT_ERROR_UM as i32, false);
                }
                for poly in tmp.polys {
                    out.add_polygon(poly);
                }
            } else {
                ovals_along(out, pts, true, width);
            }
        }
        Shape::Polygon { .. } => {}
    }
}

/// One text as `EDA_TEXT` lays it out: what [`text_hull`] needs of a board text and of a footprint's field.
struct TextSpec<'a> {
    text: &'a str,
    pos: Point,
    /// Glyph (width, height).
    size: (Um, Um),
    thickness: Um,
    /// `GetDrawRotation()`, millidegrees, KiCad's sense (counter-clockwise on the screen).
    draw_angle_mdeg: i64,
    mirror: bool,
    halign: i8,
    valign: i8,
    bold: bool,
}

/// A text's knockout, `PCB_TEXT::TransformShapeToPolygon` (`buildBoundingHull`): the text as polygons (`TransformTextToPolySet`: an oval of the pen's width on every stroke, the error
/// outside, the whole grown by the error again as `ERROR_OUTSIDE` asks), the box round them in the text's own frame (the strokes turned back about its anchor by its angle), grown by
/// the margin and turned again; and the box of the ink in the board's own axes, for the test that the text is near the shape.
fn text_hull(spec: &TextSpec, margin: i64) -> Option<(LineChain, [i64; 4])> {
    if spec.text.is_empty() || spec.size.0 <= 0 || spec.size.1 <= 0 {
        return None;
    }
    let angle_deg = spec.draw_angle_mdeg as f64 / 1000.0;
    let strokes = kicad_text_segments(&TextLayout { text: spec.text, pos: spec.pos, size: spec.size, thickness: spec.thickness, angle_deg, mirror: spec.mirror, halign: spec.halign, valign: spec.valign });
    if strokes.is_empty() {
        return None;
    }
    let pen = effective_pen_width(spec.thickness, spec.size, spec.bold);
    let mut ink = ShapePolySet::new();
    for seg in &strokes {
        ink.add_outline(oval_polygon(p64(seg.a), p64(seg.b), pen, KNOCKOUT_ERROR_UM, ErrorLoc::Outside, 0));
    }
    ink.simplify();
    // `aClearance += aMaxError; textShape.Inflate( aClearance, ROUND_ALL_CORNERS, aMaxError )` of `ERROR_OUTSIDE`.
    ink.inflate(KNOCKOUT_ERROR_UM, CornerStrategy::RoundAllCorners, KNOCKOUT_ERROR_UM as i32, false);
    let a = angle_deg.to_radians();
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    let (mut bx0, mut by0, mut bx1, mut by1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in ink.polys.iter().flat_map(|poly| poly.iter()).flat_map(|chain| chain.iter()) {
        let q = rotate_point(Point64::new(p.x - spec.pos.x, p.y - spec.pos.y), -a);
        x0 = x0.min(q.x);
        y0 = y0.min(q.y);
        x1 = x1.max(q.x);
        y1 = y1.max(q.y);
        bx0 = bx0.min(p.x);
        by0 = by0.min(p.y);
        bx1 = bx1.max(p.x);
        by1 = by1.max(p.y);
    }
    if x0 > x1 {
        return None;
    }
    let hull: LineChain = [(x0 - margin, y0 - margin), (x1 + margin, y0 - margin), (x1 + margin, y1 + margin), (x0 - margin, y1 + margin)]
        .iter()
        .map(|&(x, y)| {
            let q = rotate_point(Point64::new(x, y), a);
            Point64::new(spec.pos.x + q.x, spec.pos.y + q.y)
        })
        .collect();
    Some((hull, [bx0, by0, bx1, by1]))
}

/// The knockout of a free board text.
fn text_knockout(design: &Design, text: &Text, margin: i64) -> Option<(LineChain, [i64; 4])> {
    let extra = design.drawings.as_ref().and_then(|d| d.text_extra(&text.id));
    let spec = TextSpec {
        text: &text.content,
        pos: text.at,
        size: (extra.and_then(|e| e.width_um).unwrap_or(text.size_um), text.size_um),
        thickness: text.stroke_width,
        draw_angle_mdeg: text.angle as i64,
        mirror: text.mirror,
        halign: match text.justify {
            TextJustify::Left => -1,
            TextJustify::Center => 0,
            TextJustify::Right => 1,
        },
        valign: match extra.map(|e| e.valign).unwrap_or_default() {
            TextVAlign::Top => -1,
            TextVAlign::Center => 0,
            TextVAlign::Bottom => 1,
        },
        bold: extra.is_some_and(|e| e.bold),
    };
    text_hull(&spec, margin)
}

/// A text box's knockout (`PCB_TEXTBOX::TransformShapeToPolygon` with no margin: the box counts as filled): its corners and, with a border, ovals along its sides.
fn text_box_knockout(out: &mut ShapePolySet, tb: &TextBox) {
    let pts = tb.points();
    let corners: Vec<Point> = if tb.is_rect() {
        let (a, b) = (tb.start, tb.end);
        vec![a, Point { x: b.x, y: a.y }, b, Point { x: a.x, y: b.y }]
    } else {
        pts
    };
    out.add_outline(corners.iter().map(|p| p64(*p)).collect());
    if tb.border && tb.stroke_width_um > 0 {
        ovals_along(out, &corners, true, tb.stroke_width_um);
    }
}

/// The fields of a placed footprint that have a text and a place: Reference, Value and the user fields (`add_footprint_art` lists the same ones), with the text each shows.
fn footprint_fields(design: &Design, part: &eda_model::Part, fp: &FootprintInstance, footprint: &eda_model::Footprint) -> Vec<(FieldLayout, String)> {
    let value = part.value.as_deref().unwrap_or(&fp.id);
    let reference = effective_reference(design, fp, footprint);
    let value_layout = effective_value(design, fp);
    let mut fields = vec![(reference.clone(), reference.shown(&fp.id).to_string()), (value_layout.clone(), value_layout.shown(value).to_string())];
    for f in design.footprint_edit(&fp.id).map(|e| e.fields.as_slice()).unwrap_or_default() {
        fields.push((f.layout.clone(), f.text.clone()));
    }
    fields
}

/// What the footprints of the board put in the knockouts: their courtyard on a courtyard layer, their visible fields and their graphics on the layer, each grown by `margin`.
fn footprint_knockouts(out: &mut ShapePolySet, design: &Design, model: &ConstraintModel, layer: &str, my_box: [i64; 4], margin: i64) {
    let Some(placement) = design.placement.as_ref() else { return };
    for fp in &placement.footprints {
        let Some(part) = model.part(&fp.id) else { continue };
        let Some(footprint) = model.footprint_of(part) else { continue };
        // `footprint->GetCourtyard( layer )`: the front courtyard on F.CrtYd, the back one on B.CrtYd.
        if (layer == "F.CrtYd" && fp.side == Side::Top) || (layer == "B.CrtYd" && fp.side == Side::Bottom) {
            for outline in &footprint.courtyard_outlines {
                out.add_outline(outline.iter().map(|&p| p64(to_board(fp, p))).collect());
            }
            if footprint.courtyard_outlines.is_empty() {
                if let Some((x0, y0, x1, y1)) = eda_model::footprint::placed_courtyard(model, part, fp) {
                    out.add_outline(vec![Point64::new(x0, y0), Point64::new(x1, y0), Point64::new(x1, y1), Point64::new(x0, y1)]);
                }
            }
        }
        for (layout, shown) in footprint_fields(design, part, fp, &footprint) {
            if !layout.visible || shown.is_empty() || layout.layer != layer {
                continue;
            }
            let spec = TextSpec {
                text: &shown,
                pos: layout.board_position(fp),
                size: layout.size,
                thickness: layout.thickness,
                draw_angle_mdeg: draw_rotation(layout.board_angle(fp), layout.keep_upright),
                mirror: layout.mirror,
                halign: layout.halign,
                valign: layout.valign,
                bold: layout.bold,
            };
            if let Some((hull, bbox)) = text_hull(&spec, margin) {
                if boxes_meet(bbox, my_box) {
                    out.add_outline(hull);
                }
            }
        }
        for art in &footprint.art.shapes {
            let placed = art.placed(fp);
            if placed.on_layer(layer) && boxes_meet(shape_box(&placed.shape), my_box) {
                shape_knockout(out, &placed.shape, placed.is_any_fill(), margin);
            }
        }
    }
}

/// `PCB_SHAPE::getHatchingKnockouts`: the polygons of the other shapes, texts and text boxes on the layer of `me` whose box meets its own, and what the footprints put on the layer
/// (when the model that holds them is at hand), grown by `margin`.
fn knockouts(design: &Design, model: Option<&ConstraintModel>, me: &Shape, margin: i64) -> ShapePolySet {
    let mut out = ShapePolySet::new();
    let layer = me.layer();
    let my_box = shape_box(me);
    if let Some(dr) = design.drawings.as_ref() {
        for other in &dr.shapes {
            if other.id() == me.id() || other.layer() != layer || !boxes_meet(shape_box(other), my_box) {
                continue;
            }
            let solid = other.is_filled() || dr.board_items.hatch_of(other.id()).is_some();
            shape_knockout(&mut out, other, solid, margin);
        }
        for text in &dr.texts {
            if text.layer != layer {
                continue;
            }
            if let Some((hull, bbox)) = text_knockout(design, text, margin) {
                if boxes_meet(bbox, my_box) {
                    out.add_outline(hull);
                }
            }
        }
        for tb in &dr.board_items.text_boxes {
            if tb.layer != layer {
                continue;
            }
            let b = tb.bounds();
            let half = tb.stroke_width_um.max(0) / 2;
            if boxes_meet([b[0] - half, b[1] - half, b[2] + half, b[3] + half], my_box) {
                text_box_knockout(&mut out, tb);
            }
        }
    }
    if let Some(model) = model {
        footprint_knockouts(&mut out, design, model, layer, my_box, margin);
    }
    out
}

/// What `UpdateHatching` has worked out before it makes lines or polygons: the shape's area with the knockouts taken out, the spacing, and the knockouts.
struct Prepared {
    width: Um,
    spacing: i64,
    area: ShapePolySet,
    knockouts: ShapePolySet,
}

fn prepare(design: &Design, model: Option<&ConstraintModel>, shape: &Shape) -> Option<Prepared> {
    let width = shape.stroke_width().max(0);
    let uncapped = width * 10;
    if uncapped == 0 {
        return None;
    }
    let mut area = ShapePolySet::from_outline(outline_of(shape)?);
    // At most a hundred lines along the longer side.
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in area.outline(0) {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let major_axis = (x1 - x0).max(y1 - y0);
    let mut spacing = uncapped;
    if major_axis / spacing > 100 {
        spacing = major_axis / 100;
    }
    if spacing <= 0 {
        return None;
    }
    let knock = knockouts(design, model, shape, uncapped / 2);
    if !knock.is_empty() {
        area.boolean_subtract(&knock);
        area.fracture(true);
    }
    Some(Prepared { width, spacing, area, knockouts: knock })
}

/// The hatch lines of `shape` as `EDA_SHAPE::UpdateHatching` makes them, the board being `design` (and `model`, which holds the footprints' definitions, if given). A shape that has no inside, no line width or no room gives none.
pub fn hatching(design: &Design, model: Option<&ConstraintModel>, shape: &Shape, kind: HatchKind) -> Hatching {
    let Some(prep) = prepare(design, model, shape) else { return Hatching { width: shape.stroke_width().max(0), lines: Vec::new() } };
    let lines = prep.area.generate_hatch_lines(kind.slopes(), prep.spacing, -1).into_iter().map(|(a, b)| (pt(a), pt(b))).collect();
    Hatching { width: prep.width, lines }
}

/// `GetHatching()`: the polygons the plot has for the hatching. A hatch and a reverse hatch are a stadium per line (`TransformOvalToPolygon( .., lineWidth, lineWidth, ERROR_INSIDE )`);
/// a cross-hatch is the shape turned by -45 degrees with a grid of squares `spacing - width` wide cut out of it every `spacing`, turned back and with the knockouts taken out.
pub fn hatch_polygons(design: &Design, model: Option<&ConstraintModel>, shape: &Shape, kind: HatchKind) -> Vec<LineChain> {
    let Some(prep) = prepare(design, model, shape) else { return Vec::new() };
    if kind != HatchKind::CrossHatch {
        return prep
            .area
            .generate_hatch_lines(kind.slopes(), prep.spacing, -1)
            .into_iter()
            .map(|(a, b)| oval_polygon(a, b, prep.width, prep.width, ErrorLoc::Inside, 0))
            .collect();
    }
    let quarter = std::f64::consts::FRAC_PI_4;
    let rotate = |set: &mut ShapePolySet, angle: f64| {
        for poly in &mut set.polys {
            for chain in poly.iter_mut() {
                for p in chain.iter_mut() {
                    *p = rotate_point(*p, angle);
                }
            }
        }
    };
    let grid = prep.spacing;
    let hole_size = grid - prep.width;
    let mut lattice = prep.area.clone();
    rotate(&mut lattice, -quarter);
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for p in lattice.polys.iter().flat_map(|p| p.iter()).flat_map(|c| c.iter()) {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if x0 > x1 {
        return Vec::new();
    }
    // C++ `%` keeps the sign of the dividend, as Rust's does.
    let x_offset = x0 - x0 % grid - grid;
    let y_offset = y0 - y0 % grid - grid;
    let mut holes = ShapePolySet::new();
    let mut xx = x_offset;
    while xx <= x1 {
        let mut yy = y_offset;
        while yy <= y1 {
            holes.add_outline(vec![Point64::new(xx, yy), Point64::new(xx + hole_size, yy), Point64::new(xx + hole_size, yy + hole_size), Point64::new(xx, yy + hole_size)]);
            yy += grid;
        }
        xx += grid;
    }
    lattice.boolean_subtract(&holes);
    lattice.fracture(true);
    rotate(&mut lattice, quarter);
    if !prep.knockouts.is_empty() {
        lattice.boolean_subtract(&prep.knockouts);
        lattice.fracture(true);
    }
    lattice.polys.into_iter().flat_map(|mut poly| poly.drain(..).take(1).collect::<Vec<_>>()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{DrawingsSection, PlacementSection, Provenance, TextJustify};

    fn design_with(shapes: Vec<Shape>) -> Design {
        Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            routing: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            drawings: Some(DrawingsSection { shapes, ..Default::default() }),
        }
    }

    fn rect(layer: &str, w: Um, x0: Um, y0: Um, x1: Um, y1: Um, id: &str) -> Shape {
        Shape::Rect { id: id.into(), layer: layer.into(), stroke_width: w, filled: false, start: Point { x: x0, y: y0 }, end: Point { x: x1, y: y1 } }
    }

    #[test]
    fn a_hatched_rectangle_has_lines_ten_widths_apart_across_it() {
        // 10 mm by 10 mm, a 0.2 mm line: spacing 2 mm, slope -1 lines at a = 0, 2, 4 .. mm -- 9 chords of x + y = a, a from 2 mm to 18 mm.
        let r = rect("F.Fab", 200, 0, 0, 10_000, 10_000, "shp_a");
        let d = design_with(vec![r.clone()]);
        let h = hatching(&d, None, &r, HatchKind::Hatch);
        assert_eq!(h.width, 200);
        assert_eq!(h.lines.len(), 9, "{:?}", h.lines);
        for (a, b) in &h.lines {
            assert_eq!(a.x + a.y, b.x + b.y, "slope -1 keeps x + y");
            assert_eq!((a.x + a.y) % 2_000, 0, "the lines are on multiples of the spacing");
        }
        // Reverse hatch the other way, cross-hatch both.
        let rev = hatching(&d, None, &r, HatchKind::ReverseHatch);
        assert!(rev.lines.iter().all(|(a, b)| a.y - a.x == b.y - b.x));
        assert_eq!(hatching(&d, None, &r, HatchKind::CrossHatch).lines.len(), h.lines.len() + rev.lines.len());
    }

    #[test]
    fn the_spacing_is_cut_down_to_a_hundredth_of_the_longer_side() {
        // A 100 mm square with a 0.1 mm line: spacing would be 1 mm (100 lines); a 200 mm one would be 200, so 2 mm.
        let r = rect("F.Fab", 100, 0, 0, 200_000, 200_000, "shp_a");
        let d = design_with(vec![r.clone()]);
        let h = hatching(&d, None, &r, HatchKind::Hatch);
        // On 2 mm steps: the chords of x + y = 2k mm.
        assert!(h.lines.iter().all(|(a, _)| (a.x + a.y) % 2_000 == 0));
        assert!(h.lines.len() > 150 && h.lines.len() < 210, "{}", h.lines.len());
    }

    #[test]
    fn a_shape_without_a_line_width_or_an_inside_has_no_lines() {
        let r = rect("F.Fab", 0, 0, 0, 10_000, 10_000, "shp_a");
        let d = design_with(vec![r.clone()]);
        assert!(hatching(&d, None, &r, HatchKind::Hatch).lines.is_empty());
        let seg = Shape::Segment { id: "shp_s".into(), layer: "F.Fab".into(), stroke_width: 200, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 10_000, y: 0 } };
        assert!(hatching(&design_with(vec![seg.clone()]), None, &seg, HatchKind::Hatch).lines.is_empty());
    }

    #[test]
    fn a_circle_and_a_polygon_are_hatched_inside() {
        let c = Shape::Circle { id: "shp_c".into(), layer: "F.Fab".into(), stroke_width: 200, filled: false, center: Point { x: 0, y: 0 }, end: Point { x: 5_000, y: 0 } };
        let d = design_with(vec![c.clone()]);
        let h = hatching(&d, None, &c, HatchKind::Hatch);
        assert!(h.lines.len() >= 4);
        for (a, b) in &h.lines {
            for p in [a, b] {
                assert!(((p.x * p.x + p.y * p.y) as f64).sqrt() <= 5_001.0, "{p:?}");
            }
        }
        let tri = Shape::Polygon { id: "shp_p".into(), layer: "F.Fab".into(), stroke_width: 200, filled: false, pts: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 0, y: 10_000 }] };
        let h = hatching(&design_with(vec![tri.clone()]), None, &tri, HatchKind::ReverseHatch);
        assert!(!h.lines.is_empty());
        assert!(h.lines.iter().all(|(a, b)| a.x + a.y <= 10_001 && b.x + b.y <= 10_001));
    }

    #[test]
    fn another_shape_on_the_layer_knocks_the_lines_out_and_one_on_another_layer_does_not() {
        let r = rect("F.Fab", 200, 0, 0, 20_000, 20_000, "shp_a");
        // A line right across, 1 mm wide: it and half the spacing (1 mm) either side are free of hatching.
        let bar = |layer: &str, id: &str| Shape::Segment { id: id.into(), layer: layer.into(), stroke_width: 1_000, filled: false, start: Point { x: 10_000, y: -1_000 }, end: Point { x: 10_000, y: 21_000 } };
        let alone = hatching(&design_with(vec![r.clone()]), None, &r, HatchKind::Hatch);
        let with_bar = hatching(&design_with(vec![r.clone(), bar("F.Fab", "shp_b")]), None, &r, HatchKind::Hatch);
        let with_other_layer = hatching(&design_with(vec![r.clone(), bar("F.SilkS", "shp_c")]), None, &r, HatchKind::Hatch);
        assert_eq!(alone, with_other_layer);
        assert!(with_bar.lines.len() > alone.lines.len(), "the long lines are cut in two: {} vs {}", with_bar.lines.len(), alone.lines.len());
        // The bar is 1 mm wide, half the spacing (1 mm) is kept free either side: nothing of a line is between x = 8.5 mm and 11.5 mm but ... the lines stop at its edge.
        for (a, b) in &with_bar.lines {
            for p in [a, b] {
                assert!(p.x <= 8_500 + 3 || p.x >= 11_500 - 3 || p.x <= 0 + 3 || p.x >= 20_000 - 3, "{p:?}");
            }
        }
    }

    #[test]
    fn a_text_on_the_layer_knocks_a_box_out() {
        let r = rect("F.Fab", 200, 0, 0, 20_000, 20_000, "shp_a");
        let mut d = design_with(vec![r.clone()]);
        d.drawings.as_mut().unwrap().texts.push(Text { id: "txt_a".into(), content: "HELLO".into(), at: Point { x: 6_000, y: 10_000 }, angle: 0, layer: "F.Fab".into(), size_um: 2_000, stroke_width: 200, justify: TextJustify::Left, mirror: false });
        let with_text = hatching(&d, None, &r, HatchKind::Hatch);
        let alone = hatching(&design_with(vec![r.clone()]), None, &r, HatchKind::Hatch);
        assert!(with_text.lines.len() > alone.lines.len());
    }

    fn spec_at(angle_mdeg: i64) -> TextSpec<'static> {
        TextSpec { text: "HELLO", pos: Point { x: 10_000, y: 10_000 }, size: (2_000, 2_000), thickness: 200, draw_angle_mdeg: angle_mdeg, mirror: false, halign: -1, valign: 0, bold: false }
    }

    fn extent(hull: &LineChain) -> (i64, i64) {
        let (xs, ys): (Vec<i64>, Vec<i64>) = (hull.iter().map(|p| p.x).collect(), hull.iter().map(|p| p.y).collect());
        (xs.iter().max().unwrap() - xs.iter().min().unwrap(), ys.iter().max().unwrap() - ys.iter().min().unwrap())
    }

    #[test]
    fn the_box_round_a_turned_text_is_turned_with_it() {
        let (flat, _) = text_hull(&spec_at(0), 1_000).unwrap();
        let (up, _) = text_hull(&spec_at(90_000), 1_000).unwrap();
        let ((w0, h0), (w90, h90)) = (extent(&flat), extent(&up));
        assert!(w0 > h0 * 2, "a word is wider than it is tall: {w0} x {h0}");
        assert!((w0 - h90).abs() <= 2 && (h0 - w90).abs() <= 2, "turned a quarter, the box has the other sides: {w0} x {h0} against {w90} x {h90}");
        // Left-justified text starts at its anchor and goes along its direction: the flat one to the right, the one turned a quarter (counter-clockwise on the screen) up -- smaller y.
        assert!(flat.iter().all(|p| p.x >= 10_000 - 1_300) && flat.iter().any(|p| p.x > 14_000));
        assert!(up.iter().all(|p| p.y <= 10_000 + 1_300) && up.iter().any(|p| p.y < 6_000), "{up:?}");
        // A tilt of 45 degrees makes a box that is not axis-aligned.
        let (tilt, _) = text_hull(&spec_at(45_000), 1_000).unwrap();
        assert!(tilt.windows(2).any(|w| w[0].x != w[1].x && w[0].y != w[1].y), "{tilt:?}");
    }

    #[test]
    fn a_footprint_field_on_the_layer_knocks_the_lines_out() {
        use eda_model::footprint::{Footprint as Fp, Pad, PadKind, PadShape};
        use eda_model::{ConstraintModel, Part};
        let fp = Fp { name: "2PAD".into(), pads: vec![Pad { opposite_side: false, ext: Default::default(), number: "1".into(), at: (-1000, 0), size: (800, 800), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None }], courtyard: Some((2000, 1000)), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
        let part = Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("VALUE".into()), package: Some("2PAD".into()), footprint: Some("2PAD".into()), pins: vec![], body_um: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part], footprints: vec![fp], ..Default::default() };
        let r = rect("F.Fab", 200, 0, 0, 20_000, 20_000, "shp_a");
        let mut d = design_with(vec![r.clone()]);
        d.placement = Some(eda_model::ir::PlacementSection {
            outline: vec![],
            footprints: vec![FootprintInstance { id: "U1".into(), at: Point { x: 10_000, y: 10_000 }, rot: 0, side: Side::Top, label: Default::default() }],
            modules: vec![],
        });
        let without = hatching(&d, None, &r, HatchKind::Hatch);
        let with = hatching(&d, Some(&model), &r, HatchKind::Hatch);
        assert!(with.lines.len() > without.lines.len(), "the Value field of U1 is on F.Fab and cuts the lines it is over: {} against {}", with.lines.len(), without.lines.len());
        // On a layer none of its fields is on, the footprint cuts nothing.
        let silk = rect("F.CrtYd", 200, 0, 0, 20_000, 20_000, "shp_s");
        let mut d2 = design_with(vec![silk.clone()]);
        d2.placement = d.placement.clone();
        assert_eq!(hatching(&d2, Some(&model), &silk, HatchKind::Hatch).lines.len() > hatching(&d2, None, &silk, HatchKind::Hatch).lines.len(), true, "but its courtyard cuts a courtyard layer's hatching");
    }

    #[test]
    fn the_plot_has_a_stadium_per_line_for_a_hatch() {
        let r = rect("F.Fab", 200, 0, 0, 10_000, 10_000, "shp_a");
        let d = design_with(vec![r.clone()]);
        let lines = hatching(&d, None, &r, HatchKind::Hatch).lines;
        let polys = hatch_polygons(&d, None, &r, HatchKind::Hatch);
        assert_eq!(polys.len(), lines.len());
    }

    #[test]
    fn the_plot_of_a_cross_hatch_is_the_shape_with_holes_in_a_grid() {
        let r = rect("F.Fab", 200, 0, 0, 10_000, 10_000, "shp_a");
        let d = design_with(vec![r.clone()]);
        let polys = hatch_polygons(&d, None, &r, HatchKind::CrossHatch);
        assert!(!polys.is_empty());
        // Most of the square is a hole, so the ink is a small part of it: bars 0.2 mm wide every 2 mm in two directions.
        let area: f64 = polys.iter().map(|p| eda_clipper2::area(p).abs()).sum();
        assert!(area > 0.0 && area < 0.5 * 100_000_000.0, "{area}");
    }
}
