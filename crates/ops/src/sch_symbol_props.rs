//! What Symbol Properties and the symbol's and pin's context menus change on a placed symbol beyond its fields' texts (UI-LEFT S4, S5): the alternate
//! function of a pin, the unit a placed symbol draws, and the font face and colour of a field. Ported from eeschema at 8303b2ad:
//!
//! * `SCH_EDIT_FRAME::SetAltPinFunction` (picksymbol.cpp; the Pin Function submenu, `ALT_PIN_FUNCTION_MENU` / `ID_POPUP_SCH_ALT_PIN_FUNCTION`) and the
//!   Pin Functions table of `DIALOG_SYMBOL_PROPERTIES::TransferDataFromWindow` ("Update any assignments": `SetAlt` on every pin of the number, then
//!   `SyncOtherUnits`, which keeps "alternate pin assignments in sync in multi-unit parts"), with `SCH_PIN::SetAlt`'s rule: the pin's own name or an
//!   empty one is no alternate;
//! * `SCH_EDIT_FRAME::SelectUnit` (picksymbol.cpp; the Symbol Unit submenu, `SYMBOL_UNIT_MENU` / `ID_POPUP_SCH_SELECT_UNIT`) and the Unit choice of
//!   Symbol Properties (`SetUnitSelection` + `SetUnit`), with "Swap '%s' and '%s'" when the unit is already placed;
//! * the Font and Color columns of `FIELDS_GRID_TABLE` (fields_grid_table.cpp `SetValue`: `FDC_FONT`, `FDC_COLOR`), kept in `SchExtras::field_looks`.

use crate::Board;
use eda_engine::fields_edit::{parse_field_id, OwnerCtx};
use eda_model::ir::field_key;
use eda_model::sch_extras::{FieldFontLook, SchColor, KICAD_FONT_NAME};
use eda_model::CheckResult;
use std::collections::BTreeMap;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

/// `DEFAULT_FONT_NAME` (fields_grid_table.cpp: `_( "Default Font" )`): the Font column's first choice, the font the schematic's settings give (`SetFont( nullptr )`).
pub const DEFAULT_FONT_NAME: &str = "Default Font";

impl<'a> Board<'a> {
    /// [`crate::sch_edit::SchCmd::SetPinAlternates`].
    pub(crate) fn set_pin_alternates(&mut self, id: &str, unit: Option<u32>, alternates: &BTreeMap<String, String>, other_units: bool) -> Result<(), Vec<CheckResult>> {
        if alternates.is_empty() {
            return Err(fail("ops_nothing_to_change", id, "no pin named"));
        }
        let i = self.find_symbol_unit_index(id, unit)?;
        let sym = self.schematic()?.symbols[i].clone();
        if self.schematic()?.extras.is_locked(&sym.id) {
            return Err(fail("ops_locked", id, "the symbol is locked"));
        }
        let Some(lib) = self.design_symbol(&sym.lib_id) else {
            return Err(fail("ops_no_alternates", id, "the symbol has no library symbol whose pins have alternate functions"));
        };
        // what each named pin keeps (`SCH_PIN::SetAlt`): nothing for an empty name or the pin's own; else one of the library pin's alternates
        let mut wanted: Vec<(String, Option<String>)> = Vec::new();
        for (number, alt) in alternates {
            let Some(lib_pin) = lib.pin_by_number(number) else {
                return Err(fail("ops_unknown_pin", id, format!("the symbol has no pin '{number}'")));
            };
            if alt.is_empty() || *alt == lib_pin.name {
                wanted.push((number.clone(), None));
                continue;
            }
            let known = lib.connectivity.pin_alternates.get(number).is_some_and(|alts| alts.iter().any(|a| a.name == *alt));
            if !known {
                // `wxFAIL_MSG( "Pin '%s' has no alterate '%s'" )`: refused here rather than quietly dropped
                return Err(fail("ops_unknown_alternate", id, format!("Pin '{number}' has no alternate '{alt}'")));
            }
            wanted.push((number.clone(), Some(alt.clone())));
        }
        let sch = self.schematic_mut()?;
        // the placed unit named, and with `other_units` every other placed unit of the reference on the sheet (`SyncOtherUnits`)
        let keys: Vec<String> = if other_units { sch.symbols.iter().filter(|s| s.id == sym.id).map(|s| field_key(&s.id, s.unit)).collect() } else { vec![field_key(&sym.id, sym.unit)] };
        let before = sch.extras.pin_alternates.clone();
        for key in keys {
            let map = sch.extras.pin_alternates.entry(key.clone()).or_default();
            for (number, alt) in &wanted {
                match alt {
                    Some(a) => {
                        map.insert(number.clone(), a.clone());
                    }
                    None => {
                        map.remove(number);
                    }
                }
            }
            if map.is_empty() {
                sch.extras.pin_alternates.remove(&key);
            }
        }
        if sch.extras.pin_alternates == before {
            return Err(fail("ops_nothing_to_change", id, "the pins already have these functions"));
        }
        Ok(())
    }

