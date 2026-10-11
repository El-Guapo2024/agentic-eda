//! The server side of the hierarchy dialogs, over a real board directory: Import Sheet reads a `.kicad_sch` into the command, a copied sheet's
//! screens ride with the clipboard, the annotate report and the sheet checks answer without writing.
//!
//! The slow tier (`EDA_SLOW_TESTS=1`) hands the results to kicad-cli: the hierarchy is a KiCad project that loads, and the operations leave no
//! `unannotated`, `duplicate_reference` or `duplicate_sheet_names` finding behind.

use crate::board::{load, save, step, undo, Meta};
use eda_model::ir::Point;
use eda_model::ConstraintModel;
use eda_ops::{Cmd, Domain};
use serde_json::json;
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_cli_sch_hierarchy_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn example(file: &str) -> (PathBuf, ConstraintModel) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(file);
    let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    (path, model)
}

/// mcu30's flat schematic as the engine draws it, in a scratch board.
fn mcu30_board(name: &str) -> PathBuf {
    let dir = scratch(name);
    let (intent, model) = example("mcu_board_30plus.yaml");
    let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_um: 100, spacing_um: 300 };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    dir
}

/// The LDO example as a `.kicad_sch` KiCad would read, written beside the board: U1, C1, C2, R1, D1 and J1 -- references mcu30 has.
fn write_ldo_file(dir: &Path, file: &str) -> PathBuf {
    let (_, model) = example("ldo.yaml");
    let design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    let text = eda_kicad::export_kicad_sch(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-10", title: "LDO stage" }).unwrap();
    let path = dir.join(file);
    std::fs::write(&path, text).unwrap();
    path
}

fn import_cmd(source: &str, as_sheet: bool, keep: bool) -> Cmd {
    Cmd::ImportSheet {
        source: source.into(),
        content: None,
        as_sheet,
        name: "LDO stage".into(),
        file: "ldo_stage".into(),
        at: Point { x: 250_000, y: 40_000 },
        size: (40_000, 20_000),
        keep_annotations: keep,
        order: eda_ops::AnnotateOrder::XThenY,
        options: Default::default(),
    }
}

/// Every placed unit of the design's live screens: (reference, unit) -- no pair twice.
fn placed_units(design: &eda_model::ir::Design) -> Vec<(String, u32)> {
    let reachable = eda_model::sheet_list::reachable_files(design);
    let mut out: Vec<(String, u32)> = design.schematic.iter().flat_map(|s| s.symbols.iter().map(|x| (x.id.clone(), x.unit))).collect();
    for (file, sec) in design.sheet_contents.iter().flatten().filter(|(f, _)| reachable.contains(*f)) {
        let _ = file;
        out.extend(sec.symbols.iter().map(|x| (x.id.clone(), x.unit)));
    }
    out.sort();
    out
}

#[test]
fn import_sheet_reads_the_file_into_the_command_and_the_sheet_is_one_undo_step() {
    let dir = mcu30_board("import");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    let (_, before, _) = load(&dir).unwrap();
    assert!(before.schematic.as_ref().unwrap().sheets.is_empty());

    // a relative path is relative to the board's directory
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let sheet = design.schematic.as_ref().unwrap().sheets.iter().find(|s| s.name == "LDO stage").expect("the sheet is on the root");
    assert_eq!((sheet.file.as_str(), sheet.at, sheet.size), ("ldo_stage.kicad_sch", Point { x: 250_000, y: 40_000 }, (40_000, 20_000)));
    let screen = &design.sheet_contents.as_ref().unwrap()["ldo_stage.kicad_sch"];
    assert!(screen.symbols.len() >= 6, "the LDO's six parts: {:?}", screen.symbols.iter().map(|s| &s.id).collect::<Vec<_>>());

    // no (reference, unit) twice in the whole schematic: the file's U1, C1, ... were numbered round mcu30's
    let units = placed_units(&design);
    let mut unique = units.clone();
    unique.dedup();
    assert_eq!(units.len(), unique.len(), "{units:?}");
    assert!(units.iter().any(|(r, _)| r == "U2"), "the LDO is the second U: {units:?}");
    // the parts exist for everything the board draws, and the nets of the new sheet are in the net list
    assert!(design.sheet_contents.as_ref().unwrap()["ldo_stage.kicad_sch"].symbols.iter().all(|s| model.part(&s.id).is_some()));
    let nets = design.nets.as_ref().expect("a schematic edit stores the net list");
    assert!(nets.iter().any(|n| n.pins.iter().any(|p| p.starts_with("U2."))), "U2's pins are on nets");
    // the file the user named is untouched
    assert!(dir.join("ldo_stage.kicad_sch").exists());

    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let (_, back, _) = load(&dir).unwrap();
    assert!(back.schematic.as_ref().unwrap().sheets.is_empty() && back.sheet_contents.is_none(), "one undo takes the whole import back");
}

#[test]
fn a_file_that_is_not_there_or_not_a_schematic_is_refused_and_changes_nothing() {
    let dir = mcu30_board("import_bad");
    let before = std::fs::read_to_string(dir.join("design.json")).unwrap();
    let e = step(&dir, import_cmd("nope.kicad_sch", true, false), false, "test").unwrap_err();
    assert!(!e.is_empty());
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();
    assert!(step(&dir, import_cmd("notes.txt", true, false), false, "test").is_err());
    std::fs::write(dir.join("junk.kicad_sch"), "(not a schematic").unwrap();
    assert!(step(&dir, import_cmd("junk.kicad_sch", true, false), false, "test").is_err());
    assert_eq!(std::fs::read_to_string(dir.join("design.json")).unwrap(), before);
}

#[test]
fn import_sheet_contents_goes_on_the_root_and_keep_annotations_numbers_only_what_is_taken() {
    let dir = mcu30_board("import_contents");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", false, true), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    assert!(design.schematic.as_ref().unwrap().sheets.is_empty() && design.sheet_contents.is_none(), "the contents are on the root sheet, no sheet was made");
    let units = placed_units(&design);
    let mut unique = units.clone();
    unique.dedup();
    assert_eq!(units.len(), unique.len(), "every (reference, unit) once: {units:?}");
    assert!(units.len() >= 30 + 6);
}

#[test]
fn the_sheet_files_and_the_checks_are_answered_without_writing() {
    let dir = mcu30_board("sheet_files");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    write_ldo_file(&dir.join("sub"), "deeper.kicad_sch");
    std::fs::create_dir_all(dir.join(".kicad")).unwrap();
    write_ldo_file(&dir.join(".kicad"), "scratch.kicad_sch");
    let files = crate::sch_hierarchy_api::sheet_files(&dir);
    let paths: Vec<&str> = files["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert!(paths.contains(&"ldo_stage.kicad_sch") && paths.contains(&"sub/deeper.kicad_sch"), "{paths:?}");
    assert!(!paths.iter().any(|p| p.contains("scratch")), "derived scratch is not a project file");

    let before = std::fs::read_to_string(dir.join("design.json")).unwrap();
    let r = crate::sch_hierarchy_api::sheet_file(&dir, json!({ "path": "ldo_stage.kicad_sch" }).to_string().as_bytes());
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["blocked"], false);
    assert_eq!(r["stem"], "ldo_stage");
    assert_eq!(r["dest_file"], "ldo_stage.kicad_sch");
    assert!(r["symbols"].as_u64().unwrap() >= 6);
    let findings = r["findings"].as_array().unwrap();
    assert!(findings.iter().any(|f| f["severity"] == "info" && f["text"].as_str().unwrap().contains("are used in this schematic already")), "{findings:?}");
    assert_eq!(std::fs::read_to_string(dir.join("design.json")).unwrap(), before, "a check writes nothing");
    // a path that is not a schematic
    assert_eq!(crate::sch_hierarchy_api::sheet_file(&dir, json!({ "path": "board.json" }).to_string().as_bytes())["ok"], false);
}

/// The annotate report over a real board: the same numbers the verb gives, nothing written.
#[test]
fn the_annotate_report_matches_what_the_verb_does() {
    let dir = mcu30_board("annotate_report");
    let req = json!({ "reset_existing": true, "order": "x_then_y", "options": { "scope": "all", "algo": "sheet_x100" } });
    let before = std::fs::read_to_string(dir.join("design.json")).unwrap();
    let report = crate::sch_hierarchy_api::annotate(&dir, req.to_string().as_bytes());
    assert_eq!(report["ok"], true, "{report}");
    assert!(report["changed"].as_u64().unwrap() > 0);
    assert_eq!(std::fs::read_to_string(dir.join("design.json")).unwrap(), before, "a report writes nothing");
    let cmd: Cmd = serde_json::from_value(json!({ "op": "annotate", "reset_existing": true, "order": "x_then_y", "options": { "scope": "all", "algo": "sheet_x100" } })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    // one sheet, page 1: every part is numbered from 101
    let mut refs: Vec<String> = design.schematic.as_ref().unwrap().symbols.iter().filter(|s| s.id.starts_with('R')).map(|s| s.id.clone()).collect();
    refs.sort();
    assert_eq!(refs.first().map(String::as_str), Some("R101"), "{refs:?}");
}

#[test]
fn a_copied_sheet_is_pasted_with_its_screens_as_a_sheet_of_its_own() {
    let dir = mcu30_board("sheet_paste");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sheet_id = design.schematic.as_ref().unwrap().sheets[0].id.clone();

    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [sheet_id] }).to_string().as_bytes());
    assert_eq!(copied["ok"], true, "{copied}");
    assert_eq!(copied["sheets"], 1);
    let text = copied["text"].as_str().unwrap();
    assert!(text.contains("(sheet") && text.contains("(property \"Sheetname\" \"LDO stage\"") && text.contains("(property \"Sheetfile\" \"ldo_stage.kicad_sch\""), "{text}");

    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": text }).to_string().as_bytes());
    assert_eq!(parsed["ok"], true, "{parsed}");
    assert_eq!(parsed["preview"]["sheets"].as_array().unwrap().len(), 1, "the paste shows the sheet that comes");
    assert!(parsed["fragment"]["sheet_screens"].get("ldo_stage.kicad_sch").is_some(), "the screens ride with the fragment");

    // paste it, as the studio sends it: the fragment from the parse, moved
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 60_000, "dy": 0 })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let sheets = &design.schematic.as_ref().unwrap().sheets;
    assert_eq!(sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["LDO stage", "LDO stage1"]);
    assert_eq!(sheets[1].file, "ldo_stage1.kicad_sch");
    let units = placed_units(&design);
    let mut unique = units.clone();
    unique.dedup();
    assert_eq!(units.len(), unique.len(), "the copy's references are new: {units:?}");
    assert!(design.sheet_contents.as_ref().unwrap()["ldo_stage1.kicad_sch"].symbols.iter().all(|s| model.part(&s.id).is_some()));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_eq!(load(&dir).unwrap().1.schematic.as_ref().unwrap().sheets.len(), 1, "one undo takes the pasted sheet back");
}

