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
//! - No variant symbol (`SCH_SYMBOL_VARIANT::m_SymbolOverride`, Set Variant Symbol...) and no per-variant pin-to-pad map (`m_PinMapOverride`). A sheet's own
//!   variants (`SCH_SHEET_VARIANT`) are its flags only ([`crate::sch_extras::SchExtras::sheet_variants`], [`sheet_flags`], [`sheet_path_flags`]; UI-LEFT-2 SS15),
//!   not its field texts.

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
    /// The variant's own symbol (KiCad 25ec2335 `SCH_SYMBOL_VARIANT::m_SymbolOverride`, Set Variant Symbol...): the library id of another library symbol,
    /// pin-compatible with the symbol's own ([`crate::variant_symbol::validate_compatibility`]), that the symbol is drawn as and takes its empty-override
    /// field texts from while the variant is current (`SCH_SYMBOL::GetVariantLibSymbol`, `GetEffectiveLibSymbol`). Written as `(symbol_override "<lib id>")`
    /// in the record (`saveSymbol`) and read back (`parseSymbolOverride`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_override: Option<String>,
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
            symbol_override: None,
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

    /// `SCH_SYMBOL_VARIANT::HasDifferentials`: a flag other than the symbol's, a field text or a variant symbol (`m_SymbolOverride.has_value()`). A record
    /// without any "resolves identically to no variant at all", and the writer leaves it out (`saveSymbol`: "writing it only keeps deleted variants alive
    /// across sessions").
    pub fn has_differentials(&self, base: SymbolFlags) -> bool {
        self.flags(base) != base || !self.fields.is_empty() || self.symbol_override.is_some()
    }
}

/// The records of every variant reference `id` has on the sheet at `path` (`SCH_SYMBOL_INSTANCE::m_Variants` of the instance of that path): the path's own
/// set ([`crate::sch_extras::SchExtras::path_symbol_variants`]) where the screen is shown on several paths and that path's records differ, else the
/// reference's ([`crate::sch_extras::SchExtras::symbol_variants`]). The root (`[]`) and a screen shown once have the reference's.
pub fn records_on<'a>(sch: &'a SchematicSection, id: &str, path: &[String]) -> Option<&'a BTreeMap<String, SymbolVariant>> {
    if !path.is_empty() {
        if let Some(m) = sch.extras.path_symbol_variants.get(id).and_then(|p| p.get(&path.join("/"))) {
            return Some(m);
        }
    }
    sch.extras.symbol_variants.get(id)
}

/// The record `variant` of reference `id` on `sch`, if it has one (the reference's own records: see [`record_on`] for a sheet path's).
pub fn record<'a>(sch: &'a SchematicSection, id: &str, variant: &str) -> Option<&'a SymbolVariant> {
    sch.extras.symbol_variants.get(id)?.get(variant)
}

/// The record `variant` of reference `id` on the sheet at `path` (`SCH_SYMBOL::GetVariant( aPath, aVariantName )`), if it has one.
pub fn record_on<'a>(sch: &'a SchematicSection, id: &str, path: &[String], variant: &str) -> Option<&'a SymbolVariant> {
    records_on(sch, id, path)?.get(variant)
}

/// The flags of `sym` in `variant` (`SCH_SYMBOL::GetDNP`, `GetExcludedFromBOM`, `GetExcludedFromBoard`, `GetExcludedFromSim`, `GetExcludedFromPosFiles` with
/// the variant): the symbol's own for the default variant (`""`) or a variant it has no record of.
pub fn resolved_flags(sch: &SchematicSection, sym: &SymbolInstance, variant: &str) -> SymbolFlags {
    resolved_flags_on(sch, sym, &[], variant)
}

/// [`resolved_flags`] on the sheet at `path` (`SCH_SYMBOL::GetDNP( aInstance, aVariantName )`: the instance of that path's record).
pub fn resolved_flags_on(sch: &SchematicSection, sym: &SymbolInstance, path: &[String], variant: &str) -> SymbolFlags {
    let base = SymbolFlags::of(sch, sym);
    if variant.is_empty() {
        return base;
    }
    record_on(sch, &sym.id, path, variant).map_or(base, |r| r.flags(base))
}