    /// [`crate::sch_edit::SchCmd::SetSymbolUnit`].
    pub(crate) fn set_symbol_unit(&mut self, id: &str, unit: Option<u32>, new_unit: u32, swap: bool) -> Result<(), Vec<CheckResult>> {
        let i = self.find_symbol_unit_index(id, unit)?;
        let sym = self.schematic()?.symbols[i].clone();
        if self.schematic()?.extras.is_locked(&sym.id) {
            return Err(fail("ops_locked", id, "the symbol is locked"));
        }
        // `const int unitCount = symbol->GetUnitCount(); if( unitCount <= 1 || currentUnit == aUnit ) return; if( aUnit > unitCount ) aUnit = unitCount;`
        let lib = self.design_symbol(&sym.lib_id);
        let count = lib.as_ref().map_or(1, |l| l.unit_count.max(1));
        // `LIB_SYMBOL::GetUnitDisplayName( aUnit, false )`: the unit's own name, else its letter
        let unit_name = |u: u32| lib.as_ref().and_then(|l| l.connectivity.unit_names.get(&u).cloned()).unwrap_or_else(|| eda_model::annotate::unit_letters(u));
        if count <= 1 {
            return Err(fail("ops_nothing_to_change", id, "This symbol has only one unit."));
        }
        let target = new_unit.clamp(1, count);
        if target == sym.unit {
            return Err(fail("ops_nothing_to_change", id, "the symbol already draws this unit"));
        }
        let model = self.model;
        let sch = self.schematic_mut()?;
        let other = sch.symbols.iter().position(|s| s.id == sym.id && s.unit == target);
        let (old_key, new_key) = (field_key(&sym.id, sym.unit), field_key(&sym.id, target));
        match other {
            // `FindSymbolByRefAndUnit`: "Symbol unit '%s' is already placed (on sheet '%s')"; "Swap" gives that symbol the unit this one had
            Some(j) if swap => {
                let mine = take_unit_keys(sch, &old_key);
                let theirs = take_unit_keys(sch, &new_key);
                put_unit_keys(sch, &new_key, mine);
                put_unit_keys(sch, &old_key, theirs);
                sch.symbols[j].unit = sym.unit;
                sch.symbols[i].unit = target;
            }
            // "Duplicate": two placements of one unit -- the model keeps one per reference and unit on a sheet, so this is refused here
            Some(_) => {
                return Err(fail(
                    "ops_unit_placed",
                    id,
                    format!("Symbol unit '{}' is already placed on this sheet; swap the two units instead", unit_name(target)),
                ));
            }
            None => {
                let mine = take_unit_keys(sch, &old_key);
                put_unit_keys(sch, &new_key, mine);
                sch.symbols[i].unit = target;
            }
        }
        // `if( fieldsAutoplaced == AUTOPLACE_AUTO || fieldsAutoplaced == AUTOPLACE_MANUAL ) aSymbol->AutoplaceFields( GetScreen(), fieldsAutoplaced )`
        let keys: Vec<String> = if other.is_some() { vec![new_key, old_key] } else { vec![new_key] };
        for key in keys {
            if let Some(&algo) = sch.extras.fields_autoplaced.get(&key) {
                let snapshot = sch.clone();
                if let Some(ctx) = OwnerCtx::new(&snapshot, model, &key) {
                    sch.field_layout.insert(key.clone(), ctx.autoplace(&snapshot, model, algo));
                }
            }
        }
        Ok(())
    }

