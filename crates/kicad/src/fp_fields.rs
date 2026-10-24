//! A footprint's fields, attributes and pad extras in the `.kicad_pcb`: written the way `PCB_IO_KICAD_SEXPR` writes them
//! (`format( FOOTPRINT )`, `format( PCB_TEXT )`, `EDA_TEXT::Format`, `format( PAD )` at KiCad 8303b2ad) and read back the way
//! `PCB_IO_KICAD_SEXPR_PARSER` reads them (`parseFOOTPRINT_unchecked`, `parsePCB_TEXT_effects`, `parseEDA_TEXT`).
//!
//! The model side is [`eda_model::fp_edit`]: a [`FootprintEdit`] per footprint holds the Reference and Value layouts, the user
//! fields, the attributes and the pad edits. The writer takes an unedited footprint to exactly the text it has always been.

use std::fmt::Write as _;

use eda_model::fp_edit::{default_reference_layout, default_value_layout, FieldLayout, FootprintAttrs, FootprintEdit, FootprintKind, PadEdit, UserField, REFERENCE, VALUE};
use eda_model::ir::{FootprintInstance, Millideg, Point, Side, Nm};
use eda_model::{Footprint, Part};

use crate::sexpr::{self, Sexpr};
use crate::{duid_for, fmt_mm_f, mm, mm_to_nm as mm_to_nm_f, sexpr_str};

/// What a footprint's text needs beyond the footprint itself: its edit, if it has one, and the attributes in effect.
pub(crate) struct FpExtra<'a> {
    pub edit: Option<&'a FootprintEdit>,
    pub attrs: FootprintAttrs,
}

impl<'a> FpExtra<'a> {
    /// The edit and attributes the design holds for `id`.
    pub fn of(design: &'a eda_model::ir::Design, id: &str, footprint: &Footprint) -> Self {
        FpExtra { edit: design.footprint_edit(id), attrs: design.effective_attrs(id, footprint) }
    }
}

// ---------------------------------------------------------------------------------------------------------------- writing

/// `(effects (font (size H W) [(thickness T)] [bold] [italic]) [(justify [left|right] [top|bottom] [mirror])])`:
/// `EDA_TEXT::Format`. The size is written height first; a thickness of 0 is KiCad's auto thickness and is left out.
fn effects(l: &FieldLayout) -> String {
    effects_with(l, None, 0)
}

/// [`effects`] with the font face and the line spacing a text may have (`(face ..)` first, `(line_spacing ..)` after the size, in thousandths: 1000 is
/// KiCad's 1.0 and is not written).
pub(crate) fn effects_with(l: &FieldLayout, face: Option<&str>, line_spacing_permille: i64) -> String {
    let mut s = String::from("(effects (font");
    if let Some(f) = face.filter(|f| !f.is_empty()) {
        write!(s, " (face {})", sexpr_str(f)).unwrap();
    }
    write!(s, " (size {} {})", mm(l.size.1), mm(l.size.0)).unwrap();
    if line_spacing_permille != 0 && line_spacing_permille != 1000 {
        write!(s, " (line_spacing {})", fmt_mm_f(line_spacing_permille as f64 / 1000.0)).unwrap();
    }
    if l.thickness > 0 {
        write!(s, " (thickness {})", mm(l.thickness)).unwrap();
    }
    if l.bold {
        s.push_str(" bold");
    }
    if l.italic {
        s.push_str(" italic");
    }
    s.push(')');
    if l.mirror || l.halign != 0 || l.valign != 0 {
        s.push_str(" (justify");
        match l.halign {
            -1 => s.push_str(" left"),
            1 => s.push_str(" right"),
            _ => {}
        }
        match l.valign {
            -1 => s.push_str(" top"),
            1 => s.push_str(" bottom"),
            _ => {}
        }
        if l.mirror {
            s.push_str(" mirror");
        }
        s.push(')');
    }
    s.push(')');
    s
}

