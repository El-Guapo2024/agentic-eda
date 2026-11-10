//! "Edit Text & Graphics Properties...": the dimension items it now reaches, and "Set to layer default values" (`StyleFromSettings`).

use super::pcb_global_edit::{layer_class, line_thickness, text_size, text_thickness, LayerClass};
use super::pcb_transform_tests::{board, design, drawings, ids, model, p, pose, routing, text};
use super::*;
use eda_model::ir::{DimensionSettings, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, Shape};
use eda_model::rules::TextGraphicsDefaults;
use eda_model::units::mm;

/// A board with Board Setup's text and graphics defaults changed so every class reads differently from the shapes and the text of the fixture.
fn model_with_defaults() -> ConstraintModel {
    let mut m = model(&["F.Cu", "B.Cu"]);
    let d = &mut m.board.text_graphics;
    d.silk.line_width_nm = mm(0.12);
    d.silk.text_height_nm = mm(1.2);
    d.silk.text_width_nm = mm(1.2);
    d.silk.text_thickness_nm = mm(0.11);
    d.copper.line_width_nm = mm(0.21);
    d.copper.text_height_nm = mm(1.6);
    d.copper.text_width_nm = mm(1.6);
    d.copper.text_thickness_nm = mm(0.32);
    d.fab.line_width_nm = mm(0.09);
    d.fab.text_height_nm = mm(0.9);
    d.fab.text_width_nm = mm(0.9);
    d.fab.text_thickness_nm = mm(0.14);
    d.others.line_width_nm = mm(0.077);
    d.others.text_height_nm = mm(0.8);
    d.others.text_width_nm = mm(0.8);
    d.others.text_thickness_nm = mm(0.13);
    d.edge_cuts_line_width_nm = mm(0.055);
    d.courtyard_line_width_nm = mm(0.045);
    m
}

fn defaults_cmd(shapes: &[&str], texts: &[&str], dims: &[&str]) -> Cmd {
    Cmd::EditTextAndGraphics { shape_ids: ids(shapes), text_ids: ids(texts), dimension_ids: ids(dims), layer_defaults: true, layer: None, line_width: None, text_size: None, text_thickness: None, footprint: Default::default(), rows: Default::default() }
}

fn stroke(b: &Board<'_>, id: &str) -> Nm {
    drawings(b).shapes.iter().find(|s| s.id() == id).unwrap().stroke_width()
}

fn dim<'a>(b: &'a Board<'_>, id: &str) -> &'a eda_model::ir::Dimension {
    drawings(b).dimensions.iter().find(|d| d.id == id).unwrap()
}

#[test]
fn the_layer_class_is_what_board_design_settings_says() {
    assert_eq!(layer_class("F.SilkS"), LayerClass::Silk);
    assert_eq!(layer_class("B.SilkS"), LayerClass::Silk);
    assert_eq!(layer_class("In2.Cu"), LayerClass::Copper);
    assert_eq!(layer_class("B.Cu"), LayerClass::Copper);
    assert_eq!(layer_class("Edge.Cuts"), LayerClass::Edges);
    assert_eq!(layer_class("B.CrtYd"), LayerClass::Courtyard);
    assert_eq!(layer_class("F.Fab"), LayerClass::Fab);
    assert_eq!(layer_class("Dwgs.User"), LayerClass::Others);
    assert_eq!(layer_class("F.Mask"), LayerClass::Others);
}

#[test]
fn edges_and_courtyards_have_a_line_thickness_but_text_keeps_the_factory_constants() {
    let d = model_with_defaults().board.text_graphics;
    assert_eq!(line_thickness(&d, "Edge.Cuts"), mm(0.055));
    assert_eq!(line_thickness(&d, "F.CrtYd"), mm(0.045));
    assert_eq!((text_size(&d, "Edge.Cuts"), text_thickness(&d, "Edge.Cuts")), (mm(1.0), mm(0.15)), "`DEFAULT_TEXT_SIZE` and `DEFAULT_TEXT_WIDTH`: Board Setup has no text cells for these rows");
    assert_eq!((text_size(&d, "B.CrtYd"), text_thickness(&d, "B.CrtYd")), (mm(1.0), mm(0.15)));
    assert_eq!((text_size(&d, "F.SilkS"), text_thickness(&d, "F.SilkS")), (mm(1.2), mm(0.11)));
    assert_eq!(line_thickness(&TextGraphicsDefaults::default(), "F.SilkS"), mm(0.1));
}

#[test]
fn layer_default_values_give_a_graphic_item_its_layers_line_thickness() {
    let m = model_with_defaults();
    let mut b = board(&m);
    // shp_seg and shp_arc are on F.SilkS (150 now), shp_rect on F.Fab (100 now)
    b.apply(&defaults_cmd(&["shp_seg", "shp_arc", "shp_rect"], &[], &[])).unwrap();
    assert_eq!(stroke(&b, "shp_seg"), mm(0.12));
    assert_eq!(stroke(&b, "shp_arc"), mm(0.12));
    assert_eq!(stroke(&b, "shp_rect"), mm(0.09));
}

#[test]
fn only_the_items_named_are_restyled() {
    let m = model_with_defaults();
    let mut b = board(&m);
    b.apply(&defaults_cmd(&["shp_seg"], &[], &[])).unwrap();
    assert_eq!(stroke(&b, "shp_seg"), mm(0.12));
    assert_eq!(stroke(&b, "shp_arc"), mm(0.15), "not named");
    assert_eq!(stroke(&b, "shp_rect"), mm(0.1), "not named");
}

