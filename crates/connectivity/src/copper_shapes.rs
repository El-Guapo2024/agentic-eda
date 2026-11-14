//! The copper of the board's items that are not pads, tracks or vias, as the DRC's shapes (`eda_drc::kimath::Shape`): what `GetEffectiveShape( layer )` gives a
//! graphic, a text, a table, a dimension and a barcode on a copper layer. Read by the teardrops' copper index (`TEARDROP_MANAGER::ensureCopperIndex`,
//! [`crate::teardrop`]), the connectivity's graphics ([`crate::items`]) and Draw Via's check (`eda_ops::via_place`, `VIA_PLACER::checkDRCViolation`).

use eda_drc::kimath::Shape;
use eda_model::ir::Point;
use eda_model::units::Nm;

/// `EDA_SHAPE::SetPolyShape`: a polygon with holes is kept fractured (`if( GetPolyShape().HoleCount( ii ) ) GetPolyShape().Fracture()`), one outline with each hole
/// bridged into it, so its `SHAPE_SIMPLE` (`makeEffectiveShapes`' `COutline( ii )`) leaves the holes out and its stroke runs round them and along the bridges
/// (UI-LEFT-3 P3-16). The outline as it is when it has no hole. (`SHAPE_POLY_SET::Fracture`, as the board writer bridges it: `eda_kicad::pcb`.)
pub fn fractured_ring(outline: &[Point], holes: &[Vec<Point>]) -> Vec<Point> {
    let holes: Vec<&Vec<Point>> = holes.iter().filter(|h| h.len() >= 3).collect();
    if holes.is_empty() || outline.len() < 3 {
        return outline.to_vec();
    }
    use eda_clipper2::Point64;
    let p64 = |c: &[Point]| c.iter().map(|p| Point64::new(p.x.get(), p.y.get())).collect::<Vec<Point64>>();
    let ring = p64(outline);
    let ccw = eda_clipper2::area(&ring) > 0.0;
    let mut poly = vec![ring];
    for h in holes {
        let mut hole = p64(h);
        // a hole runs against its outline (`SHAPE_POLY_SET`'s convention, which the fracture bridges by)
        if (eda_clipper2::area(&hole) > 0.0) == ccw {
            hole.reverse();
        }
        poly.push(hole);
    }
    let mut set = eda_shape_poly_set::ShapePolySet::new();
    set.add_polygon(poly);
    set.fracture(false);
    set.polys.first().and_then(|p| p.first()).map(|r| r.iter().map(|p| Point { x: Nm::new(p.x), y: Nm::new(p.y) }).collect()).unwrap_or_else(|| outline.to_vec())
}

/// `PCB_SHAPE::GetEffectiveShape`: [`eda_drc::board::effective_shapes`] of the graphic, a polygon with holes (`holes`, `ShapeStyle::holes`) taken as its
/// [`fractured_ring`].
pub fn graphic_effective_shapes(s: &eda_model::ir::Shape, holes: &[Vec<Point>]) -> Vec<Shape> {
    match s {
        eda_model::ir::Shape::Polygon { pts, .. } if holes.iter().any(|h| h.len() >= 3) => {
            let mut whole = s.clone();
            if let eda_model::ir::Shape::Polygon { pts: p, .. } = &mut whole {
                *p = fractured_ring(pts, holes);
            }
            eda_drc::board::effective_shapes(&whole)
        }
        _ => eda_drc::board::effective_shapes(s),
    }
}

