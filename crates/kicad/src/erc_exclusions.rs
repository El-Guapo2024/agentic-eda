//! A waived ERC violation as the derived `.kicad_pro` lists it (`erc.erc_exclusions`).
//!
//! KiCad keeps an exclusion as the text of the marker it waives -- `SCH_MARKER::SerializeToString` -- and matches a marker found by a run
//! to its exclusion by that text, exactly (`SCHEMATIC::ResolveERCExclusions`):
//!
//! ```text
//! <settings key>|<x>|<y>|<main item uuid>|<aux item uuid>|<sheet-specific path>|<main item's sheet path>|<aux item's sheet path>
//! ```
//!
//! with the position in schematic units (100 nm) and `niluuid` for a violation that names one item. An exclusion no marker matches is not
//! dropped, as the board's are: `ResolveERCExclusions` turns it into an excluded marker of its own, and kicad-cli's report lists that marker
//! beside the real violation (with the same items, when they resolve). So a text may go into the project only when it is known to the letter,
//! and this module says it only for the checks whose markers it has read the source of (`eeschema/erc/erc.cpp`,
//! `eeschema/connection_graph.cpp`, `eeschema/sch_marker.cpp`); any other exclusion stays the studio's alone, as a DRC exclusion does when the
//! report cannot say where its marker is.
//!
//! What kicad-cli's report (`ERC_REPORT::WriteJsonReport`) gives is the items' uuids and positions, and the path of the sheet it lists the
//! violation under -- `MainItemHasSheetPath() ? GetMainItemSheetPath() : sheetList[0]`. The rest of the text depends on the check: which of the
//! three path fields its marker sets (`SetSheetSpecificPath`, `SetItemsSheetPaths`) and which item's position it is put at. [`rule`] is that table.

use eda_model::ir::ErcExclusion;
use std::collections::BTreeMap;

/// `niluuid`: what `RC_ITEM::GetAuxItemID` is for a violation that names one item.
pub const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// One item a violation names, as the report says it: its uuid, where it is in schematic units (100 nm) and what it is called
/// (`SCH_ITEM::GetItemDescription`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerItem {
    pub uuid: String,
    pub x: i64,
    pub y: i64,
    pub description: String,
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

