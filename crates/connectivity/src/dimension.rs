//! Task item 7: `pcbnew/pcb_dimension.cpp`'s `updateGeometry()`/`updateText()`
//! family, ported as one pure function per concern rather than five
//! `PCB_DIM_*` item classes -- see `eda_model::ir::Dimension`'s own doc.
//!
//! Rotation uses this crate's own clockwise-positive-in-Y-down convention
//! (the same one `eda_ops::rotate_point_about`/`MoveExactDialog.tsx`
//! document), verified here by tests with concrete coordinates, rather
//! than a byte-for-byte port of source's own `RotatePoint` sign
//! convention -- the two can differ in which literal angle produces
//! "clockwise" internally while still agreeing on what the feature
//! itself does: an arrow points the labelled direction, text sits the
//! labelled side of the crossbar.
//!
//! The crossbar's and the leader lines' "knockout" around the text is KiCad's (`CollectKnockedOutSegments`, `segPolyIntersection`,
//! `segCircleIntersection`): the text's box (`EDA_TEXT::GetTextBox` in the stroke font's metrics, [`eda_model::kicad_font::text_box_iu`], grown as each
//! kind's `updateGeometry` grows it and turned by the text angle) cuts every line it crosses, wherever the text is -- inline, outside or placed by hand
//! (`DIM_TEXT_POSITION::MANUAL`, kept in [`Dimension::text_pos`]). A leader's `DIM_TEXT_BORDER` draws the box (`RECTANGLE`) or the circle through it
//! (`CIRCLE`) around its text, as `PCB_DIM_LEADER::updateGeometry` does (UI-LEFT P13). A label in an outline face is measured in that font
//! ([`eda_model::kicad_font::text_box_face_iu`], UI-LEFT-3 C3-3).

use eda_model::ir::{ArrowDirection, Dimension, DimensionKind, DimensionTextBorder, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, Millideg, Point, Nm};

const ARROW_HALF_ANGLE_MILLIDEG: i64 = 27_500;
/// `INWARD_ARROW_LENGTH_TO_HEAD_RATIO` (`pcb_dimension.cpp`): how far an
/// inward-style arrow's visible tail extends outward past the crossbar's
/// own measured endpoint, as a multiple of `arrow_length`.
const INWARD_TAIL_RATIO: i64 = 2;

/// Everything a renderer needs to draw one `Dimension` and nothing it
/// would have to compute itself: every straight line segment (extension
/// lines, crossbar/leader pieces already split around inline text, arrow
/// barbs and tails, a `Center`/`Radial` cross), the text anchor and
/// angle, the raw measured distance, and the fully formatted display
/// string (prefix/value/suffix, units resolved and formatted per
/// `dim`'s own settings).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionGeometry {
    pub lines: Vec<(Point, Point)>,
    /// The circles of the dimension (`SHAPE_CIRCLE`): a leader's `DIM_TEXT_BORDER::CIRCLE` frame, `(centre, radius)`.
    pub circles: Vec<(Point, Nm)>,
    pub text_at: Point,
    pub text_angle: Millideg,
    pub measured_value_nm: Nm,
    pub text: String,
    /// An aligned or orthogonal dimension's crossbar (`GetCrossbarStart`, `GetCrossbarEnd`): two of its point editor's handles.
    pub crossbar: Option<(Point, Point)>,
    /// A radial dimension's knee (`PCB_DIM_RADIAL::GetKnee`): the end pushed out along the radius by the leader length, its point editor's fourth handle.
    pub knee: Option<Point>,
}

pub fn compute_dimension_geometry(dim: &Dimension) -> DimensionGeometry {
    match dim.kind {
        DimensionKind::Aligned { height } => aligned(dim, height),
        DimensionKind::Orthogonal { height, horizontal } => orthogonal(dim, height, horizontal),
        DimensionKind::Radial { leader_length } => radial(dim, leader_length),
        DimensionKind::Leader => leader(dim),
        DimensionKind::Center => center(dim),
    }
}

// ----------------------------------------------------------- geometry helpers

