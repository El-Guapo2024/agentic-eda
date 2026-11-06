//! Page Settings' drawing sheet file (`DIALOG_PAGES_SETTINGS`, common/dialogs/dialog_page_settings.cpp, commit 8303b2ad) on the server: where the name the
//! design keeps (`DrawingsSection::drawing_sheet`, `SchExtras::drawing_sheet`; see `eda_ops::drawing_sheet`) points, the file's text for the studio to
//! read and draw (`kicad-port/dsDataModel.ts` is the port of `DRAWING_SHEET_PARSER`), and the name a picked file is kept under.
//!
//! - `FILENAME_RESOLVER::ResolvePath( fileName, m_projectPath )`: the name's variables expanded (the board's own, KiCad's, the environment's;
//!   `${KIPRJMOD}` is the board's folder) and a relative name taken against the board's folder ([`resolve`]).
//! - `DS_DATA_MODEL::LoadDrawingSheet`'s messages: "File not found." and "File could not be opened." ([`state_json`]).
//! - `OnWksFileSelection`'s name: relative to the project when the file is in its folder (`MakeRelativeTo`), else shortened with a path variable
//!   (`NormalizePath( fileName, &Pgm().GetLocalEnvVariables(), nullptr )`), else whole ([`short_name`]).
//! - kicad-cli's plots take the resolved file with `--drawing-sheet` ([`plot_args`]), as `EESCHEMA_JOBS_HANDLER::loadSheet` /
//!   `PCBNEW_JOBS_HANDLER`'s override does.
//!
//! The browser cannot see the machine's folders: the dialog browses them through `GET /api/fs/list?what=drawing_sheet` (the `.kicad_wks` files, KiCad's
//! template folder one click away), and a file from the person's own computer is written into the board's folder first (`POST /api/drawing_sheet/upload`).

use crate::library_tables::{normalize_vars, Env};
use eda_kicad::library_table as lt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The largest drawing sheet read (a sheet with a big logo bitmap is a few hundred kilobytes).
const MAX_SHEET_BYTES: u64 = 16 * 1024 * 1024;

fn bad(msg: impl Into<String>) -> Value {
    json!({ "ok": false, "error": msg.into() })
}

/// `FILENAME_RESOLVER::ResolvePath`: the file a kept name points at for the board in `dir`.
pub(crate) fn resolve(dir: &Path, name: &str) -> PathBuf {
    resolve_with(&Env::discover(), dir, name)
}

fn resolve_with(env: &Env, dir: &Path, name: &str) -> PathBuf {
    env.vars(dir, &crate::text_variables::project_variables(dir)).expand_uri(name.trim(), dir)
}

/// KiCad's template folder (`${KICADn_TEMPLATE_DIR}`, the install's `template`), where its own `.kicad_wks` files are: the folder the dialog's browser
/// offers besides the board's.
pub(crate) fn template_dir() -> PathBuf {
    eda_kicad::kicad_shared_dir("template")
}

/// `OnWksFileSelection`'s short name for a picked file: relative to the board's folder when it is inside it, else with a path variable, else whole.
pub(crate) fn short_name(dir: &Path, file: &Path) -> String {
    short_name_with(&Env::discover(), dir, file)
}

fn short_name_with(env: &Env, dir: &Path, file: &Path) -> String {
    let project = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| lt::absolute(file, &project));
    if let Ok(rest) = file.strip_prefix(&project) {
        return rest.to_string_lossy().replace('\\', "/");
    }
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    lt::normalize_path(&file, &normalize_vars(env, &vars, dir), None)
}

