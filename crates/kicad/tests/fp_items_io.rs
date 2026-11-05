//! A library footprint's dimensions, zones, text boxes, points, reference images, barcodes, tables and groups (`eda_model::fp_items`, UI-LEFT F2) through the
//! Footprint Editor's reader and writer: [`parse_library_footprint`] reads each with the board's reader of the item (`parseFOOTPRINT_unchecked` hands each to the
//! board's `parseDIMENSION`, `parseZONE`, `parsePCB_TEXTBOX`, ...), a group's members become the ids of the items its uuids name (`resolveGroups`), and
//! [`export_kicad_mod`] writes them where `PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )` does, with the uuids the file gave them. The forms are KiCad's own: the
//! keepout of `Connector_Hirose.pretty/Hirose_DF57H-2P-2.4V_1x02_P2.4mm_Socket`, the dimensions of `Symbol.pretty/Smolhaj_Scale_0.1` and
//! `qa/data/pcbnew/api_kitchen_sink.kicad_pcb`, the text box of `RF_Module.pretty/Raytac_MDBT42Q`, the point of `Package_TO_SOT_THT.pretty/TO-251-2_Vertical`,
//! the barcode of `api_kitchen_sink`. The last test (`EDA_SLOW_TESTS=1`) has kicad-cli load the footprint we write.

use std::path::PathBuf;
use std::process::Command;

use eda_kicad::{export_kicad_mod, parse_library_footprint};
use eda_model::barcode::{BarcodeKind, EccLevel};
use eda_model::ir::{DimensionKind, DimensionTextBorder, DimensionUnits, LibraryFootprint, Point};
use eda_model::units::{mm, Nm};

fn png() -> String {
    eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(6, 4, [20, 120, 220], 300))
}

/// One item of each kind, a group (`g1`) of a pad, a graphic, the Reference field, the text box and a uuid that names nothing, and a group (`g2`) of that group
/// and the point.
fn fixture() -> String {
    let data = png();
    let lines: Vec<String> = data.as_bytes().chunks(76).map(|c| format!("\"{}\"", std::str::from_utf8(c).unwrap())).collect();
    FIXTURE.replace("PNG_DATA", &lines.join("\n\t\t\t"))
}