    /// [`crate::sch_edit::SchCmd::SetFieldLooks`].
    pub(crate) fn set_field_looks(&mut self, ids: &[String], font: Option<&str>, color: Option<SchColor>) -> Result<(), Vec<CheckResult>> {
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "fields", "select a field first"));
        }
        let model = self.model;
        let mut targets: Vec<(String, String)> = Vec::new();
        {
            let sch = self.schematic()?;
            for id in ids {
                let Some((key, name)) = parse_field_id(id) else {
                    return Err(fail("ops_unknown_item", id, "not the id of a field"));
                };
                let Some(ctx) = OwnerCtx::new(sch, model, key) else {
                    return Err(fail("ops_unknown_item", id, "no field with this id on the sheet"));
                };
                if !ctx.specs.iter().any(|s| s.name == name) {
                    return Err(fail("ops_unknown_item", id, format!("the item has no field '{name}'")));
                }
                targets.push((key.to_string(), name.to_string()));
            }
        }
        let sch = self.schematic_mut()?;
        let mut changed = false;
        for (key, name) in targets {
            let mut look = sch.extras.field_look(&key, &name).cloned().unwrap_or_default();
            if let Some(f) = font {
                look.font = font_of(f);
            }
            if let Some(c) = color {
                // a colour with every channel 0 is `COLOR4D::UNSPECIFIED`, the layer's (the convention `SetTextAttrs` and `SetStroke` take)
                look.color = (c != SchColor { r: 0, g: 0, b: 0, a: 0 }).then_some(c);
            }
            changed |= sch.extras.set_field_look(&key, &name, look);
        }
        if !changed {
            return Err(fail("ops_field_unchanged", "fields", "the fields already have this font and colour"));
        }
        Ok(())
    }
}

/// [`crate::sch_edit::SchCmd::SetPowerProps`]: a value left out is unchanged.
pub(crate) struct PowerPropsEdit {
    pub footprint: Option<String>,
    pub datasheet: Option<String>,
    pub exclude_from_sim: Option<bool>,
    pub exclude_from_bom: Option<bool>,
    pub exclude_from_board: Option<bool>,
    pub dnp: Option<bool>,
    pub exclude_from_pos_files: Option<bool>,
    pub fields: Option<Vec<eda_model::sch_extras::LabelField>>,
}

impl<'a> Board<'a> {
    /// [`crate::sch_edit::SchCmd::SetPowerProps`].
    pub(crate) fn set_power_props(&mut self, id: &str, e: &PowerPropsEdit) -> Result<(), Vec<CheckResult>> {
        let sch = self.schematic_mut()?;
        if !sch.power_symbols.iter().any(|p| p.id == id) {
            return Err(fail("ops_unknown_item", id, "no power symbol with this id on the sheet"));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the power symbol is locked"));
        }
        if let Some(fields) = &e.fields {
            // `DIALOG_SYMBOL_PROPERTIES::Validate`: "Fields must have a name."; `OnGridCellChanging`: "Field name '%s' already in use." (any case); a mandatory
            // field's name is that field's
            for (i, f) in fields.iter().enumerate() {
                let name = f.name.trim();
                if name.is_empty() {
                    return Err(fail("ops_bad_field_name", id, "Fields must have a name."));
                }
                if ["Reference", "Value", "Footprint", "Datasheet"].iter().any(|m| m.eq_ignore_ascii_case(name)) || fields[..i].iter().any(|o| o.name.trim().eq_ignore_ascii_case(name)) {
                    return Err(fail("ops_bad_field_name", id, format!("Field name '{name}' already in use.")));
                }
            }
        }
        let mut p = sch.extras.power_props.get(id).cloned().unwrap_or_default();
        let before = p.clone();
        if let Some(v) = &e.footprint {
            p.footprint = v.clone();
        }
        if let Some(v) = &e.datasheet {
            p.datasheet = v.clone();
        }
        let set = |slot: &mut bool, v: Option<bool>| {
            if let Some(v) = v {
                *slot = v;
            }
        };
        set(&mut p.exclude_from_sim, e.exclude_from_sim);
        set(&mut p.exclude_from_bom, e.exclude_from_bom);
        set(&mut p.exclude_from_board, e.exclude_from_board);
        set(&mut p.dnp, e.dnp);
        set(&mut p.exclude_from_pos_files, e.exclude_from_pos_files);
        if let Some(fields) = &e.fields {
            p.fields = fields.iter().map(|f| eda_model::sch_extras::LabelField { name: f.name.trim().to_string(), ..f.clone() }).collect();
        }
        if p == before {
            return Err(fail("ops_nothing_to_change", id, "the power symbol already has these properties"));
        }
        if p.is_default() {
            sch.extras.power_props.remove(id);
        } else {
            sch.extras.power_props.insert(id.to_string(), p);
        }
        Ok(())
    }
}

