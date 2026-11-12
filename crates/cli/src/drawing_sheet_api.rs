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
use eda_model::embedded_files::EmbeddedFiles;
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

/// The file a `kicad-embed://<name>` drawing sheet names, out of the document's embedded files (`list`): `FILENAME_RESOLVER::ResolvePath`'s `kicad-embed` branch,
/// `EMBEDDED_FILES::GetTemporaryFileName` (KiCad writes the file into its cache and loads that); "File not found." when the list has none of that name (KiCad's
/// "Failed to get temp file '%s' for kicad_embed URI" goes to a trace).
fn embedded_sheet(list: Option<&EmbeddedFiles>, name: &str) -> Option<Result<(String, Vec<u8>), String>> {
    let wanted = name.strip_prefix(eda_ops::drawing_sheet::EMBED_PREFIX)?;
    let Some(f) = list.and_then(|l| l.get(wanted)) else { return Some(Err("File not found.".into())) };
    Some(eda_kicad::embedded::decode(f).map(|d| (f.name.clone(), d.bytes)).map_err(|_| "File could not be opened.".to_string()))
}

/// The state JSON's `drawing_sheet` for a kept name: `{ path, file, text, error }` -- the name as kept, the file it resolves to, the file's text (null when it
/// could not be read) and why. `None` (KiCad's default sheet) is null. A `kicad-embed://` name is read from the document's embedded files (`list`).
pub(crate) fn state_json(dir: &Path, list: Option<&EmbeddedFiles>, name: Option<&str>) -> Value {
    let Some(name) = name else { return Value::Null };
    if let Some(found) = embedded_sheet(list, name) {
        return match found {
            Ok((_, bytes)) => json!({ "path": name, "file": name, "text": String::from_utf8_lossy(&bytes), "error": null, "embedded": true }),
            Err(e) => json!({ "path": name, "file": name, "text": null, "error": e, "embedded": true }),
        };
    }
    let file = resolve(dir, name);
    match read_sheet(&file) {
        Ok(text) => json!({ "path": name, "file": file.display().to_string(), "text": text, "error": null }),
        Err(e) => json!({ "path": name, "file": file.display().to_string(), "text": null, "error": e }),
    }
}

/// `--drawing-sheet <file>` for a kicad-cli plot when the design chose a sheet that is there; nothing otherwise (kicad-cli then draws the project's, which
/// the derived `.kicad_pro` names, or its default). An embedded sheet is written into the board's `.kicad/embedded/` first, as KiCad writes it into its
/// cache (`GetTemporaryFileName`), and that file is named.
pub(crate) fn plot_args(dir: &Path, list: Option<&EmbeddedFiles>, name: Option<&str>) -> Vec<String> {
    let Some(name) = name else { return Vec::new() };
    if let Some(found) = embedded_sheet(list, name) {
        let Ok((file, bytes)) = found else { return Vec::new() };
        let out = dir.join(".kicad").join("embedded").join(&file);
        if out.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::write(&out, bytes).is_ok() {
            return vec!["--drawing-sheet".into(), out.display().to_string()];
        }
        return Vec::new();
    }
    let file = resolve(dir, name);
    if file.is_file() {
        vec!["--drawing-sheet".into(), file.display().to_string()]
    } else {
        Vec::new()
    }
}

/// The embedded files of the board (`schematic` false) or the schematic (the root's), where an embedded drawing sheet of that document is.
pub(crate) fn document_files(dir: &Path, schematic: bool) -> Option<EmbeddedFiles> {
    let design = crate::embedded_api::read_design(dir).ok()?;
    if schematic {
        design.schematic.map(|s| s.extras.embedded)
    } else {
        design.drawings.map(|d| d.embedded)
    }
}

/// `POST /api/text/eval?clock=..` `{ texts: [..] }`: each text's `@{..}` evaluated (`EXPRESSION_EVALUATOR::Evaluate`, [`eda_model::text_vars::evaluate_math`]) with the
/// board's project set for the thread -- its repository for the `vcs..` functions, the clock the browser names for `today()`, `now()` and `timeformat()` -- as a
/// drawing sheet's texts are after their variables (KiCad 25ec2335 `DS_DRAW_ITEM_LIST::BuildFullText`). `{ texts: [..] }` in the same order.
pub(crate) fn eval_texts(dir: &Path, target: &str, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let intent = crate::board::load(dir).map(|(meta, _, _)| meta.intent).unwrap_or_default();
    let _project = crate::text_vars_json::project(dir, &intent, crate::text_vars_json::clock_of(target));
    let texts: Vec<String> = req["texts"].as_array().map(|a| a.iter().map(|t| eda_model::text_vars::evaluate_math(t.as_str().unwrap_or(""))).collect()).unwrap_or_default();
    json!({ "ok": true, "texts": texts })
}

