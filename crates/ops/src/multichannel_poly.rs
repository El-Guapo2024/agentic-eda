//! Is an item inside a rule area? `A.enclosedByArea( '<uuid>' )` (`enclosedByAreaFunc`, `pcbnew/pcbexpr_functions.cpp`) as the multichannel tool asks it of every item of the board
//! (`MULTICHANNEL_TOOL::findRoutingInRuleArea`, `findOtherItemsInRuleArea`): the item's polygon minus the area's polygon leaves nothing.
//!
//! ```text
//! item->TransformShapeToPolygon( itemShape, layer, 0, maxError, ERROR_OUTSIDE );   // a zone: *zone->Outline(), its holes with it
//! if( itemShape.IsEmpty() )  enclosedByArea = false;
//! else                       { itemShape.BooleanSubtract( *aArea->Outline() ); enclosedByArea = itemShape.IsEmpty(); }
//! ```
//!
//! The area's polygon is its outline with its holes: an item over a hole of the area sticks out of it. The item's polygon is the one KiCad builds, kind by kind
//! (`item_shape`), with the board's maximum error, and the subtraction is Clipper2's (`eda_shape_poly_set`, KiCad's own vendored 2.0.1 through `eda_clipper2`). `layer`
//! is the context's, which the tool never sets: `PCBEXPR_CONTEXT( int aConstraint = 0, PCB_LAYER_ID aLayer = F_Cu )`. That decides two kinds:
//!
//! - a barcode's polygon is only on its own layer (`PCB_BARCODE::TransformShapeToPolygon`: `if( aLayer != m_layer && aLayer != UNDEFINED_LAYER ) return;`), so a barcode is
//!   enclosed by nothing unless it is on F.Cu;
//! - a reference image has no polygon (`BOARD_ITEM::TransformShapeToPolygon` only logs "doesn't implement"), nor has a point (`PCB_POINT::TransformShapeToPolygon` is empty):
//!   an empty polygon is never enclosed, so neither is copied by Repeat Layout (a point is not even among the board's drawings: `BOARD::Add` keeps it in `m_points`).
//!
//! Where this is not KiCad's polygon to the nanometre: a barcode (KiCad's comes from zint, which this studio has not) is the box of its symbol and of its text below it, grown
//! by its knockout margin, turned -- the plotted polygon is inside it; a rectangle with rounded corners is its square-cornered rectangle; a text is the box of its strokes
//! (`buildBoundingHull`, which is KiCad's) with the stroke font's glyphs.

use super::multichannel_copy::Item;
use eda_clipper2::Point64;
use eda_model::ir::{Nm, Point, Shape, Zone};
use eda_shape_poly_set::basic_shapes::{circle_polygon, oval_polygon, ErrorLoc};
use eda_shape_poly_set::{CornerStrategy, LineChain, ShapePolySet};

fn p64(p: Point) -> Point64 {
    Point64::new(p.x.get(), p.y.get())
}

/// A rule area's polygon (`*aArea->Outline()`): its outline and its holes.
pub(crate) struct AreaPoly {
    poly: ShapePolySet,
    /// `aArea->GetBoundingBox()`.
    bbox: [i64; 4],
    /// `aRAPoly.Outline( 0 )`, the outline alone: what a tuning pattern is hit-tested against (`generator->HitTest( aRAPoly.Outline( 0 ), false )`).
    outline: ShapePolySet,
}

fn bbox_of(set: &ShapePolySet) -> [i64; 4] {
    let mut b = [i64::MAX, i64::MAX, i64::MIN, i64::MIN];
    for p in set.polys.iter().flat_map(|poly| poly.iter()).flat_map(|c| c.iter()) {
        b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
    }
    b
}

impl AreaPoly {
    pub(crate) fn new(outline: &[Point], holes: &[Vec<Point>]) -> AreaPoly {
        let mut poly = ShapePolySet::new();
        let mut only = ShapePolySet::new();
        if outline.len() >= 3 {
            let idx = poly.add_outline(outline.iter().map(|p| p64(*p)).collect());
            for h in holes.iter().filter(|h| h.len() >= 3) {
                poly.add_hole(h.iter().map(|p| p64(*p)).collect(), Some(idx));
            }
            only.add_outline(outline.iter().map(|p| p64(*p)).collect());
        }
        let bbox = bbox_of(&poly);
        AreaPoly { poly, bbox, outline: only }
    }

