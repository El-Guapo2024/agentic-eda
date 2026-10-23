//! File > Export > Specctra DSN... and File > Import > Specctra Session... (`pcbnew.EditorControl.exportSpecctraDSN`, `importSpecctraSession`), and `eda board export
//! specctra` / `eda board import-ses`.
//!
//! The DSN and the session are an exporter and an importer of our own (`eda_kicad::specctra`; kicad-cli has neither command, docs/ARCHITECTURE.md, "Engines"). The export is
//! a read of the board: it writes `export/specctra/<board>.dsn` beside the other outputs and hands the text to the page, which saves it as a download. The import is a
//! command (`eda_ops::Cmd::Specctra`, one undo step); a browser gives the page the session file's text and not its path, so the text travels in the command.

use crate::board;
use serde_json::{json, Value};
use std::path::Path;

/// The name the board's files go by: the intent's, else the board folder's.
pub(crate) fn board_name(dir: &Path) -> String {
    let from_meta = std::fs::read_to_string(dir.join("board.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|m| m["intent"].as_str().and_then(|i| Path::new(i).file_stem().and_then(|s| s.to_str()).map(str::to_string)));
    from_meta
        .or_else(|| std::fs::canonicalize(dir).ok().and_then(|d| d.file_name().and_then(|n| n.to_str()).map(str::to_string)))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "board".to_string())
}

/// `ExportBoardToSpecctraFile`: the DSN of the board in `dir`, the name it carries in the file (`pcb_name`), and what the export had to warn about.
pub(crate) fn dsn_of(dir: &Path, pcb_name: &str) -> Result<eda_kicad::specctra::DsnExport, Vec<eda_model::CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    eda_kicad::specctra::export_dsn(&design, &model, pcb_name)
}

/// `POST /api/specctra/export`: write `export/specctra/<board>.dsn`. The reply is `{ ok, message, files, warnings, name, text }`; a failure is
/// `{ ok: false, message }` with the words of `PCB_EDIT_FRAME::ExportSpecctraFile`'s dialog ("Unable to export, please fix and try again") and the reason.
pub fn export(dir: &Path) -> Value {
    let name = format!("{}.dsn", board_name(dir));
    let out = match dsn_of(dir, &name) {
        Ok(out) => out,
        Err(e) => return crate::fab_api::err(format!("Unable to export, please fix and try again: {}", board::reasons(&e))),
    };
    let out_dir = dir.join("export").join("specctra");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        return crate::fab_api::err(format!("Unable to export, please fix and try again: cannot create {}: {e}", out_dir.display()));
    }
    if let Err(e) = std::fs::write(out_dir.join(&name), &out.text) {
        return crate::fab_api::err(format!("Unable to export, please fix and try again: cannot write {}: {e}", out_dir.join(&name).display()));
    }
    json!({ "ok": true, "message": "BOARD exported OK.", "files": [format!("export/specctra/{name}")], "warnings": out.warnings, "name": name, "text": out.text })
}

/// `eda board export specctra -o FILE`: the board's DSN written to `file`; the warnings come back for the caller to print.
pub fn export_to(dir: &Path, file: &Path) -> Result<Vec<String>, Vec<eda_model::CheckResult>> {
    // `m_pcb->m_pcbname = TO_UTF8( aFilename )`: the file's name is the design's.
    let name = file.display().to_string();
    let out = dsn_of(dir, &name)?;
    std::fs::write(file, &out.text).map_err(|e| vec![eda_model::CheckResult::fail("specctra.write", &file.display().to_string(), format!("cannot write {}: {e}", file.display()))])?;
    Ok(out.warnings)
}

/// `eda board import-ses FILE`: the command File > Import > Specctra Session... sends, made of the file's text (`ImportSpecctraSession( fullFileName )`); the file's name is
/// the one the messages give.
pub fn import_cmd(file: &Path) -> Result<eda_ops::Cmd, Vec<eda_model::CheckResult>> {
    let session = std::fs::read_to_string(file).map_err(|e| vec![eda_model::CheckResult::fail("specctra.read", &file.display().to_string(), format!("cannot read {}: {e}", file.display()))])?;
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("session.ses").to_string();
    Ok(eda_ops::Cmd::Specctra { session, name })
}