/// The schematic's drawing sheet name (the root's `SchExtras::drawing_sheet`).
pub(crate) fn schematic_sheet(dir: &Path) -> Option<String> {
    crate::board::load(dir).ok().and_then(|(_, d, _)| d.schematic.and_then(|s| s.extras.drawing_sheet))
}

/// `GET /api/drawing_sheet?path=`: a file the dialog's browser picked (a whole path, or a name as kept): `{ ok, path, file, text }` -- `path` the name to keep
/// (`OnWksFileSelection`'s short name), `file` where it is, `text` what to read and sketch. A file that cannot be read is `{ ok: false, error }` with KiCad's
/// "Error loading drawing sheet '%s'." and the reason, as the dialog says it.
pub(crate) fn pick(dir: &Path, list: Option<&EmbeddedFiles>, asked: &str) -> Value {
    let asked = asked.trim();
    if asked.is_empty() {
        return json!({ "ok": true, "path": "", "file": "", "text": null });
    }
    if let Some(found) = embedded_sheet(list, asked) {
        return match found {
            Ok((_, bytes)) => json!({ "ok": true, "path": asked, "file": asked, "text": String::from_utf8_lossy(&bytes), "embedded": true }),
            Err(e) => bad(format!("Error loading drawing sheet '{asked}'.\n{e}")),
        };
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
    pick(dir, None, name)
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
        assert_eq!(state_json(&dir, None, None), Value::Null);
        let missing = state_json(&dir, None, Some("gone.kicad_wks"));
        assert_eq!((missing["path"].as_str(), missing["text"].clone(), missing["error"].as_str()), (Some("gone.kicad_wks"), Value::Null, Some("File not found.")));
        std::fs::write(dir.join("here.kicad_wks"), "(kicad_wks (version 20231118))").unwrap();
        let here = state_json(&dir, None, Some("here.kicad_wks"));
        assert_eq!(here["text"], "(kicad_wks (version 20231118))");
        assert_eq!(plot_args(&dir, None, Some("here.kicad_wks")), vec!["--drawing-sheet".to_string(), dir.join("here.kicad_wks").display().to_string()]);
        assert!(plot_args(&dir, None, Some("gone.kicad_wks")).is_empty(), "a sheet that is not there is not passed");
    }

    /// CC8 / CC7: a `kicad-embed://` sheet is read from the document's embedded files (`ResolvePath`'s `kicad-embed` branch) and written out for kicad-cli
    /// (`GetTemporaryFileName`); one the list does not have is "File not found.".
    #[test]
    fn an_embedded_sheet_is_read_from_the_documents_files() {
        let dir = tempfile_dir("embedded");
        let text = "(kicad_wks (version 20231118) (tbtext \"EMBEDDED\" (pos 10 10)))";
        let mut list = EmbeddedFiles::default();
        list.put(eda_kicad::embedded::encode_new("frame.kicad_wks", text.as_bytes()).unwrap());
        assert_eq!(list.files[0].kind, eda_model::embedded_files::EmbeddedFileType::Worksheet, "typed worksheet by its extension, as `AddFile` types it");
        let s = state_json(&dir, Some(&list), Some("kicad-embed://frame.kicad_wks"));
        assert_eq!((s["text"].as_str(), s["embedded"].as_bool()), (Some(text), Some(true)));
        let gone = state_json(&dir, Some(&list), Some("kicad-embed://other.kicad_wks"));
        assert_eq!(gone["error"], "File not found.");
        let args = plot_args(&dir, Some(&list), Some("kicad-embed://frame.kicad_wks"));
        assert_eq!(args[0], "--drawing-sheet");
        assert_eq!(std::fs::read_to_string(&args[1]).unwrap(), text);
        assert_eq!(pick(&dir, Some(&list), "kicad-embed://frame.kicad_wks")["text"], text);
        assert_eq!(pick(&dir, None, "kicad-embed://frame.kicad_wks")["ok"], false);
    }

    /// CC8: a drawing sheet's `@{..}` evaluated by the server (`BuildFullText`'s `EXPRESSION_EVALUATOR`), the clock the browser names.
    #[test]
    fn a_sheets_expressions_are_evaluated_with_the_requests_clock() {
        let dir = tempfile_dir("eval");
        let r = eval_texts(&dir, "/api/text/eval?clock=2026-11-06T23%3A20%3A00", br#"{ "texts": ["@{1+2} mm", "plain", "@{dateformat(today())}", "@{nope(}"] }"#);
        assert_eq!((r["texts"][0].as_str(), r["texts"][1].as_str(), r["texts"][3].as_str()), (Some("3 mm"), Some("plain"), Some("@{nope(}")));
        // `today()` is the UTC day of the server's moment (`GetTicks() / 86400`), whatever the browser's clock says
        let today = r["texts"][2].as_str().unwrap();
        assert!(today.len() == 10 && today.starts_with("20"), "{today}");
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