#[test]
fn layer_default_values_give_a_text_its_layers_size_and_thickness() {
    let m = model_with_defaults();
    let mut d = design();
    d.drawings.as_mut().unwrap().texts.extend([("t_cu", "F.Cu"), ("t_edge", "Edge.Cuts"), ("t_fab", "B.Fab")].map(|(id, layer)| text(id, layer, (mm(1.0), mm(1.0)), 0)));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&defaults_cmd(&[], &["txt_a", "t_cu", "t_edge", "t_fab"], &[])).unwrap();
    let t = |id: &str| drawings(&b).texts.iter().find(|t| t.id == id).map(|t| (t.size_nm, t.stroke_width)).unwrap();
    assert_eq!(t("txt_a"), (mm(1.2), mm(0.11)), "F.SilkS");
    assert_eq!(t("t_cu"), (mm(1.6), mm(0.32)), "copper");
    assert_eq!(t("t_edge"), (mm(1.0), mm(0.15)), "Edge.Cuts: the factory text");
    assert_eq!(t("t_fab"), (mm(0.9), mm(0.14)), "B.Fab");
    assert_eq!(drawings(&b).texts.iter().find(|t| t.id == "txt_a").unwrap().content, "T", "the words are not a style");
}

#[test]
fn layer_default_values_restyle_a_dimension_from_the_layer_and_the_boards_dimension_settings() {
    let m = model_with_defaults();
    let mut b = board(&m);
    // the fixture's dimension: Dwgs.User, line 150, text 1000, mm, no suffix, 2 decimals
    assert_eq!(dim(&b, "dim_a").precision, 2);
    b.apply(&Cmd::SetDimensionSettings {
        settings: DimensionSettings { units: DimensionUnits::Inch, units_format: DimensionUnitsFormat::BareSuffix, precision: 3, suppress_trailing_zeros: false, text_position: DimensionTextPosition::Inline, keep_text_aligned: false, ..DimensionSettings::default() },
    })
    .unwrap();
    b.apply(&defaults_cmd(&[], &[], &["dim_a"])).unwrap();
    let d = dim(&b, "dim_a");
    assert_eq!((d.stroke_width, d.text_size_nm, d.text_thickness_nm), (mm(0.077), mm(0.8), Some(mm(0.13))), "Dwgs.User is in the Others class");
    assert_eq!((d.units, d.units_format, d.precision), (DimensionUnits::Inch, DimensionUnitsFormat::BareSuffix, 3));
    assert_eq!((d.suppress_trailing_zeros, d.text_position, d.keep_text_aligned), (false, DimensionTextPosition::Inline, false));
    assert_eq!(d.layer, "Dwgs.User", "the layer is the one thing StyleFromSettings does not touch");
}

#[test]
fn specified_values_reach_a_dimension_and_leave_what_is_not_given() {
    let m = model_with_defaults();
    let mut b = board(&m);
    let before = dim(&b, "dim_a").clone();
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: Some("F.Fab".into()), line_width: Some(mm(0.25)), text_size: None, text_thickness: Some(mm(0.18)), footprint: Default::default(), rows: Default::default() }).unwrap();
    let d = dim(&b, "dim_a");
    assert_eq!(d.layer, "F.Fab");
    assert_eq!(d.stroke_width, mm(0.25));
    assert_eq!(d.text_thickness_nm, Some(mm(0.18)));
    assert_eq!(d.text_size_nm, before.text_size_nm, "no text size given");
    assert_eq!((d.start, d.end, d.precision, d.units), (before.start, before.end, before.precision, before.units));
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: None, text_size: Some(mm(1.3)), text_thickness: None, footprint: Default::default(), rows: Default::default() }).unwrap();
    assert_eq!(dim(&b, "dim_a").text_size_nm, mm(1.3));
    assert_eq!(dim(&b, "dim_a").stroke_width, mm(0.25));
}

/// `processItem` on a dimension (a `PCB_TEXT`, so an `EDA_TEXT`): the text width apart from the height, bold (`SetBold`: the bold pen of the smaller size in the
/// stroke font), italic and the font reach its label (`Dimension::text_effects`, UI-LEFT-2 PP11); a size given once more makes the label square again.
#[test]
fn specified_values_give_a_dimensions_label_its_width_bold_italic_and_font() {
    let m = model_with_defaults();
    let mut b = board(&m);
    let rows = TextRows { width: Some(mm(2.0)), height: Some(mm(1.5)), bold: Some(true), italic: Some(true), font: Some("DejaVu Sans".into()), ..Default::default() };
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: None, text_size: None, text_thickness: None, footprint: Default::default(), rows }).unwrap();
    let d = dim(&b, "dim_a");
    assert_eq!((d.text_size_nm, d.text_width_nm(), d.text_effects.width_nm), (mm(1.5), mm(2.0), Some(mm(2.0))));
    assert_eq!((d.text_effects.bold, d.text_effects.italic, d.text_effects.face.as_deref()), (true, true, Some("DejaVu Sans")));
    assert_eq!(d.text_thickness_nm, Some(mm(0.3)), "SetBold: the bold pen of the smaller size, a fifth of 1.5 mm");
    let rows = TextRows { width: Some(mm(1.5)), italic: Some(false), font: Some(super::pcb_global_edit::KICAD_FONT.into()), ..Default::default() };
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: None, text_size: None, text_thickness: None, footprint: Default::default(), rows }).unwrap();
    let d = dim(&b, "dim_a");
    assert_eq!((d.text_effects.width_nm, d.text_effects.italic, d.text_effects.face.clone(), d.text_effects.bold), (None, false, None, true));
}

#[test]
fn layer_defaults_ignore_the_specified_values_as_the_radio_buttons_do() {
    let m = model_with_defaults();
    let mut b = board(&m);
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: ids(&["shp_seg"]), text_ids: vec![], dimension_ids: vec![], layer_defaults: true, layer: Some("B.Fab".into()), line_width: Some(mm(0.999)), text_size: None, text_thickness: None, footprint: Default::default(), rows: Default::default() }).unwrap();
    assert_eq!(stroke(&b, "shp_seg"), mm(0.12), "the layer's default, not 999");
    assert!(matches!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap(), Shape::Segment { layer, .. } if layer == "F.SilkS"), "and still on its layer");
}

