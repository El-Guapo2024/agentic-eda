//! An SVG reader: the shapes of an SVG file as [`crate::graphics_import::ImportedShape`]s, in millimetres, Y down (an SVG's own direction).
//!
//! Ports `SVG_IMPORT_PLUGIN` (`common/import_gfx/svg_import_plugin.cpp` at KiCad 8303b2ad). The file itself is read by KiCad's own nanosvg
//! (`thirdparty/nanosvg` of kicad-src-25ec2335, vendored in `third_party/nanosvg` and built by `build.rs` with `cc`; see [`nanosvg`]), called as
//! the plugin calls it: `nsvgParse(text, "mm", 96)`. The Rust port of nanosvg this file used before is [`nanosvg_port`], INACTIVE since
//! 2026-10-28 (the `rust-nanosvg` cargo feature turns it back on). What the plugin does with the parsed shapes is here as it is there:
//!
//! * a shape with neither fill nor stroke, or that is hidden (`display:none`), is left out;
//! * a closed path is a polygon (its curves cut into segments by [`bezier_points`], `GatherInterpolatedCubicBezierPath`), which an importer makes a
//!   filled polygon or an outline; an open one is a run of cubic curves, each of which the importer keeps as a curve when it bends and as a line when
//!   it does not ([`crate::graphics_import::spline_or_line`]);
//! * a filled path that is not closed is both: a closed polygon for the fill and, if the shape has a stroke, the open outline;
//! * the paths of one shape are united under the shape's fill rule and fractured (`PostprocessNestedPolygons`, `BuildPolysetFromOrientedPaths`), which is
//!   how an SVG shape with holes becomes one polygon.
//!
//! KiCad's nanosvg draws `text` and `tspan` with Fontconfig, FreeType and HarfBuzz. This build has none of them (they are not on every machine the
//! workspace builds on), so text draws nothing, and the reader says so ("SVG text elements are not currently supported."), as it says for `image`
//! and `use`, where KiCad stays silent.

use crate::graphics_import::*;
use eda_clipper2::{ClipType, Clipper64, FillRule as ClipFill, Path64, Paths64, Point64, PolyTree64};
use eda_shape_poly_set::ShapePolySet;

#[cfg_attr(feature = "rust-nanosvg", allow(dead_code))]
mod nanosvg;
mod nanosvg_port;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Paint {
    None,
    Color(Rgba),
    Gradient,
}

/// `NSVGpath`: points of cubic Bezier curves (1 + 3n of them), and whether the path is closed.
pub(crate) struct SPath {
    pub(crate) pts: Vec<(f64, f64)>,
    pub(crate) closed: bool,
}

/// `NSVGshape`.
pub(crate) struct SShape {
    pub(crate) paths: Vec<SPath>,
    pub(crate) fill: Paint,
    pub(crate) stroke: Paint,
    pub(crate) stroke_width: f64,
    pub(crate) dash: Vec<f64>,
    pub(crate) fill_rule: FillRule,
    pub(crate) visible: bool,
}

// ---------------------------------------------------------------------------------------------------------------- the plugin's part

/// `getBezierPoint`: the point of a cubic curve a fraction `step` along it, by the plain interpolation KiCad's does.
fn bezier_point(c: &[V2; 4], step: f64) -> V2 {
    let lerp = |a: V2, b: V2| a + (b - a) * step;
    let (q0, q1, q2) = (lerp(c[0], c[1]), lerp(c[1], c[2]), lerp(c[2], c[3]));
    lerp(lerp(q0, q1), lerp(q1, q2))
}

/// `distanceFromPointToLine`.
fn distance_to_line(p: V2, a: V2, b: V2) -> f64 {
    let dir = b - a;
    if dir.x == 0.0 && dir.y == 0.0 {
        return (p - a).norm();
    }
    let len = dir.norm();
    let normal = V2::new(-dir.y / len, dir.x / len);
    let ap = p - a;
    (normal.x * ap.x + normal.y * ap.y).abs()
}

