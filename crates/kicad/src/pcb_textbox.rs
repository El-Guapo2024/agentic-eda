//! A board's text boxes (`(gr_text_box ..)`, KiCad's `PCB_TEXTBOX`) read and written: `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_TEXTBOX` / `parseTextBoxContent` and
//! `PCB_IO_KICAD_SEXPR::format( const PCB_TEXTBOX* )`, for [`eda_model::pcb_items::TextBox`].
//!
//! The box's looks are an `(effects ..)` like any text's, read and written by the code of [`crate::board_text_io`] (a text box is a `Text` and a `TextExtra` to it).
//! A footprint's own `(fp_text_box ..)` is not read here: it is part of the footprint (`FootprintExtra::tokens`).

use eda_model::units::Nm;
use std::fmt::Write as _;

use eda_model::ir::{Point, Text, TextJustify, TextVAlign};
use eda_model::pcb_items::{legacy_text_margin_nm, LineStyle, TextBox};

use crate::sexpr::{self, Sexpr};
use crate::{fmt_mm_f, mm, mm_to_nm, sexpr_str};

/// `STROKE_PARAMS`' width of a text box that says nothing: KiCad's `STROKE_PARAMS stroke( -1, LINE_STYLE::SOLID )` before the file's `(stroke ..)`.
const NO_STROKE: i64 = -1;
/// The first board file version with an explicit `(border yes|no)`; before it a text box had a border when its stroke width was not negative.
const FIRST_BORDER_FLAG: i64 = 20230825;
/// The text size of a text with no `(size ..)` in its effects (`parseEDA_TEXT`: "force legacy default units", 60 mils).
const LEGACY_TEXT_SIZE_NM: i64 = 1_524_000;

fn yes(item: &[Sexpr], name: &str) -> Option<bool> {
    sexpr::find(item, name).map(|l| sexpr::txt(l, 1).map_or(true, |v| v != "no"))
}

