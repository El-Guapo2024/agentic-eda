//! Import Graphics on the board: a DXF or SVG file as lines, arcs, circles, polygons, curves and text on a layer, in one undo step.
//!
//! Ports `DRAWING_TOOL::PlaceImportedGraphics` (`pcbnew/tools/drawing_tool.cpp`) and `GRAPHICS_IMPORTER_PCBNEW`
//! (`pcbnew/import_gfx/graphics_importer_pcbnew.cpp`) at KiCad 8303b2ad. The file is read by the plugins of `eda_kicad`
//! ([`eda_kicad::dxf_import`], [`eda_kicad::svg_import`]) into shapes in millimetres; this verb maps them with the dialog's scale and
//! origin (`MapCoordinate`), gives them the layer and line width, makes one item of the board of each, joins the open ends that are within
//! the tolerance (`ConnectBoardShapes`, [`crate::fix_shapes`]) and, when asked, puts them in a group of their own.
//!
//! The file travels with the command (like a paste's text), so the command can be replayed on its own. The dialog reads it first
//! (`POST /api/import_graphics/read`) to tell the person what is in it; the placement KiCad calls interactive is this command with the
//! origin set so that the picture's top left corner is at the cursor, followed by the Move tool, as Paste does in this studio.
//!
//! # Where this differs from KiCad
//!
//! * `replace_outline`: the studio's board outline can be a polygon of the intent that no tool selects or deletes, so an outline imported on
//!   Edge.Cuts would sit beside it (and be read as a cut-out of it). With the option the imported Edge.Cuts shapes become the board's outline
//!   (`DrawingsSection::outline_is_shapes`), as they are in a board read from a `.kicad_pcb`. The command is refused if they do not close.
//! * A text's vertical justification and the width of its characters are not in a board text here: the position is moved by half the text's
//!   height to stand for a top or bottom justification, and the size is the text's height.

use super::Board;
use eda_kicad::dxf_import::{read_dxf, DxfOptions, DxfUnits};
use eda_kicad::graphics_import::{arc_points, prepare, spline_or_line, HJustify, Imported, ImportedShape, Mapper, SplineOrLine, Stroke, VJustify, V2};
use eda_model::ir::{Point, Shape, Text, TextJustify, Nm};
use eda_model::outline::EDGE_CUTS;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

/// The file formats Import Graphics reads (`GRAPHICS_IMPORT_MGR`'s plugins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphicsFormat {
    Dxf,
    Svg,
}

impl GraphicsFormat {
    /// The format a file name's extension names (`GRAPHICS_IMPORT_MGR::GetPluginByExt`).
    pub fn from_extension(name: &str) -> Option<GraphicsFormat> {
        match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
            "dxf" => Some(GraphicsFormat::Dxf),
            "svg" => Some(GraphicsFormat::Svg),
            _ => None,
        }
    }
}

fn d_one() -> f64 {
    1.0
}

fn d_dxf_width() -> Nm {
    Nm::new(200_000)
}

/// `Cmd::ImportGraphics`: what the Import Graphics dialog (`DIALOG_IMPORT_GRAPHICS`) hands `PlaceImportedGraphics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportGraphics {
    pub format: GraphicsFormat,
    /// The file's text.
    pub text: String,
    /// The layer the items go on ("Layer:" of the dialog; the active layer when it is not checked, which the caller resolves).
    pub layer: String,
    /// "DXF default units": the unit of a DXF that names none ([`DxfUnits::from_name`]); empty is millimetres.
    #[serde(default)]
    pub dxf_units: String,
    /// "Import scale:".
    #[serde(default = "d_one")]
    pub scale: f64,
    /// The importer's offset, in nanometres, added after the scale (`SetImportOffsetMM`): the dialog's "Place at" divided by the scale
    /// (KiCad's own arithmetic), or whatever puts the picture where it is wanted.
    #[serde(default)]
    pub offset_x_nm: i64,
    #[serde(default)]
    pub offset_y_nm: i64,
    /// "DXF default line width", nanometres. Not used for an SVG (its strokes carry their own widths).
    #[serde(default = "d_dxf_width")]
    pub line_width_nm: Nm,
    /// "Group imported items".
    #[serde(default)]
    pub group: bool,
    /// "Fix discontinuities" with its tolerance, nanometres; `None` leaves the shapes as they are.
    #[serde(default)]
    pub fix_tolerance_nm: Option<Nm>,
    /// Make the imported Edge.Cuts shapes the board's outline (see the module doc).
    #[serde(default)]
    pub replace_outline: bool,
}

