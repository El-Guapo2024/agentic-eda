//! A workspace: one folder holding several projects, one of them open in the studio at a time (`eda board serve --workspace <dir>`), as KiCad's project
//! manager opens one project at a time (KiCad 25ec2335 `kicad/kicad_manager_frame.cpp::LoadProject`, which closes the open one first: `CloseProject`).
//!
//! A project is a subfolder of the workspace holding a studio board: `board.json` (the metadata `eda board new` writes) and `design.json` (the design,
//! the only source of truth: docs/ARCHITECTURE.md), the two files `board::load` reads. Folders without them (an export folder, a hidden folder) are not
//! listed; no name is filtered out (`safety-power.old` is a project like any other).
//!
//! A browser cannot browse the server's disk, so the dialogs list and create inside the workspace only: a project is named, never given as a path, and a
//! name with a path separator or `..` is refused ([`check_name`]).
//!
//! The projects opened, most recent first, are kept in `<workspace>/.eda-workspace.json` (KiCad keeps its file history in its own settings, which the
//! studio never writes): the File > Recent Projects list and the project `--workspace` alone opens.

use eda_model::CheckResult;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The file the opened projects are remembered in, beside them.
const RECENT_FILE: &str = ".eda-workspace.json";

/// KiCad's file history keeps this many (`FILE_HISTORY`'s default `m_FileHistorySize`, `COMMON_SETTINGS::m_System.file_history_size`).
const RECENT_MAX: usize = 9;

fn fail(check: &str, loc: &str, hint: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, loc, hint.into())]
}

/// Whether `dir` holds a studio project: the files `board::load` reads.
pub(crate) fn is_project(dir: &Path) -> bool {
    dir.join("board.json").is_file() && dir.join("design.json").is_file()
}

/// A project's name as a caption shows it: its folder's name (KiCad's caption shows the project file's name, which is its folder's for a project KiCad
/// made: `KICAD_MANAGER_FRAME::CreateNewProject` names both alike).
pub(crate) fn project_name(dir: &Path) -> String {
    let abs = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    abs.file_name().and_then(|n| n.to_str()).unwrap_or("project").to_string()
}

/// A name the workspace's dialogs take: one folder name inside the workspace. KiCad takes a path from a file dialog; here the name is all there is, so
/// anything that would leave the workspace folder or name a hidden one is refused.
pub(crate) fn check_name(name: &str) -> Result<(), String> {
    let name_shown = name.trim();
    if name_shown.is_empty() {
        return Err("Please enter a project name.".into());
    }
    if name != name_shown {
        return Err(format!("Project name '{name}' must not start or end with spaces."));
    }
    if name.contains(['/', '\\']) || name.contains("..") {
        return Err(format!("Project name '{name}' must not contain path separators or '..'."));
    }
    if name.starts_with('.') || name.chars().any(|c| c.is_control()) {
        return Err(format!("Project name '{name}' is not a valid folder name."));
    }
    Ok(())
}

/// One project of the workspace, as the Open Project list shows it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) dir: PathBuf,
    /// The latest modification of its design (design.json, board.json, activity.jsonl), nanoseconds since 1970.
    pub(crate) modified: u128,
}

fn nanos(p: &Path) -> u128 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())
}

/// A workspace folder.
#[derive(Debug, Clone)]
pub(crate) struct Workspace {
    pub(crate) root: PathBuf,
}

impl Workspace {
    /// The workspace at `root`, which must be a folder (made if it is missing, as KiCad makes a new project's folder).
    pub(crate) fn open(root: &Path) -> Result<Workspace, Vec<CheckResult>> {
        if !root.exists() {
            std::fs::create_dir_all(root).map_err(|e| fail("workspace_mkdir", &root.display().to_string(), e.to_string()))?;
        }
        if !root.is_dir() {
            return Err(fail("workspace_not_dir", &root.display().to_string(), "the workspace must be a folder"));
        }
        let root = std::fs::canonicalize(root).map_err(|e| fail("workspace_path", &root.display().to_string(), e.to_string()))?;
        Ok(Workspace { root })
    }

