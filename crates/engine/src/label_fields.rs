//! A label's own fields on the sheet (`SCH_LABEL_BASE::m_fields`): a global label's Sheet References (`Intersheetrefs`), a label's net class (`Netclass`) and
//! any other field the user added. Ported from eeschema at 8303b2ad:
//!
//! * `SCH_LABEL_BASE::AutoplaceFields`: where the fields go while the label's fields are autoplaced (`m_fieldsAutoplaced` is `AUTOPLACE_AUTO` or
//!   `AUTOPLACE_MANUAL`): one under the other, under the label's text (beside it for a vertical label), and a global label's Sheet References at the far end
//!   of its flag. A label placed, turned, edited in Label Properties or given Autoplace Fields has them placed so again.
//! * `SCH_MOVE_TOOL::moveItem` (`parent->SetFieldsAutoplaced( AUTOPLACE_NONE )`), `DIALOG_FIELD_PROPERTIES::UpdateField` and `DIALOG_LABEL_PROPERTIES::
//!   TransferDataFromWindow` (`positioningChanged`): once one field was placed by hand, every field of the label keeps its own place, which turns and mirrors
//!   with the label (`SCH_LABEL_BASE::Rotate90`: `field.Rotate( GetPosition(), !aClockwise )`; `MirrorSpinStyle`, `MirrorHorizontally`, `MirrorVertically`)
//!   and is not placed by `AutoplaceFields` again until Autoplace Fields asks for it.
//!
//! The places of a label whose fields were placed by hand are `SchematicSection::field_layout`'s entry under the label's id: one [`FieldPlacement`] per field
//! (by its name), its `dx` / `dy` the offset of the field's anchor from the label's on the sheet, its `angle` the text's (0 or 90°), `h` and `v` its
//! justification. A label with no entry has its fields autoplaced (the file's `(fields_autoplaced yes)`). A field the entry has no place for (one added later
//! in Label Properties) is at the label's anchor with the angle and justification of the field before it, as KiCad's dialog adds it ([`placed`]).

use eda_model::ir::{FieldPlacement, LabelKind, NetLabel, Point, SchematicSection, TextJustify, TextVAlign};
use eda_model::kicad_font::{HJustify, VJustify};
use eda_model::kicad_geom::{self, Spin, TextStyle};
use eda_model::sch_extras::{LabelField, LabelSpin, INTERSHEET_REFS_FIELD};
use eda_model::units::Nm;

/// The height of a label's field text: `SCH_FIELD`'s default, 50 mil.
const FIELD_SIZE_NM: f64 = 1_270_000.0;

/// One field of a label where it is on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLabelField {
    pub name: String,
    pub text: String,
    pub visible: bool,
    pub italic: bool,
    /// Its text size (0 is the default, 50 mil), bold and shown name (`LabelField`'s own).
    pub size_nm: Nm,
    pub bold: bool,
    pub name_shown: bool,
    /// `!SCH_FIELD::CanAutoplace` (kept only: a label's `AutoplaceFields` places every field).
    pub no_autoplace: bool,
    /// The global label's Sheet References.
    pub intersheet_refs: bool,
    pub at: Point,
    pub vertical: bool,
    pub h: HJustify,
    pub v: VJustify,
}

/// `SCH_LABEL_BASE::GetSpinStyle`: the label's own spin once Rotate or Mirror set one, else the one its wire gives it.
pub fn label_spin(sch: &SchematicSection, l: &NetLabel) -> LabelSpin {
    if let Some(spin) = sch.extras.label_spins.get(&l.id) {
        return *spin;
    }
    match crate::obstacles::label_run_dir(sch, l) {
        // the wire is above the label: the text hangs below it, reading up and ending at the anchor
        (0, 1) => LabelSpin::Bottom,
        // the wire is below: the text rises from the anchor
        (0, -1) => LabelSpin::Up,
        // the wire is on the right: the text runs left
        (-1, 0) => LabelSpin::Left,
        _ => LabelSpin::Right,
    }
}

fn spin_of(s: LabelSpin) -> Spin {
    match s {
        LabelSpin::Right => Spin::Right,
        LabelSpin::Up => Spin::Up,
        LabelSpin::Left => Spin::Left,
        LabelSpin::Bottom => Spin::Bottom,
    }
}

