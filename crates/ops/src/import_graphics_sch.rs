//! Import Graphics in the schematic editor and the Symbol Editor: a DXF or SVG file as graphic lines, shapes and text of the sheet, or as drawn
//! items of the symbol being edited, in one undo step each.
//!
//! Ports `DIALOG_IMPORT_GFX_SCH` and `SCH_DRAWING_TOOLS::ImportGraphics` / `SYMBOL_EDITOR_DRAWING_TOOLS::ImportGraphics` (`eeschema/`) with the two
//! importers they make items with, `GRAPHICS_IMPORTER_SCH` (`SCH_LINE` for a line, `SCH_SHAPE` for circles, arcs, polygons and curves, `SCH_TEXT`) and
//! `GRAPHICS_IMPORTER_LIB_SYMBOL` (every line a two-point polygon of the symbol, `SCH_SHAPE`s and `SCH_TEXT` for the rest), at KiCad 8303b2ad
//! (`eeschema/import_gfx/`). The file is read by the plugins of `eda_kicad` ([`eda_kicad::dxf_import`], [`eda_kicad::svg_import`]); the scale, the origin
//! and the default line width are mapped the way [`crate::import_graphics`] maps the board's. What the dialog does not have here is the board's: no
//! layer, no group, no discontinuities to fix.
//!
//! The file travels with the command, like a paste's text. The placement KiCad calls interactive is this command with the offset set to the cursor
//! (`delta = cursorPos`: the drawing's origin lands on the cursor), followed by the Move tool in the schematic.
//!
//! # Where this differs from KiCad
//!
//! * A shape with no stroke (an SVG shape that is only filled, `-1` in the importer: "historically -1 meant no-stroke in Eeschema") is drawn with a
//!   1 micrometre hairline: neither IR can say "no outline". A line of no stroke keeps the sheet's default width, as `SCH_LINE::GetPenWidth` gives it.
//! * A schematic text has a position, an angle (along a row or up a column: the nearer of the two) and a size, and is anchored at the left end of its
//!   baseline: the file's justification is applied to the position (by the text's width and, for a top or middle alignment, its height) and not kept. A
//!   library symbol's text is anchored at the middle of its baseline, so only the width is applied there. The size is the text's height.
//! * A polygon filled with a colour is `Color` in the schematic, with the colour; the library symbol has no such fill and gets the stroke's colour
//!   ("outline"). A library symbol has no curves either: a Bezier curve of the file is cut into line segments (`ARC_LOW_DEF_MM`, 20 micrometres),
//!   where KiCad keeps the curve.
//! * The items of a symbol are of the unit and the body style the editor is on; KiCad's importer sets the unit and leaves the body style at "all".
//! * A symbol's Y axis points up: the picture's Y is negated, so it comes out the way up it was drawn.

use super::Board;
use crate::import_graphics::{GraphicsFormat, ImportGraphics};
use eda_kicad::graphics_import::{arc_points, prepare, spline_or_line, HJustify, Imported, ImportedShape, LineStyle, Mapper, Rgba, SplineOrLine, Stroke, VJustify, V2};
use eda_model::ir::{LibraryFill, LibrarySymbolGraphic, Millideg, Point, SchLine, SchematicText, Nm};
use eda_model::kicad_font;
use eda_model::sch_extras::{SchColor, SchFill, SchGraphic, SchGraphicKind, SchLineStyle, SchStroke};
use eda_model::symbol::SPoint;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

fn d_one() -> f64 {
    1.0
}

fn d_body_style() -> u32 {
    1
}

/// `ARC_LOW_DEF_MM`: how far a flattened curve may leave the real one.
const ARC_LOW_DEF_MM: f64 = 0.02;

/// `Cmd::ImportSchGraphics`: what `DIALOG_IMPORT_GFX_SCH` hands `SCH_DRAWING_TOOLS::ImportGraphics` in the schematic editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportSchGraphics {
    pub format: GraphicsFormat,
    /// The file's text.
    pub text: String,
    /// "Default units": the unit of a DXF that names none ([`eda_kicad::dxf_import::DxfUnits::from_name`]); empty is millimetres.
    #[serde(default)]
    pub dxf_units: String,
    /// "Import scale".
    #[serde(default = "d_one")]
    pub scale: f64,
    /// The importer's offset in nanometres, added after the scale: the dialog's "At" over the scale (KiCad's own arithmetic), or the cursor.
    #[serde(default)]
    pub offset_x_nm: i64,
    #[serde(default)]
    pub offset_y_nm: i64,
    /// "Default line width", micrometres; 0 is the sheet's default width.
    #[serde(default)]
    pub line_width_nm: Nm,
}

