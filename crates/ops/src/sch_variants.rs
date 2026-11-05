//! The design-variant verbs (UI-LEFT S2; KiCad 10, ported from KiCad 25ec2335): the variant list the Tools > Variants menu, the toolbar's
//! "Add New Design Variant..." and the Symbol Fields Table's variant buttons edit, and a symbol's own record of a variant (its flags and field texts there),
//! which Symbol Properties, the Properties panel, the attribute toggles and the Fields Table edit while a variant is the current one. The data is
//! `eda_model::variants`'.
//!
//! * [`Cmd::AddDesignVariant`] -- `SCH_EDIT_FRAME::ShowAddVariantDialog` (`eeschema.EditorControl.addVariant`): `ValidateNewVariantName`, then
//!   `SCHEMATIC::AddVariant` and `SetVariantDescription`.
//! * [`Cmd::RemoveDesignVariant`] -- `SCH_EDIT_FRAME::RemoveVariant` (`removeVariant`) > `SCHEMATIC::DeleteVariant`.
//! * [`Cmd::EditDesignVariantDescription`] -- `SCH_EDIT_FRAME::EditVariantDescription` (`editVariantDescription`) > `SetVariantDescription`.
//! * [`Cmd::RenameDesignVariant`], [`Cmd::CopyDesignVariant`] -- `SCH_EDIT_FRAME::RenameVariant` / `CopyVariant` (`renameVariant`, `copyVariant`).
//! * [`Cmd::SetSymbolVariant`] -- what `SCH_SYMBOL::SetDNP( .., aInstance, aVariantName )` and the other attribute setters (`setVariantAttribute`) and
//!   `SetFieldText( .., aVariantName )` do for the current variant.
//!
//! Each is one undo step, as KiCad's `VARIANT_PROXY_UNDO_ITEM` (and the commit of the symbols' records with it) is.
//!
//! **Differs**: an empty description keeps the variant in the list (`SCHEMATIC::SetVariantDescription` erases the entry of `m_VariantDescriptions`, so a
//! variant no symbol differs in is not in the project file KiCad saves next, and is gone when it is opened again).

use eda_model::variants::{self, Flag};
use eda_model::CheckResult;
use std::collections::BTreeMap;

use super::Board;

fn fail(check: &str, subject: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, subject, msg.into())]
}

/// The flags a [`Cmd::SetSymbolVariant`] sets (`None` leaves one alone).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VariantFlags {
    pub dnp: Option<bool>,
    pub exclude_from_bom: Option<bool>,
    pub exclude_from_board: Option<bool>,
    pub exclude_from_sim: Option<bool>,
    pub exclude_from_pos_files: Option<bool>,
}

impl VariantFlags {
    fn each(&self) -> [(Flag, Option<bool>); 5] {
        [(Flag::Dnp, self.dnp), (Flag::ExcludeFromBom, self.exclude_from_bom), (Flag::ExcludeFromBoard, self.exclude_from_board), (Flag::ExcludeFromSim, self.exclude_from_sim), (Flag::ExcludeFromPosFiles, self.exclude_from_pos_files)]
    }
}

impl<'a> Board<'a> {
    /// The design's variant names (`SCHEMATIC::GetVariantNames`).
    fn variant_names(&self) -> Vec<String> {
        variants::names(&self.design)
    }

    /// The root's list of variants and descriptions (`SCHEMATIC_SETTINGS::m_VariantDescriptions`, which only the root's extras hold).
    fn variant_list_mut(&mut self) -> Result<&mut BTreeMap<String, String>, Vec<CheckResult>> {
        self.design.schematic.as_mut().map(|s| &mut s.extras.variants).ok_or_else(|| fail("ops_no_schematic", "schematic", "this board has no schematic section yet"))
    }

    /// The variant of that name, as the design spells it: refused when there is none.
    fn existing_variant(&self, name: &str) -> Result<String, Vec<CheckResult>> {
        self.variant_names().into_iter().find(|n| n == name).ok_or_else(|| fail("ops_unknown_variant", name, format!("no design variant named '{name}'")))
    }

    /// Every section of the design, the root first.
    fn sections_mut(&mut self) -> impl Iterator<Item = &mut eda_model::ir::SchematicSection> {
        self.design.schematic.iter_mut().chain(self.design.sheet_contents.iter_mut().flat_map(|c| c.values_mut()))
    }

    /// `SCH_EDIT_FRAME::ShowAddVariantDialog`: the name and the description trimmed (`Trim().Trim( false )`), the name checked
    /// (`ValidateNewVariantName( variantName, wxEmptyString )`), then `SCHEMATIC::AddVariant` and, for a description, `SetVariantDescription`.
    pub(crate) fn add_design_variant(&mut self, name: &str, description: &str) -> Result<(), Vec<CheckResult>> {
        let (name, description) = (name.trim(), description.trim());
        variants::validate_new_name(&self.variant_names(), name, "").map_err(|why| fail("ops_variant_name", name, why))?;
        self.variant_list_mut()?.insert(name.to_string(), description.to_string());
        Ok(())
    }

    /// `SCHEMATIC::DeleteVariant`: every symbol's record of the variant goes (`SCH_SCREENS::DeleteVariant`), and so does its entry.
    pub(crate) fn remove_design_variant(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        let name = self.existing_variant(name)?;
        self.variant_list_mut()?.remove(&name);
        for sch in self.sections_mut() {
            variants::delete_records(sch, &name);
        }
        Ok(())
    }