#[test]
fn a_cut_sheet_comes_back_from_the_clipboard_after_the_board_was_edited() {
    let dir = mcu30_board("sheet_cut");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sheet = design.schematic.as_ref().unwrap().sheets[0].clone();
    let before_refs = placed_units(&design);
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [sheet.id.clone()] }).to_string().as_bytes());
    // Cut: delete it
    let delete: Cmd = serde_json::from_value(json!({ "op": "sch_edit", "verb": "delete_sheet", "id": sheet.id })).unwrap();
    step(&dir, delete, false, "test").unwrap();
    assert!(load(&dir).unwrap().1.schematic.as_ref().unwrap().sheets.is_empty());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"] })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    assert_eq!(design.schematic.as_ref().unwrap().sheets[0].name, "LDO stage");
    assert_eq!(placed_units(&design), before_refs, "the references nothing else holds are kept");
}

// ------------------------------------------------------------------------------------------------------ the slow tier: kicad-cli

fn kicad_cli_ready() -> bool {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return false;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: kicad-cli not found");
        return false;
    }
    true
}

/// kicad-cli's ERC of the board as the studio would run it: the kinds of the findings, with how many.
fn erc_kinds(dir: &Path) -> std::collections::BTreeMap<String, usize> {
    let report = crate::kicad_engine::erc(dir).expect("kicad-cli loads the hierarchy and runs ERC");
    let mut kinds = std::collections::BTreeMap::new();
    for v in report["violations"].as_array().into_iter().flatten() {
        if v["severity"] == "exclusion" {
            continue;
        }
        *kinds.entry(v["check"].as_str().or_else(|| v["type"].as_str()).unwrap_or("?").to_string()).or_default() += 1;
    }
    kinds
}

