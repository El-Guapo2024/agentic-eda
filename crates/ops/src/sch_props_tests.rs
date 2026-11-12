//! Tests for the Properties verbs (`SchCmd::EditLabel`, `EditText`, `EditSheet`, `SetStroke`): what each dialog's OK changes, what it refuses, and that a
//! refused edit changes nothing.

use super::*;
use crate::sch_edit::SchCmd;
use crate::sch_move::SchMoveCmd;
use eda_model::ir::{BusEntry, Junction, LabelShape, NetLabel, Provenance, SchLine, SchematicText, SheetInstance, Wire};
use eda_model::sch_extras::{JunctionLook, LabelSpin, SchColor, SchLineStyle, SchStroke};
use std::collections::BTreeMap;
use eda_model::units::{mm, um};

fn design(sch: SchematicSection) -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: Some(sch),
        nets: None,
        routing: None,
        placement: None,
        drawings: None,
    }
}

fn p(x: Nm, y: Nm) -> Point {
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
    s.labels.push(NetLabel { id: "lbl_l".into(), net: "A".into(), at: p(Nm::ZERO, Nm::ZERO), kind: LabelKind::Local });
    s.labels.push(NetLabel { id: "lbl_g".into(), net: "B".into(), at: p(mm(5.08), Nm::ZERO), kind: LabelKind::Global { shape: LabelShape::Input } });
    s.labels.push(NetLabel { id: "lbl_h".into(), net: "C".into(), at: p(mm(10.16), Nm::ZERO), kind: LabelKind::Hierarchical { shape: LabelShape::Output } });
    s
}

fn label(id: &str, text: Option<&str>, shape: Option<LabelShape>, spin: Option<LabelSpin>) -> SchCmd {
    SchCmd::EditLabel { id: id.into(), text: text.map(str::to_string), shape, spin, size_nm: None, bold: None, italic: None, auto_rotate: None, fields: None }
}

#[test]
fn label_properties_change_the_text_the_shape_and_the_spin() {
    let (m, d) = board(labels());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, label("lbl_l", Some("A2"), None, None));
    assert_eq!(sch_of(&b).labels[0].net, "A2");
    ok(&mut b, label("lbl_g", None, Some(LabelShape::Bidirectional), None));
    assert_eq!(sch_of(&b).labels[1].kind, LabelKind::Global { shape: LabelShape::Bidirectional });
    ok(&mut b, label("lbl_h", Some("C2"), Some(LabelShape::Passive), Some(LabelSpin::Up)));
    let h = &sch_of(&b).labels[2];
    assert_eq!((h.net.as_str(), &h.kind), ("C2", &LabelKind::Hierarchical { shape: LabelShape::Passive }));
    assert_eq!(sch_of(&b).extras.label_spins.get("lbl_h"), Some(&LabelSpin::Up));
    // the label keeps its id and its place
    assert_eq!((h.id.as_str(), h.at), ("lbl_h", p(mm(10.16), Nm::ZERO)));
}

#[test]
fn label_properties_refuse_what_the_dialog_would_and_change_nothing() {
    let (m, d) = board(labels());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
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
    s.texts.push(SchematicText { id: "txt_a".into(), content: "hello".into(), at: p(Nm::ZERO, Nm::ZERO), angle: 0, size_nm: mm(1.27) });
    s
}

fn text(id: &str, t: Option<&str>, size: Option<Nm>, angle: Option<Millideg>) -> SchCmd {
    SchCmd::EditText { id: id.into(), text: t.map(str::to_string), size_nm: size, angle, h: None, v: None }
}

#[test]
fn text_properties_change_the_text_the_size_and_the_angle() {
    let (m, d) = board(text_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, text("txt_a", Some("two\nlines"), None, None));
    ok(&mut b, text("txt_a", None, Some(mm(2.54)), Some(90_000)));
    let t = &sch_of(&b).texts[0];
    assert_eq!((t.content.as_str(), t.size_nm, t.angle, t.at), ("two\nlines", mm(2.54), 90_000, p(Nm::ZERO, Nm::ZERO)));
    // 0.01 mm and 1000 mm are the dialog's limits
    ok(&mut b, text("txt_a", None, Some(mm(0.01)), None));
    ok(&mut b, text("txt_a", None, Some(mm(1.0)), None));
}

#[test]
fn text_properties_refuse_what_the_dialog_would() {
    let (m, d) = board(text_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    assert_eq!(err(&mut b, text("txt_a", Some(""), None, None)), "ops_bad_text");
    assert_eq!(err(&mut b, text("txt_a", None, Some(um(9.0)), None)), "ops_bad_text_size");
    assert_eq!(err(&mut b, text("txt_a", None, Some(mm(1000.001)), None)), "ops_bad_text_size");
    assert_eq!(err(&mut b, text("txt_a", None, None, Some(360_000))), "ops_bad_text_angle");
    assert_eq!(err(&mut b, text("nope", Some("x"), None, None)), "ops_unknown_item");
    assert_eq!(err(&mut b, text("txt_a", Some("hello"), Some(mm(1.27)), Some(0))), "ops_text_unchanged");
    assert_eq!(sch_of(&b).texts[0].content, "hello");
}

fn sheet(id: &str, name: &str, file: &str) -> SheetInstance {
    SheetInstance { id: id.into(), name: name.into(), file: file.into(), at: p(Nm::ZERO, Nm::ZERO), size: (mm(25.4), mm(12.7)), pins: vec![], page: String::new() }
}

/// A root with sheets A (a.kicad_sch) and B (b.kicad_sch), each with a screen that holds a marker text.
fn hierarchy() -> Design {
    let mut root = SchematicSection::default();
    root.sheets.push(sheet("sheet_a", "A", "a.kicad_sch"));
    root.sheets.push(sheet("sheet_b", "B", "b.kicad_sch"));
    let mut d = design(root);
    let screen = |marker: &str| {
        let mut s = SchematicSection::default();
        s.texts.push(SchematicText { id: format!("txt_{marker}"), content: marker.into(), at: p(Nm::ZERO, Nm::ZERO), angle: 0, size_nm: mm(1.27) });
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
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
    ok(&mut b, edit_sheet("sheet_a", Some(" Power "), None));
    assert_eq!(sch_of(&b).sheets[0].name, "Power", "the name is trimmed");
    assert_eq!(sch_of(&b).sheets[0].id, "sheet_a", "a rename keeps the id");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", Some("   "), None)), "ops_bad_sheet");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", Some("Power"), None)), "ops_sheet_unchanged");
    assert_eq!(err(&mut b, edit_sheet("nope", Some("x"), None)), "ops_unknown_sheet");
    assert_eq!(files(&b), vec!["a.kicad_sch", "b.kicad_sch"]);
    // UI-LEFT S21: the name another sheet has is allowed (`DIALOG_SHEET_PROPERTIES` has no such check); ERC's `duplicate_sheet_names` reports it
    ok(&mut b, edit_sheet("sheet_a", Some("B"), None));
    assert_eq!(sch_of(&b).sheets.iter().filter(|s| s.name == "B").count(), 2);
}

#[test]
fn a_sheet_that_is_the_only_user_of_its_file_takes_the_content_along_to_a_new_name() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
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
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, edit_sheet("sheet_a", None, Some("copy.kicad_sch")));
    assert_eq!(files(&b), vec!["a.kicad_sch", "b.kicad_sch", "copy.kicad_sch"], "C still shows a.kicad_sch");
    assert_eq!(marker(&b, "copy.kicad_sch"), "a");
}

#[test]
fn a_sheet_linked_to_a_file_the_project_has_shows_that_file_and_nothing_is_copied() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
    ok(&mut b, edit_sheet("sheet_a", Some("A again"), Some("b.kicad_sch")));
    assert_eq!((sch_of(&b).sheets[0].name.as_str(), sch_of(&b).sheets[0].file.as_str()), ("A again", "b.kicad_sch"));
    assert_eq!(files(&b), vec!["a.kicad_sch", "b.kicad_sch"]);
    assert_eq!(marker(&b, "b.kicad_sch"), "b");
}

