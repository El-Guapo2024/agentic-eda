//! A Copy's text is KiCad's: kicad-cli (the same board reader pcbnew's Paste uses) loads it as a board and finds every item
//! where the copy put it. Skipped, loudly, when there is no kicad-cli.

use eda_model::units::mm;
use eda_model::units::Nm;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_kicad::{export_pcb_clipboard, parse_pcb_clipboard};
use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Via};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};

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
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_clip_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// kicad-cli's DRC on `pcb`, as JSON.
fn drc(cli: &Path, dir: &Path, pcb: &Path) -> serde_json::Value {
    let report = dir.join("report.json");
    let out = Command::new(cli).args(["pcb", "drc", "--format", "json", "--severity-all", "--output"]).arg(&report).arg(pcb).output().expect("kicad-cli runs");
    assert!(report.exists(), "kicad-cli wrote no report -- it could not read the clipboard text as a board: {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap()
}

#[test]
fn kicad_cli_reads_a_copy_as_a_board_and_finds_the_items_where_the_copy_put_them() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: Some("100n".into()), package: Some("0603".into()), footprint: Some("0603".into()), pins: ["1", "2"].iter().map(|n| Pin { number: (*n).into(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, symbol: None, datasheet: None, edge: None };
    let model = ConstraintModel {
        parts: vec![part("C1")],
        nets: vec![Net { name: "VIN".into(), pins: vec!["C1.1".into()] }, Net { name: "GND".into(), pins: vec!["C1.2".into()] }],
        ..Default::default()
    };
    // Two tracks of different nets side by side, a via on one of them, a part elsewhere.
    let mut design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![], footprints: vec![FootprintInstance { id: "C1".into(), at: p(mm(150.0), mm(140.0)), rot: 0, side: Side::Top, label: LabelSide::Above }], modules: vec![] }),
        routing: Some(RoutingSection {
            tracks: vec![
                Track { id: "trk_h".into(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.25), pts: vec![p(mm(120.0), mm(120.0)), p(mm(140.0), mm(120.0))], arc_mid_offset: None },
                Track { id: "trk_v".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.25), pts: vec![p(mm(160.0), mm(110.0)), p(mm(160.0), mm(130.0))], arc_mid_offset: None },
            ],
            vias: vec![Via { id: "via_a".into(), net: "VIN".into(), at: p(mm(140.0), mm(120.0)), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
            zones: vec![],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: None,
    };
    design.assign_missing_ids();

    // Copied from a reference point at (100, 100) mm: everything moves by (-100, -100) mm.
    let text = export_pcb_clipboard(&design, &model, &["trk_h".into(), "trk_v".into(), "via_a".into(), "C1".into()], p(mm(100.0), mm(100.0))).unwrap();
    let dir = scratch("board");
    let pcb = dir.join("clip.kicad_pcb");
    std::fs::write(&pcb, &text).unwrap();
    let report = drc(&cli, &dir, &pcb);

    // Every item KiCad names -- in a violation or an unconnected pair -- with where it sits.
    let mut seen: Vec<(String, f64, f64)> = Vec::new();
    for section in ["violations", "unconnected_items"] {
        for v in report[section].as_array().unwrap() {
            for i in v["items"].as_array().unwrap() {
                seen.push((i["description"].as_str().unwrap().to_string(), i["pos"]["x"].as_f64().unwrap(), i["pos"]["y"].as_f64().unwrap()));
            }
        }
    }
    let at = |what: &str, x: f64, y: f64| seen.iter().any(|(d, px, py)| d.starts_with(what) && (px - x).abs() < 0.01 && (py - y).abs() < 0.01);
    // The copy's reference point is the origin, and each item's net came across by the table in the text.
    assert!(at("Track [VIN] on F.Cu", 20.0, 20.0), "{seen:?}");
    assert!(at("Track [GND] on F.Cu", 60.0, 10.0), "{seen:?}");
    assert!(at("Via [VIN]", 40.0, 20.0), "{seen:?}");
    // The footprint itself is named in no violation (it is named for no library rather than for an `eda` library KiCad has not got): its two pads, half a
    // pitch either side of it, put it at (50, 40), where the copy put it.
    assert!(at("Pad 1 [VIN] of C1", 49.175, 40.0), "the footprint's pads kept their nets: {seen:?}");
    assert!(at("Pad 2 [GND] of C1", 50.825, 40.0), "{seen:?}");

    // The same text is what our own reader takes back.
    let clip = parse_pcb_clipboard(&text).unwrap();
    assert_eq!((clip.tracks.len(), clip.vias.len(), clip.footprints.len()), (2, 1, 1));
    assert_eq!(clip.footprints[0].at, p(mm(50.0), mm(40.0)));
    assert_eq!(clip.vias[0].at, p(mm(40.0), mm(20.0)));
    assert_eq!(clip.footprints[0].pad_nets, vec![("1".to_string(), "VIN".to_string()), ("2".to_string(), "GND".to_string())]);
}

/// KiCad's own writer, run over our copy (`kicad-cli pcb upgrade --force` re-saves a board in KiCad 10's format, nets by name
/// and all), gives text our reader takes back as the same items: a clipboard from a running KiCad parses here.
#[test]
fn what_kicad_itself_writes_for_those_items_reads_back_the_same() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: Some("100n".into()), package: Some("0603".into()), footprint: Some("0603".into()), pins: ["1", "2"].iter().map(|n| Pin { number: (*n).into(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, symbol: None, datasheet: None, edge: None };
    let model = ConstraintModel {
        parts: vec![part("C1")],
        nets: vec![Net { name: "VIN".into(), pins: vec!["C1.1".into()] }, Net { name: "GND".into(), pins: vec!["C1.2".into()] }],
        ..Default::default()
    };
    let mut design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![], footprints: vec![FootprintInstance { id: "C1".into(), at: p(mm(150.0), mm(140.0)), rot: 90_000, side: Side::Top, label: LabelSide::Above }], modules: vec![] }),
        routing: Some(RoutingSection {
            tracks: vec![
                Track { id: "trk_h".into(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.25), pts: vec![p(mm(120.0), mm(120.0)), p(mm(140.0), mm(120.0))], arc_mid_offset: None },
                Track::new_arc("GND".into(), "B.Cu".into(), mm(0.3), p(mm(160.0), mm(110.0)), p(mm(165.0), mm(115.0)), p(mm(160.0), mm(120.0))),
            ],
            vias: vec![Via { id: "via_a".into(), net: "VIN".into(), at: p(mm(140.0), mm(120.0)), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
            zones: vec![],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: None,
    };
    design.assign_missing_ids();
    let arc_id = design.routing.as_ref().unwrap().tracks.iter().find(|t| t.arc().is_some()).unwrap().id.clone();
    let text = export_pcb_clipboard(&design, &model, &["trk_h".into(), arc_id, "via_a".into(), "C1".into()], p(mm(100.0), mm(100.0))).unwrap();

    let dir = scratch("upgrade");
    let pcb = dir.join("clip.kicad_pcb");
    std::fs::write(&pcb, &text).unwrap();
    let out = Command::new(&cli).args(["pcb", "upgrade", "--force"]).arg(&pcb).output().expect("kicad-cli runs");
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let written_by_kicad = std::fs::read_to_string(&pcb).unwrap();
    assert!(written_by_kicad.contains("(net \"VIN\")"), "KiCad 10 names the net on each item: {}", &written_by_kicad[..written_by_kicad.len().min(1500)]);

    let clip = parse_pcb_clipboard(&written_by_kicad).unwrap();
    assert_eq!(clip.tracks.len(), 2);
    let straight = clip.tracks.iter().find(|t| t.arc().is_none()).unwrap();
    assert_eq!((straight.net.as_str(), straight.layer.as_str(), straight.width, straight.pts.clone()), ("VIN", "F.Cu", mm(0.25), vec![p(mm(20.0), mm(20.0)), p(mm(40.0), mm(20.0))]));
    let arc = clip.tracks.iter().find(|t| t.arc().is_some()).unwrap();
    assert_eq!((arc.net.as_str(), arc.layer.as_str(), arc.arc()), ("GND", "B.Cu", Some((p(mm(60.0), mm(10.0)), p(mm(65.0), mm(15.0)), p(mm(60.0), mm(20.0))))));
    assert_eq!((clip.vias[0].net.as_str(), clip.vias[0].at, clip.vias[0].diameter, clip.vias[0].drill), ("VIN", p(mm(40.0), mm(20.0)), mm(0.6), mm(0.3)));
    let c1 = &clip.footprints[0];
    assert_eq!((c1.reference.as_str(), c1.at, c1.rot, c1.side), ("C1", p(mm(50.0), mm(40.0)), 90_000, Side::Top));
    assert_eq!(c1.pad_nets, vec![("1".to_string(), "VIN".to_string()), ("2".to_string(), "GND".to_string())]);
}

/// r2-pcb-edit C2: a Copy of one pad, a footprint's field and a barcode is KiCad's `SaveSelection` text: the pad in a footprint of its own on its net, the
/// field as a free text with the field's text, the barcode as itself. kicad-cli loads it and writes back all three.
#[test]
fn a_copied_pad_field_and_barcode_load_in_kicad() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let part = |r: &str| Part { reference: r.into(), mpn: None, lcsc: None, value: Some("100n".into()), package: Some("0603".into()), footprint: Some("0603".into()), pins: ["1", "2"].iter().map(|n| Pin { number: (*n).into(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, symbol: None, datasheet: None, edge: None };
    let model = ConstraintModel {
        parts: vec![part("C1")],
        nets: vec![Net { name: "VIN".into(), pins: vec!["C1.1".into()] }, Net { name: "GND".into(), pins: vec!["C1.2".into()] }],
        ..Default::default()
    };
    let barcode = eda_model::barcode::Barcode {
        id: "bar_a".into(),
        at: p(mm(120.0), mm(120.0)),
        angle: 0,
        layer: "F.SilkS".into(),
        width_nm: mm(5.0),
        height_nm: mm(5.0),
        text: "HELLO".into(),
        text_height_nm: mm(1.0),
        kind: eda_model::barcode::BarcodeKind::Qr,
        ecc: eda_model::barcode::EccLevel::L,
        show_text: true,
        knockout: false,
        margin_x_nm: Nm::ZERO,
        margin_y_nm: Nm::ZERO,
    };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![], footprints: vec![FootprintInstance { id: "C1".into(), at: p(mm(150.0), mm(140.0)), rot: 0, side: Side::Top, label: LabelSide::Above }], modules: vec![] }),
        routing: None,
        drawings: Some(eda_model::ir::DrawingsSection { barcodes: vec![barcode], ..Default::default() }),
    };
    let text = export_pcb_clipboard(&design, &model, &["C1.1".into(), "C1:Value".into(), "bar_a".into()], p(mm(100.0), mm(100.0))).unwrap();
    let dir = scratch("pad_field_barcode");
    let pcb = dir.join("clip.kicad_pcb");
    std::fs::write(&pcb, &text).unwrap();
    let out = Command::new(&cli).args(["pcb", "upgrade", "--force"]).arg(&pcb).output().expect("kicad-cli runs");
    assert!(out.status.success(), "{}\n{}\n{text}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let kicad = std::fs::read_to_string(&pcb).unwrap();
    assert_eq!(kicad.matches("(pad \"").count(), 1, "one pad: {kicad}");
    assert!(kicad.contains("(net \"VIN\")"), "the pad on its net: {kicad}");
    assert!(kicad.contains("(gr_text \"100n\""), "the Value field as a free text: {kicad}");
    assert!(kicad.contains("(barcode"), "{kicad}");

    let clip = parse_pcb_clipboard(&kicad).unwrap();
    assert_eq!((clip.footprints.len(), clip.texts.len(), clip.barcodes.len()), (1, 1, 1));
    assert_eq!(clip.footprints[0].pad_nets, vec![("1".to_string(), "VIN".to_string())]);
    assert_eq!(clip.barcodes[0].at, p(mm(20.0), mm(20.0)));
}
