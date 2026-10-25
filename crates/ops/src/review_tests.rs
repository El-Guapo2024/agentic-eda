//! The DRC and ERC review verbs: a waived violation (`design.drawings.drc_exclusions`) and the schematic's
//! per-check severities (`design.schematic.extras.erc_severities`).

use super::*;
use eda_model::ir::{DrcExclusion, DrcExclusionKey, PlacementSection, Provenance};
use eda_model::units::{mm, um};

fn design() -> Design {
    Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        routing: None,
        placement: Some(PlacementSection { outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(50.0), y: Nm::ZERO }, Point { x: mm(50.0), y: mm(50.0) }, Point { x: Nm::ZERO, y: mm(50.0) }], footprints: vec![], modules: vec![] }),
        drawings: None,
    }
}

fn exclusion(check: &str, items: &[&str]) -> DrcExclusion {
    DrcExclusion { check: check.into(), items: items.iter().map(|s| s.to_string()).collect(), ids: vec![], positions_nm: vec![], comment: String::new() }
}

fn waived(b: &Board) -> Vec<DrcExclusion> {
    b.design().drawings.as_ref().map(|d| d.drc_exclusions.clone()).unwrap_or_default()
}

fn key(check: &str, items: &[&str]) -> DrcExclusionKey {
    DrcExclusionKey { check: check.into(), items: items.iter().map(|s| s.to_string()).collect() }
}

#[test]
fn a_waived_violation_is_stored_sorted_and_adding_it_again_edits_its_comment() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    assert!(waived(&b).is_empty());
    let mut a = exclusion("clearance", &["u-2", "u-1"]);
    a.positions_nm = vec![[mm(0.3), um(4.0)], [mm(0.1), um(2.0)], [mm(0.3), um(4.0)]];
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![exclusion("track_width", &["u-9"]), a] }).unwrap();
    let list = waived(&b);
    assert_eq!(list.iter().map(|e| e.check.as_str()).collect::<Vec<_>>(), vec!["clearance", "track_width"], "kept in order, one entry for each");
    assert_eq!(list[0].positions_nm, vec![[mm(0.1), um(2.0)], [mm(0.3), um(4.0)]], "the marker positions to try are sorted and listed once");

    // The same violation again (a comment typed in "Edit exclusion comment...") replaces the entry; it does not pile up.
    let mut again = exclusion("clearance", &["u-2", "u-1"]);
    again.comment = "board edge is a slot on purpose".into();
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![again] }).unwrap();
    let list = waived(&b);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].comment, "board edge is a slot on purpose");
    assert!(list[0].positions_nm.is_empty(), "the replacement is the whole entry");

    // The order of the items is part of the violation: main item, then aux.
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![exclusion("clearance", &["u-1", "u-2"])] }).unwrap();
    assert_eq!(waived(&b).len(), 3);
}

#[test]
fn an_exclusion_is_removed_by_its_key_and_removing_one_that_is_not_there_is_refused() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![exclusion("clearance", &["a", "b"]), exclusion("clearance", &["a", "c"]), exclusion("hole_clearance", &["a", "b"])] }).unwrap();
    b.apply(&Cmd::DeleteDrcExclusions { exclusions: vec![key("clearance", &["a", "b"])] }).unwrap();
    assert_eq!(waived(&b).iter().map(|e| (e.check.as_str(), e.items.join("+"))).collect::<Vec<_>>(), vec![("clearance", "a+c".to_string()), ("hole_clearance", "a+b".to_string())]);
    // "Remove all exclusions of ..." is one step: every key in the list goes.
    b.apply(&Cmd::DeleteDrcExclusions { exclusions: vec![key("clearance", &["a", "c"]), key("hole_clearance", &["a", "b"])] }).unwrap();
    assert!(waived(&b).is_empty());
    let e = b.apply(&Cmd::DeleteDrcExclusions { exclusions: vec![key("clearance", &["a", "b"])] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_drc_exclusion");
}

#[test]
fn delete_all_markers_with_the_exclusions_forgets_every_waived_violation_in_one_step() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    // Nothing waived: refused, nothing to undo.
    let e = b.apply(&Cmd::ClearDrcExclusions).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_drc_exclusion");
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![exclusion("clearance", &["a", "b"]), exclusion("hole_clearance", &["a", "b"]), exclusion("track_width", &["t"])] }).unwrap();
    assert_eq!(waived(&b).len(), 3);
    b.apply(&Cmd::ClearDrcExclusions).unwrap();
    assert!(waived(&b).is_empty());
    let e = b.apply(&Cmd::ClearDrcExclusions).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_drc_exclusion");
    // It is a board edit, and it comes back from the studio's JSON.
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "clear_drc_exclusions" })).unwrap();
    assert!(matches!(cmd, Cmd::ClearDrcExclusions));
    assert_eq!(Cmd::ClearDrcExclusions.domain(), Domain::Pcb);
}