/// What the Font column's text sets/// What the Font column's text sets (`FIELDS_GRID_TABLE::SetValue( FDC_FONT )`): "Default Font" (or nothing) is no font of its own, "KiCad Font" the stroke
/// font by name, anything else the outline font of that family.
pub fn font_of(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t == DEFAULT_FONT_NAME {
        None
    } else if t == KICAD_FONT_NAME {
        Some(KICAD_FONT_NAME.to_string())
    } else {
        Some(t.to_string())
    }
}

/// What the sheet keeps under one placed unit's key (`field_key`), taken out to move to another key: where its fields are and how they look, whether
/// Autoplace Fields put them there, its body style, the places Swap Pins gave its pins, the alternates picked for them, and "Exclude from position files".
#[derive(Default)]
struct UnitKeyed {
    layout: Option<Vec<eda_model::ir::FieldPlacement>>,
    autoplaced: Option<eda_model::sch_extras::AutoplaceAlgo>,
    body_style: Option<u32>,
    swaps: Option<BTreeMap<String, String>>,
    alternates: Option<BTreeMap<String, String>>,
    looks: Option<BTreeMap<String, FieldFontLook>>,
    pos_excluded: bool,
}

fn take_unit_keys(sch: &mut eda_model::ir::SchematicSection, key: &str) -> UnitKeyed {
    // the unit each sheet path drew goes: the symbol draws the new unit on every path (`SchExtras::path_units`' documented difference)
    sch.extras.path_units.remove(key);
    UnitKeyed {
        layout: sch.field_layout.remove(key),
        autoplaced: sch.extras.fields_autoplaced.remove(key),
        body_style: sch.extras.body_styles.remove(key),
        swaps: sch.extras.pin_swaps.remove(key),
        alternates: sch.extras.pin_alternates.remove(key),
        looks: sch.extras.field_looks.remove(key),
        pos_excluded: sch.extras.excluded_from_pos_files.remove(key),
    }
}

fn put_unit_keys(sch: &mut eda_model::ir::SchematicSection, key: &str, v: UnitKeyed) {
    let key = key.to_string();
    if let Some(x) = v.layout {
        sch.field_layout.insert(key.clone(), x);
    }
    if let Some(x) = v.autoplaced {
        sch.extras.fields_autoplaced.insert(key.clone(), x);
    }
    if let Some(x) = v.body_style {
        sch.extras.body_styles.insert(key.clone(), x);
    }
    if let Some(x) = v.swaps {
        sch.extras.pin_swaps.insert(key.clone(), x);
    }
    if let Some(x) = v.alternates {
        sch.extras.pin_alternates.insert(key.clone(), x);
    }
    if let Some(x) = v.looks {
        sch.extras.field_looks.insert(key.clone(), x);
    }
    if v.pos_excluded {
        sch.extras.excluded_from_pos_files.insert(key);
    }
}
