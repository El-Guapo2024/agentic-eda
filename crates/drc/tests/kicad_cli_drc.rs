//! Oracle verification: routes real example boards, exports them to
//! `.kicad_pcb`, and compares `eda_drc::run`'s violations against
//! `kicad-cli pcb drc --format json --severity-all`'s on the exact same
//! file -- the core deliverable of the DRC port (see the task report for
//! the full match-rate table this produces).
//!
//! `#[ignore]`d like `crates/kicad/tests/kicad_cli_drc.rs` (needs
//! `kicad-cli`); run with:
//! `cargo test -p eda-drc --test kicad_cli_drc -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_pcb, ExportMeta};
use eda_model::ir::Design;
use eda_model::{BoardRules, ConstraintModel};
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
    mac.exists().then_some(mac)
}

fn route(design: &Design, model: &ConstraintModel, rules: &BoardRules) -> Result<Design, String> {
    let routed = eda_freeroute::design::route_design(design, model, rules, 20, 10)?;
    if !routed.unrouted.is_empty() {
        return Err(format!("unrouted: {:?}", routed.unrouted));
    }
    let mut out = design.clone();
    out.routing = Some(routed.routing);
    Ok(out)
}

/// `derive_schematic` -> `place` -> `route_design`, structural only (no
/// schematic/placement *quality* gate is enforced -- several of the ladder
/// boards fail those, per `examples/ladder/README.md`, well before their
/// geometry is otherwise perfectly DRC-able).
fn build_routed_board(yaml_path: &Path, seed: u64) -> Result<(Design, ConstraintModel), String> {
    let text = std::fs::read_to_string(yaml_path).map_err(|e| e.to_string())?;
    let model: ConstraintModel = serde_yaml::from_str(&text).map_err(|e| e.to_string())?;
    let opts = EngineOptions { seed, intent_hash: "eda_drc_oracle".into(), ..Default::default() };
    let design = derive_schematic(&model, &opts).map_err(|e| format!("{e:?}"))?;
    let placed = place(&design, &model, &PlaceOptions { seed, ..Default::default() }).map_err(|e| format!("{e:?}"))?;
    let routed = route(&placed, &model, &model.board)?;
    Ok((routed, model))
}

