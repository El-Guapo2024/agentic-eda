//! The fields of a symbol, a power symbol and a sheet as items of their own: moved, turned, mirrored and edited one by one, and placed again by
//! Autoplace Fields. Ported from eeschema at 8303b2ad:
//!
//! * `SCH_MOVE_TOOL::moveItem` (a field moves by the offset taken back through its symbol's transform, and a field moved on its own takes its item out of
//!   the autoplaced ones, `SetFieldsAutoplaced( AUTOPLACE_NONE )`);
//! * `SCH_EDIT_TOOL::Rotate` (a lone field turns its text a quarter; with other items it turns about the selection's centre, `SCH_FIELD::Rotate`) and
//!   `Mirror` (a field flips its justification);
//! * `DIALOG_FIELD_PROPERTIES::UpdateField` and `SCH_EDIT_TOOL::editFieldText` ([`SchCmd::EditField`](crate::sch_edit::SchCmd::EditField));
//! * `SCH_EDIT_TOOL::AutoplaceFields` ([`SchCmd::AutoplaceFields`](crate::sch_edit::SchCmd::AutoplaceFields)) and the automatic one that follows a turn of the
//!   symbol (`SCH_EDIT_TOOL::Rotate`: `if( m_AutoplaceFields.enable ) ... AutoplaceFields( screen, fieldsAutoplaced )`).
//!
//! A field is named by the id `fld:<owner key>:<name>` (`eda_engine::fields_edit::field_id`), so the studio selects it like any other item and every
//! move verb takes it in `ids`. What a field is on the sheet is `eda_engine::fields_edit::OwnerCtx`'s: a placement in the frame of its item, kept in
//! `SchematicSection::field_layout`.

use crate::sch_scene::{rotate_point, Scene};
use crate::Board;
use eda_engine::fields_edit::{parse_field_id, OwnerCtx, OwnerKind};
use eda_model::ir::{field_key, FieldPlacement, Point, SchematicSection, TextJustify, TextVAlign, Nm};
use eda_model::kicad_font::{HJustify, VJustify};
use eda_model::sch_extras::AutoplaceAlgo;
use eda_model::{CheckResult, ConstraintModel};
use std::collections::BTreeSet;

fn fail(check: &str, subject: &str, why: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, subject, why)]
}

/// A field picked on its own (`SCH_FIELD`): its item's key in `field_layout` and its name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FieldSel {
    pub key: String,
    pub name: String,
}

/// Is `id` the id of a field?
pub(crate) fn is_field_id(id: &str) -> bool {
    parse_field_id(id).is_some()
}

/// `GetFlippedAlignment` for a horizontal justification.
fn flip_h(h: TextJustify) -> TextJustify {
    match h {
        TextJustify::Left => TextJustify::Right,
        TextJustify::Right => TextJustify::Left,
        TextJustify::Center => TextJustify::Center,
    }
}

/// `GetFlippedAlignment` for a vertical justification.
fn flip_v(v: TextVAlign) -> TextVAlign {
    match v {
        TextVAlign::Top => TextVAlign::Bottom,
        TextVAlign::Bottom => TextVAlign::Top,
        TextVAlign::Center => TextVAlign::Center,
    }
}

impl<'m> Scene<'m> {
    /// The field `id` names, when its item is on the sheet and has it.
    pub(crate) fn find_field(&self, id: &str) -> Option<FieldSel> {
        let (key, name) = parse_field_id(id)?;
        let ctx = OwnerCtx::new(&self.sch, self.model, key)?;
        ctx.specs.iter().any(|s| s.name == name).then(|| FieldSel { key: key.to_string(), name: name.to_string() })
    }

    /// Does the user's selection hold the item that owns `key`? Such an item moves and turns its fields with it (`SCH_MOVE_TOOL::Main`: "Don't double
    /// move pins, fields, etc."; `SCH_EDIT_TOOL::Rotate`: "parent will rotate us").
    fn owner_selected(&self, key: &str) -> bool {
        use crate::sch_scene::Item;
        self.selected.iter().any(|it| match *it {
            Item::Symbol(i) => field_key(&self.sch.symbols[i].id, self.sch.symbols[i].unit) == key,
            Item::Power(i) => self.sch.power_symbols[i].id == key,
            Item::Sheet(i) => self.sch.sheets[i].id == key,
            Item::Label(i) => self.sch.labels[i].id == key,
            _ => false,
        })
    }

    /// `SCH_FIELD::IsLocked`: a field is locked when its item is.
    pub(crate) fn is_field_locked(&self, f: &FieldSel) -> bool {
        match OwnerCtx::new(&self.sch, self.model, &f.key) {
            Some(ctx) => self.sch.extras.is_locked(&owner_id(&ctx)),
            None => false,
        }
    }

    /// The picked fields whose item is not picked too.
    pub(crate) fn loose_fields(&self) -> Vec<FieldSel> {
        self.fields.iter().filter(|f| !self.owner_selected(&f.key)).cloned().collect()
    }