/// The markers' own text, per check: what `SCH_MARKER`'s creator sets (`erc.cpp` and `connection_graph.cpp` line numbers are of the source this was read from).
fn rule(check: &str) -> Option<Rule> {
    use At::{Aux, Main};
    use Paths::{None as NoPaths, Specific, SpecificMain, SpecificMainAux};
    let r = |at, paths| Some(Rule { at, paths });
    match check {
        // erc.cpp: a symbol's problem, put at the symbol, with no sheet in it (`TestLibSymbolIssues` 1706-1782, `TestFootprintLinkIssues` 1825-1933,
        // `TestSimModelIssues` 2061-2069).
        "footprint_link_issues" | "simulation_model_issue" | "lib_symbol_issues" | "lib_symbol_mismatch" => r(Main, NoPaths),
        // erc.cpp 169-172: `SetItems( sheet, test_item )`, at the first sheet. 716-721: the item with the undefined netclass.
        "duplicate_sheet_names" | "undefined_netclass" => r(Main, NoPaths),
        // erc.cpp 555-559: `SetItems( unit, secondUnit )`, put at the second unit.
        "different_unit_footprint" => r(Aux, NoPaths),
        // connection_graph.cpp 4161-4166 and 4190-4195 (a symbol's pin), 4656-4661 (a sheet pin): sheet-specific, the item's sheet set, at the pin.
        // (The hierarchical label of the root sheet, 4628-4632, sets no sheet: see [`marker_text`].)
        "pin_not_connected" => r(Main, SpecificMain),
        // erc.cpp 1225-1231: the pin that needs a driver; 1421-1429: a ground pin on another net; 1460-1465: a pin named like a stacked one.
        "pin_not_driven" | "power_pin_not_driven" | "ground_pin_not_ground" | "stacked_pin_name" => r(Main, SpecificMain),
        // erc.cpp 616-622: the unit that is placed, with its sheet.
        "missing_unit" | "missing_input_pin" | "missing_bidi_pin" | "missing_power_pin" => r(Main, SpecificMain),
        // connection_graph.cpp 4064-4069: `SetItems( no_connect )`, sheet-specific, item sheet set.
        "no_connect_dangling" => r(Main, SpecificMain),
        // connection_graph.cpp 4563-4568: the global label that appears once.
        "single_global_label" => r(Main, SpecificMain),
        // connection_graph.cpp 4298-4305: `wires[0]`'s position; 4355-4360 and 4591-4596: the label (`ERCE_LABEL_NOT_CONNECTED`, `ERCE_LABEL_SINGLE_PIN`).
        "wire_dangling" | "label_dangling" | "isolated_pin_label" => r(Main, Specific),
        // connection_graph.cpp 3757-3762, 3821-3826 and 3925-3931: sheet-specific, at the first item (the net, the label, the bus entry).
        "bus_to_net_conflict" | "bus_to_bus_conflict" | "net_not_bus_member" => r(Main, Specific),
        // erc.cpp 1184-1194: at `pin`, with the other pin's sheet as the aux path; 1273-1284: a shared pin on another net in another unit.
        "pin_to_pin" | "different_unit_net" => r(Main, SpecificMainAux),
        // connection_graph.cpp 3693-3699: `SetItems( m_driver, driver )`, at the driver.
        "multiple_net_names" => r(Aux, SpecificMainAux),
        // erc.cpp 461-470: `SetItems( symbol, &field )`, at the field.
        "field_name_whitespace" => r(Aux, SpecificMainAux),
        // Not here: `endpoint_off_grid` and `unconnected_wire_endpoint` (a marker at each end of one wire, and the report names the wire once), `generic-*`
        // and `unresolved_variable` (a text's own serialization), `label_multiple_wires` and `four_way_junction` (a point of their own), `no_connect_connected`
        // (two creators with two sets of paths), `hier_label_mismatch`, `similar_*`, `same_local_global_*`, `bus_definition_conflict`, `duplicate_pins`.
        _ => None,
    }
}

/// `SCH_MARKER::SerializeToString` of the marker kicad-cli made for a violation, from what its report says: the check's settings key, the items
/// it names (main first), the path of the sheet the report lists it under, and how many sheets the report has (a violation whose sheet the report
/// does not give is known only on a design of one). `None` when the text is not known to the letter.
pub fn marker_text(check: &str, items: &[MarkerItem], sheet: &str, sheets: usize) -> Option<String> {
    let rule = rule(check)?;
    let main = items.first()?;
    if main.uuid.is_empty() {
        return None;
    }
    let aux = items.get(1);
    let at = match rule.at {
        At::Main => main,
        At::Aux => aux?,
    };
    // The root sheet's hierarchical labels get a `pin_not_connected` of their own, with no sheet in it (connection_graph.cpp 4628-4632).
    let hier_label = check == "pin_not_connected" && main.description.to_ascii_lowercase().starts_with("hierarchical label");
    let paths = if hier_label { Paths::None } else { rule.paths };
    let single = sheets <= 1;
    let (specific, main_path, aux_path) = match paths {
        Paths::None => ("", "", ""),
        Paths::Specific if single && !sheet.is_empty() => (sheet, "", ""),
        Paths::SpecificMain if !sheet.is_empty() => (sheet, sheet, ""),
        Paths::SpecificMainAux if single && !sheet.is_empty() => (sheet, sheet, sheet),
        _ => return None,
    };
    // A violation that names one item has `niluuid` for the second; one that names two but whose second the text does not use still writes it.
    let aux_uuid = aux.map_or(NIL_UUID, |a| a.uuid.as_str());
    if aux.is_some() && aux_uuid.is_empty() {
        return None;
    }
    Some(format!("{check}|{}|{}|{}|{aux_uuid}|{specific}|{main_path}|{aux_path}", at.x, at.y, main.uuid))
}

