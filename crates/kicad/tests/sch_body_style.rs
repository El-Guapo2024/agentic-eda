//! A symbol placed in the alternate ("De Morgan") body style of its library symbol is drawn and exported in that style: KiCad reads the same body style and the same
//! pins where the file puts them.
//!
//! The pure-Rust test is our writer and our reader. The slow ones hand the file to kicad-cli (`EDA_SLOW_TESTS=1`, and they skip when there is none): its netlist
//! says which pin each wire reaches, which is the pin of the body style the symbol is in, and its own writer (`sch upgrade --force`) says what it understood of the
//! symbol.

use eda_kicad::{export_kicad_sch, import_kicad_sch, ExportMeta};
use eda_model::ir::{Design, LabelKind, NetLabel, Point, Provenance, SchematicSection, SymbolInstance, Wire};
use eda_model::symbol::{AlternateBody, LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::path::{Path, PathBuf};
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-08", title: "body style" };

fn pin(number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1 }
}

fn body(half_width: f64) -> SymbolGraphic {
    SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(-half_width, 5.08), end: SPoint::new(half_width, -5.08), stroke_mm: 0.254, filled: false }
}

/// A two-input gate. Its alternate body is wider and has the second input one grid cell further down, so a wire that meets pin 2 in one body misses it in the other.
fn gate() -> LibSymbol {
    LibSymbol {
        lib_id: "test:GATE".into(),
        graphics: vec![body(2.54)],
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
        alternate: Some(Box::new(AlternateBody::new(vec![body(3.81)], vec![pin("1", "A", "input", -7.62, 2.54, 0.0), pin("2", "B", "input", -7.62, -2.54, 0.0), pin("3", "Y", "output", 7.62, 0.0, 180.0)]))),
    }
}

fn model() -> ConstraintModel {
    let pins = ["1", "2", "3"].map(|n| Pin { number: n.into(), name: None, kind: PinKind::Signal }).to_vec();
    let part = Part { reference: "U1".into(), mpn: None, lcsc: None, value: Some("GATE".into()), package: None, footprint: Some("Foo:Bar".into()), symbol: Some("test:GATE".into()), datasheet: None, pins, body_um: None, edge: None };
    ConstraintModel { parts: vec![part], symbols: vec![gate()], ..Default::default() }
}

/// The gate placed in `style`, a wire from each of its pins in that style out to a label: `NET_A`, `NET_B`, `NET_Y`. With `wires_of`, the wires meet the pins of that
/// other style instead.
fn sheet(model: &ConstraintModel, style: u32, wires_of: u32) -> Design {
    let at = Point { x: 76_200, y: 76_200 };
    let sym = SymbolInstance { id: "U1".into(), at, rot: 0, mirrored: false, mirror_y: false, lib_id: "test:GATE".into(), unit: 1, value: "GATE".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
    let mut sch = SchematicSection { symbols: vec![sym.clone()], ..Default::default() };
    let part = model.part("U1").unwrap();
    let mut wired = sch.clone();
    wired.extras.body_styles.insert("U1".into(), wires_of);
    let tips = eda_engine::placed::pin_points(&sym, part, model.real_symbol_of_instance(&wired, &sym, part).as_ref());
    for (number, tip) in tips {
        let left = number != "3";
        let end = Point { x: tip.x + if left { -12_700 } else { 12_700 }, y: tip.y };
        let net = format!("NET_{}", ["A", "B", "Y"][number.parse::<usize>().unwrap() - 1]);
        sch.wires.push(Wire { id: format!("w_{net}"), net: net.clone(), pins: vec![], pts: vec![tip, end], bus: false });
        sch.labels.push(NetLabel { id: format!("l_{net}"), net, at: end, kind: LabelKind::Local });
    }
    if style > 1 {
        sch.extras.body_styles.insert("U1".into(), style);
    }
    Design { schema: 1, provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] }, schematic: Some(sch), nets: None, placement: None, routing: None, drawings: None, footprint_library: None, sheet_contents: None, bus_aliases: Default::default(), symbol_library: None }
}

