//! `/api/sch/*` for the schematic hierarchy: the dialogs that work across sheets and have to say what they would do before they do it.
//!
//! * `POST /api/sch/annotate`: what Annotate Schematic would do (`DIALOG_ANNOTATE`'s message window): the messages `AnnotateSymbols` reports and
//!   the findings of `CheckAnnotate`, on a copy of the board. The annotation itself is the `annotate` verb of `/api/cmd`.

use crate::board;
use eda_model::CheckResult;
use eda_ops::{AnnotateOptions, AnnotateOrder, Board};
use serde_json::{json, Value};
use std::path::Path;

fn bad(why: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail("hierarchy_request", "request", why)]
}

/// The board the way the studio shows it: a board with no stored schematic has the one derived from its intent.
fn loaded(dir: &Path) -> Result<(board::Meta, eda_model::ir::Design, eda_model::ConstraintModel), Vec<CheckResult>> {
    let (meta, mut design, model) = board::load(dir)?;
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model)?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    Ok((meta, design, model))
}

/// `POST /api/sch/annotate`: `{ sheet?: "id/id", reset_existing, order?, ids?, options? }` (the fields of `Cmd::Annotate`) -> `{ ok, changed, messages:
/// [{ severity, text }], problems: [...] }`. Nothing is written.
pub(crate) fn annotate(dir: &Path, body: &[u8]) -> Value {
    match annotate_report(dir, body) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

fn annotate_report(dir: &Path, body: &[u8]) -> Result<Value, Vec<CheckResult>> {
    let req: Value = serde_json::from_slice(body).map_err(|e| bad(format!("not JSON: {e}")))?;
    let sheet = req.get("sheet").and_then(Value::as_str).unwrap_or("").to_string();
    let reset = req.get("reset_existing").and_then(Value::as_bool).unwrap_or(false);
    let order: AnnotateOrder = match req.get("order") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| bad(format!("bad order: {e}")))?,
        _ => AnnotateOrder::default(),
    };
    let ids: Option<Vec<String>> = match req.get("ids") {
        Some(v) if !v.is_null() => Some(serde_json::from_value(v.clone()).map_err(|e| bad(format!("bad ids: {e}")))?),
        _ => None,
    };
    let options: AnnotateOptions = match req.get("options") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| bad(format!("bad options: {e}")))?,
        _ => AnnotateOptions::default(),
    };
    let (meta, design, model) = loaded(dir)?;
    let b = Board::new(design, &model, meta.snap_um, meta.spacing_um);
    let report = b.annotate_report(&sheet, reset, order, ids.as_deref(), &options)?;
    Ok(json!({
        "ok": true,
        "changed": report.changed,
        "messages": report.messages,
        "problems": report.problems,
    }))
}

// ------------------------------------------------------------------------------------------------ Import Sheet

/// How deep under the board's directory the schematics offered to Import Sheet are looked for.
const SCAN_DEPTH: usize = 3;

fn scan(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            // hidden folders hold scratch (`.kicad`, `.history`); `target` and `node_modules` hold other things than a project
            if depth < SCAN_DEPTH && !name.starts_with('.') && name != "target" && name != "node_modules" {
                scan(root, &path, depth + 1, out);
            }
        } else if path.extension().and_then(|x| x.to_str()) == Some("kicad_sch") {
            let rel = path.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string_lossy().to_string());
            let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((rel, name, bytes));
        }
    }
}

/// `GET /api/sch/sheet_files`: `{ ok, dir, files: [{ path, name, bytes }] }` -- the `.kicad_sch` files of the project (the board's directory, and
/// the folder its intent is in), by path relative to the board's directory; Import Sheet offers them beside the path the user can type.
pub(crate) fn sheet_files(dir: &Path) -> Value {
    let mut found: Vec<(String, String, u64)> = Vec::new();
    scan(dir, dir, 0, &mut found);
    if let Ok((meta, _, _)) = board::load(dir) {
        if let Some(parent) = Path::new(&meta.intent).parent().filter(|p| !p.as_os_str().is_empty()) {
            let parent = if parent.is_absolute() { parent.to_path_buf() } else { dir.join(parent) };
            if parent.canonicalize().ok() != dir.canonicalize().ok() {
                scan(&parent, &parent, SCAN_DEPTH - 1, &mut found);
                // these are named by absolute path: they are not under the board's directory
                for f in found.iter_mut().filter(|(rel, _, _)| !Path::new(rel).is_absolute() && !dir.join(rel).exists()) {
                    f.0 = parent.join(&f.0).to_string_lossy().to_string();
                }
            }
        }
    }
    found.sort();
    found.dedup();
    json!({ "ok": true, "dir": dir.to_string_lossy(), "files": found.iter().map(|(path, name, bytes)| json!({ "path": path, "name": name, "bytes": bytes })).collect::<Vec<_>>() })
}

