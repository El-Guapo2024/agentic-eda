//! Design variants (UI-LEFT S2; a KiCad 10 feature newer than 8303b2ad, ported from KiCad 25ec2335): a named set of per-symbol differences from the
//! design -- the Do not populate, Exclude from BOM, Exclude from board, Exclude from simulation and Exclude from position files flags, and field
//! texts -- which the schematic shows while it is the current variant and which kicad-cli's exports honour through `--variant`.
//!
//! Where KiCad keeps them, and where the design does:
//! - The variant list and each one's description are the project's (`SCHEMATIC_SETTINGS::m_VariantDescriptions`, the `.kicad_pro`'s
//!   `schematic.variants`, an array of `{ "name", "description" }` with the description left out when empty: `schematic_settings.cpp`,
//!   `PARAM_LAMBDA<nlohmann::json>( "variants" )`). Here: [`crate::sch_extras::SchExtras::variants`] of the root section.
//! - A placed symbol's record of a variant is in its instance (`SCH_SYMBOL_INSTANCE::m_Variants`, a `SCH_SYMBOL_VARIANT` of `sch_sheet_path.h`), and
//!   the file has it inside the symbol's `(instances (project .. (path .. (variant (name ..) (dnp ..) (exclude_from_sim ..) (in_bom ..) (on_board ..)
//!   (in_pos_files ..) (field (name ..) (value ..))))))` (`SCH_IO_KICAD_SEXPR::saveSymbol`). Here: [`crate::sch_extras::SchExtras::symbol_variants`] of
//!   the section the symbol is on, by reference then by variant name.
//! - `SCHEMATIC::m_variantNames` is the project's list and every name a symbol's record carries (`SCHEMATIC::LoadVariants`, `SCH_SCREENS::GetVariantNames`):
//!   [`names`].
//! - The current variant (`SCHEMATIC::m_currentVariant`) is the editor's, not the file's: the studio keeps it in the browser and sends it with the
//!   requests that need it.
//!
//! **Differs** (each said again where it applies):
//! - A record is the reference's, for every unit of it and every sheet path of its screen; KiCad's is a unit's instance on one sheet path. The studio keeps
//!   a reference's units in step anyway (`SCH_EDIT_TOOL::SetAttribute`, `SCH_SYMBOL::SyncOtherUnits`), and a variant edit made on one path of a screen shown
//!   twice is an edit to every path of it (as a field edit is, `eda_ops::fields_table::path_aliases`).
//! - A flag of a record may be `None`: the symbol's own flag. Every record this code makes or reads has all five set, as KiCad's `InitializeAttributes`
//!   snapshot does, so `None` is only ever a hand-written or an agent's record.
//! - No variant symbol (`SCH_SYMBOL_VARIANT::m_SymbolOverride`, Set Variant Symbol...) and no per-variant pin-to-pad map (`m_PinMapOverride`); a sheet's
//!   own variants (`SCH_SHEET_VARIANT`) are not read, as the design keeps no sheet flags.

use crate::ir::{field_key, SchematicSection, SymbolInstance};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `GetDefaultVariantName()` (`common/string_utils.cpp`: `static const wxChar defaultVariantName[] = wxT( "< Default >" )`, not translated): the name the
/// variant selectors show for the design itself. It is no variant's name (`ValidateNewVariantName` refuses it, in any case).
pub const DEFAULT_VARIANT_NAME: &str = "< Default >";

/// One placed symbol's record of one variant (`SCH_SYMBOL_VARIANT`, `VARIANT` of `sch_sheet_path.h`): its flags in that variant and its field texts
/// that differ there. A flag left `None` is the symbol's own; `fields` holds a text by field name (`Value`, `Footprint`, `Datasheet`, a user field; never
/// `Reference`, which "is immutable across variants").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolVariant {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dnp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_bom: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_board: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_sim: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_from_pos_files: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

/// The five flags of `VARIANT` (`m_DNP`, `m_ExcludedFromBOM`, `m_ExcludedFromBoard`, `m_ExcludedFromSim`, `m_ExcludedFromPosFiles`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Flag {
    Dnp,
    ExcludeFromBom,
    ExcludeFromBoard,
    ExcludeFromSim,
    ExcludeFromPosFiles,
}

