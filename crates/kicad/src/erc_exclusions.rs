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

use eda_model::units::Nm;
use eda_model::ir::{ErcExclusion, ErcMarker};
use serde_json::{json, Value};

/// One item a violation names, as the report says it: its uuid, where it is in nanometres and what it is called (`SCH_ITEM::GetItemDescription`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerItem {
    pub uuid: String,
    pub x_nm: Nm,
    pub y_nm: Nm,
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
    // the checks KiCad 25ec2335 (the box's kicad-cli) added (`api/proto/schematic/schematic_rules.proto`)
    ("pin_map_bad_pad", "ERCET_PIN_MAP_BAD_PAD"),
    ("pin_map_unmapped_pin", "ERCET_PIN_MAP_UNMAPPED_PIN"),
    ("pin_map_duplicate_pad", "ERCET_PIN_MAP_DUPLICATE_PAD"),
    ("pin_map_stale_pin", "ERCET_PIN_MAP_STALE_PIN"),
    ("empty_label_name", "ERCET_EMPTY_LABEL_NAME"),
    ("variant_symbol_invalid", "ERCET_VARIANT_SYMBOL_INVALID"),
    ("variant_symbol_incompatible", "ERCET_VARIANT_SYMBOL_INCOMPATIBLE"),
    ("wired_implicit_power", "ERCET_WIRED_IMPLICIT_POWER"),
    ("empty_power_value", "ERCET_EMPTY_POWER_VALUE"),
    ("similar_labels", "ERCET_SIMILAR_LABELS"),
    ("similar_power", "ERCET_SIMILAR_POWER"),
    ("similar_label_and_power", "ERCET_SIMILAR_LABEL_AND_POWER"),
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

