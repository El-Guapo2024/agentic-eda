//! The shapes a vector-graphics file hands the importer, and how the importer puts them on a board, a sheet or a library item.
//!
//! This is KiCad's `common/import_gfx` at 8303b2ad (10.99), split the way KiCad splits it:
//!
//! * a **plugin** reads a file into a list of [`ImportedShape`]s, all in millimetres, with the Y axis already pointing down
//!   ([`crate::dxf_import`] is `DXF_IMPORT_PLUGIN` and the part of dxflib it leans on, [`crate::svg_import`] is
//!   `SVG_IMPORT_PLUGIN` and the part of nanosvg it leans on). The list is `GRAPHICS_IMPORTER_BUFFER`'s `m_shapes`: [`Buffer`].
//! * an **importer** turns each shape into an item of the editor it is for. What the editors share is here: the scale, the offset and
//!   the line width every shape is mapped with ([`Mapper`], `GRAPHICS_IMPORTER::MapCoordinate` / `MapLineWidth`), the size check
//!   that runs before anything is added ([`prepare`], `GRAPHICS_IMPORTER_BUFFER::ImportTo`), how an arc given as a centre, a start and
//!   a sweep becomes three points ([`arc_points`], `GRAPHICS_IMPORTER_PCBNEW::AddArc`) and when a Bezier curve is a curve and when it
//!   is a line ([`spline_or_line`], `GRAPHICS_IMPORTER::setupSplineOrLine`). The editors' own items are made in `eda_ops`
//!   (`crates/ops/src/import_graphics.rs` for the board).
//!
//! # Units
//!
//! The shapes are in millimetres as `f64`; the scale and offset are applied while mapping, and the result is rounded to the
//! design's integer micrometre ([`Mapper::iu_per_mm`] is 1000). KiCad's own unit is the nanometre, so its 32-bit limits
//! (`std::numeric_limits<int>::max()` nanometres, 2147 mm) are kept in nanometres here too ([`KICAD_IU_PER_MM`]): a graphic a
//! KiCad could not hold is refused with KiCad's message.

use eda_model::ir::Point;
use eda_model::units::Nm;
use serde::Serialize;
use std::ops::{Add, Mul, Sub};

/// KiCad's internal unit per millimetre (`pcbIUScale.mmToIU( 1.0 )`): the 32-bit limits are in these.
pub const KICAD_IU_PER_MM: f64 = 1_000_000.0;

/// `GRAPHICS_IMPORTER::DEFAULT_LINE_WIDTH_DFX`: the line width, in mm, an importer has until a dialog sets another.
pub const DEFAULT_LINE_WIDTH_MM: f64 = 1.0;

/// `ARC_HIGH_DEF` (0.005 mm) in nanometres: the accuracy `setupSplineOrLine` flattens a curve with to see whether it is one.
pub const ARC_HIGH_DEF_NM: Nm = Nm::new(5_000);

// ------------------------------------------------------------------------------------------------------------------ geometry

/// A point or a vector in millimetres (`VECTOR2D`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

impl V2 {
    pub const fn new(x: f64, y: f64) -> V2 {
        V2 { x, y }
    }

    /// `VECTOR2::EuclideanNorm`.
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, s: f64) -> V2 {
        V2::new(self.x * s, self.y * s)
    }
}

/// The upper-left 2x2 of a `MATRIX3x3D` the importer uses on the plane (`IMPORTED_SHAPE::Transform` never reads or sets the
/// third row or column): `[[a, b], [c, d]]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat2 {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}

impl Mat2 {
    pub const IDENTITY: Mat2 = Mat2 { a: 1.0, b: 0.0, c: 0.0, d: 1.0 };

    /// `MATRIX3x3::SetRotation`: `[[cos, -sin], [sin, cos]]`.
    pub fn rotation(angle_rad: f64) -> Mat2 {
        let (s, c) = angle_rad.sin_cos();
        Mat2 { a: c, b: -s, c: s, d: c }
    }

    /// `MATRIX3x3::SetScale`: a diagonal.
    pub fn scale(sx: f64, sy: f64) -> Mat2 {
        Mat2 { a: sx, b: 0.0, c: 0.0, d: sy }
    }

