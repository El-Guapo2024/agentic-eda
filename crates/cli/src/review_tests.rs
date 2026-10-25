//! The review workflow through a board directory (GAPS.md package P15): an ERC exclusion with a comment reaches the derived project and kicad-cli's ERC report,
//! and an excluded marker that was deleted stays deleted when kicad-cli runs again.

use eda_model::units::{mm, Nm};
use super::tests::{scratch, setup};
use super::{derived_schematic, load, save, step, undo};
use eda_model::ir::ErcMarker;
use eda_ops::{Cmd, Domain};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The board `setup` makes (two parts whose footprint belongs to no library), with the schematic the studio shows for it stored, as the first schematic edit
/// stores it, and the footprint link check on (Schematic Setup; the derived project ignores the library checks until then): kicad-cli's ERC finds a
/// `footprint_link_issues` warning on each symbol, and nothing it does not find the same way twice.
fn board_with_schematic(name: &str) -> PathBuf {
    let dir = scratch(name);
    setup(&dir);
    let (_, mut design, model) = load(&dir).unwrap();
    let derived = derived_schematic(&design, &model).unwrap();
    design.schematic = derived.schematic;
    design.sheet_contents = derived.sheet_contents;
    design.schematic.as_mut().unwrap().extras.erc_severities.insert("footprint_link_issues".into(), "warning".into());
    save(&dir, &design).unwrap();
    dir
}

fn slow_tests_off() -> bool {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return true;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return true;
    }
    false
}

/// The studio's ERC report entry for `check` on `location`.
fn finding(report: &Value, check: &str, location: &str) -> Value {
    report["violations"].as_array().unwrap().iter().find(|v| v["check"] == check && v["location"] == location).cloned().unwrap_or_else(|| panic!("no {check} on {location} in {report:#}"))
}

fn listed(report: &Value) -> usize {
    report["violations"].as_array().unwrap().len()
}

fn add(check: &str, location: &str, comment: &str, marker: Option<ErcMarker>) -> Cmd {
    Cmd::AddErcExclusion { check: check.into(), location: location.into(), comment: comment.into(), marker }
}

/// The marker the report offers for a finding (`ErcViolation.marker`), as the web client sends it back.
fn marker_of(v: &Value) -> ErcMarker {
    serde_json::from_value(v["marker"].clone()).unwrap_or_else(|e| panic!("the report gives the marker an exclusion of the finding would be: {e}: {v:#}"))
}

/// `kicad-cli sch erc` run by hand on the schematic the studio's last run derived in `dir`, asked for `severities` and nothing else: the raw report's
/// violations of `check` as `(excluded, comment, number of items)`. The derived project is the one the studio's last run wrote.
fn kicad_cli_lists(dir: &Path, severities: &[&str], check: &str) -> Vec<(bool, String, usize)> {
    let cli = eda_kicad_engine::find_cli().expect("checked by the caller");
    let sch = dir.join(".kicad").join("board.kicad_sch");
    let out = dir.join(".kicad").join("direct-erc.json");
    let _ = std::fs::remove_file(&out);
    let ran = eda_kicad_engine::cli_command(&cli, None).args(["sch", "erc", "--format", "json", "--units", "mm"]).args(severities).arg("-o").arg(&out).arg(&sch).output().expect("kicad-cli runs");
    assert!(out.exists(), "kicad-cli wrote no report: {}{}", String::from_utf8_lossy(&ran.stdout), String::from_utf8_lossy(&ran.stderr));
    let report: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    report["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["violations"].as_array().unwrap().iter())
        .filter(|v| v["type"] == check)
        .map(|v| (v["excluded"].as_bool().unwrap_or(false), v["comment"].as_str().unwrap_or("").to_string(), v["items"].as_array().unwrap().len()))
        .collect()
}

fn project(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join(".kicad").join("board.kicad_pro")).unwrap()).unwrap()
}

