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

/// `POST /api/sch/variant_compat` `{sheet?, id, lib_ids}` -> `{ok, issues: {lib_id: [detail]}}`: Set Variant Symbol...'s chooser asks which library symbols are
/// not pin-compatible with the placed symbol `id` (KiCad 25ec2335 `SCH_EDIT_TOOL::SetVariantSymbol`'s `compatFunc`: `ValidateVariantSymbolCompatibility( *baseFlat,
/// *flat )` of the symbol's own library symbol, flattened, and the candidate loaded through the library adapter). Each candidate is read from the libraries of the
/// tables, else the design's own; one neither has, or the base when the symbol has no library symbol, has no issues (`return {}`). UI-LEFT-3 S3-14.
pub fn variant_compat(dir: &std::path::Path, body: &[u8]) -> serde_json::Value {
    use serde_json::{json, Value};
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    let lib_ids: Vec<String> = req.get("lib_ids").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let (_, design, model) = match crate::sch_clipboard_api::loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let sheet = req.get("sheet").and_then(Value::as_str).unwrap_or("");
    let (section, _) = crate::studio::resolve_sheet(&design, sheet);
    let base = section.symbols.iter().find(|s| s.id == id).and_then(|s| model.symbol_of(&s.lib_id));
    let mut issues = serde_json::Map::new();
    if let Some(base) = base {
        let symbols = crate::library_tables::libs(dir, crate::library_index::Kind::Symbol);
        for lib_id in lib_ids {
            let candidate = crate::library_search::installed_symbol(&*symbols, &lib_id).map(|(s, _)| s.to_engine_symbol()).or_else(|| model.symbol_of(&lib_id));
            let found: Vec<String> = candidate.map(|c| eda_model::variant_symbol::validate_compatibility(&base, &c).into_iter().map(|i| i.detail).collect()).unwrap_or_default();
            issues.insert(lib_id, json!(found));
        }
    }
    json!({ "ok": true, "issues": issues })
}

#[cfg(test)]
mod tests {
    /// UI-LEFT-3 S3-14 (`SetVariantSymbol`'s `compatFunc`): a resistor's chooser marks a symbol whose pins differ (the LED's, KiCad's words) and not one with
    /// the resistor's pins; an unknown id has no issues (`return {}`).
    #[test]
    fn the_variant_symbol_chooser_is_told_which_symbols_are_not_pin_compatible() {
        let dir = crate::sch_hierarchy_tests::mcu30_board("variant_compat");
        let (_, design, _) = crate::board::load(&dir).unwrap();
        let r = design.schematic.as_ref().unwrap().symbols.iter().find(|s| s.lib_id == "Device:R").expect("a resistor").id.clone();
        let body = serde_json::json!({ "id": r, "lib_ids": ["Device:R", "Device:LED", "Nope:Nothing"] }).to_string();
        let out = super::variant_compat(&dir, body.as_bytes());
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(out["issues"]["Device:R"], serde_json::json!([]));
        assert_eq!(out["issues"]["Nope:Nothing"], serde_json::json!([]));
        let led = out["issues"]["Device:LED"].as_array().expect("the LED's issues");
        assert!(!led.is_empty() && led.iter().all(|d| d.as_str().is_some_and(|d| d.starts_with("Unit 1, body style 1: pin '"))), "{led:?}");
    }
}