impl Flag {
    pub const ALL: [Flag; 5] = [Flag::Dnp, Flag::ExcludeFromBom, Flag::ExcludeFromBoard, Flag::ExcludeFromSim, Flag::ExcludeFromPosFiles];
}

/// A symbol's five flags, as it has them or as a variant resolves them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SymbolFlags {
    pub dnp: bool,
    pub exclude_from_bom: bool,
    pub exclude_from_board: bool,
    pub exclude_from_sim: bool,
    pub exclude_from_pos_files: bool,
}

impl SymbolFlags {
    /// The symbol's own flags (`SCH_SYMBOL::m_DNP`, `m_excludedFromBOM`, ...): the instance's four and its placement's "Exclude from position files"
    /// (`SchExtras::excluded_from_pos_files`).
    pub fn of(sch: &SchematicSection, sym: &SymbolInstance) -> SymbolFlags {
        SymbolFlags {
            dnp: sym.dnp,
            exclude_from_bom: sym.exclude_from_bom,
            exclude_from_board: sym.exclude_from_board,
            exclude_from_sim: sym.exclude_from_sim,
            exclude_from_pos_files: sch.extras.excluded_from_pos_files.contains(&field_key(&sym.id, sym.unit)),
        }
    }

    pub fn get(&self, f: Flag) -> bool {
        match f {
            Flag::Dnp => self.dnp,
            Flag::ExcludeFromBom => self.exclude_from_bom,
            Flag::ExcludeFromBoard => self.exclude_from_board,
            Flag::ExcludeFromSim => self.exclude_from_sim,
            Flag::ExcludeFromPosFiles => self.exclude_from_pos_files,
        }
    }
}

impl SymbolVariant {
    /// `SCH_SYMBOL_VARIANT::InitializeAttributes`: a new record starts as a snapshot of the symbol's flags, so a record made for one flag or one field keeps
    /// the others as they were when it was made.
    pub fn initialized(base: SymbolFlags) -> SymbolVariant {
        SymbolVariant {
            dnp: Some(base.dnp),
            exclude_from_bom: Some(base.exclude_from_bom),
            exclude_from_board: Some(base.exclude_from_board),
            exclude_from_sim: Some(base.exclude_from_sim),
            exclude_from_pos_files: Some(base.exclude_from_pos_files),
            fields: BTreeMap::new(),
        }
    }

    pub fn flag(&self, f: Flag) -> Option<bool> {
        match f {
            Flag::Dnp => self.dnp,
            Flag::ExcludeFromBom => self.exclude_from_bom,
            Flag::ExcludeFromBoard => self.exclude_from_board,
            Flag::ExcludeFromSim => self.exclude_from_sim,
            Flag::ExcludeFromPosFiles => self.exclude_from_pos_files,
        }
    }

    pub fn flag_mut(&mut self, f: Flag) -> &mut Option<bool> {
        match f {
            Flag::Dnp => &mut self.dnp,
            Flag::ExcludeFromBom => &mut self.exclude_from_bom,
            Flag::ExcludeFromBoard => &mut self.exclude_from_board,
            Flag::ExcludeFromSim => &mut self.exclude_from_sim,
            Flag::ExcludeFromPosFiles => &mut self.exclude_from_pos_files,
        }
    }

    /// The flags in this variant: the record's, else the symbol's (`SCH_SYMBOL::GetDNP( aInstance, aVariantName )`: `instance.m_Variants[aVariantName].m_DNP`).
    pub fn flags(&self, base: SymbolFlags) -> SymbolFlags {
        let r = |f: Flag| self.flag(f).unwrap_or(base.get(f));
        SymbolFlags { dnp: r(Flag::Dnp), exclude_from_bom: r(Flag::ExcludeFromBom), exclude_from_board: r(Flag::ExcludeFromBoard), exclude_from_sim: r(Flag::ExcludeFromSim), exclude_from_pos_files: r(Flag::ExcludeFromPosFiles) }
    }