#[test]
fn nothing_named_is_refused_and_an_unknown_id_matches_nothing() {
    let m = model_with_defaults();
    let mut b = board(&m);
    assert_eq!(b.apply(&defaults_cmd(&[], &[], &[])).unwrap_err()[0].check, "ops_bad_global_edit");
    b.apply(&defaults_cmd(&["nope"], &[], &["nope"])).unwrap();
    assert_eq!(stroke(&b, "shp_seg"), mm(0.15), "nothing changed");
    // bad explicit values are refused for a dimension too
    assert_eq!(b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: Some(Nm::ZERO), text_size: None, text_thickness: None, footprint: Default::default(), rows: Default::default() }).unwrap_err()[0].check, "ops_bad_shape");
    assert_eq!(b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: None, text_size: None, text_thickness: Some(Nm::new(-1)), footprint: Default::default(), rows: Default::default() }).unwrap_err()[0].check, "ops_bad_text");
}

#[test]
fn the_verb_reads_the_payload_older_clients_send() {
    // `layer_defaults` and `dimension_ids` did not exist: a payload without them is still a specified-values edit
    let cmd: Cmd = serde_json::from_str(r#"{"op":"edit_text_and_graphics","shape_ids":["a"],"line_width":200000}"#).unwrap();
    match cmd {
        Cmd::EditTextAndGraphics { shape_ids, dimension_ids, layer_defaults, line_width, .. } => {
            assert_eq!((shape_ids, dimension_ids, layer_defaults, line_width), (ids(&["a"]), vec![], false, Some(mm(0.2))));
        }
        other => panic!("{other:?}"),
    }
    let cmd: Cmd = serde_json::from_str(r#"{"op":"edit_text_and_graphics","dimension_ids":["d"],"layer_defaults":true}"#).unwrap();
    assert!(matches!(cmd, Cmd::EditTextAndGraphics { layer_defaults: true, .. }));
}

/// round 3 D-9: `TransferDataFromWindow` takes text boxes and table cells with the texts (`PCB_TEXT_T || PCB_TEXTBOX_T`, `RunOnChildren`) and barcodes with
/// the graphics (`PCB_SHAPE_T || PCB_BARCODE_T`).
#[test]
fn text_boxes_table_cells_and_barcodes_are_edited_too() {
    use eda_model::pcb_items::TextBox;
    use eda_model::table::Table;
    let m = model_with_defaults();
    let mut d = design();
    let dr = d.drawings.as_mut().unwrap();
    let mut tb = TextBox::new("F.SilkS", p(Nm::ZERO, Nm::ZERO), p(mm(10.0), mm(5.0)), mm(1.0), mm(0.1));
    tb.id = "tbx_a".into();
    dr.board_items.text_boxes.push(tb);
    let mut t = Table::new_grid("F.SilkS", p(mm(20.0), mm(20.0)), 2, 1, mm(10.0), mm(5.0), mm(0.1));
    t.id = "tbl_a".into();
    dr.tables.push(t);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let set = Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: ids(&["tbx_a", "tbl_a:0:1"]), dimension_ids: vec![], layer_defaults: false, layer: None, line_width: Some(mm(0.3)), text_size: Some(mm(2.0)), text_thickness: Some(mm(0.25)), footprint: Default::default(), rows: Default::default() };
    b.apply(&set).unwrap();
    let tb = &drawings(&b).board_items.text_boxes[0];
    assert_eq!((tb.size_nm, tb.thickness_nm, tb.stroke_width_nm), (mm(2.0), mm(0.25), mm(0.3)), "a text box is a text and a shape");
    let cells = &drawings(&b).tables[0].cells;
    assert_eq!((cells[1].style.height_nm, cells[1].style.thickness_nm), (mm(2.0), mm(0.25)));
    assert_ne!(cells[0].style.height_nm, mm(2.0), "only the cell named");
    // layer defaults: the silkscreen class
    b.apply(&defaults_cmd(&[], &["tbx_a", "tbl_a:0:0"], &[])).unwrap();
    let tb = &drawings(&b).board_items.text_boxes[0];
    assert_eq!((tb.size_nm, tb.thickness_nm, tb.stroke_width_nm), (mm(1.2), mm(0.11), mm(0.12)));
    assert_eq!(drawings(&b).tables[0].cells[0].style.height_nm, mm(1.2));
}

/// Round 4 B-3: `PCB_TEXT::StyleFromSettings` (and `PCB_TEXTBOX`'s, a table cell's) sets `GetTextSize( layer )`, a (width, height), and
/// `SetItalic( GetTextItalic( layer ) )`: the text is not made square at the class's height, and italic follows the class.
#[test]
fn layer_defaults_take_the_classs_width_height_and_italic() {
    use eda_model::pcb_items::TextBox;
    use eda_model::table::Table;
    let mut m = model_with_defaults();
    m.board.text_graphics.silk.text_width_nm = mm(1.2);
    m.board.text_graphics.silk.text_height_nm = mm(1.5);
    m.board.text_graphics.silk.italic = true;
    let mut d = design();
    let dr = d.drawings.as_mut().unwrap();
    let mut tb = TextBox::new("F.SilkS", p(Nm::ZERO, Nm::ZERO), p(mm(10.0), mm(5.0)), mm(1.0), mm(0.1));
    tb.id = "tbx_a".into();
    dr.board_items.text_boxes.push(tb);
    let mut t = Table::new_grid("F.SilkS", p(mm(20.0), mm(20.0)), 2, 1, mm(10.0), mm(5.0), mm(0.1));
    t.id = "tbl_a".into();
    dr.tables.push(t);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&defaults_cmd(&[], &["txt_a", "tbx_a", "tbl_a:0:0"], &[])).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.texts.iter().find(|t| t.id == "txt_a").unwrap().size_nm, mm(1.5), "the height");
    let x = dr.text_extra("txt_a").expect("the width and italic land on the text's extras");
    assert_eq!((x.width_nm, x.italic), (Some(mm(1.2)), true));
    let tb = &dr.board_items.text_boxes[0];
    assert_eq!((tb.size_nm, tb.width_nm, tb.italic), (mm(1.5), Some(mm(1.2)), true));
    let cell = &dr.tables[0].cells[0].style;
    assert_eq!((cell.height_nm, cell.width_nm, cell.italic), (mm(1.5), mm(1.2), true));

    // Italic off in the class: an italic text is made upright.
    let mut m2 = m.clone();
    m2.board.text_graphics.silk.italic = false;
    m2.board.text_graphics.silk.text_width_nm = mm(1.5);
    let mut b2 = Board::new(b.design().clone(), &m2, mm(0.1), mm(0.3));
    b2.apply(&defaults_cmd(&[], &["txt_a", "tbx_a"], &[])).unwrap();
    let dr = drawings(&b2);
    assert!(dr.text_extra("txt_a").map_or(true, |x| !x.italic && x.width_nm.is_none()), "square and upright: nothing left to say");
    assert_eq!((dr.board_items.text_boxes[0].width_nm, dr.board_items.text_boxes[0].italic), (None, false));
}