impl ImportGraphics {
    /// The units the request names, or why it does not.
    pub fn units(&self) -> Result<DxfUnits, String> {
        DxfUnits::from_name(&self.dxf_units).ok_or_else(|| format!("unknown DXF unit \"{}\"", self.dxf_units))
    }

    /// The plugin's reading of the file: `Err` when it cannot be read at all.
    pub fn read(&self) -> Result<Imported, String> {
        match self.format {
            GraphicsFormat::Dxf => read_dxf(&self.text, &DxfOptions { units: self.units()?, line_width_mm: eda_model::units::to_mm(self.line_width_nm) }),
            GraphicsFormat::Svg => eda_kicad::svg_import::read_svg(&self.text),
        }
    }

    /// The mapping the dialog sets on the importer: `SetImportOffsetMM`, `SetScale`, `SetLineWidthMM` (an SVG has no default width).
    pub fn mapper(&self) -> Mapper {
        Mapper {
            scale: self.scale,
            offset_mm: V2::new(self.offset_x_nm as f64 / 1.0e6, self.offset_y_nm as f64 / 1.0e6),
            line_width_mm: if self.format == GraphicsFormat::Dxf { eda_model::units::to_mm(self.line_width_nm) } else { 0.0 },
            iu_per_mm: eda_model::units::PCB_IU_PER_MM,
        }
    }
}

/// What a file becomes on a board (`GRAPHICS_IMPORTER_PCBNEW`'s items), and what was said on the way.
#[derive(Debug, Clone, Default)]
pub struct BoardItems {
    pub shapes: Vec<Shape>,
    pub texts: Vec<Text>,
    pub messages: Vec<String>,
}

fn point(p: (i64, i64)) -> Point {
    Point { x: Nm::from_raw_shim(p.0), y: Nm::from_raw_shim(p.1) }
}

/// `MapStrokeParams`: a stroke of -1 is none (a width of nothing on the board), any other is mapped.
fn stroke_width(mapper: &Mapper, s: &Stroke) -> Nm {
    if s.width == -1.0 {
        Nm::ZERO
    } else {
        Nm::from_raw_shim(mapper.line_width(s.width))
    }
}

