//! A waived ERC violation as the derived `.kicad_pro` lists it (`erc.erc_exclusions`).
//!
//! KiCad keeps an exclusion as the marker it waives and matches a marker found by a run to its exclusion by what the marker is: the kind of
//! finding, where it is, the items it names and the sheets it belongs to (`SCHEMATIC::ResolveERCExclusions`). Up to the pinned source
//! (`8303b2ad`) that was one text, `SCH_MARKER::SerializeToString`, `<key>|<x>|<y>|<main uuid>|<aux uuid>|<sheet path>|<main item's sheet
//! path>|<aux item's sheet path>`. The nightly the studio runs (`10.99.0-5334`) keeps it as a structured record, `kiapi.schematic.ErcExclusion`
//! (`ERC_EXCLUSION`), in the project as protobuf JSON:
//!
//! ```text
//! { "marker": { "errorType": "ERCET_PIN_NOT_CONNECTED", "position": { "xNm": "115570000", "yNm": "110490000" },
//!               "items": [ { "value": "<uuid>" } ],
//!               "sheetSpecificPath": { "path": [ { "value": "<uuid>" } ] }, "mainItemSheetPath": { ... }, "auxItemSheetPath": { ... } },
//!   "comment": "..." }
//! ```
//!
//! with the position in nanometres (the schematic's internal unit is a nanometre now, and kicad-cli's report prints millimetres as they are), the
//! sheet paths as the uuids from the root sheet down, and each path field there only when the check's marker sets it. That nightly reads an old
//! pipe-delimited text only out of a section that says it is at schema 0 (with the position in the old unit, a hundred nanometres) and drops it from any
//! other, so none is written: the project lists the records, and its `erc` section says they are at schema 1 (`export_kicad_pro_for`). A record that
//! matches no marker is not dropped: kicad-cli turns it into an excluded marker of its own and its report lists it beside the real violation (with the
//! same items, when they resolve). So a record may go into the project only when it is known to the letter, and this module says it only for the checks
//! whose markers it has read the source of (`eeschema/erc/erc.cpp`, `eeschema/connection_graph.cpp`) and probed against kicad-cli; any other exclusion
//! stays the studio's alone, as a DRC exclusion does when the report cannot say where its marker is.
//!
//! What kicad-cli's report (`ERC_REPORT::WriteJsonReport`) gives is the items' uuids and positions, and the path of the sheet it lists the
//! violation under -- `MainItemHasSheetPath() ? GetMainItemSheetPath() : sheetList[0]`. The rest of the record depends on the check: which of the
//! three path fields its marker sets (`SetSheetSpecificPath`, `SetItemsSheetPaths`) and which item's position it is put at. [`rule`] is that table.

use eda_model::ir::{ErcExclusion, ErcMarker};
use serde_json::{json, Value};

/// One item a violation names, as the report says it: its uuid, where it is in nanometres and what it is called (`SCH_ITEM::GetItemDescription`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerItem {
    pub uuid: String,
    pub x_nm: i64,
    pub y_nm: i64,
    pub description: String,
}