#[test]
fn a_sheet_file_is_a_bare_name_and_a_sheet_cannot_show_a_file_that_shows_it() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
    assert_eq!(err(&mut b, edit_sheet("sheet_a", None, Some("sub/x.kicad_sch"))), "ops_bad_sheet");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", None, Some(" "))), "ops_bad_sheet");
    assert_eq!(err(&mut b, edit_sheet("sheet_a", None, Some("a"))), "ops_sheet_unchanged", "a.kicad_sch is what it already shows");

    // a.kicad_sch places a sheet T; on that screen T may not be linked to a.kicad_sch itself
    let mut d = hierarchy();
    d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().sheets.push(sheet("sheet_t", "T", "t.kicad_sch"));
    d.sheet_contents.as_mut().unwrap().insert("t.kicad_sch".into(), SchematicSection::default());
    let mut b = Board::new(d.clone(), &m, mm(0.1), mm(0.3));
    let on_a = |cmd: SchCmd| Cmd::OnSheet { sheet: "sheet_a".into(), cmd: Box::new(Cmd::SchEdit(cmd)) };
    assert_eq!(b.apply(&on_a(edit_sheet("sheet_t", None, Some("a.kicad_sch")))).unwrap_err()[0].check, "ops_sheet_recursion");
    // nor to a file that shows a.kicad_sch: b.kicad_sch holding a sheet U that shows it would close a loop
    d.sheet_contents.as_mut().unwrap().get_mut("b.kicad_sch").unwrap().sheets.push(sheet("sheet_u", "U", "a.kicad_sch"));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
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
    s.wires.push(Wire { id: "wire_a".into(), net: "N".into(), pins: vec![], pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(10.16), Nm::ZERO)], bus: false });
    s.wires.push(Wire { id: "wire_bus".into(), net: "D[0..3]".into(), pins: vec![], pts: vec![p(Nm::ZERO, mm(5.08)), p(mm(10.16), mm(5.08))], bus: true });
    s.bus_entries.push(BusEntry { id: "bent_a".into(), at: p(mm(2.54), mm(5.08)), size: p(mm(2.54), mm(2.54)) });
    s.lines.push(SchLine { id: "sln_a".into(), pts: vec![p(Nm::ZERO, mm(12.7)), p(mm(10.16), mm(12.7))], width_nm: Nm::ZERO });
    s.junctions.push(Junction { id: "jct_a".into(), at: p(mm(5.08), Nm::ZERO) });
    // a stem makes `jct_a` a T, a junction KiCad keeps: a stroke edit is a commit of connectable items, whose clean-up drops a junction that joins nothing (r3 C3-M1)
    s.wires.push(Wire { id: "wire_stem".into(), net: "N".into(), pins: vec![], pts: vec![p(mm(5.08), Nm::ZERO), p(mm(5.08), mm(-2.54))], bus: false });
    s.labels.push(NetLabel { id: "lbl_a".into(), net: "N".into(), at: p(Nm::ZERO, Nm::ZERO), kind: LabelKind::Local });
    s
}

fn stroke(ids_: &[&str], width: Option<Nm>, style: Option<SchLineStyle>, color: Option<SchColor>, diameter: Option<Nm>) -> SchCmd {
    SchCmd::SetStroke { ids: ids(ids_), width_nm: width, style, color, diameter_nm: diameter }
}

#[test]
fn wire_bus_and_entry_strokes_are_set_and_put_back_to_the_default() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, stroke(&["wire_a", "wire_bus", "bent_a"], Some(mm(0.3)), Some(SchLineStyle::Dash), Some(red()), None));
    let strokes = &sch_of(&b).extras.strokes;
    let want = SchStroke { width_nm: mm(0.3), style: SchLineStyle::Dash, color: Some(red()) };
    assert_eq!((strokes.get("wire_a"), strokes.get("wire_bus"), strokes.get("bent_a")), (Some(&want), Some(&want), Some(&want)));
    // one field at a time: the others stay
    ok(&mut b, stroke(&["wire_a"], Some(Nm::ZERO), None, None, None));
    assert_eq!(sch_of(&b).extras.strokes["wire_a"], SchStroke { width_nm: Nm::ZERO, style: SchLineStyle::Dash, color: Some(red()) });
    // a colour of all zeroes is "unspecified": the layer's own colour
    ok(&mut b, stroke(&["wire_a"], None, Some(SchLineStyle::Default), Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), None));
    assert!(!sch_of(&b).extras.strokes.contains_key("wire_a"), "a stroke equal to the default is not stored");
    // the width cannot go negative (`std::max( 0, width )`)
    ok(&mut b, stroke(&["wire_a"], Some(um(-5.0)), Some(SchLineStyle::Dot), None, None));
    assert_eq!(sch_of(&b).extras.strokes["wire_a"], SchStroke { width_nm: Nm::ZERO, style: SchLineStyle::Dot, color: None });
}

#[test]
fn a_graphic_line_keeps_its_width_where_it_always_was() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, stroke(&["sln_a"], Some(mm(0.25)), Some(SchLineStyle::DashDot), Some(red()), None));
    assert_eq!(sch_of(&b).lines[0].width_nm, mm(0.25));
    assert_eq!(sch_of(&b).extras.strokes["sln_a"], SchStroke { width_nm: Nm::ZERO, style: SchLineStyle::DashDot, color: Some(red()) });
    ok(&mut b, stroke(&["sln_a"], Some(Nm::ZERO), Some(SchLineStyle::Default), Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), None));
    assert_eq!(sch_of(&b).lines[0].width_nm, Nm::ZERO);
    assert!(sch_of(&b).extras.strokes.is_empty());
}

#[test]
fn a_junction_takes_a_diameter_and_a_colour_and_ignores_the_line_fields() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    // as in the mixed selection of DIALOG_WIRE_BUS_PROPERTIES: the width and style are for the lines, the diameter for the junction
    ok(&mut b, stroke(&["wire_a", "jct_a"], Some(mm(0.3)), Some(SchLineStyle::Dash), Some(red()), Some(mm(1.0))));
    assert_eq!(sch_of(&b).extras.junction_looks["jct_a"], JunctionLook { diameter_nm: mm(1.0), color: Some(red()) });
    assert_eq!(sch_of(&b).extras.strokes["wire_a"], SchStroke { width_nm: mm(0.3), style: SchLineStyle::Dash, color: Some(red()) });
    assert!(!sch_of(&b).extras.strokes.contains_key("jct_a"));
    ok(&mut b, stroke(&["jct_a"], None, None, Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), Some(Nm::ZERO)));
    assert!(sch_of(&b).extras.junction_looks.is_empty());
}

#[test]
fn stroke_edits_refuse_unknown_items_and_changes_that_change_nothing() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    assert_eq!(err(&mut b, stroke(&[], Some(Nm::new(1)), None, None, None)), "ops_nothing_selected");
    // a label has no stroke; the wire named with it is left alone as well
    assert_eq!(err(&mut b, stroke(&["wire_a", "lbl_a"], Some(mm(0.3)), None, None, None)), "ops_unknown_item");
    assert!(sch_of(&b).extras.is_empty());
    assert_eq!(err(&mut b, stroke(&["wire_a"], Some(Nm::ZERO), Some(SchLineStyle::Default), None, None)), "ops_stroke_unchanged");
    assert_eq!(err(&mut b, stroke(&["jct_a"], Some(mm(0.3)), None, None, None)), "ops_stroke_unchanged", "a width means nothing to a junction");
    assert!(sch_of(&b).extras.is_empty());
}

#[test]
fn the_stroke_of_a_deleted_wire_goes_at_the_next_stroke_edit() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, stroke(&["wire_a", "wire_bus"], Some(mm(0.3)), None, None, None));
    b.apply(&Cmd::DeleteWire { id: "wire_bus".into() }).unwrap();
    ok(&mut b, stroke(&["wire_a"], Some(mm(0.4)), None, None, None));
    assert_eq!(sch_of(&b).extras.strokes.keys().cloned().collect::<Vec<_>>(), vec!["wire_a".to_string()]);
}

