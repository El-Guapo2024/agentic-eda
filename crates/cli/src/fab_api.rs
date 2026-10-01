//! `/api/fab/*`: the backend for the studio's Plot, Generate Drill Files
//! and Footprint Position Files dialogs (`web/studio/src/components/
//! PlotDialog.tsx`, `GenerateDrillDialog.tsx`, `FootprintPositionDialog.tsx`).
//!
//! Each endpoint takes the same board directory every other `/api/*`
//! route does, runs the matching `eda_fab` writer (ported from KiCad --
//! see `crates/fab/src/gerber.rs`/`drill.rs`/`position.rs`'s own doc
//! comments), writes the result into `<dir>/export/`, and returns the
//! paths written (relative to `dir`, so the dialog can show them next to
//! "open folder"). `crate::fab_cmd` is the CLI's own `eda fab ...` verb on
//! the same writers; this module exists only to translate a JSON POST
//! body into the same option structs `fab_cmd` builds from flags, so a
//! studio dialog and the CLI produce identical output for identical
//! input.

use crate::board;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn write_all(dir: &Path, files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Result<Vec<String>, String> {
    let out_dir = dir.join("export");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for (name, bytes) in files {
        let path = out_dir.join(&name);
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        written.push(format!("export/{name}"));
    }
    Ok(written)
}

fn title_of(dir: &Path) -> Result<String, String> {
    let (meta, _, _) = board::load(dir).map_err(|e| board::reasons(&e))?;
    Ok(PathBuf::from(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string())
}

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

fn ok(files: Vec<String>) -> Value {
    json!({ "ok": true, "files": files })
}

/// `POST /api/fab/gerbers`: `{"layers": ["F.Cu", "B.Cu", ...]}` (omitted
/// or empty = [`eda_fab::gerber::default_jlc_layers`], the Plot dialog's
/// own default checklist).
pub fn gerbers(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let title = match title_of(dir) {
        Ok(t) => t,
        Err(e) => return err(e),
    };
    let meta = eda_fab::gerber::FabMeta { title: title.clone(), date: eda::now_rfc3339(), rev: "rev?".into(), generator_version: env!("CARGO_PKG_VERSION").into() };
    let requested: Vec<String> = req.get("layers").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let layers = if requested.is_empty() {
        eda_fab::gerber::default_jlc_layers(model.board.layers.len().max(2))
    } else {
        match crate::fab_cmd::parse_layers(&requested.join(","), &model.board.layers) {
            Ok(l) => l,
            Err(e) => return err(board::reasons(&e)),
        }
    };
    let files = match eda_fab::gerber::plot_all(&design, &model, &meta, &layers) {
        Ok(f) => f,
        Err(e) => return err(board::reasons(&e)),
    };
    let job_bytes = eda_fab::job::write_job(&design, &model, &meta, &files).into_bytes();
    let mut out: Vec<(String, Vec<u8>)> = files.into_iter().map(|f| (f.filename, f.content.into_bytes())).collect();
    out.push((format!("{title}-job.gbrjob"), job_bytes));
    match write_all(dir, out) {
        Ok(written) => ok(written),
        Err(e) => err(e),
    }
}

/// `POST /api/fab/drill`: `{"separate_th": bool}`.
pub fn drill(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let title = match title_of(dir) {
        Ok(t) => t,
        Err(e) => return err(e),
    };
    let meta = eda_fab::drill::DrillMeta { title, date: eda::now_rfc3339(), generator_version: env!("CARGO_PKG_VERSION").into() };
    let opts = eda_fab::drill::DrillOptions { separate_th: req.get("separate_th").and_then(Value::as_bool).unwrap_or(false) };
    let files = match eda_fab::drill::write_drill(&design, &model, &meta, opts) {
        Ok(f) => f,
        Err(e) => return err(board::reasons(&e)),
    };
    match write_all(dir, files.into_iter().map(|f| (f.filename, f.content.into_bytes()))) {
        Ok(written) => ok(written),
        Err(e) => err(e),
    }
}

/// `POST /api/fab/pos`: `{"format": "csv"|"ascii", "side": "front"|"back"|"both",
/// "units_mm": bool, "smd_only": bool, "exclude_fp_th": bool}`.
pub fn pos(dir: &Path, body: &[u8]) -> Value {
    use eda_fab::position::{PosFormat, PosOptions, PosSide};
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let title = match title_of(dir) {
        Ok(t) => t,
        Err(e) => return err(e),
    };
    let format = match req.get("format").and_then(Value::as_str) {
        Some("ascii") => PosFormat::Ascii,
        _ => PosFormat::Csv,
    };
    let side = match req.get("side").and_then(Value::as_str) {
        Some("front") => PosSide::Front,
        Some("back") => PosSide::Back,
        _ => PosSide::Both,
    };
    let opts = PosOptions {
        format,
        side,
        units_mm: req.get("units_mm").and_then(Value::as_bool).unwrap_or(true),
        smd_only: req.get("smd_only").and_then(Value::as_bool).unwrap_or(false),
        exclude_fp_th: req.get("exclude_fp_th").and_then(Value::as_bool).unwrap_or(false),
    };
    let meta = eda_fab::position::PosMeta { date: eda::now_rfc3339(), generator_version: env!("CARGO_PKG_VERSION").into() };
    let content = match eda_fab::position::write_pos(&design, &model, &meta, opts) {
        Ok(c) => c,
        Err(e) => return err(board::reasons(&e)),
    };
    let ext = if format == PosFormat::Ascii { "pos" } else { "csv" };
    match write_all(dir, [(format!("{title}.{ext}"), content.into_bytes())]) {
        Ok(written) => ok(written),
        Err(e) => err(e),
    }
}

/// `POST /api/fab/bom`: no body.
pub fn bom(dir: &Path) -> Value {
    let (_, _, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let title = match title_of(dir) {
        Ok(t) => t,
        Err(e) => return err(e),
    };
    match write_all(dir, [(format!("{title}-bom.csv"), eda_fab::bom_csv(&model).into_bytes())]) {
        Ok(written) => ok(written),
        Err(e) => err(e),
    }
}