const FIXTURE: &str = r#"(footprint "Items"
	(version 20260206)
	(generator "pcbnew")
	(generator_version "10.99")
	(layer "F.Cu")
	(descr "one item of each kind")
	(property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS") (uuid "a0000000-0000-4000-8000-000000000001") (effects (font (size 1 1) (thickness 0.15))))
	(property "Value" "Items" (at 0 3 0) (layer "F.Fab") (uuid "a0000000-0000-4000-8000-000000000002") (effects (font (size 1 1) (thickness 0.15))))
	(attr smd)
	(fp_line (start -2 -1.5) (end 2 -1.5) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "b0000000-0000-4000-8000-000000000001"))
	(image (at 5 5) (layer "F.SilkS") (scale 0.5)
		(data
			PNG_DATA
		)
		(uuid "b0000000-0000-4000-8000-000000000002")
	)
	(fp_text_box "KEEP-OUT AREA"
		(start -6 -9)
		(end 6 -4.2)
		(margins 0.35 0.35 0.35 0.35)
		(layer "Dwgs.User")
		(uuid "c0000000-0000-4000-8000-000000000001")
		(effects (font (size 0.4 0.4) (thickness 0.1)) (justify top))
		(border yes)
		(stroke (width 0.1) (type solid))
		(knockout no)
	)
	(table (column_count 2) (uuid "c0000000-0000-4000-8000-000000000002") (layer "F.SilkS")
		(border (external yes) (header yes) (stroke (width 0.1) (type solid)))
		(separators (rows yes) (cols no) (stroke (width 0.05) (type solid)))
		(column_widths 5 6) (row_heights 2 2)
		(cells
			(table_cell "A" (start 10 10) (end 15 12) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "F.SilkS") (uuid "c1000000-0000-4000-8000-000000000001") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))
			(table_cell "B" (start 15 10) (end 21 12) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "F.SilkS") (uuid "c1000000-0000-4000-8000-000000000002") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))
			(table_cell "C" (start 10 12) (end 15 14) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "F.SilkS") (uuid "c1000000-0000-4000-8000-000000000003") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))
			(table_cell "D" (start 15 12) (end 21 14) (margins 0.5 0.5 0.5 0.5) (span 1 1) (layer "F.SilkS") (uuid "c1000000-0000-4000-8000-000000000004") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))
		)
	)
	(barcode (at 8 2 90) (layer "F.SilkS") (size 4 4) (text "acode") (text_height 1) (type qr) (ecc_level M) (hide no) (knockout no) (uuid "c0000000-0000-4000-8000-000000000003"))
	(dimension
		(type aligned)
		(layer "User.2")
		(uuid "d0000000-0000-4000-8000-000000000001")
		(pts (xy 0 0) (xy -56 0))
		(height 15.25)
		(format (prefix "") (suffix "") (units 3) (units_format 1) (precision 4))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "56,0000 mm" (at -28 -16.4 0) (layer "User.2") (uuid "d0000000-0000-4000-8000-000000000001") (effects (font (size 1 1) (thickness 0.15))))
	)
	(dimension
		(type leader)
		(layer "User.2")
		(uuid "d0000000-0000-4000-8000-000000000002")
		(pts (xy -25.369121 -10.545923) (xy -19.5 -12.5))
		(format (prefix "") (suffix "") (units 0) (units_format 0) (precision 4) (override_value "Antenna"))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (text_frame 1) (extension_offset 0.5))
		(gr_text "Antenna" (at -14.5 -12.5 0) (layer "User.2") (uuid "d0000000-0000-4000-8000-000000000002") (effects (font (size 1 1) (thickness 0.15))))
	)
	(dimension
		(type center)
		(layer "F.SilkS")
		(uuid "d0000000-0000-4000-8000-000000000003")
		(pts (xy 6.5 10.75) (xy 6.5 7.25))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_offset 0.5) (keep_text_aligned yes))
	)
	(dimension
		(type radial)
		(layer "F.SilkS")
		(uuid "d0000000-0000-4000-8000-000000000004")
		(pts (xy 15 13.5) (xy 13.25 14.75))
		(leader_length 3.81)
		(format (prefix "R ") (suffix "") (units 3) (units_format 1) (precision 4))
		(style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "R 2.1506 mm" (at -2.550325 16.964518 0) (layer "F.SilkS") (uuid "d0000000-0000-4000-8000-000000000004") (effects (font (size 1 1) (thickness 0.1))))
	)
	(dimension
		(type orthogonal)
		(layer "F.Fab")
		(uuid "d0000000-0000-4000-8000-000000000005")
		(pts (xy -6.75 -1.25) (xy 19.75 -3.75))
		(height 3.75)
		(orientation 0)
		(format (prefix "") (suffix "") (units 3) (units_format 1) (precision 4))
		(style (thickness 0.2) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "26.5000 mm" (at 6.5 0.7 0) (layer "F.Fab") (uuid "d0000000-0000-4000-8000-000000000005") (effects (font (size 1.5 1.5) (thickness 0.3))))
	)
	(point (at 2.29 -1.27) (size 2) (layer "F.Fab") (uuid "e0000000-0000-4000-8000-000000000001"))
	(pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "f0000000-0000-4000-8000-000000000001"))
	(pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "f0000000-0000-4000-8000-000000000002"))
	(zone
		(net 0)
		(net_name "")
		(layers "F.Cu" "B.Cu")
		(uuid "f1000000-0000-4000-8000-000000000001")
		(name "keepout")
		(hatch edge 0.508)
		(connect_pads (clearance 0))
		(min_thickness 0.25)
		(filled_areas_thickness no)
		(keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))
		(placement (enabled no) (sheetname ""))
		(fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
		(polygon (pts (xy -1.85 -1.4625) (xy 1.85 -1.4625) (xy 1.85 -0.2625) (xy -1.85 -0.2625)))
	)
	(group "g1" (uuid "f2000000-0000-4000-8000-000000000001")
		(members "f0000000-0000-4000-8000-000000000001" "b0000000-0000-4000-8000-000000000001" "a0000000-0000-4000-8000-000000000001" "c0000000-0000-4000-8000-000000000001" "99999999-0000-4000-8000-000000000000")
	)
	(group "g2" (uuid "f2000000-0000-4000-8000-000000000002") (members "f2000000-0000-4000-8000-000000000001" "e0000000-0000-4000-8000-000000000001"))
	(embedded_fonts no)
)
"#;