/// One `(property "name" "text" (at x y angle) [(unlocked yes)] (layer "L") [(hide yes)] (uuid ..) (effects ..))` of a footprint
/// (`PCB_IO_KICAD_SEXPR::format( PCB_TEXT )` for a `PCB_FIELD`).
pub(crate) fn write_property(out: &mut String, name: &str, text: &str, l: &FieldLayout, side: Side, fp_rot: Millideg, uuid: &str) {
    let (x, y) = l.file_position(side);
    let angle = fmt_mm_f(l.file_angle_deg(side, fp_rot));
    let unlocked = if l.keep_upright { "" } else { " (unlocked yes)" };
    let knockout = if l.knockout { " knockout" } else { "" };
    let hide = if l.visible { "" } else { " (hide yes)" };
    writeln!(out, "\t\t(property {} {} (at {} {} {angle}){unlocked} (layer {}{knockout}){hide}\n\t\t\t(uuid \"{uuid}\")\n\t\t\t{}\n\t\t)", sexpr_str(name), sexpr_str(text), mm(x), mm(y), sexpr_str(&l.layer), effects(l)).unwrap();
}

/// The Reference, the Value and the user fields of a footprint, in KiCad's order.
pub(crate) fn write_fields(out: &mut String, fp: &FootprintInstance, part: &Part, footprint: &Footprint, extra: &FpExtra) {
    let reference = extra.edit.and_then(|e| e.reference.clone()).unwrap_or_else(|| default_reference_layout(fp, footprint));
    let ref_uuid = duid_for(&format!("footprint:{}:ref", fp.id), &fp.id);
    write_property(out, REFERENCE, reference.shown(&fp.id), &reference, fp.side, fp.rot, &ref_uuid);

    let value = extra.edit.and_then(|e| e.value.clone()).unwrap_or_else(|| default_value_layout(fp));
    let val_uuid = duid_for(&format!("footprint:{}:val", fp.id), &fp.id);
    write_property(out, VALUE, value.shown(part.value.as_deref().unwrap_or(&fp.id)), &value, fp.side, fp.rot, &val_uuid);

    if let Some(edit) = extra.edit {
        for (i, f) in edit.fields.iter().enumerate() {
            let uuid = duid_for(&format!("footprint:{}:field:{i}", fp.id), &fp.id);
            write_property(out, &f.name, &f.text, &f.layout, fp.side, fp.rot, &uuid);
        }
    }
}

/// `(attr smd through_hole board_only exclude_from_pos_files exclude_from_bom allow_missing_courtyard dnp)`: only the tokens that
/// are set, and nothing at all when none is (`format( FOOTPRINT )`).
pub(crate) fn write_attr(out: &mut String, a: &FootprintAttrs) {
    let mut tokens: Vec<&str> = Vec::new();
    match a.kind {
        FootprintKind::Smd => tokens.push("smd"),
        FootprintKind::ThroughHole => tokens.push("through_hole"),
        FootprintKind::Unspecified => {}
    }
    if a.board_only {
        tokens.push("board_only");
    }
    if a.exclude_from_pos_files {
        tokens.push("exclude_from_pos_files");
    }
    if a.exclude_from_bom {
        tokens.push("exclude_from_bom");
    }
    if a.allow_missing_courtyard {
        tokens.push("allow_missing_courtyard");
    }
    if a.dnp {
        tokens.push("dnp");
    }
    if !tokens.is_empty() {
        writeln!(out, "\t\t(attr {})", tokens.join(" ")).unwrap();
    }
}

/// What a pad edit adds to a pad's text: the margins, which follow the net. (The shape offset is the pad's own, written inside the
/// `(drill ..)` token by `crate::pad_shape`.)
pub(crate) struct PadText {
    /// ` (solder_mask_margin ..) (solder_paste_margin ..) (solder_paste_margin_ratio ..)`, whichever are set. The pad's `(clearance ..)`
    /// and its zone connection are written by the caller from `Design::pad_zone_facts`.
    pub margins: String,
}