#[test]
fn an_exclusion_that_names_nothing_is_refused_and_stores_nothing() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    for (what, bad) in [
        ("no violation at all", vec![]),
        ("no check", vec![exclusion("", &["a"])]),
        ("no items", vec![exclusion("clearance", &[])]),
        ("an empty item", vec![exclusion("clearance", &["a", " "])]),
        ("ids that do not line up with the items", vec![DrcExclusion { ids: vec!["R1.1".into()], ..exclusion("clearance", &["a", "b"]) }]),
    ] {
        let e = b.apply(&Cmd::AddDrcExclusions { exclusions: bad }).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_drc_exclusion", "{what}");
    }
    // One bad entry refuses the batch: nothing of it is stored.
    assert!(b.apply(&Cmd::AddDrcExclusions { exclusions: vec![exclusion("clearance", &["a"]), exclusion("", &["b"])] }).is_err());
    assert!(waived(&b).is_empty());
}

#[test]
fn the_review_verbs_belong_to_the_editor_they_work_in() {
    let add = Cmd::AddDrcExclusions { exclusions: vec![exclusion("clearance", &["a"])] };
    assert_eq!(add.domain(), Domain::Pcb);
    assert!(!add.clears_routing());
    let sev = Cmd::SetErcSeverities { severities: BTreeMap::new() };
    assert_eq!(sev.domain(), Domain::Schematic, "Ctrl+Z on the Schematic tab undoes a severity change, not a board edit");
    assert!(!sev.edits_connectivity(), "a severity table does not re-read the drawing's nets");
}

#[test]
fn erc_severities_are_stored_as_sent_and_replace_the_whole_table() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let sev = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> { pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() };
    let stored = |b: &Board| b.design().schematic.as_ref().unwrap().extras.erc_severities.clone();
    // A check may be named at its own default: the derived project ignores the library-link checks unless the table says otherwise,
    // so "warning" for `lib_symbol_mismatch` (its default) has to be kept to mean anything.
    let table = sev(&[("pin_not_connected", "warning"), ("lib_symbol_mismatch", "warning"), ("single_global_label", "ignore")]);
    b.apply(&Cmd::SetErcSeverities { severities: table.clone() }).unwrap();
    assert_eq!(stored(&b), table);

    // The whole table is replaced.
    b.apply(&Cmd::SetErcSeverities { severities: sev(&[("wire_dangling", "ignore")]) }).unwrap();
    assert_eq!(stored(&b), sev(&[("wire_dangling", "ignore")]));
    b.apply(&Cmd::SetErcSeverities { severities: sev(&[]) }).unwrap();
    assert!(stored(&b).is_empty());

    for bad in [("not_a_check", "error"), ("pin_not_connected", "fatal")] {
        let e = b.apply(&Cmd::SetErcSeverities { severities: sev(&[bad]) }).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_severity", "{bad:?}");
    }
    assert!(stored(&b).is_empty(), "a refused table stores nothing");
    // The pin conflicts map's row takes the same words.
    b.apply(&Cmd::SetErcSeverities { severities: sev(&[("pin_to_pin", "ignore")]) }).unwrap();
}

