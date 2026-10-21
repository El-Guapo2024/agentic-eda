//! A board's text boxes (`(gr_text_box ..)`, KiCad's `PCB_TEXTBOX`) read and written: `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_TEXTBOX` / `parseTextBoxContent` and
//! `PCB_IO_KICAD_SEXPR::format( const PCB_TEXTBOX* )`, for [`eda_model::pcb_items::TextBox`].
//!
//! The box's looks are an `(effects ..)` like any text's, read and written by the code of [`crate::board_text_io`] (a text box is a `Text` and a `TextExtra` to it).
//! A footprint's own `(fp_text_box ..)` is not read here: it is part of the footprint (`FootprintExtra::tokens`).

use std::fmt::Write as _;

use eda_model::ir::{Point, Text, TextJustify, TextVAlign};
use eda_model::pcb_items::{legacy_text_margin_um, LineStyle, TextBox};

use crate::sexpr::{self, Sexpr};
use crate::{fmt_mm_f, mm, mm_to_um, sexpr_str};

/// `STROKE_PARAMS`' width of a text box that says nothing: KiCad's `STROKE_PARAMS stroke( -1, LINE_STYLE::SOLID )` before the file's `(stroke ..)`.
const NO_STROKE: i64 = -1;
/// The first board file version with an explicit `(border yes|no)`; before it a text box had a border when its stroke width was not negative.
const FIRST_BORDER_FLAG: i64 = 20230825;
/// The text size of a text with no `(size ..)` in its effects (`parseEDA_TEXT`: "force legacy default units", 60 mils).
const LEGACY_TEXT_SIZE_UM: i64 = 1_524;

fn yes(item: &[Sexpr], name: &str) -> Option<bool> {
    sexpr::find(item, name).map(|l| sexpr::txt(l, 1).map_or(true, |v| v != "no"))
}

fn point(l: &[Sexpr]) -> Option<Point> {
    Some(Point { x: mm_to_um(sexpr::num(l, 1)?), y: mm_to_um(sexpr::num(l, 2)?) })
}

/// The `text`, `start`, `end` and `pts` of a text box item, and whether it says it is locked the old way (a bare `locked` after the tag).
fn text_of(item: &[Sexpr]) -> (Option<&str>, bool) {
    let mut at = 1;
    let mut legacy_locked = false;
    if matches!(item.get(1), Some(Sexpr::Atom(a)) if a == "locked") && item.get(2).is_some_and(|n| matches!(n, Sexpr::Atom(_))) {
        legacy_locked = true;
        at = 2;
    }
    (item.get(at).and_then(Sexpr::text), legacy_locked)
}