    /// The box a field's text takes on the sheet (`SCH_FIELD::GetBoundingBox`).
    pub(crate) fn field_box(&self, f: &FieldSel) -> Option<(Point, Point)> {
        let ctx = OwnerCtx::new(&self.sch, self.model, &f.key)?;
        let i = ctx.specs.iter().position(|s| s.name == f.name)?;
        let pf = ctx.page(&ctx.current(&self.sch)[i], &ctx.specs[i].text);
        let r = eda_engine::fields::field_rect(&pf);
        Some((Point { x: r.x0, y: r.y0 }, Point { x: r.x1, y: r.y1 }))
    }

    /// Run `edit` on the placement of `f` (every field of its item is stored: a field moved on its own is the first to leave the live placement), then
    /// take the item out of the autoplaced ones: the user has placed this field.
    fn edit_placement(&mut self, f: &FieldSel, edit: impl FnOnce(&OwnerCtx, &SchematicSection, &mut FieldPlacement)) -> bool {
        let Some(ctx) = OwnerCtx::new(&self.sch, self.model, &f.key) else { return false };
        let mut placements = ctx.current(&self.sch);
        let Some(i) = ctx.specs.iter().position(|s| s.name == f.name) else { return false };
        edit(&ctx, &self.sch, &mut placements[i]);
        self.sch.field_layout.insert(f.key.clone(), placements);
        self.sch.extras.fields_autoplaced.remove(&f.key);
        true
    }

    /// `SCH_MOVE_TOOL::moveItem` on a field: the offset, taken back through the transform of the symbol that has it.
    pub(crate) fn move_field(&mut self, f: &FieldSel, delta: Point) {
        self.edit_placement(f, |ctx, _, p| {
            let (dx, dy) = ctx.local_delta(delta.x, delta.y);
            p.dx += dx;
            p.dy += dy;
        });
    }

    /// `SCH_EDIT_TOOL::Rotate` on a lone field: its text turns a quarter, horizontal to vertical and back, where it stands.
    pub(crate) fn turn_field_text(&mut self, f: &FieldSel) {
        self.edit_placement(f, |_, _, p| p.angle = if p.angle == 90_000 { 0 } else { 90_000 });
    }

