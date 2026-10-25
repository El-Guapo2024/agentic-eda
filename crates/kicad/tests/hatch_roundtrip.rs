//! The hatch of a board shape through the importer and the writer (GAPS.md item 24, P16i step 3b): `(fill hatch)`, `(fill reverse_hatch)` and `(fill cross_hatch)` of a
//! rectangle, a circle and a polygon are read into the board's `shape_styles` and written back as the same tokens, and come back unchanged; a solid or an empty fill is not a
//! hatch; the clipboard has the hatch; the shape's own id names its style so it follows the shape through an edit.

use eda_model::units::mm;
use eda_model::units::Nm;
use eda_kicad::{export_kicad_pcb, export_pcb_clipboard, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, Point, Shape};
use eda_model::pcb_items::HatchKind;
use eda_model::ConstraintModel;

const HEADER: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (37 "F.SilkS" user) (35 "F.Fab" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "")
"#;

fn board(items: &str) -> String {
    format!("{HEADER}{items})\n")
}

const ITEMS: &str = r#"	(gr_rect (start 0 0) (end 10 10) (stroke (width 0.2) (type default)) (fill hatch) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000001"))
	(gr_circle (center 30 5) (end 35 5) (stroke (width 0.2) (type default)) (fill reverse_hatch) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000002"))
	(gr_poly (pts (xy 50 0) (xy 60 0) (xy 55 10)) (stroke (width 0.2) (type default)) (fill cross_hatch) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000003"))
	(gr_rect (start 0 20) (end 10 30) (stroke (width 0.2) (type default)) (fill yes) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000004"))
	(gr_rect (start 20 20) (end 30 30) (stroke (width 0.2) (type default)) (fill no) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000005"))
"#;

fn export(d: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(d, model, &ExportMeta { date: "2026-01-01", title: "hatch_rt" }).expect("export_kicad_pcb")
}

fn hatch_of_shape_at(d: &Design, x: Nm, y: Nm) -> Option<HatchKind> {
    let dr = d.drawings.as_ref()?;
    let shape = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x, y }))?;
    dr.board_items.hatch_of(shape.id())
}

#[test]
fn the_three_hatches_are_read_and_a_solid_or_empty_fill_is_not_one() {
    let (design, _, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    let dr = design.drawings.as_ref().expect("drawings");
    assert_eq!(dr.shapes.len(), 5);
    assert_eq!(dr.board_items.shape_styles.len(), 3, "{:?}", dr.board_items.shape_styles);
    assert_eq!(hatch_of_shape_at(&design, Nm::ZERO, Nm::ZERO), Some(HatchKind::Hatch));
    assert_eq!(hatch_of_shape_at(&design, mm(30.0), mm(5.0)), Some(HatchKind::ReverseHatch));
    assert_eq!(hatch_of_shape_at(&design, mm(50.0), Nm::ZERO), Some(HatchKind::CrossHatch));
    assert_eq!(hatch_of_shape_at(&design, Nm::ZERO, mm(20.0)), None);
    assert_eq!(hatch_of_shape_at(&design, mm(20.0), mm(20.0)), None);
    // A hatched shape is not "filled": the fill the IR has is the solid one.
    let hatched = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x: Nm::ZERO, y: Nm::ZERO })).unwrap();
    assert!(!hatched.is_filled());
    let solid = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x: Nm::ZERO, y: mm(20.0) })).unwrap();
    assert!(solid.is_filled());
    // The styles are named by the ids the shapes have.
    for st in &dr.board_items.shape_styles {
        assert!(dr.shapes.iter().any(|s| s.id() == st.id), "{} names no shape", st.id);
    }
}

#[test]
fn the_writer_has_the_token_of_each_hatch_and_it_reads_back_the_same() {
    let (design, model, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    let text = export(&design, &model);
    assert_eq!(text.matches("(fill hatch)").count(), 1, "{text}");
    assert_eq!(text.matches("(fill reverse_hatch)").count(), 1);
    assert_eq!(text.matches("(fill cross_hatch)").count(), 1);
    assert_eq!(text.matches("(fill yes)").count(), 1);
    assert_eq!(text.matches("(fill no)").count(), 1);
    let (again, _, _) = import_kicad_pcb(&text).expect("re-imports");
    // The file lists the shapes by id, so they come back in that order; the same ones, with the same ids.
    let sorted = |d: &Design| {
        let dr = d.drawings.as_ref().unwrap();
        let mut shapes = dr.shapes.clone();
        shapes.sort_by(|a, b| a.id().cmp(b.id()));
        let mut styles = dr.board_items.shape_styles.clone();
        styles.sort_by(|a, b| a.id.cmp(&b.id));
        (shapes, styles)
    };
    assert_eq!(sorted(&again), sorted(&design));
    // And a second time over, the file is the same text.
    let (model2, design2) = (import_kicad_pcb(&text).unwrap().1, again);
    assert_eq!(export(&design2, &model2), text);
}

#[test]
fn a_hatch_the_design_has_is_written_even_when_it_was_set_by_a_verb_and_not_read() {
    let (mut design, model, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    let dr = design.drawings.as_mut().unwrap();
    let plain = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x: mm(20.0), y: mm(20.0) })).unwrap().id().to_string();
    dr.board_items.set_hatch(&plain, Some(HatchKind::ReverseHatch));
    let text = export(&design, &model);
    assert_eq!(text.matches("(fill reverse_hatch)").count(), 2);
    assert_eq!(text.matches("(fill no)").count(), 0);
    // A solid fill wins if both are somehow there.
    let solid = {
        let dr = design.drawings.as_mut().unwrap();
        let id = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x: Nm::ZERO, y: mm(20.0) })).unwrap().id().to_string();
        dr.board_items.set_hatch(&id, Some(HatchKind::Hatch));
        id
    };
    let text = export(&design, &model);
    assert!(text.contains("(fill yes)"), "the solid fill of {solid} stays: {text}");
}

#[test]
fn the_clipboard_has_the_hatch_of_a_copied_shape() {
    let (design, model, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    let dr = design.drawings.as_ref().unwrap();
    let ids: Vec<String> = dr.shapes.iter().filter(|s| dr.board_items.hatch_of(s.id()).is_some()).map(|s| s.id().to_string()).collect();
    assert_eq!(ids.len(), 3);
    let text = export_pcb_clipboard(&design, &model, &ids, Point { x: Nm::ZERO, y: Nm::ZERO }).expect("clipboard");
    assert_eq!(text.matches("(fill hatch)").count(), 1, "{text}");
    assert_eq!(text.matches("(fill reverse_hatch)").count(), 1);
    assert_eq!(text.matches("(fill cross_hatch)").count(), 1);
}

#[test]
fn the_style_of_a_shape_that_is_gone_is_dropped_before_another_takes_its_id() {
    let (mut design, _, _) = import_kicad_pcb(&board(ITEMS)).expect("imports");
    let dr = design.drawings.as_mut().unwrap();
    let gone = dr.shapes.iter().find(|s| s.points().first() == Some(&Point { x: Nm::ZERO, y: Nm::ZERO })).unwrap().id().to_string();
    dr.shapes.retain(|s| s.id() != gone);
    assert!(dr.board_items.hatch_of(&gone).is_some(), "still there until ids are assigned");
    // The same shape added again gets the same id (the id is a hash of the geometry) -- and must not come back hatched.
    dr.shapes.push(Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.2), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(10.0), y: mm(10.0) } });
    dr.assign_missing_ids();
    assert_eq!(dr.board_items.hatch_of(&gone), None);
    assert_eq!(dr.board_items.shape_styles.len(), 2);
}
