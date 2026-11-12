//! Assign Footprints (CvPcb) on the server: the footprint the preview pane draws, the project's footprint association files, and Automatically Assign Footprints.
//! Ported from KiCad 25ec2335's cvpcb/ (the cleaned 8303b2ad tree has none): `DISPLAY_FOOTPRINTS_FRAME::GetFootprint` and `InitDisplay` (display_footprints_frame.cpp),
//! `CVPCB_MAINFRAME::buildEquivalenceList` and `AutomaticFootprintMatching` (auto_associate.cpp; the pure half is `eda_model::cvpcb_equ`), and the list
//! `DIALOG_CONFIG_EQUFILES` (Manage Footprint Association Files..., dialogs/dialog_config_equfiles.cpp) edits.
//!
//! **Where the list is kept.** KiCad keeps it in the project file (`PROJECT_FILE::m_EquivalenceFiles`, `cvpcb.equivalence_files`) and the dialog's OK saves the
//! project at once (`SaveProject`), outside the undo list. The studio's derived project file is written again on every run, so the list is the board folder's
//! [`CVPCB_FILE`] (`eda_model::cvpcb_equ`) (`{"version": 1, "equivalence_files": [...]}`), as the folder's `path-variables.json` keeps the project's path variables; OK writes it, no undo
//! step, as in KiCad. The derived `.kicad_pro` is given the list too (`eda_model::cvpcb_equ::derive_into_project`: a path relative to the board's folder, or under `${KIPRJMOD}`,
//! written out whole, since the derived project lives in `<board>/.kicad`).
//!
//! **Where a file is found.** `wxExpandEnvVars( equfile )` -- here the project's variables, `KIPRJMOD` and KiCad's (`library_tables::Env::vars`) -- then the
//! path as it is when absolute or when it names a file, else `SEARCH_STACK::FindValidPath`. KiCad's working folder is the project's, so a relative path is read
//! against the board's folder first, then CvPcb's search stack as `GlobalPathsAppend` builds it (KiCad's share, footprint and template folders; UI-LEFT-2 LL7).

use crate::board;
use crate::library_index::{legal_library_name, Kind, LibSource};
use eda_model::cvpcb_equ::{self, equivalence_files, write_equivalence_files, Component, EquFile, CVPCB_FILE};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn bad(message: impl Into<String>) -> Value {
    json!({ "error": message.into() })
}

/// Where an entry of the list leads: `wxExpandEnvVars`, then the board's folder for a relative path (see the module doc).
fn resolve(dir: &Path, vars: &eda_kicad::library_table::Vars, entry: &str) -> PathBuf {
    let env = crate::library_tables::Env::discover();
    resolve_with(dir, vars, entry, &search_stack(&env))
}

/// `buildEquivalenceList`'s lookup (KiCad 25ec2335 cvpcb/auto_associate.cpp): `wxExpandEnvVars( equfile )`; an absolute path, or a relative one that names a
/// file from KiCad's working folder (the project's, here the board's), as it is; else `SEARCH_STACK::FindValidPath` over CvPcb's search stack (`Kiface().
/// KifaceSearch()`, [`search_stack`]): the first folder of it that has the file. A path found nowhere is the board folder's, which the caller finds missing
/// ("Equivalence file '%s' could not be found.").
fn resolve_with(dir: &Path, vars: &eda_kicad::library_table::Vars, entry: &str, stack: &[PathBuf]) -> PathBuf {
    let expanded = PathBuf::from(vars.expand(entry.trim()));
    if expanded.is_absolute() {
        return expanded;
    }
    let here = dir.join(&expanded);
    if here.is_file() {
        return here;
    }
    stack.iter().map(|base| base.join(&expanded)).find(|p| p.is_file()).unwrap_or(here)
}

