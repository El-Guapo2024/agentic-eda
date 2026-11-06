//! A board footprint holds every kind a library footprint can (UI-LEFT PP5, PP14), proved against KiCad's own reader and writer:
//!
//! * a footprint of a KiCad QA board (`issue7325`'s FH1, whose two zones are its own) given a dimension and a table, as a board file holds them (the dimension where it is
//!   on the board, the table's cells in the footprint's frame: `format( PCB_DIMENSION_BASE )`, `format( PCB_TEXTBOX )` of a cell with its `parentFP`): read into the
//!   footprint's items, written back as they were, moved, turned and flipped with the footprint (its zones too: `FOOTPRINT::Move`, `SetOrientation`, `Flip`);
//! * a footprint placed from a library (`Cmd::PlaceFootprint` with the library's footprint): its pad with no paste written with none, its pad-to-die length and
//!   fabrication property, its description and its dimension, table and keepout.
//!
//! The slow tier (`EDA_SLOW_TESTS=1`) hands each written board to kicad-cli (`pcb upgrade --force`: `PCB_IO_KICAD_SEXPR_PARSER`, then `PCB_IO_KICAD_SEXPR`) and reads its
//! rewrite back: the items are where the studio has them. The QA board is KiCad's, in its source tree: those tests run when `KICAD_SRC_DIR` names it.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, FootprintExtra, Point, Zone};
use eda_model::units::mm;
use eda_model::ConstraintModel;
use eda_ops::{Board, Cmd, FlipDirection};
use std::path::PathBuf;
use std::process::Command;

/// `issue7325.kicad_pcb` with a dimension and a table put into FH1 (at 111.4425, 92.3925, turned -90 in the file): the dimension from (108, 90) to (114, 90) on the
/// board, the table's one cell from (-2, -1) to (2, 1) in the footprint's frame.
fn qa_board() -> Option<String> {
    let src = std::env::var_os("KICAD_SRC_DIR")?;
    let text = std::fs::read_to_string(PathBuf::from(src).join("qa/data/pcbnew/issue7325.kicad_pcb")).ok()?;
    let at = text.find("(property \"Reference\" \"FH1\"")?;
    let start = text[..at].rfind("\n\t(footprint ")? + 1;
    // the footprint's closing parenthesis: the line that is a tab and `)`
    let end = start + text[start..].find("\n\t)")?;
    let items = r#"
		(dimension (type aligned) (layer "F.Fab") (uuid "7325d1d1-0000-0000-0000-000000000001") (pts (xy 108 90) (xy 114 90)) (height -1.5) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned))
		(table (column_count 1) (uuid "7325d1d1-0000-0000-0000-000000000002") (layer "F.Fab") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols yes) (stroke (width 0.1) (type solid))) (column_widths 4) (row_heights 2)
			(cells (table_cell "FH" (start -2 -1) (end 2 1) (margins 0.2 0.2 0.2 0.2) (span 1 1) (layer "F.Fab") (uuid "7325d1d1-0000-0000-0000-000000000003") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))))"#;
    Some(format!("{}{}{}", &text[..end], items, &text[end..]))
}

fn record<'a>(design: &'a Design, reference: &str) -> &'a FootprintExtra {
    design.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == reference).expect("the footprint's record")
}

fn own_zones<'a>(design: &'a Design, reference: &str) -> Vec<&'a Zone> {
    design.routing.as_ref().map(|r| r.zones.iter().filter(|z| z.parent_footprint.as_deref() == Some(reference)).collect()).unwrap_or_default()
}

/// The footprint's dimensions (start, end) and table cells' corners as the board has them, and its zones' first corners.
fn on_board(design: &Design, model: &ConstraintModel, reference: &str) -> (Vec<(Point, Point)>, Vec<Vec<Point>>, Vec<(Point, String)>) {
    let pose = design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == reference).unwrap().clone();
    let part = model.part(reference).unwrap();
    let items = eda_model::fp_items_place::placed(eda_model::fp_items_place::items_of(design, model, part).expect("the footprint's items"), &pose);
    let dims = items.dimensions.iter().map(|d| (d.start, d.end)).collect();
    let cells = items.tables.iter().map(|t| (0..t.cells.len()).flat_map(|i| { let (r, c) = t.row_col(i); t.cell_corners(r, c).to_vec() }).collect()).collect();
    let mut zones: Vec<(Point, String)> = own_zones(design, reference).iter().map(|z| (z.outline[0], z.layer.clone())).collect();
    zones.sort();
    (dims, cells, zones)
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() <= eda_model::ir::Nm::new(2) && (a.y - b.y).abs() <= eda_model::ir::Nm::new(2)
}

