//! The library graphics' properties verbs (`DIALOG_SHAPE_PROPERTIES`, `DIALOG_TEXT_PROPERTIES` of a text box): a shape's width, fill and the units and body styles it is common to, and a text
//! box as one graphic that is added, moved, replaced and refused as KiCad's dialogs refuse it.

use super::*;
use eda_model::ir::{Design, LibraryFill};
use eda_model::sym_fields::{FieldHAlign, FieldVAlign};
use eda_model::symbol::SPoint;
use eda_model::ConstraintModel;
use eda_model::units::mm;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(design(), m, mm(0.1), mm(0.3))
}

fn rectangle() -> LibrarySymbolGraphic {
    LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 1, body_style: 1, start: SPoint { x: -2.54, y: -2.54 }, end: SPoint { x: 2.54, y: 2.54 }, stroke_mm: 0.254, fill: LibraryFill::None }
}

fn text_box() -> LibrarySymbolGraphic {
    LibrarySymbolGraphic::TextBox {
        id: String::new(),
        unit: 1,
        body_style: 1,
        text: "10V\nREF".into(),
        start: SPoint { x: -17.78, y: 6.35 },
        end: SPoint { x: -10.16, y: -1.27 },
        angle_deg: 0.0,
        size_mm: 1.27,
        bold: true,
        italic: false,
        h_align: FieldHAlign::Left,
        v_align: FieldVAlign::Top,
        margin_mm: 0.0,
        stroke_mm: 0.254,
        fill: LibraryFill::None,
        private: false, font: None, text_color: None, stroke_color: None, fill_color: None,
    }
}

fn properties(has_alternate_body_style: bool, unit_count: u32) -> Cmd {
    Cmd::EditSymbolProperties {
        lib_id: "Lib:S".into(),
        reference_prefix: "U".into(),
        description: String::new(),
        keywords: String::new(),
        datasheet: String::new(),
        power: false,
        in_bom: true,
        on_board: true,
        pin_numbers_hidden: false,
        pin_names_hidden: false,
        pin_name_offset_mm: 0.508,
        unit_count,
        has_alternate_body_style,
        footprint_filters: vec![],
        units_locked: None,
        power_local: None,
        exclude_from_sim: None,
        exclude_from_pos_files: None,
    }
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> String {
    let e = b.apply(&cmd).unwrap_err();
    format!("{}: {}", e[0].check, e[0].hint.clone().unwrap_or_default())
}

fn graphics<'a>(b: &'a Board<'_>) -> &'a [LibrarySymbolGraphic] {
    &b.design().symbol_library.as_ref().unwrap().by_lib_id("Lib:S").unwrap().graphics
}

#[test]
fn a_shape_is_made_common_to_all_units_and_body_styles_with_its_width_and_fill_in_one_verb() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    b.apply(&properties(true, 2)).unwrap();
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: rectangle() }).unwrap();
    let id = graphics(&b)[0].id().to_string();
    // the dialog's three boxes: border width, fill, "Common to all units in symbol" and "Common to all body styles"
    b.apply(&Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), stroke_mm: 0.5, fill: LibraryFill::Background, unit: Some(0), body_style: Some(0) }).unwrap();
    let g = &graphics(&b)[0];
    assert_eq!((g.unit(), g.body_style()), (0, 0));
    assert!(matches!(g, LibrarySymbolGraphic::Rectangle { stroke_mm, fill: LibraryFill::Background, .. } if *stroke_mm == 0.5));
    // unchecked: the unit and the body style the editor is showing
    b.apply(&Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), stroke_mm: 0.5, fill: LibraryFill::Background, unit: Some(2), body_style: Some(2) }).unwrap();
    assert_eq!((graphics(&b)[0].unit(), graphics(&b)[0].body_style()), (2, 2));
    // left out, they stay
    b.apply(&Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), stroke_mm: 0.1, fill: LibraryFill::Outline, unit: None, body_style: None }).unwrap();
    assert_eq!((graphics(&b)[0].unit(), graphics(&b)[0].body_style()), (2, 2));
    // Border unchecked: a negative width, kept as the width KiCad's `STROKE_PARAMS` holds for none
    b.apply(&Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), stroke_mm: -1.0, fill: LibraryFill::Background, unit: None, body_style: None }).unwrap();
    assert!(matches!(&graphics(&b)[0], LibrarySymbolGraphic::Rectangle { stroke_mm, .. } if *stroke_mm == -0.0001));
    // a unit or a body style the symbol has not is refused
    assert_eq!(refusal(&mut b, Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), stroke_mm: 0.1, fill: LibraryFill::None, unit: Some(3), body_style: None }), "ops_bad_shape: the symbol has no unit 3");
    b.apply(&properties(false, 2)).unwrap();
    assert_eq!(refusal(&mut b, Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id, stroke_mm: 0.1, fill: LibraryFill::None, unit: None, body_style: Some(2) }), "ops_bad_shape: the symbol has no body style 2");
}

