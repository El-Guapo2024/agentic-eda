//! The server side of the hierarchy dialogs, over a real board directory: Import Sheet reads a `.kicad_sch` into the command, a copied sheet's
//! screens ride with the clipboard, the annotate report and the sheet checks answer without writing.
//!
//! The slow tier (`EDA_SLOW_TESTS=1`) hands the results to kicad-cli: the hierarchy is a KiCad project that loads, and the operations leave no
//! `unannotated`, `duplicate_reference` or `duplicate_sheet_names` finding behind.

use eda_model::units::{mm, Nm};
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
    let meta = Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
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
        at: Point { x: mm(250.0), y: mm(40.0) },
        size: (mm(40.0), mm(20.0)),
        keep_annotations: keep,
        order: eda_ops::AnnotateOrder::XThenY,
        options: Default::default(),
        as_group: false,
        group_name: String::new(),
        block: String::new(),
    }
}

/// Every placed unit of every sheet of the hierarchy, by the reference its placement gives it (a screen shown twice counts twice).
fn units_per_placement(design: &eda_model::ir::Design) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for e in eda_model::sheet_list::walk(design) {
        let sec = eda_model::sheet_list::screen_of(design, &e.path).unwrap();
        out.extend(sec.symbols.iter().map(|s| (eda_model::sheet_list::reference_under(sec, s, &e.path), s.unit)));
    }
    out.sort();
    out
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
    assert_eq!((sheet.file.as_str(), sheet.at, sheet.size), ("ldo_stage.kicad_sch", Point { x: mm(250.0), y: mm(40.0) }, (mm(40.0), mm(20.0))));
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
fn a_copied_sheet_is_pasted_as_a_second_placement_of_its_screen() {
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
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 60_000_000, "dy": 0 })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let sheets = &design.schematic.as_ref().unwrap().sheets;
    assert_eq!(sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["LDO stage", "LDO stage1"]);
    // `SetScreen( existingScreen )`: the new sheet shows the same file, which is still one screen
    assert_eq!(sheets[1].file, "ldo_stage.kicad_sch");
    assert_eq!(design.sheet_contents.as_ref().unwrap().len(), 1);
    let screen = &design.sheet_contents.as_ref().unwrap()["ldo_stage.kicad_sch"];
    let second: Vec<&eda_model::ir::SymbolPathOverride> = screen.instance_overrides.iter().filter(|o| o.sheet_path == [sheets[1].id.clone()]).collect();
    assert_eq!(second.len(), screen.symbols.len(), "every symbol has a reference of its own on the new sheet");
    let units = units_per_placement(&design);
    let mut unique = units.clone();
    unique.dedup();
    assert_eq!(units.len(), unique.len(), "the second placement's references are new: {units:?}");
    // a part for each reference of the new placement, its pins on nets of their own
    let nets = design.nets.as_ref().unwrap();
    for o in &second {
        assert!(model.part(&o.reference).is_some(), "{} is a part", o.reference);
        assert!(nets.iter().any(|n| n.pins.iter().any(|p| p.starts_with(&format!("{}.", o.reference)))), "{} is on the nets", o.reference);
    }
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
    // fields the user placed and a look they gave, then mcu30 as module sheets: the places and looks are kept and the board is as clean as before
    let (_, flat, _) = load(&dir).unwrap();
    let r3 = flat.schematic.as_ref().unwrap().symbols.iter().find(|s| s.id == "R3").unwrap().at;
    let edit: Cmd = serde_json::from_value(json!({ "op": "sch_edit", "verb": "edit_field", "id": "fld:R3:Reference", "at": { "x": r3.x - mm(12.7), "y": r3.y + mm(1.27) }, "h": "right", "size_nm": 1_778_000, "bold": true })).unwrap();
    step(&dir, edit, false, "test").unwrap();
    step(&dir, Cmd::ReorganizeSheets, false, "test").unwrap();
    let (_, sheets, _) = load(&dir).unwrap();
    let kept = sheets.sheet_contents.iter().flatten().flat_map(|(_, s)| s.symbols.iter().map(move |x| (s, x))).find(|(_, x)| x.id == "R3").unwrap();
    let place = kept.0.field_layout.get("R3").and_then(|f| f.iter().find(|p| p.name == "Reference")).expect("R3's Reference keeps a place of its own");
    assert_eq!((place.dx, place.dy), (mm(-12.7), mm(1.27)), "the field is where the user put it from its symbol");
    assert_no_reference_findings(&dir, "reorganized");
    assert!(erc_kinds(&dir).is_empty(), "the reorganized board is clean: {:?}", erc_kinds(&dir));
    // Replace All over the sheets: values change, nothing else
    let replace = Cmd::ReplaceText { search: eda_ops::search::SearchData { find: "100nF".into(), replace: "220nF".into(), ..Default::default() }, items: None, all_sheets: true };
    step(&dir, replace, false, "test").unwrap();
    assert!(erc_kinds(&dir).is_empty(), "after Replace All over the sheets: {:?}", erc_kinds(&dir));
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    assert_only_flag_clashes(&dir, "after Import Sheet");
    let (_, design, _) = load(&dir).unwrap();
    let id = design.schematic.as_ref().unwrap().sheets.iter().find(|s| s.name == "LDO stage").unwrap().id.clone();
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [id] }).to_string().as_bytes());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000_000 })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    assert_only_flag_clashes(&dir, "after pasting the sheet");
    let all: Cmd = serde_json::from_value(json!({ "op": "annotate", "reset_existing": true, "order": "x_then_y", "options": { "scope": "all", "algo": "sheet_x100" } })).unwrap();
    step(&dir, all, false, "test").unwrap();
    assert_only_flag_clashes(&dir, "after Annotate over the whole schematic");
}

// ---------------------------------------------------------------------------------------- Reorganize with a multi-unit part

/// A dual op-amp the way a library has it: units 1 and 2 the amplifiers, unit 3 the power pins.
fn dual_opamp() -> (eda_model::symbol::LibSymbol, eda_model::Part) {
    use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
    use eda_model::{Part, Pin, PinKind};
    let pin = |number: &str, name: &str, unit: u32, y: f64, left: bool| LibPin {
        number: number.into(),
        name: name.into(),
        electrical_type: if unit == 3 { "power_in" } else { "passive" }.into(),
        shape: "line".into(),
        at: SPoint::new(if left { -7.62 } else { 7.62 }, y),
        angle_deg: if left { 0.0 } else { 180.0 },
        length_mm: 2.54,
        unit,
        hidden: false,
    };
    let sym = LibSymbol { connectivity: Default::default(),
        lib_id: "Test:Dual3".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 0, start: SPoint::new(-5.08, 5.08), end: SPoint::new(5.08, -5.08), stroke_mm: 0.254, filled: false }],
        pins: vec![
            pin("1", "OUT1", 1, 0.0, false),
            pin("2", "IN1-", 1, 2.54, true),
            pin("3", "IN1+", 1, -2.54, true),
            pin("7", "OUT2", 2, 0.0, false),
            pin("6", "IN2-", 2, 2.54, true),
            pin("5", "IN2+", 2, -2.54, true),
            pin("8", "V+", 3, 2.54, false),
            pin("4", "V-", 3, -2.54, false),
        ],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "U".into(),
        unit_count: 3,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    };
    let names = [("1", "OUT1"), ("2", "IN1-"), ("3", "IN1+"), ("4", "V-"), ("5", "IN2+"), ("6", "IN2-"), ("7", "OUT2"), ("8", "V+")];
    let pins = names.iter().map(|(n, name)| Pin { number: n.to_string(), name: Some(name.to_string()), kind: PinKind::Passive }).collect();
    let part = Part { reference: "U9".into(), mpn: None, lcsc: None, value: Some("TL072".into()), package: None, footprint: None, symbol: Some("Test:Dual3".into()), datasheet: None, pins, body_nm: None, edge: None };
    (sym, part)
}

#[test]
fn slow_reorganize_with_a_multi_unit_part_is_a_hierarchy_kicad_cli_reads_without_a_unit_finding() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = scratch("slow_multi_unit");
    let (_, flat_model) = example("mcu_board_30plus.yaml");
    // mcu30 with a dual op-amp wired to the rails and to itself, as the intent file of the scratch board
    let mut model = flat_model.clone();
    let (sym, part) = dual_opamp();
    model.symbols.push(sym);
    model.parts.push(part);
    model.nets.iter_mut().find(|n| n.name == "VDD").unwrap().pins.push("U9.8".into());
    model.nets.iter_mut().find(|n| n.name == "GND").unwrap().pins.push("U9.4".into());
    // (U9.3 and U9.5 are on no net: KiCad names each the net of a pin alone, with the unit it is on, `unconnected-(U9A-IN1+-Pad3)`)
    for (name, a, b) in [("AMP_FB1", "U9.1", "U9.2"), ("AMP_FB2", "U9.7", "U9.6")] {
        model.nets.push(eda_model::Net { name: name.into(), pins: vec![a.into(), b.into()] });
    }
    let intent = dir.join("intent.yaml");
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    // the flat drawing is mcu30's, with the op-amp's three units placed on the root (the nets are the model's)
    let mut design = eda_engine::derive_schematic(&flat_model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    {
        let root = design.schematic.as_mut().unwrap();
        for (unit, x) in [(1, mm(300.0)), (2, mm(330.0)), (3, mm(360.0))] {
            root.symbols.push(eda_model::ir::SymbolInstance {
                id: "U9".into(),
                at: Point { x, y: mm(20.0) },
                rot: 0,
                mirrored: false,
                mirror_y: false,
                lib_id: "Test:Dual3".into(),
                unit,
                value: "TL072".into(),
                footprint: String::new(),
                datasheet: String::new(),
                dnp: false,
                exclude_from_bom: false,
                exclude_from_board: false,
                exclude_from_sim: false,
            });
        }
        root.symbols.sort_by(|a, b| (&a.id, a.unit).cmp(&(&b.id, b.unit)));
    }
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();

    step(&dir, Cmd::ReorganizeSheets, false, "test").unwrap();
    let (_, after, _) = load(&dir).unwrap();
    let units: Vec<u32> = after.sheet_contents.iter().flatten().flat_map(|(_, s)| s.symbols.iter()).filter(|s| s.id == "U9").map(|s| s.unit).collect();
    assert_eq!(units.len(), 3, "each unit is placed once: {units:?}");
    assert_no_reference_findings(&dir, "reorganized with a multi-unit part");
    let kinds = erc_kinds(&dir);
    assert!(kinds.keys().all(|k| k == "pin_not_connected"), "the reorganized board with its dual op-amp is clean but for its two pins left unwired: {kinds:?}");
    // every pin's net under the name kicad-cli's netlist gives it: the labels' with their sheets' paths, a lone pin's with the unit it is drawn on
    let (_, mut drawn, model) = load(&dir).unwrap();
    let ours: std::collections::BTreeMap<String, String> = crate::board::trace_kicad_names(&mut drawn, &model).into_iter().flat_map(|n| n.pins.into_iter().map(move |p| (p, n.name.clone()))).collect();
    let theirs = kicad_net_names(&dir);
    assert_eq!(theirs.get("U9.3").map(String::as_str), Some("unconnected-(U9A-IN1+-Pad3)"));
    assert_eq!(theirs.get("U9.5").map(String::as_str), Some("unconnected-(U9B-IN2+-Pad5)"));
    // (a pin alone is on no traced net: the board writer names its pad's net, `unconnected_pin_nets`, the way it does here)
    for (pin, name) in &theirs {
        let ours = ours.get(pin).cloned().unwrap_or_else(|| {
            let (reference, number) = pin.split_once('.').unwrap();
            let shown = model.part(reference).and_then(|p| p.pins.iter().find(|p| p.number == number)).and_then(|p| p.name.clone());
            let with_unit = model.symbol_links.get(reference).and_then(|l| l.units.get(number)).cloned();
            eda_engine::nets::unconnected_net_name(pin, shown.as_deref(), with_unit.as_deref())
        });
        assert_eq!(&ours, name, "{pin}: kicad-cli names its net {name}");
    }
}

