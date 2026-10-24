//! A footprint's graphics and texts: the silkscreen outline and pin-1 mark, the fab body, the assembly and user-layer drawings, the
//! `${REFERENCE}` on `F.Fab` -- everything a `.kicad_mod` holds besides its pads, its courtyard, its fields and its 3D models.
//!
//! KiCad keeps these on the `FOOTPRINT` as `PCB_SHAPE`s (`fp_line`, `fp_arc`, `fp_circle`, `fp_rect`, `fp_poly`, `fp_curve`) and
//! `PCB_TEXT`s (`fp_text user`), in the footprint's own frame, and moves, turns and flips them with it. Here a placed footprint is a
//! pose ([`FootprintInstance`]) of a definition ([`crate::Footprint`]) shared by every instance, so the art is part of the definition
//! and lives in the frame of the pads (`Pad::at`): the footprint as seen from the top, un-rotated, +x right, +y down. A move, a turn or
//! a flip changes the pose and nothing here; [`FpShape::placed`] and [`FpText::placed`] take an item to the board.
//!
//! # Frame and layers
//!
//! Layers are named as for a top-side footprint: `F.SilkS` is the silkscreen on the footprint's own side, `B.SilkS` the one on the other
//! side. A bottom-side instance draws them the other way round ([`layer_on_side`]: `PCB_TEXT::Flip`, `PCB_SHAPE::Flip` swap `F.` and `B.`
//! on a side-specific layer), mirrors x like a pad ([`crate::footprint::to_board`]) and, for a text, turns the sense of its angle and toggles
//! its mirror flag. User layers (`Dwgs.User`, `User.1` ...) are on both sides.
//!
//! A footprint's own `Edge.Cuts` (a slot, a card edge's notch) are art too, as in KiCad (`FOOTPRINT::GraphicalItems`): they move, turn and flip with it, and the
//! board outline takes them in with `BuildBoardPolygonOutlines`' footprint pass (`eda_drc::outline::board_outline_with`).
//!
//! Not in the art, because the footprint already has a home for them: the courtyard (`F.CrtYd`/`B.CrtYd`, [`crate::Footprint::courtyard_outlines`]),
//! the Reference, the Value and the user fields
//! ([`crate::fp_edit::FootprintEdit`]).
//!
//! # Texts
//!
//! A text is a [`FieldLayout`] (the same attributes a field has, in the same frame, with the same sense for angles) and the string it
//! shows. The string may use text variables: [`resolve_text_vars`].

use serde::{Deserialize, Serialize};

use crate::footprint::to_board;
use crate::fp_edit::FieldLayout;
use crate::ir::{FootprintInstance, Millideg, Point, Shape, Side, Text, TextJustify, Nm};

// ------------------------------------------------------------------------------------------------------------------------- layers

/// The technical layers of one side (`LSET::FrontTechMask`, `BackTechMask`) and copper, without the side prefix.
const SIDE_LAYER_NAMES: [&str; 7] = ["Cu", "SilkS", "Mask", "Paste", "Adhes", "CrtYd", "Fab"];

/// `LSET::SideSpecificMask()`: the layers that belong to one side of the board -- copper, silkscreen, mask, paste, adhesive, courtyard
/// and fab. The user layers do not.
pub fn is_side_specific(layer: &str) -> bool {
    for prefix in ["F.", "B."] {
        if let Some(rest) = layer.strip_prefix(prefix) {
            return SIDE_LAYER_NAMES.contains(&rest);
        }
    }
    layer.starts_with("In") && layer.ends_with(".Cu")
}

/// `BOARD::FlipLayer`: the layer on the other side of the board. A layer that belongs to neither side, and an inner copper layer (whose
/// partner depends on how many copper layers the board has), are their own opposite here.
pub fn flip_layer(layer: &str) -> String {
    for (from, to) in [("F.", "B."), ("B.", "F.")] {
        if let Some(rest) = layer.strip_prefix(from) {
            if SIDE_LAYER_NAMES.contains(&rest) {
                return format!("{to}{rest}");
            }
        }
    }
    layer.to_string()
}

/// The board layer a layer of the definition (named as for a top-side footprint) is on when the footprint is on `side`.
pub fn layer_on_side(layer: &str, side: Side) -> String {
    if side == Side::Bottom {
        flip_layer(layer)
    } else {
        layer.to_string()
    }
}