/// `segmentBezierCurve` and `createNewBezierCurveSegments`: split the curve while its middle is further than the threshold from the chord. (A depth limit
/// keeps a curve of no extent from splitting for ever.)
fn segment_bezier(start: V2, end: V2, offset: f64, step: f64, c: &[V2; 4], threshold: f64, out: &mut Vec<V2>, depth: u32) {
    let middle = bezier_point(c, offset + step);
    if depth < 24 && distance_to_line(middle, start, end) > threshold {
        let new_step = step / 2.0;
        segment_bezier(start, middle, offset, new_step, c, threshold, out, depth + 1);
        out.push(middle);
        segment_bezier(middle, end, offset + step, new_step, c, threshold, out, depth + 1);
    }
}

/// `GatherInterpolatedCubicBezierPath`: the points of a path of cubic curves. `calculateBezierSegmentationThreshold` takes a thousandth of the
/// largest side of the box of the curve's first three control points (not the fourth: KiCad's own measure).
pub fn bezier_points(pts: &[V2]) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::new();
    let mut i = 0;
    while i + 3 < pts.len() {
        let c = [pts[i], pts[i + 1], pts[i + 2], pts[i + 3]];
        let start = bezier_point(&c, 0.0);
        let end = bezier_point(&c, 1.0);
        let (min_x, max_x) = c[..3].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        let (min_y, max_y) = c[..3].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        let threshold = 0.001 * (max_x - min_x).max(max_y - min_y);
        if out.last() != Some(&start) {
            out.push(start);
        }
        segment_bezier(start, end, 0.0, 0.5, &c, threshold, &mut out, 0);
        out.push(end);
        i += 3;
    }
    out
}

/// The paint a shape's colour is stored with: black is "not read properly", so unspecified (`fillColor == COLOR4D::BLACK`).
fn stored_color(p: Paint) -> Option<Rgba> {
    match p {
        Paint::Color(c) if !(c.r == 0.0 && c.g == 0.0 && c.b == 0.0 && c.a == 1.0) => Some(c),
        _ => None,
    }
}

/// `SVG_IMPORT_PLUGIN::DrawPath`.
fn draw_path(buf: &mut Buffer, pts: &[(f64, f64)], closed: bool, stroke: Stroke, filled: bool, fill_color: Option<Rgba>) {
    let v: Vec<V2> = pts.iter().map(|&(x, y)| V2::new(x, y)).collect();
    let mut drew_polygon = false;
    if closed {
        // Closed paths are always polygons, which mean they need to be interpolated.
        let collected = if v.is_empty() { Vec::new() } else { bezier_points(&v) };
        if collected.len() > 2 {
            buf.add_polygon(collected, stroke, filled, fill_color);
            drew_polygon = true;
        }
    }
    if !drew_polygon {
        // `DrawSplinePath`: each cubic curve as a spline, which the importer makes a curve or a line.
        let mut i = 0;
        while i + 3 < v.len() {
            buf.add_spline(v[i], v[i + 1], v[i + 2], v[i + 3], stroke);
            i += 3;
        }
    }
}

