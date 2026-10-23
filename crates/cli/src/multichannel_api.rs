//! `/api/multichannel/...`: what the dialogs of the multichannel tool show (GAPS.md items 24 and 32, package P24).
//!
//! The tool itself is a command (`Cmd::Multichannel`); these routes are the reads the dialogs make first, none of which changes the board:
//!
//! * `POST /api/multichannel/areas` -- the rows of Generate Placement Rule Areas (`MULTICHANNEL_TOOL::GeneratePotentialRuleAreas`): a sheet, component class or group each, with the
//!   footprints it would hold. `{ "ok": true, "areas": [{ "kind", "key", "file", "components" }] }`.
//! * `POST /api/multichannel/sources` -- the choices of the Placement tab of Rule Area Properties: `{ "ok": true, "sheets": [..], "classes": [..], "groups": [..] }`.
//! * `POST /api/multichannel/compat` with `{ "reference": <zone id> }` -- `CheckRACompatibility`: the reference area matched against every other placement rule area.
//!   `{ "ok": true, "id", "name", "components": [..], "targets": [{ "id", "name", "ok", "message", "reasons", "components", "pairs" }] }`.

use crate::board;
use eda_ops::Board;
use serde_json::{json, Value};
use std::path::Path;

fn with_board(dir: &Path, f: impl FnOnce(&Board<'_>) -> Value) -> Value {
    match board::load(dir) {
        Ok((meta, design, model)) => {
            let b = Board::new(design, &model, meta.snap_um, meta.spacing_um);
            f(&b)
        }
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

pub fn areas(dir: &Path) -> Value {
    with_board(dir, |b| json!({ "ok": true, "areas": b.mc_potential_areas() }))
}

pub fn sources(dir: &Path) -> Value {
    with_board(dir, |b| {
        let s = b.mc_sources();
        json!({ "ok": true, "sheets": s.sheets, "classes": s.classes, "groups": s.groups })
    })
}

pub fn compat(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": format!("not a compatibility request: {e}") }),
    };
    let reference = req.get("reference").and_then(Value::as_str).unwrap_or("").to_string();
    with_board(dir, |b| match b.mc_compat(&reference) {
        Ok(plan) => {
            let mut v = serde_json::to_value(&plan).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("ok".into(), json!(true));
            }
            v
        }
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    })
}