/// Is `layer` one the art keeps? Every layer but the courtyard, which has a home of its own (module doc); a footprint's Edge.Cuts are its art.
pub fn is_art_layer(layer: &str) -> bool {
    !(layer == "F.CrtYd" || layer == "B.CrtYd")
}

// ------------------------------------------------------------------------------------------------------------------------- shapes

/// `STROKE_PARAMS`' line style (`(stroke (type ..))`). `Unset` is the file's `default`, which a painter draws as solid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineStyle {
    #[serde(rename = "default")]
    Unset,
    #[default]
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

impl LineStyle {
    /// The word `(stroke (type ..))` writes.
    pub fn token(self) -> &'static str {
        match self {
            LineStyle::Unset => "default",
            LineStyle::Solid => "solid",
            LineStyle::Dash => "dash",
            LineStyle::Dot => "dot",
            LineStyle::DashDot => "dash_dot",
            LineStyle::DashDotDot => "dash_dot_dot",
        }
    }

    /// Inverse of [`LineStyle::token`]; `None` for a word KiCad does not have.
    pub fn from_token(t: &str) -> Option<LineStyle> {
        Some(match t {
            "default" => LineStyle::Unset,
            "solid" => LineStyle::Solid,
            "dash" => LineStyle::Dash,
            "dot" => LineStyle::Dot,
            "dash_dot" => LineStyle::DashDot,
            "dash_dot_dot" => LineStyle::DashDotDot,
            _ => return None,
        })
    }
}

/// The hatched fills of `FILL_T` (`(fill hatch|reverse_hatch|cross_hatch)`); the solid fill is [`Shape`]'s own `filled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hatch {
    Hatch,
    ReverseHatch,
    CrossHatch,
}

impl Hatch {
    pub fn token(self) -> &'static str {
        match self {
            Hatch::Hatch => "hatch",
            Hatch::ReverseHatch => "reverse_hatch",
            Hatch::CrossHatch => "cross_hatch",
        }
    }

    /// Inverse of [`Hatch::token`]; `None` for a word that is not a hatched fill.
    pub fn from_token(token: &str) -> Option<Hatch> {
        match token {
            "hatch" => Some(Hatch::Hatch),
            "reverse_hatch" => Some(Hatch::ReverseHatch),
            "cross_hatch" => Some(Hatch::CrossHatch),
            _ => None,
        }
    }

    /// The slopes `EDA_SHAPE::UpdateHatching` hatches with: -1 for a hatch (lines from the top left to the bottom right in KiCad's y-down frame), +1 for a reverse hatch, both for a cross-hatch.
    pub fn slopes(self) -> &'static [f64] {
        match self {
            Hatch::Hatch => &[-1.0],
            Hatch::ReverseHatch => &[1.0],
            Hatch::CrossHatch => &[1.0, -1.0],
        }
    }
}

fn is_solid(s: &LineStyle) -> bool {
    *s == LineStyle::Solid
}
fn is_zero(v: &Nm) -> bool {
    *v == 0
}

/// One `PCB_SHAPE` of a footprint. The geometry, the layer, the stroke width and the solid fill are a [`Shape`] (its `id` is not used: art
/// items are not addressed); what `Shape` has no field for is beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FpShape {
    /// Points in the footprint's frame; `layer` as for a top-side footprint. Polygons keep their points; an `(arc ..)` inside a polygon's `(pts ..)` is
    /// flattened by the reader.
    pub shape: Shape,
    /// `(stroke (type ..))`.
    #[serde(default, skip_serializing_if = "is_solid")]
    pub style: LineStyle,
    /// A hatched fill (`shape.filled` is then false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hatch: Option<Hatch>,
    /// A rectangle's corner radius (`(radius ..)`), 0 for square corners.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[serde(deserialize_with = "crate::units::de")]
    pub radius: Nm,
    /// All the layers of a shape that is on several (`(layers "F.Cu" "F.Mask")`), the first being `shape`'s own; empty for the shape on one layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<String>,
    /// `(solder_mask_margin ..)` of a graphic on a copper layer that has a mask opening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub mask_margin: Option<Nm>,
    /// KiCad 10's line endings of a line, an arc or a curve (`(start_shape ..)`, `(end_shape ..)`): see [`crate::line_ending`]. Additive.
    #[serde(default, skip_serializing_if = "crate::line_ending::LineEndings::is_none")]
    pub endings: crate::line_ending::LineEndings,
}

impl FpShape {
    /// A solid-stroked shape with nothing else about it.
    pub fn new(shape: Shape) -> Self {
        FpShape { shape, style: LineStyle::Solid, hatch: None, radius: 0, layers: Vec::new(), mask_margin: None, endings: Default::default() }
    }