    /// `SCH_SYMBOL_VARIANT::HasDifferentials`: a flag other than the symbol's, or a field text. A record without any "resolves identically to no variant at
    /// all", and the writer leaves it out (`saveSymbol`: "writing it only keeps deleted variants alive across sessions").
    pub fn has_differentials(&self, base: SymbolFlags) -> bool {
        self.flags(base) != base || !self.fields.is_empty()
    }
}

/// The record `variant` of reference `id` on `sch`, if it has one.
pub fn record<'a>(sch: &'a SchematicSection, id: &str, variant: &str) -> Option<&'a SymbolVariant> {
    sch.extras.symbol_variants.get(id)?.get(variant)
}

/// The flags of `sym` in `variant` (`SCH_SYMBOL::GetDNP`, `GetExcludedFromBOM`, `GetExcludedFromBoard`, `GetExcludedFromSim`, `GetExcludedFromPosFiles` with
/// the variant): the symbol's own for the default variant (`""`) or a variant it has no record of.
pub fn resolved_flags(sch: &SchematicSection, sym: &SymbolInstance, variant: &str) -> SymbolFlags {
    let base = SymbolFlags::of(sch, sym);
    if variant.is_empty() {
        return base;
    }
    record(sch, &sym.id, variant).map_or(base, |r| r.flags(base))
}

/// A field's text in `variant` (`SCH_SYMBOL::GetFieldText( aField, .., aPath, aVariantName )`): the record's text of it, else `own` (the symbol's text).
/// `Reference` is never a variant's.
pub fn field_text<'a>(sch: &'a SchematicSection, id: &str, variant: &str, field: &str, own: &'a str) -> &'a str {
    if variant.is_empty() || field == "Reference" {
        return own;
    }
    record(sch, id, variant).and_then(|r| r.fields.get(field)).map_or(own, String::as_str)
}

/// `SCH_SYMBOL::setVariantAttribute` for reference `id`: with no record of `variant`, a value equal to the symbol's own changes nothing and another one makes
/// the record (`InitializeAttributes`, then the flag); with a record, the record's flag is set. Returns whether anything changed.
pub fn set_flag(sch: &mut SchematicSection, id: &str, variant: &str, flag: Flag, value: bool) -> bool {
    let Some(sym) = sch.symbols.iter().find(|s| s.id == id) else { return false };
    let base = SymbolFlags::of(sch, sym);
    let records = sch.extras.symbol_variants.entry(id.to_string()).or_default();
    let changed = match records.get_mut(variant) {
        Some(rec) => std::mem::replace(rec.flag_mut(flag), Some(value)) != Some(value),
        None if value == base.get(flag) => false,
        None => {
            let mut rec = SymbolVariant::initialized(base);
            *rec.flag_mut(flag) = Some(value);
            records.insert(variant.to_string(), rec);
            true
        }
    };
    if sch.extras.symbol_variants.get(id).is_some_and(BTreeMap::is_empty) {
        sch.extras.symbol_variants.remove(id);
    }
    changed
}

/// `SCH_SYMBOL::SetFieldText( aField, aFieldName, aFieldText, aPath, aVariantName )` for reference `id`: a text equal to the symbol's own (`own`) takes the
/// record's text of the field away; another one is the record's text of it, the record made first (`InitializeAttributes`) when there is none. Returns
/// whether anything changed.
pub fn set_field(sch: &mut SchematicSection, id: &str, variant: &str, field: &str, text: &str, own: &str) -> bool {
    let Some(sym) = sch.symbols.iter().find(|s| s.id == id) else { return false };
    let base = SymbolFlags::of(sch, sym);
    let records = sch.extras.symbol_variants.entry(id.to_string()).or_default();
    let changed = match records.get_mut(variant) {
        Some(rec) if text != own => rec.fields.insert(field.to_string(), text.to_string()).as_deref() != Some(text),
        Some(rec) => rec.fields.remove(field).is_some(),
        None if text == own => false,
        None => {
            let mut rec = SymbolVariant::initialized(base);
            rec.fields.insert(field.to_string(), text.to_string());
            records.insert(variant.to_string(), rec);
            true
        }
    };
    if sch.extras.symbol_variants.get(id).is_some_and(BTreeMap::is_empty) {
        sch.extras.symbol_variants.remove(id);
    }
    changed
}