/// CvPcb's search stack, `GlobalPathsAppend( &m_bm.m_search, FACE_CVPCB )` (common/systemdirsappend.cpp): for each base `SystemDirsAppend` finds -- `$KICAD`,
/// the install prefix (`DEFAULT_INSTALL_PATH`, `/usr/local` on Linux), the stock data path (here the folder above the installed libraries), the documents
/// folder, the program's folder and the one above it (kicad-cli's), and every folder of `PATH` -- the base, `kicad`, `kicad/share`, `share` and `share/kicad`
/// under it (a base named `bin` taken from its parent), and under each of those `modules`, `footprints`, `3dmodels` and `template`; then `/usr/local/share`.
/// Only folders that exist are kept, each once (`SEARCH_STACK::AddPaths`).
fn search_stack(env: &crate::library_tables::Env) -> Vec<PathBuf> {
    let mut maybe: Vec<PathBuf> = Vec::new();
    let split = |v: &str| std::env::split_paths(v).collect::<Vec<_>>();
    if let Some(k) = env.real.get("KICAD") {
        maybe.extend(split(k));
    }
    maybe.push(PathBuf::from("/usr/local"));
    if let Some(stock) = env.stock.symbols.parent() {
        maybe.push(stock.to_path_buf());
    }
    if let Some(home) = env.real.get("HOME") {
        maybe.push(PathBuf::from(home).join("Documents"));
    }
    if let Some(bin) = eda_kicad_engine::find_cli().as_deref().and_then(Path::parent) {
        maybe.push(bin.to_path_buf());
        if let Some(up) = bin.parent() {
            maybe.push(up.to_path_buf());
        }
    }
    if let Some(path) = env.real.get("PATH") {
        maybe.extend(split(path));
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    let add = |list: &mut Vec<PathBuf>, p: PathBuf| {
        if p.is_dir() && !list.contains(&p) {
            list.push(p);
        }
    };
    for m in maybe {
        let base = if m.file_name().is_some_and(|n| n == "bin") { m.parent().map(Path::to_path_buf).unwrap_or(m) } else { m };
        if base.parent().is_none() {
            continue; // "at least on linux": a base with no folder left
        }
        for sub in ["", "kicad", "kicad/share", "share", "share/kicad"] {
            add(&mut bases, if sub.is_empty() { base.clone() } else { base.join(sub) });
        }
    }
    let mut stack: Vec<PathBuf> = Vec::new();
    for b in &bases {
        for sub in ["modules", "footprints", "3dmodels", "template"] {
            add(&mut stack, b.join(sub));
        }
    }
    add(&mut stack, PathBuf::from("/usr/local/share"));
    stack
}

/// The variables the dialog's grid lists (`m_gridEnvVars`): `KIPRJMOD` and the footprint library folder's (`FOOTPRINT_LIBRARY_ADAPTER::GlobalPathEnvVariableName`,
/// `KICAD10_FOOTPRINT_DIR`), each with its value. The Add button writes a file under the first of them.
fn grid_vars(dir: &Path, env: &crate::library_tables::Env) -> Vec<Value> {
    let fp_var = format!("KICAD{}_FOOTPRINT_DIR", env.major);
    let fp_dir = env.global_vars().into_iter().find(|v| v.name == fp_var).map(|v| v.value).unwrap_or_default();
    vec![json!({ "name": "KIPRJMOD", "value": dir.display().to_string() }), json!({ "name": fp_var, "value": fp_dir })]
}

/// `GET /api/cvpcb/equ_files`: the list, where each entry leads and whether a file is there, the grid's two variables and the file the list is kept in.
pub(crate) fn equ_files(dir: &Path) -> Value {
    let env = crate::library_tables::Env::discover();
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let files = equivalence_files(dir);
    let resolved: Vec<Value> = files
        .iter()
        .map(|f| {
            let path = resolve(dir, &vars, f);
            json!({ "entry": f, "path": path.display().to_string(), "found": path.is_file() })
        })
        .collect();
    json!({ "files": files, "resolved": resolved, "vars": grid_vars(dir, &env), "project_file": dir.join(CVPCB_FILE).display().to_string() })
}

/// `POST /api/cvpcb/equ_files` `{files}`: `DIALOG_CONFIG_EQUFILES::OnOkClick` -- the list is replaced and saved (no undo step, as KiCad's `SaveProject`).
/// An entry is kept as written, with `\` made `/` ("Use unix separators only."); an empty entry is dropped.
pub(crate) fn set_equ_files(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return bad(format!("bad request: {e}")),
    };
    let Some(list) = req.get("files").and_then(Value::as_array) else { return bad("files is a list of paths") };
    let files: Vec<String> = list.iter().filter_map(Value::as_str).map(|s| s.trim().replace('\\', "/")).filter(|s| !s.is_empty()).collect();
    if let Err(e) = write_equivalence_files(dir, &files) {
        return bad(e);
    }
    let mut v = equ_files(dir);
    v["ok"] = json!(true);
    v
}

