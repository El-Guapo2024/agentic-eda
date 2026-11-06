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

/// The dialog's initial view: `BOM_PRESET::DefaultEditing` plus one column per user field already present on any symbol, then one hidden column per field
/// name template no symbol has (`DIALOG_SYMBOL_FIELDS_TABLE::LoadFieldNames`: `for( const TEMPLATE_FIELDNAME& tfn : m_TemplateFieldNames.GetTemplateFieldNames() )
/// AddField( tfn.m_Name, .., false, false )`), and then the view the project saved (`TransferDataToWindow`: `ApplyBomPreset( m_BomSettings )`, the `.kicad_pro`'s
/// `schematic.bom_settings`). **Differs**: with no saved view the user fields are shown (KiCad's DefaultEditing hides every column it does not name).
///
/// The mandatory Description (`addMandatoryField( FIELD_T::DESCRIPTION, false, false )`) and the generated `${ITEM_NUMBER}` ("#") are columns, not shown, as
/// `ApplyBomPreset( DefaultEditing )` leaves them (UI-LEFT-2 SS13); a symbol's field named Description is that column, not a user field's.
fn default_spec(sch: &eda_model::ir::SchematicSection, design: &eda_model::ir::Design) -> TableSpec {
    let mut spec = TableSpec::default_editing();
    for (name, label) in [(fields_table::DESCRIPTION, "Description"), (fields_table::ITEM_NUMBER, "#")] {
        if !spec.columns.iter().any(|c| c.name == name) {
            spec.columns.push(Column { name: name.to_string(), label: label.to_string(), show: false, group_by: false });
        }
    }
    for name in fields_table::user_field_names(sch) {
        if fields_table::is_mandatory_field(&name) || spec.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        spec.columns.push(Column { name: name.clone(), label: name, show: true, group_by: false });
    }
    let root = design.schematic.as_ref();
    for t in root.map(|r| r.extras.field_name_templates.as_slice()).unwrap_or(&[]) {
        if !spec.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&t.name)) {
            spec.columns.push(Column { name: t.name.clone(), label: t.name.clone(), show: false, group_by: false });
        }
    }
    if let Some(saved) = project_value(design, "bom_settings").as_ref().and_then(TableSpec::from_bom_preset) {
        spec = spec.with_spec(saved);
    }
    spec
}

fn parse_spec(req: &Value, sch: &eda_model::ir::SchematicSection, design: &eda_model::ir::Design) -> Result<TableSpec, String> {
    match req.get("spec").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad table spec: {e}")),
        None => Ok(default_spec(sch, design)),
    }
}

/// A key of the project's schematic settings (`SchExtras::project_schematic`, the `.kicad_pro`'s `schematic` object): `bom_settings`, `bom_presets`, ...
fn project_value(design: &eda_model::ir::Design, key: &str) -> Option<Value> {
    design.schematic.as_ref()?.extras.project_schematic.as_ref()?.get(key).cloned()
}

/// The project's own view presets (`m_BomPresets`, `schematic.bom_presets`), by name, in the project's order.
fn user_presets(design: &eda_model::ir::Design) -> Vec<(String, Value)> {
    project_value(design, "bom_presets").and_then(|v| v.as_array().cloned()).unwrap_or_default().into_iter().filter_map(|p| Some((p.get("name")?.as_str()?.to_string(), p))).collect()
}