    pub fn layer(&self) -> &str {
        self.shape.layer()
    }

    /// Every layer the shape is on: `layers`, or its one.
    pub fn all_layers(&self) -> Vec<&str> {
        if self.layers.is_empty() {
            vec![self.shape.layer()]
        } else {
            self.layers.iter().map(String::as_str).collect()
        }
    }

    /// Is the shape on `layer`? (Layers as for a top-side footprint.)
    pub fn on_layer(&self, layer: &str) -> bool {
        self.all_layers().contains(&layer)
    }

    /// Any fill at all (`EDA_SHAPE::IsAnyFill`).
    pub fn is_any_fill(&self) -> bool {
        self.shape.is_filled() || self.hatch.is_some()
    }

    /// The same shape in board space for a footprint at `fp`: every point through [`to_board`] (a bottom-side footprint mirrors x, then it turns
    /// and moves), the layers on the footprint's side ([`layer_on_side`]). A rectangle the footprint's turn leaves tilted, and one with rounded
    /// corners, becomes a polygon (`PCB_SHAPE::Rotate`; the corners tessellated as `ROUNDRECT::TransformToPolygon` does).
    pub fn placed(&self, fp: &FootprintInstance) -> FpShape {
        let at = |p: Point| to_board(fp, (p.x, p.y));
        let square_turn = fp.rot % 90_000 == 0;
        let shape = match &self.shape {
            Shape::Segment { id, layer, stroke_width, filled, start, end } => Shape::Segment { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, start: at(*start), end: at(*end) },
            Shape::Arc { id, layer, stroke_width, filled, start, mid, end } => Shape::Arc { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, start: at(*start), mid: at(*mid), end: at(*end) },
            Shape::Circle { id, layer, stroke_width, filled, center, end } => Shape::Circle { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, center: at(*center), end: at(*end) },
            Shape::Polygon { id, layer, stroke_width, filled, pts } => Shape::Polygon { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, pts: pts.iter().map(|p| at(*p)).collect() },
            Shape::Bezier { id, layer, stroke_width, filled, start, c1, c2, end } => {
                Shape::Bezier { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, start: at(*start), c1: at(*c1), c2: at(*c2), end: at(*end) }
            }
            Shape::Rect { id, layer, stroke_width, filled, start, end } => {
                if self.radius > 0 {
                    Shape::Polygon { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, pts: rounded_rect_points(*start, *end, self.radius).into_iter().map(at).collect() }
                } else if square_turn {
                    Shape::Rect { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, start: at(*start), end: at(*end) }
                } else {
                    Shape::Polygon { id: id.clone(), layer: layer_on_side(layer, fp.side), stroke_width: *stroke_width, filled: *filled, pts: rect_corners(*start, *end).into_iter().map(at).collect() }
                }
            }
        };
        FpShape { shape, style: self.style, hatch: self.hatch, radius: if matches!(self.shape, Shape::Rect { .. }) && self.radius > 0 { 0 } else { self.radius }, layers: self.layers.iter().map(|l| layer_on_side(l, fp.side)).collect(), mask_margin: self.mask_margin, endings: self.endings }
    }

    /// The box `(x0, y0, x1, y1)` the geometry spans in its own frame, grown by half the stroke width (`EDA_SHAPE::getBoundingBox`); an arc, a circle
    /// and a Bezier curve by the curve itself, cut to a 5 micrometre error.
    pub fn bbox(&self) -> Option<(Nm, Nm, Nm, Nm)> {
        if let Shape::Circle { center, end, stroke_width, .. } = &self.shape {
            let reach = (((end.x - center.x) as f64).hypot((end.y - center.y) as f64)).round() as Nm + stroke_width / 2;
            return Some((center.x - reach, center.y - reach, center.x + reach, center.y + reach));
        }
        crate::outline::bounding_box(std::slice::from_ref(&self.shape), crate::outline::MAX_ERROR_NM as f64).map(|(a, b)| (a.x, a.y, b.x, b.y))
    }
}

impl FpShape {
    /// [`FpShape::bbox`] of the geometry alone, the stroke not counted: the box a body is sized from (a fab line is 0.1 mm and the part is not that much bigger).
    pub fn geometry_bbox(&self) -> Option<(Nm, Nm, Nm, Nm)> {
        let mut bare = self.clone();
        bare.shape.set_stroke_width(0);
        bare.bbox()
    }
}