    /// `enclosedByAreaFunc` for an item's polygon: the boxes meet, the polygon is not empty, and nothing of it is left once the area is taken away.
    pub(crate) fn encloses(&self, shape: &ShapePolySet) -> bool {
        if shape.is_empty() || self.poly.is_empty() {
            return false;
        }
        let b = bbox_of(shape);
        // `if( !aArea->GetBoundingBox().Intersects( itemBBox ) ) return false;`
        if b[0] > self.bbox[2] || b[2] < self.bbox[0] || b[1] > self.bbox[3] || b[3] < self.bbox[1] {
            return false;
        }
        let mut rest = ShapePolySet::new();
        rest.boolean_subtract_of(shape, &self.poly);
        rest.is_empty()
    }

    /// Does the polygon meet the area's outline (inside it or across its edge)? `KIGEOM::ShapeHitTest( aPoly, shape, aContained = false )`, which is `aHittee.Collide(
    /// &aHitter )` with a closed chain: a tuning pattern's track that touches the outline brings the pattern in.
    pub(crate) fn outline_meets(&self, shape: &ShapePolySet) -> bool {
        if shape.is_empty() || self.outline.is_empty() {
            return false;
        }
        let mut common = ShapePolySet::new();
        common.boolean_intersection_of(shape, &self.outline);
        !common.is_empty()
    }
}

/// The stadiums (`TransformOvalToPolygon`) of `width` along the polyline `pts` (closed or not).
fn ovals_along(out: &mut ShapePolySet, pts: &[Point], closed: bool, width: Nm, err: i64) {
    let n = pts.len();
    if n == 1 {
        out.add_outline(circle_polygon(p64(pts[0]), (width / 2).get(), err, ErrorLoc::Outside, 0));
        return;
    }
    let count = if closed { n } else { n.saturating_sub(1) };
    for i in 0..count {
        out.add_outline(oval_polygon(p64(pts[i]), p64(pts[(i + 1) % n]), width.get(), err, ErrorLoc::Outside, 0));
    }
}

fn rect_corners(a: Point, b: Point) -> [Point; 4] {
    [a, Point { x: b.x, y: a.y }, b, Point { x: a.x, y: b.y }]
}

/// `ZONE::Outline()`: the zone's polygon, its holes cut out.
pub(crate) fn zone_shape(z: &Zone) -> ShapePolySet {
    let mut out = ShapePolySet::new();
    if z.outline.len() >= 3 {
        let idx = out.add_outline(z.outline.iter().map(|p| p64(*p)).collect());
        for h in z.holes.iter().filter(|h| h.len() >= 3) {
            out.add_hole(h.iter().map(|p| p64(*p)).collect(), Some(idx));
        }
    }
    out
}

/// `PCB_SHAPE::TransformShapeToPolygon` (`EDA_SHAPE::TransformShapeToPolygon( .., ignoreLineWidth = false, includeFill = false )`): a filled or hatched shape is solid
/// (`solidFill = IsSolidFill() || ( IsHatchedFill() && !includeFill )`), the line grown to its width either way.
fn shape_shape(out: &mut ShapePolySet, shape: &Shape, solid: bool, err: i64) {
    let width = shape.stroke_width().max(Nm::ZERO);
    match shape {
        Shape::Segment { start, end, .. } => {
            out.add_outline(oval_polygon(p64(*start), p64(*end), width.get(), err, ErrorLoc::Outside, 0));
        }
        Shape::Arc { start, mid, end, .. } => {
            let pts = eda_model::outline::Arc::through(*start, *mid, *end).polyline(err as f64);
            ovals_along(out, &pts, false, width, err);
        }
        Shape::Bezier { .. } => {
            let pts = shape.bezier_points().unwrap_or_default();
            ovals_along(out, &pts, false, width, err);
        }
        Shape::Rect { start, end, .. } => {
            let corners = rect_corners(*start, *end);
            if solid {
                out.add_outline(corners.iter().map(|p| p64(*p)).collect());
            }
            if width > Nm::ZERO || !solid {
                ovals_along(out, &corners, true, width, err);
            }
        }
        Shape::Circle { center, end, .. } => {
            let r = Nm::kiround((end.x - center.x).as_f64().hypot((end.y - center.y).as_f64()));
            if solid {
                out.add_outline(circle_polygon(p64(*center), (r + width / 2).get(), err, ErrorLoc::Outside, 0));
            } else {
                // `TransformRingToPolygon`: a ring, a disc when the line is wider than the radius
                let inner = r - width / 2;
                if inner <= Nm::ZERO {
                    out.add_outline(circle_polygon(p64(*center), (r + width / 2).get(), err, ErrorLoc::Outside, 0));
                } else {
                    let idx = out.add_outline(circle_polygon(p64(*center), (inner + width).get(), err, ErrorLoc::Outside, 0));
                    out.add_hole(circle_polygon(p64(*center), inner.get(), err, ErrorLoc::Inside, 0), Some(idx));
                }
            }
        }
        Shape::Polygon { pts, .. } if pts.len() >= 2 => {
            if solid && pts.len() >= 3 {
                let mut tmp = ShapePolySet::from_outline(pts.iter().map(|p| p64(*p)).collect());
                if width > Nm::ZERO {
                    tmp.inflate((width / 2).get(), CornerStrategy::RoundAllCorners, err as i32, false);
                }
                for poly in tmp.polys {
                    out.add_polygon(poly);
                }
            } else {
                ovals_along(out, pts, true, width, err);
            }
        }
        Shape::Polygon { .. } => {}
    }
}