#[test]
fn a_text_box_is_added_moved_and_replaced_as_one_graphic_with_its_id_kept() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: text_box() }).unwrap();
    let id = graphics(&b)[0].id().to_string();
    assert!(!id.is_empty(), "the verb gives it an id");
    // moved by the generic verb: both corners
    b.apply(&Cmd::MoveSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), dx_mm: 2.54, dy_mm: -1.27 }).unwrap();
    let LibrarySymbolGraphic::TextBox { start, end, .. } = &graphics(&b)[0] else { panic!() };
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(near(start.x, -15.24) && near(start.y, 5.08) && near(end.x, -7.62) && near(end.y, -2.54), "{start:?} {end:?}");
    // the dialog's OK: the whole graphic as the dialog left it
    let mut edited = text_box();
    if let LibrarySymbolGraphic::TextBox { text, size_mm, angle_deg, italic, h_align, v_align, stroke_mm, fill, margin_mm, start, end, .. } = &mut edited {
        *text = "5k".into();
        *size_mm = 0.635;
        *angle_deg = 90.0;
        *italic = true;
        *h_align = FieldHAlign::Center;
        *v_align = FieldVAlign::Bottom;
        *stroke_mm = -0.0001;
        *fill = LibraryFill::Background;
        *margin_mm = 0.5;
        *start = SPoint { x: 0.0, y: 0.0 };
        *end = SPoint { x: 1.27, y: -3.81 };
    }
    b.apply(&Cmd::ReplaceSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), graphic: edited.clone() }).unwrap();
    assert_eq!(graphics(&b).len(), 1, "replaced, not added");
    let mut expected = edited;
    expected.set_id(id.clone());
    assert_eq!(graphics(&b)[0], expected, "what the dialog sent, with the id the box had");
    // the common-to-all boxes
    b.apply(&properties(true, 2)).unwrap();
    let mut common = expected.clone();
    common.set_unit(0);
    common.set_body_style(0);
    b.apply(&Cmd::ReplaceSymbolGraphic { lib_id: "Lib:S".into(), id, graphic: common }).unwrap();
    assert_eq!((graphics(&b)[0].unit(), graphics(&b)[0].body_style()), (0, 0));
}

#[test]
fn a_text_box_the_dialog_would_refuse_is_refused() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: text_box() }).unwrap();
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: rectangle() }).unwrap();
    let (box_id, rect_id) = (graphics(&b)[0].id().to_string(), graphics(&b)[1].id().to_string());
    let with = |f: &dyn Fn(&mut LibrarySymbolGraphic)| {
        let mut g = text_box();
        f(&mut g);
        g
    };
    for (what, g, words) in [
        ("size 0", with(&|g| if let LibrarySymbolGraphic::TextBox { size_mm, .. } = g { *size_mm = 0.0 }), "between 0.01 and 1000 mm"),
        ("size 1001", with(&|g| if let LibrarySymbolGraphic::TextBox { size_mm, .. } = g { *size_mm = 1001.0 }), "between 0.01 and 1000 mm"),
        ("angle 45", with(&|g| if let LibrarySymbolGraphic::TextBox { angle_deg, .. } = g { *angle_deg = 45.0 }), "along it (0) or up it (90 degrees)"),
        ("margin -1", with(&|g| if let LibrarySymbolGraphic::TextBox { margin_mm, .. } = g { *margin_mm = -1.0 }), "margin cannot be negative"),
    ] {
        let said = refusal(&mut b, Cmd::ReplaceSymbolGraphic { lib_id: "Lib:S".into(), id: box_id.clone(), graphic: g.clone() });
        assert!(said.contains(words), "{what}: {said}");
        let said = refusal(&mut b, Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: g });
        assert!(said.contains(words), "{what} on add: {said}");
    }
    // a graphic keeps its kind, a shape's dialog is not a text box's
    let said = refusal(&mut b, Cmd::ReplaceSymbolGraphic { lib_id: "Lib:S".into(), id: rect_id.clone(), graphic: text_box() });
    assert_eq!(said, "ops_bad_shape: a graphic cannot become another kind of graphic");
    let said = refusal(&mut b, Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id: box_id, stroke_mm: 0.2, fill: LibraryFill::None, unit: None, body_style: None });
    assert!(said.contains("use replace_symbol_graphic"), "{said}");
    // a unit the symbol has not
    let mut g = rectangle();
    g.set_unit(7);
    assert!(refusal(&mut b, Cmd::ReplaceSymbolGraphic { lib_id: "Lib:S".into(), id: rect_id, graphic: g }).contains("no unit 7"));
    assert_eq!(graphics(&b).len(), 2, "nothing refused was kept");
}