/// `kicad-cli pcb drc`'s violations, grouped by `type`.
fn oracle_by_type(cli: &Path, pcb: &Path) -> BTreeMap<String, Vec<serde_json::Value>> {
    let report_path = pcb.with_file_name(format!("{}.drc.json", pcb.file_stem().unwrap().to_string_lossy()));
    let out = Command::new(cli)
        .args(["pcb", "drc", "--format", "json", "--severity-all", "--output"])
        .arg(&report_path)
        .arg(pcb)
        .output()
        .expect("run kicad-cli pcb drc");
    if !report_path.exists() {
        panic!("kicad-cli produced no report for {}: stdout={} stderr={}", pcb.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
    let report: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).expect("parse DRC json");
    let mut by_type: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    if let Some(vs) = report.get("violations").and_then(|v| v.as_array()) {
        for v in vs {
            by_type.entry(v.get("type").and_then(|t| t.as_str()).unwrap_or("?").to_string()).or_default().push(v.clone());
        }
    }
    by_type
}

/// Every error-type key this port produces (`eda_drc::item::ErrorType`'s
/// full list), for the match-rate table -- kept as literal strings rather
/// than importing the enum so this test also catches a key typo/drift.
const PORTED_TYPES: &[&str] = &[
    "clearance",
    "hole_clearance",
    "tracks_crossing",
    "shorting_items",
    "zones_intersect",
    "track_width",
    "annular_width",
    "drill_out_of_range",
    "via_diameter",
    "hole_to_hole",
    "holes_co_located",
    "copper_edge_clearance",
    "silk_edge_clearance",
    "courtyards_overlap",
    "pth_inside_courtyard",
    "npth_inside_courtyard",
    "silk_overlap",
    "silk_over_copper",
    "solder_mask_bridge",
    "text_height",
    "text_thickness",
    "track_dangling",
    "via_dangling",
];

struct BoardMatch {
    name: String,
    ours: BTreeMap<String, usize>,
    oracle: BTreeMap<String, usize>,
}

#[test]
#[ignore]
fn match_rate_against_kicad_cli() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().and_then(|p| p.parent()).expect("crates/drc -> repo root");
    let dir = std::env::temp_dir().join("eda_drc_oracle_test");
    std::fs::create_dir_all(&dir).unwrap();

    let candidates: Vec<PathBuf> = [
        "examples/two_pin_nets.yaml",
        "examples/mcu_board_30plus.yaml",
        "examples/mixed_track_widths.yaml",
        "examples/ldo.yaml",
        "examples/opamp_filter.yaml",
        "examples/passive_divider_ladder.yaml",
        "examples/star_net.yaml",
        "examples/through_hole_headers.yaml",
        "examples/dense_small_outline.yaml",
        "examples/nc_pins.yaml",
        "examples/all_power_ground_net.yaml",
        "examples/ldo_proximity_heavy.yaml",
        "examples/ladder/l1_usb_mcu.yaml",
        "examples/ladder/l2_sensor_hub.yaml",
        "examples/ladder/l3_motor_hub.yaml",
    ]
    .iter()
    .map(|p| repo_root.join(p))
    .collect();

    let mut results = Vec::new();
    let mut skipped = Vec::new();

    for yaml in &candidates {
        let name = yaml.file_stem().unwrap().to_string_lossy().to_string();
        let (routed, model) = match build_routed_board(yaml, 0) {
            Ok(r) => r,
            Err(e) => {
                skipped.push(format!("{name}: {e}"));
                continue;
            }
        };

        let meta = ExportMeta { date: "2026-01-01", title: &name };
        let pcb_text = match export_kicad_pcb(&routed, &model, &meta) {
            Ok(t) => t,
            Err(e) => {
                skipped.push(format!("{name}: export failed: {e:?}"));
                continue;
            }
        };
        let pcb_path = dir.join(format!("{name}.kicad_pcb"));
        std::fs::write(&pcb_path, &pcb_text).unwrap();

        let oracle = oracle_by_type(&cli, &pcb_path);
        let ours_violations = eda_drc::run(&routed, &model);
        let ours = eda_drc::counts_by_type(&ours_violations).into_iter().map(|(k, v)| (k.to_string(), v)).collect();

        results.push(BoardMatch { name, ours, oracle: oracle.into_iter().map(|(k, v)| (k, v.len())).collect() });
    }

    // ---- print the match-rate table ----
    println!("\n=== eda-drc vs kicad-cli match rate ===");
    println!("boards attempted: {}, routed+exported: {}, skipped: {}", candidates.len(), results.len(), skipped.len());
    for s in &skipped {
        println!("  skipped: {s}");
    }

    let mut totals: BTreeMap<&str, (usize, usize)> = PORTED_TYPES.iter().map(|t| (*t, (0, 0))).collect();
    for r in &results {
        println!("\n-- {} --", r.name);
        for ty in PORTED_TYPES {
            let ours = *r.ours.get(*ty).unwrap_or(&0);
            let oracle = *r.oracle.get(*ty).unwrap_or(&0);
            if ours == 0 && oracle == 0 {
                continue;
            }
            let e = totals.get_mut(ty).unwrap();
            e.0 += ours;
            e.1 += oracle;
            let mark = if ours == oracle { "OK" } else { "DIFF" };
            println!("  {ty:24} ours={ours:<4} oracle={oracle:<4} {mark}");
        }
    }

    println!("\n-- totals across {} routed board(s) --", results.len());
    println!("{:24} {:>6} {:>8}", "type", "ours", "oracle");
    for ty in PORTED_TYPES {
        let (ours, oracle) = totals[ty];
        if ours == 0 && oracle == 0 {
            continue;
        }
        println!("{ty:24} {ours:>6} {oracle:>8}");
    }

    assert!(!results.is_empty(), "no example board routed cleanly enough to export+DRC; see skipped list above");

    // Assert tight agreement on the checks this port is most confident
    // about (pure board-setting floors with no zone/net-class subtlety);
    // everything else is reported above but not asserted on, matching
    // `crates/kicad/tests/kicad_cli_drc.rs`'s own IN_SCOPE/visible-only split.
    for ty in ["track_width", "drill_out_of_range", "annular_width"] {
        let (ours, oracle) = totals[ty];
        assert_eq!(ours, oracle, "{ty}: ours={ours} oracle={oracle}");
    }
}
