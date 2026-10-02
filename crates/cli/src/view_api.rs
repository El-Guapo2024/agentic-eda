//! Shared view state: what the person in the studio is looking at, and
//! what an agent wants them to look at -- active tab, selection, camera,
//! and a "zoom to these items" request. Lives in `view.json` next to
//! `design.json` (never in the design: it is not part of the board, and
//! writing it must not reload the scene or enter undo history).
//!
//! Every write bumps `rev` and records `by` (`ui`, `cli`, or `EDA_ACTOR`).
//! The studio applies a revision it did not write itself, so an agent can
//! drive the view without the page's own camera updates fighting back.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

fn view_path(dir: &Path) -> PathBuf {
    dir.join("view.json")
}

/// The current view (defaults when nothing has been shared yet).
pub fn get(dir: &Path) -> Value {
    std::fs::read_to_string(view_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "rev": 0, "by": null, "tab": "pcb", "selection": [], "center": null, "scale": null, "zoom_to": null }))
}

/// Merge `patch`'s known fields into the view, bump `rev`, stamp `by`.
/// `zoom_to` is a one-shot request: it is cleared by any later write that
/// doesn't repeat it.
pub fn set(dir: &Path, patch: &Value, by: &str) -> Value {
    let mut v = get(dir);
    let obj: &mut Map<String, Value> = v.as_object_mut().expect("view is an object");
    for key in ["tab", "selection", "center", "scale"] {
        if let Some(x) = patch.get(key) {
            obj.insert(key.into(), x.clone());
        }
    }
    obj.insert("zoom_to".into(), patch.get("zoom_to").cloned().unwrap_or(Value::Null));
    let rev = obj.get("rev").and_then(Value::as_u64).unwrap_or(0) + 1;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    obj.insert("rev".into(), json!(rev));
    obj.insert("by".into(), json!(by));
    obj.insert("t".into(), json!(t));
    let _ = std::fs::write(view_path(dir), serde_json::to_string_pretty(&v).unwrap_or_default());
    v
}

/// `POST /api/view` body: a partial view, written as `ui`.
pub fn post(dir: &Path, body: &str) -> Value {
    match serde_json::from_str::<Value>(body) {
        Ok(patch) => set(dir, &patch, "ui"),
        Err(e) => json!({ "error": format!("bad view json: {e}") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_bumps_rev_and_one_shot_zoom_clears() {
        let dir = std::env::temp_dir().join(format!("eda-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(view_path(&dir));
        let a = set(&dir, &json!({ "selection": ["U1"], "zoom_to": ["U1"] }), "agent");
        assert_eq!(a["rev"], 1);
        assert_eq!(a["by"], "agent");
        let b = set(&dir, &json!({ "center": [1, 2] }), "ui");
        assert_eq!(b["rev"], 2);
        assert_eq!(b["selection"], json!(["U1"]), "unmentioned fields are kept");
        assert!(b["zoom_to"].is_null(), "zoom_to is one-shot");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