/// `SVG_IMPORT_PLUGIN::Import`'s part before the importer: the shapes of the parsed picture in a buffer.
fn import_shapes(shapes: &[SShape]) -> Buffer {
    let mut buf = Buffer::default();
    for shape in shapes {
        if !shape.visible {
            continue;
        }
        if shape.stroke == Paint::None && shape.fill == Paint::None {
            continue;
        }
        let line_width = if shape.stroke != Paint::None { shape.stroke_width } else { -1.0 };
        let filled = match shape.fill {
            Paint::None => false,
            Paint::Color(c) => c.a > 0.0,
            // The alpha of a gradient is not a colour's: KiCad reads whatever the pointer holds. A gradient shape is filled.
            Paint::Gradient => true,
        };
        let fill_color = if let Paint::Color(_) = shape.fill { stored_color(shape.fill) } else { None };
        let stroke_color = stored_color(shape.stroke);
        let mut dash = LineStyle::Solid;
        if !shape.dash.is_empty() {
            let (mut dots, mut dashes) = (0, 0);
            let threshold = shape.stroke_width * 1.9;
            for d in shape.dash.iter().step_by(2) {
                if *d < threshold {
                    dots += 1;
                } else {
                    dashes += 1;
                }
            }
            dash = match (dots, dashes) {
                (d, 0) if d > 0 => LineStyle::Dot,
                (0, n) if n > 0 => LineStyle::Dash,
                (1, 1) => LineStyle::DashDot,
                (2, 1) => LineStyle::DashDotDot,
                _ => LineStyle::Solid,
            };
        }
        let stroke = Stroke { width: line_width, style: dash, color: stroke_color };
        buf.new_shape(shape.fill_rule);
        for path in &shape.paths {
            if filled && !path.closed {
                // KiCad doesn't support a single object representing a filled shape that is *not* closed so create a filled, closed shape for the
                // fill, and an unfilled, open shape for the outline.
                let no_stroke = Stroke { width: -1.0, style: LineStyle::Solid, color: None };
                draw_path(&mut buf, &path.pts, true, no_stroke, true, fill_color);
                if stroke.width > 0.0 {
                    draw_path(&mut buf, &path.pts, false, stroke, false, None);
                }
            } else {
                // Either the shape has fill and no stroke, so we implicitly close it (for no difference), or it's really closed.
                let closed = path.closed || filled;
                draw_path(&mut buf, &path.pts, closed, stroke, filled, fill_color);
            }
        }
    }
    postprocess_nested_polygons(&mut buf);
    buf
}

/// `convertPolygon`: the paths of one SVG shape (closed polygons that may lie inside one another) as the outlines of one polygon set under the
/// shape's fill rule, fractured so that each outline is one ring (a hole is joined to its outline by a slit).
fn convert_polygon(out: &mut Vec<ImportedShape>, paths: &[(Vec<V2>, Stroke, bool, Option<Rgba>)], rule: FillRule) {
    let Some(first) = paths.first() else { return };
    let (stroke, filled, fill_color) = {
        let last = paths.last().unwrap_or(first);
        (last.1, last.2, last.3)
    };
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (pts, ..) in paths {
        for v in pts {
            min_x = min_x.min(v.x);
            min_y = min_y.min(v.y);
            max_x = max_x.max(v.x);
            max_y = max_y.max(v.y);
        }
    }
    let (orig_w, orig_h) = (max_x - min_x, max_y - min_y);
    // `wxCHECK( origH && origW, )`: a picture of no width or height makes nothing.
    if orig_w == 0.0 || orig_h == 0.0 || !orig_w.is_finite() || !orig_h.is_finite() {
        return;
    }
    // "as Clipper/SHAPE_POLY_SET uses ints we first need to upscale to a reasonably large size (in integer coordinates) to avoid losing accuracy."
    const CONVERT_SCALE: f64 = 1_000_000_000.0;
    let (up_w, up_h) = if orig_w > orig_h { (CONVERT_SCALE, orig_h * CONVERT_SCALE / orig_w) } else { (orig_w * CONVERT_SCALE / orig_h, CONVERT_SCALE) };
    let mut open_paths: Vec<&Vec<V2>> = Vec::new();
    let mut subject: Paths64 = Vec::new();
    for (pts, ..) in paths {
        if pts.len() < 3 {
            open_paths.push(pts);
            continue;
        }
        let mut lc: Path64 = Vec::with_capacity(pts.len());
        for v in pts {
            let p = Point64::new(((v.x - min_x) * (up_w / orig_w)).round() as i64, ((v.y - min_y) * (up_h / orig_h)).round() as i64);
            // `SHAPE_LINE_CHAIN::Append` leaves out a point equal to the last one.
            if lc.last() != Some(&p) {
                lc.push(p);
            }
        }
        subject.push(lc);
    }
    let mut c = Clipper64::new();
    c.add_subject(&subject);
    let mut tree = PolyTree64::new();
    let mut open = Vec::new();
    c.execute_tree(ClipType::Union, if rule == FillRule::EvenOdd { ClipFill::EvenOdd } else { ClipFill::NonZero }, &mut tree, &mut open);
    let mut result = ShapePolySet::from_poly_tree(&tree);
    result.fracture(true);
    for outl in 0..result.outline_count() {
        let ring = result.outline(outl);
        let pts: Vec<V2> = ring.iter().map(|p| V2::new(p.x as f64 * (orig_w / up_w) + min_x, p.y as f64 * (orig_h / up_h) + min_y)).collect();
        out.push(ImportedShape::Polygon { pts, stroke, filled, fill_color, parent: None });
    }
    for p in open_paths {
        out.push(ImportedShape::Polygon { pts: p.clone(), stroke, filled, fill_color, parent: None });
    }
}

