//! HTTP handler for "Create Polygon / Zone / Rule Area from Selection": `POST /api/convert/polys`.
//!
//! Stateless and read-only, same shape as `tune_api.rs`: it reads `design.json` fresh and returns the
//! polygons `CONVERT_TOOL::CreatePolys`' `getPolys` builds from the given items
//! (`eda_ops::convert::polys_from_items`) plus the ids that contributed. The studio then adds the
//! polygons (or zones) and deletes the sources through ordinary `/api/cmd` commands, as one undo step.
//!
//! Request: `{ids: [..], strategy: "centerline" | "copy_linewidth" | "bounding_hull", gap?: um}`.
//! Reply: `{ok: true, rings: [[[x, y], ..], ..], consumed: [..]}`.

use crate::board;
use eda_ops::convert::{polys_from_items, ConvertStrategy};
use serde_json::{json, Value};
use std::path::Path;

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

pub fn polys(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let ids: Vec<String> = req.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if ids.is_empty() {
        return err("no items given");
    }
    let strategy = req.get("strategy").and_then(|v| serde_json::from_value::<ConvertStrategy>(v.clone()).ok()).unwrap_or(ConvertStrategy::Centerline);
    let gap = req.get("gap").and_then(Value::as_i64).unwrap_or(0);
    let (_, design, _) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let r = polys_from_items(&design, &ids, strategy, gap);
    json!({
        "ok": true,
        "rings": r.rings.iter().map(|ring| ring.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "consumed": r.consumed,
    })
}