#[test]
fn a_derived_symbol_keeps_no_drawing_of_its_own_so_its_text_boxes_are_edited_on_the_root() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: text_box() }).unwrap();
    let id = graphics(&b)[0].id().to_string();
    b.apply(&Cmd::DeriveSymbol { parent: "Lib:S".into(), lib_id: "Lib:D".into(), keep_footprint: false, keep_datasheet: false, transfer_user_fields: false, keep_content_user_fields: false, overwrite: false }).unwrap();
    let said = refusal(&mut b, Cmd::ReplaceSymbolGraphic { lib_id: "Lib:D".into(), id, graphic: text_box() });
    assert!(said.starts_with("ops_derived_symbol"), "{said}");
    // the derived symbol draws the root's text box, flat
    let flat = b.design().symbol_library.as_ref().unwrap().flatten("Lib:D").unwrap();
    assert!(flat.graphics.iter().any(|g| matches!(g, LibrarySymbolGraphic::TextBox { text, .. } if text == "10V\nREF")));
    // and a placed symbol gets the box as what it is made of: a rectangle for its border
    let engine = flat.to_engine_symbol();
    assert!(engine.graphics.iter().any(|g| matches!(g, eda_model::symbol::SymbolGraphic::Rectangle { stroke_mm, .. } if *stroke_mm == 0.254)));
}

/// E12 (r2-editors-exports): a new symbol's Reference and Value are at the origin, as `LIB_SYMBOL::LIB_SYMBOL` makes its fields; the Value is the name.
#[test]
fn a_new_symbols_reference_and_value_are_at_the_origin() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Lib:S").unwrap();
    let text = eda_kicad::export_kicad_sym(sym);
    assert!(text.contains("(property \"Reference\" \"U\" (at 0 0 0)") || text.contains("(property \"Reference\" \"\" (at 0 0 0)"), "{text}");
    assert!(text.contains("(property \"Value\" \"S\" (at 0 0 0)"), "{text}");
}

/// UI-LEFT Y4: Text Properties of a plain text (`DIALOG_TEXT_PROPERTIES` of a `SCH_TEXT` in the symbol editor) is the whole text as the dialog leaves it -- its text, size,
/// angle, bold, italic, justification, units and body styles -- through `replace_symbol_graphic`, one undo step. A text whose looks an older reader kept beside it (the
/// kept style) has them moved into it first, so the dialog's are the ones the file gets.
#[test]
fn a_plain_text_is_replaced_with_its_looks_and_a_kept_style_gives_way_to_the_edit() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    let text = |h_align: FieldHAlign, bold: bool| LibrarySymbolGraphic::Text { id: String::new(), unit: 0, body_style: 0, text: "A".into(), at: SPoint { x: 1.27, y: 0.0 }, angle_deg: 0.0, size_mm: 1.27, bold, italic: false, h_align, v_align: FieldVAlign::Center, private: false, font: None, text_color: None };
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: text(FieldHAlign::Center, false) }).unwrap();
    let id = graphics(&b)[0].id().to_string();
    // as a design written on 2026-10-23 holds a left, bold text with a pen: in the kept style
    {
        let sym = b.design.symbol_library.as_mut().unwrap().by_lib_id_mut("Lib:S").unwrap();
        sym.kicad.text_styles.insert(id.clone(), eda_model::lib_keep::KeptTextStyle { thickness_mm: Some(0.3), bold: true, justify: vec!["left".into()], ..Default::default() });
    }
    // the dialog shows it as served (left and bold) and the user makes it right-justified and not bold
    let mut edited = text(FieldHAlign::Right, false);
    edited.set_id(id.clone());
    b.apply(&Cmd::ReplaceSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), graphic: edited.clone() }).unwrap();
    assert_eq!(graphics(&b)[0], edited);
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Lib:S").unwrap();
    let kept = sym.kicad.text_styles.get(&id).expect("the pen stays");
    assert_eq!((kept.thickness_mm, kept.bold, kept.justify.len()), (Some(0.3), false, 0), "what the dialog edits is no longer kept: {kept:?}");
}