/// A field's text in `variant` (`SCH_SYMBOL::GetFieldText( aField, .., aPath, aVariantName )`): the record's text of it, else `own` (the symbol's text).
/// `Reference` is never a variant's.
pub fn field_text<'a>(sch: &'a SchematicSection, id: &str, variant: &str, field: &str, own: &'a str) -> &'a str {
    if variant.is_empty() || field == "Reference" {
        return own;
    }
    record(sch, id, variant).and_then(|r| r.fields.get(field)).map_or(own, String::as_str)
}

/// Edit the records reference `id` has on the sheet at `path` with `f` (given the records and the symbol's own flags; it returns whether anything changed).
/// `path` `None` (or the root's `[]`) edits the reference's own records, which every sheet path without a set of its own reads; a path of a screen shown on
/// several paths edits that path's alone (`SCH_SYMBOL::getInstance( aPath )`): its set is made from the records it reads now, edited, and dropped again
/// when it is the reference's ([`crate::sch_extras::SchExtras::path_symbol_variants`]).
fn edit_records(sch: &mut SchematicSection, id: &str, path: Option<&[String]>, f: impl FnOnce(&mut BTreeMap<String, SymbolVariant>, SymbolFlags) -> bool) -> bool {
    let Some(sym) = sch.symbols.iter().find(|s| s.id == id) else { return false };
    let base = SymbolFlags::of(sch, sym);
    let changed = match path.filter(|p| !p.is_empty()) {
        Some(p) => {
            let key = p.join("/");
            let mut set = records_on(sch, id, p).cloned().unwrap_or_default();
            let changed = f(&mut set, base);
            sch.extras.path_symbol_variants.entry(id.to_string()).or_default().insert(key, set);
            changed
        }
        None => f(sch.extras.symbol_variants.entry(id.to_string()).or_default(), base),
    };
    tidy_records(sch, id);
    changed
}

/// Drop the empty set of records of `id` and every path's set that is the reference's own.
fn tidy_records(sch: &mut SchematicSection, id: &str) {
    if sch.extras.symbol_variants.get(id).is_some_and(BTreeMap::is_empty) {
        sch.extras.symbol_variants.remove(id);
    }
    let own = sch.extras.symbol_variants.get(id).cloned().unwrap_or_default();
    if let Some(paths) = sch.extras.path_symbol_variants.get_mut(id) {
        paths.retain(|_, set| *set != own);
        if paths.is_empty() {
            sch.extras.path_symbol_variants.remove(id);
        }
    }
}

/// `SCH_SYMBOL::setVariantAttribute` for reference `id`: with no record of `variant`, a value equal to the symbol's own changes nothing and another one makes
/// the record (`InitializeAttributes`, then the flag); with a record, the record's flag is set. Returns whether anything changed.
pub fn set_flag(sch: &mut SchematicSection, id: &str, variant: &str, flag: Flag, value: bool) -> bool {
    set_flag_on(sch, id, None, variant, flag, value)
}

/// [`set_flag`] on the sheet at `path` (see [`edit_records`]).
pub fn set_flag_on(sch: &mut SchematicSection, id: &str, path: Option<&[String]>, variant: &str, flag: Flag, value: bool) -> bool {
    edit_records(sch, id, path, |records, base| match records.get_mut(variant) {
        Some(rec) => std::mem::replace(rec.flag_mut(flag), Some(value)) != Some(value),
        None if value == base.get(flag) => false,
        None => {
            let mut rec = SymbolVariant::initialized(base);
            *rec.flag_mut(flag) = Some(value);
            records.insert(variant.to_string(), rec);
            true
        }
    })
}

/// `SCH_SYMBOL::SetFieldText( aField, aFieldName, aFieldText, aPath, aVariantName )` for reference `id`: a text equal to the symbol's own (`own`) takes the
/// record's text of the field away; another one is the record's text of it, the record made first (`InitializeAttributes`) when there is none. Returns
/// whether anything changed.
pub fn set_field(sch: &mut SchematicSection, id: &str, variant: &str, field: &str, text: &str, own: &str) -> bool {
    set_field_on(sch, id, None, variant, field, text, own)
}

