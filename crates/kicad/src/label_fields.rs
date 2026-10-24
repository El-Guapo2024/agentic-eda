//! A label's own fields in the `.kicad_sch` (`SCH_LABEL_BASE::m_fields`): a global label's Sheet References (`Intersheetrefs`, text `${INTERSHEET_REFS}`), a
//! label's net class (`Netclass`) and any other field the user added, each a `(property ..)` of the label.
//!
//! Ported from `SCH_IO_KICAD_SEXPR::saveText` / `saveField` (the writer), `SCH_IO_KICAD_SEXPR_PARSER::parseSchText` / `parseSchField` (the reader),
//! `SCH_GLOBALLABEL::SCH_GLOBALLABEL` (the Sheet References field every global label has, first) and `SCH_LABEL_BASE::AutoplaceFields` (where the fields go).
//!
//! The fields are placed by `AutoplaceFields`, as KiCad places a label's fields while they are autoplaced; a field moved by hand in KiCad is read back to
//! that place. The Sheet References are shown or hidden by Schematic Setup (`m_IntersheetRefsShow`, `RecomputeIntersheetRefs`), whatever the file says.

use crate::sexpr::{self, Sexpr};
use eda_model::ir::{LabelKind, NetLabel, Point, SchematicSection};
use eda_model::kicad_font::HJustify;
use eda_model::kicad_geom::{self, Spin, TextStyle};
use eda_model::sch_extras::{LabelField, LabelSpin, INTERSHEET_REFS_FIELD};
use std::fmt::Write as _;

/// The height of a label's field text: `SCH_FIELD`'s default, 50 mil.
const FIELD_SIZE_NM: f64 = 1_270_000.0;

/// One field of a label, where `AutoplaceFields` puts it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLabelField {
    pub name: String,
    pub text: String,
    pub visible: bool,
    pub italic: bool,
    /// The global label's Sheet References.
    pub intersheet_refs: bool,
    pub at: Point,
    pub vertical: bool,
    pub h: HJustify,
}

fn spin_of(s: LabelSpin) -> Spin {
    match s {
        LabelSpin::Right => Spin::Right,
        LabelSpin::Up => Spin::Up,
        LabelSpin::Left => Spin::Left,
        LabelSpin::Bottom => Spin::Bottom,
    }
}

/// The fields of label `l` of `sch`, placed: a global label's Sheet References first (shown when `show_intersheet_refs`), then the label's own fields in order.
pub fn label_fields_placed(sch: &SchematicSection, l: &NetLabel, show_intersheet_refs: bool) -> Vec<PlacedLabelField> {
    let global = matches!(l.kind, LabelKind::Global { .. });
    let own = sch.extras.label_fields.get(&l.id).map(Vec::as_slice).unwrap_or(&[]);
    if own.is_empty() && !global {
        return Vec::new();
    }
    let mut fields: Vec<(LabelField, bool)> = Vec::new();
    if global {
        fields.push((LabelField { name: INTERSHEET_REFS_FIELD.into(), text: "${INTERSHEET_REFS}".into(), visible: show_intersheet_refs, italic: false }, true));
    }
    fields.extend(own.iter().cloned().map(|f| (f, false)));
    let look = sch.extras.label_looks.get(&l.id).copied().unwrap_or_default();
    let size_iu = look.text_size_nm() / eda_model::units::SCH_IU_NM;
    let fx = TextStyle { size_iu, thickness_iu: if look.bold { eda_model::kicad_font::kiround(size_iu as f64 / 5.0) } else { 0 }, h: HJustify::Left, v: eda_model::kicad_font::VJustify::Center };
    let spin = spin_of(crate::label_spin_of(sch, l));
    let at = (l.at.x as f64, l.at.y as f64);
    let body = match &l.kind {
        LabelKind::Global { shape } => kicad_geom::global_label_rect(&fx, &l.net, at, spin, *shape),
        LabelKind::Hierarchical { .. } => kicad_geom::hier_label_rect(&fx, &l.net, at, spin),
        LabelKind::Local => {
            let turns = if matches!(spin, Spin::Up | Spin::Bottom) { 1 } else { 0 };
            kicad_geom::local_label_rect(&fx, &l.net, at, turns)
        }
    };
    let body_len = body.w().max(body.h());
    let sizes: Vec<(bool, f64)> = fields.iter().map(|(_, isr)| (*isr, FIELD_SIZE_NM)).collect();
    let places = kicad_geom::autoplace_label_fields(&fx, global, body_len, spin, at, &sizes);
    fields
        .into_iter()
        .zip(places)
        .map(|((f, isr), p)| PlacedLabelField { name: f.name, text: f.text, visible: f.visible, italic: f.italic, intersheet_refs: isr, at: Point { x: p.at.0.round() as i64, y: p.at.1.round() as i64 }, vertical: p.vertical, h: p.h })
        .collect()
}

/// `saveField` for each field of the label: `(property "name" "text" (at x y angle) (effects (font (size 1.27 1.27) [(italic yes)]) (justify left|right) [(hide yes)]))`.
pub(crate) fn write_label_fields(out: &mut String, sch: &SchematicSection, l: &NetLabel, show_intersheet_refs: bool) {
    for f in label_fields_placed(sch, l, show_intersheet_refs) {
        let italic = if f.italic { " (italic yes)" } else { "" };
        let justify = match f.h {
            HJustify::Left => " (justify left)",
            HJustify::Right => " (justify right)",
            HJustify::Center => "",
        };
        let hide = if f.visible { "" } else { " (hide yes)" };
        let _ = writeln!(
            out,
            "\t\t(property {} {} (at {} {} {}) (effects (font (size 1.27 1.27){italic}){justify}{hide}))",
            crate::sexpr_str(&f.name),
            crate::sexpr_str(&f.text),
            crate::mm(f.at.x),
            crate::mm(f.at.y),
            if f.vertical { 90 } else { 0 }
        );
    }
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
