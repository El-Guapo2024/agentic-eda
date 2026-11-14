//! Design variants (UI-LEFT S2; KiCad 10, newer than 8303b2ad) through the schematic reader and writer, against KiCad 25ec2335's QA project
//! `qa/data/cli/variants` -- the one its own `test_sch_export_bom_variants` (`qa/tests/cli/test_sch.py`) exports a BOM of per variant:
//!
//! - every symbol's `(variant ..)` records and the project's `schematic.variants` are read (`parseSchematicSymbol`'s `T_variant`, the `variants` `PARAM_LAMBDA`);
//! - the writer puts them back so that reading our file gives the same records;
//! - (slow) kicad-cli's BOM of our file, `--variant "Variant 1"` / `--variant Variant2` / none with KiCad's test's arguments, is KiCad's expected CSV
//!   (`variants_v1.bom.csv`, `variants_v2.bom.csv`, `variants_default.bom.csv`) byte for byte: the DNP of `J1` and `R14` and the `Value` of `R1` follow the
//!   variant;
//! - (slow) a variant excluded from the BOM is read by kicad-cli as our writer means it at our file's version (`in_bom`'s sense changed at 20260306).
//!
//! The QA data is KiCad 25ec2335's (`KICAD10_QA_DATA`, by default `$HOME/src/kicad-src-25ec2335/qa/data`); without it the tests say so and pass. Run the slow
//! ones with `EDA_SLOW_TESTS=1 cargo test -p eda-kicad --test design_variants -- --nocapture`.

use std::path::{Path, PathBuf};
use std::process::Command;

use eda_kicad::{export_kicad_pro_for, export_kicad_sch_tree, import_kicad_sch_tree_with, ExportMeta, ImportOptions};
use eda_model::ir::Design;

fn qa_project() -> Option<PathBuf> {
    let root = std::env::var_os("KICAD10_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("src/kicad-src-25ec2335/qa/data")))?;
    let p = root.join("cli/variants");
    p.join("variants.kicad_sch").exists().then_some(p)
}

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn kicad_cli() -> Option<PathBuf> {
    let out = Command::new("which").arg("kicad-cli").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!p.is_empty()).then(|| PathBuf::from(p))
}

fn work_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_design_variants_{}", std::process::id())).join(tag);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The QA project read as `eda import-kicad` reads it: the schematic tree, then the project's Schematic Setup (`project_schematic::apply`).
fn read(dir: &Path, stem: &str) -> (Design, eda_model::ConstraintModel) {
    let (mut design, model, _) = import_kicad_sch_tree_with(&dir.join(format!("{stem}.kicad_sch")), &ImportOptions { unique_references: true }).expect("the schematic reads");
    if let Ok(pro) = std::fs::read_to_string(dir.join(format!("{stem}.kicad_pro"))) {
        eda_kicad::project_schematic::apply(&mut design.schematic.as_mut().unwrap().extras, &pro);
    }
    (design, model)
}

/// The design written as the derived project is: every sheet file and the project file.
fn write(design: &Design, model: &eda_model::ConstraintModel, dir: &Path, stem: &str) {
    for (name, text) in export_kicad_sch_tree(design, model, &ExportMeta { date: "", title: stem }, &format!("{stem}.kicad_sch")).expect("the schematic writes") {
        std::fs::write(dir.join(name), text).unwrap();
    }
    std::fs::write(dir.join(format!("{stem}.kicad_pro")), export_kicad_pro_for(design, model)).unwrap();
}

fn records(design: &Design) -> Vec<(String, String, eda_model::variants::SymbolVariant)> {
    let mut out = Vec::new();
    for sch in design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values())) {
        for (reference, recs) in &sch.extras.symbol_variants {
            for (name, rec) in recs {
                out.push((reference.clone(), name.clone(), rec.clone()));
            }
        }
    }
    out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    out
}