#[test]
fn the_commands_read_from_the_json_the_studio_sends() {
    let body = serde_json::json!({
        "op": "add_drc_exclusions",
        "exclusions": [{ "check": "clearance", "items": ["a", "b"], "ids": ["R1.1", "t5"], "positions_nm": [[1_500_000_000, -2_000_000_000]], "comment": "ok" }],
    });
    let cmd: Cmd = serde_json::from_value(body).unwrap();
    assert!(matches!(&cmd, Cmd::AddDrcExclusions { exclusions } if exclusions[0].positions_nm == vec![[mm(1500.0), mm(-2000.0)]] && exclusions[0].ids.len() == 2));
    let del: Cmd = serde_json::from_value(serde_json::json!({ "op": "delete_drc_exclusions", "exclusions": [{ "check": "clearance", "items": ["a", "b"] }] })).unwrap();
    assert!(matches!(del, Cmd::DeleteDrcExclusions { .. }));
    let sev: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_erc_severities", "severities": { "pin_not_connected": "warning" } })).unwrap();
    assert!(matches!(sev, Cmd::SetErcSeverities { .. }));
    // Optional fields are optional.
    let minimal: Cmd = serde_json::from_value(serde_json::json!({ "op": "add_drc_exclusions", "exclusions": [{ "check": "clearance", "items": ["a"] }] })).unwrap();
    assert!(matches!(&minimal, Cmd::AddDrcExclusions { exclusions } if exclusions[0].comment.is_empty() && exclusions[0].ids.is_empty()));
}

#[test]
fn the_design_json_stays_the_same_without_exclusions_and_round_trips_with_them() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    assert!(!serde_json::to_string(b.design()).unwrap().contains("drc_exclusions"), "an older design.json reads, and writes, as before");
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![DrcExclusion { comment: "known".into(), positions_nm: vec![[Nm::new(1), um(2.0)]], ids: vec!["R1.1".into()], ..exclusion("annular_width", &["u-1"]) }] }).unwrap();
    let text = serde_json::to_string(b.design()).unwrap();
    let back: Design = serde_json::from_str(&text).unwrap();
    assert_eq!(back.drawings.unwrap().drc_exclusions, waived(&b));
}

/// `SchExtras::writes_nothing` decides whether `extras` is written to design.json at all, so a table of severities has to count there: a merge that loses it
/// would lose every severity the user chose, silently, at the next save.
#[test]
fn a_table_of_erc_severities_is_written_to_design_json_and_read_back() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let table: BTreeMap<String, String> = [("pin_not_connected".to_string(), "ignore".to_string())].into();
    b.apply(&Cmd::SetErcSeverities { severities: table.clone() }).unwrap();
    assert!(!b.design().schematic.as_ref().unwrap().extras.writes_nothing(), "a table alone makes the extras worth writing");
    let text = serde_json::to_string(b.design()).unwrap();
    assert!(text.contains("erc_severities") && text.contains("pin_not_connected"), "{text}");
    let back: Design = serde_json::from_str(&text).unwrap();
    assert_eq!(back.schematic.unwrap().extras.erc_severities, table);
}

// ------------------------------------------------------------------------------------------------------------ ERC exclusions

fn erc_waived(b: &Board) -> Vec<ErcExclusion> {
    b.design().schematic.as_ref().map(|s| s.erc_exclusions.clone()).unwrap_or_default()
}

/// A marker for the tests: the kind of finding, a position and the one item it names.
fn marker(error_type: &str, x: i64, item: &str) -> Option<eda_model::ir::ErcMarker> {
    if error_type.is_empty() {
        return None;
    }
    Some(eda_model::ir::ErcMarker { error_type: error_type.into(), at_nm: [Nm::from_raw_shim(x), Nm::from_raw_shim(x)], items: vec![item.into()], ..Default::default() })
}

fn add_erc(check: &str, location: &str, comment: &str, marker: Option<eda_model::ir::ErcMarker>) -> Cmd {
    Cmd::AddErcExclusion { check: check.into(), location: location.into(), comment: comment.into(), marker }
}

#[test]
fn an_erc_exclusion_keeps_its_comment_and_marker_text_and_adding_it_again_edits_them() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&add_erc("wire_dangling", "w2", "", None)).unwrap();
    b.apply(&add_erc("pin_not_connected", "U1.1", "left open on purpose", marker("ERCET_PIN_NOT_CONNECTED", 1, "u"))).unwrap();
    let list = erc_waived(&b);
    assert_eq!(list.iter().map(|e| (e.check.as_str(), e.location.as_str())).collect::<Vec<_>>(), [("pin_not_connected", "U1.1"), ("wire_dangling", "w2")], "kept sorted by (check, location)");
    assert_eq!((list[0].comment.as_str(), list[0].marker.clone()), ("left open on purpose", marker("ERCET_PIN_NOT_CONNECTED", 1, "u")));

    // "Edit exclusion comment...": the same key again replaces the comment; a call that has no marker text leaves the one it had.
    b.apply(&add_erc("pin_not_connected", "U1.1", "reviewed", None)).unwrap();
    let list = erc_waived(&b);
    assert_eq!(list.len(), 2, "no second entry for the same finding");
    assert_eq!((list[0].comment.as_str(), list[0].marker.clone()), ("reviewed", marker("ERCET_PIN_NOT_CONNECTED", 1, "u")));
    // An empty comment clears it; a new marker replaces the old one.
    b.apply(&add_erc("pin_not_connected", "U1.1", "", marker("ERCET_PIN_NOT_CONNECTED", 3000, "u"))).unwrap();
    assert_eq!((erc_waived(&b)[0].comment.as_str(), erc_waived(&b)[0].marker.clone()), ("", marker("ERCET_PIN_NOT_CONNECTED", 3000, "u")));
}