fn same(a: &(Vec<(Point, Point)>, Vec<Vec<Point>>, Vec<(Point, String)>), b: &(Vec<(Point, Point)>, Vec<Vec<Point>>, Vec<(Point, String)>)) -> bool {
    a.0.len() == b.0.len()
        && a.0.iter().zip(&b.0).all(|(x, y)| near(x.0, y.0) && near(x.1, y.1))
        && a.1.len() == b.1.len()
        && a.1.iter().zip(&b.1).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| near(*p, *q)))
        && a.2.len() == b.2.len()
        && a.2.iter().zip(&b.2).all(|(x, y)| near(x.0, y.0) && x.1 == y.1)
}

fn export(design: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-06", title: "pp14" }).expect("export")
}

/// The board read again from what we wrote: the design and the model the importer gives.
fn written_and_read(design: &Design, model: &ConstraintModel) -> (String, Design, ConstraintModel) {
    let text = export(design, model);
    let (d, m, _) = import_kicad_pcb(&text).expect("our board reads back");
    (text, d, m)
}

fn apply(design: &Design, model: &ConstraintModel, cmds: &[Cmd]) -> Design {
    let mut b = Board::new(design.clone(), model, mm(0.1), mm(0.3));
    for c in cmds {
        b.apply(c).unwrap_or_else(|e| panic!("{c:?}: {e:?}"));
    }
    b.into_design()
}

#[test]
fn a_qa_footprints_dimension_table_and_zones_are_its_own_through_our_reader_writer_and_its_transforms() {
    let Some(text) = qa_board() else {
        eprintln!("KICAD_SRC_DIR is not set: skipping the board of KiCad's own tests");
        return;
    };
    let (design, model, _) = import_kicad_pcb(&text).expect("import");
    let e = record(&design, "FH1");
    assert_eq!((e.items.dimensions.len(), e.items.tables.len(), own_zones(&design, "FH1").len()), (1, 1, 2), "the dimension and the table are FH1's items, its two zones its own");
    assert!(!e.tokens.iter().any(|t| t.starts_with("(dimension") || t.starts_with("(table")), "none is kept as the text of its form");
    let read = on_board(&design, &model, "FH1");
    assert!(near(read.0[0].0, Point { x: mm(108.0), y: mm(90.0) }) && near(read.0[0].1, Point { x: mm(114.0), y: mm(90.0) }), "the dimension where the file has it on the board: {:?}", read.0);
    // our writer, read again: the same, and the file has them inside FH1
    let (ours, back, back_model) = written_and_read(&design, &model);
    assert!(same(&read, &on_board(&back, &back_model, "FH1")), "written and read: {read:?} vs {:?}", on_board(&back, &back_model, "FH1"));
    let fh1 = &ours[ours.find("(property \"Reference\" \"FH1\"").unwrap()..];
    let fh1 = &fh1[..fh1.find("\n\t(footprint ").unwrap_or(fh1.len())];
    assert!(fh1.contains("(dimension") && fh1.contains("(table ") && fh1.contains("7325d1d1-0000-0000-0000-000000000003"), "inside FH1, the cell's uuid kept");

    // moved, turned and flipped with FH1: everything goes where FH1's pose takes it, and is written and read back there
    let moved = apply(&design, &model, &[
        Cmd::MoveItems { ids: vec!["FH1".into()], dx: mm(-20.0), dy: mm(5.0) },
        Cmd::RotateItems { ids: vec!["FH1".into()], pivot: Point { x: mm(91.4425), y: mm(97.3925) }, angle_millideg: 90_000 },
        Cmd::FlipItems { ids: vec!["FH1".into()], pivot: Point { x: mm(91.4425), y: mm(97.3925) }, direction: FlipDirection::LeftRight },
    ]);
    let after = on_board(&moved, &model, "FH1");
    assert!(!same(&read, &after), "FH1's items and zones moved with it");
    // each zone's corner is where the pose change takes it: off the board for the first pose, on for the last (the items' own frame does not change)
    let pose = |d: &Design| d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "FH1").unwrap().clone();
    let steps = eda_model::fp_items_place::repose_steps(&pose(&design), &pose(&moved));
    for (z0, z1) in own_zones(&design, "FH1").iter().zip(own_zones(&moved, "FH1")) {
        let want = steps.iter().fold(z0.outline[0], |p, s| s.point(p));
        assert!(near(z1.outline[0], want), "a zone corner {:?} -> {:?}, want {want:?}", z0.outline[0], z1.outline[0]);
        assert!(z1.layer.starts_with("B.") || z1.layer.starts_with("In"), "on the back now: {}", z1.layer);
    }
    assert_eq!(record(&moved, "FH1").zones_pose.map(|p| (p.at, p.rot, p.side)), Some((pose(&moved).at, pose(&moved).rot, pose(&moved).side)), "the record says where the zones were put for");
    let (_, back, back_model) = written_and_read(&moved, &model);
    assert!(same(&after, &on_board(&back, &back_model, "FH1")), "the moved, turned and flipped FH1 written and read: {after:?} vs {:?}", on_board(&back, &back_model, "FH1"));
    // and undone in the other order, everything is back where it was (to the nanometre for the quarter turns)
    let undone = apply(&moved, &model, &[
        Cmd::FlipItems { ids: vec!["FH1".into()], pivot: Point { x: mm(91.4425), y: mm(97.3925) }, direction: FlipDirection::LeftRight },
        Cmd::RotateItems { ids: vec!["FH1".into()], pivot: Point { x: mm(91.4425), y: mm(97.3925) }, angle_millideg: -90_000 },
        Cmd::MoveItems { ids: vec!["FH1".into()], dx: mm(20.0), dy: mm(-5.0) },
    ]);
    assert!(same(&read, &on_board(&undone, &model, "FH1")), "back: {read:?} vs {:?}", on_board(&undone, &model, "FH1"));
}

