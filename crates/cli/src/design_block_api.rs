//! `/api/design_blocks/*`: the Design Blocks panels of the schematic and board editors (`DESIGN_BLOCK_PANE`) over the design block library table.
//!
//! * `GET  /api/design_blocks` -- the libraries of the table (KiCad's global one, read only, and the board's) with the blocks in them: each block's name, description,
//!   keywords, fields and which of its two designs it has. What the tree lists (`DESIGN_BLOCK_TREE_MODEL_ADAPTER::AddLibraries`, `GetDesignBlocks`).
//! * `GET  /api/design_blocks/file?lib=&name=&what=pcb|sch` -- the text of a block's board file (Place Design Block on the board reads it as Append Board reads a file).
//! * `GET  /api/design_blocks/preview?lib=&name=&what=` -- the block's schematic or board drawn by kicad-cli (`sch export svg`, `pcb export svg`), the preview of the panel.
//!   It runs in the studio's one kicad-cli lane.
//! * `POST /api/design_blocks/prepare` -- what the Save as Design Block dialogs open with and the refusals that come before them
//!   (`SCH_EDIT_FRAME::SaveSheetAsDesignBlock`, `SaveSelectionAsDesignBlock`, `UpdateDesignBlockFromSheet`, `UpdateDesignBlockFromSelection`,
//!   `PCB_EDIT_FRAME::SaveBoardAsDesignBlock`, `SaveSelectionAsDesignBlock`, `UpdateDesignBlockFromBoard`).
//! * `POST /api/design_blocks/save` -- the block written to its library and, for a selection, the design edit that follows (the selection grouped and linked to the block, one
//!   undoable step).
//! * `POST /api/design_blocks/properties` and `/delete` -- `DESIGN_BLOCK_PANE::EditDesignBlockProperties` and `DeleteDesignBlockFromLibrary`.
//!
//! Placing a block is not here: the schematic places it with `Cmd::ImportSheet` (the block's `.kicad_sch` is the file) and the board with `Cmd::PlaceDesignBlock` (the block's
//! `.kicad_pcb` text, which `/file` hands over). A block library is the person's data on disk: nothing is written but a block's own folder in a library of the table, and only
//! when the request says to.

use crate::board;
use crate::library_tables::{self, Env};
use eda_kicad::design_block::{self as db, DesignBlock};
use eda_kicad::library_table::{Lib, TableKind};
use eda_ops::Cmd;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

fn bad(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

fn parse(body: &[u8]) -> Result<Value, Value> {
    serde_json::from_slice(body).map_err(|e| bad(format!("not a design block request: {e}")))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn ids_of(v: &Value, key: &str) -> Vec<String> {
    v.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// `fields` as the dialog's grid has them: `[{name, value}]` (or `[[name, value]]`), in order.
fn fields_of(v: &Value) -> Option<Vec<(String, String)>> {
    let list = v.get("fields")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|f| match (f.get("name").and_then(Value::as_str), f.get("value").and_then(Value::as_str)) {
                (Some(n), Some(val)) => Some((n.to_string(), val.to_string())),
                _ => {
                    let a = f.as_array()?;
                    Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_str()?.to_string()))
                }
            })
            .collect(),
    )
}

// ------------------------------------------------------------------------------------------------ libraries

/// A name taken from a request that is a block's folder name and nothing that leads out of the library.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', '\0']) && name != "." && name != ".."
}

fn library_named(env: &Env, dir: &Path, nickname: &str) -> Result<Lib, String> {
    let lib = library_tables::load_with(env.clone(), dir, TableKind::DesignBlock).libs().into_iter().find(|l| l.nickname == nickname).ok_or_else(|| format!("There is no design block library '{nickname}'."))?;
    if lib.disabled {
        return Err(format!("The design block library '{nickname}' is switched off in the library table."));
    }
    if let Some(e) = &lib.error {
        return Err(e.clone());
    }
    Ok(lib)
}

fn block_of(env: &Env, dir: &Path, nickname: &str, name: &str) -> Result<(Lib, DesignBlock), String> {
    if !safe_name(name) {
        return Err(format!("'{name}' is not the name of a design block."));
    }
    let lib = library_named(env, dir, nickname)?;
    let block = db::load(&lib.path, name)?;
    Ok((lib, block))
}

