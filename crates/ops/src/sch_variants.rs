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
//! **Differs**: an empty description keeps the variant in the list. KiCad 25ec2335 drops it at the next save and reopen when no symbol differs in it:
//! `SCHEMATIC::SetVariantDescription` (`schematic.cpp:3190-3198`) does `if( aDescription.IsEmpty() ) descriptions.erase( aVariantName );`, the project file is
//! written from that map alone (`schematic_settings.cpp:274-290`), the schematic writes no record without differentials (`sch_io_kicad_sexpr.cpp:1073-1076`),
//! and `SCHEMATIC::LoadVariants` (`schematic.cpp:3201-3223`) knows only those two sources.

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

    /// Sheet Properties' Attributes in the current variant: see [`SchCmd::SetSheetVariant`](crate::sch_edit::SchCmd::SetSheetVariant).
    pub(crate) fn set_sheet_variant(&mut self, id: &str, variant: &str, flags: &VariantFlags) -> Result<(), Vec<CheckResult>> {
        if variant.is_empty() {
            return Err(fail("ops_no_variant", "variant", "the default variant is the sheet itself: set_sheet_props"));
        }
        let variant = self.existing_variant(variant)?;
        let sch = self.schematic_mut()?;
        if !sch.sheets.iter().any(|s| s.id == id) {
            return Err(fail("ops_unknown_sheet", id, "no sheet with this id"));
        }
        if sch.extras.is_locked(id) {
            return Err(fail("ops_locked", id, "the sheet is locked"));
        }
        let mut changed = false;
        for (flag, value) in flags.each() {
            if let Some(v) = value {
                changed |= variants::set_sheet_flag(sch, id, &variant, flag, v);
            }
        }
        if !changed {
            return Err(fail("ops_nothing_to_change", id, "the sheet already has these attributes in this variant"));
        }
        Ok(())
    }

    /// The current variant's edit of the symbols `ids` (`SCH_SYMBOL::SetDNP` / `SetExcludedFromBOM` / `SetExcludedFromBoard` / `SetExcludedFromSim` /
    /// `SetExcludedFromPosFiles` and `SetFieldText` with the variant): each flag given and each field text given, on the record of `variant` of every reference,
    /// as `eda_model::variants::set_flag_on` and `set_field_on` keep it. The references are looked for on every sheet (the Fields Table covers the design; a
    /// later sheet path's reference is its screen's symbol on that path); an annotated one's units on every sheet get the same edit, as `SetAttribute` collects
    /// them and `SyncOtherUnits` keeps the fields, an unannotated one only the sheet in view's. A record is an instance's, one per sheet path
    /// (`SCH_SYMBOL::getInstance( aPath )`, UI-LEFT-2 SS16): on a screen shown on several paths, the edit is that of the path in view for the screen in view, of
    /// the path a later path's reference names, and of the symbol's own path (its id's) elsewhere. A field text equal to what the variant shows without one --
    /// the symbol's own, or its variant symbol's (`SetFieldText`: "Diff against the value the variant resolves to without an explicit override") -- takes the
    /// record's text away. A field the symbol does not have, `Reference` and an empty or unknown variant are refused. Nothing that changes nothing is refused:
    /// the dialog's OK sends every field.
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
        // The Symbol Fields Table covers the whole design (`fields_table::project_view`), so a reference is looked for on every sheet, and the row of a later
        // sheet path stands for the screen's symbol on that path (`fields_table::path_alias_paths`).
        let aliases = crate::fields_table::path_alias_paths(&self.design);
        let targets: Vec<(String, Option<Vec<String>>)> = ids.iter().map(|id| aliases.get(id).map_or((id.clone(), None), |(own, path)| (own.clone(), Some(path.clone())))).collect();
        self.schematic()?;
        for (id, _) in &targets {
            let homes: Vec<&eda_model::ir::SchematicSection> = self.design.schematic.iter().chain(self.design.sheet_contents.iter().flat_map(|c| c.values())).filter(|s| s.symbols.iter().any(|x| &x.id == id)).collect();
            if homes.is_empty() {
                return Err(fail("ops_unknown_symbol", id, "no symbol instance with this reference"));
            }
            for name in fields.keys() {
                let standard = matches!(name.as_str(), "Value" | "Footprint" | "Datasheet");
                if !standard && !homes.iter().any(|h| h.user_fields.get(id).is_some_and(|m| m.contains_key(name))) {
                    return Err(fail("ops_unknown_item", id, format!("the symbol has no field '{name}'")));
                }
            }
        }
        let paths_of = eda_model::sheet_list::paths_of_screens(&self.design);
        let focus_path = self.focus_sheet_path();
        let here = self.focus.clone();
        let annotated = |id: &str| !id.ends_with('?');
        // what each edit is to: (screen key, reference, the sheet path of the instance, or none for the reference's own records)
        let mut edits: Vec<(Option<String>, String, Option<Vec<String>>)> = Vec::new();
        let screens: Vec<(Option<String>, &eda_model::ir::SchematicSection)> =
            self.design.schematic.iter().map(|s| (None, s)).chain(self.design.sheet_contents.iter().flat_map(|c| c.iter()).map(|(k, v)| (Some(k.clone()), v))).collect();
        for (key, sch) in &screens {
            let in_view = *key == here;
            let paths = paths_of.get(key.as_deref().unwrap_or("")).cloned().unwrap_or_default();
            for (id, alias_path) in &targets {
                let Some(sym) = sch.symbols.iter().find(|s| &s.id == id) else { continue };
                let path = match alias_path {
                    // a later path's row: that path of its own screen only
                    Some(p) if paths.contains(p) => Some(p.clone()),
                    Some(_) => continue,
                    None if !(in_view || annotated(id)) => continue,
                    None if paths.len() <= 1 => None,
                    None if in_view && paths.contains(&focus_path) => Some(focus_path.clone()),
                    None => eda_model::sheet_list::own_path(sch, sym, &paths).cloned(),
                };
                edits.push((key.clone(), id.clone(), path));
            }
        }
        for (key, id, path) in edits {
            let own_texts: Vec<(String, String)> = {
                let sch = self.section_of_key(key.as_deref()).expect("a screen just listed");
                let Some(sym) = sch.symbols.iter().find(|s| s.id == id).cloned() else { continue };
                let alt = self.variant_lib_symbol(sch, &sym, path.as_deref().unwrap_or(&[]), &variant);
                fields
                    .keys()
                    .map(|name| {
                        let own = match name.as_str() {
                            "Value" if sym.value.is_empty() => self.model.part(&id).and_then(|p| p.value.clone()).unwrap_or_default(),
                            "Value" => sym.value.clone(),
                            "Footprint" => sym.footprint.clone(),
                            "Datasheet" => sym.datasheet.clone(),
                            other => sch.user_fields.get(&id).and_then(|m| m.get(other)).cloned().unwrap_or_default(),
                        };
                        let shown = alt.as_ref().and_then(|a| eda_model::variant_symbol::field_of(a, name)).map(str::to_string).unwrap_or(own);
                        (name.clone(), shown)
                    })
                    .collect()
            };
            let sch = self.section_of_key_mut(key.as_deref()).expect("a screen just listed");
            for (flag, value) in flags.each() {
                if let Some(v) = value {
                    variants::set_flag_on(sch, &id, path.as_deref(), &variant, flag, v);
                }
            }
            for (name, own) in own_texts {
                variants::set_field_on(sch, &id, path.as_deref(), &variant, &name, &fields[&name], &own);
            }
        }
        Ok(())
    }

    /// The section a screen key names: the root's for `None`, else the sheet screen's.
    fn section_of_key(&self, key: Option<&str>) -> Option<&eda_model::ir::SchematicSection> {
        match key {
            None => self.design.schematic.as_ref(),
            Some(k) => self.design.sheet_contents.as_ref()?.get(k),
        }
    }

    fn section_of_key_mut(&mut self, key: Option<&str>) -> Option<&mut eda_model::ir::SchematicSection> {
        match key {
            None => self.design.schematic.as_mut(),
            Some(k) => self.design.sheet_contents.as_mut()?.get_mut(k),
        }
    }

    /// `SCH_SYMBOL::GetVariantLibSymbol( variant, path )` of `sym` on `sch`: the library symbol its record of `variant` on the sheet at `path` names, when the
    /// design can resolve it and it is pin-compatible with the symbol's own ([`eda_model::variant_symbol::resolve_override`]).
    pub(crate) fn variant_lib_symbol(&self, sch: &eda_model::ir::SchematicSection, sym: &eda_model::ir::SymbolInstance, path: &[String], variant: &str) -> Option<eda_model::LibSymbol> {
        if variant.is_empty() {
            return None;
        }
        let over = variants::record_on(sch, &sym.id, path, variant)?.symbol_override.as_deref()?;
        let base = self.design_symbol(&sym.lib_id);
        eda_model::variant_symbol::resolve_override(base.as_ref(), &sym.lib_id, over, |id| self.design_symbol(id))
    }

    /// `SCH_EDIT_TOOL::SetVariantSymbol` (`eeschema.InteractiveEdit.setVariantSymbol`, "Set Variant Symbol...") with `lib_id`, and `ClearVariantSymbol`
    /// (`clearVariantSymbol`, "Clear Variant Symbol") without: the symbol `id` (each of its units, as an edit of a reference is) is drawn as `lib_id` in the
    /// current `variant`, on the sheet path in view (`SetVariantSymbolOverride( currentSheet, variantName, picked.LibId )`). The default variant does
    /// nothing in KiCad (`if( variantName.IsEmpty() ) return 0;`) and is refused here. A symbol the libraries do not have is KiCad's "Could not load symbol
    /// '%s' from library."; one that is not pin-compatible with the symbol's own is refused with KiCad's message (`ValidateVariantSymbolCompatibility`, the
    /// "Variant Symbol Compatibility" box); clearing a record with no variant symbol changes nothing (`if( !existingVariant ||
    /// !existingVariant->m_SymbolOverride.has_value() ) return 0;`).
    pub(crate) fn set_variant_symbol(&mut self, variant: &str, id: &str, lib_id: Option<&str>) -> Result<(), Vec<CheckResult>> {
        if variant.is_empty() {
            return Err(fail("ops_no_variant", "variant", "Set Variant Symbol needs a design variant to be current"));
        }
        let variant = self.existing_variant(variant)?;
        let file = self.focus.clone().unwrap_or_default();
        let paths = eda_model::sheet_list::paths_of_screens(&self.design).remove(&file).unwrap_or_default();
        let path = (paths.len() > 1).then(|| self.focus_sheet_path());
        let sch = self.schematic()?;
        let Some(sym) = sch.symbols.iter().find(|s| s.id == id).cloned() else {
            return Err(fail("ops_unknown_symbol", id, "no symbol with this reference on the sheet"));
        };
        if let Some(lib_id) = lib_id {
            let Some(candidate) = self.design_symbol(lib_id) else {
                return Err(fail("ops_unknown_lib_symbol", lib_id, format!("Could not load symbol '{lib_id}' from library.")));
            };
            if let Some(base) = self.design_symbol(&sym.lib_id) {
                let issues = eda_model::variant_symbol::validate_compatibility(&base, &candidate);
                if !issues.is_empty() {
                    return Err(fail("ops_variant_symbol_incompatible", lib_id, eda_model::variant_symbol::incompatible_message(lib_id, &sym.lib_id, &issues)));
                }
            }
        }
        let sch = self.schematic_mut()?;
        let changed = variants::set_symbol_override_on(sch, id, path.as_deref(), &variant, lib_id);
        if !changed {
            return Err(fail("ops_nothing_to_change", id, if lib_id.is_some() { "the symbol already has this variant symbol" } else { "the symbol has no variant symbol in this variant" }));
        }
        Ok(())
    }
}