/// `GET /api/cvpcb/equ_browse?path=`: Add's file dialog (`wxFileDialog` with `FILEEXT::EquFileWildcard()`, "Footprint Association File", starting in the folder of the
/// grid's selected variable): the folders and the `.equ` files of a folder of the server (a page cannot see the machine's), the path with variables allowed, the board's
/// folder when empty. `{path, parent, entries: [{name, dir}]}`, folders first, by name.
pub(crate) fn equ_browse(dir: &Path, typed: &str) -> Value {
    let env = crate::library_tables::Env::discover();
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let mut at = if typed.trim().is_empty() { dir.to_path_buf() } else { resolve(dir, &vars, typed) };
    if at.is_file() {
        at = at.parent().map(Path::to_path_buf).unwrap_or(at);
    }
    let Ok(read) = std::fs::read_dir(&at) else { return bad(format!("'{}' is not a folder.", at.display())) };
    let mut entries: Vec<(String, bool)> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = e.path().is_dir();
            (!name.starts_with('.') && (is_dir || name.to_lowercase().ends_with(".equ"))).then_some((name, is_dir))
        })
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    entries.truncate(4000);
    json!({
        "path": at.display().to_string(),
        "parent": at.parent().map(|p| p.display().to_string()),
        "entries": entries.iter().map(|(n, d)| json!({ "name": n, "dir": d })).collect::<Vec<_>>(),
    })
}

/// The project's association files, each read or with why it was not (`buildEquivalenceList`'s two failures).
fn read_files(dir: &Path) -> Vec<EquFile> {
    let env = crate::library_tables::Env::discover();
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    equivalence_files(dir)
        .iter()
        .map(|entry| {
            let path = resolve(dir, &vars, entry);
            if !path.exists() {
                // "Equivalence file '%s' could not be found." names the file (`fn.GetFullName()`)
                let name = Path::new(&vars.expand(entry.trim())).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| entry.clone());
                return EquFile::NotFound { name };
            }
            match std::fs::read(&path) {
                // `From_UTF8( line )`: a byte that is not UTF-8 is read as Latin-1 by wxWidgets; here it is replaced
                Ok(bytes) => EquFile::Text(String::from_utf8_lossy(&bytes).into_owned()),
                Err(_) => EquFile::Unreadable { path: path.display().to_string() },
            }
        })
        .collect()
}

/// The file an entry of the list names, for Edit (`DIALOG_CONFIG_EQUFILES::OnEditEquFile`: `ExecuteFile( editorname, wxExpandEnvVars( entry ) )`): the
/// entry expanded and found as [`resolve`] finds it. Only an association file (`.equ`) is opened or written, and never one in KiCad's configuration folder.
fn equ_path(dir: &Path, entry: &str) -> Result<PathBuf, String> {
    let env = crate::library_tables::Env::discover();
    let vars = env.vars(dir, &crate::text_variables::project_variables(dir));
    let path = resolve(dir, &vars, entry);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("equ")) {
        return Err(format!("'{}' is not a footprint association file (.equ).", path.display()));
    }
    crate::library_tables::refuse_inside_kicads_configuration(&env, &path)?;
    Ok(path)
}

/// `GET /api/cvpcb/equ_text?entry=`: Edit's text editor (UI-LEFT-2 LL7). KiCad hands the file to the person's text editor (`Pgm().GetTextEditor()`); a page
/// cannot start one, so the studio's own editor opens it: `{path, text}` (an empty text for a file not made yet), or `{error}`.
pub(crate) fn equ_text(dir: &Path, entry: &str) -> Value {
    match equ_path(dir, entry) {
        Ok(path) => match std::fs::read(&path) {
            Ok(bytes) => json!({ "path": path.display().to_string(), "text": String::from_utf8_lossy(&bytes) }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({ "path": path.display().to_string(), "text": "", "new": true }),
            Err(e) => bad(format!("Error opening equivalence file '{}': {e}", path.display())),
        },
        Err(e) => bad(e),
    }
}

/// `POST /api/cvpcb/equ_text` `{entry, text}`: the editor's Save writes the file (no undo step: it is a file of the person's, as KiCad's text editor saves it).
pub(crate) fn set_equ_text(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return bad(format!("bad request: {e}")),
    };
    let entry = req.get("entry").and_then(Value::as_str).unwrap_or("");
    let text = req.get("text").and_then(Value::as_str).unwrap_or("");
    match equ_path(dir, entry) {
        Ok(path) => match std::fs::write(&path, text) {
            Ok(()) => json!({ "ok": true, "path": path.display().to_string() }),
            Err(e) => bad(format!("Could not save '{}': {e}", path.display())),
        },
        Err(e) => bad(e),
    }
}