/// `Cmd::ImportSymbolGraphics`: the same for the symbol `lib_id` of the Symbol Editor (`GRAPHICS_IMPORTER_LIB_SYMBOL`), the items on `unit` (0 for every unit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportSymbolGraphics {
    pub lib_id: String,
    #[serde(default)]
    pub unit: u32,
    #[serde(default = "d_body_style")]
    pub body_style: u32,
    pub format: GraphicsFormat,
    pub text: String,
    #[serde(default)]
    pub dxf_units: String,
    #[serde(default = "d_one")]
    pub scale: f64,
    #[serde(default)]
    pub offset_x_nm: i64,
    #[serde(default)]
    pub offset_y_nm: i64,
    #[serde(default)]
    pub line_width_nm: Nm,
}

fn fail(check: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, "import graphics", msg.into())]
}

/// The file read and the mapping set up with the request's scale, offset and default width, in `iu_per_mm` units of the editor's own; the size check of
/// `GRAPHICS_IMPORTER_BUFFER::ImportTo` has run.
fn read_and_map(format: GraphicsFormat, text: &str, dxf_units: &str, scale: f64, offset_nm: (i64, i64), line_width_nm: Nm, iu_per_mm: f64) -> Result<(Imported, Mapper), Vec<CheckResult>> {
    if !(scale.is_finite() && scale != 0.0) {
        return Err(fail("ops_bad_import", "the import scale must be a number other than zero"));
    }
    // The board's request holds the reading and the mapping the dialogs have in common.
    let r = ImportGraphics { format, text: text.to_string(), layer: String::new(), dxf_units: dxf_units.to_string(), scale, offset_x_nm: offset_nm.0, offset_y_nm: offset_nm.1, line_width_nm, group: false, fix_tolerance_nm: None, replace_outline: false };
    let imported = r.read().map_err(|e| fail("ops_bad_import", e))?;
    let mut mapper = r.mapper();
    mapper.iu_per_mm = iu_per_mm;
    let (fits, messages) = prepare(&imported.shapes, &mut mapper);
    if !fits {
        return Err(fail("ops_bad_import", messages.join(" ")));
    }
    if imported.shapes.is_empty() {
        // "No graphic items found in file." -- and why, when the plugin said.
        let mut why = String::from("No graphic items found in file.");
        for m in &imported.messages {
            why.push(' ');
            why.push_str(m);
        }
        return Err(fail("ops_empty_import", why));
    }
    Ok((imported, mapper))
}

fn sch_color(c: Rgba) -> SchColor {
    let b = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    SchColor { r: b(c.r), g: b(c.g), b: b(c.b), a: b(c.a) }
}

fn line_style(s: LineStyle) -> SchLineStyle {
    match s {
        LineStyle::Default => SchLineStyle::Default,
        LineStyle::Solid => SchLineStyle::Solid,
        LineStyle::Dash => SchLineStyle::Dash,
        LineStyle::Dot => SchLineStyle::Dot,
        LineStyle::DashDot => SchLineStyle::DashDot,
        LineStyle::DashDotDot => SchLineStyle::DashDotDot,
    }
}

fn point(p: (i64, i64)) -> Point {
    Point { x: p.0, y: p.1 }
}

/// The width of the widest line of `text` in the stroke font at `size_mm`, in millimetres.
fn text_width_mm(text: &str, size_mm: f64) -> f64 {
    let size_iu = kicad_font::mm_to_iu(size_mm);
    text.split('\n').map(|l| kicad_font::text_advance_iu(l, size_iu)).max().unwrap_or(0) as f64 / kicad_font::IU_PER_MM
}