    /// `self * o`: `o` first.
    pub fn mul(self, o: Mat2) -> Mat2 {
        Mat2 { a: self.a * o.a + self.b * o.c, b: self.a * o.b + self.b * o.d, c: self.c * o.a + self.d * o.c, d: self.c * o.b + self.d * o.d }
    }

    pub fn apply(self, p: V2) -> V2 {
        V2::new(self.a * p.x + self.b * p.y, self.c * p.x + self.d * p.y)
    }
}

/// `RotatePoint( VECTOR2D&, centre, angle )` (`libs/kimath/src/trigo.cpp`): a positive angle turns counter-clockwise on a screen
/// whose Y axis points down, so in the plain `(x, y)` numbers it is `x' = x cos + y sin`, `y' = y cos - x sin`. The right angles are
/// exact, as KiCad's are.
pub fn rotate_point(p: V2, centre: V2, angle_deg: f64) -> V2 {
    let o = p - centre;
    let mut a = angle_deg % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    let r = if a == 0.0 {
        o
    } else if a == 90.0 {
        V2::new(o.y, -o.x)
    } else if a == 180.0 {
        V2::new(-o.x, -o.y)
    } else if a == 270.0 {
        V2::new(-o.y, o.x)
    } else {
        let (s, c) = a.to_radians().sin_cos();
        V2::new(o.y * s + o.x * c, o.y * c - o.x * s)
    };
    r + centre
}

/// A box in millimetres that may still be empty (`BOX2D`, `Merge`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BBox {
    pub min: V2,
    pub max: V2,
}

impl BBox {
    pub fn empty() -> Option<BBox> {
        None
    }

    pub fn merge_point(b: &mut Option<BBox>, p: V2) {
        match b {
            None => *b = Some(BBox { min: p, max: p }),
            Some(r) => {
                r.min = V2::new(r.min.x.min(p.x), r.min.y.min(p.y));
                r.max = V2::new(r.max.x.max(p.x), r.max.y.max(p.y));
            }
        }
    }

    pub fn merge(b: &mut Option<BBox>, o: BBox) {
        Self::merge_point(b, o.min);
        Self::merge_point(b, o.max);
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
}

// ----------------------------------------------------------------------------------------------------------------------- shapes

/// `LINE_STYLE` as far as an import sets it (`SVG_IMPORT_PLUGIN::Import` reads a dash array into one of these).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineStyle {
    /// `LINE_STYLE::DEFAULT`: what a DXF line has.
    #[default]
    Default,
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

/// `KIGFX::COLOR4D`, each channel 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

/// `IMPORTED_STROKE`: the width in millimetres (not yet scaled), the dash style and the colour. A width of 0 or less takes the
/// importer's default line width; -1 is "no stroke" (a filled shape with no outline).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Stroke {
    pub width: f64,
    pub style: LineStyle,
    pub color: Option<Rgba>,
}

impl Stroke {
    /// `IMPORTED_STROKE( aWidth )`.
    pub fn width(width: f64) -> Stroke {
        Stroke { width, style: LineStyle::Default, color: None }
    }
}

/// `GR_TEXT_H_ALIGN_T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HJustify {
    #[default]
    Left,
    Center,
    Right,
}

/// `GR_TEXT_V_ALIGN_T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VJustify {
    Top,
    Center,
    #[default]
    Bottom,
}

/// `GRAPHICS_IMPORTER::POLY_FILL_RULE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// `IMPORTED_SHAPE` and its six subclasses: one thing the plugin found in the file, in millimetres.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportedShape {
    /// `IMPORTED_LINE`.
    Line { start: V2, end: V2, stroke: Stroke },
    /// `IMPORTED_CIRCLE`.
    Circle { center: V2, radius: f64, stroke: Stroke, filled: bool, fill_color: Option<Rgba> },
    /// `IMPORTED_ARC`: from `start` about `center` through `angle_deg`, in `AddArc`'s sense (see [`arc_points`]).
    Arc { center: V2, start: V2, angle_deg: f64, stroke: Stroke },
    /// `IMPORTED_POLYGON`. `parent` is the SVG shape the path belongs to (`m_parentShapeIndex`): the paths of one shape are
    /// combined by [`Buffer::postprocess_nested_polygons`] under that shape's fill rule.
    Polygon { pts: Vec<V2>, stroke: Stroke, filled: bool, fill_color: Option<Rgba>, parent: Option<usize> },
    /// `IMPORTED_TEXT`: `height` and `width` (of a character) and `thickness` in millimetres, `orientation_deg` counter-clockwise.
    Text { origin: V2, text: String, height: f64, width: f64, thickness: f64, orientation_deg: f64, h: HJustify, v: VJustify },
    /// `IMPORTED_SPLINE`: a cubic Bezier curve.
    Spline { start: V2, c1: V2, c2: V2, end: V2, stroke: Stroke },
}