#[test]
fn a_stroke_stays_with_its_wire_when_the_wire_is_moved_or_dragged() {
    let (m, d) = board(stroke_sheet());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, stroke(&["wire_a", "jct_a"], Some(mm(0.3)), None, None, Some(mm(0.9))));
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: ids(&["wire_a", "jct_a"]), dx: mm(2.54), dy: Nm::ZERO, turns: Vec::new(), about: None })).unwrap();
    let w = sch_of(&b).wires.iter().find(|w| w.id == "wire_a").unwrap();
    assert_eq!(w.pts[0], p(mm(2.54), Nm::ZERO), "the wire moved");
    assert_eq!(sch_of(&b).extras.strokes["wire_a"].width_nm, mm(0.3), "and kept its stroke");
    assert_eq!(sch_of(&b).extras.junction_looks["jct_a"].diameter_nm, mm(0.9));
    b.apply(&Cmd::SchMove(SchMoveCmd::Drag { ids: ids(&["wire_a"]), vertices: BTreeMap::new(), dx: Nm::ZERO, dy: mm(2.54), ortho: true, grid: Nm::ZERO, turns: Vec::new(), about: None })).unwrap();
    assert_eq!(sch_of(&b).extras.strokes["wire_a"].width_nm, mm(0.3));
    assert!(sch_of(&b).wires.iter().any(|w| w.id == "wire_a"));
}