/// The four corners of the rectangle with opposite corners `a` and `b`, clockwise on the screen from the top left.
pub fn rect_corners(a: Point, b: Point) -> [Point; 4] {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    [Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]
}

/// A rectangle with rounded corners as a polygon (`ROUNDRECT::TransformToPolygon`): each corner a quarter circle cut to a 5 micrometre error. The
/// radius is limited to half the shorter side.
pub fn rounded_rect_points(a: Point, b: Point, radius: Nm) -> Vec<Point> {
    let [tl, tr, br, bl] = rect_corners(a, b);
    let r = radius.min((tr.x - tl.x) / 2).min((bl.y - tl.y) / 2).max(0);
    if r == 0 {
        return vec![tl, tr, br, bl];
    }
    let err = 5_000.0_f64;
    let per_quarter = ((std::f64::consts::FRAC_PI_2 / (1.0 - (err / r as f64).min(1.0)).acos()).ceil() as usize).clamp(2, 64);
    let mut out = Vec::new();
    // Corner centres and the angle each quarter starts at (screen angles, y down): top-left starts at 180 degrees and goes clockwise.
    let corners = [(Point { x: tl.x + r, y: tl.y + r }, 180.0_f64), (Point { x: tr.x - r, y: tr.y + r }, 270.0), (Point { x: br.x - r, y: br.y - r }, 0.0), (Point { x: bl.x + r, y: bl.y - r }, 90.0)];
    for (c, start) in corners {
        for i in 0..=per_quarter {
            let a = (start + 90.0 * i as f64 / per_quarter as f64).to_radians();
            out.push(Point { x: c.x + (r as f64 * a.cos()).round() as Nm, y: c.y + (r as f64 * a.sin()).round() as Nm });
        }
    }
    out.dedup();
    out
}

// ------------------------------------------------------------------------------------------------------------------------- text

/// One `PCB_TEXT` of a footprint (`fp_text user`). `layout` is in the footprint's frame ([`FieldLayout`]'s module doc has the frame): `at` and
/// `angle` as for a top-side footprint, `layer` as for a top-side footprint, `mirror` the flag as it is on a top-side footprint, `visible` always
/// true (KiCad turns a hidden footprint text into a hidden field; the reader leaves it out).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FpText {
    /// The text as the file has it: it may use variables ([`resolve_text_vars`]).
    pub text: String,
    pub layout: FieldLayout,
    /// `(font (face ..))`: a font other than KiCad's stroke font. Kept so the file comes back as it was; the studio draws the stroke font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// `(font (line_spacing ..))` in thousandths (1000 is KiCad's 1.0, the default and not written).
    #[serde(default, skip_serializing_if = "is_default_spacing")]
    pub line_spacing_permille: i64,
}

fn is_default_spacing(v: &i64) -> bool {
    *v == 0 || *v == 1000
}

/// What a text may name with `${..}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextVars<'a> {
    pub reference: &'a str,
    pub value: &'a str,
    /// The footprint's `lib:name` id; `${FOOTPRINT_LIBRARY}` is the part before the colon and `${FOOTPRINT_NAME}` the rest.
    pub footprint: &'a str,
}

/// `FOOTPRINT::ResolveTextVar` for the variables that need nothing but the footprint: `${REFERENCE}`, `${VALUE}`, `${FOOTPRINT_LIBRARY}`,
/// `${FOOTPRINT_NAME}` and `${LAYER}` (the footprint's own side, `F.Cu` or `B.Cu`); the legacy `%R` and `%V`. A variable it does not know stays as
/// written, like KiCad's when nothing resolves it.
pub fn resolve_text_vars(text: &str, vars: &TextVars, side: Side) -> String {
    if !text.contains("${") && !text.contains('%') {
        return text.replace("\\n", "\n");
    }
    let (lib, name) = vars.footprint.split_once(':').unwrap_or(("", vars.footprint));
    let mut out = text.replace("%R", "${REFERENCE}").replace("%V", "${VALUE}");
    for (var, val) in [("REFERENCE", vars.reference), ("VALUE", vars.value), ("FOOTPRINT_LIBRARY", lib), ("FOOTPRINT_NAME", name), ("LAYER", if side == Side::Bottom { "B.Cu" } else { "F.Cu" })] {
        out = out.replace(&format!("${{{var}}}"), val);
    }
    out.replace("\\n", "\n")
}

