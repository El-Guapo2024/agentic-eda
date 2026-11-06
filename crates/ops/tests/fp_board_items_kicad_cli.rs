//! A board footprint's text boxes, reference images, points and barcodes (UI-LEFT P17, `eda_model::fp_items_place`, `eda_kicad`'s `fp_board_items_io`): read into the
//! footprint's items, carried by Move, Rotate and Flip of the footprint, copied by Duplicate, Create Array and Paste, an image's pixels set with the turn, and written
//! back as KiCad writes them. The slow tier (`EDA_SLOW_TESTS=1`) hands the file to kicad-cli: `pcb upgrade --force` (`PCB_IO_KICAD_SEXPR_PARSER`, then
//! `PCB_IO_KICAD_SEXPR`) keeps every item where the studio has it, with the image's pixels byte for byte, and kicad-cli plots the footprint's text box and barcode
//! exactly where it plots the same items put on the board by themselves at the places the studio's canvas draws them.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::fp_items::FootprintItems;
use eda_model::fp_items_place::placed;
use eda_model::ir::{Design, Point, Side};
use eda_model::units::mm;
use eda_model::ConstraintModel;
use eda_ops::{ArrayGeometry, Board, Cmd, FlipDirection};
use std::path::PathBuf;
use std::process::Command;

fn png(w: u32, h: u32, rgb: [u8; 3]) -> String {
    eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(w, h, rgb, 300))
}

fn data_lines(data: &str) -> String {
    data.as_bytes().chunks(76).map(|c| format!("\"{}\"", std::str::from_utf8(c).unwrap())).collect::<Vec<_>>().join(" ")
}

/// U1 at (50, 40) holds a text box (in its own frame, as the file has it), an image, a point and a barcode (where they are on the board, as the file has them).
fn board_text() -> String {
    format!(
        r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (15 "B.Paste" user) (5 "F.SilkS" user) (7 "B.SilkS" user) (35 "F.Fab" user) (37 "B.Fab" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "A")
  (gr_rect (start 0 0) (end 100 80) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (footprint "Lib:Logo" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000011") (at 50 40 0)
    (property "Reference" "U1" (at 0 -6 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "Logo" (at 0 6 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (fp_line (start -5 -4) (end 5 -4) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000014"))
    (fp_text_box "REV ${{REFERENCE}}" (start 1 1) (end 9 4) (margins 0.5 0.5 0.5 0.5) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000015")
      (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.12) (type solid)))
    (image (at 44 40) (layer "F.SilkS") (data {}) (uuid "00000000-0000-0000-0000-000000000016"))
    (barcode (at 50 34 0) (layer "F.SilkS") (size 3 3) (text "U1") (text_height 1) (type qr) (ecc_level L) (hide yes) (knockout no) (uuid "00000000-0000-0000-0000-000000000017"))
    (point (at 53 42) (size 1) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000018"))
    (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-000000000019"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-00000000001a"))
  )
)"#,
        data_lines(&png(24, 12, [200, 30, 30]))
    )
}

fn items_of<'a>(design: &'a Design, reference: &str) -> &'a FootprintItems {
    &design.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == reference).unwrap_or_else(|| panic!("{reference} has its record")).items
}

fn write(design: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-06", title: "fp kinds" }).expect("export")
}

/// The four kinds of U1 as the board has them, in board coordinates.
fn on_board(design: &Design, reference: &str) -> FootprintItems {
    let pose = design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == reference).unwrap();
    placed(items_of(design, reference), pose)
}

#[test]
fn the_items_are_read_into_the_footprint_and_written_back_where_kicad_has_them() {
    let (design, _model, _) = import_kicad_pcb(&board_text()).expect("import");
    let e = design.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "U1").unwrap();
    assert!(!e.tokens.iter().any(|t| t.starts_with("(fp_text_box") || t.starts_with("(image") || t.starts_with("(point") || t.starts_with("(barcode")), "no longer kept as text: {:?}", e.tokens);
    let items = &e.items;
    assert_eq!((items.text_boxes.len(), items.images.len(), items.points.len(), items.barcodes.len()), (1, 1, 1, 1));
    // the frame of the pads: the image 6 mm left of the footprint, the barcode 6 mm above it, the point at (3, 2)
    assert_eq!(items.images[0].at, Point { x: mm(-6.0), y: mm(0.0) });
    assert_eq!(items.barcodes[0].at, Point { x: mm(0.0), y: mm(-6.0) });
    assert_eq!(items.points[0].at, Point { x: mm(3.0), y: mm(2.0) });
    assert_eq!((items.text_boxes[0].start, items.text_boxes[0].end), (Point { x: mm(1.0), y: mm(1.0) }, Point { x: mm(9.0), y: mm(4.0) }));
}