#[test]
fn the_new_verbs_read_as_the_studio_sends_them() {
    let parse = |json: &str| -> Cmd { serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}")) };
    assert_eq!(parse(r#"{"op":"sch_edit","verb":"edit_label","id":"lbl_a","text":"X","shape":"tri_state","spin":"bottom"}"#), Cmd::SchEdit(label("lbl_a", Some("X"), Some(LabelShape::TriState), Some(LabelSpin::Bottom))));
    assert_eq!(parse(r#"{"op":"sch_edit","verb":"edit_text","id":"txt_a","size_nm":2540000,"angle":90000}"#), Cmd::SchEdit(text("txt_a", None, Some(mm(2.54)), Some(90_000))));
    assert_eq!(parse(r#"{"op":"sch_edit","verb":"edit_sheet","id":"sheet_a","name":"P","file":"p.kicad_sch"}"#), Cmd::SchEdit(edit_sheet("sheet_a", Some("P"), Some("p.kicad_sch"))));
    assert_eq!(
        parse(r#"{"op":"sch_edit","verb":"set_stroke","ids":["wire_a"],"width_nm":300000,"style":"dash_dot_dot","color":{"r":255,"g":0,"b":0,"a":255},"diameter_nm":900000}"#),
        Cmd::SchEdit(stroke(&["wire_a"], Some(mm(0.3)), Some(SchLineStyle::DashDotDot), Some(red()), Some(mm(0.9))))
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
    assert!(!is(stroke(&["wire_a"], Some(Nm::new(1)), None, None, None)));
    assert!(Cmd::OnSheet { sheet: "s".into(), cmd: Box::new(Cmd::SchEdit(stroke(&["wire_a"], Some(Nm::new(1)), None, None, None))) }.edits_connectivity() == false);
}

fn fields_cmd(id: &str, fields: Vec<eda_model::sch_extras::LabelField>) -> SchCmd {
    SchCmd::EditLabel { id: id.into(), text: None, shape: None, spin: None, size_nm: None, bold: None, italic: None, auto_rotate: None, fields: Some(fields) }
}

fn fld(name: &str, text: &str) -> eda_model::sch_extras::LabelField {
    eda_model::sch_extras::LabelField { name: name.into(), text: text.into(), visible: true, italic: true, ..Default::default() }
}

/// Label Properties' grid of the label's own fields (`DIALOG_LABEL_PROPERTIES::TransferDataFromWindow`): the net class is stored as `Netclass` ("Net Class" is how
/// it reads), an empty unnamed field goes, an empty net class goes when another is there, an unnamed one is "untitled", two of one name are refused, and the
/// same grid again is no change.
#[test]
fn label_properties_keep_the_labels_own_fields_as_the_dialogs_ok_does() {
    let (m, d) = board(labels());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, fields_cmd("lbl_l", vec![fld("Net Class", "Power"), fld("", ""), fld("", "a note")]));
    let got = sch_of(&b).extras.label_fields.get("lbl_l").cloned().unwrap();
    assert_eq!(got.iter().map(|f| (f.name.as_str(), f.text.as_str())).collect::<Vec<_>>(), vec![("Netclass", "Power"), ("untitled", "a note")]);
    assert_eq!(sch_of(&b).extras.label_netclass("lbl_l"), Some("Power"));
    // an empty net class beside another goes
    ok(&mut b, fields_cmd("lbl_g", vec![fld("Netclass", ""), fld("Netclass", "HV")]));
    assert_eq!(sch_of(&b).extras.label_fields["lbl_g"].len(), 1);
    // two of one name, in any case: refused, nothing changed
    let before = sch_of(&b).extras.label_fields.clone();
    assert_eq!(err(&mut b, fields_cmd("lbl_l", vec![fld("Note", "a"), fld("note", "b")])), "ops_label_field_name");
    assert_eq!(err(&mut b, fields_cmd("lbl_l", vec![fld("Intersheetrefs", "x")])), "ops_label_field_name");
    assert_eq!(sch_of(&b).extras.label_fields, before);
    // the same grid again changes nothing
    assert_eq!(err(&mut b, fields_cmd("lbl_l", got.clone())), "ops_label_unchanged");
    // an empty grid takes the fields away
    ok(&mut b, fields_cmd("lbl_l", vec![]));
    assert!(!sch_of(&b).extras.label_fields.contains_key("lbl_l"));
}

// ------------------------------------------------------------------- the Properties panel's rows of UI-LEFT S7 (task-u1-props)

#[test]
fn set_text_attrs_keeps_a_free_texts_bold_italic_colour_and_hyperlink_and_a_labels_colour() {
    let mut s = text_sheet();
    s.labels.push(NetLabel { id: "lbl_l".into(), net: "A".into(), at: p(mm(5.08), Nm::ZERO), kind: LabelKind::Local });
    let (m, d) = board(s);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let attrs = |ids_: &[&str], bold, italic, color, href: Option<&str>| SchCmd::SetTextAttrs { ids: ids(ids_), bold, italic, color, href: href.map(str::to_string), font: None, exclude_from_sim: None };
    ok(&mut b, attrs(&["txt_a"], Some(true), None, Some(red()), Some(" https://kicad.org ")));
    let a = &sch_of(&b).extras.text_attrs["txt_a"];
    assert_eq!((a.bold, a.italic, a.color, a.href.as_deref()), (true, false, Some(red()), Some("https://kicad.org")));
    ok(&mut b, attrs(&["lbl_l"], None, None, Some(red()), None));
    assert_eq!(sch_of(&b).extras.text_attrs["lbl_l"].color, Some(red()));
    // UNSPECIFIED and no hyperlink take them away, and an entry that says nothing goes.
    ok(&mut b, attrs(&["txt_a", "lbl_l"], None, None, Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), None));
    ok(&mut b, attrs(&["txt_a"], Some(false), None, None, Some("")));
    assert!(sch_of(&b).extras.text_attrs.is_empty(), "{:?}", sch_of(&b).extras.text_attrs);
    assert_eq!(err(&mut b, attrs(&["lbl_l"], Some(true), None, None, None)), "ops_bad_text", "a label's bold is Label Properties'");
    assert_eq!(err(&mut b, attrs(&["nope"], None, None, Some(red()), None)), "ops_unknown_item");
    assert_eq!(err(&mut b, attrs(&["txt_a"], Some(false), None, None, None)), "ops_nothing_to_change");
}

#[test]
fn set_line_ends_moves_a_wires_or_a_lines_end_and_cleans_the_sheet_up() {
    let mut s = SchematicSection::default();
    s.wires.push(Wire { id: "wire_a".into(), net: "N".into(), pins: vec![], pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(10.16), Nm::ZERO)], bus: false });
    s.lines.push(SchLine { id: "sln_a".into(), pts: vec![p(Nm::ZERO, mm(5.08)), p(mm(5.08), mm(5.08))], width_nm: Nm::ZERO });
    let (m, d) = board(s);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let ends = |id: &str, start: Option<Point>, end: Option<Point>| SchCmd::SetLineEnds { id: id.into(), start, end };
    ok(&mut b, ends("wire_a", None, Some(p(mm(12.7), Nm::ZERO))));
    assert_eq!(sch_of(&b).wires.iter().map(|w| w.pts.clone()).collect::<Vec<_>>(), vec![vec![p(Nm::ZERO, Nm::ZERO), p(mm(12.7), Nm::ZERO)]]);
    ok(&mut b, ends("sln_a", Some(p(mm(1.27), mm(5.08))), None));
    assert_eq!(sch_of(&b).lines[0].pts, vec![p(mm(1.27), mm(5.08)), p(mm(5.08), mm(5.08))]);
    assert_eq!(err(&mut b, ends("sln_a", None, Some(p(mm(1.27), mm(5.08))))), "ops_bad_wire", "a line whose ends meet is no line");
    assert_eq!(err(&mut b, ends("sln_a", None, None)), "ops_bad_wire");
    assert_eq!(err(&mut b, ends("nope", Some(p(Nm::ZERO, Nm::ZERO)), None)), "ops_unknown_item");
}

#[test]
fn set_exclude_from_pos_files_marks_every_placed_unit_of_the_reference() {
    let mut s = SchematicSection::default();
    let sym = |unit: u32, y: Nm| eda_model::ir::SymbolInstance { id: "U1".into(), at: p(Nm::ZERO, y), rot: 0, mirrored: false, mirror_y: false, lib_id: "Amp:Dual".into(), unit, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
    s.symbols = vec![sym(1, Nm::ZERO), sym(2, mm(10.16))];
    let (m, d) = board(s);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, SchCmd::SetExcludeFromPosFiles { ids: ids(&["U1"]), exclude: true });
    assert_eq!(sch_of(&b).extras.excluded_from_pos_files.iter().cloned().collect::<Vec<_>>(), ["U1", "U1#2"]);
    assert_eq!(err(&mut b, SchCmd::SetExcludeFromPosFiles { ids: ids(&["U1"]), exclude: true }), "ops_nothing_to_change");
    ok(&mut b, SchCmd::SetExcludeFromPosFiles { ids: ids(&["U1"]), exclude: false });
    assert!(sch_of(&b).extras.excluded_from_pos_files.is_empty());
    assert_eq!(err(&mut b, SchCmd::SetExcludeFromPosFiles { ids: ids(&["U9"]), exclude: true }), "ops_unknown_symbol");
}

// ------------------------------------------------------------------- Text, Label and Text Box Properties' looks (UI-LEFT S6, task-u3-schtext)

fn text_attrs_cmd(ids_: &[&str], font: Option<&str>, href: Option<&str>, exclude: Option<bool>, color: Option<SchColor>) -> SchCmd {
    SchCmd::SetTextAttrs { ids: ids(ids_), bold: None, italic: None, color, href: href.map(str::to_string), font: font.map(str::to_string), exclude_from_sim: exclude }
}

#[test]
fn the_text_dialogs_font_link_and_simulation_exclusion_are_kept_and_default_font_is_no_font() {
    let mut s = text_sheet();
    s.labels.push(NetLabel { id: "lbl_l".into(), net: "A".into(), at: p(mm(5.08), Nm::ZERO), kind: LabelKind::Local });
    s.extras.graphics.push(eda_model::sch_extras::SchGraphic {
        id: "shp_tb".into(),
        shape: eda_model::sch_extras::SchGraphicKind::TextBox { start: p(Nm::ZERO, mm(10.0)), end: p(mm(20.0), mm(15.0)), text: "box".into(), angle: 0, size_nm: mm(1.27), bold: false, italic: false, h_align: Default::default(), v_align: Default::default(), margin_nm: Nm::ZERO, margins_nm: None },
        width_nm: Nm::ZERO,
        line_style: Default::default(),
        color: None,
        fill: Default::default(),
        fill_color: None,
    });
    let (m, d) = board(s);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, text_attrs_cmd(&["txt_a"], Some("KiCad Font"), Some("#2"), Some(true), None));
    let a = &sch_of(&b).extras.text_attrs["txt_a"];
    assert_eq!((a.font.as_deref(), a.href.as_deref(), a.exclude_from_sim), (Some("KiCad Font"), Some("#2"), true));
    // a text box takes a font, a colour, a link and the exclusion; its bold is its shape's
    ok(&mut b, text_attrs_cmd(&["shp_tb"], Some("DejaVu Sans"), Some("https://kicad.org"), Some(true), Some(red())));
    assert_eq!(sch_of(&b).extras.text_attrs["shp_tb"].font.as_deref(), Some("DejaVu Sans"));
    assert_eq!(err(&mut b, SchCmd::SetTextAttrs { ids: ids(&["shp_tb"]), bold: Some(true), italic: None, color: None, href: None, font: None, exclude_from_sim: None }), "ops_bad_text");
    // a label takes a font and a colour, not a link or the exclusion
    ok(&mut b, text_attrs_cmd(&["lbl_l"], Some("DejaVu Sans"), None, None, None));
    assert_eq!(sch_of(&b).extras.text_attrs["lbl_l"].font.as_deref(), Some("DejaVu Sans"));
    assert_eq!(err(&mut b, text_attrs_cmd(&["lbl_l"], None, Some("#1"), None, None)), "ops_bad_text");
    assert_eq!(err(&mut b, text_attrs_cmd(&["lbl_l"], None, None, Some(true), None)), "ops_bad_text");
    // `ValidateHyperlink`: a page, a URL with a scheme, or nothing
    assert_eq!(err(&mut b, text_attrs_cmd(&["txt_a"], None, Some("not a link"), None, None)), "ops_bad_hyperlink");
    // "Default Font" is the default one (`GetFontSelection` returns `nullptr`): the entry that says nothing else goes
    ok(&mut b, text_attrs_cmd(&["lbl_l"], Some("Default Font"), None, None, None));
    assert!(!sch_of(&b).extras.text_attrs.contains_key("lbl_l"));
}

#[test]
fn a_hyperlink_is_a_page_or_a_url_with_a_scheme() {
    for good in ["", "#1", "#A2", "https://kicad.org", "file:///tmp/x.pdf", "mailto:a@b.c"] {
        assert!(crate::sch_props::valid_hyperlink(good), "{good}");
    }
    for bad in ["kicad.org", "1http://x", "a b:c", ":x"] {
        assert!(!crate::sch_props::valid_hyperlink(bad), "{bad}");
    }
}

fn with_label_field() -> SchematicSection {
    let mut s = SchematicSection::default();
    s.labels.push(NetLabel { id: "lbl_g".into(), net: "SIG".into(), at: p(mm(10.16), mm(10.16)), kind: LabelKind::Global { shape: LabelShape::Input } });
    s.extras.label_spins.insert("lbl_g".into(), LabelSpin::Right);
    s.extras.label_fields.insert("lbl_g".into(), vec![eda_model::sch_extras::LabelField { name: "Netclass".into(), text: "HV".into(), visible: true, italic: true, ..Default::default() }]);
    s
}

fn placed_field(b: &Board, name: &str) -> eda_engine::label_fields::PlacedLabelField {
    let sch = sch_of(b);
    let l = sch.labels.iter().find(|l| l.id == "lbl_g").unwrap();
    eda_engine::label_fields::placed(sch, l, false).into_iter().find(|f| f.name == name).unwrap()
}

#[test]
fn a_labels_field_moved_by_hand_stays_and_takes_the_label_out_of_autoplace() {
    let (m, d) = board(with_label_field());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let before = placed_field(&b, "Netclass");
    // `SCH_MOVE_TOOL::moveItem` on the field alone: `parent->SetFieldsAutoplaced( AUTOPLACE_NONE )`
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: ids(&["fld:lbl_g:Netclass"]), dx: mm(2.54), dy: mm(1.27), turns: Vec::new(), about: None })).unwrap();
    let moved = placed_field(&b, "Netclass");
    assert_eq!(moved.at, p(before.at.x + mm(2.54), before.at.y + mm(1.27)));
    assert!(eda_engine::label_fields::hand_placed(sch_of(&b), "lbl_g"));
    // the label edited (a new text) keeps the hand-placed field where it is: no autoplace while the fields are not autoplaced
    ok(&mut b, SchCmd::EditLabel { id: "lbl_g".into(), text: Some("A_MUCH_LONGER_NAME".into()), shape: None, spin: None, size_nm: None, bold: None, italic: None, auto_rotate: None, fields: None });
    assert_eq!(placed_field(&b, "Netclass").at, moved.at);
    // the label moved takes it along
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: ids(&["lbl_g"]), dx: mm(5.08), dy: Nm::ZERO, turns: Vec::new(), about: None })).unwrap();
    assert_eq!(placed_field(&b, "Netclass").at, p(moved.at.x + mm(5.08), moved.at.y));
    // Autoplace Fields puts it back under the label and the label's fields are autoplaced again
    ok(&mut b, SchCmd::AutoplaceFields { ids: ids(&["lbl_g"]) });
    assert!(!eda_engine::label_fields::hand_placed(sch_of(&b), "lbl_g"));
    assert_eq!(err(&mut b, SchCmd::AutoplaceFields { ids: ids(&["lbl_g"]) }), "ops_fields_unchanged");
}

