//! "Edit Text & Graphics Properties...": the dimension items it now reaches, and "Set to layer default values" (`StyleFromSettings`).

use super::pcb_global_edit::{layer_class, line_thickness, text_size, text_thickness, LayerClass};
use super::pcb_transform_tests::{board, design, drawings, ids, model, p, text};
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
    Cmd::EditTextAndGraphics { shape_ids: ids(shapes), text_ids: ids(texts), dimension_ids: ids(dims), layer_defaults: true, layer: None, line_width: None, text_size: None, text_thickness: None }
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
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: Some("F.Fab".into()), line_width: Some(mm(0.25)), text_size: None, text_thickness: Some(mm(0.18)) }).unwrap();
    let d = dim(&b, "dim_a");
    assert_eq!(d.layer, "F.Fab");
    assert_eq!(d.stroke_width, mm(0.25));
    assert_eq!(d.text_thickness_nm, Some(mm(0.18)));
    assert_eq!(d.text_size_nm, before.text_size_nm, "no text size given");
    assert_eq!((d.start, d.end, d.precision, d.units), (before.start, before.end, before.precision, before.units));
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: None, text_size: Some(mm(1.3)), text_thickness: None }).unwrap();
    assert_eq!(dim(&b, "dim_a").text_size_nm, mm(1.3));
    assert_eq!(dim(&b, "dim_a").stroke_width, mm(0.25));
}

#[test]
fn layer_defaults_ignore_the_specified_values_as_the_radio_buttons_do() {
    let m = model_with_defaults();
    let mut b = board(&m);
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: ids(&["shp_seg"]), text_ids: vec![], dimension_ids: vec![], layer_defaults: true, layer: Some("B.Fab".into()), line_width: Some(mm(0.999)), text_size: None, text_thickness: None }).unwrap();
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
    assert_eq!(b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: Some(Nm::ZERO), text_size: None, text_thickness: None }).unwrap_err()[0].check, "ops_bad_shape");
    assert_eq!(b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: ids(&["dim_a"]), layer_defaults: false, layer: None, line_width: None, text_size: None, text_thickness: Some(Nm::new(-1)) }).unwrap_err()[0].check, "ops_bad_text");
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
    let set = Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: ids(&["tbx_a", "tbl_a:0:1"]), dimension_ids: vec![], layer_defaults: false, layer: None, line_width: Some(mm(0.3)), text_size: Some(mm(2.0)), text_thickness: Some(mm(0.25)) };
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