/// Exclude -> kicad-cli reports it excluded, with the comment; Delete Marker on the excluded marker -> it is a violation again and stays one on the next run,
/// where KiCad (which records the markers that are left, `SCHEMATIC::RecordERCExclusions`) forgets it too; Undo -> excluded again (slow tier: a dozen kicad-cli runs).
#[test]
fn an_erc_exclusion_with_a_comment_reaches_kicad_cli_and_a_deleted_marker_stays_deleted() {
    if slow_tests_off() {
        return;
    }
    let dir = board_with_schematic("review_erc_exclusion");
    let first = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let u1 = finding(&first, "footprint_link_issues", "U1");
    let u2 = finding(&first, "footprint_link_issues", "U2");
    assert_eq!((u1["severity"].as_str(), u1["kicad_matched"].as_bool()), (Some("warning"), Some(false)), "nothing is waived yet: {u1:#}");
    let marker = marker_of(&u1);
    assert_eq!(marker.error_type, "ERCET_FOOTPRINT_LINK_ISSUES");
    assert!(marker.sheet.is_empty() && marker.main_sheet.is_empty(), "a symbol's footprint link has no sheet in its marker: {marker:?}");
    assert!(marker.at_nm[0] > mm(1.0), "the position is in nanometres, as kicad-cli's report says millimetres: {marker:?}");
    let (n, warnings) = (listed(&first), first["counts"]["footprint_link_issues"].as_u64().unwrap());
    assert!(warnings >= 2, "{first:#}");

    // Exclude U1's with a comment, the way "Exclude with comment..." does.
    step(&dir, add("footprint_link_issues", "U1", "bare footprint on purpose", Some(marker.clone())), false, "ui").unwrap();
    let after = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let waived = finding(&after, "footprint_link_issues", "U1");
    assert_eq!(waived["severity"], "excluded");
    assert_eq!(waived["kicad_matched"], true, "kicad-cli matched the text itself: {waived:#}");
    assert_eq!(waived["comment"], "bare footprint on purpose", "and the comment came back");
    assert_eq!(listed(&after), n, "no marker of its own is left beside the violation: a record that matches makes none");
    assert_eq!(after["counts"]["footprint_link_issues"].as_u64().unwrap(), warnings - 1);
    assert_eq!(finding(&after, "footprint_link_issues", "U2")["severity"], u2["severity"], "the other symbol is untouched");
    let listed_in_project = project(&dir);
    assert_eq!(listed_in_project["erc"]["erc_exclusions"][0]["marker"]["errorType"], "ERCET_FOOTPRINT_LINK_ISSUES", "the derived project lists it");
    assert_eq!(listed_in_project["erc"]["erc_exclusions"][0]["comment"], "bare footprint on purpose");

    // kicad-cli itself, on the derived files: a plain report no longer has the warning, one that asks for exclusions has it flagged and commented.
    let plain = ["--severity-error", "--severity-warning"];
    assert_eq!(kicad_cli_lists(&dir, &plain, "footprint_link_issues").len() as u64, warnings - 1, "kicad-cli stops reporting it");
    assert_eq!(
        kicad_cli_lists(&dir, &["--severity-exclusions"], "footprint_link_issues"),
        vec![(true, "bare footprint on purpose".to_string(), 1)],
        "asked for exclusions it lists the one marker, flagged, with the comment"
    );

    // Delete Marker on the excluded marker: the exclusion goes (KiCad's next run records the markers that are left, and this one is not).
    step(&dir, Cmd::DeleteErcExclusion { check: "footprint_link_issues".into(), location: "U1".into(), sheet: None }, false, "ui").unwrap();
    for run in 0..2 {
        let again = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
        let back = finding(&again, "footprint_link_issues", "U1");
        assert_eq!((back["severity"].as_str(), back["kicad_matched"].as_bool()), (Some("warning"), Some(false)), "run {run}: a violation again, as nothing waives it");
        assert_eq!(listed(&again), n, "run {run}: and no excluded marker is left behind");
    }
    assert!(project(&dir)["erc"]["erc_exclusions"].is_null(), "the project lists nothing");
    assert!(kicad_cli_lists(&dir, &["--severity-exclusions"], "footprint_link_issues").is_empty(), "kicad-cli has no excluded marker either");

    // Undo takes the deletion back: excluded again, with its comment.
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let undone = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let again = finding(&undone, "footprint_link_issues", "U1");
    assert_eq!((again["severity"].as_str(), again["comment"].as_str()), (Some("excluded"), Some("bare footprint on purpose")));
}