#[test]
fn field_properties_of_a_labels_field_edits_its_text_and_place_and_keeps_the_sheet_references_text() {
    let (m, d) = board(with_label_field());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let field = |id: &str, text: Option<&str>, at: Option<Point>, visible: Option<bool>| SchCmd::EditField { id: id.into(), text: text.map(str::to_string), at, vertical: None, h: None, v: None, size_nm: None, bold: None, italic: None, visible, name_shown: None, allow_autoplace: None, local_at: None, local_vertical: None, keep_autoplaced: false };
    ok(&mut b, field("fld:lbl_g:Netclass", Some("LV"), None, None));
    assert_eq!(sch_of(&b).extras.label_fields["lbl_g"][0].text, "LV");
    assert!(!eda_engine::label_fields::hand_placed(sch_of(&b), "lbl_g"), "a new text alone keeps the fields autoplaced");
    ok(&mut b, field("fld:lbl_g:Netclass", None, Some(p(mm(30.48), mm(30.48))), None));
    assert_eq!(placed_field(&b, "Netclass").at, p(mm(30.48), mm(30.48)));
    assert!(eda_engine::label_fields::hand_placed(sch_of(&b), "lbl_g"));
    // the Sheet References keep `${INTERSHEET_REFS}` and Schematic Setup's visibility, and can be placed by hand like any field
    assert_eq!(err(&mut b, field("fld:lbl_g:Intersheetrefs", Some("x"), None, None)), "ops_field_text_fixed");
    assert_eq!(err(&mut b, field("fld:lbl_g:Intersheetrefs", None, None, Some(true))), "ops_field_text_fixed");
    ok(&mut b, field("fld:lbl_g:Intersheetrefs", None, Some(p(mm(40.64), mm(10.16))), None));
    assert_eq!(placed_field(&b, "Intersheetrefs").at, p(mm(40.64), mm(10.16)));
    assert_eq!(err(&mut b, field("fld:lbl_g:Netclass", None, Some(p(mm(30.48), mm(30.48))), None)), "ops_field_unchanged");
}

/// UI-LEFT-2 SS12: a label's field keeps its own text size, bold and shown name (`DIALOG_FIELD_PROPERTIES::UpdateField`: `SetTextSize`, `SetBold`,
/// `SetNameShown`; Label Properties' grid: `FDC_TEXT_SIZE`, `FDC_BOLD`, `FDC_SHOW_NAME`; Edit Text & Graphics' `processItem`), and the autoplaced field is
/// stacked by its own height.
#[test]
fn a_labels_field_keeps_its_size_bold_and_shown_name() {
    let (m, d) = board(with_label_field());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let before = placed_field(&b, "Netclass");
    let looks = |id: &str, size: Option<Nm>, bold: Option<bool>, name_shown: Option<bool>| SchCmd::EditField { id: id.into(), text: None, at: None, vertical: None, h: None, v: None, size_nm: size, bold, italic: None, visible: None, name_shown, allow_autoplace: None, local_at: None, local_vertical: None, keep_autoplaced: false };
    ok(&mut b, looks("fld:lbl_g:Netclass", Some(mm(2.54)), Some(true), Some(true)));
    let f = &sch_of(&b).extras.label_fields["lbl_g"][0];
    assert_eq!((f.size_nm, f.bold, f.name_shown), (mm(2.54), true, true));
    // still autoplaced, and under the label by its own height: `accumulated + field.GetTextHeight() / 2`
    assert!(!eda_engine::label_fields::hand_placed(sch_of(&b), "lbl_g"));
    let after = placed_field(&b, "Netclass");
    assert_eq!((after.at.y - before.at.y, after.size_nm, after.bold, after.name_shown), (mm(2.54) / 2 - mm(1.27) / 2, mm(2.54), true, true));
    // back to 50 mil is the default kept as none; the same again changes nothing
    ok(&mut b, looks("fld:lbl_g:Netclass", Some(mm(1.27)), None, None));
    assert_eq!(sch_of(&b).extras.label_fields["lbl_g"][0].size_nm, Nm::ZERO);
    assert_eq!(err(&mut b, looks("fld:lbl_g:Netclass", Some(mm(1.27)), Some(true), Some(true))), "ops_field_unchanged");
    // the Sheet References keep the default look (the studio stores nothing of that field)
    assert_eq!(err(&mut b, looks("fld:lbl_g:Intersheetrefs", None, Some(true), None)), "ops_bad_field");
    // Label Properties' grid: the row's Text Size, Bold and Show Name
    let row = eda_model::sch_extras::LabelField { name: "Netclass".into(), text: "HV".into(), visible: true, italic: true, size_nm: mm(1.778), bold: false, name_shown: true };
    ok(&mut b, SchCmd::EditLabel { id: "lbl_g".into(), text: None, shape: None, spin: None, size_nm: None, bold: None, italic: None, auto_rotate: None, fields: Some(vec![row.clone()]) });
    assert_eq!(sch_of(&b).extras.label_fields["lbl_g"], vec![row]);
    // Edit Text & Graphics on the label fields: size, bold and Show Field Names
    let spec = crate::sch_global_edit::GlobalEditSpec { label_fields: true, text_size_nm: Some(mm(2.0)), bold: Some(true), show_field_names: Some(false), ..Default::default() };
    ok(&mut b, SchCmd::GlobalEditTextGraphics { spec });
    let f = &sch_of(&b).extras.label_fields["lbl_g"][0];
    assert_eq!((f.size_nm, f.bold, f.name_shown), (mm(2.0), true, false));
}

#[test]
fn a_label_turned_or_mirrored_turns_its_hand_placed_fields_about_its_anchor() {
    let (m, d) = board(with_label_field());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SchMove(SchMoveCmd::Move { ids: ids(&["fld:lbl_g:Netclass"]), dx: mm(2.54), dy: Nm::ZERO, turns: Vec::new(), about: None })).unwrap();
    let anchor = sch_of(&b).labels[0].at;
    let f = placed_field(&b, "Netclass");
    let (dx, dy) = (f.at.x - anchor.x, f.at.y - anchor.y);
    b.apply(&Cmd::SchMove(SchMoveCmd::Rotate { ids: ids(&["lbl_g"]), vertices: BTreeMap::new(), ccw: true, about: None, grid: Nm::ZERO })).unwrap();
    let anchor = sch_of(&b).labels[0].at;
    let f = placed_field(&b, "Netclass");
    // `RotatePoint( pt, aCenter, ANGLE_90 )`: (x, y) -> (y, -x), the text vertical
    assert_eq!((f.at.x - anchor.x, f.at.y - anchor.y, f.vertical), (dy, -dx, true));
    // a lone label mirrored left-right where it stands (`MirrorSpinStyle( true )`): the anchor stays, the field's x goes to the other side of it
    let (fx, fy) = (f.at.x - anchor.x, f.at.y - anchor.y);
    b.apply(&Cmd::SchMove(SchMoveCmd::Mirror { ids: ids(&["lbl_g"]), vertices: BTreeMap::new(), vertical: false, about: None, grid: Nm::ZERO })).unwrap();
    let anchor = sch_of(&b).labels[0].at;
    let g = placed_field(&b, "Netclass");
    assert_eq!((g.at.x - anchor.x, g.at.y - anchor.y), (-fx, fy));
}

