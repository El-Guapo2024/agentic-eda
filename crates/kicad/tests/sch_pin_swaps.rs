//! Pins that Swap Pins moved, through the `.kicad_sch` and the clipboard (package P10b, step 3): a placed symbol whose pins are not where its library symbol has them keeps a copy of
//! its library symbol of its own (`(lib_name "<symbol>_<n>")`, `SCH_SCREEN::Append`), with the places of two pins traded (`SwapPinGeometry`). The writer has always written that copy
//! of a sheet's symbols; the reader now reads it back as a swap (`SchExtras::pin_swaps`), so the wires are on the pins they were on, and the clipboard writes it too, so a Copy, a
//! Paste and a Duplicate of the symbol give the copy the same pins -- here and in KiCad.
//!
//! The slow test (`EDA_SLOW_TESTS=1`, and it skips when there is no kicad-cli) has KiCad read the clipboard text and derive the nets.

use eda_model::units::mm;
use eda_model::units::Nm;
use eda_kicad::{export_kicad_sch, import_kicad_sch, parse_clipboard, write_clipboard, CopyInput, ExportMeta};
use eda_model::ir::{Design, LabelKind, NetLabel, Point, Provenance, SchematicSection, SymbolInstance, Wire};
use eda_model::symbol::{LibPin, LibSymbol, SPoint, SymbolGraphic};
use eda_model::{ConstraintModel, Part, Pin, PinKind};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

const META: ExportMeta<'static> = ExportMeta { date: "2026-10-15", title: "pin swaps" };

fn pin(number: &str, name: &str, etype: &str, x: f64, y: f64, angle: f64) -> LibPin {
    LibPin { number: number.into(), name: name.into(), electrical_type: etype.into(), shape: "line".into(), at: SPoint::new(x, y), angle_deg: angle, length_mm: 2.54, unit: 1, hidden: false }
}