#[test]
fn the_qa_projects_variants_are_read_and_written_back() {
    let Some(qa) = qa_project() else {
        eprintln!("skipped: needs KiCad 25ec2335's QA data (KICAD10_QA_DATA)");
        return;
    };
    let (design, model) = read(&qa, "variants");
    let root = design.schematic.as_ref().unwrap();
    assert_eq!(root.extras.variants.get("Variant 1").map(String::as_str), Some("testing variant 1 description"), "the project's `schematic.variants`");
    assert_eq!(root.extras.variants.get("Variant2").map(String::as_str), Some("Test of variant 2 desc"));
    assert_eq!(eda_model::variants::names_for_ui(&design), ["< Default >", "Variant 1", "Variant2"]);
    let read_back = records(&design);
    let find = |r: &str, v: &str| read_back.iter().find(|(a, b, _)| a == r && b == v).map(|(_, _, rec)| rec.clone());
    assert_eq!(find("J1", "Variant 1").and_then(|r| r.dnp), Some(true), "`(variant (name \"Variant 1\") (dnp yes))` of J1");
    assert_eq!(find("R1", "Variant 1").unwrap().fields.get("Value").map(String::as_str), Some("10K_V1"));
    assert_eq!(find("R1", "Variant2").unwrap().fields.get("Value").map(String::as_str), Some("10K_V2"));
    assert_eq!(find("R14", "Variant2").and_then(|r| r.dnp), Some(true));
    assert_eq!(find("R1", "Variant 1").and_then(|r| r.dnp), Some(false), "a record starts as the symbol's own flags (`InitializeAttributes`)");

    // written, and read again: the same records, and the file says only what differs
    let dir = work_dir("roundtrip");
    write(&design, &model, &dir, "variants");
    let text = std::fs::read_to_string(dir.join("variants.kicad_sch")).unwrap();
    assert!(text.contains("(variant (name \"Variant 1\") (dnp yes))"), "J1's record, its one difference");
    assert!(text.contains("(variant (name \"Variant 1\") (field (name \"Value\") (value \"10K_V1\")))"), "R1's record: no flag equal to the symbol's");
    let pro: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("variants.kicad_pro")).unwrap()).unwrap();
    assert_eq!(pro["schematic"]["variants"], serde_json::json!([{ "name": "Variant 1", "description": "testing variant 1 description" }, { "name": "Variant2", "description": "Test of variant 2 desc" }]));
    let (again, _) = read(&dir, "variants");
    assert_eq!(records(&again), read_back, "the records survive the writer and the reader");
}

/// KiCad's own test of the BOM per variant, on the file our writer made of the QA project.
#[test]
fn kicad_clis_bom_of_our_file_is_kicads_per_variant() {
    if slow_tests_off() {
        return;
    }
    let (Some(qa), Some(cli)) = (qa_project(), kicad_cli()) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's QA data");
        return;
    };
    let (design, model) = read(&qa, "variants");
    let dir = work_dir("bom");
    write(&design, &model, &dir, "variants");
    for (variant, expected) in [(None, "variants_default.bom.csv"), (Some("Variant 1"), "variants_v1.bom.csv"), (Some("Variant2"), "variants_v2.bom.csv")] {
        let out = dir.join(format!("{}.csv", variant.unwrap_or("default")));
        let mut cmd = Command::new(&cli);
        cmd.args(["sch", "export", "bom"]);
        if let Some(v) = variant {
            cmd.args(["--variant", v]);
        }
        let status = cmd.args(["--exclude-dnp", "--fields", "Reference,Value", "--labels", "Refs,Value", "-o"]).arg(&out).arg(dir.join("variants.kicad_sch")).output().unwrap();
        assert!(status.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&status.stderr));
        let ours = std::fs::read_to_string(&out).unwrap();
        let kicads = std::fs::read_to_string(qa.join(expected)).unwrap();
        assert_eq!(ours, kicads, "the BOM of {variant:?}");
    }
}