/// `SCH_SCREEN::DeleteVariant` (`SCH_SYMBOL::DeleteVariant` on every instance): every record of `variant` on the section goes.
pub fn delete_records(sch: &mut SchematicSection, variant: &str) {
    for records in sch.extras.symbol_variants.values_mut() {
        records.remove(variant);
    }
    sch.extras.symbol_variants.retain(|_, r| !r.is_empty());
}

/// `SCH_SCREEN::RenameVariant` (`SCH_SYMBOL::RenameVariant`): every record of `old` is a record of `new`.
pub fn rename_records(sch: &mut SchematicSection, old: &str, new: &str) {
    if old == new {
        return;
    }
    for records in sch.extras.symbol_variants.values_mut() {
        if let Some(r) = records.remove(old) {
            records.insert(new.to_string(), r);
        }
    }
}

/// `SCH_SCREEN::CopyVariant` (`SCH_SYMBOL::CopyVariant`): every record of `source` is also one of `new` (a record `new` has already stays, as
/// `m_Variants.insert` keeps it).
pub fn copy_records(sch: &mut SchematicSection, source: &str, new: &str) {
    for records in sch.extras.symbol_variants.values_mut() {
        if let Some(r) = records.get(source).cloned() {
            records.entry(new.to_string()).or_insert(r);
        }
    }
}

/// A reference renamed: its records follow it (the variants live in the symbol's instances, so they go wherever the symbol goes).
pub fn rename_reference(sch: &mut SchematicSection, old: &str, new: &str) {
    if old == new {
        return;
    }
    if let Some(r) = sch.extras.symbol_variants.remove(old) {
        sch.extras.symbol_variants.insert(new.to_string(), r);
    }
}

/// A user field removed from a symbol (`SCH_SYMBOL::RemoveField`: "This will remove it from all variants"), or renamed (the variants' text of it is
/// under the new name): `new` `None` removes it.
pub fn rename_field(sch: &mut SchematicSection, id: Option<&str>, old: &str, new: Option<&str>) {
    for (reference, records) in sch.extras.symbol_variants.iter_mut() {
        if id.is_some_and(|i| i != reference) {
            continue;
        }
        for rec in records.values_mut() {
            if let Some(text) = rec.fields.remove(old) {
                if let Some(new) = new {
                    rec.fields.insert(new.to_string(), text);
                }
            }
        }
    }
}

/// Every section of the design: the root's and each sheet screen's.
fn sections(design: &crate::ir::Design) -> impl Iterator<Item = &SchematicSection> {
    design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values()))
}

/// `SCHEMATIC::m_variantNames` (`SCHEMATIC::LoadVariants`): the project's variants (the root's [`crate::sch_extras::SchExtras::variants`]) and every name a
/// symbol's record carries (`SCH_SCREENS::GetVariantNames`), each once, in name order (`std::set<wxString>`).
pub fn names(design: &crate::ir::Design) -> Vec<String> {
    let mut out: std::collections::BTreeSet<String> = design.schematic.iter().flat_map(|s| s.extras.variants.keys().cloned()).collect();
    for sch in sections(design) {
        out.extend(sch.extras.symbol_variants.values().flat_map(|r| r.keys().cloned()));
    }
    out.into_iter().collect()
}

/// `SCHEMATIC::GetVariantNamesForUI`: [`DEFAULT_VARIANT_NAME`] first, then the variants as `SortVariantNames` orders them (`StrNumCmp`).
pub fn names_for_ui(design: &crate::ir::Design) -> Vec<String> {
    let mut v = names(design);
    v.sort_by(|a, b| crate::sheet_list::str_num_cmp(a, b));
    v.insert(0, DEFAULT_VARIANT_NAME.to_string());
    v
}

/// `SCHEMATIC::HasVariant`: a variant of that name, in any case (`CmpNoCase`).
pub fn has_variant(design: &crate::ir::Design, name: &str) -> bool {
    names(design).iter().any(|n| n.to_lowercase() == name.to_lowercase())
}

