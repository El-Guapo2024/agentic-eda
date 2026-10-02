//! Schematic plotting (SVG + PDF) -- port of `eeschema/sch_plotter.cpp`
//! (`SCH_PLOTTER::createSVGFiles`/`plotOneSheetSVG`, `createPDFFile`/
//! `plotOneSheetPDF`/`setupPlotPagePDF`) on top of [`crate::plotter`]'s
//! `PLOTTER` port, with the per-item `Plot()` methods of the schematic
//! classes (`SCH_SCREEN::Plot`, `SCH_LINE::Plot`, `SCH_JUNCTION::Plot`,
//! `SCH_NO_CONNECT::Plot`, `SCH_BUS_ENTRY_BASE::Plot`, `SCH_SYMBOL::Plot` ->
//! `LIB_SYMBOL::Plot` + `SCH_SHAPE::Plot` + `SCH_PIN::PlotPinType`/
//! `PlotPinTexts` + `SCH_FIELD::Plot`, `SCH_TEXT::Plot`,
//! `SCH_LABEL_BASE::Plot`, `SCH_SHEET::Plot`) and the drawing sheet
//! (`PlotDrawingSheet`, `common/plotters/common_plot_functions.cpp`, fed by
//! the real `defaultDrawingSheet` description, parsed with this crate's own
//! s-expression reader and laid out the way `DS_DATA_ITEM::SyncDrawItems`
//! does).
//!
//! The data is this workspace's own IR (`Design`/`ConstraintModel`), not a
//! `SCH_SCREEN`, so a few things KiCad reads from stored item properties
//! are derived instead (every place is called out inline and in
//! `web/studio/PARITY-sch.md` section 7):
//! - there are no junction items: dots come from the same geometric rule
//!   the studio canvas uses (`components/schematic/junctions.ts`);
//! - there are no per-field positions: Reference/Value are placed above and
//!   below the symbol body, centred (what the canvas does too);
//! - symbol `filled` is one bool, so a fill is plotted as `FILLED_SHAPE`
//!   (outline colour), never `FILLED_WITH_BG_BODYCOLOR`;
//! - label flag outlines are simplified polygons, not `CreateGraphicShape`.

use std::collections::BTreeMap;

use eda_model::ir::{Design, LabelKind, Point, SchematicSection, SymbolInstance, TitleBlock};
use eda_model::symbol::SPoint;
use eda_model::{CheckResult, ConstraintModel, LibPin, LibSymbol, SymbolGraphic};

use crate::plotter::pdf::PdfPlotter;
use crate::plotter::svg::SvgPlotter;
use crate::plotter::*;
use crate::sexpr::{self, Sexpr};

/// How the plot resolves a placed symbol's library symbol (real library,
/// built-in, or a synthesized generic box) -- supplied by the caller
/// because the synthesized box lives with the studio's JSON view.
pub type LibResolver<'a> = dyn Fn(&SymbolInstance) -> LibSymbol + 'a;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotFormat {
    Svg,
    Pdf,
}

/// `PageFormatReq` (`PAGE_SIZE_AUTO`, `PAGE_SIZE_A4`, `PAGE_SIZE_A`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PageSizeSelect {
    #[default]
    Auto,
    A4,
    A,
}

/// `SCH_PLOT_OPTS` (the fields the SVG/PDF paths read).
#[derive(Clone, Debug)]
pub struct SchPlotOpts {
    pub plot_all: bool,
    pub plot_drawing_sheet: bool,
    /// Hierarchical page numbers to keep (`m_plotPages`); empty = all.
    pub plot_pages: Vec<String>,
    pub black_and_white: bool,
    pub page_size_select: PageSizeSelect,
    pub use_background_color: bool,
    /// Which sheet `plot_all == false` plots: placement ids from the root
    /// (`SCHEMATIC::CurrentSheet`); empty = the root sheet.
    pub current_sheet: Vec<String>,
}

impl Default for SchPlotOpts {
    /// `SCH_PLOT_OPTS::SCH_PLOT_OPTS`.
    fn default() -> Self {
        SchPlotOpts { plot_all: true, plot_drawing_sheet: true, plot_pages: Vec::new(), black_and_white: false, page_size_select: PageSizeSelect::Auto, use_background_color: true, current_sheet: Vec::new() }
    }
}

/// Everything outside the design a plot needs (all passed in so output is
/// byte-deterministic, like `ExportMeta`).
#[derive(Clone, Debug)]
pub struct PlotMeta {
    /// `GetISO8601CurrentDateTime()`, e.g. `2026-10-02T12:00:00`.
    pub iso_date: String,
    /// Producer line for the drawing sheet's `${KICAD_VERSION}`.
    pub tool: String,
    /// Project/root schematic base name (`rootFn.GetName()`).
    pub project: String,
    /// Title/date used when the root sheet has no stored title block (the
    /// same fallback `export_kicad_sch` takes from `ExportMeta`).
    pub fallback_title: String,
    pub fallback_date: String,
}

#[derive(Clone, Debug)]
pub struct PlotFile {
    pub filename: String,
    pub bytes: Vec<u8>,
}

// ------------------------------------------------------------------ constants

/// `DEFAULT_LINE_WIDTH_MILS` / `DEFAULT_WIRE_WIDTH_MILS` = 6, `DEFAULT_BUS_WIDTH_MILS` = 12.
const DEFAULT_PEN: Iu = 1524;
const BUS_PEN: Iu = 3048;
/// `ADVANCED_CFG::m_MinPlotPenWidth` = 0.0212 mm.
const MIN_PEN: Iu = 212;
/// `DEFAULT_TEXT_SIZE` / `DEFAULT_PINNAME_SIZE` / `DEFAULT_PINNUM_SIZE` = 50 mils.
const TEXT_SIZE: Iu = 12700;
/// `DEFAULT_PIN_NAME_OFFSET`: 20 mils.
const PIN_NAME_OFFSET: Iu = 5080;
/// `PIN_TEXT_MARGIN` = 4 mils.
const PIN_TEXT_MARGIN: Iu = 1016;
/// `TARGET_PIN_RADIUS` = 15 mils.
const TARGET_PIN_RADIUS: Iu = 3810;
/// `m_PinSymbolSize` = `DEFAULT_TEXT_SIZE * IU_PER_MILS / 2`.
const PIN_SYMBOL_SIZE: Iu = 6350;
/// `DEFAULT_NOCONNECT_SIZE` = 48 mils.
const NOCONNECT_SIZE: Iu = 12192;
/// `SCHEMATIC_SETTINGS::GetJunctionSize()`: wire width x the default
/// junction multiplier (`junction_size_mult_list[3]` = 6.0).
const JUNCTION_DIAMETER: Iu = 9144;
/// `DEFAULT_LABEL_SIZE_RATIO` / `DEFAULT_TEXT_OFFSET_RATIO`.
const TEXT_OFFSET_RATIO: f64 = 0.15;

fn um_to_iu(um: i64) -> Iu {
    um * 10
}

fn mm_to_iu(mm: f64) -> Iu {
    ki_round(mm * IU_PER_MM)
}

fn pt_iu(p: Point) -> Pt {
    Pt::new(um_to_iu(p.x), um_to_iu(p.y))
}

// ---------------------------------------------------------------------- theme

/// The KiCad default colour theme (`common/settings/builtin_color_themes.h`),
/// the same generated data the studio canvas draws with.
struct Theme {
    colors: BTreeMap<String, Color>,
}

impl Theme {
    fn load() -> Theme {
        let raw = include_str!("../../../web/studio/src/kicad/colors.json");
        let v: serde_json::Value = serde_json::from_str(raw).expect("colors.json is checked in and generated; it must parse");
        let mut colors = BTreeMap::new();
        if let Some(obj) = v.get("colors").and_then(|c| c.as_object()) {
            for (k, c) in obj {
                if let Some(s) = c.as_str() {
                    colors.insert(k.clone(), Color::from_hex(s));
                }
            }
        }
        Theme { colors }
    }
    fn layer(&self, name: &str) -> Color {
        self.colors.get(name).copied().unwrap_or(Color::BLACK)
    }
}

// ----------------------------------------------------------------- sheet list

/// One entry of `SCH_SHEET_LIST` -- a sheet *instance* (a placement path).
#[derive(Clone, Debug)]
pub struct SheetEntry {
    /// Placement ids from the root (empty for the root itself).
    pub ids: Vec<String>,
    /// Placement names from the root.
    pub names: Vec<String>,
    /// The screen's file name (`SCH_SCREEN::GetFileName`).
    pub file: String,
    /// The screen's content with this placement's `instance_overrides`
    /// already applied (`SCH_SHEET_PATH::UpdateAllScreenReferences`).
    pub section: SchematicSection,
    /// Hierarchical page number, "1".. in tree order
    /// (`SortByHierarchicalPageNumbers`).
    pub page: String,
}