/// Reads a drawing sheet file as `LoadDrawingSheet` does before parsing it: its text, or KiCad's message.
fn read_sheet(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err("File not found.".into());
    }
    let meta = std::fs::metadata(path).map_err(|_| "File could not be opened.".to_string())?;
    if meta.len() > MAX_SHEET_BYTES {
        return Err("Drawing sheet was not fully read.".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "File could not be opened.".to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The state JSON's `drawing_sheet` for a kept name: `{ path, file, text, error }` -- the name as kept, the file it resolves to, the file's text (null when it
/// could not be read) and why. `None` (KiCad's default sheet) is null.
pub(crate) fn state_json(dir: &Path, name: Option<&str>) -> Value {
    let Some(name) = name else { return Value::Null };
    let file = resolve(dir, name);
    match read_sheet(&file) {
        Ok(text) => json!({ "path": name, "file": file.display().to_string(), "text": text, "error": null }),
        Err(e) => json!({ "path": name, "file": file.display().to_string(), "text": null, "error": e }),
    }
}

/// `--drawing-sheet <file>` for a kicad-cli plot when the design chose a sheet that is there; nothing otherwise (kicad-cli then draws the project's, which
/// the derived `.kicad_pro` names, or its default).
pub(crate) fn plot_args(dir: &Path, name: Option<&str>) -> Vec<String> {
    let Some(name) = name else { return Vec::new() };
    let file = resolve(dir, name);
    if file.is_file() {
        vec!["--drawing-sheet".into(), file.display().to_string()]
    } else {
        Vec::new()
    }
}

/// The schematic's drawing sheet name (the root's `SchExtras::drawing_sheet`).
pub(crate) fn schematic_sheet(dir: &Path) -> Option<String> {
    crate::board::load(dir).ok().and_then(|(_, d, _)| d.schematic.and_then(|s| s.extras.drawing_sheet))
}

/// `GET /api/drawing_sheet?path=`: a file the dialog's browser picked (a whole path, or a name as kept): `{ ok, path, file, text }` -- `path` the name to keep
/// (`OnWksFileSelection`'s short name), `file` where it is, `text` what to read and sketch. A file that cannot be read is `{ ok: false, error }` with KiCad's
/// "Error loading drawing sheet '%s'." and the reason, as the dialog says it.
pub(crate) fn pick(dir: &Path, asked: &str) -> Value {
    let asked = asked.trim();
    if asked.is_empty() {
        return json!({ "ok": true, "path": "", "file": "", "text": null });
    }
    if !asked.to_ascii_lowercase().ends_with(".kicad_wks") {
        return bad(format!("'{asked}' is not a drawing sheet file (*.kicad_wks)."));
    }
    let file = resolve(dir, asked);
    match read_sheet(&file) {
        Ok(text) => json!({ "ok": true, "path": short_name(dir, &file), "file": file.display().to_string(), "text": text }),
        Err(e) => bad(format!("Error loading drawing sheet '{}'.\n{e}", file.display())),
    }
}

/// `GET /api/drawing_sheet/folders`: where the dialog's browser starts -- the board's folder and KiCad's template folder.
pub(crate) fn folders(dir: &Path) -> Value {
    let templates = template_dir();
    json!({ "project": dir.display().to_string(), "templates": templates.is_dir().then(|| templates.display().to_string()) })
}

/// `POST /api/drawing_sheet/upload` `{ name, text, overwrite? }`: a `.kicad_wks` from the person's own computer, written into the board's folder so the project
/// can name it (KiCad's file dialog reads it where it is; a page cannot hand the server a path). The name is the file's own (no folder); a file of that name
/// with other text is kept unless `overwrite`. Answers as [`pick`] does for the written file.
pub(crate) fn upload(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let name = req["name"].as_str().unwrap_or("").trim();
    let text = req["text"].as_str().unwrap_or("");
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') || name.contains(['\n', '\r', '"']) {
        return bad("Give the drawing sheet a file name of its own (no folder).");
    }
    if !name.to_ascii_lowercase().ends_with(".kicad_wks") {
        return bad(format!("'{name}' is not a drawing sheet file (*.kicad_wks)."));
    }
    if text.len() as u64 > MAX_SHEET_BYTES {
        return bad("Drawing sheet was not fully read.");
    }
    let path = dir.join(name);
    if path.exists() && !req["overwrite"].as_bool().unwrap_or(false) && std::fs::read_to_string(&path).map(|t| t != text).unwrap_or(true) {
        return bad(format!("A file named '{name}' is already in the board's folder."));
    }
    if let Err(e) = std::fs::write(&path, text) {
        return bad(format!("{}: {e}", path.display()));
    }
    pick(dir, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env() -> Env {
        let mut real = BTreeMap::new();
        real.insert("HOME".to_string(), "/nonexistent-home".to_string());
        real.insert(crate::library_tables::CONFIG_DIR_ENV.to_string(), "/nonexistent-config".to_string());
        Env::from_environment(real)
    }

    #[test]
    fn a_name_is_resolved_against_the_boards_folder() {
        let dir = tempfile_dir("resolve");
        let e = env();
        assert_eq!(resolve_with(&e, &dir, "frame.kicad_wks"), dir.join("frame.kicad_wks"));
        assert_eq!(resolve_with(&e, &dir, "${KIPRJMOD}/sheets/a.kicad_wks"), dir.join("sheets/a.kicad_wks"));
        assert_eq!(resolve_with(&e, &dir, "/abs/b.kicad_wks"), PathBuf::from("/abs/b.kicad_wks"));
    }

    #[test]
    fn a_picked_file_in_the_boards_folder_is_kept_relative() {
        let dir = tempfile_dir("short");
        let e = env();
        assert_eq!(short_name_with(&e, &dir, &dir.join("sheets").join("a.kicad_wks")), "sheets/a.kicad_wks");
        assert_eq!(short_name_with(&e, &dir, Path::new("/elsewhere/b.kicad_wks")), "/elsewhere/b.kicad_wks", "no variable holds it: whole");
    }

    #[test]
    fn the_state_says_what_kicad_says_of_a_missing_file() {
        let dir = tempfile_dir("state");
        assert_eq!(state_json(&dir, None), Value::Null);
        let missing = state_json(&dir, Some("gone.kicad_wks"));
        assert_eq!((missing["path"].as_str(), missing["text"].clone(), missing["error"].as_str()), (Some("gone.kicad_wks"), Value::Null, Some("File not found.")));
        std::fs::write(dir.join("here.kicad_wks"), "(kicad_wks (version 20231118))").unwrap();
        let here = state_json(&dir, Some("here.kicad_wks"));
        assert_eq!(here["text"], "(kicad_wks (version 20231118))");
        assert_eq!(plot_args(&dir, Some("here.kicad_wks")), vec!["--drawing-sheet".to_string(), dir.join("here.kicad_wks").display().to_string()]);
        assert!(plot_args(&dir, Some("gone.kicad_wks")).is_empty(), "a sheet that is not there is not passed");
    }

    #[test]
    fn an_upload_lands_in_the_boards_folder_once() {
        let dir = tempfile_dir("upload");
        let up = |name: &str, text: &str, overwrite: bool| upload(&dir, serde_json::to_vec(&json!({ "name": name, "text": text, "overwrite": overwrite })).unwrap().as_slice());
        let r = up("mine.kicad_wks", "(kicad_wks (version 20231118))", false);
        assert_eq!((r["ok"].as_bool(), r["path"].as_str()), (Some(true), Some("mine.kicad_wks")));
        assert_eq!(std::fs::read_to_string(dir.join("mine.kicad_wks")).unwrap(), "(kicad_wks (version 20231118))");
        assert_eq!(up("mine.kicad_wks", "(kicad_wks (version 20231118))", false)["ok"], true, "the same text again is fine");
        assert_eq!(up("mine.kicad_wks", "(other)", false)["ok"], false, "other text is not written over without asking");
        assert_eq!(up("mine.kicad_wks", "(other)", true)["ok"], true);
        assert_eq!(up("../x.kicad_wks", "x", false)["ok"], false);
        assert_eq!(up("x.txt", "x", false)["ok"], false);
    }

    fn tempfile_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda-drawing-sheet-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
