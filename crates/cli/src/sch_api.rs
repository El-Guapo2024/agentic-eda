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

/// The schematic with the dialog's staged changes (if any) overlaid, and the schematic as it stands: every sheet of the design as one section, as KiCad's table covers the
/// project (`fields_table::project_view`). The table shows the first; the DNP and Exclude-from-BOM filters read the second (`fields_table::build_table_over`).
fn staged_schematic(req: &Value, design: &eda_model::ir::Design) -> Result<(eda_model::ir::SchematicSection, eda_model::ir::SchematicSection), String> {
    let base = fields_table::project_view(design).ok_or("this design has no schematic")?;
    let mut sch = base.clone();
    if let Some(c) = req.get("changes").filter(|v| !v.is_null()) {
        let changes: FieldChanges = serde_json::from_value(c.clone()).map_err(|e| format!("bad changes: {e}"))?;
        // an edit shows on every row of its symbol: the screen's and each later sheet path's (`fields_table::changes_for_view`)
        fields_table::apply_field_changes(&mut sch, &fields_table::changes_for_view(design, &changes))?;
    }
    Ok((sch, base))
}

/// `POST /api/sch/fields_table`: `{spec?, changes?, preset?}` ->
/// `{ok, spec, presets, user_fields, rows: [{refs, flag, item_number, cells, mixed, children}]}`. `preset` is the name of one of KiCad's built-in views
/// (`BOM_PRESET::BuiltInPresets`, `presets` lists them): it is applied to `spec` (`ApplyBomPreset`) before the table is built, and the spec that comes back is the result.
pub fn fields_table(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (sch, base) = match staged_schematic(&req, &design) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let mut spec = match parse_spec(&req, &sch) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    if let Some(name) = req.get("preset").and_then(Value::as_str) {
        spec = match spec.with_preset(name) {
            Some(s) => s,
            None => return err(format!("no built-in preset named '{name}'")),
        };
    }
    let table = fields_table::build_table_over(&sch, &base, &model, &spec);
    let presets: Vec<&str> = TableSpec::built_in().into_iter().map(|(n, _)| n).collect();
    json!({ "ok": true, "spec": spec, "presets": presets, "user_fields": fields_table::user_field_names(&sch), "rows": table.rows })
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
    let (sch, base) = match staged_schematic(&req, &design) {
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
    let text = fields_table::export_bom_over(&sch, &base, &model, &spec, &fmt);
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

/// `POST /api/sch/find`: `{search: SearchData, scope?: [ids], sheet?: "id/id/...", all_sheets?: bool}` ->
/// `{ok, current_rank, matches: [{key, uid, kind, id, name, at: [x, y], text, sheet, sheet_name, page, sheet_rank}]}` in
/// `nextMatch` order (ascending x, y). `scope` (the dialog's "Search only
/// selected objects") keeps only matches whose owning id is listed. `sheet`
/// is the sheet in view (the root when absent): the positions are on its
/// page, and Replace edits that sheet (`Cmd::OnSheet`).
///
/// `all_sheets` (the dialog's "Search the current sheet only" off, `SCH_FIND_REPLACE_TOOL::FindNext`'s `searchAllSheets`) searches the
/// whole hierarchy: the sheets in page order (`SCH_SHEET_LIST::SortByPageNumbers`), each one's matches in `nextMatch` order. Every match
/// says which sheet it is on (`sheet`, the placement ids from the root; `page`; `sheet_rank`, its sheet's place in the page order) and has
/// a `uid` (the sheet and the key: a key alone names an item of one sheet), and `current_rank` is the rank of `sheet`, where a Find with
/// no cursor starts. A search restricted to a selection (`scope`) is of the sheet in view, as in KiCad.
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
    let scope: Option<Vec<String>> = req.get("scope").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
    let sheet_text = req.get("sheet").and_then(Value::as_str).unwrap_or("");
    let all_sheets = req.get("all_sheets").and_then(Value::as_bool).unwrap_or(false) && scope.is_none();
    let in_view: Vec<String> = sheet_text.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
    let walk = eda_model::sheet_list::walk(&design);
    let sorted = eda_model::sheet_list::sort_by_page_numbers(&walk);
    let rank_of = |path: &[String]| sorted.iter().position(|e| e.path == path).unwrap_or(0);
    let hit_json = |h: &search::Hit, entry: &eda_model::sheet_list::SheetEntry, rank: usize| {
        json!({
            "key": h.key(), "uid": format!("{}#{}", entry.path_text(), h.key()), "kind": h.kind, "id": h.id, "name": h.name, "at": [h.at.x, h.at.y], "text": h.text,
            "sheet": entry.path, "sheet_name": entry.name, "page": entry.effective_page(), "sheet_rank": rank,
        })
    };
    let mut matches: Vec<Value> = Vec::new();
    if all_sheets {
        for (rank, entry) in sorted.iter().enumerate() {
            let Some(sch) = eda_model::sheet_list::screen_of(&design, &entry.path) else { continue };
            matches.extend(search::find_items(sch, &model, &search).iter().map(|h| hit_json(h, entry, rank)));
        }
    } else {
        let (sch, _) = crate::studio::resolve_sheet(&design, sheet_text);
        let rank = rank_of(&in_view);
        let entry = sorted.get(rank).cloned().unwrap_or_else(|| eda_model::sheet_list::SheetEntry { path: in_view.clone(), name: String::new(), file: String::new(), page: String::new(), virtual_page: 1 });
        matches.extend(
            search::find_items(&sch, &model, &search)
                .iter()
                .filter(|h| scope.as_ref().is_none_or(|s| s.contains(&h.id)))
                .map(|h| hit_json(h, &entry, rank)),
        );
    }
    json!({ "ok": true, "current_rank": rank_of(&in_view), "matches": matches })
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

/// `GET /api/sch/erc_severities`: the per-check severities Schematic Setup > Violation Severity edits -- the checks the schematic reports
/// at a severity other than KiCad's default for them, plus the two library-link checks the derived project always ignores unless the
/// table says otherwise (`eda_kicad::effective_erc_severities`), which is what kicad-cli's ERC runs with -- and whether the design
/// stores a table of its own.
pub fn erc_severities(dir: &Path) -> Value {
    let (_, design, _) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let custom = design.schematic.as_ref().is_some_and(|s| !s.extras.erc_severities.is_empty());
    json!({ "ok": true, "severities": eda_kicad::effective_erc_severities(&design), "custom": custom })
}

/// `GET /api/sch/formatting`: `{ok, connection_grid_mils, custom, intersheet_refs}` -- Schematic Setup > Formatting, the settings of that page the design keeps: the
/// connection grid (KiCad's default, 50 mils, unless the design says otherwise) and whether the design says so, and the Inter-sheet References. The project file carries
/// them to kicad-cli.
pub fn formatting(dir: &Path) -> Value {
    let (_, design, _) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let own = design.schematic.as_ref().and_then(|s| s.extras.connection_grid_mils);
    // Inter-sheet References (`SCHEMATIC_SETTINGS::m_IntersheetRefs*`), KiCad's defaults when the design set none
    json!({ "ok": true, "connection_grid_mils": own.unwrap_or(eda_model::sch_extras::DEFAULT_CONNECTION_GRID_MILS), "min_connection_grid_mils": eda_model::sch_extras::MIN_CONNECTION_GRID_MILS, "max_connection_grid_mils": eda_model::sch_extras::MAX_CONNECTION_GRID_MILS, "custom": own.is_some(), "intersheet_refs": eda_kicad::design_intersheet_refs(&design) })
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