/// The fields of label `l` in their order, each with whether it is the global label's Sheet References: the Sheet References first (shown when
/// `show_intersheet_refs`), then the label's own fields.
fn fields_of(sch: &SchematicSection, l: &NetLabel, show_intersheet_refs: bool) -> Vec<(LabelField, bool)> {
    let global = matches!(l.kind, LabelKind::Global { .. });
    let own = sch.extras.label_fields.get(&l.id).map(Vec::as_slice).unwrap_or(&[]);
    let mut fields: Vec<(LabelField, bool)> = Vec::new();
    if global {
        // its own looks (`SchExtras::intersheet_refs_looks`: size, bold, italic, shown name), its text and visibility Schematic Setup's
        let look = sch.extras.intersheet_refs_looks.get(&l.id).cloned().unwrap_or_default();
        fields.push((LabelField { name: INTERSHEET_REFS_FIELD.into(), text: "${INTERSHEET_REFS}".into(), visible: show_intersheet_refs, ..look }, true));
    }
    fields.extend(own.iter().cloned().map(|f| (f, false)));
    fields
}

/// Does label `l` have fields (a global label always has its Sheet References)?
pub fn has_fields(sch: &SchematicSection, l: &NetLabel) -> bool {
    matches!(l.kind, LabelKind::Global { .. }) || sch.extras.label_fields.get(&l.id).is_some_and(|f| !f.is_empty())
}

/// Were the fields of the label `id` placed by hand (`GetFieldsAutoplaced() == AUTOPLACE_NONE`)?
pub fn hand_placed(sch: &SchematicSection, id: &str) -> bool {
    sch.field_layout.contains_key(id) && sch.labels.iter().any(|l| l.id == id)
}

/// `SCH_LABEL_BASE::AutoplaceFields`: the fields of `l` where it puts them.
pub fn autoplaced(sch: &SchematicSection, l: &NetLabel, show_intersheet_refs: bool) -> Vec<PlacedLabelField> {
    let fields = fields_of(sch, l, show_intersheet_refs);
    if fields.is_empty() {
        return Vec::new();
    }
    let global = matches!(l.kind, LabelKind::Global { .. });
    let look = sch.extras.label_looks.get(&l.id).copied().unwrap_or_default();
    let size_iu = look.text_size_nm() / Nm::new(eda_model::units::SCH_IU_NM);
    let fx = TextStyle { size_iu, thickness_iu: if look.bold { eda_model::kicad_font::kiround(size_iu as f64 / 5.0) } else { 0 }, h: HJustify::Left, v: VJustify::Center };
    let spin = spin_of(label_spin(sch, l));
    let at = (l.at.x.as_f64(), l.at.y.as_f64());
    let body = match &l.kind {
        LabelKind::Global { shape } => kicad_geom::global_label_rect(&fx, &l.net, at, spin, *shape),
        LabelKind::Hierarchical { .. } => kicad_geom::hier_label_rect(&fx, &l.net, at, spin),
        LabelKind::Local => {
            let turns = if matches!(spin, Spin::Up | Spin::Bottom) { 1 } else { 0 };
            kicad_geom::local_label_rect(&fx, &l.net, at, turns)
        }
    };
    let body_len = body.w().max(body.h());
    // each field's own height (`field.GetTextHeight()`) stacks it under the one before
    let sizes: Vec<(bool, f64)> = fields.iter().map(|(f, isr)| (*isr, if f.size_nm > Nm::ZERO { f.size_nm.as_f64() } else { FIELD_SIZE_NM })).collect();
    let places = kicad_geom::autoplace_label_fields(&fx, global, body_len, spin, at, &sizes);
    fields
        .into_iter()
        .zip(places)
        .map(|((f, isr), p)| PlacedLabelField {
            name: f.name,
            text: f.text,
            visible: f.visible,
            italic: f.italic,
            size_nm: f.size_nm,
            bold: f.bold,
            name_shown: f.name_shown,
            no_autoplace: f.no_autoplace,
            intersheet_refs: isr,
            at: Point { x: Nm::kiround(p.at.0), y: Nm::kiround(p.at.1) },
            vertical: p.vertical,
            h: p.h,
            v: VJustify::Center,
        })
        .collect()
}