/// `Lib:Kept`: an SMD pad with no paste, a pad with a die length and a fabrication property, a description, a dimension, a table and a keepout.
const KEPT: &str = r#"(footprint "Kept" (version 20241229) (generator "pcbnew") (layer "F.Cu")
  (descr "a kept footprint") (tags "kept test")
  (property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Kept" (at 0 3 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (attr smd)
  (fp_rect (start -2.5 -2.5) (end 2.5 2.5) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
  (dimension (type aligned) (layer "F.Fab") (pts (xy -1 2) (xy 1 2)) (height 1) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned))
  (table (column_count 1) (layer "F.Fab") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols yes) (stroke (width 0.1) (type solid))) (column_widths 3) (row_heights 1.5)
    (cells (table_cell "T" (start -1.5 -2) (end 1.5 -0.5) (margins 0.2 0.2 0.2 0.2) (span 1 1) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))))
  (pad "1" smd roundrect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask") (roundrect_rratio 0.25) (property pad_prop_heatsink) (die_length 0.5))
  (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  (zone (net 0) (net_name "") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed)) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy -3 3) (xy 3 3) (xy 3 4) (xy -3 4))))
)"#;

/// A board of nothing but its outline, imported as a KiCad board is, with `Lib:Kept` placed at (50, 40) as the studio's server hands the command over (the library's
/// footprint in it) and read the way the studio reads the board again (the placed footprint a part of its own).
fn placed_kept() -> (Design, ConstraintModel) {
    let board = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (15 "B.Paste" user) (5 "F.SilkS" user) (7 "B.SilkS" user) (35 "F.Fab" user) (37 "B.Fab" user) (31 "F.CrtYd" user) (29 "B.CrtYd" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_rect (start 0 0) (end 100 80) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
)"#;
    let (design, model, _) = import_kicad_pcb(board).expect("import");
    let lib = eda_kicad::parse_library_footprint(KEPT).expect("the library footprint").footprint;
    let design = apply(&design, &model, &[Cmd::PlaceFootprint { footprint: "Lib:Kept".into(), at: Point { x: mm(50.0), y: mm(40.0) }, reference: "K1".into(), value: String::new(), definition: Some(lib) }]);
    // the board's own part, as the studio folds it in (`fold_board_parts`): the copy of the footprint the command left on it
    let mut model = model;
    let bp = design.drawings.as_ref().unwrap().board_parts.iter().find(|b| b.reference == "K1").unwrap().clone();
    let mut fp = bp.definition.as_ref().expect("the library's footprint kept with the part").to_engine_footprint();
    fp.name = bp.footprint.clone();
    model.footprints.push(fp);
    model.parts.push(eda_model::Part { reference: "K1".into(), mpn: None, lcsc: None, value: bp.value.clone(), package: None, footprint: Some(bp.footprint.clone()), symbol: None, datasheet: None, pins: vec![], body_nm: None, edge: None });
    (design, model)
}