/// [`set_field`] on the sheet at `path` (see [`edit_records`]). `own` is the text the variant shows without a text of its own: the symbol's, or the
/// variant symbol's non-empty one (`SetFieldText`: "Diff against the value the variant resolves to without an explicit override").
pub fn set_field_on(sch: &mut SchematicSection, id: &str, path: Option<&[String]>, variant: &str, field: &str, text: &str, own: &str) -> bool {
    edit_records(sch, id, path, |records, base| match records.get_mut(variant) {
        Some(rec) if text != own => rec.fields.insert(field.to_string(), text.to_string()).as_deref() != Some(text),
        Some(rec) => rec.fields.remove(field).is_some(),
        None if text == own => false,
        None => {
            let mut rec = SymbolVariant::initialized(base);
            rec.fields.insert(field.to_string(), text.to_string());
            records.insert(variant.to_string(), rec);
            true
        }
    })
}

/// `SCH_SYMBOL::SetVariantSymbolOverride( aPath, aVariantName, aLibId )` (`lib_id` `Some`) and `ClearVariantSymbolOverride` (`None`) for reference `id` on the
/// sheet at `path`: setting makes the record when there is none (`InitializeAttributes`); clearing a record with no variant symbol changes nothing. Returns
/// whether anything changed.
pub fn set_symbol_override_on(sch: &mut SchematicSection, id: &str, path: Option<&[String]>, variant: &str, lib_id: Option<&str>) -> bool {
    edit_records(sch, id, path, |records, base| match (records.get_mut(variant), lib_id) {
        (Some(rec), Some(l)) => rec.symbol_override.replace(l.to_string()).as_deref() != Some(l),
        (Some(rec), None) => rec.symbol_override.take().is_some(),
        (None, Some(l)) => {
            let mut rec = SymbolVariant::initialized(base);
            rec.symbol_override = Some(l.to_string());
            records.insert(variant.to_string(), rec);
            true
        }
        (None, None) => false,
    })
}

/// Every set of records of the section: the references' own and each sheet path's.
fn record_sets_mut(sch: &mut SchematicSection) -> impl Iterator<Item = &mut BTreeMap<String, SymbolVariant>> {
    sch.extras.symbol_variants.values_mut().chain(sch.extras.path_symbol_variants.values_mut().flat_map(|p| p.values_mut()))
}

/// [`tidy_records`] for every reference of the section.
fn tidy_all(sch: &mut SchematicSection) {
    let ids: Vec<String> = sch.extras.symbol_variants.keys().chain(sch.extras.path_symbol_variants.keys()).cloned().collect();
    for id in ids {
        tidy_records(sch, &id);
    }
}

/// `SCH_SCREEN::DeleteVariant` (`SCH_SYMBOL::DeleteVariant` on every instance, and the sheets' `SCH_SHEET::DeleteVariant`): every record of `variant` on the section goes.
pub fn delete_records(sch: &mut SchematicSection, variant: &str) {
    for records in record_sets_mut(sch) {
        records.remove(variant);
    }
    tidy_all(sch);
    for records in sch.extras.sheet_variants.values_mut() {
        records.remove(variant);
    }
    sch.extras.sheet_variants.retain(|_, r| !r.is_empty());
}

/// `SCH_SCREEN::RenameVariant` (`SCH_SYMBOL::RenameVariant`): every record of `old` is a record of `new`.
pub fn rename_records(sch: &mut SchematicSection, old: &str, new: &str) {
    if old == new {
        return;
    }
    for records in record_sets_mut(sch) {
        if let Some(r) = records.remove(old) {
            records.insert(new.to_string(), r);
        }
    }
    for records in sch.extras.sheet_variants.values_mut() {
        if let Some(r) = records.remove(old) {
            records.insert(new.to_string(), r);
        }
    }
}

/// `SCH_SCREEN::CopyVariant` (`SCH_SYMBOL::CopyVariant`): every record of `source` is also one of `new` (a record `new` has already stays, as
/// `m_Variants.insert` keeps it).
pub fn copy_records(sch: &mut SchematicSection, source: &str, new: &str) {
    for records in record_sets_mut(sch) {
        if let Some(r) = records.get(source).cloned() {
            records.entry(new.to_string()).or_insert(r);
        }
    }
    for records in sch.extras.sheet_variants.values_mut() {
        if let Some(r) = records.get(source).copied() {
            records.entry(new.to_string()).or_insert(r);
        }
    }
}

