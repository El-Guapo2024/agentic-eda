//! Optional integration test: routes `examples/ldo.yaml` with seed 3 through
//! schematic -> placement -> routing, exports both `.kicad_sch` and
//! `.kicad_pcb`, and runs `kicad-cli pcb drc` on the result. Only runs when
//! `kicad-cli` is present. `#[ignore]` by default — run with
//! `cargo test -p eda-kicad --test kicad_cli_drc -- --ignored --nocapture`.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_pcb, export_kicad_sch, ExportMeta};
use eda_model::ConstraintModel;
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

    let drc_report = dir.join("drc.json");
    let drc_out = Command::new(&cli)
        // --refill-zones: a zone is stored as an outline plus a cached
        // fill, and we export only the outline. Without the refill KiCad
        // checks connectivity against an empty plane and reports every
        // stitching via as dangling -- a false failure that says nothing
        // about the board.
        .args(["pcb", "drc", "--refill-zones", "--format", "json", "--severity-all", "--exit-code-violations", "--output"])
        .arg(&drc_report)
        .arg(&pcb_path)
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

/// The design routed by the FreeRouting port, or what it left unrouted.
fn route(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, rules: &eda_model::BoardRules, _seed: u64) -> Result<eda_model::ir::Design, Vec<String>> {
    let routed = eda_freeroute::design::route_design(design, model, rules, 20).map_err(|e| vec![e])?;
    if !routed.unrouted.is_empty() {
        return Err(routed.unrouted);
    }
    let mut out = design.clone();
    out.routing = Some(routed.routing);
    Ok(out)
}
