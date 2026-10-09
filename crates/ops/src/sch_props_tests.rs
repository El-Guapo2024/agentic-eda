//! Tests for the Properties verbs (`SchCmd::EditLabel`, `EditText`, `EditSheet`, `SetStroke`): what each dialog's OK changes, what it refuses, and that a
//! refused edit changes nothing.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_move::SchMoveCmd;
use eda_model::ir::{BusEntry, Junction, LabelShape, NetLabel, Provenance, SchLine, SchematicText, SheetInstance, Wire};
use eda_model::sch_extras::{JunctionLook, LabelSpin, SchColor, SchLineStyle, SchStroke};
use std::collections::BTreeMap;

fn design(sch: SchematicSection) -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn p(x: Um, y: Um) -> Point {
    Point { x, y }
}

fn red() -> SchColor {
    SchColor { r: 255, g: 0, b: 0, a: 255 }
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn board(sch: SchematicSection) -> (ConstraintModel, Design) {
    (ConstraintModel::default(), design(sch))
}

fn err(b: &mut Board, cmd: SchCmd) -> String {
    b.apply(&Cmd::SchEdit(cmd)).unwrap_err()[0].check.clone()
}

fn ok(b: &mut Board, cmd: SchCmd) {
    b.apply(&Cmd::SchEdit(cmd)).unwrap_or_else(|e| panic!("{e:?}"));
}

fn sch_of<'b>(b: &'b Board) -> &'b SchematicSection {
    b.design().schematic.as_ref().unwrap()
}

fn labels() -> SchematicSection {
    let mut s = SchematicSection::default();
    s.labels.push(NetLabel { id: "lbl_l".into(), net: "A".into(), at: p(0, 0), kind: LabelKind::Local });
    s.labels.push(NetLabel { id: "lbl_g".into(), net: "B".into(), at: p(5_080, 0), kind: LabelKind::Global { shape: LabelShape::Input } });
    s.labels.push(NetLabel { id: "lbl_h".into(), net: "C".into(), at: p(10_160, 0), kind: LabelKind::Hierarchical { shape: LabelShape::Output } });
    s
}

fn label(id: &str, text: Option<&str>, shape: Option<LabelShape>, spin: Option<LabelSpin>) -> SchCmd {
    SchCmd::EditLabel { id: id.into(), text: text.map(str::to_string), shape, spin, size_um: None, bold: None, italic: None }
}

#[test]
fn label_properties_change_the_text_the_shape_and_the_spin() {
    let (m, d) = board(labels());
    let mut b = Board::new(d, &m, 100, 300);
    ok(&mut b, label("lbl_l", Some("A2"), None, None));
    assert_eq!(sch_of(&b).labels[0].net, "A2");
    ok(&mut b, label("lbl_g", None, Some(LabelShape::Bidirectional), None));
    assert_eq!(sch_of(&b).labels[1].kind, LabelKind::Global { shape: LabelShape::Bidirectional });
    ok(&mut b, label("lbl_h", Some("C2"), Some(LabelShape::Passive), Some(LabelSpin::Up)));
    let h = &sch_of(&b).labels[2];
    assert_eq!((h.net.as_str(), &h.kind), ("C2", &LabelKind::Hierarchical { shape: LabelShape::Passive }));
    assert_eq!(sch_of(&b).extras.label_spins.get("lbl_h"), Some(&LabelSpin::Up));
    // the label keeps its id and its place
    assert_eq!((h.id.as_str(), h.at), ("lbl_h", p(10_160, 0)));
}

#[test]
fn label_properties_refuse_what_the_dialog_would_and_change_nothing() {
    let (m, d) = board(labels());
    let mut b = Board::new(d, &m, 100, 300);
    let before = format!("{:?}", sch_of(&b).labels);
    // "Label can not be empty."
    assert_eq!(err(&mut b, label("lbl_l", Some("  "), None, None)), "ops_bad_label");
    // a plain label has no shape; the text sent with it is not applied either
    assert_eq!(err(&mut b, label("lbl_l", Some("X"), Some(LabelShape::Input), None)), "ops_bad_label_shape");
    assert_eq!(err(&mut b, label("nope", Some("X"), None, None)), "ops_unknown_item");
    // the same values again, and nothing at all, are no change
    assert_eq!(err(&mut b, label("lbl_l", Some("A"), None, None)), "ops_label_unchanged");
    assert_eq!(err(&mut b, label("lbl_g", None, Some(LabelShape::Input), None)), "ops_label_unchanged");
    assert_eq!(err(&mut b, label("lbl_l", None, None, None)), "ops_label_unchanged");
    assert_eq!(format!("{:?}", sch_of(&b).labels), before);
    ok(&mut b, label("lbl_l", None, None, Some(LabelSpin::Left)));
    assert_eq!(err(&mut b, label("lbl_l", None, None, Some(LabelSpin::Left))), "ops_label_unchanged");
    assert!(!sch_of(&b).extras.is_empty());
}