// ------------------------------------------------------------------------------------------------ the footprints' items (UI-LEFT P12)

use super::pcb_global_edit::{FootprintItems, Ipc4761Preset, TextRows};
use eda_model::fp_art::{FpShape, FpText};
use eda_model::fp_edit::FieldLayout;

/// The fixture's model with art on its footprint (both U1 and U2 are on it): a silkscreen line, a fab rectangle, a fab `${REFERENCE}` and a user text.
fn model_with_art() -> ConstraintModel {
    let mut m = model_with_defaults();
    let art = &mut m.footprints[0].art;
    art.shapes.push(FpShape::new(Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: p(mm(-2.0), mm(-1.5)), end: p(mm(2.0), mm(-1.5)) }));
    art.shapes.push(FpShape::new(Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, start: p(mm(-2.0), mm(-1.0)), end: p(mm(2.0), mm(1.0)) }));
    art.texts.push(FpText::new("${REFERENCE}", p(Nm::ZERO, Nm::ZERO), "F.Fab"));
    art.texts.push(FpText::new("note", p(mm(3.0), mm(3.0)), "F.SilkS"));
    m
}

fn fp_cmd(footprint: FootprintItems, rows: TextRows, layer: Option<&str>, line_width: Option<Nm>, thickness: Option<Nm>) -> Cmd {
    Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: vec![], layer_defaults: false, layer: layer.map(str::to_string), line_width, text_size: None, text_thickness: thickness, footprint, rows }
}

fn fields(list: &[&str]) -> FootprintItems {
    FootprintItems { field_ids: ids(list), ..Default::default() }
}

/// The footprint U1 is on now: the board's own copy when it has one.
fn own_footprint(b: &Board<'_>, r: &str) -> Option<Footprint> {
    b.design().footprint_assignment(r).and_then(|a| a.definition.clone())
}

#[test]
fn a_footprints_reference_and_value_take_the_text_rows_and_another_footprint_is_left() {
    let m = model_with_art();
    let mut b = board(&m);
    let rows = TextRows { width: Some(mm(1.4)), height: Some(mm(0.9)), bold: Some(true), italic: Some(true), font: Some("Arial".into()), keep_upright: Some(false), visible: Some(false), ..Default::default() };
    b.apply(&fp_cmd(fields(&["U1:Reference", "U1:Value"]), rows, Some("F.Fab"), None, Some(mm(0.12)))).unwrap();
    let e = b.design().footprint_edit("U1").unwrap();
    for l in [e.reference.as_ref().unwrap(), e.value.as_ref().unwrap()] {
        assert_eq!((l.size, l.thickness, l.layer.as_str()), ((mm(1.4), mm(0.9)), mm(0.12), "F.Fab"));
        assert_eq!((l.bold, l.italic, l.keep_upright, l.visible, l.face.as_deref()), (true, true, false, false, Some("Arial")));
    }
    assert!(b.design().footprint_edit("U2").is_none(), "U2 was not named");
    assert!(own_footprint(&b, "U1").is_none(), "a field is no art: the footprint stays the library's");

    // "KiCad Font" is the stroke font: no face; "auto" thickness is 0 (`SetAutoThickness( true )`)
    b.apply(&fp_cmd(fields(&["U1:Reference"]), TextRows { font: Some("KiCad Font".into()), auto_thickness: true, ..Default::default() }, None, None, Some(mm(0.2)))).unwrap();
    let l = b.design().footprint_edit("U1").unwrap().reference.clone().unwrap();
    assert_eq!((l.face, l.thickness, l.size), (None, Nm::ZERO, (mm(1.4), mm(0.9))), "auto wins over a thickness, the rest is left");
}

#[test]
fn a_user_field_is_edited_in_place_and_an_unknown_field_matches_nothing() {
    let m = model_with_art();
    let mut b = board(&m);
    let vendor = eda_model::fp_edit::UserField { name: "Vendor".into(), text: "ACME".into(), layout: FieldLayout::new(p(Nm::ZERO, mm(2.0)), "F.Fab") };
    b.apply(&Cmd::EditBoardFootprint { part: "U2".into(), reference: None, value: None, fields: Some(vec![vendor]), attrs: None }).unwrap();
    b.apply(&fp_cmd(fields(&["U2:Vendor", "U2:Nope", "U9:Reference"]), TextRows { visible: Some(false), height: Some(mm(2.0)), ..Default::default() }, None, None, None)).unwrap();
    let e = b.design().footprint_edit("U2").unwrap();
    assert_eq!((e.fields.len(), e.fields[0].text.as_str(), e.fields[0].layout.visible, e.fields[0].layout.size.1), (1, "ACME", false, mm(2.0)));
    assert!(e.reference.is_none() && e.value.is_none(), "only the field named");
}

