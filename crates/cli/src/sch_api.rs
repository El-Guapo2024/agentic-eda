//! `/api/sch/*`: read-side and export backends for the schematic Symbol
//! Fields Table, Find / Find and Replace and ERC pin-map dialogs
//! (`web/studio/src/components/SymbolFieldsTableDialog.tsx`,
//! `FindReplaceDialog.tsx`, `SchematicSetupDialog.tsx`).
//!
//! Every *edit* those dialogs make is an `/api/cmd` verb with undo
//! (`Cmd::SetSymbolFields`, `Cmd::ReplaceText`, `Cmd::SetErcPinMapCell`,
//! `Cmd::ResetErcPinMap`); this module only holds the queries that have no
//! side effect on the design plus the one file-writing export:
//!
//!  * `POST /api/sch/fields_table`: the grid (`FIELDS_EDITOR_GRID_DATA_MODEL::
//!    RebuildRows` + `GetValue`) for a view spec, optionally overlaying the
//!    dialog's *staged* (not yet applied) changes the way KiCad's own data
//!    store does, so the grid regroups on unapplied edits.
//!  * `POST /api/sch/bom_export`: `FIELDS_EDITOR_GRID_DATA_MODEL::Export`,
//!    previewed (`PreviewRefresh`) or written to a file (`OnExport`).
//!  * `POST /api/sch/find`: `SCH_FIND_REPLACE_TOOL::nextMatch`'s ordered
//!    match list; the dialog cycles through it for Find Next / Previous.
//!  * `GET /api/sch/erc_pin_map`: the effective ERC pin map plus whether
//!    it is customized (`PANEL_SETUP_PINMAP`).

use crate::board;
use eda_ops::fields_table::{self, BomFmt, Column, FieldChanges, TableSpec};
use eda_ops::search::{self, SearchData};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// The dialog's initial view: `BOM_PRESET::DefaultEditing` plus one column
/// per user field already present on any symbol
/// (`DIALOG_SYMBOL_FIELDS_TABLE::LoadFieldNames`).
fn default_spec(sch: &eda_model::ir::SchematicSection) -> TableSpec {
    let mut spec = TableSpec::default_editing();
    for name in fields_table::user_field_names(sch) {
        spec.columns.push(Column { name: name.clone(), label: name, show: true, group_by: false });
    }
    spec
}

fn parse_spec(req: &Value, sch: &eda_model::ir::SchematicSection) -> Result<TableSpec, String> {
    match req.get("spec").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad table spec: {e}")),
        None => Ok(default_spec(sch)),
    }
}

/// The board as the studio shows it: a board with no stored schematic has the one derived from its intent (module sheets), so the dialogs
/// read what is on screen, whichever sheet it is.
fn loaded(dir: &Path) -> Result<(board::Meta, eda_model::ir::Design, eda_model::ConstraintModel), Value> {
    let (meta, mut design, model) = board::load(dir).map_err(|e| err(board::reasons(&e)))?;
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model).map_err(|e| err(board::reasons(&e)))?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    Ok((meta, design, model))
}

/// The schematic with the dialog's staged changes (if any) overlaid: every sheet of the design as one section, as KiCad's table covers the
/// project (`fields_table::project_view`).
fn staged_schematic(req: &Value, design: &eda_model::ir::Design) -> Result<eda_model::ir::SchematicSection, String> {
    let mut sch = fields_table::project_view(design).ok_or("this design has no schematic")?;
    if let Some(c) = req.get("changes").filter(|v| !v.is_null()) {
        let changes: FieldChanges = serde_json::from_value(c.clone()).map_err(|e| format!("bad changes: {e}"))?;
        fields_table::apply_field_changes(&mut sch, &changes)?;
    }
    Ok(sch)
}

/// `POST /api/sch/fields_table`: `{spec?, changes?}` ->
/// `{ok, spec, user_fields, rows: [{refs, flag, item_number, cells, mixed, children}]}`.
pub fn fields_table(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let sch = match staged_schematic(&req, &design) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let spec = match parse_spec(&req, &sch) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let table = fields_table::build_table(&sch, &model, &spec);
    json!({ "ok": true, "spec": spec, "user_fields": fields_table::user_field_names(&sch), "rows": table.rows })
}

fn fmt_of(req: &Value) -> Result<BomFmt, String> {
    match req.get("fmt").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad BOM format: {e}")),
        None => Ok(BomFmt::csv()),
    }
}

