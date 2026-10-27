//! File > Export > Specctra DSN... and File > Import > Specctra Session... (`pcbnew.EditorControl.exportSpecctraDSN`, `importSpecctraSession`), and `eda board export
//! specctra` / `eda board import-ses`.
//!
//! The DSN is kicad-cli's (`kicad-cli pcb export specctra`, docs/ARCHITECTURE.md, "Engines") when the kicad-cli found has that command, which this process asks
//! it once (`eda_kicad_engine::cli_has_export`); a kicad-cli without it (10.99.0-5334 and older) gets the board exported by our own port of `FromBOARD`
//! (`eda_kicad::specctra::export_dsn`), a fallback kept until every machine has a kicad-cli with the command. The session import (`eda_ops::Cmd::Specctra`) is
//! KiCad's own where kicad-cli has `api-server` ([`kicad_session_importer`]: `ImportSpecctraSession` on a server kept up by `eda_kicad_engine::api`), else our port
//! of `FromSESSION` (`eda_kicad::specctra::session`, INACTIVE but as that fallback); `EDA_KICAD_API=0` forces the fallback.
//!
//! The export is a read of the board: it writes `export/specctra/<board>.dsn` beside the other outputs and hands the text to the page, which saves it as a download;
//! the reply says which engine wrote it. The import is a command (one undo step); a browser gives the page the session file's text and not its path, so the text
//! travels in the command.

use crate::board;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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

/// Which engine writes the DSN: `"kicad-cli"` or `"eda"`, the reply's `engine`.
pub(crate) enum Engine {
    KicadCli,
    /// DEPRECATED (fallback for kicad-cli without pcb export specctra; delete once every machine has it)
    Eda,
}

impl Engine {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Engine::KicadCli => "kicad-cli",
            Engine::Eda => "eda",
        }
    }
}

/// Set to anything, the export takes our own exporter as if kicad-cli had no `pcb export specctra` (the fallback's tests; a kicad-cli whose exporter is broken).
pub(crate) const NO_CLI_ENV: &str = "EDA_KICAD_CLI_NO_SPECCTRA";

/// The engine of this machine: kicad-cli when it has `pcb export specctra` (asked once per process and kicad-cli binary), else ours.
pub(crate) fn engine() -> Engine {
    if std::env::var_os(NO_CLI_ENV).is_some() {
        return Engine::Eda;
    }
    match eda_kicad_engine::find_cli() {
        Some(cli) if eda_kicad_engine::cli_has_export(&cli, "pcb", "specctra") => Engine::KicadCli,
        _ => Engine::Eda,
    }
}

/// `ExportBoardToSpecctraFile` of the board in `dir` into `cwd.join(output)`, the design named `output` in the file (`m_pcbname = aFilename`): by kicad-cli, or
/// by our own exporter when this machine's kicad-cli cannot ([`engine`]). Answers the warnings and the engine; an error is the reason KiCad's dialog shows after
/// "Unable to export, please fix and try again" -- kicad-cli's words, or ours.
fn write_dsn(dir: &Path, cwd: &Path, output: &str) -> Result<(Vec<String>, Engine), Vec<eda_model::CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    match engine() {
        Engine::KicadCli => {
            let warnings = eda_kicad_engine::export_specctra(&design, &model, &dir.join(".kicad"), &board_name(dir), cwd, output)?;
            Ok((warnings, Engine::KicadCli))
        }
        // DEPRECATED (fallback for kicad-cli without pcb export specctra; delete once every machine has it)
        Engine::Eda => {
            let out = eda_kicad::specctra::export_dsn(&design, &model, output)?;
            let file = cwd.join(output);
            std::fs::write(&file, &out.text).map_err(|e| vec![eda_model::CheckResult::fail("specctra.write", &file.display().to_string(), format!("cannot write {}: {e}", file.display()))])?;
            Ok((out.warnings, Engine::Eda))
        }
    }
}

