//! Schematic details (package P10) in the `.kicad_sch` the studio writes: pins that Swap Pins moved (a cached copy of the symbol with `lib_name`, as KiCad
//! writes it), a sheet pin moved along the edge of its sheet and a justified text that was mirrored.
//!
//! The pure-Rust tests are our writer. The slow ones hand the file to kicad-cli (`EDA_SLOW_TESTS=1`, and they skip when there is none): it loads every file and runs
//! its ERC, and its netlist says which pin each wire reaches -- the swapped pins are on each other's nets there too.

use eda_kicad::{export_kicad_sch, ExportMeta};
use eda_model::ir::{Design, LabelKind, NetLabel, Point, Provenance, SchematicSection, SymbolInstance, Wire};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-11", title: "schematic details" };

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
    let part = Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("GATE".into()), package: None, footprint: Some("Foo:Bar".into()), symbol: Some("test:GATE".into()), datasheet: None, pins, body_um: None, edge: None };
    ConstraintModel { parts: vec![part], symbols: vec![gate()], ..Default::default() }
}

fn design_with(sch: SchematicSection) -> Design {
    Design { schema: 1, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None }
}

/// The gate with a wire from each pin (where the library puts it) out to a label: `NET_A`, `NET_B`, `NET_Y`; with `swapped`, pins 1 and 2 have traded places.
fn gate_sheet(m: &ConstraintModel, swapped: bool) -> SchematicSection {
    let sym = SymbolInstance { id: "U1".into(), at: Point { x: 76_200, y: 76_200 }, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:GATE".into(), unit: 1, value: "GATE".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
    let mut sch = SchematicSection { symbols: vec![sym.clone()], ..Default::default() };
    let part = m.part("U1").unwrap();
    for (number, tip) in eda_engine::placed::pin_points(&sym, part, m.real_symbol_of(&sym.lib_id, part).as_ref()) {
        let left = number != "3";
        let end = Point { x: tip.x + if left { -12_700 } else { 12_700 }, y: tip.y };
        let net = format!("NET_{}", ["A", "B", "Y"][number.parse::<usize>().unwrap() - 1]);
        sch.wires.push(Wire { id: format!("w_{net}"), net: net.clone(), pins: vec![], pts: vec![tip, end], bus: false });
        sch.labels.push(NetLabel { id: format!("l_{net}"), net, at: end, kind: LabelKind::Local });
    }
    if swapped {
        sch.extras.swap_pin_places("U1", "1", "2");
    }
    sch
}

/// The `(at x y angle)` of pin `number` in the cached symbol `name` of `text`.
fn cached_pin_at(text: &str, name: &str, number: &str) -> String {
    let start = text.find(&format!("\t\t(symbol \"{name}\"")).unwrap_or_else(|| panic!("no cached symbol {name}"));
    let entry = &text[start..];
    let end = entry[3..].find("\n\t\t(symbol ").map_or(entry.len(), |i| i + 3).min(entry.find("\n\t)\n").unwrap_or(entry.len()));
    let entry = &entry[..end];
    let at = entry.find(&format!("(number \"{number}\"")).unwrap_or_else(|| panic!("no pin {number} in {name}"));
    let pin_start = entry[..at].rfind("(pin ").unwrap();
    let from = pin_start + entry[pin_start..].find("(at ").unwrap();
    entry[from..from + entry[from..].find(')').unwrap() + 1].to_string()
}

#[test]
fn swapped_pins_are_written_as_the_cached_copy_of_the_symbol_with_a_lib_name() {
    let m = model();
    let plain = export_kicad_sch(&design_with(gate_sheet(&m, false)), &m, &META).expect("the sheet exports");
    let swapped = export_kicad_sch(&design_with(gate_sheet(&m, true)), &m, &META).expect("the sheet exports");
    assert!(!plain.contains("lib_name"), "a symbol with its library's pins needs no copy of its own");
    // the instance names its copy and its library symbol, as KiCad writes them
    assert!(swapped.contains("(symbol (lib_name \"GATE_1\") (lib_id \"test:GATE\")"), "{}", swapped.lines().filter(|l| l.contains("lib_id")).collect::<Vec<_>>().join("\n"));
    // the copy has pin 1 where pin 2 is in the library symbol, and pin 2 where pin 1 is; pin 3 stays
    let (a1, a2, a3) = (cached_pin_at(&plain, "test:GATE", "1"), cached_pin_at(&plain, "test:GATE", "2"), cached_pin_at(&plain, "test:GATE", "3"));
    assert_ne!(a1, a2);
    assert_eq!(cached_pin_at(&swapped, "GATE_1", "1"), a2);
    assert_eq!(cached_pin_at(&swapped, "GATE_1", "2"), a1);
    assert_eq!(cached_pin_at(&swapped, "GATE_1", "3"), a3);
    // the library symbol of that name is still there, unchanged, for any other instance
    assert_eq!(cached_pin_at(&swapped, "test:GATE", "1"), a1);
    // the pins of the instance are still listed by number
    assert!(swapped.contains("(pin \"1\" (uuid") && swapped.contains("(pin \"2\" (uuid"));
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

/// The pins of `U1` on each net, as kicad-cli's netlist says: `{"NET_A": ["2"], ...}`.
fn kicad_nets(cli: &Path, file: &Path, out: &Path) -> BTreeMap<String, Vec<String>> {
    let run = Command::new(cli).args(["sch", "export", "netlist", "--format", "kicadsexpr", "-o"]).arg(out).arg(file).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "kicad-cli sch export netlist: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let text = std::fs::read_to_string(out).unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
    let quoted = |s: &str, key: &str| -> Option<String> {
        let at = s.find(key)? + key.len();
        let rest = &s[at..];
        Some(rest[..rest.find('"')?].to_string())
    };
    let mut nets = BTreeMap::new();
    for chunk in text.split("(net (code").skip(1) {
        let name = quoted(chunk, "(name \"").unwrap_or_default();
        let pins: Vec<String> = chunk.split("(node (ref \"").skip(1).filter(|n| n.starts_with("U1\"")).filter_map(|n| quoted(n, "(pin \"")).collect();
        nets.insert(name.trim_start_matches('/').to_string(), pins);
    }
    nets
}

/// `kicad-cli sch erc` on `file`: the report, which exists only when the file loaded.
fn erc(cli: &Path, file: &Path) -> serde_json::Value {
    let report = file.with_extension("erc.json");
    let out = Command::new(cli).args(["sch", "erc", "--format", "json", "--output"]).arg(&report).arg(file).output().expect("run kicad-cli");
    assert!(report.exists(), "kicad-cli did not produce an ERC report for {}: {}{}", file.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).expect("the ERC report is JSON")
}

#[test]
fn kicad_loads_the_swapped_pins_and_reaches_the_pins_where_the_wires_are() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let m = model();
    let dir = std::env::temp_dir().join(format!("eda_sch_details_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let nets_of = |name: &str, swapped: bool| {
        let text = export_kicad_sch(&design_with(gate_sheet(&m, swapped)), &m, &META).expect("the sheet exports");
        let path = dir.join(format!("{name}.kicad_sch"));
        std::fs::write(&path, text).unwrap();
        // the ERC runs on it: the file loads
        let report = erc(&cli, &path);
        assert!(report.get("sheets").is_some(), "{name}: {report}");
        kicad_nets(&cli, &path, &dir.join(format!("{name}.net")))
    };
    let pins = |nets: &BTreeMap<String, Vec<String>>, net: &str| nets.get(net).cloned().unwrap_or_default();
    // as the library has them, the wire of NET_A meets pin 1; with pins 1 and 2 swapped it meets pin 2, and NET_B meets pin 1
    let plain = nets_of("plain", false);
    assert_eq!((pins(&plain, "NET_A"), pins(&plain, "NET_B"), pins(&plain, "NET_Y")), (vec!["1".to_string()], vec!["2".to_string()], vec!["3".to_string()]), "{plain:?}");
    let swapped = nets_of("swapped", true);
    assert_eq!((pins(&swapped, "NET_A"), pins(&swapped, "NET_B"), pins(&swapped, "NET_Y")), (vec!["2".to_string()], vec!["1".to_string()], vec!["3".to_string()]), "{swapped:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