/// `PCB_TABLE::GetEffectiveShape`: the polygon of the table's outer corners and, for the borders it draws, their strokes (`DrawBorders`'s segments of the
/// stroke's width). The outline's sides stand for every border here, at the widest stroke the table draws (an inner separator lies inside the polygon).
pub fn table_effective_shape(t: &eda_model::table::Table) -> Shape {
    let (w, h) = t.local_size();
    let corners: Vec<Point> = [(Nm::ZERO, Nm::ZERO), (w, Nm::ZERO), (w, h), (Nm::ZERO, h)].iter().map(|&c| t.to_world(c)).collect();
    let sep = if t.separators.rows || t.separators.cols { t.separators.stroke.width_nm / 2 } else { Nm::ZERO };
    let border = if t.border.external || t.border.header { t.border.stroke.width_nm / 2 } else { Nm::ZERO };
    let gap = sep.max(border);
    if gap <= Nm::ZERO {
        return Shape::Polygon { pts: corners };
    }
    let ring: Vec<eda_drc::kimath::Seg> = (0..4).map(|k| eda_drc::kimath::Seg::new(corners[k], corners[(k + 1) % 4])).collect();
    Shape::Compound { parts: vec![Shape::Polygon { pts: corners }, Shape::Strokes { segs: ring, r: gap }] }
}

/// `PCB_DIMENSION_BASE::GetEffectiveShape`: `GetEffectiveTextShape()` (the label's strokes, its pen wide) and every shape of `GetShapes()` -- the lines as
/// `SHAPE_SEGMENT`s of no width, a leader's circle frame as a solid `SHAPE_CIRCLE`.
pub fn dimension_effective_shapes(dim: &eda_model::ir::Dimension) -> Vec<Shape> {
    let g = crate::dimension::compute_dimension_geometry(dim);
    let mut out: Vec<Shape> = g.lines.iter().map(|&(a, b)| Shape::Stadium { a, b, r: Nm::ZERO }).collect();
    out.extend(g.circles.iter().map(|&(c, r)| Shape::Circle { c, r }));
    if !g.text.is_empty() {
        let size = (dim.text_size_nm, dim.text_size_nm);
        let thickness = dim.text_thickness_nm.unwrap_or_else(|| Nm::kiround(dim.text_size_nm.as_f64() * 0.15));
        let pen = eda_drc::stroke_font::effective_pen_width(thickness, size, false);
        let rot = (g.text_angle as f64 / 1000.0).rem_euclid(360.0);
        let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &g.text, pos: g.text_at, size, thickness, angle_deg: rot, mirror: false, halign: 0, valign: 0 });
        if !segs.is_empty() {
            out.push(Shape::Strokes { segs, r: pen / 2 });
        }
    }
    out
}

/// `PCB_BARCODE::GetEffectiveShape`, as far as the model knows the barcode: the box of its symbol (the barcode's width and height about its centre, turned
/// by its angle) and, when the text is shown, the box of the printed text under it.
pub fn barcode_box_shapes(b: &eda_model::barcode::Barcode) -> Vec<Shape> {
    let (hw, hh) = (b.width_nm.as_f64() / 2.0, b.height_nm.as_f64() / 2.0);
    // `RotatePoint( corner, m_pos, m_angle )`: counter-clockwise on the screen, y down
    let (sin, cos) = (b.angle as f64 / 1000.0).to_radians().sin_cos();
    let corner = |x: f64, y: f64| Point { x: b.at.x + Nm::kiround(x * cos + y * sin), y: b.at.y + Nm::kiround(-x * sin + y * cos) };
    let mut out = vec![Shape::Polygon { pts: vec![corner(-hw, -hh), corner(hw, -hh), corner(hw, hh), corner(-hw, hh)] }];
    if b.show_text && !b.text.is_empty() {
        let h = b.text_height_nm.max(Nm::new(1));
        let pen = Nm::kiround(h.as_f64() / 8.0).max(Nm::new(1));
        let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &b.text, pos: Point { x: Nm::ZERO, y: Nm::ZERO }, size: (h, h), thickness: pen, angle_deg: 0.0, mirror: false, halign: 0, valign: 0 });
        let r = pen / 2;
        if let (Some(x0), Some(x1), Some(y0), Some(y1)) = (segs.iter().flat_map(|s| [s.a.x, s.b.x]).min(), segs.iter().flat_map(|s| [s.a.x, s.b.x]).max(), segs.iter().flat_map(|s| [s.a.y, s.b.y]).min(), segs.iter().flat_map(|s| [s.a.y, s.b.y]).max()) {
            let (tw, th) = ((x1 - x0 + r * 2).as_f64(), (y1 - y0 + r * 2).as_f64());
            // centred under the symbol, its top 1 mm below the symbol's bottom (`ComputeTextPoly`)
            let top = hh + 1_000_000.0;
            out.push(Shape::Polygon { pts: vec![corner(-tw / 2.0, top), corner(tw / 2.0, top), corner(tw / 2.0, top + th), corner(-tw / 2.0, top + th)] });
        }
    }
    out
}

