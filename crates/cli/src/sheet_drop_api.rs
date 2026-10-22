//! `POST /api/sch/dropped_sheets`: the schematic files of a drop on the sheet, put where Import Sheet can read them (`eeschema.EditorControl.ddAppendFile`).
//!
//! Ports what KiCad does for a `.kicad_sch` dropped on the schematic editor: `EDA_BASE_FRAME::OnDropFiles` looks the extension up in the frame's `m_acceptedExts`
//! (`sch_edit_frame.cpp`: `kicad_sch` -> `ddAppendFile`) and runs the action with the file's path; `SCH_EDIT_TOOL::DdAppendFile` runs Import Sheet with it
//! (`m_toolMgr->RunAction( SCH_ACTIONS::importSheet, aEvent.Parameter<wxString*>() )`), which reads the file and the sheets it places from the folder it is in
//! (`SCH_EDIT_FRAME::LoadSheetFromFile`).
//!
//! A browser hands the page a file's text and not its path, and a sheet that places other sheets finds them by their names beside it, so the page posts every `.kicad_sch` of the
//! drop and this puts them in a scratch folder of the server (the temporary folder: nothing of the project's changes) the way their placements say: the one that no other dropped file
//! places is the file that is imported -- Import Sheet's `source` is the path this answers -- and the files it places are written where it names them, relative to the file that
//! places them. A sheet whose file was not dropped is named in `missing`, with the file that places it (the check `LoadSheetFromFile` makes of a file read without its sheets, which says
//! the same of a sheet it cannot find); a file dropped and not placed by the one imported is in `left_out` (KiCad would run Import Sheet for each of the files in turn).
//!
//! Request: `{ "files": [{ "name": "main.kicad_sch", "text": "<the file>" }, ..] }`. Reply: `{ "ok": true, "path": "<the file to import, on the server>", "root": "main.kicad_sch",
//! "files": ["main.kicad_sch", "sub/power.kicad_sch"], "missing": [{ "file": "adc.kicad_sch", "placed_by": "main.kicad_sch" }], "left_out": ["notes.kicad_sch"] }`, or
//! `{ "ok": false, "message": ".." }`. The folder of the previous drops that are a day old is removed as a new one is made.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};

/// The most files one drop may carry and the most text: a project's schematics are a few dozen files of a few hundred kilobytes.
const MAX_FILES: usize = 64;
const MAX_BYTES: usize = 64 << 20;

/// A file of the drop, by the name the browser gave it (the last component of its path).
struct Dropped {
    name: String,
    text: String,
}

