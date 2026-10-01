//! Optional integration test: routes `examples/ldo.yaml` with seed 3 through
//! schematic -> placement -> routing, exports both `.kicad_sch` and
//! `.kicad_pcb`, and runs `kicad-cli pcb drc` on the result. Only runs when
//! `kicad-cli` is present. `#[ignore]` by default — run with
//! `cargo test -p eda-kicad --test kicad_cli_drc -- --ignored --nocapture`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_pcb, export_kicad_sch, ExportMeta};
use eda_model::footprint::placed_pads;
use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Um};
use eda_model::{ConstraintModel, Net, Part, Pin, PinKind};
use eda_place::{place, PlaceOptions};

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    if mac.exists() {
        return Some(mac);
    }
    None
}

// Violation types genuinely in scope for our router/placer/exporter to get
// right. Everything else (courtyards, silkscreen, footprint library
// mismatches, etc.) is printed for visibility but not asserted on.
const IN_SCOPE: &[&str] = &["clearance", "track_width", "shorting_items", "unconnected_items"];

#[test]
#[ignore]
fn kicad_cli_drc_ldo_seed3() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/kicad -> repo root");
    let yaml_path = repo_root.join("examples/ldo.yaml");
    let text = std::fs::read_to_string(&yaml_path).expect("read examples/ldo.yaml");
    let model: ConstraintModel = serde_yaml::from_str(&text).expect("parse ldo.yaml");

    let seed = 3u64;
    let opts = EngineOptions { seed, intent_hash: "kicad_cli_drc_test".into(), ..Default::default() };
    let design = derive_schematic(&model, &opts).expect("derive_schematic");
    let placed = place(&design, &model, &PlaceOptions { seed, ..Default::default() }).expect("place");
    let routed = match route(&placed, &model, &model.board, seed) {
        Ok(r) => r,
        Err(checks) => {
            // Pre-existing repo issue, not a KiCad-exporter bug: `examples/ldo.yaml`'s
            // U1 (SOT-223, 4 pins incl. VOUT_TAB) references pin "4", but
            // `eda_model::footprint::builtin("SOT-223")` only defines 3 pads (the
            // tab's second copper pad is merged into pin 2), so `route()` can't
            // resolve U1.4's pad and fails its precondition before routing even
            // starts. This is a model/intent mismatch upstream of everything this
            // task is scoped to touch (crates/kicad, crates/eda re-exports,
            // crates/cli export hook, bench/kicad) — printed in full and failed
            // loudly rather than silently worked around.
            panic!("route() failed on examples/ldo.yaml seed {seed} (pre-existing footprint/intent mismatch, not a KiCad exporter issue):\n{checks:#?}");
        }
    };

    let meta = ExportMeta { date: "2026-01-01", title: "ldo_drc_test" };
    let sch_text = export_kicad_sch(&routed, &model, &meta).expect("export_kicad_sch");
    let pcb_text = export_kicad_pcb(&routed, &model, &meta).expect("export_kicad_pcb");

    let dir = std::env::temp_dir().join("eda_kicad_drc_test");
    std::fs::create_dir_all(&dir).unwrap();
    let sch_path = dir.join("ldo_drc_test.kicad_sch");
    let pcb_path = dir.join("ldo_drc_test.kicad_pcb");
    std::fs::write(&sch_path, &sch_text).unwrap();
    std::fs::write(&pcb_path, &pcb_text).unwrap();

    // ERC too, for completeness/visibility (not asserted here — kicad_cli_erc.rs owns that).
    let erc_report = dir.join("erc.json");
    let erc_out = Command::new(&cli)
        .args(["sch", "erc", "--format", "json", "--output"])
        .arg(&erc_report)
        .arg(&sch_path)
        .output()
        .expect("failed to run kicad-cli sch erc");
    eprintln!("ERC stdout:\n{}\nERC stderr:\n{}", String::from_utf8_lossy(&erc_out.stdout), String::from_utf8_lossy(&erc_out.stderr));

    let by_type = drc_by_type(&cli, &pcb_path);

    let mut in_scope_failures = Vec::new();
    for ty in IN_SCOPE {
        if let Some(vs) = by_type.get(*ty) {
            for v in vs {
                in_scope_failures.push(format!("[{ty}] {}", serde_json::to_string(v).unwrap_or_default()));
            }
        }
    }

    if !in_scope_failures.is_empty() {
        eprintln!("IN-SCOPE DRC VIOLATIONS ({} total):", in_scope_failures.len());
        for f in &in_scope_failures {
            eprintln!("  {f}");
        }
    }
    let out_of_scope: usize = by_type.iter().filter(|(k, _)| !IN_SCOPE.contains(&k.as_str())).map(|(_, v)| v.len()).sum();
    if out_of_scope > 0 {
        println!("(also {out_of_scope} out-of-scope violations — not asserted on, see stdout above for detail)");
    }

    assert!(in_scope_failures.is_empty(), "{} in-scope DRC violations found:\n{}", in_scope_failures.len(), in_scope_failures.join("\n"));
}

