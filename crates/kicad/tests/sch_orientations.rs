//! Symbols of one library symbol turned and mirrored each its own way, through the `.kicad_sch` the studio derives. The writer places every symbol at angle 0 with its
//! library symbol baked through its turn and mirror; the cache held one entry per `lib_id`, baked through the first reference, so a second reference of that symbol
//! turned otherwise was drawn -- and connected by kicad-cli -- the first one's way (KiCad's own `issue18346.kicad_sch`, a mirrored connector, came back with its pins off
//! its wires). Now such a reference has a copy of its own in the cache (`(lib_name "<symbol>_<n>")`), as KiCad keeps a cache copy (`SCH_SCREEN::Append`).
//!
//! The slow test (`EDA_SLOW_TESTS=1`, and it skips when there is no kicad-cli) has kicad-cli read the file and derive the nets.

use eda_kicad::{export_kicad_sch, ExportMeta};
use eda_model::ir::{Design, LabelKind, NetLabel, Point, Provenance, SchematicSection, SymbolInstance, Wire};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-23", title: "orientations" };

fn pin(number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1 }
}

/// A two-input gate: A and B on the left (one grid cell apart), Y on the right.
fn gate() -> LibSymbol {
    LibSymbol {
        lib_id: "test:GATE".into(),
        graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-2.54, 5.08), end: SPoint::new(2.54, -5.08), stroke_mm: 0.254, filled: false }],
        pins: vec![pin("1", "A", "input", -7.62, 2.54, 0.0), pin("2", "B", "input", -7.62, 0.0, 0.0), pin("3", "Y", "output", 7.62, 0.0, 180.0)],
        power: false,
        in_bom: true,
        on_board: true,
        datasheet: String::new(),
        description: String::new(),
        reference_prefix: "U".into(),
        unit_count: 1,
        pin_names_hidden: false,
        pin_numbers_hidden: false,
        pin_name_offset_mm: 0.508,
        alternate: None,
    }
}

fn model() -> ConstraintModel {
    let pins = ["1", "2", "3"].map(|n| Pin { number: n.into(), name: None, kind: PinKind::Signal }).to_vec();
    let part = |reference: &str| Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: Some("GATE".into()),
        package: None,
        footprint: Some("Foo:Bar".into()),
        symbol: Some("test:GATE".into()),
        datasheet: None,
        pins: pins.clone(),
        body_nm: None,
        edge: None,
    };
    ConstraintModel { parts: ["U1", "U2", "U3", "U4", "U5", "U6"].map(part).to_vec(), symbols: vec![gate()], ..Default::default() }
}