impl ImportedShape {
    /// `IMPORTED_SHAPE::Transform( aTransform, aTranslation )`: how a block's shapes are placed by an INSERT. An arc keeps its
    /// sweep (only its start and centre move, as in KiCad); a circle's radius is the length of the turned and scaled radius.
    /// A text is the one place this departs from KiCad: KiCad multiplies the text's (width, height) by the matrix, which sets a
    /// text on its side inside a block that is turned; here the size follows the scale of the matrix and the text turns with it.
    pub fn transform(&mut self, m: Mat2, t: V2) {
        let at = |p: V2| m.apply(p) + t;
        match self {
            ImportedShape::Line { start, end, .. } => {
                *start = at(*start);
                *end = at(*end);
            }
            ImportedShape::Circle { center, radius, .. } => {
                *center = at(*center);
                *radius = m.apply(V2::new(*radius, 0.0)).norm();
            }
            ImportedShape::Arc { center, start, .. } => {
                *start = at(*start);
                *center = at(*center);
            }
            ImportedShape::Polygon { pts, .. } => pts.iter_mut().for_each(|p| *p = at(*p)),
            ImportedShape::Text { origin, height, width, orientation_deg, .. } => {
                *origin = at(*origin);
                let sx = m.apply(V2::new(1.0, 0.0)).norm();
                let sy = m.apply(V2::new(0.0, 1.0)).norm();
                *width *= sx;
                *height *= sy;
                // The block's turn, in the sense the text's own angle is measured (counter-clockwise on the drawing).
                let turn = m.apply(V2::new(1.0, 0.0));
                *orientation_deg -= turn.y.atan2(turn.x).to_degrees();
            }
            ImportedShape::Spline { start, c1, c2, end, .. } => {
                for p in [start, c1, c2, end] {
                    *p = at(*p);
                }
            }
        }
    }

    /// `IMPORTED_SHAPE::GetBoundingBox`: a box that holds the shape, for the size checks. (KiCad's box for an arc is not the arc's; the
    /// arc's own points are used here, which is what it was written to compute.)
    pub fn bbox(&self) -> Option<BBox> {
        let mut b = None;
        match self {
            ImportedShape::Line { start, end, .. } => {
                BBox::merge_point(&mut b, *start);
                BBox::merge_point(&mut b, *end);
            }
            ImportedShape::Circle { center, radius, .. } => {
                BBox::merge_point(&mut b, *center - V2::new(*radius, *radius));
                BBox::merge_point(&mut b, *center + V2::new(*radius, *radius));
            }
            ImportedShape::Arc { center, start, angle_deg, .. } => {
                // The points the sweep passes through, about every 5 degrees as `IMPORTED_ARC::GetBoundingBox` samples them.
                let steps = (angle_deg.abs() / 5.0).ceil().max(1.0) as i32;
                for i in 0..=steps {
                    BBox::merge_point(&mut b, rotate_point(*start, *center, -*angle_deg * (i as f64 / steps as f64)));
                }
            }
            ImportedShape::Polygon { pts, .. } => pts.iter().for_each(|p| BBox::merge_point(&mut b, *p)),
            ImportedShape::Text { origin, text, height, width, .. } => {
                BBox::merge_point(&mut b, *origin);
                BBox::merge_point(&mut b, *origin + V2::new(*width * text.chars().count() as f64, *height));
            }
            ImportedShape::Spline { start, c1, c2, end, .. } => {
                for p in [start, c1, c2, end] {
                    BBox::merge_point(&mut b, *p);
                }
            }
        }
        b
    }
}