/// `in_bom` in a variant: our file is at 20250114, before "Variant in_bom semantics corrected" (20260306), where kicad-cli reads the word as the exclusion
/// itself; the writer says it that way, so a variant that leaves a symbol out of the BOM leaves it out of kicad-cli's.
#[test]
fn a_variant_left_out_of_the_bom_is_left_out_of_kicad_clis() {
    if slow_tests_off() {
        return;
    }
    let (Some(qa), Some(cli)) = (qa_project(), kicad_cli()) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's QA data");
        return;
    };
    let (mut design, model) = read(&qa, "variants");
    let sch = design.schematic.as_mut().unwrap();
    assert!(eda_model::variants::set_flag(sch, "R2", "Variant2", eda_model::variants::Flag::ExcludeFromBom, true));
    let dir = work_dir("in_bom");
    write(&design, &model, &dir, "variants");
    let bom = |variant: Option<&str>| {
        let out = dir.join(format!("{}.csv", variant.unwrap_or("default")));
        let mut cmd = Command::new(&cli);
        cmd.args(["sch", "export", "bom"]);
        if let Some(v) = variant {
            cmd.args(["--variant", v]);
        }
        assert!(cmd.args(["--fields", "Reference,Value", "-o"]).arg(&out).arg(dir.join("variants.kicad_sch")).output().unwrap().status.success());
        std::fs::read_to_string(&out).unwrap()
    };
    let has_r2 = |t: &str| t.lines().any(|l| l.split(',').next() == Some("\"R2\""));
    assert!(has_r2(&bom(None)), "R2 is in the default variant's BOM");
    assert!(has_r2(&bom(Some("Variant 1"))), "and in Variant 1's");
    assert!(!has_r2(&bom(Some("Variant2"))), "Variant2 leaves it out");
}

/// A sheet's own Do not populate and its record of a variant (`SCH_SHEET_INSTANCE::m_Variants`, UI-LEFT-2 SS15): written in the sheet's `(instances ..)`, read back,
/// and the symbols inside it are left out of kicad-cli's BOM with `--exclude-dnp` in that variant only (`ref.GetSheetPath().GetDNP( variantName )`).
#[test]
fn a_sheets_variant_record_comes_back_and_kicad_clis_bom_leaves_its_symbols_out() {
    let Some(qa) = qa_project() else {
        eprintln!("skipped: needs KiCad 25ec2335's QA data (KICAD10_QA_DATA)");
        return;
    };
    let (mut design, model) = read(&qa, "variants");
    let sheet_id = design.schematic.as_ref().unwrap().sheets.first().expect("the QA project has a sheet").id.clone();
    let inside: Vec<String> = {
        let path = vec![sheet_id.clone()];
        let screen = eda_model::sheet_list::screen_of(&design, &path).expect("its screen");
        screen.symbols.iter().filter(|s| !s.id.starts_with('#')).map(|s| eda_model::sheet_list::reference_under(screen, s, &path)).collect()
    };
    assert!(!inside.is_empty(), "the sheet holds symbols");
    assert!(eda_model::variants::set_sheet_flag(design.schematic.as_mut().unwrap(), &sheet_id, "Variant2", eda_model::variants::Flag::Dnp, true));
    let dir = work_dir("sheet_variant");
    write(&design, &model, &dir, "variants");
    let text = std::fs::read_to_string(dir.join("variants.kicad_sch")).unwrap();
    assert!(text.contains("(variant (name \"Variant2\") (dnp yes))"), "the sheet's record in its instance path");
    let (again, _) = read(&dir, "variants");
    let back = &again.schematic.as_ref().unwrap().extras.sheet_variants;
    assert_eq!(back.values().next().and_then(|m| m.get("Variant2")).map(|r| r.dnp), Some(true), "read back: {back:?}");
    if slow_tests_off() {
        return;
    }
    let Some(cli) = kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let bom = |variant: Option<&str>| {
        let out = dir.join(format!("{}.csv", variant.unwrap_or("default")));
        let mut cmd = Command::new(&cli);
        cmd.args(["sch", "export", "bom"]);
        if let Some(v) = variant {
            cmd.args(["--variant", v]);
        }
        let run = cmd.args(["--exclude-dnp", "--group-by", "", "--fields", "Reference,Value", "-o"]).arg(&out).arg(dir.join("variants.kicad_sch")).output().unwrap();
        assert!(run.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&run.stderr));
        std::fs::read_to_string(&out).unwrap()
    };
    let has = |t: &str, r: &str| t.lines().any(|l| l.split(',').next().is_some_and(|c| c.trim_matches('"').split(' ').any(|x| x == r)));
    let first = &inside[0];
    assert!(has(&bom(None), first), "{first} is in the default variant's BOM");
    assert!(has(&bom(Some("Variant 1")), first), "and in Variant 1's");
    assert!(!has(&bom(Some("Variant2")), first), "Variant2's DNP of its sheet leaves it out:\n{}", bom(Some("Variant2")));
    // the sheet's own Do not populate: every variant without a record of its own
    let sch = design.schematic.as_mut().unwrap();
    let mut props = sch.extras.sheet_props_of(&sheet_id);
    props.dnp = true;
    sch.extras.set_sheet_props(&sheet_id, props);
    write(&design, &model, &dir, "variants");
    assert!(!has(&bom(None), first), "the sheet's own DNP leaves it out of the default BOM");
}