/// A user-chosen output path stays inside the board directory: relative,
/// no `..`. (KiCad lets the user write anywhere; a studio served over HTTP
/// must not.)
fn safe_relative(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if path.trim().is_empty() {
        return Err("No output file specified in Export tab.".into());
    }
    if p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir)) {
        return Err("the output file must be a relative path inside the board directory".into());
    }
    Ok(p.to_path_buf())
}

/// `POST /api/sch/bom_export`: `{spec?, fmt?, changes?, path?, preview?}`.
/// `preview: true` (or no `path`) returns `{ok, text}` only
/// (`PreviewRefresh`); otherwise writes `path` (relative to the board
/// directory, default `export/<intent>-bom.csv`) like `OnExport`, creating
/// parent directories (`EnsureFileDirectoryExists`).
pub fn bom_export(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (meta, design, model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let sch = match staged_schematic(&req, &design) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let spec = match parse_spec(&req, &sch) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let fmt = match fmt_of(&req) {
        Ok(f) => f,
        Err(e) => return err(e),
    };
    let text = fields_table::export_bom(&sch, &model, &spec, &fmt);
    let preview = req.get("preview").and_then(Value::as_bool).unwrap_or(false);
    let Some(path) = req.get("path").and_then(Value::as_str) else {
        let stem = PathBuf::from(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
        return json!({ "ok": true, "text": text, "default_path": format!("export/{stem}-bom.csv") });
    };
    if preview {
        return json!({ "ok": true, "text": text });
    }
    let rel = match safe_relative(path) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let full = dir.join(&rel);
    if let Some(parent) = full.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return err(format!("Could not open/create path '{}': {e}", parent.display()));
        }
    }
    if let Err(e) = std::fs::write(&full, text.as_bytes()) {
        return err(format!("Could not create BOM output '{}': {e}", rel.display()));
    }
    json!({ "ok": true, "file": rel.to_string_lossy(), "text": text })
}

/// `POST /api/sch/find`: `{search: SearchData, scope?: [ids], sheet?: "id/id/..."}` ->
/// `{ok, matches: [{key, kind, id, name, at: [x, y], text}]}` in
/// `nextMatch` order (ascending x, y). `scope` (the dialog's "Search only
/// selected objects") keeps only matches whose owning id is listed. `sheet`
/// is the sheet in view (the root when absent): the positions are on its
/// page, and Replace edits that sheet (`Cmd::OnSheet`), so Find searches it.
pub fn find(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let search: SearchData = match serde_json::from_value(req.get("search").cloned().unwrap_or(Value::Null)) {
        Ok(s) => s,
        Err(e) => return err(format!("bad search: {e}")),
    };
    let (_, design, model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (sch, _) = crate::studio::resolve_sheet(&design, req.get("sheet").and_then(Value::as_str).unwrap_or(""));
    let sch = &sch;
    let scope: Option<Vec<String>> = req.get("scope").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
    let matches: Vec<Value> = search::find_items(sch, &model, &search)
        .into_iter()
        .filter(|h| scope.as_ref().is_none_or(|s| s.contains(&h.id)))
        .map(|h| json!({ "key": h.key(), "kind": h.kind, "id": h.id, "name": h.name, "at": [h.at.x, h.at.y], "text": h.text }))
        .collect();
    json!({ "ok": true, "matches": matches })
}

/// `GET /api/sch/erc_pin_map`: `{ok, matrix, custom}` -- the matrix ERC
/// actually runs with (the design's own, else KiCad's default; the project file
/// carries it to kicad-cli, `eda_kicad::export_kicad_pro_for`) and whether the design
/// stores its own.
pub fn erc_pin_map(dir: &Path) -> Value {
    let (_, design, _) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let matrix: Vec<Vec<u8>> = eda_kicad::custom_erc_pin_map(&design).unwrap_or_else(eda_model::ir::ErcPinMap::default_matrix);
    let custom = design.schematic.as_ref().is_some_and(|s| s.erc_pin_map.is_some());
    json!({ "ok": true, "matrix": matrix, "custom": custom })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_paths_must_stay_inside_the_board_directory() {
        assert!(safe_relative("export/bom.csv").is_ok());
        assert!(safe_relative("bom.csv").is_ok());
        assert!(safe_relative("../bom.csv").is_err());
        assert!(safe_relative("/etc/bom.csv").is_err());
        assert!(safe_relative("  ").is_err());
    }
}