fn sub(a: Point, b: Point) -> Point {
    Point { x: a.x - b.x, y: a.y - b.y }
}
fn add(a: Point, b: Point) -> Point {
    Point { x: a.x + b.x, y: a.y + b.y }
}
fn norm(p: Point) -> f64 {
    p.x.as_f64().hypot(p.y.as_f64())
}
/// `v` rescaled to length `len` (sign of `len` flips the direction),
/// preserving direction otherwise. The zero vector has no direction, so
/// it stays zero -- same degenerate case `VECTOR2I::Resize` has.
fn resize(v: Point, len: Nm) -> Point {
    let n = norm(v);
    if n < 1.0 {
        return Point { x: Nm::ZERO, y: Nm::ZERO };
    }
    let scale = len.as_f64() / n;
    Point { x: Nm::kiround(v.x.as_f64() * scale), y: Nm::kiround(v.y.as_f64() * scale) }
}
/// The perpendicular of `v`, rotated 90 degrees clockwise (this crate's
/// own Y-down convention -- see this module's header).
fn perp(v: Point) -> Point {
    Point { x: -v.y, y: v.x }
}
/// Angle of `v` from the +X axis, clockwise-positive in Y-down screen
/// coordinates (`atan2`'s own natural direction once Y grows downward), to
/// the millidegree (KiCad's `EDA_ANGLE` is a double; a whole degree wrote a
/// kept-aligned label at 34 where KiCad has 33.69).
fn angle_of(v: Point) -> i64 {
    (v.y.as_f64().atan2(v.x.as_f64()).to_degrees() * 1000.0).round() as i64
}
fn rotate(p: Point, angle_millideg: i64) -> Point {
    let rad = (angle_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    Point { x: Nm::kiround(p.x.as_f64() * cos - p.y.as_f64() * sin), y: Nm::kiround(p.x.as_f64() * sin + p.y.as_f64() * cos) }
}
fn rotate_about(p: Point, pivot: Point, angle_millideg: i64) -> Point {
    add(rotate(sub(p, pivot), angle_millideg), pivot)
}

/// An arrowhead whose point is `tip`: two barbs of `length`, each
/// splayed [`ARROW_HALF_ANGLE_MILLIDEG`] off the direction `toward_angle`
/// reversed, so the "V" opens toward `toward_angle` -- a classic
/// arrowhead pointing that way. An optional `tail` of the given length
/// extends `tip` further in the *opposite* direction (source's inward-
/// style visible stub, `drawAnArrow`'s own `aLength` tail argument).
fn arrowhead(tip: Point, toward_angle_millideg: i64, length: Nm, tail: Nm) -> Vec<(Point, Point)> {
    let mut lines = Vec::with_capacity(3);
    if tail != Nm::ZERO {
        let t = rotate(Point { x: tail, y: Nm::ZERO }, toward_angle_millideg + 180_000);
        lines.push((tip, add(tip, t)));
    }
    let back = toward_angle_millideg + 180_000;
    let b1 = rotate(Point { x: length, y: Nm::ZERO }, back + ARROW_HALF_ANGLE_MILLIDEG);
    let b2 = rotate(Point { x: length, y: Nm::ZERO }, back - ARROW_HALF_ANGLE_MILLIDEG);
    lines.push((tip, add(tip, b1)));
    lines.push((tip, add(tip, b2)));
    lines
}

/// Resolve `Automatic` to millimetres -- this model's own board-wide
/// convention elsewhere (`crates/model/src/ir.rs` `pub type Nm`'s own
/// doc) -- since unlike source, nothing server-side tracks which display
/// unit the UI currently shows (`state.units` is frontend-only, see
/// `web/studio/src/state/units.ts`'s own header). Source's own
/// `AUTOMATIC` resolves once, at the moment it's set (`SetUnitsMode`),
/// to whatever the frame's units were right then -- never a live follow
/// either, just resolved against a different fallback than this port's.
fn resolve_units(units: DimensionUnits) -> DimensionUnits {
    match units {
        DimensionUnits::Automatic => DimensionUnits::Mm,
        other => other,
    }
}

/// The decimals of a `DIM_PRECISION` level in `units` (already resolved): `PCB_DIMENSION_BASE::GetValueText`'s
/// `if( precision >= 6 ) switch( m_units ) { INCH: precision - 4; MILS: std::max( 0, precision - 7 ); MM: precision - 5 }`.
pub fn decimals(precision: u8, units: DimensionUnits) -> usize {
    let p = precision.min(9) as i64;
    if p < 6 {
        return p as usize;
    }
    (match units {
        DimensionUnits::Mil => (p - 7).max(0),
        DimensionUnits::Mm => p - 5,
        DimensionUnits::Inch | DimensionUnits::Automatic => p - 4,
    }) as usize
}

/// `PCB_DIMENSION_BASE::GetValueText` plus the prefix/suffix/units-suffix
/// wrapping `updateText()` adds around it.
fn format_value(dim: &Dimension, measured_value_nm: Nm) -> String {
    if let Some(text) = &dim.override_text {
        let mut s = dim.prefix.clone();
        s.push_str(text);
        s.push_str(&dim.suffix);
        return s;
    }

    let units = resolve_units(dim.units);
    let (value, unit_suffix) = match units {
        DimensionUnits::Mm => (eda_model::units::to_mm(measured_value_nm), "mm"),
        DimensionUnits::Mil => (eda_model::units::to_mils(measured_value_nm), "mil"),
        DimensionUnits::Inch => (measured_value_nm.as_f64() / eda_model::units::NM_PER_INCH as f64, "in"),
        DimensionUnits::Automatic => unreachable!("resolved above"),
    };

    let precision = decimals(dim.precision, units);
    let mut number = format!("{value:.precision$}");

    if dim.suppress_trailing_zeros && number.contains('.') {
        while number.ends_with('0') {
            number.pop();
        }
        if number.ends_with('.') {
            number.pop();
        }
    }

    let mut text = String::new();
    text.push_str(&dim.prefix);
    text.push_str(&number);
    match dim.units_format {
        DimensionUnitsFormat::NoSuffix => {}
        DimensionUnitsFormat::BareSuffix => {
            text.push(' ');
            text.push_str(unit_suffix);
        }
        DimensionUnitsFormat::ParenSuffix => {
            text.push_str(" (");
            text.push_str(unit_suffix);
            text.push(')');
        }
    }
    text.push_str(&dim.suffix);
    text
}

/// The pen of the dimension's text (`GetEffectiveTextPenWidth`): its own thickness, or 15 % of the size, which the writer writes for one that has none,
/// clamped to a quarter of the smaller of its width and height (`ClampTextPenSize( penWidth, GetTextSize() )`, not strict).
fn text_pen(dim: &Dimension) -> Nm {
    let pen = dim.text_thickness_nm.unwrap_or_else(|| Nm::kiround(dim.text_size_nm.as_f64() * 0.15));
    let size = dim.text_size_nm.abs().min(dim.text_width_nm().abs());
    // `std::min( aPenSize, KiROUND( (double) aSize * 0.25 ) )`
    pen.min(Nm::kiround(size.as_f64() * 0.25))
}

/// The four corners of the text's box as the dimension's `updateGeometry` collides its lines with it: `GetTextBox( nullptr ).Inflate( grow_x, grow_y )`
/// (the box of the text at `text_at`, `EDA_TEXT::GetTextBox`, in its face's metrics: its width and height, horizontal justification, mirror and
/// italic lean, UI-LEFT-2 PP11), as `polyBox` is built (origin, (origin.x, end.y), end, (end.x, origin.y)) and turned about its centre by the text angle
/// (`polyBox.Rotate( GetTextAngle(), textBox.GetCenter() )`, counter-clockwise on the screen). Also the box's centre and width, which a leader's circle
/// frame is made from.
fn text_poly(dim: &Dimension, text: &str, text_at: Point, angle: Millideg, grow_x: Nm, grow_y: Nm) -> ([Point; 4], Point, Nm) {
    use eda_model::kicad_font::{text_box_face_iu, HJustify, TextBoxStyle, VJustify};
    let style = TextBoxStyle {
        width_iu: dim.text_width_nm().get(),
        height_iu: dim.text_size_nm.get(),
        h: match dim.text_effects.h_justify {
            eda_model::ir::TextJustify::Left => HJustify::Left,
            eda_model::ir::TextJustify::Center => HJustify::Center,
            eda_model::ir::TextJustify::Right => HJustify::Right,
        },
        v: VJustify::Center,
        mirrored: dim.text_mirrored(),
        italic: dim.text_effects.italic,
    };
    let (x0, y0, x1, y1) = text_box_face_iu(text, text_pen(dim).get(), style, dim.text_effects.face.as_deref(), dim.text_effects.bold);
    let (x0, y0) = (text_at.x + Nm::new(x0) - grow_x, text_at.y + Nm::new(y0) - grow_y);
    let (x1, y1) = (text_at.x + Nm::new(x1) + grow_x, text_at.y + Nm::new(y1) + grow_y);
    let centre = Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 };
    // `rotate` turns clockwise on the screen; the text angle is counter-clockwise.
    let turn = |p: Point| rotate_about(p, centre, -(angle as i64));
    ([turn(Point { x: x0, y: y0 }), turn(Point { x: x0, y: y1 }), turn(Point { x: x1, y: y1 }), turn(Point { x: x1, y: y0 })], centre, x1 - x0)
}

