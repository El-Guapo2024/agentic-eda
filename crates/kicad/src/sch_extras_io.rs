//! `.kicad_sch` read/write of the schematic editor's drawn graphics ([`eda_model::sch_extras::SchGraphic`]):
//! rectangles, circles, arcs, beziers, filled polygons, text boxes, rule areas and directive labels
//! (`netclass_flag`), written the way `SCH_IO_KICAD_SEXPR` (`saveShape`, `saveTextBox`, `saveRuleArea`,
//! `saveText`) writes them and read back from the same tokens.

use eda_model::units::mm;
use crate::sexpr::{self, Sexpr};
use crate::{duid_for, sch_mm, sexpr_str};
use eda_model::ir::{Millideg, Point, SchematicSection, Nm};
use eda_model::sch_extras::{DirectiveShape, JunctionLook, SchColor, SchFill, SchGraphic, SchGraphicKind, SchHAlign, SchLineStyle, SchStroke, SchVAlign};
use std::fmt::Write;

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// An alpha 0..=255 as KiCad's 0..1 decimal (`FormatDouble2Str`: shortest form, up to 4 places).
fn alpha_str(a: u8) -> String {
    let v = (a as f64 / 255.0 * 10_000.0).round() / 10_000.0;
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn color_str(c: SchColor) -> String {
    format!("{} {} {} {}", c.r, c.g, c.b, alpha_str(c.a))
}

fn stroke_str(g: &SchGraphic) -> String {
    stroke_params(g.width_nm, g.line_style, g.color)
}

/// `(stroke (width w) (type t) [(color ...)])` (`STROKE_PARAMS::Format`).
fn stroke_params(width_nm: Nm, style: SchLineStyle, color: Option<SchColor>) -> String {
    let mut s = format!("(stroke (width {}) (type {})", sch_mm(width_nm), style.token());
    if let Some(c) = color {
        let _ = write!(s, " (color {})", color_str(c));
    }
    s.push(')');
    s
}

/// The stroke of a wire, bus, bus entry or graphic line as `SCH_IO_KICAD_SEXPR::saveLine` / `saveBusEntry` write it: the stroke Wire/Bus Properties set
/// (`SchExtras::strokes`), the default one when none was. A graphic line's width is its own (`width_nm`), the entry carries the rest.
pub(crate) fn line_stroke_str(width_nm: Nm, stroke: Option<&SchStroke>) -> String {
    let st = stroke.copied().unwrap_or_default();
    stroke_params(if width_nm != Nm::ZERO { width_nm } else { st.width_nm }, st.style, st.color)
}

/// `(diameter d) (color r g b a)` of a junction (`SCH_IO_KICAD_SEXPR::saveJunction`): zero diameter and a colour of all zeroes are the defaults.
pub(crate) fn junction_attrs(look: Option<&JunctionLook>) -> String {
    let look = look.copied().unwrap_or_default();
    format!("(diameter {}) (color {})", sch_mm(look.diameter_nm), look.color.map(color_str).unwrap_or_else(|| "0 0 0 0".to_string()))
}

fn fill_str(g: &SchGraphic) -> String {
    match (g.fill, g.fill_color) {
        (SchFill::Color, Some(c)) => format!("(fill (type color) (color {}))", color_str(c)),
        (f, _) => format!("(fill (type {}))", f.token()),
    }
}

fn pts_str(pts: &[Point]) -> String {
    pts.iter().map(|p| format!("(xy {} {})", sch_mm(p.x), sch_mm(p.y))).collect::<Vec<_>>().join(" ")
}

fn angle_str(m: Millideg) -> String {
    let d = m as f64 / 1000.0;
    if d == d.trunc() {
        format!("{}", d as i64)
    } else {
        format!("{d}")
    }
}

/// The `(effects ...)` of a free-standing schematic text: its size and how `j` justifies it, written as `EDA_TEXT::Format` does (centre is not named, so a
/// centred text has no `(justify ...)`).
pub(crate) fn text_effects_str(size_nm: Nm, j: eda_model::sch_extras::SchTextJustify) -> String {
    use eda_model::ir::{TextJustify, TextVAlign};
    let h = match j.h {
        TextJustify::Left => SchHAlign::Left,
        TextJustify::Center => SchHAlign::Center,
        TextJustify::Right => SchHAlign::Right,
    };
    let v = match j.v {
        TextVAlign::Top => SchVAlign::Top,
        TextVAlign::Center => SchVAlign::Center,
        TextVAlign::Bottom => SchVAlign::Bottom,
    };
    effects_str(size_nm, false, false, h, v)
}

/// What a free-standing text's `(effects ...)` and `(at x y angle)` say about how it lies: the text's angle made upright (`KeepUpright`: along the sheet or up
/// it) and its justification, which a file without `(justify ...)` gives as centre, centre (`parseEDA_TEXT`).
pub(crate) fn read_text_lie(item: &[Sexpr], look: &EffectsLook) -> (Millideg, eda_model::sch_extras::SchTextJustify) {
    use eda_model::ir::{TextJustify, TextVAlign};
    let angle_deg = sexpr::find(item, "at").and_then(|a| sexpr::num(a, 3)).unwrap_or(0.0);
    let angle = if eda_model::kicad_geom::keep_upright_is_vertical(angle_deg) { 90_000 } else { 0 };
    let has = |w: &str| look.justify.iter().any(|j| j == w);
    let h = if has("left") {
        TextJustify::Left
    } else if has("right") {
        TextJustify::Right
    } else {
        TextJustify::Center
    };
    let v = if has("top") {
        TextVAlign::Top
    } else if has("bottom") {
        TextVAlign::Bottom
    } else {
        TextVAlign::Center
    };
    (angle, eda_model::sch_extras::SchTextJustify { h, v })
}

/// The `(effects ...)` of a text box: font size, bold/italic and the justification (a box defaults to left/top).
fn effects_str(size_nm: Nm, bold: bool, italic: bool, h: SchHAlign, v: SchVAlign) -> String {
    let mut f = format!("(font (size {0} {0})", sch_mm(size_nm));
    if bold {
        f.push_str(" (bold yes)");
    }
    if italic {
        f.push_str(" (italic yes)");
    }
    f.push(')');
    let mut j = String::new();
    match h {
        SchHAlign::Left => j.push_str(" left"),
        SchHAlign::Right => j.push_str(" right"),
        SchHAlign::Center => {}
    }
    match v {
        SchVAlign::Top => j.push_str(" top"),
        SchVAlign::Bottom => j.push_str(" bottom"),
        SchVAlign::Center => {}
    }
    if j.is_empty() {
        format!("(effects {f})")
    } else {
        format!("(effects {f} (justify{j}))")
    }
}

/// `(scale s)` in millionths as the decimal a file holds: no trailing zeros (`{:g}` writes six significant digits; this writes them all).
pub(crate) fn scale_str(scale_ppm: i64) -> String {
    let whole = scale_ppm / 1_000_000;
    let frac = scale_ppm.rem_euclid(1_000_000);
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{frac:06}").trim_end_matches('0'))
    }
}