fn text_sheet() -> SchematicSection {
    let mut s = SchematicSection::default();
    s.texts.push(SchematicText { id: "txt_a".into(), content: "hello".into(), at: p(0, 0), angle: 0, size_um: 1_270 });
    s
}

fn text(id: &str, t: Option<&str>, size: Option<Um>, angle: Option<Millideg>) -> SchCmd {
    SchCmd::EditText { id: id.into(), text: t.map(str::to_string), size_um: size, angle }
}

#[test]
fn text_properties_change_the_text_the_size_and_the_angle() {
    let (m, d) = board(text_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    ok(&mut b, text("txt_a", Some("two\nlines"), None, None));
    ok(&mut b, text("txt_a", None, Some(2_540), Some(90_000)));
    let t = &sch_of(&b).texts[0];
    assert_eq!((t.content.as_str(), t.size_um, t.angle, t.at), ("two\nlines", 2_540, 90_000, p(0, 0)));
    // 0.01 mm and 1000 mm are the dialog's limits
    ok(&mut b, text("txt_a", None, Some(10), None));
    ok(&mut b, text("txt_a", None, Some(1_000_000), None));
}

#[test]
fn text_properties_refuse_what_the_dialog_would() {
    let (m, d) = board(text_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    assert_eq!(err(&mut b, text("txt_a", Some(""), None, None)), "ops_bad_text");
    assert_eq!(err(&mut b, text("txt_a", None, Some(9), None)), "ops_bad_text_size");
    assert_eq!(err(&mut b, text("txt_a", None, Some(1_000_001), None)), "ops_bad_text_size");
    assert_eq!(err(&mut b, text("txt_a", None, None, Some(360_000))), "ops_bad_text_angle");
    assert_eq!(err(&mut b, text("nope", Some("x"), None, None)), "ops_unknown_item");
    assert_eq!(err(&mut b, text("txt_a", Some("hello"), Some(1_270), Some(0))), "ops_text_unchanged");
    assert_eq!(sch_of(&b).texts[0].content, "hello");
}

fn sheet(id: &str, name: &str, file: &str) -> SheetInstance {
    SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: p(0, 0), size: (25_400, 12_700), pins: vec![], page: String::new() }
}

/// A root with sheets A (a.kicad_sch) and B (b.kicad_sch), each with a screen that holds a marker text.
fn hierarchy() -> Design {
    let mut root = SchematicSection::default();
    root.sheets.push(sheet("sheet_a", "A", "a.kicad_sch"));
    root.sheets.push(sheet("sheet_b", "B", "b.kicad_sch"));
    let mut d = design(root);
    let screen = |marker: &str| {
        let mut s = SchematicSection::default();
        s.texts.push(SchematicText { id: format!("txt_{marker}"), content: marker.into(), at: p(0, 0), angle: 0, size_um: 1_270 });
        s
    };
    let mut contents = BTreeMap::new();
    contents.insert("a.kicad_sch".to_string(), screen("a"));
    contents.insert("b.kicad_sch".to_string(), screen("b"));
    d.sheet_contents = Some(contents);
    d
}

fn edit_sheet(id: &str, name: Option<&str>, file: Option<&str>) -> SchCmd {
    SchCmd::EditSheet { id: id.into(), name: name.map(str::to_string), file: file.map(str::to_string) }
}

fn files(b: &Board) -> Vec<String> {
    b.design().sheet_contents.as_ref().unwrap().keys().cloned().collect()
}

fn marker(b: &Board, file: &str) -> String {
    b.design().sheet_contents.as_ref().unwrap()[file].texts[0].content.clone()
}

#[test]
fn sheet_properties_rename_the_sheet() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, 100, 300);
    ok(&mut b, edit_sheet("sheet_a", Some(" Power "), None));
    assert_eq!(sch_of(&b).sheets[0].name, "Power", "the name is trimmed");
    assert_eq!(sch_of(&b).sheets[0].id, "sheet_a", "a rename keeps the id");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", Some("B"), None)), "ops_sheet_name_taken");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", Some("   "), None)), "ops_bad_sheet");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", Some("Power"), None)), "ops_sheet_unchanged");
    assert_eq!(err(&mut b, edit_sheet("nope", Some("x"), None)), "ops_unknown_sheet");
    assert_eq!(files(&b), vec!["a.kicad_sch", "b.kicad_sch"]);
}