pub(crate) fn pad_text(edit: Option<&PadEdit>) -> PadText {
    let Some(e) = edit else { return PadText { margins: String::new() } };
    let mut margins = String::new();
    if let Some(m) = e.solder_mask_margin {
        write!(margins, " (solder_mask_margin {})", mm(m)).unwrap();
    }
    if let Some(m) = e.solder_paste_margin {
        write!(margins, " (solder_paste_margin {})", mm(m)).unwrap();
    }
    if let Some(r) = e.solder_paste_margin_ratio {
        write!(margins, " (solder_paste_margin_ratio {})", fmt_mm_f(r)).unwrap();
    }
    PadText { margins }
}

// ---------------------------------------------------------------------------------------------------------------- reading

fn bool_tok(item: &[Sexpr], name: &str) -> Option<bool> {
    // `(name yes)`, `(name no)`, `(name)` (= yes) or a bare `name` atom in the item itself.
    if let Some(l) = sexpr::find(item, name) {
        return Some(sexpr::txt(l, 1).map_or(true, |v| v != "no"));
    }
    item.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == name)).then_some(true)
}

/// A `(property ..)` or `(fp_text reference|value ..)`'s layout, un-mirrored and un-rotated into the footprint's frame
/// ([`FieldLayout::set_from_file`]). `text` is its raw text.
pub(crate) fn parse_layout(item: &[Sexpr], side: Side, fp_rot: Millideg) -> FieldLayout {
    let default_layer = if side == Side::Bottom { "B.Fab" } else { "F.Fab" };
    let layer_node = sexpr::find(item, "layer");
    let layer = layer_node.and_then(|l| sexpr::txt(l, 1)).unwrap_or(default_layer);
    let mut l = FieldLayout::new(Point { x: 0, y: 0 }, layer);
    l.knockout = layer_node.is_some_and(|n| n.iter().skip(2).any(|c| matches!(c, Sexpr::Atom(a) if a == "knockout")));
    if let Some(at) = sexpr::find(item, "at") {
        let (x, y) = (sexpr::num(at, 1).unwrap_or(0.0), sexpr::num(at, 2).unwrap_or(0.0));
        let angle = sexpr::num(at, 3).unwrap_or(0.0);
        l.set_from_file(side, fp_rot, mm_to_nm_f(x), mm_to_nm_f(y), angle);
        // The legacy `(at x y angle unlocked)`.
        if at.iter().skip(1).any(|c| matches!(c, Sexpr::Atom(a) if a == "unlocked")) {
            l.keep_upright = false;
        }
    } else {
        l.set_from_file(side, fp_rot, 0, 0, 0.0);
    }
    if let Some(u) = bool_tok(item, "unlocked") {
        l.keep_upright = !u;
    }
    let effects = sexpr::find(item, "effects");
    l.visible = !(bool_tok(item, "hide").unwrap_or(false) || effects.is_some_and(|e| bool_tok(e, "hide").unwrap_or(false)));
    // `mirror` is `FieldLayout::new`'s side default until the file says otherwise.
    l.mirror = false;
    if let Some(e) = effects {
        if let Some(font) = sexpr::find(e, "font") {
            if let Some(s) = sexpr::find(font, "size") {
                // `(size HEIGHT WIDTH)`
                l.size = (mm_to_nm_f(sexpr::num(s, 2).unwrap_or(1.0)), mm_to_nm_f(sexpr::num(s, 1).unwrap_or(1.0)));
            }
            l.thickness = sexpr::find(font, "thickness").and_then(|t| sexpr::num(t, 1)).map(mm_to_nm_f).unwrap_or(0);
            l.bold = bool_tok(font, "bold").unwrap_or(false);
            l.italic = bool_tok(font, "italic").unwrap_or(false);
        }
        if let Some(j) = sexpr::find(e, "justify") {
            for tok in j.iter().skip(1).filter_map(Sexpr::text) {
                match tok {
                    "left" => l.halign = -1,
                    "right" => l.halign = 1,
                    "top" => l.valign = -1,
                    "bottom" => l.valign = 1,
                    "mirror" => l.mirror = true,
                    _ => {}
                }
            }
        }
    }
    l
}

