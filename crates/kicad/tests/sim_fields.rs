//! What the SPICE simulator needs of the KiCad files the studio writes (package P21): a symbol placed from a library keeps the library
//! symbol's `Sim.*` fields (KiCad copies every field of a library symbol onto a symbol it places, and `SIM_LIB_MGR::CreateModel` reads them off
//! the placed symbol), and the derived project carries the simulator's settings as `schematic.ngspice`.

use eda_kicad::{export_kicad_pro_for, export_kicad_sch, sim_fields_of, ExportMeta};
use eda_model::ir::{Design, Point, Provenance, SchematicSection, SymbolInstance};
use eda_model::sch_extras::NgspiceSettings;
use eda_model::symbol::{LibPin, LibSymbol, SPoint};
use eda_model::{ConstraintModel, Part, Pin, PinKind};

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-23", title: "sim" };

fn source() -> LibSymbol {
    let pin = |n: &str, y: f64| LibPin { number: n.into(), name: String::new(), electrical_type: "passive".into(), shape: "line".into(), at: SPoint::new(0.0, y), angle_deg: if y > 0.0 { 270.0 } else { 90.0 }, length_mm: 2.54, unit: 1 };
    LibSymbol {
        lib_id: "Simulation_SPICE:VSIN".into(),
        graphics: vec![],
        pins: vec![pin("1", 5.08), pin("2", -5.08)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "V".into(),
        unit_count: 1,
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        pin_name_offset_mm: 0.0,
        alternate: None,
    }
}

fn model() -> ConstraintModel {
    let pins = ["1", "2"].map(|n| Pin { number: n.into(), name: None, kind: PinKind::Passive }).to_vec();
    let part = Part { reference: "V1".into(), mpn: None, lcsc: None, value: Some("VSIN".into()), package: None, footprint: None, symbol: Some("Simulation_SPICE:VSIN".into()), datasheet: None, pins, body_nm: None, edge: None };
    let mut m = ConstraintModel { parts: vec![part], symbols: vec![source()], ..Default::default() };
    m.symbol_sim_fields.insert("Simulation_SPICE:VSIN".into(), vec![("Sim.Pins".into(), "1=+ 2=-".into()), ("Sim.Device".into(), "V".into()), ("Sim.Type".into(), "SIN".into()), ("Sim.Params".into(), "dc=0 ampl=1 f=1k ac=1".into())]);
    m
}

fn design(sch: SchematicSection) -> Design {
    Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        placement: None,
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    }
}

fn sheet() -> SchematicSection {
    let sym = SymbolInstance {
        id: "V1".into(),
        at: Point { x: 50_800_000, y: 50_800_000 },
        rot: 0,
        mirrored: false,
        mirror_y: false,
        lib_id: "Simulation_SPICE:VSIN".into(),
        unit: 1,
        value: "VSIN".into(),
        footprint: String::new(),
        datasheet: String::new(),
        dnp: false,
        exclude_from_bom: false,
        exclude_from_board: false,
        exclude_from_sim: false,
    };
    SchematicSection { symbols: vec![sym], ..Default::default() }
}

#[test]
fn a_placed_symbol_gets_the_library_symbols_simulation_fields_it_has_none_of() {
    let text = export_kicad_sch(&design(sheet()), &model(), &META).unwrap();
    assert!(text.contains("(property \"Sim.Device\" \"V\""), "{text}");
    assert!(text.contains("(property \"Sim.Params\" \"dc=0 ampl=1 f=1k ac=1\""));
    // one of its own wins
    let mut own = sheet();
    own.user_fields.entry("V1".into()).or_default().insert("Sim.Params".into(), "dc=0 ampl=2 f=10k ac=1".into());
    let text = export_kicad_sch(&design(own), &model(), &META).unwrap();
    assert_eq!(text.matches("(property \"Sim.Params\"").count(), 1, "{text}");
    assert!(text.contains("ampl=2"));
    // a sheet read from a KiCad file has its symbols' own fields
    let mut imported = sheet();
    imported.imported_from_kicad = true;
    let text = export_kicad_sch(&design(imported), &model(), &META).unwrap();
    assert!(!text.contains("Sim.Device"));
}

#[test]
fn a_library_files_simulation_fields_follow_extends() {
    let dir = std::env::temp_dir().join(format!("eda-sim-fields-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lib = dir.join("Sim.kicad_sym");
    std::fs::write(
        &lib,
        r#"(kicad_symbol_lib (version 20241209) (generator "test")
  (symbol "BASE" (property "Reference" "Q") (property "Sim.Device" "NPN") (property "Sim.Pins" "1=C 2=B 3=E") (property "Description" "x"))
  (symbol "DERIVED" (extends "BASE") (property "Sim.Pins" "1=B 2=C 3=E") (property "Sim.Type" "GUMMELPOON"))
)"#,
    )
    .unwrap();
    assert_eq!(sim_fields_of(&lib, "BASE"), vec![("Sim.Device".to_string(), "NPN".to_string()), ("Sim.Pins".to_string(), "1=C 2=B 3=E".to_string())]);
    let d = sim_fields_of(&lib, "DERIVED");
    assert_eq!(d.iter().find(|(k, _)| k == "Sim.Pins").unwrap().1, "1=B 2=C 3=E", "the nearest field wins");
    assert!(d.iter().any(|(k, v)| k == "Sim.Device" && v == "NPN"), "the parent's fields come along");
    assert!(sim_fields_of(&lib, "NONE").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_derived_project_carries_the_simulators_settings_as_kicad_writes_them() {
    let mut sch = sheet();
    sch.extras.ngspice = Some(NgspiceSettings { workbook_filename: "sim.wbk".into(), fix_include_paths: false, model_mode: NgspiceSettings::PSPICE });
    let pro: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design(sch), &model())).unwrap();
    assert_eq!(pro["schematic"]["ngspice"], serde_json::json!({ "fix_include_paths": false, "meta": { "version": 0 }, "model_mode": 2, "workbook_filename": "sim.wbk" }));
    let none: serde_json::Value = serde_json::from_str(&export_kicad_pro_for(&design(sheet()), &model())).unwrap();
    assert!(none["schematic"].get("ngspice").is_none(), "nothing set, nothing written");
}
