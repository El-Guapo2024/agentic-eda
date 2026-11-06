//! A symbol cross-reference typed in the project's unit notation (r7, UI-LEFT S22): `SCHEMATIC::ConvertRefsToKIIDs` compares a typed `${REF:FIELD}` with
//! `GetRef( &aPath, true )`, which names a unit by `SubReference` in the project's notation. In a project whose units read `U1.1`, `U1.2`, the dotted form is
//! the one that names unit 2; our writer used to know the letter form only (`U1B`), so `${U1.2:Value}` was written `<Unresolved: U1.2>`.
//!
//! The pure-Rust test is the writer's text. The slow one hands the file and its project to kicad-cli (`EDA_SLOW_TESTS=1`, skipped without one): its XML netlist
//! writes a field as `GetShownText`, so the cross-reference reads the value of the symbol it names.

use eda_kicad::{export_kicad_sch, ExportMeta};
use eda_model::ir::{Design, Point, Provenance, SchematicSection, SymbolInstance};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::path::PathBuf;
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-11-06", title: "xref" };

fn pin(number: &str, unit: u32, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: "~".into(), electrical_type: "passive".into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit, hidden: false }
}

fn lib(lib_id: &str, units: u32) -> LibSymbol {
    let mut pins = Vec::new();
    let mut graphics = Vec::new();
    for u in 1..=units {
        pins.push(pin(&format!("{}", 2 * u - 1), u, -5.08, 0.0, 0.0));
        pins.push(pin(&format!("{}", 2 * u), u, 5.08, 0.0, 180.0));
        graphics.push(SymbolGraphic::Rectangle { unit: u, start: SPoint::new(-2.54, 2.54), end: SPoint::new(2.54, -2.54), stroke_mm: 0.254, filled: false });
    }
    LibSymbol {
        connectivity: Default::default(),
        lib_id: lib_id.into(),
        graphics,
        pins,
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: if units > 1 { "U".into() } else { "R".into() },
        unit_count: units,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    }
}

fn part(reference: &str, value: &str, symbol: &str, pins: &[&str]) -> Part {
    let pins = pins.iter().map(|n| Pin { number: (*n).into(), name: None, kind: PinKind::Signal }).collect();
    Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(value.into()), package: None, footprint: Some("Foo:Bar".into()), symbol: Some(symbol.into()), datasheet: None, pins, body_nm: None, edge: None }
}

fn model() -> ConstraintModel {
    ConstraintModel {
        parts: vec![part("U1", "DUAL", "test:DUAL", &["1", "2", "3", "4"]), part("R3", "10k", "test:R", &["1", "2"])],
        symbols: vec![lib("test:DUAL", 2), lib("test:R", 1)],
        ..Default::default()
    }
}