/// `SHAPE_POLY_SET::Contains` of a point in the (convex, four-cornered) text box: inside by the even-odd rule, as `SHAPE_LINE_CHAIN::PointInside` counts.
fn poly_contains(poly: &[Point; 4], p: Point) -> bool {
    let (px, py) = (p.x.as_f64(), p.y.as_f64());
    let mut inside = false;
    for i in 0..4 {
        let (a, b) = (poly[i], poly[(i + 1) % 4]);
        let (ax, ay, bx, by) = (a.x.as_f64(), a.y.as_f64(), b.x.as_f64(), b.y.as_f64());
        if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax {
            inside = !inside;
        }
    }
    inside
}

/// `SEG::Intersect`: where two segments cross, rounded to the nanometre; `None` when they do not (parallel ones included).
fn seg_intersect(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, q) = (sub(b, a), sub(d, c));
    let denom = r.x.as_f64() * q.y.as_f64() - r.y.as_f64() * q.x.as_f64();
    if denom == 0.0 {
        return None;
    }
    let ac = sub(c, a);
    let t = (ac.x.as_f64() * q.y.as_f64() - ac.y.as_f64() * q.x.as_f64()) / denom;
    let u = (ac.x.as_f64() * r.y.as_f64() - ac.y.as_f64() * r.x.as_f64()) / denom;
    if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
        return None;
    }
    Some(Point { x: a.x + Nm::kiround(r.x.as_f64() * t), y: a.y + Nm::kiround(r.y.as_f64() * t) })
}

fn dist2(a: Point, b: Point) -> f64 {
    let d = sub(a, b);
    d.x.as_f64() * d.x.as_f64() + d.y.as_f64() * d.y.as_f64()
}

/// `segPolyIntersection( aPoly, aSeg, aStart )`: walking the segment from its start (its end when `!from_start`), the first point where it meets the box;
/// the far end when it never does; `None` when it starts inside the box, or would end where it starts.
fn seg_poly_intersection(poly: &[Point; 4], a: Point, b: Point, from_start: bool) -> Option<Point> {
    let (start, mut endpoint) = if from_start { (a, b) } else { (b, a) };
    if poly_contains(poly, start) {
        return None;
    }
    for i in 0..4 {
        if let Some(x) = seg_intersect(poly[i], poly[(i + 1) % 4], a, b) {
            if dist2(x, start) < dist2(endpoint, start) {
                endpoint = x;
            }
        }
    }
    (start != endpoint).then_some(endpoint)
}

/// `segCircleIntersection( aCircle, aSeg )`: as [`seg_poly_intersection`] from the start, against a circle (`CIRCLE::Contains`, `CIRCLE::Intersect( SEG )`).
fn seg_circle_intersection(centre: Point, radius: f64, a: Point, b: Point) -> Option<Point> {
    if dist2(a, centre) < radius * radius {
        return None;
    }
    let mut endpoint = b;
    let d = sub(b, a);
    let f = sub(a, centre);
    let (dx, dy, fx, fy) = (d.x.as_f64(), d.y.as_f64(), f.x.as_f64(), f.y.as_f64());
    let qa = dx * dx + dy * dy;
    if qa > 0.0 {
        let qb = 2.0 * (fx * dx + fy * dy);
        let qc = fx * fx + fy * fy - radius * radius;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            for t in [(-qb - disc.sqrt()) / (2.0 * qa), (-qb + disc.sqrt()) / (2.0 * qa)] {
                if (0.0..=1.0).contains(&t) {
                    let x = Point { x: a.x + Nm::kiround(dx * t), y: a.y + Nm::kiround(dy * t) };
                    if dist2(x, a) < dist2(endpoint, a) {
                        endpoint = x;
                    }
                }
            }
        }
    }
    (a != endpoint).then_some(endpoint)
}

/// `CollectKnockedOutSegments( aPoly, aSeg, .. )`: the segment `a`-`b` less the part the text's box covers -- 0, 1 or 2 pieces.
fn knock_out(poly: &[Point; 4], a: Point, b: Point) -> Vec<(Point, Point)> {
    let (contains_a, contains_b) = (poly_contains(poly, a), poly_contains(poly, b));
    let end_a = seg_poly_intersection(poly, a, b, true);
    let end_b = seg_poly_intersection(poly, a, b, false);
    let mut out = Vec::new();
    if let Some(ea) = end_a {
        out.push((a, ea));
    }
    if let Some(eb) = end_b {
        let can_add = match end_a {
            Some(ea) => !((eb == a && ea == b) || (ea == eb && a == b)),
            None => true,
        };
        if can_add {
            out.push((eb, b));
        }
    }
    if !contains_a && !contains_b && end_a.is_none() && end_b.is_none() {
        out.push((a, b));
    }
    out
}

/// The text where the dimension's `updateText` leaves it: placed by hand (`DIM_TEXT_POSITION::MANUAL`, `text_pos`) it stays; otherwise `computed`.
fn kept_or(dim: &Dimension, computed: Point) -> Point {
    match (dim.text_position, dim.text_pos) {
        (DimensionTextPosition::Manual, Some(p)) => p,
        _ => computed,
    }
}

// --------------------------------------------------------------- aligned