/// `GRAPHICS_IMPORTER_BUFFER::PostprocessNestedPolygons`.
fn postprocess_nested_polygons(buf: &mut Buffer) {
    let mut new_shapes: Vec<ImportedShape> = Vec::new();
    let mut cur: Option<usize> = None;
    let mut polypaths: Vec<(Vec<V2>, Stroke, bool, Option<Rgba>)> = Vec::new();
    let rules = buf.fill_rules.clone();
    let shapes = std::mem::take(&mut buf.shapes);
    for shape in shapes {
        match shape {
            ImportedShape::Polygon { pts, stroke, filled, fill_color, parent: Some(index) } => {
                if Some(index) != cur {
                    if let Some(c) = cur {
                        convert_polygon(&mut new_shapes, &polypaths, rules[c]);
                        polypaths.clear();
                    }
                }
                cur = Some(index);
                polypaths.push((pts, stroke, filled, fill_color));
            }
            other => new_shapes.push(other),
        }
    }
    if let Some(c) = cur {
        convert_polygon(&mut new_shapes, &polypaths, rules[c]);
    }
    buf.shapes = new_shapes;
}

// ---------------------------------------------------------------------------------------------------------------------------- entry

const NOT_SVG: &str = "This is not an SVG file: it has no <svg> element.";

/// What the SVG parser hands the plugin: the `NSVGimage`'s shapes, and the messages for what it left out.
pub(crate) struct Parsed {
    pub(crate) shapes: Vec<SShape>,
    pub(crate) messages: Vec<String>,
}

/// Reads an SVG file. `Err` is a file that is not SVG at all; everything the plugin cannot handle is a message in the result.
pub fn read_svg(text: &str) -> Result<Imported, String> {
    #[cfg(not(feature = "rust-nanosvg"))]
    return nanosvg::parse(text).map(import);
    #[cfg(feature = "rust-nanosvg")]
    return read_svg_port(text);
}

/// [`read_svg`] through the INACTIVE Rust port of nanosvg ([`nanosvg_port`]): what `read_svg` is with the `rust-nanosvg` feature, and what
/// `tests/svg_import.rs` compares KiCad's nanosvg with.
pub fn read_svg_port(text: &str) -> Result<Imported, String> {
    nanosvg_port::parse(text).map(import)
}