/// Where a text's anchor goes to stand for the file's justification, on the drawing (Y down) in the file's own millimetres: `to_middle` anchors the result at
/// the middle of the text's baseline (a library symbol), else at its left end (the schematic's). `angle_deg` is the text's own angle, counter-clockwise on screen.
fn justified_anchor_mm(origin: V2, text: &str, size_mm: f64, angle_deg: f64, h: HJustify, v: VJustify, to_middle: bool) -> V2 {
    let w = text_width_mm(text, size_mm);
    // Along the text, from the anchor the file gives to the anchor wanted.
    let along = match (h, to_middle) {
        (HJustify::Left, false) | (HJustify::Center, true) => 0.0,
        (HJustify::Center, false) => -w / 2.0,
        (HJustify::Right, false) => -w,
        (HJustify::Left, true) => w / 2.0,
        (HJustify::Right, true) => -w / 2.0,
    };
    // Down the text's own height to its baseline: a bottom alignment is the baseline, a middle one half a size above it, a top one a size above it.
    let down = if to_middle {
        0.0
    } else {
        match v {
            VJustify::Bottom => 0.0,
            VJustify::Center => size_mm / 2.0,
            VJustify::Top => size_mm,
        }
    };
    let a = angle_deg.to_radians();
    let (ux, uy) = (a.cos(), -a.sin()); // along the text on a Y-down drawing
    let (dx, dy) = (a.sin(), a.cos()); // down from it
    V2::new(origin.x + ux * along + dx * down, origin.y + uy * along + dy * down)
}

// -------------------------------------------------------------------------------------------------------------------- the schematic

/// What the schematic importer makes of a file.
#[derive(Debug, Clone, Default)]
pub struct SchItems {
    /// Graphic lines, each with the style and colour of its stroke when it has one (the width is the line's own).
    pub lines: Vec<(SchLine, Option<SchStroke>)>,
    pub graphics: Vec<SchGraphic>,
    pub texts: Vec<SchematicText>,
}

