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
///
/// The camera is in the design's units: `center` a point in nanometres, `scale` pixels per nanometre. A `view.json` written before
/// the design was in nanometres has no `"units": "nm"`; its micrometre camera is read as the same view (the centre x1000, the scale
/// /1000), and the next write stores it in nanometres.
pub fn get(dir: &Path) -> Value {
    let mut v = std::fs::read_to_string(view_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "rev": 0, "by": null, "tab": "pcb", "selection": [], "center": null, "scale": null, "zoom_to": null, "units": "nm" }));
    let obj = v.as_object_mut().expect("view is an object");
    if obj.get("units").and_then(Value::as_str) != Some("nm") {
        let times = |x: &Value, k: f64| x.as_f64().map(|f| json!(f * k));
        if let Some(c) = obj.get_mut("center") {
            match c {
                Value::Array(a) => {
                    for x in a.iter_mut() {
                        if let Some(n) = times(x, 1000.0) {
                            *x = n;
                        }
                    }
                }
                Value::Object(o) => {
                    for k in ["x", "y"] {
                        if let Some(n) = o.get(k).and_then(|x| times(x, 1000.0)) {
                            o.insert(k.into(), n);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(n) = obj.get("scale").and_then(|x| times(x, 0.001)) {
            obj.insert("scale".into(), n);
        }
        obj.insert("units".into(), json!("nm"));
    }
    v
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
    obj.insert("units".into(), json!("nm"));
    let rev = obj.get("rev").and_then(Value::as_u64).unwrap_or(0) + 1;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    obj.insert("rev".into(), json!(rev));
    obj.insert("by".into(), json!(by));
    obj.insert("t".into(), json!(t));
    let _ = std::fs::write(view_path(dir), serde_json::to_string_pretty(&v).unwrap_or_default());
    v
}

/// `POST /api/view` body: a partial view, written as `ui`. With `base_rev`
/// (the revision the page last knew), a change that someone else made
/// after that revision wins: the page's update is a stale debounce of an
/// older state, so it is dropped rather than overwriting the newer view.
pub fn post(dir: &Path, body: &str) -> Value {
    match serde_json::from_str::<Value>(body) {
        Ok(patch) => {
            let current = get(dir);
            let newer_from_someone_else = patch.get("base_rev").and_then(Value::as_u64).is_some_and(|base| {
                current.get("rev").and_then(Value::as_u64).unwrap_or(0) > base && current.get("by").and_then(Value::as_str) != Some("ui")
            });
            if newer_from_someone_else {
                return current;
            }
            set(dir, &patch, "ui")
        }
        Err(e) => json!({ "error": format!("bad view json: {e}") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_ui_post_does_not_overwrite_a_newer_agent_view() {
        let dir = std::env::temp_dir().join(format!("eda-view-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(view_path(&dir));
        let ui = post(&dir, r#"{"selection":["U1"]}"#);
        assert_eq!(ui["rev"], 1);
        set(&dir, &json!({ "selection": ["R1"] }), "agent"); // rev 2
        let stale = post(&dir, r#"{"selection":[],"base_rev":1}"#);
        assert_eq!(stale["selection"], json!(["R1"]), "the agent's newer selection wins");
        let fresh = post(&dir, r#"{"selection":[],"base_rev":2}"#);
        assert_eq!(fresh["selection"], json!([]), "a page that has seen rev 2 may change it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_bumps_rev_and_one_shot_zoom_clears() {
        let dir = std::env::temp_dir().join(format!("eda-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(view_path(&dir));
        let a = set(&dir, &json!({ "selection": ["U1"], "zoom_to": ["U1"] }), "agent");
        assert_eq!(a["rev"], 1);
        assert_eq!(a["by"], "agent");
        let b = set(&dir, &json!({ "center": [1, 2000] }), "ui");
        assert_eq!(b["rev"], 2);
        assert_eq!(b["selection"], json!(["U1"]), "unmentioned fields are kept");
        assert!(b["zoom_to"].is_null(), "zoom_to is one-shot");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_micrometre_view_is_read_as_the_same_view_in_nanometres_and_stored_so_once() {
        let dir = std::env::temp_dir().join(format!("eda-view-um-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(view_path(&dir), r#"{"rev":3,"by":"ui","tab":"pcb","selection":[],"center":[12500,-4000],"scale":0.25,"zoom_to":null}"#).unwrap();
        let v = get(&dir);
        assert_eq!((v["center"].clone(), v["scale"].clone(), v["units"].clone()), (json!([12_500_000.0, -4_000_000.0]), json!(0.00025), json!("nm")));
        let w = set(&dir, &json!({ "tab": "schematic" }), "ui");
        assert_eq!(w["center"], json!([12_500_000.0, -4_000_000.0]), "a write stores the nanometres");
        assert_eq!(get(&dir)["center"], json!([12_500_000.0, -4_000_000.0]), "and reading them again does not scale them twice");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