/// `SCH_EDIT_FRAME::ValidateNewVariantName( aName, aExcludeName )`: the reason, in KiCad's words, a new variant (or a variant renamed from `exclude`) cannot
/// be named `name`, compared without case.
pub fn validate_new_name(existing: &[String], name: &str, exclude: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Variant name cannot be empty.".to_string());
    }
    if name.to_lowercase() == DEFAULT_VARIANT_NAME.to_lowercase() {
        return Err(format!("'{DEFAULT_VARIANT_NAME}' is a reserved variant name."));
    }
    for e in existing {
        if e.to_lowercase() == name.to_lowercase() && e.to_lowercase() != exclude.to_lowercase() {
            return Err(format!("Variant '{e}' already exists."));
        }
    }
    Ok(())
}

/// `sch` as `variant` shows it: each symbol with its flags and its Value, Footprint, Datasheet and user field texts in the variant
/// (`GetDNP( sheet, variant )`, `GetFieldText( .., variant )`). `""` (the default variant) is `sch` itself. What the Symbol Fields Table builds its grid
/// from while it shows a variant (`SYMBOL_FIELDS_EDITOR_GRID_DATA_MODEL::getLiveFieldValueForVariant`).
pub fn resolve_section(sch: &SchematicSection, variant: &str) -> SchematicSection {
    let mut out = sch.clone();
    if variant.is_empty() {
        return out;
    }
    for i in 0..out.symbols.len() {
        let id = out.symbols[i].id.clone();
        let Some(rec) = record(sch, &id, variant).cloned() else { continue };
        let base = SymbolFlags::of(sch, &sch.symbols[i]);
        let flags = rec.flags(base);
        let s = &mut out.symbols[i];
        s.dnp = flags.dnp;
        s.exclude_from_bom = flags.exclude_from_bom;
        s.exclude_from_board = flags.exclude_from_board;
        s.exclude_from_sim = flags.exclude_from_sim;
        let key = field_key(&s.id, s.unit);
        if flags.exclude_from_pos_files {
            out.extras.excluded_from_pos_files.insert(key);
        } else {
            out.extras.excluded_from_pos_files.remove(&key);
        }
        for (name, text) in &rec.fields {
            match name.as_str() {
                "Reference" => {}
                "Value" => out.symbols[i].value = text.clone(),
                "Footprint" => out.symbols[i].footprint = text.clone(),
                "Datasheet" => out.symbols[i].datasheet = text.clone(),
                other => {
                    out.user_fields.entry(id.clone()).or_default().insert(other.to_string(), text.clone());
                }
            }
        }
    }
    out
}