/// The `(footprint ..)` of `reference` in the board file `text`, from its head (its `(descr ..)` comes before its fields) to the next footprint.
fn footprint_of<'a>(text: &'a str, reference: &str) -> &'a str {
    let at = text.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap_or_else(|| panic!("{reference}"));
    let start = text[..at].rfind("(footprint ").unwrap();
    let end = text[at..].find("(footprint ").map_or(text.len(), |e| at + e);
    &text[start..end]
}

/// What a written board says of K1 that the library says too.
fn assert_kept(text: &str, what: &str) {
    let k1 = footprint_of(text, "K1");
    let pad1 = k1.split("(pad ").find(|p| p.starts_with("\"1\"")).expect("pad 1");
    let layers = &pad1[pad1.find("(layers").unwrap()..];
    let layers = &layers[..layers.find(')').unwrap() + 1];
    assert!(!layers.contains("Paste"), "{what}: the pad with no paste has none: {layers}");
    assert!(pad1.contains("pad_prop_heatsink") && pad1.contains("(die_length 0.5"), "{what}: its property and die length: {pad1}");
    for form in ["(descr \"a kept footprint\")", "(dimension", "(table", "(keepout"] {
        // (KiCad's writer puts a form's head on a line of its own: `(table\n`)
        assert!(k1.contains(&format!("{form} ")) || k1.contains(&format!("{form}\n")) || k1.contains(&format!("{form}\t")) || form.ends_with(')') && k1.contains(form), "{what}: {form}");
    }
}

#[test]
fn a_footprint_placed_from_a_library_is_written_with_what_the_library_says() {
    let (design, model) = placed_kept();
    let (ours, back, back_model) = written_and_read(&design, &model);
    assert_kept(&ours, "our writer");
    let e = record(&back, "K1");
    assert_eq!((e.items.dimensions.len(), e.items.tables.len(), own_zones(&back, "K1").len()), (1, 1, 1));
    assert!(e.pads.iter().any(|p| p.no_paste), "read again, a pad has no paste");
    assert!(same(&on_board(&design, &model, "K1"), &on_board(&back, &back_model, "K1")), "the dimension, the table and the keepout read back where they were");
}

/// kicad-cli, when the slow tier asks for it.
fn cli() -> Option<PathBuf> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    Some(std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into()))
}