/// `(attr ..)`'s tokens (`parseFOOTPRINT_unchecked`'s `T_attr`); the legacy `virtual` means excluded from the position files and
/// the BOM.
pub(crate) fn parse_attrs(attr: &[Sexpr]) -> FootprintAttrs {
    let mut a = FootprintAttrs::default();
    for t in attr.iter().skip(1).filter_map(Sexpr::text) {
        match t {
            "smd" => a.kind = FootprintKind::Smd,
            "through_hole" => a.kind = FootprintKind::ThroughHole,
            "board_only" => a.board_only = true,
            "exclude_from_pos_files" => a.exclude_from_pos_files = true,
            "exclude_from_bom" => a.exclude_from_bom = true,
            "allow_missing_courtyard" => a.allow_missing_courtyard = true,
            "dnp" => a.dnp = true,
            "virtual" => {
                a.exclude_from_pos_files = true;
                a.exclude_from_bom = true;
            }
            _ => {}
        }
    }
    a
}

/// What a `(footprint ..)` says about its fields and attributes, as a [`FootprintEdit`] without pad edits: the Reference and Value
/// layouts (always, so the file's own placement survives), the user fields (the properties that are not Reference, Value,
/// Datasheet, Description, the legacy Footprint field or `ki_fp_filters`), and the attributes when the footprint has an `(attr ..)`.
/// `id` is the id the importer gave the footprint, `value` the part's value.
#[cfg(test)]
pub(crate) fn parse_footprint_edit(fp: &[Sexpr], id: &str, value: Option<&str>, side: Side, fp_rot: Millideg) -> FootprintEdit {
    parse_footprint_edit_in(fp, id, value, side, fp_rot, i64::MAX)
}

/// The first file version whose properties are fields with a place and a layer of their own (`PCB_FIELD`s: KiCad 8). Before it a `(property "name" "value")`
/// held a value and nothing more.
pub(crate) const FIELD_PROPERTIES_VERSION: i64 = 20230620;

/// [`parse_footprint_edit`] for a file of `file_version`. A property of an older file is not drawn (`parseFOOTPRINT_unchecked`: "Hide the field by default if
/// it is a legacy field that did not have text effects applied"), and the ones that held the sheet, the symbol's keywords and its description are not
/// fields at all.
pub(crate) fn parse_footprint_edit_in(fp: &[Sexpr], id: &str, value: Option<&str>, side: Side, fp_rot: Millideg, file_version: i64) -> FootprintEdit {
    let legacy = file_version < FIELD_PROPERTIES_VERSION;
    let mut edit = FootprintEdit::new(id);
    for p in sexpr::find_all(fp, "property") {
        let (Some(name), Some(text)) = (sexpr::txt(p, 1), sexpr::txt(p, 2)) else { continue };
        if legacy && matches!(name, "ki_keywords" | "ki_locked" | "ki_description" | "Sheetfile" | "Sheet file" | "Sheetname" | "Sheet name") {
            continue;
        }
        let mut layout = parse_layout(p, side, fp_rot);
        if legacy {
            layout.visible = bool_tok(p, "hide") == Some(false);
        }
        match name {
            REFERENCE => edit.reference = Some(with_text(layout, text, id)),
            VALUE => edit.value = Some(with_text(layout, text, value.unwrap_or(id))),
            "Datasheet" | "Description" | "Footprint" | "ki_fp_filters" | "ki_keywords" | "ki_locked" | "ki_description" => {}
            _ => edit.fields.push(UserField { name: name.to_string(), text: text.to_string(), layout }),
        }
    }
    // The pre-KiCad-8 form: `(fp_text reference "R1" (at ..) (layer ..) (effects ..))`.
    for t in sexpr::find_all(fp, "fp_text") {
        let (Some(kind), Some(text)) = (sexpr::txt(t, 1), sexpr::txt(t, 2)) else { continue };
        let layout = parse_layout(t, side, fp_rot);
        match kind {
            "reference" if edit.reference.is_none() => edit.reference = Some(with_text(layout, text, id)),
            "value" if edit.value.is_none() => edit.value = Some(with_text(layout, text, value.unwrap_or(id))),
            _ => {}
        }
    }
    edit.attrs = sexpr::find(fp, "attr").map(parse_attrs);
    edit
}

