//! An Altium project's variants (`[ProjectVariantN]` of the `.PrjPcb`), read as KiCad 25ec2335 reads them for its "Import Non-KiCad Project > Altium Project"
//! (`common/io/altium/altium_project_variants.cpp`: `ParseAltiumProjectVariants`, `ParseVariationString`) and put on the symbols as its schematic importer puts them
//! (`SCH_IO_ALTIUM::LoadSchematicProject`, "Apply Altium project variants to schematic symbols"): each variant a design variant of the schematic
//! (`SCHEMATIC::AddVariant`), and each variation a symbol's record of it (`SCH_SYMBOL_VARIANT`, `InitializeAttributes` then the variation): a component not fitted
//! (`Kind=1`) is Do not populate, Exclude from BOM and Exclude from position files; one fitted with an alternate part (`Kind=0`) takes the alternate's `LibReference` as its
//! Value and its `Description` and `Footprint`.
//!
//! kicad-cli's `sch import` takes one `.SchDoc` and no project, so it never sees the variants; `eda import-kicad Project.PrjPcb` reads the project itself and applies them
//! here, to the sheets it combined. **Differs**: KiCad matches a variation to a symbol by the component's Altium unique id first (`m_altiumSymbolToUid`), which the converted
//! `.kicad_sch` does not keep, so a variation is matched by its designator only (KiCad's fallback, `variantsByDesignator`): in a design of repeated channels, where several
//! components share a unique id and differ by designator, the result is the same; where two components share a designator, both take the variation.

use eda_model::sheet_file::SheetFile;
use eda_model::variants::{SymbolFlags, SymbolVariant};
use std::collections::BTreeMap;

/// `ALTIUM_VARIANT_ENTRY`: one `VariationN=` line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct VariantEntry {
    pub designator: String,
    /// The component's own unique id (the last segment of the Altium path).
    pub unique_id: String,
    pub kind: i64,
    pub alternate_fields: BTreeMap<String, String>,
}

/// `ALTIUM_PROJECT_VARIANT`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProjectVariant {
    pub name: String,
    pub description: String,
    pub variations: Vec<VariantEntry>,
}

/// `ParseVariationString`: `Designator=R1|UniqueId=\ABC\DEF|Kind=1|AlternatePart=|...`. The fields after `AlternatePart` are the alternate part's.
pub(crate) fn parse_variation(value: &str) -> VariantEntry {
    let mut entry = VariantEntry::default();
    let mut in_alternate = false;
    for token in value.split('|') {
        let Some((key, val)) = token.split_once('=') else { continue };
        let (key, val) = (key.trim(), val.trim());
        if key.eq_ignore_ascii_case("AlternatePart") {
            in_alternate = true;
            if !val.is_empty() {
                entry.alternate_fields.insert("LibReference".to_string(), val.to_string());
            }
        } else if in_alternate {
            if !val.is_empty() {
                entry.alternate_fields.insert(key.to_string(), val.to_string());
            }
        } else if key.eq_ignore_ascii_case("Designator") {
            entry.designator = val.to_string();
        } else if key.eq_ignore_ascii_case("UniqueId") {
            entry.unique_id = val.rsplit('\\').next().unwrap_or(val).to_string();
        } else if key.eq_ignore_ascii_case("Kind") {
            // `wxString::ToLong` leaves 0 on a number it cannot read
            entry.kind = val.parse().unwrap_or(0);
        }
    }
    entry
}

/// `ParseAltiumProjectVariants` on the text of a `.PrjPcb` (an INI file, `wxFileConfig`): every `[ProjectVariantN]` section with a variation that names a designator; its name
/// is its `Description`, or the section's name when that is empty.
pub(crate) fn parse_project_variants(text: &str) -> Vec<ProjectVariant> {
    let mut sections: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_string(), BTreeMap::new()));
        } else if let (Some((_, keys)), Some((k, v))) = (sections.last_mut(), line.split_once('=')) {
            keys.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let mut out = Vec::new();
    for (group, keys) in sections {
        let Some(num) = group.strip_prefix("ProjectVariant") else { continue };
        if num.parse::<i64>().is_err() {
            continue;
        }
        let description = keys.get("Description").cloned().unwrap_or_default();
        let name = if description.is_empty() { group.clone() } else { description.clone() };
        let count: i64 = keys.get("VariationCount").and_then(|c| c.parse().ok()).unwrap_or(0);
        let variations: Vec<VariantEntry> = (1..=count).filter_map(|i| keys.get(&format!("Variation{i}")).filter(|v| !v.is_empty())).map(|v| parse_variation(v)).filter(|e| !e.designator.is_empty()).collect();
        if !variations.is_empty() {
            out.push(ProjectVariant { name, description, variations });
        }
    }
    out
}