fn read(text: &str) -> LibraryFootprint {
    let parsed = parse_library_footprint(text).expect("parses");
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    parsed.footprint
}

fn p(x: f64, y: f64) -> Point {
    Point { x: Nm::from_mm(x), y: Nm::from_mm(y) }
}

/// The id of the item the file gave `uuid`.
fn id_of(fp: &LibraryFootprint, uuid: &str) -> String {
    fp.kicad.uuids.iter().find(|(_, u)| *u == uuid).map(|(id, _)| id.clone()).unwrap_or_else(|| panic!("no item has the uuid {uuid}: {:?}", fp.kicad.uuids))
}

#[test]
fn every_kind_of_item_of_a_footprint_is_read_into_its_items_with_the_boards_readers() {
    let fp = read(&fixture());
    let it = &fp.items;
    assert!(fp.kicad.tokens.is_empty(), "no item is left as text: {:?}", fp.kicad.tokens);
    assert_eq!((fp.pads.len(), fp.graphics.len()), (2, 1));

    // the dimensions, of the five kinds, as `parseDIMENSION` reads them
    assert_eq!(it.dimensions.len(), 5);
    let dim = |uuid: &str| it.dimensions.iter().find(|d| d.id == id_of(&fp, uuid)).expect("the dimension");
    let aligned = dim("d0000000-0000-4000-8000-000000000001");
    assert_eq!((aligned.kind.clone(), aligned.layer.as_str(), aligned.start, aligned.end), (DimensionKind::Aligned { height: mm(15.25) }, "User.2", p(0.0, 0.0), p(-56.0, 0.0)));
    assert_eq!((aligned.units, aligned.precision, aligned.keep_text_aligned, aligned.extension_height), (DimensionUnits::Automatic, 4, true, Nm::from_mm(0.58642)));
    let leader = dim("d0000000-0000-4000-8000-000000000002");
    assert_eq!((leader.kind.clone(), leader.override_text.as_deref(), leader.text_frame, leader.text_pos), (DimensionKind::Leader, Some("Antenna"), DimensionTextBorder::Rectangle, Some(p(-14.5, -12.5))));
    assert_eq!(dim("d0000000-0000-4000-8000-000000000003").kind, DimensionKind::Center);
    let radial = dim("d0000000-0000-4000-8000-000000000004");
    assert_eq!((radial.kind.clone(), radial.prefix.as_str()), (DimensionKind::Radial { leader_length: mm(3.81) }, "R "));
    let ortho = dim("d0000000-0000-4000-8000-000000000005");
    assert_eq!((ortho.kind.clone(), ortho.stroke_width, ortho.text_size_nm), (DimensionKind::Orthogonal { height: mm(3.75), horizontal: true }, mm(0.2), mm(1.5)));

    // the keepout, with no net, its pitch kept beside it
    assert_eq!(it.zones.len(), 1);
    let z = &it.zones[0];
    assert_eq!(id_of(&fp, "f1000000-0000-4000-8000-000000000001"), z.id);
    assert!(z.is_rule_area && z.keepout_tracks && z.keepout_vias && z.keepout_copper_pour && !z.keepout_pads && !z.keepout_footprints, "{z:?}");
    assert_eq!((z.net.as_str(), z.name.as_str(), z.layer_set(), z.outline.len()), ("", "keepout", vec!["F.Cu", "B.Cu"], 4));
    assert_eq!(z.outline[0], p(-1.85, -1.4625));
    assert!(z.uuid.is_empty(), "the uuid is kept with the others' (kicad.uuids)");
    assert_eq!(z.border_hatch_pitch, mm(0.508), "the zone keeps the file's hatch pitch (`Zone::border_hatch_pitch`)");

    // the text box, the point, the image, the barcode, the table
    assert_eq!(it.text_boxes.len(), 1);
    let tb = &it.text_boxes[0];
    assert_eq!((tb.text.as_str(), tb.layer.as_str(), tb.start, tb.end, tb.margin_top_nm, tb.border), ("KEEP-OUT AREA", "Dwgs.User", p(-6.0, -9.0), p(6.0, -4.2), mm(0.35), true));
    assert_eq!((tb.size_nm, tb.stroke_width_nm, tb.valign), (mm(0.4), mm(0.1), eda_model::ir::TextVAlign::Top));
    assert_eq!(it.points.len(), 1);
    assert_eq!((it.points[0].at, it.points[0].size_nm, it.points[0].layer.as_str()), (p(2.29, -1.27), mm(2.0), "F.Fab"));
    assert_eq!(it.images.len(), 1);
    assert_eq!((it.images[0].at, it.images[0].scale_ppm, it.images[0].layer.as_str()), (p(5.0, 5.0), 500_000, "F.SilkS"));
    assert_eq!(it.images[0].data, png());
    assert_eq!(it.barcodes.len(), 1);
    let b = &it.barcodes[0];
    assert_eq!((b.kind, b.ecc, b.angle, b.text.as_str(), b.width_nm, b.text_height_nm), (BarcodeKind::Qr, EccLevel::M, 90_000, "acode", mm(4.0), mm(1.0)));
    assert_eq!(it.tables.len(), 1);
    let t = &it.tables[0];
    assert_eq!((t.col_count(), t.cells.len(), t.at, t.col_widths.clone()), (2, 4, p(10.0, 10.0), vec![mm(5.0), mm(6.0)]));
    assert_eq!(t.cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["A", "B", "C", "D"]);
    assert!(t.uuid.is_empty() && t.cells[0].uuid == "c1000000-0000-4000-8000-000000000001", "the table's uuid is kept by its id, its cells keep theirs");

    // the groups: the members' ids, the uuid that names nothing dropped (`resolveGroups`), the inner group a member of the outer
    assert_eq!(it.groups.len(), 2);
    let g1 = it.groups.iter().find(|g| g.name == "g1").expect("g1");
    let g2 = it.groups.iter().find(|g| g.name == "g2").expect("g2");
    let mut want = vec![fp.pads.iter().find(|p| p.number == "1").unwrap().id.clone(), fp.graphics[0].id().to_string(), "field:Reference".to_string(), tb.id.clone()];
    want.sort();
    assert_eq!(g1.member_ids, want);
    let mut want = vec![g1.id.clone(), it.points[0].id.clone()];
    want.sort();
    assert_eq!(g2.member_ids, want);
    assert_eq!((id_of(&fp, "f2000000-0000-4000-8000-000000000001"), id_of(&fp, "f2000000-0000-4000-8000-000000000002")), (g1.id.clone(), g2.id.clone()));

    // every item has its id and its uuid
    for id in it.ids() {
        assert!(!id.is_empty() && fp.kicad.uuids.contains_key(id), "{id}");
    }
}