/// The polygon of a barcode on F.Cu: the box of its symbol (`SetRect( m_pos - size / 2, m_pos + size / 2 )`) and, with its text shown, the box of the text 1 mm below it
/// (`ComputeTextPoly`: `textPos.y = symbolBBox.GetBottom() - textBBox.GetTop() + mmToIU( 1 )`, centred under the symbol); knocked out, the box round them grown by the margin or a
/// tenth of the smaller side (rounded up to 0.1 mm); then turned about its position. KiCad's polygon (the symbol's modules, the glyphs) is inside it.
fn barcode_shape(out: &mut ShapePolySet, b: &eda_model::barcode::Barcode) {
    let (w, h) = (b.width_nm, b.height_nm);
    let (mut x0, mut y0, mut x1, mut y1) = (b.at.x - w / 2, b.at.y - h / 2, b.at.x + w / 2, b.at.y + h / 2);
    let mut boxes: Vec<[Nm; 4]> = vec![[x0, y0, x1, y1]];
    if b.show_text && !b.text.is_empty() && b.text_height_nm > Nm::ZERO {
        let size = (b.text_height_nm, b.text_height_nm);
        let pen = eda_drc::stroke_font::effective_pen_width(Nm::kiround(b.text_height_nm.as_f64() / 8.0), size, false);
        let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &b.text, pos: Point::default(), size, thickness: pen, angle_deg: 0.0, mirror: false, halign: 0, valign: 0 });
        if !segs.is_empty() {
            let (mut tx0, mut ty0, mut tx1, mut ty1) = (Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN);
            for s in &segs {
                for p in [s.a, s.b] {
                    tx0 = tx0.min(p.x);
                    ty0 = ty0.min(p.y);
                    tx1 = tx1.max(p.x);
                    ty1 = ty1.max(p.y);
                }
            }
            let (tw, th) = (tx1 - tx0 + pen, ty1 - ty0 + pen);
            let top = y1 + eda_model::units::mm(1.0);
            let cx = b.at.x;
            boxes.push([cx - tw / 2, top, cx + tw / 2, top + th]);
        }
    }
    if b.knockout {
        x0 = boxes.iter().map(|q| q[0]).min().unwrap_or(x0);
        y0 = boxes.iter().map(|q| q[1]).min().unwrap_or(y0);
        x1 = boxes.iter().map(|q| q[2]).max().unwrap_or(x1);
        y1 = boxes.iter().map(|q| q[3]).max().unwrap_or(y1);
        let min_side = w.min(h).get();
        let ten = (min_side + 9) / 10;
        let step = 100_000i64;
        let ten_rounded = Nm::new((ten + step - 1) / step * step);
        let (mx, my) = (b.margin_x_nm.max(ten_rounded), b.margin_y_nm.max(ten_rounded));
        boxes = vec![[x0 - mx, y0 - my, x1 + mx, y1 + my]];
    }
    let turn = b.angle as i64;
    for q in boxes {
        let pts: LineChain = rect_corners(Point { x: q[0], y: q[1] }, Point { x: q[2], y: q[3] })
            .iter()
            .map(|p| p64(if turn == 0 { *p } else { super::rotate_point_about(*p, b.at, -turn) }))
            .collect();
        out.add_outline(pts);
    }
}

