//! Ported from `pcbnew/drc/drc_test_provider_annular_width.cpp` and
//! `drc_test_provider_via_diameter.cpp` (the latter is a thin wrapper
//! around `VIA_DIAMETER_CONSTRAINT`, folded in here since both only touch
//! vias in our model -- we have no differently-shaped-per-layer padstacks).
//!
//! Generated: `DRCE_ANNULAR_WIDTH`, `DRCE_VIA_DIAMETER`.

use crate::board::DrcBoard;
use crate::constraints;
use crate::item::{format_um, DrcRefItem, DrcViolation, ErrorType};
use eda_model::{BoardRules, PadKind};

pub fn check(board: &DrcBoard, rules: &BoardRules) -> Vec<DrcViolation> {
    let mut out = Vec::new();
    let min_annular = constraints::annular_width_min(rules);
    let min_via_dia = constraints::via_diameter_min(rules);

    for v in &board.vias {
        // Annular width, ported from `getPadAnnulusPts`'s via branch: the
        // ring is (diameter - drill) / 2, measured along one axis (both
        // axes are equal for a round via/drill).
        let width = (v.diameter - v.drill) / 2;
        if width < min_annular {
            let item = DrcRefItem { description: format!("Via [{}] on {}-{}", v.net.as_deref().unwrap_or("<no net>"), v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() };
            out.push(DrcViolation::new(ErrorType::AnnularWidth, format!("(board setup constraints min annular width {}; actual {})", format_um(min_annular), format_um(width)), vec![item]));
        }
        if v.diameter < min_via_dia {
            let item = DrcRefItem { description: format!("Via [{}] on {}-{}", v.net.as_deref().unwrap_or("<no net>"), v.from_layer, v.to_layer), pos: (v.at.x, v.at.y), id: v.id.clone() };
            out.push(DrcViolation::new(ErrorType::ViaDiameter, format!("(board setup constraints min diameter {}; actual {})", format_um(min_via_dia), format_um(v.diameter)), vec![item]));
        }
    }

    // Plated through-hole pads: same (size - drill) / 2 formula, taking the
    // tighter of the two axes -- `getPadAnnulusPts`'s fast path, which
    // `Run()` shows applies to every pad shape this port produces (rect,
    // roundrect, circle, oval).
    for p in &board.pads {
        if p.kind != PadKind::ThroughHole {
            continue;
        }
        let Some(drill) = p.drill_round else { continue };
        let (w, h) = match &p.copper {
            crate::kimath::Shape::Rect { x0, y0, x1, y1 } | crate::kimath::Shape::RoundRect { x0, y0, x1, y1, .. } => (x1 - x0, y1 - y0),
            crate::kimath::Shape::Circle { r, .. } => (r * 2, r * 2),
            crate::kimath::Shape::Stadium { a, b, r } => {
                let len = crate::kimath::dist(*a, *b);
                (len + r * 2, r * 2)
            }
            crate::kimath::Shape::Polygon { .. } => continue,
        };
        let width = ((w - drill) / 2).min((h - drill) / 2);
        if width < min_annular {
            let item = DrcRefItem { description: format!("Pad {} [{}] of {}", p.number, p.net.as_deref().unwrap_or("<no net>"), p.footprint_ref), pos: (p.center.x, p.center.y), id: p.id.clone() };
            out.push(DrcViolation::new(ErrorType::AnnularWidth, format!("(board setup constraints min annular width {}; actual {})", format_um(min_annular), format_um(width)), vec![item]));
        }
    }

    out
}