/// A library to write to: in the table, readable and writable (`IsDesignBlockLibWritable`: "Library '%s' is read only.").
fn writable_library(env: &Env, dir: &Path, nickname: &str) -> Result<Lib, String> {
    let lib = library_named(env, dir, nickname)?;
    if !db::is_writable(&lib.path) {
        return Err(format!("Library '{nickname}' is read only."));
    }
    Ok(lib)
}

fn block_json(b: &DesignBlock) -> Value {
    json!({
        "name": b.name,
        "description": b.description,
        "keywords": b.keywords,
        "fields": b.fields.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(),
        "schematic": b.schematic_file.is_some(),
        "board": b.board_file.is_some(),
        "schematic_path": b.schematic_file.as_ref().map(|p| p.display().to_string()),
        "board_path": b.board_file.as_ref().map(|p| p.display().to_string()),
    })
}

/// `GET /api/design_blocks`.
pub(crate) fn list(dir: &Path) -> Value {
    list_with(&Env::discover(), dir)
}

pub(crate) fn list_with(env: &Env, dir: &Path) -> Value {
    let loaded = library_tables::load_with(env.clone(), dir, TableKind::DesignBlock);
    let mut libraries: Vec<Value> = Vec::new();
    let mut problems: Vec<Value> = Vec::new();
    for lib in loaded.libs() {
        if lib.disabled {
            continue;
        }
        if let Some(e) = &lib.error {
            problems.push(json!({ "nickname": lib.nickname, "error": e }));
            continue;
        }
        // `AddLibraries`: `if( row->Hidden() ) continue;`
        if lib.hidden {
            continue;
        }
        let mut blocks: Vec<Value> = Vec::new();
        let names = match db::enumerate(&lib.path) {
            Ok(n) => n,
            Err(e) => {
                problems.push(json!({ "nickname": lib.nickname, "error": e }));
                continue;
            }
        };
        for name in names {
            match db::load(&lib.path, &name) {
                Ok(b) => blocks.push(block_json(&b)),
                // `GetDesignBlocks`: a block that cannot be read is left out of the tree
                Err(e) => problems.push(json!({ "nickname": lib.nickname, "block": name, "error": e })),
            }
        }
        libraries.push(json!({
            "name": lib.nickname,
            "description": lib.description,
            "scope": lib.scope.name(),
            "path": lib.path.display().to_string(),
            "writable": db::is_writable(&lib.path),
            "blocks": blocks,
        }));
    }
    // a row of a table that could not be read (KiCad's default table, chained by the global one, is not there)
    let mut notes: Vec<String> = Vec::new();
    for table in loaded.global.iter().chain(loaded.project.iter()) {
        notes.extend(rows_that_failed(table));
    }
    if let Some(e) = &loaded.global_error {
        notes.push(e.clone());
    }
    if let Some(e) = &loaded.project_error {
        notes.push(e.clone());
    }
    json!({ "ok": true, "libraries": libraries, "problems": problems, "notes": notes, "project_dir": dir.display().to_string() })
}

fn rows_that_failed(table: &eda_kicad::library_table::LoadedTable) -> Vec<String> {
    let mut out = Vec::new();
    for (row, loaded) in table.table.rows.iter().zip(&table.rows) {
        if !loaded.ok {
            out.push(format!("{}: {}", row.nickname, loaded.error));
        }
        if let Some(child) = &loaded.child {
            out.extend(rows_that_failed(child));
        }
    }
    out
}

/// `GET /api/design_blocks/file?lib=&name=&what=`: the text of the block's board (`pcb`) or schematic (`sch`) file.
pub(crate) fn file(dir: &Path, lib: &str, name: &str, what: &str) -> Value {
    file_with(&Env::discover(), dir, lib, name, what)
}

pub(crate) fn file_with(env: &Env, dir: &Path, lib: &str, name: &str, what: &str) -> Value {
    let (_, block) = match block_of(env, dir, lib, name) {
        Ok(b) => b,
        Err(e) => return bad(e),
    };
    let (path, none) = match what {
        "sch" => (block.schematic_file, "Design block has no schematic to place."),
        _ => (block.board_file, "Design block has no layout to place."),
    };
    let Some(path) = path else { return bad(none) };
    match std::fs::read_to_string(&path) {
        Ok(t) => json!({ "ok": true, "text": t, "path": path.display().to_string(), "name": block.name }),
        Err(e) => bad(format!("{}: {e}", path.display())),
    }
}

// ------------------------------------------------------------------------------------------------ the preview