/// kicad-cli's netlist of the board: the name of each pin's net, by `REF.PIN`, power symbols left out.
fn kicad_net_names(dir: &Path) -> std::collections::BTreeMap<String, String> {
    let done = crate::kicad_engine::export_sch(dir, "netlist", &["--format".to_string(), "kicadxml".to_string()]).expect("kicad-cli exports the netlist");
    let text = std::fs::read_to_string(dir.join(done["files"][0].as_str().expect("one netlist file"))).unwrap();
    let attr = |line: &str, name: &str| line.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap_or("").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'");
    let mut out = std::collections::BTreeMap::new();
    let mut net = String::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with("<net ") {
            net = attr(line, "name");
        } else if line.starts_with("<node ") && !attr(line, "ref").starts_with('#') {
            out.insert(format!("{}.{}", attr(line, "ref"), attr(line, "pin")), net.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------------------- a screen shown by two sheets

/// kicad-cli's netlist of the board (`kicad-cli sch export netlist --format kicadxml`): the pins of each net, `REF.PIN` sorted, power symbols left out.
pub(crate) fn kicad_net_groups(dir: &Path) -> Vec<Vec<String>> {
    let done = crate::kicad_engine::export_sch(dir, "netlist", &["--format".to_string(), "kicadxml".to_string()]).expect("kicad-cli exports the netlist");
    let file = dir.join(done["files"][0].as_str().expect("one netlist file"));
    let text = std::fs::read_to_string(file).unwrap();
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with("<net ") {
            current = Some(Vec::new());
        } else if line.starts_with("</net>") {
            if let Some(mut g) = current.take() {
                g.sort();
                groups.push(g);
            }
        } else if line.starts_with("<node ") {
            let attr = |name: &str| line.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
            let reference = attr("ref");
            if let Some(g) = current.as_mut().filter(|_| !reference.starts_with('#')) {
                g.push(format!("{reference}.{}", attr("pin")));
            }
        }
    }
    groups.retain(|g| !g.is_empty());
    groups.sort();
    groups
}

/// The pins of the nets the board's own net list (`design.nets`) has, sorted as [`kicad_net_groups`] sorts them.
pub(crate) fn our_net_groups(dir: &Path) -> Vec<Vec<String>> {
    let (_, design, _) = load(dir).unwrap();
    let mut groups: Vec<Vec<String>> = design.nets.unwrap_or_default().into_iter().map(|n| {
        let mut p = n.pins;
        p.sort();
        p
    }).filter(|p| !p.is_empty()).collect();
    groups.sort();
    groups
}

/// A copied sheet pasted beside its original shows the same screen (`SCH_EDITOR_CONTROL::Paste`: `SetScreen( existingScreen )`), and kicad-cli makes a net per
/// placement of it: the same drawing, two sets of nets, the second under the references the new placement gave the symbols. The studio's net list is
/// the one kicad-cli exports for every net either placement's parts are on, the ERC finds no duplicate reference, and Undo takes the placement back.
#[test]
fn slow_a_pasted_sheet_shares_its_screen_and_kicad_cli_makes_a_net_per_placement() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_shared_screen");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sheet = design.schematic.as_ref().unwrap().sheets[0].clone();
    let first_refs: Vec<String> = design.sheet_contents.as_ref().unwrap()[&sheet.file].symbols.iter().map(|s| s.id.clone()).collect();
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [sheet.id] }).to_string().as_bytes());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000_000 })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let second = design.schematic.as_ref().unwrap().sheets[1].clone();
    assert_eq!(second.file, sheet.file, "one screen, two placements");
    let screen = &design.sheet_contents.as_ref().unwrap()[&sheet.file];
    let second_refs: Vec<String> = screen.instance_overrides.iter().filter(|o| o.sheet_path == [second.id.clone()]).map(|o| o.reference.clone()).collect();
    assert_eq!(second_refs.len(), first_refs.len());

    // the derived files say it the way KiCad does: one file, each symbol with a reference per path
    let (d, m) = crate::kicad_engine::load_with_schematic(&dir).unwrap();
    let files = eda_kicad::export_kicad_sch_tree(&d, &m, &eda_kicad::ExportMeta { date: "2026-10-23", title: "t" }, "root.kicad_sch").unwrap();
    assert_eq!(files.len(), 2, "the root and the one shared screen");
    let child = &files.iter().find(|(f, _)| f == &sheet.file).unwrap().1;
    for r in first_refs.iter().chain(&second_refs) {
        assert!(child.contains(&format!("(reference \"{r}\")")), "{r} under its path");
    }

    let kicad = kicad_net_groups(&dir);
    let ours = our_net_groups(&dir);
    let refs: std::collections::BTreeSet<&str> = first_refs.iter().chain(&second_refs).map(String::as_str).collect();
    // each net cut down to the pins of the two placements (the rest of mcu30's stored net list is the intent's, not what its drawing says)
    let here = |groups: &[Vec<String>]| -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = groups.iter().map(|g| g.iter().filter(|p| p.split_once('.').is_some_and(|(r, _)| refs.contains(r))).cloned().collect::<Vec<_>>()).filter(|g| g.len() >= 2).collect();
        out.sort();
        out
    };
    let (kicad_here, ours_here) = (here(&kicad), here(&ours));
    println!("kicad-cli: {} nets touch the two placements", kicad_here.len());
    assert!(kicad_here.iter().any(|g| g.iter().any(|p| p.starts_with(&format!("{}.", second_refs[0])))), "kicad-cli's netlist has the second placement's parts on nets");
    assert_eq!(ours_here, kicad_here, "a net per placement, the same nets kicad-cli exports");
    assert_no_reference_findings(&dir, "after pasting a second placement of the screen");

    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let (_, back, _) = load(&dir).unwrap();
    assert_eq!(back.schematic.as_ref().unwrap().sheets.len(), 1, "one undo takes the placement back");
    assert!(back.sheet_contents.as_ref().unwrap()[&sheet.file].instance_overrides.iter().all(|o| o.sheet_path != [second.id.clone()]));
}

// ---------------------------------------------------------------------------------------- a bus across sheets

/// A board of four resistors, R1 and R2 on the root and R3 and R4 on a sheet "child", each pin 1 under a label: the members of `bus_label` on the root,
/// D0 and D1 in the child; on the root a
/// bus labelled `bus_label` to the sheet pin `D[0..1]`, inside the child a bus with no label yet. Returns the board, the child's placement id and where
/// the child's hierarchical label `D[0..1]` is to go.
fn bus_board(name: &str, bus_label: &str) -> (PathBuf, String, Point) {
    use eda_model::ir::{LabelKind, LabelShape, NetLabel, SheetInstance, SheetPin, Wire};
    let dir = scratch(name);
    let intent = dir.join("intent.yaml");
    let part = |r: &str| format!("  - {{ reference: {r}, value: 10k, package: \"0603\", pins: [{{ number: \"1\", kind: passive }}, {{ number: \"2\", kind: passive }}] }}\n");
    let yaml = format!("parts:\n{}{}{}{}nets:\n  - {{ name: A, pins: [R1.2, R2.2] }}\n  - {{ name: B, pins: [R3.2, R4.2] }}\n", part("R1"), part("R2"), part("R3"), part("R4"));
    std::fs::write(&intent, yaml).unwrap();
    let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    let mut root = design.schematic.take().unwrap();
    let mut child = root.clone();
    root.symbols.retain(|s| s.id == "R1" || s.id == "R2");
    child.symbols.retain(|s| s.id == "R3" || s.id == "R4");
    for sec in [&mut root, &mut child] {
        sec.wires.clear();
        sec.labels.clear();
        sec.power_symbols.clear();
        sec.no_connects.clear();
        sec.junctions.clear();
    }
    let pin1 = |sec: &eda_model::ir::SchematicSection, r: &str| -> Point {
        let sym = sec.symbols.iter().find(|s| s.id == r).unwrap();
        let part = model.part(r).unwrap();
        eda_engine::placed::pin_points_in(sec, sym, part, model.real_symbol_of_instance(sec, sym, part).as_ref()).into_iter().find(|(n, _)| n == "1").unwrap().1
    };
    let label = |net: &str, at: Point, kind: LabelKind| NetLabel { id: String::new(), net: net.into(), at, kind };
    let wire = |pts: Vec<Point>, bus: bool| Wire { id: String::new(), net: String::new(), pins: vec![], pts, bus };
    // each pin 1 a short wire out to its label
    for (sec, refs) in [(&mut root, ["R1", "R2"]), (&mut child, ["R3", "R4"])] {
        let names = if refs[0] == "R1" { eda_kicad::expand_bus_members(bus_label, &[]).unwrap() } else { vec!["D0".to_string(), "D1".to_string()] };
        for (r, net) in refs.iter().zip(names) {
            let at = pin1(sec, r);
            let end = Point { x: at.x - mm(5.08), y: at.y };
            sec.wires.push(wire(vec![at, end], false));
            sec.labels.push(label(&net, end, LabelKind::Local));
        }
    }
    // the child: a bus from the hierarchical label down
    let hl = Point { x: mm(20.32), y: mm(20.32) };
    child.wires.push(wire(vec![hl, Point { x: mm(20.32), y: mm(40.64) }], true));
    // the root: the sheet, its bus pin, and a bus label on a stub of bus away from it
    let sheet_at = Point { x: mm(152.4), y: mm(25.4) };
    let pin_at = Point { x: sheet_at.x, y: sheet_at.y + mm(5.08) };
    root.sheets.push(SheetInstance { id: String::new(), name: "child".into(), file: "child.kicad_sch".into(), at: sheet_at, size: (mm(25.4), mm(20.32)), pins: vec![SheetPin { id: String::new(), name: "D[0..1]".into(), shape: LabelShape::Bidirectional, at: pin_at }], page: String::new() });
    let stub = Point { x: pin_at.x - mm(25.4), y: pin_at.y };
    root.wires.push(wire(vec![stub, pin_at], true));
    root.labels.push(label(bus_label, stub, LabelKind::Local));
    root.assign_missing_ids();
    child.assign_missing_ids();
    let sheet_id = root.sheets[0].id.clone();
    design.schematic = Some(root);
    design.sheet_contents = Some([("child.kicad_sch".to_string(), child)].into_iter().collect());
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    (dir, sheet_id, hl)
}

/// A bus across sheets: the hierarchical label `D[0..1]` put on the child's bus (one `add_label` on the sheet, one undo step) puts each member's nets on the
/// two sheets on one net, as kicad-cli's netlist has them (`propagateToNeighbors`, `propagate_bus_neighbors`); a root bus named `X[0..1]` (its nets X0 and
/// X1) matches the child's D0 and D1 by index (`matchBusMember`). Before the label, and after its undo, the sheets' nets are apart in both.
#[test]
fn slow_a_bus_across_sheets_is_one_net_per_member_as_kicad_cli_makes_it() {
    if !kicad_cli_ready() {
        return;
    }
    for bus_label in ["D[0..1]", "X[0..1]"] {
        let (dir, sheet_id, hl) = bus_board(&format!("slow_bus_{}", &bus_label[..1]), bus_label);
        // the nets of the pins 1 (the pins 2 are on the intent's nets A and B)
        let cut = |groups: Vec<Vec<String>>| -> Vec<Vec<String>> {
            let mut out: Vec<Vec<String>> = groups.into_iter().map(|g| g.into_iter().filter(|p| p.ends_with(".1")).collect::<Vec<_>>()).filter(|g| !g.is_empty()).collect();
            out.sort();
            out
        };
        let apart = vec![vec!["R1.1".to_string()], vec!["R2.1".into()], vec!["R3.1".into()], vec!["R4.1".into()]];
        assert_eq!(cut(kicad_net_groups(&dir)), apart, "{bus_label}: kicad-cli has the two sheets apart before the bus is drawn");
        let ours_before = our_net_groups(&dir);
        let add = Cmd::AddLabel { net: "D[0..1]".into(), at: hl, kind: eda_model::ir::LabelKind::Hierarchical { shape: eda_model::ir::LabelShape::Bidirectional } };
        step(&dir, Cmd::OnSheet { sheet: sheet_id.clone(), cmd: Box::new(add) }, false, "test").unwrap();
        let joined = vec![vec!["R1.1".to_string(), "R3.1".into()], vec!["R2.1".to_string(), "R4.1".into()]];
        let kicad = cut(kicad_net_groups(&dir));
        assert_eq!(kicad, joined, "{bus_label}: kicad-cli joins each member's nets across the sheets");
        assert_eq!(cut(our_net_groups(&dir)), joined, "{bus_label}: and so does the studio's net list");
        let (_, design, _) = load(&dir).unwrap();
        let name_of = |pin: &str| design.nets.as_ref().unwrap().iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap();
        assert_eq!(name_of("R3.1"), if bus_label.starts_with('X') { "/X0" } else { "/D0" }, "the root's member names the net, with the root's path");
        let kinds = erc_kinds(&dir);
        // (a bus labelled X[0..1] on a sheet pin D[0..1] is ERC's `bus_to_bus_conflict`: no member in common by name, which the netlist does not mind)
        let allowed = |k: &str| bus_label.starts_with('X') && k == "bus_to_bus_conflict";
        assert!(!kinds.keys().any(|k| (k.starts_with("bus_") && !allowed(k)) || k == "net_not_bus_member" || k == "hier_label_mismatch"), "{bus_label}: {kinds:?}");
        undo(&dir, "test", Some(Domain::Schematic)).unwrap();
        assert_eq!(cut(kicad_net_groups(&dir)), apart, "{bus_label}: one undo takes the bus and the joins back");
        assert_eq!(our_net_groups(&dir), ours_before, "{bus_label}: and the studio's net list is the one it was");
    }
}

// ---------------------------------------------------------------------------------------- a label's own fields

fn edit_label_fields(id: &str, fields: Vec<eda_model::sch_extras::LabelField>) -> Cmd {
    Cmd::SchEdit(eda_ops::sch_edit::SchCmd::EditLabel { id: id.into(), text: None, shape: None, spin: None, size_nm: None, bold: None, italic: None, auto_rotate: None, fields: Some(fields) })
}

fn netclass(text: &str) -> eda_model::sch_extras::LabelField {
    eda_model::sch_extras::LabelField { name: "Netclass".into(), text: text.into(), visible: true, italic: true }
}

