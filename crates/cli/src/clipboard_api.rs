//! `POST /api/clipboard/copy`: the text a PCB Copy puts on the system clipboard.
//!
//! A Copy changes nothing on the board (so it is no `/api/cmd` verb and has no undo step): it reads the named items from the
//! design as it stands and writes them the way KiCad's clipboard does (`CLIPBOARD_IO::SaveSelection`,
//! [`eda_kicad::export_clipboard`]). The Paste is the other half, `Cmd::PasteClipboard`, which takes that text back.
//!
//! Request: `{ "ids": [...], "reference": { "x": µm, "y": µm } }` -- the ids of the items to copy (placed parts' references and
//! track, via, zone, shape, text, dimension and group ids) and the point the copy is measured from (the cursor's grid point for a
//! plain Copy, the picked one for Copy with Reference Point; the origin when left out). Reply: `{ "ok": true, "text": "..." }` or
//! `{ "ok": false, "message": "..." }`.

use crate::board;
use eda_model::ir::Point;
use serde_json::{json, Value};
use std::path::Path;

pub fn copy(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "message": format!("not a copy request: {e}") }),
    };
    let ids: Vec<String> = req.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    // Read field by field: this build's `serde_json` keeps numbers arbitrary-precision (starlark turns it on), which a derived
    // reader of a tagged type cannot take.
    let reference = req.get("reference").filter(|r| r.is_object()).map(|r| Point { x: r.get("x").and_then(Value::as_i64).unwrap_or(0), y: r.get("y").and_then(Value::as_i64).unwrap_or(0) }).unwrap_or(Point { x: 0, y: 0 });
    let (_, design, model) = match board::load(dir) {
        Ok(t) => t,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e) }),
    };
    match eda_kicad::export_clipboard(&design, &model, &ids, reference) {
        Ok(text) => json!({ "ok": true, "text": text }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}