#[test]
fn the_items_are_written_where_kicad_writes_them_and_read_back_the_same() {
    let fp = read(&fixture());
    let text = export_kicad_mod(&fp);
    for form in ["(fp_text_box \"KEEP-OUT AREA\"", "(hatch edge 0.508)", "(point (at 2.29 -1.27) (size 2) (layer \"F.Fab\") (uuid \"e0000000-0000-4000-8000-000000000001\"))", "(barcode (at 8 2 90)", "(table (column_count 2) (uuid \"c0000000-0000-4000-8000-000000000002\")", "(image\n"] {
        assert!(text.contains(form), "{form}: {text}");
    }
    assert!(!text.contains("(gr_text_box") && !text.contains("(net "), "a footprint's text box is fp_text_box; its zone has no net: {text}");
    // a dimension's text is the dimension: one uuid for both, as KiCad writes it
    assert!(text.matches("(uuid \"d0000000-0000-4000-8000-000000000001\")").count() == 2, "{text}");
    // the group names its members by the uuids they are written with, a field and an inner group among them
    assert!(text.contains("(group \"g1\" (uuid \"f2000000-0000-4000-8000-000000000001\") (members \"a0000000-0000-4000-8000-000000000001\" \"b0000000-0000-4000-8000-000000000001\" \"c0000000-0000-4000-8000-000000000001\" \"f0000000-0000-4000-8000-000000000001\"))"), "{text}");
    assert!(text.contains("(group \"g2\" (uuid \"f2000000-0000-4000-8000-000000000002\") (members \"e0000000-0000-4000-8000-000000000001\" \"f2000000-0000-4000-8000-000000000001\"))"), "{text}");

    // `format( const FOOTPRINT* )`'s order: the drawings (`cmp_drawings`: shapes, images, texts, text boxes, tables, barcodes, dimensions by type), the points,
    // the pads, the zones, the groups
    let at = |form: &str| text.find(form).unwrap_or_else(|| panic!("{form}: {text}"));
    let order = ["(fp_line", "(image", "(fp_text_box", "(table", "(barcode", "(dimension (type aligned)", "(dimension (type leader)", "(dimension (type center)", "(dimension (type radial)", "(dimension (type orthogonal)", "(point", "(pad \"1\"", "(zone", "(group"];
    for w in order.windows(2) {
        assert!(at(w[0]) < at(w[1]), "{} before {}: {text}", w[0], w[1]);
    }

    let back = read(&text);
    assert_eq!(back.items, fp.items, "the items, ids and all, come back as they were read");
    assert_eq!(back.kicad.uuids, fp.kicad.uuids);
    assert!(back.kicad.tokens.is_empty(), "{:?}", back.kicad.tokens);
    assert_eq!(export_kicad_mod(&back), text, "and are written the same again");
}