fn design_with(sch: SchematicSection) -> Design {
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

/// U1 unturned, U2 turned a quarter, U3 mirrored left-right, U4 mirrored top-bottom, U5 turned a quarter and mirrored left-right and U6 turned three quarters
/// and mirrored top-bottom, each pin with a wire out to a label `<REF>_<PIN>` where the tracer (`eda_engine::placed`) has the pin -- which is where the
/// editor and the painter draw it (`sch_move_tests::the_editor_s_pins_of_a_turned_symbol_are_the_writer_s_in_every_orientation`).
fn sheet(m: &ConstraintModel) -> SchematicSection {
    let mut sch = SchematicSection::default();
    for (i, (reference, rot, mirrored, mirror_y)) in [("U1", 0, false, false), ("U2", 90_000, false, false), ("U3", 0, true, false), ("U4", 0, false, true), ("U5", 90_000, true, false), ("U6", 270_000, false, true)].into_iter().enumerate() {
        let sym = SymbolInstance {
            id: reference.into(),
            at: Point { x: 50_800 + 50_800 * i as i64, y: 76_200 },
            rot,
            mirrored,
            mirror_y,
            lib_id: "test:GATE".into(),
            unit: 1,
            value: "GATE".into(),
            footprint: String::new(),
            datasheet: String::new(),
            dnp: false,
            exclude_from_bom: false,
            exclude_from_board: false,
            exclude_from_sim: false,
        };
        let part = m.part(reference).unwrap();
        let lib = m.real_symbol_of(&sym.lib_id, part);
        let geom = eda_engine::symgeom::SymbolGeom::of(&sym, part, lib.as_ref());
        for (number, tip) in eda_engine::placed::pin_points(&sym, part, lib.as_ref()) {
            let net = format!("{reference}_{number}");
            let toward_body = geom.pins.iter().find(|g| g.number == number).map(|g| g.orient.dir()).expect("the pin has a geometry");
            let end = Point { x: tip.x - (toward_body.0 * 12_700.0).round() as i64, y: tip.y - (toward_body.1 * 12_700.0).round() as i64 };
            sch.wires.push(Wire { id: format!("w_{net}"), net: net.clone(), pins: vec![], pts: vec![tip, end], bus: false });
            sch.labels.push(NetLabel { id: format!("l_{net}"), net, at: end, kind: LabelKind::Local });
        }
        sch.symbols.push(sym);
    }
    sch
}

#[test]
fn a_symbol_turned_otherwise_than_the_first_of_its_library_symbol_has_a_cache_copy_of_its_own() {
    let m = model();
    let text = export_kicad_sch(&design_with(sheet(&m)), &m, &META).expect("the sheet exports");
    let instance = |reference: &str| {
        let at = text.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap();
        let start = text[..at].rfind("\t(symbol").unwrap();
        text[start..at].to_string()
    };
    assert!(!instance("U1").contains("lib_name"), "the first reference is the cache entry's: {}", instance("U1"));
    assert!(instance("U2").contains("(lib_name \"GATE_1\")"), "{}", instance("U2"));
    assert!(instance("U3").contains("(lib_name \"GATE_2\")"), "{}", instance("U3"));
    for name in ["\"test:GATE\"", "\"GATE_1\"", "\"GATE_2\""] {
        assert!(text.contains(&format!("\t\t(symbol {name}")), "the cache holds {name}");
    }
}

fn find_kicad_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).filter(|p| p.exists()) {
        return Some(p);
    }
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli")).filter(|p| p.exists())
}

/// `net name -> sorted REF.PIN` from kicad-cli's `kicadxml` netlist.
fn xml_nets(netlist: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in netlist.lines().map(str::trim) {
        if line.starts_with("<net ") {
            let name = line.split("name=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").trim_start_matches('/').to_string();
            current = Some(name.clone());
            out.entry(name).or_default();
        } else if line.starts_with("</net>") {
            current = None;
        } else if line.starts_with("<node ") {
            let attr = |n: &str| line.split(&format!("{n}=\"")).nth(1).and_then(|r| r.split('"').next()).map(String::from);
            if let (Some(name), Some(r), Some(p)) = (&current, attr("ref"), attr("pin")) {
                out.get_mut(name).unwrap().push(format!("{r}.{p}"));
            }
        }
    }
    for pins in out.values_mut() {
        pins.sort();
    }
    out
}

#[test]
fn kicad_cli_connects_each_turned_symbol_to_the_wires_on_its_own_pins() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let m = model();
    let text = export_kicad_sch(&design_with(sheet(&m)), &m, &META).expect("the sheet exports");
    let dir = std::env::temp_dir().join(format!("eda_sch_orientations_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("orientations.kicad_sch");
    std::fs::write(&path, text).unwrap();
    let out = dir.join("orientations.xml");
    let run = Command::new(&cli).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(&path).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "kicad-cli could not read the file: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let nets = xml_nets(&std::fs::read_to_string(&out).unwrap());
    for reference in ["U1", "U2", "U3", "U4", "U5", "U6"] {
        for pin in ["1", "2", "3"] {
            let net = format!("{reference}_{pin}");
            assert_eq!(nets.get(&net).map(Vec::as_slice), Some(&[format!("{reference}.{pin}")][..]), "{net}: {nets:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