/// One `(gr_text_box ..)` of a board file of version `version`; `None` when it has neither a rectangle nor a polygon.
pub(crate) fn read(item: &[Sexpr], version: i64) -> Option<TextBox> {
    let (text, _) = text_of(item);
    let text = text?.to_string();
    let start = sexpr::find(item, "start").and_then(point);
    let end = sexpr::find(item, "end").and_then(point);
    let pts: Vec<Point> = sexpr::find(item, "pts").map(|p| sexpr::find_all(p, "xy").filter_map(point).collect()).unwrap_or_default();
    let (start, end, pts) = match (start, end, pts.len()) {
        (Some(s), Some(e), 0) => (s, e, Vec::new()),
        (_, _, n) if n >= 3 => (pts[0], pts[2.min(n - 1)], pts),
        _ => return None,
    };

    // `(stroke (width W) (type T) [(color r g b a)])`: a box with no stroke token has `STROKE_PARAMS( -1, SOLID )`.
    let stroke = sexpr::find(item, "stroke");
    let width = stroke.and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).map(mm_to_um).unwrap_or(NO_STROKE);
    let line_style = stroke.and_then(|s| sexpr::find(s, "type")).and_then(|t| sexpr::txt(t, 1)).map(LineStyle::from_token).unwrap_or(LineStyle::Solid);
    let stroke_color = stroke.and_then(|s| sexpr::find(s, "color")).map(|c| c.iter().skip(1).filter_map(Sexpr::text).collect::<Vec<_>>().join(" ")).filter(|c| !c.is_empty());
    // `if( m_requiredVersion < 20230825 ) aTextBox->SetBorderEnabled( stroke.GetWidth() >= 0 );`
    let border = if version < FIRST_BORDER_FLAG { width >= 0 } else { yes(item, "border").unwrap_or(true) };
    // `PCB_SHAPE::GetWidth`: "negative stroke-widths are only used in EEschema".
    let stroke_width_um = width.max(0);

    // The looks: `parseEDA_TEXT` starts a box from centre and centre when it has `(effects ..)`; without them the box is left and centre (`PCB_TEXTBOX::PCB_TEXTBOX`).
    let has_effects = sexpr::find(item, "effects").is_some();
    let style = crate::board_text_io::read_style(item);
    let has_size = sexpr::find(item, "effects").and_then(|e| sexpr::find(e, "font")).and_then(|f| sexpr::find(f, "size")).is_some();
    let size_um = if has_size { style.size_um } else { LEGACY_TEXT_SIZE_UM };

    let layer = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("Dwgs.User").to_string();
    let angle = ((sexpr::find(item, "angle").and_then(|a| sexpr::num(a, 1)).unwrap_or(0.0) * 1000.0).round() as i64).rem_euclid(360_000) as u32;
    let mut t = TextBox {
        id: String::new(),
        layer,
        text,
        start,
        end,
        pts,
        angle,
        margin_left_um: 0,
        margin_top_um: 0,
        margin_right_um: 0,
        margin_bottom_um: 0,
        border,
        stroke_width_um,
        line_style,
        stroke_color,
        knockout: yes(item, "knockout").unwrap_or(false),
        size_um,
        width_um: style.extra.width_um.filter(|_| has_size),
        thickness_um: if style.extra.auto_thickness { 0 } else { style.stroke_width },
        halign: if has_effects { style.justify } else { TextJustify::Left },
        valign: if has_effects { style.extra.valign } else { TextVAlign::Center },
        mirror: style.mirror,
        bold: style.extra.bold,
        italic: style.extra.italic,
        line_spacing_permille: style.extra.line_spacing_permille,
        face: style.extra.face,
    };
    // `(margins L T R B)`, or `GetLegacyTextMargin` for all four.
    match sexpr::find(item, "margins") {
        Some(m) => {
            let n = |i| sexpr::num(m, i).map_or(0, mm_to_um);
            (t.margin_left_um, t.margin_top_um, t.margin_right_um, t.margin_bottom_um) = (n(1), n(2), n(3), n(4));
        }
        None => {
            let margin = legacy_text_margin_um(t.stroke_width_um, t.size_um);
            (t.margin_left_um, t.margin_top_um, t.margin_right_um, t.margin_bottom_um) = (margin, margin, margin, margin);
        }
    }
    Some(t)
}

/// Every `(gr_text_box ..)` of a board, with the uuid and the lock of each in `refs`.
pub(crate) fn read_all(root: &[Sexpr], refs: &mut Vec<crate::import_items::ItemRef>) -> Vec<TextBox> {
    let version = sexpr::find(root, "version").and_then(|v| sexpr::num(v, 1)).unwrap_or(0.0) as i64;
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "gr_text_box") {
        if let Some(t) = read(item, version) {
            let mut r = crate::import_items::ItemRef::of(item);
            r.locked |= text_of(item).1;
            out.push(t);
            refs.push(r);
        }
    }
    out
}

/// The text box as the `Text` and `TextExtra` its `(effects ..)` are written from.
fn as_text(t: &TextBox) -> (Text, eda_model::board_text::TextExtra) {
    let text = Text {
        id: t.id.clone(),
        content: t.text.clone(),
        at: t.start,
        angle: t.angle,
        layer: t.layer.clone(),
        size_um: t.size_um,
        stroke_width: t.effective_pen_um(),
        justify: t.halign,
        mirror: t.mirror,
    };
    let extra = eda_model::board_text::TextExtra {
        id: t.id.clone(),
        width_um: t.width_um.filter(|w| *w != t.size_um),
        auto_thickness: t.thickness_um == 0,
        valign: t.valign,
        bold: t.bold,
        italic: t.italic,
        knockout: false,
        line_spacing_permille: t.line_spacing_permille,
        face: t.face.clone(),
    };
    (text, extra)
}