fn instance(id: &str, lib_id: &str, unit: u32, value: &str, x: f64) -> SymbolInstance {
    SymbolInstance { id: id.into(), at: Point { x: mm(x), y: mm(50.8) }, rot: 0, mirrored: false, mirror_y: false, lib_id: lib_id.into(), unit, value: value.into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

/// U1 (two units) and R3, whose field `XREF` names U1's units, in a project of the `.1` notation.
fn design(xref: &str) -> Design {
    let mut sch = SchematicSection { symbols: vec![instance("U1", "test:DUAL", 1, "DUAL", 50.8), instance("U1", "test:DUAL", 2, "DUAL", 76.2), instance("R3", "test:R", 1, "10k", 101.6)], ..Default::default() };
    sch.user_fields.entry("R3".into()).or_default().insert("XREF".into(), xref.into());
    sch.extras.subpart_id_separator = Some('.' as u32);
    sch.extras.subpart_first_id = Some('1' as u32);
    Design { schema: eda_model::ir::SCHEMA_VERSION, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None }
}

#[test]
fn the_dotted_form_names_a_unit_in_a_project_of_that_notation() {
    let m = model();
    let text = export_kicad_sch(&design("${U1.2:Value} ${U1B:Value} ${R3:Value}"), &m, &META).expect("the sheet exports");
    let line = text.lines().find(|l| l.contains("\"XREF\"")).expect("the XREF field is written");
    assert!(!line.contains("<Unresolved: U1.2>") && line.contains("<Unresolved: U1B>"), "the dotted form finds unit 2, the letter form nothing: {line}");
    assert_eq!(line.matches("${/").count(), 2, "U1.2 and R3 are written as KiCad stores them: {line}");
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    let out = Command::new("which").arg("kicad-cli").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !p.is_empty()).then(|| PathBuf::from(p))
}

#[test]
fn kicad_resolves_the_dotted_cross_reference_we_write() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_xref_notation_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let m = model();
    let text = export_kicad_sch(&design("${U1.2:Value}"), &m, &META).expect("the sheet exports");
    std::fs::write(dir.join("xref.kicad_sch"), text).unwrap();
    // the project's unit notation (`SCHEMATIC_SETTINGS::m_SubpartIdSeparator` '.', `m_SubpartFirstId` '1')
    std::fs::write(dir.join("xref.kicad_pro"), r#"{ "meta": { "filename": "xref.kicad_pro", "version": 3 }, "schematic": { "subpart_id_separator": 46, "subpart_first_id": 49 } }"#).unwrap();
    let out = dir.join("xref.xml");
    let run = Command::new(&cli).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(dir.join("xref.kicad_sch")).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "kicad-cli sch export netlist: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let xml = std::fs::read_to_string(&out).unwrap();
    let field = xml.split("<field name=\"XREF\">").nth(1).and_then(|r| r.split("</field>").next()).expect("R3's XREF field in the netlist").to_string();
    assert_eq!(field, "DUAL", "kicad-cli reads U1's unit 2 through the dotted form: {xml}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `SCHEMATIC::ConvertKIIDsToRefs` on the way in: a file KiCad saved keeps a cross-reference by the symbol's path; the import gives the model the reference in
/// the project's notation (`${U1.2:Value}`), and the next write turns it into the derived file's path, which kicad-cli resolves (slow part).
#[test]
fn a_cross_reference_a_file_keeps_by_path_reads_back_as_the_reference() {
    let dir = std::env::temp_dir().join(format!("eda_xref_read_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let m = model();
    let text = export_kicad_sch(&design("${U1.2:Value}"), &m, &META).expect("the sheet exports");
    assert!(text.contains("\"XREF\" \"${/"), "the file keeps it by path");
    std::fs::write(dir.join("xref.kicad_sch"), &text).unwrap();
    std::fs::write(dir.join("xref.kicad_pro"), r#"{ "meta": { "filename": "xref.kicad_pro", "version": 3 }, "schematic": { "subpart_id_separator": 46, "subpart_first_id": 49 } }"#).unwrap();
    let (back, back_model, _) = eda_kicad::import_kicad_sch_tree(&dir.join("xref.kicad_sch")).expect("the file reads");
    let sch = back.schematic.as_ref().unwrap();
    assert_eq!(sch.user_fields.get("R3").and_then(|f| f.get("XREF")).map(String::as_str), Some("${U1.2:Value}"), "{:?}", sch.user_fields);
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        return;
    }
    let Some(cli) = find_kicad_cli() else { return };
    let mut again = back.clone();
    if let Some(s) = again.schematic.as_mut() {
        s.extras.subpart_id_separator = Some('.' as u32);
        s.extras.subpart_first_id = Some('1' as u32);
    }
    let rewritten = export_kicad_sch(&again, &back_model, &META).expect("the sheet exports again");
    std::fs::write(dir.join("xref.kicad_sch"), rewritten).unwrap();
    let out = dir.join("again.xml");
    let run = Command::new(&cli).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(dir.join("xref.kicad_sch")).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "{}", String::from_utf8_lossy(&run.stderr));
    let xml = std::fs::read_to_string(&out).unwrap();
    let field = xml.split("<field name=\"XREF\">").nth(1).and_then(|r| r.split("</field>").next()).unwrap_or_default().to_string();
    assert_eq!(field, "DUAL", "{xml}");
    let _ = std::fs::remove_dir_all(&dir);
}