#[test]
fn a_footprints_own_texts_and_graphics_go_into_its_own_copy_of_the_footprint() {
    let m = model_with_art();
    let mut b = board(&m);
    let items = FootprintItems { text_ids: ids(&["U1~t1"]), shape_ids: ids(&["U1~g0"]), ..Default::default() };
    let rows = TextRows { bold: Some(true), font: Some("Arial".into()), height: Some(mm(1.5)), ..Default::default() };
    b.apply(&fp_cmd(items, rows, Some("F.Fab"), Some(mm(0.3)), None)).unwrap();
    let own = own_footprint(&b, "U1").expect("U1 has its own footprint now");
    assert_eq!(b.design().footprint_assignment("U1").unwrap().footprint, "LOPSIDED", "under the name it had");
    assert_eq!((own.art.shapes[0].shape.stroke_width(), own.art.shapes[0].layer()), (mm(0.3), "F.Fab"));
    assert_eq!(own.art.shapes[1], m.footprints[0].art.shapes[1], "the graphic not named is the library's");
    let t = &own.art.texts[1];
    assert_eq!((t.layout.layer.as_str(), t.layout.bold, t.layout.size, t.face.as_deref()), ("F.Fab", true, (mm(1.0), mm(1.5)), Some("Arial")));
    assert_eq!(own.art.texts[0], m.footprints[0].art.texts[0]);
    assert_eq!(own.pads, m.footprints[0].pads, "the pads are the footprint's");
    assert!(own_footprint(&b, "U2").is_none(), "U2 is on the same library footprint and keeps it");
    assert_eq!(m.footprints[0].art.shapes[0].shape.stroke_width(), mm(0.12));

    // a second edit works on the copy
    b.apply(&fp_cmd(FootprintItems { shape_ids: ids(&["U1~g1"]), ..Default::default() }, TextRows::default(), None, Some(mm(0.05)), None)).unwrap();
    let own = own_footprint(&b, "U1").unwrap();
    assert_eq!((own.art.shapes[0].shape.stroke_width(), own.art.shapes[1].shape.stroke_width()), (mm(0.3), mm(0.05)));
}

#[test]
fn a_bottom_side_footprints_art_keeps_its_layers_as_for_the_top_side() {
    let m = model_with_art();
    let mut d = design();
    d.placement.as_mut().unwrap().footprints[0].side = Side::Bottom;
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    // "B.Fab" on the board is the top-side frame's "F.Fab"
    b.apply(&fp_cmd(FootprintItems { text_ids: ids(&["U1~t1"]), shape_ids: ids(&["U1~g0"]), ..Default::default() }, TextRows::default(), Some("B.Fab"), None, None)).unwrap();
    let own = own_footprint(&b, "U1").unwrap();
    assert_eq!((own.art.shapes[0].layer(), own.art.texts[1].layout.layer.as_str()), ("F.Fab", "F.Fab"));
}

#[test]
fn center_on_footprint_puts_the_texts_at_the_centre_of_the_footprints_box() {
    let m = model_with_art();
    let mut b = board(&m);
    let rows = TextRows { center_on_footprint: true, ..Default::default() };
    b.apply(&fp_cmd(FootprintItems { field_ids: ids(&["U2:Reference"]), text_ids: ids(&["U2~t1"]), ..Default::default() }, rows, None, None, None)).unwrap();
    // U2 is at (60, 20) turned 90 degrees; its box (pads, courtyard, art but the texts) is centred on its origin: the courtyard is symmetric about it
    let at = pose(&b, "U2");
    let reference = b.design().footprint_edit("U2").unwrap().reference.clone().unwrap();
    assert_eq!(reference.board_position(&at), p(mm(60.0), mm(20.0)));
    let own = own_footprint(&b, "U2").unwrap();
    assert_eq!(own.art.texts[1].layout.board_position(&at), p(mm(60.0), mm(20.0)));
    assert_eq!(own.art.texts[1].layout.board_angle(&at), m.footprints[0].art.texts[1].layout.board_angle(&at), "the angle is left");
}

#[test]
fn layer_defaults_restyle_a_footprints_fields_texts_and_graphics() {
    let mut m = model_with_art();
    m.board.text_graphics.fab.upright = true;
    m.board.text_graphics.fab.italic = true;
    let mut b = board(&m);
    let items = FootprintItems { field_ids: ids(&["U1:Value"]), text_ids: ids(&["U1~t0"]), shape_ids: ids(&["U1~g0", "U1~g1"]) };
    let cmd = Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: vec![], layer_defaults: true, layer: Some("F.Cu".into()), line_width: Some(mm(1.0)), text_size: None, text_thickness: None, footprint: items, rows: TextRows { bold: Some(true), ..Default::default() } };
    b.apply(&cmd).unwrap();
    // the value is on F.Fab: the fab class (0.9 mm, 0.14 mm, italic, upright); the specified values are not looked at
    let v = b.design().footprint_edit("U1").unwrap().value.clone().unwrap();
    assert_eq!((v.size, v.thickness, v.italic, v.keep_upright, v.bold, v.layer.as_str()), ((mm(0.9), mm(0.9)), mm(0.14), true, true, false, "F.Fab"));
    let own = own_footprint(&b, "U1").unwrap();
    assert_eq!((own.art.shapes[0].shape.stroke_width(), own.art.shapes[1].shape.stroke_width()), (mm(0.12), mm(0.09)), "silk, then fab line thickness");
    let t = &own.art.texts[0].layout;
    assert_eq!((t.size, t.thickness, t.italic, t.keep_upright), ((mm(0.9), mm(0.9)), mm(0.14), true, true));
}