    /// `SCH_FIELD::Rotate( center, ccw )` for a field turned with other items: the text turns a quarter (and swaps the side it is justified to when
    /// that is what keeps it reading from its anchor), and its anchor goes round `center`.
    pub(crate) fn rotate_field_about(&mut self, f: &FieldSel, center: Point, ccw: bool) {
        self.edit_placement(f, |ctx, _, p| {
            let text = ctx.specs.iter().find(|s| s.name == p.name).map(|s| s.text.clone()).unwrap_or_default();
            let anchor = ctx.page(p, &text).at;
            let vertical = p.angle == 90_000;
            // `SCH_FIELD::Rotate`: the stored justification, before the symbol's transform
            if vertical {
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
            let at = Point { x: Nm::kiround(anchor.0), y: Nm::kiround(anchor.1) };
            let to = rotate_point(at, center, ccw);
            (p.dx, p.dy) = ctx.local_pos((to.x.as_f64(), to.y.as_f64()));
        });
    }

    /// `SCH_EDIT_TOOL::Mirror` on a field: its justification flips (the vertical one for a mirror top to bottom, the horizontal one for left to right).
    pub(crate) fn mirror_field(&mut self, f: &FieldSel, vertical: bool) {
        self.edit_placement(f, |_, _, p| {
            if vertical {
                p.v = flip_v(p.v);
            } else {
                p.h = flip_h(p.h);
            }
        });
    }

    /// The sheet with its wires as they are now in the scene (the scene keeps them split in segments, apart from the section): what the fields keep
    /// clear of.
    fn section_with_wires(&self) -> SchematicSection {
        let mut sch = self.sch.clone();
        sch.wires = self
            .segs
            .iter()
            .filter(|s| !s.dead)
            .map(|s| eda_model::ir::Wire { id: String::new(), net: s.net.clone(), pins: Vec::new(), pts: vec![s.a, s.b], bus: s.bus })
            .collect();
        sch
    }

    /// Place the fields of the item that owns `key` again, the way it was placed before, if it was by Autoplace Fields (what follows `SCH_SYMBOL::Rotate`
    /// and `SCH_SHEET::Rotate`: `AutoplaceFields( screen, m_fieldsAutoplaced )`).
    pub(crate) fn replace_fields_if_autoplaced(&mut self, key: &str) {
        let Some(&algo) = self.sch.extras.fields_autoplaced.get(key) else { return };
        let with_wires = self.section_with_wires();
        let Some(ctx) = OwnerCtx::new(&with_wires, self.model, key) else { return };
        let placed = ctx.autoplace(&with_wires, self.model, algo);
        self.sch.field_layout.insert(key.to_string(), placed);
    }
}

/// A power symbol's user-defined fields are shown and italic as `SchExtras::power_props` keeps them ([`eda_engine::fields::power_placements`]): what a stored
/// placement of one says is written there too, so Field Properties' Show and Italic, and Symbol Properties', are one state.
pub(crate) fn sync_power_user_fields(sch: &mut SchematicSection, key: &str) {
    let Some(stored) = sch.field_layout.get(key) else { return };
    let Some(props) = sch.extras.power_props.get_mut(key) else { return };
    for f in props.fields.iter_mut() {
        if let Some(p) = stored.iter().find(|p| p.name == f.name) {
            f.visible = p.visible;
            f.italic = p.italic;
        }
    }
}

/// A field of a symbol, power symbol or sheet, picked by its page-frame justification: what Field Properties asks and answers in.
pub(crate) fn hjustify(h: TextJustify) -> HJustify {
    match h {
        TextJustify::Left => HJustify::Left,
        TextJustify::Center => HJustify::Center,
        TextJustify::Right => HJustify::Right,
    }
}

pub(crate) fn vjustify(v: TextVAlign) -> VJustify {
    match v {
        TextVAlign::Top => VJustify::Top,
        TextVAlign::Center => VJustify::Center,
        TextVAlign::Bottom => VJustify::Bottom,
    }
}

/// What Field Properties changes of one field (`DIALOG_FIELD_PROPERTIES::UpdateField`); a member left out is unchanged.
#[derive(Debug, Clone, Default)]
pub(crate) struct FieldEdit {
    pub text: Option<String>,
    /// Where the text's anchor is on the sheet.
    pub at: Option<Point>,
    /// Whether the text runs up the sheet.
    pub vertical: Option<bool>,
    /// How the text is justified as it reads on the sheet (`GetEffectiveHorizJustify`).
    pub h: Option<TextJustify>,
    pub v: Option<TextVAlign>,
    pub size_nm: Option<Nm>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub visible: Option<bool>,
    pub name_shown: Option<bool>,
    pub allow_autoplace: Option<bool>,
    /// Symbol Properties' X Position and Y Position (`FIELDS_GRID_TABLE::SetValue( FDC_POSX / FDC_POSY )`: `SetTextPos` of the field offset from the symbol,
    /// `field.Offset( -m_symbol->GetPosition() )`): the anchor in the item's own unturned frame, from its origin.
    pub local_at: Option<Point>,
    /// Symbol Properties' Orientation (`FDC_ORIENTATION`: `SetTextAngle( ANGLE_VERTICAL )` or `ANGLE_HORIZONTAL`): the text's angle before the item turns it;
    /// the justification it keeps is its own (the H and V Align cells are the effective ones, `h` and `v`).
    pub local_vertical: Option<bool>,
    /// Symbol Properties' edits, not Field Properties': the item's fields stay autoplaced and are not placed again here (`DIALOG_SYMBOL_PROPERTIES::
    /// TransferDataFromWindow` copies each field and leaves `GetFieldsAutoplaced()`; `SCH_EDIT_TOOL::Properties` runs `AutoplaceFields( screen,
    /// fieldsAutoplaced )` after OK, [`SchCmd::AutoplaceFieldsKept`](crate::sch_edit::SchCmd::AutoplaceFieldsKept)).
    pub keep_autoplaced: bool,
}

/// `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM`: "Don't allow text to disappear; it can be difficult to correct if you can't select it".
const TEXT_SIZE_MIN_NM: Nm = Nm::new(10_000);
const TEXT_SIZE_MAX_NM: Nm = Nm::new(1_000_000_000);

impl<'a> Board<'a> {
    /// Field Properties: see [`SchCmd::EditField`](crate::sch_edit::SchCmd::EditField).
    pub(crate) fn edit_sch_field(&mut self, id: &str, edit: &FieldEdit) -> Result<(), Vec<CheckResult>> {
        let Some((key, name)) = parse_field_id(id) else {
            return Err(fail("ops_unknown_item", id, "not the id of a field"));
        };
        let (key, name) = (key.to_string(), name.to_string());
        if let Some(size) = edit.size_nm {
            if !(TEXT_SIZE_MIN_NM..=TEXT_SIZE_MAX_NM).contains(&size) {
                return Err(fail("ops_bad_text_size", id, "The text size must be between 0.01 and 1000 mm."));
            }
        }
        let model = self.model;
        let before = self.schematic()?.clone();
        let Some(ctx) = OwnerCtx::new(&before, model, &key) else {
            return Err(fail("ops_unknown_item", id, "no field with this id on the sheet (a schematic read from a KiCad file has no places for its fields yet)"));
        };
        let Some(index) = ctx.specs.iter().position(|s| s.name == name) else {
            return Err(fail("ops_unknown_item", id, format!("the item has no field '{name}'")));
        };
        if self.schematic()?.extras.is_locked(&owner_id(&ctx)) {
            return Err(fail("ops_locked", id, "the item is locked"));
        }
        if ctx.kind == OwnerKind::Label {
            return self.edit_label_field(&ctx, index, id, edit);
        }

        // the text: each kind of field has its own way of changing it
        let mut key_now = key.clone();
        if let Some(text) = &edit.text {
            if *text != ctx.specs[index].text {
                key_now = self.edit_field_text(&ctx, &name, text)?;
            }
        }

        // the place and the look
        let sch = self.schematic_mut()?;
        let ctx = OwnerCtx::new(sch, model, &key_now).ok_or_else(|| fail("ops_unknown_item", id, "the item has gone"))?;
        let mut placements = ctx.current(sch);
        let old = placements[index].clone();
        // Symbol Properties' place columns first, in the item's own frame (`SetTextPos`, `SetTextAngle`: the field keeps its own justification); then
        // what reads on the sheet (Field Properties' place, and the H and V Align cells, which are the effective justification)
        let mut base = old.clone();
        if let Some(l) = edit.local_at {
            (base.dx, base.dy) = (l.x, l.y);
        }
        if let Some(lv) = edit.local_vertical {
            base.angle = if lv { 90_000 } else { 0 };
        }
        let now = ctx.page(&base, &ctx.specs[index].text);
        let anchor = edit.at.map(|p| (p.x.as_f64(), p.y.as_f64())).unwrap_or(now.at);
        let vertical = edit.vertical.unwrap_or(now.vertical);
        let h = edit.h.map(hjustify).unwrap_or(now.h);
        let v = edit.v.map(vjustify).unwrap_or(now.v);
        let paged = edit.at.is_some() || edit.vertical.is_some() || edit.h.is_some() || edit.v.is_some();
        let mut p = if paged { ctx.placed_at(&base, anchor, vertical, h, v) } else { base.clone() };
        if let Some(s) = edit.size_nm {
            p.size_nm = if s == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { s };
        }
        p.bold = edit.bold.unwrap_or(old.bold);
        p.italic = edit.italic.unwrap_or(old.italic);
        p.visible = edit.visible.unwrap_or(old.visible);
        p.name_shown = edit.name_shown.unwrap_or(old.name_shown);
        p.no_autoplace = edit.allow_autoplace.map(|a| !a).unwrap_or(old.no_autoplace);
        // `positioningModified`: the position, the orientation or the justification as they read on the sheet
        let moved = (anchor.0 - now.at.0).abs() > 50.0 || (anchor.1 - now.at.1).abs() > 50.0 || vertical != now.vertical || h != now.h || v != now.v;
        let changed = p != old || edit.text.as_ref().is_some_and(|t| *t != before_text(&before, &key, &name, model));
        if !changed {
            return Err(fail("ops_field_unchanged", id, "the field already has these properties"));
        }
        placements[index] = p;
        sch.field_layout.insert(key_now.clone(), placements);
        // a power symbol's user field is shown and italic as the power symbol keeps it (`SchExtras::power_props`)
        sync_power_user_fields(sch, &key_now);
        if edit.keep_autoplaced {
            // Symbol Properties: the item's fields are placed again, if at all, after OK (`AutoplaceFieldsKept`)
        } else if moved {
            sch.extras.fields_autoplaced.remove(&key_now);
        } else if let Some(&algo) = sch.extras.fields_autoplaced.get(&key_now).filter(|_| eda_engine::fields::autoplace_prefs().enable || sch.sheets.iter().any(|s| s.id == key_now)) {
            // `editFieldText`: an item whose fields were autoplaced has them placed again, for the new text, size or visibility -- when "Automatically place symbol
            // fields" is on, and for a sheet's always (`if( m_frame->eeconfig()->m_AutoplaceFields.enable || parentType == SCH_SHEET_T )`)
            let snapshot = sch.clone();
            if let Some(ctx) = OwnerCtx::new(&snapshot, model, &key_now) {
                sch.field_layout.insert(key_now.clone(), ctx.autoplace(&snapshot, model, algo));
            }
        }
        Ok(())
    }