    /// `SCH_EDIT_FRAME::EditVariantDescription`: the description trimmed, then `SCHEMATIC::SetVariantDescription` (see the module doc for an empty one).
    pub(crate) fn edit_design_variant_description(&mut self, name: &str, description: &str) -> Result<(), Vec<CheckResult>> {
        let name = self.existing_variant(name)?;
        self.variant_list_mut()?.insert(name, description.trim().to_string());
        Ok(())
    }

    /// `SCH_EDIT_FRAME::RenameVariant`: the new name trimmed; the same name changes nothing; another is checked (`ValidateNewVariantName( newName, oldName )`,
    /// so the variant may change the case of its own name), then `SCHEMATIC::RenameVariant` moves the description and every record.
    pub(crate) fn rename_design_variant(&mut self, name: &str, new_name: &str) -> Result<(), Vec<CheckResult>> {
        let name = self.existing_variant(name)?;
        let new_name = new_name.trim();
        if new_name == name {
            return Ok(());
        }
        variants::validate_new_name(&self.variant_names(), new_name, &name).map_err(|why| fail("ops_variant_name", new_name, why))?;
        let list = self.variant_list_mut()?;
        let description = list.remove(&name).unwrap_or_default();
        list.insert(new_name.to_string(), description);
        for sch in self.sections_mut() {
            variants::rename_records(sch, &name, new_name);
        }
        Ok(())
    }

    /// `SCH_EDIT_FRAME::CopyVariant`: the new name trimmed and checked (`ValidateNewVariantName( newName, wxEmptyString )`), then `SCHEMATIC::CopyVariant`: a
    /// variant of that name with the source's description and a copy of every record of it.
    pub(crate) fn copy_design_variant(&mut self, name: &str, new_name: &str) -> Result<(), Vec<CheckResult>> {
        let name = self.existing_variant(name)?;
        let new_name = new_name.trim();
        variants::validate_new_name(&self.variant_names(), new_name, "").map_err(|why| fail("ops_variant_name", new_name, why))?;
        let list = self.variant_list_mut()?;
        let description = list.get(&name).cloned().unwrap_or_default();
        list.insert(new_name.to_string(), description);
        for sch in self.sections_mut() {
            variants::copy_records(sch, &name, new_name);
        }
        Ok(())
    }

    /// The current variant's edit of the symbols `ids` (`SCH_SYMBOL::SetDNP` / `SetExcludedFromBOM` / `SetExcludedFromBoard` / `SetExcludedFromSim` /
    /// `SetExcludedFromPosFiles` and `SetFieldText` with the variant): each flag given and each field text given, on the record of `variant` of every reference,
    /// as `eda_model::variants::set_flag` and `set_field` keep it. The references are the sheet in view's; an annotated one's units on the other sheets get the
    /// same edit, as `SetAttribute` collects them and `SyncOtherUnits` keeps the fields. A field the symbol does not have, `Reference` and an empty or unknown
    /// variant are refused. Nothing that changes nothing is refused: the dialog's OK sends every field.
    pub(crate) fn set_symbol_variant(&mut self, variant: &str, ids: &[String], flags: &VariantFlags, fields: &BTreeMap<String, String>) -> Result<(), Vec<CheckResult>> {
        if variant.is_empty() {
            return Err(fail("ops_no_variant", "variant", "the default variant is the symbol itself: edit the symbol"));
        }
        let variant = self.existing_variant(variant)?;
        if ids.is_empty() {
            return Err(fail("ops_nothing_selected", "variant", "select a symbol first"));
        }
        if fields.contains_key("Reference") {
            return Err(fail("ops_variant_reference", "Reference", "the reference is the same in every design variant"));
        }
        {
            let sch = self.schematic()?;
            for id in ids {
                if !sch.symbols.iter().any(|s| &s.id == id) {
                    return Err(fail("ops_unknown_symbol", id, "no symbol instance with this reference"));
                }
                for name in fields.keys() {
                    let standard = matches!(name.as_str(), "Value" | "Footprint" | "Datasheet");
                    if !standard && !sch.user_fields.get(id).is_some_and(|m| m.contains_key(name)) {
                        return Err(fail("ops_unknown_item", id, format!("the symbol has no field '{name}'")));
                    }
                }
            }
        }
        let model = self.model;
        let here = self.focus.clone();
        let annotated = |id: &str| !id.ends_with('?');
        let screens = std::iter::once((None, self.design.schematic.as_mut())).chain(self.design.sheet_contents.iter_mut().flat_map(|c| c.iter_mut()).map(|(k, v)| (Some(k.clone()), Some(v))));
        for (key, sch) in screens {
            let Some(sch) = sch else { continue };
            let in_view = key == here;
            for id in ids.iter().filter(|id| in_view || annotated(id)) {
                let Some(sym) = sch.symbols.iter().find(|s| &s.id == id).cloned() else { continue };
                for (flag, value) in flags.each() {
                    if let Some(v) = value {
                        variants::set_flag(sch, id, &variant, flag, v);
                    }
                }
                for (name, text) in fields {
                    let own = match name.as_str() {
                        "Value" if sym.value.is_empty() => model.part(id).and_then(|p| p.value.clone()).unwrap_or_default(),
                        "Value" => sym.value.clone(),
                        "Footprint" => sym.footprint.clone(),
                        "Datasheet" => sym.datasheet.clone(),
                        other => sch.user_fields.get(id).and_then(|m| m.get(other)).cloned().unwrap_or_default(),
                    };
                    variants::set_field(sch, id, &variant, name, text, &own);
                }
            }
        }
        Ok(())
    }
}