/// The variants on the schematic: the root's list (`AddVariant`; a description only when it differs from the name, `SetVariantDescription`), and on every symbol of every sheet
/// that a variation names by its designator, its record of that variant. Returns how many records were made.
pub(crate) fn apply(file: &mut SheetFile, variants: &[ProjectVariant]) -> usize {
    for pv in variants {
        let description = if !pv.description.is_empty() && pv.description != pv.name { pv.description.clone() } else { String::new() };
        file.root.extras.variants.insert(pv.name.clone(), description);
    }
    let mut made = 0;
    let screens = std::iter::once(&mut file.root).chain(file.screens.values_mut());
    for sch in screens {
        let mut records: Vec<(String, String, SymbolVariant)> = Vec::new();
        for sym in &sch.symbols {
            for pv in variants {
                for entry in pv.variations.iter().filter(|e| e.designator == sym.id) {
                    // `variant.InitializeAttributes( *symbol )`, then the variation
                    let mut v = SymbolVariant::initialized(SymbolFlags::of(sch, sym));
                    if entry.kind == 1 {
                        v.dnp = Some(true);
                        v.exclude_from_bom = Some(true);
                        v.exclude_from_pos_files = Some(true);
                    } else if entry.kind == 0 {
                        for (key, value) in &entry.alternate_fields {
                            let field = if key.eq_ignore_ascii_case("LibReference") {
                                "Value"
                            } else if key.eq_ignore_ascii_case("Description") {
                                "Description"
                            } else if key.eq_ignore_ascii_case("Footprint") {
                                "Footprint"
                            } else {
                                continue;
                            };
                            v.fields.insert(field.to_string(), value.clone());
                        }
                    }
                    records.push((sym.id.clone(), pv.name.clone(), v));
                }
            }
        }
        for (reference, variant, v) in records {
            // a symbol of several units is one reference: its record is made once
            if sch.extras.symbol_variants.get(&reference).is_some_and(|m| m.contains_key(&variant)) {
                continue;
            }
            sch.extras.symbol_variants.entry(reference).or_default().insert(variant, v);
            made += 1;
        }
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{SchematicSection, SymbolInstance};

    const PRJ: &str = "\u{feff}[Design]\r\nVersion=1.0\r\n[ProjectVariant1]\r\nDescription=Lite\r\nAllowFabrication=0\r\nVariationCount=2\r\nVariation1=Designator=R2|UniqueId=\\LUBXRDIF\\ABCDEFGH|Kind=1|AlternatePart=\r\nVariation2=Designator=C5|UniqueId=QWERTYUI|Kind=0|AlternatePart=CAP-100N|Description=100 nF 0402|Footprint=C_0402|Comment=\r\n[ProjectVariant2]\r\nDescription=\r\nVariationCount=1\r\nVariation1=Designator=U1|Kind=1|AlternatePart=\r\n[ProjectVariantX]\r\nVariationCount=1\r\nVariation1=Designator=U9|Kind=1\r\n[ProjectVariant3]\r\nDescription=Empty\r\nVariationCount=1\r\nVariation1=UniqueId=X|Kind=1\r\n";

    #[test]
    fn the_variants_of_a_project_file_are_read_as_parse_altium_project_variants_reads_them() {
        let v = parse_project_variants(PRJ);
        assert_eq!(v.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Lite", "ProjectVariant2"], "a section that is not numbered, and a variant with no designated variation, are not variants");
        let lite = &v[0];
        assert_eq!(lite.variations[0], VariantEntry { designator: "R2".into(), unique_id: "ABCDEFGH".into(), kind: 1, alternate_fields: BTreeMap::new() });
        let c5 = &lite.variations[1];
        assert_eq!((c5.kind, c5.unique_id.as_str()), (0, "QWERTYUI"));
        assert_eq!(c5.alternate_fields, [("LibReference", "CAP-100N"), ("Description", "100 nF 0402"), ("Footprint", "C_0402")].map(|(k, v)| (k.to_string(), v.to_string())).into_iter().collect(), "an empty field of the alternate part is not kept");
    }

    #[test]
    fn a_variation_is_a_symbols_record_of_the_variant() {
        let sym = |id: &str| -> SymbolInstance { serde_json::from_value(serde_json::json!({ "id": id, "lib_id": "Device:R", "at": { "x": 0, "y": 0 }, "rot": 0 })).unwrap() };
        let mut file = SheetFile::default();
        file.root.symbols = vec![sym("R2"), sym("C5")];
        let mut sheet = SchematicSection::default();
        sheet.symbols = vec![sym("U1")];
        file.screens.insert("power.kicad_sch".into(), sheet);
        let made = apply(&mut file, &parse_project_variants(PRJ));
        assert_eq!(made, 3);
        assert_eq!(file.root.extras.variants.keys().collect::<Vec<_>>(), ["Lite", "ProjectVariant2"]);
        assert!(file.root.extras.variants.values().all(String::is_empty), "the description is the name: not set again");
        let r2 = &file.root.extras.symbol_variants["R2"]["Lite"];
        assert_eq!((r2.dnp, r2.exclude_from_bom, r2.exclude_from_pos_files, r2.exclude_from_board), (Some(true), Some(true), Some(true), Some(false)), "not fitted; the other flags the symbol's own");
        let c5 = &file.root.extras.symbol_variants["C5"]["Lite"];
        assert_eq!((c5.dnp, c5.fields.get("Value").map(String::as_str), c5.fields.get("Footprint").map(String::as_str)), (Some(false), Some("CAP-100N"), Some("C_0402")));
        assert!(file.screens["power.kicad_sch"].extras.symbol_variants["U1"].contains_key("ProjectVariant2"), "on the sheet the symbol is on");
    }
}