#[test]
fn a_sheet_that_is_the_only_user_of_its_file_takes_the_content_along_to_a_new_name() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, 100, 300);
    ok(&mut b, edit_sheet("sheet_a", None, Some("power")));
    assert_eq!(sch_of(&b).sheets[0].file, "power.kicad_sch", "the extension is added");
    assert_eq!(files(&b), vec!["b.kicad_sch", "power.kicad_sch"]);
    assert_eq!(marker(&b, "power.kicad_sch"), "a");
}

#[test]
fn a_sheet_that_shares_its_file_copies_the_content_to_a_new_name() {
    let m = ConstraintModel::default();
    let mut d = hierarchy();
    d.schematic.as_mut().unwrap().sheets.push(sheet("sheet_c", "C", "a.kicad_sch"));
    let mut b = Board::new(d, &m, 100, 300);
    ok(&mut b, edit_sheet("sheet_a", None, Some("copy.kicad_sch")));
    assert_eq!(files(&b), vec!["a.kicad_sch", "b.kicad_sch", "copy.kicad_sch"], "C still shows a.kicad_sch");
    assert_eq!(marker(&b, "copy.kicad_sch"), "a");
}

#[test]
fn a_sheet_linked_to_a_file_the_project_has_shows_that_file_and_nothing_is_copied() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, 100, 300);
    ok(&mut b, edit_sheet("sheet_a", Some("A again"), Some("b.kicad_sch")));
    assert_eq!((sch_of(&b).sheets[0].name.as_str(), sch_of(&b).sheets[0].file.as_str()), ("A again", "b.kicad_sch"));
    assert_eq!(files(&b), vec!["a.kicad_sch", "b.kicad_sch"]);
    assert_eq!(marker(&b, "b.kicad_sch"), "b");
}

#[test]
fn a_sheet_file_is_a_bare_name_and_a_sheet_cannot_show_a_file_that_shows_it() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, 100, 300);
    assert_eq!(err(&mut b, edit_sheet("sheet_a", None, Some("sub/x.kicad_sch"))), "ops_bad_sheet");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", None, Some(" "))), "ops_bad_sheet");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", None, Some("a"))), "ops_sheet_unchanged", "a.kicad_sch is what it already shows");

    // a.kicad_sch places a sheet T; on that screen T may not be linked to a.kicad_sch itself
    let mut d = hierarchy();
    d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().sheets.push(sheet("sheet_t", "T", "t.kicad_sch"));
    d.sheet_contents.as_mut().unwrap().insert("t.kicad_sch".into(), SchematicSection::default());
    let mut b = Board::new(d.clone(), &m, 100, 300);
    let on_a = |cmd: SchCmd| Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchEdit(cmd)) };
    assert_eq!(b.apply(&on_a(edit_sheet("sheet_t", None, Some("a.kicad_sch")))).unwrap_err()[0].check, "ops_sheet_recursion");
    // nor to a file that shows a.kicad_sch: b.kicad_sch holding a sheet U that shows it would close a loop
    d.sheet_contents.as_mut().unwrap().get_mut("b.kicad_sch").unwrap().sheets.push(sheet("sheet_u", "U", "a.kicad_sch"));
    let mut b = Board::new(d, &m, 100, 300);
    assert_eq!(b.apply(&on_a(edit_sheet("sheet_t", None, Some("b.kicad_sch")))).unwrap_err()[0].check, "ops_sheet_recursion", "b shows a, so a cannot show b");
    // a plain rename on that screen is fine
    ok_on(&mut b, "sheet_a", edit_sheet("sheet_t", Some("T2"), None));
    let t = &b.design().sheet_contents.as_ref().unwrap()["a.kicad_sch"].sheets[0];
    assert_eq!(t.name, "T2");
}

fn ok_on(b: &mut Board, sheet: &str, cmd: SchCmd) {
    b.apply(&Cmd::OnSheet { sheet: sheet.into(), cmd: Box::new(Cmd::SchEdit(cmd)) }).unwrap_or_else(|e| panic!("{e:?}"));
}

