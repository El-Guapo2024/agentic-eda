//! Ported from `pcbnew/drc/drc_test_provider_text_dims.cpp`, restricted (as
//! the real implicit rule is: `loadImplicitRules` scopes both constraints to
//! `LSET({F_SilkS, B_SilkS})`) to text actually on a silk layer -- our model
//! only ever places free-standing `Text` there or on fab/other layers, and
//! a footprint's reference/value fields are silk by construction (see
//! `eda_kicad::pcb::write_footprint`).
//!
//! Font is always KiCad's stroke font in this model (no outline/TrueType
//! text), so only the plain min-height/min-thickness branch of the C++
//! applies; the outline-font glyph-collapse test is not ported.
//!
//! Generated: `DRCE_TEXT_HEIGHT`, `DRCE_TEXT_THICKNESS`.

use crate::board::DrcBoard;
use crate::constraints;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use eda_model::BoardRules;

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let min_height = constraints::min_silk_text_height(rules);
    let min_thickness = constraints::min_silk_text_thickness(rules);

    for t in &board.texts {
        if t.layer != "F.SilkS" && t.layer != "B.SilkS" {
            continue;
        }
        let item = || DrcRefItem { description: format!("Text on {}", t.layer), pos: (t.at.x, t.at.y), id: t.id.clone() };
        if t.size_um < min_height {
            out.push(DrcViolation::new(ErrorType::TextHeight, format!("(board setup constraints min height {}; actual {})", format_um(min_height), format_um(t.size_um)), vec![item()]));
        }
        if t.stroke_width < min_thickness {
            out.push(DrcViolation::new(ErrorType::TextThickness, format!("(board setup constraints min thickness {}; actual {})", format_um(min_thickness), format_um(t.stroke_width)), vec![item()]));
        }
    }
    out
}