#[test]
fn an_erc_finding_with_no_location_cannot_be_excluded_and_deleting_needs_the_exact_key() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let e = b.apply(&add_erc("wire_dangling", "", "why", None)).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_erc_exclusion");
    assert!(erc_waived(&b).is_empty());

    b.apply(&add_erc("wire_dangling", "w1", "x", marker("ERCET_WIRE_DANGLING", 1, "w"))).unwrap();
    let e = b.apply(&Cmd::DeleteErcExclusion { check: "wire_dangling".into(), location: "w9".into(), sheet: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_erc_exclusion");
    assert_eq!(erc_waived(&b).len(), 1);
    // Deleting takes the comment and the marker text with it: KiCad forgets an excluded marker that was deleted, and so does the project.
    b.apply(&Cmd::DeleteErcExclusion { check: "wire_dangling".into(), location: "w1".into(), sheet: None }).unwrap();
    assert!(erc_waived(&b).is_empty());
}

/// p13-p16 F5: the findings of the two placements of a shared screen have one `(check, location)` and two sheets; each placement's exclusion is its own,
/// and deleting one (Delete Marker sends the marker's sheet) leaves the other.
#[test]
fn an_erc_exclusion_on_one_placement_leaves_the_other() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let on = |sheet: &str| marker("ERCET_PIN_NOT_CONNECTED", 1, "u").map(|mk| eda_model::ir::ErcMarker { sheet: vec!["root".into(), sheet.into()], main_sheet: vec!["root".into(), sheet.into()], ..mk });
    b.apply(&add_erc("pin_not_connected", "R1.2", "first", on("s1"))).unwrap();
    b.apply(&add_erc("pin_not_connected", "R1.2", "second", on("s2"))).unwrap();
    assert_eq!(erc_waived(&b).iter().map(|e| e.comment.as_str()).collect::<Vec<_>>(), ["first", "second"], "one exclusion per placement");
    b.apply(&add_erc("pin_not_connected", "R1.2", "first, reviewed", on("s1"))).unwrap();
    assert_eq!(erc_waived(&b).len(), 2, "the same placement again edits its own");
    b.apply(&Cmd::DeleteErcExclusion { check: "pin_not_connected".into(), location: "R1.2".into(), sheet: Some(vec!["root".into(), "s2".into()]) }).unwrap();
    assert_eq!(erc_waived(&b).iter().map(|e| e.comment.as_str()).collect::<Vec<_>>(), ["first, reviewed"]);
    let e = b.apply(&Cmd::DeleteErcExclusion { check: "pin_not_connected".into(), location: "R1.2".into(), sheet: Some(vec!["root".into(), "s2".into()]) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_erc_exclusion");
}

#[test]
fn the_erc_exclusion_json_reads_with_or_without_a_comment_and_writes_nothing_extra_for_a_bare_one() {
    let bare: Cmd = serde_json::from_value(serde_json::json!({ "op": "add_erc_exclusion", "check": "wire_dangling", "location": "w1" })).unwrap();
    assert!(matches!(&bare, Cmd::AddErcExclusion { comment, marker, .. } if comment.is_empty() && marker.is_none()), "an older client's body still reads");
    let full: Cmd = serde_json::from_value(serde_json::json!({ "op": "add_erc_exclusion", "check": "wire_dangling", "location": "w1", "comment": "ok", "marker": { "error_type": "ERCET_WIRE_DANGLING", "at_nm": [1, 1], "items": ["w"], "sheet": ["r"] } })).unwrap();
    assert!(matches!(&full, Cmd::AddErcExclusion { comment, marker, .. } if comment == "ok" && marker.as_ref().is_some_and(|m| m.error_type == "ERCET_WIRE_DANGLING" && m.sheet == vec!["r".to_string()])));
    assert_eq!(full.domain(), Domain::Schematic);

    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&add_erc("wire_dangling", "w1", "", None)).unwrap();
    let text = serde_json::to_string(b.design()).unwrap();
    assert!(text.contains("erc_exclusions") && !text.contains("\"comment\"") && !text.contains("\"marker\""), "a bare exclusion serializes as it always did: {text}");
    b.apply(&add_erc("wire_dangling", "w1", "why", marker("ERCET_WIRE_DANGLING", 1, "w"))).unwrap();
    let back: Design = serde_json::from_str(&serde_json::to_string(b.design()).unwrap()).unwrap();
    assert_eq!(back.schematic.unwrap().erc_exclusions, erc_waived(&b));
}

#[test]
fn delete_all_markers_with_the_exclusions_clears_the_list_and_is_refused_when_it_is_empty() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    let e = b.apply(&Cmd::ClearErcExclusions).unwrap_err();
    assert_eq!(e[0].check, "ops_no_schematic", "a board with no schematic has no exclusion to clear");
    b.apply(&add_erc("wire_dangling", "w1", "x", marker("ERCET_WIRE_DANGLING", 1, "w"))).unwrap();
    b.apply(&add_erc("label_dangling", "l1", "", None)).unwrap();
    assert_eq!(erc_waived(&b).len(), 2);
    b.apply(&Cmd::ClearErcExclusions).unwrap();
    assert!(erc_waived(&b).is_empty());
    let e = b.apply(&Cmd::ClearErcExclusions).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_erc_exclusion", "nothing left to clear");
    // The exclusions are about the report, not the drawing: no net is re-read from the schematic after them, and they belong to the root's.
    assert!(!Cmd::ClearErcExclusions.edits_connectivity() && !add_erc("a", "b", "", None).edits_connectivity());
    assert_eq!(Cmd::ClearErcExclusions.domain(), Domain::Schematic);
    let on_sheet: Cmd = serde_json::from_value(serde_json::json!({ "op": "clear_erc_exclusions" })).unwrap();
    assert!(matches!(on_sheet, Cmd::ClearErcExclusions));
}

