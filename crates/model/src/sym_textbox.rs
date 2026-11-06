//! A library symbol's text box (`SCH_TEXTBOX` on `LAYER_DEVICE`, [`crate::ir::LibrarySymbolGraphic::TextBox`]) as a placed symbol has it.
//!
//! The engine's library symbol keeps its text boxes in [`crate::symbol::LibConnectivity::text_boxes`]; its `graphics` has each one's border and fill as a rectangle
//! ([`crate::ir::LibrarySymbolGraphic::to_engine_graphics`]). The corners of a box go through the placed symbol's transform like any other point of it; what the
//! symbol's turn and mirror do to the text is not the same in KiCad's two drawings of it:
//!
//! - The canvas, `SCH_PAINTER::draw( const SCH_SYMBOL* )` (eeschema/sch_painter.cpp): a copy of the library symbol has every item put through the symbol's
//!   orientation (`OrientAndMirrorSymbolItems`, eeschema/symb_transforms_utils.cpp) -- `SCH_TEXTBOX::Rotate` turns a horizontal text vertical and back,
//!   `MirrorHorizontally` / `MirrorVertically` swap a left and a right justification of a horizontal / vertical text ("Text is NOT really mirrored; it just has its
//!   justification flipped"). The studio's canvas does that (web/studio `kicad-port/symTextBoxPlaced.ts`).
//! - The plot, `SCH_SYMBOL::Plot` -> `SCH_TEXTBOX::Plot` (eeschema/sch_textbox.cpp), what kicad-cli's SVG, PDF ... are: the corners through the symbol's
//!   `TRANSFORM`, the text turned to the other direction when the transform turns a quarter (`if( renderSettings->m_Transform.y1 )`), and its justification left as
//!   the library has it, mirrored or not. The derived `.kicad_sch` writes each symbol unturned with its library symbol baked through its turn and mirror, so its
//!   text boxes are baked so ([`plotted_text_box`]): kicad-cli then plots the design as it plots KiCad's own file of it (checked against kicad-cli's plot of the
//!   unbaked symbol, `crates/kicad/tests/sch_lib_text_box.rs`).
//!
//! In both, a box's text with variables is the placed symbol's (`expandLibItemTextVars` / `SCH_FIELD::GetShownText` of a field of the symbol). KiCad 25ec2335 is the same.

use crate::ir::LibrarySymbolGraphic;
use crate::symbol::SPoint;

/// Whether a text box's `angle_deg` (0 or 90) runs its text up the box (`GetTextAngle().IsVertical()`).
pub fn is_vertical(angle_deg: f64) -> bool {
    (angle_deg.round() as i64).rem_euclid(180) == 90
}

/// Whether a placed symbol's transform turns a quarter (`TRANSFORM::y1 != 0`): its turn, as the engine folds it with the mirrors (`eda_engine::symgeom::kicad_turn`), is
/// 90 or 270 degrees. A turn that is not a quarter one is rounded to the nearest (a placed symbol has no other).
pub fn turns_a_quarter(rot_millideg: u32) -> bool {
    (((rot_millideg % 360_000) as f64 / 90_000.0).round() as u32) % 2 == 1
}

/// `SCH_TEXTBOX::Plot` of a library text box of a symbol placed with a transform that turns a quarter or not (see the module doc): `place` takes each corner where the
/// transform and the symbol's position put it (`renderSettings->TransformCoordinate( m_start ) + aOffset`), the text runs the other way after a quarter turn
/// (`SetTextAngle( GetTextAngle() == ANGLE_HORIZONTAL ? ANGLE_VERTICAL : ANGLE_HORIZONTAL )`), and its justification is the library's. Anything but a text box
/// comes back as it is.
pub fn plotted_text_box(g: &LibrarySymbolGraphic, quarter_turn: bool, place: impl Fn(SPoint) -> SPoint) -> LibrarySymbolGraphic {
    let mut out = g.clone();
    if let LibrarySymbolGraphic::TextBox { start, end, angle_deg, .. } = &mut out {
        *start = place(*start);
        *end = place(*end);
        let vertical = is_vertical(*angle_deg) != quarter_turn;
        *angle_deg = if vertical { 90.0 } else { 0.0 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sym_fields::{FieldHAlign, FieldVAlign};

    #[test]
    fn a_quarter_turn_is_one_of_90_or_270_degrees() {
        assert_eq!([0, 90_000, 180_000, 270_000, 450_000, 89_000].map(turns_a_quarter), [false, true, false, true, true, true]);
    }

    #[test]
    fn a_plotted_text_box_has_its_corners_placed_and_runs_the_other_way_after_a_quarter_turn() {
        let tb = LibrarySymbolGraphic::TextBox {
            id: String::new(),
            unit: 1,
            body_style: 1,
            text: "10V".into(),
            start: SPoint::new(-17.78, 6.35),
            end: SPoint::new(-10.16, -1.27),
            angle_deg: 0.0,
            size_mm: 1.27,
            bold: true,
            italic: false,
            h_align: FieldHAlign::Left,
            v_align: FieldVAlign::Top,
            margin_mm: 1.016,
            stroke_mm: 0.254,
            fill: crate::ir::LibraryFill::None,
            private: false,
            font: None,
            text_color: None,
            stroke_color: None,
            fill_color: None,
        };
        let turned = plotted_text_box(&tb, true, |p| SPoint::new(-p.y, p.x));
        let LibrarySymbolGraphic::TextBox { start, end, angle_deg, h_align, v_align, ref text, .. } = turned else { panic!("a text box") };
        assert_eq!((start, end, angle_deg, h_align, v_align, text.as_str()), (SPoint::new(-6.35, -17.78), SPoint::new(1.27, -10.16), 90.0, FieldHAlign::Left, FieldVAlign::Top, "10V"));
        // mirrored (unturned): the corners swap sides and the justification stays the library's
        let mirrored = plotted_text_box(&tb, false, |p| SPoint::new(-p.x, p.y));
        let LibrarySymbolGraphic::TextBox { start, angle_deg, h_align, .. } = mirrored else { panic!("a text box") };
        assert_eq!((start, angle_deg, h_align), (SPoint::new(17.78, 6.35), 0.0, FieldHAlign::Left));
        // a vertical box turned a quarter is horizontal
        let LibrarySymbolGraphic::TextBox { angle_deg, .. } = plotted_text_box(&plotted_text_box(&tb, true, |p| p), true, |p| p) else { panic!("a text box") };
        assert_eq!(angle_deg, 0.0);
    }
}