/// A marker that is not the one kicad-cli made (the symbol was moved since, or the position is wrong) matches nothing, and KiCad turns it into a marker of
/// its own: the studio's report must not list that as a second violation, and the exclusion still holds in the studio's own report (slow tier).
#[test]
fn an_exclusion_whose_marker_matches_nothing_does_not_add_a_finding() {
    if slow_tests_off() {
        return;
    }
    let dir = board_with_schematic("review_erc_stale");
    let first = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let mut marker = marker_of(&finding(&first, "footprint_link_issues", "U1"));
    let n = listed(&first);
    // The right item, a position it is not at.
    marker.at_nm[0] = Nm::new(1);
    step(&dir, add("footprint_link_issues", "U1", "moved since", Some(marker)), false, "ui").unwrap();

    let after = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let u1 = finding(&after, "footprint_link_issues", "U1");
    assert_eq!((u1["severity"].as_str(), u1["kicad_matched"].as_bool(), u1["comment"].as_str()), (Some("excluded"), Some(false), Some("moved since")), "waived in the studio's report, which kicad-cli's project does not hold: {u1:#}");
    assert_eq!(listed(&after), n, "and kicad-cli's stray excluded marker, with the same symbol, is not a second row");
    assert_eq!(kicad_cli_lists(&dir, &["--severity-exclusions"], "footprint_link_issues").len(), 1, "kicad-cli made a marker of its own out of it");
}

/// Without kicad-cli: the exclusion's comment and marker are one undo step, and they are in the project the studio derives.
#[test]
fn an_erc_exclusion_is_one_undo_step_and_its_marker_is_in_the_derived_project() {
    let dir = board_with_schematic("review_erc_undo");
    let marker = ErcMarker { error_type: "ERCET_FOOTPRINT_LINK_ISSUES".into(), at_nm: [mm(113.03), mm(100.33)], items: vec!["a4f97020-e09b-4e24-b611-b376f3c27df7".into()], ..Default::default() };
    step(&dir, add("footprint_link_issues", "U1", "why", Some(marker.clone())), false, "ui").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let pro: Value = serde_json::from_str(&eda_kicad::export_kicad_pro_for(&design, &model)).unwrap();
    assert_eq!(
        pro["erc"]["erc_exclusions"],
        serde_json::json!([{ "marker": { "errorType": "ERCET_FOOTPRINT_LINK_ISSUES", "position": { "xNm": "113030000", "yNm": "100330000" }, "items": [{ "value": "a4f97020-e09b-4e24-b611-b376f3c27df7" }] }, "comment": "why" }])
    );

    // "Edit exclusion comment..." is another step; Undo takes it back to the first comment.
    step(&dir, add("footprint_link_issues", "U1", "edited", None), false, "ui").unwrap();
    assert_eq!(load(&dir).unwrap().1.schematic.unwrap().erc_exclusions[0].comment, "edited");
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let kept = load(&dir).unwrap().1.schematic.unwrap().erc_exclusions;
    assert_eq!((kept[0].comment.as_str(), kept[0].marker.clone()), ("why", Some(marker)));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(load(&dir).unwrap().1.schematic.unwrap().erc_exclusions.is_empty());
}

/// Delete Marker on an excluded DRC marker and Delete All Markers with the exclusions (`RecordDRCExclusions` forgets what no marker holds): the verbs are one undo
/// step each, and the derived project lists what is left.
#[test]
fn deleting_drc_markers_with_their_exclusions_is_undoable_and_the_project_follows() {
    let dir = scratch("review_drc_delete");
    setup(&dir);
    let spec = |check: &str, items: &[&str]| eda_model::ir::DrcExclusion { check: check.into(), items: items.iter().map(|s| s.to_string()).collect(), ids: vec![], positions_nm: vec![], comment: String::new() };
    let key = |check: &str, items: &[&str]| eda_model::ir::DrcExclusionKey { check: check.into(), items: items.iter().map(|s| s.to_string()).collect() };
    let waived = |dir: &Path| load(dir).unwrap().1.drawings.map_or(0, |d| d.drc_exclusions.len());
    step(&dir, Cmd::AddDrcExclusions { exclusions: vec![spec("track_width", &["t1"]), spec("track_width", &["t2"]), spec("annular_width", &["v1"])] }, false, "ui").unwrap();
    // Delete Marker on one excluded marker: its exclusion goes.
    step(&dir, Cmd::DeleteDrcExclusions { exclusions: vec![key("track_width", &["t1"])] }, false, "ui").unwrap();
    assert_eq!(waived(&dir), 2);
    // Delete All Markers, "Errors, Warnings and Exclusions": the rest goes, in one step, and the derived project lists none.
    step(&dir, Cmd::ClearDrcExclusions, false, "ui").unwrap();
    assert_eq!(waived(&dir), 0);
    let (_, design, model) = load(&dir).unwrap();
    let pro: Value = serde_json::from_str(&eda_kicad::export_kicad_pro_for(&design, &model)).unwrap();
    assert!(pro["board"]["design_settings"]["drc_exclusions"].as_array().is_none_or(|l| l.is_empty()), "{pro}");
    // Each is its own undo step.
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(waived(&dir), 2);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(waived(&dir), 3);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(waived(&dir), 0);
}