/// The items of `GRAPHICS_IMPORTER_PCBNEW` for `imported`, mapped with `mapper` (whose offset the size check may move), on `layer`.
pub fn board_items(imported: &Imported, mapper: &mut Mapper, layer: &str) -> BoardItems {
    let mut out = BoardItems::default();
    let shapes = &imported.shapes;
    let (fits, mut messages) = prepare(shapes, mapper);
    out.messages.append(&mut messages);
    if !fits {
        return out;
    }
    let m = *mapper;
    let layer = layer.to_string();
    // Arcs with a radius KiCad cannot hold become segments: "radius < MAX_INT / 2" in its nanometres.
    let max_radius_nm = i32::MAX as f64 / 2.0;
    for shape in shapes {
        match shape {
            ImportedShape::Line { start, end, stroke } => add_line(&mut out, &m, &layer, *start, *end, stroke),
            ImportedShape::Circle { center, radius, stroke, filled, .. } => {
                let c = m.coord(*center);
                let e = m.coord(V2::new(center.x + radius, center.y));
                out.shapes.push(Shape::Circle { id: String::new(), layer: layer.clone(), stroke_width: stroke_width(&m, stroke), filled: *filled, center: point(c), end: point(e) });
            }
            ImportedShape::Arc { center, start, angle_deg, stroke } => {
                let (s, mid, e) = arc_points(*center, *start, *angle_deg);
                let (cs, cc) = (m.coord(*start), m.coord(*center));
                let radius = ((cs.0 - cc.0) as f64).hypot((cs.1 - cc.1) as f64);
                if radius >= max_radius_nm {
                    // "Arc cannot be handled: convert it to a segment"
                    add_line(&mut out, &m, &layer, *start, e, stroke);
                    continue;
                }
                let (ps, pm, pe) = (m.coord(s), m.coord(mid), m.coord(e));
                let cross = (pm.0 - ps.0) as i128 * (pe.1 - ps.1) as i128 - (pm.1 - ps.1) as i128 * (pe.0 - ps.0) as i128;
                if cross == 0 {
                    // Three points on a line (an arc too small to hold a bend at this unit): a segment, or nothing.
                    if ps != pe {
                        out.shapes.push(Shape::Segment { id: String::new(), layer: layer.clone(), stroke_width: stroke_width(&m, stroke), filled: false, start: point(ps), end: point(pe) });
                    }
                    continue;
                }
                out.shapes.push(Shape::Arc { id: String::new(), layer: layer.clone(), stroke_width: stroke_width(&m, stroke), filled: false, start: point(ps), mid: point(pm), end: point(pe) });
            }
            ImportedShape::Polygon { pts, stroke, filled, .. } => {
                let mut mapped: Vec<Point> = pts.iter().map(|p| point(m.coord(*p))).collect();
                // A polygon needs three points to be one (`IsPolyShapeValid`).
                mapped.dedup();
                if mapped.len() > 1 && mapped.first() == mapped.last() {
                    mapped.pop();
                }
                if mapped.len() >= 3 {
                    out.shapes.push(Shape::Polygon { id: String::new(), layer: layer.clone(), stroke_width: stroke_width(&m, stroke), filled: *filled, pts: mapped });
                }
            }
            ImportedShape::Text { origin, text, height, thickness, orientation_deg, h, v, .. } => {
                let size = Nm::from_raw_shim((height * m.scaling_factor()).round() as i64);
                if size <= Nm::ZERO || text.is_empty() {
                    continue;
                }
                let at = m.coord(*origin);
                // The board's text is centred vertically on its position; a bottom or top justification moves it up or down by half its height,
                // along the text's own up direction.
                let a = orientation_deg.to_radians();
                let up = (-a.sin(), -a.cos());
                let shift = match v {
                    VJustify::Bottom => size.raw_shim() as f64 / 2.0,
                    VJustify::Top => -(size.raw_shim() as f64) / 2.0,
                    VJustify::Center => 0.0,
                };
                let at = Point { x: Nm::from_raw_shim(at.0 + (up.0 * shift).round() as i64), y: Nm::from_raw_shim(at.1 + (up.1 * shift).round() as i64) };
                let mut mdeg = (orientation_deg * 1000.0).round() as i64 % 360_000;
                if mdeg < 0 {
                    mdeg += 360_000;
                }
                let justify = match h {
                    HJustify::Left => TextJustify::Left,
                    HJustify::Center => TextJustify::Center,
                    HJustify::Right => TextJustify::Right,
                };
                out.texts.push(Text { id: String::new(), content: text.clone(), at, angle: mdeg as u32, layer: layer.clone(), size_nm: size, stroke_width: Nm::from_raw_shim(m.line_width(*thickness).max(1)), justify, mirror: false });
            }
            ImportedShape::Spline { start, c1, c2, end, stroke } => {
                let (ps, p1, p2, pe) = (m.coord(*start), m.coord(*c1), m.coord(*c2), m.coord(*end));
                match spline_or_line(ps, p1, p2, pe) {
                    SplineOrLine::Curve => out.shapes.push(Shape::Bezier { id: String::new(), layer: layer.clone(), stroke_width: stroke_width(&m, stroke), filled: false, start: point(ps), c1: point(p1), c2: point(p2), end: point(pe) }),
                    SplineOrLine::Line => out.shapes.push(Shape::Segment { id: String::new(), layer: layer.clone(), stroke_width: stroke_width(&m, stroke), filled: false, start: point(ps), end: point(pe) }),
                    SplineOrLine::Skip => {}
                }
            }
        }
    }
    out
}