/// A text on the board: where it is, how it is turned, and the attributes the painter and the checks need, all absolute.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedText {
    /// The shown text, variables resolved.
    pub text: String,
    /// The layer it is on, on the footprint's side ([`layer_on_side`]).
    pub layer: String,
    pub at: Point,
    /// `PCB_TEXT::GetTextAngle`: the absolute angle, millidegrees counter-clockwise on the screen, `[0, 360000)`.
    pub angle: i64,
    /// `PCB_TEXT::GetDrawRotation`: the angle the glyphs are drawn at -- folded into `(-90, 90]` degrees for a text that keeps itself upright.
    pub draw_angle: i64,
    /// `(width, height)` of a glyph.
    pub size: (Nm, Nm),
    pub thickness: Nm,
    pub halign: i8,
    pub valign: i8,
    pub mirror: bool,
    pub bold: bool,
    pub italic: bool,
    pub knockout: bool,
    pub keep_upright: bool,
}

/// `PCB_TEXT::GetDrawRotation` of a text kept upright: the angle folded into `(-90, 90]` degrees; any other text is normalised to `[0, 360)`.
pub fn draw_rotation(angle_mdeg: i64, keep_upright: bool) -> i64 {
    let mut a = angle_mdeg.rem_euclid(360_000);
    if keep_upright {
        if a > 180_000 {
            a -= 360_000;
        }
        while a > 90_000 {
            a -= 180_000;
        }
        while a <= -90_000 {
            a += 180_000;
        }
    }
    a
}

impl FpText {
    /// A text on `layer` with KiCad's silkscreen defaults (1 mm, 0.15 mm), kept upright.
    pub fn new(text: impl Into<String>, at: Point, layer: &str) -> Self {
        FpText { text: text.into(), layout: FieldLayout::new(at, layer), face: None, line_spacing_permille: 0 }
    }

    /// The text for a footprint at `fp` (see [`PlacedText`]): the anchor through [`to_board`], the angle in the sense of the footprint's side
    /// ([`FieldLayout::board_angle`]), the layer on that side and, on the bottom side, the mirror flag toggled for a side-specific layer
    /// (`PCB_TEXT::Flip`).
    pub fn placed(&self, fp: &FootprintInstance, vars: &TextVars) -> PlacedText {
        let l = &self.layout;
        let angle = l.board_angle(fp);
        let layer = layer_on_side(&l.layer, fp.side);
        let mirror = if fp.side == Side::Bottom && is_side_specific(&l.layer) { !l.mirror } else { l.mirror };
        PlacedText {
            text: resolve_text_vars(&self.text, vars, fp.side),
            layer,
            at: l.board_position(fp),
            angle,
            draw_angle: draw_rotation(angle, l.keep_upright),
            size: l.size,
            thickness: l.thickness,
            halign: l.halign,
            valign: l.valign,
            mirror,
            bold: l.bold,
            italic: l.italic,
            knockout: l.knockout,
            keep_upright: l.keep_upright,
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------- the whole

/// The graphics and texts of a footprint (see the module doc).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FpArt {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<FpShape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<FpText>,
}

impl FpArt {
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty() && self.texts.is_empty()
    }

    /// The shapes on `layer` (as for a top-side footprint).
    pub fn shapes_on<'a>(&'a self, layer: &'a str) -> impl Iterator<Item = &'a FpShape> + 'a {
        self.shapes.iter().filter(move |s| s.on_layer(layer))
    }