/// A footprint of an older `design.json` kept its items as the text of their forms (`LibraryFootprintKeep::tokens`): they are still written, and the reader now takes
/// them into its items.
#[test]
fn an_older_footprint_that_kept_its_items_as_text_still_writes_them() {
    let mut fp = read(&fixture());
    fp.items = Default::default();
    fp.kicad.tokens = vec![
        "(zone (net 0) (net_name \"\") (layer \"F.Cu\") (uuid \"f1000000-0000-4000-8000-000000000009\") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour allowed) (footprints allowed)) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))".into(),
        "(group \"\" (uuid \"f2000000-0000-4000-8000-000000000009\") (members \"f0000000-0000-4000-8000-000000000002\" \"f1000000-0000-4000-8000-000000000009\"))".into(),
    ];
    let text = export_kicad_mod(&fp);
    assert!(text.contains("\t(zone (net 0) (net_name \"\") (layer \"F.Cu\") (uuid \"f1000000-0000-4000-8000-000000000009\")") && text.contains("\t(group \"\" (uuid \"f2000000-0000-4000-8000-000000000009\")"), "{text}");
    let back = read(&text);
    assert!(back.kicad.tokens.is_empty(), "{:?}", back.kicad.tokens);
    assert_eq!((back.items.zones.len(), back.items.groups.len()), (1, 1));
    let pad2 = back.pads.iter().find(|p| p.number == "2").unwrap();
    let mut want = vec![pad2.id.clone(), back.items.zones[0].id.clone()];
    want.sort();
    assert_eq!(back.items.groups[0].member_ids, want);
}