/// `PCB_TABLE::TransformShapeToPolygon`: every cell as a text box (`PCB_TABLECELL` is a `PCB_TEXTBOX`, its border on and its line `DEFAULT_LINE_WIDTH`, 0.1 mm) grown by
/// the gap of the table's lines -- half the separators' width when it has separators, half the border's when it has a border, the larger.
fn table_shape(out: &mut ShapePolySet, t: &eda_model::table::Table, err: i64) {
    let mut gap = Nm::ZERO;
    if t.separators.cols || t.separators.rows {
        gap = gap.max(t.separators.stroke.width_nm / 2);
    }
    if t.border.external || t.border.header {
        gap = gap.max(t.border.stroke.width_nm / 2);
    }
    let width = eda_model::units::mm(0.1) + gap * 2;
    for row in 0..t.row_count() {
        for col in 0..t.col_count() {
            if t.cell(row, col).is_none_or(|c| c.is_covered()) {
                continue;
            }
            let corners = t.cell_corners(row, col);
            out.add_outline(corners.iter().map(|p| p64(*p)).collect());
            if width > Nm::ZERO {
                ovals_along(out, &corners, true, width, err);
            }
        }
    }
}

/// `PCB_TEXTBOX::TransformShapeToPolygon` with no clearance: the box counts as filled (its corners), and with a border the stadiums of its line along its sides.
fn text_box_shape(out: &mut ShapePolySet, tb: &eda_model::pcb_items::TextBox, err: i64) {
    let corners: Vec<Point> = if tb.is_rect() { rect_corners(tb.start, tb.end).to_vec() } else { tb.points() };
    if corners.len() < 3 {
        return;
    }
    out.add_outline(corners.iter().map(|p| p64(*p)).collect());
    if tb.border && tb.stroke_width_nm > Nm::ZERO {
        ovals_along(out, &corners, true, tb.stroke_width_nm, err);
    }
}

impl Item {
    /// `item->TransformShapeToPolygon( itemShape, F_Cu, 0, maxError, ERROR_OUTSIDE )`, kind by kind (see the module doc); empty for a kind with no polygon.
    pub(crate) fn shape(&self, max_error: Nm) -> ShapePolySet {
        let err = max_error.get().max(1);
        let mut out = ShapePolySet::new();
        match self {
            // `PCB_TRACK`: `TransformOvalToPolygon( m_Start, m_End, m_width )`; an arc's `TransformArcToPolygon` along its points
            Item::Track(t) => ovals_along(&mut out, &t.pts, false, t.width, err),
            // `PCB_VIA`: `TransformCircleToPolygon( m_Start, GetWidth( aLayer ) / 2 )`
            Item::Via(v) => {
                out.add_outline(circle_polygon(p64(v.at), (v.diameter / 2).get(), err, ErrorLoc::Outside, 0));
            }
            Item::Zone(z) => out = zone_shape(z),
            Item::Shape(s, style) => {
                let solid = s.is_filled() || style.as_ref().is_some_and(|st| st.hatch.is_some());
                shape_shape(&mut out, s, solid, err);
            }
            Item::Text(t, extra) => {
                if let Some(hull) = eda_drc::hatch::text_shape(t, extra.as_ref()) {
                    out.add_outline(hull);
                }
            }
            // `PCB_DIMENSION_BASE::TransformShapeToPolygon`: its lines (`m_shapes`), each `m_lineThickness` wide, and its circles; not its text
            Item::Dimension(d) => {
                let g = eda_connectivity::dimension::compute_dimension_geometry(d);
                for (a, b) in &g.lines {
                    out.add_outline(oval_polygon(p64(*a), p64(*b), d.stroke_width.get(), err, ErrorLoc::Outside, 0));
                }
                for (c, r) in &g.circles {
                    out.add_outline(circle_polygon(p64(*c), (*r + d.stroke_width / 2).get(), err, ErrorLoc::Outside, 0));
                }
            }
            Item::TextBox(tb) => text_box_shape(&mut out, tb, err),
            Item::Table(t) => table_shape(&mut out, t, err),
            Item::Barcode(b) if b.layer == "F.Cu" => barcode_shape(&mut out, b),
            Item::Barcode(_) | Item::Image(_) | Item::Point(_) => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::Track;
    use eda_model::units::mm;

    fn p(x: f64, y: f64) -> Point {
        Point { x: mm(x), y: mm(y) }
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)]
    }