/// The markers' own fields, per check: what `SCH_MARKER`'s creator sets (`erc.cpp` and `connection_graph.cpp` line numbers are of the source this was read from:
/// 8303b2ad unless a row says KiCad 25ec2335, the box's kicad-cli, whose new connectivity engine places its markers through `ERC_MARKER_PLACER`; the rows
/// marked [probed] were run against kicad-cli 10.99.0-5334, and `footprint_link_issues` again against 25ec2335, whose matchers accept these forms and are more
/// lenient with a few: a record with no paths still matches, `ResolveERCExclusions`' pathless fallback). The connectivity rows were read from 8303b2ad and
/// were compared line by line with 25ec2335's `ERC_MARKER_PLACER` calls in round 4 and all match (r4, the `r4-schematic` review).
fn rule(check: &str) -> Option<Rule> {
    use At::{Aux, Main};
    use Paths::{Specific, SpecificMain, SpecificMainAux};
    let r = |at, paths| Some(Rule { at, paths });
    match check {
        // KiCad 25ec2335 (the box's kicad-cli; r3 D3-L5) erc.cpp: a symbol's problem, put at the symbol, sheet-specific with the symbol's sheet
        // (`TestLibSymbolIssues` 2986-2990, `TestFootprintLinkIssues` 3060 and the footprint filters 3097 through `ERC_MARKER_PLACER::Report( .., ITEM_PATHS::MAIN )`,
        // `TestSimModelIssues` 3223-3225), and the item with the undefined netclass (1119-1121). (8303b2ad set no sheet in any of them; a record made
        // then still matches, through `ResolveERCExclusions`' pathless fallback.) [`footprint_link_issues` probed]
        "footprint_link_issues" | "footprint_filter" | "simulation_model_issue" | "lib_symbol_issues" | "lib_symbol_mismatch" | "undefined_netclass" => r(Main, SpecificMain),
        // 25ec2335 erc.cpp 398-403: `SetItems( sheet, test_item )`, `SetSheetSpecificPath( path )`, `SetItemsSheetPaths( path, path )`, at the first sheet.
        "duplicate_sheet_names" => r(Main, SpecificMainAux),
        // 25ec2335 erc.cpp 1014-1017: `SetItems( reference, unit )`, put at the second unit, sheet-specific on its sheet, the items on theirs (the report
        // names one sheet, so only a single-sheet report knows the first unit's).
        "different_unit_footprint" => r(Aux, SpecificMainAux),
        // connection_graph.cpp 4161-4166 and 4190-4195 (a symbol's pin), 4656-4661 (a sheet pin): sheet-specific, the item's sheet set, at the pin.
        // (The hierarchical label of the root sheet, 4628-4632, sets no sheet: see [`marker`].)
        // [probed]
        "pin_not_connected" => r(Main, SpecificMain),
        // KiCad 25ec2335 erc.cpp `TestConnectivity`'s `HierarchyErrors` (`MISSING_LABEL`, `MISSING_PIN`): `SetItems( error.item )`, `markers.Place( .., error.sheet,
        // error.position, ITEM_PATHS::MAIN, true )` -- the sheet pin or the hierarchical label, at its own position, sheet-specific (UI-LEFT-2 BB2) [probed].
        "hier_label_mismatch" => r(Main, SpecificMain),
        // KiCad 25ec2335 `TestOffGridEndpoints`: `reportSourceErrors( .., ERCE_ENDPOINT_OFF_GRID, OffGridEndpoints( grid ) )`, `ITEM_PATHS::MAIN`, at the
        // source's position -- a wire's start when it is off the grid, else its end (`ENGINE::OffGridEndpoints`), a symbol's first pin off it. The caller
        // gives a wire's item the end's position ([`off_grid_end`]) [probed].
        "endpoint_off_grid" => r(Main, SpecificMain),
        // `TestConnectivity`'s `DanglingWireEndpoints`: `SetItems( endpoint.item )`, `markers.Place( .., endpoint.sheet, endpoint.position )` -- sheet-specific, no
        // item path, at the dangling end; the caller gives the item that end's position [probed].
        "unconnected_wire_endpoint" => r(Main, Specific),
        // KiCad 25ec2335 erc.cpp `TestDuplicatePinNets` (one path, no engine branch): `SetItems( pinNetPairs[0].first, conflictPin )`,
        // `SetSheetSpecificPath( sheet )`, `SetItemsSheetPaths( sheet, sheet )`, at the first pin [read; no fixture makes it here].
        "duplicate_pins" => r(Main, SpecificMainAux),
        // erc.cpp 1225-1231: the pin that needs a driver [probed: `pin_not_driven`, `power_pin_not_driven`]; 1421-1429: a ground pin on another net; 1460-1465: a
        // pin named like a stacked one.
        "pin_not_driven" | "power_pin_not_driven" | "ground_pin_not_ground" | "stacked_pin_name" => r(Main, SpecificMain),
        // KiCad 25ec2335 erc.cpp 2323-2329: `ercItem->SetItems( pin.item ); markers.Place( .., pin.sheet, pin.position, ITEM_PATHS::MAIN )`; 766 and 818:
        // `reportSourceErrors( .., ERCE_EMPTY_POWER_VALUE / ERCE_EMPTY_LABEL_NAME, .. )`, `markers.Report( aCode, { source.item }, source.sheet, source.position,
        // ITEM_PATHS::MAIN )` (r4 D4-L4).
        "wired_implicit_power" | "empty_label_name" | "empty_power_value" => r(Main, SpecificMain),
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
        // KiCad 25ec2335 erc.cpp `reportNameConflict` (`TestSimilarLabels`, `TestSameLocalGlobalLabel`): `SetItems( aMain.item, aAux.item )`,
        // `Place( item, aMain.sheet, aMain.position, aMain.sheet, aAux.sheet )` -- at the main label or power pin, sheet-specific on its sheet, each item on
        // its own (UI-LEFT B7) [probed on kicad-cli 4fe1ca2060: `similar_labels`, `same_local_global_label`; the other three are the same call].
        "similar_labels" | "similar_power" | "similar_label_and_power" | "same_local_global_label" | "same_local_global_power" => r(Main, SpecificMainAux),
        // KiCad 25ec2335 erc.cpp `TestFourWayJunction` and `TestLabelMultipleWires` (the engine's branch): `markers.Report( .., junction.sheet, junction.position,
        // ITEM_PATHS::NONE )` -- sheet-specific, no item path, at the point; the items the report does not name are the tree's ([`witnesses`]) [probed: four-way].
        "four_way_junction" | "label_multiple_wires" => r(Main, Specific),
        // `TestNoConnectPins` (`NoConnectPinConflicts`, ITEM_PATHS::NONE, at the pin) and `TestConnectivity`'s `NoConnectFlagErrors` (ITEM_PATHS::MAIN, at the pin
        // or the flag): see [`marker_with`], which tells them apart [probed: both].
        "no_connect_connected" => r(Main, Specific),
        // `TestTextVars`: `SetItems( source.item )`, `SetSheetSpecificPath( sheet )`, `SetItemsSheetPaths( sheet )`, at the text (`reportAssertions`: the assertion's
        // position) -- a field's finding names its owner, or the field by its parent and text ([`witnesses`]) [probed: a text, a symbol's field, both kinds].
        "unresolved_variable" | "generic-warning" | "generic-error" => r(Main, SpecificMain),
        // Not here: `field_name_whitespace` -- `TestFieldNameWhitespace` names `{ owner, field }` by the field's own uuid, and a field has none in the file: each
        // load gives it a new one (probed: two runs of kicad-cli on one file, two uuids), and its `ToProto` has no `child` for this check, so no record matches a
        // later run (KiCad's own exclusion of one does not survive reopening the project either). `bus_definition_conflict`: no check of 25ec2335 raises
        // `ERCE_BUS_ALIAS_CONFLICT` (only `erc_item.cpp` defines it), so kicad-cli never reports one.
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
    marker_with(check, severity, items, sheet, sheets, &|_| Vec::new())
}

/// [`marker`] on a design of several sheets, knowing where each item is (`item_sheets`: the sheet paths of an item's uuid, [`item_sheets`]): the sheet of an
/// item the report does not list the violation under -- the aux item of a check whose marker names both items' sheets (`SetItemsSheetPaths( a, b )`: the
/// name checks, a pin-to-pin conflict, a unit's footprint or net), or the item of a check whose marker names only its sheet -- is the one sheet its screen
/// is placed on, or, for a screen placed more than once, the placement the report names when it is one of them. `None` when that still leaves it open.
pub fn marker_with(check: &str, severity: &str, items: &[MarkerItem], sheet: &str, sheets: usize, item_sheets: &dyn Fn(&str) -> Vec<Vec<String>>) -> Option<ErcMarker> {
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
    // A no-connect flag on a connected net (`NoConnectFlagErrors`: `{ pin, flag }` or `{ flag }`, `ITEM_PATHS::MAIN`) names the flag last; a pin of the
    // no-connect type touching other items (`NoConnectPinConflicts`, `ITEM_PATHS::NONE`) never names one.
    let nc_flag = check == "no_connect_connected" && items.last().is_some_and(|i| i.description == "No Connect");
    let paths = if hier_label {
        Paths::None
    } else if nc_flag {
        Paths::SpecificMain
    } else {
        rule.paths
    };
    let single = sheets <= 1;
    let path = uuids_of(sheet);
    // the sheet an item is on: the one placement of its screen, or the report's sheet when it is one of several
    let sheet_of = |i: &MarkerItem| -> Option<Vec<String>> {
        let placed = item_sheets(&i.uuid);
        match placed.as_slice() {
            [one] => Some(one.clone()),
            [] => None,
            many => many.iter().find(|p| **p == path).cloned(),
        }
    };
    let (specific, main_path, aux_path) = match paths {
        Paths::None => (Vec::new(), Vec::new(), Vec::new()),
        Paths::Specific if single && !path.is_empty() => (path, Vec::new(), Vec::new()),
        Paths::Specific => (sheet_of(main)?, Vec::new(), Vec::new()),
        Paths::SpecificMain if !path.is_empty() => (path.clone(), path, Vec::new()),
        Paths::SpecificMainAux if single && !path.is_empty() => (path.clone(), path.clone(), path),
        Paths::SpecificMainAux => {
            // `SetSheetSpecificPath( a )`, `SetItemsSheetPaths( a, b )`: the report lists it under the main item's sheet
            let main_path = sheet_of(main).or_else(|| (!path.is_empty()).then(|| path.clone()))?;
            let aux_path = match items.get(1) {
                Some(aux) => sheet_of(aux)?,
                None => main_path.clone(),
            };
            (main_path.clone(), main_path, aux_path)
        }
        _ => return None,
    };
    Some(ErcMarker { error_type: error_type.to_string(), at_nm: [at.x_nm, at.y_nm], items: items.iter().map(|i| i.uuid.clone()).collect(), sheet: specific, main_sheet: main_path, aux_sheet: aux_path, child: String::new() })
}

/// Whether an exclusion's marker and a finding's are of one sheet (`SCH_MARKER::GetSheetSpecificPath`): the same sheet path, or either not known (an
/// exclusion of a check whose marker this module does not know, or one waived before its marker was kept) -- then the `(check, location)` key alone
/// decides, as it always did.
pub fn same_sheet(a: Option<&ErcMarker>, b: Option<&ErcMarker>) -> bool {
    match (a, b) {
        // (a marker kept with no sheet is of no sheet in particular: one of a check 8303b2ad put no sheet in, r3 D3-L5)
        (Some(a), Some(b)) => a.sheet.is_empty() || b.sheet.is_empty() || a.sheet == b.sheet,
        _ => true,
    }
}

/// Whether an exclusion's marker and a finding's are one marker: [`same_sheet`], and the same position where both are known -- two findings of one item
/// (a wire's two dangling ends) are two markers, as KiCad's exclusions are (`DRC_EXCLUSION`'s whole marker is its key).
pub fn same_marker(a: Option<&ErcMarker>, b: Option<&ErcMarker>) -> bool {
    same_sheet(a, b) && match (a, b) {
        (Some(a), Some(b)) => a.at_nm == b.at_nm || a.error_type != b.error_type,
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
    if !marker.child.is_empty() {
        m["child"] = json!({ "textValue": marker.child });
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
        // `ERC_ITEM::Create( key )` takes the first item type of the key: `pin_to_pin` is the warning (`pinTableWarning` comes before the error)
        let error_type = error_type(check, "warning")?;
        let iu = |i: usize| Nm::new(props.get(i).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0) * eda_model::units::SCH_IU_NM);
        let main = props.get(3).map(|s| s.trim()).unwrap_or("");
        let aux = props.get(4).map(|s| s.trim()).unwrap_or("");
        // `ERC_EXCLUSION::FromLegacyStrings`: a generic or unresolved-variable text names its parent and, as the aux part, the child's text
        let child_text = matches!(check, "generic-warning" | "generic-error" | "unresolved_variable") && !main.is_empty() && main != NIL_UUID && !aux.is_empty() && aux != NIL_UUID;
        let items = if child_text { vec![main.to_string()] } else { [main, aux].into_iter().filter(|u| !u.is_empty() && *u != NIL_UUID).map(String::from).collect() };
        let child = if child_text { aux.to_string() } else { String::new() };
        let p = |i: usize| props.get(i).map(|t| text_path(t)).unwrap_or_default();
        Some((ErcMarker { error_type: error_type.to_string(), at_nm: [iu(1), iu(2)], items, sheet: p(5), main_sheet: p(6), aux_sheet: p(7), child }, comment.to_string()))
    };
    if legacy {
        // `ERC_SETTINGS::migrateSchema0to1` keeps the texts in a `std::set<std::pair<wxString, wxString>>` (`m_ErcExclusionsLegacy`): each (text, comment)
        // once, an empty text left out, sorted -- so of two comments of one text the first in that order is the one `ResolveERCExclusions` keeps (r3 D3-L4)
        let pairs: std::collections::BTreeSet<(&str, &str)> = entries
            .iter()
            .filter_map(|e| match e {
                Value::Array(pair) => Some((pair.first()?.as_str()?, pair.get(1).and_then(Value::as_str).unwrap_or(""))),
                Value::String(text) => Some((text.as_str(), "")),
                _ => None,
            })
            .filter(|(text, _)| !text.is_empty())
            .collect();
        return pairs.into_iter().filter_map(|(text, comment)| from_text(text, comment)).filter(|(m, _)| !m.items.is_empty()).collect();
    }
    entries
        .iter()
        .filter_map(|e| match e {
            Value::Object(_) => {
                let m = e.get("marker")?;
                let error_type = field(m, "error_type", "errorType").as_str()?.to_string();
                check_of(&error_type)?;
                let pos = field(m, "position", "position");
                // protobuf JSON prints a 64-bit integer as a string; a number is taken too
                let nm = |v: Value| Nm::new(v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0));
                let items = m.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| i.get("value").and_then(Value::as_str)).filter(|u| *u != NIL_UUID).map(String::from).collect()).unwrap_or_default();
                let marker = ErcMarker {
                    error_type,
                    at_nm: [nm(field(&pos, "x_nm", "xNm")), nm(field(&pos, "y_nm", "yNm"))],
                    items,
                    sheet: path(field(m, "sheet_specific_path", "sheetSpecificPath")),
                    main_sheet: path(field(m, "main_item_sheet_path", "mainItemSheetPath")),
                    aux_sheet: path(field(m, "aux_item_sheet_path", "auxItemSheetPath")),
                    child: field(&field(m, "child", "child"), "text_value", "textValue").as_str().unwrap_or("").to_string(),
                };
                Some((marker, e.get("comment").and_then(Value::as_str).unwrap_or("").to_string()))
            }
            _ => None,
        })
        .filter(|(m, _)| !m.items.is_empty())
        .collect()
}