/// The records of every reference on `sch`, as the file writes one symbol's `(variant ..)` blocks: each `(name, record)` with differentials against the
/// symbol's own flags, by name (`std::map<wxString, SCH_SYMBOL_VARIANT>`).
pub fn written_records<'a>(sch: &'a SchematicSection, sym: &SymbolInstance) -> Vec<(&'a str, &'a SymbolVariant)> {
    let base = SymbolFlags::of(sch, sym);
    sch.extras.symbol_variants.get(&sym.id).into_iter().flatten().filter(|(_, r)| r.has_differentials(base)).map(|(n, r)| (n.as_str(), r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Nm, Point};

    fn sym(id: &str) -> SymbolInstance {
        SymbolInstance {
            id: id.into(),
            at: Point { x: Nm::ZERO, y: Nm::ZERO },
            rot: 0,
            mirrored: false,
            mirror_y: false,
            lib_id: String::new(),
            unit: 1,
            value: "10k".into(),
            footprint: String::new(),
            datasheet: String::new(),
            dnp: false,
            exclude_from_bom: false,
            exclude_from_board: false,
            exclude_from_sim: false,
        }
    }

    fn sheet() -> SchematicSection {
        let mut s: SchematicSection = serde_json::from_str(r#"{"symbols": [], "wires": [], "labels": [], "texts": [], "power_symbols": [], "no_connects": []}"#).unwrap();
        s.symbols.push(sym("R1"));
        s
    }

    #[test]
    fn a_flag_equal_to_the_symbols_makes_no_record_and_another_snapshots_the_rest() {
        let mut s = sheet();
        assert!(!set_flag(&mut s, "R1", "V1", Flag::Dnp, false), "setVariantAttribute: `if( aEnable == this->*aBase ) return;`");
        assert!(s.extras.symbol_variants.is_empty());
        assert!(set_flag(&mut s, "R1", "V1", Flag::Dnp, true));
        let rec = record(&s, "R1", "V1").unwrap();
        assert_eq!(rec.dnp, Some(true));
        assert_eq!(rec.exclude_from_bom, Some(false), "InitializeAttributes snapshots the symbol's other flags");
        assert!(resolved_flags(&s, &s.symbols[0], "V1").dnp);
        assert!(!resolved_flags(&s, &s.symbols[0], "").dnp, "the default variant is the symbol's own");
        assert!(!resolved_flags(&s, &s.symbols[0], "V2").dnp, "a variant the symbol has no record of is the symbol's own");
        // the snapshot stays when the symbol's own flag changes: the record still says not excluded
        s.symbols[0].exclude_from_bom = true;
        assert!(!resolved_flags(&s, &s.symbols[0], "V1").exclude_from_bom);
        assert!(written_records(&s, &s.symbols[0]).len() == 1);
    }

    #[test]
    fn a_field_equal_to_the_symbols_own_text_takes_the_override_away() {
        let mut s = sheet();
        assert!(!set_field(&mut s, "R1", "V1", "Value", "10k", "10k"));
        assert!(set_field(&mut s, "R1", "V1", "Value", "4k7", "10k"));
        assert_eq!(field_text(&s, "R1", "V1", "Value", "10k"), "4k7");
        assert_eq!(field_text(&s, "R1", "", "Value", "10k"), "10k");
        assert!(set_field(&mut s, "R1", "V1", "Value", "10k", "10k"));
        assert!(record(&s, "R1", "V1").unwrap().fields.is_empty(), "the record stays, its text of the field goes");
        assert!(written_records(&s, &s.symbols[0]).is_empty(), "a record without differentials is not written");
    }

    #[test]
    fn rename_copy_and_delete_move_every_record() {
        let mut s = sheet();
        set_field(&mut s, "R1", "V1", "Value", "1k", "10k");
        copy_records(&mut s, "V1", "V2");
        rename_records(&mut s, "V1", "V3");
        assert_eq!(s.extras.symbol_variants["R1"].keys().collect::<Vec<_>>(), ["V2", "V3"]);
        delete_records(&mut s, "V2");
        delete_records(&mut s, "V3");
        assert!(s.extras.symbol_variants.is_empty());
    }

    #[test]
    fn new_names_are_checked_as_kicad_checks_them() {
        let existing = vec!["Alpha".to_string()];
        assert_eq!(validate_new_name(&existing, "", "").unwrap_err(), "Variant name cannot be empty.");
        assert_eq!(validate_new_name(&existing, "< default >", "").unwrap_err(), "'< Default >' is a reserved variant name.");
        assert_eq!(validate_new_name(&existing, "ALPHA", "").unwrap_err(), "Variant 'Alpha' already exists.");
        assert!(validate_new_name(&existing, "alpha", "Alpha").is_ok(), "a rename may change the case of its own name");
        assert!(validate_new_name(&existing, "Beta", "").is_ok());
    }

    #[test]
    fn the_resolved_section_carries_the_variants_texts_and_flags() {
        let mut s = sheet();
        set_flag(&mut s, "R1", "V1", Flag::ExcludeFromPosFiles, true);
        set_field(&mut s, "R1", "V1", "Value", "1k", "10k");
        set_field(&mut s, "R1", "V1", "MPN", "RC0603", "");
        let r = resolve_section(&s, "V1");
        assert_eq!(r.symbols[0].value, "1k");
        assert_eq!(r.user_fields["R1"]["MPN"], "RC0603");
        assert!(r.extras.excluded_from_pos_files.contains("R1"));
        assert_eq!(resolve_section(&s, "").symbols[0].value, "10k");
    }
}