/// What a plugin read: the shapes, the messages it reported for what it could not handle, and the size of the picture.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Imported {
    pub shapes: Vec<ImportedShape>,
    /// `GRAPHICS_IMPORT_PLUGIN::GetMessages`, one per report (a repeated report is repeated).
    pub messages: Vec<String>,
    /// The picture's limits as the plugin saw them (`GetImageBBox`), mm; `None` for an empty one.
    pub bbox: Option<BBox>,
}

impl Imported {
    /// `GetImageWidth`.
    pub fn width_mm(&self) -> f64 {
        self.bbox.map_or(0.0, |b| b.width())
    }

    /// `GetImageHeight`.
    pub fn height_mm(&self) -> f64 {
        self.bbox.map_or(0.0, |b| b.height())
    }

    /// The messages once each, with how many times they came: what a dialog lists.
    pub fn message_counts(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = Vec::new();
        for m in &self.messages {
            match out.iter_mut().find(|(t, _)| t == m) {
                Some((_, n)) => *n += 1,
                None => out.push((m.clone(), 1)),
            }
        }
        out
    }
}

/// `GRAPHICS_IMPORTER_BUFFER`: the shapes a plugin collects, before anything is made of them.
#[derive(Debug, Clone, Default)]
pub struct Buffer {
    pub shapes: Vec<ImportedShape>,
    /// `m_shapeFillRules`: one per [`Buffer::new_shape`].
    pub fill_rules: Vec<FillRule>,
}

impl Buffer {
    /// `NewShape`: what follows belongs to a new SVG shape that fills with `rule`.
    pub fn new_shape(&mut self, rule: FillRule) {
        self.fill_rules.push(rule);
    }

    pub fn add_line(&mut self, start: V2, end: V2, stroke: Stroke) {
        self.shapes.push(ImportedShape::Line { start, end, stroke });
    }

    pub fn add_circle(&mut self, center: V2, radius: f64, stroke: Stroke, filled: bool) {
        self.shapes.push(ImportedShape::Circle { center, radius, stroke, filled, fill_color: None });
    }

    pub fn add_arc(&mut self, center: V2, start: V2, angle_deg: f64, stroke: Stroke) {
        self.shapes.push(ImportedShape::Arc { center, start, angle_deg, stroke });
    }

    pub fn add_polygon(&mut self, pts: Vec<V2>, stroke: Stroke, filled: bool, fill_color: Option<Rgba>) {
        // `SetParentShapeIndex( m_shapeFillRules.size() - 1 )`: -1 when no shape was begun.
        let parent = self.fill_rules.len().checked_sub(1);
        self.shapes.push(ImportedShape::Polygon { pts, stroke, filled, fill_color, parent });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_text(&mut self, origin: V2, text: String, height: f64, width: f64, thickness: f64, orientation_deg: f64, h: HJustify, v: VJustify) {
        self.shapes.push(ImportedShape::Text { origin, text, height, width, thickness, orientation_deg, h, v });
    }

    pub fn add_spline(&mut self, start: V2, c1: V2, c2: V2, end: V2, stroke: Stroke) {
        self.shapes.push(ImportedShape::Spline { start, c1, c2, end, stroke });
    }

    pub fn add_shape(&mut self, shape: ImportedShape) {
        self.shapes.push(shape);
    }
}

// -------------------------------------------------------------------------------------------------------------------- mapping

/// What an import is mapped with: `GRAPHICS_IMPORTER`'s scale, import offset and line width, and the unit the editor's items are in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapper {
    /// `GetScale`: the import scale (the same on both axes, as the dialogs set it).
    pub scale: f64,
    /// `GetImportOffsetMM`: added after the scale, in millimetres.
    pub offset_mm: V2,
    /// `GetLineWidthMM`: the width a stroke of no width takes, in millimetres (before the scale).
    pub line_width_mm: f64,
    /// The editor's units per millimetre (`m_millimeterToIu`): [`eda_model::units::PCB_IU_PER_MM`] for the board's and the schematic's
    /// nanometres, 10000 for a library symbol's schematic units (100 nm, [`eda_model::units::SCH_IU_NM`]). [`Mapper::coord`] and
    /// [`Mapper::line_width`] answer in that unit, so they are counts (`i64`), not lengths.
    pub iu_per_mm: f64,
}