/// A label's net class field (`Netclass`, Label Properties' grid: one `edit_label` with `fields`, one undo step) is in the file kicad-cli reads: its ERC reports a class
/// the project does not define (`ERC_TESTER::TestMissingNetclasses`, "Netclass %s is not defined") and none once the board defines it; the label's net is then in
/// the class -- in the studio's model and in kicad-cli's netlist (`<net .. class=..>`). Schematic Setup's Inter-sheet References reach the project file.
#[test]
fn slow_a_labels_net_class_field_is_read_by_kicad_cli() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_label_fields");
    let (_, design, model) = load(&dir).unwrap();
    let root = design.schematic.as_ref().unwrap();
    let label = root.labels.iter().find(|l| l.kind == eda_model::ir::LabelKind::Local && root.wires.iter().any(|w| w.pts.contains(&l.at))).expect("mcu30's flat sheet has a local label on a wire").clone();
    // the label's text: KiCad names its net `/LED1` (the root's path in front); the model has that name, or the intent's where this old drawing shows the
    // net in pieces
    let net = label.net.clone();
    let _ = &model;
    let undefined = |dir: &Path| erc_kinds(dir).get("undefined_netclass").copied().unwrap_or(0);
    assert_eq!(undefined(&dir), 0);

    step(&dir, edit_label_fields(&label.id, vec![netclass("NoSuchClass")]), false, "test").unwrap();
    let (_, d, _) = load(&dir).unwrap();
    assert_eq!(d.schematic.as_ref().unwrap().extras.label_netclass(&label.id), Some("NoSuchClass"));
    assert_eq!(undefined(&dir), 1, "kicad-cli reads the label's net class and has no such class");

    // the board defines the class: no finding, and the label's net is in it
    let (_, _, model) = load(&dir).unwrap();
    let defaults = eda_model::rules::net_class_settings_of(&model.board);
    let power = eda_model::NetClass { name: "HighCurrent".into(), nets: vec![], track_width: Some(mm(0.4)), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 };
    step(&dir, Cmd::SetNetClasses { settings: eda_model::rules::NetClassSettings { classes: vec![power], ..defaults } }, false, "test").unwrap();
    step(&dir, edit_label_fields(&label.id, vec![netclass("HighCurrent")]), false, "test").unwrap();
    assert_eq!(undefined(&dir), 0, "the class is defined");
    let (_, _, model) = load(&dir).unwrap();
    let class = model.board.net_classes.iter().find(|c| c.name == "HighCurrent").unwrap();
    assert!(class.nets.contains(&eda_kicad::exact_pattern(&net)) || class.nets.contains(&eda_kicad::exact_pattern(&format!("/{net}"))), "{net} is in the class: {:?}", class.nets);
    let done = crate::kicad_engine::export_sch(&dir, "netlist", &["--format".to_string(), "kicadxml".to_string()]).unwrap();
    let xml = std::fs::read_to_string(dir.join(done["files"][0].as_str().unwrap())).unwrap();
    let line = xml.lines().map(str::trim).find(|l| l.starts_with("<net ") && (l.contains(&format!("name=\"/{net}\"")) || l.contains(&format!("name=\"{net}\"")))).unwrap_or_else(|| panic!("{net} in the netlist"));
    // (a composite of the classes the net is in, the Default one last: `NET_SETTINGS::makeEffectiveNetclass`)
    let class = line.split("class=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
    assert!(class.split(',').any(|c| c == "HighCurrent"), "kicad-cli puts the net in the label's class: {line}");

    // one undo takes the field's edit back
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_eq!(load(&dir).unwrap().1.schematic.as_ref().unwrap().extras.label_netclass(&label.id), Some("NoSuchClass"));

    // Schematic Setup > Formatting > Inter-sheet References: in the project kicad-cli reads
    let settings = eda_model::sch_extras::IntersheetRefs { show: true, own_page: false, short: true, prefix: "<".into(), suffix: ">".into() };
    step(&dir, Cmd::SetIntersheetRefs { settings }, false, "test").unwrap();
    let (d, m) = crate::kicad_engine::load_with_schematic(&dir).unwrap();
    let pro: serde_json::Value = serde_json::from_str(&eda_kicad::export_kicad_pro_for(&d, &m)).unwrap();
    assert_eq!(pro["schematic"]["drawing"]["intersheets_ref_show"], true);
    assert_eq!(pro["schematic"]["drawing"]["intersheets_ref_prefix"], "<");
    assert!(erc_kinds(&dir).get("undefined_netclass").is_some(), "kicad-cli still loads the project");
}

/// A defined class "HighCurrent" on the board in `dir` (Board Setup > Net Classes).
fn define_high_current(dir: &Path) {
    let (_, _, model) = load(dir).unwrap();
    let defaults = eda_model::rules::net_class_settings_of(&model.board);
    let class = eda_model::NetClass { name: "HighCurrent".into(), nets: vec![], track_width: Some(mm(0.4)), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 };
    step(dir, Cmd::SetNetClasses { settings: eda_model::rules::NetClassSettings { classes: vec![class], ..defaults } }, false, "test").unwrap();
}

/// The classes kicad-cli's netlist puts the net of `pin` ("REF.PIN") in (`<net .. class="A,B">`).
fn kicad_classes_of_pin(dir: &Path, pin: &str) -> Vec<String> {
    let done = crate::kicad_engine::export_sch(dir, "netlist", &["--format".to_string(), "kicadxml".to_string()]).unwrap();
    let xml = std::fs::read_to_string(dir.join(done["files"][0].as_str().unwrap())).unwrap();
    let (r, n) = pin.split_once('.').unwrap();
    let mut class = String::new();
    for line in xml.lines().map(str::trim) {
        if line.starts_with("<net ") {
            class = line.split("class=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
        } else if line.starts_with("<node ") && line.contains(&format!("ref=\"{r}\"")) && line.contains(&format!("pin=\"{n}\"")) {
            return class.split(',').map(String::from).collect();
        }
    }
    panic!("{pin} is in no net of kicad-cli's netlist");
}

/// A bus label's net class is its members' (`checkNetclassDrivers`: `processBusMember` appends the classes to each member's name): the nets of D0 and D1 are in the
/// class in the studio's model and in kicad-cli's netlist, which reads the same field from the file.
#[test]
fn slow_a_bus_labels_net_class_is_its_members() {
    if !kicad_cli_ready() {
        return;
    }
    let (dir, sheet_id, hl) = bus_board("slow_bus_class", "D[0..1]");
    let add = Cmd::AddLabel { net: "D[0..1]".into(), at: hl, kind: eda_model::ir::LabelKind::Hierarchical { shape: eda_model::ir::LabelShape::Bidirectional } };
    step(&dir, Cmd::OnSheet { sheet: sheet_id, cmd: Box::new(add) }, false, "test").unwrap();
    define_high_current(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let bus = design.schematic.as_ref().unwrap().labels.iter().find(|l| l.net == "D[0..1]").unwrap().id.clone();
    step(&dir, edit_label_fields(&bus, vec![netclass("HighCurrent")]), false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let class = model.board.net_classes.iter().find(|c| c.name == "HighCurrent").unwrap();
    for pin in ["R1.1", "R2.1", "R3.1", "R4.1"] {
        let net = design.nets.as_ref().unwrap().iter().find(|n| n.pins.iter().any(|p| p == pin)).unwrap().name.clone();
        assert!(class.nets.contains(&eda_kicad::exact_pattern(&net)), "{pin}'s net {net} is in the bus's class: {:?}", class.nets);
        assert!(kicad_classes_of_pin(&dir, pin).iter().any(|c| c == "HighCurrent"), "kicad-cli puts {pin}'s net in the bus's class");
    }
}

/// A label's net class on a screen shown twice is in force on both placements (`checkNetclassDrivers` runs for each net, and each placement has its own): the
/// second placement's net is in the class too, in the studio's model and in kicad-cli's netlist.
#[test]
fn slow_a_shared_screens_label_class_is_in_force_on_each_placement() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_shared_class");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sheet = design.schematic.as_ref().unwrap().sheets[0].clone();
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [sheet.id] }).to_string().as_bytes());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let paste: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000_000 })).unwrap();
    step(&dir, paste, false, "test").unwrap();
    define_high_current(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let second = design.schematic.as_ref().unwrap().sheets[1].id.clone();
    let screen = design.sheet_contents.as_ref().unwrap()[&sheet.file].clone();
    let (label, wire) = screen.labels.iter().filter(|l| l.kind == eda_model::ir::LabelKind::Local).find_map(|l| screen.wires.iter().find(|w| !w.bus && w.pts.contains(&l.at) && !w.pins.is_empty()).map(|w| (l.clone(), w.clone()))).expect("a local label on a wire with a pin");
    let edit = Cmd::OnSheet { sheet: sheet.id.clone(), cmd: Box::new(edit_label_fields(&label.id, vec![netclass("HighCurrent")])) };
    step(&dir, edit, false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let class = model.board.net_classes.iter().find(|c| c.name == "HighCurrent").unwrap();
    let pin = wire.pins[0].clone();
    let (r, n) = pin.split_once('.').unwrap();
    let sym = screen.symbols.iter().find(|s| s.id == r).unwrap();
    let other = format!("{}.{n}", eda_model::sheet_list::reference_under(&screen, sym, std::slice::from_ref(&second)));
    for p in [&pin, &other] {
        let net = design.nets.as_ref().unwrap().iter().find(|x| x.pins.iter().any(|q| q == p)).unwrap_or_else(|| panic!("{p} on a net")).name.clone();
        assert!(class.nets.contains(&eda_kicad::exact_pattern(&net)), "{p}'s net {net} is in the class: {:?}", class.nets);
        assert!(kicad_classes_of_pin(&dir, p).iter().any(|c| c == "HighCurrent"), "kicad-cli puts {p}'s net in the class");
    }
}

/// The `<text>`s of the SVG pages kicad-cli plots of the board's schematic (`kicad-cli sch export svg`): KiCad writes each text it draws once more as a hidden text.
fn plotted_texts(dir: &Path) -> Vec<String> {
    let done = crate::kicad_engine::export_sch(dir, "svg", &[]).expect("kicad-cli plots the schematic");
    let mut out = Vec::new();
    for f in done["files"].as_array().unwrap() {
        let svg = std::fs::read_to_string(dir.join(f.as_str().unwrap())).unwrap();
        for part in svg.split("<text").skip(1) {
            if let Some(t) = part.split_once('>').and_then(|(_, r)| r.split("</text>").next()) {
                out.push(t.trim().to_string());
            }
        }
    }
    out
}

/// A global label's Sheet References (`SCH_GLOBALLABEL`'s mandatory field, `${INTERSHEET_REFS}`) as Schematic Setup > Formatting sets them (`set_intersheet_refs`,
/// one undo step): kicad-cli's plot shows the pages the label's name is on as the studio shows them, its own page left out when "Show own page reference" is off.
#[test]
fn slow_a_global_labels_sheet_references_read_as_kicad_cli_plots_them() {
    if !kicad_cli_ready() {
        return;
    }
    let (dir, sheet_id, _) = bus_board("slow_isr", "D[0..1]");
    let global = || eda_model::ir::LabelKind::Global { shape: eda_model::ir::LabelShape::Input };
    step(&dir, Cmd::AddLabel { net: "GLOBX".into(), at: Point { x: mm(50.8), y: mm(101.6) }, kind: global() }, false, "test").unwrap();
    step(&dir, Cmd::OnSheet { sheet: sheet_id.clone(), cmd: Box::new(Cmd::AddLabel { net: "GLOBX".into(), at: Point { x: mm(60.96), y: mm(101.6) }, kind: global() }) }, false, "test").unwrap();
    assert!(!plotted_texts(&dir).iter().any(|t| t.starts_with('[')), "not shown by default (`m_IntersheetRefsShow` is off)");
    let shown = |dir: &Path, sheet: &str| -> Vec<String> {
        let (_, d, m) = load(dir).unwrap();
        let j = crate::studio::schematic_json_of(&d, &m, sheet);
        j["labels"].as_array().unwrap().iter().flat_map(|l| l["fields"].as_array().cloned().unwrap_or_default()).filter(|f| f["intersheet_refs"] == true && f["visible"] == true).map(|f| f["shown"].as_str().unwrap().to_string()).collect()
    };
    let mut settings = eda_model::sch_extras::IntersheetRefs { show: true, ..Default::default() };
    step(&dir, Cmd::SetIntersheetRefs { settings: settings.clone() }, false, "test").unwrap();
    let texts = plotted_texts(&dir);
    assert_eq!(texts.iter().filter(|t| *t == "[1,2]").count(), 2, "both labels show both pages: {texts:?}");
    assert_eq!((shown(&dir, ""), shown(&dir, &sheet_id)), (vec!["[1,2]".to_string()], vec!["[1,2]".to_string()]), "as the studio shows them");
    settings.own_page = false;
    step(&dir, Cmd::SetIntersheetRefs { settings }, false, "test").unwrap();
    let texts = plotted_texts(&dir);
    assert!(texts.iter().any(|t| t == "[2]") && texts.iter().any(|t| t == "[1]"), "each the other's page only: {texts:?}");
    assert_eq!((shown(&dir, ""), shown(&dir, &sheet_id)), (vec!["[2]".to_string()], vec!["[1]".to_string()]));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_eq!(shown(&dir, ""), vec!["[1,2]".to_string()], "one undo puts the setting back");
}

/// Delete of the second placement of a screen takes the references it gave the screen's symbols with it (`SCH_SCREENS::PruneOrphanedSymbolInstances`), and a
/// placement pasted later where it was -- so under the same id, which is made from the file and the place -- is numbered afresh, not handed the old ones.
#[test]
fn a_deleted_placement_leaves_no_references_behind() {
    let dir = mcu30_board("sheet_delete_prunes");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sheet = design.schematic.as_ref().unwrap().sheets[0].clone();
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [sheet.id] }).to_string().as_bytes());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let paste: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000_000 })).unwrap();
    step(&dir, paste.clone(), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let second = design.schematic.as_ref().unwrap().sheets[1].id.clone();
    assert!(!design.sheet_contents.as_ref().unwrap()[&sheet.file].instance_overrides.is_empty());

    step(&dir, Cmd::SchEdit(eda_ops::sch_edit::SchCmd::DeleteSheet { id: second.clone() }), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    assert!(design.sheet_contents.as_ref().unwrap()[&sheet.file].instance_overrides.is_empty(), "the placement's references went with it");

    // the sheet pasted where it was again
    step(&dir, paste, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    assert_eq!(design.schematic.as_ref().unwrap().sheets[1].id, second, "the same id again");
    let units = units_per_placement(&design);
    let mut unique = units.clone();
    unique.dedup();
    assert_eq!(units.len(), unique.len(), "{units:?}");
    let screen = &design.sheet_contents.as_ref().unwrap()[&sheet.file];
    assert_eq!(screen.instance_overrides.iter().filter(|o| o.sheet_path == [second.clone()]).count(), screen.symbols.len(), "one reference per symbol, not two");
}

/// A screen shown twice, edited on one of its placements: the references the other placement gives its symbols follow them (`SCH_SYMBOL_INSTANCE` belongs
/// to the symbol, wherever it goes), a symbol added there gets a reference of its own on every placement (KiCad gives it an instance per path and
/// annotates each), and a symbol deleted takes its references with it.
#[test]
fn the_references_of_a_shared_screen_follow_its_edits() {
    let dir = mcu30_board("shared_screen_edits");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sheet = design.schematic.as_ref().unwrap().sheets[0].clone();
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [sheet.id] }).to_string().as_bytes());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let paste: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000_000 })).unwrap();
    step(&dir, paste, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let second = design.schematic.as_ref().unwrap().sheets[1].id.clone();
    let screen = &design.sheet_contents.as_ref().unwrap()[&sheet.file];
    let moved = screen.symbols[0].clone();
    let its_second = eda_model::sheet_list::reference_under(screen, &moved, std::slice::from_ref(&second));
    assert_ne!(its_second, moved.id);
    let unique = |design: &eda_model::ir::Design| {
        let units = units_per_placement(design);
        let mut u = units.clone();
        u.dedup();
        assert_eq!(units.len(), u.len(), "a reference per placement: {units:?}");
    };

    // moved on the first placement
    let mv: Cmd = serde_json::from_value(json!({ "op": "on_sheet", "sheet": sheet.id, "cmd": { "op": "sch_move", "verb": "move", "ids": [moved.id], "dx": 2_540_000, "dy": 0 } })).unwrap();
    step(&dir, mv, false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let screen = &design.sheet_contents.as_ref().unwrap()[&sheet.file];
    let now = screen.symbols.iter().find(|s| s.id == moved.id).unwrap();
    assert_ne!(now.at, moved.at, "the symbol moved");
    assert_eq!(eda_model::sheet_list::reference_under(screen, now, std::slice::from_ref(&second)), its_second, "its reference on the other placement went with it");
    assert!(model.part(&its_second).is_some());
    unique(&design);

    // a symbol added on the first placement: a reference of its own on the second
    let add: Cmd = serde_json::from_value(json!({ "op": "on_sheet", "sheet": sheet.id, "cmd": { "op": "add_symbol", "id": "R90", "lib_id": "Device:R", "at": { "x": 120_000_000, "y": 120_000_000 }, "rot_millideg": 0, "value": "1k", "footprint": "" } })).unwrap();
    step(&dir, add, false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let screen = &design.sheet_contents.as_ref().unwrap()[&sheet.file];
    let added = screen.symbols.iter().find(|s| s.id == "R90").expect("the symbol is on the shared screen");
    let added_second = eda_model::sheet_list::reference_under(screen, added, std::slice::from_ref(&second));
    assert_ne!(added_second, "R90", "a reference of its own on the other placement");
    assert!(model.part(&added_second).is_some(), "{added_second} is a part");
    unique(&design);

    // and deleted: its reference on the other placement goes with it
    let del: Cmd = serde_json::from_value(json!({ "op": "on_sheet", "sheet": sheet.id, "cmd": { "op": "delete_symbol", "id": "R90" } })).unwrap();
    step(&dir, del, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let screen = &design.sheet_contents.as_ref().unwrap()[&sheet.file];
    assert!(screen.instance_overrides.iter().all(|o| o.reference != added_second), "{:?}", screen.instance_overrides);
    assert_eq!(screen.instance_overrides.len(), screen.symbols.len());
    unique(&design);
}

/// Annotate (reset, the entire schematic) on a design that shows a screen twice leaves no reference on two placements.
#[test]
fn annotate_of_a_design_with_a_shared_screen_leaves_no_duplicate() {
    let dir = mcu30_board("shared_screen_annotate");
    write_ldo_file(&dir, "ldo_stage.kicad_sch");
    step(&dir, import_cmd("ldo_stage.kicad_sch", true, false), false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sheet = design.schematic.as_ref().unwrap().sheets[0].clone();
    let copied = crate::sch_clipboard_api::copy(&dir, json!({ "ids": [sheet.id] }).to_string().as_bytes());
    let parsed = crate::sch_clipboard_api::parse(&dir, json!({ "text": copied["text"] }).to_string().as_bytes());
    let paste: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000_000 })).unwrap();
    step(&dir, paste, false, "test").unwrap();
    for reset in [false, true] {
        let annotate: Cmd = serde_json::from_value(json!({ "op": "annotate", "reset_existing": reset, "options": { "scope": "all" } })).unwrap();
        let r = step(&dir, annotate, false, "test");
        assert!(!reset || r.is_ok(), "{r:?}");
        let (_, design, _) = load(&dir).unwrap();
        let units = units_per_placement(&design);
        let mut u = units.clone();
        u.dedup();
        assert_eq!(units.len(), u.len(), "reset {reset}: a reference per placement: {units:?}");
    }
}

/// A KiCad project whose root shows `ldo.kicad_sch` twice, written beside the board as `<file>` (the root) and `ldo.kicad_sch`.
fn write_twice_placed_tree(dir: &Path, file: &str) {
    write_twice_placed_tree_with(dir, file, |_| {});
}

/// [`write_twice_placed_tree`] with the shown screen changed by `edit` first.
fn write_twice_placed_tree_with(dir: &Path, file: &str, edit: impl Fn(&mut eda_model::ir::SchematicSection)) {
    let (_, model) = example("ldo.yaml");
    let mut child = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    edit(child.schematic.as_mut().unwrap());
    let mut design = child.clone();
    let mut root = eda_model::ir::SchematicSection { symbols: vec![], wires: vec![], labels: vec![], power_symbols: vec![], no_connects: vec![], ..child.schematic.clone().unwrap() };
    root.sheets = ["a", "b"].iter().enumerate().map(|(i, n)| eda_model::ir::SheetInstance { id: format!("sh_{n}"), name: format!("LDO {n}"), file: "ldo.kicad_sch".into(), at: Point { x: mm(20.0) + mm(60.0) * i as i64, y: mm(20.0) }, size: (mm(40.0), mm(20.0)), pins: vec![], page: String::new() }).collect();
    design.schematic = Some(root);
    design.sheet_contents = Some([("ldo.kicad_sch".to_string(), child.schematic.unwrap())].into_iter().collect());
    let files = eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-23", title: "twice" }, file).unwrap();
    for (f, text) in files {
        std::fs::write(dir.join(f), text).unwrap();
    }
}

/// Import Sheet of a project that shows one screen twice keeps it one screen (KiCad's `LoadSheetFromFile` reads it once), and the second placement's symbols
/// have references of their own (`SCH_SYMBOL_INSTANCE` per path), with and without Keep: no reference is on two placements, each is a part with nets of its
/// own. (Before: both placements showed the screen's own references, so the board had every LDO part twice under one name.)
#[test]
fn an_imported_tree_that_shows_a_screen_twice_numbers_each_placement() {
    for keep in [false, true] {
        let dir = mcu30_board(&format!("twice_import_{keep}"));
        write_twice_placed_tree(&dir, "twice.kicad_sch");
        step(&dir, import_cmd("twice.kicad_sch", true, keep), false, "test").unwrap();
        let (_, design, model) = load(&dir).unwrap();
        let contents = design.sheet_contents.as_ref().unwrap();
        let screen = contents.iter().find(|(_, s)| s.sheets.is_empty() && !s.symbols.is_empty()).map(|(f, _)| f.clone()).unwrap();
        let placements: Vec<_> = eda_model::sheet_list::walk(&design).into_iter().filter(|e| e.file == screen).collect();
        assert_eq!(placements.len(), 2, "keep {keep}: one screen, two placements");
        let units = units_per_placement(&design);
        let mut unique = units.clone();
        unique.dedup();
        assert_eq!(units.len(), unique.len(), "keep {keep}: a reference per placement: {units:?}");
        let sec = &contents[&screen];
        let nets = design.nets.as_ref().unwrap();
        for s in &sec.symbols {
            let second = eda_model::sheet_list::reference_under(sec, s, &placements[1].path);
            assert_ne!(second, s.id, "keep {keep}");
            assert!(model.part(&second).is_some(), "keep {keep}: {second} is a part");
            assert!(nets.iter().any(|n| n.pins.iter().any(|p| p.starts_with(&format!("{second}.")))), "keep {keep}: {second} is on the nets");
        }
    }
}

/// The same import in kicad-cli: no `duplicate_reference`, and its netlist of the two placements is the studio's net list.
#[test]
fn slow_an_imported_tree_that_shows_a_screen_twice_is_a_net_per_placement_in_kicad_cli() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_twice_import");
    let before: std::collections::BTreeSet<String> = load(&dir).unwrap().2.parts.iter().map(|p| p.reference.clone()).collect();
    write_twice_placed_tree(&dir, "twice.kicad_sch");
    step(&dir, import_cmd("twice.kicad_sch", true, false), false, "test").unwrap();
    assert_no_reference_findings(&dir, "after importing a tree that shows a screen twice");
    let (_, design, _) = load(&dir).unwrap();
    let refs: std::collections::BTreeSet<String> = units_per_placement(&design).into_iter().map(|(r, _)| r).filter(|r| !before.contains(r)).collect();
    let here = |groups: &[Vec<String>]| -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = groups.iter().map(|g| g.iter().filter(|p| p.split_once('.').is_some_and(|(r, _)| refs.contains(r))).cloned().collect::<Vec<_>>()).filter(|g| g.len() >= 2).collect();
        out.sort();
        out
    };
    let (kicad, ours) = (here(&kicad_net_groups(&dir)), here(&our_net_groups(&dir)));
    assert!(!kicad.is_empty());
    assert_eq!(ours, kicad, "a net per placement, as kicad-cli reads the derived files");
}

