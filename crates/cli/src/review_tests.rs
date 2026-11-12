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
    assert!(!marker.sheet.is_empty() && marker.main_sheet == marker.sheet, "a symbol's footprint link names its sheet, as KiCad 25ec2335's marker does (r3 D3-L5): {marker:?}");
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

/// A `kicad-cli pcb drc` violation of the studio's report: `(type, item uuids, excluded, kicad_matched)`.
fn drc_rows(report: &Value, kind: &str) -> Vec<Value> {
    report["violations"].as_array().unwrap().iter().filter(|v| v["type"] == kind).cloned().collect()
}

/// The DRC exclusion the web client sends for a violation of the studio's report (`drcExclusionSpec`).
fn drc_exclusion_of(v: &Value) -> eda_model::ir::DrcExclusion {
    let strings = |key: &str| -> Vec<String> { v["items"].as_array().unwrap().iter().map(|i| i[key].as_str().unwrap_or("").to_string()).collect() };
    eda_model::ir::DrcExclusion {
        check: v["type"].as_str().unwrap().to_string(),
        items: strings("uuid"),
        ids: strings("id"),
        positions_nm: v["marker_nm"].as_array().unwrap().iter().map(|p| [Nm::new(p[0].as_i64().unwrap()), Nm::new(p[1].as_i64().unwrap())]).collect(),
        comment: String::new(),
    }
}

/// `kicad-cli pcb drc` run by hand on the board the studio derived in `dir`, asked for `severities`: the number of violations of `kind` it lists.
fn kicad_cli_drc_lists(dir: &Path, severities: &[&str], kind: &str) -> usize {
    let cli = eda_kicad_engine::find_cli().expect("checked by the caller");
    let board = dir.join(".kicad").join("board.kicad_pcb");
    let out = dir.join(".kicad").join("direct-drc.json");
    let _ = std::fs::remove_file(&out);
    let ran = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "drc", "--format", "json", "--units", "mm"]).args(severities).arg("-o").arg(&out).arg(&board).output().expect("kicad-cli runs");
    assert!(out.exists(), "kicad-cli wrote no report: {}{}", String::from_utf8_lossy(&ran.stdout), String::from_utf8_lossy(&ran.stderr));
    let report: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    report["violations"].as_array().unwrap().iter().filter(|v| v["type"] == kind).count()
}

