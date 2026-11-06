//! A board footprint's text boxes, reference images, points and barcodes through a board directory (UI-LEFT P17): the state draws them where the footprint puts them
//! (`parts[].items`, `crate::fp_board_items_json`), their files and shapes are served by the ids the state gives them, a turn of the footprint and the pixels of its
//! image are one undo step, and the sheet's image takes the pixels a turn sends (`set_image_data`).

use eda_model::units::mm;
use super::tests::{scratch, setup_schematic};
use super::{load, step, undo};
use eda_model::ir::Point;
use eda_model::sch_extras::{SchGraphic, SchGraphicKind};
use eda_ops::sch_edit::SchCmd;
use eda_ops::{pcb_items::PcbItemCmd, Cmd, Domain};
use std::path::{Path, PathBuf};

fn png(w: u32, h: u32) -> String {
    eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(w, h, [200, 30, 30], 300))
}

/// A board of one footprint, U1 at (50, 40), holding a text box that shows its reference, an image, a point and a barcode, imported as a KiCad board is.
fn imported(name: &str) -> PathBuf {
    let dir = scratch(name);
    let data = png(24, 12);
    let lines = data.as_bytes().chunks(76).map(|c| format!("\"{}\"", std::str::from_utf8(c).unwrap())).collect::<Vec<_>>().join(" ");
    let text = format!(
        r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (5 "F.SilkS" user) (7 "B.SilkS" user) (35 "F.Fab" user) (37 "B.Fab" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_rect (start 0 0) (end 100 80) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (footprint "Lib:Logo" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000011") (at 50 40 0)
    (property "Reference" "U1" (at 0 -6 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "Logo" (at 0 6 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (fp_text_box "REV ${{REFERENCE}}" (start 1 1) (end 9 4) (margins 0.5 0.5 0.5 0.5) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000015")
      (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.12) (type solid)))
    (image (at 44 40) (layer "F.SilkS") (data {lines}) (uuid "00000000-0000-0000-0000-000000000016"))
    (barcode (at 50 34 0) (layer "F.SilkS") (size 3 3) (text "${{REFERENCE}}") (text_height 1) (type qr) (ecc_level L) (hide yes) (knockout no) (uuid "00000000-0000-0000-0000-000000000017"))
    (point (at 53 42) (size 1) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000018"))
    (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-000000000019"))
  )
)"#
    );
    let file = dir.join("logo.kicad_pcb");
    std::fs::write(&file, text).unwrap();
    let out = dir.join("board");
    crate::import_kicad::run(&[file.display().to_string(), "-o".into(), out.display().to_string()]).unwrap();
    out
}

fn state(dir: &Path) -> serde_json::Value {
    let job: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
    crate::studio::state(dir, &job, crate::text_vars_json::clock_of("/api/state")).unwrap()
}

fn items(dir: &Path) -> serde_json::Value {
    state(dir)["parts"].as_array().unwrap().iter().find(|p| p["ref"] == "U1").unwrap()["items"].clone()
}

#[test]
fn the_state_draws_a_footprints_items_where_the_footprint_puts_them_and_serves_them_by_their_ids() {
    let dir = imported("p17_fp_items_state");
    let it = items(&dir);
    let tb = &it["text_boxes"][0];
    assert!(tb["id"].as_str().unwrap().starts_with("U1~tbx_"), "{it}");
    assert_eq!((tb["start"].clone(), tb["end"].clone()), (serde_json::json!([51_000_000, 41_000_000]), serde_json::json!([59_000_000, 44_000_000])), "on the board");
    assert_eq!(tb["shown"], "REV U1", "the footprint's variables resolve");
    assert_eq!(it["barcodes"][0]["shown"], "U1");
    let img = &it["images"][0];
    assert_eq!((img["x"].as_i64(), img["y"].as_i64(), img["px"].clone()), (Some(44_000_000), Some(40_000_000), serde_json::json!([24, 12])));
    assert_eq!((it["points"][0]["x"].as_i64(), it["points"][0]["y"].as_i64()), (Some(53_000_000), Some(42_000_000)));

    // the image's file and the barcode's shape, by the ids the state gives them
    let image_id = img["id"].as_str().unwrap().to_string();
    let reply = crate::image_api::pcb_image(&dir, &format!("/api/pcb_image?id={image_id}"));
    assert_eq!((reply.status, reply.kind), ("200 OK", "image/png"));
    let barcode = crate::barcode_api::shape_route(&dir, &format!("/api/barcode_shape?id={}", it["barcodes"][0]["id"].as_str().unwrap()));
    assert_ne!(barcode.status, "404 Not Found", "{}", barcode.body);

    // a quarter turn of the footprint and the image's turned pixels, one step: the items move with it, one undo takes both back
    let turned = png(12, 24);
    let rotate = Cmd::RotateItems { ids: vec!["U1".into()], pivot: Point { x: mm(50.0), y: mm(40.0) }, angle_millideg: -90_000 };
    let pixels = Cmd::PcbItem(PcbItemCmd::SetRefImageData { id: image_id.clone(), data: turned.clone() });
    step(&dir, Cmd::Batch { cmds: vec![rotate, pixels] }, false, "test").unwrap();
    let it = items(&dir);
    assert_eq!((it["images"][0]["x"].as_i64(), it["images"][0]["y"].as_i64(), it["images"][0]["px"].clone()), (Some(50_000_000), Some(46_000_000), serde_json::json!([12, 24])), "a quarter turn counter-clockwise takes the image below the footprint, its pixels turned");
    assert_eq!((it["points"][0]["x"].as_i64(), it["points"][0]["y"].as_i64()), (Some(52_000_000), Some(37_000_000)));
    let (_, design, _) = load(&dir).unwrap();
    assert_eq!(design.drawings.unwrap().footprint_extras.iter().find(|e| e.id == "U1").unwrap().items.images[0].data, turned);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    let it = items(&dir);
    assert_eq!((it["images"][0]["x"].as_i64(), it["images"][0]["px"].clone()), (Some(44_000_000), serde_json::json!([24, 12])), "one undo takes the turn and the pixels back");
}

#[test]
fn a_sheet_image_takes_the_pixels_a_turn_sends_in_the_same_step() {
    let dir = scratch("p17_sch_image_pixels");
    setup_schematic(&dir);
    let graphic = SchGraphic::new(SchGraphicKind::Image { at: Point { x: mm(50.8), y: mm(38.1) }, scale_ppm: 1_000_000, data: png(24, 12) });
    step(&dir, Cmd::sch(SchCmd::AddGraphic { graphic }), false, "test").unwrap();
    let data_of = |dir: &Path| -> (String, String) {
        let (_, design, _) = load(dir).unwrap();
        let g = design.schematic.unwrap().extras.graphics.into_iter().find(|g| matches!(g.shape, SchGraphicKind::Image { .. })).unwrap();
        match g.shape {
            SchGraphicKind::Image { data, .. } => (g.id, data),
            _ => unreachable!(),
        }
    };
    let (id, before) = data_of(&dir);
    let turned = png(12, 24);
    let rotate = Cmd::SchMove(eda_ops::sch_move::SchMoveCmd::Rotate { ids: vec![id.clone()], vertices: Default::default(), ccw: true, about: None, grid: eda_model::ir::Nm::ZERO });
    step(&dir, Cmd::Batch { cmds: vec![rotate, Cmd::sch(SchCmd::SetImageData { id: id.clone(), data: turned.clone() })] }, false, "test").unwrap();
    assert_eq!(data_of(&dir).1, turned);
    assert_eq!(step(&dir, Cmd::sch(SchCmd::SetImageData { id: id.clone(), data: "AAAA".into() }), false, "test").unwrap_err()[0].check, "ops_bad_graphic");
    assert_eq!(step(&dir, Cmd::sch(SchCmd::SetImageData { id: "nope".into(), data: turned }), false, "test").unwrap_err()[0].check, "ops_unknown_graphic");
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_eq!(data_of(&dir).1, before, "one undo takes the turn and the pixels back");
}
