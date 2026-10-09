//! The Appearance panel's settings, kept per project: which layers and objects are on, the opacities, the net colour mode, the hidden nets and net
//! classes, the colours of nets and net classes, the layer presets and viewports the person saved. They live in `appearance.json` next to `design.json`
//! and never in the design: they say how the person looks at the board, not what it is, so writing them does not reload the scene or enter the undo
//! history (the same reasoning as `view_api`'s `view.json`).
//!
//! The file is the studio's counterpart of the two files KiCad splits this between -- `local` is the project's local settings (`<project>.kicad_prl`),
//! `project` what KiCad keeps in the project file (`<project>.kicad_pro`: `net_settings.net_colors`, the net classes' `pcb_color`, `board.layer_presets`,
//! `board.viewports`). The derived KiCad project the engine writes (`crates/kicad-engine`) takes both over. The page owns the keys and their meaning
//! (`web/studio/src/kicad-port/appearanceFile.ts`); this module stores two objects and refuses anything else, so a bad write cannot leave the file
//! unreadable.
//!
//! `GET /api/appearance` returns the file (an empty one when nothing was saved); `POST /api/appearance` takes `{ local?, project? }` and replaces each
//! section that is present. Last write wins -- two pages open on one project is the same single person looking at it twice.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// The file's version, bumped when a key changes meaning.
pub const VERSION: u64 = 1;

/// A write larger than this is refused: a project's presets and colours are kilobytes.
const MAX_BYTES: usize = 2_000_000;

fn file_path(dir: &Path) -> PathBuf {
    dir.join("appearance.json")
}

fn empty() -> Value {
    json!({ "version": VERSION, "local": {}, "project": {} })
}

/// The stored settings, or an empty file when none was saved (or the file is not what this module writes).
pub fn get(dir: &Path) -> Value {
    read(dir).unwrap_or_else(empty)
}

/// The file as stored, `None` when it is missing or unreadable. Both sections are always objects.
pub fn read(dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(file_path(dir)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let obj = v.as_object()?;
    let section = |k: &str| obj.get(k).filter(|s| s.is_object()).cloned().unwrap_or_else(|| json!({}));
    Some(json!({ "version": obj.get("version").and_then(Value::as_u64).unwrap_or(VERSION), "local": section("local"), "project": section("project") }))
}

/// `POST /api/appearance`: replace the sections `body` carries, keep the others, write the file, return it.
pub fn post(dir: &Path, body: &str) -> Value {
    if body.len() > MAX_BYTES {
        return json!({ "error": format!("appearance settings too large ({} bytes)", body.len()) });
    }
    let patch: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json!({ "error": format!("bad appearance json: {e}") }),
    };
    let Some(patch) = patch.as_object() else { return json!({ "error": "appearance settings must be a JSON object" }) };
    let mut current = get(dir);
    let cur: &mut Map<String, Value> = current.as_object_mut().expect("the file is an object");
    for key in ["local", "project"] {
        match patch.get(key) {
            None => {}
            Some(v) if v.is_object() => {
                cur.insert(key.into(), v.clone());
            }
            Some(_) => return json!({ "error": format!("appearance `{key}` must be an object") }),
        }
    }
    cur.insert("version".into(), json!(VERSION));
    let text = serde_json::to_string_pretty(&current).unwrap_or_default();
    // Write beside the file and rename, so a crash mid-write leaves the old settings and not half of the new ones.
    let tmp = dir.join("appearance.json.tmp");
    if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, file_path(dir))) {
        let _ = std::fs::remove_file(&tmp);
        return json!({ "error": format!("could not write appearance.json: {e}") });
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-appearance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn nothing_saved_reads_as_an_empty_file() {
        let dir = scratch("empty");
        assert_eq!(get(&dir), json!({ "version": 1, "local": {}, "project": {} }));
        assert!(read(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_is_stored_beside_the_design_and_read_back() {
        let dir = scratch("roundtrip");
        let saved = post(&dir, r#"{"local":{"hidden_nets":["GND"],"opacity":{"zones":0.4}},"project":{"net_colors":{"GND":"rgb(1, 2, 3)"}}}"#);
        assert_eq!(saved["local"]["hidden_nets"], json!(["GND"]));
        assert_eq!(get(&dir), saved);
        assert!(dir.join("appearance.json").exists());
        assert!(!dir.join("design.json").exists(), "never part of the design");
        assert!(!dir.join("appearance.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_section_that_is_missing_from_the_write_keeps_what_was_stored() {
        let dir = scratch("sections");
        post(&dir, r#"{"local":{"a":1},"project":{"b":2}}"#);
        let v = post(&dir, r#"{"local":{"a":3}}"#);
        assert_eq!(v["local"], json!({ "a": 3 }));
        assert_eq!(v["project"], json!({ "b": 2 }), "the project section was not in the write");
        let v = post(&dir, r#"{"project":{}}"#);
        assert_eq!(v["project"], json!({}), "an empty section replaces");
        assert_eq!(v["local"], json!({ "a": 3 }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_write_is_refused_and_leaves_the_file_alone() {
        let dir = scratch("bad");
        let ok = post(&dir, r#"{"local":{"a":1}}"#);
        for body in ["not json", "[1]", r#"{"local":[1]}"#, r#"{"project":"x"}"#] {
            let r = post(&dir, body);
            assert!(r.get("error").is_some(), "{body} should be refused: {r}");
        }
        assert_eq!(get(&dir), ok);
        assert!(post(&dir, &" ".repeat(MAX_BYTES + 1)).get("error").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_somebody_else_wrote_is_read_with_both_sections_objects() {
        let dir = scratch("foreign");
        std::fs::write(dir.join("appearance.json"), r#"{"version":1,"local":7,"extra":true}"#).unwrap();
        assert_eq!(get(&dir), json!({ "version": 1, "local": {}, "project": {} }));
        std::fs::write(dir.join("appearance.json"), "{ not json").unwrap();
        assert_eq!(get(&dir), json!({ "version": 1, "local": {}, "project": {} }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