fn stroke_sheet() -> SchematicSection {
    let mut s = SchematicSection::default();
    s.wires.push(Wire { id: "wire_a".into(), net: "N".into(), pins: vec![], pts: vec![p(0, 0), p(10_160, 0)], bus: false });
    s.wires.push(Wire { id: "wire_bus".into(), net: "D[0..3]".into(), pins: vec![], pts: vec![p(0, 5_080), p(10_160, 5_080)], bus: true });
    s.bus_entries.push(BusEntry { id: "bent_a".into(), at: p(2_540, 5_080), size: p(2_540, 2_540) });
    s.lines.push(SchLine { id: "sln_a".into(), pts: vec![p(0, 12_700), p(10_160, 12_700)], width_um: 0 });
    s.junctions.push(Junction { id: "jct_a".into(), at: p(5_080, 0) });
    s.labels.push(NetLabel { id: "lbl_a".into(), net: "N".into(), at: p(0, 0), kind: LabelKind::Local });
    s
}

fn stroke(ids_: &[&str], width: Option<Um>, style: Option<SchLineStyle>, color: Option<SchColor>, diameter: Option<Um>) -> SchCmd {
    SchCmd::SetStroke { ids: ids(ids_), width_um: width, style, color, diameter_um: diameter }
}

#[test]
fn wire_bus_and_entry_strokes_are_set_and_put_back_to_the_default() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    ok(&mut b, stroke(&["wire_a", "wire_bus", "bent_a"], Some(300), Some(SchLineStyle::Dash), Some(red()), None));
    let strokes = &sch_of(&b).extras.strokes;
    let want = SchStroke { width_um: 300, style: SchLineStyle::Dash, color: Some(red()) };
    assert_eq!((strokes.get("wire_a"), strokes.get("wire_bus"), strokes.get("bent_a")), (Some(&want), Some(&want), Some(&want)));
    // one field at a time: the others stay
    ok(&mut b, stroke(&["wire_a"], Some(0), None, None, None));
    assert_eq!(sch_of(&b).extras.strokes["wire_a"], SchStroke { width_um: 0, style: SchLineStyle::Dash, color: Some(red()) });
    // a colour of all zeroes is "unspecified": the layer's own colour
    ok(&mut b, stroke(&["wire_a"], None, Some(SchLineStyle::Default), Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), None));
    assert!(!sch_of(&b).extras.strokes.contains_key("wire_a"), "a stroke equal to the default is not stored");
    // the width cannot go negative (`std::max( 0, width )`)
    ok(&mut b, stroke(&["wire_a"], Some(-5), Some(SchLineStyle::Dot), None, None));
    assert_eq!(sch_of(&b).extras.strokes["wire_a"], SchStroke { width_um: 0, style: SchLineStyle::Dot, color: None });
}

#[test]
fn a_graphic_line_keeps_its_width_where_it_always_was() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    ok(&mut b, stroke(&["sln_a"], Some(250), Some(SchLineStyle::DashDot), Some(red()), None));
    assert_eq!(sch_of(&b).lines[0].width_um, 250);
    assert_eq!(sch_of(&b).extras.strokes["sln_a"], SchStroke { width_um: 0, style: SchLineStyle::DashDot, color: Some(red()) });
    ok(&mut b, stroke(&["sln_a"], Some(0), Some(SchLineStyle::Default), Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), None));
    assert_eq!(sch_of(&b).lines[0].width_um, 0);
    assert!(sch_of(&b).extras.strokes.is_empty());
}

#[test]
fn a_junction_takes_a_diameter_and_a_colour_and_ignores_the_line_fields() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    // as in the mixed selection of DIALOG_WIRE_BUS_PROPERTIES: the width and style are for the lines, the diameter for the junction
    ok(&mut b, stroke(&["wire_a", "jct_a"], Some(300), Some(SchLineStyle::Dash), Some(red()), Some(1_000)));
    assert_eq!(sch_of(&b).extras.junction_looks["jct_a"], JunctionLook { diameter_um: 1_000, color: Some(red()) });
    assert_eq!(sch_of(&b).extras.strokes["wire_a"], SchStroke { width_um: 300, style: SchLineStyle::Dash, color: Some(red()) });
    assert!(!sch_of(&b).extras.strokes.contains_key("jct_a"));
    ok(&mut b, stroke(&["jct_a"], None, None, Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), Some(0)));
    assert!(sch_of(&b).extras.junction_looks.is_empty());
}