fn point(l: &[Sexpr]) -> Option<Point> {
    Some(Point { x: mm_to_nm(sexpr::num(l, 1)?), y: mm_to_nm(sexpr::num(l, 2)?) })
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
    let width = stroke.and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).map(mm_to_nm).unwrap_or(Nm::from_raw_shim(NO_STROKE));
    let line_style = stroke.and_then(|s| sexpr::find(s, "type")).and_then(|t| sexpr::txt(t, 1)).map(LineStyle::from_token).unwrap_or(LineStyle::Solid);
    let stroke_color = stroke.and_then(|s| sexpr::find(s, "color")).map(|c| c.iter().skip(1).filter_map(Sexpr::text).collect::<Vec<_>>().join(" ")).filter(|c| !c.is_empty());
    // `if( m_requiredVersion < 20230825 ) aTextBox->SetBorderEnabled( stroke.GetWidth() >= 0 );`
    let border = if version < FIRST_BORDER_FLAG { width >= Nm::ZERO } else { yes(item, "border").unwrap_or(true) };
    // `PCB_SHAPE::GetWidth`: "negative stroke-widths are only used in EEschema".
    let stroke_width_nm = width.max(Nm::ZERO);

    // The looks: `parseEDA_TEXT` starts a box from centre and centre when it has `(effects ..)`; without them the box is left and centre (`PCB_TEXTBOX::PCB_TEXTBOX`).
    let has_effects = sexpr::find(item, "effects").is_some();
    let style = crate::board_text_io::read_style(item);
    let has_size = sexpr::find(item, "effects").and_then(|e| sexpr::find(e, "font")).and_then(|f| sexpr::find(f, "size")).is_some();
    let size_nm = if has_size { style.size_nm } else { Nm::from_raw_shim(LEGACY_TEXT_SIZE_NM) };

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
        margin_left_nm: Nm::ZERO,
        margin_top_nm: Nm::ZERO,
        margin_right_nm: Nm::ZERO,
        margin_bottom_nm: Nm::ZERO,
        border,
        stroke_width_nm,
        line_style,
        stroke_color,
        knockout: yes(item, "knockout").unwrap_or(false),
        size_nm,
        width_nm: style.extra.width_nm.filter(|_| has_size),
        thickness_nm: if style.extra.auto_thickness { Nm::ZERO } else { style.stroke_width },
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
            let n = |i| sexpr::num(m, i).map_or(Nm::ZERO, mm_to_nm);
            (t.margin_left_nm, t.margin_top_nm, t.margin_right_nm, t.margin_bottom_nm) = (n(1), n(2), n(3), n(4));
        }
        None => {
            let margin = legacy_text_margin_nm(t.stroke_width_nm, t.size_nm);
            (t.margin_left_nm, t.margin_top_nm, t.margin_right_nm, t.margin_bottom_nm) = (margin, margin, margin, margin);
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
        size_nm: t.size_nm,
        stroke_width: t.effective_pen_nm(),
        justify: t.halign,
        mirror: t.mirror,
    };
    let extra = eda_model::board_text::TextExtra {
        id: t.id.clone(),
        width_nm: t.width_nm.filter(|w| *w != t.size_nm),
        auto_thickness: t.thickness_nm == Nm::ZERO,
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
    write!(out, " (margins {} {} {} {})", mm(t.margin_left_nm), mm(t.margin_top_nm), mm(t.margin_right_nm), mm(t.margin_bottom_nm)).unwrap();
    if t.angle != 0 {
        write!(out, " (angle {})", fmt_mm_f(t.angle as f64 / 1000.0)).unwrap();
    }
    write!(out, " (layer {}) (uuid \"{uuid}\")", sexpr_str(&t.layer)).unwrap();
    let (text, extra) = as_text(t);
    write!(out, "\n\t\t{}", crate::board_text_io::effects(&text, Some(&extra))).unwrap();
    write!(out, "\n\t\t(border {}) (stroke (width {}) (type {})", if t.border { "yes" } else { "no" }, mm(t.stroke_width_nm), t.line_style.token()).unwrap();
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
        assert_eq!((b.start, b.end, b.layer.as_str()), (Point { x: Nm::from_raw_shim(20_000_000), y: Nm::from_raw_shim(20_000_000) }, Point { x: Nm::from_raw_shim(30_000_000), y: Nm::from_raw_shim(25_000_000) }, "F.SilkS"));
        assert_eq!((b.margin_left_nm, b.margin_top_nm, b.margin_right_nm, b.margin_bottom_nm), (Nm::from_raw_shim(1_000_000), Nm::from_raw_shim(1_500_000), Nm::from_raw_shim(2_000_000), Nm::from_raw_shim(2_500_000)));
        assert_eq!((b.border, b.stroke_width_nm, b.line_style, b.knockout), (true, Nm::from_raw_shim(120_000), LineStyle::Dot, true));
        // `(size HEIGHT WIDTH)`
        assert_eq!((b.size_nm, b.width_nm, b.thickness_nm), (Nm::from_raw_shim(1_000_000), Some(Nm::from_raw_shim(1_200_000)), Nm::from_raw_shim(150_000)));
        assert_eq!((b.halign, b.valign, b.mirror), (TextJustify::Left, TextVAlign::Top, false));
        assert_eq!(b.angle, 0);
    }

    #[test]
    fn a_box_with_no_margins_has_the_legacy_margin_and_one_with_no_effects_is_left_and_centre() {
        let b = read_one(r#"(kicad_pcb (version 20241229) (gr_text_box "x" (start 0 0) (end 5 5) (layer "F.SilkS") (border yes) (stroke (width 0.2) (type solid))))"#);
        // 0.2 / 2 + 1.524 * 0.75
        assert_eq!(b.margin_left_nm, Nm::from_raw_shim(100_000 + 1_143_000));
        assert_eq!((b.halign, b.valign, b.size_nm), (TextJustify::Left, TextVAlign::Center, Nm::from_raw_shim(1_524_000)));
        // The effects of a file that wrote none of the justification are centre and centre.
        let c = read_one(r#"(kicad_pcb (version 20241229) (gr_text_box "x" (start 0 0) (end 5 5) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15)))))"#);
        assert_eq!((c.halign, c.valign), (TextJustify::Center, TextVAlign::Center));
    }

    #[test]
    fn before_the_border_flag_the_stroke_width_says_whether_there_is_a_border() {
        let old = |w: &str| read_one(&format!(r#"(kicad_pcb (version 20230620) (gr_text_box "x" (start 0 0) (end 5 5) (layer "F.SilkS") (stroke (width {w}) (type solid)) (effects (font (size 1 1)))))"#));
        assert!(old("0.1").border);
        assert!(!old("-1").border, "a negative width was no border");
        assert_eq!(old("-1").stroke_width_nm, Nm::ZERO);
    }

    #[test]
    fn a_box_turned_by_any_angle_is_a_polygon_and_the_text_angle_is_kept() {
        let b = read_one(r#"(kicad_pcb (version 20241229) (gr_text_box "x" (pts (xy 0 0) (xy 5 5) (xy 0 10) (xy -5 5)) (angle 45) (layer "F.SilkS") (effects (font (size 1 1))) (border no)))"#);
        assert_eq!(b.pts.len(), 4);
        assert_eq!((b.angle, b.border, b.is_rect()), (45_000, false, false));
        assert_eq!(b.bounds(), [Nm::from_raw_shim(-5_000_000), Nm::ZERO, Nm::from_raw_shim(5_000_000), Nm::from_raw_shim(10_000_000)]);
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
        p.pts = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::from_raw_shim(5_000_000), y: Nm::from_raw_shim(5_000_000) }, Point { x: Nm::ZERO, y: Nm::from_raw_shim(10_000_000) }, Point { x: Nm::from_raw_shim(-5_000_000), y: Nm::from_raw_shim(5_000_000) }];
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
        b.thickness_nm = Nm::ZERO;
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