/// U1 turned 30 degrees, then flipped and moved: every item goes with the footprint as KiCad's footprint takes its children (`FOOTPRINT::SetOrientation`, `Flip`, `SetPosition`).
fn transformed() -> (Design, ConstraintModel) {
    let (design, model, _) = import_kicad_pcb(&board_text()).expect("import");
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    let at = Point { x: mm(50.0), y: mm(40.0) };
    b.apply(&Cmd::RotateItems { ids: vec!["U1".into()], pivot: at, angle_millideg: -30_000 }).expect("rotate");
    b.apply(&Cmd::FlipItems { ids: vec!["U1".into()], pivot: at, direction: FlipDirection::LeftRight }).expect("flip");
    b.apply(&Cmd::MoveItems { ids: vec!["U1".into()], dx: mm(5.0), dy: mm(-3.0) }).expect("move");
    (b.into_design(), model)
}

#[test]
fn move_rotate_and_flip_carry_the_items_and_the_file_reads_back_the_same() {
    let (design, model) = import_kicad_pcb(&board_text()).map(|(d, m, _)| (d, m)).unwrap();
    let before = on_board(&design, "U1");
    let (after, model2) = transformed();
    let _ = model;
    let pose = after.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U1").unwrap().clone();
    assert_eq!(pose.side, Side::Bottom);
    assert_eq!(items_of(&after, "U1"), items_of(&design, "U1"), "the items are the footprint's: the verbs change its pose, not them");
    let now = on_board(&after, "U1");
    assert_ne!(now.images[0].at, before.images[0].at);
    assert_eq!((now.text_boxes[0].layer.as_str(), now.text_boxes[0].mirror), ("B.SilkS", true), "flipped to the back, its text mirrored");
    assert_eq!((now.points[0].layer.as_str(), now.barcodes[0].layer.as_str(), now.images[0].layer.as_str()), ("B.Fab", "B.SilkS", "B.SilkS"));
    assert!(now.text_boxes[0].pts.len() == 4, "a box turned 30 degrees is the polygon of its corners");
    // written and read back: the same items, in the same places on the board
    let text = write(&after, &model2);
    let u1 = &text[text.find("(footprint \"Lib:Logo\"").unwrap()..];
    assert!(u1.contains("(fp_text_box \"REV ${REFERENCE}\"") && u1.contains("(barcode") && u1.contains("(point") && u1.contains("(image"), "{u1}");
    let (back, _, _) = import_kicad_pcb(&text).expect("read back");
    let again = on_board(&back, "U1");
    let near = |a: Point, b: Point| (a.x - b.x).abs() <= eda_model::ir::Nm::new(1) && (a.y - b.y).abs() <= eda_model::ir::Nm::new(1);
    assert!(near(again.images[0].at, now.images[0].at) && near(again.points[0].at, now.points[0].at) && near(again.barcodes[0].at, now.barcodes[0].at), "{again:?}\n{now:?}");
    assert_eq!(again.barcodes[0].angle, now.barcodes[0].angle);
    assert!(again.text_boxes[0].pts.iter().zip(&now.text_boxes[0].pts).all(|(a, b)| near(*a, *b)), "{:?} {:?}", again.text_boxes[0], now.text_boxes[0]);
    assert_eq!((again.text_boxes[0].angle, again.text_boxes[0].mirror, again.text_boxes[0].layer.as_str()), (now.text_boxes[0].angle, true, "B.SilkS"));
    assert_eq!(again.images[0].data, now.images[0].data, "the pixels as they were");
}