/// One text box as `format( const PCB_TEXTBOX* )` writes it; returns its uuid.
pub(crate) fn write(out: &mut String, t: &TextBox, locked: bool) -> String {
    let uuid = crate::duid_for(&format!("textbox:{}", t.id), &t.id);
    write!(out, "\t(gr_text_box {}", sexpr_str(&t.text)).unwrap();
    if locked {
        write!(out, " (locked yes)").unwrap();
    }
    if t.pts.is_empty() {
        write!(out, "\n\t\t(start {} {}) (end {} {})", mm(t.start.x), mm(t.start.y), mm(t.end.x), mm(t.end.y)).unwrap();
    } else {
        write!(out, "\n\t\t(pts").unwrap();
        for p in &t.pts {
            write!(out, " (xy {} {})", mm(p.x), mm(p.y)).unwrap();
        }
        write!(out, ")").unwrap();
    }
    write!(out, " (margins {} {} {} {})", mm(t.margin_left_um), mm(t.margin_top_um), mm(t.margin_right_um), mm(t.margin_bottom_um)).unwrap();
    if t.angle != 0 {
        write!(out, " (angle {})", fmt_mm_f(t.angle as f64 / 1000.0)).unwrap();
    }
    write!(out, " (layer {}) (uuid \"{uuid}\")", sexpr_str(&t.layer)).unwrap();
    let (text, extra) = as_text(t);
    write!(out, "\n\t\t{}", crate::board_text_io::effects(&text, Some(&extra))).unwrap();
    write!(out, "\n\t\t(border {}) (stroke (width {}) (type {})", if t.border { "yes" } else { "no" }, mm(t.stroke_width_um), t.line_style.token()).unwrap();
    if let Some(c) = &t.stroke_color {
        write!(out, " (color {c})").unwrap();
    }
    writeln!(out, ") (knockout {})\n\t)", if t.knockout { "yes" } else { "no" }).unwrap();
    uuid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_one(text: &str) -> TextBox {
        let tree = sexpr::parse(text).unwrap();
        let root = tree.as_list().unwrap();
        let mut refs = Vec::new();
        let mut v = read_all(root, &mut refs);
        assert_eq!(v.len(), refs.len());
        v.remove(0)
    }

    const BOX: &str = r#"(kicad_pcb (version 20241229)
  (gr_text_box "Box\nof text" (start 20 20) (end 30 25) (margins 1 1.5 2 2.5) (layer "F.SilkS") (uuid "11111111-1111-1111-1111-111111111111")
    (effects (font (size 1 1.2) (thickness 0.15)) (justify left top))
    (border yes) (stroke (width 0.12) (type dot)) (knockout yes))
)"#;

    #[test]
    fn a_text_box_is_read_as_parse_pcb_textbox_reads_it() {
        let b = read_one(BOX);
        assert_eq!(b.text, "Box\nof text");
        assert_eq!((b.start, b.end, b.layer.as_str()), (Point { x: 20_000, y: 20_000 }, Point { x: 30_000, y: 25_000 }, "F.SilkS"));
        assert_eq!((b.margin_left_um, b.margin_top_um, b.margin_right_um, b.margin_bottom_um), (1_000, 1_500, 2_000, 2_500));
        assert_eq!((b.border, b.stroke_width_um, b.line_style, b.knockout), (true, 120, LineStyle::Dot, true));
        // `(size HEIGHT WIDTH)`
        assert_eq!((b.size_um, b.width_um, b.thickness_um), (1_000, Some(1_200), 150));
        assert_eq!((b.halign, b.valign, b.mirror), (TextJustify::Left, TextVAlign::Top, false));
        assert_eq!(b.angle, 0);
    }

    #[test]
    fn a_box_with_no_margins_has_the_legacy_margin_and_one_with_no_effects_is_left_and_centre() {
        let b = read_one(r#"(kicad_pcb (version 20241229) (gr_text_box "x" (start 0 0) (end 5 5) (layer "F.SilkS") (border yes) (stroke (width 0.2) (type solid))))"#);
        // 0.2 / 2 + 1.524 * 0.75
        assert_eq!(b.margin_left_um, 100 + 1_143);
        assert_eq!((b.halign, b.valign, b.size_um), (TextJustify::Left, TextVAlign::Center, 1_524));
        // The effects of a file that wrote none of the justification are centre and centre.
        let c = read_one(r#"(kicad_pcb (version 20241229) (gr_text_box "x" (start 0 0) (end 5 5) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15)))))"#);
        assert_eq!((c.halign, c.valign), (TextJustify::Center, TextVAlign::Center));
    }

    #[test]
    fn before_the_border_flag_the_stroke_width_says_whether_there_is_a_border() {
        let old = |w: &str| read_one(&format!(r#"(kicad_pcb (version 20230620) (gr_text_box "x" (start 0 0) (end 5 5) (layer "F.SilkS") (stroke (width {w}) (type solid)) (effects (font (size 1 1)))))"#));
        assert!(old("0.1").border);
        assert!(!old("-1").border, "a negative width was no border");
        assert_eq!(old("-1").stroke_width_um, 0);
    }

    #[test]
    fn a_box_turned_by_any_angle_is_a_polygon_and_the_text_angle_is_kept() {
        let b = read_one(r#"(kicad_pcb (version 20241229) (gr_text_box "x" (pts (xy 0 0) (xy 5 5) (xy 0 10) (xy -5 5)) (angle 45) (layer "F.SilkS") (effects (font (size 1 1))) (border no)))"#);
        assert_eq!(b.pts.len(), 4);
        assert_eq!((b.angle, b.border, b.is_rect()), (45_000, false, false));
        assert_eq!(b.bounds(), [-5_000, 0, 5_000, 10_000]);
    }

    #[test]
    fn what_is_written_is_read_back_the_same() {
        let b = read_one(BOX);
        let mut out = String::from("(kicad_pcb (version 20241229)\n");
        let uuid = write(&mut out, &b, true);
        out.push_str(")\n");
        assert!(out.contains("(locked yes)") && out.contains(&uuid), "{out}");
        let tree = sexpr::parse(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
        let mut refs = Vec::new();
        let back = read_all(tree.as_list().unwrap(), &mut refs);
        assert_eq!(back, vec![b.clone()]);
        assert!(refs[0].locked && refs[0].uuid.as_deref() == Some(uuid.as_str()));
        // A rotated polygon box and the file's own angle.
        let mut p = b;
        p.pts = vec![Point { x: 0, y: 0 }, Point { x: 5_000, y: 5_000 }, Point { x: 0, y: 10_000 }, Point { x: -5_000, y: 5_000 }];
        p.start = p.pts[0];
        p.end = p.pts[2];
        p.angle = 45_000;
        p.border = false;
        let mut out = String::from("(kicad_pcb (version 20241229)\n");
        write(&mut out, &p, false);
        out.push_str(")\n");
        let tree = sexpr::parse(&out).unwrap();
        let back = read_all(tree.as_list().unwrap(), &mut Vec::new());
        assert_eq!(back, vec![p]);
    }

    #[test]
    fn a_stroke_colour_and_a_font_survive() {
        let mut b = read_one(BOX);
        b.stroke_color = Some("255 0 0 1".into());
        b.thickness_um = 0;
        b.bold = true;
        b.italic = true;
        b.line_spacing_permille = 1_500;
        b.mirror = true;
        let mut out = String::from("(kicad_pcb (version 20241229)\n");
        write(&mut out, &b, false);
        out.push_str(")\n");
        assert!(out.contains("(color 255 0 0 1)") && out.contains("(line_spacing 1.5)") && out.contains("bold italic") && out.contains("mirror"), "{out}");
        assert!(!out.contains("(thickness"), "an automatic pen writes no thickness: {out}");
        let back = read_one(&out);
        assert_eq!(back, b);
    }
}