/// `GRAPHICS_IMPORTER_SCH`'s items for `imported`, in micrometres (`mapper.iu_per_mm` is 1000).
pub fn sch_items(imported: &Imported, mapper: &Mapper) -> SchItems {
    let m = *mapper;
    let mut out = SchItems::default();
    // `MapStrokeParams` for a shape: -1 is "no stroke", which this IR draws as a hairline; any other width is mapped.
    let shape_stroke = |s: &Stroke| -> (Nm, SchLineStyle, Option<SchColor>) { (if s.width == -1.0 { 1 } else { m.line_width(s.width) }, line_style(s.style), s.color.map(sch_color)) };
    // "Arcs with a too big radius cannot" be handled: radius >= INT_MAX / 2 in KiCad's tenth-of-a-micrometre unit, a tenth of that in this one.
    let max_radius_nm = i32::MAX as f64 / 2.0 / 10.0;
    let add_line = |out: &mut SchItems, a: V2, b: V2, stroke: &Stroke| {
        let (pa, pb) = (m.coord(a), m.coord(b));
        // "Skip 0 len lines:"
        if pa == pb {
            return;
        }
        // A width of -1 is not a pen width (`SCH_LINE::GetPenWidth` takes the default for it).
        let width = if stroke.width == -1.0 { 0 } else { m.line_width(stroke.width) };
        let st = SchStroke { width_nm: 0, style: line_style(stroke.style), color: stroke.color.map(sch_color) };
        out.lines.push((SchLine { id: String::new(), pts: vec![point(pa), point(pb)], width_nm: width }, (!st.is_default()).then_some(st)));
    };
    for shape in &imported.shapes {
        match shape {
            ImportedShape::Line { start, end, stroke } => add_line(&mut out, *start, *end, stroke),
            ImportedShape::Circle { center, radius, stroke, filled, .. } => {
                let c = m.coord(*center);
                let e = m.coord(V2::new(center.x + radius, center.y));
                let r = (((e.0 - c.0) as f64).hypot((e.1 - c.1) as f64)).round() as Nm;
                if r == 0 {
                    continue;
                }
                let (w, style, color) = shape_stroke(stroke);
                let mut g = SchGraphic::new(SchGraphicKind::Circle { center: point(c), radius_nm: r });
                (g.width_nm, g.line_style, g.color) = (w, style, color);
                if *filled {
                    // `SetFilled( true )` is "filled shape": the outline's colour.
                    g.fill = SchFill::Outline;
                }
                out.graphics.push(g);
            }
            ImportedShape::Arc { center, start, angle_deg, stroke } => {
                let (s, mid, e) = arc_points(*center, *start, *angle_deg);
                let (cs, cc) = (m.coord(*start), m.coord(*center));
                if ((cs.0 - cc.0) as f64).hypot((cs.1 - cc.1) as f64) >= max_radius_nm {
                    // "Arc cannot be handled: convert it to a segment"
                    add_line(&mut out, *start, e, stroke);
                    continue;
                }
                let (ps, pm, pe) = (m.coord(s), m.coord(mid), m.coord(e));
                let cross = (pm.0 - ps.0) as i128 * (pe.1 - ps.1) as i128 - (pm.1 - ps.1) as i128 * (pe.0 - ps.0) as i128;
                if cross == 0 {
                    // Three points on a line: an arc too small to hold a bend at this unit.
                    add_line(&mut out, s, e, stroke);
                    continue;
                }
                let (w, style, color) = shape_stroke(stroke);
                let mut g = SchGraphic::new(SchGraphicKind::Arc { start: point(ps), mid: point(pm), end: point(pe) });
                (g.width_nm, g.line_style, g.color) = (w, style, color);
                out.graphics.push(g);
            }
            ImportedShape::Polygon { pts, stroke, filled, fill_color, .. } => {
                let mut mapped: Vec<Point> = pts.iter().map(|p| point(m.coord(*p))).collect();
                mapped.dedup();
                // The polygon of this IR closes by itself; KiCad's "need to close last point" repeats the first.
                if mapped.len() > 1 && mapped.first() == mapped.last() {
                    mapped.pop();
                }
                if mapped.len() < 3 {
                    continue;
                }
                let (w, style, color) = shape_stroke(stroke);
                let mut g = SchGraphic::new(SchGraphicKind::Polygon { pts: mapped });
                (g.width_nm, g.line_style, g.color) = (w, style, color);
                if *filled {
                    // `aFillColor != COLOR4D::UNSPECIFIED ? FILLED_WITH_COLOR : FILLED_SHAPE`
                    match fill_color {
                        Some(c) => {
                            g.fill = SchFill::Color;
                            g.fill_color = Some(sch_color(*c));
                        }
                        None => g.fill = SchFill::Outline,
                    }
                }
                out.graphics.push(g);
            }
            ImportedShape::Text { origin, text, height, orientation_deg, h, v, .. } => {
                let size_nm = (height * m.scaling_factor()).round() as Nm;
                if size_nm <= 0 || text.is_empty() {
                    continue;
                }
                // A schematic text runs along a row or up a column: the nearer of the two.
                let turn = orientation_deg.rem_euclid(180.0);
                let (angle, angle_deg): (Millideg, f64) = if (45.0..135.0).contains(&turn) { (90_000, 90.0) } else { (0, 0.0) };
                // The file's alignment, in its own millimetres before the scale, to the left end of the baseline.
                let at = justified_anchor_mm(*origin, text, *height, angle_deg, *h, *v, false);
                out.texts.push(SchematicText { id: String::new(), content: text.clone(), at: point(m.coord(at)), angle, size_nm });
            }
            ImportedShape::Spline { start, c1, c2, end, stroke } => {
                let (ps, p1, p2, pe) = (m.coord(*start), m.coord(*c1), m.coord(*c2), m.coord(*end));
                match spline_or_line(ps, p1, p2, pe) {
                    SplineOrLine::Curve => {
                        let (w, style, color) = shape_stroke(stroke);
                        let mut g = SchGraphic::new(SchGraphicKind::Bezier { start: point(ps), c1: point(p1), c2: point(p2), end: point(pe) });
                        (g.width_nm, g.line_style, g.color) = (w, style, color);
                        out.graphics.push(g);
                    }
                    // "SCH_LINES aren't SCH_SHAPES"
                    SplineOrLine::Line => add_line(&mut out, *start, *end, stroke),
                    SplineOrLine::Skip => {}
                }
            }
        }
    }
    out
}