/// `POST /api/specctra/export`: write `export/specctra/<board>.dsn`. The reply is `{ ok, message, files, warnings, name, text, engine }` (`engine`: `"kicad-cli"`
/// or `"eda"`, which wrote it); a failure is `{ ok: false, message }` with the words of `PCB_EDIT_FRAME::ExportSpecctraFile`'s dialog ("Unable to export, please
/// fix and try again") and the reason.
pub fn export(dir: &Path) -> Value {
    let name = format!("{}.dsn", board_name(dir));
    let out_dir = dir.join("export").join("specctra");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        return crate::fab_api::err(format!("Unable to export, please fix and try again: cannot create {}: {e}", out_dir.display()));
    }
    // Written in its own folder under its bare name: the design in the file is called `<board>.dsn`, as the file the page saves.
    let (warnings, engine) = match write_dsn(dir, &out_dir, &name) {
        Ok(out) => out,
        Err(e) => return crate::fab_api::err(format!("Unable to export, please fix and try again: {}", board::reasons(&e))),
    };
    let text = match std::fs::read_to_string(out_dir.join(&name)) {
        Ok(t) => t,
        Err(e) => return crate::fab_api::err(format!("Unable to export, please fix and try again: cannot read {}: {e}", out_dir.join(&name).display())),
    };
    json!({ "ok": true, "message": "BOARD exported OK.", "files": [format!("export/specctra/{name}")], "warnings": warnings, "name": name, "text": text, "engine": engine.name() })
}

/// `eda board export specctra -o FILE`: the board's DSN written to `file` (relative to this process's folder, as given); the warnings and the engine come back for
/// the caller to print.
pub fn export_to(dir: &Path, file: &Path) -> Result<(Vec<String>, &'static str), Vec<eda_model::CheckResult>> {
    // `m_pcb->m_pcbname = TO_UTF8( aFilename )`: the file's name, as given, is the design's.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (warnings, engine) = write_dsn(dir, &cwd, &file.display().to_string())?;
    Ok((warnings, engine.name()))
}

/// `eda board import-ses FILE`: the command File > Import > Specctra Session... sends, made of the file's text (`ImportSpecctraSession( fullFileName )`); the file's name is
/// the one the messages give.
pub fn import_cmd(file: &Path) -> Result<eda_ops::Cmd, Vec<eda_model::CheckResult>> {
    let session = std::fs::read_to_string(file).map_err(|e| vec![eda_model::CheckResult::fail("specctra.read", &file.display().to_string(), format!("cannot read {}: {e}", file.display()))])?;
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("session.ses").to_string();
    Ok(eda_ops::Cmd::Specctra { session, name })
}

/// KiCad's warning of the last session import that skipped items, until the studio's `/api/cmd` takes it for its reply ([`take_session_warning`]).
static SESSION_WARNING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The warning of the session import just applied, if KiCad skipped items of it (taken: the next import starts without one).
pub(crate) fn take_session_warning() -> Option<String> {
    SESSION_WARNING.lock().ok().and_then(|mut w| w.take())
}

/// Set to anything, `kicad_session_importer` reports which engine read each session on stderr (`eda board import-ses` prints it; the tests read it).
pub(crate) const SAY_ENGINE_ENV: &str = "EDA_SAY_SESSION_ENGINE";

/// The importer `Cmd::Specctra` asks first (`eda_ops::set_session_importer`, set in `main`): KiCad's `ImportSpecctraSession` through the api-server lane. `None`
/// sends the command to our own port: the lane is closed (no `api-server` in this kicad-cli, `EDA_KICAD_API=0`), the server lacks the command (before KiCad
/// 11.0), or it failed twice under the request. A session KiCad refuses is refused in its words. What KiCad skipped (a part of another board's session) and its
/// report go to stderr.
pub(crate) fn kicad_session_importer(text: &str, source: &str, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Option<Result<eda_kicad::specctra::session::SessionImport, String>> {
    let say = std::env::var_os(SAY_ENGINE_ENV).is_some();
    match eda_kicad_engine::api::specctra::import_session(text, source, design, model) {
        Ok(r) => {
            if r.skipped > 0 {
                // KiCad's warning (`SPECCTRA_DB::FromSESSION`: "%d session item(s) were skipped due to unresolved reference, layer, or padstack."), for the
                // CLI's log and the studio's reply (`take_session_warning`).
                let words = if r.report.trim().is_empty() { format!("{} session item(s) were skipped due to unresolved reference, layer, or padstack.", r.skipped) } else { r.report.trim().to_string() };
                eprintln!("{words}");
                if let Ok(mut w) = SESSION_WARNING.lock() {
                    *w = Some(words);
                }
            }
            if say {
                eprintln!("session engine: kicad-cli api-server {}", r.engine);
            }
            Some(Ok(r.import))
        }
        Err(e) if e.falls_back() => {
            if say {
                eprintln!("session engine: eda ({e})");
            }
            None
        }
        Err(e) => Some(Err(match &e {
            eda_kicad_engine::api::ApiError::Status { message, .. } if !message.is_empty() => message.clone(),
            _ => e.to_string(),
        })),
    }
}