#[test]
fn duplicate_create_array_and_paste_copy_the_items_with_the_footprint() {
    let (design, model, _) = import_kicad_pcb(&board_text()).expect("import");
    let original = items_of(&design, "U1").clone();
    let copies = |d: &Design| d.drawings.as_ref().unwrap().footprint_extras.iter().filter(|e| e.id != "U1" && e.items == original).count();

    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::Duplicate { ids: vec!["U1".into()] }).expect("duplicate");
    assert_eq!(copies(b.design()), 1, "the duplicate has the footprint's items");

    let mut b = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    let grid = ArrayGeometry::Grid { nx: 3, ny: 1, dx: mm(15.0), dy: mm(0.0), offset_x: mm(0.0), offset_y: mm(0.0), centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false };
    b.apply(&Cmd::CreateArray { ids: vec!["U1".into()], geometry: grid, arrange: false, reannotate: true, group_positions: Default::default() }).expect("array");
    assert_eq!(copies(b.design()), 2, "every copy of the array has them");

    let clip = eda_kicad::export_pcb_clipboard(&design, &model, &["U1".to_string()], Point { x: mm(50.0), y: mm(40.0) }).expect("copy");
    assert!(clip.contains("(fp_text_box") && clip.contains("(barcode") && clip.contains("(point") && clip.contains("(image"), "{clip}");
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::PasteClipboard { text: clip, at: Point { x: mm(20.0), y: mm(20.0) } }).expect("paste");
    let pasted = b.design().drawings.as_ref().unwrap().footprint_extras.iter().filter(|e| e.id != "U1").map(|e| e.items.clone()).collect::<Vec<_>>();
    assert_eq!(pasted.len(), 1);
    assert_eq!(pasted[0].text_boxes.len() + pasted[0].images.len() + pasted[0].points.len() + pasted[0].barcodes.len(), 4, "the paste has the four: {:?}", pasted[0]);
    assert_eq!(pasted[0].images[0].data, original.images[0].data);
}

#[test]
fn the_pixels_a_turn_sends_are_the_footprints_images_in_the_same_step() {
    let (design, model, _) = import_kicad_pcb(&board_text()).expect("import");
    let id = format!("U1~{}", items_of(&design, "U1").images[0].id);
    let turned = png(12, 24, [200, 30, 30]);
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    let rotate = Cmd::RotateItems { ids: vec!["U1".into()], pivot: Point { x: mm(50.0), y: mm(40.0) }, angle_millideg: -90_000 };
    let pixels = Cmd::PcbItem(eda_ops::pcb_items::PcbItemCmd::SetRefImageData { id: id.clone(), data: turned.clone() });
    b.apply(&Cmd::Batch { cmds: vec![rotate, pixels] }).expect("one batch");
    assert_eq!(items_of(b.design(), "U1").images[0].data, turned);
    let unknown = Cmd::PcbItem(eda_ops::pcb_items::PcbItemCmd::SetRefImageData { id: "U1~img_000000000000".into(), data: turned });
    assert_eq!(b.apply(&unknown).unwrap_err()[0].check, "ops_unknown_ref_image");
}

// ---------------------------------------------------------------------------------------------------------------------------------------- kicad-cli

fn cli() -> Option<PathBuf> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    Some(std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into()))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fp_board_items_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn upgrade(cli: &PathBuf, path: &PathBuf) -> String {
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    std::fs::read_to_string(path).unwrap()
}

/// kicad-cli's SVG plot of `layers`, the lines that say when and by whom left out, the drawing primitives sorted (the order they are plotted in is not what is compared).
fn plot(cli: &PathBuf, board: &PathBuf, layers: &str, out: &PathBuf) -> Vec<String> {
    let done = Command::new(cli).args(["pcb", "export", "svg", "--mode-single", "--exclude-drawing-sheet", "--page-size-mode", "2", "--layers", layers, "-o"]).arg(out).arg(board).output().expect("kicad-cli runs");
    assert!(done.status.success(), "kicad-cli did not plot {}: {}{}", board.display(), String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr));
    let svg = std::fs::read_to_string(out).unwrap();
    let mut lines: Vec<String> = svg.lines().map(str::trim).filter(|l| l.starts_with("<path") || l.starts_with("<circle") || l.starts_with("<polyline") || l.starts_with("<polygon")).map(str::to_string).collect();
    lines.sort();
    lines
}

