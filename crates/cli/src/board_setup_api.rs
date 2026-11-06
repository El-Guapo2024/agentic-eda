//! The server's half of Board Setup > Design Rules > Component Classes (`web/studio/src/components/boardSetup/ComponentClassesPage.tsx`; the verb is
//! `Cmd::SetComponentClasses`, the model `eda_model::component_classes`):
//!
//!   - **`POST /api/component_classes`**: `{"assignments": [ClassAssignment]}` -- the page's assignments as they stand, saved or not -- is answered with what
//!     the page needs of the board: `{"ok", "fields", "sheets", "compiles": [bool], "matches": [[reference] | null]}`. `fields` and `sheets` are the lists
//!     `PANEL_ASSIGN_COMPONENT_CLASSES`' constructor gathers for the Footprint Field and Sheet conditions; `compiles[i]` is `Validate`'s question of the i-th
//!     assignment (`CompileAssignmentRule` gives it a rule); `matches[i]` the footprints it matches, which "Highlight matching footprints" brightens
//!     (`PANEL_COMPONENT_CLASS_ASSIGNMENT::OnHighlightItemsClick`), null when it does not compile. Nothing is written.
//!
//! The rule board (`eda_drc::rule_board`) is the board as KiCad's rule engine sees it: the footprints with their fields, sheets and the classes they have now,
//! so a custom condition that reads `A.hasComponentClass('..')` is judged as the board's own assignment rules are.

use eda_model::component_classes::ClassAssignment;
use serde_json::{json, Value};
use std::path::Path;

/// `POST /api/component_classes`.
pub(crate) fn component_classes(dir: &Path, body: &[u8]) -> Value {
    let fail = |message: String| json!({ "ok": false, "message": message });
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let assignments: Vec<ClassAssignment> = match req.get("assignments").cloned().map(serde_json::from_value).unwrap_or(Ok(Vec::new())) {
        Ok(a) => a,
        Err(e) => return fail(format!("not a list of component class assignments: {e}")),
    };
    let (_, design, model) = match crate::board::load(dir) {
        Ok(l) => l,
        Err(e) => return fail(crate::board::reasons(&e)),
    };
    let board = eda_drc::board::build(&design, &model);
    let rb = eda_drc::rule_board::RuleBoard::new(&design, &model, &board, eda_drc::constraints::Units::Mm, None);
    let (fields, sheets) = rb.component_class_page_lists();
    let matches: Vec<Option<Vec<String>>> = assignments.iter().map(|a| rb.assignment_matches(a)).collect();
    let compiles: Vec<bool> = matches.iter().map(Option::is_some).collect();
    json!({ "ok": true, "fields": fields, "sheets": sheets, "compiles": compiles, "matches": matches })
}