impl Mapper {
    /// `GRAPHICS_IMPORTER_PCBNEW::MapCoordinate` (and the schematic's): the scale, then the offset, then the editor's unit, rounded.
    pub fn coord(&self, p: V2) -> (i64, i64) {
        let c = p * self.scale + self.offset_mm;
        ((c.x * self.iu_per_mm).round() as i64, (c.y * self.iu_per_mm).round() as i64)
    }

    /// `ImportScalingFactor`: the scale times the editor's unit.
    pub fn scaling_factor(&self) -> f64 {
        self.scale * self.iu_per_mm
    }

    /// `GRAPHICS_IMPORTER_PCBNEW::MapLineWidth`: a width in mm to the editor's unit, truncated; a width of nothing is the default
    /// line width. (The scale applies to the default too.)
    pub fn line_width(&self, width_mm: f64) -> i64 {
        let scale = self.scaling_factor().abs();
        if width_mm <= 0.0 {
            (self.line_width_mm * scale) as i64
        } else {
            (width_mm * scale) as i64
        }
    }
}

/// The size check that runs before a buffer is imported, and the offset it sets (`GRAPHICS_IMPORTER_BUFFER::ImportTo`).
///
/// A graphic KiCad's 32-bit nanometres cannot hold (wider or taller than 2147 mm once scaled) is refused with the maximum scale that
/// would fit; a graphic placed where it would run off that range gets its offset moved back inside, with a message. Returns the
/// messages to report and whether the shapes may be imported.
pub fn prepare(shapes: &[ImportedShape], mapper: &mut Mapper) -> (bool, Vec<String>) {
    let mut messages = Vec::new();
    let mut bbox: Option<BBox> = None;
    for s in shapes {
        if let Some(b) = s.bbox() {
            BBox::merge(&mut bbox, b);
        }
    }
    let Some(b) = bbox else { return (true, messages) };
    let scaled = BBox { min: b.min * mapper.scale, max: b.max * mapper.scale };
    let int_max = i32::MAX as f64;
    let int_min = i32::MIN as f64;
    if scaled.width() * KICAD_IU_PER_MM > int_max || scaled.height() * KICAD_IU_PER_MM > int_max {
        let scale_factor = int_max / (KICAD_IU_PER_MM + 100.0);
        let max_scale = (scale_factor / scaled.width()).max(scale_factor / scaled.height());
        messages.push(format!("Imported graphic is too large. Maximum scale is {max_scale:.6}"));
        return (false, messages);
    }
    if mapper.offset_mm == V2::new(0.0, 0.0) {
        // "They haven't set the import offset, so we set it to the bounding box origin to keep the graphics in the KiCad drawing area."
        if scaled.max.x * KICAD_IU_PER_MM > int_max || scaled.max.y * KICAD_IU_PER_MM > int_max || scaled.min.x * KICAD_IU_PER_MM < int_min || scaled.min.y * KICAD_IU_PER_MM < int_min {
            mapper.offset_mm = V2::new(-scaled.min.x, -scaled.min.y);
        }
    } else {
        let total = mapper.scale * KICAD_IU_PER_MM;
        let max_x = (mapper.offset_mm.x + b.max.x) * total;
        let max_y = (mapper.offset_mm.y + b.max.y) * total;
        let min_x = (mapper.offset_mm.x + b.min.x) * total;
        let min_y = (mapper.offset_mm.y + b.min.y) * total;
        let mut offset = mapper.offset_mm;
        let mut adjusted = false;
        if max_x >= int_max {
            offset.x -= (max_x - int_max + 100.0) / total;
            adjusted = true;
        } else if min_x <= int_min {
            offset.x -= (min_x - int_min - 100.0) / total;
            adjusted = true;
        }
        if max_y >= int_max {
            offset.y -= (max_y - int_max + 100.0) / total;
            adjusted = true;
        } else if min_y <= int_min {
            offset.y -= (min_y - int_min - 100.0) / total;
            adjusted = true;
        }
        if adjusted {
            messages.push(format!("Import offset adjusted to ({:.6}, {:.6}) to fit within numeric limits", offset.x, offset.y));
            mapper.offset_mm = offset;
        }
    }
    (true, messages)
}

