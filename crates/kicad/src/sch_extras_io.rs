//! `.kicad_sch` read/write of the schematic editor's drawn graphics ([`eda_model::sch_extras::SchGraphic`]):
//! rectangles, circles, arcs, beziers, filled polygons, text boxes, rule areas and directive labels
//! (`netclass_flag`), written the way `SCH_IO_KICAD_SEXPR` (`saveShape`, `saveTextBox`, `saveRuleArea`,
//! `saveText`) writes them and read back from the same tokens.

use crate::sexpr::{self, Sexpr};
use crate::{duid_for, mm, sexpr_str};
use eda_model::ir::{Millideg, Point, SchematicSection, Um};
use eda_model::sch_extras::{DirectiveShape, SchColor, SchFill, SchGraphic, SchGraphicKind, SchHAlign, SchLineStyle, SchVAlign};
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
    let mut s = format!("(stroke (width {}) (type {})", mm(g.width_um), g.line_style.token());
    if let Some(c) = g.color {
        let _ = write!(s, " (color {})", color_str(c));
    }
    s.push(')');
    s
}

fn fill_str(g: &SchGraphic) -> String {
    match (g.fill, g.fill_color) {
        (SchFill::Color, Some(c)) => format!("(fill (type color) (color {}))", color_str(c)),
        (f, _) => format!("(fill (type {}))", f.token()),
    }
}

fn pts_str(pts: &[Point]) -> String {
    pts.iter().map(|p| format!("(xy {} {})", mm(p.x), mm(p.y))).collect::<Vec<_>>().join(" ")
}

fn angle_str(m: Millideg) -> String {
    let d = m as f64 / 1000.0;
    if d == d.trunc() {
        format!("{}", d as i64)
    } else {
        format!("{d}")
    }
}