#[test]
fn a_board_text_takes_a_width_apart_from_its_height_bold_italic_a_face_and_auto_thickness() {
    let m = model_with_defaults();
    let mut b = board(&m);
    let set = |rows: TextRows, thickness: Option<Nm>| Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: ids(&["txt_a"]), dimension_ids: vec![], layer_defaults: false, layer: None, line_width: None, text_size: None, text_thickness: thickness, footprint: Default::default(), rows };
    b.apply(&set(TextRows { width: Some(mm(2.0)), bold: Some(true), italic: Some(true), font: Some("Arial".into()), ..Default::default() }, None)).unwrap();
    let t = drawings(&b).texts.iter().find(|t| t.id == "txt_a").unwrap().clone();
    let x = drawings(&b).text_extra("txt_a").unwrap().clone();
    assert_eq!((t.size_nm, x.width_nm, x.bold, x.italic, x.face.as_deref()), (mm(1.0), Some(mm(2.0)), true, true, Some("Arial")));
    // auto: `GetPenSizeForBold` of the smaller size (1 mm / 5)
    b.apply(&set(TextRows { auto_thickness: true, height: Some(mm(3.0)), ..Default::default() }, Some(mm(0.4)))).unwrap();
    let t = drawings(&b).texts.iter().find(|t| t.id == "txt_a").unwrap().clone();
    let x = drawings(&b).text_extra("txt_a").unwrap().clone();
    assert_eq!((t.size_nm, x.width_nm, x.auto_thickness, t.stroke_width), (mm(3.0), Some(mm(2.0)), true, mm(0.4)));
    // back to a square, regular, stroke-font text: the extra says nothing and goes
    b.apply(&set(TextRows { width: Some(mm(3.0)), bold: Some(false), italic: Some(false), font: Some("KiCad Font".into()), ..Default::default() }, Some(mm(0.3)))).unwrap();
    assert!(drawings(&b).text_extra("txt_a").is_none(), "{:?}", drawings(&b).text_extras);
    assert_eq!(drawings(&b).texts.iter().find(|t| t.id == "txt_a").unwrap().stroke_width, mm(0.3));
    // a size of zero is refused
    assert_eq!(b.apply(&set(TextRows { height: Some(Nm::ZERO), ..Default::default() }, None)).unwrap_err()[0].check, "ops_bad_text");
}

#[test]
fn the_footprint_items_and_rows_read_from_the_studios_json() {
    let cmd: Cmd = serde_json::from_value(serde_json::json!({
        "op": "edit_text_and_graphics", "layer": "F.Fab", "text_thickness": 120000,
        "footprint": { "field_ids": ["U1:Reference"], "text_ids": ["U1~t0"], "shape_ids": ["U1~g1"] },
        "rows": { "width": 1400000, "height": 900000, "auto_thickness": true, "bold": true, "italic": false, "font": "Arial", "keep_upright": false, "visible": true, "center_on_footprint": true }
    }))
    .unwrap();
    let Cmd::EditTextAndGraphics { footprint, rows, text_thickness, .. } = cmd else { panic!() };
    assert_eq!((footprint.field_ids, footprint.text_ids, footprint.shape_ids), (ids(&["U1:Reference"]), ids(&["U1~t0"]), ids(&["U1~g1"])));
    assert_eq!(rows, TextRows { width: Some(mm(1.4)), height: Some(mm(0.9)), auto_thickness: true, bold: Some(true), italic: Some(false), font: Some("Arial".into()), keep_upright: Some(false), visible: Some(true), center_on_footprint: true });
    assert_eq!(text_thickness, Some(mm(0.12)));
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "edit_tracks_and_vias", "ids": ["via_a"], "unconnected": "remove_all", "protection": "iia_inverted" })).unwrap();
    assert!(matches!(cmd, Cmd::EditTracksAndVias { unconnected: Some(eda_model::padstack::UnconnectedLayers::RemoveAll), protection: Some(Ipc4761Preset::IiaInverted), .. }));
}

// ------------------------------------------------------------------------------------------------ Edit Track & Via Properties' via rows

fn via_cmd(unconnected: Option<eda_model::padstack::UnconnectedLayers>, protection: Option<Ipc4761Preset>) -> Cmd {
    Cmd::EditTracksAndVias { ids: ids(&["via_a", "trk_a"]), track_width: None, via_size: None, layer: None, unconnected, protection }
}

#[test]
fn the_ipc_4761_presets_are_kicads_list_in_its_order() {
    let names: Vec<&str> = Ipc4761Preset::ALL.iter().map(|p| p.name()).collect();
    assert_eq!(names.len(), 19);
    assert_eq!(&names[..4], &["From rules", "None", "Type I-a (tented top)", "Type I-b (tented both sides)"]);
    assert_eq!(names[14], "Type V (filled )", "KiCad's own spelling");
    assert_eq!(names[18], "Type VII (filled and capped)");
    // each name once, and the serde words are snake case
    assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), 19);
    assert_eq!(serde_json::to_value(Ipc4761Preset::IiiaInverted).unwrap(), serde_json::json!("iiia_inverted"));
}

#[test]
fn a_preset_sets_every_feature_of_the_via_and_from_rules_clears_them() {
    let m = model_with_defaults();
    let mut b = board(&m);
    b.apply(&via_cmd(None, Some(Ipc4761Preset::Ivb))).unwrap();
    let props = drawings(&b).via_extra("via_a").unwrap().props();
    // IV-b: plugged and tented both sides, nothing covered, not filled, not capped
    assert_eq!((props.front_tenting, props.back_tenting, props.front_plugging, props.back_plugging), (Some(true), Some(true), Some(true), Some(true)));
    assert_eq!((props.front_covering, props.back_covering, props.filling, props.capping), (Some(false), Some(false), Some(false), Some(false)));
    let tent = drawings(&b).via_tenting.iter().find(|t| t.net == "GND").unwrap();
    assert_eq!((tent.front, tent.back), (Some(true), Some(true)), "the DRC's tenting table follows");

    b.apply(&via_cmd(None, Some(Ipc4761Preset::ViaInverted))).unwrap();
    let props = drawings(&b).via_extra("via_a").unwrap().props();
    assert_eq!((props.front_tenting, props.back_tenting, props.filling, props.front_plugging), (Some(false), Some(true), Some(true), Some(false)));

    b.apply(&via_cmd(None, Some(Ipc4761Preset::FromBoard))).unwrap();
    assert!(drawings(&b).via_extra("via_a").is_none(), "From rules leaves the via with KiCad's defaults: no extra");
    assert!(drawings(&b).via_tenting.is_empty());
}