/// What a library footprint's items cannot hold stays the text the file wrote: a locked item (a library footprint has no lock for its items), a dimension of the
/// pre-6.0 form, a zone with its fill or with the arcs of its outline; and a group that names one of them, or a locked group, stays text too, naming its members by
/// their uuids, which every item keeps.
#[test]
fn a_form_the_items_cannot_hold_and_a_group_naming_one_stay_as_the_file_wrote_them() {
    let extra = r#"	(dimension (type aligned) (locked yes) (layer "F.Fab") (uuid "d9000000-0000-4000-8000-000000000001") (pts (xy 0 0) (xy 5 0)) (height 1) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5)) (gr_text "5.00 mm" (at 2.5 -1 0) (layer "F.Fab") (uuid "d9000000-0000-4000-8000-000000000001") (effects (font (size 1 1) (thickness 0.15)))))
	(zone (layer "F.Cu") (uuid "d9000000-0000-4000-8000-000000000002") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))) (filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 1 0) (xy 1 1))))
	(zone (layer "F.Cu") (uuid "d9000000-0000-4000-8000-000000000003") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 0 0) (arc (start 1 0) (mid 1.5 0.5) (end 1 1)))))
	(group "holds a locked item" (uuid "d9000000-0000-4000-8000-000000000004") (members "d9000000-0000-4000-8000-000000000001" "f0000000-0000-4000-8000-000000000002"))
	(group "holds that group" (uuid "d9000000-0000-4000-8000-000000000005") (members "d9000000-0000-4000-8000-000000000004"))
	(group "locked" (locked yes) (uuid "d9000000-0000-4000-8000-000000000006") (members "e0000000-0000-4000-8000-000000000001"))
	(embedded_fonts no)"#;
    let text = fixture().replace("\t(embedded_fonts no)", extra);
    let fp = read(&text);
    let tags: Vec<&str> = fp.kicad.tokens.iter().map(|t| t.split([' ', '\n']).next().unwrap()).collect();
    assert_eq!(tags, ["(dimension", "(zone", "(zone", "(group", "(group", "(group"], "{:?}", fp.kicad.tokens);
    assert_eq!((fp.items.dimensions.len(), fp.items.zones.len(), fp.items.groups.len()), (5, 1, 2), "the others are items");
    let written = export_kicad_mod(&fp);
    for t in &fp.kicad.tokens {
        assert!(written.contains(t.as_str()), "{t}: {written}");
    }
    // the point, a member of the locked group (text) and of g2 (an item): both name it by its uuid
    assert_eq!(written.matches("\"e0000000-0000-4000-8000-000000000001\"").count(), 3, "{written}");
    let back = read(&written);
    assert_eq!((back.items, back.kicad.tokens), (fp.items.clone(), fp.kicad.tokens.clone()));
}

/// KiCad's own library footprints with these items -- the keepouts of a connector, the dimensions and the leaders of a logo, the text boxes of a module, the group
/// of a panelization mouse bite, the point of a package -- read, written and read again give back the same items. Skipped when the libraries are not installed.
#[test]
fn kicads_own_footprints_with_items_read_and_write_back_the_same_items() {
    let root = eda_kicad::kicad_shared_dir("footprints");
    let files = [
        "Connector_Hirose.pretty/Hirose_DF57H-2P-2.4V_1x02_P2.4mm_Socket.kicad_mod",
        "Symbol.pretty/Smolhaj_Scale_0.1.kicad_mod",
        "RF_Module.pretty/Raytac_MDBT42Q.kicad_mod",
        "Panelization.pretty/MouseBite-Slot_2.54x5.08mm_on-edge_D0.5mm_P0.85mm_5h_Trace.kicad_mod",
        "Package_TO_SOT_THT.pretty/TO-251-2_Vertical.kicad_mod",
    ];
    let mut seen = 0;
    for f in files {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else { continue };
        seen += 1;
        let fp = parse_library_footprint(&text).unwrap_or_else(|e| panic!("{f}: {e}")).footprint;
        assert!(!fp.items.is_empty(), "{f} has items");
        assert!(fp.kicad.tokens.iter().all(|t| !t.starts_with("(zone") && !t.starts_with("(dimension") && !t.starts_with("(group") && !t.starts_with("(point") && !t.starts_with("(fp_text_box")), "{f}: {:?}", fp.kicad.tokens);
        let back = parse_library_footprint(&export_kicad_mod(&fp)).unwrap().footprint;
        assert_eq!(back.items, fp.items, "{f}");
        assert_eq!((back.pads.len(), back.graphics.len(), back.texts.len()), (fp.pads.len(), fp.graphics.len(), fp.texts.len()), "{f}");
    }
    if seen == 0 {
        eprintln!("KiCad footprint libraries not found at {}; skipping", root.display());
    }
}