    /// Field Properties of a label's field (`DIALOG_FIELD_PROPERTIES` on an `SCH_FIELD` of an `SCH_LABEL_BASE`): its text and whether it is shown and italic are
    /// the label's (`SchExtras::label_fields`); a global label's Sheet References keeps its text (`${INTERSHEET_REFS}`) and is shown as Schematic Setup says
    /// ("Intersheet reference visibility is controlled globally from Schematic Setup > General > Formatting"). A new place, orientation or justification
    /// takes the label's fields out of the autoplaced ones (`UpdateField`: `parent->SetFieldsAutoplaced( AUTOPLACE_NONE )`): every field keeps the place it
    /// has, this one the new one (`eda_engine::label_fields`). Its text size, bold and "Show field name" are the field's own (`UpdateField`: `SetTextSize`,
    /// `SetBold`, `SetNameShown`; UI-LEFT-2 SS12), and its font face and colour are Field Properties' Font and Color (`set_field_looks`). **Differs**: a global
    /// label's Sheet References keeps the default size, regular, without its name (the studio makes that field up from Schematic Setup and stores nothing of it).
    fn edit_label_field(&mut self, ctx: &OwnerCtx, index: usize, id: &str, edit: &FieldEdit) -> Result<(), Vec<CheckResult>> {
        let name = ctx.specs[index].name.clone();
        let isr = name == eda_model::sch_extras::INTERSHEET_REFS_FIELD && ctx.label().is_some_and(|l| matches!(l.kind, eda_model::ir::LabelKind::Global { .. }));
        if isr && (edit.size_nm.is_some_and(|s| s != eda_model::ir::DEFAULT_FIELD_SIZE_NM) || edit.bold == Some(true) || edit.name_shown == Some(true)) {
            return Err(fail("ops_bad_field", id, "a global label's Sheet References keep the default size, regular, without their name"));
        }
        let sch = self.schematic_mut()?;
        let label_id = ctx.key.clone();
        let mut fields = sch.extras.label_fields.get(&label_id).cloned().unwrap_or_default();
        let mut text_changed = false;
        if isr {
            if edit.text.as_ref().is_some_and(|t| *t != ctx.specs[index].text) {
                return Err(fail("ops_field_text_fixed", id, "the Sheet References of a global label show the pages its net is on"));
            }
            if edit.visible.is_some_and(|v| v != ctx.specs[index].visible) {
                return Err(fail("ops_field_text_fixed", id, "Intersheet reference visibility is controlled globally from Schematic Setup > General > Formatting"));
            }
        } else if let Some(f) = fields.iter_mut().find(|f| f.name == name) {
            if let Some(t) = &edit.text {
                text_changed |= f.text != *t;
                f.text = t.clone();
            }
            if let Some(v) = edit.visible {
                text_changed |= f.visible != v;
                f.visible = v;
            }
            if let Some(i) = edit.italic {
                text_changed |= f.italic != i;
                f.italic = i;
            }
            if let Some(size) = edit.size_nm {
                let size = if size == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { size };
                text_changed |= f.size_nm != size;
                f.size_nm = size;
            }
            if let Some(b) = edit.bold {
                text_changed |= f.bold != b;
                f.bold = b;
            }
            if let Some(n) = edit.name_shown {
                text_changed |= f.name_shown != n;
                f.name_shown = n;
            }
        } else {
            return Err(fail("ops_unknown_item", id, format!("the label has no field '{name}'")));
        }
        let mut placements = ctx.current(sch);
        let old = placements[index].clone();
        let now = ctx.page(&old, &ctx.specs[index].text);
        let anchor = edit.at.map(|p| (p.x.as_f64(), p.y.as_f64())).unwrap_or(now.at);
        let vertical = edit.vertical.unwrap_or(now.vertical);
        let h = edit.h.map(hjustify).unwrap_or(now.h);
        let v = edit.v.map(vjustify).unwrap_or(now.v);
        // `positioningModified`: the position, the orientation or the justification
        let moved = (anchor.0 - now.at.0).abs() > 50.0 || (anchor.1 - now.at.1).abs() > 50.0 || vertical != now.vertical || h != now.h || v != now.v;
        if !moved && !text_changed {
            return Err(fail("ops_field_unchanged", id, "the field already has these properties"));
        }
        if text_changed {
            sch.extras.label_fields.insert(label_id.clone(), fields);
        }
        if moved {
            placements[index] = ctx.placed_at(&old, anchor, vertical, h, v);
            sch.field_layout.insert(label_id, placements);
        }
        Ok(())
    }