/// UI-LEFT B7/B8 (slow tier): the marker of a clearance between two tracks and of two crossing tracks is where KiCad's collision puts it, so an exclusion
/// of either reaches kicad-cli (`eda_kicad::drc_exclusions::contact_point`); and the DRC dialog's "Report All Errors for Each Track" is `--all-track-errors`:
/// without it a track reports its first error only (`DRC_ENGINE::GetReportAllTrackErrors`; each track is tested against the tracks of a greater uuid).
#[test]
fn a_track_clearance_and_a_crossing_reach_kicad_cli_and_all_track_errors_reports_every_one() {
    if slow_tests_off() {
        return;
    }
    let dir = scratch("review_drc_tracks");
    setup(&dir);
    let track = |net: &str, pts: &[(f64, f64)]| Cmd::AddTrack { net: net.into(), layer: "F.Cu".into(), width: mm(0.2), pts: pts.iter().map(|&(x, y)| eda_model::ir::Point { x: mm(x), y: mm(y) }).collect() };
    // Every track starts on a pad of its net (U1.1 GND at 4,5; U1.2 VCC at 6,5; U2.1 GND at 14,5), so kicad-cli keeps its net: a VCC track down from U1.2,
    // a GND track from each side ending 0.15 mm from it (0.2 mm clearance), and a GND track crossing it.
    step(&dir, track("VCC", &[(6.0, 5.0), (6.0, 15.0)]), false, "test").unwrap();
    step(&dir, track("GND", &[(4.0, 5.0), (4.0, 12.0), (5.65, 12.0)]), false, "test").unwrap();
    step(&dir, track("GND", &[(14.0, 5.0), (14.0, 12.0), (6.35, 12.0)]), false, "test").unwrap();
    step(&dir, track("GND", &[(4.0, 5.0), (4.0, 8.0), (8.0, 8.0)]), false, "test").unwrap();
    let errors_of = |r: &Value| drc_rows(r, "clearance").len() + drc_rows(r, "tracks_crossing").len();

    let every = eda_kicad_engine::DrcOptions { all_track_errors: true, ..Default::default() };
    let all = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    assert_eq!((drc_rows(&all, "clearance").len(), drc_rows(&all, "tracks_crossing").len()), (2, 1), "every error of the VCC track: {all:#}");
    // Without the option the VCC track keeps its first error against the tracks of a greater uuid; a track of a smaller uuid reports its own.
    let pairs: Vec<Vec<String>> = drc_rows(&all, "clearance").into_iter().chain(drc_rows(&all, "tracks_crossing")).map(|v| v["items"].as_array().unwrap().iter().map(|i| i["uuid"].as_str().unwrap().to_string()).collect()).collect();
    let vcc = pairs[0].iter().find(|u| pairs.iter().all(|p| p.contains(u))).cloned().expect("the VCC track is in every pair");
    let others: Vec<&String> = pairs.iter().flatten().filter(|u| **u != vcc).collect();
    let expected = others.iter().filter(|u| ***u < vcc).count() + usize::from(others.iter().any(|u| **u > vcc));
    let first = crate::kicad_engine::drc_with(&dir, false, false).expect("kicad-cli ran");
    assert_eq!(errors_of(&first), expected, "the first error of each track only (VCC {vcc}, the others {others:?})");
    eprintln!("without --all-track-errors: {expected} of the 3 errors");

    // Exclude a clearance and the crossing: kicad-cli matches both, a run of it no longer lists them, and it made no stray marker of a guess.
    let (clearance, crossing) = (drc_rows(&all, "clearance")[0].clone(), drc_rows(&all, "tracks_crossing")[0].clone());
    step(&dir, Cmd::AddDrcExclusions { exclusions: vec![drc_exclusion_of(&clearance), drc_exclusion_of(&crossing)] }, false, "ui").unwrap();
    let after = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    for kind in ["clearance", "tracks_crossing"] {
        let waived: Vec<Value> = drc_rows(&after, kind).into_iter().filter(|v| v["excluded"] == true).collect();
        assert_eq!(waived.len(), 1, "{kind}: {after:#}");
        assert_eq!(waived[0]["kicad_matched"], true, "{kind}: kicad-cli matched the exclusion at the contact point the studio computed");
    }
    let plain = ["--all-track-errors", "--severity-error", "--severity-warning"];
    assert_eq!(kicad_cli_drc_lists(&dir, &plain, "clearance"), 1, "kicad-cli stops reporting the waived clearance and keeps the other");
    assert_eq!(kicad_cli_drc_lists(&dir, &plain, "tracks_crossing"), 0, "and the crossing");
    let excluded = ["--all-track-errors", "--severity-exclusions"];
    assert_eq!((kicad_cli_drc_lists(&dir, &excluded, "clearance"), kicad_cli_drc_lists(&dir, &excluded, "tracks_crossing")), (1, 1), "one excluded marker each");
    let _ = std::fs::remove_dir_all(&dir);
}

/// UI-LEFT-2 BB1 (slow tier): a clearance between a track and a pad, and between a via and a pad, is placed where KiCad's collision of the track's
/// segment or the via's circle with the pad's compound puts it (`eda_kicad::drc_exclusions::board_copper`, `collide`: `Collide( const SHAPE_RECT&,
/// const SHAPE_SEGMENT& )`, `Collide( const SHAPE_RECT&, const SHAPE_CIRCLE& )`), so an exclusion of either reaches kicad-cli.
#[test]
fn a_pad_clearance_reaches_kicad_cli() {
    if slow_tests_off() {
        return;
    }
    // the pads as a rectangle (`SHAPE_RECT`), a rounded rectangle (the inset rectangle and four side segments), a circle and an oval
    for (shape, ratio, size) in [(eda_model::PadShape::Rect, None, (0.8, 0.8)), (eda_model::PadShape::RoundRect, Some(0.25), (0.8, 0.8)), (eda_model::PadShape::Circle, None, (0.8, 0.8)), (eda_model::PadShape::Oval, None, (0.8, 1.2))] {
        pad_clearance_reaches_kicad_cli(shape, ratio, size);
    }
}