// ----------------------------------------------------------------------------------------------------------------------
// kicad-cli
// ----------------------------------------------------------------------------------------------------------------------

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

/// kicad-cli loads the footprint our writer makes with every kind of item: `fp upgrade` writes it back (KiCad's own `format( const FOOTPRINT* )`) with each item,
/// the group with its members, and what it writes for our file reads as the items KiCad's write of the original does; `fp export svg` plots it.
#[test]
fn kicad_cli_loads_and_keeps_every_item_our_writer_writes() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let dir = std::env::temp_dir().join(format!("eda_fp_items_io_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let original = fixture();
    let ours = export_kicad_mod(&read(&original));
    let mut upgraded = Vec::new();
    for (name, text) in [("original", &original), ("ours", &ours)] {
        let lib = dir.join(format!("{name}.pretty"));
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("Items.kicad_mod"), text).unwrap();
        let up = dir.join(format!("{name}_up.pretty"));
        let out = Command::new(&cli).args(["fp", "upgrade", "--force", "-o"]).arg(&up).arg(&lib).output().expect("runs kicad-cli");
        assert!(out.status.success(), "kicad-cli fp upgrade of {name}: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        upgraded.push(std::fs::read_to_string(up.join("Items.kicad_mod")).unwrap_or_else(|_| panic!("kicad-cli wrote no footprint for {name}")));
        let svg = dir.join(format!("{name}_svg"));
        std::fs::create_dir_all(&svg).unwrap();
        let out = Command::new(&cli).args(["fp", "export", "svg", "-o"]).arg(&svg).arg(&lib).output().expect("runs kicad-cli");
        assert!(out.status.success(), "kicad-cli fp export svg of {name}: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let plot = std::fs::read_to_string(svg.join("Items.svg")).expect("an svg");
        assert!(plot.contains("<path") || plot.contains("<polyline"), "{name}: kicad-cli plotted nothing");
    }
    let kicad_ours = &upgraded[1];
    // the footprint's own forms, one tab in (a table's cells are deeper)
    for (form, n) in [("(dimension", 5), ("(zone", 1), ("(fp_text_box", 1), ("(point", 1), ("(image", 1), ("(barcode", 1), ("(table", 1), ("(group", 2)] {
        assert_eq!(kicad_ours.matches(&format!("\n\t{form}")).count(), n, "kicad-cli keeps {n} {form} of our file: {kicad_ours}");
    }
    let (from_original, from_ours) = (read(&upgraded[0]), read(&upgraded[1]));
    assert_eq!(from_ours.items, from_original.items, "KiCad's write of our file has the items of KiCad's write of the original");
    let first = read(&original);
    assert_eq!(from_ours.items.groups, first.items.groups, "and the groups the original has");
    // every item keeps its uuid (KiCad gives the Datasheet and Description fields the original has not a new one of their own each time)
    for (id, uuid) in &first.kicad.uuids {
        assert_eq!((from_ours.kicad.uuids.get(id), from_original.kicad.uuids.get(id)), (Some(uuid), Some(uuid)), "{id}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