static PREVIEWS: OnceLock<Mutex<HashMap<(PathBuf, u64, u64), String>>> = OnceLock::new();

/// The block's file drawn as an SVG by kicad-cli, cached by the file's place, size and time.
pub(crate) fn preview(dir: &Path, lib: &str, name: &str, what: &str) -> Value {
    preview_with(&Env::discover(), dir, lib, name, what)
}

pub(crate) fn preview_with(env: &Env, dir: &Path, lib: &str, name: &str, what: &str) -> Value {
    let (_, block) = match block_of(env, dir, lib, name) {
        Ok(b) => b,
        Err(e) => return bad(e),
    };
    let (scope, file, ext) = match what {
        "sch" => ("sch", block.schematic_file, "kicad_sch"),
        _ => ("pcb", block.board_file, "kicad_pcb"),
    };
    let Some(file) = file else { return bad(if scope == "sch" { "Design block has no schematic." } else { "Design block has no layout." }) };
    let meta = std::fs::metadata(&file).ok();
    let stamp = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
    let key = (file.clone(), meta.as_ref().map_or(0, |m| m.len()), stamp);
    let cache = PREVIEWS.get_or_init(Default::default);
    if let Some(svg) = cache.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        return json!({ "ok": true, "svg": svg, "what": what, "cached": true });
    }
    // kicad-cli reads a file by its path: a copy of the block's file under a name of ours, so nothing in the library is touched and an odd name is no trouble
    let scratch = std::env::temp_dir().join(format!("eda-design-block-preview-{}-{}", std::process::id(), stamp));
    let _ = std::fs::remove_dir_all(&scratch);
    let result = (|| -> Result<String, String> {
        std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
        let input = scratch.join(format!("preview.{ext}"));
        std::fs::copy(&file, &input).map_err(|e| format!("{}: {e}", file.display()))?;
        eda_kicad_engine::export_file_svg(scope, &input, &scratch.join("out")).map_err(|e| board::reasons(&e))
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    match result {
        Ok(svg) => {
            let mut c = cache.lock().unwrap_or_else(PoisonError::into_inner);
            if c.len() >= 48 {
                c.clear();
            }
            c.insert(key, svg.clone());
            json!({ "ok": true, "svg": svg, "what": what, "cached": false })
        }
        Err(e) => bad(e),
    }
}

// ------------------------------------------------------------------------------------------------ the design

/// The board the way the studio shows it: a board with no stored schematic has the one derived from its intent.
fn loaded(dir: &Path) -> Result<(board::Meta, eda_model::ir::Design, eda_model::ConstraintModel), String> {
    let (meta, mut design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model).map_err(|e| board::reasons(&e))?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    Ok((meta, design, model))
}

fn project_name(meta: &board::Meta, dir: &Path) -> String {
    Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).map(str::to_string).or_else(|| dir.file_name().and_then(|n| n.to_str()).map(str::to_string)).unwrap_or_else(|| "board".into())
}

/// The file the sheet at `sheet_path` (`/`-joined sheet ids, empty for the root) is kept in, and its name.
fn sheet_file_and_name(design: &eda_model::ir::Design, sheet_path: &str) -> Option<(Option<String>, String)> {
    let mut section = design.schematic.as_ref()?;
    let mut found: Option<(String, String)> = None;
    for id in sheet_path.split('/').filter(|s| !s.is_empty()) {
        let sheet = section.sheets.iter().find(|s| s.id == id)?;
        section = design.sheet_contents.as_ref()?.get(&sheet.file)?;
        found = Some((sheet.file.clone(), sheet.name.clone()));
    }
    Some(match found {
        Some((file, name)) => (Some(file), name),
        None => (None, String::new()),
    })
}

/// A schematic design block's file text, and what it is made of.
struct Made {
    text: String,
    /// The name the dialog opens with.
    default_name: String,
    /// What the dialog's field grid starts with.
    fields: Vec<(String, String)>,
    items: usize,
}