#[test]
fn the_annular_ring_row_sets_the_unconnected_layer_mode_of_every_via_named() {
    let m = model_with_defaults();
    let mut b = board(&m);
    use eda_model::padstack::UnconnectedLayers as U;
    b.apply(&via_cmd(Some(U::RemoveExceptStartAndEnd), None)).unwrap();
    assert_eq!(drawings(&b).via_extra("via_a").unwrap().unconnected_layers(), U::RemoveExceptStartAndEnd);
    // with a preset in the same step: both
    b.apply(&via_cmd(Some(U::StartEndOnly), Some(Ipc4761Preset::Vii))).unwrap();
    let x = drawings(&b).via_extra("via_a").unwrap().clone();
    assert_eq!((x.unconnected_layers(), x.props().filling, x.props().capping), (U::StartEndOnly, Some(true), Some(true)));
    // the sizes are left
    let v = routing(&b).vias.iter().find(|v| v.id == "via_a").unwrap();
    assert_eq!((v.diameter, v.drill), (mm(0.6), mm(0.3)));
    b.apply(&via_cmd(Some(U::KeepAll), Some(Ipc4761Preset::FromBoard))).unwrap();
    assert!(drawings(&b).via_extra("via_a").is_none());
}

/// UI-LEFT P15: a footprint's own text is an item of the board (`PCB_SELECTION_TOOL::Selectable` lets a footprint's `PCB_TEXT` be picked): moved, turned and
/// flipped on its own, edited in Text Properties and deleted, each in the instance's own copy of its footprint; the other footprint on the same library
/// footprint is left as it was, and a move of the footprint carries the text with it as before.
#[test]
fn a_footprints_own_text_is_moved_turned_edited_and_deleted_on_its_own() {
    let m = model_with_art();
    let mut b = board(&m);
    let pose = b.design().placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U1").unwrap().clone();
    let before = m.footprints[0].art.texts[1].layout.board_position(&pose);
    b.apply(&Cmd::MoveItems { ids: ids(&["U1~t1"]), dx: mm(1.0), dy: mm(-2.0) }).unwrap();
    let own = own_footprint(&b, "U1").expect("U1 has its own copy now");
    assert_eq!(own.art.texts[1].layout.board_position(&pose), p(before.x + mm(1.0), before.y - mm(2.0)));
    assert!(own_footprint(&b, "U2").is_none(), "U2 is still on the library footprint");
    // turned a quarter about its own position: the angle, not the place
    let at = own.art.texts[1].layout.board_position(&pose);
    b.apply(&Cmd::RotateItems { ids: ids(&["U1~t1"]), pivot: at, angle_millideg: 90_000 }).unwrap();
    let t = own_footprint(&b, "U1").unwrap().art.texts[1].layout.clone();
    assert_eq!((t.board_position(&pose), t.board_angle(&pose)), (at, 270_000));
    // flipped: to the other side's layer, mirrored
    b.apply(&Cmd::FlipItems { ids: ids(&["U1~t1"]), pivot: at, direction: FlipDirection::LeftRight }).unwrap();
    let t = own_footprint(&b, "U1").unwrap().art.texts[1].layout.clone();
    assert_eq!((t.layer.as_str(), t.mirror), ("B.SilkS", true));
    // Text Properties: its text and its layout (the layer as the board shows it)
    let mut l = t.clone();
    l.layer = "F.Fab".into();
    l.size = (mm(0.8), mm(0.8));
    b.apply(&Cmd::EditBoardFootprintText { id: "U1~t1".into(), text: Some("pin 1".into()), layout: Some(l), delete: false }).unwrap();
    let t = own_footprint(&b, "U1").unwrap().art.texts[1].clone();
    assert_eq!((t.text.as_str(), t.layout.layer.as_str(), t.layout.size), ("pin 1", "F.Fab", (mm(0.8), mm(0.8))));
    assert_eq!(b.apply(&Cmd::EditBoardFootprintText { id: "U1~t1".into(), text: Some(" ".into()), layout: None, delete: false }).unwrap_err()[0].hint.as_deref(), Some("The text can't be empty."));
    assert_eq!(b.apply(&Cmd::EditBoardFootprintText { id: "U1~t1".into(), text: Some("pin 1".into()), layout: None, delete: false }).unwrap_err()[0].check, "ops_nothing_to_change");
    // deleted
    b.apply(&Cmd::EditBoardFootprintText { id: "U1~t1".into(), text: None, layout: None, delete: true }).unwrap();
    assert_eq!(own_footprint(&b, "U1").unwrap().art.texts.len(), 1);
    assert_eq!(b.apply(&Cmd::EditBoardFootprintText { id: "U1~t1".into(), text: Some("x".into()), layout: None, delete: false }).unwrap_err()[0].check, "ops_unknown_item");
    // the footprint moved carries its texts (the text is not moved twice)
    let t0 = own_footprint(&b, "U1").unwrap().art.texts[0].layout.clone();
    b.apply(&Cmd::MoveItems { ids: ids(&["U1", "U1~t0"]), dx: mm(5.0), dy: Nm::ZERO }).unwrap();
    assert_eq!(own_footprint(&b, "U1").unwrap().art.texts[0].layout, t0, "in the footprint's frame, the text is where it was");
}

// ------------------------------------------------------------------------------------------------ UI-LEFT-2 PP23: hidden texts, courtyards, the footprints' items

/// `fp->GraphicalItems()` holds a hidden text as it holds a shown one: "Footprint text items" reaches it (the studio sends its id, `REF~t<k>`).
#[test]
fn a_footprints_hidden_text_is_reached_as_a_shown_one() {
    let mut m = model_with_art();
    let mut hidden = FpText::new("${REFERENCE}", p(Nm::ZERO, mm(1.0)), "F.Fab");
    hidden.layout.visible = false;
    m.footprints[0].art.texts.push(hidden);
    let mut b = board(&m);
    b.apply(&fp_cmd(FootprintItems { text_ids: ids(&["U1~t2"]), ..Default::default() }, TextRows { height: Some(mm(1.7)), ..Default::default() }, None, None, None)).unwrap();
    let own = own_footprint(&b, "U1").unwrap();
    assert_eq!((own.art.texts[2].layout.size.1, own.art.texts[2].layout.visible), (mm(1.7), false), "edited, and still hidden");
}