/// `SVG_IMPORT_PLUGIN::Import` on the parsed shapes.
fn import(parsed: Parsed) -> Imported {
    let buf = import_shapes(&parsed.shapes);
    let mut bbox: Option<BBox> = None;
    for s in &buf.shapes {
        if let Some(b) = s.bbox() {
            BBox::merge(&mut bbox, b);
        }
    }
    Imported { shapes: buf.shapes, messages: parsed.messages, bbox }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(body: &str) -> Imported {
        // 100 x 100 mm with a view box of 100 units: a user unit is a millimetre.
        read_svg(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"#)).unwrap()
    }

    /// nanosvg works in `float`: a coordinate of tens of millimetres is good to a few millionths of a millimetre.
    fn near(a: V2, x: f64, y: f64) -> bool {
        (a.x - x).abs() < 1e-5 && (a.y - y).abs() < 1e-5
    }

    #[test]
    fn a_stroked_line_is_a_line_with_the_strokes_width() {
        let i = read(r##"<line x1="10" y1="20" x2="50" y2="20" fill="none" stroke="#000" stroke-width="0.5"/>"##);
        // A line is a cubic curve with its control points at the thirds, which the importer finds flat.
        let ImportedShape::Spline { start, c1, c2, end, stroke } = i.shapes[0].clone() else { panic!("{:?}", i.shapes) };
        assert!(near(start, 10.0, 20.0) && near(end, 50.0, 20.0));
        assert!(near(c1, 10.0 + 40.0 / 3.0, 20.0) && near(c2, 50.0 - 40.0 / 3.0, 20.0));
        assert!((stroke.width - 0.5).abs() < 1e-9, "{stroke:?}");
        assert_eq!(i.shapes.len(), 1);
    }

    #[test]
    fn a_line_with_the_default_fill_gets_an_unstroked_copy_for_its_fill_as_in_kicad() {
        // An SVG shape is filled black unless it says otherwise, and nanosvg fills a line too: a filled open path is "a filled, closed shape for the
        // fill, and an unfilled, open shape for the outline", the first with no stroke.
        let i = read(r#"<line x1="0" y1="0" x2="10" y2="0" stroke="blue" stroke-width="1"/>"#);
        let widths: Vec<f64> = i.shapes.iter().map(|s| if let ImportedShape::Spline { stroke, .. } = s { stroke.width } else { f64::NAN }).collect();
        assert_eq!(widths, vec![-1.0, 1.0], "{:?}", i.shapes);
    }

    #[test]
    fn a_shape_with_no_fill_and_no_stroke_and_a_hidden_one_are_left_out() {
        let i = read(r#"<path d="M0 0 L10 0 L10 10" fill="none"/><rect width="5" height="5" fill="none" stroke="none"/><rect width="5" height="5" display="none"/><g display="none"><rect width="5" height="5"/></g>"#);
        assert!(i.shapes.is_empty(), "{:?}", i.shapes);
    }

    #[test]
    fn a_rect_is_a_filled_polygon_and_with_rounded_corners_a_ring_of_many_points() {
        let i = read(r##"<rect x="10" y="20" width="30" height="10" fill="#f00"/>"##);
        let ImportedShape::Polygon { pts, filled, fill_color, stroke, .. } = i.shapes[0].clone() else { panic!("{:?}", i.shapes) };
        assert!(filled && stroke.width == -1.0, "no stroke: {stroke:?}");
        assert_eq!(fill_color.map(|c| (c.r, c.g, c.b)), Some((1.0, 0.0, 0.0)));
        let b = i.bbox.unwrap();
        assert!(near(b.min, 10.0, 20.0) && near(b.max, 40.0, 30.0), "{b:?}");
        assert!(pts.len() >= 4);
        let round = read(r#"<rect x="0" y="0" width="30" height="20" rx="4" fill="black"/>"#);
        let ImportedShape::Polygon { pts, .. } = round.shapes[0].clone() else { panic!() };
        assert!(pts.len() > 12, "the corners are curves: {}", pts.len());
        // The rounded rect still spans its box.
        let b = round.bbox.unwrap();
        assert!(near(b.min, 0.0, 0.0) && near(b.max, 30.0, 20.0), "{b:?}");
    }

    #[test]
    fn a_circle_and_an_ellipse_are_closed_polygons_on_their_own_curve() {
        let i = read(r#"<circle cx="50" cy="50" r="10" fill="none" stroke="black"/><ellipse cx="20" cy="20" rx="8" ry="4" fill="none" stroke="black"/>"#);
        assert_eq!(i.shapes.len(), 2);
        let ImportedShape::Polygon { pts, filled, .. } = i.shapes[0].clone() else { panic!() };
        assert!(!filled);
        for p in &pts {
            assert!((((p.x - 50.0).powi(2) + (p.y - 50.0).powi(2)).sqrt() - 10.0).abs() < 0.02, "{p:?}");
        }
        let ImportedShape::Polygon { pts, .. } = i.shapes[1].clone() else { panic!() };
        let b = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        assert!((b.0 - 12.0).abs() < 0.05 && (b.1 - 28.0).abs() < 0.05, "{b:?}");
    }

    #[test]
    fn a_polygon_is_closed_and_a_polyline_is_a_run_of_lines() {
        let i = read(r#"<polygon points="0,0 10,0 10,10" fill="none" stroke="black"/><polyline points="20,0 30,0 30,10" fill="none" stroke="black"/>"#);
        let polys: Vec<&ImportedShape> = i.shapes.iter().filter(|s| matches!(s, ImportedShape::Polygon { .. })).collect();
        assert_eq!(polys.len(), 1, "{:?}", i.shapes);
        let ImportedShape::Polygon { pts, .. } = polys[0] else { unreachable!() };
        assert!(pts.len() >= 3);
        let splines = i.shapes.iter().filter(|s| matches!(s, ImportedShape::Spline { .. })).count();
        assert_eq!(splines, 2, "the two segments of the polyline");
    }

    #[test]
    fn path_commands_relative_and_absolute_reach_the_same_points() {
        let a = read(r#"<path d="M10 10 L30 10 L30 30 L10 30 Z" fill="none" stroke="black"/>"#);
        let b = read(r#"<path d="m10 10 l20 0 l0 20 h-20 z" fill="none" stroke="black"/>"#);
        let c = read(r#"<path d="M10,10 H30 V30 H10 Z" fill="none" stroke="black"/>"#);
        let box_of = |i: &Imported| i.bbox.unwrap();
        for i in [&a, &b, &c] {
            assert!(near(box_of(i).min, 10.0, 10.0) && near(box_of(i).max, 30.0, 30.0), "{:?}", box_of(i));
        }
    }

    #[test]
    fn quadratic_and_cubic_curves_and_their_smooth_forms_end_where_they_say() {
        let i = read(r#"<path d="M0 0 C10 0 20 10 30 10 S50 0 60 0 Q70 10 80 0 T100 0" fill="none" stroke="black"/>"#);
        let ends: Vec<V2> = i.shapes.iter().filter_map(|s| if let ImportedShape::Spline { end, .. } = s { Some(*end) } else { None }).collect();
        assert_eq!(ends.len(), 4, "{:?}", i.shapes);
        assert!(near(ends[0], 30.0, 10.0) && near(ends[1], 60.0, 0.0) && near(ends[2], 80.0, 0.0) && near(ends[3], 100.0, 0.0), "{ends:?}");
    }

    #[test]
    fn an_arc_is_curves_of_at_most_a_quarter_turn_through_the_right_points() {
        // A half circle of radius 10 from (0, 20) to (20, 20) bulging upwards: nanosvg cuts it in three (a half turn over a quarter, and one more).
        let i = read(r#"<path d="M0 20 A10 10 0 0 1 20 20" fill="none" stroke="black"/>"#);
        let curves: Vec<_> = i.shapes.iter().filter_map(|s| if let ImportedShape::Spline { start, end, .. } = s { Some((*start, *end)) } else { None }).collect();
        assert_eq!(curves.len(), 3, "{:?}", i.shapes);
        assert!(near(curves[0].0, 0.0, 20.0) && near(curves[2].1, 20.0, 20.0));
        for (_, end) in &curves {
            let d = ((end.x - 10.0).powi(2) + (end.y - 20.0).powi(2)).sqrt();
            assert!((d - 10.0).abs() < 1e-6, "every end is on the circle: {end:?}");
        }
        assert!(curves[1].0.y < 20.0 && curves[1].1.y < 20.0, "bulging upwards (y down): {curves:?}");
        // The two flags may run together: "a10 10 0 11 20 0".
        let j = read(r#"<path d="M0 20 a10 10 0 1020 0" fill="none" stroke="black"/>"#);
        assert!(!j.shapes.is_empty());
    }

    #[test]
    fn the_transform_of_a_group_and_of_a_shape_both_apply_the_shapes_first() {
        let i = read(r#"<g transform="translate(10 0)"><rect x="0" y="0" width="5" height="5" transform="scale(2)" fill="black"/></g>"#);
        let b = i.bbox.unwrap();
        // scale(2) first (0..10), then the group's translate (10..20).
        assert!(near(b.min, 10.0, 0.0) && near(b.max, 20.0, 10.0), "{b:?}");
        let r = read(r#"<rect x="10" y="0" width="10" height="5" transform="rotate(90)" fill="black"/>"#);
        let b = r.bbox.unwrap();
        // rotate(90): (x, y) -> (-y, x): x from -5 to 0, y from 10 to 20.
        assert!(near(b.min, -5.0, 10.0) && near(b.max, 0.0, 20.0), "{b:?}");
    }

    #[test]
    fn style_attributes_and_the_style_property_say_the_same() {
        let a = read(r#"<rect width="5" height="5" style="fill:none;stroke:#00ff00;stroke-width:2"/>"#);
        let b = read(r##"<rect width="5" height="5" fill="none" stroke="#0f0" stroke-width="2"/>"##);
        let c = read(r#"<g fill="none" stroke="rgb(0, 255, 0)" stroke-width="2"><rect width="5" height="5"/></g>"#);
        for i in [&a, &b, &c] {
            let ImportedShape::Polygon { stroke, filled, .. } = i.shapes[0].clone() else { panic!("{:?}", i.shapes) };
            assert!(!filled && (stroke.width - 2.0).abs() < 1e-9, "{stroke:?}");
            assert_eq!(stroke.color.map(|c| (c.r, c.g, c.b)), Some((0.0, 1.0, 0.0)));
        }
        // KiCad builds nanosvg without NANOSVG_ALL_COLOR_KEYWORDS: only the basic keywords (red, green, blue, yellow, cyan, magenta, black,
        // grey, gray, white) are colours, and any other name, "lime" among them, is grey (128, 128, 128). (The INACTIVE port knows all 147.)
        #[cfg(not(feature = "rust-nanosvg"))]
        let d = read(r#"<rect width="5" height="5" fill="none" stroke="lime" stroke-width="2"/>"#);
        #[cfg(not(feature = "rust-nanosvg"))]
        {
            let ImportedShape::Polygon { stroke, .. } = d.shapes[0].clone() else { panic!("{:?}", d.shapes) };
            assert_eq!(stroke.color.map(|c| (c.r, c.g, c.b)), Some((128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0)));
        }
    }

    /// KiCad's nanosvg reads `<style>` sheets of class selectors (a KiCad change to nanosvg); the INACTIVE port does not, and fills the
    /// shape black.
    #[cfg(not(feature = "rust-nanosvg"))]
    #[test]
    fn a_style_sheet_class_paints_the_shape_as_in_kicad() {
        let i = read(r#"<style>.a { fill: none; stroke: #ff0000; stroke-width: 2 }</style><rect class="a" width="5" height="5"/>"#);
        let ImportedShape::Polygon { stroke, filled, .. } = i.shapes[0].clone() else { panic!("{:?}", i.shapes) };
        assert!(!filled && (stroke.width - 2.0).abs() < 1e-6, "{stroke:?}");
        assert_eq!(stroke.color.map(|c| (c.r, c.g, c.b)), Some((1.0, 0.0, 0.0)));
    }

    #[test]
    fn the_scale_of_the_picture_is_the_view_box_and_the_units_of_the_size() {
        // 10 mm wide, view box 20 units wide: a unit is half a millimetre.
        let i = read_svg(r#"<svg width="10mm" height="10mm" viewBox="0 0 20 20"><rect width="20" height="20" fill="black"/></svg>"#).unwrap();
        assert!(near(i.bbox.unwrap().max, 10.0, 10.0), "{:?}", i.bbox);
        // No size and no view box: a user unit is a pixel at 96 dpi, and the picture is moved to the origin.
        let j = read_svg(r#"<svg><rect x="96" y="96" width="96" height="96" fill="black"/></svg>"#).unwrap();
        let b = j.bbox.unwrap();
        assert!(near(b.min, 0.0, 0.0) && near(b.max, 25.4, 25.4), "{b:?}");
        // Inches.
        let k = read_svg(r#"<svg width="1in" height="1in"><rect width="96" height="96" fill="black"/></svg>"#).unwrap();
        assert!(near(k.bbox.unwrap().max, 25.4, 25.4), "{:?}", k.bbox);
    }

    #[test]
    fn a_filled_open_path_is_a_polygon_for_the_fill_and_a_curve_run_for_the_stroke() {
        let i = read(r##"<path d="M0 0 L10 0 L10 10" fill="#888" stroke="black" stroke-width="1"/>"##);
        let polys = i.shapes.iter().filter(|s| matches!(s, ImportedShape::Polygon { .. })).count();
        let curves = i.shapes.iter().filter(|s| matches!(s, ImportedShape::Spline { .. })).count();
        assert_eq!((polys, curves), (1, 2), "{:?}", i.shapes);
    }

    #[test]
    fn a_shape_with_a_hole_is_one_polygon_that_does_not_cover_the_hole() {
        // A 20 mm square with a 10 mm square cut out (the inner path runs the other way: a hole under either fill rule).
        let i = read(r##"<path d="M0 0 L20 0 L20 20 L0 20 Z M5 5 L5 15 L15 15 L15 5 Z" fill="#333" fill-rule="evenodd"/>"##);
        let polys: Vec<&Vec<V2>> = i.shapes.iter().filter_map(|s| if let ImportedShape::Polygon { pts, .. } = s { Some(pts) } else { None }).collect();
        assert_eq!(polys.len(), 1, "one fractured ring: {:?}", i.shapes);
        let ring = polys[0];
        // Fractured: the ring visits the outline and the hole's corners.
        assert!(ring.iter().any(|p| near(*p, 5.0, 5.0)) && ring.iter().any(|p| near(*p, 15.0, 15.0)) && ring.iter().any(|p| near(*p, 20.0, 20.0)), "{ring:?}");
        // The shoelace area of the ring is the 20 x 20 square less the 10 x 10 hole.
        let mut a = 0.0;
        for k in 0..ring.len() {
            let (p, q) = (ring[k], ring[(k + 1) % ring.len()]);
            a += p.x * q.y - q.x * p.y;
        }
        assert!((a.abs() / 2.0 - 300.0).abs() < 1e-3, "area {}", a.abs() / 2.0);
    }

    #[test]
    fn dashes_are_told_from_dots() {
        let line = |dash: &str| read(&format!(r#"<path d="M0 0 L10 0" fill="none" stroke="black" stroke-width="1" stroke-dasharray="{dash}"/>"#));
        let style = |i: &Imported| match &i.shapes[0] {
            ImportedShape::Spline { stroke, .. } => stroke.style,
            other => panic!("{other:?}"),
        };
        assert_eq!(style(&line("3 2")), LineStyle::Dash);
        assert_eq!(style(&line("1 2")), LineStyle::Dot);
        assert_eq!(style(&line("none")), LineStyle::Solid);
        assert_eq!(style(&line("4 1 1 1")), LineStyle::DashDot);
    }

    #[test]
    fn text_and_images_are_reported_not_silently_lost() {
        let i = read(r#"<text x="1" y="2">hi</text><image href="a.png"/>"#);
        assert!(i.shapes.is_empty());
        assert_eq!(i.messages.len(), 2, "{:?}", i.messages);
    }

    #[test]
    fn comments_doctype_cdata_and_a_gt_inside_an_attribute_do_not_confuse_the_reader() {
        let src = r#"<?xml version="1.0"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "x.dtd" [ <!ENTITY a "b"> ]>
<!-- a comment with <rect width="1" height="1"/> in it -->
<svg width="10mm" height="10mm" viewBox="0 0 10 10"><![CDATA[ <rect/> ]]><desc data-x="a>b"/>
<rect width="4" height="4" fill="black"/></svg>"#;
        let i = read_svg(src).unwrap();
        assert_eq!(i.shapes.len(), 1, "{:?}", i.shapes);
        assert!(read_svg("<html></html>").is_err());
    }
}