/// KiCad's names for the kinds of finding (`kiapi.schematic.ErcErrorType`, in libkiapi) by the check's settings key: `ERCE_*` of
/// `eeschema/erc/erc_item.cpp` with the prefix `ERCET_`. `pin_to_pin` is two kinds, see [`error_type`].
const ERROR_TYPES: &[(&str, &str)] = &[
    ("duplicate_sheet_names", "ERCET_DUPLICATE_SHEET_NAME"),
    ("endpoint_off_grid", "ERCET_ENDPOINT_OFF_GRID"),
    ("pin_not_connected", "ERCET_PIN_NOT_CONNECTED"),
    ("pin_not_driven", "ERCET_PIN_NOT_DRIVEN"),
    ("power_pin_not_driven", "ERCET_POWERPIN_NOT_DRIVEN"),
    ("duplicate_pins", "ERCET_DUPLICATE_PIN_ERROR"),
    ("generic-warning", "ERCET_GENERIC_WARNING"),
    ("generic-error", "ERCET_GENERIC_ERROR"),
    ("hier_label_mismatch", "ERCET_HIERARCHICAL_LABEL"),
    ("four_way_junction", "ERCET_FOUR_WAY_JUNCTION"),
    ("label_multiple_wires", "ERCET_LABEL_MULTIPLE_WIRES"),
    ("no_connect_connected", "ERCET_NOCONNECT_CONNECTED"),
    ("no_connect_dangling", "ERCET_NOCONNECT_NOT_CONNECTED"),
    ("label_dangling", "ERCET_LABEL_NOT_CONNECTED"),
    ("isolated_pin_label", "ERCET_LABEL_SINGLE_PIN"),
    ("single_global_label", "ERCET_SINGLE_GLOBAL_LABEL"),
    ("same_local_global_label", "ERCET_SAME_LOCAL_GLOBAL_LABEL"),
    ("same_local_global_power", "ERCET_SAME_LOCAL_GLOBAL_POWER"),
    ("different_unit_footprint", "ERCET_DIFFERENT_UNIT_FP"),
    ("different_unit_net", "ERCET_DIFFERENT_UNIT_NET"),
    ("bus_definition_conflict", "ERCET_BUS_ALIAS_CONFLICT"),
    ("multiple_net_names", "ERCET_DRIVER_CONFLICT"),
    ("net_not_bus_member", "ERCET_BUS_ENTRY_CONFLICT"),
    ("bus_to_bus_conflict", "ERCET_BUS_TO_BUS_CONFLICT"),
    ("bus_to_net_conflict", "ERCET_BUS_TO_NET_CONFLICT"),
    ("ground_pin_not_ground", "ERCET_GROUND_PIN_NOT_GROUND"),
    ("stacked_pin_name", "ERCET_STACKED_PIN_SYNTAX"),
    ("field_name_whitespace", "ERCET_FIELD_NAME_WHITESPACE"),
    ("unresolved_variable", "ERCET_UNRESOLVED_VARIABLE"),
    ("undefined_netclass", "ERCET_UNDEFINED_NETCLASS"),
    ("simulation_model_issue", "ERCET_SIMULATION_MODEL"),
    ("wire_dangling", "ERCET_WIRE_DANGLING"),
    ("lib_symbol_issues", "ERCET_LIB_SYMBOL_ISSUES"),
    ("lib_symbol_mismatch", "ERCET_LIB_SYMBOL_MISMATCH"),
    ("footprint_link_issues", "ERCET_FOOTPRINT_LINK_ISSUES"),
    ("footprint_filter", "ERCET_FOOTPRINT_FILTERS"),
    ("unannotated", "ERCET_UNANNOTATED"),
    ("extra_units", "ERCET_EXTRA_UNITS"),
    ("missing_unit", "ERCET_MISSING_UNIT"),
    ("missing_input_pin", "ERCET_MISSING_INPUT_PIN"),
    ("missing_bidi_pin", "ERCET_MISSING_BIDI_PIN"),
    ("missing_power_pin", "ERCET_MISSING_POWER_INPUT_PIN"),
    ("unit_value_mismatch", "ERCET_DIFFERENT_UNIT_VALUE"),
    ("duplicate_reference", "ERCET_DUPLICATE_REFERENCE"),
    ("bus_entry_needed", "ERCET_BUS_ENTRY_NEEDED"),
    ("unconnected_wire_endpoint", "ERCET_UNCONNECTED_WIRE_ENDPOINT"),
];

/// The kind of finding the check's marker is (`kiapi.schematic.ErcErrorType`). The pin conflict is a warning or an error by the conflict map's cell, which the
/// report gives as the severity.
pub fn error_type(check: &str, severity: &str) -> Option<&'static str> {
    if check == "pin_to_pin" {
        return Some(if severity == "warning" { "ERCET_PIN_TO_PIN_WARNING" } else { "ERCET_PIN_TO_PIN_ERROR" });
    }
    ERROR_TYPES.iter().find(|(k, _)| *k == check).map(|(_, t)| *t)
}

/// Which item's position the marker sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum At {
    /// `SetItems( a, b )` and `new SCH_MARKER( item, a->GetPosition() )`.
    Main,
    /// ... `b->GetPosition()`.
    Aux,
}

/// Which of the three path fields the check's marker sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paths {
    /// None: the marker belongs to no sheet in particular (`IsSheetSpecific()` is false, `MainItemHasSheetPath()` too).
    None,
    /// `SetSheetSpecificPath( sheet )` alone. The report lists the violation under the root, whatever sheet it is on, so the sheet is known only
    /// when the report has a single one.
    Specific,
    /// `SetSheetSpecificPath( sheet )` and `SetItemsSheetPaths( sheet )`: the report lists the violation under its sheet.
    SpecificMain,
    /// `SetSheetSpecificPath( a )` and `SetItemsSheetPaths( a, b )`: the aux item's sheet is the main item's here (the report does not say where
    /// the aux item is), so only a single-sheet report knows it.
    SpecificMainAux,
}

struct Rule {
    at: At,
    paths: Paths,
}