    /// The workspace's projects, newest first (by the last change to their design), then by name.
    pub(crate) fn list(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                let dir = e.path();
                (!name.starts_with('.') && dir.is_dir() && is_project(&dir)).then(|| {
                    let modified = ["design.json", "board.json", "activity.jsonl"].iter().map(|f| nanos(&dir.join(f))).max().unwrap_or(0);
                    Entry { name, dir, modified }
                })
            })
            .collect();
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
        out
    }

    /// The folder of project `name`, which must be one of the workspace's projects.
    pub(crate) fn project(&self, name: &str) -> Result<PathBuf, String> {
        check_name(name)?;
        let dir = self.root.join(name);
        if !is_project(&dir) {
            return Err(format!("'{name}' is not a project of the workspace {}.", self.root.display()));
        }
        Ok(dir)
    }

    /// `-C <dir>` given with `--workspace`: a folder of the workspace by its path (absolute, or relative to where `serve` was started) or by its name.
    pub(crate) fn resolve(&self, given: &Path) -> Result<PathBuf, String> {
        let by_path = std::fs::canonicalize(given).ok().filter(|p| p.parent() == Some(self.root.as_path()));
        match by_path {
            Some(p) => self.project(p.file_name().and_then(|n| n.to_str()).unwrap_or_default()),
            None if given.components().count() == 1 => self.project(&given.display().to_string()),
            None => Err(format!("{} is not a project folder of the workspace {}.", given.display(), self.root.display())),
        }
    }

    fn recent_path(&self) -> PathBuf {
        self.root.join(RECENT_FILE)
    }

    /// The projects opened, most recent first, that are still projects of the workspace (KiCad's file history drops a file that is gone:
    /// `FILE_HISTORY::AddFilesToMenu` lists only existing files).
    pub(crate) fn recent(&self) -> Vec<String> {
        let names: Vec<String> = std::fs::read_to_string(self.recent_path())
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["recent"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        names.into_iter().filter(|n| self.project(n).is_ok()).collect()
    }

    /// `name` was opened: first in the history (`FILE_HISTORY::AddFileToHistory`), at most [`RECENT_MAX`] kept.
    pub(crate) fn note_opened(&self, name: &str) {
        let mut names = self.recent();
        names.retain(|n| n != name);
        names.insert(0, name.to_string());
        names.truncate(RECENT_MAX);
        let tmp = self.root.join(format!("{RECENT_FILE}.tmp"));
        if std::fs::write(&tmp, serde_json::to_string_pretty(&json!({ "recent": names })).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, self.recent_path());
        }
    }

    /// The project `--workspace` alone opens: the most recently opened one, else none (the studio starts with no project, as the project manager
    /// does with no project in its history).
    pub(crate) fn most_recent(&self) -> Option<PathBuf> {
        self.recent().first().and_then(|n| self.project(n).ok())
    }

    /// File > New Project...: `<workspace>/<name>/`, an empty board and schematic made by `eda board new` from an empty intent
    /// (`<name>/<name>.yaml`, so the board is named after its project as KiCad names a new project's board and schematic after it:
    /// `KICAD_MANAGER_FRAME::CreateNewProject`). KiCad 25ec2335 `KICAD_MANAGER_CONTROL::newProjectDirectory`'s checks, with the browser's limits: a folder
    /// that cannot be made gives KiCad's message; an existing folder that is not empty is refused (KiCad asks "The selected folder is not empty..." and
    /// may go on; here the name is the only way to say where, so another name is asked for instead of mixing a new project into a folder).
    pub(crate) fn create(&self, name: &str) -> Result<PathBuf, String> {
        check_name(name)?;
        let dir = self.root.join(name);
        if is_project(&dir) {
            return Err(format!("A project named '{name}' already exists in the workspace."));
        }
        if dir.exists() {
            let empty = std::fs::read_dir(&dir).map(|mut d| d.next().is_none()).unwrap_or(false);
            if !dir.is_dir() || !empty {
                return Err(format!("The folder '{name}' already exists in the workspace and is not empty. Choose another name."));
            }
        } else if std::fs::create_dir(&dir).is_err() {
            return Err(format!("Folder '{}' could not be created.\n\nMake sure you have write permissions and try again.", dir.display()));
        }
        let intent = dir.join(format!("{name}.yaml"));
        std::fs::write(&intent, "# The intent of a project made by File > New Project...: no parts yet; the schematic and the board are drawn in the studio.\nparts: []\nnets: []\n")
            .map_err(|e| format!("cannot write {}: {e}", intent.display()))?;
        let args: Vec<String> = vec!["new".into(), intent.display().to_string(), "-o".into(), dir.display().to_string()];
        crate::board::run(&args, |_| Ok(empty_design())).map_err(|e| crate::board::reasons(&e))?;
        Ok(dir)
    }

    /// `GET /api/workspace`'s answer: the workspace, its projects (newest first), the recent ones and the one open.
    pub(crate) fn json(&self, active: Option<&Path>) -> Value {
        let active_name = active.map(project_name);
        json!({
            "workspace": self.root.display().to_string(),
            "workspace_name": project_name(&self.root),
            "active": active_name,
            "projects": self.list().iter().map(|e| json!({
                "name": e.name,
                "path": e.dir.display().to_string(),
                "modified_ms": (e.modified / 1_000_000) as u64,
                "active": active_name.as_deref() == Some(e.name.as_str()),
            })).collect::<Vec<_>>(),
            "recent": self.recent(),
        })
    }
}