/// The layout with the text it shows when that is not `own`.
fn with_text(mut l: FieldLayout, text: &str, own: &str) -> FieldLayout {
    l.text = (text != own).then(|| text.to_string());
    l
}

/// A pad's offset and margins as an edit without identity (`number`/`nth` are the caller's): the shape offset inside `(drill ..)`
/// (x un-mirrored for a bottom-side footprint), `(solder_mask_margin ..)`, `(solder_paste_margin ..)`,
/// and `(solder_paste_margin_ratio ..)`. Before file version 20240201 a margin of 0 meant "inherit". The pad's `(clearance ..)` and
/// zone connection are read by the importer into `FootprintExtra`, with the rest of the zone facts.
pub(crate) fn parse_pad_extras(pad: &[Sexpr], side: Side, file_version: i64) -> PadEdit {
    let mut e = PadEdit::default();
    if let Some(d) = sexpr::find(pad, "drill") {
        if let Some(o) = sexpr::find(d, "offset") {
            let (x, y) = (mm_to_nm_f(sexpr::num(o, 1).unwrap_or(0.0)), mm_to_nm_f(sexpr::num(o, 2).unwrap_or(0.0)));
            if x != 0 || y != 0 {
                e.offset = Some(Point { x: if side == Side::Bottom { -x } else { x }, y });
            }
        }
    }
    let margin = |name: &str| -> Option<Nm> {
        let m = sexpr::find(pad, name).and_then(|f| sexpr::num(f, 1)).map(mm_to_nm_f)?;
        if file_version <= 20240201 && m == 0 {
            None
        } else {
            Some(m)
        }
    };
    e.solder_mask_margin = margin("solder_mask_margin");
    e.solder_paste_margin = margin("solder_paste_margin");
    e.solder_paste_margin_ratio = sexpr::find(pad, "solder_paste_margin_ratio").and_then(|f| sexpr::num(f, 1)).filter(|r| *r != 0.0 || file_version > 20240201);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Sexpr> {
        sexpr::parse(text).unwrap().as_list().unwrap().to_vec()
    }

    #[test]
    fn a_property_is_written_the_way_kicad_formats_a_field() {
        let mut l = FieldLayout::new(Point { x: 1_000_000, y: -2_000_000 }, "F.SilkS");
        l.size = (1_200_000, 800_000);
        l.thickness = 150_000;
        l.bold = true;
        l.halign = -1;
        l.valign = 1;
        l.visible = false;
        l.keep_upright = false;
        let mut out = String::new();
        write_property(&mut out, "Reference", "R1", &l, Side::Top, 0, "u-1");
        assert!(out.contains("(property \"Reference\" \"R1\" (at 1 -2 0) (unlocked yes) (layer \"F.SilkS\") (hide yes)"), "{out}");
        assert!(out.contains("(effects (font (size 0.8 1.2) (thickness 0.15) bold) (justify left bottom))"), "height is written first: {out}");
    }

    #[test]
    fn a_bottom_side_field_is_written_unmirrored_in_the_file_frame_and_mirrored_in_its_effects() {
        let mut l = FieldLayout::new(Point { x: 1_000_000, y: 0 }, "B.SilkS");
        l.angle = 90_000;
        let mut out = String::new();
        write_property(&mut out, "Value", "10k", &l, Side::Bottom, 0, "u-2");
        // x is negated; the sense of the angle turns: -90 deg = 270
        assert!(out.contains("(at -1 0 270)"), "{out}");
        assert!(out.contains("(justify mirror)"), "{out}");
    }

    #[test]
    fn what_is_written_reads_back_to_the_same_layout() {
        for (side, rot) in [(Side::Top, 0), (Side::Top, 90_000), (Side::Bottom, 0), (Side::Bottom, 270_000)] {
            let mut l = FieldLayout::new(Point { x: 1_234_000, y: -2_345_000 }, if side == Side::Bottom { "B.SilkS" } else { "F.SilkS" });
            l.angle = 45_000;
            l.size = (1_100_000, 900_000);
            l.thickness = 120_000;
            l.italic = true;
            l.halign = 1;
            l.valign = -1;
            let mut out = String::new();
            write_property(&mut out, "Vendor", "ACME", &l, side, rot, "u-3");
            let node = parse(&format!("(footprint \"x\" {out})"));
            let p = sexpr::find(&node, "property").unwrap();
            let back = parse_layout(p, side, rot);
            assert_eq!(back, l, "{side:?} {rot}");
        }
    }

    #[test]
    fn a_footprint_edit_is_read_from_its_fields_and_attributes() {
        let node = parse(
            r#"(footprint "Lib:Name" (layer "F.Cu") (at 10 20 90)
                (attr smd dnp exclude_from_bom)
                (property "Reference" "R7" (at 0 -1.5 0) (layer "F.SilkS") (uuid "a") (effects (font (size 1 1) (thickness 0.15))))
                (property "Value" "10k" (at 0 1.5 0) (layer "F.Fab") (hide yes) (uuid "b") (effects (font (size 1 1) (thickness 0.15))))
                (property "Datasheet" "" (at 0 0 0) (layer "F.Fab") (hide yes) (effects (font (size 1.27 1.27))))
                (property "Vendor" "ACME" (at 1 2 0) (layer "F.Fab") (hide yes) (effects (font (size 1 1) (thickness 0.15)))))"#,
        );
        let e = parse_footprint_edit(&node, "R7", Some("10k"), Side::Top, 270_000);
        assert_eq!(e.reference.as_ref().unwrap().layer, "F.SilkS");
        assert!(e.reference.as_ref().unwrap().visible);
        assert!(!e.value.as_ref().unwrap().visible);
        assert_eq!(e.reference.as_ref().unwrap().text, None, "the text is the reference");
        assert_eq!(e.fields.len(), 1);
        assert_eq!((e.fields[0].name.as_str(), e.fields[0].text.as_str()), ("Vendor", "ACME"));
        assert!(!e.fields[0].layout.visible);
        let a = e.attrs.unwrap();
        assert_eq!(a.kind, FootprintKind::Smd);
        assert!(a.dnp && a.exclude_from_bom && !a.board_only && !a.exclude_from_pos_files);
    }

    #[test]
    fn a_property_of_an_old_file_is_hidden_and_the_sheet_ones_are_not_fields() {
        let node = parse(
            r#"(footprint "x" (layer "F.Cu") (at 0 0)
                (property "Sheet file" "a.kicad_sch") (property "Sheetname" "root") (property "ki_description" "an inductor") (property "Manufacturer" "ACME")
                (property "Shown" "yes" (hide no)))"#,
        );
        let e = parse_footprint_edit_in(&node, "L1", None, Side::Top, 0, 20210722);
        let names: Vec<&str> = e.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["Manufacturer", "Shown"]);
        assert_eq!((e.fields[0].layout.visible, e.fields[1].layout.visible), (false, true));
        // the same properties in a current file are fields like any other, shown
        let e = parse_footprint_edit_in(&node, "L1", None, Side::Top, 0, 20241229);
        assert_eq!(e.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["Sheet file", "Sheetname", "Manufacturer", "Shown"]);
        assert!(e.fields.iter().all(|f| f.layout.visible));
    }

    #[test]
    fn a_reference_shared_with_another_footprint_keeps_the_text_the_file_has() {
        let node = parse(r#"(footprint "x" (layer "F.Cu") (at 0 0) (property "Reference" "R1" (at 0 -1 0) (layer "F.SilkS") (effects (font (size 1 1)))))"#);
        let e = parse_footprint_edit(&node, "R1#2", None, Side::Top, 0);
        assert_eq!(e.reference.unwrap().text.as_deref(), Some("R1"));
    }

    #[test]
    fn the_old_fp_text_form_gives_the_reference_and_value() {
        let node = parse(
            r#"(footprint "x" (layer "F.Cu") (at 0 0)
                (fp_text reference "C3" (at 0 -2 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
                (fp_text value "1u" (at 0 2 0) (layer "F.Fab") hide (effects (font (size 1 1) (thickness 0.15)))))"#,
        );
        let e = parse_footprint_edit(&node, "C3", Some("1u"), Side::Top, 0);
        assert_eq!(e.reference.as_ref().unwrap().at, Point { x: 0, y: -2_000_000 });
        assert!(!e.value.as_ref().unwrap().visible, "the bare `hide` of the old files");
        assert!(e.attrs.is_none(), "no (attr ..) is no attributes edit");
    }

    #[test]
    fn attributes_are_written_in_kicads_order_and_not_at_all_when_empty() {
        let mut out = String::new();
        write_attr(&mut out, &FootprintAttrs::default());
        assert!(out.is_empty());
        write_attr(&mut out, &FootprintAttrs { kind: FootprintKind::ThroughHole, board_only: true, exclude_from_pos_files: true, exclude_from_bom: true, dnp: true, allow_missing_courtyard: true });
        assert_eq!(out.trim(), "(attr through_hole board_only exclude_from_pos_files exclude_from_bom allow_missing_courtyard dnp)");
        let back = parse_attrs(&parse(out.trim()));
        assert_eq!(back, FootprintAttrs { kind: FootprintKind::ThroughHole, board_only: true, exclude_from_pos_files: true, exclude_from_bom: true, dnp: true, allow_missing_courtyard: true });
        assert_eq!(parse_attrs(&parse("(attr virtual)")), FootprintAttrs { exclude_from_pos_files: true, exclude_from_bom: true, ..Default::default() });
    }

    #[test]
    fn a_pads_margins_are_written_and_read_back_and_its_offset_is_read_on_either_side() {
        let mut e = PadEdit::none("1", 1);
        e.offset = Some(Point { x: 300_000, y: -100_000 });
        e.solder_mask_margin = Some(50_000);
        e.solder_paste_margin = Some(-30_000);
        e.solder_paste_margin_ratio = Some(-0.1);
        let t = pad_text(Some(&e));
        assert_eq!(t.margins, " (solder_mask_margin 0.05) (solder_paste_margin -0.03) (solder_paste_margin_ratio -0.1)");
        for side in [Side::Top, Side::Bottom] {
            // The offset in the file's frame: x negated on the bottom side, where the whole frame is mirrored.
            let ox = if side == Side::Bottom { -300_000 } else { 300_000 };
            let node = parse(&format!("(pad \"1\" smd rect (at 0 0) (size 1 1) (drill (offset {} {})){})", mm(ox), mm(-100_000), t.margins));
            let back = parse_pad_extras(&node, side, 20241229);
            assert_eq!((back.offset, back.solder_mask_margin, back.solder_paste_margin, back.solder_paste_margin_ratio), (e.offset, e.solder_mask_margin, e.solder_paste_margin, e.solder_paste_margin_ratio), "{side:?}");
        }
        assert!(pad_text(None).margins.is_empty());
    }

    #[test]
    fn an_old_file_reads_a_zero_margin_as_unset() {
        let node = parse("(pad \"1\" smd rect (at 0 0) (size 1 1) (solder_mask_margin 0))");
        let old = parse_pad_extras(&node, Side::Top, 20231231);
        assert_eq!(old.solder_mask_margin, None);
        let new = parse_pad_extras(&node, Side::Top, 20241229);
        assert_eq!(new.solder_mask_margin, Some(0));
    }
}