    /// The text of a field changes: a symbol's Reference renames the part, its Value, Footprint and Datasheet are the part's own, a power symbol's Value is
    /// the net it asserts, a sheet's name and file are the sheet's. Returns the key of the item afterwards (a new reference is a new key).
    fn edit_field_text(&mut self, ctx: &OwnerCtx, name: &str, text: &str) -> Result<String, Vec<CheckResult>> {
        let blank = text.trim().is_empty();
        match (ctx.kind, name) {
            (OwnerKind::Symbol, "Reference") => {
                if blank {
                    // `GetFieldValidationErrorMessage( FIELD_T::REFERENCE, "" )`
                    return Err(fail("ops_bad_symbol", &ctx.key, "The value of the field cannot be empty."));
                }
                let sym = ctx.symbol().expect("a symbol");
                self.rename_symbol(&sym.id, text.trim())?;
                Ok(field_key(text.trim(), sym.unit))
            }
            // `DIALOG_FIELD_PROPERTIES::init` gives the Value no `FIELD_VALIDATOR` (only Reference, Footprint, Datasheet and the sheet's fields), so it may be emptied
            (OwnerKind::Symbol, "Value") => {
                let id = ctx.symbol().expect("a symbol").id.clone();
                self.edit_symbol_fields(&id, Some(text), None, None)?;
                Ok(ctx.key.clone())
            }
            (OwnerKind::Symbol, "Footprint") => {
                let id = ctx.symbol().expect("a symbol").id.clone();
                self.edit_symbol_fields(&id, None, Some(text), None)?;
                Ok(ctx.key.clone())
            }
            (OwnerKind::Symbol, "Datasheet") => {
                let id = ctx.symbol().expect("a symbol").id.clone();
                self.edit_symbol_fields(&id, None, None, Some(text))?;
                Ok(ctx.key.clone())
            }
            // a user-defined field (`SchematicSection::user_fields`, part-wide): any text, none included (`SCH_FIELD::SetText`)
            (OwnerKind::Symbol, other) if !eda_engine::fields::is_standard_field(other) => {
                let id = ctx.symbol().expect("a symbol").id.clone();
                self.schematic_mut()?.user_fields.entry(id).or_default().insert(other.to_string(), text.to_string());
                Ok(ctx.key.clone())
            }
            (OwnerKind::Power, "Value") => {
                if blank {
                    return Err(fail("ops_bad_field", &ctx.key, "A value cannot be empty."));
                }
                let sch = self.schematic_mut()?;
                if let Some(ps) = sch.power_symbols.iter_mut().find(|p| p.id == ctx.key) {
                    ps.net = text.trim().to_string();
                }
                Ok(ctx.key.clone())
            }
            // a user-defined field of a power symbol (`SchExtras::power_props`): any text, as a symbol's (UI-LEFT-2 SS7)
            (OwnerKind::Power, other) if other != "Reference" => {
                let sch = self.schematic_mut()?;
                let Some(f) = sch.extras.power_props.get_mut(&ctx.key).and_then(|p| p.fields.iter_mut().find(|f| f.name == other)) else {
                    return Err(fail("ops_unknown_item", &ctx.key, format!("the power symbol has no field '{other}'")));
                };
                f.text = text.to_string();
                Ok(ctx.key.clone())
            }
            (OwnerKind::Power, _) => Err(fail("ops_field_text_fixed", &ctx.key, "the reference of a power symbol is not edited")),
            (OwnerKind::Sheet, "Sheetname") => {
                self.edit_sch_sheet(&ctx.key, Some(text), None)?;
                Ok(ctx.key.clone())
            }
            (OwnerKind::Sheet, "Sheetfile") => {
                self.edit_sch_sheet(&ctx.key, None, Some(text))?;
                Ok(ctx.key.clone())
            }
            _ => Err(fail("ops_field_text_fixed", &ctx.key, format!("the text of '{name}' is not edited"))),
        }
    }