/// A bottom-side part's pads have to land in KiCad where the engine put
/// them. `eda_model::footprint::to_board` -- under `placed_pads`, so under
/// the placer, the router and the gates -- mirrors a bottom-side part's
/// local x before rotating it. KiCad mirrors nothing when it loads a
/// footprint: it rotates the pad's `(at x y)` by the footprint angle and
/// translates it, whatever the layer. Exported with the local x as-is, a
/// bottom SOT-23's pin 1 lands in pin 3's column and a bottom 0603's pins
/// swap places, nets and all.
///
/// The tracks are drawn by hand between `placed_pads` centres, no router,
/// so the export is the only thing under test: every net has to come out
/// connected, with no track dangling or running over another net's pad.
/// The bottom parts' reference and value text has to come out mirrored
/// and the top parts' not, as KiCad writes a flipped footprint's fields.
#[test]
#[ignore]
fn kicad_cli_drc_bottom_side_pads() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };

    let part = |reference: &str, package: &str, pins: usize| Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some(package.into()),
        footprint: Some(package.into()),
        pins: (1..=pins).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None, symbol: None, datasheet: None,
        edge: None,
    };
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|p| p.to_string()).collect() };
    // Two-pin nets get a track from pad to pad. The one-pin nets give the
    // pad a routed track would wrongly land on a net of its own, so a
    // misplaced pad shows up and not just a stray track.
    let model = ConstraintModel {
        parts: vec![part("U1", "SOT-23", 3), part("U2", "SOT-23", 3), part("U3", "SOT-23", 3), part("R1", "0603", 2), part("J1", "PINHEADER-2", 2)],
        nets: vec![
            // U1: bottom, unrotated. Pins 1 and 2 share a column, pin 3 has the other.
            net("A", &["U1.1", "U1.2"]),
            net("B", &["U1.3"]),
            // U2: bottom and rotated, so the mirror has to come before the rotation.
            net("C", &["U2.1", "U2.2"]),
            net("D", &["U2.3"]),
            // R1: a bottom 0603, whose unmirrored pins swap places. Its far
            // ends are a top-side through-hole header (reachable on B.Cu),
            // which does not swap: joined to another bottom 0603 pin to
            // pin, the swap cancels out and KiCad sees nothing wrong.
            net("E", &["R1.1", "J1.2"]),
            net("F", &["R1.2", "J1.1"]),
            // U3: top and rotated, the control -- the fix must not touch it.
            net("G", &["U3.1", "U3.2"]),
            net("H", &["U3.3"]),
        ],
        ..Default::default()
    };
    let at = |id: &str, x: Um, y: Um, rot, side| FootprintInstance { id: id.into(), at: Point { x, y }, rot, side, label: Default::default() };
    let footprints = vec![
        at("U1", 6_000, 6_000, 0, Side::Bottom),
        at("U2", 14_000, 6_000, 90_000, Side::Bottom),
        at("U3", 22_000, 6_000, 90_000, Side::Top),
        at("R1", 6_000, 14_000, 0, Side::Bottom),
        at("J1", 6_000, 18_000, 0, Side::Top),
    ];

    let instance = |pin: &str| footprints.iter().find(|f| pin.split_once('.').unwrap().0 == f.id).unwrap();
    let pad_at = |pin: &str| -> Point {
        let (reference, number) = pin.split_once('.').unwrap();
        let pads = placed_pads(&model, model.part(reference).unwrap(), instance(pin)).unwrap();
        pads.into_iter().find(|p| p.number == number).unwrap().center
    };
    // The premise: the engine mirrors a bottom part (U1's pin 1 is at
    // local x < 0) and leaves a top one alone.
    assert!(pad_at("U1.1").x > 6_000 && pad_at("R1.1").x > 6_000, "the engine no longer mirrors bottom-side parts");
    assert!(pad_at("U3.1").y < 6_000, "the engine mirrors a top-side part");

    let tracks: Vec<Track> = model
        .nets
        .iter()
        .filter(|n| n.pins.len() == 2)
        .map(|n| Track {
            id: String::new(),
            net: n.name.clone(),
            pins: n.pins.clone(),
            layer: if instance(&n.pins[0]).side == Side::Bottom { "B.Cu" } else { "F.Cu" }.into(),
            width: model.board.track_width,
            pts: vec![pad_at(&n.pins[0]), pad_at(&n.pins[1])],
        })
        .collect();
    let outline = vec![Point { x: 0, y: 0 }, Point { x: 28_000, y: 0 }, Point { x: 28_000, y: 22_000 }, Point { x: 0, y: 22_000 }];
    let design = Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![],
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "bottom_side_pads".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        placement: Some(PlacementSection { outline, footprints: footprints.clone(), modules: Vec::new() }),
        routing: Some(RoutingSection { tracks, vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    };

    let meta = ExportMeta { date: "2026-01-01", title: "bottom_side_pads" };
    let pcb_text = export_kicad_pcb(&design, &model, &meta).expect("export_kicad_pcb");
    let dir = std::env::temp_dir().join("eda_kicad_bottom_side_test");
    std::fs::create_dir_all(&dir).unwrap();
    let pcb_path = dir.join("bottom_side_pads.kicad_pcb");
    std::fs::write(&pcb_path, &pcb_text).unwrap();

    let by_type = drc_by_type(&cli, &pcb_path);
    let failures: Vec<String> = IN_SCOPE
        .iter()
        .chain(&["track_dangling", "nonmirrored_text_on_back_layer", "mirrored_text_on_front_layer"])
        .flat_map(|ty| by_type.get(*ty).into_iter().flatten().map(move |v| format!("[{ty}] {v}")))
        .collect();
    assert!(failures.is_empty(), "{} DRC violations on the bottom-side board:\n{}", failures.len(), failures.join("\n"));
}