    /// The box of the shapes on `layer`, in the footprint's frame: the footprint's body when the layer is `F.Fab`.
    pub fn bbox_on(&self, layer: &str) -> Option<(Nm, Nm, Nm, Nm)> {
        self.shapes_on(layer).filter_map(FpShape::bbox).reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    /// The art with its frame moved by `by` (a top-side pose: [`to_board`] turns and shifts every point, a text's angle drops by the turn): the library
    /// footprint's art in the frame of a footprint whose pads sit where `by` takes the library's.
    pub fn moved(&self, by: &FootprintInstance) -> FpArt {
        FpArt {
            shapes: self.shapes.iter().map(|s| s.placed(by)).collect(),
            texts: self
                .texts
                .iter()
                .map(|t| {
                    let mut t = t.clone();
                    t.layout.at = t.layout.board_position(by);
                    t.layout.angle = t.layout.board_angle(by);
                    t
                })
                .collect(),
        }
    }

    /// The box of the geometry of the shapes on `layer`, the strokes not counted (see [`FpShape::geometry_bbox`]): a footprint's body is that of `F.Fab`.
    pub fn geometry_bbox_on(&self, layer: &str) -> Option<(Nm, Nm, Nm, Nm)> {
        self.shapes_on(layer).filter_map(FpShape::geometry_bbox).reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    /// The art of an editable library footprint (`LibraryFootprint::graphics` and `texts`): a shape on the courtyard or the board edge is left out
    /// (the footprint's courtyard is its own field), a text is kept upright like every footprint text.
    pub fn from_library(graphics: &[Shape], texts: &[Text]) -> FpArt {
        FpArt {
            shapes: graphics.iter().filter(|g| is_art_layer(g.layer())).map(|g| FpShape::new(strip_id(g))).collect(),
            texts: texts.iter().map(FpText::from_library).collect(),
        }
    }

    /// The reverse: the shapes and texts as the editor lists them. What the editor's types have no field for (a dash style, a hatch, a vertical
    /// justification, bold, italic ...) is not carried, and neither is a hidden text (the editor's has no way to hide one).
    pub fn to_library(&self) -> (Vec<Shape>, Vec<Text>) {
        (self.shapes.iter().map(|s| s.shape.clone()).collect(), self.texts.iter().filter(|t| t.layout.visible).map(FpText::to_library).collect())
    }
}

fn strip_id(s: &Shape) -> Shape {
    let mut s = s.clone();
    s.set_id(String::new());
    s
}

impl FpText {
    /// The art text of an editor `Text`.
    pub fn from_library(t: &Text) -> FpText {
        let mut text = FpText::new(t.content.clone(), t.at, &t.layer);
        let l = &mut text.layout;
        l.angle = t.angle as i64;
        l.size = (t.size_nm, t.size_nm);
        l.thickness = t.stroke_width;
        l.halign = match t.justify {
            TextJustify::Left => -1,
            TextJustify::Center => 0,
            TextJustify::Right => 1,
        };
        l.mirror = t.mirror;
        text
    }

    /// The editor `Text` of an art text.
    pub fn to_library(&self) -> Text {
        let l = &self.layout;
        Text {
            id: String::new(),
            content: self.text.clone(),
            at: l.at,
            angle: l.angle.rem_euclid(360_000) as Millideg,
            layer: l.layer.clone(),
            size_nm: l.size.1,
            stroke_width: l.thickness,
            justify: match l.halign {
                -1 => TextJustify::Left,
                1 => TextJustify::Right,
                _ => TextJustify::Center,
            },
            mirror: l.mirror,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    fn fp(side: Side, rot: Millideg) -> FootprintInstance {
        FootprintInstance { id: "R1".into(), at: p(10_000_000, 20_000_000), rot, side, label: Default::default() }
    }

    fn line(layer: &str, a: Point, b: Point) -> FpShape {
        FpShape::new(Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: 120_000, filled: false, start: a, end: b })
    }

    #[test]
    fn side_specific_layers_flip_and_user_layers_do_not() {
        for (a, b) in [("F.SilkS", "B.SilkS"), ("F.Fab", "B.Fab"), ("F.Cu", "B.Cu"), ("F.Mask", "B.Mask"), ("F.Paste", "B.Paste"), ("F.Adhes", "B.Adhes"), ("B.CrtYd", "F.CrtYd")] {
            assert!(is_side_specific(a) && is_side_specific(b));
            assert_eq!(flip_layer(a), b);
            assert_eq!(flip_layer(b), a);
        }
        for user in ["Dwgs.User", "Cmts.User", "Eco1.User", "User.1", "Margin", "Edge.Cuts"] {
            assert!(!is_side_specific(user), "{user}");
            assert_eq!(flip_layer(user), user);
        }
        assert!(is_side_specific("In1.Cu"));
        assert_eq!(flip_layer("In1.Cu"), "In1.Cu");
        assert_eq!(layer_on_side("F.SilkS", Side::Top), "F.SilkS");
        assert_eq!(layer_on_side("F.SilkS", Side::Bottom), "B.SilkS");
    }

    #[test]
    fn a_shape_follows_its_footprints_pose() {
        let s = line("F.SilkS", p(1_000_000, 0), p(2_000_000, 500_000));
        // Top side, turned a quarter clockwise on the screen: (x, y) -> (-y, x) about the origin, then moved.
        let placed = s.placed(&fp(Side::Top, 90_000));
        assert_eq!(placed.shape.points(), vec![p(10_000_000, 21_000_000), p(9_500_000, 22_000_000)]);
        assert_eq!(placed.layer(), "F.SilkS");
        // Bottom side: x is mirrored first, and the silkscreen is the other side's.
        let placed = s.placed(&fp(Side::Bottom, 0));
        assert_eq!(placed.shape.points(), vec![p(9_000_000, 20_000_000), p(8_000_000, 20_500_000)]);
        assert_eq!(placed.layer(), "B.SilkS");
        // Not a silkscreen: the user layer does not change sides.
        assert_eq!(line("Dwgs.User", p(0, 0), p(1, 1)).placed(&fp(Side::Bottom, 0)).layer(), "Dwgs.User");
    }

    #[test]
    fn a_rectangle_stays_a_rectangle_under_a_square_turn_and_is_a_polygon_otherwise() {
        let r = FpShape::new(Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100_000, filled: false, start: p(-1_000_000, -500_000), end: p(1_000_000, 500_000) });
        assert!(matches!(r.placed(&fp(Side::Top, 90_000)).shape, Shape::Rect { .. }));
        assert!(matches!(r.placed(&fp(Side::Bottom, 180_000)).shape, Shape::Rect { .. }));
        let tilted = r.placed(&fp(Side::Top, 45_000));
        let Shape::Polygon { pts, .. } = &tilted.shape else { panic!("a tilted rectangle is a polygon: {:?}", tilted.shape) };
        assert_eq!(pts.len(), 4);
        // a rounded one is a polygon at any angle, with more than its four corners
        let rounded = FpShape { radius: 300_000, ..r.clone() };
        let Shape::Polygon { pts, .. } = rounded.placed(&fp(Side::Top, 0)).shape else { panic!("a rounded rectangle is a polygon") };
        assert!(pts.len() > 8, "{} points", pts.len());
        assert!(pts.iter().all(|q| (9_000_000..=11_000_000).contains(&q.x) && (19_500_000..=20_500_000).contains(&q.y)));
    }

    #[test]
    fn bounding_boxes_include_the_stroke_and_follow_curves() {
        let l = line("F.Fab", p(-1_000_000, -500_000), p(1_000_000, 500_000));
        assert_eq!(l.bbox(), Some((-1_060_000, -560_000, 1_060_000, 560_000)));
        let c = FpShape::new(Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: 100_000, filled: false, center: p(0, 0), end: p(1_000_000, 0) });
        assert_eq!(c.bbox(), Some((-1_050_000, -1_050_000, 1_050_000, 1_050_000)));
        let mut art = FpArt::default();
        art.shapes.push(l);
        art.shapes.push(line("F.SilkS", p(5_000_000, 5_000_000), p(6_000_000, 6_000_000)));
        assert_eq!(art.bbox_on("F.Fab"), Some((-1_060_000, -560_000, 1_060_000, 560_000)));
        assert_eq!(art.bbox_on("B.Fab"), None);
    }

    #[test]
    fn a_text_turns_with_its_footprint_and_flips_with_it() {
        let mut t = FpText::new("hello", p(1_000_000, 0), "F.SilkS");
        t.layout.angle = 30_000;
        let vars = TextVars { reference: "R1", value: "10k", footprint: "Resistor_SMD:R_0603" };
        // top side, turned 90 degrees clockwise on the screen: KiCad's counter-clockwise angle drops by 90
        let top = t.placed(&fp(Side::Top, 90_000), &vars);
        assert_eq!((top.at, top.angle, top.layer.as_str(), top.mirror), (p(10_000_000, 21_000_000), 300_000, "F.SilkS", false));
        // bottom side: the sense of the angle turns over, the layer goes to the back and the text is mirrored (`PCB_TEXT::Flip`)
        let bottom = t.placed(&fp(Side::Bottom, 0), &vars);
        assert_eq!((bottom.at, bottom.angle, bottom.layer.as_str(), bottom.mirror), (p(9_000_000, 20_000_000), 330_000, "B.SilkS", true));
        // a text on a user layer is not mirrored by the flip
        let mut u = FpText::new("note", p(0, 0), "Dwgs.User");
        u.layout.mirror = false;
        let placed = u.placed(&fp(Side::Bottom, 0), &vars);
        assert_eq!((placed.layer.as_str(), placed.mirror), ("Dwgs.User", false));
    }

    #[test]
    fn a_text_that_keeps_upright_is_never_drawn_upside_down() {
        assert_eq!(draw_rotation(0, true), 0);
        assert_eq!(draw_rotation(90_000, true), 90_000);
        assert_eq!(draw_rotation(180_000, true), 0);
        assert_eq!(draw_rotation(270_000, true), 90_000);
        assert_eq!(draw_rotation(200_000, true), 20_000);
        assert_eq!(draw_rotation(200_000, false), 200_000);
        assert_eq!(draw_rotation(-30_000, false), 330_000);
        assert_eq!(draw_rotation(-30_000, true), -30_000);
    }

    #[test]
    fn variables_resolve_to_the_footprint_and_unknown_ones_stay() {
        let vars = TextVars { reference: "U3", value: "LM358", footprint: "Package_SO:SOIC-8" };
        assert_eq!(resolve_text_vars("${REFERENCE} ${VALUE}", &vars, Side::Top), "U3 LM358");
        assert_eq!(resolve_text_vars("%R/%V", &vars, Side::Top), "U3/LM358");
        assert_eq!(resolve_text_vars("${FOOTPRINT_LIBRARY}:${FOOTPRINT_NAME} on ${LAYER}", &vars, Side::Bottom), "Package_SO:SOIC-8 on B.Cu");
        assert_eq!(resolve_text_vars("${NET_NAME(1)} ${Vendor}", &vars, Side::Top), "${NET_NAME(1)} ${Vendor}");
        assert_eq!(resolve_text_vars("two\\nlines", &vars, Side::Top), "two\nlines");
    }

    #[test]
    fn the_editors_graphics_and_texts_become_art_and_come_back() {
        let graphics = vec![
            Shape::Segment { id: "fpg_1".into(), layer: "F.SilkS".into(), stroke_width: 120_000, filled: false, start: p(0, 0), end: p(1_000_000, 0) },
            // a courtyard line is the footprint's courtyard, not art
            Shape::Segment { id: "fpg_2".into(), layer: "F.CrtYd".into(), stroke_width: 50_000, filled: false, start: p(0, 0), end: p(1_000_000, 1_000_000) },
        ];
        let texts = vec![Text { id: "fpt_1".into(), content: "${REFERENCE}".into(), at: p(0, 500_000), angle: 90_000, layer: "F.Fab".into(), size_nm: 800_000, stroke_width: 120_000, justify: TextJustify::Left, mirror: false }];
        let art = FpArt::from_library(&graphics, &texts);
        assert_eq!((art.shapes.len(), art.texts.len()), (1, 1));
        assert_eq!(art.shapes[0].shape.id(), "", "art items are not addressed");
        assert_eq!((art.texts[0].layout.halign, art.texts[0].layout.size), (-1, (800_000, 800_000)));
        assert!(art.texts[0].layout.keep_upright);
        let (g, t) = art.to_library();
        assert_eq!(g.len(), 1);
        assert_eq!((t[0].content.as_str(), t[0].angle, t[0].size_nm, t[0].justify), ("${REFERENCE}", 90_000, 800_000, TextJustify::Left));
    }

    #[test]
    fn empty_art_serialises_to_nothing_and_reads_back() {
        let json = serde_json::to_string(&FpArt::default()).unwrap();
        assert_eq!(json, "{}");
        let mut art = FpArt::default();
        art.shapes.push(FpShape { style: LineStyle::Dash, hatch: Some(Hatch::CrossHatch), layers: vec!["F.Cu".into(), "F.Mask".into()], ..line("F.Cu", p(0, 0), p(1, 1)) });
        art.texts.push(FpText { face: Some("Arial".into()), line_spacing_permille: 1200, ..FpText::new("x", p(0, 0), "F.SilkS") });
        let back: FpArt = serde_json::from_str(&serde_json::to_string(&art).unwrap()).unwrap();
        assert_eq!(back, art);
        // and yaml, which an imported board's intent is written in
        let back: FpArt = serde_yaml::from_str(&serde_yaml::to_string(&art).unwrap()).unwrap();
        assert_eq!(back, art);
    }

    /// The footprint editor's texts have no way to hide one, so a hidden text is not handed to it as a visible one.
    #[test]
    fn a_hidden_text_is_not_a_library_text() {
        let mut art = FpArt::default();
        art.texts.push(FpText::new("shown", p(0, 0), "F.SilkS"));
        let mut hidden = FpText::new("hidden", p(0, 500), "F.Fab");
        hidden.layout.visible = false;
        art.texts.push(hidden);
        let (_, texts) = art.to_library();
        assert_eq!(texts.iter().map(|t| t.content.as_str()).collect::<Vec<_>>(), ["shown"]);
    }
}