/// A channel sheet placed twice (UI-LEFT-2 SS16): KiCad keeps a symbol's records per instance (`SCH_SYMBOL_INSTANCE::m_Variants`, one per sheet path), so each path's
/// `(variant ..)` blocks are read as that path's, a variant symbol (`(symbol_override ..)`) with them, and written back under the same path.
#[test]
fn each_sheet_paths_records_and_variant_symbol_come_back_under_their_path() {
    let dir = work_dir("paths");
    let root = "00000000-0000-0000-0000-000000000001";
    let (a, b) = ("00000000-0000-0000-0000-0000000000a1", "00000000-0000-0000-0000-0000000000a2");
    let sheet = |uuid: &str, name: &str, x: i32, page: &str| {
        format!(
            "\t(sheet (at {x} 10) (size 20 10) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (fields_autoplaced yes) (stroke (width 0.1524) (type solid)) (fill (color 0 0 0 0.0000)) (uuid \"{uuid}\")\n\t\t(property \"Sheetname\" \"{name}\" (at {x} 9.3 0) (effects (font (size 1.27 1.27)) (justify left bottom)))\n\t\t(property \"Sheetfile\" \"chan.kicad_sch\" (at {x} 20.5 0) (effects (font (size 1.27 1.27)) (justify left top)))\n\t\t(property \"Channel\" \"{name}\" (at {x} 22 0) (effects (font (size 1.27 1.27)) (justify left top)))\n\t\t(instances (project \"p\" (path \"/{root}\" (page \"{page}\") (variant (name \"Lite\") (field (name \"Channel\") (value \"{name}-lite\"))))))\n\t)\n"
        )
    };
    let top = format!(
        "(kicad_sch (version 20250114) (generator \"eeschema\") (generator_version \"9.0\") (uuid \"{root}\") (paper \"A4\")\n\t(lib_symbols)\n{}{}\t(sheet_instances (path \"/\" (page \"1\")))\n)\n",
        sheet(a, "A", 10, "2"),
        sheet(b, "B", 40, "3")
    );
    let chan = format!(
        "(kicad_sch (version 20250114) (generator \"eeschema\") (generator_version \"9.0\") (uuid \"00000000-0000-0000-0000-0000000000c1\") (paper \"A4\")\n\t(lib_symbols (symbol \"Device:R\" (pin_numbers (hide yes)) (pin_names (offset 0)) (exclude_from_sim no) (in_bom yes) (on_board yes) (property \"Reference\" \"R\" (at 2.032 0 90) (effects (font (size 1.27 1.27)))) (property \"Value\" \"R\" (at 0 0 90) (effects (font (size 1.27 1.27)))) (symbol \"R_0_1\" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none)))) (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\" (effects (font (size 1.27 1.27)))) (number \"1\" (effects (font (size 1.27 1.27))))) (pin passive line (at 0 -3.81 90) (length 1.27) (name \"~\" (effects (font (size 1.27 1.27)))) (number \"2\" (effects (font (size 1.27 1.27))))))))\n\t(symbol (lib_id \"Device:R\") (at 50 50 0) (unit 1) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (uuid \"00000000-0000-0000-0000-0000000000d1\")\n\t\t(property \"Reference\" \"R1\" (at 52 49 0) (effects (font (size 1.27 1.27))))\n\t\t(property \"Value\" \"10k\" (at 52 51 0) (effects (font (size 1.27 1.27))))\n\t\t(property \"Footprint\" \"\" (at 50 50 0) (effects (font (size 1.27 1.27)) (hide yes)))\n\t\t(property \"Datasheet\" \"\" (at 50 50 0) (effects (font (size 1.27 1.27)) (hide yes)))\n\t\t(instances (project \"p\"\n\t\t\t(path \"/{root}/{a}\" (reference \"R1\") (unit 1) (variant (name \"Lite\") (field (name \"Value\") (value \"4k7\"))))\n\t\t\t(path \"/{root}/{b}\" (reference \"R2\") (unit 1) (variant (name \"Lite\") (dnp yes) (symbol_override \"Device:R_Small\")))\n\t\t))\n\t)\n)\n"
    );
    std::fs::write(dir.join("top.kicad_sch"), top).unwrap();
    std::fs::write(dir.join("chan.kicad_sch"), chan).unwrap();
    let (design, model) = read(&dir, "top");
    let screen = design.sheet_contents.as_ref().unwrap().values().next().unwrap();
    let pa = vec![a.to_string()];
    let pb = vec![b.to_string()];
    let ra = eda_model::variants::record_on(screen, "R1", &pa, "Lite").expect("path A's record");
    assert_eq!(ra.fields.get("Value").map(String::as_str), Some("4k7"));
    assert_eq!(ra.symbol_override, None);
    let rb = eda_model::variants::record_on(screen, "R1", &pb, "Lite").expect("path B's record");
    assert_eq!(rb.dnp, Some(true));
    assert!(rb.fields.is_empty(), "path B's record is its own, not path A's");
    assert_eq!(rb.symbol_override.as_deref(), Some("Device:R_Small"));
    // a sheet's variant text of its user field (`SCH_SHEET_VARIANT::m_Fields`)
    let top_sch = design.schematic.as_ref().unwrap();
    assert_eq!(eda_model::variants::sheet_flags(top_sch, a, "Lite").fields.get("Channel").map(String::as_str), Some("A-lite"));
    let out = work_dir("paths_out");
    write(&design, &model, &out, "top");
    let chan_file = std::fs::read_dir(&out).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n != "top.kicad_sch" && n.to_string_lossy().ends_with(".kicad_sch"))).expect("the channel file");
    let text = std::fs::read_to_string(&chan_file).unwrap();
    assert!(text.contains("(variant (name \"Lite\") (field (name \"Value\") (value \"4k7\")))"), "{text}");
    assert!(text.contains("(variant (name \"Lite\") (dnp yes) (symbol_override \"Device:R_Small\"))"), "{text}");
    let top_text = std::fs::read_to_string(out.join("top.kicad_sch")).unwrap();
    assert!(top_text.contains("(variant (name \"Lite\") (field (name \"Channel\") (value \"A-lite\")))"), "{top_text}");
    let (again, _) = read(&out, "top");
    let screen = again.sheet_contents.as_ref().unwrap().values().next().unwrap();
    assert_eq!(eda_model::variants::record_on(screen, "R1", &pb, "Lite").and_then(|r| r.symbol_override.clone()).as_deref(), Some("Device:R_Small"));
    assert!(eda_model::variants::record_on(screen, "R1", &pa, "Lite").is_some_and(|r| r.symbol_override.is_none() && r.dnp == Some(false)));

    // (slow) kicad-cli reads the file, `symbol_override` and all, and its BOM of 'Lite' is each path's: R1 with path A's Value, R2 (path B) left out as DNP
    if slow_tests_off() {
        return;
    }
    let Some(cli) = kicad_cli() else { return };
    let bom = out.join("lite.csv");
    let run = Command::new(&cli).args(["sch", "export", "bom", "--variant", "Lite", "--exclude-dnp", "--fields", "Reference,Value", "--labels", "Refs,Value", "-o"]).arg(&bom).arg(out.join("top.kicad_sch")).output().unwrap();
    assert!(run.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&run.stderr));
    let text = std::fs::read_to_string(&bom).unwrap();
    assert!(text.contains("\"R1\",\"4k7\""), "{text}");
    assert!(!text.contains("R2"), "path B's R2 is DNP in 'Lite': {text}");
}

