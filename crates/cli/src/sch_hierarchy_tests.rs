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
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 60_000, "dy": 0 })).unwrap();
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
    let edit: Cmd = serde_json::from_value(json!({ "op": "sch_edit", "verb": "edit_field", "id": "fld:R3:Reference", "at": { "x": r3.x - 12_700, "y": r3.y + 1_270 }, "h": "right", "size_um": 1778, "bold": true })).unwrap();
    step(&dir, edit, false, "test").unwrap();
    step(&dir, Cmd::ReorganizeSheets, false, "test").unwrap();
    let (_, sheets, _) = load(&dir).unwrap();
    let kept = sheets.sheet_contents.iter().flatten().flat_map(|(_, s)| s.symbols.iter().map(move |x| (s, x))).find(|(_, x)| x.id == "R3").unwrap();
    let place = kept.0.field_layout.get("R3").and_then(|f| f.iter().find(|p| p.name == "Reference")).expect("R3's Reference keeps a place of its own");
    assert_eq!((place.dx, place.dy), (-12_700, 1_270), "the field is where the user put it from its symbol");
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
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000 })).unwrap();
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
    let pins = ["1", "2", "3", "4", "5", "6", "7", "8"].iter().map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect();
    let part = Part { reference: "U9".into(), mpn: None, lcsc: None, value: Some("TL072".into()), package: None, footprint: None, symbol: Some("Test:Dual3".into()), datasheet: None, pins, body_um: None, edge: None };
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
    for (name, a, b) in [("AMP_FB1", "U9.1", "U9.2"), ("AMP_FB2", "U9.7", "U9.6"), ("AMP_X", "U9.3", "U9.5")] {
        model.nets.push(eda_model::Net { name: name.into(), pins: vec![a.into(), b.into()] });
    }
    let intent = dir.join("intent.yaml");
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    // the flat drawing is mcu30's, with the op-amp's three units placed on the root (the nets are the model's)
    let mut design = eda_engine::derive_schematic(&flat_model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    {
        let root = design.schematic.as_mut().unwrap();
        for (unit, x) in [(1, 300_000), (2, 330_000), (3, 360_000)] {
            root.symbols.push(eda_model::ir::SymbolInstance {
                id: "U9".into(),
                at: Point { x, y: 20_000 },
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
    let meta = Meta { intent: intent.display().to_string(), snap_um: 100, spacing_um: 300 };
    std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();

    step(&dir, Cmd::ReorganizeSheets, false, "test").unwrap();
    let (_, after, _) = load(&dir).unwrap();
    let units: Vec<u32> = after.sheet_contents.iter().flatten().flat_map(|(_, s)| s.symbols.iter()).filter(|s| s.id == "U9").map(|s| s.unit).collect();
    assert_eq!(units.len(), 3, "each unit is placed once: {units:?}");
    assert_no_reference_findings(&dir, "reorganized with a multi-unit part");
    let kinds = erc_kinds(&dir);
    assert!(kinds.is_empty(), "the reorganized board with its dual op-amp is clean: {kinds:?}");
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
    let cmd: Cmd = serde_json::from_value(json!({ "op": "paste_sch", "fragment": parsed["fragment"], "dx": 0, "dy": 40_000 })).unwrap();
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