/// The three points of an imported arc (`GRAPHICS_IMPORTER_PCBNEW::AddArc`): `start`, the middle of the sweep and the end, from a
/// `centre`, a `start` and the sweep `angle_deg` in the importer's sense (a negative angle is the counter-clockwise sweep of a DXF
/// arc). Worked out in floating point, before the coordinates are rounded, "to avoid rounding errors when operating in integer
/// space".
pub fn arc_points(center: V2, start: V2, angle_deg: f64) -> (V2, V2, V2) {
    let end = rotate_point(start, center, -angle_deg);
    let mid = rotate_point(start, center, -angle_deg / 2.0);
    (start, mid, end)
}

// -------------------------------------------------------------------------------------------------------------------- splines

/// What `GRAPHICS_IMPORTER::setupSplineOrLine` makes of a Bezier curve whose points are already mapped to the editor's unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplineOrLine {
    /// A curve: it bends.
    Curve,
    /// "The spline is degenerated (i.e. a segment)": a line from its start to its end.
    Line,
    /// A segment shorter than 20 nanometres: nothing.
    Skip,
}

/// `SEG::mutualDistanceSquared` + `SEG::ApproxCollinear( other, 1 )` on integer points: are the end points of the shorter segment
/// within a unit of the line through the longer one?
fn approx_collinear(a: ((i64, i64), (i64, i64)), b: ((i64, i64), (i64, i64))) -> bool {
    let sq_len = |s: ((i64, i64), (i64, i64))| {
        let (dx, dy) = ((s.1 .0 - s.0 .0) as i128, (s.1 .1 - s.0 .1) as i128);
        dx * dx + dy * dy
    };
    let (a, b) = if sq_len(a) < sq_len(b) { (b, a) } else { (a, b) };
    let p = a.0 .1 as i128 - a.1 .1 as i128;
    let q = a.1 .0 as i128 - a.0 .0 as i128;
    let r = -p * a.0 .0 as i128 - q * a.0 .1 as i128;
    let l = p * p + q * q;
    if l == 0 {
        return false;
    }
    let det1 = p * b.0 .0 as i128 + q * b.0 .1 as i128 + r;
    let det2 = p * b.1 .0 as i128 + q * b.1 .1 as i128 + r;
    let d1 = det1 * det1 / l;
    let d2 = det2 * det2 / l;
    d1 <= 1 && d2 <= 1
}