/// The `erc_exclusions` array of the project's `erc` section: `[text, comment]` for each exclusion whose text is known, in the order KiCad's own
/// `std::set` keeps them (`ERC_SETTINGS`' `erc_exclusions` `PARAM_LAMBDA`).
pub fn project_entries(exclusions: &[ErcExclusion]) -> Vec<serde_json::Value> {
    let mut entries: BTreeMap<&str, &str> = BTreeMap::new();
    for e in exclusions.iter().filter(|e| !e.marker.is_empty()) {
        entries.insert(e.marker.as_str(), e.comment.as_str());
    }
    entries.into_iter().map(|(text, comment)| serde_json::json!([text, comment])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/172726e6-8494-4d0f-8495-8bee6ad09c57";

    fn item(uuid: &str, x: i64, y: i64, description: &str) -> MarkerItem {
        MarkerItem { uuid: uuid.into(), x, y, description: description.into() }
    }

    fn excl(check: &str, location: &str, marker: &str, comment: &str) -> ErcExclusion {
        ErcExclusion { check: check.into(), location: location.into(), marker: marker.into(), comment: comment.into() }
    }

    /// Texts kicad-cli 10.99 consumed when they were written to a project (probed on a derived schematic, then the violation was reported as excluded).
    #[test]
    fn a_pin_not_connected_marker_is_sheet_specific_with_the_items_sheet_set() {
        let pin = item("693a38f5-591c-4c06-9786-e8973d13af99", 1_155_700, 1_104_900, "Symbol #PWR03 Pin 1 [Power input, Line]");
        assert_eq!(
            marker_text("pin_not_connected", &[pin], ROOT, 1).unwrap(),
            format!("pin_not_connected|1155700|1104900|693a38f5-591c-4c06-9786-e8973d13af99|{NIL_UUID}|{ROOT}|{ROOT}|")
        );
    }

    #[test]
    fn a_symbols_footprint_link_issue_has_no_sheet_in_it() {
        let symbol = item("a4f97020-e09b-4e24-b611-b376f3c27df7", 1_130_300, 1_003_300, "Symbol C1 [C]");
        assert_eq!(marker_text("footprint_link_issues", &[symbol], ROOT, 1).unwrap(), format!("footprint_link_issues|1130300|1003300|a4f97020-e09b-4e24-b611-b376f3c27df7|{NIL_UUID}|||"));
        // The same text on a design of many sheets: nothing of a sheet in it, so the report's group does not matter.
        let symbol = item("a4f97020-e09b-4e24-b611-b376f3c27df7", 1_130_300, 1_003_300, "Symbol C1 [C]");
        assert!(marker_text("footprint_link_issues", &[symbol], "/r/s", 3).unwrap().ends_with("|||"));
    }

    #[test]
    fn a_dangling_wire_or_label_names_its_sheet_once_and_a_dangling_no_connect_twice() {
        let wire = item("11111111-0000-4000-8000-000000000001", 508_000, 508_000, "Horizontal Wire, length 10.16 mm");
        assert_eq!(marker_text("wire_dangling", &[wire.clone()], ROOT, 1).unwrap(), format!("wire_dangling|508000|508000|11111111-0000-4000-8000-000000000001|{NIL_UUID}|{ROOT}||"));
        assert_eq!(marker_text("label_dangling", &[wire.clone()], ROOT, 1).unwrap(), format!("label_dangling|508000|508000|11111111-0000-4000-8000-000000000001|{NIL_UUID}|{ROOT}||"));
        assert_eq!(marker_text("no_connect_dangling", &[wire], ROOT, 1).unwrap(), format!("no_connect_dangling|508000|508000|11111111-0000-4000-8000-000000000001|{NIL_UUID}|{ROOT}|{ROOT}|"));
    }

    #[test]
    fn the_report_lists_a_violation_under_the_root_so_a_sheet_the_report_does_not_give_is_known_only_on_a_design_of_one_sheet() {
        let wire = item("w", 1, 2, "Wire");
        assert!(marker_text("wire_dangling", &[wire.clone()], ROOT, 2).is_none(), "a dangling wire of a hierarchical design: which sheet it is on is not in the report");
        assert!(marker_text("pin_to_pin", &[item("a", 1, 2, "Pin"), item("b", 3, 4, "Pin")], "/r/s", 2).is_none(), "the other pin's sheet is not in the report either");
        // A violation the report files under its own sheet is known on any design.
        assert!(marker_text("power_pin_not_driven", &[item("a", 1, 2, "Pin")], "/r/s", 3).unwrap().ends_with("|/r/s|/r/s|"));
    }

    #[test]
    fn two_items_make_a_pair_and_the_marker_sits_at_the_one_the_check_puts_it() {
        let a = item("aaaaaaaa-0000-4000-8000-000000000000", 10, 20, "Symbol U1 Pin 1");
        let b = item("bbbbbbbb-0000-4000-8000-000000000000", 30, 40, "Symbol U2 Pin 1");
        // `pin_to_pin` is put at the first pin, `multiple_net_names` at the second item (the driver).
        assert_eq!(marker_text("pin_to_pin", &[a.clone(), b.clone()], ROOT, 1).unwrap(), format!("pin_to_pin|10|20|{}|{}|{ROOT}|{ROOT}|{ROOT}", a.uuid, b.uuid));
        assert_eq!(marker_text("multiple_net_names", &[a.clone(), b.clone()], ROOT, 1).unwrap(), format!("multiple_net_names|30|40|{}|{}|{ROOT}|{ROOT}|{ROOT}", a.uuid, b.uuid));
        assert!(marker_text("multiple_net_names", &[a], ROOT, 1).is_none(), "a pair check with one item named: the position is the second item's, which is not there");
    }

    #[test]
    fn a_root_sheets_hierarchical_label_has_no_sheet_in_its_pin_not_connected() {
        let label = item("l", 5, 6, "Hierarchical Label 'IN' [Input]");
        assert_eq!(marker_text("pin_not_connected", &[label], ROOT, 1).unwrap(), format!("pin_not_connected|5|6|l|{NIL_UUID}|||"));
    }

    #[test]
    fn a_check_whose_marker_this_has_not_read_the_source_of_has_no_text() {
        for check in ["endpoint_off_grid", "unconnected_wire_endpoint", "unresolved_variable", "generic-warning", "four_way_junction", "no_connect_connected", "similar_labels"] {
            assert!(marker_text(check, &[item("a", 1, 2, "Wire")], ROOT, 1).is_none(), "{check}");
        }
        assert!(marker_text("footprint_link_issues", &[], ROOT, 1).is_none(), "a violation naming no item cannot be keyed");
        assert!(marker_text("footprint_link_issues", &[item("", 1, 2, "Symbol")], ROOT, 1).is_none());
    }

    #[test]
    fn the_project_lists_each_known_text_with_its_comment_sorted_and_leaves_the_studios_own_out() {
        let list = project_entries(&[
            excl("pin_not_connected", "U1.1", "pin_not_connected|2|2|u|n|||", "unused on purpose"),
            excl("wire_dangling", "w1", "", "no text: the studio's alone"),
            excl("footprint_link_issues", "R1", "footprint_link_issues|1|1|r|n|||", ""),
        ]);
        assert_eq!(list, vec![serde_json::json!(["footprint_link_issues|1|1|r|n|||", ""]), serde_json::json!(["pin_not_connected|2|2|u|n|||", "unused on purpose"])]);
        assert!(project_entries(&[]).is_empty());
    }
}
