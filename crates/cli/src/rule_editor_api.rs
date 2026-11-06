//! The server's half of the DRC Rule Editor (`web/studio/src/components/RuleEditorDialog.tsx`; the model is `eda_drc::rule_editor`, the verb
//! `Cmd::EditDrcRule`):
//!
//!   - **`GET /api/rule_editor`**: the board's rules as the editor's tree holds them -- `{"ok", "entries": [Entry + "layer_token"], "layer_options":
//!     {panel: [{token, label}]}, "codes": {panel: code}, "defaults": Fields, "nets", "netclasses", "areas"}`, or `{"ok": false, "message"}` when the file
//!     does not parse. `layer_token` is the layer selector's choice for the rule (`setSelectedLayers`); the lists feed the condition rows' Net, Netclass and
//!     Within Area selectors (`NET_SELECTOR`, `NETCLASS_SELECTOR`, `AREA_SELECTOR`: the board's nets, its net classes, its named rule areas, sorted).
//!   - **`POST /api/rule_editor/check`**: `{"entry", "rule"}` is what Save would say of a panel (`onSaveButtonClicked`): `{"ok", "valid", "message",
//!     "generated"}`, without changing anything; `{"syntax": {"name", "body"}}` is a Custom rule's Check Syntax (`DRC_RE_CUSTOM_RULE_PANEL::onCheckSyntax`):
//!     `{"ok", "message"}`; `{"compile": text}` is Board Setup > Custom Rules' "Check rule syntax" (`PANEL_SETUP_RULES::OnCompile`: the text's variables
//!     expanded, the parser's messages and `checkPlausibility`'s warnings): `{"ok", "messages": [{"text", "severity"}]}`.
//!   - **`POST /api/rule_editor/show_matches`**: `{"entry", "rule"}` -> the items of the board the panel's rule selects ("Show Matches", [`show_matches`]).

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
    let (meta, design, model) = match crate::board::load(dir) {
        Ok(l) => l,
        Err(e) => return fail(crate::board::reasons(&e)),
    };
    let board = eda_ops::rule_editor::editor_board(&model);
    // Board Setup > Custom Rules' "Check rule syntax" (`PANEL_SETUP_RULES::OnCompile`, R6-D5): `{"compile": text}` -> `{"ok", "messages": [{"text", "severity"}]}`.
    // The text is expanded first (`rulesText = ExpandTextVars( rulesText, &resolver )`, the resolver `BOARD::ResolveTextVar( token, 0 )`: the project's variables
    // and the board's own, `${REF:FIELD}` by the footprint's reference -- what `ConvertCrossReferencesToKIIDs` turns into a uuid for KiCad's resolver to find again),
    // then parsed, then held to the board (`checkPlausibility`).
    if let Some(text) = req.get("compile").and_then(Value::as_str) {
        let shown = crate::text_vars_json::PcbShown::new(dir, &design, &model, crate::text_vars_json::project(dir, &meta.intent, eda_model::text_vars::Clock::utc_now()));
        let expanded = eda_model::text_vars::expand_text_vars(text, Some(&|token: &str| shown.board().resolve(token, 0)), 0, 0);
        let net_classes: Vec<String> = model.board.net_classes.iter().map(|c| c.name.clone()).collect();
        let plausibility = rule_editor::PlausibilityBoard { net_classes: &net_classes, board: &board };
        let messages: Vec<Value> = rule_editor::compile_report_with(&expanded, &board.env(), Some(&plausibility))
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

/// `POST /api/rule_editor/show_matches`: `{"entry", "rule"}` (the open panel, as Save would take it) -> `{"ok": true, "count", "ids"}` -- "Show Matches"
/// (`DIALOG_DRC_RULE_EDITOR::highlightMatchingItems`): the items of the board the panel's generated rule selects (`eda_drc::show_matches`), `count` KiCad's
/// number of them (the button's "Show Matches (%d)") and `ids` what the studio selects and frames -- or `{"ok": true, "error": true}` where KiCad returns -1
/// ("Show Matches (error)": the rule does not parse, or its condition does not compile). Nothing is written.
pub(crate) fn show_matches(dir: &Path, body: &[u8]) -> Value {
    let fail = |message: String| json!({ "ok": false, "message": message });
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return fail(format!("not JSON: {e}")),
    };
    let rule: RuleEdit = match serde_json::from_value(req.get("rule").cloned().unwrap_or(Value::Null)) {
        Ok(r) => r,
        Err(e) => return fail(format!("not a rule panel: {e}")),
    };
    let (_, design, model) = match crate::board::load(dir) {
        Ok(l) => l,
        Err(e) => return fail(crate::board::reasons(&e)),
    };
    let board = eda_ops::rule_editor::editor_board(&model);
    let entries = match eda_ops::rule_editor::entries(&model, &board) {
        Ok(e) => e,
        Err(message) => return fail(message),
    };
    let entry = req.get("entry").and_then(Value::as_u64).map(|i| i as usize);
    // `m_ruleEditorPanel->TransferDataFromWindow()` then `GetGeneratedRule()`: the panel's rule as it stands, checked or not.
    let generated = match rule_editor::check_edit(&entries, entry, &rule, &board) {
        Ok(c) => c.generated,
        Err(message) => return fail(message),
    };
    let drc_board = eda_drc::board::build(&design, &model);
    let rb = eda_drc::rule_board::RuleBoard::new(&design, &model, &drc_board, eda_drc::constraints::Units::Mm, None);
    match eda_drc::show_matches::show_matches(&rb, &generated) {
        Ok(m) => json!({ "ok": true, "count": m.count, "ids": m.ids }),
        Err(()) => json!({ "ok": true, "error": true }),
    }
}