/// kicad-cli's rewrite of our board for `design` (`pcb upgrade --force`), and that read back.
fn through_kicad_cli(cli: &PathBuf, tag: &str, design: &Design, model: &ConstraintModel) -> (String, Design, ConstraintModel) {
    let dir = std::env::temp_dir().join(format!("eda_fp_record_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(&path, export(design, model)).unwrap();
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let (back, back_model, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    (theirs, back, back_model)
}

#[test]
fn kicad_cli_keeps_a_footprint_placed_from_a_library_with_its_no_paste_pad_and_its_dimension() {
    let Some(cli) = cli() else { return };
    let (design, model) = placed_kept();
    let (theirs, back, back_model) = through_kicad_cli(&cli, "placed", &design, &model);
    assert_kept(&theirs, "kicad-cli's rewrite");
    assert!(same(&on_board(&design, &model, "K1"), &on_board(&back, &back_model, "K1")), "kicad-cli keeps the dimension, the table and the keepout where we put them");
    // turned and flipped first: the same
    let turned = apply(&design, &model, &[
        Cmd::RotateItems { ids: vec!["K1".into()], pivot: Point { x: mm(50.0), y: mm(40.0) }, angle_millideg: 90_000 },
        Cmd::FlipItems { ids: vec!["K1".into()], pivot: Point { x: mm(50.0), y: mm(40.0) }, direction: FlipDirection::LeftRight },
    ]);
    let (theirs, back, back_model) = through_kicad_cli(&cli, "turned", &turned, &model);
    let k1 = footprint_of(&theirs, "K1");
    let pad1 = k1.split("(pad ").find(|p| p.starts_with("\"1\"")).unwrap();
    assert!(pad1.contains("\"B.Cu\"") && !pad1[..pad1.find("(uuid").unwrap_or(pad1.len())].contains("Paste"), "on the back, still no paste: {pad1}");
    assert!(same(&on_board(&turned, &model, "K1"), &on_board(&back, &back_model, "K1")), "turned and flipped: {:?} vs {:?}", on_board(&turned, &model, "K1"), on_board(&back, &back_model, "K1"));
}

#[test]
fn kicad_cli_keeps_a_qa_footprints_dimension_table_and_zones_where_we_put_them() {
    let Some(cli) = cli() else { return };
    let Some(text) = qa_board() else {
        eprintln!("KICAD_SRC_DIR is not set: skipping the board of KiCad's own tests");
        return;
    };
    let (design, model, _) = import_kicad_pcb(&text).expect("import");
    let moved = apply(&design, &model, &[
        Cmd::MoveItems { ids: vec!["FH1".into()], dx: mm(-20.0), dy: mm(5.0) },
        Cmd::RotateItems { ids: vec!["FH1".into()], pivot: Point { x: mm(91.4425), y: mm(97.3925) }, angle_millideg: 30_000 },
    ]);
    for (tag, d) in [("as_read", &design), ("moved", &moved)] {
        let (_, back, back_model) = through_kicad_cli(&cli, tag, d, &model);
        assert!(same(&on_board(d, &model, "FH1"), &on_board(&back, &back_model, "FH1")), "{tag}: {:?} vs {:?}", on_board(d, &model, "FH1"), on_board(&back, &back_model, "FH1"));
    }
}

/// The first corner of each zone of `reference`'s, as the board has them.
fn zone_corners(design: &Design, reference: &str) -> Vec<Point> {
    own_zones(design, reference).iter().map(|z| z.outline[0]).collect()
}

/// `FOOTPRINT::Duplicate`, `ARRAY_TOOL` and Paste clone a footprint whole: the copy has the footprint's zone, which then goes with the copy, not with the original.
#[test]
fn duplicate_create_array_and_paste_of_a_footprint_carry_its_zone() {
    let (design, model) = placed_kept();
    let k1 = zone_corners(&design, "K1");
    assert_eq!(k1, vec![Point { x: mm(47.0), y: mm(43.0) }]);
    // Duplicate, then the Move tool's move of the copy
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::Duplicate { ids: vec!["K1".into()] }).unwrap();
    let copy = b.design().placement.as_ref().unwrap().footprints.iter().map(|f| f.id.clone()).find(|r| r != "K1").expect("the copy");
    assert_eq!(zone_corners(b.design(), &copy), k1, "the copy has the zone, where the copy is");
    b.apply(&Cmd::MoveItems { ids: vec![copy.clone()], dx: mm(10.0), dy: eda_model::ir::Nm::ZERO }).unwrap();
    assert_eq!(zone_corners(b.design(), &copy), vec![Point { x: mm(57.0), y: mm(43.0) }], "the copy's zone goes with the copy");
    assert_eq!(zone_corners(b.design(), "K1"), k1, "the original's stays");
    // Create Array: each copy's zone where its copy is put
    let array: eda_ops::ArrayGeometry = serde_json::from_value(serde_json::json!({ "kind": "grid", "nx": 3, "ny": 1, "dx": 5_000_000, "dy": 0 })).expect("a grid");
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::CreateArray { ids: vec!["K1".into()], geometry: array, arrange: false, reannotate: true, group_positions: Default::default() }).unwrap();
    let mut corners: Vec<Point> = b.design().placement.as_ref().unwrap().footprints.iter().flat_map(|f| zone_corners(b.design(), &f.id)).collect();
    corners.sort();
    assert_eq!(corners, vec![Point { x: mm(47.0), y: mm(43.0) }, Point { x: mm(52.0), y: mm(43.0) }, Point { x: mm(57.0), y: mm(43.0) }], "one zone per footprint, each with its own");
    // Paste of the footprint's clipboard text: the pasted footprint's zone is its own, not the board's
    let text = eda_kicad::export_pcb_clipboard(&design, &model, &["K1".to_string()], Point { x: mm(50.0), y: mm(40.0) }).expect("the clipboard");
    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::PasteClipboard { text, at: Point { x: mm(20.0), y: mm(0.0) } }).unwrap();
    let pasted = b.design().placement.as_ref().unwrap().footprints.iter().map(|f| f.id.clone()).find(|r| r != "K1").expect("the pasted footprint");
    assert_eq!(own_zones(b.design(), &pasted).len(), 1, "the pasted footprint has its zone");
    assert!(b.design().routing.as_ref().unwrap().zones.iter().all(|z| z.parent_footprint.is_some()), "no zone of the board's own came of it");
}