/// The table's scope (`scope`, `sheet_path`, `selection` of the request: [`fields_table::scope_refs`]) applied to the staged schematic and the one the filters read:
/// the symbols out of scope go from both, so the rows are built from those in scope only (`RebuildRows`).
fn apply_scope(req: &Value, design: &eda_model::ir::Design, sections: [&mut eda_model::ir::SchematicSection; 2]) {
    let list = |k: &str| req.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
    let scope = req.get("scope").and_then(Value::as_str).unwrap_or("all");
    let Some(refs) = fields_table::scope_refs(design, scope, &list("sheet_path"), &list("selection")) else { return };
    for sch in sections {
        sch.symbols.retain(|s| refs.contains(&s.id));
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

/// The design variant the table shows (`variant`: a variant's name; absent, empty or `< Default >` is the default variant) and its staged edits (`variant_edits`,
/// [`fields_table::VariantFieldEdit`]s), laid over the staged schematic `sch` (KiCad 25ec2335 `SYMBOL_FIELDS_EDITOR_GRID_DATA_MODEL::SetCurrentVariant`: the grid
/// shows that variant's values, the edits of every visited variant kept until Apply). Returns the variant's name and `sch` with the variant edits on it; the
/// view the grid shows is [`eda_model::variants::resolve_section`] of it. An edit made on a later sheet path's row is an edit to the screen's symbol, shown on
/// each of its rows, as a field edit is ([`fields_table::path_aliases`]).
fn staged_variant(req: &Value, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, sch: &eda_model::ir::SchematicSection) -> Result<(String, eda_model::ir::SchematicSection), String> {
    let variant = req.get("variant").and_then(Value::as_str).map(str::trim).filter(|v| !v.eq_ignore_ascii_case(eda_model::variants::DEFAULT_VARIANT_NAME)).unwrap_or("").to_string();
    let mut staged = sch.clone();
    if let Some(v) = req.get("variant_edits").filter(|v| !v.is_null()) {
        let edits: Vec<fields_table::VariantFieldEdit> = serde_json::from_value(v.clone()).map_err(|e| format!("bad variant edits: {e}"))?;
        let aliases = fields_table::path_aliases(design);
        let mut mapped: Vec<fields_table::VariantFieldEdit> = Vec::new();
        for e in edits {
            let own = aliases.get(&e.id).cloned().unwrap_or_else(|| e.id.clone());
            for id in std::iter::once(own.clone()).chain(aliases.iter().filter(|(_, of)| **of == own).map(|(a, _)| a.clone())) {
                mapped.push(fields_table::VariantFieldEdit { id, ..e.clone() });
            }
        }
        fields_table::apply_variant_edits(&mut staged, model, &mapped)?;
    }
    Ok((variant, staged))
}

/// `POST /api/sch/fields_table`: `{spec?, changes?, preset?, variant?, variant_edits?}` ->
/// `{ok, spec, presets, user_fields, rows: [{refs, flag, item_number, cells, mixed, children}]}`. `preset` is the name of one of KiCad's built-in views
/// (`BOM_PRESET::BuiltInPresets`, `presets` lists them): it is applied to `spec` (`ApplyBomPreset`) before the table is built, and the spec that comes back is the result.
pub fn fields_table(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match loaded(dir) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (mut sch, mut base) = match staged_schematic(&req, &design) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let mut spec = match parse_spec(&req, &sch, &design) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let users = user_presets(&design);
    if let Some(name) = req.get("preset").and_then(Value::as_str) {
        // a built-in view, else one the project saved (`ApplyBomPreset`)
        let user = users.iter().find(|(n, _)| n == name).and_then(|(_, p)| TableSpec::from_bom_preset(p));
        spec = match spec.with_preset(name).or_else(|| user.map(|u| spec.with_spec(u))) {
            Some(s) => s,
            None => return err(format!("no preset named '{name}'")),
        };
    }
    // the columns are the project's whatever the scope; the rows are the scope's
    apply_scope(&req, &design, [&mut sch, &mut base]);
    let (variant, staged) = match staged_variant(&req, &design, &model, &sch) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let table = if variant.is_empty() {
        fields_table::build_table_over(&staged, &base, &model, &spec)
    } else {
        // the variant's values, its filters (`ResolveDNP( sheet, m_currentVariant )`) and the cells it differs in from the symbols' own
        let shown = eda_model::variants::resolve_section(&staged, &variant);
        let mut t = fields_table::build_table_over(&shown, &eda_model::variants::resolve_section(&base, &variant), &model, &spec);
        fields_table::mark_variant_overrides(&mut t.rows, &shown, &staged, &model, &spec);
        t
    };
    let presets: Vec<&str> = TableSpec::built_in().into_iter().map(|(n, _)| n).collect();
    // the project's own presets, each with its view, so the dialog can tell which one the view is (`syncBomPresetSelection`)
    let user: Vec<Value> = users.iter().filter_map(|(n, p)| Some(json!({ "name": n, "spec": TableSpec::from_bom_preset(p)? }))).collect();
    let built_in: Vec<Value> = TableSpec::built_in().into_iter().map(|(n, s)| json!({ "name": n, "spec": s })).collect();
    // the export's format presets (`m_BomFmtPresets`, `schematic.bom_fmt_presets`) and the format the dialog opens on (`m_BomFmtSettings`, `bom_fmt_settings`),
    // and the view the project saved (`m_BomSettings`), which the dialog compares its closing view with (`savePresetsToSchematic`)
    let fmt_presets: Vec<Value> = project_value(&design, "bom_fmt_presets").and_then(|v| v.as_array().cloned()).unwrap_or_default().into_iter().filter(|p| serde_json::from_value::<BomFmt>(p.clone()).is_ok()).collect();
    let fmt_settings = project_value(&design, "bom_fmt_settings").filter(|v| serde_json::from_value::<BomFmt>(v.clone()).is_ok());
    let bom_settings = project_value(&design, "bom_settings").filter(|v| TableSpec::from_bom_preset(v).is_some());
    json!({ "ok": true, "spec": spec, "presets": presets, "built_in_presets": built_in, "user_presets": user, "user_fields": fields_table::user_field_names(&sch), "rows": table.rows,
            "fmt_presets": fmt_presets, "fmt_settings": fmt_settings, "bom_settings": bom_settings, "user_preset_json": users.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>() })
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

/// `POST /api/sch/bom_export`: `{spec?, fmt?, changes?, path?, preview?, variant?, variant_edits?}`.
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
    let (mut sch, mut base) = match staged_schematic(&req, &design) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let spec = match parse_spec(&req, &sch, &design) {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    // the rows of the table's scope (`OnExport` writes the data model's rows)
    apply_scope(&req, &design, [&mut sch, &mut base]);
    let fmt = match fmt_of(&req) {
        Ok(f) => f,
        Err(e) => return err(e),
    };
    // the variant the table shows (`OnExport` exports the data model's current variant)
    let (variant, staged) = match staged_variant(&req, &design, &model, &sch) {
        Ok(v) => v,
        Err(e) => return err(e),
    };
    let text = fields_table::export_bom_over(&eda_model::variants::resolve_section(&staged, &variant), &eda_model::variants::resolve_section(&base, &variant), &model, &spec, &fmt);
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

/// `POST /api/find/text`: `{search: SearchData, texts: [..], replace?: bool}` -> `{ok, matches: [bool], replaced: [string | null]}` -- `EDA_ITEM::Matches( aText,
/// aSearchData )` of each text, and with `replace` `EDA_ITEM::Replace`'s new text (null where nothing was replaced), with the one matcher of the schematic's
/// Find ([`search::matches_text`], [`search::replace_text`]: plain, whole words, wildcards and `eda_model::wx_regex`). The Symbol Editor's Find and Replace
/// (`SCH_FIND_REPLACE_TOOL` in a `SYMBOL_EDIT_FRAME`) walks the symbol it is editing in the browser and asks this which of its texts match: the rules of
/// which item is searched are the walk's (`web/studio/src/kicad-port/symFind.ts`), the matching is this.
pub fn find_text(body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let search: SearchData = match serde_json::from_value(req.get("search").cloned().unwrap_or_else(|| json!({}))) {
        Ok(s) => s,
        Err(e) => return err(format!("bad search: {e}")),
    };
    let texts: Vec<String> = req.get("texts").and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let replace = req.get("replace").and_then(Value::as_bool).unwrap_or(false);
    let matches: Vec<bool> = texts.iter().map(|t| search::matches_text(t, &search)).collect();
    let replaced: Vec<Value> = if replace { texts.iter().map(|t| search::replace_text(t, &search).map_or(Value::Null, Value::String)).collect() } else { Vec::new() };
    json!({ "ok": true, "matches": matches, "replaced": replaced })
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
    // the page's other settings (`eda_model::sch_setup::SchFormatting`) as stored -- what the page shows, out-of-range sizes included, as KiCad's panel shows them
    let formatting = design.schematic.as_ref().and_then(|s| s.extras.formatting.clone()).unwrap_or_default();
    // Inter-sheet References (`SCHEMATIC_SETTINGS::m_IntersheetRefs*`), KiCad's defaults when the design set none
    json!({ "ok": true, "connection_grid_mils": own.unwrap_or(eda_model::sch_extras::DEFAULT_CONNECTION_GRID_MILS), "min_connection_grid_mils": eda_model::sch_extras::MIN_CONNECTION_GRID_MILS, "max_connection_grid_mils": eda_model::sch_extras::MAX_CONNECTION_GRID_MILS, "custom": own.is_some(), "intersheet_refs": eda_kicad::design_intersheet_refs(&design), "formatting": formatting })
}

/// `GET /api/sch/setup`: the Schematic Setup pages that keep project settings, as the design holds them now -- General > Annotation (the order, numbering,
/// start number, reference reuse and the unit notation's first id and separator), Field Name Templates, BOM Presets (the names of the project's saved presets)
/// and Project > Bus Alias Definitions. KiCad's defaults where the design set none.
pub fn setup(dir: &Path) -> Value {
    let (_, design, _) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let extras = design.schematic.as_ref().map(|s| s.extras.clone()).unwrap_or_default();
    let names = |key: &str| -> Vec<String> { extras.project_schematic.as_ref().and_then(|p| p.get(key)).and_then(Value::as_array).map(|a| a.iter().filter_map(|p| p.get("name").and_then(Value::as_str)).map(String::from).collect()).unwrap_or_default() };
    json!({
        "ok": true,
        "annotation": extras.annotation.clone().unwrap_or_default(),
        "subpart_first_id": extras.subpart_first_id.unwrap_or('A' as u32),
        "subpart_id_separator": extras.subpart_id_separator.unwrap_or(0),
        "field_name_templates": extras.field_name_templates,
        "bom_presets": names("bom_presets"),
        "bom_fmt_presets": names("bom_fmt_presets"),
        "bus_aliases": design.bus_aliases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Symbol Editor's Find asks the schematic's matcher: plain (any case), whole words, a regular expression, and Replace's new text.
    #[test]
    fn find_text_matches_and_replaces_with_the_schematics_matcher() {
        let ask = |search: Value, texts: &[&str], replace: bool| find_text(json!({ "search": search, "texts": texts, "replace": replace }).to_string().as_bytes());
        let r = ask(json!({ "find": "vcc" }), &["VCC", "AVCC_1", "GND"], false);
        assert_eq!(r["matches"], json!([true, true, false]), "plain, case ignored");
        let r = ask(json!({ "find": "VCC", "mode": "whole_word", "match_case": true }), &["VCC", "AVCC", "VCC_1", "VCC 3"], false);
        assert_eq!(r["matches"], json!([true, false, false, true]), "whole words: `_` is a word character");
        let r = ask(json!({ "find": "^P[0-9]+$", "mode": "regex" }), &["P12", "PA", "p3"], false);
        assert_eq!(r["matches"], json!([true, false, true]), "a regular expression, any case");
        let r = ask(json!({ "find": "vcc", "replace": "VDD" }), &["AVCC", "GND"], true);
        assert_eq!(r["replaced"], json!(["AVDD", null]));
        assert_eq!(find_text(b"{}")["ok"], json!(true), "no search is an empty one");
    }

    /// UI-LEFT-2 SS13: the table opens with the mandatory Description and the generated "#" as columns not shown (`LoadFieldNames`, then
    /// `ApplyBomPreset( DefaultEditing )`), a symbol's field named Description being that column, not a user field's of its own.
    #[test]
    fn the_table_opens_with_the_description_and_item_number_columns_hidden() {
        let mut sch = eda_model::ir::SchematicSection::default();
        sch.user_fields.entry("R1".into()).or_default().insert("Description".into(), "Resistor".into());
        sch.user_fields.entry("R1".into()).or_default().insert("MPN".into(), "X".into());
        let design = eda_model::ir::Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: eda_model::ir::Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(sch.clone()),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        let spec = default_spec(&sch, &design);
        let col = |n: &str| spec.columns.iter().filter(|c| c.name == n).cloned().collect::<Vec<_>>();
        assert_eq!(col("Description").len(), 1, "one Description column");
        assert!(!col("Description")[0].show && !col(fields_table::ITEM_NUMBER)[0].show);
        assert_eq!(col(fields_table::ITEM_NUMBER)[0].label, "#");
        assert!(col("MPN")[0].show, "a user field is shown (the studio's own default)");
    }

    #[test]
    fn output_paths_must_stay_inside_the_board_directory() {
        assert!(safe_relative("export/bom.csv").is_ok());
        assert!(safe_relative("bom.csv").is_ok());
        assert!(safe_relative("../bom.csv").is_err());
        assert!(safe_relative("/etc/bom.csv").is_err());
        assert!(safe_relative("  ").is_err());
    }
}