fn pad_clearance_reaches_kicad_cli(shape: eda_model::PadShape, ratio: Option<f64>, size: (f64, f64)) {
    let dir = scratch(&format!("review_drc_pads_{shape:?}"));
    setup(&dir);
    let intent = dir.join("intent.yaml");
    let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    for pad in &mut model.footprints[0].pads {
        pad.shape = shape;
        pad.roundrect_ratio = ratio;
        pad.size = (mm(size.0), mm(size.1));
    }
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    let pt = |x: f64, y: f64| eda_model::ir::Point { x: mm(x), y: mm(y) };
    // U1.2 (VCC) is a 0.8 mm square at (6, 5). A GND track from U2.1 (14, 5) stops 0.15 mm from its right side; a GND track from U1.1 (4, 5) runs
    // below it to a GND via whose copper comes 0.15 mm from its bottom side.
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![pt(14.0, 5.0), pt(6.65, 5.0)] }, false, "test").unwrap();
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![pt(4.0, 5.0), pt(4.0, 7.0), pt(7.5, 7.0), pt(7.5, 5.85), pt(6.0, 5.85)] }, false, "test").unwrap();
    step(&dir, Cmd::AddVia { net: "GND".into(), x: mm(6.0), y: mm(5.85), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }, false, "test").unwrap();
    // A VCC track from U2.2 (16, 5) passing 0.05 mm under the via (its copper and its hole: `clearance` and `hole_clearance`), and a GND track from U2.1
    // ending 0.2 mm from the board's bottom edge (`copper_edge_clearance`).
    step(&dir, Cmd::AddTrack { net: "VCC".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![pt(16.0, 5.0), pt(16.0, 6.3), pt(5.0, 6.3)] }, false, "test").unwrap();
    step(&dir, Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![pt(14.0, 5.0), pt(14.0, 19.7)] }, false, "test").unwrap();
    let every = eda_kicad_engine::DrcOptions { all_track_errors: true, ..Default::default() };
    let all = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    let pad_rows: Vec<Value> = drc_rows(&all, "clearance").into_iter().filter(|v| v["items"].as_array().unwrap().iter().any(|i| i["description"].as_str().unwrap_or("").starts_with("Pad"))).collect();
    assert!(pad_rows.len() >= 2, "{shape:?}: a track and a via too near the pad: {all:#}");
    let holes = drc_rows(&all, "hole_clearance");
    let edges = drc_rows(&all, "copper_edge_clearance");
    assert!(!holes.is_empty() && !edges.is_empty(), "{shape:?}: a hole clearance and an edge clearance: {all:#}");
    let rows: Vec<Value> = pad_rows.iter().chain(&holes).chain(&edges).cloned().collect();
    step(&dir, Cmd::AddDrcExclusions { exclusions: rows.iter().map(drc_exclusion_of).collect() }, false, "ui").unwrap();
    let after = crate::kicad_engine::drc_with_options(&dir, every).expect("kicad-cli ran");
    let waived: Vec<Value> = ["clearance", "hole_clearance", "copper_edge_clearance"].iter().flat_map(|k| drc_rows(&after, k)).filter(|v| v["excluded"] == true).collect();
    assert_eq!(waived.len(), rows.len(), "{shape:?}: {after:#}");
    for v in &waived {
        assert_eq!(v["kicad_matched"], true, "{shape:?}: kicad-cli matched the exclusion at the point the studio computed: {v:#}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