    /// Add a user-defined field: see [`SchCmd::AddUserField`](crate::sch_edit::SchCmd::AddUserField).
    pub(crate) fn add_user_field(&mut self, id: &str, name: &str, text: &str) -> Result<(), Vec<CheckResult>> {
        let name = name.trim();
        let sch = self.schematic_mut()?;
        if !sch.symbols.iter().any(|s| s.id == id) {
            return Err(fail("ops_unknown_item", id, "no symbol with this reference on the sheet"));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the symbol is locked"));
        }
        if name.is_empty() {
            // `DIALOG_SYMBOL_PROPERTIES::Validate`
            return Err(fail("ops_bad_field", id, "Fields must have a name."));
        }
        // the table's rule (`FIELDS_EDITOR_GRID_DATA_MODEL`): a name that is generated (`${...}`) is no field of the symbol
        if eda_model::text_vars::has_text_vars(name) || name.contains('{') {
            return Err(fail("ops_bad_field", id, format!("'{name}' is not a valid field name.")));
        }
        let taken = eda_engine::fields::STANDARD_FIELDS.iter().any(|n| n.eq_ignore_ascii_case(name)) || sch.user_fields.get(id).is_some_and(|m| m.keys().any(|n| n.eq_ignore_ascii_case(name)));
        if taken {
            // `OnGridCellChanging` (`CmpNoCase`)
            return Err(fail("ops_field_exists", id, format!("Field name '{name}' already in use.")));
        }
        sch.user_fields.entry(id.to_string()).or_default().insert(name.to_string(), text.to_string());
        Ok(())
    }

    /// Remove a user-defined field: see [`SchCmd::RemoveUserField`](crate::sch_edit::SchCmd::RemoveUserField).
    pub(crate) fn remove_user_field(&mut self, id: &str, name: &str) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if eda_engine::fields::is_standard_field(name) {
            return Err(fail("ops_field_mandatory", id, format!("The {name} field cannot be removed.")));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the symbol is locked"));
        }
        if sch.user_fields.get_mut(id).and_then(|m| m.remove(name)).is_none() {
            return Err(fail("ops_unknown_item", id, format!("the symbol has no field '{name}'")));
        }
        if sch.user_fields.get(id).is_some_and(|m| m.is_empty()) {
            sch.user_fields.remove(id);
        }
        drop_field_placements(sch, &[id.to_string()], name);
        // `SCH_SYMBOL::RemoveField`: "This will remove it from all variants"
        eda_model::variants::rename_field(sch, Some(id), name, None);
        Ok(())
    }

