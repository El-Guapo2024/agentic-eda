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
    let meta = Meta { intent: intent.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
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
        at: Point { x: 250_000_000, y: 40_000_000 },
        size: (40_000_000, 20_000_000),
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
        out.extend(sec.symbols.iter().map(|s| (eda_model::sheet_list::reference_under(sec, s, e.path.last().map(String::as_str)), s.unit)));
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
    assert_eq!((sheet.file.as_str(), sheet.at, sheet.size), ("ldo_stage.kicad_sch", Point { x: 250_000_000, y: 40_000_000 }, (40_000_000, 20_000_000)));
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
    let second: Vec<&eda_model::ir::SymbolPathOverride> = screen.instance_overrides.iter().filter(|o| o.parent_sheet_instance_id == sheets[1].id).collect();
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
    let edit: Cmd = serde_json::from_value(json!({ "op": "sch_edit", "verb": "edit_field", "id": "fld:R3:Reference", "at": { "x": r3.x - 12_700_000, "y": r3.y + 1_270_000 }, "h": "right", "size_nm": 1_778_000, "bold": true })).unwrap();
    step(&dir, edit, false, "test").unwrap();
    step(&dir, Cmd::ReorganizeSheets, false, "test").unwrap();
    let (_, sheets, _) = load(&dir).unwrap();
    let kept = sheets.sheet_contents.iter().flatten().flat_map(|(_, s)| s.symbols.iter().map(move |x| (s, x))).find(|(_, x)| x.id == "R3").unwrap();
    let place = kept.0.field_layout.get("R3").and_then(|f| f.iter().find(|p| p.name == "Reference")).expect("R3's Reference keeps a place of its own");
    assert_eq!((place.dx, place.dy), (-12_700_000, 1_270_000), "the field is where the user put it from its symbol");
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
    };
    let sym = LibSymbol {
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
        for (unit, x) in [(1, 300_000_000), (2, 330_000_000), (3, 360_000_000)] {
            root.symbols.push(eda_model::ir::SymbolInstance {
                id: "U9".into(),
                at: Point { x, y: 20_000_000 },
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
    let meta = Meta { intent: intent.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
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
    let second_refs: Vec<String> = screen.instance_overrides.iter().filter(|o| o.parent_sheet_instance_id == second.id).map(|o| o.reference.clone()).collect();
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
    assert!(back.sheet_contents.as_ref().unwrap()[&sheet.file].instance_overrides.iter().all(|o| o.parent_sheet_instance_id != second.id));
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
            let end = Point { x: at.x - 5_080_000, y: at.y };
            sec.wires.push(wire(vec![at, end], false));
            sec.labels.push(label(&net, end, LabelKind::Local));
        }
    }
    // the child: a bus from the hierarchical label down
    let hl = Point { x: 20_320_000, y: 20_320_000 };
    child.wires.push(wire(vec![hl, Point { x: 20_320_000, y: 40_640_000 }], true));
    // the root: the sheet, its bus pin, and a bus label on a stub of bus away from it
    let sheet_at = Point { x: 152_400_000, y: 25_400_000 };
    let pin_at = Point { x: sheet_at.x, y: sheet_at.y + 5_080_000 };
    root.sheets.push(SheetInstance { id: String::new(), name: "child".into(), file: "child.kicad_sch".into(), at: sheet_at, size: (25_400_000, 20_320_000), pins: vec![SheetPin { id: String::new(), name: "D[0..1]".into(), shape: LabelShape::Bidirectional, at: pin_at }], page: String::new() });
    let stub = Point { x: pin_at.x - 25_400_000, y: pin_at.y };
    root.wires.push(wire(vec![stub, pin_at], true));
    root.labels.push(label(bus_label, stub, LabelKind::Local));
    root.assign_missing_ids();
    child.assign_missing_ids();
    let sheet_id = root.sheets[0].id.clone();
    design.schematic = Some(root);
    design.sheet_contents = Some([("child.kicad_sch".to_string(), child)].into_iter().collect());
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
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
    let power = eda_model::NetClass { name: "HighCurrent".into(), nets: vec![], track_width: Some(400_000), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 };
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
    let class = eda_model::NetClass { name: "HighCurrent".into(), nets: vec![], track_width: Some(400_000), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 };
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
    let other = format!("{}.{n}", eda_model::sheet_list::reference_under(&screen, sym, Some(&second)));
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
    step(&dir, Cmd::AddLabel { net: "GLOBX".into(), at: Point { x: 50_800_000, y: 101_600_000 }, kind: global() }, false, "test").unwrap();
    step(&dir, Cmd::OnSheet { sheet: sheet_id.clone(), cmd: Box::new(Cmd::AddLabel { net: "GLOBX".into(), at: Point { x: 60_960_000, y: 101_600_000 }, kind: global() }) }, false, "test").unwrap();
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
    assert_eq!(screen.instance_overrides.iter().filter(|o| o.parent_sheet_instance_id == second).count(), screen.symbols.len(), "one reference per symbol, not two");
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
    let its_second = eda_model::sheet_list::reference_under(screen, &moved, Some(&second));
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
    assert_eq!(eda_model::sheet_list::reference_under(screen, now, Some(&second)), its_second, "its reference on the other placement went with it");
    assert!(model.part(&its_second).is_some());
    unique(&design);

    // a symbol added on the first placement: a reference of its own on the second
    let add: Cmd = serde_json::from_value(json!({ "op": "on_sheet", "sheet": sheet.id, "cmd": { "op": "add_symbol", "id": "R90", "lib_id": "Device:R", "at": { "x": 120_000_000, "y": 120_000_000 }, "rot_millideg": 0, "value": "1k", "footprint": "" } })).unwrap();
    step(&dir, add, false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let screen = &design.sheet_contents.as_ref().unwrap()[&sheet.file];
    let added = screen.symbols.iter().find(|s| s.id == "R90").expect("the symbol is on the shared screen");
    let added_second = eda_model::sheet_list::reference_under(screen, added, Some(&second));
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
    let (_, model) = example("ldo.yaml");
    let child = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    let mut design = child.clone();
    let mut root = eda_model::ir::SchematicSection { symbols: vec![], wires: vec![], labels: vec![], power_symbols: vec![], no_connects: vec![], ..child.schematic.clone().unwrap() };
    root.sheets = ["a", "b"].iter().enumerate().map(|(i, n)| eda_model::ir::SheetInstance { id: format!("sh_{n}"), name: format!("LDO {n}"), file: "ldo.kicad_sch".into(), at: Point { x: 20_000_000 + 60_000_000 * i as i64, y: 20_000_000 }, size: (40_000_000, 20_000_000), pins: vec![], page: String::new() }).collect();
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
            let second = eda_model::sheet_list::reference_under(sec, s, placements[1].path.last().map(String::as_str));
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

/// The QA project `rel` read by our reader as it is (no Import Sheet: the symbols at their library origin) and traced as the studio traces a board
/// (`board::trace_design`), beside kicad-cli's netlist of KiCad's own file: (ours, KiCad's), nets of two pins or more.
fn qa_traced(rel: &str) -> Option<(Vec<Vec<String>>, Vec<Vec<String>>)> {
    let src = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from)?.join("qa/data/eeschema").join(rel);
    let work = scratch(&format!("qat_{}", rel.replace(['/', '.'], "_")));
    for e in std::fs::read_dir(src.parent().unwrap()).unwrap().flatten().filter(|e| e.path().is_file()) {
        std::fs::copy(e.path(), work.join(e.file_name())).unwrap();
    }
    let root = work.join(src.file_name().unwrap());
    let kicad = kicad_cli_net_groups(&root);
    let (mut design, mut model, _) = eda_kicad::import_kicad_sch_tree(&root).ok()?;
    crate::board::reconcile_schematic(&mut design, &mut model, None);
    let pin_names = std::collections::BTreeMap::new();
    let aliases = design.bus_aliases.clone();
    let members = move |name: &str| eda_kicad::expand_bus_members(name, &aliases);
    let global_power = |lib_id: &str| -> Option<bool> { model.symbol_of(lib_id).map(|lib| lib.pins.iter().any(|p| p.electrical_type == "power_in")) };
    let naming = eda_engine::nets::Naming { pin_names: Some(&pin_names), bus_members: Some(&members), global_power: Some(&global_power), ..Default::default() };
    let mut ours: Vec<Vec<String>> = crate::board::trace_design(&mut design.clone(), &model, &naming).into_iter().map(|n| n.pins.into_iter().filter(|p| !p.starts_with('#')).collect::<Vec<_>>()).filter(|p: &Vec<String>| p.len() >= 2).collect();
    for g in ours.iter_mut() {
        g.sort();
    }
    ours.sort();
    Some((ours, kicad.into_iter().filter(|g| g.len() >= 2).collect()))
}

/// KiCad's QA projects that show a screen twice, carry buses across sheets or give labels fields of their own, read by our reader and traced as the studio
/// traces a board: the nets are kicad-cli's netlist of KiCad's own files. (`KICAD_SRC_DIR` must name KiCad's source.)
const QA_TRACED_ALIKE: &[&str] = &[
    "issue13212.kicad_sch",
    "issue23840/BusAndVectors.kicad_sch",
    "netlists/test_global_promotion_2/test_global_promotion_2.kicad_sch",
    "netlists/complex_hierarchy/complex_hierarchy.kicad_sch",
    "netlists/complex_hierarchy_shared/complex_hierarchy.kicad_sch",
    "legacy_hierarchy/legacy_hierarchy.kicad_sch",
    "netlists/test_hier_renaming/test_hier_renaming.kicad_sch",
    "netlists/prefix_bus_alias/prefix_bus_alias.kicad_sch",
    "netlists/weak_vector_bus_disambiguation/weak_vector_bus_disambiguation.kicad_sch",
    "netlists/group_bus_matching/group_bus_matching.kicad_sch",
    "netlists/issue16439/issue16439.kicad_sch",
    "issue18119/issue18119.kicad_sch",
    "issue19646/issue19646.kicad_sch",
    "issue23962/issue23962.kicad_sch",
    "issue24044/issue24044.kicad_sch",
    "issue18092/issue18092.kicad_sch",
    "issue13112.kicad_sch",
    "issue12814.kicad_sch",
];

/// The ones the studio still reads otherwise, each with why (PARITY-sch.md section 38, "Review"): printed, not asserted.
const QA_TRACED_OTHERWISE: &[(&str, &str)] = &[
    ("issue10926_1.kicad_sch", "a screen shown twice inside a screen shown twice: one override per immediate parent"),
    ("netlists/hierarchy_aliases/hierarchy_aliases.kicad_sch", "a screen shown twice inside a screen shown twice"),
    ("netlists/bus_connection/bus_connection.kicad_sch", "two buses on one sheet joined only by a member name they share"),
    ("issue17771/issue1771.kicad_sch", "two buses on one sheet joined only by a member name they share"),
    ("ERC_dynamic_power_symbol_test.kicad_sch", "a power symbol's ${#} is not resolved per placement"),
    ("netlists/video/video.kicad_sch", "not looked into"),
];

#[test]
fn slow_kicad_qa_hierarchies_trace_as_kicad_cli_reads_them() {
    if !kicad_cli_ready() || std::env::var_os("KICAD_SRC_DIR").is_none() {
        return;
    }
    let only = |a: &[Vec<String>], b: &[Vec<String>]| -> Vec<Vec<String>> { a.iter().filter(|x| !b.contains(x)).cloned().collect() };
    for rel in QA_TRACED_ALIKE {
        let (ours, kicad) = qa_traced(rel).unwrap_or_else(|| panic!("{rel} reads"));
        assert!(!kicad.is_empty() || ours.is_empty(), "{rel}");
        assert_eq!(ours, kicad, "{rel}: the studio's nets are kicad-cli's; ours only {:?}, kicad-cli's only {:?}", only(&ours, &kicad), only(&kicad, &ours));
    }
    for (rel, why) in QA_TRACED_OTHERWISE {
        let Some((ours, kicad)) = qa_traced(rel) else { continue };
        println!("{rel} ({why}): {} nets differ of {}", only(&kicad, &ours).len(), kicad.len());
    }
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
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![Point { x: 0, y: 0 }, Point { x: 35_000_000, y: 0 }, Point { x: 35_000_000, y: 35_000_000 }, Point { x: 0, y: 35_000_000 }], footprints: vec![], modules: vec![] });
    design.drawings = Some(eda_model::ir::DrawingsSection::default());
    save(&dir, &design).unwrap();
    let meta = Meta { intent: intent.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
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
    let meta = Meta { intent: intent.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
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
            *at = Point { x: 254_000_000, y: 50_800_000 };
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
    // the ratchet: 87 of KiCad's 130 QA roots on 2026-10-23 (nightly 20261021); before the review of that day, 53 (turned and mirrored symbols were traced
    // and written otherwise than drawn, and one file hung Import Sheet)
    if only.is_none() {
        assert!(same >= 87, "{same} judged alike, fewer than the 87 of 2026-10-23");
    }
}

/// [`kicad_cli_net_groups`] that answers no nets when kicad-cli writes no netlist.
fn kicad_cli_net_groups_opt(root: &Path) -> Vec<Vec<String>> {
    std::panic::catch_unwind(|| kicad_cli_net_groups(root)).unwrap_or_default()
}
