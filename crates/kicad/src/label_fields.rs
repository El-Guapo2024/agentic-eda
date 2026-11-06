//! A label's own fields in the `.kicad_sch` (`SCH_LABEL_BASE::m_fields`): a global label's Sheet References (`Intersheetrefs`, text `${INTERSHEET_REFS}`), a
//! label's net class (`Netclass`) and any other field the user added, each a `(property ..)` of the label.
//!
//! Ported from `SCH_IO_KICAD_SEXPR::saveText` / `saveField` (the writer), `SCH_IO_KICAD_SEXPR_PARSER::parseSchText` / `parseSchField` (the reader),
//! `SCH_GLOBALLABEL::SCH_GLOBALLABEL` (the Sheet References field every global label has, first) and `SCH_LABEL_BASE::AutoplaceFields` (where the fields go).
//!
//! The fields are where `eda_engine::label_fields` has them: placed by `AutoplaceFields` while the label's fields are autoplaced (`(fields_autoplaced yes)`),
//! else at the places the user gave them (`SchematicSection::field_layout` under the label's id), which a file without `(fields_autoplaced ..)` gives too
//! (`parseSchText`: `SetFieldsAutoplaced( AUTOPLACE_NONE )` until the token is read). The Sheet References are shown or hidden by Schematic Setup
//! (`m_IntersheetRefsShow`, `RecomputeIntersheetRefs`), whatever the file says.

use crate::sexpr::{self, Sexpr};
use eda_model::ir::{FieldPlacement, NetLabel, Point, SchematicSection, TextJustify, TextVAlign};
use eda_model::kicad_font::{HJustify, VJustify};
use eda_model::sch_extras::{LabelField, INTERSHEET_REFS_FIELD};
use std::fmt::Write as _;

pub use eda_engine::label_fields::PlacedLabelField;

/// The fields of label `l` of `sch`, placed: a global label's Sheet References first (shown when `show_intersheet_refs`), then the label's own fields in order.
pub fn label_fields_placed(sch: &SchematicSection, l: &NetLabel, show_intersheet_refs: bool) -> Vec<PlacedLabelField> {
    eda_engine::label_fields::placed(sch, l, show_intersheet_refs)
}

/// `saveField` for each field of the label: `(property "name" "text" (at x y angle) (effects (font [(face ..)] (size 1.27 1.27) [(italic yes)] [(color ..)])
/// (justify left|right [top|bottom]) [(hide yes)]))`, with the font face and colour Field Properties set (`SchExtras::field_looks` under the label's id).
pub(crate) fn write_label_fields(out: &mut String, sch: &SchematicSection, l: &NetLabel, show_intersheet_refs: bool) {
    for f in label_fields_placed(sch, l, show_intersheet_refs) {
        let look = sch.extras.field_look(&l.id, &f.name);
        let face = look.and_then(|k| k.font.as_deref()).map(|n| format!("(face {}) ", crate::sexpr_str(n))).unwrap_or_default();
        let color = look.and_then(|k| k.color).map(|c| format!(" (color {} {} {} {})", c.r, c.g, c.b, crate::sch_extras_io::alpha_str(c.a))).unwrap_or_default();
        let italic = if f.italic { " (italic yes)" } else { "" };
        let mut j = String::new();
        match f.h {
            HJustify::Left => j.push_str(" left"),
            HJustify::Right => j.push_str(" right"),
            HJustify::Center => {}
        }
        match f.v {
            VJustify::Top => j.push_str(" top"),
            VJustify::Bottom => j.push_str(" bottom"),
            VJustify::Center => {}
        }
        let justify = if j.is_empty() { String::new() } else { format!(" (justify{j})") };
        let hide = if f.visible { "" } else { " (hide yes)" };
        let _ = writeln!(
            out,
            "\t\t(property {} {} (at {} {} {}) (effects (font {face}(size 1.27 1.27){italic}{color}){justify}{hide}))",
            crate::sexpr_str(&f.name),
            crate::sexpr_str(&f.text),
            crate::mm(f.at.x),
            crate::mm(f.at.y),
            if f.vertical { 90 } else { 0 }
        );
    }
}