    /// Autoplace Fields: see [`SchCmd::AutoplaceFields`](crate::sch_edit::SchCmd::AutoplaceFields).
    pub(crate) fn autoplace_sch_fields(&mut self, ids: &[String]) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "selection", "nothing is selected"));
        }
        let model = self.model;
        let sch = self.schematic_mut()?;
        // the items that have fields: a symbol (each placed unit), a power symbol, a sheet, or the item a selected field belongs to
        let mut keys: Vec<String> = Vec::new();
        for id in ids {
            let found: Vec<String> = if let Some((key, _)) = parse_field_id(id) {
                vec![key.to_string()]
            } else {
                let units: Vec<String> = sch.symbols.iter().filter(|s| s.id == *id || field_key(&s.id, s.unit) == *id).map(|s| field_key(&s.id, s.unit)).collect();
                if !units.is_empty() {
                    units
                } else if sch.power_symbols.iter().any(|p| p.id == *id) || sch.sheets.iter().any(|s| s.id == *id) || sch.labels.iter().any(|l| l.id == *id && eda_engine::label_fields::has_fields(sch, l)) {
                    vec![id.clone()]
                } else {
                    Vec::new()
                }
            };
            for k in found {
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        if keys.is_empty() {
            return Err(fail("ops_unknown_item", &ids[0], "nothing here has fields to place"));
        }
        let locked: BTreeSet<String> = sch.extras.locked.iter().cloned().collect();
        let mut changed = false;
        for key in keys {
            let snapshot = sch.clone();
            let Some(ctx) = OwnerCtx::new(&snapshot, model, &key) else { continue };
            if locked.contains(&owner_id(&ctx)) {
                continue;
            }
            // a label's fields go back to the autoplaced ones (`SCH_LABEL_BASE::AutoplaceFields( screen, AUTOPLACE_MANUAL )`): placed live from now on
            if ctx.kind == OwnerKind::Label {
                changed |= sch.field_layout.remove(&key).is_some();
                continue;
            }
            let before = ctx.current(&snapshot);
            let placed = ctx.autoplace(&snapshot, model, AutoplaceAlgo::Manual);
            if placed != before || snapshot.extras.fields_autoplaced.get(&key) != Some(&AutoplaceAlgo::Manual) || !snapshot.field_layout.contains_key(&key) {
                changed = true;
            }
            sch.field_layout.insert(key.clone(), placed);
            sch.extras.fields_autoplaced.insert(key, AutoplaceAlgo::Manual);
        }
        if !changed {
            return Err(fail("ops_fields_unchanged", &ids[0], "the fields are already where Autoplace Fields puts them"));
        }
        Ok(())
    }

    /// [`SchCmd::AutoplaceFieldsKept`](crate::sch_edit::SchCmd::AutoplaceFieldsKept): what `SCH_EDIT_TOOL::Properties` does after Symbol Properties' OK
    /// ("`AUTOPLACE_ALGO fieldsAutoplaced = symbol->GetFieldsAutoplaced(); if( fieldsAutoplaced == AUTOPLACE_AUTO || fieldsAutoplaced == AUTOPLACE_MANUAL )
    /// symbol->AutoplaceFields( m_frame->GetScreen(), fieldsAutoplaced )`"): the fields of each item `keys` names (a placed unit's key, a power symbol's id) whose
    /// fields Autoplace Fields put where they are are placed again, in the way they were; an item whose fields were placed by hand keeps them. Nothing to place
    /// is no error: the dialog's step is the rest of its batch.
    pub(crate) fn autoplace_fields_kept(&mut self, keys: &[String]) -> Result<(), Vec<CheckResult>> {
        let model = self.model;
        let sch = self.schematic_mut()?;
        // `if( m_frame->eeconfig()->m_AutoplaceFields.enable )`: with "Automatically place symbol fields" off the fields stay as the dialog left them
        if !eda_engine::fields::autoplace_prefs().enable {
            return Ok(());
        }
        for key in keys {
            let Some(&algo) = sch.extras.fields_autoplaced.get(key) else { continue };
            let snapshot = sch.clone();
            let Some(ctx) = OwnerCtx::new(&snapshot, model, key) else { continue };
            if snapshot.extras.is_locked(&owner_id(&ctx)) {
                continue;
            }
            sch.field_layout.insert(key.clone(), ctx.autoplace(&snapshot, model, algo));
            sync_power_user_fields(sch, key);
        }
        Ok(())
    }

    /// [`SchCmd::ReorderFields`](crate::sch_edit::SchCmd::ReorderFields): Symbol Properties' Move Up / Move Down (`DIALOG_SYMBOL_PROPERTIES::OnMoveUp` /
    /// `OnMoveDown`: "row > m_fields->GetMandatoryRowCount()", a user field never goes above the mandatory ones; OK gives the fields their order,
    /// `SetOrdinal( ordinal++ )`). The user fields of the placed unit `key` take the order `names` gives them (a name the unit does not have is passed over; a
    /// field `names` leaves out keeps its place after them); the order is the order of the unit's stored placements, which the fields are listed, autoplaced,
    /// written and read back in ([`eda_engine::fields::symbol_specs_in`]). Refused when the order is the one it has.
    pub(crate) fn reorder_sch_fields(&mut self, key: &str, names: &[String]) -> Result<(), Vec<CheckResult>> {
        let model = self.model;
        let sch = self.schematic_mut()?;
        let Some(ctx) = OwnerCtx::new(sch, model, key) else {
            return Err(fail("ops_unknown_item", key, "no placed symbol with this key on the sheet"));
        };
        if ctx.kind != OwnerKind::Symbol {
            return Err(fail("ops_unknown_item", key, "only a placed symbol's user fields are reordered here (a power symbol's are Symbol Properties' list)"));
        }
        if sch.extras.is_locked(&owner_id(&ctx)) {
            return Err(fail("ops_locked", key, "the symbol is locked"));
        }
        let placements = ctx.current(sch);
        let (mandatory, user): (Vec<FieldPlacement>, Vec<FieldPlacement>) = placements.into_iter().partition(|p| eda_engine::fields::is_standard_field(&p.name));
        let rank = |p: &FieldPlacement| names.iter().position(|n| *n == p.name).unwrap_or(usize::MAX);
        let mut ordered = user.clone();
        ordered.sort_by_key(rank);
        if ordered.iter().map(|p| &p.name).eq(user.iter().map(|p| &p.name)) {
            return Err(fail("ops_nothing_to_change", key, "the fields are already in this order"));
        }
        sch.field_layout.insert(key.to_string(), mandatory.into_iter().chain(ordered).collect());
        Ok(())
    }
}