/// Where each item of a derived schematic is: every uuid of the tree (a symbol's, its pins', a wire's, a label's, a sheet's and its pins') with the sheet paths
/// (root first, as `KIID_PATH` lists them) of the placements of the screen that holds it, the root's file `root` and each sheet's file read by `read` (its
/// name from the root's folder, `Sheetfile` taken from the placing file's own folder). The report names an item by its uuid and lists the violation under one
/// sheet; a marker that also names the aux item's sheet, or a sheet other than the one the report lists it under, needs this (`SCH_SHEET_PATH` of the
/// item). An item of a screen placed more than once has a path per placement.
pub fn item_sheets(root: &str, read: &dyn Fn(&str) -> Option<String>) -> std::collections::HashMap<String, Vec<Vec<String>>> {
    use crate::sexpr::{find, find_all, tag, txt, Sexpr};
    let mut out: std::collections::HashMap<String, Vec<Vec<String>>> = std::collections::HashMap::new();
    fn uuids(s: &Sexpr, into: &mut Vec<String>) {
        if let Some(list) = s.as_list() {
            if tag(list) == Some("uuid") {
                if let Some(u) = txt(list, 1) {
                    into.push(u.to_string());
                }
                return;
            }
            for child in list {
                uuids(child, into);
            }
        }
    }
    let Some(text) = read(root) else { return out };
    let Some(root_uuid) = file_uuid(&text) else { return out };
    // (file name from the root's folder, its text, the path of this placement); depth-bounded against a sheet that places its own file
    let mut todo: Vec<(String, String, Vec<String>)> = vec![(root.to_string(), text, vec![root_uuid])];
    while let Some((name, text, path)) = todo.pop() {
        let Ok(tree) = crate::sexpr::parse(&text) else { continue };
        let Some(top) = tree.as_list() else { continue };
        for item in top.iter().skip(1) {
            let Some(list) = item.as_list() else { continue };
            if matches!(tag(list), Some("uuid" | "lib_symbols" | "sheet_instances" | "symbol_instances" | "embedded_files" | "title_block" | "paper" | "version" | "generator" | "generator_version")) {
                continue;
            }
            let mut found = Vec::new();
            uuids(item, &mut found);
            for u in found {
                out.entry(u).or_default().push(path.clone());
            }
            if tag(list) == Some("sheet") && path.len() < 32 {
                let sheet_uuid = find(list, "uuid").and_then(|u| txt(u, 1));
                let file = find_all(list, "property").find(|p| matches!(txt(p, 1), Some("Sheetfile" | "Sheet file"))).and_then(|p| txt(p, 2));
                if let (Some(sheet_uuid), Some(file)) = (sheet_uuid, file) {
                    let folder = std::path::Path::new(&name).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                    let child = if folder.is_empty() { file.to_string() } else { format!("{folder}/{file}") };
                    if let Some(child_text) = read(&child) {
                        let mut child_path = path.clone();
                        child_path.push(sheet_uuid.to_string());
                        todo.push((child, child_text, child_path));
                    }
                }
            }
        }
    }
    for paths in out.values_mut() {
        paths.sort();
        paths.dedup();
    }
    out
}

/// The wires and bus entries of a derived tree by uuid: each `(wire ..)` / `(bus ..)` segment's two ends in nanometres (`SCH_LINE`'s `A`, `B`), and the
/// uuids of the `(bus_entry ..)`s (whose ports this module does not place). Every screen is read once (`root`, then each `Sheetfile` from its placing
/// file's folder).
pub fn wire_segments(root: &str, read: &dyn Fn(&str) -> Option<String>) -> (std::collections::HashMap<String, [[i64; 2]; 2]>, std::collections::HashSet<String>) {
    use crate::sexpr::{find, find_all, tag, txt};
    let mut wires = std::collections::HashMap::new();
    let mut entries = std::collections::HashSet::new();
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![root.to_string()];
    let nm = |t: Option<&str>| t.and_then(|v| v.parse::<f64>().ok()).map(|mm| (mm * 1_000_000.0).round() as i64);
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) || seen.len() > 512 {
            continue;
        }
        let Some(text) = read(&name) else { continue };
        let Ok(tree) = crate::sexpr::parse(&text) else { continue };
        let Some(top) = tree.as_list() else { continue };
        for item in top.iter().skip(1) {
            let Some(l) = item.as_list() else { continue };
            let uuid = find(l, "uuid").and_then(|u| txt(u, 1)).map(String::from);
            match tag(l) {
                Some("wire" | "bus") => {
                    let pts: Vec<[i64; 2]> = find(l, "pts").map(|ps| find_all(ps, "xy").filter_map(|p| Some([nm(txt(p, 1))?, nm(txt(p, 2))?])).collect()).unwrap_or_default();
                    if let (Some(u), [a, b]) = (uuid, pts.as_slice()) {
                        wires.insert(u, [*a, *b]);
                    }
                }
                Some("bus_entry") => {
                    if let Some(u) = uuid {
                        entries.insert(u);
                    }
                }
                Some("sheet") => {
                    if let Some(file) = find_all(l, "property").find(|p| matches!(txt(p, 1), Some("Sheetfile" | "Sheet file"))).and_then(|p| txt(p, 2)) {
                        let folder = std::path::Path::new(&name).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                        todo.push(if folder.is_empty() { file.to_string() } else { format!("{folder}/{file}") });
                    }
                }
                _ => {}
            }
        }
    }
    (wires, entries)
}

/// `ENGINE::OffGridEndpoints`' place on a wire: `point.x % aGrid != 0 || point.y % aGrid != 0` -- the start when it is off the grid, else the end.
pub fn off_grid_end(ends: [[i64; 2]; 2], grid_nm: i64) -> [i64; 2] {
    let off = |p: [i64; 2]| grid_nm > 0 && (p[0] % grid_nm != 0 || p[1] % grid_nm != 0);
    if off(ends[0]) {
        ends[0]
    } else {
        ends[1]
    }
}

/// A symbol's pin on its sheet, as the connectivity's `PIN_FACT` has it: its uuid, its symbol's, where its end is in nanometres, and whether it is of the
/// no-connect type, hidden, or a power flag's (`noConnectSources`: a pin of a power symbol that is a power output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinFact {
    pub uuid: String,
    pub owner: String,
    pub at: [i64; 2],
    pub nc: bool,
    pub hidden: bool,
    pub power_flag: bool,
}

/// A field of a symbol, a sheet or a label: its owner's uuid, its name, its text as the file holds it and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldFact {
    pub owner: String,
    pub name: String,
    pub text: String,
    pub at: [i64; 2],
}

/// What one screen of a derived tree holds that a marker the report does not spell out is put together from ([`witnesses`]).
#[derive(Debug, Clone, Default)]
pub struct ScreenFacts {
    pub pins: Vec<PinFact>,
    /// The wires and buses (`SCH_LINE`), each with its two ends.
    pub lines: Vec<(String, [[i64; 2]; 2])>,
    /// Labels of every kind and sheet pins, by uuid, at their connection point, with the label's own text (a sheet pin's name).
    pub labels: Vec<(String, [i64; 2], String)>,
    pub fields: Vec<FieldFact>,
}

/// The screens of a derived tree ([`tree_facts`]) and the screen each item uuid is on.
#[derive(Debug, Clone, Default)]
pub struct TreeFacts {
    pub screens: Vec<ScreenFacts>,
    pub screen_of: std::collections::HashMap<String, usize>,
}