/// p13-p16 F5: the two placements of a shared screen give one finding each, with one `(check, location)` (the screen has one id per item) and their own
/// marker sheets. Excluding one placement's leaves the other an error -- in kicad-cli, which matches the marker, and in the studio's report -- and deleting
/// that exclusion by its sheet leaves nothing excluded.
#[test]
fn slow_an_erc_exclusion_on_one_placement_leaves_the_other() {
    if !kicad_cli_ready() {
        return;
    }
    let dir = mcu30_board("slow_twice_erc_exclusion");
    // the screen without its wires: its pins are not connected, on each placement
    write_twice_placed_tree_with(&dir, "twice.kicad_sch", |sch| sch.wires.clear());
    step(&dir, import_cmd("twice.kicad_sch", true, false), false, "test").unwrap();
    let report = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let vs = report["violations"].as_array().unwrap().clone();
    let sheet_of = |v: &serde_json::Value| v["marker"]["sheet"].clone();
    // a finding whose (check, location) another finding of another sheet has
    let pair = vs
        .iter()
        .find_map(|a| {
            let b = vs.iter().find(|b| b["check"] == a["check"] && b["location"] == a["location"] && !a["location"].as_str().unwrap_or("").is_empty() && a["marker"].is_object() && b["marker"].is_object() && sheet_of(a) != sheet_of(b))?;
            Some((a.clone(), b.clone()))
        })
        .unwrap_or_else(|| panic!("a finding on both placements: {report:#}"));
    let (a, b) = pair;
    let (check, location) = (a["check"].as_str().unwrap().to_string(), a["location"].as_str().unwrap().to_string());
    let marker: eda_model::ir::ErcMarker = serde_json::from_value(a["marker"].clone()).unwrap();
    step(&dir, Cmd::AddErcExclusion { check: check.clone(), location: location.clone(), comment: "one placement".into(), marker: Some(marker.clone()) }, false, "test").unwrap();
    let after = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let find = |r: &serde_json::Value, of: &serde_json::Value| r["violations"].as_array().unwrap().iter().find(|v| v["check"] == check && v["location"] == location && sheet_of(v) == sheet_of(of)).cloned().unwrap_or_else(|| panic!("{check} {location} on {} in {r:#}", sheet_of(of)));
    let (wa, wb) = (find(&after, &a), find(&after, &b));
    assert_eq!((wa["severity"].as_str(), wa["kicad_matched"].as_bool()), (Some("excluded"), Some(true)), "the one excluded, by kicad-cli too: {wa:#}");
    assert_eq!(wb["severity"], b["severity"], "the other placement's is what it was: {wb:#}");
    assert_eq!(wb["kicad_matched"], false);
    step(&dir, Cmd::DeleteErcExclusion { check: check.clone(), location: location.clone(), sheet: Some(marker.sheet.clone()) }, false, "test").unwrap();
    let again = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    assert_eq!((find(&again, &a)["severity"].clone(), find(&again, &b)["severity"].clone()), (a["severity"].clone(), b["severity"].clone()));
}

// ---------------------------------------------------------------------------------------- KiCad's QA projects: the studio's nets against kicad-cli's

/// kicad-cli's netlist of the schematic `root` (`kicad-cli sch export netlist --format kicadxml`): the pins of each net, `REF.PIN` sorted, power
/// symbols left out, nets of one pin kept.
fn kicad_cli_net_groups(root: &Path) -> Vec<Vec<String>> {
    let out = root.with_extension("net.xml");
    let status = std::process::Command::new(eda_kicad_engine::find_cli().unwrap()).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(root).output().unwrap();
    assert!(out.exists(), "kicad-cli exports {}: {}", root.display(), String::from_utf8_lossy(&status.stderr));
    let text = std::fs::read_to_string(&out).unwrap();
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with("<net ") {
            current = Some(Vec::new());
        } else if line.starts_with("</net>") {
            if let Some(mut g) = current.take() {
                g.sort();
                groups.push(g);
            }
        } else if line.starts_with("<node ") {
            let attr = |name: &str| line.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
            let reference = attr("ref");
            if let Some(g) = current.as_mut().filter(|_| !reference.starts_with('#')) {
                g.push(format!("{reference}.{}", attr("pin")));
            }
        }
    }
    groups.retain(|g| !g.is_empty());
    groups.sort();
    groups
}

/// `design` traced as the studio traces a board (`board::trace_design`, with the design's bus aliases and the library's power inputs).
fn traced_as_the_studio(design: &eda_model::ir::Design, model: &ConstraintModel) -> Vec<eda_engine::nets::TracedNet> {
    crate::board::trace_kicad_names(&mut design.clone(), model)
}