/// `SCH_SHEET::GetDNP( &instance, variant )`, `GetExcludedFromSim`, `GetExcludedFromBOM`, `GetExcludedFromBoard`: the flags of the sheet `id` of the section, in
/// `variant` when it has a record of it (`instance.m_Variants.contains( aVariantName )`), else its own (`m_DNP`, ...); the default variant (`""`) is its own.
pub fn sheet_flags(sch: &SchematicSection, id: &str, variant: &str) -> crate::sch_extras::SheetVariant {
    let base = sch.extras.sheet_props_of(id);
    if !variant.is_empty() {
        if let Some(r) = sch.extras.sheet_variants.get(id).and_then(|m| m.get(variant)) {
            return *r;
        }
    }
    crate::sch_extras::SheetVariant::initialized(&base)
}

/// `SCH_SHEET::SetDNP( value, &instance, variant )` and its kin for one flag: a record made for the variant starts as the sheet's own flags
/// (`variant.InitializeAttributes( *this )`); a record left with no differential is not kept. Returns whether anything changed.
pub fn set_sheet_flag(sch: &mut SchematicSection, id: &str, variant: &str, flag: Flag, value: bool) -> bool {
    let base = sch.extras.sheet_props_of(id);
    let before = sch.extras.sheet_variants.get(id).and_then(|m| m.get(variant)).copied();
    let mut rec = before.unwrap_or_else(|| crate::sch_extras::SheetVariant::initialized(&base));
    match flag {
        Flag::Dnp => rec.dnp = value,
        Flag::ExcludeFromSim => rec.exclude_from_sim = value,
        Flag::ExcludeFromBom => rec.exclude_from_bom = value,
        Flag::ExcludeFromBoard => rec.exclude_from_board = value,
        Flag::ExcludeFromPosFiles => return false,
    }
    let after = rec.has_differentials(&base).then_some(rec);
    if after == before {
        return false;
    }
    match after {
        Some(r) => {
            sch.extras.sheet_variants.entry(id.to_string()).or_default().insert(variant.to_string(), r);
        }
        None => {
            if let Some(m) = sch.extras.sheet_variants.get_mut(id) {
                m.remove(variant);
                if m.is_empty() {
                    sch.extras.sheet_variants.remove(id);
                }
            }
        }
    }
    true
}

/// `SCH_SHEET_PATH::GetDNP( aVariantName )`, `GetExcludedFromSim`, `GetExcludedFromBOM`, `GetExcludedFromBoard` (8303b2ad `sch_sheet_path.cpp`): a flag is set for
/// a sheet path when any sheet of it has it set, in `variant` (each sheet asked on the path above it: `sheet->GetDNP( &copy, aVariantName )`), the default variant
/// being each sheet's own. `path` is the model's path of placement ids from the root; the root itself has no flags. The four flags in a `SymbolFlags` (no
/// position-files flag: a sheet has none).
pub fn sheet_path_flags(design: &crate::ir::Design, path: &[String], variant: &str) -> SymbolFlags {
    let mut out = SymbolFlags::default();
    for k in 1..=path.len() {
        let Some(screen) = crate::sheet_list::screen_of(design, &path[..k - 1]) else { continue };
        let f = sheet_flags(screen, &path[k - 1], variant);
        out.dnp |= f.dnp;
        out.exclude_from_sim |= f.exclude_from_sim;
        out.exclude_from_bom |= f.exclude_from_bom;
        out.exclude_from_board |= f.exclude_from_board;
    }
    out
}

/// A reference renamed: its records follow it (the variants live in the symbol's instances, so they go wherever the symbol goes).
pub fn rename_reference(sch: &mut SchematicSection, old: &str, new: &str) {
    if old == new {
        return;
    }
    if let Some(r) = sch.extras.symbol_variants.remove(old) {
        sch.extras.symbol_variants.insert(new.to_string(), r);
    }
    if let Some(r) = sch.extras.path_symbol_variants.remove(old) {
        sch.extras.path_symbol_variants.insert(new.to_string(), r);
    }
}