    fn track(a: Point, b: Point, w: f64) -> Item {
        Item::Track(Track { id: "t".into(), net: String::new(), pins: vec![], layer: "F.Cu".into(), width: mm(w), pts: vec![a, b], arc_mid_offset: None })
    }

    #[test]
    fn an_item_over_a_hole_of_the_area_is_not_enclosed_and_one_beside_it_is() {
        let area = AreaPoly::new(&square(0.0, 0.0, 20.0, 20.0), &[square(8.0, 8.0, 12.0, 12.0)]);
        let err = mm(0.005);
        assert!(area.encloses(&track(p(2.0, 2.0), p(6.0, 2.0), 0.25).shape(err)), "clear of the hole");
        assert!(!area.encloses(&track(p(2.0, 10.0), p(18.0, 10.0), 0.25).shape(err)), "across the hole");
        assert!(!area.encloses(&track(p(9.0, 9.0), p(11.0, 11.0), 0.25).shape(err)), "inside the hole");
        // the end cap of a wide track reaching into the hole
        assert!(!area.encloses(&track(p(2.0, 10.0), p(7.8, 10.0), 0.5).shape(err)));
        // with no hole the same track is inside
        let whole = AreaPoly::new(&square(0.0, 0.0, 20.0, 20.0), &[]);
        assert!(whole.encloses(&track(p(2.0, 10.0), p(18.0, 10.0), 0.25).shape(err)));
    }

    #[test]
    fn a_zone_with_a_hole_round_the_areas_hole_is_enclosed() {
        // `itemShape = *zone->Outline()`: the zone's own hole takes away the part of it the area's hole would have left
        let area = AreaPoly::new(&square(0.0, 0.0, 20.0, 20.0), &[square(8.0, 8.0, 12.0, 12.0)]);
        let mut z = Zone { outline: square(1.0, 1.0, 19.0, 19.0), holes: vec![square(7.0, 7.0, 13.0, 13.0)], ..Default::default() };
        assert!(area.encloses(&Item::Zone(z.clone()).shape(mm(0.005))));
        z.holes.clear();
        assert!(!area.encloses(&Item::Zone(z).shape(mm(0.005))), "a zone without its hole covers the area's hole");
    }

    #[test]
    fn an_empty_polygon_is_never_enclosed() {
        let area = AreaPoly::new(&square(0.0, 0.0, 20.0, 20.0), &[]);
        let point = Item::Point(eda_model::pcb_items::PcbPoint::new("F.Cu", p(5.0, 5.0)));
        assert!(!area.encloses(&point.shape(mm(0.005))));
        let mut b = eda_model::barcode::Barcode {
            id: "bar".into(),
            at: p(5.0, 5.0),
            angle: 0,
            layer: "F.SilkS".into(),
            width_nm: mm(3.0),
            height_nm: mm(3.0),
            text: "AB".into(),
            text_height_nm: mm(1.0),
            kind: Default::default(),
            ecc: Default::default(),
            show_text: true,
            knockout: false,
            margin_x_nm: Nm::ZERO,
            margin_y_nm: Nm::ZERO,
        };
        assert!(!area.encloses(&Item::Barcode(b.clone()).shape(mm(0.005))), "a barcode off F.Cu has no polygon on the context's layer");
        b.layer = "F.Cu".into();
        assert!(area.encloses(&Item::Barcode(b).shape(mm(0.005))));
    }

    #[test]
    fn a_pattern_track_meets_the_outline_when_it_crosses_it_or_lies_inside() {
        let area = AreaPoly::new(&square(0.0, 0.0, 20.0, 20.0), &[square(8.0, 8.0, 12.0, 12.0)]);
        let err = mm(0.005);
        assert!(area.outline_meets(&track(p(-5.0, 5.0), p(5.0, 5.0), 0.2).shape(err)));
        assert!(area.outline_meets(&track(p(9.0, 9.0), p(11.0, 11.0), 0.2).shape(err)), "the outline alone, not its holes");
        assert!(!area.outline_meets(&track(p(-5.0, -5.0), p(-1.0, -5.0), 0.2).shape(err)));
    }
}