impl SheetEntry {
    /// `SCH_SHEET_PATH::PathHumanReadable`: `/` for the root,
    /// `/<name>/<name>/` below it.
    pub fn path_human_readable(&self) -> String {
        let mut s = String::from("/");
        for n in &self.names {
            s.push_str(n);
            s.push('/');
        }
        s
    }
    /// `SCH_SHEET_PATH::PathAsString`: `/` + each placement id + `/`.
    pub fn path_as_string(&self) -> String {
        let mut s = String::from("/");
        for n in &self.ids {
            s.push_str(n);
            s.push('/');
        }
        s
    }
}

/// `SCH_SHEET_LIST::BuildSheetList( root, true )`: the whole tree,
/// depth-first in placement order, root first.
pub fn sheet_list(design: &Design, project: &str) -> Vec<SheetEntry> {
    let mut out = Vec::new();
    let Some(root) = &design.schematic else { return out };
    out.push(SheetEntry { ids: vec![], names: vec![], file: format!("{project}.kicad_sch"), section: root.clone(), page: String::new() });
    fn visit(design: &Design, sch: &SchematicSection, ids: &mut Vec<String>, names: &mut Vec<String>, out: &mut Vec<SheetEntry>) {
        if ids.len() > 32 {
            return; // a file that (indirectly) places itself
        }
        for child in &sch.sheets {
            let Some(content) = design.sheet_contents.as_ref().and_then(|c| c.get(&child.file)) else { continue };
            ids.push(child.id.clone());
            names.push(child.name.clone());
            let mut section = content.clone();
            // `resolve_overrides` (hierarchy.rs): per-placement reference/unit.
            if !content.instance_overrides.is_empty() {
                for s in section.symbols.iter_mut() {
                    if let Some(ov) = content.instance_overrides.iter().find(|o| o.at == s.at && o.parent_sheet_instance_id == child.id) {
                        s.id = ov.reference.clone();
                        s.unit = ov.unit;
                    }
                }
            }
            out.push(SheetEntry { ids: ids.clone(), names: names.clone(), file: child.file.clone(), section, page: String::new() });
            visit(design, content, ids, names, out);
            ids.pop();
            names.pop();
        }
    }
    visit(design, root, &mut Vec::new(), &mut Vec::new(), &mut out);
    for (i, e) in out.iter_mut().enumerate() {
        e.page = (i + 1).to_string();
    }
    out
}

// ------------------------------------------------------------- transform math

/// `sch_symbol.cpp::SetOrientation`'s eight matrices, as `transform.ts`
/// tabulates them: `[x1, y1, x2, y2]`, `x' = x1 x + y1 y`, `y' = x2 x + y2 y`.
fn orientation_matrix(rot_deg: f64, mirror: Option<char>) -> [f64; 4] {
    let r = (((rot_deg / 90.0).round() as i64 * 90) % 360 + 360) % 360;
    match (r, mirror) {
        (0, None) => [1.0, 0.0, 0.0, 1.0],
        (90, None) => [0.0, 1.0, -1.0, 0.0],
        (180, None) => [-1.0, 0.0, 0.0, -1.0],
        (270, None) => [0.0, -1.0, 1.0, 0.0],
        (0, Some('x')) => [1.0, 0.0, 0.0, -1.0],
        (0, Some('y')) => [-1.0, 0.0, 0.0, 1.0],
        (90, Some('x')) => [0.0, 1.0, 1.0, 0.0],
        (90, Some('y')) => [0.0, -1.0, -1.0, 0.0],
        (180, Some('x')) => [-1.0, 0.0, 0.0, 1.0],
        (180, Some('y')) => [1.0, 0.0, 0.0, -1.0],
        (270, Some('x')) => [0.0, -1.0, -1.0, 0.0],
        (270, Some('y')) => [0.0, 1.0, 1.0, 0.0],
        _ => [1.0, 0.0, 0.0, 1.0],
    }
}

/// A placed symbol's library-frame -> sheet-IU mapping
/// (`SCH_RENDER_SETTINGS::TransformCoordinate` + `m_pos`).
struct Placement {
    m: [f64; 4],
    at_um: (f64, f64),
}

impl Placement {
    fn new(at: Point, rot_millideg: u32, mirrored: bool, mirror_y: bool) -> Placement {
        let mirror = if mirrored {
            Some('y')
        } else if mirror_y {
            Some('x')
        } else {
            None
        };
        Placement { m: orientation_matrix(rot_millideg as f64 / 1000.0, mirror), at_um: (at.x as f64, at.y as f64) }
    }
    /// A library point (mm, file Y-up) to sheet IU: Y-flip once, instance
    /// matrix, translate.
    fn pt(&self, p: SPoint) -> Pt {
        let (ix, iy) = (p.x * 1000.0, -p.y * 1000.0);
        let tx = self.m[0] * ix + self.m[1] * iy;
        let ty = self.m[2] * ix + self.m[3] * iy;
        Pt::new(ki_round((tx + self.at_um.0) * 10.0), ki_round((ty + self.at_um.1) * 10.0))
    }
    fn vec(&self, v: (f64, f64)) -> (f64, f64) {
        (self.m[0] * v.0 + self.m[1] * v.1, self.m[2] * v.0 + self.m[3] * v.1)
    }
}

// --------------------------------------------------------------------- context

struct Ctx<'a> {
    theme: &'a Theme,
    resolver: &'a LibResolver<'a>,
    meta: &'a PlotMeta,
}

fn eff_pen(w: Iu) -> Iu {
    (if w > 0 { w } else { DEFAULT_PEN }).max(MIN_PEN)
}

/// `EDA_TEXT::GetEffectiveTextPenWidth` + the clamps `SCH_FIELD::Plot` /
/// `SCH_TEXT::Plot` / `SCH_LABEL_BASE::Plot` apply (non-bold text).
fn text_pen(size: Iu) -> Iu {
    let mut pen = DEFAULT_PEN;
    pen = pen.min(ki_round(size as f64 * 0.25)); // ClampTextPenSize( pen, size )
    pen = pen.max(MIN_PEN);
    pen.min(size / 4)
}

fn plot_text(p: &mut dyn Plotter, pos: Pt, color: Color, s: &str, angle_deg: f64, size: Iu, h: HAlign, v: VAlign, pen: Iu) {
    let mut a = TextAttrs::new(size, h, v, pen);
    a.angle_deg = angle_deg;
    p.text(pos, color, s, &a);
}

// ------------------------------------------------------------- symbol plotting

fn graphic_visible(unit: u32, inst_unit: u32) -> bool {
    unit == 0 || unit == inst_unit
}