/// `(data ..)` of an image: the base64 in lines of 76 characters, each its own quoted string (`FormatStreamData`), indented `indent`.
pub(crate) fn data_str(data: &str, indent: &str) -> String {
    let mut out = format!("{indent}(data");
    for line in data.as_bytes().chunks(76) {
        let _ = write!(out, "\n{indent}\t\"{}\"", std::str::from_utf8(line).unwrap_or(""));
    }
    let _ = write!(out, "\n{indent})\n");
    out
}

/// Write every graphic of `sch` (drawing order), each with its `(locked yes)` when locked.
pub(crate) fn write_graphics(out: &mut String, sch: &SchematicSection) {
    for g in &sch.extras.graphics {
        let uuid = duid_for(&format!("sch_graphic:{}", g.id_seed()), &g.id);
        let locked = if sch.extras.is_locked(&g.id) { "\n\t\t(locked yes)" } else { "" };
        match &g.shape {
            SchGraphicKind::Rectangle { start, end, corner_radius_nm } => {
                let radius = if *corner_radius_nm > Nm::ZERO { format!("\n\t\t(radius {})", sch_mm(*corner_radius_nm)) } else { String::new() };
                let _ = writeln!(out, "\t(rectangle\n\t\t(start {} {})\n\t\t(end {} {}){radius}\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", sch_mm(start.x), sch_mm(start.y), sch_mm(end.x), sch_mm(end.y), stroke_str(g), fill_str(g));
            }
            SchGraphicKind::Circle { center, radius_nm } => {
                let _ = writeln!(out, "\t(circle\n\t\t(center {} {})\n\t\t(radius {})\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", sch_mm(center.x), sch_mm(center.y), sch_mm(*radius_nm), stroke_str(g), fill_str(g));
            }
            SchGraphicKind::Arc { start, mid, end } => {
                let _ = writeln!(out, "\t(arc\n\t\t(start {} {})\n\t\t(mid {} {})\n\t\t(end {} {})\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", sch_mm(start.x), sch_mm(start.y), sch_mm(mid.x), sch_mm(mid.y), sch_mm(end.x), sch_mm(end.y), stroke_str(g), fill_str(g));
            }
            SchGraphicKind::Bezier { start, c1, c2, end } => {
                let _ = writeln!(out, "\t(bezier\n\t\t(pts {})\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", pts_str(&[*start, *c1, *c2, *end]), stroke_str(g), fill_str(g));
            }
            SchGraphicKind::Polygon { pts } => {
                let _ = writeln!(out, "\t(polyline\n\t\t(pts {})\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", pts_str(pts), stroke_str(g), fill_str(g));
            }
            SchGraphicKind::RuleArea { pts, exclude_from_sim, exclude_from_bom, exclude_from_board, dnp } => {
                let lock = if sch.extras.is_locked(&g.id) { "(locked yes) " } else { "" };
                let _ = writeln!(
                    out,
                    "\t(rule_area {lock}(exclude_from_sim {}) (in_bom {}) (on_board {}) (dnp {})\n\t\t(polyline\n\t\t\t(pts {})\n\t\t\t{}\n\t\t\t{}\n\t\t\t(uuid \"{uuid}\")\n\t\t)\n\t)",
                    yes(*exclude_from_sim),
                    yes(!*exclude_from_bom),
                    yes(!*exclude_from_board),
                    yes(*dnp),
                    pts_str(pts),
                    stroke_str(g),
                    fill_str(g)
                );
            }
            SchGraphicKind::TextBox { start, end, text, angle, size_nm, bold, italic, h_align, v_align, margin_nm } => {
                let margin = if *margin_nm > Nm::ZERO { *margin_nm } else { default_text_box_margin(*size_nm, g.width_nm) };
                let (w, h) = (end.x - start.x, end.y - start.y);
                let _ = writeln!(
                    out,
                    "\t(text_box {}\n\t\t(exclude_from_sim no)\n\t\t(at {} {} {})\n\t\t(size {} {})\n\t\t(margins {m} {m} {m} {m})\n\t\t{}\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)",
                    sexpr_str(text),
                    sch_mm(start.x),
                    sch_mm(start.y),
                    angle_str(*angle),
                    sch_mm(w),
                    sch_mm(h),
                    stroke_str(g),
                    fill_str(g),
                    effects_str(*size_nm, *bold, *italic, *h_align, *v_align),
                    m = sch_mm(margin)
                );
            }
            SchGraphicKind::Image { at, scale_ppm, data } => {
                // `SCH_IO_KICAD_SEXPR::saveBitmap`: the position, the scale when it is not 1, the uuid, the lock and the file in lines of 76 characters.
                let scale = if *scale_ppm != eda_model::image_info::SCALE_PPM { format!("\n\t\t(scale {})", scale_str(*scale_ppm)) } else { String::new() };
                let _ = writeln!(out, "\t(image\n\t\t(at {} {}){scale}\n\t\t(uuid \"{uuid}\"){locked}\n{}\t)", sch_mm(at.x), sch_mm(at.y), data_str(data, "\t\t"));
            }
            SchGraphicKind::Directive { at, orientation, shape, pin_length_nm, netclass, component_class } => {
                // `SCH_DIRECTIVE_LABEL::AutoplaceFields`: the fields stack along the flag's pole.
                // the field's 1.27 mm text, `m_symbolSize` (`MilsToIU( 20 )`, twice for a diamond or a rectangle) and `GetTextOffset()` (0.15 of the text: 1905 IU)
                let size = mm(1.27);
                let symbol = mm(0.508) * if matches!(shape, DirectiveShape::Diamond | DirectiveShape::Rectangle) { 2 } else { 1 };
                let margin = Nm::new(190_500);
                let mut origin = *pin_length_nm;
                let mut props = String::new();
                for (name, value, italic) in [("Netclass", netclass, false), ("Component Class", component_class, true)] {
                    if value.is_empty() {
                        let _ = write!(props, "\n\t\t(property {} \"\" (at {} {} 0) (effects (font (size 1.27 1.27){}) (justify left)))", sexpr_str(name), sch_mm(at.x), sch_mm(at.y), if italic { " (italic yes)" } else { "" });
                        continue;
                    }
                    let (dx, dy, vertical) = match orientation % 360_000 {
                        90_000 => (-origin, -(symbol + margin), true),
                        180_000 => (symbol + margin, origin, false),
                        270_000 => (origin, -(symbol + margin), true),
                        _ => (symbol + margin, -origin, false),
                    };
                    let _ = write!(props, "\n\t\t(property {} {} (at {} {} {}) (effects (font (size 1.27 1.27){}) (justify left)))", sexpr_str(name), sexpr_str(value), sch_mm(at.x + dx), sch_mm(at.y + dy), if vertical { 90 } else { 0 }, if italic { " (italic yes)" } else { "" });
                    origin -= size + margin;
                }
                let _ = writeln!(
                    out,
                    "\t(netclass_flag \"\"\n\t\t(length {})\n\t\t(shape {})\n\t\t(at {} {} {})\n\t\t(fields_autoplaced yes)\n\t\t(effects (font (size 1.27 1.27)) (justify left bottom))\n\t\t(uuid \"{uuid}\"){locked}{props}\n\t)",
                    sch_mm(*pin_length_nm),
                    shape.token(),
                    sch_mm(at.x),
                    sch_mm(at.y),
                    angle_str(*orientation)
                );
            }
        }
    }
}