/// A two-input gate: A and B on the left (one grid cell apart), Y on the right.
fn gate() -> LibSymbol {
    LibSymbol { connectivity: Default::default(),
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
    let part = Part {
        reference: "U1".into(),
        mpn: None,
        lcsc: None,
        value: Some("GATE".into()),
        package: None,
        footprint: Some("Foo:Bar".into()),
        symbol: Some("test:GATE".into()),
        datasheet: None,
        pins,
        body_nm: None,
        edge: None,
    };
    ConstraintModel { parts: vec![part], symbols: vec![gate()], ..Default::default() }
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

/// The gate `U1` turned `rot`, with a wire from each pin (where the library puts it) out to a label (`NET_A`, `NET_B`, `NET_Y`); with `swapped`, pins 1 and 2 have traded places, so pin 1
/// now sits at the end of the wire of `NET_B` and pin 2 at that of `NET_A`.
fn sheet(m: &ConstraintModel, rot: u32, swapped: bool) -> SchematicSection {
    let sym = SymbolInstance {
        id: "U1".into(),
        at: Point { x: mm(76.2), y: mm(76.2) },
        rot,
        mirrored: false,
        mirror_y: false,
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
    let mut sch = SchematicSection { symbols: vec![sym.clone()], ..Default::default() };
    let part = m.part("U1").unwrap();
    let lib = m.real_symbol_of(&sym.lib_id, part);
    // the pins where the library puts them (the symbol turned, none swapped yet), each with a wire out of the symbol along its line
    let geom = eda_engine::symgeom::SymbolGeom::of(&sym, part, lib.as_ref());
    for (number, tip) in eda_engine::placed::pin_points(&sym, part, lib.as_ref()) {
        let net = format!("NET_{}", ["A", "B", "Y"][number.parse::<usize>().unwrap() - 1]);
        let toward_body = geom.pins.iter().find(|g| g.number == number).map(|g| g.orient.dir()).expect("the pin has a geometry");
        let end = Point { x: tip.x - Nm::kiround(toward_body.0 * 12_700_000.0), y: tip.y - Nm::kiround(toward_body.1 * 12_700_000.0) };
        sch.wires.push(Wire { id: format!("w_{net}"), net: net.clone(), pins: vec![], pts: vec![tip, end], bus: false });
        sch.labels.push(NetLabel { id: format!("l_{net}"), net, at: end, kind: LabelKind::Local });
    }
    if swapped {
        sch.extras.swap_pin_places("U1", "1", "2");
    }
    sch
}

/// The pins on each net: net name -> sorted `REF.PIN`.
fn nets_of(model: &ConstraintModel) -> BTreeMap<String, Vec<String>> {
    model.nets.iter().map(|n| (n.name.trim_start_matches('/').to_string(), { let mut p = n.pins.clone(); p.sort(); p })).collect()
}

// ------------------------------------------------------------------------------------------------------------------------------ the file

#[test]
fn a_file_with_swapped_pins_reads_back_with_the_swap_and_the_wires_on_the_pins_they_were_on() {
    let m = model();
    for rot in [0, 90_000, 180_000, 270_000] {
        let text = export_kicad_sch(&design_with(sheet(&m, rot, true)), &m, &META).expect("the sheet exports");
        assert!(text.contains("(lib_name \"GATE_1\")"), "rot {rot}: the instance names its own copy");
        let (design, model, _) = import_kicad_sch(&text).unwrap_or_else(|e| panic!("rot {rot}: {e:?}"));
        let back = design.schematic.expect("a sheet");
        // pin 1 took the place of pin 2 and pin 2 the place of pin 1; pin 3 stays
        let swaps = back.extras.pin_swaps.get("U1").unwrap_or_else(|| panic!("rot {rot}: no swap read: {:?}", back.extras.pin_swaps));
        assert_eq!(swaps.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(), [("1", "2"), ("2", "1")], "rot {rot}");
        // so the wire of NET_B is on pin 1 and that of NET_A on pin 2
        let nets = nets_of(&model);
        assert_eq!(nets.get("NET_B").map(Vec::as_slice), Some(&["U1.1".to_string()][..]), "rot {rot}: {nets:?}");
        assert_eq!(nets.get("NET_A").map(Vec::as_slice), Some(&["U1.2".to_string()][..]), "rot {rot}: {nets:?}");
        assert_eq!(nets.get("NET_Y").map(Vec::as_slice), Some(&["U1.3".to_string()][..]), "rot {rot}: {nets:?}");
    }
}

#[test]
fn a_symbol_with_its_library_pins_reads_with_no_swap() {
    let m = model();
    let plain = export_kicad_sch(&design_with(sheet(&m, 0, false)), &m, &META).unwrap();
    assert!(!plain.contains("lib_name"));
    let (design, model, _) = import_kicad_sch(&plain).unwrap();
    assert!(design.schematic.unwrap().extras.pin_swaps.is_empty());
    let nets = nets_of(&model);
    assert_eq!(nets.get("NET_A").map(Vec::as_slice), Some(&["U1.1".to_string()][..]));
    assert_eq!(nets.get("NET_B").map(Vec::as_slice), Some(&["U1.2".to_string()][..]));
}

#[test]
fn what_is_written_again_is_the_same_cached_copy() {
    // the swap survives a second round trip: read, written and read again
    let m = model();
    let first = export_kicad_sch(&design_with(sheet(&m, 0, true)), &m, &META).unwrap();
    let (design, model2, _) = import_kicad_sch(&first).unwrap();
    let second = export_kicad_sch(&design, &model2, &META).expect("the sheet reads and writes again");
    assert!(second.contains("(lib_name \"GATE_1\")"), "the swap is written again as a copy of its own");
    let (again, _, _) = import_kicad_sch(&second).unwrap();
    assert_eq!(again.schematic.unwrap().extras.pin_swaps.get("U1").map(|m| m.len()), Some(2));
}

// ------------------------------------------------------------------------------------------------------------------------- the clipboard

fn copy_of(m: &ConstraintModel, sch: &SchematicSection, ids: &[&str]) -> String {
    let design = design_with(sch.clone());
    write_clipboard(&CopyInput { design: &design, model: m, section: sch, project: "demo", sheet_path: &[] }, &ids.iter().map(|s| s.to_string()).collect::<Vec<_>>()).expect("the selection copies").text
}

#[test]
fn the_clipboard_carries_the_swapped_pins_as_the_symbols_own_copy() {
    let m = model();
    let swapped = sheet(&m, 0, true);
    let text = copy_of(&m, &swapped, &["U1", "w_NET_A", "w_NET_B", "w_NET_Y"]);
    assert!(text.contains("(lib_name \"GATE_1\")"), "{}", text.lines().filter(|l| l.contains("lib_")).collect::<Vec<_>>().join("\n"));
    // the library symbol and the copy are both in the cache; the copy has pin 1 where the library has pin 2
    assert!(text.contains("(symbol \"test:GATE\"") && text.contains("(symbol \"GATE_1\""));
    let fragment = parse_clipboard(&text).expect("the clipboard reads");
    let swaps = fragment.section.extras.pin_swaps.get("U1").expect("the swap is read from the clipboard");
    assert_eq!(swaps.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(), [("1", "2"), ("2", "1")]);
    // a symbol with its library's pins copies with no copy of its own
    let plain = copy_of(&m, &sheet(&m, 0, false), &["U1", "w_NET_A"]);
    assert!(!plain.contains("lib_name"));
    assert!(parse_clipboard(&plain).unwrap().section.extras.pin_swaps.is_empty());
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
fn kicad_reads_the_copy_of_a_symbol_with_swapped_pins_with_the_wires_on_the_pins_they_were_on() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let m = model();
    let swapped = sheet(&m, 0, true);
    let text = copy_of(&m, &swapped, &["U1", "w_NET_A", "w_NET_B", "w_NET_Y", "l_NET_A", "l_NET_B", "l_NET_Y"]);
    // `Paste` hands every pasted symbol the instance path of the sheet it lands on; a file needs it written out.
    let root = "11111111-1111-4111-8111-111111111111";
    let text = text.replace("(path \"\"", &format!("(path \"/{root}\""));
    let file = format!("(kicad_sch (version 20260326) (generator \"eeschema\") (generator_version \"10.0\") (uuid \"{root}\") (paper \"A4\")\n{text}\n)\n");
    let dir = std::env::temp_dir().join(format!("eda_sch_pin_swaps_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fragment.kicad_sch");
    if let Some(keep) = std::env::var_os("EDA_KEEP_FRAGMENT") {
        let _ = std::fs::write(keep, &file);
    }
    std::fs::write(&path, file).unwrap();
    let out = dir.join("fragment.xml");
    let run = Command::new(&cli).args(["sch", "export", "netlist", "--format", "kicadxml", "-o"]).arg(&out).arg(&path).output().expect("run kicad-cli");
    assert!(run.status.success() && out.exists(), "kicad-cli could not read the fragment: {}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let nets = xml_nets(&std::fs::read_to_string(&out).unwrap());
    // KiCad reads the symbol's own copy: pin 1 is where pin 2 was, on the wire of NET_B
    assert_eq!(nets.get("NET_B").map(Vec::as_slice), Some(&["U1.1".to_string()][..]), "{nets:?}");
    assert_eq!(nets.get("NET_A").map(Vec::as_slice), Some(&["U1.2".to_string()][..]), "{nets:?}");
    assert_eq!(nets.get("NET_Y").map(Vec::as_slice), Some(&["U1.3".to_string()][..]), "{nets:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