/// `GRAPHICS_IMPORTER_PCBNEW::AddLine`: "Skip 0 len lines".
fn add_line(out: &mut BoardItems, m: &Mapper, layer: &str, start: V2, end: V2, stroke: &Stroke) {
    let (s, e) = (m.coord(start), m.coord(end));
    if s == e {
        return;
    }
    out.shapes.push(Shape::Segment { id: String::new(), layer: layer.to_string(), stroke_width: stroke_width(m, stroke), filled: false, start: point(s), end: point(e) });
}

impl Board<'_> {
    /// `Cmd::ImportGraphics`.
    pub(crate) fn import_graphics(&mut self, req: &ImportGraphics) -> Result<(), Vec<CheckResult>> {
        let fail = |check: &str, msg: String| Err(vec![CheckResult::fail(check, "import graphics", msg)]);
        if req.layer.is_empty() {
            return fail("ops_bad_import", "Please select a valid layer.".into());
        }
        if !self.layer_known(&req.layer) {
            return fail("ops_bad_import", format!("the board has no layer \"{}\"", req.layer));
        }
        if !(req.scale.is_finite() && req.scale != 0.0) {
            return fail("ops_bad_import", "the import scale must be a number other than zero".into());
        }
        let imported = req.read().map_err(|e| vec![CheckResult::fail("ops_bad_import", "import graphics", e)])?;
        let mut mapper = req.mapper();
        let mut items = board_items(&imported, &mut mapper, &req.layer);
        if items.shapes.is_empty() && items.texts.is_empty() {
            // "No graphic items found in file." -- and why, when the plugin or the size check said.
            let mut why = String::from("No graphic items found in file.");
            for m in items.messages.iter().chain(imported.messages.iter()) {
                why.push(' ');
                why.push_str(m);
            }
            return fail("ops_empty_import", why);
        }
        if let Some(tol) = req.fix_tolerance_nm {
            crate::fix_shapes::connect_board_shapes(&mut items.shapes, tol.max(Nm::ZERO));
        }
        let on_edge = req.layer == EDGE_CUTS;
        let flips_outline = req.replace_outline && on_edge && !items.shapes.is_empty() && !eda_model::outline::outline_is_shapes(&self.design);
        if flips_outline {
            // The new Edge.Cuts have to make an outline, or the board would lose the one it has for nothing.
            let outline = eda_drc::outline::board_outline_of(&items.shapes, false);
            if !outline.valid || outline.polys.outline_count() == 0 {
                return fail(
                    "ops_open_outline",
                    "the imported shapes on Edge.Cuts do not make a closed outline, so they cannot replace the board's; try \"Fix discontinuities\" with a larger tolerance, or import without replacing it".into(),
                );
            }
        }

        let dr = self.drawings_mut();
        let (first_shape, first_text) = (dr.shapes.len(), dr.texts.len());
        dr.shapes.extend(items.shapes);
        dr.texts.extend(items.texts);
        dr.assign_missing_ids();
        let new_ids: Vec<String> = dr.shapes[first_shape..].iter().map(|s| s.id().to_string()).chain(dr.texts[first_text..].iter().map(|t| t.id.clone())).collect();
        if flips_outline {
            dr.outline_is_shapes = true;
        }
        if req.group && new_ids.len() >= 2 {
            self.group_items(&new_ids)?;
        }
        if flips_outline {
            eda_drc::outline::refresh_outline_summary(&mut self.design, Some(self.model));
        }
        Ok(())
    }

    /// Is `layer` a layer this board has: a copper layer of the stack, a technical layer, or one of the nine user layers?
    fn layer_known(&self, layer: &str) -> bool {
        const TECH_LAYERS: [&str; 18] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];
        self.model.board.layers.iter().any(|c| c == layer) || TECH_LAYERS.contains(&layer) || layer.strip_prefix("User.").is_some_and(|n| n.parse::<u8>().is_ok_and(|n| (1..=9).contains(&n)))
    }
}