/// `SCH_TEXTBOX::GetLegacyTextMargin` for a box on `LAYER_NOTES`: half the stroke width plus 0.75 of the text height.
pub fn default_text_box_margin(size_nm: Nm, stroke_width_nm: Nm) -> Nm {
    Nm::kiround(stroke_width_nm.as_f64() / 2.0) + Nm::kiround(size_nm.as_f64() * 0.75)
}

/// One graphic read from a file, with whether it carried `(locked yes)`.
pub(crate) struct ReadGraphic {
    pub graphic: SchGraphic,
    pub locked: bool,
    /// The item's `(uuid ..)`, which a `(group ..)` names it by.
    pub uuid: Option<String>,
}

impl ReadGraphic {
    fn new(graphic: SchGraphic, locked: bool) -> ReadGraphic {
        ReadGraphic { graphic, locked, uuid: None }
    }
}

/// The `(uuid ..)` of an item.
pub(crate) fn uuid_of(item: &[Sexpr]) -> Option<String> {
    sexpr::find(item, "uuid").and_then(|u| sexpr::txt(u, 1)).map(String::from)
}

pub(crate) fn parse_bool(node: &[Sexpr], tag: &str) -> Option<bool> {
    sexpr::find(node, tag).map(|n| matches!(sexpr::txt(n, 1), Some("yes") | Some("true") | None))
}

