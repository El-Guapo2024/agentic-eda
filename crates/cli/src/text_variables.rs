//! The project's text variables of a board folder (`PROJECT::GetTextVars`, the project file's `text_variables`).
//!
//! Two places can hold them. The Text Variables page (Board Setup, `panel_text_variables.cpp`) stores its rows in `design.json`
//! (`drawings.rules.text_variables`, the `set_text_variables` verb), where an Undo takes an edit back like any other. The board folder's
//! `path-variables.json` is where an imported project's variables came in and where Configure Paths kept the board's own before the page existed; it
//! is an input, like the intent. Once the page has been used it is the project's whole set and the file is only the seed it started from
//! ([`eda_model::rules::effective_text_variables`]); until then the file is.
//!
//! [`project_variables`] is the one reader of the effective set (the library tables, Configure Paths, the 3D model names and the studio's state go
//! through it), [`set_project_variables`] the one writer Configure Paths uses, so the two dialogs are two views of one list.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use eda_kicad::library_table as lt;

/// Just enough of `design.json` to find the page: serde skips the rest of the file without building it.
#[derive(serde::Deserialize, Default)]
struct Peek {
    #[serde(default)]
    drawings: Option<PeekDrawings>,
}
#[derive(serde::Deserialize, Default)]
struct PeekDrawings {
    #[serde(default)]
    rules: Option<PeekRules>,
}
#[derive(serde::Deserialize, Default)]
struct PeekRules {
    #[serde(default)]
    text_variables: Option<BTreeMap<String, String>>,
}

type Page = Option<BTreeMap<String, String>>;

/// The page of `design.json`, read again only when the file changed (the library tables ask for the variables on many requests, and `design.json` can be
/// megabytes). Keyed by the file's path, validated by its modification time and size.
fn page(dir: &Path) -> Page {
    static CACHE: Mutex<Option<HashMap<PathBuf, (SystemTime, u64, Page)>>> = Mutex::new(None);
    let path = dir.join("design.json");
    let meta = std::fs::metadata(&path).ok()?;
    let stamp = (meta.modified().ok()?, meta.len());
    if let Some((time, len, page)) = CACHE.lock().ok()?.as_ref().and_then(|c| c.get(&path)).cloned() {
        if (time, len) == stamp {
            return page;
        }
    }
    let peek: Peek = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).unwrap_or_default();
    let page = peek.drawings.and_then(|d| d.rules).and_then(|r| r.text_variables);
    if let Ok(mut cache) = CACHE.lock() {
        cache.get_or_insert_with(HashMap::new).insert(path, (stamp.0, stamp.1, page.clone()));
    }
    page
}

/// The text variables of the project in `dir`: the Text Variables page when it has been used, else the folder's `path-variables.json`.
pub(crate) fn project_variables(dir: &Path) -> BTreeMap<String, String> {
    page(dir).unwrap_or_else(|| lt::read_project_variables(dir))
}

/// Store the project's whole set of variables. A board (a folder with its `design.json` and `board.json`) takes them through the `set_text_variables` verb, so
/// the edit is one undo step like every other Board Setup page; a bare folder has nothing to undo and keeps them in `path-variables.json`.
pub(crate) fn set_project_variables(dir: &Path, vars: &BTreeMap<String, String>) -> Result<(), String> {
    if dir.join("design.json").is_file() && dir.join("board.json").is_file() {
        let cmd = eda_ops::Cmd::SetTextVariables { variables: vars.clone() };
        return crate::board::step(dir, cmd, false, "ui").map(|_| ()).map_err(|e| crate::board::reasons(&e));
    }
    lt::write_project_variables(dir, vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-text-variables-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn vars(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
        rows.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_page_replaces_the_files_variables_and_only_when_it_is_in_design_json() {
        let dir = temp("effective");
        lt::write_project_variables(&dir, &vars(&[("SEED", "1")])).unwrap();
        assert_eq!(project_variables(&dir), vars(&[("SEED", "1")]), "no design.json: the file");
        std::fs::write(dir.join("design.json"), r#"{"schema":1,"drawings":{"shapes":[]}}"#).unwrap();
        assert_eq!(project_variables(&dir), vars(&[("SEED", "1")]), "a design without the page: the file");
        assert!(page(&dir).is_none());
        std::fs::write(dir.join("design.json"), r#"{"schema":1,"other":{"text_variables":{"X":"1"}},"drawings":{"rules":{"text_variables":{"REV":"B"}}}}"#).unwrap();
        assert_eq!(project_variables(&dir), vars(&[("REV", "B")]), "the page replaces the seed, and only the page's own place counts");
        assert!(page(&dir).is_some());
        std::fs::write(dir.join("design.json"), r#"{"schema":1,"drawings":{"rules":{"text_variables":{}}}}"#).unwrap();
        assert!(project_variables(&dir).is_empty(), "a page with every variable deleted shadows the file");
        std::fs::write(dir.join("design.json"), "not json").unwrap();
        assert_eq!(project_variables(&dir), vars(&[("SEED", "1")]), "an unreadable design.json is no page");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bare_folder_keeps_its_variables_in_the_file() {
        let dir = temp("bare");
        set_project_variables(&dir, &vars(&[("TEAM", "/srv")])).unwrap();
        assert_eq!(lt::read_project_variables(&dir), vars(&[("TEAM", "/srv")]));
        set_project_variables(&dir, &BTreeMap::new()).unwrap();
        assert!(!dir.join(lt::PATH_VARIABLES_FILE).exists(), "none left removes the file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