/// `FOOTPRINT_LIST::GetFootprintInfo( fpid )`: a `Lib:Name` the footprint libraries of the tables have (`<library folder>/Name.kicad_mod`), or one the design's own
/// footprint library holds (the studio's project footprints, which the Assign Footprints dialog lists too).
fn footprint_exists<S: LibSource + ?Sized>(libs: &S, design: &eda_model::ir::Design, fpid: &str) -> bool {
    if design.footprint_library.as_ref().is_some_and(|l| l.by_name(fpid).is_some()) {
        return true;
    }
    let Some((lib, item)) = fpid.split_once(':') else { return false };
    if lib.is_empty() || item.is_empty() {
        return false;
    }
    libs.lib_path(Kind::Footprint, lib).is_ok_and(|folder| folder.join(format!("{item}.kicad_mod")).is_file())
}

/// `POST /api/cvpcb/auto_associate` `{components: [{ref, value, footprint, lib_id}]}`: Automatically Assign Footprints over the symbols the dialog lists, each with the
/// footprint it has there (an assignment not yet applied counts: CvPcb's netlist holds it). The footprint filters are the library symbol's (`GetFootprintFilters`).
/// The answer is what to assign -- the dialog stages it with its other assignments, applied as one undo step on OK -- and KiCad's messages: `status` ("%lu
/// footprint/symbol equivalences found."), `load_errors` ("Equivalence File Load Error") and `warnings` ("CvPcb Warning").
pub(crate) fn auto_associate(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return bad(format!("bad request: {e}")),
    };
    let Some(rows) = req.get("components").and_then(Value::as_array) else { return bad("components is a list") };
    let (_, design, model) = match board::load(dir) {
        Ok(l) => l,
        Err(e) => return bad(board::reasons(&e)),
    };
    let symbols = crate::library_tables::libs(dir, Kind::Symbol);
    let footprints = crate::library_tables::libs(dir, Kind::Footprint);
    let mut filters_of: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let components: Vec<Component> = rows
        .iter()
        .map(|r| {
            let lib_id = text(r, "lib_id");
            let filters = filters_of
                .entry(lib_id.clone())
                .or_insert_with(|| if lib_id.is_empty() { Vec::new() } else { crate::library_api::symbol_in_libs(&*symbols, &design, &model, &lib_id).map(|(s, _)| s.footprint_filters).unwrap_or_default() })
                .clone();
            Component { reference: text(r, "ref"), value: text(r, "value"), footprint: text(r, "footprint"), filters }
        })
        .collect();
    let m = cvpcb_equ::automatic_matching(&components, &read_files(dir), |fpid| footprint_exists(&*footprints, &design, fpid));
    json!({
        "assignments": m.assignments.iter().map(|(r, f)| json!({ "ref": r, "footprint": f })).collect::<Vec<_>>(),
        "found": m.found,
        "status": format!("{} footprint/symbol equivalences found.", m.found),
        "load_errors": m.load_errors,
        "warnings": m.warnings,
    })
}

