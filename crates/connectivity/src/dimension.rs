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
//! Not ported here (see PARITY-pcb.md section 18 for the full list):
//! the crossbar/leader-line "knockout" only approximates inline text's
//! own rendered width (this model has no real font-metrics engine
//! anywhere -- `eda_engine::geometry::CHAR_WIDTH_FACTOR`'s own `0.6 *
//! font_size` heuristic, reused here rather than re-derived, is the same
//! approximation every other text-overlap check in this project already
//! accepts); a leader's optional text border (rectangle/circle); `DIM_
//! PRECISION`'s four unit-dependent "V_VVV" levels (`Dimension::precision`
//! is a flat decimal count instead).

use eda_model::ir::{ArrowDirection, Dimension, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, Millideg, Point, Um};

/// `CHAR_WIDTH_FACTOR` in `crates/engine/src/geometry.rs` -- same
/// approximation, reused rather than re-derived (see this module's own
/// header). That constant is in mm-per-character-per-mm-of-font-size;
/// this crate works in whole micrometres throughout.
const CHAR_WIDTH_FACTOR: f64 = 0.6;

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
    pub text_at: Point,
    pub text_angle: Millideg,
    pub measured_value_um: Um,
    pub text: String,
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
    ((p.x * p.x + p.y * p.y) as f64).sqrt()
}
/// `v` rescaled to length `len` (sign of `len` flips the direction),
/// preserving direction otherwise. The zero vector has no direction, so
/// it stays zero -- same degenerate case `VECTOR2I::Resize` has.
fn resize(v: Point, len: Um) -> Point {
    let n = norm(v);
    if n < 1.0 {
        return Point { x: 0, y: 0 };
    }
    let scale = len as f64 / n;
    Point { x: (v.x as f64 * scale).round() as Um, y: (v.y as f64 * scale).round() as Um }
}
/// The perpendicular of `v`, rotated 90 degrees clockwise (this crate's
/// own Y-down convention -- see this module's header).
fn perp(v: Point) -> Point {
    Point { x: -v.y, y: v.x }
}
/// Angle of `v` from the +X axis, clockwise-positive in Y-down screen
/// coordinates (`atan2`'s own natural direction once Y grows downward).
fn angle_of(v: Point) -> i64 {
    (v.y as f64).atan2(v.x as f64).to_degrees().round() as i64 * 1000
}
fn rotate(p: Point, angle_millideg: i64) -> Point {
    let rad = (angle_millideg as f64) / 1000.0 * std::f64::consts::PI / 180.0;
    let (sin, cos) = rad.sin_cos();
    Point { x: (p.x as f64 * cos - p.y as f64 * sin).round() as Um, y: (p.x as f64 * sin + p.y as f64 * cos).round() as Um }
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
fn arrowhead(tip: Point, toward_angle_millideg: i64, length: Um, tail: Um) -> Vec<(Point, Point)> {
    let mut lines = Vec::with_capacity(3);
    if tail != 0 {
        let t = rotate(Point { x: tail, y: 0 }, toward_angle_millideg + 180_000);
        lines.push((tip, add(tip, t)));
    }
    let back = toward_angle_millideg + 180_000;
    let b1 = rotate(Point { x: length, y: 0 }, back + ARROW_HALF_ANGLE_MILLIDEG);
    let b2 = rotate(Point { x: length, y: 0 }, back - ARROW_HALF_ANGLE_MILLIDEG);
    lines.push((tip, add(tip, b1)));
    lines.push((tip, add(tip, b2)));
    lines
}

/// Resolve `Automatic` to millimetres -- this model's own board-wide
/// convention elsewhere (`crates/model/src/ir.rs` `pub type Um`'s own
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

/// `PCB_DIMENSION_BASE::GetValueText` plus the prefix/suffix/units-suffix
/// wrapping `updateText()` adds around it.
fn format_value(dim: &Dimension, measured_value_um: Um) -> String {
    if let Some(text) = &dim.override_text {
        let mut s = dim.prefix.clone();
        s.push_str(text);
        s.push_str(&dim.suffix);
        return s;
    }

    let units = resolve_units(dim.units);
    let (value, unit_suffix) = match units {
        DimensionUnits::Mm => (measured_value_um as f64 / 1000.0, "mm"),
        DimensionUnits::Mil => (measured_value_um as f64 / 25.4, "mil"),
        DimensionUnits::Inch => (measured_value_um as f64 / 25_400.0, "in"),
        DimensionUnits::Automatic => unreachable!("resolved above"),
    };

    let precision = dim.precision.min(5) as usize;
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

/// Estimated half-width (um) of `text` set at `size_um` -- see this
/// module's own header on why this, and not real font metrics.
fn half_width(text: &str, size_um: Um) -> Um {
    ((text.chars().count() as f64 * CHAR_WIDTH_FACTOR * size_um as f64) / 2.0).round() as Um
}

/// Split `seg` (a crossbar or leader-to-text line) around `text_at`,
/// knocking out a gap `2*half_width(text)` wide centred there, along the
/// segment's own direction -- only when `text_at` actually falls near
/// the segment (inline text sitting on the line); otherwise the segment
/// is returned whole, same as source's own `CollectKnockedOutSegments`
/// never splitting a crossbar that an outside-positioned text box
/// doesn't actually collide with.
fn knockout(a: Point, b: Point, text_at: Point, text: &str, text_size_um: Um) -> Vec<(Point, Point)> {
    let dir = sub(b, a);
    let len = norm(dir);
    if len < 1.0 || text.is_empty() {
        return vec![(a, b)];
    }
    let t = ((text_at.x - a.x) as f64 * dir.x as f64 + (text_at.y - a.y) as f64 * dir.y as f64) / (len * len);
    let proj = Point { x: a.x + (dir.x as f64 * t).round() as Um, y: a.y + (dir.y as f64 * t).round() as Um };
    // Perpendicular distance from text_at to the line -- only knock out
    // a gap when the text is genuinely sitting close to the line, not
    // merely somewhere near its extent (an outside-positioned text's
    // perpendicular offset is a full text height or more away).
    let perp_dist = norm(sub(text_at, proj));
    if perp_dist > text_size_um as f64 {
        return vec![(a, b)];
    }
    let hw = half_width(text, text_size_um).max(1);
    let gap_start = (t * len).round() as Um - hw;
    let gap_end = (t * len).round() as Um + hw;
    if gap_start <= 0 || gap_end >= len.round() as Um {
        // The gap swallows (or nearly swallows) the whole segment -- draw nothing.
        if gap_start <= 0 && gap_end >= len.round() as Um {
            return vec![];
        }
    }
    let mut out = Vec::new();
    if gap_start > 0 {
        out.push((a, add(a, resize(dir, gap_start))));
    }
    if gap_end < len.round() as Um {
        out.push((add(a, resize(dir, gap_end)), b));
    }
    out
}

// --------------------------------------------------------------- aligned

fn aligned_text_position(crossbar_start: Point, crossbar_end: Point, text_position: DimensionTextPosition, keep_aligned: bool, text_size_um: Um, stroke_width: Um) -> (Point, i64) {
    let center = Point { x: (crossbar_start.x + crossbar_end.x) / 2, y: (crossbar_start.y + crossbar_end.y) / 2 };
    let rel_center = sub(crossbar_end, crossbar_start);
    let half = Point { x: rel_center.x / 2, y: rel_center.y / 2 };

    let text_at = match text_position {
        DimensionTextPosition::Inline => center,
        DimensionTextPosition::Outside => {
            let offset_distance = stroke_width + text_size_um;
            let rotation = if half.x == 0 {
                if half.y > 0 {
                    -90_000
                } else {
                    90_000
                }
            } else if half.x < 0 {
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

fn aligned(dim: &Dimension, height: Um) -> DimensionGeometry {
    let dimension = sub(dim.end, dim.start);
    let measured_value_um = norm(dimension).round() as Um;

    let extension = if height > 0 { perp(dimension) } else { Point { x: dimension.y, y: -dimension.x } };

    let extension_len = height.abs() - dim.extension_offset + dim.extension_height;
    let ext1_start = add(dim.start, resize(extension, dim.extension_offset));
    let ext1_end = add(ext1_start, resize(extension, extension_len));
    let ext2_start = add(dim.end, resize(extension, dim.extension_offset));
    let ext2_end = add(ext2_start, resize(extension, extension_len));

    let cross_offset = resize(extension, height.abs()); // perp already carries the sign via its own branch
    let crossbar_start = add(dim.start, cross_offset);
    let crossbar_end = add(dim.end, cross_offset);

    let (text_at, text_angle) = aligned_text_position(crossbar_start, crossbar_end, dim.text_position, dim.keep_text_aligned, dim.text_size_um, dim.stroke_width);
    let text_angle = (if dim.keep_text_aligned { text_angle } else { dim.text_angle as i64 }).rem_euclid(360_000) as Millideg;
    let text = format_value(dim, measured_value_um);

    let mut lines = vec![(ext1_start, ext1_end), (ext2_start, ext2_end)];
    lines.extend(knockout(crossbar_start, crossbar_end, text_at, &text, dim.text_size_um));

    let dim_angle = angle_of(dimension);
    match dim.arrow_direction {
        ArrowDirection::Inward => {
            lines.extend(arrowhead(crossbar_start, dim_angle + 180_000, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
            lines.extend(arrowhead(crossbar_end, dim_angle, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
        }
        ArrowDirection::Outward => {
            lines.extend(arrowhead(crossbar_start, dim_angle + 180_000, dim.arrow_length, 0));
            lines.extend(arrowhead(crossbar_end, dim_angle, dim.arrow_length, 0));
        }
    }

    DimensionGeometry { lines, text_at, text_angle, measured_value_um, text }
}

// ------------------------------------------------------------ orthogonal

fn orthogonal(dim: &Dimension, height: Um, horizontal: bool) -> DimensionGeometry {
    let measurement = if horizontal { dim.end.x - dim.start.x } else { dim.end.y - dim.start.y };
    let measured_value_um = measurement.abs();

    let extension = if horizontal { Point { x: 0, y: height } } else { Point { x: height, y: 0 } };
    let ext1_len = height.abs() - dim.extension_offset + dim.extension_height;
    let ext1_start = add(dim.start, resize(extension, dim.extension_offset));
    let ext1_end = add(ext1_start, resize(extension, ext1_len));

    let cross_offset = resize(extension, height.abs());
    let crossbar_start = add(dim.start, cross_offset);
    let crossbar_end = if horizontal { Point { x: dim.end.x, y: crossbar_start.y } } else { Point { x: crossbar_start.x, y: dim.end.y } };

    let extension2 = if horizontal { Point { x: 0, y: dim.end.y - crossbar_end.y } } else { Point { x: dim.end.x - crossbar_end.x, y: 0 } };
    let ext2_len = norm(extension2).round() as Um - dim.extension_offset + dim.extension_height;
    let ext2_start = sub(crossbar_end, resize(extension2, dim.extension_height));
    let ext2_end = add(ext2_start, resize(extension2, ext2_len));

    let crossbar_center = Point { x: (crossbar_start.x + crossbar_end.x) / 2, y: (crossbar_start.y + crossbar_end.y) / 2 };
    let half = sub(crossbar_center, crossbar_start);
    let text_at = match dim.text_position {
        DimensionTextPosition::Inline => crossbar_center,
        DimensionTextPosition::Outside => {
            let offset_distance = dim.stroke_width + dim.text_size_um;
            let offset = if horizontal { Point { x: 0, y: -offset_distance } } else { Point { x: -offset_distance, y: 0 } };
            add(crossbar_start, add(half, offset))
        }
    };
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

    let text = format_value(dim, measured_value_um);
    let mut lines = vec![(ext1_start, ext1_end), (ext2_start, ext2_end)];
    lines.extend(knockout(crossbar_start, crossbar_end, text_at, &text, dim.text_size_um));

    let crossbar_angle = angle_of(sub(crossbar_end, crossbar_start));
    match dim.arrow_direction {
        ArrowDirection::Inward => {
            lines.extend(arrowhead(crossbar_start, crossbar_angle + 180_000, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
            lines.extend(arrowhead(crossbar_end, crossbar_angle, dim.arrow_length, dim.arrow_length * INWARD_TAIL_RATIO));
        }
        ArrowDirection::Outward => {
            lines.extend(arrowhead(crossbar_start, crossbar_angle + 180_000, dim.arrow_length, 0));
            lines.extend(arrowhead(crossbar_end, crossbar_angle, dim.arrow_length, 0));
        }
    }

    DimensionGeometry { lines, text_at, text_angle, measured_value_um, text }
}

// ---------------------------------------------------------------- radial

fn radial(dim: &Dimension, leader_length: Um) -> DimensionGeometry {
    let center = dim.start;
    let radius_vec = sub(dim.end, center);
    let measured_value_um = norm(radius_vec).round() as Um;

    // The small `+` mark at the centre -- fixed size (arrow_length), not
    // `end - start`, see PCB_DIM_RADIAL::updateGeometry's own
    // `centerArm(0, m_arrowLength)`.
    let arm = Point { x: 0, y: dim.arrow_length };
    let mut lines = vec![(sub(center, arm), add(center, arm))];
    let arm2 = rotate(arm, -90_000);
    lines.push((sub(center, arm2), add(center, arm2)));

    let knee = add(dim.end, resize(radius_vec, leader_length));
    let text_at = add(knee, resize(radius_vec, dim.text_size_um.max(1)));
    let text_angle = (if dim.keep_text_aligned {
        let mut a = (-angle_of(sub(text_at, knee))).rem_euclid(360_000);
        if a > 90_000 && a <= 270_000 {
            a -= 180_000;
        }
        a
    } else {
        dim.text_angle as i64
    })
    .rem_euclid(360_000) as Millideg;

    let text = format_value(dim, measured_value_um);
    lines.extend(knockout(dim.end, knee, text_at, &text, dim.text_size_um));
    lines.extend(knockout(knee, text_at, text_at, &text, dim.text_size_um));

    let radial_angle = angle_of(radius_vec);
    lines.extend(arrowhead(dim.end, radial_angle, dim.arrow_length, 0));

    DimensionGeometry { lines, text_at, text_angle, measured_value_um, text }
}

// ---------------------------------------------------------------- leader

fn leader(dim: &Dimension) -> DimensionGeometry {
    let first_line = sub(dim.end, dim.start);
    let measured_value_um = norm(first_line).round() as Um;
    let start = add(dim.start, resize(first_line, dim.extension_offset));

    let text = format_value(dim, measured_value_um);
    // Text sits a fixed offset off the knee, continuing the arrow's own
    // direction -- source lets the user drag it freely (`GetTextPos()`
    // is independently settable); this port places it automatically,
    // same "no manual sub-element dragging" scope PARITY-pcb.md already
    // notes elsewhere.
    let text_at = add(dim.end, resize(first_line, dim.text_size_um.max(1) * 2));
    let text_angle = (if dim.keep_text_aligned { 0 } else { dim.text_angle as i64 }).rem_euclid(360_000) as Millideg;

    let mut lines = knockout(start, dim.end, text_at, &text, dim.text_size_um);
    lines.extend(arrowhead(start, angle_of(first_line), dim.arrow_length, 0));
    if !text.is_empty() {
        lines.extend(knockout(dim.end, text_at, text_at, &text, dim.text_size_um));
    }

    DimensionGeometry { lines, text_at, text_angle, measured_value_um, text }
}

// ---------------------------------------------------------------- center

fn center(dim: &Dimension) -> DimensionGeometry {
    let center_pt = dim.start;
    let arm = sub(dim.end, dim.start);
    let lines = vec![(sub(center_pt, arm), add(center_pt, arm)), (sub(center_pt, rotate(arm, -90_000)), add(center_pt, rotate(arm, -90_000)))];

    DimensionGeometry { lines, text_at: center_pt, text_angle: 0, measured_value_um: 0, text: String::new() }
}

fn rotate_point_set(pts: &mut [Point], pivot: Point, angle_millideg: i64) {
    for p in pts.iter_mut() {
        *p = rotate_about(*p, pivot, angle_millideg);
    }
}

/// Translate a dimension's own feature points by `(dx, dy)` -- what
/// dragging it does (`PCB_DIMENSION_BASE::Move`). Text position/angle are
/// always recomputed from `start`/`end`, never carried along separately.
pub fn translate_dimension(dim: &mut Dimension, dx: Um, dy: Um) {
    dim.start.x += dx;
    dim.start.y += dy;
    dim.end.x += dx;
    dim.end.y += dy;
}

/// Rotate a dimension's own feature points about `pivot` -- `PCB_
/// DIMENSION_BASE::Rotate`. `text_angle` (only meaningful when
/// `keep_text_aligned` is false) rotates along too.
pub fn rotate_dimension(dim: &mut Dimension, pivot: Point, angle_millideg: i64) {
    let mut pts = [dim.start, dim.end];
    rotate_point_set(&mut pts, pivot, angle_millideg);
    dim.start = pts[0];
    dim.end = pts[1];
    dim.text_angle = ((dim.text_angle as i64 + angle_millideg).rem_euclid(360_000)) as Millideg;
}

#[cfg(test)]
mod tests {
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
            text_size_um: 1000,
            stroke_width: 150,
            arrow_length: 1000,
            extension_offset: 200,
            extension_height: 500,
            arrow_direction: ArrowDirection::Outward,
            text_thickness_um: None,
        }
    }

    #[test]
    fn aligned_measures_the_euclidean_distance() {
        let dim = base_dim(DimensionKind::Aligned { height: 2000 }, Point { x: 0, y: 0 }, Point { x: 3000, y: 4000 });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_um, 5000); // 3-4-5 triangle
        assert_eq!(g.text, "5");
    }

    #[test]
    fn aligned_horizontal_line_puts_the_crossbar_above_for_positive_height() {
        let dim = base_dim(DimensionKind::Aligned { height: 1000 }, Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 });
        let g = compute_dimension_geometry(&dim);
        // Two extension lines plus a crossbar (possibly split) plus two
        // 2-segment arrowheads -- at least 2 + 1 + 4 = 7 lines.
        assert!(g.lines.len() >= 7, "{} lines", g.lines.len());
        // The crossbar must sit at y = -1000 (one of the extension lines'
        // own far endpoint) for a positive height with this crate's
        // perp() convention -- whichever sign, it must be consistent and
        // off the feature line (y != 0).
        let crossbar_y: i64 = g.lines.iter().map(|(a, b)| if a.y == b.y && a.y != 0 { Some(a.y) } else { None }).find_map(|v| v).expect("a horizontal crossbar segment");
        assert_ne!(crossbar_y, 0);
    }

    #[test]
    fn aligned_negative_height_puts_the_crossbar_on_the_other_side() {
        let pos = compute_dimension_geometry(&base_dim(DimensionKind::Aligned { height: 1000 }, Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }));
        let neg = compute_dimension_geometry(&base_dim(DimensionKind::Aligned { height: -1000 }, Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }));
        let find_y = |g: &DimensionGeometry| g.lines.iter().find_map(|(a, b)| if a.y == b.y && a.y != 0 { Some(a.y) } else { None }).unwrap();
        assert_eq!(find_y(&pos), -find_y(&neg));
    }

    #[test]
    fn orthogonal_horizontal_ignores_the_y_delta_in_its_measurement() {
        let dim = base_dim(DimensionKind::Orthogonal { height: 1000, horizontal: true }, Point { x: 0, y: 0 }, Point { x: 5000, y: 9999 });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_um, 5000);
    }

    #[test]
    fn orthogonal_vertical_ignores_the_x_delta() {
        let dim = base_dim(DimensionKind::Orthogonal { height: 1000, horizontal: false }, Point { x: 0, y: 0 }, Point { x: 9999, y: 7000 });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_um, 7000);
    }

    #[test]
    fn radial_measures_the_radius_and_draws_a_center_cross() {
        let dim = base_dim(DimensionKind::Radial { leader_length: 500 }, Point { x: 0, y: 0 }, Point { x: 2000, y: 0 });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.measured_value_um, 2000);
        // The centre cross is the two segments straddling (0,0) with the
        // fixed arrow_length arm -- both must be present regardless of
        // the radius.
        let straddles_origin = |a: Point, b: Point| a.x == -b.x && a.y == -b.y && (a.x != 0 || a.y != 0);
        let cross_arms = g.lines.iter().filter(|(a, b)| straddles_origin(*a, *b)).count();
        assert_eq!(cross_arms, 2, "both arms of the centre cross must be present");
    }

    #[test]
    fn leader_places_an_arrow_at_start_and_a_line_toward_the_text() {
        let mut dim = base_dim(DimensionKind::Leader, Point { x: 0, y: 0 }, Point { x: 5000, y: 0 });
        dim.override_text = Some("note".into());
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "note");
        assert!(!g.lines.is_empty());
    }

    #[test]
    fn center_mark_has_no_text_and_two_perpendicular_arms() {
        let dim = base_dim(DimensionKind::Center, Point { x: 1000, y: 1000 }, Point { x: 1500, y: 1000 });
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
        let mut dim = base_dim(DimensionKind::Aligned { height: 1000 }, Point { x: 0, y: 0 }, Point { x: 1000, y: 0 });
        dim.prefix = "W=".into();
        dim.suffix = "!".into();
        dim.override_text = Some("TBD".into());
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "W=TBD!");
    }

    #[test]
    fn suppressing_trailing_zeros_drops_a_bare_decimal_point_too() {
        let dim = base_dim(DimensionKind::Aligned { height: 1000 }, Point { x: 0, y: 0 }, Point { x: 2000, y: 0 });
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "2");
    }

    #[test]
    fn units_format_adds_the_right_suffix() {
        let mut dim = base_dim(DimensionKind::Aligned { height: 1000 }, Point { x: 0, y: 0 }, Point { x: 1000, y: 0 });
        dim.units_format = DimensionUnitsFormat::BareSuffix;
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "1 mm");
        dim.units_format = DimensionUnitsFormat::ParenSuffix;
        let g = compute_dimension_geometry(&dim);
        assert_eq!(g.text, "1 (mm)");
    }

    #[test]
    fn translate_moves_both_feature_points() {
        let mut dim = base_dim(DimensionKind::Leader, Point { x: 0, y: 0 }, Point { x: 1000, y: 0 });
        translate_dimension(&mut dim, 500, -200);
        assert_eq!(dim.start, Point { x: 500, y: -200 });
        assert_eq!(dim.end, Point { x: 1500, y: -200 });
    }

    #[test]
    fn rotate_about_pivot_moves_a_point_90_degrees_clockwise() {
        let mut dim = base_dim(DimensionKind::Leader, Point { x: 1000, y: 0 }, Point { x: 1000, y: 0 });
        rotate_dimension(&mut dim, Point { x: 0, y: 0 }, 90_000);
        assert_eq!(dim.start, Point { x: 0, y: 1000 });
    }
}
