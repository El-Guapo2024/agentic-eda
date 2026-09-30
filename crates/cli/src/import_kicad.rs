//! `eda import-kicad <board.kicad_pcb> -o <dir>` -- read a real KiCad board
//! into a fresh `eda board` directory (`design.json` + `intent.yaml` +
//! `board.json`, the same shape `eda board new` writes -- see
//! `crate::board::Meta`), so `eda board status/serve/check` can open it
//! straight away.
//!
//! ```text
//! eda import-kicad templates/Arduino/Arduino_Uno_R3.kicad_pcb -o work/uno
//! eda board status -C work/uno
//! eda board serve  -C work/uno
//! ```

use eda_model::CheckResult;
use std::path::PathBuf;

fn flag(rest: &[String], name: &str) -> Option<String> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned()
}

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg)]
}

/// Same shape as `board::Meta` (kept private there), duplicated rather
/// than exposed across the module boundary just for this one writer.
#[derive(serde::Serialize)]
struct Meta {
    intent: String,
    snap_um: i64,
    spacing_um: i64,
}

pub fn run(rest: &[String]) -> Result<(), Vec<CheckResult>> {
    let input = rest
        .first()
        .cloned()
        .ok_or_else(|| fail("kicad_import_usage", "import-kicad", "usage: eda import-kicad <board.kicad_pcb> -o <dir>"))?;
    let out: PathBuf = flag(rest, "-o")
        .or_else(|| flag(rest, "--out"))
        .map(PathBuf::from)
        .ok_or_else(|| fail("kicad_import_usage", "import-kicad", "eda import-kicad needs -o <dir>"))?;

    let text = std::fs::read_to_string(&input).map_err(|e| fail("io", &input, format!("could not read {input}: {e}")))?;
    let (design, model, notes) = eda::import_kicad_pcb(&text)?;

    std::fs::create_dir_all(&out).map_err(|e| fail("io", &out.display().to_string(), e.to_string()))?;

    let intent_path = out.join("intent.yaml");
    let intent_yaml = serde_yaml::to_string(&model).map_err(|e| fail("kicad_import_encode", "intent.yaml", e.to_string()))?;
    std::fs::write(&intent_path, intent_yaml).map_err(|e| fail("io", "intent.yaml", e.to_string()))?;

    let design_bytes = design.canonical_bytes().map_err(|e| fail("kicad_import_encode", "design.json", e.to_string()))?;
    std::fs::write(out.join("design.json"), design_bytes).map_err(|e| fail("io", "design.json", e.to_string()))?;

    // `board::load` reads `meta.intent` as a plain path (no relativizing to
    // the board dir), so it has to survive the board directory moving
    // relative to the cwd it was created from -- same as `eda board new`.
    let abs_intent = std::fs::canonicalize(&intent_path).unwrap_or(intent_path);
    let meta = Meta { intent: abs_intent.display().to_string(), snap_um: model.solver.place_snap_um, spacing_um: model.solver.place_spacing_um };
    std::fs::write(out.join("board.json"), serde_json::to_string_pretty(&meta).expect("meta encodes")).map_err(|e| fail("io", "board.json", e.to_string()))?;

    let pl = design.placement.as_ref();
    let fp_count = pl.map_or(0, |p| p.footprints.len());
    let outline_pts = pl.map_or(0, |p| p.outline.len());
    let (tracks, vias) = design.routing.as_ref().map_or((0, 0), |r| (r.tracks.len(), r.vias.len()));
    eprintln!(
        "import-kicad: {fp_count} footprint(s) ({} explicit footprint def(s)), {} net(s), {tracks} track(s), {vias} via(s) -> {}",
        model.footprints.len(),
        model.nets.len(),
        out.display()
    );
    eprintln!(
        "  outline: {} ({outline_pts} point(s){})",
        notes.outline_source,
        if notes.outline_open { ", did not close into a loop" } else { "" }
    );
    if notes.zones_skipped > 0 {
        eprintln!("  {} zone(s)/pour(s) skipped -- not imported (see report for a proposed model shape)", notes.zones_skipped);
    }
    if notes.track_arcs_approximated > 0 {
        eprintln!("  {} arc(s) approximated as short straight segments (our model has no arc primitive)", notes.track_arcs_approximated);
    }
    if notes.non_rect_pad_shapes_approximated > 0 {
        eprintln!("  {} pad(s) with an unsupported shape (trapezoid/custom) approximated as rect", notes.non_rect_pad_shapes_approximated);
    }
    eprintln!("wrote {}", out.join("design.json").display());
    Ok(())
}