/// What an item's `(effects (font (size H W) [bold] [italic]) (justify ...))` says (`EDA_TEXT::Format`, `SCH_IO_KICAD_SEXPR_PARSER::parseEDA_TEXT`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EffectsLook {
    /// The text's height in micrometres (`GetTextHeight`); 1.27 mm when the item says nothing.
    pub size_nm: Nm,
    pub bold: bool,
    pub italic: bool,
    /// The words of `(justify ...)`: `left`, `right`, `top`, `bottom`, `mirror`.
    pub justify: Vec<String>,
}

/// The `(effects ...)` of `item`. `bold` and `italic` are read as `(bold yes)` (what KiCad writes since 9) and as the bare words older files have.
pub(crate) fn read_effects(item: &[Sexpr]) -> EffectsLook {
    let effects = sexpr::find(item, "effects");
    let font = effects.and_then(|e| sexpr::find(e, "font"));
    let size_nm = font.and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(mm(1.27));
    let flag = |tag: &str| font.is_some_and(|f| parse_bool(f, tag).unwrap_or(false) || f.iter().skip(1).any(|c| c.text() == Some(tag)));
    let justify: Vec<String> = effects.and_then(|e| sexpr::find(e, "justify")).map(|j| j.iter().skip(1).filter_map(Sexpr::text).map(String::from).collect()).unwrap_or_default();
    EffectsLook { size_nm, bold: flag("bold"), italic: flag("italic"), justify }
}

fn pt(node: &[Sexpr]) -> Option<Point> {
    Some(Point { x: crate::import::sch_mm_to_nm(sexpr::num(node, 1)?), y: crate::import::sch_mm_to_nm(sexpr::num(node, 2)?) })
}

fn pts_of(node: &[Sexpr]) -> Vec<Point> {
    sexpr::find(node, "pts").map(|p| sexpr::find_all(p, "xy").filter_map(pt).collect()).unwrap_or_default()
}

pub(crate) fn parse_color(node: &[Sexpr]) -> Option<SchColor> {
    let ch = |i: usize| sexpr::num(node, i).map(|v| v.round().clamp(0.0, 255.0) as u8);
    let a = sexpr::num(node, 4).map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8).unwrap_or(255);
    Some(SchColor { r: ch(1)?, g: ch(2)?, b: ch(3)?, a })
}