// ------------------------------------------------------------------------------------------------------------ Schematic Setup > Formatting

fn grid(b: &Board) -> Option<u32> {
    b.design().schematic.as_ref().and_then(|s| s.extras.connection_grid_mils)
}

#[test]
fn the_connection_grid_is_from_25_to_10000_mils_and_kicads_default_is_no_setting_at_all() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    assert_eq!(grid(&b), None);
    b.apply(&Cmd::SetConnectionGrid { mils: 25 }).unwrap();
    assert_eq!(grid(&b), Some(25));
    b.apply(&Cmd::SetConnectionGrid { mils: 10_000 }).unwrap();
    assert_eq!(grid(&b), Some(10_000));
    // 50 is KiCad's default: back to nothing, so a design that never left it serializes as before.
    b.apply(&Cmd::SetConnectionGrid { mils: 50 }).unwrap();
    assert_eq!(grid(&b), None);
    assert!(b.design().schematic.as_ref().unwrap().extras.writes_nothing());
    for bad in [0, 24_000, 10_001_000] {
        let e = b.apply(&Cmd::SetConnectionGrid { mils: bad }).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_connection_grid", "{bad}");
    }
    assert_eq!(grid(&b), None, "a refused value stores nothing");
}

#[test]
fn a_connection_grid_is_written_to_design_json_and_read_back_and_the_command_reads_from_the_studios_json() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SetConnectionGrid { mils: 100 }).unwrap();
    assert!(!b.design().schematic.as_ref().unwrap().extras.writes_nothing(), "the grid alone makes the extras worth writing");
    let text = serde_json::to_string(b.design()).unwrap();
    assert!(text.contains("connection_grid_mils"), "{text}");
    let back: Design = serde_json::from_str(&text).unwrap();
    assert_eq!(back.schematic.unwrap().extras.connection_grid_mils, Some(100));
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_connection_grid", "mils": 25 })).unwrap();
    assert!(matches!(cmd, Cmd::SetConnectionGrid { mils: 25 }));
    assert_eq!(cmd.domain(), Domain::Schematic);
    assert!(!cmd.edits_connectivity());
}