/// `SaveSheetAsDesignBlock`: the sheet's screen as a file. "Design blocks with nested sheets are not supported."
fn sheet_made(dir: &Path, sheet_path: &str) -> Result<Made, String> {
    let (meta, design, model) = loaded(dir)?;
    let project = project_name(&meta, dir);
    let (section, _) = crate::studio::resolve_sheet(&design, sheet_path);
    if !section.sheets.is_empty() {
        return Err("Design blocks with nested sheets are not supported.".into());
    }
    let (file, name) = sheet_file_and_name(&design, sheet_path).ok_or_else(|| "That sheet is not in the design.".to_string())?;
    let files = eda_kicad::export_kicad_sch_tree(&design, &model, &eda_kicad::ExportMeta { date: &crate::kicad_engine::chrono_like_today(), title: &project }, "board.kicad_sch").map_err(|e| board::reasons(&e))?;
    // the screen is written where the derived tree puts it: a sheet above the root's folder under `__up__/` (`sheet_file_in_work`, r4 B4-M1)
    let wanted = file.as_deref().map(eda_kicad::sheet_file_in_work).unwrap_or_else(|| "board.kicad_sch".into());
    let text = files.into_iter().find(|(n, _)| *n == wanted).map(|(_, t)| t).ok_or_else(|| format!("The file of the sheet ({wanted}) was not written."))?;
    let items = section.symbols.len() + section.power_symbols.len() + section.wires.len() + section.labels.len() + section.texts.len();
    Ok(Made { text, default_name: if name.is_empty() { project } else { name }, fields: Vec::new(), items })
}

/// `SaveSelectionAsDesignBlock`: the selected items as a file. A selection with a sheet is the sheet's own save when it is the only thing selected, else refused.
fn selection_made(dir: &Path, sheet_path: &str, ids: &[String]) -> Result<Made, String> {
    if ids.is_empty() {
        return Err("Please select some items to save as a design block.".into());
    }
    let (meta, design, model) = loaded(dir)?;
    let project = project_name(&meta, dir);
    let (section, _) = crate::studio::resolve_sheet(&design, sheet_path);
    let sheet_ids: Vec<&String> = ids.iter().filter(|id| section.sheets.iter().any(|s| &s.id == *id)).collect();
    if !sheet_ids.is_empty() {
        return Err(if ids.len() == 1 { "SHEET".to_string() } else { "Design blocks with nested sheets are not supported.".to_string() });
    }
    let out = eda_kicad::write_clipboard(&eda_kicad::CopyInput { design: &design, model: &model, section: &section, project: &project, sheet_path: &[] }, ids).map_err(|e| board::reasons(&e))?;
    if out.items == 0 {
        return Err("Please select some items to save as a design block.".into());
    }
    if !out.sheet_files.is_empty() {
        return Err("Design blocks with nested sheets are not supported.".into());
    }
    // a single group named by the person: the block is named after it (`group->GetName()`), else the screen's name
    let default_name = match ids {
        [one] => section.extras.groups.iter().find(|g| &g.id == one).map(|g| g.name.clone()).filter(|n| !n.is_empty()),
        _ => None,
    }
    .unwrap_or_else(|| sheet_file_and_name(&design, sheet_path).and_then(|(file, name)| file.map(|f| f.trim_end_matches(".kicad_sch").to_string()).or(Some(name))).filter(|n| !n.is_empty()).unwrap_or(project));
    Ok(Made { text: eda_kicad::fragment_as_sheet_file(&out.text), default_name, fields: Vec::new(), items: out.items })
}