/// Every graphic shape kind and free text, run through real `kicad-cli pcb
/// drc`: the point isn't the (empty) DRC result, it's that kicad-cli
/// parses the file at all -- `drc_by_type` already asserts the report was
/// produced, which fails loudly if a `gr_*`/`gr_text` token this exporter
/// wrote is not what KiCad 9's own grammar expects.
#[test]
#[ignore]
fn kicad_cli_drc_shapes_and_text() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };

    let mut drawings = eda_model::ir::DrawingsSection {
        shapes: vec![
            eda_model::ir::Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 1_000, y: 1_000 }, end: Point { x: 9_000, y: 1_000 } },
            eda_model::ir::Shape::Arc {
                id: String::new(),
                layer: "Cmts.User".into(),
                stroke_width: 100,
                filled: false,
                start: Point { x: 15_000, y: 5_000 },
                mid: Point { x: 12_071, y: 12_071 },
                end: Point { x: 5_000, y: 15_000 },
            },
            eda_model::ir::Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100, filled: true, start: Point { x: 1_000, y: 1_000 }, end: Point { x: 9_000, y: 9_000 } },
            eda_model::ir::Shape::Circle { id: String::new(), layer: "B.SilkS".into(), stroke_width: 120, filled: false, center: Point { x: 10_000, y: 10_000 }, end: Point { x: 12_000, y: 10_000 } },
            eda_model::ir::Shape::Polygon {
                id: String::new(),
                layer: "F.CrtYd".into(),
                stroke_width: 50,
                filled: true,
                pts: vec![Point { x: 1_000, y: 1_000 }, Point { x: 4_000, y: 1_000 }, Point { x: 4_000, y: 4_000 }, Point { x: 1_000, y: 4_000 }],
            },
        ],
        texts: vec![eda_model::ir::Text {
            id: String::new(),
            content: "REV A".into(),
            at: Point { x: 5_000, y: 18_000 },
            angle: 0,
            layer: "F.SilkS".into(),
            size_um: 1_000,
            stroke_width: 150,
            justify: eda_model::ir::TextJustify::Center,
            mirror: false,
        }],
        ..Default::default()
    };
    drawings.assign_missing_ids();

    let design = Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![],
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "shapes_text_drc".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        placement: Some(PlacementSection {
            outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }],
            footprints: vec![],
            modules: Vec::new(),
        }),
        routing: None,
        drawings: Some(drawings),
    };
    let model = ConstraintModel::default();
    let meta = ExportMeta { date: "2026-01-01", title: "shapes_text_drc" };
    let pcb_text = export_kicad_pcb(&design, &model, &meta).expect("export_kicad_pcb");
    let dir = std::env::temp_dir().join("eda_kicad_shapes_text_test");
    std::fs::create_dir_all(&dir).unwrap();
    let pcb_path = dir.join("shapes_text_drc.kicad_pcb");
    std::fs::write(&pcb_path, &pcb_text).unwrap();

    let by_type = drc_by_type(&cli, &pcb_path);
    let in_scope: usize = IN_SCOPE.iter().filter_map(|ty| by_type.get(*ty)).map(|v| v.len()).sum();
    assert_eq!(in_scope, 0, "shapes/text alone must not create DRC violations: {by_type:?}");
}