/// `(stroke (width w) (type t) (color ...))` -> width (um), style, colour.
pub(crate) fn parse_stroke(node: &[Sexpr]) -> (Nm, SchLineStyle, Option<SchColor>) {
    let Some(s) = sexpr::find(node, "stroke") else { return (Nm::ZERO, SchLineStyle::Default, None) };
    let width = sexpr::find(s, "width").and_then(|w| sexpr::num(w, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(Nm::ZERO);
    let style = sexpr::find(s, "type").and_then(|t| sexpr::txt(t, 1)).map(SchLineStyle::from_token).unwrap_or_default();
    let color = sexpr::find(s, "color").and_then(parse_color);
    // KiCad writes a colour of all zeroes for "unspecified" (`COLOR4D::UNSPECIFIED`).
    (width, style, color.filter(|c| !(c.r == 0 && c.g == 0 && c.b == 0 && c.a == 0)))
}

fn parse_fill(node: &[Sexpr]) -> (SchFill, Option<SchColor>) {
    let Some(f) = sexpr::find(node, "fill") else { return (SchFill::None, None) };
    let ty = sexpr::find(f, "type").and_then(|t| sexpr::txt(t, 1)).unwrap_or("none");
    let fill = match ty {
        "hatch" | "reverse_hatch" | "cross_hatch" => SchFill::Color,
        other => SchFill::from_token(other),
    };
    (fill, sexpr::find(f, "color").and_then(parse_color))
}

fn styled(shape: SchGraphicKind, node: &[Sexpr]) -> SchGraphic {
    let (width_nm, line_style, color) = parse_stroke(node);
    let (fill, fill_color) = parse_fill(node);
    SchGraphic { id: String::new(), shape, width_nm, line_style, color, fill, fill_color }
}

/// Read every graphic out of a `(kicad_sch ...)` root. A top-level `(polyline ...)` with no fill is an
/// open notes-layer line (`SchLine`, read elsewhere) and is skipped here; a filled one is a polygon.
pub(crate) fn read_graphics(root: &[Sexpr]) -> Vec<ReadGraphic> {
    let version = sexpr::find(root, "version").and_then(|v| sexpr::num(v, 1)).unwrap_or(0.0) as i64;
    let mut out: Vec<ReadGraphic> = Vec::new();
    let locked_of = |n: &[Sexpr]| parse_bool(n, "locked").unwrap_or(false);
    for item in root.iter().filter_map(|n| n.as_list()) {
        let first = out.len();
        match sexpr::tag(item) {
            Some("rectangle") => {
                let (Some(start), Some(end)) = (sexpr::find(item, "start").and_then(pt), sexpr::find(item, "end").and_then(pt)) else { continue };
                let corner_radius_nm = sexpr::find(item, "radius").and_then(|r| sexpr::num(r, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(Nm::ZERO);
                out.push(ReadGraphic::new(styled(SchGraphicKind::Rectangle { start, end, corner_radius_nm }, item), locked_of(item)));
            }
            Some("circle") => {
                let (Some(center), Some(r)) = (sexpr::find(item, "center").and_then(pt), sexpr::find(item, "radius").and_then(|r| sexpr::num(r, 1))) else { continue };
                out.push(ReadGraphic::new(styled(SchGraphicKind::Circle { center, radius_nm: crate::import::sch_mm_to_nm(r) }, item), locked_of(item)));
            }
            Some("arc") => {
                let (Some(start), Some(mid), Some(end)) = (sexpr::find(item, "start").and_then(pt), sexpr::find(item, "mid").and_then(pt), sexpr::find(item, "end").and_then(pt)) else { continue };
                out.push(ReadGraphic::new(styled(SchGraphicKind::Arc { start, mid, end }, item), locked_of(item)));
            }
            Some("bezier") => {
                let p = pts_of(item);
                if let [start, c1, c2, end] = p[..] {
                    out.push(ReadGraphic::new(styled(SchGraphicKind::Bezier { start, c1, c2, end }, item), locked_of(item)));
                }
            }
            Some("polyline") => {
                let (fill, _) = parse_fill(item);
                let pts = pts_of(item);
                if fill != SchFill::None && pts.len() >= 3 {
                    out.push(ReadGraphic::new(styled(SchGraphicKind::Polygon { pts }, item), locked_of(item)));
                }
            }
            Some("rule_area") => {
                let Some(poly) = sexpr::find(item, "polyline") else { continue };
                let pts = pts_of(poly);
                if pts.len() < 3 {
                    continue;
                }
                let shape = SchGraphicKind::RuleArea {
                    pts,
                    exclude_from_sim: parse_bool(item, "exclude_from_sim").unwrap_or(false),
                    exclude_from_bom: !parse_bool(item, "in_bom").unwrap_or(true),
                    exclude_from_board: !parse_bool(item, "on_board").unwrap_or(true),
                    dnp: parse_bool(item, "dnp").unwrap_or(false),
                };
                out.push(ReadGraphic::new(styled(shape, poly), locked_of(item)));
            }
            Some("text_box") => {
                let Some(text) = sexpr::txt(item, 1).map(String::from) else { continue };
                let Some(at) = sexpr::find(item, "at") else { continue };
                let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
                let angle = (sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round().rem_euclid(360_000.0) as Millideg;
                let Some(size) = sexpr::find(item, "size").and_then(|s| Some((sexpr::num(s, 1)?, sexpr::num(s, 2)?))) else { continue };
                let start = Point { x: crate::import::sch_mm_to_nm(x), y: crate::import::sch_mm_to_nm(y) };
                let end = Point { x: start.x + crate::import::sch_mm_to_nm(size.0), y: start.y + crate::import::sch_mm_to_nm(size.1) };
                let margin_nm = sexpr::find(item, "margins").and_then(|m| sexpr::num(m, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(Nm::ZERO);
                let effects = sexpr::find(item, "effects");
                let font = effects.and_then(|e| sexpr::find(e, "font"));
                let size_nm = font.and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(mm(1.27));
                let flag = |tag: &str| font.and_then(|f| parse_bool(f, tag)).unwrap_or(false);
                let justify: Vec<&str> = effects.and_then(|e| sexpr::find(e, "justify")).map(|j| j.iter().skip(1).filter_map(Sexpr::text).collect()).unwrap_or_default();
                let h_align = if justify.contains(&"left") {
                    SchHAlign::Left
                } else if justify.contains(&"right") {
                    SchHAlign::Right
                } else {
                    SchHAlign::Center
                };
                let v_align = if justify.contains(&"top") {
                    SchVAlign::Top
                } else if justify.contains(&"bottom") {
                    SchVAlign::Bottom
                } else {
                    SchVAlign::Center
                };
                let shape = SchGraphicKind::TextBox { start, end, text, angle, size_nm, bold: flag("bold"), italic: flag("italic"), h_align, v_align, margin_nm };
                out.push(ReadGraphic::new(styled(shape, item), locked_of(item)));
            }
            Some("image") => {
                // `SCH_IO_KICAD_SEXPR_PARSER::parseImage`
                let Some(at) = sexpr::find(item, "at").and_then(pt) else { continue };
                let data: String = sexpr::find(item, "data").map(|d| d.iter().skip(1).filter_map(Sexpr::text).collect()).unwrap_or_default();
                let data = eda_model::image_info::compact(&data);
                let Some(header) = eda_model::image_info::header_of_base64(&data) else { continue };
                // `refImage.SetImageScale( std::isnormal( scale ) ? scale : 1.0 )`, and `SetImageScale` ignores a scale that is not positive
                let mut scale = sexpr::find(item, "scale").and_then(|s| sexpr::num(s, 1)).filter(|v| v.is_normal() && *v > 0.0).unwrap_or(1.0);
                // "20230121 or older file format versions assumed 300 image PPI at load/save"
                if version <= 20230121 {
                    scale *= header.ppi as f64 / eda_model::image_info::DEFAULT_PPI as f64;
                }
                let scale_ppm = (scale * eda_model::image_info::SCALE_PPM as f64).round() as i64;
                out.push(ReadGraphic::new(SchGraphic::new(SchGraphicKind::Image { at, scale_ppm: scale_ppm.max(1), data }), locked_of(item)));
            }
            Some("netclass_flag") => {
                let Some(at) = sexpr::find(item, "at") else { continue };
                let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
                let orientation = (sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round().rem_euclid(360_000.0) as Millideg;
                let pin_length_nm = sexpr::find(item, "length").and_then(|l| sexpr::num(l, 1)).map(crate::import::sch_mm_to_nm).unwrap_or(mm(2.54));
                let shape = sexpr::find(item, "shape").and_then(|s| sexpr::txt(s, 1)).map(DirectiveShape::from_token).unwrap_or_default();
                // a field by its canonical name: `SCH_FIELD::GetCanonicalName` takes "Net Class" for "Netclass" on a label (`labelTypes` holds
                // `SCH_DIRECTIVE_LABEL_T`), and `CONNECTION_GRAPH` gives the net the class of either spelling (r5 D5-H4, QA `issue23962`)
                fn canonical(n: &str) -> &str {
                    if n == "Net Class" { "Netclass" } else { n }
                }
                let field = |name: &str| sexpr::find_all(item, "property").find(|p| sexpr::txt(p, 1).map(canonical) == Some(name)).and_then(|p| sexpr::txt(p, 2)).unwrap_or("").to_string();
                let kind = SchGraphicKind::Directive { at: Point { x: crate::import::sch_mm_to_nm(x), y: crate::import::sch_mm_to_nm(y) }, orientation, shape, pin_length_nm, netclass: field("Netclass"), component_class: field("Component Class") };
                out.push(ReadGraphic::new(SchGraphic::new(kind), locked_of(item)));
            }
            _ => {}
        }
        // a rule area keeps its uuid in its polyline (`saveRuleArea`), the other kinds on the item
        if let Some(read) = out.get_mut(first) {
            read.uuid = uuid_of(item).or_else(|| sexpr::find(item, "polyline").and_then(uuid_of));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;

    /// r5 D5-H4 (`SCH_FIELD::GetCanonicalName`: "Net Class" is "Netclass" on a label): a directive label's `(property "Net Class" "HV")` is its net class,
    /// and is written back under the canonical name.
    #[test]
    fn a_directive_labels_net_class_field_is_read_under_either_name() {
        let text = r#"(kicad_sch (version 20250114) (netclass_flag "" (length 2.54) (shape round) (at 10 10 0) (uuid "d1111111-1111-1111-1111-111111111111") (property "Net Class" "HV" (at 10 8 0))))"#;
        let tree = sexpr::parse(text).unwrap();
        let read = read_graphics(tree.as_list().unwrap());
        let netclass = read.iter().find_map(|r| match &r.graphic.shape {
            SchGraphicKind::Directive { netclass, .. } => Some(netclass.clone()),
            _ => None,
        });
        assert_eq!(netclass.as_deref(), Some("HV"));
        let mut sch = SchematicSection::default();
        sch.extras.graphics.push(read[0].graphic.clone());
        let mut out = String::new();
        write_graphics(&mut out, &sch);
        assert!(out.contains("(property \"Netclass\" \"HV\""), "{out}");
    }

    fn p(x: Nm, y: Nm) -> Point {
        Point { x, y }
    }

    fn sample_graphics() -> Vec<SchGraphic> {
        let mut rect = SchGraphic::new(SchGraphicKind::Rectangle { start: p(mm(10.0), mm(20.0)), end: p(mm(30.0), mm(50.0)), corner_radius_nm: Nm::ZERO });
        rect.width_nm = mm(0.254);
        rect.line_style = SchLineStyle::Dash;
        rect.fill = SchFill::Background;
        let mut circle = SchGraphic::new(SchGraphicKind::Circle { center: p(mm(5.0), mm(5.0)), radius_nm: mm(2.5) });
        circle.fill = SchFill::Color;
        circle.fill_color = Some(SchColor { r: 255, g: 0, b: 0, a: 255 });
        circle.color = Some(SchColor { r: 0, g: 128, b: 0, a: 255 });
        vec![
            rect,
            circle,
            SchGraphic::new(SchGraphicKind::Arc { start: p(Nm::ZERO, Nm::ZERO), mid: p(mm(1.0), mm(-1.0)), end: p(mm(2.0), Nm::ZERO) }),
            SchGraphic::new(SchGraphicKind::Bezier { start: p(Nm::ZERO, Nm::ZERO), c1: p(mm(1.0), mm(2.0)), c2: p(mm(3.0), mm(2.0)), end: p(mm(4.0), Nm::ZERO) }),
            {
                let mut poly = SchGraphic::new(SchGraphicKind::Polygon { pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(10.0), Nm::ZERO), p(mm(10.0), mm(10.0))] });
                poly.fill = SchFill::Outline;
                poly
            },
            SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(20.0), Nm::ZERO), p(mm(20.0), mm(10.0)), p(Nm::ZERO, mm(10.0))], exclude_from_sim: true, exclude_from_bom: false, exclude_from_board: true, dnp: true }),
            SchGraphic::new(SchGraphicKind::TextBox { start: p(Nm::ZERO, Nm::ZERO), end: p(mm(40.0), mm(10.0)), text: "Hello\nworld".into(), angle: 90_000, size_nm: mm(1.27), bold: true, italic: false, h_align: SchHAlign::Left, v_align: SchVAlign::Top, margin_nm: Nm::ZERO }),
            SchGraphic::new(SchGraphicKind::Directive { at: p(mm(50.8), mm(25.4)), orientation: 90_000, shape: DirectiveShape::Diamond, pin_length_nm: mm(2.54), netclass: "HV".into(), component_class: "Power".into() }),
            SchGraphic::new(SchGraphicKind::Image { at: p(mm(60.0), mm(40.0)), scale_ppm: 750_000, data: eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(60, 40, [10, 20, 30], 254)) }),
        ]
    }

    fn read_back(sch: &SchematicSection) -> Vec<ReadGraphic> {
        let mut text = String::from("(kicad_sch (version 20250114)\n");
        write_graphics(&mut text, sch);
        text.push_str(")\n");
        let tree = sexpr::parse(&text).expect("the written graphics parse");
        read_graphics(tree.as_list().unwrap())
    }

    #[test]
    fn every_graphic_kind_round_trips_through_kicad_sch_text() {
        let mut sch = SchematicSection::default();
        sch.extras.graphics = sample_graphics();
        sch.assign_missing_ids();
        let back = read_back(&sch);
        assert_eq!(back.len(), sch.extras.graphics.len());
        for (want, got) in sch.extras.graphics.iter().zip(back.iter()) {
            let mut w = want.clone();
            w.id = String::new();
            let mut g = got.graphic.clone();
            g.id = String::new();
            // A text box written with the default margin reads back with the explicit margin; everything else is exact.
            if let (SchGraphicKind::TextBox { margin_nm: wm, .. }, SchGraphicKind::TextBox { margin_nm: gm, .. }) = (&mut w.shape, &g.shape) {
                *wm = *gm;
            }
            assert_eq!(w, g, "{:?}", want.shape);
            assert!(!got.locked);
        }
    }

    /// A text box's line break is written as `\n` inside the quotes, as `OUTPUTFORMATTER::Quotes` writes it: a raw line feed in a string is a schematic kicad-cli
    /// refuses ("Failed to load schematic").
    #[test]
    fn a_text_boxs_line_break_is_written_escaped_so_kicad_can_load_the_sheet() {
        let mut sch = SchematicSection::default();
        sch.extras.graphics = sample_graphics();
        sch.assign_missing_ids();
        let mut text = String::new();
        write_graphics(&mut text, &sch);
        assert!(text.contains("(text_box \"Hello\\nworld\""), "{text}");
        assert!(!text.contains("Hello\nworld"), "no raw line feed inside the string");
    }

    #[test]
    fn locked_graphics_carry_the_flag_both_ways() {
        let mut sch = SchematicSection::default();
        sch.extras.graphics = sample_graphics();
        sch.assign_missing_ids();
        let ids: Vec<String> = sch.extras.graphics.iter().map(|g| g.id.clone()).collect();
        for id in &ids {
            sch.extras.set_locked(id, true);
        }
        let back = read_back(&sch);
        assert!(back.iter().all(|g| g.locked), "every graphic reads back locked");
    }

    #[test]
    fn an_unfilled_top_level_polyline_is_left_to_the_notes_line_reader() {
        let tree = sexpr::parse("(kicad_sch (polyline (pts (xy 0 0) (xy 1 1) (xy 2 0)) (stroke (width 0) (type default)) (fill (type none))) (polyline (pts (xy 0 0) (xy 5 0) (xy 5 5)) (stroke (width 0) (type default)) (fill (type background))))").unwrap();
        let got = read_graphics(tree.as_list().unwrap());
        assert_eq!(got.len(), 1, "only the filled polyline is a polygon graphic");
        assert!(matches!(got[0].graphic.shape, SchGraphicKind::Polygon { .. }));
        assert_eq!(got[0].graphic.fill, SchFill::Background);
    }

    /// `saveBitmap` writes `(image (at ..) (scale s) (uuid ..) [(locked yes)] (data ..))` with the data in lines of 76 characters, scale one left out; `parseImage` reads it back,
    /// and in a file of version 20230121 or older multiplies the scale by the image's definition over 300 ("older versions assumed 300 PPI").
    #[test]
    fn an_image_is_written_as_save_bitmap_writes_it_and_read_back_with_the_old_versions_scale_rule() {
        let data = eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(60, 40, [10, 20, 30], 254));
        let mut sch = SchematicSection::default();
        sch.extras.graphics = vec![SchGraphic::new(SchGraphicKind::Image { at: p(mm(60.0), mm(40.0)), scale_ppm: 1_000_000, data: data.clone() }), SchGraphic::new(SchGraphicKind::Image { at: p(mm(1.0), mm(2.0)), scale_ppm: 2_250_000, data: data.clone() })];
        sch.assign_missing_ids();
        let ids: Vec<String> = sch.extras.graphics.iter().map(|g| g.id.clone()).collect();
        sch.extras.set_locked(&ids[1], true);
        let mut text = String::from("(kicad_sch (version 20250114)\n");
        write_graphics(&mut text, &sch);
        text.push_str(")\n");
        assert!(text.contains("(image\n\t\t(at 60 40)\n\t\t(uuid"), "scale one is not written: {text}");
        assert!(text.contains("(at 1 2)\n\t\t(scale 2.25)\n\t\t(uuid"), "{text}");
        assert_eq!(text.matches("(locked yes)").count(), 1);
        assert!(text.lines().filter(|l| l.trim_start().starts_with('"')).all(|l| l.trim().len() <= 78), "lines of at most 76 characters");
        let back = read_graphics(sexpr::parse(&text).unwrap().as_list().unwrap());
        assert_eq!(back.len(), 2);
        assert!(matches!(&back[0].graphic.shape, SchGraphicKind::Image { at, scale_ppm: 1_000_000, data: d } if *at == p(mm(60.0), mm(40.0)) && *d == data));
        assert!(matches!(&back[1].graphic.shape, SchGraphicKind::Image { scale_ppm: 2_250_000, .. }));
        assert!(!back[0].locked && back[1].locked);
        // an old file: the scale is of a 300 ppi image
        let old = text.replace("(version 20250114)", "(version 20230121)");
        let back = read_graphics(sexpr::parse(&old).unwrap().as_list().unwrap());
        assert!(matches!(&back[0].graphic.shape, SchGraphicKind::Image { scale_ppm, .. } if *scale_ppm == 846_667), "254 / 300: {:?}", back[0].graphic.shape);
        // data that is no image, a scale that is not a number
        let junk = sexpr::parse("(kicad_sch (image (at 1 1) (data \"AAAA\")) (image (at 1 1)))").unwrap();
        assert!(read_graphics(junk.as_list().unwrap()).is_empty());
        let odd = text.replace("(scale 2.25)", "(scale 0)");
        let back = read_graphics(sexpr::parse(&odd).unwrap().as_list().unwrap());
        assert!(matches!(&back[1].graphic.shape, SchGraphicKind::Image { scale_ppm: 1_000_000, .. }));
    }

    #[test]
    fn scale_text_has_no_trailing_zeros() {
        assert_eq!(scale_str(1_000_000), "1");
        assert_eq!(scale_str(500_000), "0.5");
        assert_eq!(scale_str(2_250_000), "2.25");
        assert_eq!(scale_str(333_333), "0.333333");
        assert_eq!(scale_str(1_000_001), "1.000001");
    }

    #[test]
    fn text_box_margin_defaults_to_the_kicad_legacy_formula() {
        assert_eq!(default_text_box_margin(mm(1.27), Nm::ZERO), Nm::new(952_500));
        assert_eq!(default_text_box_margin(mm(1.27), mm(0.2)), Nm::new(100_000 + 952_500));
    }
}