/// [`ScreenFacts`] of every screen of a derived tree (`root`, then each `Sheetfile` from its placing file's folder, each read once): a symbol's pins of its own
/// unit and body style at `SCH_SYMBOL`'s transform of the library pin (`(at)` is the pin's end; the library is y up), the instance's `(pin ..)` uuids given to
/// the library's pins of each number in order (`SCH_SYMBOL::UpdatePins`).
pub fn tree_facts(root: &str, read: &dyn Fn(&str) -> Option<String>) -> TreeFacts {
    use crate::sexpr::{find, find_all, tag, txt, Sexpr};
    let mut facts = TreeFacts::default();
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![root.to_string()];
    let nm = |t: Option<&str>| t.and_then(|v| v.parse::<f64>().ok()).map(|mm| (mm * 1_000_000.0).round() as i64);
    let xy = |l: &[Sexpr]| -> Option<[i64; 2]> { Some([nm(txt(l, 1))?, nm(txt(l, 2))?]) };
    let uuid_of = |l: &[Sexpr]| find(l, "uuid").and_then(|u| txt(u, 1)).map(String::from);
    let fields_of = |l: &[Sexpr], owner: &str, out: &mut Vec<FieldFact>| {
        for p in find_all(l, "property") {
            if let (Some(name), Some(text), Some(at)) = (txt(p, 1), txt(p, 2), find(p, "at").and_then(xy)) {
                out.push(FieldFact { owner: owner.to_string(), name: name.to_string(), text: text.to_string(), at });
            }
        }
    };
    while let Some(name) = todo.pop() {
        if !seen.insert(name.clone()) || seen.len() > 512 {
            continue;
        }
        let Some(text) = read(&name) else { continue };
        let Ok(tree) = crate::sexpr::parse(&text) else { continue };
        let Some(top) = tree.as_list() else { continue };
        // the library pins of each symbol: (unit, body style, number, type, hidden, at)
        type LibPin = (u32, u32, String, String, bool, [i64; 2]);
        let mut libs: std::collections::HashMap<String, (bool, Vec<LibPin>)> = std::collections::HashMap::new();
        if let Some(ls) = find(top, "lib_symbols") {
            for def in find_all(ls, "symbol") {
                let Some(lib_id) = txt(def, 1) else { continue };
                let power = find(def, "power").is_some();
                let mut pins = Vec::new();
                for sub in find_all(def, "symbol") {
                    // `NAME_<unit>_<body style>`
                    let sub_name = txt(sub, 1).unwrap_or("");
                    let mut parts = sub_name.rsplit('_');
                    let style: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    let unit: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    for pin in find_all(sub, "pin") {
                        let (Some(kind), Some(at)) = (txt(pin, 1), find(pin, "at").and_then(xy)) else { continue };
                        let number = find(pin, "number").and_then(|n| txt(n, 1)).unwrap_or("").to_string();
                        let hidden = pin.iter().any(|a| a.text() == Some("hide")) || find(pin, "hide").is_some_and(|h| txt(h, 1) != Some("no"));
                        pins.push((unit, style, number, kind.to_string(), hidden, at));
                    }
                }
                libs.insert(lib_id.to_string(), (power, pins));
            }
        }
        let index = facts.screens.len();
        let mut screen = ScreenFacts::default();
        for item in top.iter().skip(1) {
            let Some(l) = item.as_list() else { continue };
            let uuid = uuid_of(l);
            match tag(l) {
                Some("symbol") => {
                    let (Some(u), Some(lib_id), Some(at)) = (uuid.clone(), find(l, "lib_id").and_then(|x| txt(x, 1)), find(l, "at")) else { continue };
                    let Some(pos) = xy(at) else { continue };
                    let angle = txt(at, 3).and_then(|a| a.parse::<f64>().ok()).unwrap_or(0.0);
                    let xf = eda_model::kicad_geom::Xf::of_instance(angle, find(l, "mirror").and_then(|m| txt(m, 1)));
                    let unit: u32 = find(l, "unit").and_then(|x| txt(x, 1)).and_then(|v| v.parse().ok()).unwrap_or(1);
                    let style: u32 = find(l, "body_style").or_else(|| find(l, "convert")).and_then(|x| txt(x, 1)).and_then(|v| v.parse().ok()).unwrap_or(1);
                    // the instance's pin uuids by number, in order
                    let mut by_number: std::collections::HashMap<&str, Vec<String>> = std::collections::HashMap::new();
                    for p in find_all(l, "pin") {
                        if let (Some(n), Some(pu)) = (txt(p, 1), find(p, "uuid").and_then(|x| txt(x, 1))) {
                            by_number.entry(n).or_default().push(pu.to_string());
                        }
                    }
                    let mut taken: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                    if let Some((power, pins)) = libs.get(lib_id) {
                        for (pu, ps, number, kind, hidden, lat) in pins {
                            if (*pu != 0 && *pu != unit) || (*ps != 0 && *ps != style) {
                                continue;
                            }
                            let nth = taken.entry(number.clone()).or_default();
                            let Some(pin_uuid) = by_number.get(number.as_str()).and_then(|v| v.get(*nth)).cloned() else { continue };
                            *nth += 1;
                            let (lx, ly) = (lat[0], -lat[1]);
                            let p = [pos[0] + xf.x1 as i64 * lx + xf.y1 as i64 * ly, pos[1] + xf.x2 as i64 * lx + xf.y2 as i64 * ly];
                            facts.screen_of.insert(pin_uuid.clone(), index);
                            screen.pins.push(PinFact { uuid: pin_uuid, owner: u.clone(), at: p, nc: kind == "no_connect", hidden: *hidden, power_flag: *power && kind == "power_out" });
                        }
                    }
                    fields_of(l, &u, &mut screen.fields);
                    facts.screen_of.insert(u, index);
                }
                Some("wire" | "bus") => {
                    let pts: Vec<[i64; 2]> = find(l, "pts").map(|ps| find_all(ps, "xy").filter_map(xy).collect()).unwrap_or_default();
                    if let (Some(u), [a, b]) = (uuid, pts.as_slice()) {
                        facts.screen_of.insert(u.clone(), index);
                        screen.lines.push((u, [*a, *b]));
                    }
                }
                Some("label" | "global_label" | "hierarchical_label" | "directive_label" | "netclass_flag") => {
                    if let (Some(u), Some(at)) = (uuid, find(l, "at").and_then(xy)) {
                        fields_of(l, &u, &mut screen.fields);
                        facts.screen_of.insert(u.clone(), index);
                        screen.labels.push((u, at, txt(l, 1).unwrap_or("").to_string()));
                    }
                }
                Some("text") => {
                    if let Some(u) = uuid {
                        facts.screen_of.insert(u, index);
                    }
                }
                Some("sheet") => {
                    if let Some(u) = uuid.clone() {
                        fields_of(l, &u, &mut screen.fields);
                        facts.screen_of.insert(u, index);
                    }
                    for pin in find_all(l, "pin") {
                        if let (Some(pu), Some(at)) = (uuid_of(pin), find(pin, "at").and_then(xy)) {
                            facts.screen_of.insert(pu.clone(), index);
                            screen.labels.push((pu, at, txt(pin, 1).unwrap_or("").to_string()));
                        }
                    }
                    if let Some(file) = find_all(l, "property").find(|p| matches!(txt(p, 1), Some("Sheetfile" | "Sheet file"))).and_then(|p| txt(p, 2)) {
                        let folder = std::path::Path::new(&name).parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                        todo.push(if folder.is_empty() { file.to_string() } else { format!("{folder}/{file}") });
                    }
                }
                _ => {}
            }
        }
        facts.screens.push(screen);
    }
    facts
}

/// The text variables a symbol's or sheet's field resolves without the project (`SCH_SYMBOL::ResolveTextVar`, `SCH_SHEET::ResolveTextVar`, the title block's):
/// a field naming one of these, or another field of its own owner, is not a finding.
const KNOWN_VARS: &[&str] = &[
    "REFERENCE", "VALUE", "FOOTPRINT", "DATASHEET", "DESCRIPTION", "KEYWORDS", "FOOTPRINT_LIBRARY", "FOOTPRINT_NAME", "UNIT", "SHORT_REFERENCE", "SYMBOL_LIBRARY", "SYMBOL_NAME",
    "SYMBOL_DESCRIPTION", "SYMBOL_KEYWORDS", "EXCLUDE_FROM_BOM", "EXCLUDE_FROM_BOARD", "EXCLUDE_FROM_SIM", "DNP", "NET_NAME", "NET_CLASS", "PIN_NAME", "SHEETNAME",
    "SHEETPATH", "SHEETFILE", "FILENAME", "FILEPATH", "PROJECTNAME", "CURRENT_DATE", "ISSUE_DATE", "REVISION", "TITLE", "COMPANY", "PAPER", "LAYER", "#", "##",
    "COMMENT1", "COMMENT2", "COMMENT3", "COMMENT4", "COMMENT5", "COMMENT6", "COMMENT7", "COMMENT8", "COMMENT9", "VARIANT", "VARIANTNAME", "VARIANT_DESCRIPTION",
];