// ------------------------------------------------------------------- Sheet Properties' Style, Attributes and fields; top-level sheets (UI-LEFT S8)

fn sheet_props(id: &str) -> SchCmd {
    SchCmd::SetSheetProps { id: id.into(), border_width_nm: None, border_color: None, fill_color: None, exclude_from_sim: None, exclude_from_bom: None, exclude_from_board: None, dnp: None }
}

#[test]
fn sheet_properties_keep_the_border_the_fill_and_the_attributes_and_the_legacy_look_keeps_no_entry() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
    let SchCmd::SetSheetProps { id, .. } = sheet_props("sheet_a") else { unreachable!() };
    ok(&mut b, SchCmd::SetSheetProps { id: id.clone(), border_width_nm: Some(mm(0.3)), border_color: Some(red()), fill_color: Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), exclude_from_sim: None, exclude_from_bom: Some(true), exclude_from_board: None, dnp: Some(true) });
    let p = &sch_of(&b).extras.sheet_props["sheet_a"];
    assert_eq!((p.border_width_nm, p.border_color, p.fill_color, p.exclude_from_bom, p.dnp, p.exclude_from_sim), (mm(0.3), Some(red()), None, true, true, false), "all zeroes is UNSPECIFIED");
    assert_eq!(err(&mut b, SchCmd::SetSheetProps { id: id.clone(), border_width_nm: None, border_color: None, fill_color: None, exclude_from_sim: None, exclude_from_bom: None, exclude_from_board: None, dnp: Some(true) }), "ops_nothing_to_change");
    // back to the look the writer has always given a sheet: no entry
    let legacy = eda_model::sch_extras::SheetProps::legacy();
    ok(&mut b, SchCmd::SetSheetProps { id: id.clone(), border_width_nm: Some(legacy.border_width_nm), border_color: Some(SchColor { r: 0, g: 0, b: 0, a: 0 }), fill_color: legacy.fill_color, exclude_from_sim: None, exclude_from_bom: Some(false), exclude_from_board: None, dnp: Some(false) });
    assert!(sch_of(&b).extras.sheet_props.is_empty());
    assert_eq!(err(&mut b, sheet_props("nope")), "ops_unknown_sheet");
}

/// `SCH_EDIT_TOOL::SetAttribute` takes the sheets and rule areas of the selection too (`RequestSelection( { SCH_SYMBOL_T, SCH_SHEET_T, SCH_RULE_AREA_T } )`),
/// and `new_state` is worked out over all of them: set when any lacks the attribute, cleared when all have it.
#[test]
fn the_attribute_toggles_take_sheets_and_rule_areas_with_one_state_for_all() {
    use eda_model::sch_extras::{SchGraphic, SchGraphicKind};
    let m = ConstraintModel::default();
    let mut d = hierarchy();
    let mut area = SchGraphic::new(SchGraphicKind::RuleArea { pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(10.0), Nm::ZERO), p(mm(10.0), mm(10.0))], exclude_from_sim: false, exclude_from_bom: true, exclude_from_board: false, dnp: false });
    area.id = "rarea_1".into();
    d.schematic.as_mut().unwrap().extras.graphics.push(area);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let toggle = || SchCmd::ToggleSymbolAttribute { ids: ids(&["sheet_a", "rarea_1"]), attribute: "exclude_from_bom".into() };
    let bom = |b: &Board| {
        let s = sch_of(b);
        let area = match &s.extras.graphics[0].shape {
            SchGraphicKind::RuleArea { exclude_from_bom, .. } => *exclude_from_bom,
            _ => unreachable!(),
        };
        (s.extras.sheet_props_of("sheet_a").exclude_from_bom, area)
    };
    ok(&mut b, toggle());
    assert_eq!(bom(&b), (true, true), "the sheet lacked it: both set");
    ok(&mut b, toggle());
    assert_eq!(bom(&b), (false, false), "both had it: both cleared");
    assert!(sch_of(&b).extras.sheet_props.is_empty(), "the legacy look keeps no entry");
}

/// A power symbol is an `SCH_SYMBOL`, so `SetAttribute` takes it with the rest of the selection: its flags are `SchExtras::power_props`' and share the one
/// `new_state`; a power symbol left with no flag set keeps no entry.
#[test]
fn the_attribute_toggles_take_power_symbols() {
    use eda_model::ir::PowerSymbol;
    let m = ConstraintModel::default();
    let mut d = hierarchy();
    let sch = d.schematic.as_mut().unwrap();
    sch.power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(101.6), mm(50.8)), rot: 0, net: "GND".into(), pin: String::new() });
    sch.extras.power_props.insert("#PWR01".into(), eda_model::sch_extras::PowerProps { footprint: "fp".into(), ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let toggle = |attr: &str, list: &[&str]| SchCmd::ToggleSymbolAttribute { ids: ids(list), attribute: attr.into() };
    ok(&mut b, toggle("exclude_from_sim", &["#PWR01"]));
    assert!(sch_of(&b).extras.power_props["#PWR01"].exclude_from_sim);
    ok(&mut b, toggle("exclude_from_sim", &["#PWR01"]));
    assert!(!sch_of(&b).extras.power_props["#PWR01"].exclude_from_sim, "set: cleared again; the footprint stays");
    // with a sheet that lacks the flag, both are set; then both cleared
    ok(&mut b, toggle("exclude_from_bom", &["#PWR01", "sheet_a"]));
    assert!(sch_of(&b).extras.power_props["#PWR01"].exclude_from_bom && sch_of(&b).extras.sheet_props_of("sheet_a").exclude_from_bom);
    ok(&mut b, toggle("exclude_from_bom", &["#PWR01", "sheet_a"]));
    assert!(!sch_of(&b).extras.power_props["#PWR01"].exclude_from_bom && !sch_of(&b).extras.sheet_props_of("sheet_a").exclude_from_bom);
    // a power symbol with nothing of its own keeps no entry
    let mut d = hierarchy();
    d.schematic.as_mut().unwrap().power_symbols.push(PowerSymbol { id: "#PWR01".into(), lib_id: "power:GND".into(), at: p(mm(101.6), mm(50.8)), rot: 0, net: "GND".into(), pin: String::new() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, toggle("exclude_from_board", &["#PWR01"]));
    assert!(sch_of(&b).extras.power_props["#PWR01"].exclude_from_board);
    ok(&mut b, toggle("exclude_from_board", &["#PWR01"]));
    assert!(sch_of(&b).extras.power_props.is_empty());
}

#[test]
fn sheet_fields_are_named_once_and_an_empty_row_goes() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
    let f = |name: &str, text: &str| eda_model::sch_extras::LabelField { name: name.into(), text: text.into(), visible: false, italic: false, ..Default::default() };
    ok(&mut b, SchCmd::SetSheetFields { id: "sheet_a".into(), fields: vec![f("Field2", "x"), f("", ""), f(" Rev ", "B")] });
    assert_eq!(sch_of(&b).extras.sheet_fields["sheet_a"], vec![f("Field2", "x"), f("Rev", "B")], "the empty row is dropped, a name trimmed");
    assert_eq!(b.apply(&Cmd::SchEdit(SchCmd::SetSheetFields { id: "sheet_a".into(), fields: vec![f("", "y")] })).unwrap_err()[0].hint.as_deref().unwrap_or_default(), "Fields must have a name.");
    assert_eq!(b.apply(&Cmd::SchEdit(SchCmd::SetSheetFields { id: "sheet_a".into(), fields: vec![f("Rev", "1"), f("Rev", "2")] })).unwrap_err()[0].hint.as_deref().unwrap_or_default(), "Field name 'Rev' already in use.");
    assert_eq!(err(&mut b, SchCmd::SetSheetFields { id: "sheet_a".into(), fields: vec![f("Sheetfile", "1")] }), "ops_field_exists", "the mandatory fields' names are taken");
    ok(&mut b, SchCmd::SetSheetFields { id: "sheet_a".into(), fields: vec![] });
    assert!(sch_of(&b).extras.sheet_fields.is_empty());
}

#[test]
fn a_virtual_roots_top_level_sheets_are_added_and_deleted_but_not_the_last_and_not_on_an_ordinary_root() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
    assert_eq!(err(&mut b, SchCmd::AddTopLevelSheet { name: "Power".into() }), "ops_not_top_level", "a root that is its own top-level sheet");
    let mut d = hierarchy();
    d.schematic.as_mut().unwrap().extras.virtual_root = true;
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    ok(&mut b, SchCmd::AddTopLevelSheet { name: "My Power".into() });
    let new = sch_of(&b).sheets.last().unwrap().clone();
    assert_eq!((new.name.as_str(), new.file.as_str()), ("My Power", "my_power.kicad_sch"), "the file is the name, underscores for spaces, in lower case");
    assert_eq!(new.page, "4", "the lowest page no sheet shows: the root 1, A 2, B 3");
    assert!(files(&b).contains(&"my_power.kicad_sch".to_string()));
    // the same name again gets a file of its own (KiCad 25ec2335's hierarchy pane refuses no name: ERC reports `duplicate_sheet_names`, UI-LEFT S21)
    ok(&mut b, SchCmd::AddTopLevelSheet { name: "my power".into() });
    assert_eq!(sch_of(&b).sheets.last().unwrap().file, "my_power_1.kicad_sch");
    ok(&mut b, SchCmd::AddTopLevelSheet { name: "My Power".into() });
    assert_eq!(sch_of(&b).sheets.last().unwrap().file, "my_power_2.kicad_sch");
    let again = sch_of(&b).sheets.last().unwrap().id.clone();
    ok(&mut b, SchCmd::DeleteTopLevelSheet { id: again });
    for id in ["sheet_a", "sheet_b", &new.id] {
        ok(&mut b, SchCmd::DeleteTopLevelSheet { id: id.to_string() });
    }
    assert_eq!(sch_of(&b).sheets.len(), 1);
    let last = sch_of(&b).sheets[0].id.clone();
    assert_eq!(b.apply(&Cmd::SchEdit(SchCmd::DeleteTopLevelSheet { id: last })).unwrap_err()[0].hint.as_deref().unwrap_or_default(), "Cannot delete the last top-level sheet.");
}

