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
use eda_model::ir::{field_key, FieldPlacement, Point, SchematicSection, TextJustify, TextVAlign, Um};
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

    /// The box a field's text takes on the sheet (`SCH_FIELD::GetBoundingBox`), micrometres.
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
            let at = Point { x: anchor.0.round() as i64, y: anchor.1.round() as i64 };
            let to = rotate_point(at, center, ccw);
            let (lx, ly) = ctx.local_pos((to.x as f64, to.y as f64));
            p.dx = lx;
            p.dy = ly;
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
    pub size_um: Option<Um>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub visible: Option<bool>,
    pub name_shown: Option<bool>,
    pub allow_autoplace: Option<bool>,
}

/// `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM`: "Don't allow text to disappear; it can be difficult to correct if you can't select it".
const TEXT_SIZE_MIN_UM: Um = 10;
const TEXT_SIZE_MAX_UM: Um = 1_000_000;

impl<'a> Board<'a> {
    /// Field Properties: see [`SchCmd::EditField`](crate::sch_edit::SchCmd::EditField).
    pub(crate) fn edit_sch_field(&mut self, id: &str, edit: &FieldEdit) -> Result<(), Vec<CheckResult>> {
        let Some((key, name)) = parse_field_id(id) else {
            return Err(fail("ops_unknown_item", id, "not the id of a field"));
        };
        let (key, name) = (key.to_string(), name.to_string());
        if let Some(size) = edit.size_um {
            if !(TEXT_SIZE_MIN_UM..=TEXT_SIZE_MAX_UM).contains(&size) {
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
        let now = ctx.page(&old, &ctx.specs[index].text);
        let anchor = edit.at.map(|p| (p.x as f64, p.y as f64)).unwrap_or(now.at);
        let vertical = edit.vertical.unwrap_or(now.vertical);
        let h = edit.h.map(hjustify).unwrap_or(now.h);
        let v = edit.v.map(vjustify).unwrap_or(now.v);
        let mut p = ctx.placed_at(&old, anchor, vertical, h, v);
        if let Some(s) = edit.size_um {
            p.size_um = if s == eda_model::ir::DEFAULT_FIELD_SIZE_UM { 0 } else { s };
        }
        p.bold = edit.bold.unwrap_or(old.bold);
        p.italic = edit.italic.unwrap_or(old.italic);
        p.visible = edit.visible.unwrap_or(old.visible);
        p.name_shown = edit.name_shown.unwrap_or(old.name_shown);
        p.no_autoplace = edit.allow_autoplace.map(|a| !a).unwrap_or(old.no_autoplace);
        // `positioningModified`: the position, the orientation or the justification as they read on the sheet
        let moved = (anchor.0 - now.at.0).abs() > 0.5 || (anchor.1 - now.at.1).abs() > 0.5 || vertical != now.vertical || h != now.h || v != now.v;
        let changed = p != old || edit.text.as_ref().is_some_and(|t| *t != before_text(&before, &key, &name, model));
        if !changed {
            return Err(fail("ops_field_unchanged", id, "the field already has these properties"));
        }
        placements[index] = p;
        sch.field_layout.insert(key_now.clone(), placements);
        if moved {
            sch.extras.fields_autoplaced.remove(&key_now);
        } else if let Some(&algo) = sch.extras.fields_autoplaced.get(&key_now) {
            // `editFieldText`: an item whose fields were autoplaced has them placed again, for the new text, size or visibility
            let snapshot = sch.clone();
            if let Some(ctx) = OwnerCtx::new(&snapshot, model, &key_now) {
                sch.field_layout.insert(key_now.clone(), ctx.autoplace(&snapshot, model, algo));
            }
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
                    return Err(fail("ops_bad_symbol", &ctx.key, "A reference cannot be empty."));
                }
                let sym = ctx.symbol().expect("a symbol");
                self.rename_symbol(&sym.id, text.trim())?;
                Ok(field_key(text.trim(), sym.unit))
            }
            (OwnerKind::Symbol, "Value") => {
                if blank {
                    return Err(fail("ops_bad_field", &ctx.key, "A value cannot be empty."));
                }
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
                } else if sch.power_symbols.iter().any(|p| p.id == *id) || sch.sheets.iter().any(|s| s.id == *id) {
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
    keys.into_iter().collect()
}

/// A symbol is renamed: what is kept per unit under its reference (where its fields are, whether Autoplace Fields put them there, its body style) is
/// kept under the new one.
pub(crate) fn rename_symbol_keys(sch: &mut SchematicSection, id: &str, new_id: &str) {
    for old in keys_of(sch, id) {
        let new = format!("{new_id}{}", &old[id.len()..]);
        if let Some(v) = sch.field_layout.remove(&old) {
            sch.field_layout.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.fields_autoplaced.remove(&old) {
            sch.extras.fields_autoplaced.insert(new.clone(), v);
        }
        if let Some(v) = sch.extras.body_styles.remove(&old) {
            sch.extras.body_styles.insert(new, v);
        }
    }
}

/// A placed unit is deleted: what was kept for it goes.
pub(crate) fn drop_symbol_keys(sch: &mut SchematicSection, id: &str, unit: u32) {
    let key = field_key(id, unit);
    sch.field_layout.remove(&key);
    sch.extras.fields_autoplaced.remove(&key);
    sch.extras.body_styles.remove(&key);
}