/// UI-LEFT Y2: a bezier of the symbol is a graphic like any shape: added, moved by all four points, given a width and fill by Shape Properties.
#[test]
fn a_bezier_is_added_moved_and_given_its_width_and_fill() {
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    let p = |x: f64, y: f64| SPoint { x, y };
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: LibrarySymbolGraphic::Bezier { id: String::new(), unit: 0, body_style: 1, start: p(0.0, 0.0), c1: p(1.27, 2.54), c2: p(2.54, 2.54), end: p(3.81, 0.0), stroke_mm: 0.0, fill: LibraryFill::None } }).unwrap();
    let id = graphics(&b)[0].id().to_string();
    b.apply(&Cmd::MoveSymbolGraphic { lib_id: "Lib:S".into(), id: id.clone(), dx_mm: 1.27, dy_mm: -1.27 }).unwrap();
    b.apply(&Cmd::EditSymbolGraphic { lib_id: "Lib:S".into(), id, stroke_mm: 0.254, fill: LibraryFill::Background, unit: None, body_style: None }).unwrap();
    let LibrarySymbolGraphic::Bezier { start, c1, c2, end, stroke_mm, fill, .. } = &graphics(&b)[0] else { panic!("{:?}", graphics(&b)) };
    let near = |a: SPoint, b: SPoint| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
    assert!(near(*start, p(1.27, -1.27)) && near(*c1, p(2.54, 1.27)) && near(*c2, p(3.81, 1.27)) && near(*end, p(5.08, -1.27)));
    assert_eq!((*stroke_mm, *fill), (0.254, LibraryFill::Background));
}

/// UI-LEFT-2 YY2: `set_symbol_graphic_looks` puts a shape's Private flag, colours, hatch, radius and line style beside it as given (an empty id is the graphic just
/// added, as Duplicate's batch sends it); the defaults take the entries out; deleting the shape takes them with it.
#[test]
fn a_graphics_looks_and_line_style_are_set_as_given_and_go_with_the_graphic() {
    use eda_model::lib_keep::KeptGraphicLooks;
    use eda_model::sch_extras::SchColor;
    let m = ConstraintModel::default();
    let mut b = board(&m);
    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    let looks = KeptGraphicLooks { private: true, stroke_color: Some(SchColor { r: 1, g: 2, b: 3, a: 255 }), hatch: Some("cross_hatch".into()), radius_mm: 0.5, ..Default::default() };
    b.apply(&Cmd::Batch { cmds: vec![
        Cmd::AddSymbolGraphic { lib_id: "Lib:S".into(), graphic: rectangle() },
        Cmd::SetSymbolGraphicLooks { lib_id: "Lib:S".into(), id: String::new(), looks: looks.clone(), line_style: Some("dash".into()), thickness_mm: None },
    ] })
    .unwrap();
    let id = graphics(&b)[0].id().to_string();
    let sym = |b: &Board<'_>| b.design().symbol_library.as_ref().unwrap().by_lib_id("Lib:S").unwrap().clone();
    assert_eq!(sym(&b).kicad.graphic_looks.get(&id), Some(&looks));
    assert_eq!(sym(&b).kicad.stroke_types.get(&id).map(String::as_str), Some("dash"));
    assert!(sym(&b).to_engine_symbol().graphics.is_empty(), "a private shape is not drawn on a sheet");
    assert_eq!(refusal(&mut b, Cmd::SetSymbolGraphicLooks { lib_id: "Lib:S".into(), id: id.clone(), looks: KeptGraphicLooks { hatch: Some("dots".into()), ..Default::default() }, line_style: None, thickness_mm: None }), "ops_bad_shape: no such hatch");
    b.apply(&Cmd::SetSymbolGraphicLooks { lib_id: "Lib:S".into(), id: id.clone(), looks: KeptGraphicLooks::default(), line_style: None, thickness_mm: None }).unwrap();
    assert!(sym(&b).kicad.graphic_looks.is_empty() && sym(&b).kicad.stroke_types.is_empty());
    b.apply(&Cmd::SetSymbolGraphicLooks { lib_id: "Lib:S".into(), id: id.clone(), looks, line_style: Some("dot".into()), thickness_mm: None }).unwrap();
    b.apply(&Cmd::DeleteSymbolGraphic { lib_id: "Lib:S".into(), id }).unwrap();
    assert!(sym(&b).kicad.graphic_looks.is_empty() && sym(&b).kicad.stroke_types.is_empty(), "gone with the shape");
}