#[test]
fn a_pasted_label_keeps_its_hand_placed_fields_and_looks_and_a_pasted_text_its_looks() {
    use eda_model::sch_clipboard::{PasteMode, SchFragment};
    let mut section = with_label_field();
    section.imported_from_kicad = true;
    section.field_layout.insert("lbl_g".into(), vec![eda_model::ir::FieldPlacement { dx: mm(5.08), dy: mm(2.54), ..eda_model::ir::FieldPlacement::at_origin("Netclass") }]);
    section.extras.text_attrs.insert("lbl_g".into(), eda_model::sch_extras::TextAttrs { color: Some(red()), font: Some("KiCad Font".into()), ..Default::default() });
    section.texts.push(SchematicText { id: "txt_c".into(), content: "copied".into(), at: p(Nm::ZERO, Nm::ZERO), angle: 0, size_nm: mm(1.27) });
    section.extras.text_attrs.insert("txt_c".into(), eda_model::sch_extras::TextAttrs { bold: true, href: Some("#1".into()), ..Default::default() });
    let fragment = SchFragment { section, ..Default::default() };
    let (m, d) = board(SchematicSection::default());
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::PasteSch { fragment, dx: mm(25.4), dy: Nm::ZERO, mode: PasteMode::Unique, numbering: Default::default() }).unwrap();
    let sch = sch_of(&b);
    let l = sch.labels.iter().find(|l| l.net == "SIG").expect("the pasted label");
    assert!(eda_engine::label_fields::hand_placed(sch, &l.id), "its fields stay where they were put");
    assert_eq!(sch.field_layout[&l.id][0].dx, mm(5.08));
    assert_eq!(sch.extras.text_attrs[&l.id].font.as_deref(), Some("KiCad Font"));
    let t = sch.texts.iter().find(|t| t.content == "copied").expect("the pasted text");
    assert!(sch.extras.text_attrs[&t.id].bold && sch.extras.text_attrs[&t.id].href.as_deref() == Some("#1"));
}

// ------------------------------------------------------------------- UI-LEFT-2 SS11: the Properties panel's rows that were read-only

#[test]
fn a_directive_label_takes_a_font_and_a_colour_and_nothing_else_of_set_text_attrs() {
    use eda_model::sch_extras::{DirectiveShape, SchGraphic, SchGraphicKind};
    let mut s = text_sheet();
    let mut flag = SchGraphic::new(SchGraphicKind::Directive { at: p(mm(10.0), mm(10.0)), orientation: 0, shape: DirectiveShape::Round, pin_length_nm: mm(2.54), netclass: "HV".into(), component_class: String::new(), text_size_nm: Nm::ZERO });
    flag.id = "dirl_a".into();
    s.extras.graphics.push(flag);
    let (m, d) = board(s);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    // `EDA_TEXT_DESC`'s Font and Color are not masked for an `SCH_DIRECTIVE_LABEL`
    ok(&mut b, text_attrs_cmd(&["dirl_a"], Some("KiCad Font"), None, None, Some(red())));
    let a = &sch_of(&b).extras.text_attrs["dirl_a"];
    assert_eq!((a.font.as_deref(), a.color), (Some("KiCad Font"), Some(red())));
    // its italic, bold and link are (`SCH_DIRECTIVE_LABEL_DESC`, `SCH_LABEL_BASE_DESC`)
    assert_eq!(err(&mut b, text_attrs_cmd(&["dirl_a"], None, Some("#1"), None, None)), "ops_bad_text");
    assert_eq!(err(&mut b, SchCmd::SetTextAttrs { ids: ids(&["dirl_a"]), bold: Some(true), italic: None, color: None, href: None, font: None, exclude_from_sim: None }), "ops_bad_text");
}

#[test]
fn set_image_offset_keeps_the_point_an_image_is_scaled_about_and_is_one_undo_step() {
    use eda_model::sch_extras::{SchGraphic, SchGraphicKind};
    let mut s = text_sheet();
    let mut img = SchGraphic::new(SchGraphicKind::Image { at: p(mm(50.0), mm(50.0)), scale_ppm: 1_000_000, data: eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(4, 4, [1, 2, 3], 254)) });
    img.id = "img_a".into();
    s.extras.graphics.push(img);
    let (m, d) = board(s);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let set = |x: Nm, y: Nm| SchCmd::SetImageOffset { id: "img_a".into(), offset: p(x, y) };
    ok(&mut b, set(mm(1.0), mm(-2.0)));
    assert_eq!(sch_of(&b).extras.image_offsets["img_a"], p(mm(1.0), mm(-2.0)));
    assert_eq!(err(&mut b, set(mm(1.0), mm(-2.0))), "ops_nothing_to_change");
    assert_eq!(err(&mut b, SchCmd::SetImageOffset { id: "txt_a".into(), offset: p(Nm::ZERO, Nm::ZERO) }), "ops_unknown_item");
    // its centre again: no entry
    ok(&mut b, set(Nm::ZERO, Nm::ZERO));
    assert!(sch_of(&b).extras.image_offsets.is_empty());
    b.apply(&Cmd::SchEdit(SchCmd::SetLocked { ids: ids(&["img_a"]), locked: true })).unwrap();
    assert_eq!(err(&mut b, set(mm(1.0), Nm::ZERO)), "ops_locked");
}