/// The places of a label's fields as the file has them, when they are not autoplaced: `parseSchText` sets `AUTOPLACE_NONE` until it reads
/// `(fields_autoplaced [yes])`, and a label with no field is autoplaced. Each `(property ..)`'s anchor as an offset from the label's `at`, its angle and its
/// `(justify ..)` (`parseEDA_TEXT`: centre and centre when there is none), by the field's canonical name (`Intersheetrefs` for a global label's Sheet
/// References, `Netclass` for "Net Class"). `None` when the fields are autoplaced.
pub(crate) fn read_label_field_places(item: &[Sexpr], global: bool, label_at: Point) -> Option<Vec<FieldPlacement>> {
    if crate::sch_extras_io::parse_bool(item, "fields_autoplaced").unwrap_or(false) {
        return None;
    }
    let mut out = Vec::new();
    for p in sexpr::find_all(item, "property") {
        let Some(name) = sexpr::txt(p, 1) else { continue };
        if name.is_empty() {
            continue;
        }
        let name = if global && (name.eq_ignore_ascii_case(INTERSHEET_REFS_FIELD) || name.eq_ignore_ascii_case("Intersheet References")) {
            INTERSHEET_REFS_FIELD
        } else if name == "Net Class" {
            eda_model::sch_extras::NETCLASS_FIELD
        } else {
            name
        };
        let Some(at) = sexpr::find(p, "at") else { continue };
        let (Some(x), Some(y)) = (sexpr::num(at, 1), sexpr::num(at, 2)) else { continue };
        let angle = sexpr::num(at, 3).unwrap_or(0.0);
        let look = crate::sch_extras_io::read_effects(p);
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
        let (x, y) = (crate::import::sch_mm_to_nm(x), crate::import::sch_mm_to_nm(y));
        out.push(FieldPlacement {
            dx: x - label_at.x,
            dy: y - label_at.y,
            angle: if eda_model::kicad_geom::keep_upright_is_vertical(angle) { 90_000 } else { 0 },
            h,
            v,
            ..FieldPlacement::at_origin(name)
        });
    }
    (!out.is_empty()).then_some(out)
}

/// `parseSchText`'s `(property ..)`s of a label: its own fields, in order (`parseSchField`). A global label's Sheet References is not one of them: it is the label's
/// mandatory field, shown as Schematic Setup says -- `parseSchField` knows it by its canonical name without regard to case, or by the name KiCad 6 wrote
/// ("Intersheet References"), and only on a global label (on another label a field of that name is a field like any other). A field's place is not kept (see
/// the module doc).
pub(crate) fn read_label_fields(item: &[Sexpr], global: bool) -> Vec<LabelField> {
    let mut out = Vec::new();
    for p in sexpr::find_all(item, "property") {
        let Some(name) = sexpr::txt(p, 1) else { continue };
        if name.is_empty() || (global && (name.eq_ignore_ascii_case(INTERSHEET_REFS_FIELD) || name.eq_ignore_ascii_case("Intersheet References"))) {
            continue;
        }
        // `GetCanonicalName`: "Net Class" is the net class field
        let name = if name == "Net Class" { eda_model::sch_extras::NETCLASS_FIELD } else { name };
        let text = sexpr::txt(p, 2).unwrap_or("").to_string();
        let effects = sexpr::find(p, "effects");
        let hidden = crate::sch_extras_io::parse_bool(p, "hide").unwrap_or(false) || effects.is_some_and(|e| crate::sch_extras_io::parse_bool(e, "hide").unwrap_or(false) || e.iter().skip(1).any(|c| c.text() == Some("hide")));
        let look = crate::sch_extras_io::read_effects(p);
        out.push(LabelField { name: name.to_string(), text, visible: !hidden, italic: look.italic });
    }
    out
}