/// `SaveBoardAsDesignBlock`: the board as the derived `.kicad_pcb`.
fn board_made(dir: &Path) -> Result<Made, String> {
    let (meta, design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let project = project_name(&meta, dir);
    let fills = crate::fill_api::cached(dir);
    let text = eda_kicad::export_kicad_pcb_with(&design, &model, &eda_kicad::ExportMeta { date: &crate::kicad_engine::chrono_like_today(), title: &project }, fills.as_ref()).map_err(|e| board::reasons(&e))?;
    let items = design.placement.as_ref().map_or(0, |p| p.footprints.len());
    if items == 0 && design.routing.as_ref().is_none_or(|r| r.tracks.is_empty()) && design.drawings.as_ref().is_none_or(|d| d.shapes.is_empty()) {
        return Err("The board has nothing in it to save as a design block.".into());
    }
    Ok(Made { text, default_name: project, fields: Vec::new(), items })
}

/// `saveSelectionToDesignBlock`: a temporary board with the selected items alone -- clones of them, with the nets they use and without the group they were picked by --
/// saved as a board file. Here the temporary board is the design with everything else taken out: the footprints, the copper, the graphics and the texts that were not
/// selected, the board's outline, and the nets nothing selected is on. A group in the selection brings the items below it (`DeepClone`).
fn board_selection_made(dir: &Path, ids: &[String]) -> Result<Made, String> {
    if ids.is_empty() {
        return Err("Please select some items to save as a design block.".into());
    }
    let (meta, design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let project = project_name(&meta, dir);
    let leaves: Vec<String> = design.drawings.as_ref().map_or_else(|| ids.to_vec(), |d| d.expand_groups(ids));
    let keep: std::collections::BTreeSet<&str> = leaves.iter().map(String::as_str).collect();
    let mut d = design.clone();
    let mut m = model.clone();
    if let Some(p) = d.placement.as_mut() {
        p.footprints.retain(|f| keep.contains(f.id.as_str()));
        p.outline.clear();
        p.modules.clear();
    }
    if let Some(r) = d.routing.as_mut() {
        r.tracks.retain(|t| keep.contains(t.id.as_str()));
        r.vias.retain(|v| keep.contains(v.id.as_str()));
        r.zones.retain(|z| keep.contains(z.id.as_str()));
    }
    let mut used_nets: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    if let Some(r) = d.routing.as_ref() {
        used_nets.extend(r.tracks.iter().map(|t| t.net.clone()));
        used_nets.extend(r.vias.iter().map(|v| v.net.clone()));
        used_nets.extend(r.zones.iter().map(|z| z.net.clone()));
    }
    if let Some(dr) = d.drawings.as_mut() {
        dr.shapes.retain(|s| keep.contains(s.id()));
        dr.texts.retain(|t| keep.contains(t.id.as_str()));
        dr.dimensions.retain(|x| keep.contains(x.id.as_str()));
        dr.board_parts.retain(|p| keep.contains(p.reference.as_str()));
        dr.footprint_edits.retain(|e| keep.contains(e.id.as_str()));
        dr.zone_overrides.retain(|z| keep.contains(z.id.as_str()));
        // the groups inside the selection stay (with the members that stay), the group the selection was picked by does not
        let picked: std::collections::BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        dr.groups.retain(|g| !picked.contains(g.id.as_str()));
        // the groups that lost members to the cut fall under the `< 2` rule; a group of one that was one already stays
        let affected: std::collections::BTreeSet<String> = dr.groups.iter().filter(|g| g.member_ids.iter().any(|x| !keep.contains(x.as_str()))).map(|g| g.id.clone()).collect();
        for g in dr.groups.iter_mut() {
            g.member_ids.retain(|x| keep.contains(x.as_str()));
        }
        eda_model::groups::prune_affected_groups(&mut dr.groups, &affected);
    }
    let placed: std::collections::BTreeSet<String> = d.placement.iter().flat_map(|p| p.footprints.iter().map(|f| f.id.clone())).collect();
    for n in m.nets.iter_mut() {
        n.pins.retain(|p| p.split_once('.').is_some_and(|(r, _)| placed.contains(r)));
    }
    m.nets.retain(|n| !n.pins.is_empty() || used_nets.contains(&n.name));
    let items = d.placement.as_ref().map_or(0, |p| p.footprints.len())
        + d.routing.as_ref().map_or(0, |r| r.tracks.len() + r.vias.len() + r.zones.len())
        + d.drawings.as_ref().map_or(0, |x| x.shapes.len() + x.texts.len() + x.dimensions.len());
    if items == 0 {
        return Err("Please select some items to save as a design block.".into());
    }
    let text = eda_kicad::export_kicad_pcb_with(&d, &m, &eda_kicad::ExportMeta { date: &crate::kicad_engine::chrono_like_today(), title: &project }, None).map_err(|e| board::reasons(&e))?;
    let default_name = match ids {
        [one] => design.drawings.as_ref().and_then(|x| x.group(one)).map(|g| g.name.clone()).filter(|n| !n.is_empty()),
        _ => None,
    }
    .unwrap_or(project);
    Ok(Made { text, default_name, fields: Vec::new(), items })
}

/// What a request asks to be saved: the design of the sheet in view, a selection of it, the board, or a selection of the board.
enum Source {
    Sheet(String),
    Selection(String, Vec<String>),
    Board,
    BoardSelection(Vec<String>),
}

impl Source {
    fn of(req: &Value) -> Result<Source, String> {
        let sheet = text(req, "sheet");
        match req.get("what").and_then(Value::as_str) {
            Some("sheet") => Ok(Source::Sheet(sheet)),
            Some("selection") => Ok(Source::Selection(sheet, ids_of(req, "ids"))),
            Some("board") => Ok(Source::Board),
            Some("board_selection") => Ok(Source::BoardSelection(ids_of(req, "ids"))),
            _ => Err("what is sheet, selection, board or board_selection".into()),
        }
    }

    fn is_board(&self) -> bool {
        matches!(self, Source::Board | Source::BoardSelection(_))
    }

    /// The file this source makes. A selection that is one sheet is that sheet's save.
    fn make(&self, dir: &Path) -> Result<Made, String> {
        match self {
            Source::Sheet(sheet) => sheet_made(dir, sheet),
            Source::Selection(sheet, ids) => match selection_made(dir, sheet, ids) {
                Err(e) if e == "SHEET" => {
                    let path = if sheet.is_empty() { ids[0].clone() } else { format!("{sheet}/{}", ids[0]) };
                    sheet_made(dir, &path)
                }
                other => other,
            },
            Source::Board => board_made(dir),
            Source::BoardSelection(ids) => board_selection_made(dir, ids),
        }
    }

    /// The design edit that follows a save of a selection (not of a sheet or a whole board): the items grouped and linked to the block.
    fn link(&self, dir: &Path, lib_id: &str) -> Option<Result<String, String>> {
        let (cmd, ids) = match self {
            Source::Selection(sheet, ids) => {
                // a lone sheet is the sheet's save, which makes no group
                let (_, design, _) = loaded(dir).ok()?;
                let (section, _) = crate::studio::resolve_sheet(&design, sheet);
                if ids.iter().any(|id| section.sheets.iter().any(|s| &s.id == id)) {
                    return None;
                }
                let groupable = eda_ops::groupable_ids(&section);
                let ids: Vec<String> = ids.iter().filter(|id| groupable.contains(*id)).cloned().collect();
                let cmd = Cmd::SchEdit(eda_ops::sch_edit::SchCmd::LinkDesignBlock { ids: ids.clone(), block: lib_id.to_string() });
                (if sheet.is_empty() { cmd } else { Cmd::OnSheet { sheet: sheet.clone(), cmd: Box::new(cmd) } }, ids)
            }
            Source::BoardSelection(ids) => (Cmd::LinkDesignBlock { ids: ids.clone(), block: lib_id.to_string() }, ids.clone()),
            _ => return None,
        };
        if ids.is_empty() {
            return None;
        }
        Some(board::step(dir, cmd, false, "ui").map_err(|e| board::reasons(&e)))
    }
}

/// `POST /api/design_blocks/prepare` `{what, ids?, sheet?, lib, update?}`: whether the save can go on, and what its dialog opens with. `update` names a block to update
/// (`{name}`): its metadata is what the dialog shows, the name cannot be edited, and `has_design` says whether the block already has the design the save would replace
/// (`checkOverwriteDbSchematic`, `checkOverwriteDbLayout`).
pub(crate) fn prepare(dir: &Path, body: &[u8]) -> Value {
    prepare_with(&Env::discover(), dir, body)
}

pub(crate) fn prepare_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    let req = match parse(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let lib = text(&req, "lib");
    if lib.is_empty() {
        return bad(if req.get("update").is_some() { "Please select a design block to save the schematic to." } else { "Please select a library to save the design block to." });
    }
    let source = match Source::of(&req) {
        Ok(s) => s,
        Err(e) => return bad(e),
    };
    let library = match writable_library(env, dir, &lib) {
        Ok(l) => l,
        Err(e) => return bad(e),
    };
    let made = match source.make(dir) {
        Ok(m) => m,
        Err(e) => return bad(e),
    };
    match req.get("update") {
        Some(update) => {
            let name = text(update, "name");
            let block = match block_of(env, dir, &lib, &name) {
                Ok((_, b)) => b,
                Err(_) => return bad(format!("Design block '{name}' does not exist.")),
            };
            let has_design = if source.is_board() { block.board_file.is_some() } else { block.schematic_file.is_some() };
            // the sheet's own fields are copied over the block's, the ones the block has and the sheet has not left alone
            let mut fields = block.fields.clone();
            for (n, v) in &made.fields {
                match fields.iter_mut().find(|(x, _)| x == n) {
                    Some(slot) => slot.1 = v.clone(),
                    None => fields.push((n.clone(), v.clone())),
                }
            }
            json!({ "ok": true, "name": block.name, "description": block.description, "keywords": block.keywords, "fields": fields.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(), "has_design": has_design, "items": made.items, "library": library.nickname })
        }
        None => json!({ "ok": true, "name": made.default_name, "description": "", "keywords": "", "fields": made.fields.iter().map(|(n, v)| json!({ "name": n, "value": v })).collect::<Vec<_>>(), "items": made.items, "library": library.nickname }),
    }
}

/// A fresh scratch file path for the text a block is written from.
fn scratch_file(ext: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::env::temp_dir().join(format!("design_block_{}_{}.{ext}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
}

/// `POST /api/design_blocks/save` `{what, ids?, sheet?, lib, name, description?, keywords?, fields?, overwrite?, update?}`. A new block that exists already is refused unless
/// `overwrite` is on (`checkOverwriteDb`: the answer says `exists`); with `update` the block must exist and keeps what the request does not give.
pub(crate) fn save(dir: &Path, body: &[u8]) -> Value {
    save_with(&Env::discover(), dir, body)
}

pub(crate) fn save_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    let req = match parse(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let lib = text(&req, "lib");
    let name = text(&req, "name");
    if lib.is_empty() {
        return bad("Please select a library to save the design block to.");
    }
    let source = match Source::of(&req) {
        Ok(s) => s,
        Err(e) => return bad(e),
    };
    let library = match writable_library(env, dir, &lib) {
        Ok(l) => l,
        Err(e) => return bad(e),
    };
    let updating = req.get("update").and_then(Value::as_bool).unwrap_or(false);
    let exists = db::exists(&library.path, &name);
    if updating && !exists {
        return bad(format!("Design block '{name}' does not exist."));
    }
    if !updating {
        if let Some(problem) = db::name_problem(&name) {
            return bad(problem);
        }
        if exists && req.get("overwrite").and_then(Value::as_bool) != Some(true) {
            return json!({ "ok": false, "exists": true, "message": format!("Design block '{name}' already exists in library '{lib}'.") });
        }
    }
    let made = match source.make(dir) {
        Ok(m) => m,
        Err(e) => return bad(e),
    };
    // the block: what the library has of it (an update, or a block written over), then what the request gives
    let mut block = if exists { db::load(&library.path, &name).unwrap_or_else(|_| DesignBlock { name: name.clone(), ..Default::default() }) } else { DesignBlock { name: name.clone(), ..Default::default() } };
    block.name = name.clone();
    if req.get("description").is_some() {
        block.description = text(&req, "description");
    }
    if req.get("keywords").is_some() {
        block.keywords = text(&req, "keywords");
    }
    if let Some(fields) = fields_of(&req) {
        match db::clean_fields(&fields) {
            Ok(f) => block.fields = f,
            Err(e) => return bad(e),
        }
    }
    let ext = if source.is_board() { "kicad_pcb" } else { "kicad_sch" };
    let temp = scratch_file(ext);
    if let Err(e) = std::fs::write(&temp, &made.text) {
        return bad(format!("Error saving temporary file to create design block: {e}"));
    }
    if source.is_board() {
        block.board_file = Some(temp.clone());
    } else {
        block.schematic_file = Some(temp.clone());
    }
    let saved = db::save(&library.path, &block);
    let _ = std::fs::remove_file(&temp);
    if let Err(e) = saved {
        return bad(e);
    }
    let lib_id = format!("{lib}:{name}");
    let (linked, link_error) = match source.link(dir, &lib_id) {
        Some(Ok(_)) => (true, None),
        Some(Err(e)) => (false, Some(e)),
        None => (false, None),
    };
    json!({ "ok": true, "lib": lib, "name": name, "lib_id": lib_id, "created": !exists, "linked": linked, "link_error": link_error })
}

/// `POST /api/design_blocks/properties` `{lib, name, new_name?, description, keywords, fields, overwrite?}`: `DESIGN_BLOCK_PANE::EditDesignBlockProperties`. A new name saves the
/// block under it and deletes the old one; a name the library has is refused unless `overwrite` says to replace it.
pub(crate) fn properties(dir: &Path, body: &[u8]) -> Value {
    properties_with(&Env::discover(), dir, body)
}

pub(crate) fn properties_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    let req = match parse(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let (lib, name) = (text(&req, "lib"), text(&req, "name"));
    let library = match writable_library(env, dir, &lib) {
        Ok(l) => l,
        Err(e) => return bad(e),
    };
    if !safe_name(&name) {
        return bad(format!("'{name}' is not the name of a design block."));
    }
    let mut block = match db::load(&library.path, &name) {
        Ok(b) => b,
        Err(e) => return bad(format!("Error loading design block {name} from library '{lib}'.\n{e}")),
    };
    let new_name = req.get("new_name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| name.clone());
    if let Some(problem) = db::name_problem(&new_name) {
        return bad(problem);
    }
    block.name = new_name.clone();
    block.description = text(&req, "description");
    block.keywords = text(&req, "keywords");
    if let Some(fields) = fields_of(&req) {
        match db::clean_fields(&fields) {
            Ok(f) => block.fields = f,
            Err(e) => return bad(e),
        }
    }
    let renamed = new_name != name;
    if renamed && db::exists(&library.path, &new_name) && req.get("overwrite").and_then(Value::as_bool) != Some(true) {
        return json!({ "ok": false, "exists": true, "message": format!("Design block '{new_name}' already exists in library '{lib}'.") });
    }
    if let Err(e) = db::save(&library.path, &block) {
        return bad(e);
    }
    if renamed {
        if let Err(e) = db::delete(&library.path, &name) {
            return bad(e);
        }
    }
    json!({ "ok": true, "lib": lib, "name": new_name, "lib_id": format!("{lib}:{new_name}"), "renamed": renamed })
}

/// `POST /api/design_blocks/delete` `{lib, name}`: `DESIGN_BLOCK_PANE::DeleteDesignBlockFromLibrary`. The block's whole folder.
pub(crate) fn delete(dir: &Path, body: &[u8]) -> Value {
    delete_with(&Env::discover(), dir, body)
}

pub(crate) fn delete_with(env: &Env, dir: &Path, body: &[u8]) -> Value {
    let req = match parse(body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let (lib, name) = (text(&req, "lib"), text(&req, "name"));
    let library = match writable_library(env, dir, &lib) {
        Ok(l) => l,
        Err(e) => return bad(e),
    };
    if !safe_name(&name) {
        return bad(format!("'{name}' is not the name of a design block."));
    }
    match db::delete(&library.path, &name) {
        Ok(()) => json!({ "ok": true, "message": format!("Design block '{name}' deleted from library '{lib}'") }),
        Err(e) => bad(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r4 B4-M1 (`SCH_DESIGN_BLOCK_CONTROL::SaveSheetAsDesignBlock` saves the sheet's screen, wherever its file is): a sheet whose file is above the root's folder
    /// (`../shared/amp.kicad_sch`) is written as `__up__/shared/amp.kicad_sch` in the derived tree (179c5cf5), and its design block is that screen.
    #[test]
    fn a_sheet_above_the_root_folder_saves_as_a_design_block() {
        let base = std::env::temp_dir().join(format!("eda_dblock_up_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("proj")).unwrap();
        std::fs::create_dir_all(base.join("shared")).unwrap();
        let root = r#"(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0") (uuid "11111111-1111-1111-1111-111111111111") (paper "A4")
 (sheet (at 100 100) (size 20 20) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (uuid "21111111-1111-1111-1111-111111111111")
  (property "Sheetname" "AMP" (at 100 99 0) (effects (font (size 1.27 1.27)) (justify left bottom)))
  (property "Sheetfile" "../shared/amp.kicad_sch" (at 100 121 0) (effects (font (size 1.27 1.27)) (justify left top)))
  (instances (project "root" (path "/11111111-1111-1111-1111-111111111111" (page "2")))))
 (sheet_instances (path "/" (page "1"))))
"#;
        let amp = r#"(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0") (uuid "41111111-1111-1111-1111-111111111111") (paper "A4")
 (text "AMPLIFIER" (exclude_from_sim no) (at 50 50 0) (effects (font (size 1.27 1.27))) (uuid "51111111-1111-1111-1111-111111111111")))
"#;
        std::fs::write(base.join("proj/root.kicad_sch"), root).unwrap();
        std::fs::write(base.join("shared/amp.kicad_sch"), amp).unwrap();
        let dir = base.join("proj/board");
        crate::import_kicad::run(&[base.join("proj/root.kicad_sch").display().to_string(), "-o".into(), dir.display().to_string()]).unwrap();
        let (_, design, _) = board::load(&dir).unwrap();
        let sheet = design.schematic.as_ref().unwrap().sheets[0].id.clone();
        let made = sheet_made(&dir, &sheet).unwrap_or_else(|e| panic!("{e}"));
        assert!(made.text.contains("AMPLIFIER"), "the amp screen: {}", made.text);
        let _ = std::fs::remove_dir_all(&base);
    }
}