/// A design with nothing in it: no outline, no parts, no schematic drawn (KiCad's new project: an empty board and an empty schematic). The placement
/// section is there, empty: a board always has one (`eda_ops::Board`).
pub(crate) fn empty_design() -> eda_model::ir::Design {
    eda_model::ir::Design {
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: eda_model::ir::Provenance { engine_version: env!("CARGO_PKG_VERSION").into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(eda_model::ir::PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
        routing: None,
        drawings: None,
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
    }
}

/// When a time in milliseconds since 1970 was, for the CLI's listing.
fn ago(ms: u128) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let s = now.saturating_sub(ms) / 1000;
    match s {
        0..=59 => format!("{s}s ago"),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=86399 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    }
}

/// `eda workspace list|new <name>|open <name> [--workspace <dir>] [--port N]`: list and new work on the folder directly; open asks the studio serving
/// the workspace (`POST /api/workspace/open` on `--port`, default 8765) to switch, since the open project is the server's state.
pub fn run(rest: &[String]) -> Result<(), Vec<CheckResult>> {
    let flag = |name: &str| rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned();
    let usage = "usage: eda workspace <list | new <name> | open <name>> [--workspace <dir>] [--port N]";
    let root = flag("--workspace").or_else(|| flag("-W")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let verb = rest.first().map(String::as_str).unwrap_or("");
    let arg = rest.get(1).filter(|a| !a.starts_with("--")).cloned();
    match verb {
        "list" => {
            let ws = Workspace::open(&root)?;
            let recent = ws.recent();
            for e in ws.list() {
                let mark = if recent.first() == Some(&e.name) { "*" } else { " " };
                println!("{mark} {:<32} {}", e.name, ago(e.modified / 1_000_000));
            }
            Ok(())
        }
        "new" => {
            let name = arg.ok_or_else(|| fail("workspace_usage", "new", usage))?;
            let ws = Workspace::open(&root)?;
            let dir = ws.create(&name).map_err(|e| fail("workspace_new", &name, e))?;
            eprintln!("workspace: made {}", dir.display());
            Ok(())
        }
        "open" => {
            let name = arg.ok_or_else(|| fail("workspace_usage", "open", usage))?;
            let port: u16 = flag("--port").and_then(|p| p.parse().ok()).unwrap_or(8765);
            let reply = post_json(port, "/api/workspace/open", &json!({ "name": name })).map_err(|e| fail("workspace_open", &format!("127.0.0.1:{port}"), e))?;
            if reply["ok"].as_bool() == Some(true) {
                eprintln!("workspace: the studio on port {port} has opened {name}");
                Ok(())
            } else {
                Err(fail("workspace_open", &name, reply["message"].as_str().unwrap_or("the studio refused").to_string()))
            }
        }
        _ => Err(fail("workspace_usage", verb, usage)),
    }
}

/// One JSON POST to the studio on this machine (no HTTP client crate: the studio answers `Connection: close`).
fn post_json(port: u16, path: &str, body: &Value) -> Result<Value, String> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("no studio on port {port}: {e}"))?;
    let body = body.to_string();
    write!(s, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).map_err(|e| e.to_string())?;
    let mut text = String::new();
    s.read_to_string(&mut text).map_err(|e| e.to_string())?;
    let json = text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    serde_json::from_str(json).map_err(|e| format!("the studio's answer is not JSON ({e}): {}", text.lines().next().unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let t = std::env::temp_dir().join(format!("eda-ws-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&t).unwrap();
        t
    }

    #[test]
    fn a_name_cannot_leave_the_workspace() {
        for bad in ["", " ", "a/b", "a\\b", "..", "../x", "x..y", ".hidden", "tab\there", " lead"] {
            assert!(check_name(bad).is_err(), "{bad:?} must be refused");
        }
        for good in ["mcu30", "safety-power.old", "My Board 2", "ünïcode"] {
            assert!(check_name(good).is_ok(), "{good:?} is a name");
        }
    }

    #[test]
    fn the_list_holds_the_projects_newest_first_and_nothing_else() {
        let t = temp();
        let ws = Workspace::open(&t).unwrap();
        assert!(ws.list().is_empty());
        ws.create("first").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        ws.create("second.copy").unwrap();
        // not projects: a plain folder, a folder with only a board.json, a hidden folder, a file
        std::fs::create_dir_all(t.join("export")).unwrap();
        std::fs::create_dir_all(t.join("half")).unwrap();
        std::fs::write(t.join("half/board.json"), "{}").unwrap();
        std::fs::create_dir_all(t.join(".hidden")).unwrap();
        std::fs::write(t.join("notes.txt"), "x").unwrap();
        let names: Vec<_> = ws.list().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["second.copy", "first"]);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_new_project_is_an_empty_board_named_after_it_and_a_name_is_made_once() {
        let t = temp();
        let ws = Workspace::open(&t).unwrap();
        let dir = ws.create("blank").unwrap();
        assert_eq!(dir, ws.root.join("blank"));
        let (meta, design, model) = crate::board::load(&dir).unwrap();
        assert!(meta.intent.ends_with("blank.yaml"), "the board is named after the project: {}", meta.intent);
        assert!(model.parts.is_empty() && design.placement.as_ref().is_some_and(|p| p.outline.is_empty() && p.footprints.is_empty()) && design.schematic.is_none(), "an empty board and schematic");
        assert!(ws.create("blank").unwrap_err().contains("already exists"));
        assert!(ws.create("../escape").is_err());
        assert!(!t.parent().unwrap().join("escape").exists());
        // an empty folder of that name is taken, as KiCad takes an empty folder; one with files is not
        std::fs::create_dir_all(t.join("empty")).unwrap();
        assert!(ws.create("empty").is_ok());
        std::fs::create_dir_all(t.join("full")).unwrap();
        std::fs::write(t.join("full/x.txt"), "x").unwrap();
        assert!(ws.create("full").unwrap_err().contains("not empty"));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn the_history_is_most_recent_first_and_forgets_what_is_gone() {
        let t = temp();
        let ws = Workspace::open(&t).unwrap();
        assert_eq!(ws.most_recent(), None, "no history: no project");
        for n in ["a", "b", "c"] {
            ws.create(n).unwrap();
        }
        ws.note_opened("a");
        ws.note_opened("b");
        ws.note_opened("a");
        assert_eq!(ws.recent(), ["a", "b"]);
        assert_eq!(ws.most_recent(), Some(ws.root.join("a")));
        std::fs::remove_dir_all(ws.root.join("a")).unwrap();
        assert_eq!(ws.recent(), ["b"], "a project deleted on disk leaves the history");
        assert_eq!(ws.resolve(Path::new("b")).unwrap(), ws.root.join("b"));
        assert_eq!(ws.resolve(&ws.root.join("c")).unwrap(), ws.root.join("c"));
        assert!(ws.resolve(Path::new("/tmp")).is_err());
        let _ = std::fs::remove_dir_all(&t);
    }
}