/// The fields of `l` where they are: the places the user gave them (`field_layout`'s entry under the label's id), else where `AutoplaceFields` puts them.
///
/// A field the entry has no place for is one added since the fields were placed by hand (Label Properties' Add Field): KiCad's new field (`DIALOG_LABEL_PROPERTIES::
/// OnAddField`: `SCH_FIELD newField( m_currentLabel, .. )`, `newField.SetAttributes( m_fields->at( m_fields->size() - 1 ) )`) has the angle and justification
/// of the field before it -- the default centre and centre when there is none -- and the grid's place (0, 0), the label's anchor once `TransferDataFromWindow`
/// offsets it (`field.Offset( m_currentLabel->GetPosition() )`); `positioningChanged` compares the fields the label had only, so the label stays placed by hand
/// and nothing moves the new field (UI-LEFT-3 S3-17).
pub fn placed(sch: &SchematicSection, l: &NetLabel, show_intersheet_refs: bool) -> Vec<PlacedLabelField> {
    let auto = autoplaced(sch, l, show_intersheet_refs);
    let Some(stored) = sch.field_layout.get(&l.id) else { return auto };
    let mut before: Option<(bool, HJustify, VJustify)> = None;
    auto.into_iter()
        .map(|mut f| {
            if !stored.iter().any(|p| p.name == f.name) {
                let (vertical, h, v) = before.unwrap_or((false, HJustify::Center, VJustify::Center));
                (f.at, f.vertical, f.h, f.v) = (l.at, vertical, h, v);
            }
            if let Some(p) = stored.iter().find(|p| p.name == f.name) {
                f.at = Point { x: l.at.x + p.dx, y: l.at.y + p.dy };
                f.vertical = p.angle == 90_000;
                f.h = match p.h {
                    TextJustify::Left => HJustify::Left,
                    TextJustify::Center => HJustify::Center,
                    TextJustify::Right => HJustify::Right,
                };
                f.v = match p.v {
                    TextVAlign::Top => VJustify::Top,
                    TextVAlign::Center => VJustify::Center,
                    TextVAlign::Bottom => VJustify::Bottom,
                };
            }
            before = Some((f.vertical, f.h, f.v));
            f
        })
        .collect()
}

/// A placed field of label `l` as the place `field_layout` keeps for it: the offset from the label's anchor, the text's angle, its justification.
pub fn placement_of(l: &NetLabel, f: &PlacedLabelField) -> FieldPlacement {
    FieldPlacement {
        name: f.name.clone(),
        dx: f.at.x - l.at.x,
        dy: f.at.y - l.at.y,
        angle: if f.vertical { 90_000 } else { 0 },
        h: match f.h {
            HJustify::Left => TextJustify::Left,
            HJustify::Center => TextJustify::Center,
            HJustify::Right => TextJustify::Right,
        },
        v: match f.v {
            VJustify::Top => TextVAlign::Top,
            VJustify::Center => TextVAlign::Center,
            VJustify::Bottom => TextVAlign::Bottom,
        },
        visible: f.visible,
        size_nm: f.size_nm,
        bold: f.bold,
        italic: f.italic,
        name_shown: f.name_shown,
        no_autoplace: f.no_autoplace,
    }
}

/// `SCH_FIELD::Rotate( GetPosition(), aRotateCCW )` on each hand-placed field of a label turned a quarter (`SCH_LABEL_BASE::Rotate90( !aRotateCCW )` while
/// its fields are not autoplaced): the text's justification and angle as `SCH_FIELD::Rotate` turns them, and its offset from the label's anchor a quarter turn
/// round it (`RotatePoint( pt, aCenter, aRotateCCW ? ANGLE_90 : ANGLE_270 )`: counter-clockwise on the sheet, whose y runs down).
pub fn rotate_hand_placed(sch: &mut SchematicSection, id: &str, ccw: bool) {
    let Some(places) = sch.field_layout.get_mut(id) else { return };
    for p in places.iter_mut() {
        if p.angle == 90_000 {
            // vertical: "case GR_TEXT_H_ALIGN_LEFT: if( aRotateCCW ) SetHorizJustify( GR_TEXT_H_ALIGN_RIGHT )", and the other way
            if ccw {
                p.h = flip_h(p.h);
            }
            p.angle = 0;
        } else {
            if !ccw {
                p.h = flip_h(p.h);
            }
            p.angle = 90_000;
        }
        // `RotatePoint` by +90° (counter-clockwise as drawn): (x, y) -> (y, -x); by 270°: (x, y) -> (-y, x)
        let (dx, dy) = (p.dx, p.dy);
        (p.dx, p.dy) = if ccw { (dy, -dx) } else { (-dy, dx) };
    }
}