/// The markers' own fields, per check: what `SCH_MARKER`'s creator sets (`erc.cpp` and `connection_graph.cpp` line numbers are of the source this was read from;
/// the ones marked [probed] were run against kicad-cli 10.99.0-5334, whose matcher accepts these forms and is more lenient with a few).
fn rule(check: &str) -> Option<Rule> {
    use At::{Aux, Main};
    use Paths::{None as NoPaths, Specific, SpecificMain, SpecificMainAux};
    let r = |at, paths| Some(Rule { at, paths });
    match check {
        // erc.cpp: a symbol's problem, put at the symbol, with no sheet in it (`TestLibSymbolIssues` 1706-1782, `TestFootprintLinkIssues` 1825-1933,
        // `TestSimModelIssues` 2061-2069).
        // (`footprint_link_issues` [probed].)
        "footprint_link_issues" | "simulation_model_issue" | "lib_symbol_issues" | "lib_symbol_mismatch" => r(Main, NoPaths),
        // erc.cpp 169-172: `SetItems( sheet, test_item )`, at the first sheet. 716-721: the item with the undefined netclass.
        "duplicate_sheet_names" | "undefined_netclass" => r(Main, NoPaths),
        // erc.cpp 555-559: `SetItems( unit, secondUnit )`, put at the second unit.
        "different_unit_footprint" => r(Aux, NoPaths),
        // connection_graph.cpp 4161-4166 and 4190-4195 (a symbol's pin), 4656-4661 (a sheet pin): sheet-specific, the item's sheet set, at the pin.
        // (The hierarchical label of the root sheet, 4628-4632, sets no sheet: see [`marker`].)
        // [probed]
        "pin_not_connected" => r(Main, SpecificMain),
        // erc.cpp 1225-1231: the pin that needs a driver [probed: `pin_not_driven`, `power_pin_not_driven`]; 1421-1429: a ground pin on another net; 1460-1465: a
        // pin named like a stacked one.
        "pin_not_driven" | "power_pin_not_driven" | "ground_pin_not_ground" | "stacked_pin_name" => r(Main, SpecificMain),
        // erc.cpp 616-622: the unit that is placed, with its sheet.
        "missing_unit" | "missing_input_pin" | "missing_bidi_pin" | "missing_power_pin" => r(Main, SpecificMain),
        // connection_graph.cpp 4064-4069: `SetItems( no_connect )`, sheet-specific, item sheet set [probed].
        "no_connect_dangling" => r(Main, SpecificMain),
        // connection_graph.cpp 4563-4568: the global label that appears once [probed].
        "single_global_label" => r(Main, SpecificMain),
        // connection_graph.cpp 4298-4305: `wires[0]`'s position; 4355-4360 and 4591-4596: the label (`ERCE_LABEL_NOT_CONNECTED`, `ERCE_LABEL_SINGLE_PIN`) [probed: all three].
        "wire_dangling" | "label_dangling" | "isolated_pin_label" => r(Main, Specific),
        // connection_graph.cpp 3757-3762, 3821-3826 and 3925-3931: sheet-specific, at the first item (the net, the label, the bus entry).
        "bus_to_net_conflict" | "bus_to_bus_conflict" | "net_not_bus_member" => r(Main, Specific),
        // erc.cpp 1184-1194: at `pin`, with the other pin's sheet as the aux path [probed]; 1273-1284: a shared pin on another net in another unit.
        "pin_to_pin" | "different_unit_net" => r(Main, SpecificMainAux),
        // connection_graph.cpp 3693-3699: `SetItems( m_driver, driver )`, at the driver.
        "multiple_net_names" => r(Aux, SpecificMainAux),
        // Not here: `endpoint_off_grid` and `unconnected_wire_endpoint` (a marker at each end of one wire, and the report names the wire once), `generic-*`
        // and `unresolved_variable` (a text's own record), `field_name_whitespace` (a field has no uuid of its own: the marker names it with a
        // `kiapi.schematic.ErcSymbolChildReference`, and none of the forms above matched on kicad-cli), `label_multiple_wires` and `four_way_junction` (a point
        // of their own), `no_connect_connected` (two creators with two sets of paths), `hier_label_mismatch`, `similar_*`, `same_local_global_*`,
        // `bus_definition_conflict`, `duplicate_pins`.
        _ => None,
    }
}

/// The uuids of a sheet path as kicad-cli's report writes it (`ERC_SHEET::uuid_path`, `/<root>/<sheet>`), root first.
fn uuids_of(path: &str) -> Vec<String> {
    path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The marker kicad-cli made for a violation, from what its report says: the check's settings key and the severity it reports it at, the items it
/// names (main first), the path of the sheet the report lists it under, and how many sheets the report has (a violation whose sheet the report does
/// not give is known only on a design of one). `None` when the marker is not known to the letter.
pub fn marker(check: &str, severity: &str, items: &[MarkerItem], sheet: &str, sheets: usize) -> Option<ErcMarker> {
    let rule = rule(check)?;
    let error_type = error_type(check, severity)?;
    let main = items.first()?;
    if main.uuid.is_empty() || items.iter().any(|i| i.uuid.is_empty()) {
        return None;
    }
    let at = match rule.at {
        At::Main => main,
        At::Aux => items.get(1)?,
    };
    // The root sheet's hierarchical labels get a `pin_not_connected` of their own, with no sheet in it (connection_graph.cpp 4628-4632).
    let hier_label = check == "pin_not_connected" && main.description.to_ascii_lowercase().starts_with("hierarchical label");
    let paths = if hier_label { Paths::None } else { rule.paths };
    let single = sheets <= 1;
    let path = uuids_of(sheet);
    let (specific, main_path, aux_path) = match paths {
        Paths::None => (Vec::new(), Vec::new(), Vec::new()),
        Paths::Specific if single && !path.is_empty() => (path, Vec::new(), Vec::new()),
        Paths::SpecificMain if !path.is_empty() => (path.clone(), path, Vec::new()),
        Paths::SpecificMainAux if single && !path.is_empty() => (path.clone(), path.clone(), path),
        _ => return None,
    };
    Some(ErcMarker { error_type: error_type.to_string(), at_nm: [at.x_nm, at.y_nm], items: items.iter().map(|i| i.uuid.clone()).collect(), sheet: specific, main_sheet: main_path, aux_sheet: aux_path })
}

/// Whether an exclusion's marker and a finding's are of one sheet (`SCH_MARKER::GetSheetSpecificPath`): the same sheet path, or either not known (an
/// exclusion of a check whose marker this module does not know, or one waived before its marker was kept) -- then the `(check, location)` key alone
/// decides, as it always did.
pub fn same_sheet(a: Option<&ErcMarker>, b: Option<&ErcMarker>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.sheet == b.sheet,
        _ => true,
    }
}