/// `POST /api/sch/sheet_file`: `{ path, name?, keep_annotations? }` -> what Import Sheet would take in, with `LoadSheetFromFile`'s checks: `{ ok, path,
/// stem, sheets: [name], symbols, references, libraries, findings: [{ severity, text }], blocked }`. Nothing is written.
pub(crate) fn sheet_file(dir: &Path, body: &[u8]) -> Value {
    match check_sheet_file(dir, body) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

fn check_sheet_file(dir: &Path, body: &[u8]) -> Result<Value, Vec<CheckResult>> {
    let req: Value = serde_json::from_slice(body).map_err(|e| bad(format!("not JSON: {e}")))?;
    let source = req.get("path").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if source.is_empty() {
        return Err(bad("name a schematic file"));
    }
    let path = crate::sheet_import::resolve(dir, &source);
    let content = eda_kicad::load_sheet_file(&path)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("sheet").to_string();
    let keep = req.get("keep_annotations").and_then(Value::as_bool).unwrap_or(false);
    let name = req.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()).unwrap_or(&stem).to_string();
    let (meta, design, model) = loaded(dir)?;
    let board = Board::new(design.clone(), &model, meta.snap_um, meta.spacing_um);
    let mut findings: Vec<Value> = board.check_sheet_file(&content, &name, keep).iter().map(|f| json!({ "severity": f.severity, "text": f.text })).collect();

    // `LoadSheetFromFile`'s library check: a library the project lacks (the symbols come in from the definitions the file carries)
    let mut lib_ids: Vec<String> = content
        .all_screens()
        .flat_map(|(_, s)| s.symbols.iter().map(|x| x.lib_id.clone()).chain(s.power_symbols.iter().map(|p| p.lib_id.clone())))
        .filter(|id| id.split_once(':').is_some_and(|(l, n)| !l.is_empty() && !n.is_empty()) && !eda_model::is_synthetic_lib_id(id))
        .collect();
    lib_ids.sort();
    lib_ids.dedup();
    let mut missing: Vec<String> = lib_ids.iter().filter(|id| crate::library_api::symbol_in(&design, &model, id).is_none()).filter_map(|id| id.split_once(':').map(|(l, _)| l.to_string())).collect();
    missing.sort();
    missing.dedup();
    if !missing.is_empty() {
        findings.push(json!({
            "severity": "warning",
            "text": format!("There are library names in the selected schematic that are missing from the current project: {}. The symbols are drawn from the definitions the file carries; their links to those libraries will not follow a library update.", missing.join(", ")),
        }));
    }
    let blocked = findings.iter().any(|f| f["severity"] == "error");
    let sheets: Vec<String> = content.root.sheets.iter().map(|s| s.name.clone()).collect();
    let symbols: usize = content.all_screens().map(|(_, s)| s.symbols.len()).sum();
    // the file the copy will be called, if the name is the stem: the design's own screens are not written over
    let taken: std::collections::BTreeSet<String> = design.sheet_contents.iter().flat_map(|c| c.keys().cloned()).collect();
    let dest_file = eda_engine::hier::sheet_file_name(&name, &taken);
    Ok(json!({
        "ok": true,
        "path": source,
        "stem": stem,
        "sheets": sheets,
        "symbols": symbols,
        "references": content.references(),
        "libraries": content.library_nicknames(),
        "findings": findings,
        "blocked": blocked,
        "dest_file": dest_file,
    }))
}