// ------------------------------------------------------------------- UI-LEFT-2 SS15: the sheet fields' looks, a sheet pin's font and colour, the sheets' variants

fn sym_at(id: &str) -> eda_model::ir::SymbolInstance {
    eda_model::ir::SymbolInstance { id: id.into(), at: p(Nm::ZERO, Nm::ZERO), rot: 0, mirrored: false, mirror_y: false, lib_id: "Device:R".into(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false }
}

#[test]
fn a_sheet_user_fields_place_font_and_colour_are_kept_and_go_with_the_field() {
    let m = ConstraintModel::default();
    let mut b = Board::new(hierarchy(), &m, mm(0.1), mm(0.3));
    let f = |name: &str| eda_model::sch_extras::LabelField { name: name.into(), text: "x".into(), size_nm: mm(2.54), bold: true, name_shown: true, ..Default::default() };
    ok(&mut b, SchCmd::SetSheetFields { id: "sheet_a".into(), fields: vec![f("Rev")] });
    let look = |name: &str, dx: Nm| crate::sch_props::SheetFieldLookEdit { name: name.into(), dx, dy: mm(-1.27), angle: 90_000, h: eda_model::ir::TextJustify::Left, v: eda_model::ir::TextVAlign::Top, font: Some("KiCad Font".into()), color: Some(red()) };
    ok(&mut b, SchCmd::SetSheetFieldLooks { id: "sheet_a".into(), fields: vec![look("Rev", mm(2.54))] });
    let place = sch_of(&b).extras.sheet_field_places["sheet_a"]["Rev"];
    assert_eq!((place.dx, place.dy, place.angle, place.h, place.v), (mm(2.54), mm(-1.27), 90_000, eda_model::ir::TextJustify::Left, eda_model::ir::TextVAlign::Top));
    assert_eq!(sch_of(&b).extras.field_look("sheet_a", "Rev").map(|l| (l.font.clone(), l.color)), Some((Some("KiCad Font".to_string()), Some(red()))));
    // the field on the page: at the sheet's corner plus its offset, vertical, its size and bold its own
    let drawn = eda_engine::fields::sheet_user_fields(sch_of(&b), &sch_of(&b).sheets[0]);
    assert_eq!((drawn[0].at, drawn[0].vertical, drawn[0].size_nm, drawn[0].bold, drawn[0].name_shown), ((mm(2.54).as_f64(), mm(-1.27).as_f64()), true, mm(2.54).as_f64(), true, true));
    assert_eq!(err(&mut b, SchCmd::SetSheetFieldLooks { id: "sheet_a".into(), fields: vec![look("Rev", mm(2.54))] }), "ops_nothing_to_change");
    assert_eq!(err(&mut b, SchCmd::SetSheetFieldLooks { id: "sheet_a".into(), fields: vec![look("Nope", mm(2.54))] }), "ops_unknown_field");
    // a field renamed or deleted leaves no place or look behind
    ok(&mut b, SchCmd::SetSheetFields { id: "sheet_a".into(), fields: vec![f("Revision")] });
    assert!(sch_of(&b).extras.sheet_field_places.is_empty());
    assert!(sch_of(&b).extras.field_look("sheet_a", "Rev").is_none());
}

#[test]
fn a_sheet_pins_font_and_colour_are_set_text_attrs_and_go_with_the_pin() {
    let mut root = SchematicSection::default();
    let mut s = sheet("sheet_a", "A", "a.kicad_sch");
    s.pins.push(eda_model::ir::SheetPin { id: "spin_a".into(), name: "IN".into(), shape: LabelShape::Input, at: p(Nm::ZERO, mm(2.54)) });
    root.sheets.push(s);
    let m = ConstraintModel::default();
    let mut b = Board::new(design(root), &m, mm(0.1), mm(0.3));
    ok(&mut b, SchCmd::SetTextAttrs { ids: ids(&["spin_a"]), bold: None, italic: None, color: Some(red()), href: None, font: Some("KiCad Font".into()), exclude_from_sim: None });
    let a = &sch_of(&b).extras.text_attrs["spin_a"];
    assert_eq!((a.color, a.font.as_deref()), (Some(red()), Some("KiCad Font")));
    assert_eq!(err(&mut b, SchCmd::SetTextAttrs { ids: ids(&["spin_a"]), bold: Some(true), italic: None, color: None, href: None, font: None, exclude_from_sim: None }), "ops_bad_text", "a pin's bold is Sheet Pin Properties' edit_sheet_pin");
    ok(&mut b, SchCmd::DeleteSheetPin { id: "spin_a".into() });
    assert!(sch_of(&b).extras.text_attrs.is_empty());
}

#[test]
fn a_sheets_attributes_in_a_variant_are_its_record_and_reach_the_symbols_inside() {
    let mut d = hierarchy();
    d.sheet_contents.as_mut().unwrap().get_mut("a.kicad_sch").unwrap().symbols.push(sym_at("R1"));
    let m = ConstraintModel::default();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddDesignVariant { name: "Lite".into(), description: String::new() }).unwrap();
    let set = |variant: &str, dnp: Option<bool>| SchCmd::SetSheetVariant { id: "sheet_a".into(), variant: variant.into(), dnp, exclude_from_sim: None, exclude_from_bom: None, exclude_from_board: None };
    ok(&mut b, set("Lite", Some(true)));
    let sch = sch_of(&b);
    assert!(eda_model::variants::sheet_flags(sch, "sheet_a", "Lite").dnp, "SetDNP( true, &instance, \"Lite\" )");
    assert!(!eda_model::variants::sheet_flags(sch, "sheet_a", "").dnp, "the sheet's own flag is not the variant's");
    // the symbols inside: their sheet path's flags (`SCH_SHEET_PATH::GetDNP( variant )`)
    let path = vec!["sheet_a".to_string()];
    assert!(eda_model::variants::sheet_path_flags(b.design(), &path, "Lite").dnp);
    assert!(!eda_model::variants::sheet_path_flags(b.design(), &path, "").dnp);
    assert_eq!(b.design().symbol_bom_flags("R1"), (false, false), "the board's footprint follows the default variant");
    assert_eq!(err(&mut b, set("Lite", Some(true))), "ops_nothing_to_change");
    assert_eq!(err(&mut b, set("", Some(true))), "ops_no_variant");
    assert_eq!(err(&mut b, set("Nope", Some(true))), "ops_unknown_variant");
    // a record that no longer differs is not kept; renaming and removing the variant take the sheet's records along
    ok(&mut b, set("Lite", Some(false)));
    assert!(sch_of(&b).extras.sheet_variants.is_empty());
    ok(&mut b, set("Lite", Some(true)));
    b.apply(&Cmd::RenameDesignVariant { name: "Lite".into(), new_name: "Pro".into() }).unwrap();
    assert!(sch_of(&b).extras.sheet_variants["sheet_a"].contains_key("Pro"));
    b.apply(&Cmd::RemoveDesignVariant { name: "Pro".into() }).unwrap();
    assert!(sch_of(&b).extras.sheet_variants.is_empty());
    // the sheet's own Do not populate and Exclude from BOM reach the footprint of a symbol inside it (`sheet.GetDNP()` in the netlist)
    ok(&mut b, SchCmd::SetSheetProps { id: "sheet_a".into(), border_width_nm: None, border_color: None, fill_color: None, exclude_from_sim: None, exclude_from_bom: Some(true), exclude_from_board: None, dnp: Some(true) });
    assert_eq!(b.design().symbol_bom_flags("R1"), (true, true));
    // and the Fields Table's filters
    let mut base = crate::fields_table::project_view(b.design()).unwrap();
    crate::fields_table::with_sheet_flags(&mut base, b.design(), "");
    let r1 = base.symbols.iter().find(|s| s.id == "R1").unwrap();
    assert!(r1.dnp && r1.exclude_from_bom);
}
