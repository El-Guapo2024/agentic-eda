//! The DRC and ERC review verbs: a waived violation (`design.drawings.drc_exclusions`) and the schematic's
//! per-check severities (`design.schematic.extras.erc_severities`).

use super::*;
use eda_model::ir::{DrcExclusion, DrcExclusionKey, PlacementSection, Provenance};

fn design() -> Design {
    Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        routing: None,
        placement: Some(PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 50_000, y: 0 }, Point { x: 50_000, y: 50_000 }, Point { x: 0, y: 50_000 }], footprints: vec![], modules: vec![] }),
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
    let mut b = Board::new(design(), &m, 100, 300);
    assert!(waived(&b).is_empty());
    let mut a = exclusion("clearance", &["u-2", "u-1"]);
    a.positions_nm = vec![[300, 4], [100, 2], [300, 4]];
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![exclusion("track_width", &["u-9"]), a] }).unwrap();
    let list = waived(&b);
    assert_eq!(list.iter().map(|e| e.check.as_str()).collect::<Vec<_>>(), vec!["clearance", "track_width"], "kept in order, one entry for each");
    assert_eq!(list[0].positions_nm, vec![[100, 2], [300, 4]], "the marker positions to try are sorted and listed once");

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
    let mut b = Board::new(design(), &m, 100, 300);
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
fn an_exclusion_that_names_nothing_is_refused_and_stores_nothing() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, 100, 300);
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
    let mut b = Board::new(design(), &m, 100, 300);
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
        "exclusions": [{ "check": "clearance", "items": ["a", "b"], "ids": ["R1.1", "t5"], "positions_nm": [[1500000, -2000000]], "comment": "ok" }],
    });
    let cmd: Cmd = serde_json::from_value(body).unwrap();
    assert!(matches!(&cmd, Cmd::AddDrcExclusions { exclusions } if exclusions[0].positions_nm == vec![[1_500_000, -2_000_000]] && exclusions[0].ids.len() == 2));
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
    let mut b = Board::new(design(), &m, 100, 300);
    assert!(!serde_json::to_string(b.design()).unwrap().contains("drc_exclusions"), "an older design.json reads, and writes, as before");
    b.apply(&Cmd::AddDrcExclusions { exclusions: vec![DrcExclusion { comment: "known".into(), positions_nm: vec![[1, 2]], ids: vec!["R1.1".into()], ..exclusion("annular_width", &["u-1"]) }] }).unwrap();
    let text = serde_json::to_string(b.design()).unwrap();
    let back: Design = serde_json::from_str(&text).unwrap();
    assert_eq!(back.drawings.unwrap().drc_exclusions, waived(&b));
}