/// The two real footprints this feature exists for -- loaded through the
/// library loader, not hand-built -- placed, routed by the real router,
/// exported, and checked against real kicad-cli: the same IN_SCOPE bar
/// every other DRC test here holds to. This is what proves the slotted
/// shield pads, the non-plated locating pegs and the repeated "SH" pad
/// number are not just accepted by our own gates but by KiCad itself.
#[test]
#[ignore]
fn kicad_cli_drc_real_footprints() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let root = eda_kicad::default_footprint_library_root();
    let Some(usb_c_path) = eda_kicad::find_footprint_file(&root, "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12") else {
        eprintln!("KiCad footprint libraries not found at {}; skipping", root.display());
        return;
    };
    let usb_c_text = std::fs::read_to_string(&usb_c_path).unwrap();
    let usb_c = eda_kicad::parse_footprint_file(&usb_c_text, "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12").expect("parses");
    let button_path = eda_kicad::find_footprint_file(&root, "Button_Switch_SMD:SW_SPST_B3U-1000P").expect("ships alongside the connector's library");
    let button_text = std::fs::read_to_string(&button_path).unwrap();
    let button = eda_kicad::parse_footprint_file(&button_text, "Button_Switch_SMD:SW_SPST_B3U-1000P").expect("parses");

    let make_part = |r: &str, footprint: &str, fp: &eda_model::Footprint| Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: None,
        footprint: Some(footprint.into()),
        pins: fp.pads.iter().map(|p| Pin { number: p.number.clone(), name: None, kind: PinKind::Passive }).collect(),
        body_um: None, symbol: None, datasheet: None,
        edge: None,
    };
    let j1 = make_part("J1", "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12", &usb_c);
    let sw1 = make_part("SW1", "Button_Switch_SMD:SW_SPST_B3U-1000P", &button);
    let model = ConstraintModel {
        parts: vec![j1, sw1],
        // USB-C is reversible, so this connector's real footprint pairs up
        // A1/B12, A12/B1, A4/B9 and A9/B4 at *exactly* coincident
        // positions (real KiCad geometry, not an artifact of our reader) --
        // each pair is one rail, and DRC rightly calls two same-position
        // pads on different nets a clearance violation, so each pair goes
        // on its own net here, same as any real design would. The shared
        // "SH" pin is the interesting one: since all four physical shield
        // pads carry that one number, this net asks the router to treat
        // all four as one net and connect every one of them, plus the
        // button -- the repeated-pad-number contract, on real geometry.
        // (The signal row's own 0.3-0.5mm pitch pads are packed too
        // tightly for a 2-layer board to break out of at all -- the
        // shield walls them in on both layers -- so this test does not
        // ask the router to cross the row; a real design would need an
        // inner layer or hand-placed vias for that.)
        nets: vec![
            Net { name: "VBUS_A".into(), pins: vec!["J1.A1".into(), "J1.B12".into()] },
            Net { name: "VBUS_B".into(), pins: vec!["J1.A12".into(), "J1.B1".into()] },
            Net { name: "GND_A".into(), pins: vec!["J1.A4".into(), "J1.B9".into()] },
            Net { name: "GND_B".into(), pins: vec!["J1.A9".into(), "J1.B4".into()] },
            Net { name: "SHIELD".into(), pins: vec!["J1.SH".into(), "SW1.1".into()] },
        ],
        footprints: vec![usb_c, button],
        ..Default::default()
    };

    let outline = vec![Point { x: 0, y: 0 }, Point { x: 40_000, y: 0 }, Point { x: 40_000, y: 30_000 }, Point { x: 0, y: 30_000 }];
    let footprints = vec![
        FootprintInstance { id: "J1".into(), at: Point { x: 12_000, y: 8_000 }, rot: 0, side: Side::Top, label: Default::default() },
        FootprintInstance { id: "SW1".into(), at: Point { x: 30_000, y: 20_000 }, rot: 0, side: Side::Top, label: Default::default() },
    ];
    let design = Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![],
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "real_footprints".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        placement: Some(PlacementSection { outline, footprints, modules: Vec::new() }),
        routing: None,
        drawings: None,
    };

    let routed = route(&design, &model, &model.board, 0).unwrap_or_else(|e| panic!("route failed: {e:?}"));
    let meta = ExportMeta { date: "2026-01-01", title: "real_footprints" };
    let pcb_text = export_kicad_pcb(&routed, &model, &meta).expect("export_kicad_pcb");
    let dir = std::env::temp_dir().join("eda_kicad_real_footprints_test");
    std::fs::create_dir_all(&dir).unwrap();
    let pcb_path = dir.join("real_footprints.kicad_pcb");
    std::fs::write(&pcb_path, &pcb_text).unwrap();

    let by_type = drc_by_type(&cli, &pcb_path);
    let in_scope: usize = IN_SCOPE.iter().filter_map(|ty| by_type.get(*ty)).map(|v| v.len()).sum();
    assert_eq!(in_scope, 0, "the real USB-C connector and button, placed and routed, must clear DRC: {by_type:?}");
}

