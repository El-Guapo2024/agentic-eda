//! A library footprint on the board whole (UI-LEFT PP5, PP14, FF4), through a board directory and its library tables: Place Footprint, Update Footprint from Library
//! and Change Footprint give the board's footprint the library's pads' layer sets, die lengths and properties, its description, tags, private layers, jumper groups and
//! margins, its pad with no copper, its dimension, table and zone, so the board file says what the library's does and Compare with Library finds nothing; the dimension,
//! the table and the zone are drawn and moved, turned and flipped with the footprint; Ctrl+E's copy shows all of it and Update Footprint on Board puts back what was
//! drawn or edited there.

use super::tests::scratch;
use super::{load, step};
use eda_model::ir::{BoardFootprintCopy, Point};
use eda_model::units::mm;
use eda_ops::fp_editor::FpEditorCmd;
use eda_ops::fp_items::{FpItem, FpItemCmd};
use eda_ops::{Cmd, ExchangeOptions, FlipDirection};
use std::path::{Path, PathBuf};

/// `Lib:Kept`: what a board footprint used to lose of its library's -- an SMD pad with no paste, a pad with a die length and a fabrication property, a paste-only pad,
/// the description, tags, a Datasheet, private layers, jumper groups and margins, a dimension, a table and a keepout.
const KEPT: &str = r#"(footprint "Kept" (version 20241229) (generator "pcbnew") (layer "F.Cu")
  (descr "a kept footprint") (tags "kept test")
  (property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Kept" (at 0 3 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (property "Datasheet" "kept.pdf" (at 0 0 0) (layer "F.Fab") (hide yes) (effects (font (size 1.27 1.27))))
  (attr smd)
  (private_layers "User.1")
  (jumper_pad_groups ("1" "2"))
  (solder_mask_margin 0.05) (solder_paste_margin -0.03) (clearance 0.3)
  (fp_line (start -2 -2) (end 2 -2) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
  (fp_rect (start -2.5 -2.5) (end 2.5 2.5) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
  (dimension (type aligned) (layer "F.Fab") (pts (xy -1 2) (xy 1 2)) (height 1) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned))
  (table (column_count 1) (layer "F.Fab") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols yes) (stroke (width 0.1) (type solid))) (column_widths 3) (row_heights 1.5)
    (cells (table_cell "T" (start -1.5 -1) (end 1.5 0.5) (margins 0.2 0.2 0.2 0.2) (span 1 1) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))))
  (pad "1" smd roundrect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask") (roundrect_rratio 0.25) (property pad_prop_heatsink) (die_length 0.5))
  (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "" smd rect (at 0 1) (size 0.5 0.5) (layers "F.Paste"))
  (zone (net 0) (net_name "") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed)) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy -3 3) (xy 3 3) (xy 3 4) (xy -3 4))))
)"#;

/// A board of one footprint, U1 (`Lib:Logo`, two plain pads) at (50, 40), imported as a KiCad board is; the library `Lib` in its tables holds `Logo` and `Kept`.
fn board(name: &str) -> PathBuf {
    let dir = scratch(name);
    let text = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (15 "B.Paste" user) (5 "F.SilkS" user) (7 "B.SilkS" user) (35 "F.Fab" user) (37 "B.Fab" user) (31 "F.CrtYd" user) (29 "B.CrtYd" user) (25 "Edge.Cuts" user) (39 "User.1" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_rect (start 0 0) (end 100 80) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (footprint "Lib:Logo" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000011") (at 50 40 0)
    (property "Reference" "U1" (at 0 -6 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "Logo" (at 0 6 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-000000000019"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-00000000001a"))
  )
)"#;
    let file = dir.join("logo.kicad_pcb");
    std::fs::write(&file, text).unwrap();
    let out = dir.join("board");
    crate::import_kicad::run(&[file.display().to_string(), "-o".into(), out.display().to_string()]).unwrap();
    let lib = out.join("libs").join("Lib.pretty");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("Kept.kicad_mod"), KEPT).unwrap();
    // `Logo` as the library has it now: the pads, the dimension, the table and the keepout of `Kept` (an update brings them)
    std::fs::write(lib.join("Logo.kicad_mod"), KEPT.replace("(footprint \"Kept\"", "(footprint \"Logo\"").replace("\"Value\" \"Kept\"", "\"Value\" \"Logo\"")).unwrap();
    assert_eq!(crate::library_tables::add_library(&out, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/Lib.pretty"}"#)["ok"], true);
    out
}

/// The derived board file.
fn pcb(dir: &Path) -> String {
    let (_, design, model) = load(dir).unwrap();
    eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-11-06", title: "kept" }).unwrap()
}

/// The `(footprint ..)` of `reference` in the board file `pcb`.
fn footprint_of<'a>(pcb: &'a str, reference: &str) -> &'a str {
    let at = pcb.find(&format!("(property \"Reference\" \"{reference}\"")).expect("the footprint");
    let start = pcb[..at].rfind("(footprint ").unwrap();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in pcb[start..].char_indices() {
        match c {
            '"' if prev != '\\' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return &pcb[start..start + i + 1];
                }
            }
            _ => {}
        }
        prev = c;
    }
    &pcb[start..]
}

