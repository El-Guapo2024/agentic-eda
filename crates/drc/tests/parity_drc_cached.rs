//! Offline companion to `parity_drc.rs`: re-runs *our* side of the DRC
//! parity measurement on KiCad's QA boards and compares per-type **counts**
//! against the kicad-cli counts already recorded in
//! `docs/parity/raw/drc.json` -- for environments that have the KiCad
//! source tree (and so the QA corpus under `qa/data`) but no kicad-cli.
//! Counts only: the recorded file keeps per-type totals, not positions, so
//! this can show "we report 1849 too many shorting_items on issue22475"
//! but not which ones. `parity_drc.rs` stays the real, position-matched
//! measurement.
//!
//!   EDA_KICAD_QA_BOARDS=<kicad>/qa/data \
//!     cargo test -p eda-drc --test parity_drc_cached -- --ignored --nocapture
//!
//! Optional `PARITY_BOARD=<substring>` limits the run to matching boards.
//! Prints a per-type table (kicad / ours / diff) plus the boards with the
//! largest absolute diffs; never writes any file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eda_kicad::import_kicad_pcb;

const OUR_NON_KICAD_TYPES: &[&str] = &[
    "placement_proximity",
    "placement_decoupling",
    "placement_stub_crossings",
    "placement_board_use",
    "placement_net_compactness",
    "placement_edge_connector",
    "placement_refdes_clear",
    "routing_track_width",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// Same import + sidecar merging `parity_drc.rs`'s `process_qa_board` does.
fn our_counts(pcb: &Path) -> Result<BTreeMap<String, usize>, String> {
    let text = std::fs::read_to_string(pcb).map_err(|e| e.to_string())?;
    let (design, mut model, _notes) = import_kicad_pcb(&text).map_err(|e| format!("{e:?}"))?;
    if let Ok(pro) = std::fs::read_to_string(pcb.with_extension("kicad_pro")) {
        eda_kicad::merge_project_net_classes(&mut model, &pro);
        eda_kicad::merge_project_rule_severities(&mut model, &pro);
        eda_kicad::merge_project_design_rules(&mut model, &pro);
    }
    if let Ok(dru) = std::fs::read_to_string(pcb.with_extension("kicad_dru")) {
        eda_kicad::merge_custom_rules(&mut model, &dru);
    }
    let mut out = BTreeMap::new();
    for v in eda_connectivity::run_drc(&design, &model) {
        if OUR_NON_KICAD_TYPES.contains(&v.error_type) {
            continue;
        }
        *out.entry(v.error_type.to_string()).or_insert(0) += 1;
    }
    Ok(out)
}

#[test]
#[ignore]
fn parity_drc_cached_counts() {
    let Some(qa) = std::env::var_os("EDA_KICAD_QA_BOARDS").map(PathBuf::from) else {
        println!("EDA_KICAD_QA_BOARDS not set; skipping");
        return;
    };
    let filter = std::env::var("PARITY_BOARD").ok();
    let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo_root().join("docs/parity/raw/drc.json")).unwrap()).unwrap();

    let mut totals: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut board_diffs: Vec<(usize, String, String)> = Vec::new();
    for b in raw["boards"].as_array().unwrap() {
        if b["source"] != "qa" || !b["error"].is_null() {
            continue;
        }
        let name = b["board"].as_str().unwrap();
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        // Recorded as "<parent>/<file>" relative to qa/data's parent dirs.
        let pcb = if qa.join(name).exists() { qa.join(name) } else { qa.join("pcbnew").join(name) };
        let ours = match our_counts(&pcb) {
            Ok(c) => c,
            Err(e) => {
                println!("{name}: ERROR {e}");
                continue;
            }
        };
        let mut types: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (t, s) in b["types"].as_object().unwrap() {
            types.entry(t.clone()).or_default().0 = s["kicad"].as_u64().unwrap() as usize;
        }
        for (t, n) in &ours {
            types.entry(t.clone()).or_default().1 = *n;
        }
        let mut detail = Vec::new();
        let mut diff_sum = 0;
        for (t, (k, o)) in &types {
            let e = totals.entry(t.clone()).or_default();
            e.0 += k;
            e.1 += o;
            if k != o {
                diff_sum += k.abs_diff(*o);
                detail.push(format!("{t} {k}->{o}"));
            }
        }
        board_diffs.push((diff_sum, name.to_string(), detail.join(", ")));
    }

    println!("\n{:<34} {:>7} {:>7} {:>7}", "type", "kicad", "ours", "diff");
    let (mut tk, mut to, mut td) = (0, 0, 0);
    for (t, (k, o)) in &totals {
        println!("{t:<34} {k:>7} {o:>7} {:>+7}", *o as i64 - *k as i64);
        tk += k;
        to += o;
        td += k.abs_diff(*o);
    }
    println!("{:<34} {tk:>7} {to:>7}   |diff| {td}", "TOTAL");
    board_diffs.sort_by(|a, b| b.0.cmp(&a.0));
    println!("\nboards by |diff|:");
    for (d, n, detail) in board_diffs.iter().take(25) {
        println!("{d:>6}  {n}: {detail}");
    }
}

/// Debug helper: dump our violations of one type on one board.
///   PARITY_BOARD=issue22475 PARITY_TYPE=tracks_crossing cargo test ... dump_type -- --ignored --nocapture
#[test]
#[ignore]
fn dump_type() {
    let (Some(qa), Ok(board), Ok(ty)) = (std::env::var_os("EDA_KICAD_QA_BOARDS").map(PathBuf::from), std::env::var("PARITY_BOARD"), std::env::var("PARITY_TYPE")) else {
        return;
    };
    let pcb = walk(&qa).into_iter().find(|p| p.to_string_lossy().contains(&board) && p.extension().is_some_and(|e| e == "kicad_pcb")).expect("board");
    let text = std::fs::read_to_string(&pcb).unwrap();
    let (design, mut model, _) = import_kicad_pcb(&text).unwrap();
    if let Ok(pro) = std::fs::read_to_string(pcb.with_extension("kicad_pro")) {
        eda_kicad::merge_project_net_classes(&mut model, &pro);
        eda_kicad::merge_project_rule_severities(&mut model, &pro);
        eda_kicad::merge_project_design_rules(&mut model, &pro);
    }
    if let Ok(dru) = std::fs::read_to_string(pcb.with_extension("kicad_dru")) {
        eda_kicad::merge_custom_rules(&mut model, &dru);
    }
    let limit: usize = std::env::var("PARITY_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let all: Vec<_> = eda_connectivity::run_drc(&design, &model).into_iter().filter(|v| v.error_type == ty).collect();
    println!("{} {} violations", all.len(), ty);
    for v in all.iter().take(limit) {
        println!("{} | {}", v.description, v.items.iter().map(|i| format!("{} @({},{}) [{}]", i.description, i.pos.0, i.pos.1, i.id)).collect::<Vec<_>>().join(" ; "));
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