/// A user field removed from a symbol (`SCH_SYMBOL::RemoveField`: "This will remove it from all variants"), or renamed (the variants' text of it is
/// under the new name): `new` `None` removes it.
pub fn rename_field(sch: &mut SchematicSection, id: Option<&str>, old: &str, new: Option<&str>) {
    let own = sch.extras.symbol_variants.iter_mut().map(|(r, m)| (r.as_str(), m));
    let paths = sch.extras.path_symbol_variants.iter_mut().flat_map(|(r, p)| p.values_mut().map(move |m| (r.as_str(), m)));
    for (reference, records) in own.chain(paths) {
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

/// The symbol a reference names, the section it is on and the sheet path of that instance (the netlist's component for the reference): a later path's
/// reference of a screen shown twice is its screen's symbol on that path (`crate::sheet_list::reference_under`); a symbol's own id is the first path its id
/// is the reference on. `None` for a reference no symbol has.
pub fn instance_of<'a>(design: &'a crate::ir::Design, reference: &str) -> Option<(&'a SchematicSection, &'a SymbolInstance, Vec<String>)> {
    for e in crate::sheet_list::walk(design) {
        let Some(sch) = crate::sheet_list::screen_of(design, &e.path) else { continue };
        if let Some(sym) = sch.symbols.iter().find(|s| crate::sheet_list::reference_under(sch, s, &e.path) == reference) {
            return Some((sch, sym, e.path));
        }
    }
    sections(design).find_map(|sch| sch.symbols.iter().find(|s| s.id == reference).map(|s| (sch, s, Vec::new())))
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
        out.extend(sch.extras.path_symbol_variants.values().flat_map(|p| p.values()).flat_map(|r| r.keys().cloned()));
        out.extend(sch.extras.sheet_variants.values().flat_map(|r| r.keys().cloned()));
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
    written_records_on(sch, sym, &[])
}

/// [`written_records`] of the instance on the sheet at `path` (each `(path ..)` of the symbol's `(instances ..)` writes its own instance's records).
pub fn written_records_on<'a>(sch: &'a SchematicSection, sym: &SymbolInstance, path: &[String]) -> Vec<(&'a str, &'a SymbolVariant)> {
    let base = SymbolFlags::of(sch, sym);
    records_on(sch, &sym.id, path).into_iter().flatten().filter(|(_, r)| r.has_differentials(base)).map(|(n, r)| (n.as_str(), r)).collect()
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

    /// KiCad keeps a record per instance (`SCH_SYMBOL_INSTANCE::m_Variants`): on a screen shown on two paths, an edit made on one path is that path's alone, and
    /// a path whose records come back to the reference's own keeps no set of its own (UI-LEFT-2 SS16).
    #[test]
    fn a_record_made_on_one_sheet_path_is_that_paths_alone() {
        let mut s = sheet();
        let (a, b) = (vec!["sa".to_string()], vec!["sb".to_string()]);
        assert!(set_field_on(&mut s, "R1", Some(&b), "V1", "Value", "4k7", "10k"));
        assert_eq!(record_on(&s, "R1", &b, "V1").unwrap().fields["Value"], "4k7");
        assert!(record_on(&s, "R1", &a, "V1").is_none(), "the other path reads the reference's records, which have none");
        assert!(set_flag_on(&mut s, "R1", Some(&a), "V1", Flag::Dnp, true));
        assert!(resolved_flags_on(&s, &s.symbols[0], &a, "V1").dnp);
        assert!(!resolved_flags_on(&s, &s.symbols[0], &b, "V1").dnp);
        assert!(set_symbol_override_on(&mut s, "R1", Some(&b), "V1", Some("Device:R_Small")));
        assert_eq!(record_on(&s, "R1", &b, "V1").unwrap().symbol_override.as_deref(), Some("Device:R_Small"));
        assert!(written_records_on(&s, &s.symbols[0], &b).len() == 1 && written_records_on(&s, &s.symbols[0], &a).len() == 1);
        // the path's set back to what the reference has: no set of its own
        assert!(set_symbol_override_on(&mut s, "R1", Some(&b), "V1", None));
        assert!(set_field_on(&mut s, "R1", Some(&b), "V1", "Value", "10k", "10k"));
        assert!(set_flag_on(&mut s, "R1", Some(&b), "V1", Flag::Dnp, true));
        rename_records(&mut s, "V1", "V2");
        assert!(record_on(&s, "R1", &b, "V2").is_some());
        delete_records(&mut s, "V2");
        assert!(s.extras.symbol_variants.is_empty() && s.extras.path_symbol_variants.is_empty());
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