/// `GET /api/cvpcb/footprint?id=`: what the footprint viewer shows for `id` (`DISPLAY_FOOTPRINTS_FRAME::InitDisplay`: the footprint picked in the list, else the
/// selected symbol's; the caller says which). `{footprint, lib, in_project}`, or `{error}` in `GetFootprint`'s words: "Footprint ID '%s' is not valid.", "Library '%s'
/// is not in the footprint library table.", "Footprint '%s' not found.", "Error loading footprint: %s". Studio addition: the design's own copy of a footprint (its
/// footprint library) is found first, and a bare name (no library) as the board's model resolves it -- the studio's built-in packages, which no table lists.
pub(crate) fn footprint(dir: &Path, id: &str) -> Value {
    let id = id.trim();
    if id.is_empty() {
        return json!({ "footprint": null });
    }
    // the design's own copy (its footprint library); a bare name (no library) also as the board's model resolves it -- the studio's built-in packages
    let (_, design, _) = match board::load(dir) {
        Ok(l) => l,
        Err(e) => return bad(board::reasons(&e)),
    };
    if design.footprint_library.as_ref().is_some_and(|l| l.by_name(id).is_some()) || !id.contains(':') {
        if let Ok((fp, in_project)) = crate::library_api::footprint_any(dir, id) {
            let lib = id.split_once(':').map(|(l, _)| l).unwrap_or("");
            return json!({ "footprint": fp, "lib": lib, "in_project": in_project });
        }
    }
    // `LIB_ID::Parse`
    let (lib, item) = id.split_once(':').unwrap_or(("", id));
    if item.is_empty() || (!lib.is_empty() && !legal_library_name(lib)) || item.contains(['/', '\\']) {
        return bad(format!("Footprint ID '{id}' is not valid."));
    }
    let libs = crate::library_tables::libs(dir, Kind::Footprint);
    let Ok(folder) = libs.lib_path(Kind::Footprint, lib) else { return bad(format!("Library '{lib}' is not in the footprint library table.")) };
    if !folder.join(format!("{item}.kicad_mod")).is_file() {
        return bad(format!("Footprint '{id}' not found."));
    }
    match crate::library_search::read_installed_footprint(&*libs, id) {
        Ok(Some(fp)) => json!({ "footprint": fp, "lib": lib, "in_project": false }),
        Ok(None) => bad(format!("Footprint '{id}' not found.")),
        Err(why) => bad(format!("Error loading footprint: {why}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda-cvpcb-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// `buildEquivalenceList`: a relative path names the board folder's file when there is one, else the first folder of CvPcb's search stack that has it.
    #[test]
    fn a_relative_association_file_is_the_board_folders_else_the_first_search_folders() {
        let dir = temp("stack");
        let (board, a, b) = (dir.join("board"), dir.join("share/kicad/template"), dir.join("other/template"));
        for d in [&board, &a, &b] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(b.join("x.equ"), "").unwrap();
        std::fs::write(a.join("x.equ"), "").unwrap();
        let vars = eda_kicad::library_table::Vars::default();
        let stack = vec![a.clone(), b.clone()];
        assert_eq!(resolve_with(&board, &vars, "x.equ", &stack), a.join("x.equ"), "the first folder of the stack that has it");
        std::fs::write(board.join("x.equ"), "").unwrap();
        assert_eq!(resolve_with(&board, &vars, "x.equ", &stack), board.join("x.equ"), "the working folder's first");
        assert_eq!(resolve_with(&board, &vars, "none.equ", &stack), board.join("none.equ"), "found nowhere: the board folder's, which is missing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_is_kept_beside_the_board_and_an_empty_one_removes_the_file() {
        let dir = temp("list");
        assert!(equivalence_files(&dir).is_empty());
        let v = set_equ_files(&dir, br#"{"files": ["${KIPRJMOD}/mine.equ", " ", "C:\\kicad\\std.equ"]}"#);
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(equivalence_files(&dir), vec!["${KIPRJMOD}/mine.equ".to_string(), "C:/kicad/std.equ".to_string()], "an empty entry dropped, unix separators");
        assert_eq!(v["resolved"][0]["path"], dir.join("mine.equ").display().to_string());
        assert_eq!(v["resolved"][0]["found"], false);
        assert_eq!(v["vars"][0]["name"], "KIPRJMOD");
        assert!(v["vars"][1]["name"].as_str().unwrap().ends_with("_FOOTPRINT_DIR"));
        set_equ_files(&dir, br#"{"files": []}"#);
        assert!(!dir.join(CVPCB_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_files_are_read_from_the_board_folder_and_a_missing_one_is_named() {
        let dir = temp("read");
        std::fs::write(dir.join("mine.equ"), "'LED' 'LED_SMD:LED_0603_1608Metric'\n").unwrap();
        write_equivalence_files(&dir, &["${KIPRJMOD}/mine.equ".into(), "sub/gone.equ".into(), "mine.equ".into()]).unwrap();
        let read = read_files(&dir);
        assert_eq!(read[0], EquFile::Text("'LED' 'LED_SMD:LED_0603_1608Metric'\n".into()));
        assert_eq!(read[1], EquFile::NotFound { name: "gone.equ".into() });
        assert!(matches!(read[2], EquFile::Text(_)), "relative to the board's folder");
        // Add's file dialog: the folders and the .equ files
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let b = equ_browse(&dir, "");
        let names: Vec<(&str, bool)> = b["entries"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap(), e["dir"].as_bool().unwrap())).collect();
        assert!(names.contains(&("sub", true)) && names.contains(&("mine.equ", false)) && !names.iter().any(|(n, _)| *n == "notes.txt"), "{names:?}");
        assert_eq!(equ_browse(&dir, "${KIPRJMOD}/sub")["path"], dir.join("sub").display().to_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A board with a footprint library of its own (`MyFp`, in its project table) holding `R_0603` and `LED_0603`, and an association file in its folder.
    fn board_with_library(name: &str) -> PathBuf {
        let dir = crate::board::tests::scratch(name);
        crate::board::tests::setup(&dir);
        let lib = dir.join("MyFp.pretty");
        std::fs::create_dir_all(&lib).unwrap();
        for fp in ["R_0603", "LED_0603"] {
            std::fs::write(lib.join(format!("{fp}.kicad_mod")), format!("(footprint \"{fp}\" (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\")) (pad \"2\" smd rect (at 2 0) (size 1 1) (layers \"F.Cu\")))")).unwrap();
        }
        std::fs::write(dir.join("fp-lib-table"), "(fp_lib_table\n\t(version 7)\n\t(lib (name \"MyFp\") (type \"KiCad\") (uri \"${KIPRJMOD}/MyFp.pretty\") (options \"\") (descr \"\"))\n)\n").unwrap();
        std::fs::write(dir.join("mine.equ"), "# mine\n'10k' 'MyFp:R_0603'\n'RED' 'MyFp:LED_0603'\n'RED' 'MyFp:LED_0805'\n'GONE' 'MyFp:Nothing'\n").unwrap();
        dir
    }

    #[test]
    fn auto_associate_reads_the_projects_files_and_assigns_by_value_what_the_libraries_have() {
        let dir = board_with_library("cvpcb_auto");
        let ask = |components: Value| auto_associate(&dir, json!({ "components": components }).to_string().as_bytes());
        // no list yet: nothing found, nothing assigned
        let v = ask(json!([{ "ref": "R1", "value": "10k", "footprint": "" }]));
        assert_eq!((v["found"].clone(), v["assignments"].clone(), v["status"].clone()), (json!(0), json!([]), json!("0 footprint/symbol equivalences found.")), "{v}");
        set_equ_files(&dir, br#"{"files": ["${KIPRJMOD}/mine.equ", "${KIPRJMOD}/missing.equ"]}"#);
        let v = ask(json!([
            { "ref": "R1", "value": "10K", "footprint": "" },
            { "ref": "R2", "value": "10k", "footprint": "MyFp:R_0603" },
            { "ref": "D1", "value": "red", "footprint": "" },
            { "ref": "X1", "value": "GONE", "footprint": "" },
        ]));
        assert_eq!(v["found"], 4, "{v}");
        assert_eq!(v["assignments"], json!([{ "ref": "R1", "footprint": "MyFp:R_0603" }, { "ref": "D1", "footprint": "MyFp:LED_0603" }]), "a symbol with a footprint is left alone; of RED's two the one the library has");
        assert_eq!(v["load_errors"], "Equivalence file 'missing.equ' could not be found.");
        // D1 took the first of RED's two (no filter: `found = filtercount == 0`), so the second, which no library has, was never looked at
        assert_eq!(v["warnings"], "Component X1: footprint MyFp:Nothing not found in any of the project footprint libraries.");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_viewer_draws_a_footprint_of_the_tables_or_says_why_in_kicads_words() {
        let dir = board_with_library("cvpcb_viewer");
        let v = footprint(&dir, "MyFp:R_0603");
        assert_eq!((v["lib"].clone(), v["in_project"].clone()), (json!("MyFp"), json!(false)), "{v}");
        assert_eq!(v["footprint"]["pads"].as_array().map(Vec::len), Some(2));
        assert_eq!(footprint(&dir, "")["footprint"], Value::Null, "nothing picked, nothing assigned: an empty viewer");
        assert_eq!(footprint(&dir, "MyFp:")["error"], "Footprint ID 'MyFp:' is not valid.");
        assert_eq!(footprint(&dir, "NoSuchLib:R_0603")["error"], "Library 'NoSuchLib' is not in the footprint library table.");
        assert_eq!(footprint(&dir, "MyFp:R_9999")["error"], "Footprint 'MyFp:R_9999' not found.");
        std::fs::write(dir.join("MyFp.pretty").join("Broken.kicad_mod"), "(footprint \"Broken\" (pad").unwrap();
        assert!(footprint(&dir, "MyFp:Broken")["error"].as_str().unwrap().starts_with("Error loading footprint: "));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