/// The dialogs' "Save..." (slow tier): the report is kicad-cli's own file -- `ERC.rpt` text, `ERC.json` -- for the severities the Show boxes list, with the
/// exclusion and its comment in it when that box is on; a run that fails says why.
#[test]
fn a_saved_report_is_the_file_kicad_cli_writes_for_the_severities_the_dialog_shows() {
    if slow_tests_off() {
        return;
    }
    let dir = board_with_schematic("review_saved_report");
    let first = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let marker = marker_of(&finding(&first, "footprint_link_issues", "U1"));
    step(&dir, add("footprint_link_issues", "U1", "bare footprint on purpose", Some(marker)), false, "ui").unwrap();

    let body = |format: &str, errors: bool, warnings: bool, exclusions: bool| serde_json::json!({ "format": format, "errors": errors, "warnings": warnings, "exclusions": exclusions, "units": "mm" }).to_string().into_bytes();
    // Errors and warnings: the text report, as the dialog's "Report file" type writes it; the waived finding is not in it.
    let text = crate::kicad_engine::erc_report(&dir, &body("report", true, true, false));
    assert_eq!((text["ok"].as_bool(), text["file_name"].as_str()), (Some(true), Some("ERC.rpt")), "{text}");
    let rpt = text["report"].as_str().unwrap();
    assert!(rpt.starts_with("ERC report ("), "{rpt}");
    assert!(rpt.contains("Report includes: Errors, Warnings"), "{rpt}");
    assert!(rpt.contains("[footprint_link_issues]") && !rpt.contains("bare footprint on purpose"), "the other symbol's warning is there, the waived one's comment is not: {rpt}");
    // Exclusions only: the waived finding with its comment, and the other symbol's warning left out.
    let only = crate::kicad_engine::erc_report(&dir, &body("report", false, false, true));
    let rpt = only["report"].as_str().unwrap();
    assert!(rpt.contains("Report includes: Exclusions") && rpt.contains("bare footprint on purpose"), "{rpt}");
    assert_eq!(rpt.matches("[footprint_link_issues]").count(), 1, "{rpt}");
    // The JSON file type: kicad-cli's own JSON, every severity.
    let json = crate::kicad_engine::erc_report(&dir, &body("json", true, true, true));
    assert_eq!(json["file_name"], "ERC.json");
    let parsed: Value = serde_json::from_str(json["report"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["included_severities"], serde_json::json!(["error", "warning", "exclusion"]));
    let excluded: Vec<&Value> = parsed["sheets"].as_array().unwrap().iter().flat_map(|s| s["violations"].as_array().unwrap().iter()).filter(|v| v["excluded"] == true).collect();
    assert_eq!(excluded.len(), 1, "the waived finding is in the JSON file: {parsed:#}");
    assert_eq!(excluded[0]["comment"], "bare footprint on purpose");

    // The DRC dialog's: the same, from `kicad-cli pcb drc`.
    let drc = crate::kicad_engine::drc_report(&dir, &body("report", true, true, true));
    assert_eq!((drc["ok"].as_bool(), drc["file_name"].as_str()), (Some(true), Some("DRC.rpt")), "{drc}");
    assert!(drc["report"].as_str().unwrap().starts_with("** Drc report for "), "{drc}");
    let drc_json = crate::kicad_engine::drc_report(&dir, &body("json", true, true, true));
    assert!(serde_json::from_str::<Value>(drc_json["report"].as_str().unwrap()).unwrap()["violations"].is_array());
}