/// `SCH_LABEL_BASE::MirrorSpinStyle( aLeftRight )` on the hand-placed fields of label `id` (a label mirrored where it stands): a field that lies along the
/// mirror has its justification flipped ("if LEFT then RIGHT, else LEFT": a centred one goes left), and its anchor is mirrored about the label's.
pub fn mirror_spin_hand_placed(sch: &mut SchematicSection, id: &str, left_right: bool) {
    let Some(places) = sch.field_layout.get_mut(id) else { return };
    for p in places.iter_mut() {
        let horizontal = p.angle != 90_000;
        if (left_right && horizontal) || (!left_right && !horizontal) {
            p.h = if p.h == TextJustify::Left { TextJustify::Right } else { TextJustify::Left };
        }
        if left_right {
            p.dx = -p.dx;
        } else {
            p.dy = -p.dy;
        }
    }
}

/// `SCH_LABEL_BASE::MirrorHorizontally` (`vertical` false) / `MirrorVertically` (true) on the hand-placed fields of label `id` (a label mirrored with other
/// items): a field that lies along the mirror flips its justification (`FlipHJustify`: a centred one stays), and its anchor goes to the other side of the
/// label's.
pub fn mirror_hand_placed(sch: &mut SchematicSection, id: &str, vertical: bool) {
    let Some(places) = sch.field_layout.get_mut(id) else { return };
    for p in places.iter_mut() {
        let along = if vertical { p.angle == 90_000 } else { p.angle != 90_000 };
        if along {
            p.h = flip_h(p.h);
        }
        if vertical {
            p.dy = -p.dy;
        } else {
            p.dx = -p.dx;
        }
    }
}