/// A sheet path as protobuf JSON (`kiapi.common.types.SheetPath`).
fn sheet_path_json(path: &[String]) -> Value {
    json!({ "path": path.iter().map(|u| json!({ "value": u })).collect::<Vec<_>>() })
}

/// One exclusion as the project keeps it (`kiapi.schematic.ErcExclusion` as protobuf JSON): the marker, and the comment when there is one.
pub fn entry(marker: &ErcMarker, comment: &str) -> Value {
    let mut m = json!({
        "errorType": marker.error_type,
        // protobuf JSON prints a 64-bit integer as a string
        "position": { "xNm": marker.at_nm[0].to_string(), "yNm": marker.at_nm[1].to_string() },
        "items": marker.items.iter().map(|u| json!({ "value": u })).collect::<Vec<_>>(),
    });
    if !marker.sheet.is_empty() {
        m["sheetSpecificPath"] = sheet_path_json(&marker.sheet);
    }
    if !marker.main_sheet.is_empty() {
        m["mainItemSheetPath"] = sheet_path_json(&marker.main_sheet);
    }
    if !marker.aux_sheet.is_empty() {
        m["auxItemSheetPath"] = sheet_path_json(&marker.aux_sheet);
    }
    let mut e = json!({ "marker": m });
    if !comment.is_empty() {
        e["comment"] = json!(comment);
    }
    e
}

/// The `erc_exclusions` array of the project's `erc` section: one record for each exclusion whose marker is known, in the order of the exclusions
/// (sorted by check and location), without a second record for a marker that two exclusions share.
pub fn project_entries(exclusions: &[ErcExclusion]) -> Vec<Value> {
    let mut seen: Vec<&ErcMarker> = Vec::new();
    let mut out = Vec::new();
    for e in exclusions {
        let Some(marker) = e.marker.as_ref().filter(|m| !m.error_type.is_empty() && !m.items.is_empty()) else { continue };
        if seen.contains(&marker) {
            continue;
        }
        seen.push(marker);
        out.push(entry(marker, &e.comment));
    }
    out
}

/// `KIID::niluuid`, which a marker with one item may name as its second.
const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// The check's settings key of a kind of finding (`kiapi.schematic.ErcErrorType`), the reverse of [`error_type`].
fn check_of(error_type: &str) -> Option<&'static str> {
    if error_type == "ERCET_PIN_TO_PIN_WARNING" || error_type == "ERCET_PIN_TO_PIN_ERROR" {
        return Some("pin_to_pin");
    }
    ERROR_TYPES.iter().find(|(_, t)| *t == error_type).map(|(k, _)| *k)
}