/// `PCB_TEXT::GetEffectiveShape` of a board text, with what its extras say (round 6 C6-3): the glyph width (`width_nm`, else square), the vertical
/// justification, bold (the automatic pen of a text with no thickness), and for a knockout text `TransformTextToPolySet`'s
/// `buildBoundingHull( &finalPoly, textShape, margin )` (R7-4): the strokes turned back by the draw rotation about the text's position
/// (`poly.Rotate( -GetDrawRotation(), GetDrawPos() )`), their box (`BBox( aClearance )`) grown by `GetKnockoutTextMargin( size, penWidth )` =
/// `max( KiROUND( penWidth / 2.0 ), KiROUND( size.y / 9.0 ) )`, its four corners turned forward again (`RotatePoint( corner, GetDrawPos(),
/// GetDrawRotation() )`). The glyphs it then subtracts are inside the rectangle and no via fits in one.
pub fn board_text_effective_shape(t: &eda_model::ir::Text, extra: Option<&eda_model::board_text::TextExtra>) -> Option<Shape> {
    let default = eda_model::board_text::TextExtra::default();
    let x = extra.unwrap_or(&default);
    let size = (x.width_nm.unwrap_or(t.size_nm), t.size_nm);
    let thickness = if x.auto_thickness { Nm::ZERO } else { t.stroke_width };
    let pen = eda_drc::stroke_font::effective_pen_width(thickness, size, x.bold);
    let halign = match t.justify {
        eda_model::ir::TextJustify::Left => -1,
        eda_model::ir::TextJustify::Center => 0,
        eda_model::ir::TextJustify::Right => 1,
    };
    let valign = match x.valign {
        eda_model::ir::TextVAlign::Top => -1,
        eda_model::ir::TextVAlign::Center => 0,
        eda_model::ir::TextVAlign::Bottom => 1,
    };
    let rot = (t.angle as f64 / 1000.0).rem_euclid(360.0);
    let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &t.content, pos: t.at, size, thickness, angle_deg: rot, mirror: t.mirror, halign, valign });
    if segs.is_empty() {
        return None;
    }
    if !x.knockout {
        return Some(Shape::Strokes { segs, r: pen / 2 });
    }
    let margin = Nm::kiround(pen.as_f64() / 2.0).max(Nm::kiround(size.1.as_f64() / 9.0));
    // `RotatePoint( p, centre, angle )` with y down: x' = x cos + y sin, y' = -x sin + y cos (relative to the centre)
    let (sin, cos) = rot.to_radians().sin_cos();
    let turn = |p: (f64, f64), s: f64| (p.0 * cos + p.1 * s, -p.0 * s + p.1 * cos);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in segs.iter().flat_map(|s| [s.a, s.b]) {
        let q = turn(((p.x - t.at.x).as_f64(), (p.y - t.at.y).as_f64()), -sin);
        (x0, y0, x1, y1) = (x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1));
    }
    // the rendered text's own box is its strokes' with half the pen, then the margin
    let grow = (pen.get() / 2 + margin.get()) as f64;
    let pts = [(x0 - grow, y0 - grow), (x1 + grow, y0 - grow), (x1 + grow, y1 + grow), (x0 - grow, y1 + grow)]
        .into_iter()
        .map(|c| {
            let q = turn(c, sin);
            Point { x: t.at.x + Nm::kiround(q.0), y: t.at.y + Nm::kiround(q.1) }
        })
        .collect();
    Some(Shape::Polygon { pts })
}