// -------------------------------------------------------------------------------------------------------------------- the symbol

/// `GRAPHICS_IMPORTER_LIB_SYMBOL`'s items for `imported`: the mapper's unit is the schematic's own (a tenth of a micrometre, `iu_per_mm` 10000), the
/// result is in millimetres with the Y axis up, on `unit` and `body_style`.
pub fn symbol_items(imported: &Imported, mapper: &Mapper, unit: u32, body_style: u32) -> Vec<LibrarySymbolGraphic> {
    let m = *mapper;
    let mut out: Vec<LibrarySymbolGraphic> = Vec::new();
    let to_mm = |p: (i64, i64)| SPoint::new(p.0 as f64 / m.iu_per_mm, -(p.1 as f64) / m.iu_per_mm);
    // `MapStrokeParams`: -1 is "no stroke" -- a hairline of a micrometre here; any other width is mapped, in millimetres.
    let stroke_mm = |s: &Stroke| -> f64 {
        if s.width == -1.0 {
            0.001
        } else {
            m.line_width(s.width) as f64 / m.iu_per_mm
        }
    };
    let max_radius = i32::MAX as f64 / 2.0;
    let line = |out: &mut Vec<LibrarySymbolGraphic>, a: V2, b: V2, stroke: &Stroke| {
        let (pa, pb) = (m.coord(a), m.coord(b));
        // "Skip 0 len lines:"
        if pa == pb {
            return;
        }
        out.push(LibrarySymbolGraphic::Polyline { id: String::new(), unit, body_style, pts: vec![to_mm(pa), to_mm(pb)], stroke_mm: stroke_mm(stroke), fill: LibraryFill::None });
    };
    for shape in &imported.shapes {
        match shape {
            ImportedShape::Line { start, end, stroke } => line(&mut out, *start, *end, stroke),
            ImportedShape::Circle { center, radius, stroke, filled, .. } => {
                let c = m.coord(*center);
                let e = m.coord(V2::new(center.x + radius, center.y));
                let r = ((e.0 - c.0) as f64).hypot((e.1 - c.1) as f64) / m.iu_per_mm;
                if r == 0.0 {
                    continue;
                }
                out.push(LibrarySymbolGraphic::Circle { id: String::new(), unit, body_style, center: to_mm(c), radius_mm: r, stroke_mm: stroke_mm(stroke), fill: if *filled { LibraryFill::Outline } else { LibraryFill::None } });
            }
            ImportedShape::Arc { center, start, angle_deg, stroke } => {
                let (s, mid, e) = arc_points(*center, *start, *angle_deg);
                let (cs, cc) = (m.coord(*start), m.coord(*center));
                if ((cs.0 - cc.0) as f64).hypot((cs.1 - cc.1) as f64) >= max_radius {
                    line(&mut out, *start, e, stroke);
                    continue;
                }
                let (ps, pm, pe) = (m.coord(s), m.coord(mid), m.coord(e));
                let cross = (pm.0 - ps.0) as i128 * (pe.1 - ps.1) as i128 - (pm.1 - ps.1) as i128 * (pe.0 - ps.0) as i128;
                if cross == 0 {
                    line(&mut out, s, e, stroke);
                    continue;
                }
                // Mirroring the Y axis keeps the three points of an arc: it is the same arc seen the other way up.
                out.push(LibrarySymbolGraphic::Arc { id: String::new(), unit, body_style, start: to_mm(ps), mid: to_mm(pm), end: to_mm(pe), stroke_mm: stroke_mm(stroke), fill: LibraryFill::None });
            }
            ImportedShape::Polygon { pts, stroke, filled, .. } => {
                let mut mapped: Vec<(i64, i64)> = pts.iter().map(|p| m.coord(*p)).collect();
                mapped.dedup();
                if mapped.len() > 1 && mapped.first() == mapped.last() {
                    mapped.pop();
                }
                // `IsPolyShapeValid`: a polygon needs three points.
                if mapped.len() < 3 {
                    continue;
                }
                // "Need to close last point for libedit"
                let mut ps: Vec<SPoint> = mapped.iter().map(|p| to_mm(*p)).collect();
                ps.push(ps[0]);
                out.push(LibrarySymbolGraphic::Polyline { id: String::new(), unit, body_style, pts: ps, stroke_mm: stroke_mm(stroke), fill: if *filled { LibraryFill::Outline } else { LibraryFill::None } });
            }
            ImportedShape::Text { origin, text, height, orientation_deg, h, v, .. } => {
                let size_mm = height * m.scale;
                if size_mm <= 0.0 || text.is_empty() {
                    continue;
                }
                let angle = orientation_deg.rem_euclid(360.0);
                // The file's alignment, in its own millimetres before the scale, to the middle of the baseline.
                let at = justified_anchor_mm(*origin, text, *height, angle, *h, *v, true);
                out.push(LibrarySymbolGraphic::Text { id: String::new(), unit, body_style, text: text.clone(), at: to_mm(m.coord(at)), angle_deg: angle, size_mm });
            }
            ImportedShape::Spline { start, c1, c2, end, stroke } => {
                let (ps, p1, p2, pe) = (m.coord(*start), m.coord(*c1), m.coord(*c2), m.coord(*end));
                // `RebuildBezierToSegmentsPointsList( ARC_LOW_DEF_MM )` in the importer's own unit; "if the spline is degenerated (i.e. a segment) add it as
                // segment or discard it if null".
                let tolerance = (ARC_LOW_DEF_MM * m.iu_per_mm).round() as Nm;
                let poly = eda_model::bezier::bezier_polyline(point(ps), point(p1), point(p2), point(pe), tolerance);
                if poly.len() <= 2 {
                    line(&mut out, *start, *end, stroke);
                    continue;
                }
                let pts: Vec<SPoint> = poly.iter().map(|p| to_mm((p.x, p.y))).collect();
                out.push(LibrarySymbolGraphic::Polyline { id: String::new(), unit, body_style, pts, stroke_mm: stroke_mm(stroke), fill: LibraryFill::None });
            }
        }
    }
    out
}

