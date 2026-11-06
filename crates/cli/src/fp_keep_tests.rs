//! A board footprint's own items through a board directory (UI-LEFT P30): Ctrl+E's copy shows them and Update Footprint on Board keeps them (with one drawn in the
//! copy), the state draws them after the board is read again, Update Footprint from Library and Change Footprint take the library footprint's items through the
//! library tables (the server puts them in the command, `library_items`), one undo takes an exchange back, and Update PCB from Schematic proposes nothing that
//! would drop them.

use super::tests::scratch;
use super::{load, step, undo};
use eda_model::ir::{BoardFootprintCopy, Point};
use eda_model::pcb_items::TextBox;
use eda_model::units::mm;
use eda_ops::fp_editor::FpEditorCmd;
use eda_ops::fp_items::{FpItem, FpItemCmd};
use eda_ops::{Cmd, Domain, ExchangeOptions};
use std::path::{Path, PathBuf};

/// A board of one footprint, U1 (`Lib:Logo`) at (50, 40), holding a user text, a text box, an image and a barcode, imported as a KiCad board is; the library `Lib`
/// in its tables holds `Logo` (a text box of its own) and `Other` (a barcode of its own).
fn imported(name: &str) -> PathBuf {
    let dir = scratch(name);
    let data = eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(24, 12, [200, 30, 30], 300));
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
    (fp_text user "NOTE" (at 0 3 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-00000000001d") (effects (font (size 1 1) (thickness 0.15))))
    (fp_text_box "REV ${{REFERENCE}}" (start 1 1) (end 9 4) (margins 0.5 0.5 0.5 0.5) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000015")
      (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.12) (type solid)))
    (image (at 44 40) (layer "F.SilkS") (data {lines}) (uuid "00000000-0000-0000-0000-000000000016"))
    (barcode (at 50 34 0) (layer "F.SilkS") (size 3 3) (text "U1") (text_height 1) (type qr) (ecc_level L) (hide yes) (knockout no) (uuid "00000000-0000-0000-0000-000000000017"))
    (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-000000000019"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "00000000-0000-0000-0000-00000000001a"))
  )
)"#
    );
    let file = dir.join("logo.kicad_pcb");
    std::fs::write(&file, text).unwrap();
    let out = dir.join("board");
    crate::import_kicad::run(&[file.display().to_string(), "-o".into(), out.display().to_string()]).unwrap();
    let lib = out.join("libs").join("Lib.pretty");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(
        lib.join("Logo.kicad_mod"),
        r#"(footprint "Logo" (version 20241229) (generator "pcbnew") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -6 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Logo" (at 0 6 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (attr smd)
  (fp_text_box "LIB BOX" (start -4 1) (end 4 3) (margins 0.5 0.5 0.5 0.5) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.1) (type solid)))
  (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))"#,
    )
    .unwrap();
    std::fs::write(
        lib.join("Other.kicad_mod"),
        r#"(footprint "Other" (version 20241229) (generator "pcbnew") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -6 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Other" (at 0 6 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (attr smd)
  (barcode (at 0 -3 0) (layer "F.SilkS") (size 2 2) (text "OTHER") (text_height 1) (type qr) (ecc_level L) (hide yes) (knockout no))
  (pad "1" smd rect (at -2.5 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd rect (at 2.5 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))"#,
    )
    .unwrap();
    assert_eq!(crate::library_tables::add_library(&out, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/Lib.pretty"}"#)["ok"], true);
    out
}

/// U1's items as the state draws them: the texts of its text boxes and barcodes, and how many images.
fn drawn(dir: &Path) -> (Vec<String>, Vec<String>, usize) {
    let job: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
    let state = crate::studio::state(dir, &job, crate::text_vars_json::clock_of("/api/state")).unwrap();
    let it = state["parts"].as_array().unwrap().iter().find(|p| p["ref"] == "U1").unwrap()["items"].clone();
    // (`shown` is the text with the footprint's variables resolved, where it has any)
    let texts = |kind: &str| it[kind].as_array().map(|a| a.iter().map(|t| t["shown"].as_str().or(t["text"].as_str()).unwrap_or_default().to_string()).collect()).unwrap_or_default();
    (texts("text_boxes"), texts("barcodes"), it["images"].as_array().map_or(0, Vec::len))
}

/// The user texts of U1's footprint, as the board has them.
fn user_texts(dir: &Path) -> Vec<String> {
    let (_, _, model) = load(dir).unwrap();
    model.footprint_of(model.part("U1").unwrap()).unwrap().art.texts.iter().map(|t| t.text.clone()).collect()
}

#[test]
fn a_footprints_own_items_survive_the_editors_copy_and_follow_the_library_on_an_exchange() {
    let dir = imported("p30_fp_keep");
    assert_eq!(drawn(&dir), (vec!["REV U1".to_string()], vec!["U1".to_string()], 1));

    // Ctrl+E: the copy shows them; a text box drawn in it; Update Footprint on Board
    step(&dir, Cmd::FpEditor(FpEditorCmd::OpenFromBoard { reference: "U1".into() }), false, "test").unwrap();
    let key = BoardFootprintCopy::key_of("U1");
    let (_, design, _) = load(&dir).unwrap();
    let copy = design.footprint_library.as_ref().unwrap().by_name(&key).unwrap().clone();
    assert_eq!((copy.items.text_boxes.len(), copy.items.images.len(), copy.items.barcodes.len()), (1, 1, 1), "the copy shows the footprint's items");
    let mut drawn_box = TextBox::new("F.Fab", Point { x: mm(-4.0), y: mm(4.0) }, Point { x: mm(4.0), y: mm(6.0) }, mm(1.0), mm(0.1));
    drawn_box.text = "DRAWN".into();
    step(&dir, Cmd::FpItem(FpItemCmd::Add { footprint: key.clone(), item: FpItem::TextBox(drawn_box) }), false, "test").unwrap();
    step(&dir, Cmd::FpEditor(FpEditorCmd::SaveToBoard { footprint: key }), false, "test").unwrap();
    // read again from the board's directory: all of them, and the user text
    assert_eq!(drawn(&dir), (vec!["REV U1".to_string(), "DRAWN".to_string()], vec!["U1".to_string()], 1));
    assert_eq!(user_texts(&dir), ["NOTE"]);
    // Update PCB from Schematic has nothing to change on it
    let pending = crate::fp_assign_api::update_pcb(&dir);
    assert_eq!(pending["changes"].as_array().map(Vec::len), Some(0), "{pending}");

    // Update Footprint from Library: the library's text box (the server read it through the tables), the user text kept
    let update = Cmd::ExchangeFootprints { references: vec!["U1".into()], footprint: None, options: ExchangeOptions::update(), library: Default::default(), library_items: Default::default() };
    step(&dir, update, false, "test").unwrap();
    assert_eq!(drawn(&dir), (vec!["LIB BOX".to_string()], vec![], 0));
    assert_eq!(user_texts(&dir), ["NOTE"]);
    // one undo takes it back, with the board's own items
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(drawn(&dir), (vec!["REV U1".to_string(), "DRAWN".to_string()], vec!["U1".to_string()], 1));

    // Change Footprint: the new footprint's barcode
    let change = Cmd::ExchangeFootprints { references: vec!["U1".into()], footprint: Some("Lib:Other".into()), options: ExchangeOptions::change(), library: Default::default(), library_items: Default::default() };
    step(&dir, change, false, "test").unwrap();
    assert_eq!(drawn(&dir), (vec![], vec!["OTHER".to_string()], 0));
    assert_eq!(user_texts(&dir), ["NOTE"]);
}
