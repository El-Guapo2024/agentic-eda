//! `POST /api/swap_nets/preview`: the question Swap Pad Nets and Swap Gate Nets ask before they swap (`PromptConnectedPadDecision`, `pcbnew/tools/edit_tool_move_fct.cpp`) --
//! which pads that are not selected the copper connected to the swapped pads reaches. The body is `{ "cmd": <swap_pad_nets | swap_gate_nets> }`; the verb is tried on a copy
//! of the board with no answer given, and the reply is `{ "ok": true, "pads": ["R2.1", ..] }` (empty: nothing to ask, the swap can go) or `{ "ok": false, "message": .. }` (the
//! swap is refused, and why). Nothing on the board changes.

use serde_json::{json, Value};
use std::path::Path;

pub(crate) fn preview(dir: &Path, body: &[u8]) -> Value {
    let fail = |message: String| json!({ "ok": false, "message": message });
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return fail(format!("not JSON: {e}")),
    };
    let mut cmd: eda_ops::Cmd = match serde_json::from_value(req.get("cmd").cloned().unwrap_or(Value::Null)) {
        Ok(c) => c,
        Err(e) => return fail(format!("not a board command: {e}")),
    };
    // No answer yet: that is what the verb answers with the list.
    match &mut cmd {
        eda_ops::Cmd::SwapPadNets { connected_pads, .. } | eda_ops::Cmd::SwapGateNets { connected_pads, .. } => *connected_pads = None,
        _ => return fail("not a swap of nets".to_string()),
    }
    let (meta, design, model) = match crate::board::load(dir) {
        Ok(loaded) => loaded,
        Err(e) => return fail(crate::board::reasons(&e)),
    };
    let mut board = eda_ops::Board::new(design, &model, meta.snap_nm, meta.spacing_nm);
    match board.apply(&cmd) {
        Ok(()) => json!({ "ok": true, "pads": [] }),
        Err(failures) => match failures.iter().find(|c| c.check == "ops_swap_unselected_pads") {
            Some(c) => json!({ "ok": true, "pads": c.detail.as_ref().and_then(|d| d.get("pads")).cloned().unwrap_or_else(|| json!([])) }),
            None => fail(crate::board::reasons(&failures)),
        },
    }
}