impl Board<'_> {
    /// `Cmd::ImportSchGraphics`: the file's items on the sheet in view.
    pub(crate) fn import_sch_graphics(&mut self, req: &ImportSchGraphics) -> Result<(), Vec<CheckResult>> {
        let (imported, mapper) = read_and_map(req.format, &req.text, &req.dxf_units, req.scale, (req.offset_x_nm, req.offset_y_nm), req.line_width_nm, 1000.0)?;
        let items = sch_items(&imported, &mapper);
        if items.lines.is_empty() && items.graphics.is_empty() && items.texts.is_empty() {
            return Err(fail("ops_empty_import", "No graphic items found in file."));
        }
        let sch = self.schematic_mut_or_create();
        let first_line = sch.lines.len();
        let (lines, strokes): (Vec<SchLine>, Vec<Option<SchStroke>>) = items.lines.into_iter().unzip();
        sch.lines.extend(lines);
        sch.extras.graphics.extend(items.graphics);
        sch.texts.extend(items.texts);
        sch.assign_missing_ids();
        // A graphic line keeps its width in the line and its style and colour in the stroke table.
        for (i, st) in strokes.into_iter().enumerate() {
            if let Some(st) = st {
                let id = sch.lines[first_line + i].id.clone();
                sch.extras.strokes.insert(id, st);
            }
        }
        Ok(())
    }

    /// `Cmd::ImportSymbolGraphics`: the file's items in the symbol `lib_id`.
    pub(crate) fn import_symbol_graphics(&mut self, req: &ImportSymbolGraphics) -> Result<(), Vec<CheckResult>> {
        // The importer works in the schematic's own unit, a tenth of a micrometre, so its rounding and its radius test are KiCad's.
        let (imported, mapper) = read_and_map(req.format, &req.text, &req.dxf_units, req.scale, (req.offset_x_nm, req.offset_y_nm), req.line_width_nm, 10_000.0)?;
        let items = symbol_items(&imported, &mapper, req.unit, req.body_style);
        if items.is_empty() {
            return Err(fail("ops_empty_import", "No graphic items found in file."));
        }
        let sym = self.drawing_symbol_mut(&req.lib_id)?;
        sym.graphics.extend(items);
        sym.assign_missing_ids();
        Ok(())
    }
}