/// p12 S2: an invisible power input of a symbol that is not a power symbol is a global power pin named by its pin (`SCH_PIN::IsGlobalPower`'s legacy
/// rule), so U1's and U2's hidden `VCC` pins are on the net of the power symbol `VCC` -- U2's though a wire takes it to R2.2 (`GetDriverPriority` gives
/// the pin `GLOBAL_POWER_PIN` wired or not, so R2.2 is on `VCC` too). A visible power input (pin 3) is an ordinary pin. kicad-cli's netlist of the
/// file, when it is there, is the same.
#[test]
fn a_hidden_power_input_pin_joins_the_net_of_its_name() {
    let work = scratch("hidden_power_pins");
    let root = work.join("hidden_power_pins.kicad_sch");
    std::fs::write(&root, include_str!("../tests/fixtures/hidden_power_pins.kicad_sch")).unwrap();
    let (design, model, _) = eda_kicad::import_kicad_sch_tree(&root).unwrap();
    let nets = traced_as_the_studio(&design, &model);
    let net = |pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).unwrap_or_else(|| panic!("{pin} in {nets:?}")).clone();
    assert_eq!((net("U1.2").name.as_str(), net("U1.2").pins.clone()), ("VCC", vec!["R1.1".to_string(), "R2.2".into(), "U1.2".into(), "U2.2".into()]), "{nets:?}");
    assert_eq!(net("U1.3").pins, vec!["U1.3".to_string()], "a visible power input is an ordinary pin: {nets:?}");
    // the derived schematic keeps the pin invisible, so kicad-cli reads it as the same global power pin
    let files = eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-24", title: "t" }, "root.kicad_sch").unwrap();
    assert_eq!(files[0].1.matches(") (hide yes)\n\t\t\t\t\t(name \"VCC\"").count(), 1, "pin 2 of T:U, written once: {}", files[0].1);
    if kicad_cli_ready() {
        let mut ours: Vec<Vec<String>> = nets.iter().map(|n| n.pins.iter().filter(|p| !p.starts_with('#')).cloned().collect::<Vec<_>>()).filter(|p| p.len() >= 2).collect();
        ours.sort();
        let kicad: Vec<Vec<String>> = kicad_cli_net_groups(&root).into_iter().filter(|g| g.len() >= 2).collect();
        assert_eq!(ours, kicad, "kicad-cli's netlist of the file");
    }
}