/// The last component of a path, either separator.
fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `rel` as a path that stays inside its folder: the normal components only, and `..` taking one back where there is one to take (the one a sheet names above the root is the root itself).
fn inside(rel: &str) -> PathBuf {
    let mut out: Vec<std::ffi::OsString> = Vec::new();
    for c in Path::new(&rel.replace('\\', "/")).components() {
        match c {
            Component::Normal(n) => out.push(n.to_os_string()),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out.into_iter().collect()
}

fn bad(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// A scratch folder for one drop, named so that the folders of earlier drops that nobody needs any more can be told by their name and their age.
fn fresh_scratch() -> Result<PathBuf, String> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let base = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&base) {
        let day = std::time::Duration::from_secs(24 * 3600);
        for e in entries.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > day);
            if old && e.file_name().to_string_lossy().starts_with("eda-dropped-sheets-") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let dir = base.join(format!("eda-dropped-sheets-{}-{stamp}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make a folder for the dropped files: {e}"))?;
    Ok(dir)
}

pub fn put(body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return bad(format!("not a drop: {e}")),
    };
    let Some(list) = req.get("files").and_then(Value::as_array) else { return bad("no files were dropped") };
    let mut files: Vec<Dropped> = Vec::new();
    for f in list {
        let (Some(name), Some(text)) = (f.get("name").and_then(Value::as_str), f.get("text").and_then(Value::as_str)) else { return bad("a dropped file has no name or no text") };
        let name = base_name(name).to_string();
        if !name.to_ascii_lowercase().ends_with(".kicad_sch") {
            return bad(format!("{name} is not a schematic file (.kicad_sch)"));
        }
        files.push(Dropped { name, text: text.to_string() });
    }
    if files.is_empty() {
        return bad("no schematic file was dropped");
    }
    if files.len() > MAX_FILES || files.iter().map(|f| f.text.len()).sum::<usize>() > MAX_BYTES {
        return bad(format!("a drop of more than {MAX_FILES} files or {} MB of schematics is not read", MAX_BYTES >> 20));
    }

    // The files each one places, as the file names them; a file dropped under the name a placement gives is the one it places.
    let refs: Vec<Vec<String>> = files.iter().map(|f| eda_kicad::referenced_sheet_files(&f.text)).collect();
    let mut by_base: BTreeMap<String, usize> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        by_base.entry(f.name.clone()).or_insert(i);
    }
    let lower: BTreeMap<String, usize> = by_base.iter().map(|(k, v)| (k.to_ascii_lowercase(), *v)).collect();
    let find = |named: &str| -> Option<usize> { by_base.get(base_name(named)).or_else(|| lower.get(&base_name(named).to_ascii_lowercase())).copied() };

    // The file that is imported: the first the others do not place (a file placing itself does not count as placed by another).
    let placed: BTreeSet<usize> = refs.iter().enumerate().flat_map(|(i, rs)| rs.iter().filter_map(move |r| find(r).filter(|j| *j != i))).collect();
    let root = (0..files.len()).find(|i| !placed.contains(i)).unwrap_or(0);

    let scratch = match fresh_scratch() {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    // The files where their placements say, parents first; a file the root reaches by two paths is written at both.
    let mut written: Vec<PathBuf> = Vec::new();
    let mut reached: BTreeSet<usize> = BTreeSet::new();
    let mut missing: Vec<Value> = Vec::new();
    let mut queue: VecDeque<(usize, PathBuf)> = VecDeque::from([(root, PathBuf::from(&files[root].name))]);
    while let Some((i, rel)) = queue.pop_front() {
        if written.contains(&rel) {
            continue;
        }
        let target = scratch.join(&rel);
        if let Some(parent) = target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return bad(format!("could not write {}: {e}", rel.display()));
            }
        }
        if let Err(e) = std::fs::write(&target, &files[i].text) {
            return bad(format!("could not write {}: {e}", rel.display()));
        }
        written.push(rel.clone());
        reached.insert(i);
        let folder = rel.parent().map(Path::to_path_buf).unwrap_or_default();
        for r in &refs[i] {
            match find(r) {
                Some(j) => queue.push_back((j, inside(&format!("{}/{}", folder.display(), r)))),
                None => {
                    if !missing.iter().any(|m| m["file"] == r.as_str() && m["placed_by"] == files[i].name.as_str()) {
                        missing.push(json!({ "file": r, "placed_by": files[i].name }));
                    }
                }
            }
        }
    }
    let left_out: Vec<&str> = files.iter().enumerate().filter(|(i, _)| !reached.contains(i)).map(|(_, f)| f.name.as_str()).collect();
    json!({
        "ok": true,
        "path": scratch.join(&files[root].name).to_string_lossy(),
        "root": files[root].name,
        "files": written.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect::<Vec<_>>(),
        "missing": missing,
        "left_out": left_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schematic text that places the sheets `files` (name -> file), as KiCad writes the placement.
    fn placing(files: &[(&str, &str)]) -> String {
        let sheets: String = files
            .iter()
            .map(|(name, file)| format!("(sheet (at 10 10) (size 20 10) (property \"Sheetname\" \"{name}\" (at 0 0 0)) (property \"Sheetfile\" \"{file}\" (at 0 0 0)))"))
            .collect();
        format!("(kicad_sch (version 20250114) (generator \"eeschema\") (uuid \"00000000-0000-0000-0000-000000000001\") (paper \"A4\") {sheets})")
    }

    fn drop_of(files: &[(&str, String)]) -> Vec<u8> {
        json!({ "files": files.iter().map(|(n, t)| json!({ "name": n, "text": t })).collect::<Vec<_>>() }).to_string().into_bytes()
    }

    #[test]
    fn a_file_alone_is_the_file_imported() {
        let r = put(&drop_of(&[("lonely.kicad_sch", placing(&[]))]));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!((r["root"].as_str(), r["files"].clone(), r["missing"].clone(), r["left_out"].clone()), (Some("lonely.kicad_sch"), json!(["lonely.kicad_sch"]), json!([]), json!([])));
        let path = PathBuf::from(r["path"].as_str().unwrap());
        assert!(path.is_file() && path.ends_with("lonely.kicad_sch"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The file that no other places is the root whatever the order of the drop; the sheets it places are written beside it, a sheet in a folder in that folder, and the one named
    /// at a place no file was dropped for is said.
    #[test]
    fn the_file_nobody_places_is_imported_with_its_sheets_beside_it_and_the_missing_ones_are_named() {
        let main = placing(&[("Power", "power.kicad_sch"), ("Sub", "sub/adc.kicad_sch"), ("Gone", "gone.kicad_sch")]);
        let r = put(&drop_of(&[("adc.kicad_sch", placing(&[("Deep", "deep.kicad_sch")])), ("power.kicad_sch", placing(&[])), ("main.kicad_sch", main)]));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!(r["root"], json!("main.kicad_sch"));
        let files: Vec<String> = r["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
        assert_eq!(files[0], "main.kicad_sch");
        assert!(files.contains(&"power.kicad_sch".to_string()) && files.contains(&"sub/adc.kicad_sch".to_string()), "{files:?}");
        assert_eq!(r["missing"], json!([{ "file": "gone.kicad_sch", "placed_by": "main.kicad_sch" }, { "file": "deep.kicad_sch", "placed_by": "adc.kicad_sch" }]));
        let path = PathBuf::from(r["path"].as_str().unwrap());
        let dir = path.parent().unwrap();
        assert!(dir.join("power.kicad_sch").is_file() && dir.join("sub/adc.kicad_sch").is_file());
        // the importer reads the tree as it is written: the two sheets that are there, and notes for the ones that are not
        let read = eda_kicad::load_sheet_file(&path).unwrap();
        assert!(read.notes.iter().any(|n| n.contains("'gone.kicad_sch' that 'main.kicad_sch' places was not found")), "{:?}", read.notes);
        assert!(read.notes.iter().any(|n| n.contains("'deep.kicad_sch' that 'adc.kicad_sch' places was not found")), "{:?}", read.notes);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn files_that_are_not_placed_by_the_one_imported_are_left_out_and_said() {
        let r = put(&drop_of(&[("a.kicad_sch", placing(&[("B", "b.kicad_sch")])), ("b.kicad_sch", placing(&[])), ("other.kicad_sch", placing(&[]))]));
        assert_eq!(r["root"], json!("a.kicad_sch"), "the first the others do not place");
        assert_eq!(r["left_out"], json!(["other.kicad_sch"]));
        let _ = std::fs::remove_dir_all(Path::new(r["path"].as_str().unwrap()).parent().unwrap());
    }

    #[test]
    fn a_name_is_found_in_either_case_and_a_path_stays_inside_the_folder() {
        // `Sheetfile` "../up/Power.kicad_sch" names the dropped `power.kicad_sch`: written where the placement says, inside the folder
        let r = put(&drop_of(&[("main.kicad_sch", placing(&[("P", "../up/Power.kicad_sch")])), ("power.kicad_sch", placing(&[]))]));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!(r["files"], json!(["main.kicad_sch", "up/Power.kicad_sch"]));
        assert_eq!(r["missing"], json!([]));
        assert_eq!(inside("../../x/./y.kicad_sch"), PathBuf::from("x/y.kicad_sch"));
        assert_eq!(inside("/abs/z.kicad_sch"), PathBuf::from("abs/z.kicad_sch"));
        let _ = std::fs::remove_dir_all(Path::new(r["path"].as_str().unwrap()).parent().unwrap());
    }

    #[test]
    fn a_cycle_and_a_file_that_places_itself_are_read_once() {
        let r = put(&drop_of(&[("a.kicad_sch", placing(&[("B", "b.kicad_sch")])), ("b.kicad_sch", placing(&[("A", "a.kicad_sch"), ("Me", "b.kicad_sch")]))]));
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!(r["root"], json!("a.kicad_sch"), "every file is placed by another: the first");
        assert_eq!(r["files"], json!(["a.kicad_sch", "b.kicad_sch"]));
        let _ = std::fs::remove_dir_all(Path::new(r["path"].as_str().unwrap()).parent().unwrap());
    }

    #[test]
    fn what_is_not_a_schematic_or_is_too_much_is_refused() {
        assert_eq!(put(&drop_of(&[("notes.txt", "x".into())]))["ok"], json!(false));
        assert_eq!(put(b"{}")["ok"], json!(false));
        assert_eq!(put(&drop_of(&[]))["ok"], json!(false));
        assert_eq!(put(b"nonsense")["ok"], json!(false));
        let many: Vec<(String, String)> = (0..=MAX_FILES).map(|i| (format!("f{i}.kicad_sch"), placing(&[]))).collect();
        let body = json!({ "files": many.iter().map(|(n, t)| json!({ "name": n, "text": t })).collect::<Vec<_>>() }).to_string();
        assert_eq!(put(body.as_bytes())["ok"], json!(false));
        // a path in the name is the file's name
        let r = put(&drop_of(&[("C:\\work\\deep\\x.kicad_sch", placing(&[]))]));
        assert_eq!(r["root"], json!("x.kicad_sch"));
        let _ = std::fs::remove_dir_all(Path::new(r["path"].as_str().unwrap()).parent().unwrap());
    }
}