fn aligned_text_position(crossbar_start: Point, crossbar_end: Point, text_position: DimensionTextPosition, keep_aligned: bool, text_size_nm: Nm, pen: Nm) -> (Point, i64) {
    let center = Point { x: (crossbar_start.x + crossbar_end.x) / 2, y: (crossbar_start.y + crossbar_end.y) / 2 };
    let rel_center = sub(crossbar_end, crossbar_start);
    let half = Point { x: rel_center.x / 2, y: rel_center.y / 2 };

    let text_at = match text_position {
        DimensionTextPosition::Inline => center,
        // `updateText` does not move a text placed by hand; one with no place of its own sits outside.
        DimensionTextPosition::Outside | DimensionTextPosition::Manual => {
            // `textOffsetDistance = GetEffectiveTextPenWidth() + GetTextHeight()`
            let offset_distance = pen + text_size_nm;
            let rotation = if half.x == Nm::ZERO {
                if half.y > Nm::ZERO {
                    -90_000
                } else {
                    90_000
                }
            } else if half.x < Nm::ZERO {
                -90_000
            } else {
                90_000
            };
            let perp_dir = resize(rotate(half, rotation), offset_distance);
            add(crossbar_start, add(half, perp_dir))
        }
    };

    let text_angle = if keep_aligned {
        let mut a = (-angle_of(half)).rem_euclid(360_000);
        if a > 90_000 && a <= 270_000 {
            a -= 180_000;
        }
        a
    } else {
        0
    };

    (text_at, text_angle)
}