fn flip_h(h: TextJustify) -> TextJustify {
    match h {
        TextJustify::Left => TextJustify::Right,
        TextJustify::Right => TextJustify::Left,
        TextJustify::Center => TextJustify::Center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::LabelShape;

    fn label(id: &str, kind: LabelKind) -> NetLabel {
        NetLabel { id: id.into(), net: "SIG".into(), at: Point { x: Nm::new(10_000_000), y: Nm::new(20_000_000) }, kind }
    }

    fn section_with(l: NetLabel) -> SchematicSection {
        let mut sch = SchematicSection { labels: vec![l.clone()], ..Default::default() };
        sch.extras.label_fields.insert(l.id.clone(), vec![LabelField { name: "Netclass".into(), text: "HV".into(), visible: true, italic: true, ..Default::default() }]);
        sch.extras.label_spins.insert(l.id, LabelSpin::Right);
        sch
    }

    #[test]
    fn a_label_with_no_places_has_its_fields_autoplaced_and_one_with_places_keeps_them() {
        let l = label("lbl_a", LabelKind::Global { shape: LabelShape::Input });
        let mut sch = section_with(l.clone());
        let auto = placed(&sch, &l, false);
        assert_eq!(auto, autoplaced(&sch, &l, false));
        assert_eq!(auto.len(), 2, "the Sheet References first, then the net class");
        assert!(auto[0].intersheet_refs && auto[1].name == "Netclass");
        assert!(!hand_placed(&sch, "lbl_a"));
        // the net class moved by hand: the label keeps every field's place, the moved one's where it was put
        let mut places: Vec<FieldPlacement> = auto.iter().map(|f| placement_of(&l, f)).collect();
        places[1].dx += Nm::new(2_540_000);
        sch.field_layout.insert("lbl_a".into(), places);
        assert!(hand_placed(&sch, "lbl_a"));
        let now = placed(&sch, &l, false);
        assert_eq!(now[0].at, auto[0].at);
        assert_eq!(now[1].at, Point { x: auto[1].at.x + Nm::new(2_540_000), y: auto[1].at.y });
        // a label moved takes its hand-placed fields with it: the places are offsets from its anchor
        let mut moved = l.clone();
        moved.at.x += Nm::new(1_000_000);
        sch.labels[0] = moved.clone();
        assert_eq!(placed(&sch, &moved, false)[1].at.x, now[1].at.x + Nm::new(1_000_000));
    }

    /// UI-LEFT-3 S3-17 (`DIALOG_LABEL_PROPERTIES::OnAddField`, `TransferDataFromWindow`): a field added to a label whose fields were placed by hand is at the
    /// label's anchor with the angle and justification of the field before it, the others staying where they were; with no field before it, centred.
    #[test]
    fn a_field_added_to_a_hand_placed_label_is_at_its_anchor_like_the_field_before() {
        let l = label("lbl_a", LabelKind::Global { shape: LabelShape::Input });
        let mut sch = section_with(l.clone());
        let auto = placed(&sch, &l, false);
        let mut places: Vec<FieldPlacement> = auto.iter().map(|f| placement_of(&l, f)).collect();
        places[1].dx += Nm::new(2_540_000);
        places[1].angle = 90_000;
        places[1].h = TextJustify::Right;
        places[1].v = TextVAlign::Top;
        sch.field_layout.insert("lbl_a".into(), places);
        sch.extras.label_fields.get_mut("lbl_a").unwrap().push(LabelField { name: "Note".into(), text: "x".into(), visible: true, ..Default::default() });
        let now = placed(&sch, &l, false);
        assert_eq!(now.len(), 3);
        assert_eq!(now[1].at, Point { x: auto[1].at.x + Nm::new(2_540_000), y: auto[1].at.y }, "the hand-placed field stays");
        assert_eq!((now[2].at, now[2].vertical, now[2].h, now[2].v), (l.at, true, HJustify::Right, VJustify::Top), "the new one at the anchor, as the one before");
        // a local label's only field, added after the label was placed by hand with no field: centred at the anchor
        let m = label("lbl_m", LabelKind::Local);
        let mut sch = section_with(m.clone());
        sch.extras.label_fields.remove("lbl_m");
        sch.field_layout.insert("lbl_m".into(), Vec::new());
        sch.extras.label_fields.insert("lbl_m".into(), vec![LabelField { name: "Note".into(), text: "x".into(), visible: true, ..Default::default() }]);
        let f = &placed(&sch, &m, false)[0];
        assert_eq!((f.at, f.vertical, f.h, f.v), (m.at, false, HJustify::Center, VJustify::Center));
    }

    #[test]
    fn hand_placed_fields_turn_and_mirror_about_the_labels_anchor_as_sch_field_does() {
        let l = label("lbl_b", LabelKind::Local);
        let mut sch = section_with(l.clone());
        let p = FieldPlacement { dx: Nm::new(1_000), dy: Nm::new(2_000), h: TextJustify::Left, ..FieldPlacement::at_origin("Netclass") };
        sch.field_layout.insert("lbl_b".into(), vec![p]);
        // `RotatePoint` by 90 (ccw): (1, 2) -> (2, -1); the horizontal text becomes vertical, its justification kept (ccw)
        rotate_hand_placed(&mut sch, "lbl_b", true);
        let q = &sch.field_layout["lbl_b"][0];
        assert_eq!((q.dx, q.dy, q.angle, q.h), (Nm::new(2_000), Nm::new(-1_000), 90_000, TextJustify::Left));
        // and back clockwise: vertical -> horizontal, the justification kept (`if( aRotateCCW )` only)
        rotate_hand_placed(&mut sch, "lbl_b", false);
        let q = &sch.field_layout["lbl_b"][0];
        assert_eq!((q.dx, q.dy, q.angle, q.h), (Nm::new(1_000), Nm::new(2_000), 0, TextJustify::Left));
        // `MirrorSpinStyle( true )`: a horizontal field flips left/right, its x about the anchor
        mirror_spin_hand_placed(&mut sch, "lbl_b", true);
        let q = &sch.field_layout["lbl_b"][0];
        assert_eq!((q.dx, q.dy, q.h), (Nm::new(-1_000), Nm::new(2_000), TextJustify::Right));
        // `MirrorVertically`: a horizontal field keeps its justification, its y flips
        mirror_hand_placed(&mut sch, "lbl_b", true);
        let q = &sch.field_layout["lbl_b"][0];
        assert_eq!((q.dx, q.dy, q.h), (Nm::new(-1_000), Nm::new(-2_000), TextJustify::Right));
    }
}