/// The exclusions of a project's `erc.erc_exclusions` as the file names them: the record (`kiapi.schematic.ErcExclusion` as protobuf JSON, `ERC_EXCLUSION`'s
/// `from_json`: the field names as the proto has them, which KiCad writes (`preserve_proto_field_names`), or in camel case, which `JsonStringToMessage` takes too),
/// or the marker's text of a project at schema 0 (`SCH_MARKER::SerializeToString`, `key|x|y|main|aux|sheet|main sheet|aux sheet`, the position in schematic
/// units of 100 nm, alone or as `[ "<text>", "<comment>" ]`: `ERC_SETTINGS::migrateSchema0to1`, `ERC_EXCLUSION::FromLegacyStrings`). The marker's item uuids
/// and sheet paths are the file's; a text item's own (`generic_*`, `unresolved_variable`: a parent's uuid and a text) is left out, and so is a kind of finding
/// this module does not name. Which form is read is the section's schema, as KiCad's loader has it: the texts at 0 (`migrateSchema0to1` turns them into records, the
/// records alongside are not read), the records at any other version or none (`entry.is_object()`, a text dropped).
pub fn read_project(pro_text: &str) -> Vec<(ErcMarker, String)> {
    let Ok(pro) = serde_json::from_str::<Value>(pro_text) else { return Vec::new() };
    let Some(entries) = pro.pointer("/erc/erc_exclusions").and_then(Value::as_array) else { return Vec::new() };
    let legacy = pro.pointer("/erc/meta/version").and_then(Value::as_i64) == Some(0);
    let field = |v: &Value, a: &str, b: &str| v.get(a).or_else(|| v.get(b)).cloned().unwrap_or(Value::Null);
    let path = |v: Value| -> Vec<String> { v.get("path").and_then(Value::as_array).map(|a| a.iter().filter_map(|u| u.get("value").and_then(Value::as_str)).map(String::from).collect()).unwrap_or_default() };
    let text_path = |t: &str| -> Vec<String> { t.split('/').map(str::trim).filter(|u| !u.is_empty()).map(String::from).collect() };
    let from_text = |text: &str, comment: &str| -> Option<(ErcMarker, String)> {
        let props: Vec<&str> = text.split('|').collect();
        if props.len() != 5 && props.len() != 8 {
            return None;
        }
        let check = *props.first()?;
        if matches!(check, "generic-warning" | "generic-error" | "unresolved_variable") {
            return None;
        }
        let error_type = error_type(check, "error")?;
        let iu = |i: usize| props.get(i).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0) * 100;
        let items = [3, 4].iter().filter_map(|&i| props.get(i).map(|s| s.trim())).filter(|u| !u.is_empty() && *u != NIL_UUID).map(String::from).collect();
        let p = |i: usize| props.get(i).map(|t| text_path(t)).unwrap_or_default();
        Some((ErcMarker { error_type: error_type.to_string(), at_nm: [iu(1), iu(2)], items, sheet: p(5), main_sheet: p(6), aux_sheet: p(7) }, comment.to_string()))
    };
    entries
        .iter()
        .filter_map(|e| match e {
            Value::Object(_) if !legacy => {
                let m = e.get("marker")?;
                let error_type = field(m, "error_type", "errorType").as_str()?.to_string();
                check_of(&error_type)?;
                let pos = field(m, "position", "position");
                // protobuf JSON prints a 64-bit integer as a string; a number is taken too
                let nm = |v: Value| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0);
                let items = m.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| i.get("value").and_then(Value::as_str)).filter(|u| *u != NIL_UUID).map(String::from).collect()).unwrap_or_default();
                let marker = ErcMarker {
                    error_type,
                    at_nm: [nm(field(&pos, "x_nm", "xNm")), nm(field(&pos, "y_nm", "yNm"))],
                    items,
                    sheet: path(field(m, "sheet_specific_path", "sheetSpecificPath")),
                    main_sheet: path(field(m, "main_item_sheet_path", "mainItemSheetPath")),
                    aux_sheet: path(field(m, "aux_item_sheet_path", "auxItemSheetPath")),
                };
                Some((marker, e.get("comment").and_then(Value::as_str).unwrap_or("").to_string()))
            }
            Value::Array(pair) if legacy => from_text(pair.first()?.as_str()?, pair.get(1).and_then(Value::as_str).unwrap_or("")),
            Value::String(text) if legacy => from_text(text, ""),
            _ => None,
        })
        .filter(|(m, _)| !m.items.is_empty())
        .collect()
}

/// A `.kicad_sch` text's own `(uuid ..)`: the root sheet's, which the sheet paths of a project's markers start with.
pub fn file_uuid(text: &str) -> Option<String> {
    let tree = crate::sexpr::parse(text).ok()?;
    crate::sexpr::find(tree.as_list()?, "uuid").and_then(|u| crate::sexpr::txt(u, 1)).map(String::from)
}