fn aligned(dim: &Dimension, height: Nm) -> DimensionGeometry {
    let dimension = sub(dim.end, dim.start);
    let measured_value_nm = Nm::kiround(norm(dimension));

    let extension = if height > Nm::ZERO { perp(dimension) } else { Point { x: dimension.y, y: -dimension.x } };

    let extension_len = height.abs() - dim.extension_offset + dim.extension_height;
    let ext1_start = add(dim.start, resize(extension, dim.extension_offset));
    let ext1_end = add(ext1_start, resize(extension, extension_len));
    let ext2_start = add(dim.end, resize(extension, dim.extension_offset));
    let ext2_end = add(ext2_start, resize(extension, extension_len));

    let cross_offset = resize(extension, height.abs()); // perp already carries the sign via its own branch
    let crossbar_start = add(dim.start, cross_offset);
    let crossbar_end = add(dim.end, cross_offset);

    let (text_at, text_angle) = aligned_text_position(crossbar_start, crossbar_end, dim.text_position, dim.keep_text_aligned, dim.text_size_nm, text_pen(dim));
    let text_at = kept_or(dim, text_at);
    let text_angle = (if dim.keep_text_aligned { text_angle } else { dim.text_angle as i64 }).rem_euclid(360_000) as Millideg;
    let text = format_value(dim, measured_value_nm);

    let mut lines = vec![(ext1_start, ext1_end), (ext2_start, ext2_end)];
    // `GetTextBox( nullptr ).Inflate( GetTextWidth() / 2, - GetEffectiveTextPenWidth() )`
    let (poly, _, _) = text_poly(dim, &text, text_at, text_angle, dim.text_width_nm() / 2, -text_pen(dim));
    lines.extend(knock_out(&poly, crossbar_start, crossbar_end));

    let dim_angle = angle_of(dimension);
    match dim.arrow_direction {
        ArrowDirection::Inward => {
            lines.extend(arrowhead(crossbar_start, dim_angle + 180_000, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
            lines.extend(arrowhead(crossbar_end, dim_angle, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
        }
        ArrowDirection::Outward => {
            lines.extend(arrowhead(crossbar_start, dim_angle + 180_000, dim.arrow_length, Nm::ZERO));
            lines.extend(arrowhead(crossbar_end, dim_angle, dim.arrow_length, Nm::ZERO));
        }
    }

    DimensionGeometry { lines, circles: Vec::new(), text_at, text_angle, measured_value_nm, text, crossbar: Some((crossbar_start, crossbar_end)), knee: None }
}

// ------------------------------------------------------------ orthogonal

fn orthogonal(dim: &Dimension, height: Nm, horizontal: bool) -> DimensionGeometry {
    let measurement = if horizontal { dim.end.x - dim.start.x } else { dim.end.y - dim.start.y };
    let measured_value_nm = measurement.abs();

    let extension = if horizontal { Point { x: Nm::ZERO, y: height } } else { Point { x: height, y: Nm::ZERO } };
    let ext1_len = height.abs() - dim.extension_offset + dim.extension_height;
    let ext1_start = add(dim.start, resize(extension, dim.extension_offset));
    let ext1_end = add(ext1_start, resize(extension, ext1_len));

    let cross_offset = resize(extension, height.abs());
    let crossbar_start = add(dim.start, cross_offset);
    let crossbar_end = if horizontal { Point { x: dim.end.x, y: crossbar_start.y } } else { Point { x: crossbar_start.x, y: dim.end.y } };

    let extension2 = if horizontal { Point { x: Nm::ZERO, y: dim.end.y - crossbar_end.y } } else { Point { x: dim.end.x - crossbar_end.x, y: Nm::ZERO } };
    let ext2_len = Nm::kiround(norm(extension2)) - dim.extension_offset + dim.extension_height;
    let ext2_start = sub(crossbar_end, resize(extension2, dim.extension_height));
    let ext2_end = add(ext2_start, resize(extension2, ext2_len));

    let crossbar_center = Point { x: (crossbar_start.x + crossbar_end.x) / 2, y: (crossbar_start.y + crossbar_end.y) / 2 };
    let half = sub(crossbar_center, crossbar_start);
    let text_at = kept_or(
        dim,
        match dim.text_position {
            DimensionTextPosition::Inline => crossbar_center,
            DimensionTextPosition::Outside | DimensionTextPosition::Manual => {
                // `textOffsetDistance = GetEffectiveTextPenWidth() + GetTextHeight()`
                let offset_distance = text_pen(dim) + dim.text_size_nm;
                let offset = if horizontal { Point { x: Nm::ZERO, y: -offset_distance } } else { Point { x: -offset_distance, y: Nm::ZERO } };
                add(crossbar_start, add(half, offset))
            }
        },
    );
    let text_angle = (if dim.keep_text_aligned {
        if half.x.abs() > half.y.abs() {
            0
        } else {
            90_000
        }
    } else {
        dim.text_angle as i64
    })
    .rem_euclid(360_000) as Millideg;

    let text = format_value(dim, measured_value_nm);
    let mut lines = vec![(ext1_start, ext1_end), (ext2_start, ext2_end)];
    // `PCB_DIM_ORTHOGONAL::updateGeometry`: `GetTextBox( nullptr ).Inflate( GetTextWidth() / 2, GetEffectiveTextPenWidth() )` -- grown by the pen, where an
    // aligned dimension's box shrinks by it (this was the aligned one's before UI-LEFT-2 PP11).
    let (poly, _, _) = text_poly(dim, &text, text_at, text_angle, dim.text_width_nm() / 2, text_pen(dim));
    lines.extend(knock_out(&poly, crossbar_start, crossbar_end));

    let crossbar_angle = angle_of(sub(crossbar_end, crossbar_start));
    match dim.arrow_direction {
        ArrowDirection::Inward => {
            lines.extend(arrowhead(crossbar_start, crossbar_angle + 180_000, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
            lines.extend(arrowhead(crossbar_end, crossbar_angle, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
        }
        ArrowDirection::Outward => {
            lines.extend(arrowhead(crossbar_start, crossbar_angle + 180_000, dim.arrow_length, Nm::ZERO));
            lines.extend(arrowhead(crossbar_end, crossbar_angle, dim.arrow_length, Nm::ZERO));
        }
    }

    DimensionGeometry { lines, circles: Vec::new(), text_at, text_angle, measured_value_nm, text, crossbar: Some((crossbar_start, crossbar_end)), knee: None }
}

// ---------------------------------------------------------------- radial

fn radial(dim: &Dimension, leader_length: Nm) -> DimensionGeometry {
    let center = dim.start;
    let radius_vec = sub(dim.end, center);
    let measured_value_nm = Nm::kiround(norm(radius_vec));

    // The small `+` mark at the centre -- fixed size (arrow_length), not
    // `end - start`, see PCB_DIM_RADIAL::updateGeometry's own
    // `centerArm(0, m_arrowLength)`.
    let arm = Point { x: Nm::ZERO, y: dim.arrow_length };
    let mut lines = vec![(sub(center, arm), add(center, arm))];
    let arm2 = rotate(arm, -90_000);
    lines.push((sub(center, arm2), add(center, arm2)));

    let knee = add(dim.end, resize(radius_vec, leader_length));
    // `PCB_DIM_RADIAL::updateGeometry` draws the line to `GetTextPos()` and never places it: the user's text position is the dimension's
    // own. A dimension created here (no `text_pos`) gets an automatic spot past the knee.
    let text_at = dim.text_pos.unwrap_or_else(|| add(knee, resize(radius_vec, dim.text_size_nm.max(Nm::new(1)))));
    let text_angle = (if dim.keep_text_aligned {
        // `PCB_DIM_RADIAL::updateText`: `textAngle = FULL_CIRCLE - EDA_ANGLE( textLine )`, normalized, folded into
        // (-90, 90], then `KiROUND( textAngle.AsDegrees() )` -- the nearest whole degree, where an aligned text keeps millidegrees.
        let text_line = sub(text_at, knee);
        let mut a = (360.0 - text_line.y.as_f64().atan2(text_line.x.as_f64()).to_degrees()).rem_euclid(360.0);
        if a > 90.0 && a <= 270.0 {
            a -= 180.0;
        }
        (a.round() as i64) * 1000
    } else {
        dim.text_angle as i64
    })
    .rem_euclid(360_000) as Millideg;

    let text = format_value(dim, measured_value_nm);
    // `GetTextBox( nullptr ).Inflate( GetTextWidth() / 2, GetEffectiveTextPenWidth() )`; the arrow's line and the line to the text knocked out by it.
    let (poly, _, _) = text_poly(dim, &text, text_at, text_angle, dim.text_width_nm() / 2, text_pen(dim));
    lines.extend(knock_out(&poly, dim.end, knee));
    lines.extend(knock_out(&poly, knee, text_at));

    let radial_angle = angle_of(radius_vec);
    lines.extend(arrowhead(dim.end, radial_angle, dim.arrow_length, Nm::ZERO));

    DimensionGeometry { lines, circles: Vec::new(), text_at, text_angle, measured_value_nm, text, crossbar: None, knee: Some(knee) }
}

// ---------------------------------------------------------------- leader

/// `PCB_DIM_LEADER::updateGeometry`: the text's box (`GetTextBox( nullptr ).Inflate( GetTextWidth() / 2, GetEffectiveTextPenWidth() * 2 )`, turned by the
/// text angle) or, for a `DIM_TEXT_BORDER::CIRCLE` frame, the circle through it (radius half the box's width less half the pen) is what the two lines run
/// to: the arrow's line from `start` (moved in by the extension offset) to where the segment `start`-`end` meets the frame (`end` when it does not), the
/// arrow at its start, the frame itself when there is text (a `RECTANGLE` as its four sides, a `CIRCLE` as a circle; `ROUNDRECT` draws none, as 8303b2ad's
/// `switch( m_textBorder )` has no case for it), and, when the arrow's line reaches `end`, the line from `end` to where `end`-text meets the frame.
fn leader(dim: &Dimension) -> DimensionGeometry {
    let first_line = sub(dim.end, dim.start);
    let measured_value_nm = Nm::kiround(norm(first_line));
    let start = add(dim.start, resize(first_line, dim.extension_offset));

    let text = format_value(dim, measured_value_nm);
    // `PCB_DIM_LEADER::updateGeometry` builds its line from `m_end` to `GetTextPos()`; with no text position (a leader created here) the
    // text sits a fixed offset off the knee, continuing the arrow's own direction. Its `updateText` is `updateGeometry`: the angle is the
    // file's, never recomputed, whatever `keep_text_aligned` says.
    let text_at = dim.text_pos.unwrap_or_else(|| add(dim.end, resize(first_line, dim.text_size_nm.max(Nm::new(1)) * 2)));
    let text_angle = (dim.text_angle as i64).rem_euclid(360_000) as Millideg;

    let pen = text_pen(dim);
    let (poly, centre, box_width) = text_poly(dim, &text, text_at, text_angle, dim.text_width_nm() / 2, pen * 2);
    let radius = box_width.as_f64() / 2.0 - pen.as_f64() / 2.0;
    let (arrow_seg_end, text_seg_end) = if dim.text_frame == DimensionTextBorder::Circle {
        (seg_circle_intersection(centre, radius, dim.start, dim.end), seg_circle_intersection(centre, radius, dim.end, text_at))
    } else {
        (seg_poly_intersection(&poly, dim.start, dim.end, true), seg_poly_intersection(&poly, dim.end, text_at, true))
    };
    let arrow_seg_end = arrow_seg_end.unwrap_or(dim.end);

    let mut lines = vec![(start, arrow_seg_end)];
    lines.extend(arrowhead(start, angle_of(first_line), dim.arrow_length, Nm::ZERO));
    let mut circles = Vec::new();
    if !text.is_empty() {
        match dim.text_frame {
            DimensionTextBorder::Rectangle => lines.extend((0..4).map(|i| (poly[i], poly[(i + 1) % 4]))),
            DimensionTextBorder::Circle => circles.push((centre, Nm::kiround(radius))),
            DimensionTextBorder::None | DimensionTextBorder::RoundRect => {}
        }
    }
    if let (Some(t), true) = (text_seg_end, arrow_seg_end == dim.end) {
        lines.push((dim.end, t));
    }

    DimensionGeometry { lines, circles, text_at, text_angle, measured_value_nm, text, crossbar: None, knee: None }
}

// ---------------------------------------------------------------- center

fn center(dim: &Dimension) -> DimensionGeometry {
    let center_pt = dim.start;
    let arm = sub(dim.end, dim.start);
    let lines = vec![(sub(center_pt, arm), add(center_pt, arm)), (sub(center_pt, rotate(arm, -90_000)), add(center_pt, rotate(arm, -90_000)))];

    DimensionGeometry { lines, circles: Vec::new(), text_at: center_pt, text_angle: 0, measured_value_nm: Nm::ZERO, text: String::new(), crossbar: None, knee: None }
}

fn rotate_point_set(pts: &mut [Point], pivot: Point, angle_millideg: i64) {
    for p in pts.iter_mut() {
        *p = rotate_about(*p, pivot, angle_millideg);
    }
}

/// Translate a dimension's own feature points by `(dx, dy)` -- what
/// dragging it does (`PCB_DIMENSION_BASE::Move`, which offsets the text too:
/// `PCB_TEXT::Offset( offset )`). A kept text position moves along; an
/// aligned/orthogonal text has none and is recomputed from `start`/`end`.
pub fn translate_dimension(dim: &mut Dimension, dx: Nm, dy: Nm) {
    dim.start.x += dx;
    dim.start.y += dy;
    dim.end.x += dx;
    dim.end.y += dy;
    if let Some(p) = &mut dim.text_pos {
        p.x += dx;
        p.y += dy;
    }
}

/// Rotate a dimension's own feature points about `pivot` by `angle_millideg`, clockwise-positive on the screen as every
/// turn of this crate -- `PCB_DIMENSION_BASE::Rotate`. `text_angle` (only meaningful when `keep_text_aligned` is false)
/// turns along: it is KiCad's counter-clockwise file angle, so KiCad's `newAngle = GetTextAngle() + aAngle` (with
/// `aAngle` counter-clockwise) is a subtraction here, as `PCB_TEXT::Rotate` is for a text. A kept text position turns
/// about the pivot with the feature points (`RotatePoint( pt, aRotCentre, aAngle )` in `PCB_DIMENSION_BASE::Rotate`).
pub fn rotate_dimension(dim: &mut Dimension, pivot: Point, angle_millideg: i64) {
    let mut pts = [dim.start, dim.end];
    rotate_point_set(&mut pts, pivot, angle_millideg);
    dim.start = pts[0];
    dim.end = pts[1];
    if let Some(p) = &mut dim.text_pos {
        *p = rotate_about(*p, pivot, angle_millideg);
    }
    dim.text_angle = ((dim.text_angle as i64 - angle_millideg).rem_euclid(360_000)) as Millideg;
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    fn base_dim(kind: DimensionKind, start: Point, end: Point) -> Dimension {
        Dimension {
            id: String::new(),
            layer: "Dwgs.User".into(),
            kind,
            start,
            end,
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
            stroke_width: mm(0.15),
            arrow_length: mm(1.0),
            extension_offset: mm(0.2),
            extension_height: mm(0.5),
            arrow_direction: ArrowDirection::Outward,
            text_pos: None,
            text_frame: Default::default(),
            text_effects: Default::default(),
            text_thickness_nm: None,
        }
    }

    /// The width of the gap an inline label cuts in a horizontal crossbar at y = `y`.
    fn crossbar_gap(g: &DimensionGeometry) -> Nm {
        let mut xs: Vec<Nm> = g.lines.iter().filter(|(a, b)| a.y == b.y).flat_map(|(a, b)| [a.x, b.x]).collect();
        xs.sort();
        xs[2] - xs[1]
    }

    /// `PCB_DIM_ORTHOGONAL::updateGeometry` grows the label's box by the pen (`Inflate( GetTextWidth() / 2, GetEffectiveTextPenWidth() )`) where
    /// `PCB_DIM_ALIGNED::updateGeometry` shrinks it (`- GetEffectiveTextPenWidth()`): the same inline label cuts the same horizontal crossbar the same width
    /// (the height of the box is across it), and a label twice as wide a gap one advance wider (UI-LEFT-2 PP11).
    #[test]
    fn an_inline_label_cuts_its_own_box_out_of_the_crossbar() {
        let (a, b) = (Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(20.0), y: Nm::ZERO });
        let mut aligned = base_dim(DimensionKind::Aligned { height: mm(3.0) }, a, b);
        aligned.text_position = DimensionTextPosition::Inline;
        let mut ortho = base_dim(DimensionKind::Orthogonal { height: mm(3.0), horizontal: true }, a, b);
        ortho.text_position = DimensionTextPosition::Inline;
        let (ga, go) = (compute_dimension_geometry(&aligned), compute_dimension_geometry(&ortho));
        assert_eq!(crossbar_gap(&ga), crossbar_gap(&go), "the box's height is across the crossbar");
        let mut wide = aligned.clone();
        wide.text_effects.width_nm = Some(mm(2.0));
        let gw = compute_dimension_geometry(&wide);
        // "20": '2' and '0' advance 20/21 em each, less the 0.2 em gap after the last: (40/21 - 0.2) mm more, and the inflate's half width 0.5 mm more each side
        let more = Nm::kiround((40.0 / 21.0 - 0.2) * 1e6) + mm(1.0);
        assert!((crossbar_gap(&gw) - crossbar_gap(&ga) - more).abs() <= Nm::new(2), "{:?} {:?} {:?}", crossbar_gap(&gw), crossbar_gap(&ga), more);
        // a pen thicker than a quarter of the size is clamped (`ClampTextPenSize`)
        let mut thick = aligned.clone();
        thick.text_thickness_nm = Some(mm(0.5));
        let mut quarter = aligned.clone();
        quarter.text_thickness_nm = Some(mm(0.25));
        assert_eq!(compute_dimension_geometry(&thick).lines, compute_dimension_geometry(&quarter).lines);
    }

    #[test]
    fn aligned_measures_the_euclidean_distance() {
        let dim = base_dim(DimensionKind::Aligned { height: mm(2.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(3.0), y: mm(4.0) });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_nm, mm(5.0)); // 3-4-5 triangle
        assert_eq!(g.text, "5");
    }

    #[test]
    fn aligned_horizontal_line_puts_the_crossbar_above_for_positive_height() {
        let dim = base_dim(DimensionKind::Aligned { height: mm(1.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO });
        let g = compute_dimension_geometry(&dim);
        // Two extension lines plus a crossbar (possibly split) plus two
        // 2-segment arrowheads -- at least 2 + 1 + 4 = 7 lines.
        assert!(g.lines.len() >= 7, "{} lines", g.lines.len());
        // The crossbar must sit at y = -1000 (one of the extension lines'
        // own far endpoint) for a positive height with this crate's
        // perp() convention -- whichever sign, it must be consistent and
        // off the feature line (y != 0).
        let crossbar_y: Nm = g.lines.iter().map(|(a, b)| if a.y == b.y && a.y != Nm::ZERO { Some(a.y) } else { None }).find_map(|v| v).expect("a horizontal crossbar segment");
        assert_ne!(crossbar_y, Nm::ZERO);
    }

    #[test]
    fn aligned_negative_height_puts_the_crossbar_on_the_other_side() {
        let pos = compute_dimension_geometry(&base_dim(DimensionKind::Aligned { height: mm(1.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }));
        let neg = compute_dimension_geometry(&base_dim(DimensionKind::Aligned { height: mm(-1.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }));
        let find_y = |g: &DimensionGeometry| g.lines.iter().find_map(|(a, b)| if a.y == b.y && a.y != Nm::ZERO { Some(a.y) } else { None }).unwrap();
        assert_eq!(find_y(&pos), -find_y(&neg));
    }

    #[test]
    fn orthogonal_horizontal_ignores_the_y_delta_in_its_measurement() {
        let dim = base_dim(DimensionKind::Orthogonal { height: mm(1.0), horizontal: true }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: mm(9.999) });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_nm, mm(5.0));
    }

    #[test]
    fn orthogonal_vertical_ignores_the_x_delta() {
        let dim = base_dim(DimensionKind::Orthogonal { height: mm(1.0), horizontal: false }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(9.999), y: mm(7.0) });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_nm, mm(7.0));
    }

    #[test]
    fn radial_measures_the_radius_and_draws_a_center_cross() {
        let dim = base_dim(DimensionKind::Radial { leader_length: mm(0.5) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(2.0), y: Nm::ZERO });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_nm, mm(2.0));
        // The centre cross is the two segments straddling (0,0) with the
        // fixed arrow_length arm -- both must be present regardless of
        // the radius.
        let straddles_origin = |a: Point, b: Point| a.x == -b.x && a.y == -b.y && (a.x != Nm::ZERO || a.y != Nm::ZERO);
        let cross_arms = g.lines.iter().filter(|(a, b)| straddles_origin(*a, *b)).count();
        assert_eq!(cross_arms, 2, "both arms of the centre cross must be present");
    }

    #[test]
    fn leader_places_an_arrow_at_start_and_a_line_toward_the_text() {
        let mut dim = base_dim(DimensionKind::Leader, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO });
        dim.override_text = Some("note".into());
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "note");
        assert!(!g.lines.is_empty());
    }

    /// `DIM_TEXT_POSITION::MANUAL`: `updateText` leaves the text where it was put, for an aligned and an orthogonal dimension alike; the crossbar runs under
    /// it, knocked out where the text's box covers it (`CollectKnockedOutSegments`), and whole when the text is off it.
    #[test]
    fn a_text_placed_by_hand_stays_and_knocks_out_the_crossbar_it_sits_on() {
        let origin = Point { x: Nm::ZERO, y: Nm::ZERO };
        for kind in [DimensionKind::Aligned { height: mm(-2.0) }, DimensionKind::Orthogonal { height: mm(-2.0), horizontal: true }] {
            let mut dim = base_dim(kind, origin, Point { x: mm(20.0), y: Nm::ZERO });
            let outside = compute_dimension_geometry(&dim);
            let (cs, ce) = outside.crossbar.expect("a crossbar");
            assert_eq!((cs.y, ce.y), (mm(-2.0), mm(-2.0)), "{kind:?}");
            let crossbar_pieces = |g: &DimensionGeometry| g.lines.iter().filter(|(a, b)| a.y == mm(-2.0) && b.y == mm(-2.0)).count();
            assert_eq!(crossbar_pieces(&outside), 1, "the text outside leaves the crossbar whole: {kind:?}");
            // On the crossbar, a quarter of the way along.
            dim.text_position = DimensionTextPosition::Manual;
            dim.text_pos = Some(Point { x: mm(5.0), y: mm(-2.0) });
            let manual = compute_dimension_geometry(&dim);
            assert_eq!(manual.text_at, Point { x: mm(5.0), y: mm(-2.0) }, "{kind:?}");
            assert_eq!(crossbar_pieces(&manual), 2, "{kind:?}: {:?}", manual.lines);
            let gap: Vec<Nm> = manual.lines.iter().filter(|(a, b)| a.y == mm(-2.0) && b.y == mm(-2.0)).flat_map(|(a, b)| [a.x, b.x]).collect();
            // "20" at 1 mm with a 0.15 mm pen: the box is two glyphs and the pen's reach wide (`StringBoundaryLimits`), grown by half the size on each side,
            // 3.15 mm in all, centred on the text.
            assert_eq!(gap, vec![Nm::ZERO, mm(5.0) - Nm::new(1_577_381), mm(5.0) + Nm::new(1_577_381), mm(20.0)], "{kind:?}");
            // A manual mode with no position of its own falls back to the outside place.
            dim.text_pos = None;
            assert_eq!(compute_dimension_geometry(&dim).text_at, outside.text_at);
        }
    }

    /// `PCB_DIM_LEADER::updateGeometry`: a rectangle frame is the text's grown box as four lines, a circle frame a circle through it, and the line from the
    /// knee stops at the frame; none draws no frame and the line still stops at the box.
    #[test]
    fn a_leaders_frame_is_drawn_round_its_text_and_the_line_stops_at_it() {
        let mut dim = base_dim(DimensionKind::Leader, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: mm(5.0) });
        dim.override_text = Some("NOTE".into());
        dim.text_pos = Some(Point { x: mm(15.0), y: mm(5.0) });
        let plain = compute_dimension_geometry(&dim);
        assert!(plain.circles.is_empty());
        let to_text = *plain.lines.last().unwrap();
        assert_eq!(to_text.0, dim.end, "the last line starts at the knee");
        assert!(to_text.1.x > mm(5.0) && to_text.1.x < mm(15.0) && to_text.1.y == mm(5.0), "and stops at the text's box: {to_text:?}");
        dim.text_frame = DimensionTextBorder::Rectangle;
        let rect = compute_dimension_geometry(&dim);
        assert_eq!(rect.lines.len(), plain.lines.len() + 4, "{:?}", rect.lines);
        assert_eq!(rect.lines.last().unwrap(), &to_text, "the line to the box is the same");
        dim.text_frame = DimensionTextBorder::Circle;
        let circle = compute_dimension_geometry(&dim);
        assert_eq!(circle.lines.len(), plain.lines.len());
        let (c, r) = circle.circles[0];
        assert!((c.x - mm(15.0)).abs() < mm(0.1) && (c.y - mm(5.0)).abs() < mm(0.1), "{c:?}");
        let end = circle.lines.last().unwrap().1;
        assert!((((end.x - c.x).as_f64()).hypot((end.y - c.y).as_f64()) - r.as_f64()).abs() < 2.0, "the line ends on the circle: {end:?} {c:?} {r:?}");
        // No text, no frame.
        dim.override_text = Some(String::new());
        assert!(compute_dimension_geometry(&dim).circles.is_empty());
    }

    #[test]
    fn center_mark_has_no_text_and_two_perpendicular_arms() {
        let dim = base_dim(DimensionKind::Center, Point { x: mm(1.0), y: mm(1.0) }, Point { x: mm(1.5), y: mm(1.0) });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "");
        assert_eq!(g.lines.len(), 2);
        let (a0, b0) = g.lines[0];
        let (a1, b1) = g.lines[1];
        // One arm horizontal (matches end-start), the other vertical (rotated 90).
        assert_eq!(a0.y, b0.y);
        assert_eq!(a1.x, b1.x);
    }

    #[test]
    fn override_text_replaces_the_computed_number_but_keeps_prefix_and_suffix() {
        let mut dim = base_dim(DimensionKind::Aligned { height: mm(1.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO });
        dim.prefix = "W=".into();
        dim.suffix = "!".into();
        dim.override_text = Some("TBD".into());
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "W=TBD!");
    }

    #[test]
    fn suppressing_trailing_zeros_drops_a_bare_decimal_point_too() {
        let dim = base_dim(DimensionKind::Aligned { height: mm(1.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(2.0), y: Nm::ZERO });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "2");
    }

    #[test]
    fn units_format_adds_the_right_suffix() {
        let mut dim = base_dim(DimensionKind::Aligned { height: mm(1.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO });
        dim.units_format = DimensionUnitsFormat::BareSuffix;
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "1 mm");
        dim.units_format = DimensionUnitsFormat::ParenSuffix;
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "1 (mm)");
    }

    #[test]
    fn translate_moves_both_feature_points() {
        let mut dim = base_dim(DimensionKind::Leader, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO });
        translate_dimension(&mut dim, mm(0.5), mm(-0.2));
        assert_eq!(dim.start, Point { x: mm(0.5), y: mm(-0.2) });
        assert_eq!(dim.end, Point { x: mm(1.5), y: mm(-0.2) });
    }

    #[test]
    fn rotate_about_pivot_moves_a_point_90_degrees_clockwise() {
        let mut dim = base_dim(DimensionKind::Leader, Point { x: mm(1.0), y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO });
        rotate_dimension(&mut dim, Point { x: Nm::ZERO, y: Nm::ZERO }, 90_000);
        assert_eq!(dim.start, Point { x: Nm::ZERO, y: mm(1.0) });
        assert_eq!(dim.text_angle, 270_000, "a clockwise quarter turn is KiCad's -90");
    }

    #[test]
    fn a_unit_dependent_precision_gives_the_decimals_of_its_units() {
        // `GetValueText`: V_VV (6) is 0.00 in / 0 mils / 0.0 mm, V_VVVVV (9) 0.00000 in / 0.00 mils / 0.0000 mm.
        assert_eq!([DimensionUnits::Inch, DimensionUnits::Mil, DimensionUnits::Mm].map(|u| decimals(6, u)), [2, 0, 1]);
        assert_eq!([DimensionUnits::Inch, DimensionUnits::Mil, DimensionUnits::Mm].map(|u| decimals(9, u)), [5, 2, 4]);
        assert_eq!(decimals(3, DimensionUnits::Mil), 3);
        let mut dim = base_dim(DimensionKind::Aligned { height: mm(1.0) }, Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(12.34), y: Nm::ZERO });
        dim.precision = 6;
        dim.suppress_trailing_zeros = false;
        assert_eq!(compute_dimension_geometry(&dim).text, "12.3");
    }
}