/// A library footprint's courtyard graphics (`FpArt::courtyard`) are `PCB_SHAPE`s of the footprint: "Footprint graphic items" sets their width and layer, and
/// "Set to layer default values" gives them the courtyard class's line thickness.
#[test]
fn a_library_footprints_courtyard_graphics_take_the_graphic_rows() {
    let mut m = model_with_art();
    m.footprints[0].art.courtyard.push(FpShape::new(Shape::Rect { id: String::new(), layer: "F.CrtYd".into(), stroke_width: mm(0.05), filled: false, start: p(mm(-3.0), mm(-2.0)), end: p(mm(3.0), mm(2.0)) }));
    let mut b = board(&m);
    let part = m.part("U1").unwrap().clone();
    let at = pose(&b, "U1");
    assert_eq!(super::courtyard_graphics(b.design(), &m, &part, &at, &m.footprints[0]), vec![("U1~c0".to_string(), "F.CrtYd".to_string())]);
    b.apply(&fp_cmd(FootprintItems { shape_ids: ids(&["U1~c0"]), ..Default::default() }, TextRows::default(), None, Some(mm(0.07)), None)).unwrap();
    assert_eq!(own_footprint(&b, "U1").unwrap().art.courtyard[0].shape.stroke_width(), mm(0.07));
    let defaults = Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: vec![], layer_defaults: true, layer: None, line_width: None, text_size: None, text_thickness: None, footprint: FootprintItems { shape_ids: ids(&["U1~c0"]), ..Default::default() }, rows: TextRows::default() };
    b.apply(&defaults).unwrap();
    assert_eq!(own_footprint(&b, "U1").unwrap().art.courtyard[0].shape.stroke_width(), mm(0.045), "the courtyard class");
}

/// The courtyard items of the board file (`FootprintExtra::courtyard_items`) are edited in their text while the footprint is written with them.
#[test]
fn a_board_files_courtyard_items_are_edited_in_their_text() {
    let m = model_with_art();
    let mut d = design();
    let part = m.part("U1").unwrap().clone();
    let written = eda_model::fp_items_place::written_name(&m, &part);
    let item = r#"(fp_rect (start -3 -2) (end 3 2) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd") (uuid "c1"))"#;
    let dr = d.drawings.as_mut().unwrap();
    dr.footprint_extras.push(eda_model::ir::FootprintExtra { id: "U1".into(), source_name: written.clone(), courtyard_items: vec![item.into()], ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let at = pose(&b, "U1");
    assert_eq!(super::courtyard_graphics(b.design(), &m, &part, &at, &m.footprints[0]), vec![("U1~c0".to_string(), "F.CrtYd".to_string())]);
    b.apply(&fp_cmd(FootprintItems { shape_ids: ids(&["U1~c0"]), ..Default::default() }, TextRows::default(), Some("B.CrtYd"), Some(mm(0.1)), None)).unwrap();
    let extra = drawings(&b).footprint_extras.iter().find(|x| x.id == "U1").unwrap();
    assert_eq!(extra.courtyard_items[0], r#"(fp_rect (start -3 -2) (end 3 2) (stroke (width 0.1) (type solid)) (fill none) (layer "B.CrtYd") (uuid "c1"))"#);
    assert!(own_footprint(&b, "U1").is_none(), "the footprint is left the library's");
}

/// A footprint's own dimensions, text boxes and barcodes (`FootprintExtra::items`, named by their board ids) take the rows a board item of their kind takes; a
/// dimension's label also "Keep upright", and on a footprint on the back a layer asked for is kept as for the top side.
#[test]
fn a_footprints_own_dimensions_and_text_boxes_take_the_rows() {
    use eda_model::pcb_items::TextBox;
    let m = model_with_defaults();
    let mut d = design();
    d.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "U1").unwrap().side = Side::Bottom;
    let dr = d.drawings.as_mut().unwrap();
    let mut dim = dr.dimensions[0].clone();
    dim.layer = "F.Fab".into();
    let mut tb = TextBox::new("F.SilkS", p(Nm::ZERO, Nm::ZERO), p(mm(4.0), mm(2.0)), mm(1.0), mm(0.1));
    tb.id = "tbx_1".into();
    let mut items = eda_model::fp_items::FootprintItems::default();
    items.dimensions.push(dim);
    items.text_boxes.push(tb);
    dr.footprint_extras.push(eda_model::ir::FootprintExtra { id: "U1".into(), items, ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let cmd = Cmd::EditTextAndGraphics {
        shape_ids: vec![],
        text_ids: ids(&["U1~tbx_1"]),
        dimension_ids: ids(&["U1~dim_a"]),
        layer_defaults: false,
        layer: Some("B.Fab".into()),
        line_width: Some(mm(0.2)),
        text_size: None,
        text_thickness: None,
        footprint: Default::default(),
        rows: TextRows { height: Some(mm(1.3)), keep_upright: Some(false), ..Default::default() },
    };
    b.apply(&cmd).unwrap();
    let items = &drawings(&b).footprint_extras.iter().find(|x| x.id == "U1").unwrap().items;
    let dim = &items.dimensions[0];
    assert_eq!((dim.layer.as_str(), dim.stroke_width, dim.text_size_nm, dim.text_effects.keep_upright), ("F.Fab", mm(0.2), mm(1.3), false), "B.Fab on the back is F.Fab in the footprint's frame");
    let tb = &items.text_boxes[0];
    assert_eq!((tb.layer.as_str(), tb.size_nm, tb.stroke_width_nm), ("F.Fab", mm(1.3), mm(0.2)));
    assert_eq!(drawings(&b).dimensions[0].stroke_width, design().drawings.unwrap().dimensions[0].stroke_width, "the board's dim_a is another item");

    // layer defaults: the class of the layer on the board (B.Fab: fab), keep upright from it for a footprint's dimension
    b.apply(&defaults_cmd(&[], &["U1~tbx_1"], &["U1~dim_a"])).unwrap();
    let items = &drawings(&b).footprint_extras.iter().find(|x| x.id == "U1").unwrap().items;
    assert_eq!((items.dimensions[0].stroke_width, items.dimensions[0].text_size_nm, items.dimensions[0].text_effects.keep_upright), (mm(0.09), mm(0.9), m.board.text_graphics.fab.upright));
    assert_eq!(items.text_boxes[0].size_nm, mm(0.9));
}