/// UI-LEFT-3 S3-33: a sheet drawn on a screen shown twice keeps its records of the variants per sheet path (`SCH_SHEET_INSTANCE::m_Variants`, one per path:
/// `parseSheet` reads each `(path ..)`'s `(variant ..)`, `saveSheet` writes each path's): the inner sheet of a channel placed twice is DNP in 'Lite' on path A
/// only; read, written back under the same paths, and (slow) kicad-cli's BOM of 'Lite' leaves out the symbol inside it on path A only.
#[test]
fn a_sheet_on_a_shared_screen_keeps_each_paths_variant_records() {
    let dir = work_dir("sheet_paths");
    let root = "00000000-0000-0000-0000-000000000001";
    let (a, b, inner) = ("00000000-0000-0000-0000-0000000000a1", "00000000-0000-0000-0000-0000000000a2", "00000000-0000-0000-0000-0000000000b1");
    let head = |uuid: &str| format!("(kicad_sch (version 20250114) (generator \"eeschema\") (generator_version \"9.0\") (uuid \"{uuid}\") (paper \"A4\")\n");
    let sheet = |uuid: &str, name: &str, file: &str, x: i32, instances: &str| {
        format!(
            "\t(sheet (at {x} 10) (size 20 10) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (fields_autoplaced yes) (stroke (width 0.1524) (type solid)) (fill (color 0 0 0 0.0000)) (uuid \"{uuid}\")\n\t\t(property \"Sheetname\" \"{name}\" (at {x} 9.3 0) (effects (font (size 1.27 1.27)) (justify left bottom)))\n\t\t(property \"Sheetfile\" \"{file}\" (at {x} 20.5 0) (effects (font (size 1.27 1.27)) (justify left top)))\n\t\t(instances (project \"p\" {instances}))\n\t)\n"
        )
    };
    let top = format!(
        "{}\t(lib_symbols)\n{}{}\t(sheet_instances (path \"/\" (page \"1\")))\n)\n",
        head(root),
        sheet(a, "A", "chan.kicad_sch", 10, &format!("(path \"/{root}\" (page \"2\"))")),
        sheet(b, "B", "chan.kicad_sch", 40, &format!("(path \"/{root}\" (page \"3\"))"))
    );
    let chan = format!(
        "{}\t(lib_symbols)\n{})\n",
        head("00000000-0000-0000-0000-0000000000c1"),
        sheet(inner, "Inner", "inner.kicad_sch", 10, &format!("(path \"/{root}/{a}\" (page \"4\") (variant (name \"Lite\") (dnp yes))) (path \"/{root}/{b}\" (page \"5\"))"))
    );
    let r = "(symbol \"Device:R\" (pin_numbers (hide yes)) (pin_names (offset 0)) (exclude_from_sim no) (in_bom yes) (on_board yes) (property \"Reference\" \"R\" (at 2.032 0 90) (effects (font (size 1.27 1.27)))) (property \"Value\" \"R\" (at 0 0 90) (effects (font (size 1.27 1.27)))) (symbol \"R_0_1\" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none)))) (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\" (effects (font (size 1.27 1.27)))) (number \"1\" (effects (font (size 1.27 1.27))))) (pin passive line (at 0 -3.81 90) (length 1.27) (name \"~\" (effects (font (size 1.27 1.27)))) (number \"2\" (effects (font (size 1.27 1.27)))))))";
    let inside = format!(
        "{}\t(lib_symbols {r})\n\t(symbol (lib_id \"Device:R\") (at 50 50 0) (unit 1) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (uuid \"00000000-0000-0000-0000-0000000000d1\")\n\t\t(property \"Reference\" \"R10\" (at 52 49 0) (effects (font (size 1.27 1.27))))\n\t\t(property \"Value\" \"10k\" (at 52 51 0) (effects (font (size 1.27 1.27))))\n\t\t(property \"Footprint\" \"\" (at 50 50 0) (effects (font (size 1.27 1.27)) (hide yes)))\n\t\t(property \"Datasheet\" \"\" (at 50 50 0) (effects (font (size 1.27 1.27)) (hide yes)))\n\t\t(instances (project \"p\"\n\t\t\t(path \"/{root}/{a}/{inner}\" (reference \"R10\") (unit 1))\n\t\t\t(path \"/{root}/{b}/{inner}\" (reference \"R11\") (unit 1))\n\t\t))\n\t)\n)\n",
        head("00000000-0000-0000-0000-0000000000e1")
    );
    std::fs::write(dir.join("top.kicad_sch"), top).unwrap();
    std::fs::write(dir.join("chan.kicad_sch"), chan).unwrap();
    std::fs::write(dir.join("inner.kicad_sch"), inside).unwrap();
    let (design, model) = read(&dir, "top");
    let dnp_on = |d: &Design, first: &str| eda_model::variants::sheet_path_flags(d, &[first.to_string(), inner.to_string()], "Lite").dnp;
    assert!(dnp_on(&design, a) && !dnp_on(&design, b), "path A's record only");
    let out = work_dir("sheet_paths_out");
    write(&design, &model, &out, "top");
    let chan_file = std::fs::read_dir(&out).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("\"Inner\""))).expect("the channel file");
    let text = std::fs::read_to_string(&chan_file).unwrap();
    assert_eq!(text.matches("(variant (name \"Lite\") (dnp yes))").count(), 1, "one path's record: {text}");
    let (again, _) = read(&out, "top");
    let (sa, sb): (Vec<String>, Vec<String>) = {
        let sheets = &again.schematic.as_ref().unwrap().sheets;
        (vec![sheets.iter().find(|s| s.name == "A").unwrap().id.clone()], vec![sheets.iter().find(|s| s.name == "B").unwrap().id.clone()])
    };
    let inner_id = eda_model::sheet_list::screen_of(&again, &sa).unwrap().sheets[0].id.clone();
    let flags = |p: &[String]| eda_model::variants::sheet_path_flags(&again, &[p[0].clone(), inner_id.clone()], "Lite").dnp;
    assert!(flags(&sa) && !flags(&sb), "read back per path");
    if slow_tests_off() {
        return;
    }
    let Some(cli) = kicad_cli() else { return };
    let bom = out.join("lite.csv");
    let run = Command::new(&cli).args(["sch", "export", "bom", "--variant", "Lite", "--exclude-dnp", "--group-by", "", "--fields", "Reference,Value", "-o"]).arg(&bom).arg(out.join("top.kicad_sch")).output().unwrap();
    assert!(run.status.success(), "kicad-cli: {}", String::from_utf8_lossy(&run.stderr));
    let text = std::fs::read_to_string(&bom).unwrap();
    assert!(text.contains("R11") && !text.contains("R10"), "R10 (path A) is DNP in 'Lite' through its sheet, R11 (path B) is not: {text}");
}