/// What the board file says of the footprint that the library's says too.
fn assert_library_kept(fp: &str, what: &str) {
    let pad1 = fp.split("(pad ").find(|p| p.starts_with("\"1\"")).expect("pad 1");
    assert!(pad1.contains("(layers \"F.Cu\" \"F.Mask\")"), "{what}: a pad with no paste is written with none: {pad1}");
    assert!(pad1.contains("(property pad_prop_heatsink)") && pad1.contains("(die_length 0.5)"), "{what}: the pad's fabrication property and die length: {pad1}");
    assert!(fp.contains("(layers \"F.Paste\")"), "{what}: the paste-only pad");
    for form in ["(descr \"a kept footprint\")", "(tags \"kept test\")", "(private_layers \"User.1\")", "(jumper_pad_groups", "(solder_mask_margin 0.05)", "(solder_paste_margin -0.03)", "(clearance 0.3)"] {
        assert!(fp.contains(form), "{what}: {form}\n{fp}");
    }
    assert_eq!((fp.matches("(dimension").count(), fp.matches("(table ").count(), fp.matches("(zone").count() - fp.matches("(zone_connect").count()), (1, 1, 1), "{what}: the dimension, the table and the keepout\n{fp}");
}

/// Compare with Library's lines for `reference` (`/api/exchange_plan` of an update).
fn differences(dir: &Path, reference: &str) -> Vec<String> {
    let plan = crate::fp_assign_api::exchange_plan(dir, format!(r#"{{"references":["{reference}"],"footprint":null}}"#).as_bytes());
    plan["entries"][0]["lines"].as_array().map(|a| a.iter().map(|l| l.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_else(|| panic!("{plan}"))
}

/// The first point of the outline of `reference`'s zone on the board.
fn zone_corner(dir: &Path, reference: &str) -> Point {
    let (_, design, _) = load(dir).unwrap();
    let z = design.routing.as_ref().unwrap().zones.iter().find(|z| z.parent_footprint.as_deref() == Some(reference)).expect("the footprint's zone");
    z.outline[0]
}

#[test]
fn a_footprint_placed_from_a_library_keeps_what_the_library_says_and_its_zone_follows_it() {
    let dir = board("pp5_place");
    step(&dir, Cmd::PlaceFootprint { footprint: "Lib:Kept".into(), at: Point { x: mm(70.0), y: mm(40.0) }, reference: "K1".into(), value: String::new(), definition: None }, false, "test").unwrap();
    let text = pcb(&dir);
    assert_library_kept(footprint_of(&text, "K1"), "placed");
    let pad1 = footprint_of(&text, "K1").split("(pad ").find(|p| p.starts_with("\"1\"")).unwrap().to_string();
    assert!(pad1.contains("(pintype \"passive\")"), "the pad takes its pin's type, as a footprint made here does: {pad1}");
    assert_eq!(differences(&dir, "K1"), Vec::<String>::new(), "Compare with Library finds nothing");

    // the state draws the dimension and the table with the footprint, in board space
    let job: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
    let state = crate::studio::state(&dir, &job, crate::text_vars_json::clock_of("/api/state")).unwrap();
    let items = state["parts"].as_array().unwrap().iter().find(|p| p["ref"] == "K1").unwrap()["items"].clone();
    assert_eq!((items["dimensions"].as_array().map(Vec::len), items["tables"].as_array().map(Vec::len)), (Some(1), Some(1)), "{items}");
    assert_eq!(items["dimensions"][0]["start"][0], mm(69.0).get(), "at the footprint's place");
    assert!(items["tables"][0]["id"].as_str().unwrap().starts_with("K1~tbl_"));

    // the keepout is where the footprint puts it, and goes with it: a move, a turn, a flip
    assert_eq!(zone_corner(&dir, "K1"), Point { x: mm(67.0), y: mm(43.0) });
    step(&dir, Cmd::MoveItems { ids: vec!["K1".into()], dx: mm(5.0), dy: mm(0.0) }, false, "test").unwrap();
    assert_eq!(zone_corner(&dir, "K1"), Point { x: mm(72.0), y: mm(43.0) });
    step(&dir, Cmd::RotateItems { ids: vec!["K1".into()], pivot: Point { x: mm(75.0), y: mm(40.0) }, angle_millideg: 90_000 }, false, "test").unwrap();
    assert_eq!(zone_corner(&dir, "K1"), Point { x: mm(72.0), y: mm(37.0) }, "a quarter turn clockwise on the screen about the footprint");
    step(&dir, Cmd::FlipItems { ids: vec!["K1".into()], pivot: Point { x: mm(75.0), y: mm(40.0) }, direction: FlipDirection::LeftRight }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let z = design.routing.as_ref().unwrap().zones.iter().find(|z| z.parent_footprint.as_deref() == Some("K1")).unwrap();
    assert_eq!((z.outline[0], z.layer.as_str()), (Point { x: mm(78.0), y: mm(37.0) }, "B.Cu"), "mirrored and on the back");
    // the file has the turned and flipped footprint's dimension and table, and kicad's pads still have no paste where the library has none
    let text = pcb(&dir);
    let fp = footprint_of(&text, "K1");
    assert!(fp.contains("(layers \"B.Cu\" \"B.Mask\")"), "{fp}");
    assert_eq!(differences(&dir, "K1"), Vec::<String>::new(), "flipped and turned, still the library's");
}

#[test]
fn update_and_change_footprint_take_the_librarys_pads_record_items_and_zone() {
    let dir = board("pp5_exchange");
    // the board's Logo has paste on both pads and none of the rest: Compare with Library says so
    let before = differences(&dir, "U1");
    assert!(before.iter().any(|l| l.contains("layers differ")), "{before:?}");
    step(&dir, Cmd::ExchangeFootprints { references: vec!["U1".into()], footprint: None, options: ExchangeOptions::update(), library: Default::default(), library_items: Default::default() }, false, "test").unwrap();
    assert_library_kept(footprint_of(&pcb(&dir), "U1"), "updated");
    assert_eq!(differences(&dir, "U1"), Vec::<String>::new(), "after the update Compare with Library finds nothing");
    assert_eq!(zone_corner(&dir, "U1"), Point { x: mm(47.0), y: mm(43.0) });

    // Change Footprint to Kept: the same, the zone the new footprint's (one, not two)
    step(&dir, Cmd::ExchangeFootprints { references: vec!["U1".into()], footprint: Some("Lib:Kept".into()), options: ExchangeOptions::change(), library: Default::default(), library_items: Default::default() }, false, "test").unwrap();
    assert_library_kept(footprint_of(&pcb(&dir), "U1"), "changed");
    let (_, design, _) = load(&dir).unwrap();
    assert_eq!(design.routing.as_ref().unwrap().zones.iter().filter(|z| z.parent_footprint.as_deref() == Some("U1")).count(), 1);
}

#[test]
fn ctrl_e_shows_the_footprints_record_and_items_and_update_on_board_puts_back_what_was_drawn_and_edited() {
    let dir = board("ff4_copy");
    step(&dir, Cmd::PlaceFootprint { footprint: "Lib:Kept".into(), at: Point { x: mm(70.0), y: mm(40.0) }, reference: "K1".into(), value: String::new(), definition: None }, false, "test").unwrap();
    step(&dir, Cmd::FpEditor(FpEditorCmd::OpenFromBoard { reference: "K1".into() }), false, "test").unwrap();
    let key = BoardFootprintCopy::key_of("K1");
    let (_, design, _) = load(&dir).unwrap();
    let copy = design.footprint_library.as_ref().unwrap().by_name(&key).unwrap().clone();
    assert_eq!((copy.items.dimensions.len(), copy.items.tables.len(), copy.items.zones.len()), (1, 1, 1), "the copy shows the dimension, the table and the keepout");
    assert_eq!(copy.items.zones[0].outline[0], Point { x: mm(-3.0), y: mm(3.0) }, "in the footprint's frame");
    assert_eq!((copy.description.as_str(), copy.keywords.as_str()), ("a kept footprint", "kept test"));
    assert_eq!(copy.datasheet.as_ref().and_then(|d| d.text.as_deref()), Some("kept.pdf"));
    assert_eq!((copy.settings.private_layers.clone(), copy.settings.jumper_pad_groups.clone()), (vec!["User.1".to_string()], vec![vec!["1".to_string(), "2".to_string()]]));

    // a second dimension and a second keepout drawn in the copy, its description, keywords, Datasheet, private layers and jumper groups edited
    let mut dim = copy.items.dimensions[0].clone();
    dim.id = String::new();
    dim.start.y = mm(-1.5);
    dim.end.y = mm(-1.5);
    step(&dir, Cmd::FpItem(FpItemCmd::Add { footprint: key.clone(), item: FpItem::Dimension(dim) }), false, "test").unwrap();
    let mut zone = copy.items.zones[0].clone();
    zone.id = String::new();
    zone.outline.iter_mut().for_each(|p| p.y = -p.y);
    step(&dir, Cmd::FpItem(FpItemCmd::Add { footprint: key.clone(), item: FpItem::Zone(zone) }), false, "test").unwrap();
    // (Footprint Properties' General tab and its settings, as the dialog sends them: `edit_footprint_properties` and `fp_editor set_settings`)
    let now = load(&dir).unwrap().1.footprint_library.unwrap().by_name(&key).unwrap().clone();
    let mut datasheet = now.datasheet.clone().unwrap();
    datasheet.text = Some("edited.pdf".into());
    let general = Cmd::EditFootprintProperties {
        name: key.clone(),
        description: "edited in the copy".into(),
        keywords: "edited".into(),
        attributes: now.attributes,
        reference_visible: now.reference_visible,
        value_visible: now.value_visible,
        model: now.model.clone(),
        models3d: None,
        zone_connection: now.zone_connection,
        reference: None,
        value: None,
        fields: None,
        datasheet: Some(datasheet),
        description_field: None,
    };
    let settings = eda_model::fp_settings::FootprintSettings { private_layers: vec!["User.2".into()], jumper_pad_groups: Vec::new(), ..now.settings.clone() };
    step(&dir, Cmd::Batch { cmds: vec![general, Cmd::FpEditor(FpEditorCmd::SetSettings { footprint: key.clone(), settings })] }, false, "test").unwrap();
    step(&dir, Cmd::FpEditor(FpEditorCmd::SaveToBoard { footprint: key }), false, "test").unwrap();

    let text = pcb(&dir);
    let fp = footprint_of(&text, "K1");
    assert_eq!((fp.matches("(dimension").count(), fp.matches("(table ").count(), fp.matches("(zone").count() - fp.matches("(zone_connect").count()), (2, 1, 2), "{fp}");
    for form in ["(descr \"edited in the copy\")", "(tags \"edited\")", "(private_layers \"User.2\")", "(property \"Datasheet\" \"edited.pdf\""] {
        assert!(fp.contains(form), "{form}\n{fp}");
    }
    assert!(!fp.contains("(jumper_pad_groups") && !fp.contains("kept.pdf"), "{fp}");
    let pad1 = fp.split("(pad ").find(|p| p.starts_with("\"1\"")).unwrap();
    assert!(pad1.contains("(layers \"F.Cu\" \"F.Mask\")"), "the pads' facts stay: {pad1}");
}

/// UI-LEFT-3 F3-11: Ctrl+E of a footprint on the back, turned, brings everything to the front at no angle (`LoadFootprintFromBoard`: `Flip` when it is not on F.Cu,
/// then `SetOrientation( ANGLE_0 )`), its zones with the rest: the copy is the library footprint's frame, item for item.
#[test]
fn ctrl_e_of_a_turned_footprint_on_the_back_shows_its_zones_at_the_front_with_the_rest() {
    let dir = board("f311_back");
    step(&dir, Cmd::PlaceFootprint { footprint: "Lib:Kept".into(), at: Point { x: mm(70.0), y: mm(40.0) }, reference: "K1".into(), value: String::new(), definition: None }, false, "test").unwrap();
    step(&dir, Cmd::RotateItems { ids: vec!["K1".into()], pivot: Point { x: mm(70.0), y: mm(40.0) }, angle_millideg: 30_000 }, false, "test").unwrap();
    step(&dir, Cmd::FlipItems { ids: vec!["K1".into()], pivot: Point { x: mm(70.0), y: mm(40.0) }, direction: FlipDirection::LeftRight }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let z = design.routing.as_ref().unwrap().zones.iter().find(|z| z.parent_footprint.as_deref() == Some("K1")).unwrap();
    assert_eq!(z.layer, "B.Cu", "the keepout went to the back with the footprint");
    step(&dir, Cmd::FpEditor(FpEditorCmd::OpenFromBoard { reference: "K1".into() }), false, "test").unwrap();
    let key = BoardFootprintCopy::key_of("K1");
    let (_, design, _) = load(&dir).unwrap();
    let copy = design.footprint_library.as_ref().unwrap().by_name(&key).unwrap().clone();
    let near = |a: Point, b: Point| (a.x.get() - b.x.get()).abs() <= 2 && (a.y.get() - b.y.get()).abs() <= 2;
    let zone = &copy.items.zones[0];
    assert_eq!(zone.layer, "F.Cu", "the keepout at the front, as the footprint");
    let want = [(-3.0, 3.0), (3.0, 3.0), (3.0, 4.0), (-3.0, 4.0)].map(|(x, y)| Point { x: mm(x), y: mm(y) });
    assert!(zone.outline.iter().zip(want).all(|(a, b)| near(*a, b)), "the library's outline: {:?}", zone.outline);
    let pad1 = copy.pads.iter().find(|p| p.number == "1").unwrap();
    assert!(near(pad1.at, Point { x: mm(-1.0), y: mm(0.0) }), "pad 1 where the library has it: {:?}", pad1.at);
    let dim = &copy.items.dimensions[0];
    assert!(near(dim.start, Point { x: mm(-1.0), y: mm(2.0) }) && dim.layer == "F.Fab", "the dimension too: {:?} {}", dim.start, dim.layer);
}

/// UI-LEFT-3 F3-11: a footprint group holding a field (`PCB_FIELD` is groupable) keeps it in Ctrl+E's copy, and Update Footprint on Board writes it back in the group.
#[test]
fn a_footprint_group_keeps_its_field_through_ctrl_e_and_update_on_board() {
    let dir = scratch("f311_group_field");
    let text = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (5 "F.SilkS" user) (7 "B.SilkS" user) (35 "F.Fab" user) (37 "B.Fab" user) (31 "F.CrtYd" user) (29 "B.CrtYd" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_rect (start 0 0) (end 100 80) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (footprint "Lib:Logo" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000011") (at 50 40 0)
    (property "Reference" "U1" (at 0 -6 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "Logo" (at 0 6 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (fp_line (start -2 -2) (end 2 -2) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000014"))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-000000000019"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-00000000001a"))
    (group "G" (uuid "00000000-0000-0000-0000-0000000000f1") (members "00000000-0000-0000-0000-000000000012" "00000000-0000-0000-0000-000000000014" "00000000-0000-0000-0000-000000000019"))
  )
)"#;
    let file = dir.join("group.kicad_pcb");
    std::fs::write(&file, text).unwrap();
    let out = dir.join("board");
    crate::import_kicad::run(&[file.display().to_string(), "-o".into(), out.display().to_string()]).unwrap();
    step(&out, Cmd::FpEditor(FpEditorCmd::OpenFromBoard { reference: "U1".into() }), false, "test").unwrap();
    let key = BoardFootprintCopy::key_of("U1");
    let (_, design, _) = load(&out).unwrap();
    let copy = design.footprint_library.as_ref().unwrap().by_name(&key).unwrap().clone();
    assert_eq!(copy.items.groups.len(), 1, "{:?}", copy.items.groups);
    let members = &copy.items.groups[0].member_ids;
    assert!(members.iter().any(|m| m == "field:Reference"), "the Reference stays in the group: {members:?}");
    assert_eq!(members.len(), 3, "the field, the line and the pad: {members:?}");
    // nothing edited: Update Footprint on Board writes the group back with its three members
    step(&out, Cmd::FpEditor(FpEditorCmd::SaveToBoard { footprint: key }), false, "test").unwrap();
    let text = pcb(&out);
    let fp = footprint_of(&text, "U1");
    let group = &fp[fp.find("(group").expect("the group")..];
    let reference = &fp[fp.find("(property \"Reference\"").unwrap()..];
    let uuid = &reference[reference.find("(uuid \"").unwrap() + 7..];
    let uuid = &uuid[..uuid.find('"').unwrap()];
    assert!(group.contains(uuid), "the Reference's uuid ({uuid}) among the members: {group}");
    let members_text = &group[group.find("(members").unwrap()..];
    assert_eq!(members_text[..members_text.find(')').unwrap()].matches('"').count(), 6, "three members: {members_text}");
}

/// UI-LEFT-3 F3-12: a library footprint's zone drawn with an arc keeps it on the board (the board's footprint is a clone of the library's), where the footprint puts it
/// and as it turns and flips; Ctrl+E's copy has it in the footprint's frame, the copy's Move takes it along, and Update Footprint on Board puts it back.
#[test]
fn a_footprint_zones_arc_goes_onto_the_board_into_ctrl_e_and_back() {
    let dir = board("f312_arcs");
    let arcs = KEPT
        .replace("(footprint \"Kept\"", "(footprint \"Arcs\"")
        .replace("(polygon (pts (xy -3 3) (xy 3 3) (xy 3 4) (xy -3 4)))", "(polygon (pts (xy -3 3) (xy 3 3) (arc (start 3 4) (mid 0 5) (end -3 4))))");
    std::fs::write(dir.join("libs").join("Lib.pretty").join("Arcs.kicad_mod"), arcs).unwrap();
    step(&dir, Cmd::PlaceFootprint { footprint: "Lib:Arcs".into(), at: Point { x: mm(70.0), y: mm(40.0) }, reference: "K1".into(), value: String::new(), definition: None }, false, "test").unwrap();
    let fp = pcb(&dir);
    assert!(footprint_of(&fp, "K1").contains("(arc (start 73 44) (mid 70 45) (end 67 44))"), "the arc where the footprint puts it: {}", footprint_of(&fp, "K1"));
    step(&dir, Cmd::RotateItems { ids: vec!["K1".into()], pivot: Point { x: mm(70.0), y: mm(40.0) }, angle_millideg: 90_000 }, false, "test").unwrap();
    step(&dir, Cmd::FlipItems { ids: vec!["K1".into()], pivot: Point { x: mm(70.0), y: mm(40.0) }, direction: FlipDirection::LeftRight }, false, "test").unwrap();
    let fp = pcb(&dir);
    assert!(footprint_of(&fp, "K1").contains("(arc "), "turned and flipped, the arc stays: {}", footprint_of(&fp, "K1"));
    step(&dir, Cmd::FpEditor(FpEditorCmd::OpenFromBoard { reference: "K1".into() }), false, "test").unwrap();
    let key = BoardFootprintCopy::key_of("K1");
    let copy = load(&dir).unwrap().1.footprint_library.unwrap().by_name(&key).unwrap().clone();
    let lib_text = eda_kicad::export_kicad_mod(&copy);
    assert!(lib_text.contains("(arc (start 3 4) (mid 0 5) (end -3 4))"), "the copy's zone has the library's arc, in its frame: {lib_text}");
    // the copy's zone moved: its arc with it; then put back on the board
    let zid = copy.items.zones[0].id.clone();
    step(&dir, Cmd::FpItem(FpItemCmd::Move { footprint: key.clone(), ids: vec![zid], dx: mm(1.0), dy: mm(0.0) }), false, "test").unwrap();
    let moved = load(&dir).unwrap().1.footprint_library.unwrap().by_name(&key).unwrap().clone();
    assert!(eda_kicad::export_kicad_mod(&moved).contains("(arc (start 4 4) (mid 1 5) (end -2 4))"), "{}", eda_kicad::export_kicad_mod(&moved));
    step(&dir, Cmd::FpEditor(FpEditorCmd::SaveToBoard { footprint: key }), false, "test").unwrap();
    let fp = pcb(&dir);
    assert!(footprint_of(&fp, "K1").contains("(arc "), "Update Footprint on Board keeps it: {}", footprint_of(&fp, "K1"));
}