#[test]
fn stroke_edits_refuse_unknown_items_and_changes_that_change_nothing() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    assert_eq!(err(&mut b, stroke(&[], Some(1), None, None, None)), "ops_nothing_selected");
    // a label has no stroke; the wire named with it is left alone as well
    assert_eq!(err(&mut b, stroke(&["wire_a", "lbl_a"], Some(300), None, None, None)), "ops_unknown_item");
    assert!(sch_of(&b).extras.is_empty());
    assert_eq!(err(&mut b, stroke(&["wire_a"], Some(0), Some(SchLineStyle::Default), None, None)), "ops_stroke_unchanged");
    assert_eq!(err(&mut b, stroke(&["jct_a"], Some(300), None, None, None)), "ops_stroke_unchanged", "a width means nothing to a junction");
    assert!(sch_of(&b).extras.is_empty());
}

#[test]
fn the_stroke_of_a_deleted_wire_goes_at_the_next_stroke_edit() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    ok(&mut b, stroke(&["wire_a", "wire_bus"], Some(300), None, None, None));
    b.apply(&Cmd::DeleteWire { id: "wire_bus".into() }).unwrap();
    ok(&mut b, stroke(&["wire_a"], Some(400), None, None, None));
    assert_eq!(sch_of(&b).extras.strokes.keys().cloned().collect::<Vec<_>>(), vec!["wire_a".to_string()]);
}

#[test]
fn a_stroke_stays_with_its_wire_when_the_wire_is_moved_or_dragged() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, 100, 300);
    ok(&mut b, stroke(&["wire_a", "jct_a"], Some(300), None, None, Some(900)));
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: ids(&["wire_a", "jct_a"]), dx: 2_540, dy: 0, turns: Vec::new(), about: None })).unwrap();
    let w = sch_of(&b).wires.iter().find(|w| w.id == "wire_a").unwrap();
    assert_eq!(w.pts[0], p(2_540, 0), "the wire moved");
    assert_eq!(sch_of(&b).extras.strokes["wire_a"].width_um, 300, "and kept its stroke");
    assert_eq!(sch_of(&b).extras.junction_looks["jct_a"].diameter_um, 900);
    b.apply(&Cmd::SchMove(SchMoveCmd::Drag { ids: ids(&["wire_a"]), vertices: BTreeMap::new(), dx: 0, dy: 2_540, ortho: true, grid: 0, turns: Vec::new(), about: None })).unwrap();
    assert_eq!(sch_of(&b).extras.strokes["wire_a"].width_um, 300);
    assert!(sch_of(&b).wires.iter().any(|w| w.id == "wire_a"));
}

#[test]
fn the_new_verbs_read_as_the_studio_sends_them() {
    let parse = |json: &str| -> Cmd { serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}")) };
    assert_eq!(parse(r#"{"op":"sch_edit","verb":"edit_label","id":"lbl_a","text":"X","shape":"tri_state","spin":"bottom"}"#), Cmd::SchEdit(label("lbl_a", Some("X"), Some(LabelShape::TriState), Some(LabelSpin::Bottom))));
    assert_eq!(parse(r#"{"op":"sch_edit","verb":"edit_text","id":"txt_a","size_um":2540,"angle":90000}"#), Cmd::SchEdit(text("txt_a", None, Some(2_540), Some(90_000))));
    assert_eq!(parse(r#"{"op":"sch_edit","verb":"edit_sheet","id":"sheet_a","name":"P","file":"p.kicad_sch"}"#), Cmd::SchEdit(edit_sheet("sheet_a", Some("P"), Some("p.kicad_sch"))));
    assert_eq!(
        parse(r#"{"op":"sch_edit","verb":"set_stroke","ids":["wire_a"],"width_um":300,"style":"dash_dot_dot","color":{"r":255,"g":0,"b":0,"a":255},"diameter_um":900}"#),
        Cmd::SchEdit(stroke(&["wire_a"], Some(300), Some(SchLineStyle::DashDotDot), Some(red()), Some(900)))
    );
    // a field left out is not a change
    assert_eq!(parse(r#"{"op":"sch_edit","verb":"set_stroke","ids":["wire_a"]}"#), Cmd::SchEdit(stroke(&["wire_a"], None, None, None, None)));
}

#[test]
fn only_a_label_and_a_sheet_edit_can_change_what_is_connected() {
    let is = |c: SchCmd| Cmd::SchEdit(c).edits_connectivity();
    assert!(is(label("lbl_a", Some("X"), None, None)), "a label's text is the name of its net");
    assert!(is(edit_sheet("sheet_a", Some("X"), None)));
    assert!(!is(text("txt_a", Some("x"), None, None)));
    assert!(!is(stroke(&["wire_a"], Some(1), None, None, None)));
    assert!(Cmd::OnSheet { sheet: "s".into(), cmd: Box::new(Cmd::SchEdit(stroke(&["wire_a"], Some(1), None, None, None))) }.edits_connectivity() == false);
}
