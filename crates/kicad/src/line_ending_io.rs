//! KiCad 10's line endings of a graphic shape (`(start_shape ..)`, `(end_shape ..)`, file version 20260818) read and written -- see [`eda_model::line_ending`].
//!
//! Ported from `PCB_IO_KICAD_SEXPR_PARSER::parseLineEnding` and `LINE_ENDING::Format` (`common/line_ending.cpp`) of KiCad 25ec2335.

use eda_model::units::Nm;
use eda_model::line_ending::{LineEnding, LineEndingStyle, LineEndings};
use eda_model::pcb_items::LineStyle;

use crate::mm_to_nm;
use crate::sexpr::{self, Sexpr};

/// One `(start_shape <style> [(length ..)] [(width ..)] [(stroke (width ..) (type ..))])`: `parseLineEnding`. A style the parser does not know is no ending (KiCad would
/// refuse the file: "Expecting arrow, circle, square, arrow_open, or none").
pub(crate) fn read_one(form: &[Sexpr]) -> LineEnding {
    let Some(style) = sexpr::txt(form, 1).and_then(LineEndingStyle::from_token) else { return LineEnding::default() };
    let um = |name: &str| sexpr::find(form, name).and_then(|f| sexpr::num(f, 1)).map(mm_to_nm).unwrap_or(Nm::ZERO);
    let stroke = sexpr::find(form, "stroke");
    LineEnding {
        style,
        length_nm: um("length"),
        width_nm: um("width"),
        stroke_width_nm: stroke.and_then(|s| sexpr::find(s, "width")).and_then(|w| sexpr::num(w, 1)).map(mm_to_nm).unwrap_or(Nm::ZERO),
        stroke_style: stroke.and_then(|s| sexpr::find(s, "type")).and_then(|t| sexpr::txt(t, 1)).map(LineStyle::from_token).unwrap_or_default(),
    }
}

/// The endings of a shape item (`parsePCB_SHAPE`'s `T_start_shape` and `T_end_shape`). A closed shape keeps what it was given, as `EDA_SHAPE` does, though nothing draws
/// or writes them while it is closed (`GetStartEnding` hands back a blank one).
pub(crate) fn read(item: &[Sexpr]) -> LineEndings {
    LineEndings { start: sexpr::find(item, "start_shape").map(read_one).unwrap_or_default(), end: sexpr::find(item, "end_shape").map(read_one).unwrap_or_default() }
}

/// `aShape->GetStartEnding().Format( m_out, pcbIUScale, "start_shape" ); aShape->GetEndEnding().Format( .., "end_shape" )`, after the shape's stroke; nothing for a
/// closed shape (`EDA_SHAPE::GetStartEnding` returns the blank ending when `IsClosed()`), whose `closed` the caller says.
pub(crate) fn words(endings: &LineEndings, closed: bool) -> String {
    if closed {
        return String::new();
    }
    endings.format(crate::mm)
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    
    use super::*;

    #[test]
    fn an_ending_reads_as_parse_line_ending_reads_it() {
        let item = sexpr::parse(r#"(gr_line (start 0 0) (end 1 0) (stroke (width 0.6) (type default)) (start_shape arrow_open (length 3) (width 2.5) (stroke (width 0.6) (type dash))) (end_shape circle) (layer "F.SilkS"))"#).unwrap();
        let e = read(item.as_list().unwrap());
        assert_eq!(e.start, LineEnding { style: LineEndingStyle::ArrowOpen, length_nm: mm(3.0), width_nm: mm(2.5), stroke_width_nm: mm(0.6), stroke_style: LineStyle::Dash });
        assert_eq!(e.end, LineEnding::new(LineEndingStyle::Circle));
        assert_eq!(words(&e, false), " (start_shape arrow_open (length 3) (width 2.5) (stroke (width 0.6) (type dash))) (end_shape circle)");
        assert_eq!(words(&e, true), "", "a closed shape writes none");
    }

    #[test]
    fn a_board_keeps_the_endings_and_the_net_of_its_shapes_through_a_round_trip() {
        let board = r#"(kicad_pcb (version 20260818) (generator "pcbnew") (general (thickness 1.6)) (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (5 "F.SilkS" user "F.Silkscreen"))
  (gr_line (start 0 0) (end 10 0) (stroke (width 0.6) (type default)) (start_shape arrow (length 3) (width 3)) (end_shape arrow_open (length 3) (width 3) (stroke (width 0.6) (type default))) (layer "F.Cu") (net "SIG") (uuid "11111111-1111-4111-8111-111111111111"))
  (gr_arc (start 0 5) (mid 5 10) (end 10 5) (stroke (width 0.2) (type solid)) (end_shape square) (layer "F.SilkS") (uuid "22222222-2222-4222-8222-222222222222"))
  (gr_rect (start 20 20) (end 30 30) (stroke (width 0.2) (type solid)) (fill no) (start_shape arrow) (layer "F.SilkS") (uuid "33333333-3333-4333-8333-333333333333"))
)"#;
        let (design, model, _) = crate::import::import_kicad_pcb(board).unwrap();
        let dr = design.drawings.as_ref().unwrap();
        let line = dr.shapes.iter().find(|s| matches!(s, eda_model::ir::Shape::Segment { .. })).unwrap();
        assert_eq!(dr.board_items.endings_of(line.id()).start, LineEnding { style: LineEndingStyle::Arrow, length_nm: mm(3.0), width_nm: mm(3.0), ..Default::default() });
        assert_eq!(dr.board_items.net_of(line.id()), Some("SIG"));
        let out = crate::export_kicad_pcb(&design, &model, &crate::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        let written = out.lines().find(|l| l.contains("(gr_line (start 0 0)")).unwrap();
        assert!(written.contains("(stroke (width 0.6) (type solid)) (start_shape arrow (length 3) (width 3)) (end_shape arrow_open (length 3) (width 3) (stroke (width 0.6) (type default)))"), "{written}");
        assert!(written.contains("(layer \"F.Cu\") (net "), "{written}");
        assert!(out.lines().any(|l| l.contains("(gr_arc") && l.contains("(end_shape square)")), "{out}");
        // A closed shape draws and writes no ending (`GetStartEnding` of a closed shape is the blank one).
        assert!(!out.lines().any(|l| l.contains("(gr_rect") && l.contains("start_shape")), "{out}");
    }
}
