//! `/api/sch/*` for the schematic hierarchy: the dialogs that work across sheets and have to say what they would do before they do it.
//!
//! * `POST /api/sch/annotate`: what Annotate Schematic would do (`DIALOG_ANNOTATE`'s message window): the messages `AnnotateSymbols` reports and
//!   the findings of `CheckAnnotate`, on a copy of the board. The annotation itself is the `annotate` verb of `/api/cmd`.

use crate::board;
use eda_model::CheckResult;
use eda_ops::{AnnotateOptions, AnnotateOrder, Board};
use serde_json::{json, Value};
use std::path::Path;

fn bad(why: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail("hierarchy_request", "request", why)]
}

/// The board the way the studio shows it: a board with no stored schematic has the one derived from its intent.
fn loaded(dir: &Path) -> Result<(board::Meta, eda_model::ir::Design, eda_model::ConstraintModel), Vec<CheckResult>> {
    let (meta, mut design, model) = board::load(dir)?;
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model)?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    Ok((meta, design, model))
}

/// `POST /api/sch/annotate`: `{ sheet?: "id/id", reset_existing, order?, ids?, options? }` (the fields of `Cmd::Annotate`) -> `{ ok, changed, messages:
/// [{ severity, text }], problems: [...] }`. Nothing is written.
pub(crate) fn annotate(dir: &Path, body: &[u8]) -> Value {
    match annotate_report(dir, body) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

fn annotate_report(dir: &Path, body: &[u8]) -> Result<Value, Vec<CheckResult>> {
    let req: Value = serde_json::from_slice(body).map_err(|e| bad(format!("not JSON: {e}")))?;
    let sheet = req.get("sheet").and_then(Value::as_str).unwrap_or("").to_string();
    let reset = req.get("reset_existing").and_then(Value::as_bool).unwrap_or(false);
    let order: AnnotateOrder = match req.get("order") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| bad(format!("bad order: {e}")))?,
        _ => AnnotateOrder::default(),
    };
    let ids: Option<Vec<String>> = match req.get("ids") {
        Some(v) if !v.is_null() => Some(serde_json::from_value(v.clone()).map_err(|e| bad(format!("bad ids: {e}")))?),
        _ => None,
    };
    let options: AnnotateOptions = match req.get("options") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|e| bad(format!("bad options: {e}")))?,
        _ => AnnotateOptions::default(),
    };
    let (meta, design, model) = loaded(dir)?;
    let b = Board::new(design, &model, meta.snap_um, meta.spacing_um);
    let report = b.annotate_report(&sheet, reset, order, ids.as_deref(), &options)?;
    Ok(json!({
        "ok": true,
        "changed": report.changed,
        "messages": report.messages,
        "problems": report.problems,
    }))
}