/// A KiCad project's own ERC exclusions as the design's (`SchematicSection::erc_exclusions`), for a schematic imported from that project. Each item the marker
/// names by its uuid in the file is found by the id the import gave it (`file_ids`, `SchImportNotes::item_ids`), and then by the uuid the derived schematic gives it
/// (`derived`, the map `export_kicad_sch_tree_mapped` returns); the sheet paths go the same way, the file's root sheet (`file_root`) being the derived one's
/// (`derived_root`) and every other sheet keeping its uuid (a sheet read from a file is written with the file's, `crate::sheet_uuid`). The exclusion's location is the id of the marker's main item, as the studio's report keys
/// it. One with an item the import does not have is left out, as the DRC's are (KiCad keeps such a record as a marker of its own).
pub fn from_project(pro_text: &str, file_ids: &std::collections::BTreeMap<String, String>, derived: &std::collections::HashMap<String, String>, file_root: &str, derived_root: &str) -> Vec<ErcExclusion> {
    let mut uuid_of: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    let mut by_uuid: Vec<(&String, &String)> = derived.iter().collect();
    by_uuid.sort();
    for (uuid, id) in by_uuid {
        uuid_of.entry(id.as_str()).or_insert(uuid.as_str());
    }
    let sheet = |p: &[String]| -> Vec<String> { p.iter().map(|u| if u == file_root { derived_root.to_string() } else { u.clone() }).collect() };
    let mut out: Vec<ErcExclusion> = Vec::new();
    for (m, comment) in read_project(pro_text) {
        let Some(ids) = m.items.iter().map(|u| file_ids.get(u).cloned()).collect::<Option<Vec<String>>>() else { continue };
        let Some(items) = ids.iter().map(|id| uuid_of.get(id.as_str()).map(|u| u.to_string())).collect::<Option<Vec<String>>>() else { continue };
        let Some(check) = check_of(&m.error_type) else { continue };
        let marker = ErcMarker { items, sheet: sheet(&m.sheet), main_sheet: sheet(&m.main_sheet), aux_sheet: sheet(&m.aux_sheet), ..m };
        let e = ErcExclusion { check: check.to_string(), location: ids[0].clone(), comment, marker: Some(marker) };
        if !out.iter().any(|o| o.check == e.check && o.location == e.location && o.marker == e.marker) {
            out.push(e);
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/172726e6-8494-4d0f-8495-8bee6ad09c57";
    const ROOT_UUID: &str = "172726e6-8494-4d0f-8495-8bee6ad09c57";

    fn item(uuid: &str, x_nm: i64, y_nm: i64, description: &str) -> MarkerItem {
        MarkerItem { uuid: uuid.into(), x_nm, y_nm, description: description.into() }
    }

    fn excl(check: &str, location: &str, marker: Option<ErcMarker>, comment: &str) -> ErcExclusion {
        ErcExclusion { check: check.into(), location: location.into(), marker, comment: comment.into() }
    }

    fn only(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    /// Records kicad-cli 10.99.0-5334 consumed when they were written to a project (probed on a derived schematic: the violation was reported as excluded, with the
    /// comment, and no marker of its own was left beside it).
    #[test]
    fn a_pin_not_connected_marker_is_sheet_specific_with_the_items_sheet_set_and_is_written_as_protobuf_json() {
        let pin = item("693a38f5-591c-4c06-9786-e8973d13af99", 115_570_000, 110_490_000, "Symbol #PWR03 Pin 1 [Power input, Line]");
        let m = marker("pin_not_connected", "error", &[pin], ROOT, 1).unwrap();
        assert_eq!(
            m,
            ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [115_570_000, 110_490_000], items: only(&["693a38f5-591c-4c06-9786-e8973d13af99"]), sheet: only(&[ROOT_UUID]), main_sheet: only(&[ROOT_UUID]), aux_sheet: vec![] }
        );
        assert_eq!(
            entry(&m, "unused on purpose"),
            json!({
                "marker": {
                    "errorType": "ERCET_PIN_NOT_CONNECTED",
                    "position": { "xNm": "115570000", "yNm": "110490000" },
                    "items": [{ "value": "693a38f5-591c-4c06-9786-e8973d13af99" }],
                    "sheetSpecificPath": { "path": [{ "value": ROOT_UUID }] },
                    "mainItemSheetPath": { "path": [{ "value": ROOT_UUID }] },
                },
                "comment": "unused on purpose",
            })
        );
        assert!(entry(&m, "").get("comment").is_none(), "no comment: none is written");
    }

    #[test]
    fn a_symbols_footprint_link_issue_has_no_sheet_in_it() {
        let symbol = item("a4f97020-e09b-4e24-b611-b376f3c27df7", 113_030_000, 100_330_000, "Symbol C1 [C]");
        let m = marker("footprint_link_issues", "warning", &[symbol.clone()], ROOT, 1).unwrap();
        assert_eq!((m.error_type.as_str(), m.at_nm), ("ERCET_FOOTPRINT_LINK_ISSUES", [113_030_000, 100_330_000]));
        assert!(m.sheet.is_empty() && m.main_sheet.is_empty() && m.aux_sheet.is_empty());
        let e = entry(&m, "");
        assert!(e["marker"].get("sheetSpecificPath").is_none() && e["marker"].get("mainItemSheetPath").is_none(), "{e}");
        // The same on a design of many sheets: nothing of a sheet in it, so the report's group does not matter.
        assert!(marker("footprint_link_issues", "warning", &[symbol], "/r/s", 3).unwrap().sheet.is_empty());
    }

    #[test]
    fn a_dangling_wire_or_label_names_its_sheet_once_and_a_dangling_no_connect_twice() {
        let wire = item("11111111-0000-4000-8000-000000000001", 50_800_000, 50_800_000, "Horizontal Wire, length 10.16 mm");
        let sheet_only = |m: &ErcMarker| (m.sheet.clone(), m.main_sheet.clone(), m.aux_sheet.clone());
        let w = marker("wire_dangling", "error", &[wire.clone()], ROOT, 1).unwrap();
        assert_eq!((w.error_type.as_str(), sheet_only(&w)), ("ERCET_WIRE_DANGLING", (only(&[ROOT_UUID]), vec![], vec![])));
        let l = marker("label_dangling", "error", &[wire.clone()], ROOT, 1).unwrap();
        assert_eq!((l.error_type.as_str(), sheet_only(&l)), ("ERCET_LABEL_NOT_CONNECTED", (only(&[ROOT_UUID]), vec![], vec![])));
        let n = marker("no_connect_dangling", "warning", &[wire], ROOT, 1).unwrap();
        assert_eq!((n.error_type.as_str(), sheet_only(&n)), ("ERCET_NOCONNECT_NOT_CONNECTED", (only(&[ROOT_UUID]), only(&[ROOT_UUID]), vec![])));
    }

    #[test]
    fn the_report_lists_a_violation_under_the_root_so_a_sheet_the_report_does_not_give_is_known_only_on_a_design_of_one_sheet() {
        let wire = item("w", 1, 2, "Wire");
        assert!(marker("wire_dangling", "error", &[wire.clone()], ROOT, 2).is_none(), "a dangling wire of a hierarchical design: which sheet it is on is not in the report");
        assert!(marker("pin_to_pin", "warning", &[item("a", 1, 2, "Pin"), item("b", 3, 4, "Pin")], "/r/s", 2).is_none(), "the other pin's sheet is not in the report either");
        // A violation the report files under its own sheet is known on any design, with the whole path from the root.
        let m = marker("power_pin_not_driven", "error", &[item("a", 1, 2, "Pin")], "/r/s", 3).unwrap();
        assert_eq!((m.sheet, m.main_sheet), (only(&["r", "s"]), only(&["r", "s"])));
    }

    #[test]
    fn two_items_make_a_pair_and_the_marker_sits_at_the_one_the_check_puts_it() {
        let a = item("aaaaaaaa-0000-4000-8000-000000000000", 10, 20, "Symbol U1 Pin 1");
        let b = item("bbbbbbbb-0000-4000-8000-000000000000", 30, 40, "Symbol U2 Pin 1");
        // `pin_to_pin` is put at the first pin, `multiple_net_names` at the second item (the driver).
        let p = marker("pin_to_pin", "warning", &[a.clone(), b.clone()], ROOT, 1).unwrap();
        assert_eq!((p.error_type.as_str(), p.at_nm, p.items.len(), p.aux_sheet.len()), ("ERCET_PIN_TO_PIN_WARNING", [10, 20], 2, 1));
        assert_eq!(marker("pin_to_pin", "error", &[a.clone(), b.clone()], ROOT, 1).unwrap().error_type, "ERCET_PIN_TO_PIN_ERROR");
        let d = marker("multiple_net_names", "error", &[a.clone(), b], ROOT, 1).unwrap();
        assert_eq!((d.error_type.as_str(), d.at_nm), ("ERCET_DRIVER_CONFLICT", [30, 40]));
        assert!(marker("multiple_net_names", "error", &[a], ROOT, 1).is_none(), "a pair check with one item named: the position is the second item's, which is not there");
    }

    #[test]
    fn a_root_sheets_hierarchical_label_has_no_sheet_in_its_pin_not_connected() {
        let label = item("l", 5, 6, "Hierarchical Label 'IN' [Input]");
        let m = marker("pin_not_connected", "error", &[label], ROOT, 1).unwrap();
        assert!(m.sheet.is_empty() && m.main_sheet.is_empty());
    }

    #[test]
    fn a_check_whose_marker_this_has_not_read_the_source_of_has_no_marker() {
        for check in ["endpoint_off_grid", "unconnected_wire_endpoint", "unresolved_variable", "generic-warning", "four_way_junction", "no_connect_connected", "similar_labels", "field_name_whitespace"] {
            assert!(marker(check, "error", &[item("a", 1, 2, "Wire")], ROOT, 1).is_none(), "{check}");
        }
        assert!(marker("footprint_link_issues", "warning", &[], ROOT, 1).is_none(), "a violation naming no item cannot be keyed");
        assert!(marker("footprint_link_issues", "warning", &[item("", 1, 2, "Symbol")], ROOT, 1).is_none());
    }

    #[test]
    fn every_check_with_a_marker_rule_has_a_kind_of_finding_and_the_names_are_kicads() {
        for check in ["footprint_link_issues", "simulation_model_issue", "lib_symbol_issues", "lib_symbol_mismatch", "duplicate_sheet_names", "undefined_netclass", "different_unit_footprint", "pin_not_connected", "pin_not_driven", "power_pin_not_driven", "ground_pin_not_ground", "stacked_pin_name", "missing_unit", "missing_input_pin", "missing_bidi_pin", "missing_power_pin", "no_connect_dangling", "single_global_label", "wire_dangling", "label_dangling", "isolated_pin_label", "bus_to_net_conflict", "bus_to_bus_conflict", "net_not_bus_member", "pin_to_pin", "different_unit_net", "multiple_net_names"] {
            assert!(rule(check).is_some() && error_type(check, "error").is_some(), "{check}");
        }
        // `ERCE_` of erc_item.cpp with the library's `ERCET_`; two spellings that differ.
        assert_eq!(error_type("hier_label_mismatch", "error"), Some("ERCET_HIERARCHICAL_LABEL"));
        assert_eq!(error_type("unit_value_mismatch", "error"), Some("ERCET_DIFFERENT_UNIT_VALUE"));
        assert_eq!(error_type("multiple_net_names", "error"), Some("ERCET_DRIVER_CONFLICT"));
        assert_eq!(error_type("pin_to_pin", "warning"), Some("ERCET_PIN_TO_PIN_WARNING"));
        assert_eq!(error_type("not_a_check", "error"), None);
        assert!(ERROR_TYPES.iter().all(|(_, t)| t.starts_with("ERCET_")));
    }

    #[test]
    fn the_project_lists_each_known_marker_with_its_comment_and_leaves_the_studios_own_out() {
        let sym = |u: &str| ErcMarker { error_type: "ERCET_FOOTPRINT_LINK_ISSUES".into(), at_nm: [1, 1], items: vec![u.into()], ..Default::default() };
        let list = project_entries(&[
            excl("footprint_link_issues", "R1", Some(sym("r1")), ""),
            excl("wire_dangling", "w1", None, "no marker: the studio's alone"),
            excl("footprint_link_issues", "R2", Some(sym("r2")), "bare footprint on purpose"),
            excl("footprint_link_issues", "R2b", Some(sym("r2")), "the same marker"),
            excl("x", "y", Some(ErcMarker::default()), "an empty marker is nothing"),
        ]);
        assert_eq!(list.len(), 2, "{list:?}");
        assert_eq!(list[0]["marker"]["items"][0]["value"], "r1");
        assert!(list[0].get("comment").is_none());
        assert_eq!((list[1]["marker"]["items"][0]["value"].as_str(), list[1]["comment"].as_str()), (Some("r2"), Some("bare footprint on purpose")));
        assert!(project_entries(&[]).is_empty());
    }

    /// p13-p16 F6 (the ERC half): a project's own exclusions, as KiCad 10 writes them (records, the proto's field names) and as KiCad 8 and 9 did (texts at
    /// schema 0, positions in 100 nm), come in with their items and sheets named as the derived schematic names them; one whose item the import lacks is left out.
    #[test]
    fn a_projects_erc_exclusions_are_read_and_named_as_the_derived_schematic_names_them() {
        let records = json!({ "erc": { "meta": { "version": 1 }, "erc_exclusions": [
            { "marker": { "error_type": "ERCET_PIN_NOT_CONNECTED", "position": { "x_nm": "1000", "y_nm": 2000 }, "items": [{ "value": "f-pin" }],
                          "sheet_specific_path": { "path": [{ "value": "f-root" }, { "value": "s1" }] }, "main_item_sheet_path": { "path": [{ "value": "f-root" }, { "value": "s1" }] } },
              "comment": "kept" },
            { "marker": { "errorType": "ERCET_PIN_TO_PIN_WARNING", "position": { "xNm": "5", "yNm": "6" }, "items": [{ "value": "f-pin" }, { "value": "f-gone" }] } },
            "pin_not_connected|1|2|f-pin|00000000-0000-0000-0000-000000000000|||"
        ] } })
        .to_string();
        let file_ids: std::collections::BTreeMap<String, String> = [("f-pin".to_string(), "U1.1".to_string())].into();
        let derived: std::collections::HashMap<String, String> = [("d-pin".to_string(), "U1.1".to_string())].into();
        let got = from_project(&records, &file_ids, &derived, "f-root", "d-root");
        let marker = ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [1000, 2000], items: only(&["d-pin"]), sheet: only(&["d-root", "s1"]), main_sheet: only(&["d-root", "s1"]), aux_sheet: vec![] };
        assert_eq!(got, vec![excl("pin_not_connected", "U1.1", Some(marker), "kept")], "the record whose second item is not in the import is left out, and the text is not read at schema 1");
        // schema 0: the texts, and not the records
        let legacy = json!({ "erc": { "meta": { "version": 0 }, "erc_exclusions": [["pin_not_connected|13335|8128|f-pin|00000000-0000-0000-0000-000000000000|||", "comment1"], "generic-warning|1|1|f-pin|some text|||", "bad|1"] } }).to_string();
        let got = from_project(&legacy, &file_ids, &derived, "f-root", "d-root");
        let marker = ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [1_333_500, 812_800], items: only(&["d-pin"]), ..Default::default() };
        assert_eq!(got, vec![excl("pin_not_connected", "U1.1", Some(marker), "comment1")]);
        assert_eq!(file_uuid("(kicad_sch (version 20250114) (uuid \"ab-cd\"))").as_deref(), Some("ab-cd"));
    }
}