/// Import the reviewer's case `case` (`tests/fixtures/r3_nets/<case>/`, round 3's `/tmp/findings-r3-schematic.jjpH/cases`) and the net of each pin, `#` pins left out.
fn r3_case_nets(case: &str, files: &[(&str, &str)]) -> (PathBuf, std::collections::BTreeMap<String, String>) {
    let work = scratch(&format!("r3_{case}"));
    for (f, text) in files {
        std::fs::write(work.join(f), text).unwrap();
    }
    let dir = work.join("board");
    crate::import_kicad::run(&[work.join("t.kicad_sch").display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let nets = design.nets.unwrap_or_default().into_iter().flat_map(|n| n.pins.into_iter().filter(|p| !p.starts_with('#')).map(move |p| (p, n.name.replace("{slash}", "/")))).collect();
    (dir, nets)
}

/// Round 3's connectivity cases, as kicad-cli names their nets:
/// - A3-H2 (`BindBundle`: a sheet pin names its members on its sheet when it drives its bus, `drivesRecord`): `c5`, one screen behind two sheet symbols
///   with a bus pin `D[0..1]` each, is one net `/A/D0` of R1, R3 and R4; `c5d`, a bus labelled `D[0..1]` elsewhere on the root, is one net `/D0`;
/// - A3-M1 (`canDrive &= .. !reference.StartsWith( "#" )`): `c1`, `#R1` wired to R2.1, is `Net-(R2-Pad1)`;
/// - A3-M2 (`Align`: by place unless both buses are groups): `c4`, a global `{D1 D0}` and a local `D[0..1]` on one bus, joins R1 (D0) and R2 (D1) as `D0`;
/// - A3-M3 (`AssignNames`' `preferred`: a strong claim holds the name): `c7`, a sheet pin `X` on R1.1 and a local label `X` on R2.1, is `/X` for R2.1.
#[test]
fn round_3s_connectivity_cases_are_named_as_kicad_cli_names_them() {
    let fx = |c: &str, f: &str| -> String { std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r3_nets").join(c).join(f)).unwrap() };
    let cases: [(&str, &[&str], &[(&str, &str)]); 5] = [
        ("c5", &["t.kicad_sch", "sub.kicad_sch"], &[("R1.1", "/A/D0"), ("R3.1", "/A/D0"), ("R4.1", "/A/D0")]),
        ("c5d", &["t.kicad_sch", "sub.kicad_sch"], &[("R1.1", "/D0"), ("R3.1", "/D0")]),
        ("c1", &["t.kicad_sch"], &[("R2.1", "Net-(R2-Pad1)")]),
        ("c4", &["t.kicad_sch"], &[("R1.1", "D0"), ("R2.1", "D0")]),
        ("c7", &["t.kicad_sch", "sub.kicad_sch"], &[("R1.1", "/X_1"), ("R2.1", "/X")]),
    ];
    for (case, files, want) in cases {
        let texts: Vec<(&str, String)> = files.iter().map(|f| (*f, fx(case, f))).collect();
        let refs: Vec<(&str, &str)> = texts.iter().map(|(f, t)| (*f, t.as_str())).collect();
        let (dir, nets) = r3_case_nets(case, &refs);
        for (pin, name) in want {
            assert_eq!(nets.get(*pin).map(String::as_str), Some(*name), "{case}: {pin} in {nets:?}");
        }
        if kicad_cli_ready() {
            let kicad = kicad_net_names(&dir);
            for (pin, name) in want {
                assert_eq!(kicad.get(*pin).map(|n| n.replace("{slash}", "/")).as_deref(), Some(*name), "{case}: kicad-cli's name of the derived file for {pin}");
            }
        }
    }
}

/// r3 A3-H1 (`conn_facts.cpp`: a `PIN_FACT` per pin object, a pin common to every unit drawn on each placed unit; `GetRef( &aPath, true )`): a dual part
/// whose pin 3 "C" is common to both units, U1A and U1B both placed, a wire from one unit's copy to R1.1. kicad-cli keeps R1.1 and U1.3 on one net named
/// after the wired copy's unit, `Net-(U1A-C)` or `Net-(U1B-C)`; before, the copy of the unit written last overwrote the other, so a wire on U1A's copy
/// connected nothing. (KiCad's netlist also lists the other copy as `unconnected-(..)`; the net list here holds a pin once.)
#[test]
fn a_common_pin_wired_on_one_unit_only_stays_on_its_wire() {
    for (file, text, name) in [("a", include_str!("../tests/fixtures/common_pin/wired_on_a.kicad_sch"), "Net-(U1A-C)"), ("b", include_str!("../tests/fixtures/common_pin/wired_on_b.kicad_sch"), "Net-(U1B-C)")] {
        let work = scratch(&format!("common_pin_{file}"));
        std::fs::write(work.join("t.kicad_sch"), text).unwrap();
        let dir = work.join("board");
        crate::import_kicad::run(&[work.join("t.kicad_sch").display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
        let (_, design, _) = load(&dir).unwrap();
        let net = design.nets.unwrap_or_default().into_iter().find(|n| n.pins.iter().any(|p| p == "R1.1")).map(|n| (n.name, n.pins));
        assert_eq!(net, Some((name.to_string(), vec!["R1.1".to_string(), "U1.3".into()])), "wired on unit {file}");
        if kicad_cli_ready() {
            assert_eq!(kicad_net_names(&dir).get("R1.1").map(String::as_str), Some(name), "kicad-cli's name of the derived file, wired on unit {file}");
        }
    }
}

/// r3 A3-H3 (`DeriveSignal`, `SignalBaseName`: a net nothing drives is named from the drawing every time): R2.1 wired to R3.1 is `Net-(R2-Pad1)`; a wire that
/// joins R1.1 makes it `Net-(R1-Pad1)`, the name kicad-cli gives the derived file, not the old name.
#[test]
fn an_undriven_net_an_edit_touches_takes_kicads_name() {
    let work = scratch("undriven_rename");
    std::fs::write(work.join("t.kicad_sch"), include_str!("../tests/fixtures/three_resistors.kicad_sch")).unwrap();
    let dir = work.join("board");
    crate::import_kicad::run(&[work.join("t.kicad_sch").display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    let net = |dir: &Path, pin: &str| load(dir).unwrap().1.nets.unwrap_or_default().into_iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| (n.name, n.pins));
    assert_eq!(net(&dir, "R2.1").map(|n| n.0).as_deref(), Some("Net-(R2-Pad1)"));
    let pt = |x: i64, y: i64| Point { x: Nm::new(x), y: Nm::new(y) };
    step(&dir, Cmd::AddWire { pts: vec![pt(50_800_000, 46_990_000), pt(76_200_000, 46_990_000)], bus: false }, false, "test").unwrap();
    assert_eq!(net(&dir, "R2.1"), Some(("Net-(R1-Pad1)".to_string(), vec!["R1.1".to_string(), "R2.1".into(), "R3.1".into()])));
    let (_, design, _) = load(&dir).unwrap();
    assert!(design.schematic.as_ref().unwrap().wires.iter().filter(|w| !w.pins.is_empty()).all(|w| w.net == "Net-(R1-Pad1)"), "every wire carries it");
    if kicad_cli_ready() {
        assert_eq!(kicad_net_names(&dir).get("R2.1").map(String::as_str), Some("Net-(R1-Pad1)"), "kicad-cli's name of the derived file");
    }
}

/// r2 B-M4 (`SCH_SYMBOL_INSTANCE::m_Unit`, `SCH_SYMBOL::GetPins( aSheet )` -> `GetUnitSelection( aSheet )`): a shared channel screen whose dual part is U1
/// unit A on one path and U1 unit B on the other, unit B's pins and box 2.54 mm higher. kicad-cli's netlist of the file puts pins 1 and 2 on `/a/OUT` and
/// `/a/IN`, pins 7 and 6 on `/b/OUT2` and `/b/IN2` (the wires of the other unit's pins are on no pin there); the import keeps
/// the second path's unit, its pins are on that path's nets, and the derived file says `(unit 2)` for that path, so kicad-cli reads it the same.
#[test]
fn a_symbol_drawn_as_another_unit_on_a_second_path_keeps_its_pins_there() {
    let work = scratch("per_path_unit");
    std::fs::write(work.join("root.kicad_sch"), include_str!("../tests/fixtures/per_path_unit/root.kicad_sch")).unwrap();
    std::fs::write(work.join("ch.kicad_sch"), include_str!("../tests/fixtures/per_path_unit/ch.kicad_sch")).unwrap();
    let dir = work.join("board");
    crate::import_kicad::run(&[work.join("root.kicad_sch").display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let mut nets: Vec<(String, Vec<String>)> = design.nets.clone().unwrap_or_default().into_iter().map(|n| (n.name, n.pins)).collect();
    nets.sort();
    let net = |name: &str, pins: &[&str]| (name.to_string(), pins.iter().map(|p| p.to_string()).collect::<Vec<_>>());
    nets.retain(|(_, pins)| !pins.is_empty());
    assert_eq!(nets, vec![net("/a/IN", &["U1.2"]), net("/a/OUT", &["U1.1"]), net("/b/IN2", &["U1.6"]), net("/b/OUT2", &["U1.7"])], "kicad-cli's nets of the file");
    if kicad_cli_ready() {
        let kicad = kicad_net_names(&dir);
        let want: std::collections::BTreeMap<String, String> = [("U1.1", "/a/OUT"), ("U1.2", "/a/IN"), ("U1.6", "/b/IN2"), ("U1.7", "/b/OUT2")].iter().map(|(p, n)| (p.to_string(), n.to_string())).collect();
        assert_eq!(kicad, want, "kicad-cli reads the derived files as it reads the original");
    }
}

/// r2 D-E1's note: QA `issue22938`'s `0IBF_Supply:+48V` and `IBF_supply:+7V` are local power symbols (`(power local)`), on the root and on its sheet:
/// kicad-cli names four nets `/+48V`, `/+7V`, `/Spannungsversorgung/+48V`, `/Spannungsversorgung/+7V` and reports six `power_pin_not_driven`. Ours read them
/// as global power symbols, so the derived file had two nets of them and four findings; now every pin's net is named as kicad-cli names it in the original,
/// and the derived ERC reports six.
#[test]
fn slow_qa_issue22938s_local_power_symbols_are_a_net_per_sheet() {
    let Some(qa) = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from) else { return };
    if !kicad_cli_ready() {
        return;
    }
    let src = qa.join("eeschema/issue22938");
    let work = scratch("issue22938");
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        std::fs::copy(e.path(), work.join(e.file_name())).unwrap();
    }
    let root = work.join("issue22938.kicad_sch");
    let dir = work.join("board");
    crate::import_kicad::run(&[root.display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let ours: std::collections::BTreeMap<String, String> = design.nets.unwrap_or_default().into_iter().flat_map(|n| n.pins.into_iter().filter(|p| !p.starts_with('#')).map(move |p| (p, n.name.clone()))).collect();
    let derived = kicad_net_names(&dir);
    assert!(derived.values().any(|n| n == "/Spannungsversorgung/+48V") && derived.values().any(|n| n == "/+48V"), "{derived:?}");
    assert_eq!(ours, derived, "our nets are kicad-cli's of the derived files");
    assert_eq!(erc_kinds(&dir).get("power_pin_not_driven").copied(), Some(6), "as kicad-cli reports on the original");
}

/// r2 D-E1 (`ERC_SETTINGS`'s `rule_severities` and `pin_map` loaders, `SCHEMATIC_SETTINGS`'s `connection_grid_size`): `eda import-kicad` of a schematic brings
/// its project's ERC severities, pin map and connection grid with it, so the derived project's ERC judges as kicad-cli judges the original (QA `issue22938`
/// ignores `pin_not_connected`; ours reported 50 of them). A library-link check stays at the derived project's ignore.
#[test]
fn a_kicad_projects_erc_settings_come_in_with_the_schematic() {
    let work = scratch("project_erc_settings");
    let root = work.join("p.kicad_sch");
    std::fs::write(&root, include_str!("../tests/fixtures/hidden_power_pins.kicad_sch")).unwrap();
    let mut pin_map = eda_model::ir::ErcPinMap::default_matrix();
    pin_map[1][1] = (pin_map[1][1] + 1) % 3;
    // one bad cell is left at its default; the rest of the map is kept (r3 D3-L3, `ERC_SETTINGS`' loader)
    let mut written = json!(pin_map);
    written[2][3] = json!("x");
    std::fs::write(root.with_extension("kicad_pro"), json!({ "erc": { "rule_severities": { "pin_not_connected": "ignore", "lib_symbol_mismatch": "warning", "pin_to_pin": "warning", "empty_label_name": "ignore", "wired_implicit_power": "error", "similar_labels": "Warning" }, "pin_map": written }, "schematic": { "connection_grid_size": 100.0 } }).to_string()).unwrap();
    let dir = work.join("board");
    crate::import_kicad::run(&[root.display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let severities = eda_kicad::effective_erc_severities(&design);
    assert_eq!(severities.get("pin_not_connected").map(String::as_str), Some("ignore"));
    // two of the checks KiCad 25ec2335 (the box's kicad-cli) added (r3 D3-H1)
    assert_eq!(severities.get("empty_label_name").map(String::as_str), Some("ignore"));
    assert_eq!(severities.get("wired_implicit_power").map(String::as_str), Some("error"));
    // r4 D4-L3 (`SeverityFromString`: anything but "warning" or "ignore" is an error)
    assert_eq!(severities.get("similar_labels").map(String::as_str), Some("error"), "\"Warning\" is not \"warning\"");
    assert_eq!(severities.get("lib_symbol_mismatch").map(String::as_str), Some("ignore"), "the derived project's library-link checks stay ignored");
    assert_eq!(eda_kicad::custom_erc_pin_map(&design).unwrap()[1][1], pin_map[1][1]);
    assert_eq!(design.schematic.as_ref().unwrap().extras.connection_grid_mils, Some(100));
}

/// p13-p16 F6, the ERC half: a KiCad project's own ERC exclusions come in with `eda import-kicad`, named as the derived schematic names its items, and
/// kicad-cli still waives the violation on the studio's run. The record is the one KiCad 10 writes for kicad-cli's own marker of U1's unconnected pin 1 (and
/// kicad-cli reads it as excluded on the original project first).
#[test]
fn slow_a_kicad_projects_erc_exclusions_come_in_with_the_schematic() {
    if !kicad_cli_ready() {
        return;
    }
    let work = scratch("project_erc_exclusions");
    let root = work.join("p.kicad_sch");
    std::fs::write(&root, include_str!("../tests/fixtures/hidden_power_pins.kicad_sch")).unwrap();
    let erc = |root: &Path| -> serde_json::Value {
        let out = root.with_extension("erc.json");
        let _ = std::fs::remove_file(&out);
        std::process::Command::new(eda_kicad_engine::find_cli().unwrap()).args(["sch", "erc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&out).arg(root).output().unwrap();
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap()
    };
    let report = erc(&root);
    let sheet = report["sheets"][0]["uuid_path"].as_str().unwrap().to_string();
    let v = report["sheets"][0]["violations"].as_array().unwrap().iter().find(|v| v["type"] == "pin_not_connected" && v["items"][0]["description"].as_str().unwrap_or("").contains("U1")).cloned().expect("U1's pin 1 is not connected");
    let nm = |mm: &serde_json::Value| ((mm.as_f64().unwrap() * 1e6).round() as i64).to_string();
    let path = json!({ "path": sheet.split('/').filter(|u| !u.is_empty()).map(|u| json!({ "value": u })).collect::<Vec<_>>() });
    let record = json!({ "marker": { "error_type": "ERCET_PIN_NOT_CONNECTED", "position": { "x_nm": nm(&v["items"][0]["pos"]["x"]), "y_nm": nm(&v["items"][0]["pos"]["y"]) }, "items": [{ "value": v["items"][0]["uuid"] }], "sheet_specific_path": path, "main_item_sheet_path": path }, "comment": "left open" });
    std::fs::write(root.with_extension("kicad_pro"), json!({ "meta": { "filename": "p.kicad_pro", "version": 3 }, "erc": { "meta": { "version": 1 }, "erc_exclusions": [record] } }).to_string()).unwrap();
    let again = erc(&root);
    let excluded = |r: &serde_json::Value| -> Vec<String> { r["sheets"].as_array().unwrap().iter().flat_map(|s| s["violations"].as_array().unwrap().iter()).filter(|v| v["excluded"] == true).map(|v| format!("{} {}", v["type"].as_str().unwrap(), v["comment"].as_str().unwrap_or(""))).collect() };
    assert_eq!(excluded(&again), vec!["pin_not_connected left open".to_string()], "the record is KiCad's: kicad-cli waives it on the original");

    let dir = work.join("board");
    crate::import_kicad::run(&[root.display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let waived = &design.schematic.as_ref().unwrap().erc_exclusions;
    assert_eq!(waived.iter().map(|e| (e.check.as_str(), e.location.as_str(), e.comment.as_str())).collect::<Vec<_>>(), [("pin_not_connected", "U1.1", "left open")]);
    let studio = crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let u1 = studio["violations"].as_array().unwrap().iter().find(|v| v["check"] == "pin_not_connected" && v["location"] == "U1.1").cloned().unwrap_or_else(|| panic!("{studio:#}"));
    assert_eq!((u1["severity"].as_str(), u1["comment"].as_str()), (Some("excluded"), Some("left open")), "{u1:#}");
    // the marker is the derived schematic's: its item and its sheet
    let kept = waived[0].marker.clone().unwrap();
    assert_eq!((json!(kept.items), json!(kept.sheet), json!(kept.main_sheet)), (u1["marker"]["items"].clone(), u1["marker"]["sheet"].clone(), u1["marker"]["main_sheet"].clone()), "{u1:#}");
    // and its position the file's, which is the derived one: kicad-cli matches the record on the studio's run
    assert_eq!((json!(kept.at_nm), u1["kicad_matched"].clone()), (u1["marker"]["at_nm"].clone(), json!(true)), "{u1:#}");
}

/// `eda import-kicad` of a KiCad project's schematic (QA `bus_connection`, three sheets and bus aliases) derives a schematic kicad-cli reads as it reads the
/// original: no unconnected pin the original has not, and the same nets. (The screens were kept in KiCad's frame, a symbol at its library origin, which the
/// writer bakes against the corner of its box: every pin was off the file's wires.)
#[test]
fn slow_an_imported_kicad_schematic_derives_one_kicad_cli_reads_as_the_original() {
    if !kicad_cli_ready() || std::env::var_os("KICAD_SRC_DIR").is_none() {
        return;
    }
    let src = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from).unwrap().join("qa/data/eeschema/netlists/bus_connection");
    let work = scratch("import_bus_connection");
    for e in std::fs::read_dir(&src).unwrap().flatten().filter(|e| e.path().is_file()) {
        std::fs::copy(e.path(), work.join(e.file_name())).unwrap();
    }
    let root = work.join("bus_connection.kicad_sch");
    let unconnected = |report: &serde_json::Value| -> std::collections::BTreeSet<String> {
        report["sheets"].as_array().unwrap().iter().flat_map(|s| s["violations"].as_array().unwrap().iter()).filter(|v| v["type"] == "pin_not_connected").filter_map(|v| v["items"][0]["description"].as_str().map(String::from)).collect()
    };
    let out = work.join("erc.json");
    std::process::Command::new(eda_kicad_engine::find_cli().unwrap()).args(["sch", "erc", "--format", "json", "--severity-all", "--units", "mm", "-o"]).arg(&out).arg(&root).output().unwrap();
    let original = unconnected(&serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap());
    let dir = work.join("board");
    crate::import_kicad::run(&[root.display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    crate::kicad_engine::erc(&dir).expect("kicad-cli ran");
    let derived_report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(".kicad").join("erc.json")).unwrap()).unwrap();
    let derived = unconnected(&derived_report);
    assert!(derived.is_subset(&original), "unconnected only in the derived file: {:?}", derived.difference(&original).collect::<Vec<_>>());
    let kicad_nets = |r: &Path| -> Vec<Vec<String>> { kicad_cli_net_groups(r).into_iter().filter(|g| g.len() >= 2).collect() };
    assert_eq!(kicad_nets(&dir.join(".kicad").join("board.kicad_sch")), kicad_nets(&root), "kicad-cli's nets of the derived file are the original's");
}

/// kicad-cli's netlist of the schematic `root` (`kicad-cli sch export netlist --format kicadxml`): the name of each pin's net, by `REF.PIN`, power
/// symbols left out, nets of one pin kept.
fn kicad_cli_net_names(root: &Path) -> std::collections::BTreeMap<String, String> {
    let out = root.with_extension("names.net.xml");
    let status = std::process::Command::new(eda_kicad_engine::find_cli().unwrap()).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(root).output().unwrap();
    assert!(out.exists(), "kicad-cli exports {}: {}", root.display(), String::from_utf8_lossy(&status.stderr));
    let text = std::fs::read_to_string(&out).unwrap();
    let attr = |line: &str, name: &str| line.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap_or("").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
    let mut names = std::collections::BTreeMap::new();
    let mut net = String::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with("<net ") {
            net = attr(line, "name");
        } else if line.starts_with("<node ") && !attr(line, "ref").starts_with('#') && !attr(line, "ref").contains('?') {
            // (an unannotated reference, `R?`, is shared by several symbols: their pins cannot be told apart by `REF.PIN`)
            names.insert(format!("{}.{}", attr(line, "ref"), attr(line, "pin")), net.clone());
        }
    }
    names
}

/// Copy the folder `from` into `to`, its subfolders too (a project may keep its sheets in one, QA `complex_hierarchy_shared`).
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with("._") {
            continue;
        }
        if e.path().is_dir() {
            copy_tree(&e.path(), &to.join(&name));
        } else {
            std::fs::copy(e.path(), to.join(&name)).unwrap();
        }
    }
}

/// The nets of a QA project, as KiCad and the studio name them.
struct QaTraced {
    /// The pins of each net of two pins or more, ours and kicad-cli's.
    ours: Vec<Vec<String>>,
    kicad: Vec<Vec<String>>,
    /// `(pin, our net's name, kicad-cli's)` for every pin of kicad-cli's netlist whose net we name otherwise ("" when we put it on no net).
    names: Vec<(String, String, String)>,
    /// `(pin, the net kicad-cli names it on of the schematic the studio derives, the original's)` where the two differ.
    derived: Vec<(String, String, String)>,
}

/// The QA project `rel` brought in as a user brings it in (`eda import-kicad x.kicad_sch`: `Cmd::ReplaceSchematic`, the screens into the design's frame,
/// the references of each sheet path), its whole folder copied, subfolders too, and traced with the names the studio gives a board's nets
/// (`board::trace_kicad_names`), beside kicad-cli's netlist of KiCad's own file.
fn qa_traced(rel: &str) -> Option<QaTraced> {
    let src = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from)?.join("qa/data/eeschema").join(rel);
    traced_beside_kicad_cli(&src, &format!("qat_{}", rel.replace(['/', '.'], "_")))
}

/// [`qa_traced`] of the project whose root schematic is `src`, in the scratch folder `name`.
fn traced_beside_kicad_cli(src: &Path, name: &str) -> Option<QaTraced> {
    let work = scratch(name);
    let project = work.join("src");
    copy_tree(src.parent().unwrap(), &project);
    let root = project.join(src.file_name().unwrap());
    let kicad_names = kicad_cli_net_names(&root);
    let dir = work.join("board");
    crate::import_kicad::run(&[root.display().to_string(), "-o".into(), dir.display().to_string()]).ok()?;
    let (_, design, model) = load(&dir).ok()?;
    let traced = traced_as_the_studio(&design, &model);
    let mut our_names: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut ours: Vec<Vec<String>> = Vec::new();
    for n in traced {
        let mut pins: Vec<String> = n.pins.into_iter().filter(|p| !p.starts_with('#') && !p.split_once('.').is_some_and(|(r, _)| r.contains('?'))).collect();
        pins.sort();
        for p in &pins {
            our_names.insert(p.clone(), n.name.clone());
        }
        if pins.len() >= 2 {
            ours.push(pins);
        }
    }
    ours.sort();
    let mut groups: std::collections::BTreeMap<&String, Vec<String>> = std::collections::BTreeMap::new();
    for (pin, net) in &kicad_names {
        groups.entry(net).or_default().push(pin.clone());
    }
    let mut kicad: Vec<Vec<String>> = groups.into_values().filter(|g| g.len() >= 2).collect();
    for g in kicad.iter_mut() {
        g.sort();
    }
    kicad.sort();
    // a pin on no traced net is alone: the board names its pad's net as KiCad names a lone pin's (`unconnected_pin_nets`); kicad-cli's netlist writes
    // the names unescaped (`NETLIST_EXPORTER_XML::makeListOfNets`: `UnescapeString`)
    let alone = |pin: &str| -> String {
        let (reference, number) = pin.split_once('.').unwrap_or((pin, ""));
        let shown = model.part(reference).and_then(|p| p.pins.iter().find(|p| p.number == number)).and_then(|p| p.name.clone());
        let with_unit = model.symbol_links.get(reference).and_then(|l| l.units.get(number)).cloned();
        eda_engine::nets::unconnected_net_name(pin, shown.as_deref(), with_unit.as_deref())
    };
    let names = kicad_names.iter().filter_map(|(pin, theirs)| {
        let mine = our_names.get(pin).cloned().unwrap_or_else(|| alone(pin)).replace("{slash}", "/");
        (&mine != theirs).then(|| (pin.clone(), mine, theirs.clone()))
    }).collect();
    // and the schematic the studio derives from it (`.kicad/board.kicad_sch` and its screens, subfolders too), read by kicad-cli
    let derived_names = kicad_net_names(&dir).into_iter().filter(|(p, _)| !p.split_once('.').is_some_and(|(r, _)| r.contains('?'))).collect::<std::collections::BTreeMap<_, _>>();
    let derived = kicad_names.iter().filter_map(|(pin, theirs)| {
        let d = derived_names.get(pin).cloned().unwrap_or_default();
        (&d != theirs).then(|| (pin.clone(), d, theirs.clone()))
    }).collect();
    Some(QaTraced { ours, kicad, names, derived })
}

/// KiCad's QA projects that show a screen twice, carry buses across sheets or give labels fields of their own, brought in by `eda import-kicad` and traced as
/// the studio traces a board: the nets are kicad-cli's netlist of KiCad's own files, pin for pin and **name for name**. (`KICAD_SRC_DIR` must name KiCad's
/// source.) Until round 2 this compared pin groups only, of a design read by `import_kicad_sch_tree` (no `Cmd::ReplaceSchematic`), with the root folder's files
/// alone: through the real import path 11 of the 24 traced as kicad-cli reads them, and 10 named their nets as it does.
const QA_TRACED_ALIKE: &[&str] = &[
    "issue13212.kicad_sch",
    "issue23840/BusAndVectors.kicad_sch",
    "netlists/test_global_promotion_2/test_global_promotion_2.kicad_sch",
    "netlists/test_hier_renaming/test_hier_renaming.kicad_sch",
    "netlists/prefix_bus_alias/prefix_bus_alias.kicad_sch",
    "netlists/weak_vector_bus_disambiguation/weak_vector_bus_disambiguation.kicad_sch",
    "netlists/group_bus_matching/group_bus_matching.kicad_sch",
    "netlists/issue16439/issue16439.kicad_sch",
    "issue18119/issue18119.kicad_sch",
    "issue18092/issue18092.kicad_sch",
    "issue13112.kicad_sch",
    "issue12814.kicad_sch",
    "ERC_dynamic_power_symbol_test.kicad_sch",
    "netlists/bus_connection/bus_connection.kicad_sch",
    "issue17771/issue1771.kicad_sch",
    // a hidden power input that is not on the edge its angle names (`4C4001`'s pins 10 and 20): it used to join GND and +5V
    "netlists/video/video.kicad_sch",
    // a label's `${#}` in a math expression, per placement (r2 A-N10)
    "issue23962/issue23962.kicad_sch",
    "issue24044/issue24044.kicad_sch",
    // its sheets in a subfolder (r2 B-H2), its symbols unannotated (`C?`, left out of the comparison)
    "netlists/complex_hierarchy_shared/complex_hierarchy.kicad_sch",
    // a pin name another unit repeats (r2 A-N6)
    "netlists/complex_hierarchy/complex_hierarchy.kicad_sch",
    "legacy_hierarchy/legacy_hierarchy.kicad_sch",
    "issue19646/issue19646.kicad_sch",
    // a screen shown twice inside a screen shown twice: a reference per whole sheet path
    "issue10926_1.kicad_sch",
    "netlists/hierarchy_aliases/hierarchy_aliases.kicad_sch",
];

/// The ones the studio still reads otherwise, each with why (PARITY-sch.md section 38, "Review"): printed, not asserted.
const QA_TRACED_OTHERWISE: &[(&str, &str)] = &[
];

#[test]
fn slow_kicad_qa_hierarchies_trace_as_kicad_cli_reads_them() {
    if !kicad_cli_ready() || std::env::var_os("KICAD_SRC_DIR").is_none() {
        return;
    }
    let only = |a: &[Vec<String>], b: &[Vec<String>]| -> Vec<Vec<String>> { a.iter().filter(|x| !b.contains(x)).cloned().collect() };
    let mut wrong: Vec<String> = Vec::new();
    for rel in QA_TRACED_ALIKE {
        let t = qa_traced(rel).unwrap_or_else(|| panic!("{rel} reads"));
        let (groups, names) = (only(&t.kicad, &t.ours).len(), t.names.len());
        println!("{rel}: {groups} of {} nets of 2+ pins differ, {names} of the pins' net names differ, {} in the derived schematic", t.kicad.len(), t.derived.len());
        if !t.derived.is_empty() {
            println!("   derived: {:?}", t.derived.iter().take(8).collect::<Vec<_>>());
        }
        if t.kicad.is_empty() && !t.ours.is_empty() {
            wrong.push(format!("{rel}: kicad-cli has no nets"));
        }
        if t.ours != t.kicad {
            wrong.push(format!("{rel}: the studio's nets are kicad-cli's; ours only {:?}, kicad-cli's only {:?}", only(&t.ours, &t.kicad), only(&t.kicad, &t.ours)));
        }
        if !t.names.is_empty() {
            wrong.push(format!("{rel}: (pin, ours, kicad-cli's) {:?}", t.names.iter().take(12).collect::<Vec<_>>()));
        }
        if !t.derived.is_empty() {
            wrong.push(format!("{rel}: kicad-cli reads the derived schematic otherwise: (pin, derived, original) {:?}", t.derived.iter().take(12).collect::<Vec<_>>()));
        }
    }
    for (rel, why) in QA_TRACED_OTHERWISE {
        let Some(t) = qa_traced(rel) else { continue };
        println!("{rel} ({why}): {} nets differ of {}, {} names differ", only(&t.kicad, &t.ours).len(), t.kicad.len(), t.names.len());
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A board whose design stores no schematic yet (a board made by `eda board new`) is checked with the schematic the engine draws for it, and keeps its own
/// board: the parity DRC (`--schematic-parity`) needs both, the ERC its waivers in `drawings`. It used to be replaced by the bare derived design, so the
/// DRC dialog's "Test for parity between PCB and schematic" said "design has no placement section" (e2e/drc-review.check.js).
#[test]
fn a_board_without_a_stored_schematic_is_loaded_with_the_derived_one_and_keeps_its_board() {
    let dir = scratch("load_with_schematic");
    let (intent, model) = example("mcu_board_30plus.yaml");
    let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.schematic = None;
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(35.0), y: Nm::ZERO }, Point { x: mm(35.0), y: mm(35.0) }, Point { x: Nm::ZERO, y: mm(35.0) }], footprints: vec![], modules: vec![] });
    design.drawings = Some(eda_model::ir::DrawingsSection::default());
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    let (d, _) = crate::kicad_engine::load_with_schematic(&dir).unwrap();
    assert!(d.schematic.as_ref().is_some_and(|s| !s.sheets.is_empty() || !s.symbols.is_empty()), "the derived schematic");
    assert_eq!(d.placement.as_ref().map(|p| p.outline.len()), Some(4), "the board's placement is kept");
    assert!(d.drawings.is_some(), "and its drawings");
    // the module sheets' screens come with the derived root, as they do for a board whose design is the derived one
    let alone = crate::board::derived_schematic(&design, &crate::board::load(&dir).unwrap().2).unwrap();
    assert_eq!(d.sheet_contents.as_ref().map(|c| c.len()), alone.sheet_contents.as_ref().map(|c| c.len()), "the screens of the module sheets");
    assert!(d.sheet_contents.as_ref().is_some_and(|c| !c.is_empty()), "mcu30 has module sheets");
}

// ------------------------------------------------------------------- KiCad's QA projects: read, written back, judged by kicad-cli as the original

/// kicad-cli's ERC of the schematic `root` (`kicad-cli sch erc --format json`): each violation's type with the count, over every sheet.
fn kicad_cli_erc_counts(root: &Path) -> Option<std::collections::BTreeMap<String, usize>> {
    let out = root.with_extension("erc.json");
    let _ = std::fs::remove_file(&out);
    std::process::Command::new(eda_kicad_engine::find_cli().unwrap()).args(["sch", "erc", "--format", "json", "--severity-all", "-o"]).arg(&out).arg(root).output().ok()?;
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).ok()?).ok()?;
    let mut counts = std::collections::BTreeMap::new();
    for sheet in report["sheets"].as_array().into_iter().flatten() {
        for v in sheet["violations"].as_array().into_iter().flatten() {
            *counts.entry(v["type"].as_str().unwrap_or("?").to_string()).or_insert(0) += 1;
        }
    }
    Some(counts)
}

/// The roots of KiCad's QA schematics: every `.kicad_sch` under `qa/data/eeschema` that no file of its folder places as a sheet.
fn qa_schematic_roots() -> Vec<PathBuf> {
    let Some(src) = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from) else { return Vec::new() };
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut files: Vec<PathBuf> = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "kicad_sch") && !p.file_name().unwrap().to_string_lossy().starts_with("._") {
                files.push(p);
            }
        }
        let placed: std::collections::BTreeSet<String> = files.iter().flat_map(|f| eda_kicad::referenced_sheet_files(&std::fs::read_to_string(f).unwrap_or_default())).map(|f| f.rsplit('/').next().unwrap_or(&f).to_lowercase()).collect();
        out.extend(files.into_iter().filter(|f| !placed.contains(&f.file_name().unwrap().to_string_lossy().to_lowercase())));
    }
    let mut out = Vec::new();
    walk(&src.join("qa/data/eeschema"), &mut out);
    out.sort();
    out
}

/// A board with nothing on it: no parts, an empty root sheet.
fn empty_board(name: &str) -> PathBuf {
    let dir = scratch(name);
    let model = ConstraintModel::default();
    let intent = dir.join("empty.yaml");
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
    dir
}

/// Every QA root brought into an empty board the way the studio brings a KiCad schematic in (Import Sheet, its contents on the root, the
/// annotations kept), and the files the studio derives from it judged by kicad-cli against the original: its ERC (violation types with
/// their counts; both at the default settings, the original read without its project file) and its netlist (the pins of each net).
/// Printed, one line per project that differs. `EDA_QA_SCH_ONLY=<part of a path>` runs the ones that match.
#[test]
fn slow_kicad_qa_schematics_written_back_are_judged_as_the_originals() {
    if !kicad_cli_ready() || std::env::var_os("KICAD_SRC_DIR").is_none() {
        return;
    }
    let only = std::env::var("EDA_QA_SCH_ONLY").ok();
    let (mut same, mut differ, mut failed) = (0, 0, 0);
    for src in qa_schematic_roots() {
        let rel = src.to_string_lossy().split("qa/data/eeschema/").nth(1).unwrap_or("").to_string();
        if only.as_ref().is_some_and(|o| !rel.contains(o.as_str())) {
            continue;
        }
        let tag = rel.replace(['/', '.'], "_");
        let orig = scratch(&format!("qarw_o_{tag}"));
        let dir = empty_board(&format!("qarw_w_{tag}"));
        for e in std::fs::read_dir(src.parent().unwrap()).unwrap().flatten().filter(|e| e.path().is_file()) {
            if e.path().extension().is_some_and(|x| x == "kicad_pro" || x == "kicad_prl") {
                continue;
            }
            std::fs::copy(e.path(), orig.join(e.file_name())).unwrap();
            std::fs::copy(e.path(), dir.join(e.file_name())).unwrap();
        }
        let root = orig.join(src.file_name().unwrap());
        let Some(erc_o) = kicad_cli_erc_counts(&root) else {
            println!("SKIP {rel}: kicad-cli has no ERC of the original");
            continue;
        };
        let groups_o = kicad_cli_net_groups_opt(&root);
        let file = src.file_name().unwrap().to_string_lossy().to_string();
        // the click on a point of the 50 mil grid, as the tool snaps it
        let mut cmd = import_cmd(&file, std::env::var_os("EDA_QA_SCH_AS_SHEET").is_some(), true);
        if let Cmd::ImportSheet { at, .. } = &mut cmd {
            *at = Point { x: mm(254.0), y: mm(50.8) };
        }
        if let Err(e) = step(&dir, cmd, false, "test") {
            println!("FAIL {rel}: Import Sheet refuses it: {e:?}");
            failed += 1;
            continue;
        }
        let Ok(erc) = crate::kicad_engine::erc(&dir) else {
            println!("FAIL {rel}: kicad-cli has no ERC of ours");
            failed += 1;
            continue;
        };
        let mut erc_w = std::collections::BTreeMap::new();
        for v in erc["violations"].as_array().into_iter().flatten() {
            *erc_w.entry(v["check"].as_str().or_else(|| v["type"].as_str()).unwrap_or("?").to_string()).or_insert(0) += 1;
        }
        let groups_w = std::panic::catch_unwind(|| kicad_net_groups(&dir)).unwrap_or_default();
        // (`lib_symbol_issues` says the original's libraries are not in this machine's tables; ours are the project's own. `footprint_link_issues` says
        // the same of the footprints' libraries, a check the studio's project leaves to its own library check, PARITY-pcb.md section 53)
        let erc_diff: Vec<String> = erc_o.keys().chain(erc_w.keys()).collect::<std::collections::BTreeSet<_>>().into_iter().filter(|k| *k != "lib_symbol_issues" && *k != "footprint_link_issues" && erc_o.get(*k) != erc_w.get(*k)).map(|k| format!("{k} {}->{}", erc_o.get(k).unwrap_or(&0), erc_w.get(k).unwrap_or(&0))).collect();
        let nets_only_o = groups_o.iter().filter(|g| !groups_w.contains(g)).count();
        let nets_only_w = groups_w.iter().filter(|g| !groups_o.contains(g)).count();
        if erc_diff.is_empty() && nets_only_o == 0 && nets_only_w == 0 {
            same += 1;
        } else {
            differ += 1;
            println!("DIFF {rel}: erc [{}]; nets {} of {} only in the original, {} only in ours", erc_diff.join(", "), nets_only_o, groups_o.len(), nets_only_w);
        }
    }
    println!("QA schematics written back: {same} judged alike, {differ} differ, {failed} failed");
    // the ratchet: 100 of KiCad's 130 QA roots on 2026-10-28 (r4: `lib_name` resolution and the pin facts); 87 on 2026-10-23 (nightly 20261021); before
    // the review of that day, 53 (turned and mirrored symbols were traced and written otherwise than drawn, and one file hung Import Sheet)
    if only.is_none() {
        assert!(same >= 100, "{same} judged alike, fewer than the 100 of 2026-10-28");
    }
}

/// [`kicad_cli_net_groups`] that answers no nets when kicad-cli writes no netlist.
fn kicad_cli_net_groups_opt(root: &Path) -> Vec<Vec<String>> {
    std::panic::catch_unwind(|| kicad_cli_net_groups(root)).unwrap_or_default()
}

/// The QA project `rel` brought in with `eda import-kicad`, its folder copied whole: the board folder, or `None` without `KICAD_SRC_DIR`.
fn qa_imported(rel: &str, name: &str) -> Option<PathBuf> {
    let src = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from)?.join("qa/data/eeschema").join(rel);
    let work = scratch(name);
    copy_tree(src.parent().unwrap(), &work.join("src"));
    let dir = work.join("board");
    crate::import_kicad::run(&[work.join("src").join(src.file_name().unwrap()).display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    Some(dir)
}

/// The placement ids from the root down to the first sheet (depth first) whose screen is `file`.
fn path_to_file(design: &eda_model::ir::Design, file: &str) -> Vec<String> {
    eda_model::sheet_list::walk(design).into_iter().find(|e| e.file == file).unwrap_or_else(|| panic!("a sheet of {file}")).path
}

/// r3 B3-M1: a sheet's `Sheetfile` is its `SHEET_FILENAME` field's text (`SCH_SHEET::GetFileName`), as the file that places it says it: the Filter sheet of
/// QA `complex_hierarchy_shared`, placed by `ampli_ht/ampli_ht.kicad_sch`, says `filter.kicad_sch`, not its key `ampli_ht/filter.kicad_sch`; and
/// `${SHEETFILE}` (`SCH_SHEET::ResolveTextVar`) on its screen shows the same.
#[test]
fn a_sheets_file_shows_as_the_placing_file_names_it() {
    let Some(dir) = qa_imported("netlists/complex_hierarchy_shared/complex_hierarchy.kicad_sch", "sheetfile_text") else { return };
    let (meta, design, model) = load(&dir).unwrap();
    let filter = path_to_file(&design, "ampli_ht/filter.kicad_sch");
    let parent = filter[..filter.len() - 1].join("/");
    let json = crate::studio::schematic_json_of(&design, &model, &parent);
    let sheet = json["sheets"].as_array().unwrap().iter().find(|s| s["id"] == filter.last().unwrap().as_str()).unwrap();
    let field = sheet["fields"].as_array().unwrap().iter().find(|f| f["name"] == "Sheetfile").unwrap();
    assert_eq!(field["text"], "filter.kicad_sch", "{sheet}");
    // the page of the sheet in view, which a placed symbol's "sheet number x 100" counts from (r3 D3-M3): the file's own page of the ampli_ht sheet
    let entry = eda_model::sheet_list::walk(&design).into_iter().find(|e| e.path_text() == parent).unwrap();
    assert!(entry.page_number_as_int() > 1);
    assert_eq!(json["page_number"], entry.page_number_as_int());
    let (_, breadcrumb) = crate::studio::resolve_sheet(&design, &filter.join("/"));
    let project = crate::text_vars_json::project(&dir, &meta.intent, eda_model::text_vars::Clock::utc_now());
    let shown = crate::text_vars_json::SchShown::new(&dir, &design, &model, &breadcrumb, project);
    assert_eq!(shown.text_shown("${SHEETFILE}"), "filter.kicad_sch");
}

/// r3 B3-M3: the studio's shown text resolves a sheet's user fields as the tracer does (one `SCH_SHEET::ResolveTextVar`): `${param}` on a sub-sheet of QA
/// `ERC_dynamic_power_symbol_test` shows that sheet's `param` field.
#[test]
fn the_shown_text_resolves_a_sheets_user_fields() {
    let Some(dir) = qa_imported("ERC_dynamic_power_symbol_test.kicad_sch", "sheet_user_fields_shown") else { return };
    let (meta, design, model) = load(&dir).unwrap();
    let root = design.schematic.as_ref().unwrap();
    let sub = root.sheets.iter().find(|s| s.name == "ERC_dynamic_power_symbol_subsheet1").expect("the sub-sheet");
    let (_, breadcrumb) = crate::studio::resolve_sheet(&design, &sub.id);
    let project = crate::text_vars_json::project(&dir, &meta.intent, eda_model::text_vars::Clock::utc_now());
    let shown = crate::text_vars_json::SchShown::new(&dir, &design, &model, &breadcrumb, project);
    assert_eq!(shown.text_shown("${param}"), "param_subsheet_1");
}

// ---------------------------------------------------------------------------------------- round 4: the pin facts of KiCad 25ec2335's connectivity

/// Import the round-4 case `case` (`tests/fixtures/r4_connectivity/<case>`, the reviewer's generated files) into a scratch board: the board folder, the
/// copied root, and the board's nets as `name -> pins`.
fn r4_case(case: &str) -> (PathBuf, PathBuf, std::collections::BTreeMap<String, Vec<String>>) {
    round_case("r4_connectivity", case)
}

/// [`r4_case`] of the fixtures folder `round` (`r4_connectivity`, `r5_connectivity`).
fn round_case(round: &str, case: &str) -> (PathBuf, PathBuf, std::collections::BTreeMap<String, Vec<String>>) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(round).join(case);
    let work = scratch(&format!("{round}_{case}"));
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        std::fs::copy(e.path(), work.join(e.file_name())).unwrap();
    }
    let root = work.join("t.kicad_sch");
    let dir = work.join("board");
    crate::import_kicad::run(&[root.display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let nets = design.nets.unwrap_or_default().into_iter().map(|n| (n.name, n.pins)).collect();
    (dir, root, nets)
}

/// r4 A4-H2, A4-H3, A4-H4, A4-M1, A4-M2 (KiCad 25ec2335 `conn_facts.cpp`, `conn_islands.cpp::BuildScreenIslands`): a symbol's jumper pin group is one net
/// (`j1`), and so is every pin of one number when duplicate pin numbers are jumpers (`j2`); two pins `1` at two places without the flag are two nodes
/// (`j3`; KiCad lists `J1.1` on both nets, the board's net list holds a pin once); a stacked number `[1-2]` is each of its pads, named by the smallest
/// (`s1`); a no-connect pin joins nothing (`n1`); a bus wire entry between two wires joins them (`g6`). The nets `eda import-kicad` stores are kicad-cli's
/// of the original, and kicad-cli reads the derived file as it reads the original.
#[test]
fn a_symbols_jumper_pins_are_one_net_as_kicad_cli_reads_them() {
    let cases: &[(&str, &[(&str, &[&str])])] = &[
        ("j1", &[("Net-(J1-A)", &["J1.1", "J1.2", "R1.1", "R2.1"])]),
        ("j2", &[("Net-(J1-A)", &["J1.1", "R1.1", "R2.1"])]),
        ("j3", &[("Net-(J1-A)", &["J1.1", "R1.1"]), ("Net-(J1-A)_1", &["R2.1"])]),
        ("s1", &[("Net-(J1-Pad1)", &["J1.1", "J1.2", "R1.1"]), ("unconnected-(J1-Pad3)", &["J1.3"])]),
        ("n1", &[("unconnected-(R1-Pad1)", &["R1.1"]), ("unconnected-(U1-NC-Pad1)", &["U1.1"])]),
        ("g6", &[("Net-(R1-Pad1)", &["R1.1", "R3.1"])]),
    ];
    for (case, want) in cases {
        let (dir, root, nets) = r4_case(case);
        for (name, pins) in *want {
            assert_eq!(nets.get(*name).map(|p| p.iter().map(String::as_str).collect::<Vec<_>>()), Some(pins.to_vec()), "{case}: {name} in {nets:?}");
        }
        if kicad_cli_ready() {
            let derived = kicad_net_groups(&dir);
            assert_eq!(derived, kicad_cli_net_groups(&root), "{case}: kicad-cli reads the derived file as the original");
        }
    }
}

/// Round 5's connectivity cases (`tests/fixtures/r5_connectivity`, the reviewer's files), with the nets kicad-cli gives the original (`#` pins left out):
/// - D5-H3 (`SCH_PIN::GetType`: a pin is of the type of the alternate its symbol picked; `conn_facts.cpp`: `nc = pin->GetType() == PT_NC`): QA
///   `issue22286/bugtest` (`a22286`), a connector whose library pins are all `no_connect` with alternates of other types, picked on J1;
/// - A5-H1 (`conn_facts.cpp`: an item per pin of each placed `SCH_SYMBOL`): `j5`, pin `1` in unit A and in unit B, both placed and wired apart;
/// - A5-M1 (`PROPAGATES[ENTRY][ENTRY]` is false): `e1`, two bus wire entries end to end with a wire on each far end, join nothing;
/// - A5-M2 (one `LIB_SYMBOL` for every body style: `JumperPinGroups()` whatever the style): `dm1`, a jumper group on a symbol placed in body style 2.
///
/// The derived file reads in kicad-cli as the original.
#[test]
fn round_5s_connectivity_cases_are_traced_as_kicad_cli_traces_them() {
    let cases: &[(&str, &[(&str, &[&str])])] = &[
        ("a22286", &[("GND", &["J1.1", "J1.8"]), ("+5V", &["J1.2"]), ("Net-(J1-Pad3)", &["J1.3", "R1.1"]), ("Net-(J1-Pad7)", &["J1.7", "R1.2"])]),
        ("j5", &[("Net-(J1A-A)", &["J1.1", "R1.1"]), ("Net-(J1B-B)", &["R2.1"])]),
        ("e1", &[("unconnected-(R1-Pad1)", &["R1.1"]), ("unconnected-(R3-Pad1)", &["R3.1"])]),
        ("dm1", &[("Net-(J1-A)", &["J1.1", "J1.2", "R1.1", "R2.1"])]),
    ];
    for (case, want) in cases {
        let (dir, root, nets) = round_case("r5_connectivity", case);
        for (name, pins) in *want {
            let got = nets.get(*name).map(|p| p.iter().map(String::as_str).filter(|p| !p.starts_with('#')).collect::<Vec<_>>());
            assert_eq!(got, Some(pins.to_vec()), "{case}: {name} in {nets:?}");
        }
        if kicad_cli_ready() {
            assert_eq!(kicad_net_groups(&dir), kicad_cli_net_groups(&root), "{case}: kicad-cli reads the derived file as the original");
        }
    }
    // the derived file keeps the alternates J1 picked
    let (dir, _, _) = round_case("r5_connectivity", "a22286");
    let (_, design, model) = load(&dir).unwrap();
    let files = eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-28", title: "t" }, "root.kicad_sch").unwrap();
    let derived = &files[0].1;
    assert!(derived.contains("(alternate \"3.out\")") && derived.contains("(alternate \"3.out\" output line)"), "the derived file names the alternate and defines it");
}

/// r4 A4-H1 (`SCH_PIN::IsLocalPower`: the placed symbol's library symbol decides): one library id cached `(power local)` on the root and `(power)` on the
/// sheet (`p7`) keeps each screen's kind, as kicad-cli reads the original; the design's one library symbol of the id is the root's local one.
#[test]
fn a_screens_own_power_kind_is_kept_where_it_differs_from_the_library() {
    let (dir, root, nets) = r4_case("p7");
    assert_eq!(nets.get("+48V").map(Vec::len), Some(3), "{nets:?}");
    let (_, design, _) = load(&dir).unwrap();
    let sheet = design.sheet_contents.as_ref().unwrap().values().next().unwrap();
    assert!(sheet.extras.global_power.contains("lp:+48V") && design.schematic.as_ref().unwrap().extras.local_power.is_empty(), "only the sheet that differs keeps a kind of its own");
    if kicad_cli_ready() {
        assert_eq!(kicad_net_groups(&dir), kicad_cli_net_groups(&root));
    }
}

/// r4 A4-H1: a local power symbol placed in the editor (`P`) is local because its library symbol is (`SCH_PIN::IsLocalPower`), not because an import
/// listed it: a second `lp:+48V` put on the sheet's R3 pin 2 is on the sheet's `/A/+48V` with R3 pin 1, not a global `+48V`, and the sheet's file has
/// it `(power local)`.
#[test]
fn a_placed_local_power_symbol_is_a_net_of_its_sheet() {
    let (dir, _root, _) = r4_case("p1");
    let (_, design, _) = load(&dir).unwrap();
    assert!(design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())).all(|s| s.extras.local_power.is_empty()), "the library decides, no screen lists it");
    let sheet_id = design.schematic.as_ref().unwrap().sheets[0].id.clone();
    let add = Cmd::AddPowerSymbol { lib_id: "lp:+48V".into(), at: Point { x: mm(50.8), y: mm(54.61) }, rot_millideg: 180_000, net: "+48V".into(), pin: String::new(), annotate: None };
    step(&dir, Cmd::OnSheet { sheet: sheet_id, cmd: Box::new(add) }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let net_of = |pin: &str| design.nets.iter().flatten().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone());
    assert_eq!(net_of("R3.2").as_deref(), Some("/A/+48V"), "{:?}", design.nets);
    assert_eq!(net_of("R3.1").as_deref(), Some("/A/+48V"));
    assert_eq!(net_of("R1.1").as_deref(), Some("/+48V"));
    eda_kicad::export_kicad_sch_tree(&design, &load(&dir).unwrap().2, &eda_kicad::ExportMeta { date: "2026-10-28", title: "t" }, "board.kicad_sch")
        .unwrap()
        .iter()
        .filter(|(_, text)| text.contains("lp:+48V"))
        .for_each(|(file, text)| assert!(text.contains("(power local)") && !text.contains("\t\t\t(power)\n"), "{file}"));
}

/// r4 C4-M2 (`SCH_EDITOR_CONTROL::HighlightNet`: `conn->Name()`, a label's own connection): a local label put on a pin end with no wire gives the studio
/// the net it is on (`/X`), not its text, so Highlight Net on it lights the net its pins are on.
#[test]
fn a_label_on_a_pin_with_no_wire_has_the_net_of_the_pin() {
    let (dir, _, nets) = r4_case("lbl");
    assert_eq!(nets.get("/X").map(Vec::len), Some(2), "{nets:?}");
    let sheet = crate::studio::schematic_json(&dir, "", eda_model::text_vars::Clock::utc_now()).unwrap();
    let labels = sheet["labels"].as_array().unwrap();
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|l| l["net_name"] == "/X"), "{labels:?}");
}
