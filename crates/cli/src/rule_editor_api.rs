//! The server's half of the DRC Rule Editor (`web/studio/src/components/RuleEditorDialog.tsx`; the model is `eda_drc::rule_editor`, the verb
//! `Cmd::EditDrcRule`):
//!
//!   - **`GET /api/rule_editor`**: the board's rules as the editor's tree holds them -- `{"ok", "entries": [Entry + "layer_token"], "layer_options":
//!     {panel: [{token, label}]}, "codes": {panel: code}, "defaults": Fields, "nets", "netclasses", "areas"}`, or `{"ok": false, "message"}` when the file
//!     does not parse. `layer_token` is the layer selector's choice for the rule (`setSelectedLayers`); the lists feed the condition rows' Net, Netclass and
//!     Within Area selectors (`NET_SELECTOR`, `NETCLASS_SELECTOR`, `AREA_SELECTOR`: the board's nets, its net classes, its named rule areas, sorted).
//!   - **`POST /api/rule_editor/check`**: `{"entry", "rule"}` is what Save would say of a panel (`onSaveButtonClicked`): `{"ok", "valid", "message",
//!     "generated"}`, without changing anything; `{"syntax": {"name", "body"}}` is a Custom rule's Check Syntax (`DRC_RE_CUSTOM_RULE_PANEL::onCheckSyntax`):
//!     `{"ok", "message"}`; `{"compile": text}` is Board Setup > Custom Rules' "Check rule syntax" (`PANEL_SETUP_RULES::OnCompile`): `{"ok", "messages":
//!     [{"text", "severity"}]}`.

use eda_drc::rule_editor::{self, Fields, PanelType, RuleEdit};
use serde_json::{json, Map, Value};
use std::path::Path;

/// `GET /api/rule_editor`.
pub(crate) fn view(dir: &Path) -> Value {
    let (_, design, model) = match crate::board::load(dir) {
        Ok(l) => l,
        Err(e) => return json!({ "ok": false, "message": crate::board::reasons(&e) }),
    };
    let board = eda_ops::rule_editor::editor_board(&model);
    let entries = match eda_ops::rule_editor::entries(&model, &board) {
        Ok(e) => e,
        Err(message) => return json!({ "ok": false, "message": message }),
    };
    let listed: Vec<Value> = entries
        .iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
            v["layer_token"] = json!(rule_editor::selected_token(e.panel, &e.layers, &e.layer_source, &board));
            v
        })
        .collect();
    let mut options = Map::new();
    let mut codes = Map::new();
    for p in PanelType::ALL {
        let key = serde_json::to_value(p).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        options.insert(key.clone(), json!(rule_editor::layer_options(p.layer_category(), &board)));
        codes.insert(key, json!(p.code()));
    }
    let mut nets: Vec<String> = model.nets.iter().map(|n| n.name.clone()).filter(|n| !n.is_empty()).collect();
    nets.sort();
    nets.dedup();
    let mut netclasses: Vec<String> = model.board.net_classes.iter().map(|c| c.name.clone()).collect();
    netclasses.sort();
    netclasses.dedup();
    let mut areas: Vec<String> = design.routing.iter().flat_map(|r| r.zones.iter()).filter(|z| z.is_rule_area && !z.name.is_empty()).map(|z| z.name.clone()).collect();
    areas.sort();
    areas.dedup();
    json!({
        "ok": true,
        "entries": listed,
        "layer_options": options,
        "codes": codes,
        "defaults": Fields::default(),
        "nets": nets,
        "netclasses": netclasses,
        "areas": areas,
    })
}

/// `POST /api/rule_editor/check`.
pub(crate) fn check(dir: &Path, body: &[u8]) -> Value {
    let fail = |message: String| json!({ "ok": false, "message": message });
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return fail(format!("not JSON: {e}")),
    };
    let (_, _, model) = match crate::board::load(dir) {
        Ok(l) => l,
        Err(e) => return fail(crate::board::reasons(&e)),
    };
    let board = eda_ops::rule_editor::editor_board(&model);
    // Board Setup > Custom Rules' "Check rule syntax" (`PANEL_SETUP_RULES::OnCompile`, R6-D5): `{"compile": text}` -> `{"ok", "messages": [{"text", "severity"}]}`
    if let Some(text) = req.get("compile").and_then(Value::as_str) {
        let messages: Vec<Value> = rule_editor::compile_report(text, &board.env())
            .into_iter()
            .map(|r| {
                let severity = match r.severity {
                    eda_drc::dru::ReportedSeverity::Error => "error",
                    eda_drc::dru::ReportedSeverity::Warning => "warning",
                    eda_drc::dru::ReportedSeverity::Info => "info",
                };
                json!({ "text": r.text, "severity": severity })
            })
            .collect();
        return json!({ "ok": true, "messages": messages });
    }
    if let Some(syntax) = req.get("syntax") {
        let name = syntax["name"].as_str().unwrap_or("test");
        let text = syntax["body"].as_str().unwrap_or("");
        return json!({ "ok": true, "message": rule_editor::check_custom_syntax(name, text, &board.env()) });
    }
    let rule: RuleEdit = match serde_json::from_value(req.get("rule").cloned().unwrap_or(Value::Null)) {
        Ok(r) => r,
        Err(e) => return fail(format!("not a rule panel: {e}")),
    };
    let entry = req.get("entry").and_then(Value::as_u64).map(|i| i as usize);
    let entries = match eda_ops::rule_editor::entries(&model, &board) {
        Ok(e) => e,
        Err(message) => return fail(message),
    };
    match rule_editor::check_edit(&entries, entry, &rule, &board) {
        Ok(c) => json!({ "ok": true, "valid": c.valid, "message": c.message, "generated": c.generated }),
        Err(message) => fail(message),
    }
}
