//! The rows of KiCad 10's Drill Groups dialog (`DIALOG_DRILL_GROUPS::Reload`, pcbnew/dialogs/dialog_drill_groups.cpp of KiCad 25ec2335): `GET /api/drill_groups`.
//!
//! `DRILL_CHART_MODEL( profile ).Build( board, EnumerateDrillSpans( board ) )` -- every hole of the board grouped by the board's drill symbol profile, with no filter
//! ([`eda_ops::drill_chart::groups`]) -- each group with the mark the board's symbol resolver gives it ("Symbols come from the resolver, not the profile, so a board where no
//! chart has ever been placed still shows the marks its map would draw": [`eda_ops::drill_chart::resolve`]), the profile's name and its Freeze assignments. The dialog's edits
//! are `set_drill_profile` commands.

use crate::board;
use eda_model::drill_chart::DrillMarkMode;
use eda_model::ir::Design;
use eda_model::ConstraintModel;
use serde_json::{json, Value};
use std::path::Path;

/// `markText`: a letter, the diameter in millimetres to two places, or "Shape N" (counted from 1).
pub(crate) fn mark_text(mode: DrillMarkMode, shape_index: i32, letter: &str, diameter_mm: f64) -> String {
    match mode {
        DrillMarkMode::Letter => letter.to_string(),
        DrillMarkMode::SizeText => format!("{diameter_mm:.2}"),
        DrillMarkMode::Shape => format!("Shape {}", shape_index + 1),
    }
}

/// The dialog's rows: `{ key, symbol: {mode, shape_index, letter, description}, mark_text, diameter_nm, is_slot, size_nm: [x, y], not_plated, top, bottom, operations, sites,
/// members }`, in the model's order (plated first, then by diameter, then by key).
pub(crate) fn rows(design: &Design, model: &ConstraintModel) -> Vec<Value> {
    let profile = eda_ops::drill_chart::profile_of(design);
    let resolved = eda_ops::drill_chart::resolve(design, model);
    let groups = eda_ops::drill_chart::groups(design, model, &profile, &Default::default());
    groups
        .into_iter()
        .map(|g| {
            let symbol = resolved.get(&g.key).cloned().unwrap_or(g.symbol);
            let f = &g.first;
            json!({
                "key": g.key,
                "symbol": symbol,
                "mark_text": mark_text(symbol.mode, symbol.shape_index, &symbol.letter, eda_model::units::to_mm(f.diameter)),
                "diameter_nm": f.diameter,
                "is_slot": f.is_slot,
                "size_nm": [f.size.0, f.size.1],
                "not_plated": f.not_plated,
                "top": f.top,
                "bottom": f.bottom,
                "operations": g.operations,
                "sites": g.sites.len(),
                "members": g.members,
            })
        })
        .collect()
}

/// `GET /api/drill_groups`: `{ ok, profile_name, freeze_assignments, groups }`.
pub fn drill_groups(dir: &Path) -> Value {
    match board::load(dir) {
        Ok((_, design, model)) => {
            let profile = eda_ops::drill_chart::profile_of(&design);
            json!({ "ok": true, "profile_name": profile.name, "freeze_assignments": profile.freeze_assignments, "groups": rows(&design, &model) })
        }
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_groups_mark_reads_as_the_dialogs_mark_text() {
        assert_eq!(mark_text(DrillMarkMode::Shape, 0, "", 0.3), "Shape 1");
        assert_eq!(mark_text(DrillMarkMode::Letter, 0, "B", 0.3), "B");
        assert_eq!(mark_text(DrillMarkMode::SizeText, 0, "", 0.8), "0.80");
    }
}