/// What an operation on the hierarchy must not leave behind, whatever the board had before.
const REFERENCE_CHECKS: &[&str] = &["unannotated", "duplicate_reference", "duplicate_sheet_names", "different_unit_footprint", "different_unit_net"];

fn assert_no_reference_findings(dir: &Path, what: &str) {
    let kinds = erc_kinds(dir);
    println!("kicad-cli ERC {what}: {kinds:?}");
    let bad: Vec<(&String, &usize)> = kinds.iter().filter(|(k, _)| REFERENCE_CHECKS.contains(&k.as_str())).collect();
    assert!(bad.is_empty(), "{what}: kicad-cli's ERC reports {bad:?} (all: {kinds:?})");
}

/// What a sheet that comes in may leave in kicad-cli's ERC: two power flags on one net (the sheet has its own, so does the design), nothing else.
fn assert_only_flag_clashes(dir: &Path, what: &str) {
    assert_no_reference_findings(dir, what);
    let report = crate::kicad_engine::erc(dir).expect("kicad-cli runs ERC");
    for v in report["violations"].as_array().into_iter().flatten().filter(|v| v["severity"] != "exclusion") {
        let kind = v["check"].as_str().or_else(|| v["type"].as_str()).unwrap_or("?");
        let flags_only = v["items"].as_array().is_some_and(|items| !items.is_empty() && items.iter().all(|i| i["id"].as_str().is_some_and(|id| id.starts_with("#FLG"))));
        assert!(kind == "pin_to_pin" && flags_only, "{what}: kicad-cli's ERC reports {kind} on {}", v["items"]);
    }
}

#[test]
fn slow_the_annotated_import_and_paste_leave_kicad_cli_with_no_reference_finding() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_hierarchy");
    // mcu30 as module sheets, then a file in as a sheet, then that sheet pasted, then everything annotated again by sheet number
    step(&dir, Cmd::ReorganizeSheets, false, "test").unwrap();
    assert_no_reference_findings(&dir, "reorganized");
    assert!(erc_kinds(&dir).is_empty(), "the reorganized board is clean: {:?}", erc_kinds(&dir));
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    assert_only_flag_clashes(&dir, "after Import Sheet");
    let (_, design, _) = load(&dir).unwrap();
    let id = design.schematic.as_ref().unwrap().sheets.iter().find(|s| s.name == "LDO stage").unwrap().id.clone();
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [id] }).to_string().as_bytes());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000 })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    assert_only_flag_clashes(&dir, "after pasting the sheet");
    let all: Cmd = serde_json::from_value(json!({ "op": "annotate", "reset_existing": true, "order": "x_then_y", "options": { "scope": "all", "algo": "sheet_x100" } })).unwrap();
    step(&dir, all, false, "test").unwrap();
    assert_only_flag_clashes(&dir, "after Annotate over the whole schematic");
}