#[test]
fn the_file_has_both_bodies_and_the_style_of_the_symbol() {
    let m = model();
    for style in [1, 2] {
        let text = export_kicad_sch(&sheet(&m, style, style), &m, &META).expect("the sheet exports");
        assert_eq!(text.contains("(convert 2)"), style == 2, "only a symbol in the alternate style says so");
        let (back, back_model, _) = import_kicad_sch(&text).expect("the file reads");
        let sch = back.schematic.unwrap();
        assert_eq!(sch.body_style_of(&sch.symbols[0]), style);
        assert!(back_model.symbols[0].alternate.is_some(), "the symbol keeps both bodies");
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

/// The pins of `U1` on each net, as kicad-cli's netlist says: `{"NET_A": ["1"], ...}`.
fn kicad_nets(cli: &Path, file: &Path, out: &Path) -> std::collections::BTreeMap<String, Vec<String>> {
    let run = Command::new(cli).args(["sch", "export", "netlist", "--format", "kicadsexpr", "-o"]).arg(out).arg(file).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "kicad-cli sch export netlist: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    // the netlist is indented over many lines: one space between tokens reads it
    let text = std::fs::read_to_string(out).unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
    // `(net (code "1") (name "/NET_A") ... (node (ref "U1") (pin "1") ...) ...)`: one chunk per net
    let quoted = |s: &str, key: &str| -> Option<String> {
        let at = s.find(key)? + key.len();
        let rest = &s[at..];
        Some(rest[..rest.find('"')?].to_string())
    };
    let mut nets = std::collections::BTreeMap::new();
    for chunk in text.split("(net (code").skip(1) {
        let name = quoted(chunk, "(name \"").unwrap_or_default();
        let pins: Vec<String> = chunk.split("(node (ref \"").skip(1).filter(|n| n.starts_with("U1\"")).filter_map(|n| quoted(n, "(pin \"")).collect();
        nets.insert(name.trim_start_matches('/').to_string(), pins);
    }
    nets
}

#[test]
fn kicad_reaches_the_pins_of_the_body_style_the_symbol_is_in() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let m = model();
    let dir = std::env::temp_dir().join(format!("eda_body_style_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let nets_of = |name: &str, style: u32, wires_of: u32| {
        let text = export_kicad_sch(&sheet(&m, style, wires_of), &m, &META).expect("the sheet exports");
        let path = dir.join(format!("{name}.kicad_sch"));
        std::fs::write(&path, text).unwrap();
        kicad_nets(&cli, &path, &dir.join(format!("{name}.net")))
    };
    let pins = |nets: &std::collections::BTreeMap<String, Vec<String>>, net: &str| nets.get(net).cloned().unwrap_or_default();

    // the wires meet the pins of the body the symbol is drawn in: every net reaches its pin, whichever body that is
    for style in [1, 2] {
        let nets = nets_of(&format!("met_{style}"), style, style);
        assert_eq!((pins(&nets, "NET_A"), pins(&nets, "NET_B"), pins(&nets, "NET_Y")), (vec!["1".to_string()], vec!["2".to_string()], vec!["3".to_string()]), "style {style}: {nets:?}");
    }
    // and the test can fail: wires drawn for the other body miss pin 2
    for (style, wires_of) in [(1, 2), (2, 1)] {
        let nets = nets_of(&format!("missed_{style}"), style, wires_of);
        assert!(pins(&nets, "NET_B").is_empty(), "style {style} with the wires of style {wires_of}: {nets:?}");
        assert_eq!(pins(&nets, "NET_A"), vec!["1".to_string()], "the pin both bodies share is reached either way");
    }

    // what KiCad's own writer says of the symbol: its body style and that it has two
    let path = dir.join("met_2.kicad_sch");
    let up = Command::new(&cli).args(["sch", "upgrade", "--force"]).arg(&path).output().expect("run kicad-cli");
    assert!(up.status.success(), "kicad-cli sch upgrade: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let kicads = std::fs::read_to_string(&path).unwrap();
    assert!(kicads.contains("(body_style 2)"), "KiCad reads the instance as being in body style 2");
    assert!(kicads.contains("(body_styles demorgan)"), "and the symbol as one with a De Morgan pair");
    let _ = std::fs::remove_dir_all(&dir);
}