/// The `(effects ...)` of a text box: font size, bold/italic and the justification (a box defaults to left/top).
fn effects_str(size_um: Um, bold: bool, italic: bool, h: SchHAlign, v: SchVAlign) -> String {
    let mut f = format!("(font (size {0} {0})", mm(size_um));
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

/// Write every graphic of `sch` (drawing order), each with its `(locked yes)` when locked.
pub(crate) fn write_graphics(out: &mut String, sch: &SchematicSection) {
    for g in &sch.extras.graphics {
        let uuid = duid_for(&format!("sch_graphic:{}", g.id_seed()), &g.id);
        let locked = if sch.extras.is_locked(&g.id) { "\n\t\t(locked yes)" } else { "" };
        match &g.shape {
            SchGraphicKind::Rectangle { start, end, corner_radius_um } => {
                let radius = if *corner_radius_um > 0 { format!("\n\t\t(radius {})", mm(*corner_radius_um)) } else { String::new() };
                let _ = writeln!(out, "\t(rectangle\n\t\t(start {} {})\n\t\t(end {} {}){radius}\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", mm(start.x), mm(start.y), mm(end.x), mm(end.y), stroke_str(g), fill_str(g));
            }
            SchGraphicKind::Circle { center, radius_um } => {
                let _ = writeln!(out, "\t(circle\n\t\t(center {} {})\n\t\t(radius {})\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", mm(center.x), mm(center.y), mm(*radius_um), stroke_str(g), fill_str(g));
            }
            SchGraphicKind::Arc { start, mid, end } => {
                let _ = writeln!(out, "\t(arc\n\t\t(start {} {})\n\t\t(mid {} {})\n\t\t(end {} {})\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)", mm(start.x), mm(start.y), mm(mid.x), mm(mid.y), mm(end.x), mm(end.y), stroke_str(g), fill_str(g));
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
            SchGraphicKind::TextBox { start, end, text, angle, size_um, bold, italic, h_align, v_align, margin_um } => {
                let margin = if *margin_um > 0 { *margin_um } else { default_text_box_margin(*size_um, g.width_um) };
                let (w, h) = (end.x - start.x, end.y - start.y);
                let _ = writeln!(
                    out,
                    "\t(text_box {}\n\t\t(exclude_from_sim no)\n\t\t(at {} {} {})\n\t\t(size {} {})\n\t\t(margins {m} {m} {m} {m})\n\t\t{}\n\t\t{}\n\t\t{}\n\t\t(uuid \"{uuid}\"){locked}\n\t)",
                    sexpr_str(text),
                    mm(start.x),
                    mm(start.y),
                    angle_str(*angle),
                    mm(w),
                    mm(h),
                    stroke_str(g),
                    fill_str(g),
                    effects_str(*size_um, *bold, *italic, *h_align, *v_align),
                    m = mm(margin)
                );
            }
            SchGraphicKind::Directive { at, orientation, shape, pin_length_um, netclass, component_class } => {
                // `SCH_DIRECTIVE_LABEL::AutoplaceFields`: the fields stack along the flag's pole.
                let size = 1270;
                let symbol = 508 * if matches!(shape, DirectiveShape::Diamond | DirectiveShape::Rectangle) { 2 } else { 1 };
                let margin = 191;
                let mut origin = *pin_length_um;
                let mut props = String::new();
                for (name, value, italic) in [("Netclass", netclass, false), ("Component Class", component_class, true)] {
                    if value.is_empty() {
                        let _ = write!(props, "\n\t\t(property {} \"\" (at {} {} 0) (effects (font (size 1.27 1.27){}) (justify left)))", sexpr_str(name), mm(at.x), mm(at.y), if italic { " (italic yes)" } else { "" });
                        continue;
                    }
                    let (dx, dy, vertical) = match orientation % 360_000 {
                        90_000 => (-origin, -(symbol + margin), true),
                        180_000 => (symbol + margin, origin, false),
                        270_000 => (origin, -(symbol + margin), true),
                        _ => (symbol + margin, -origin, false),
                    };
                    let _ = write!(props, "\n\t\t(property {} {} (at {} {} {}) (effects (font (size 1.27 1.27){}) (justify left)))", sexpr_str(name), sexpr_str(value), mm(at.x + dx), mm(at.y + dy), if vertical { 90 } else { 0 }, if italic { " (italic yes)" } else { "" });
                    origin -= size + margin;
                }
                let _ = writeln!(
                    out,
                    "\t(netclass_flag \"\"\n\t\t(length {})\n\t\t(shape {})\n\t\t(at {} {} {})\n\t\t(fields_autoplaced yes)\n\t\t(effects (font (size 1.27 1.27)) (justify left bottom))\n\t\t(uuid \"{uuid}\"){locked}{props}\n\t)",
                    mm(*pin_length_um),
                    shape.token(),
                    mm(at.x),
                    mm(at.y),
                    angle_str(*orientation)
                );
            }
        }
    }
}

/// `SCH_TEXTBOX::GetLegacyTextMargin` for a box on `LAYER_NOTES`: half the stroke width plus 0.75 of the text height.
pub fn default_text_box_margin(size_um: Um, stroke_width_um: Um) -> Um {
    (stroke_width_um as f64 / 2.0).round() as Um + (size_um as f64 * 0.75).round() as Um
}

/// One graphic read from a file, with whether it carried `(locked yes)`.
pub(crate) struct ReadGraphic {
    pub graphic: SchGraphic,
    pub locked: bool,
}

pub(crate) fn parse_bool(node: &[Sexpr], tag: &str) -> Option<bool> {
    sexpr::find(node, tag).map(|n| matches!(sexpr::txt(n, 1), Some("yes") | Some("true") | None))
}

fn pt(node: &[Sexpr]) -> Option<Point> {
    Some(Point { x: crate::import::mm_to_um(sexpr::num(node, 1)?), y: crate::import::mm_to_um(sexpr::num(node, 2)?) })
}

fn pts_of(node: &[Sexpr]) -> Vec<Point> {
    sexpr::find(node, "pts").map(|p| sexpr::find_all(p, "xy").filter_map(pt).collect()).unwrap_or_default()
}

fn parse_color(node: &[Sexpr]) -> Option<SchColor> {
    let ch = |i: usize| sexpr::num(node, i).map(|v| v.round().clamp(0.0, 255.0) as u8);
    let a = sexpr::num(node, 4).map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8).unwrap_or(255);
    Some(SchColor { r: ch(1)?, g: ch(2)?, b: ch(3)?, a })
}

/// `(stroke (width w) (type t) (color ...))` -> width (um), style, colour.
fn parse_stroke(node: &[Sexpr]) -> (Um, SchLineStyle, Option<SchColor>) {
    let Some(s) = sexpr::find(node, "stroke") else { return (0, SchLineStyle::Default, None) };
    let width = sexpr::find(s, "width").and_then(|w| sexpr::num(w, 1)).map(crate::import::mm_to_um).unwrap_or(0);
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
    let (width_um, line_style, color) = parse_stroke(node);
    let (fill, fill_color) = parse_fill(node);
    SchGraphic { id: String::new(), shape, width_um, line_style, color, fill, fill_color }
}

/// Read every graphic out of a `(kicad_sch ...)` root. A top-level `(polyline ...)` with no fill is an
/// open notes-layer line (`SchLine`, read elsewhere) and is skipped here; a filled one is a polygon.
pub(crate) fn read_graphics(root: &[Sexpr]) -> Vec<ReadGraphic> {
    let mut out: Vec<ReadGraphic> = Vec::new();
    let locked_of = |n: &[Sexpr]| parse_bool(n, "locked").unwrap_or(false);
    for item in root.iter().filter_map(|n| n.as_list()) {
        match sexpr::tag(item) {
            Some("rectangle") => {
                let (Some(start), Some(end)) = (sexpr::find(item, "start").and_then(pt), sexpr::find(item, "end").and_then(pt)) else { continue };
                let corner_radius_um = sexpr::find(item, "radius").and_then(|r| sexpr::num(r, 1)).map(crate::import::mm_to_um).unwrap_or(0);
                out.push(ReadGraphic { graphic: styled(SchGraphicKind::Rectangle { start, end, corner_radius_um }, item), locked: locked_of(item) });
            }
            Some("circle") => {
                let (Some(center), Some(r)) = (sexpr::find(item, "center").and_then(pt), sexpr::find(item, "radius").and_then(|r| sexpr::num(r, 1))) else { continue };
                out.push(ReadGraphic { graphic: styled(SchGraphicKind::Circle { center, radius_um: crate::import::mm_to_um(r) }, item), locked: locked_of(item) });
            }
            Some("arc") => {
                let (Some(start), Some(mid), Some(end)) = (sexpr::find(item, "start").and_then(pt), sexpr::find(item, "mid").and_then(pt), sexpr::find(item, "end").and_then(pt)) else { continue };
                out.push(ReadGraphic { graphic: styled(SchGraphicKind::Arc { start, mid, end }, item), locked: locked_of(item) });
            }
            Some("bezier") => {
                let p = pts_of(item);
                if let [start, c1, c2, end] = p[..] {
                    out.push(ReadGraphic { graphic: styled(SchGraphicKind::Bezier { start, c1, c2, end }, item), locked: locked_of(item) });
                }
            }
            Some("polyline") => {
                let (fill, _) = parse_fill(item);
                let pts = pts_of(item);
                if fill != SchFill::None && pts.len() >= 3 {
                    out.push(ReadGraphic { graphic: styled(SchGraphicKind::Polygon { pts }, item), locked: locked_of(item) });
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
                out.push(ReadGraphic { graphic: styled(shape, poly), locked: locked_of(item) });
            }
            Some("text_box") => {
                let Some(text) = sexpr::txt(item, 1).map(String::from) else { continue };
                let Some(at) = sexpr::find(item, "at") else { continue };
                let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
                let angle = (sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round().rem_euclid(360_000.0) as Millideg;
                let Some(size) = sexpr::find(item, "size").and_then(|s| Some((sexpr::num(s, 1)?, sexpr::num(s, 2)?))) else { continue };
                let start = Point { x: crate::import::mm_to_um(x), y: crate::import::mm_to_um(y) };
                let end = Point { x: start.x + crate::import::mm_to_um(size.0), y: start.y + crate::import::mm_to_um(size.1) };
                let margin_um = sexpr::find(item, "margins").and_then(|m| sexpr::num(m, 1)).map(crate::import::mm_to_um).unwrap_or(0);
                let effects = sexpr::find(item, "effects");
                let font = effects.and_then(|e| sexpr::find(e, "font"));
                let size_um = font.and_then(|f| sexpr::find(f, "size")).and_then(|s| sexpr::num(s, 1)).map(crate::import::mm_to_um).unwrap_or(1270);
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
                let shape = SchGraphicKind::TextBox { start, end, text, angle, size_um, bold: flag("bold"), italic: flag("italic"), h_align, v_align, margin_um };
                out.push(ReadGraphic { graphic: styled(shape, item), locked: locked_of(item) });
            }
            Some("netclass_flag") => {
                let Some(at) = sexpr::find(item, "at") else { continue };
                let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
                let orientation = (sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round().rem_euclid(360_000.0) as Millideg;
                let pin_length_um = sexpr::find(item, "length").and_then(|l| sexpr::num(l, 1)).map(crate::import::mm_to_um).unwrap_or(2540);
                let shape = sexpr::find(item, "shape").and_then(|s| sexpr::txt(s, 1)).map(DirectiveShape::from_token).unwrap_or_default();
                let field = |name: &str| sexpr::find_all(item, "property").find(|p| sexpr::txt(p, 1) == Some(name)).and_then(|p| sexpr::txt(p, 2)).unwrap_or("").to_string();
                let kind = SchGraphicKind::Directive { at: Point { x: crate::import::mm_to_um(x), y: crate::import::mm_to_um(y) }, orientation, shape, pin_length_um, netclass: field("Netclass"), component_class: field("Component Class") };
                out.push(ReadGraphic { graphic: SchGraphic::new(kind), locked: locked_of(item) });
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: Um, y: Um) -> Point {
        Point { x, y }
    }

    fn sample_graphics() -> Vec<SchGraphic> {
        let mut rect = SchGraphic::new(SchGraphicKind::Rectangle { start: p(10_000, 20_000), end: p(30_000, 50_000), corner_radius_um: 0 });
        rect.width_um = 254;
        rect.line_style = SchLineStyle::Dash;
        rect.fill = SchFill::Background;
        let mut circle = SchGraphic::new(SchGraphicKind::Circle { center: p(5_000, 5_000), radius_um: 2_500 });
        circle.fill = SchFill::Color;
        circle.fill_color = Some(SchColor { r: 255, g: 0, b: 0, a: 255 });
        circle.color = Some(SchColor { r: 0, g: 128, b: 0, a: 255 });
        vec![
            rect,
            circle,
            SchGraphic::new(SchGraphicKind::Arc { start: p(0, 0), mid: p(1_000, -1_000), end: p(2_000, 0) }),
            SchGraphic::new(SchGraphicKind::Bezier { start: p(0, 0), c1: p(1_000, 2_000), c2: p(3_000, 2_000), end: p(4_000, 0) }),
            {
                let mut poly = SchGraphic::new(SchGraphicKind::Polygon { pts: vec![p(0, 0), p(10_000, 0), p(10_000, 10_000)] });
                poly.fill = SchFill::Outline;
                poly
            },
            SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(0, 0), p(20_000, 0), p(20_000, 10_000), p(0, 10_000)], exclude_from_sim: true, exclude_from_bom: false, exclude_from_board: true, dnp: true }),
            SchGraphic::new(SchGraphicKind::TextBox { start: p(0, 0), end: p(40_000, 10_000), text: "Hello\nworld".into(), angle: 90_000, size_um: 1_270, bold: true, italic: false, h_align: SchHAlign::Left, v_align: SchVAlign::Top, margin_um: 0 }),
            SchGraphic::new(SchGraphicKind::Directive { at: p(50_800, 25_400), orientation: 90_000, shape: DirectiveShape::Diamond, pin_length_um: 2_540, netclass: "HV".into(), component_class: "Power".into() }),
        ]
    }

    fn read_back(sch: &SchematicSection) -> Vec<ReadGraphic> {
        let mut text = String::from("(kicad_sch\n");
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
            if let (SchGraphicKind::TextBox { margin_um: wm, .. }, SchGraphicKind::TextBox { margin_um: gm, .. }) = (&mut w.shape, &g.shape) {
                *wm = *gm;
            }
            assert_eq!(w, g, "{:?}", want.shape);
            assert!(!got.locked);
        }
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

    #[test]
    fn text_box_margin_defaults_to_the_kicad_legacy_formula() {
        assert_eq!(default_text_box_margin(1_270, 0), 953);
        assert_eq!(default_text_box_margin(1_270, 200), 100 + 953);
    }
}
