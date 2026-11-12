//! A placed symbol as a design variant shows it, for the studio's views (UI-LEFT-2 SS16, PP16; KiCad 25ec2335): its flags and field texts in the variant
//! on the sheet path of its instance (`SCH_SYMBOL::GetDNP( aPath, aVariantName )` ..., `GetFieldText( .., aPath, aVariantName )`), the field texts its
//! variant symbol gives where the record has none of its own (`GetVariantLibSymbol`), and so what the board's footprint of it shows in the board's current
//! variant (`FOOTPRINT::GetFieldValueForVariant`, `GetDNPForVariant` ..., which the board takes from the schematic by Update PCB from Schematic,
//! `BOARD_NETLIST_UPDATER::applyComponentVariants`).

use std::collections::BTreeMap;

use eda_model::ir::{Design, SchematicSection, SymbolInstance};
use eda_model::variants::SymbolFlags;
use eda_model::{ConstraintModel, LibSymbol};

/// A symbol's instance in a variant.
pub(crate) struct InVariant {
    /// The symbol's own flags and the flags in the variant.
    pub base: SymbolFlags,
    pub flags: SymbolFlags,
    /// The texts the variant gives its fields, by name: the record's, else its variant symbol's non-empty one. A field the variant says nothing of is absent.
    pub fields: BTreeMap<String, String>,
}

/// `SCH_SYMBOL::GetVariantLibSymbol( variant, path )` with the model's library symbols.
pub(crate) fn variant_lib_symbol(model: &ConstraintModel, sch: &SchematicSection, sym: &SymbolInstance, path: &[String], variant: &str) -> Option<LibSymbol> {
    if variant.is_empty() {
        return None;
    }
    let over = eda_model::variants::record_on(sch, &sym.id, path, variant)?.symbol_override.as_deref()?;
    eda_model::variant_symbol::resolve_override(model.symbol_of(&sym.lib_id).as_ref(), &sym.lib_id, over, |id| model.symbol_of(id))
}

/// `sym` of `sch` on the sheet at `path` in `variant`; `None` for the default variant.
pub(crate) fn symbol_in_variant(model: &ConstraintModel, sch: &SchematicSection, sym: &SymbolInstance, path: &[String], variant: &str) -> Option<InVariant> {
    if variant.is_empty() {
        return None;
    }
    let base = SymbolFlags::of(sch, sym);
    let rec = eda_model::variants::record_on(sch, &sym.id, path, variant);
    let flags = rec.map_or(base, |r| r.flags(base));
    let mut fields: BTreeMap<String, String> = rec.map(|r| r.fields.clone()).unwrap_or_default();
    fields.remove("Reference");
    if let Some(alt) = variant_lib_symbol(model, sch, sym, path, variant) {
        let mut names: Vec<String> = ["Value", "Footprint", "Datasheet", "Description"].iter().map(|n| n.to_string()).collect();
        names.extend(sch.user_fields.get(&sym.id).into_iter().flat_map(|m| m.keys().cloned()));
        for n in names {
            if !fields.contains_key(&n) {
                if let Some(t) = eda_model::variant_symbol::field_of(&alt, &n) {
                    fields.insert(n, t.to_string());
                }
            }
        }
    }
    Some(InVariant { base, flags, fields })
}

/// [`symbol_in_variant`] of the instance a reference names (`eda_model::variants::instance_of`): what the board's footprint of that reference shows.
pub(crate) fn reference_in_variant(design: &Design, model: &ConstraintModel, reference: &str, variant: &str) -> Option<InVariant> {
    let (sch, sym, path) = eda_model::variants::instance_of(design, reference)?;
    symbol_in_variant(model, sch, sym, &path, variant)
}

/// The variant's description (`GetVariantDescription`), "" for none.
pub(crate) fn description(design: &Design, variant: &str) -> String {
    design.schematic.as_ref().and_then(|s| s.extras.variants.get(variant)).cloned().unwrap_or_default()
}

/// The `variant` a request names (`?variant=<name>`, percent-encoded), "" for the default; a name the design has no variant of is the default
/// (`SCHEMATIC::SetCurrentVariant`, `BOARD::SetCurrentVariant`: only a name it has).
pub(crate) fn of_request(design: &Design, target: &str) -> String {
    let query = target.split('?').nth(1).unwrap_or("");
    let asked = query.split('&').find_map(|kv| kv.strip_prefix("variant=")).map(crate::text_vars_json::percent_decode).unwrap_or_default();
    if asked.is_empty() || !eda_model::variants::names(design).contains(&asked) {
        return String::new();
    }
    asked
}