/// `GRAPHICS_IMPORTER::setupSplineOrLine`: a curve whose control points lie on its chord, or that flattens to nothing but its two
/// end points at [`ARC_HIGH_DEF_NM`], is a segment; a segment shorter than 20 nm is dropped. The points are nanometres.
pub fn spline_or_line(start: Point, c1: Point, c2: Point, end: Point) -> SplineOrLine {
    // `ApproxCollinear` is KiCad's integer arithmetic on the points' counts of nanometres
    let t = |p: Point| (p.x.get(), p.y.get());
    let s_e = (t(start), t(end));
    let mut degenerate = approx_collinear(s_e, (t(start), t(c1))) && approx_collinear(s_e, (t(end), t(c2)));
    if !degenerate {
        degenerate = eda_model::bezier::bezier_polyline(start, c1, c2, end, ARC_HIGH_DEF_NM).len() <= 2;
    }
    if degenerate {
        // "segment smaller than MIN_SEG_LEN_ACCEPTABLE_NM nanometers are skipped"
        const MIN_SEG_LEN_ACCEPTABLE_NM: Nm = Nm::new(20);
        // `SEG::Length()`: `EuclideanNorm()`, rounded to an integer
        if Nm::kiround((end.x - start.x).as_f64().hypot((end.y - start.y).as_f64())) < MIN_SEG_LEN_ACCEPTABLE_NM {
            SplineOrLine::Skip
        } else {
            SplineOrLine::Line
        }
    } else {
        SplineOrLine::Curve
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::mm;

    fn mapper() -> Mapper {
        Mapper { scale: 1.0, offset_mm: V2::new(0.0, 0.0), line_width_mm: 0.2, iu_per_mm: 1000.0 }
    }

    #[test]
    fn a_coordinate_is_scaled_then_offset_then_put_in_the_design_unit() {
        let m = Mapper { scale: 2.0, offset_mm: V2::new(10.0, -5.0), ..mapper() };
        assert_eq!(m.coord(V2::new(1.5, 2.0)), (13_000, -1_000));
    }

    #[test]
    fn a_line_width_takes_the_scale_and_nothing_takes_the_default_width() {
        let m = Mapper { scale: 2.0, ..mapper() };
        assert_eq!(m.line_width(0.25), 500);
        assert_eq!(m.line_width(0.0), 400, "the default width scales too");
        assert_eq!(m.line_width(-1.0), 400);
    }

    #[test]
    fn the_right_angles_of_a_rotation_are_exact() {
        let c = V2::new(1.0, 1.0);
        assert_eq!(rotate_point(V2::new(3.0, 1.0), c, 90.0), V2::new(1.0, -1.0));
        assert_eq!(rotate_point(V2::new(3.0, 1.0), c, -90.0), V2::new(1.0, 3.0));
        assert_eq!(rotate_point(V2::new(3.0, 1.0), c, 180.0), V2::new(-1.0, 1.0));
    }

    #[test]
    fn an_arc_with_a_negative_sweep_runs_counter_clockwise_on_the_screen() {
        // A quarter turn about the origin from (10, 0): the sweep -90 takes +x to -y in the numbers, which is up on a screen whose
        // Y axis points down, so counter-clockwise as the DXF arc it stands for.
        let (s, m, e) = arc_points(V2::new(0.0, 0.0), V2::new(10.0, 0.0), -90.0);
        assert_eq!(s, V2::new(10.0, 0.0));
        assert!((e.x).abs() < 1e-9 && (e.y + 10.0).abs() < 1e-9, "{e:?}");
        let half = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
        assert!((m.x - half).abs() < 1e-9 && (m.y + half).abs() < 1e-9, "{m:?}");
    }

    #[test]
    fn a_graphic_too_large_for_kicad_is_refused_with_the_scale_that_would_fit() {
        let mut m = mapper();
        let big = ImportedShape::Line { start: V2::new(0.0, 0.0), end: V2::new(5_000.0, 0.0), stroke: Stroke::width(0.0) };
        let (ok, msgs) = prepare(&[big], &mut m);
        assert!(!ok);
        assert!(msgs[0].starts_with("Imported graphic is too large. Maximum scale is "), "{msgs:?}");
    }

    #[test]
    fn a_graphic_far_from_the_origin_with_no_offset_is_moved_back_to_the_origin() {
        let mut m = mapper();
        let far = ImportedShape::Line { start: V2::new(3_000.0, 3_000.0), end: V2::new(3_010.0, 3_000.0), stroke: Stroke::width(0.0) };
        let (ok, _) = prepare(&[far], &mut m);
        assert!(ok);
        assert_eq!(m.offset_mm, V2::new(-3_000.0, -3_000.0));
    }

    #[test]
    fn a_curve_on_its_chord_is_a_line_and_a_bent_one_is_a_curve() {
        let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        assert_eq!(spline_or_line(p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(3.0, 0.0)), SplineOrLine::Line);
        assert_eq!(spline_or_line(p(0.0, 0.0), p(0.0, 1.0), p(3.0, 1.0), p(3.0, 0.0)), SplineOrLine::Curve);
        let q = p(0.005, 0.005);
        assert_eq!(spline_or_line(q, q, q, q), SplineOrLine::Skip);
    }

    #[test]
    fn a_block_shape_turned_and_moved_by_an_insert() {
        let mut line = ImportedShape::Line { start: V2::new(1.0, 0.0), end: V2::new(2.0, 0.0), stroke: Stroke::width(0.1) };
        line.transform(Mat2::rotation(std::f64::consts::FRAC_PI_2).mul(Mat2::scale(2.0, 2.0)), V2::new(10.0, 10.0));
        let ImportedShape::Line { start, end, .. } = line else { unreachable!() };
        assert!((start.x - 10.0).abs() < 1e-9 && (start.y - 12.0).abs() < 1e-9, "{start:?}");
        assert!((end.x - 10.0).abs() < 1e-9 && (end.y - 14.0).abs() < 1e-9, "{end:?}");
    }
}