/// The `${..}` references of a text.
fn text_vars(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("${") {
        let after = &rest[i + 2..];
        let Some(j) = after.find('}') else { break };
        out.push(&after[..j]);
        rest = &after[j + 1..];
    }
    out
}

/// The marker's own items and place for the checks whose report names fewer items than the marker, or names an item the file has no uuid for, put together from
/// the derived tree as KiCad 25ec2335's checks make them. `items` are the report's (main first); on `Some`, they are the marker's, the first at the marker's
/// position, and the string is the marker's `child` text (empty for none). `None`: the tree does not say (the marker stays the studio's).
/// - `four_way_junction` (`ENGINE::FourWayJunctions`, `TestFourWayJunction`): at the end of the main item where the others meet it, one pin per
///   symbol (the visible one first, then the least uuid) sorted, then the wires and buses with an end there sorted -- the first four;
/// - `label_multiple_wires` (`LabelWireConflicts`, `TestLabelMultipleWires`): the label, then up to three wires sorted whose inside (not an end) holds it;
/// - `no_connect_connected` of a pin (`NoConnectPinConflicts`, `TestNoConnectPins`): at the pin, up to three no-connect pins there sorted, then the items they
///   touch sorted -- pins of another type (not a power flag's), wire and bus ends, labels and sheet pins; a no-connect flag and a junction are not among them
///   (probed) -- four at most; the flag's own finding names its items as the report does;
/// - `unresolved_variable` of a symbol or a sheet (`TestTextVars`: the field's source names its owner, at the field): the one field of the owner whose text
///   holds a variable it cannot resolve;
/// - `generic-warning` / `generic-error` of a field (`SCH_MARKER`'s `ToProto`: a field has no uuid in the file, so its parent's uuid and the field's text,
///   `ErcSymbolChildReference`): the field at the report's place.
pub fn witnesses(facts: &TreeFacts, check: &str, items: &mut Vec<MarkerItem>) -> Option<String> {
    let main = items.first()?.clone();
    let screen = facts.screen_of.get(&main.uuid).and_then(|&i| facts.screens.get(i));
    let set_at = |items: &mut Vec<MarkerItem>, at: [i64; 2]| {
        if let Some(first) = items.first_mut() {
            (first.x_nm, first.y_nm) = (Nm::new(at[0]), Nm::new(at[1]));
        }
    };
    let item = |uuid: &str, at: [i64; 2]| MarkerItem { uuid: uuid.to_string(), x_nm: Nm::new(at[0]), y_nm: Nm::new(at[1]), description: String::new() };
    // the report names the marker's first two items: what is put together must start with them
    let agrees = |built: &[String], items: &[MarkerItem]| items.iter().zip(built).all(|(a, b)| a.uuid == *b) && built.len() >= items.len();
    match check {
        "four_way_junction" => {
            let screen = screen?;
            // the point is where the main item meets the others: a pin's end, or an end of a wire (the message's "at %d, %d" is in a unit of its own)
            let candidates: Vec<[i64; 2]> = match screen.lines.iter().find(|(u, _)| *u == main.uuid) {
                Some((_, ends)) => ends.to_vec(),
                None => screen.pins.iter().filter(|p| p.uuid == main.uuid).map(|p| p.at).collect(),
            };
            let at_point = |at: [i64; 2]| -> Vec<String> {
                let mut per_owner: std::collections::BTreeMap<&str, &PinFact> = std::collections::BTreeMap::new();
                for p in screen.pins.iter().filter(|p| p.at == at) {
                    let pick = per_owner.entry(p.owner.as_str()).or_insert(p);
                    if (p.hidden, &p.uuid) < (pick.hidden, &pick.uuid) {
                        *pick = p;
                    }
                }
                let mut pins: Vec<String> = per_owner.values().map(|p| p.uuid.clone()).collect();
                pins.sort();
                let mut lines: Vec<String> = screen.lines.iter().filter(|(_, e)| e[0] == at || e[1] == at).map(|(u, _)| u.clone()).collect();
                lines.sort();
                lines.dedup();
                pins.into_iter().chain(lines).take(4).collect()
            };
            let mut found: Vec<([i64; 2], Vec<String>)> = candidates.into_iter().map(|at| (at, at_point(at))).filter(|(_, b)| b.len() == 4 && agrees(b, items)).collect();
            found.dedup();
            let [(at, built)] = found.as_slice() else { return None };
            let at = *at;
            *items = built.iter().map(|u| item(u, at)).collect();
            Some(String::new())
        }
        "label_multiple_wires" => {
            let screen = screen?;
            let at = [main.x_nm.get(), main.y_nm.get()];
            let inside = |e: &[[i64; 2]; 2]| {
                let (a, b) = (e[0], e[1]);
                let cross = (b[0] - a[0]) as i128 * (at[1] - a[1]) as i128 - (b[1] - a[1]) as i128 * (at[0] - a[0]) as i128;
                cross == 0 && at != a && at != b && at[0] >= a[0].min(b[0]) && at[0] <= a[0].max(b[0]) && at[1] >= a[1].min(b[1]) && at[1] <= a[1].max(b[1])
            };
            let mut wires: Vec<String> = screen.lines.iter().filter(|(_, e)| inside(e)).map(|(u, _)| u.clone()).collect();
            wires.sort();
            if wires.len() < 2 {
                return None;
            }
            let built: Vec<String> = std::iter::once(main.uuid.clone()).chain(wires.into_iter().take(3)).collect();
            if !agrees(&built, items) {
                return None;
            }
            *items = built.iter().map(|u| item(u, at)).collect();
            Some(String::new())
        }
        "no_connect_connected" if items.last().is_some_and(|i| i.description == "No Connect") => Some(String::new()),
        "no_connect_connected" => {
            let screen = screen?;
            let at = [main.x_nm.get(), main.y_nm.get()];
            let mut pins: Vec<String> = screen.pins.iter().filter(|p| p.at == at && p.nc).map(|p| p.uuid.clone()).collect();
            let mut others: Vec<String> = screen.pins.iter().filter(|p| p.at == at && !p.nc && !p.power_flag).map(|p| p.uuid.clone()).collect();
            others.extend(screen.lines.iter().filter(|(_, e)| e[0] == at || e[1] == at).map(|(u, _)| u.clone()));
            others.extend(screen.labels.iter().filter(|(_, p, _)| *p == at).map(|(u, _, _)| u.clone()));
            pins.sort();
            others.sort();
            others.dedup();
            let np = pins.len().min(3);
            let built: Vec<String> = pins.into_iter().take(np).chain(others.into_iter().take(4 - np)).collect();
            if !agrees(&built, items) {
                return None;
            }
            *items = built.iter().map(|u| item(u, at)).collect();
            Some(String::new())
        }
        "unresolved_variable" => {
            let Some(screen) = screen else { return Some(String::new()) };
            // a symbol or a sheet: the field; a label or a text: the item itself, at its place
            if screen.labels.iter().any(|(u, _, _)| *u == main.uuid) || !screen.fields.iter().any(|f| f.owner == main.uuid) {
                return Some(String::new());
            }
            let own: Vec<&FieldFact> = screen.fields.iter().filter(|f| f.owner == main.uuid).collect();
            let resolves = |var: &str| {
                let v = var.to_ascii_uppercase();
                KNOWN_VARS.contains(&v.as_str()) || own.iter().any(|f| f.name.to_ascii_uppercase() == v) || var.starts_with("ERC_")
            };
            let unresolved: Vec<&&FieldFact> = own.iter().filter(|f| text_vars(&f.text).iter().any(|v| !resolves(v))).collect();
            let [field] = unresolved.as_slice() else { return None };
            set_at(items, field.at);
            Some(String::new())
        }
        "generic-warning" | "generic-error" if !facts.screen_of.contains_key(&main.uuid) => {
            let at = [main.x_nm.get(), main.y_nm.get()];
            let hits: Vec<&FieldFact> = facts.screens.iter().flat_map(|s| s.fields.iter()).filter(|f| f.at == at && f.text.contains("${ERC_")).collect();
            let [field] = hits.as_slice() else { return None };
            *items = vec![item(&field.owner, at)];
            Some(field.text.clone())
        }
        _ => Some(String::new()),
    }
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
        // KiCad 25ec2335 `SCHEMATIC::ResolveERCExclusions`: "Legacy format can sometimes have the same exclusion multiple times, without and with a comment.
        // If this happens, replace the existing one if we can go from no comment to comment" (r2 D-E2)
        match out.iter_mut().find(|o| o.check == e.check && o.location == e.location && o.marker == e.marker) {
            Some(o) if o.comment.is_empty() && !e.comment.is_empty() => o.comment = e.comment,
            Some(_) => {}
            None => out.push(e),
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use eda_model::units::Nm;
    use super::*;

    const ROOT: &str = "/172726e6-8494-4d0f-8495-8bee6ad09c57";
    const ROOT_UUID: &str = "172726e6-8494-4d0f-8495-8bee6ad09c57";

    fn item(uuid: &str, x_nm: i64, y_nm: i64, description: &str) -> MarkerItem {
        MarkerItem { uuid: uuid.into(), x_nm: Nm::new(x_nm), y_nm: Nm::new(y_nm), description: description.into() }
    }

    fn excl(check: &str, location: &str, marker: Option<ErcMarker>, comment: &str) -> ErcExclusion {
        ErcExclusion { check: check.into(), location: location.into(), marker, comment: comment.into() }
    }

    fn only(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    /// UI-LEFT-2 BB2: on a design of several sheets, each item is on the sheet its screen is placed on; an item of a screen placed twice has both paths.
    #[test]
    fn the_items_of_a_tree_are_on_the_sheets_their_screen_is_placed_on() {
        let root = "(kicad_sch (version 1) (uuid \"R\") (symbol (lib_id \"x\") (uuid \"s1\") (pin \"1\" (uuid \"p1\")))
            (sheet (at 0 0) (uuid \"A\") (property \"Sheetname\" \"a\") (property \"Sheetfile\" \"sub/child.kicad_sch\") (pin \"D\" input (uuid \"sp\")))
            (sheet (at 9 0) (uuid \"B\") (property \"Sheetname\" \"b\") (property \"Sheetfile\" \"sub/child.kicad_sch\"))
            (sheet (at 9 9) (uuid \"C\") (property \"Sheetname\" \"c\") (property \"Sheetfile\" \"other.kicad_sch\"))
            (sheet_instances (path \"/\" (page \"1\"))))";
        let child = "(kicad_sch (version 1) (uuid \"K\") (label \"x\" (at 0 0 0) (uuid \"l1\")))";
        let other = "(kicad_sch (version 1) (uuid \"O\") (wire (pts (xy 0 0) (xy 1 0)) (uuid \"w1\")))";
        let read = |name: &str| match name {
            "board.kicad_sch" => Some(root.to_string()),
            "sub/child.kicad_sch" => Some(child.to_string()),
            "other.kicad_sch" => Some(other.to_string()),
            _ => None,
        };
        let at = item_sheets("board.kicad_sch", &read);
        assert_eq!(at["s1"], vec![only(&["R"])]);
        assert_eq!(at["p1"], vec![only(&["R"])], "a symbol's pin is on its symbol's sheet");
        assert_eq!(at["sp"], vec![only(&["R"])], "a sheet's pin is on the placing sheet");
        assert_eq!(at["w1"], vec![only(&["R", "C"])]);
        assert_eq!(at["l1"], vec![only(&["R", "A"]), only(&["R", "B"])], "a screen placed twice: a path per placement");

        // A name check between the root and the sheet: the main item's sheet is the report's, the aux item's is where it is.
        let items = [item("s1", 1, 2, "Label 'SIG'"), item("w1", 3, 4, "Label 'sig'")];
        let sheets = |u: &str| at.get(u).cloned().unwrap_or_default();
        let m = marker_with("similar_labels", "warning", &items, "/R", 3, &sheets).unwrap();
        assert_eq!((m.sheet.clone(), m.main_sheet.clone(), m.aux_sheet.clone()), (only(&["R"]), only(&["R"]), only(&["R", "C"])));
        assert_eq!(marker("similar_labels", "warning", &items, "/R", 3), None, "without the tree the aux item's sheet is not known");
        // An item of a screen placed twice: the report's placement when it is one of them, else not known.
        let label = [item("l1", 1, 2, "Label 'x'")];
        assert_eq!(marker_with("label_dangling", "error", &label, "/R/B", 3, &sheets).unwrap().sheet, only(&["R", "B"]));
        assert_eq!(marker_with("label_dangling", "error", &label, "/R", 3, &sheets), None);
    }

    /// UI-LEFT-3 B3-2: the markers whose report names fewer items than the marker, or a field by a uuid the file does not keep, are put together from the tree:
    /// a symbol's pins at their ends (the library is y up), the wires at a point, a label on two wires' insides, a field by its owner and text.
    #[test]
    fn the_tree_gives_the_items_a_marker_names_beyond_the_report() {
        let sheet = "(kicad_sch (version 1) (uuid \"R\")
            (lib_symbols (symbol \"T:X\" (power) (symbol \"X_1_1\" (pin passive line (at -3.81 0 0) (length 2.54) (number \"1\")) (pin no_connect line (at 3.81 0 180) (length 2.54) (number \"2\")))))
            (symbol (lib_id \"T:X\") (at 50.8 50.8 90) (unit 1) (uuid \"s1\") (property \"Note\" \"${NOPE} ${VALUE}\" (at 50.8 57 0)) (property \"Chk\" \"${ERC_ERROR boom}\" (at 50.8 59 0))
                (pin \"1\" (uuid \"p1\")) (pin \"2\" (uuid \"p2\")))
            (wire (pts (xy 50.8 46.99) (xy 50.8 40)) (uuid \"wa\")) (wire (pts (xy 50.8 54.61) (xy 50.8 60)) (uuid \"wb\")) (label \"L\" (at 50.8 54.61 0) (uuid \"lb\"))
            (wire (pts (xy 10 10) (xy 20 10)) (uuid \"w1\")) (wire (pts (xy 20 10) (xy 30 10)) (uuid \"w2\")) (wire (pts (xy 20 0) (xy 20 10)) (uuid \"w3\")) (wire (pts (xy 20 20) (xy 20 10)) (uuid \"w4\"))
            (wire (pts (xy 0 30) (xy 10 30)) (uuid \"h1\")) (wire (pts (xy 5 25) (xy 5 35)) (uuid \"h2\")) (label \"M\" (at 5 30 0) (uuid \"lm\")))";
        let facts = tree_facts("s.kicad_sch", &|n: &str| (n == "s.kicad_sch").then(|| sheet.to_string()));
        let pins = &facts.screens[0].pins;
        // turned 90 degrees: the pin at (-3.81, 0) of the library is below the symbol on the page, the one at (3.81, 0) above... in y down
        assert_eq!(pins.iter().map(|p| (p.uuid.as_str(), p.at, p.nc)).collect::<Vec<_>>(), vec![("p1", [50_800_000, 54_610_000], false), ("p2", [50_800_000, 46_990_000], true)]);
        let mi = |u: &str, x: f64, y: f64, d: &str| item(u, (x * 1e6) as i64, (y * 1e6) as i64, d);
        // four wire ends at (20, 10): the first four lines sorted
        let mut four = vec![mi("w1", 10.0, 10.0, "Horizontal Wire"), mi("w2", 20.0, 10.0, "Horizontal Wire")];
        assert_eq!(witnesses(&facts, "four_way_junction", &mut four), Some(String::new()));
        assert_eq!(four.iter().map(|i| i.uuid.as_str()).collect::<Vec<_>>(), ["w1", "w2", "w3", "w4"]);
        assert_eq!((four[0].x_nm, four[0].y_nm), (Nm::new(20_000_000), Nm::new(10_000_000)), "at the point they meet");
        // a no-connect pin touching a wire end and a label: the pin, then the others sorted
        let mut nc = vec![mi("p2", 50.8, 46.99, "Symbol U1 Pin 2"), mi("wa", 50.8, 46.99, "Vertical Wire")];
        assert!(witnesses(&facts, "no_connect_connected", &mut nc).is_some());
        assert_eq!(nc.iter().map(|i| i.uuid.as_str()).collect::<Vec<_>>(), ["p2", "wa"]);
        let mut wrong = vec![mi("p2", 50.8, 46.99, "Symbol U1 Pin 2"), mi("zz", 50.8, 46.99, "Vertical Wire")];
        assert_eq!(witnesses(&facts, "no_connect_connected", &mut wrong), None, "the report's items must lead the built list");
        // a label on the insides of two crossing wires
        let mut lab = vec![mi("lm", 5.0, 30.0, "Label 'M'"), mi("h1", 0.0, 30.0, "Horizontal Wire")];
        assert!(witnesses(&facts, "label_multiple_wires", &mut lab).is_some());
        assert_eq!(lab.iter().map(|i| i.uuid.as_str()).collect::<Vec<_>>(), ["lm", "h1", "h2"]);
        // the symbol's one field with a variable it cannot resolve (`${VALUE}` it can): the marker is at the field
        let mut unres = vec![mi("s1", 50.8, 50.8, "Symbol U1 [X]")];
        assert!(witnesses(&facts, "unresolved_variable", &mut unres).is_some());
        assert_eq!((unres[0].uuid.as_str(), unres[0].y_nm), ("s1", Nm::new(57_000_000)));
        // an assertion in a field: the field's parent, and the field's text as the child
        let mut generic = vec![mi("random-field-uuid", 50.8, 59.0, "Field Chk")];
        assert_eq!(witnesses(&facts, "generic-error", &mut generic), Some("${ERC_ERROR boom}".to_string()));
        assert_eq!(generic.iter().map(|i| i.uuid.as_str()).collect::<Vec<_>>(), ["s1"]);
        let m = marker("generic-error", "error", &generic, "/R", 1).map(|m| ErcMarker { child: "${ERC_ERROR boom}".into(), ..m }).unwrap();
        assert_eq!(entry(&m, "")["marker"]["child"], json!({ "textValue": "${ERC_ERROR boom}" }));
        // the record reads back with its child
        let pro = json!({ "erc": { "meta": { "version": 1 }, "erc_exclusions": [entry(&m, "c")] } }).to_string();
        assert_eq!(read_project(&pro), vec![(m.clone(), "c".to_string())]);
        // a no-connect flag's own finding is the item's sheet's (`ITEM_PATHS::MAIN`); a pin's is the sheet's only
        let flag = marker("no_connect_connected", "warning", &[mi("p1", 1.0, 2.0, "Symbol U2 Pin 1"), mi("nc", 1.0, 2.0, "No Connect")], "/R", 1).unwrap();
        assert_eq!((flag.sheet.clone(), flag.main_sheet.clone()), (only(&["R"]), only(&["R"])));
        let pin = marker("no_connect_connected", "warning", &nc, "/R", 1).unwrap();
        assert_eq!((pin.sheet.clone(), pin.main_sheet.clone()), (only(&["R"]), vec![]));
        assert_eq!(marker("field_name_whitespace", "warning", &[mi("s1", 1.0, 1.0, "Symbol"), mi("f", 1.0, 1.0, "Field")], "/R", 1), None, "a field's uuid is new on each load");
    }

    /// `ERC_EXCLUSION::FromLegacyStrings`: a generic or unresolved-variable text names the parent and the child's text.
    #[test]
    fn a_legacy_generic_exclusion_keeps_its_childs_text() {
        let text = "generic-error|500|590|s1|${ERC_ERROR boom}|/R|/R|";
        let pro = json!({ "erc": { "meta": { "version": 0 }, "erc_exclusions": [text] } }).to_string();
        let got = read_project(&pro);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].0.items.clone(), got[0].0.child.as_str()), (only(&["s1"]), "${ERC_ERROR boom}"));
    }

    /// Records kicad-cli 10.99.0-5334 consumed when they were written to a project (probed on a derived schematic: the violation was reported as excluded, with the
    /// comment, and no marker of its own was left beside it).
    #[test]
    fn a_pin_not_connected_marker_is_sheet_specific_with_the_items_sheet_set_and_is_written_as_protobuf_json() {
        let pin = item("693a38f5-591c-4c06-9786-e8973d13af99", 115_570_000, 110_490_000, "Symbol #PWR03 Pin 1 [Power input, Line]");
        let m = marker("pin_not_connected", "error", &[pin], ROOT, 1).unwrap();
        assert_eq!(
            m,
            ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [mm(115.57), mm(110.49)], items: only(&["693a38f5-591c-4c06-9786-e8973d13af99"]), sheet: only(&[ROOT_UUID]), main_sheet: only(&[ROOT_UUID]), aux_sheet: vec![], child: String::new() }
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

    /// r4 D4-L4 (KiCad 25ec2335 erc.cpp 2323-2329, 766, 818: `ITEM_PATHS::MAIN` at the item): the three checks 201f2183 added have a marker, sheet-specific with
    /// the item's sheet, so an exclusion of one reaches kicad-cli.
    #[test]
    fn the_new_checks_have_markers() {
        let pin = item("693a38f5-591c-4c06-9786-e8973d13af99", 1_000_000, 2_000_000, "Symbol U1 Pin 7 [Power input, Line]");
        for check in ["wired_implicit_power", "empty_label_name", "empty_power_value"] {
            let m = marker(check, "warning", std::slice::from_ref(&pin), ROOT, 1).unwrap_or_else(|| panic!("{check}"));
            assert_eq!((m.sheet.clone(), m.main_sheet.clone(), m.aux_sheet.clone()), (only(&[ROOT_UUID]), only(&[ROOT_UUID]), vec![]), "{check}");
            assert_eq!(m.at_nm, [Nm::new(1_000_000), Nm::new(2_000_000)]);
        }
    }

    /// r2 D-E2: a legacy (schema 0) exclusion listed twice, first without and then with a comment, keeps the comment (`ResolveERCExclusions`).
    #[test]
    fn a_legacy_exclusion_listed_twice_keeps_its_comment() {
        let text = "pin_not_connected|13335|8128|f-pin|00000000-0000-0000-0000-000000000000|||";
        let pro = json!({ "erc": { "meta": { "version": 0 }, "erc_exclusions": [text, [text, "why"]] } }).to_string();
        let ids: std::collections::BTreeMap<String, String> = [("f-pin".to_string(), "U1.1".to_string())].into_iter().collect();
        let derived: std::collections::HashMap<String, String> = [("d-pin".to_string(), "U1.1".to_string())].into_iter().collect();
        let got = from_project(&pro, &ids, &derived, "", "");
        assert_eq!(got.iter().map(|e| e.comment.as_str()).collect::<Vec<_>>(), ["why"]);
        // r3 D3-L4: with two comments, the first of the sorted set (`m_ErcExclusionsLegacy`), not of the file
        let pro = json!({ "erc": { "meta": { "version": 0 }, "erc_exclusions": [[text, "b"], [text, "a"]] } }).to_string();
        assert_eq!(from_project(&pro, &ids, &derived, "", "").iter().map(|e| e.comment.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    /// r3 D3-L5: in KiCad 25ec2335 a symbol's footprint link issue is sheet-specific, with the symbol's sheet (`ERC_MARKER_PLACER::Report( .., ITEM_PATHS::MAIN )`);
    /// 8303b2ad's had no sheet, and a record of then still matches one of now (`same_sheet`, and kicad-cli's pathless fallback).
    #[test]
    fn a_symbols_footprint_link_issue_names_the_symbols_sheet() {
        let symbol = item("a4f97020-e09b-4e24-b611-b376f3c27df7", 113_030_000, 100_330_000, "Symbol C1 [C]");
        let m = marker("footprint_link_issues", "warning", &[symbol.clone()], ROOT, 1).unwrap();
        assert_eq!((m.error_type.as_str(), m.at_nm), ("ERCET_FOOTPRINT_LINK_ISSUES", [mm(113.03), mm(100.33)]));
        assert_eq!((m.sheet.clone(), m.main_sheet.clone(), m.aux_sheet.clone()), (only(&[ROOT_UUID]), only(&[ROOT_UUID]), vec![]));
        // on a design of many sheets the report lists it under its sheet
        assert_eq!(marker("footprint_link_issues", "warning", &[symbol], "/r/s", 3).unwrap().sheet, only(&["r", "s"]));
        let old = ErcMarker { sheet: vec![], main_sheet: vec![], ..m.clone() };
        assert!(same_sheet(Some(&old), Some(&m)), "a marker kept before is of no sheet in particular");
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
        assert_eq!((p.error_type.as_str(), p.at_nm, p.items.len(), p.aux_sheet.len()), ("ERCET_PIN_TO_PIN_WARNING", [Nm::new(10), Nm::new(20)], 2, 1));
        assert_eq!(marker("pin_to_pin", "error", &[a.clone(), b.clone()], ROOT, 1).unwrap().error_type, "ERCET_PIN_TO_PIN_ERROR");
        let d = marker("multiple_net_names", "error", &[a.clone(), b], ROOT, 1).unwrap();
        assert_eq!((d.error_type.as_str(), d.at_nm), ("ERCET_DRIVER_CONFLICT", [Nm::new(30), Nm::new(40)]));
        assert!(marker("multiple_net_names", "error", &[a], ROOT, 1).is_none(), "a pair check with one item named: the position is the second item's, which is not there");
    }

    #[test]
    fn a_root_sheets_hierarchical_label_has_no_sheet_in_its_pin_not_connected() {
        let label = item("l", 5, 6, "Hierarchical Label 'IN' [Input]");
        let m = marker("pin_not_connected", "error", &[label], ROOT, 1).unwrap();
        assert!(m.sheet.is_empty() && m.main_sheet.is_empty());
    }

    #[test]
    fn two_labels_of_one_name_are_a_marker_at_the_main_one_with_each_items_sheet() {
        // `reportNameConflict`: at the main item, the sheet-specific path and both items' paths; known when the design has one sheet.
        let (a, b) = (item("lab-a", 60, 50, "Label 'abc'"), item("lab-b", 60, 76, "Label 'ABC'"));
        for check in ["similar_labels", "similar_power", "similar_label_and_power", "same_local_global_label", "same_local_global_power"] {
            let m = marker(check, "warning", &[a.clone(), b.clone()], ROOT, 1).unwrap_or_else(|| panic!("{check}"));
            assert_eq!((m.at_nm, m.items.clone()), ([a.x_nm, a.y_nm], vec!["lab-a".to_string(), "lab-b".to_string()]), "{check}");
            assert!(!m.sheet.is_empty() && m.main_sheet == m.sheet && m.aux_sheet == m.sheet, "{check}: {m:?}");
            assert!(marker(check, "warning", &[a.clone(), b.clone()], "/r/s", 2).is_none(), "{check}: the aux label's sheet is not in a report of two sheets");
        }
        assert_eq!(marker("similar_labels", "warning", &[a.clone(), b.clone()], ROOT, 1).unwrap().error_type, "ERCET_SIMILAR_LABELS");
    }

    #[test]
    fn a_check_whose_marker_this_has_not_read_the_source_of_has_no_marker() {
        // (`endpoint_off_grid` and `unconnected_wire_endpoint` have a rule now; the caller gives a wire's item the end's position)
        // (`four_way_junction`, `label_multiple_wires`, `no_connect_connected`, `unresolved_variable`, `generic-*` have one now, their items from the tree: `witnesses`)
        for check in ["field_name_whitespace", "bus_definition_conflict"] {
            assert!(marker(check, "error", &[item("a", 1, 2, "Wire")], ROOT, 1).is_none(), "{check}");
        }
        assert!(marker("footprint_link_issues", "warning", &[], ROOT, 1).is_none(), "a violation naming no item cannot be keyed");
        assert!(marker("footprint_link_issues", "warning", &[item("", 1, 2, "Symbol")], ROOT, 1).is_none());
    }

    #[test]
    fn every_check_with_a_marker_rule_has_a_kind_of_finding_and_the_names_are_kicads() {
        for check in ["footprint_link_issues", "simulation_model_issue", "lib_symbol_issues", "lib_symbol_mismatch", "duplicate_sheet_names", "undefined_netclass", "different_unit_footprint", "pin_not_connected", "pin_not_driven", "power_pin_not_driven", "ground_pin_not_ground", "stacked_pin_name", "missing_unit", "missing_input_pin", "missing_bidi_pin", "missing_power_pin", "no_connect_dangling", "single_global_label", "wire_dangling", "label_dangling", "isolated_pin_label", "bus_to_net_conflict", "bus_to_bus_conflict", "net_not_bus_member", "pin_to_pin", "different_unit_net", "multiple_net_names", "similar_labels", "similar_power", "similar_label_and_power", "same_local_global_label", "same_local_global_power", "four_way_junction", "label_multiple_wires", "no_connect_connected", "unresolved_variable", "generic-warning", "generic-error", "duplicate_pins"] {
            assert!(rule(check).is_some() && error_type(check, "error").is_some(), "{check}");
        }
        // `ERCE_` of erc_item.cpp with the library's `ERCET_`; two spellings that differ.
        assert_eq!(error_type("hier_label_mismatch", "error"), Some("ERCET_HIERARCHICAL_LABEL"));
        assert_eq!(error_type("unit_value_mismatch", "error"), Some("ERCET_DIFFERENT_UNIT_VALUE"));
        assert_eq!(error_type("multiple_net_names", "error"), Some("ERCET_DRIVER_CONFLICT"));
        assert_eq!(error_type("pin_to_pin", "warning"), Some("ERCET_PIN_TO_PIN_WARNING"));
        assert_eq!(error_type("not_a_check", "error"), None);
        assert!(ERROR_TYPES.iter().all(|(_, t)| t.starts_with("ERCET_")));
        // every check of the running kicad-cli's table has its kind (r3 D3-H1: the nine 25ec2335 added among them)
        for (check, _) in eda_model::erc_checks::ERC_CHECKS {
            assert!(error_type(check, "error").is_some(), "{check}");
        }
    }

    #[test]
    fn the_project_lists_each_known_marker_with_its_comment_and_leaves_the_studios_own_out() {
        let sym = |u: &str| ErcMarker { error_type: "ERCET_FOOTPRINT_LINK_ISSUES".into(), at_nm: [Nm::new(1), Nm::new(1)], items: vec![u.into()], ..Default::default() };
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
        let marker = ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [Nm::new(1000), Nm::new(2000)], items: only(&["d-pin"]), sheet: only(&["d-root", "s1"]), main_sheet: only(&["d-root", "s1"]), aux_sheet: vec![], child: String::new() };
        assert_eq!(got, vec![excl("pin_not_connected", "U1.1", Some(marker), "kept")], "the record whose second item is not in the import is left out, and the text is not read at schema 1");
        // schema 0: the texts, and not the records
        let legacy = json!({ "erc": { "meta": { "version": 0 }, "erc_exclusions": [["pin_not_connected|13335|8128|f-pin|00000000-0000-0000-0000-000000000000|||", "comment1"], "generic-warning|1|1|f-pin|some text|||", "bad|1"] } }).to_string();
        let got = from_project(&legacy, &file_ids, &derived, "f-root", "d-root");
        let marker = ErcMarker { error_type: "ERCET_PIN_NOT_CONNECTED".into(), at_nm: [Nm::new(1_333_500), Nm::new(812_800)], items: only(&["d-pin"]), ..Default::default() };
        // a generic text names its parent and the child's text (`FromLegacyStrings`, UI-LEFT-3 B3-2)
        let generic = ErcMarker { error_type: "ERCET_GENERIC_WARNING".into(), at_nm: [Nm::new(100), Nm::new(100)], items: only(&["d-pin"]), child: "some text".into(), ..Default::default() };
        assert_eq!(got, vec![excl("generic-warning", "U1.1", Some(generic), ""), excl("pin_not_connected", "U1.1", Some(marker), "comment1")]);
        let pin_to_pin = json!({ "erc": { "meta": { "version": 0 }, "erc_exclusions": ["pin_to_pin|1|1|f-pin|00000000-0000-0000-0000-000000000000|||"] } }).to_string();
        assert_eq!(read_project(&pin_to_pin)[0].0.error_type, "ERCET_PIN_TO_PIN_WARNING", "a legacy pin_to_pin text is the warning");
        assert_eq!(file_uuid("(kicad_sch (version 20250114) (uuid \"ab-cd\"))").as_deref(), Some("ab-cd"));
    }
}