/// `SCH_SHAPE::Plot` (foreground half; background fills are
/// `FILLED_WITH_BG_BODYCOLOR` only, which the IR cannot express).
fn plot_graphic(p: &mut dyn Plotter, ctx: &Ctx, g: &SymbolGraphic, pl: &Placement, base_color: Color) {
    let set = |p: &mut dyn Plotter, stroke_mm: f64| {
        let pen = eff_pen(mm_to_iu(stroke_mm));
        p.set_color(base_color);
        p.set_current_line_width(pen);
        p.set_dash(pen, LineStyle::Solid);
        pen
    };
    match g {
        SymbolGraphic::Rectangle { start, end, stroke_mm, filled, .. } => {
            let pen = set(p, *stroke_mm);
            p.rect(pl.pt(*start), pl.pt(*end), if *filled { FillT::FilledShape } else { FillT::NoFill }, pen);
        }
        SymbolGraphic::Polyline { pts, stroke_mm, filled, .. } => {
            let pen = set(p, *stroke_mm);
            let pts: Vec<Pt> = pts.iter().map(|q| pl.pt(*q)).collect();
            p.plot_poly(&pts, if *filled { FillT::FilledShape } else { FillT::NoFill }, pen);
        }
        SymbolGraphic::Circle { center, radius_mm, stroke_mm, filled, .. } => {
            let pen = set(p, *stroke_mm);
            p.circle(pl.pt(*center), mm_to_iu(*radius_mm * 2.0), if *filled { FillT::FilledShape } else { FillT::NoFill }, pen);
        }
        SymbolGraphic::Arc { start, mid, end, stroke_mm, filled, .. } => {
            let pen = set(p, *stroke_mm);
            let f = |q: &SPoint| {
                let w = pl.pt(*q);
                (w.x as f64, w.y as f64)
            };
            p.arc_3pt(f(start), f(mid), f(end), if *filled { FillT::FilledShape } else { FillT::NoFill }, pen);
        }
        SymbolGraphic::Text { text, at, angle_deg, size_mm, .. } => {
            let size = mm_to_iu(*size_mm);
            // Text in a library symbol: horizontal stays horizontal unless the
            // transform swaps axes (`SCH_TEXT::Plot`'s `LAYER_DEVICE` branch).
            let swapped = pl.m[0] == 0.0;
            let mut ang = *angle_deg;
            if swapped {
                ang += 90.0;
            }
            plot_text(p, pl.pt(*at), base_color, text, ang, size, HAlign::Center, VAlign::Center, text_pen(size));
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
    let _ = ctx;
}

/// `SCH_PIN::PlotPinType`.
fn plot_pin_type(p: &mut dyn Plotter, ctx: &Ctx, pin: &LibPin, pos: Pt, orient: char, length: Iu) {
    let color = ctx.theme.layer("LAYER_PIN");
    let pen = DEFAULT_PEN;
    p.set_color(color);
    p.set_current_line_width(pen);
    let (mut map_x1, mut map_y1) = (0, 0);
    let (mut x1, mut y1) = (pos.x, pos.y);
    match orient {
        'U' => {
            y1 = pos.y - length;
            map_y1 = 1;
        }
        'D' => {
            y1 = pos.y + length;
            map_y1 = -1;
        }
        'L' => {
            x1 = pos.x - length;
            map_x1 = 1;
        }
        _ => {
            x1 = pos.x + length;
            map_x1 = -1;
        }
    }
    let shape = pin.shape.as_str();
    let deco = PIN_SYMBOL_SIZE;
    let ext = PIN_SYMBOL_SIZE;
    if shape == "inverted" || shape == "inverted_clock" {
        p.circle(Pt::new(map_x1 * ext + x1, map_y1 * ext + y1), ext * 2, FillT::NoFill, pen);
        p.move_to(Pt::new(map_x1 * ext * 2 + x1, map_y1 * ext * 2 + y1));
        p.finish_to(pos);
    } else if shape == "edge_clock_high" || shape == "falling_edge_clock" {
        if map_y1 == 0 {
            p.move_to(Pt::new(x1, y1 + deco));
            p.line_to(Pt::new(x1 + map_x1 * deco * 2, y1));
            p.finish_to(Pt::new(x1, y1 - deco));
        } else {
            p.move_to(Pt::new(x1 + deco, y1));
            p.line_to(Pt::new(x1, y1 + map_y1 * deco * 2));
            p.finish_to(Pt::new(x1 - deco, y1));
        }
        p.move_to(Pt::new(map_x1 * deco * 2 + x1, map_y1 * deco * 2 + y1));
        p.finish_to(pos);
    } else {
        p.move_to(Pt::new(x1, y1));
        p.finish_to(pos);
    }
    if matches!(shape, "clock" | "inverted_clock" | "clock_low") {
        if map_y1 == 0 {
            p.move_to(Pt::new(x1, y1 + deco));
            p.line_to(Pt::new(x1 - map_x1 * deco * 2, y1));
            p.finish_to(Pt::new(x1, y1 - deco));
        } else {
            p.move_to(Pt::new(x1 + deco, y1));
            p.line_to(Pt::new(x1, y1 - map_y1 * deco * 2));
            p.finish_to(Pt::new(x1 - deco, y1));
        }
    }
    if matches!(shape, "input_low" | "clock_low") {
        if map_y1 == 0 {
            p.move_to(Pt::new(x1 + map_x1 * ext * 2, y1));
            p.line_to(Pt::new(x1 + map_x1 * ext * 2, y1 - ext * 2));
            p.finish_to(Pt::new(x1, y1));
        } else {
            p.move_to(Pt::new(x1, y1 + map_y1 * ext * 2));
            p.line_to(Pt::new(x1 - ext * 2, y1 + map_y1 * ext * 2));
            p.finish_to(Pt::new(x1, y1));
        }
    }
    if shape == "output_low" {
        if map_y1 == 0 {
            p.move_to(Pt::new(x1, y1 - ext * 2));
            p.finish_to(Pt::new(x1 + map_x1 * ext * 2, y1));
        } else {
            p.move_to(Pt::new(x1 - ext * 2, y1));
            p.finish_to(Pt::new(x1, y1 + map_y1 * ext * 2));
        }
    } else if shape == "non_logic" {
        p.move_to(Pt::new(x1 - (map_x1 + map_y1) * ext, y1 - (map_y1 - map_x1) * ext));
        p.finish_to(Pt::new(x1 + (map_x1 + map_y1) * ext, y1 + (map_y1 - map_x1) * ext));
        p.move_to(Pt::new(x1 - (map_x1 - map_y1) * ext, y1 - (map_y1 + map_x1) * ext));
        p.finish_to(Pt::new(x1 + (map_x1 - map_y1) * ext, y1 + (map_y1 + map_x1) * ext));
    }
    if pin.electrical_type == "no_connect" {
        // Draw a N.C. symbol
        let d = TARGET_PIN_RADIUS;
        p.move_to(Pt::new(pos.x - d, pos.y - d));
        p.finish_to(Pt::new(pos.x + d, pos.y + d));
        p.move_to(Pt::new(pos.x + d, pos.y - d));
        p.finish_to(Pt::new(pos.x - d, pos.y + d));
    }
}

/// `SCH_PIN::PlotPinTexts` (names inside the body at the symbol's pin-name
/// offset, numbers above the pin line; single-line numbers only).
fn plot_pin_texts(p: &mut dyn Plotter, ctx: &Ctx, pin: &LibPin, pos: Pt, orient: char, length: Iu) {
    let mut draw_name = !pin.name.is_empty() && pin.name != "~";
    let draw_num = !pin.number.is_empty();
    if !draw_name && !draw_num {
        return;
    }
    if pin.electrical_type == "no_connect" && !draw_name {
        draw_name = false;
    }
    let pen = DEFAULT_PEN;
    let name_offset = PIN_TEXT_MARGIN + pen;
    let num_offset = PIN_TEXT_MARGIN + pen;
    let name_color = ctx.theme.layer("LAYER_PINNAM");
    let num_color = ctx.theme.layer("LAYER_PINNUM");
    let (mut x1, mut y1) = (pos.x, pos.y);
    match orient {
        'U' => y1 -= length,
        'D' => y1 += length,
        'L' => x1 -= length,
        _ => x1 += length,
    }
    let inside = PIN_NAME_OFFSET;
    let size = TEXT_SIZE;
    let horizontal = orient == 'L' || orient == 'R';
    if inside > 0 {
        if horizontal {
            if draw_name {
                if orient == 'R' {
                    plot_text(p, Pt::new(x1 + inside, y1), name_color, &pin.name, 0.0, size, HAlign::Left, VAlign::Center, pen);
                } else {
                    plot_text(p, Pt::new(x1 - inside, y1), name_color, &pin.name, 0.0, size, HAlign::Right, VAlign::Center, pen);
                }
            }
            if draw_num {
                plot_text(p, Pt::new((x1 + pos.x) / 2, y1 - num_offset), num_color, &pin.number, 0.0, size, HAlign::Center, VAlign::Bottom, pen);
            }
        } else {
            if orient == 'D' {
                if draw_name {
                    plot_text(p, Pt::new(x1, y1 + inside), name_color, &pin.name, 90.0, size, HAlign::Right, VAlign::Center, pen);
                }
            } else if draw_name {
                plot_text(p, Pt::new(x1, y1 - inside), name_color, &pin.name, 90.0, size, HAlign::Left, VAlign::Center, pen);
            }
            if draw_num {
                plot_text(p, Pt::new(x1 - num_offset, (y1 + pos.y) / 2), num_color, &pin.number, 90.0, size, HAlign::Center, VAlign::Bottom, pen);
            }
        }
    } else if horizontal {
        if draw_name && draw_num {
            plot_text(p, Pt::new((x1 + pos.x) / 2, y1 - name_offset), name_color, &pin.name, 0.0, size, HAlign::Center, VAlign::Bottom, pen);
            plot_text(p, Pt::new((x1 + pos.x) / 2, y1 + num_offset), num_color, &pin.number, 0.0, size, HAlign::Center, VAlign::Top, pen);
        } else if draw_name {
            plot_text(p, Pt::new((x1 + pos.x) / 2, y1 - name_offset), name_color, &pin.name, 0.0, size, HAlign::Center, VAlign::Bottom, pen);
        } else if draw_num {
            plot_text(p, Pt::new((x1 + pos.x) / 2, y1 - name_offset), num_color, &pin.number, 0.0, size, HAlign::Center, VAlign::Bottom, pen);
        }
    } else if draw_name && draw_num {
        plot_text(p, Pt::new(x1 - name_offset, (y1 + pos.y) / 2), name_color, &pin.name, 90.0, size, HAlign::Center, VAlign::Bottom, pen);
        plot_text(p, Pt::new(x1 + num_offset, (y1 + pos.y) / 2), num_color, &pin.number, 90.0, size, HAlign::Center, VAlign::Top, pen);
    } else if draw_name {
        plot_text(p, Pt::new(x1 - name_offset, (y1 + pos.y) / 2), name_color, &pin.name, 90.0, size, HAlign::Center, VAlign::Bottom, pen);
    } else if draw_num {
        plot_text(p, Pt::new(x1 - num_offset, (y1 + pos.y) / 2), num_color, &pin.number, 90.0, size, HAlign::Center, VAlign::Bottom, pen);
    }
}

/// `SCH_PIN::Plot` for one placed pin.
fn plot_pin(p: &mut dyn Plotter, ctx: &Ctx, pin: &LibPin, pl: &Placement, hide_names_numbers: bool) {
    let len_iu = mm_to_iu(pin.length_mm);
    if len_iu <= 0 {
        return; // KiCad hides zero-length (power) pins
    }
    let tip = pl.pt(pin.at);
    // `PinDrawOrient`: the direction from the connection point toward the
    // body, through the instance transform. File convention: 0 = right,
    // 90 = up; internally (Y-down) up is -y.
    let a = (((pin.angle_deg / 90.0).round() as i64 * 90) % 360 + 360) % 360;
    let d_internal = match a {
        0 => (1.0, 0.0),
        90 => (0.0, -1.0),
        180 => (-1.0, 0.0),
        _ => (0.0, 1.0),
    };
    let d = pl.vec(d_internal);
    let orient = if d.0 > 0.5 {
        'R'
    } else if d.0 < -0.5 {
        'L'
    } else if d.1 < 0.0 {
        'U'
    } else {
        'D'
    };
    plot_pin_type(p, ctx, pin, tip, orient, len_iu);
    if !hide_names_numbers {
        plot_pin_texts(p, ctx, pin, tip, orient, len_iu);
    }
}

/// World-space bounding box of a placed symbol's visible graphics and
/// pins -- what the studio canvas anchors ref/value text to
/// (`resolveLibSymbol().bbox`).
fn symbol_bbox(lib: &LibSymbol, pl: &Placement, unit: u32) -> Option<(Pt, Pt)> {
    let mut pts: Vec<Pt> = Vec::new();
    for g in lib.graphics.iter().filter(|g| graphic_visible(g.unit(), unit)) {
        match g {
            SymbolGraphic::Rectangle { start, end, .. } => pts.extend([pl.pt(*start), pl.pt(*end)]),
            SymbolGraphic::Polyline { pts: ps, .. } => pts.extend(ps.iter().map(|q| pl.pt(*q))),
            SymbolGraphic::Circle { center, radius_mm, .. } => {
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    pts.push(pl.pt(SPoint::new(center.x + dx * radius_mm, center.y + dy * radius_mm)));
                }
            }
            SymbolGraphic::Arc { start, mid, end, .. } => pts.extend([pl.pt(*start), pl.pt(*mid), pl.pt(*end)]),
            SymbolGraphic::Text { .. } => {}
        }
    }
    for pin in lib.pins.iter().filter(|q| graphic_visible(q.unit, unit)) {
        pts.push(pl.pt(pin.at));
    }
    let first = *pts.first()?;
    let (mut lo, mut hi) = (first, first);
    for q in &pts {
        lo = Pt::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = Pt::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    Some((lo, hi))
}

/// Multi-unit reference suffix (`LIB_SYMBOL::SubReference`): unit 1 = "A".
fn unit_suffix(unit: u32, unit_count: u32) -> String {
    if unit_count <= 1 || unit == 0 {
        return String::new();
    }
    char::from_u32('A' as u32 + (unit - 1) % 26).map(|c| c.to_string()).unwrap_or_default()
}

/// `SCH_SYMBOL::Plot`.
fn plot_symbol(p: &mut dyn Plotter, ctx: &Ctx, sym: &SymbolInstance, model: &ConstraintModel) {
    let lib = (ctx.resolver)(sym);
    let pl = Placement::new(sym.at, sym.rot, sym.mirrored, sym.mirror_y);
    let unit = sym.unit.max(1);
    let device = if ctx.theme.colors.is_empty() { Color::BLACK } else { ctx.theme.layer("LAYER_DEVICE") };
    // `for( bool local_background : { true, false } )`: the background half
    // only ever fills with FILLED_WITH_BG_BODYCOLOR, which the IR cannot
    // carry, so only the foreground half draws anything.
    for g in lib.graphics.iter().filter(|g| graphic_visible(g.unit(), unit)) {
        plot_graphic(p, ctx, g, &pl, device);
    }
    let hide = lib.power; // power symbols' pins are hidden (`(hide yes)`) in KiCad's libraries
    if !hide {
        for pin in lib.pins.iter().filter(|q| graphic_visible(q.unit, unit)) {
            plot_pin(p, ctx, pin, &pl, false);
        }
    }

    // Fields: Reference above, Value below the body, centred.
    let value = if sym.value.is_empty() { model.part(&sym.id).and_then(|q| q.value.clone()).unwrap_or_default() } else { sym.value.clone() };
    if let Some((lo, hi)) = symbol_bbox(&lib, &pl, unit) {
        let cx = (lo.x + hi.x) / 2;
        let pen = text_pen(TEXT_SIZE);
        let reference = format!("{}{}", sym.id, unit_suffix(unit, lib.unit_count));
        if !reference.is_empty() && !reference.starts_with('#') {
            plot_text(p, Pt::new(cx, lo.y - TEXT_SIZE), ctx.theme.layer("LAYER_REFERENCEPART"), &reference, 0.0, TEXT_SIZE, HAlign::Center, VAlign::Center, pen);
        }
        if !value.is_empty() && !value.starts_with('~') {
            plot_text(p, Pt::new(cx, hi.y + TEXT_SIZE), ctx.theme.layer("LAYER_VALUEPART"), &value, 0.0, TEXT_SIZE, HAlign::Center, VAlign::Center, pen);
        }
    }
}

/// A power symbol is a `SCH_SYMBOL` too (`SCH_SYMBOL::Plot`): its library
/// graphics plus the Value field (the net name) next to the body.
fn plot_power_symbol(p: &mut dyn Plotter, ctx: &Ctx, ps: &eda_model::ir::PowerSymbol, model: &ConstraintModel) {
    let lib = model.symbol_of(&ps.lib_id);
    let pl = Placement::new(ps.at, ps.rot, false, false);
    let device = ctx.theme.layer("LAYER_DEVICE");
    let Some(lib) = lib else {
        let r = 1524;
        p.set_color(device);
        p.circle(pt_iu(ps.at), r, FillT::FilledShape, 0);
        return;
    };
    for g in lib.graphics.iter().filter(|g| graphic_visible(g.unit(), 1)) {
        plot_graphic(p, ctx, g, &pl, device);
    }
    let at = pt_iu(ps.at);
    let (lo, hi) = symbol_bbox(&lib, &pl, 1).unwrap_or((at, at));
    // Text goes on the side the body extends toward (GND below, rails above).
    let below = (lo.y + hi.y) / 2 > at.y;
    let (y, v) = if below { (hi.y + TEXT_SIZE, VAlign::Center) } else { (lo.y - TEXT_SIZE, VAlign::Center) };
    plot_text(p, Pt::new(at.x, y), ctx.theme.layer("LAYER_VALUEPART"), &ps.net, 0.0, TEXT_SIZE, HAlign::Center, v, text_pen(TEXT_SIZE));
}

// -------------------------------------------------------------- other items

/// `SCH_LINE::Plot` for each segment of a wire/bus polyline.
fn plot_wire(p: &mut dyn Plotter, ctx: &Ctx, w: &eda_model::ir::Wire) {
    let (layer, pen) = if w.bus { ("LAYER_BUS", BUS_PEN) } else { ("LAYER_WIRE", DEFAULT_PEN) };
    let pen = pen.max(MIN_PEN);
    for seg in w.pts.windows(2) {
        p.set_color(ctx.theme.layer(layer));
        p.set_current_line_width(pen);
        p.set_dash(pen, LineStyle::Solid);
        p.move_to(pt_iu(seg[0]));
        p.finish_to(pt_iu(seg[1]));
        p.set_dash(pen, LineStyle::Solid);
    }
}

/// `SCH_BUS_ENTRY_BASE::Plot`.
fn plot_bus_entry(p: &mut dyn Plotter, ctx: &Ctx, be: &eda_model::ir::BusEntry) {
    let pen = DEFAULT_PEN.max(MIN_PEN);
    p.set_current_line_width(pen);
    p.set_color(ctx.theme.layer("LAYER_WIRE"));
    p.set_dash(pen, LineStyle::Solid);
    p.move_to(pt_iu(be.at));
    p.finish_to(pt_iu(Point { x: be.at.x + be.size.x, y: be.at.y + be.size.y }));
    p.set_dash(pen, LineStyle::Solid);
}

/// `SCH_NO_CONNECT::Plot`.
fn plot_no_connect(p: &mut dyn Plotter, ctx: &Ctx, nc: &eda_model::ir::NoConnect) {
    let delta = NOCONNECT_SIZE / 2;
    let c = pt_iu(nc.at);
    p.set_current_line_width(DEFAULT_PEN.max(MIN_PEN));
    p.set_color(ctx.theme.layer("LAYER_NOCONNECT"));
    p.move_to(Pt::new(c.x - delta, c.y - delta));
    p.finish_to(Pt::new(c.x + delta, c.y + delta));
    p.move_to(Pt::new(c.x + delta, c.y - delta));
    p.finish_to(Pt::new(c.x - delta, c.y + delta));
}

/// `SCH_JUNCTION::Plot`.
fn plot_junction(p: &mut dyn Plotter, ctx: &Ctx, at: Point) {
    p.set_current_line_width(DEFAULT_PEN.max(MIN_PEN));
    p.set_color(ctx.theme.layer("LAYER_JUNCTION"));
    p.circle(pt_iu(at), JUNCTION_DIAMETER, FillT::FilledShape, 0);
}

fn on_segment_interior(p: Point, a: Point, b: Point) -> bool {
    if a.x == b.x {
        p.x == a.x && p.y > a.y.min(b.y) && p.y < a.y.max(b.y)
    } else if a.y == b.y {
        p.y == a.y && p.x > a.x.min(b.x) && p.x < a.x.max(b.x)
    } else {
        false
    }
}

/// Junction dots, derived (the IR stores none) -- a port of
/// `components/schematic/junctions.ts::junctionPoints`: 3+ same-net wire
/// endpoints coincident, or a wire end / label / power symbol landing on
/// another wire's interior.
fn junction_points(sch: &SchematicSection) -> Vec<Point> {
    let mut counts: BTreeMap<(String, i64, i64), (Point, usize)> = BTreeMap::new();
    for w in sch.wires.iter().filter(|w| !w.pts.is_empty()) {
        for pt in [w.pts[0], *w.pts.last().unwrap()] {
            let e = counts.entry((w.net.clone(), pt.x, pt.y)).or_insert((pt, 0));
            e.1 += 1;
        }
    }
    let mut dots: Vec<Point> = counts.values().filter(|e| e.1 >= 3).map(|e| e.0).collect();
    let add = |dots: &mut Vec<Point>, p: Point| {
        if !dots.contains(&p) {
            dots.push(p);
        }
    };
    let mut candidates: Vec<Point> = sch.power_symbols.iter().map(|q| q.at).chain(sch.labels.iter().map(|l| l.at)).collect();
    for w in sch.wires.iter().filter(|w| !w.pts.is_empty()) {
        candidates.push(w.pts[0]);
        candidates.push(*w.pts.last().unwrap());
    }
    for other in &sch.wires {
        for seg in other.pts.windows(2) {
            for c in &candidates {
                if on_segment_interior(*c, seg[0], seg[1]) {
                    add(&mut dots, *c);
                }
            }
        }
    }
    dots
}

/// Which way a label's text should extend: away from the wire that ends at
/// its anchor (`SPIN_STYLE`); the IR stores no spin.
fn label_spin(sch: &SchematicSection, at: Point) -> char {
    for w in &sch.wires {
        for (i, pt) in w.pts.iter().enumerate() {
            if *pt != at {
                continue;
            }
            let neighbour = if i > 0 { w.pts[i - 1] } else { *w.pts.get(1).unwrap_or(pt) };
            if neighbour == at {
                continue;
            }
            return if neighbour.x < at.x {
                'R' // wire on the left: text extends right
            } else if neighbour.x > at.x {
                'L'
            } else if neighbour.y > at.y {
                'U'
            } else {
                'B'
            };
        }
    }
    'R'
}

/// `SCH_LABEL_BASE::Plot`: the text offset above/left of the anchor
/// (`GetSchematicTextOffset`), and for global/hierarchical labels the flag
/// outline (a simplified `CreateGraphicShape`).
fn plot_label(p: &mut dyn Plotter, ctx: &Ctx, sch: &SchematicSection, l: &eda_model::ir::NetLabel) {
    let (layer, shape) = match &l.kind {
        LabelKind::Local => ("LAYER_LOCLABEL", None),
        LabelKind::Global { shape } => ("LAYER_GLOBLABEL", Some(*shape)),
        LabelKind::Hierarchical { shape } => ("LAYER_HIERLABEL", Some(*shape)),
    };
    let color = ctx.theme.layer(layer);
    let size = TEXT_SIZE;
    let pen = text_pen(size);
    p.set_color(color);
    p.set_current_line_width(pen);
    let at = pt_iu(l.at);
    let spin = label_spin(sch, l.at);
    let dist = ki_round(TEXT_OFFSET_RATIO * size as f64);
    let global_pad = if shape.is_some() { size } else { 0 };
    // SCH_LABEL_BASE::GetSchematicTextOffset: offset perpendicular to the text direction.
    let (pos, angle, h) = match spin {
        'R' => (Pt::new(at.x + global_pad, at.y - dist), 0.0, HAlign::Left),
        'L' => (Pt::new(at.x - global_pad, at.y - dist), 0.0, HAlign::Right),
        'U' => (Pt::new(at.x - dist, at.y - global_pad), 90.0, HAlign::Left),
        _ => (Pt::new(at.x - dist, at.y + global_pad), 90.0, HAlign::Right),
    };
    let valign = if shape.is_some() { VAlign::Center } else { VAlign::Bottom };
    let pos = if shape.is_some() { Pt::new(pos.x, at.y) } else { pos };
    plot_text(p, pos, color, &l.net, angle, size, h, valign, pen);
    if shape.is_some() {
        // A flag body around the text: height = size + padding, length = text width + 2 * padding.
        let w = text_width(&l.net, (size, size), pen, false);
        let half = size / 2 + pen;
        let len = w + 2 * size;
        let (a, b) = match spin {
            'R' => ((at.x, at.y), (at.x + len, at.y)),
            'L' => ((at.x - len, at.y), (at.x, at.y)),
            'U' => ((at.x, at.y - len), (at.x, at.y)),
            _ => ((at.x, at.y), (at.x, at.y + len)),
        };
        let poly: Vec<Pt> = if matches!(spin, 'R' | 'L') {
            vec![Pt::new(a.0, a.1), Pt::new(a.0 + half, a.1 - half), Pt::new(b.0 - half, b.1 - half), Pt::new(b.0, b.1), Pt::new(b.0 - half, b.1 + half), Pt::new(a.0 + half, a.1 + half), Pt::new(a.0, a.1)]
        } else {
            vec![Pt::new(a.0, a.1), Pt::new(a.0 - half, a.1 + half), Pt::new(b.0 - half, b.1 - half), Pt::new(b.0, b.1), Pt::new(b.0 + half, b.1 - half), Pt::new(a.0 + half, a.1 + half), Pt::new(a.0, a.1)]
        };
        p.set_color(ctx.theme.layer(layer));
        p.plot_poly(&poly, FillT::NoFill, pen);
    }
}

/// `SCH_TEXT::Plot` (the non-`LAYER_DEVICE` branch) for free text.
fn plot_free_text(p: &mut dyn Plotter, ctx: &Ctx, t: &eda_model::ir::SchematicText) {
    let size = um_to_iu(t.size_um);
    let pen = text_pen(size);
    p.set_current_line_width(pen);
    for (i, line) in t.content.split('\n').enumerate() {
        let interline = ki_round(size as f64 * 1.68 * 0.9583);
        let a = (t.angle as f64 / 1000.0).to_radians();
        let off = i as Iu * interline;
        let at = pt_iu(t.at);
        let pos = Pt::new(at.x - ki_round(off as f64 * a.sin()), at.y + ki_round(off as f64 * a.cos()));
        plot_text(p, pos, ctx.theme.layer("LAYER_NOTES"), line, t.angle as f64 / 1000.0, size, HAlign::Left, VAlign::Bottom, pen);
    }
}

/// `SCH_SHEET::Plot` (+ its fields and pins).
fn plot_sheet(p: &mut dyn Plotter, ctx: &Ctx, s: &eda_model::ir::SheetInstance, background: bool) {
    if background && !p.color_mode() {
        return;
    }
    let border = ctx.theme.layer("LAYER_SHEET");
    let bg = ctx.theme.layer("LAYER_SHEET_BACKGROUND");
    let a = pt_iu(s.at);
    let b = Pt::new(a.x + um_to_iu(s.size.0), a.y + um_to_iu(s.size.1));
    if background && bg.a > 0.0 {
        p.set_color(bg);
        p.rect(a, b, FillT::FilledShape, 1);
    } else {
        p.set_color(border);
        p.rect(a, b, FillT::NoFill, DEFAULT_PEN.max(MIN_PEN));
    }
    let pen = text_pen(TEXT_SIZE);
    // SHEET_NAME above the top-left corner, SHEET_FILENAME below the bottom-left.
    plot_text(p, Pt::new(a.x, a.y - ki_round(TEXT_SIZE as f64 * TEXT_OFFSET_RATIO) - pen), ctx.theme.layer("LAYER_SHEETNAME"), &s.name, 0.0, TEXT_SIZE, HAlign::Left, VAlign::Bottom, pen);
    plot_text(p, Pt::new(a.x, b.y + ki_round(TEXT_SIZE as f64 * TEXT_OFFSET_RATIO) + pen), ctx.theme.layer("LAYER_SHEETFILENAME"), &format!("File: {}", s.file), 0.0, TEXT_SIZE, HAlign::Left, VAlign::Top, pen);
    for pin in &s.pins {
        let at = pt_iu(pin.at);
        let on_left = at.x <= a.x;
        let h = if on_left { HAlign::Left } else { HAlign::Right };
        let x = if on_left { at.x + TEXT_SIZE } else { at.x - TEXT_SIZE };
        plot_text(p, Pt::new(x, at.y), ctx.theme.layer("LAYER_SHEETLABEL"), &pin.name, 0.0, TEXT_SIZE, h, VAlign::Center, pen);
    }
}

/// `SCH_SCREEN::Plot( aPlotter, aPlotOpts, items )`: the background pass
/// over every item, then the foreground pass in descending `KICAD_T`
/// order, then junctions.
fn plot_screen(p: &mut dyn Plotter, ctx: &Ctx, model: &ConstraintModel, sch: &SchematicSection) {
    // Background pass: only sheets draw anything (`SCH_SHEET::Plot`).
    for s in &sch.sheets {
        plot_sheet(p, ctx, s, true);
    }
    // Foreground, `a->Type() > b->Type()`: SHEET, SYMBOL, HIER_LABEL, GLOBAL_LABEL,
    // LABEL, LINE (buses before wires: higher layer id first), BUS_WIRE_ENTRY,
    // NO_CONNECT, TEXT.
    for s in &sch.sheets {
        plot_sheet(p, ctx, s, false);
    }
    for sym in &sch.symbols {
        plot_symbol(p, ctx, sym, model);
    }
    for ps in &sch.power_symbols {
        plot_power_symbol(p, ctx, ps, model);
    }
    for kind in ["hier", "global", "local"] {
        for l in &sch.labels {
            let k = match l.kind {
                LabelKind::Hierarchical { .. } => "hier",
                LabelKind::Global { .. } => "global",
                LabelKind::Local => "local",
            };
            if k == kind {
                plot_label(p, ctx, sch, l);
            }
        }
    }
    for w in sch.wires.iter().filter(|w| w.bus) {
        plot_wire(p, ctx, w);
    }
    for w in sch.wires.iter().filter(|w| !w.bus) {
        plot_wire(p, ctx, w);
    }
    for be in &sch.bus_entries {
        plot_bus_entry(p, ctx, be);
    }
    for nc in &sch.no_connects {
        plot_no_connect(p, ctx, nc);
    }
    for t in &sch.texts {
        plot_free_text(p, ctx, t);
    }
    for at in junction_points(sch) {
        plot_junction(p, ctx, at);
    }
}

// --------------------------------------------------------------- drawing sheet

/// `defaultDrawingSheet` (`common/drawing_sheet/drawing_sheet_default_description.cpp`), verbatim.
const DEFAULT_DRAWING_SHEET: &str = concat!(
    "(kicad_wks (version 20210606) (generator pl_editor)\n",
    "(setup (textsize 1.5 1.5)(linewidth 0.15)(textlinewidth 0.15)\n",
    "(left_margin 10)(right_margin 10)(top_margin 10)(bottom_margin 10))\n",
    "(rect (name \"\") (start 110 34) (end 2 2) (comment \"rect around the title block\"))\n",
    "(rect (name \"\") (start 0 0 ltcorner) (end 0 0) (repeat 2) (incrx 2) (incry 2))\n",
    "(line (name \"\") (start 50 2 ltcorner) (end 50 0 ltcorner) (repeat 30) (incrx 50))\n",
    "(tbtext \"1\" (name \"\") (pos 25 1 ltcorner) (font (size 1.3 1.3)) (repeat 100) (incrx 50))\n",
    "(line (name \"\") (start 50 2 lbcorner) (end 50 0 lbcorner) (repeat 30) (incrx 50))\n",
    "(tbtext \"1\" (name \"\") (pos 25 1 lbcorner) (font (size 1.3 1.3)) (repeat 100) (incrx 50))\n",
    "(line (name \"\") (start 0 50 ltcorner) (end 2 50 ltcorner) (repeat 30) (incry 50))\n",
    "(tbtext \"A\" (name \"\") (pos 1 25 ltcorner) (font (size 1.3 1.3)) (justify center) (repeat 100) (incry 50))\n",
    "(line (name \"\") (start 0 50 rtcorner) (end 2 50 rtcorner) (repeat 30) (incry 50))\n",
    "(tbtext \"A\" (name \"\") (pos 1 25 rtcorner) (font (size 1.3 1.3)) (justify center) (repeat 100) (incry 50))\n",
    "(tbtext \"Date: ${ISSUE_DATE}\" (name \"\") (pos 87 6.9))\n",
    "(line (name \"\") (start 110 5.5) (end 2 5.5))\n",
    "(tbtext \"${KICAD_VERSION}\" (name \"\") (pos 109 4.1) (comment \"Kicad version\"))\n",
    "(line (name \"\") (start 110 8.5) (end 2 8.5))\n",
    "(tbtext \"Rev: ${REVISION}\" (name \"\") (pos 24 6.9) (font bold))\n",
    "(tbtext \"Size: ${PAPER}\" (name \"\") (pos 109 6.9) (comment \"Paper format name\"))\n",
    "(tbtext \"Id: ${#}/${##}\" (name \"\") (pos 24 4.1) (comment \"Sheet id\"))\n",
    "(line (name \"\") (start 110 12.5) (end 2 12.5))\n",
    "(tbtext \"Title: ${TITLE}\" (name \"\") (pos 109 10.7) (font (size 2 2) bold italic))\n",
    "(tbtext \"File: ${FILENAME}\" (name \"\") (pos 109 14.3))\n",
    "(line (name \"\") (start 110 18.5) (end 2 18.5))\n",
    "(tbtext \"Sheet: ${SHEETPATH}\" (name \"\") (pos 109 17))\n",
    "(tbtext \"${COMPANY}\" (name \"\") (pos 109 20) (font bold) (comment \"Company name\"))\n",
    "(tbtext \"${COMMENT1}\" (name \"\") (pos 109 23) (comment \"Comment 0\"))\n",
    "(tbtext \"${COMMENT2}\" (name \"\") (pos 109 26) (comment \"Comment 1\"))\n",
    "(tbtext \"${COMMENT3}\" (name \"\") (pos 109 29) (comment \"Comment 2\"))\n",
    "(tbtext \"${COMMENT4}\" (name \"\") (pos 109 32) (comment \"Comment 3\"))\n",
    "(line (name \"\") (start 90 8.5) (end 90 5.5))\n",
    "(line (name \"\") (start 26 8.5) (end 26 2))\n",
    ")\n"
);

#[derive(Clone, Copy, PartialEq)]
enum Corner {
    Lt,
    Lb,
    Rb,
    Rt,
}

/// `POINT_COORD`.
#[derive(Clone, Copy)]
struct Coord {
    pos: (f64, f64),
    anchor: Corner,
}

#[derive(Clone, Copy, PartialEq)]
enum DsKind {
    Line,
    Rect,
    Text,
}

/// `DS_DATA_ITEM` (+ `DS_DATA_ITEM_TEXT`).
struct DsItem {
    kind: DsKind,
    start: Coord,
    end: Coord,
    repeat: i64,
    incr: (f64, f64),
    incr_label: i64,
    line_width: f64,
    text: String,
    size: (f64, f64),
    bold: bool,
    italic: bool,
    h: HAlign,
    v: VAlign,
    orient: f64,
}

struct DsModel {
    items: Vec<DsItem>,
    default_text_size: (f64, f64),
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

fn num(l: &[Sexpr], i: usize) -> f64 {
    sexpr::num(l, i).unwrap_or(0.0)
}

fn parse_coord(l: &[Sexpr]) -> Coord {
    let mut c = Coord { pos: (num(l, 1), num(l, 2)), anchor: Corner::Rb };
    for a in l.iter().skip(3) {
        match a.text() {
            Some("ltcorner") => c.anchor = Corner::Lt,
            Some("lbcorner") => c.anchor = Corner::Lb,
            Some("rbcorner") => c.anchor = Corner::Rb,
            Some("rtcorner") => c.anchor = Corner::Rt,
            _ => {}
        }
    }
    c
}

/// `DRAWING_SHEET_PARSER` for the constructs the default sheet uses.
fn parse_drawing_sheet(src: &str) -> DsModel {
    let root = sexpr::parse(src).expect("the default drawing sheet parses");
    let items_l = root.as_list().unwrap_or(&[]);
    let mut m = DsModel { items: Vec::new(), default_text_size: (1.5, 1.5), left: 10.0, right: 10.0, top: 10.0, bottom: 10.0 };
    for it in items_l.iter().skip(1) {
        let Some(l) = it.as_list() else { continue };
        match sexpr::tag(l) {
            Some("setup") => {
                if let Some(t) = sexpr::find(l, "textsize") {
                    m.default_text_size = (num(t, 1), num(t, 2));
                }
                if let Some(t) = sexpr::find(l, "left_margin") {
                    m.left = num(t, 1);
                }
                if let Some(t) = sexpr::find(l, "right_margin") {
                    m.right = num(t, 1);
                }
                if let Some(t) = sexpr::find(l, "top_margin") {
                    m.top = num(t, 1);
                }
                if let Some(t) = sexpr::find(l, "bottom_margin") {
                    m.bottom = num(t, 1);
                }
            }
            Some(k @ ("line" | "rect" | "tbtext")) => {
                let kind = match k {
                    "line" => DsKind::Line,
                    "rect" => DsKind::Rect,
                    _ => DsKind::Text,
                };
                let default_c = Coord { pos: (0.0, 0.0), anchor: Corner::Rb };
                let mut d = DsItem { kind, start: default_c, end: default_c, repeat: 1, incr: (0.0, 0.0), incr_label: 1, line_width: 0.0, text: String::new(), size: (0.0, 0.0), bold: false, italic: false, h: HAlign::Left, v: VAlign::Center, orient: 0.0 };
                if kind == DsKind::Text {
                    d.text = sexpr::txt(l, 1).unwrap_or("").to_string();
                }
                for a in l.iter().skip(1) {
                    let Some(al) = a.as_list() else { continue };
                    match sexpr::tag(al) {
                        Some("start") | Some("pos") => d.start = parse_coord(al),
                        Some("end") => d.end = parse_coord(al),
                        Some("repeat") => d.repeat = num(al, 1) as i64,
                        Some("incrx") => d.incr.0 = num(al, 1),
                        Some("incry") => d.incr.1 = num(al, 1),
                        Some("incrlabel") => d.incr_label = num(al, 1) as i64,
                        Some("linewidth") => d.line_width = num(al, 1),
                        Some("rotate") => d.orient = num(al, 1),
                        Some("font") => {
                            for f in al.iter().skip(1) {
                                match f {
                                    Sexpr::Atom(w) if w == "bold" => d.bold = true,
                                    Sexpr::Atom(w) if w == "italic" => d.italic = true,
                                    Sexpr::List(sl) if sexpr::tag(sl) == Some("size") => d.size = (num(sl, 1), num(sl, 2)),
                                    _ => {}
                                }
                            }
                        }
                        Some("justify") => {
                            for j in al.iter().skip(1) {
                                match j.text() {
                                    Some("center") => {
                                        d.h = HAlign::Center;
                                        d.v = VAlign::Center;
                                    }
                                    Some("left") => d.h = HAlign::Left,
                                    Some("right") => d.h = HAlign::Right,
                                    Some("top") => d.v = VAlign::Top,
                                    Some("bottom") => d.v = VAlign::Bottom,
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
                m.items.push(d);
            }
            _ => {}
        }
    }
    m
}

/// `TITLE_BLOCK::TextVarResolver` + `DS_DRAW_ITEM_LIST::BuildFullText`.
struct DsVars {
    kicad_version: String,
    page: String,
    count: usize,
    sheet_path: String,
    file_name: String,
    paper: String,
    title: String,
    company: String,
    rev: String,
    date: String,
    comments: Vec<String>,
}

fn build_full_text(base: &str, v: &DsVars) -> String {
    let mut out = String::new();
    let b: Vec<char> = base.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '$' && b.get(i + 1) == Some(&'{') {
            if let Some(end) = b[i + 2..].iter().position(|c| *c == '}') {
                let token: String = b[i + 2..i + 2 + end].iter().collect();
                let val = match token.as_str() {
                    "KICAD_VERSION" => v.kicad_version.clone(),
                    "#" => v.page.clone(),
                    "##" => v.count.to_string(),
                    "SHEETPATH" => v.sheet_path.clone(),
                    "FILENAME" => v.file_name.clone(),
                    "PAPER" => v.paper.clone(),
                    "TITLE" => v.title.clone(),
                    "COMPANY" => v.company.clone(),
                    "REVISION" => v.rev.clone(),
                    "ISSUE_DATE" => v.date.clone(),
                    t if t.starts_with("COMMENT") => t[7..].parse::<usize>().ok().and_then(|n| v.comments.get(n.wrapping_sub(1)).cloned()).unwrap_or_default(),
                    _ => String::new(),
                };
                out.push_str(&val);
                i += 2 + end + 1;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// `DS_DATA_ITEM_TEXT::IncrementLabel`.
fn increment_label(base: &str, incr: i64) -> String {
    let mut chars: Vec<char> = base.chars().collect();
    let Some(last) = chars.pop() else { return String::new() };
    if last.is_ascii_digit() {
        chars.extend((incr + (last as i64 - '0' as i64)).to_string().chars());
    } else if let Some(c) = char::from_u32((last as i64 + incr) as u32) {
        chars.push(c);
    }
    chars.into_iter().collect()
}

/// `PlotDrawingSheet` over the default drawing sheet (`DS_DRAW_ITEM_LIST::BuildDrawItemsList`).
fn plot_drawing_sheet(p: &mut dyn Plotter, page: &PageInfo, vars: &DsVars, color: Color) {
    let m = parse_drawing_sheet(DEFAULT_DRAWING_SHEET);
    // `DS_DATA_MODEL::SetupDrawEnvironment`
    let lt = (m.left, m.top);
    let rb = (page.width_mils * 25.4 / 1000.0 - m.right, page.height_mils * 25.4 / 1000.0 - m.bottom);
    let abs = |c: &Coord, incr: (f64, f64), ii: i64| -> (f64, f64) {
        let pos = (c.pos.0 + incr.0 * ii as f64, c.pos.1 + incr.1 * ii as f64);
        match c.anchor {
            Corner::Rb => (rb.0 - pos.0, rb.1 - pos.1),
            Corner::Rt => (rb.0 - pos.0, lt.1 + pos.1),
            Corner::Lb => (lt.0 + pos.0, rb.1 - pos.1),
            Corner::Lt => (lt.0 + pos.0, lt.1 + pos.1),
        }
    };
    let inside = |pt: (f64, f64)| !(rb.0 < pt.0 || lt.0 > pt.0 || rb.1 < pt.1 || lt.1 > pt.1);
    let iu = |v: (f64, f64)| Pt::new(ki_round(v.0 * IU_PER_MM), ki_round(v.1 * IU_PER_MM));

    let plot_color = if p.color_mode() { color } else { Color::BLACK };
    for it in &m.items {
        let mut label = build_full_text(&it.text, vars);
        for j in 0..it.repeat {
            let (s, e) = (abs(&it.start, it.incr, j), abs(&it.end, it.incr, j));
            if j > 0 && !(inside(s) && inside(e)) {
                continue;
            }
            p.set_color(plot_color);
            match it.kind {
                DsKind::Line => {
                    let pen = mm_to_iu(if it.line_width != 0.0 { it.line_width } else { 0.15 }).max(DEFAULT_PEN);
                    p.set_current_line_width(pen);
                    p.move_to(iu(s));
                    p.finish_to(iu(e));
                }
                DsKind::Rect => {
                    let pen = mm_to_iu(if it.line_width != 0.0 { it.line_width } else { 0.15 }).max(DEFAULT_PEN);
                    let (a, b) = (iu(s), iu(e));
                    p.set_current_line_width(pen);
                    p.move_to(a);
                    p.line_to(Pt::new(b.x, a.y));
                    p.line_to(Pt::new(b.x, b.y));
                    p.line_to(Pt::new(a.x, b.y));
                    p.finish_to(a);
                }
                DsKind::Text => {
                    let sz = (if it.size.0 == 0.0 { m.default_text_size.0 } else { it.size.0 }, if it.size.1 == 0.0 { m.default_text_size.1 } else { it.size.1 });
                    let size = (mm_to_iu(sz.0), mm_to_iu(sz.1));
                    let mut pen = mm_to_iu(if it.line_width != 0.0 { it.line_width } else { 0.15 });
                    if it.bold {
                        pen = ki_round(size.0.min(size.1) as f64 / 5.0);
                    }
                    // GetEffectiveTextPenWidth: clamp to a quarter of the size; then at least the default pen.
                    pen = pen.min(ki_round(size.0.min(size.1) as f64 * 0.25)).max(DEFAULT_PEN);
                    if !label.is_empty() {
                        let mut a = TextAttrs::new(size.0, it.h, it.v, pen);
                        a.size = size;
                        a.angle_deg = it.orient;
                        a.italic = it.italic;
                        a.bold = it.bold;
                        p.text(iu(s), plot_color, &label, &a);
                    }
                    if it.repeat > 1 {
                        label = increment_label(&build_full_text(&it.text, vars), (j + 1) * it.incr_label);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the plotter

fn title_block_of<'a>(entry: &'a SheetEntry, meta: &PlotMeta, owned: &'a mut Option<TitleBlock>) -> &'a TitleBlock {
    if let Some(tb) = &entry.section.title_block {
        return tb;
    }
    *owned = Some(TitleBlock {
        title: if entry.ids.is_empty() { meta.fallback_title.clone() } else { String::new() },
        date: if entry.ids.is_empty() { meta.fallback_date.clone() } else { String::new() },
        rev: String::new(),
        company: String::new(),
        comments: Vec::new(),
    });
    owned.as_ref().unwrap()
}

fn ds_vars(entry: &SheetEntry, count: usize, meta: &PlotMeta, tb: &TitleBlock) -> DsVars {
    DsVars {
        kicad_version: meta.tool.clone(),
        page: entry.page.clone(),
        count,
        sheet_path: entry.path_human_readable(),
        file_name: entry.file.clone(),
        paper: PageInfo::A4.name.to_string(),
        title: tb.title.clone(),
        company: tb.company.clone(),
        rev: tb.rev.clone(),
        date: tb.date.clone(),
        comments: tb.comments.clone(),
    }
}

/// `plotOneSheetSVG` / `plotOneSheetPDF` body, after the plotter is set up.
fn plot_one_sheet(p: &mut dyn Plotter, ctx: &Ctx, model: &ConstraintModel, entry: &SheetEntry, count: usize, opts: &SchPlotOpts, actual: PageInfo) {
    if opts.use_background_color && p.color_mode() {
        p.set_color(ctx.theme.layer("LAYER_SCHEMATIC_BACKGROUND"));
        // Use page size selected in schematic to know the schematic bg area
        p.rect(Pt::new(0, 0), Pt::new(actual.width_iu(), actual.height_iu()), FillT::FilledShape, 1);
    }
    if opts.plot_drawing_sheet {
        let mut owned = None;
        let tb = title_block_of(entry, ctx.meta, &mut owned).clone();
        let vars = ds_vars(entry, count, ctx.meta, &tb);
        let color = ctx.theme.layer("LAYER_SCHEMATIC_DRAWINGSHEET");
        plot_drawing_sheet(p, &actual, &vars, color);
    }
    plot_screen(p, ctx, model, &entry.section);
}

/// `PAGE_SIZE_*` selection: the plot page and the scale `std::min( scalex, scaley )`.
fn plot_page(sel: PageSizeSelect, actual: PageInfo) -> (PageInfo, f64) {
    let page = match sel {
        PageSizeSelect::Auto => actual,
        PageSizeSelect::A4 => PageInfo::A4,
        PageSizeSelect::A => PageInfo { name: "A", width_mils: 11000.0, height_mils: 8500.0 },
    };
    let sx = page.width_mils / actual.width_mils;
    let sy = page.height_mils / actual.height_mils;
    (page, sx.min(sy))
}

/// `D:YYYY:MM:DD:HH:MM:SS` out of an ISO-8601 stamp.
fn pdf_date(iso: &str) -> String {
    let d: Vec<&str> = iso.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).collect();
    let g = |i: usize| d.get(i).copied().unwrap_or("00");
    format!("D:{}:{}:{}:{}:{}:{}", g(0), g(1), g(2), g(3), g(4), g(5))
}

#[cfg(test)]
#[path = "sch_plot_tests.rs"]
pub(crate) mod tests;

/// `SCH_PLOTTER::PlotSchematic` for SVG/PDF. SVG: one file per sheet
/// (`createSVGFiles`, named `GetUniqueFilenameForCurrentSheet` =
/// `<root>-<sheet>-<sheet>`); PDF: one multi-page file (`createPDFFile`,
/// one page per sheet, `<root>.pdf`).
pub fn plot_schematic(design: &Design, model: &ConstraintModel, format: PlotFormat, opts: &SchPlotOpts, meta: &PlotMeta, resolver: &LibResolver) -> Result<Vec<PlotFile>, Vec<CheckResult>> {
    if design.schematic.is_none() {
        return Err(vec![CheckResult::fail("plot.no_schematic", "design", "design has no schematic section to plot")]);
    }
    let all = sheet_list(design, &meta.project);
    let count = all.len();
    let mut sheets: Vec<SheetEntry> = if opts.plot_all {
        all.clone()
    } else {
        let cur = all.iter().find(|e| e.ids == opts.current_sheet).or_else(|| all.first()).cloned();
        cur.into_iter().collect()
    };
    if opts.plot_all && !opts.plot_pages.is_empty() {
        // `TrimToPageNumbers`
        sheets.retain(|e| opts.plot_pages.contains(&e.page));
    }
    if sheets.is_empty() {
        return Err(vec![CheckResult::fail("plot.no_sheets", "design", "No sheets to plot.")]);
    }
    let theme = Theme::load();
    let ctx = Ctx { theme: &theme, resolver, meta };
    let actual = PageInfo::A4; // the IR carries no paper size; the exporter writes `(paper "A4")`
    let (page, scale) = plot_page(opts.page_size_select, actual);
    let unique_name = |e: &SheetEntry| -> String {
        let mut n = meta.project.clone();
        for s in &e.names {
            n.push('-');
            n.push_str(s);
        }
        n.replace(['/', '\\'], "_")
    };

    match format {
        PlotFormat::Svg => {
            let mut files = Vec::new();
            for e in &sheets {
                let filename = format!("{}.svg", unique_name(e));
                let mut svg = SvgPlotter::new(&meta.iso_date);
                svg.set_page_settings(page);
                svg.set_color_mode(!opts.black_and_white);
                svg.set_viewport(Pt::default(), IUS_PER_DECIMIL, scale, false);
                svg.set_creator("Eeschema-SVG");
                svg.set_file_name(&filename);
                svg.start_plot(&e.page);
                plot_one_sheet(&mut svg, &ctx, model, e, count, opts, actual);
                files.push(PlotFile { filename, bytes: svg.end_plot().into_bytes() });
            }
            Ok(files)
        }
        PlotFormat::Pdf => {
            let filename = format!("{}.pdf", unique_name(&sheets[0]));
            let mut pdf = PdfPlotter::new(&pdf_date(&meta.iso_date));
            pdf.set_color_mode(!opts.black_and_white);
            pdf.set_creator("Eeschema-PDF");
            let root_tb = all[0].section.title_block.clone();
            pdf.set_title(&root_tb.map(|t| t.title).filter(|t| !t.is_empty()).unwrap_or_else(|| meta.fallback_title.clone()));
            pdf.set_file_name(&filename);
            for (i, e) in sheets.iter().enumerate() {
                let name = e.names.last().cloned().unwrap_or_default(); // the root sheet's SHEET_NAME field is empty
                if i == 0 {
                    pdf.set_page_settings(page);
                    pdf.set_viewport(Pt::default(), IUS_PER_DECIMIL, scale, false);
                    pdf.start_plot(&e.page, &name);
                } else {
                    // For the following pages you need to close the (finished) page,
                    // reconfigure, and then start a new one.
                    pdf.close_page();
                    pdf.set_page_settings(page);
                    pdf.set_viewport(Pt::default(), IUS_PER_DECIMIL, scale, false);
                    // The parent page: the sheet path minus its last placement.
                    let parent = all.iter().find(|a| a.ids.len() + 1 == e.ids.len() && e.ids.starts_with(&a.ids));
                    let (ppage, pname) = parent.map(|a| (a.page.clone(), a.names.last().cloned().unwrap_or_default())).unwrap_or_default();
                    pdf.start_page(&e.page, &name, &ppage, &pname);
                }
                plot_one_sheet(&mut pdf, &ctx, model, e, count, opts, actual);
            }
            Ok(vec![PlotFile { filename, bytes: pdf.end_plot() }])
        }
    }
}