/// `kicad-cli pcb drc` on `pcb`: its violations and unconnected items
/// grouped by type, with the counts printed. The report is written next to
/// the board as drc.json.
fn drc_by_type(cli: &Path, pcb: &Path) -> BTreeMap<String, Vec<serde_json::Value>> {
    let drc_report = pcb.with_file_name("drc.json");
    let drc_out = Command::new(cli)
        // --refill-zones: a zone is stored as an outline plus a cached
        // fill, and we export only the outline. Without the refill KiCad
        // checks connectivity against an empty plane and reports every
        // stitching via as dangling -- a false failure that says nothing
        // about the board.
        .args(["pcb", "drc", "--refill-zones", "--format", "json", "--severity-all", "--exit-code-violations", "--output"])
        .arg(&drc_report)
        .arg(pcb)
        .output()
        .expect("failed to run kicad-cli pcb drc");
    eprintln!(
        "DRC exit: {:?}\nDRC stdout:\n{}\nDRC stderr:\n{}",
        drc_out.status.code(),
        String::from_utf8_lossy(&drc_out.stdout),
        String::from_utf8_lossy(&drc_out.stderr)
    );

    assert!(drc_report.exists(), "kicad-cli did not produce a DRC report — pcb file likely failed to parse");
    let report_text = std::fs::read_to_string(&drc_report).unwrap();
    let report: serde_json::Value = serde_json::from_str(&report_text).expect("parse DRC json report");

    let mut by_type: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    if let Some(violations) = report.get("violations").and_then(|v| v.as_array()) {
        for v in violations {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
            by_type.entry(ty).or_default().push(v.clone());
        }
    }
    // KiCad also reports unconnected items separately under "unconnected_items"
    // at the top level in some versions; fold those in too if present.
    if let Some(unconnected) = report.get("unconnected_items").and_then(|v| v.as_array()) {
        for v in unconnected {
            by_type.entry("unconnected_items".into()).or_default().push(v.clone());
        }
    }

    println!("DRC violation counts by type:");
    for (ty, vs) in &by_type {
        println!("  {ty}: {}", vs.len());
    }
    by_type
}

/// The design routed by the FreeRouting port, or what it left unrouted.
fn route(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, rules: &eda_model::BoardRules, _seed: u64) -> Result<eda_model::ir::Design, Vec<String>> {
    let routed = eda_freeroute::design::route_design(design, model, rules, 20, 10).map_err(|e| vec![e])?;
    if !routed.unrouted.is_empty() {
        return Err(routed.unrouted);
    }
    let mut out = design.clone();
    out.routing = Some(routed.routing);
    Ok(out)
}