/// The id the item that owns the fields is locked by (a symbol by its reference, whatever its unit).
fn owner_id(ctx: &OwnerCtx) -> String {
    match ctx.symbol() {
        Some(sym) => sym.id.clone(),
        None => ctx.key.clone(),
    }
}

/// The text the field shows in `sch` now.
fn before_text(sch: &SchematicSection, key: &str, name: &str, model: &ConstraintModel) -> String {
    OwnerCtx::new(sch, model, key).and_then(|c| c.specs.iter().find(|s| s.name == name).map(|s| s.text.clone())).unwrap_or_default()
}

/// The keys of `field_layout` and its companions that belong to the symbol `id` (any unit): `id`, `id#2`, ...
fn keys_of(sch: &SchematicSection, id: &str) -> Vec<String> {
    let prefix = format!("{id}#");
    let mut keys: BTreeSet<String> = BTreeSet::new();
    keys.extend(sch.field_layout.keys().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.extend(sch.extras.fields_autoplaced.keys().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.extend(sch.extras.body_styles.keys().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.extend(sch.extras.pin_swaps.keys().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.extend(sch.extras.pin_alternates.keys().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.extend(sch.extras.field_looks.keys().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.extend(sch.extras.path_units.keys().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.extend(sch.extras.excluded_from_pos_files.iter().filter(|k| k.as_str() == id || k.starts_with(&prefix)).cloned());
    keys.into_iter().collect()
}

/// The places a user-defined field had on the units of the symbols `refs` go (a field removed from them).
pub(crate) fn drop_field_placements(sch: &mut SchematicSection, refs: &[String], name: &str) {
    for id in refs {
        for key in keys_of(sch, id) {
            if let Some(placements) = sch.field_layout.get_mut(&key) {
                placements.retain(|p| p.name != name);
            }
            // its font face and colour go with it (`SchExtras::field_looks`)
            sch.extras.set_field_look(&key, name, Default::default());
        }
    }
}

/// A user-defined field of the symbols `refs` is renamed: the places it had keep it.
pub(crate) fn rename_field_placements(sch: &mut SchematicSection, refs: &[String], from: &str, to: &str) {
    for id in refs {
        for key in keys_of(sch, id) {
            if let Some(placements) = sch.field_layout.get_mut(&key) {
                for p in placements.iter_mut().filter(|p| p.name == from) {
                    p.name = to.to_string();
                }
            }
            if let Some(look) = sch.extras.field_looks.get_mut(&key).and_then(|m| m.remove(from)) {
                sch.extras.field_looks.entry(key.clone()).or_default().insert(to.to_string(), look);
            }
        }
    }
}

/// A symbol is renamed: what is kept per unit under its reference (where its fields are, whether Autoplace Fields put them there, its body style, the places
/// Swap Pins gave its pins, the unit each sheet path draws (r4 A4-M3), "Exclude from position files" (r4 B4-M2)) is kept under the new one: KiCad keeps them
/// in the symbol (`SCH_SYMBOL_INSTANCE::m_Unit`, `m_excludedFromPosFiles`), which a new reference does not change.
pub(crate) fn rename_symbol_keys(sch: &mut SchematicSection, id: &str, new_id: &str) {
    // the groups that list the reference list the new one
    crate::sch_groups::rename_member(sch, id, new_id);
    // its records of the design variants are its instances' (`SCH_SYMBOL_INSTANCE::m_Variants`)
    eda_model::variants::rename_reference(sch, id, new_id);
    for old in keys_of(sch, id) {
        let new = format!("{new_id}{}", &old[id.len()..]);
        if let Some(v) = sch.field_layout.remove(&old) {
            sch.field_layout.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.fields_autoplaced.remove(&old) {
            sch.extras.fields_autoplaced.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.body_styles.remove(&old) {
            sch.extras.body_styles.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.pin_swaps.remove(&old) {
            sch.extras.pin_swaps.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.pin_alternates.remove(&old) {
            sch.extras.pin_alternates.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.field_looks.remove(&old) {
            sch.extras.field_looks.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.path_units.remove(&old) {
            sch.extras.path_units.insert(new.clone(), v);
        }
        if sch.extras.excluded_from_pos_files.remove(&old) {
            sch.extras.excluded_from_pos_files.insert(new);
        }
    }
}

/// A placed unit is deleted: what was kept for it goes.
pub(crate) fn drop_symbol_keys(sch: &mut SchematicSection, id: &str, unit: u32) {
    let key = field_key(id, unit);
    sch.field_layout.remove(&key);
    sch.extras.fields_autoplaced.remove(&key);
    sch.extras.body_styles.remove(&key);
    sch.extras.pin_swaps.remove(&key);
    sch.extras.pin_alternates.remove(&key);
    sch.extras.field_looks.remove(&key);
    sch.extras.path_units.remove(&key);
    sch.extras.excluded_from_pos_files.remove(&key);
}