#[test]
fn kicad_cli_keeps_the_items_of_a_turned_and_flipped_footprint_where_the_studio_has_them() {
    let Some(cli) = cli() else { return };
    let (design, model) = transformed();
    let dir = scratch("upgrade");
    let path = dir.join("fp_kinds.kicad_pcb");
    std::fs::write(&path, write(&design, &model)).unwrap();
    let theirs = upgrade(&cli, &path);
    let u1 = &theirs[theirs.find("(footprint \"Lib:Logo\"").unwrap()..];
    assert!(u1.contains("(fp_text_box") && u1.contains("(barcode") && u1.contains("(point") && u1.contains("(image"), "kicad-cli kept the four in the footprint:\n{u1}");
    let (back, _, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    let (ours, again) = (on_board(&design, "U1"), on_board(&back, "U1"));
    let near = |a: Point, b: Point| (a.x - b.x).abs() <= eda_model::ir::Nm::new(1) && (a.y - b.y).abs() <= eda_model::ir::Nm::new(1);
    assert!(near(again.images[0].at, ours.images[0].at) && near(again.points[0].at, ours.points[0].at) && near(again.barcodes[0].at, ours.barcodes[0].at), "kicad-cli's rewrite:\n{u1}");
    assert_eq!((again.barcodes[0].angle, again.barcodes[0].layer.as_str()), (ours.barcodes[0].angle, ours.barcodes[0].layer.as_str()));
    let (t, u) = (&again.text_boxes[0], &ours.text_boxes[0]);
    assert!(t.pts.iter().zip(&u.pts).all(|(a, b)| near(*a, *b)) && t.pts.len() == u.pts.len(), "{t:?}\n{u:?}");
    assert_eq!((t.angle, t.mirror, t.layer.as_str(), t.text.as_str()), (u.angle, u.mirror, u.layer.as_str(), u.text.as_str()));
    assert_eq!(again.images[0].data, ours.images[0].data, "the pixels byte for byte");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kicad_cli_plots_the_footprints_text_box_and_barcode_where_the_studio_draws_them() {
    let Some(cli) = cli() else { return };
    let (design, model) = transformed();
    let dir = scratch("plot");
    // the footprint's own items, as kicad-cli places them
    let fp_path = dir.join("in_footprint.kicad_pcb");
    std::fs::write(&fp_path, write(&design, &model)).unwrap();
    // the same items taken out of the footprint and put on the board where the studio's canvas draws them (`placed`)
    let mut loose = design.clone();
    let shown = on_board(&design, "U1");
    let dr = loose.drawings.as_mut().unwrap();
    let e = dr.footprint_extras.iter_mut().find(|e| e.id == "U1").unwrap();
    e.items = FootprintItems::default();
    for mut t in shown.text_boxes.clone() {
        // the board's own box shows `${REFERENCE}` as it is; the footprint's shows U1
        t.text = t.text.replace("${REFERENCE}", "U1");
        t.id.clear();
        dr.board_items.text_boxes.push(t);
    }
    for mut b in shown.barcodes.clone() {
        b.id.clear();
        dr.barcodes.push(b);
    }
    dr.assign_missing_ids();
    let loose_path = dir.join("on_board.kicad_pcb");
    std::fs::write(&loose_path, write(&loose, &model)).unwrap();
    let layers = "B.SilkS";
    let a = plot(&cli, &fp_path, layers, &dir.join("a.svg"));
    let b = plot(&cli, &loose_path, layers, &dir.join("b.svg"));
    assert!(a.len() > 10, "the plot has the box, its text and the code: {a:?}");
    assert_eq!(a, b, "the footprint's text box and barcode plot where the board's own do at the places the canvas draws them");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kicad_cli_keeps_an_images_turned_pixels() {
    let Some(cli) = cli() else { return };
    let (design, model, _) = import_kicad_pcb(&board_text()).expect("import");
    let id = format!("U1~{}", items_of(&design, "U1").images[0].id);
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    let at = Point { x: mm(50.0), y: mm(40.0) };
    // a quarter turn and a flip: the 24 x 12 image is 12 x 24 now, which the browser would send (here a file of that size stands for it)
    let turned = png(12, 24, [30, 30, 200]);
    let cmds = vec![
        Cmd::RotateItems { ids: vec!["U1".into()], pivot: at, angle_millideg: -90_000 },
        Cmd::FlipItems { ids: vec!["U1".into()], pivot: at, direction: FlipDirection::LeftRight },
        Cmd::PcbItem(eda_ops::pcb_items::PcbItemCmd::SetRefImageData { id, data: turned.clone() }),
    ];
    b.apply(&Cmd::Batch { cmds }).expect("one batch");
    let design = b.into_design();
    let dir = scratch("pixels");
    let path = dir.join("pixels.kicad_pcb");
    std::fs::write(&path, write(&design, &model)).unwrap();
    let theirs = upgrade(&cli, &path);
    let (back, _, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    let img = &on_board(&back, "U1").images[0];
    assert_eq!(img.data, turned, "kicad-cli wrote the turned file as it read it");
    assert_eq!(img.size_nm(), (mm(12.0 * 25.4 / 300.0), mm(24.0 * 25.4 / 300.0)), "12 x 24 pixels at 300 per inch");
    assert_eq!(img.layer, "B.SilkS");
    let _ = std::fs::remove_dir_all(&dir);
}
