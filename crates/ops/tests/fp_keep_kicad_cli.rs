//! What a board footprint keeps of its own through the operations that build it again from a footprint definition (UI-LEFT P30): its user texts, text boxes,
//! reference images, points and barcodes (`Footprint::art`, `FootprintExtra::items`), the footprint's own copy of its definition (`FootprintAssignment::definition`)
//! and what its board file said of it (`FootprintExtra::tokens`, the uuid, the sheet, the classes).
//!
//! * Edit Footprint (Ctrl+E) and Update Footprint on Board: KiCad's copy is a clone of the board footprint (`FOOTPRINT_EDIT_FRAME::LoadFootprintFromBoard`) that
//!   `SaveFootprintToBoard` exchanges for the board's, so nothing the copy holds is lost -- the copy shows the items, an item drawn in it reaches the board, and the
//!   record comes back with it.
//! * Update Footprint from Library and Change Footprint (`DIALOG_EXCHANGE_FOOTPRINTS::processFootprint` -> `PCB_EDIT_FRAME::ExchangeFootprint`): the new footprint is
//!   the library's, so its text boxes, images, points and barcodes are the library's (they are no `PCB_TEXT`, which alone the exchange carries over); a user text
//!   stays where it was unless "Remove text items if not in library footprint" is on; the uuid, the sheet and the classes stay; the Datasheet and Description fields
//!   stay unless "Update/reset text content" is on.
//! * Update PCB from Schematic's exchange (`BOARD_NETLIST_UPDATER::replaceFootprint`, every option on): the library's items, no extra text.
//! * A design read again (design.json saved and read, the model built from it) and an edit after it keep them.
//!
//! The slow tier (`EDA_SLOW_TESTS=1`) hands each written board to kicad-cli (`pcb upgrade --force`: `PCB_IO_KICAD_SEXPR_PARSER`, then `PCB_IO_KICAD_SEXPR`) and reads
//! its rewrite back: the footprint has the same items and texts.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::fp_items::FootprintItems;
use eda_model::ir::{BoardFootprintCopy, Design, FootprintExtra, Point, Side};
use eda_model::pcb_items::TextBox;
use eda_model::units::mm;
use eda_model::{ConstraintModel, Footprint};
use eda_ops::fp_editor::FpEditorCmd;
use eda_ops::fp_items::{FpItem, FpItemCmd};
use eda_ops::{Board, Cmd, ExchangeOptions};
use std::path::PathBuf;
use std::process::Command;

fn png() -> String {
    eda_model::image_info::base64_encode(&eda_model::image_info::solid_png(24, 12, [200, 30, 30], 300))
}

/// U1 (`Lib:Logo`) at (50, 40) holds a user text, a text box, an image, a barcode and a point, and its board file gives it a uuid, a sheet, a class, a description,
/// tags, the Datasheet and Description fields and a paste margin.
fn board_text() -> String {
    let data = png().as_bytes().chunks(76).map(|c| format!("\"{}\"", std::str::from_utf8(c).unwrap())).collect::<Vec<_>>().join(" ");
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
    (descr "the board's logo")
    (tags "logo")
    (property "Reference" "U1" (at 0 -6 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "Logo" (at 0 6 0) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (property "Datasheet" "https://example.com/logo.pdf" (at 0 0 0) (layer "F.Fab") (hide yes) (uuid "00000000-0000-0000-0000-00000000001b") (effects (font (size 1 1) (thickness 0.15))))
    (property "Description" "The logo" (at 0 0 0) (layer "F.Fab") (hide yes) (uuid "00000000-0000-0000-0000-00000000001c") (effects (font (size 1 1) (thickness 0.15))))
    (component_classes (class "LOGO"))
    (sheetname "/")
    (sheetfile "logo.kicad_sch")
    (solder_paste_margin -0.03)
    (attr smd)
    (fp_line (start -5 -4) (end 5 -4) (stroke (width 0.12) (type solid)) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000014"))
    (fp_text user "NOTE" (at 0 3 0) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-00000000001d") (effects (font (size 1 1) (thickness 0.15))))
    (fp_text_box "REV ${{REFERENCE}}" (start 1 1) (end 9 4) (margins 0.5 0.5 0.5 0.5) (layer "F.SilkS") (uuid "00000000-0000-0000-0000-000000000015")
      (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.12) (type solid)))
    (image (at 44 40) (layer "F.SilkS") (data {data}) (uuid "00000000-0000-0000-0000-000000000016"))
    (barcode (at 50 34 0) (layer "F.SilkS") (size 3 3) (text "U1") (text_height 1) (type qr) (ecc_level L) (hide yes) (knockout no) (uuid "00000000-0000-0000-0000-000000000017"))
    (point (at 53 42) (size 1) (layer "F.Fab") (uuid "00000000-0000-0000-0000-000000000018"))
    (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-000000000019"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-00000000001a"))
  )
)"#
    )
}

/// The library's `Lib:Logo`: the same pads, a text box of its own and nothing else.
const LOGO: &str = r#"(footprint "Logo" (version 20241229) (generator "pcbnew") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -6 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Logo" (at 0 6 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (attr smd)
  (fp_line (start -5 -4) (end 5 -4) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
  (fp_text_box "LIB BOX" (start -4 1) (end 4 3) (margins 0.5 0.5 0.5 0.5) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.1) (type solid)))
  (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
)"#;

/// The library's `Lib:Other`: pads wider apart and a barcode of its own.
const OTHER: &str = r#"(footprint "Other" (version 20241229) (generator "pcbnew") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -6 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Other" (at 0 6 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (attr smd)
  (barcode (at 0 -3 0) (layer "F.SilkS") (size 2 2) (text "OTHER") (text_height 1) (type qr) (ecc_level L) (hide yes) (knockout no))
  (pad "1" smd rect (at -2.5 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd rect (at 2.5 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
)"#;

/// The footprint `name` of the library file `text`, as the studio's server hands it to the command: the engine's footprint and its items.
fn library(name: &str, text: &str) -> (Footprint, FootprintItems) {
    let fp = eda_kicad::parse_footprint_file(text, name).expect("the library footprint");
    let items = eda_model::fp_items_place::placed_kinds(&eda_kicad::parse_library_footprint(text).expect("the library footprint").footprint.items);
    (fp, items)
}

fn exchange(footprint: Option<&str>, options: ExchangeOptions, libs: &[(&str, &str)]) -> Cmd {
    let libs: Vec<(String, (Footprint, FootprintItems))> = libs.iter().map(|(n, t)| (n.to_string(), library(n, t))).collect();
    Cmd::ExchangeFootprints {
        references: vec!["U1".into()],
        footprint: footprint.map(str::to_string),
        options,
        library: libs.iter().map(|(n, (f, _))| (n.clone(), f.clone())).collect(),
        library_items: libs.iter().map(|(n, (_, i))| (n.clone(), i.clone())).collect(),
    }
}

/// The design as the studio reads it again (design.json written and read) and the model it builds from it: the footprint each part is on and the board's own copies
/// of them (`crates/cli/src/board.rs`: `apply_footprint_assignments`, `fold_footprint_edits`).
fn reload(design: &Design, intent: &ConstraintModel) -> (Design, ConstraintModel) {
    let design: Design = serde_json::from_str(&serde_json::to_string(design).unwrap()).expect("design.json reads back");
    let mut model = intent.clone();
    eda_model::fp_assign::apply_footprint_assignments(&design, &mut model);
    for a in &design.drawings.as_ref().unwrap().footprint_assignments {
        if let Some(def) = &a.definition {
            model.instance_footprints.insert(a.reference.clone(), def.clone());
        }
    }
    (design, model)
}

fn apply(design: &Design, model: &ConstraintModel, cmds: &[Cmd]) -> (Design, ConstraintModel) {
    let mut b = Board::new(design.clone(), model, mm(0.1), mm(0.3));
    for c in cmds {
        b.apply(c).unwrap_or_else(|e| panic!("{c:?}: {e:?}"));
    }
    let design = b.into_design();
    reload(&design, model)
}

fn record(design: &Design) -> FootprintExtra {
    design.drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "U1").cloned().unwrap_or_default()
}

/// U1's items as the board has them (the ones the writer writes and the canvas draws).
fn items(design: &Design, model: &ConstraintModel) -> FootprintItems {
    eda_model::fp_items_place::items_of(design, model, model.part("U1").unwrap()).cloned().unwrap_or_default()
}

/// The texts of U1's footprint (its `fp_text user`s), with where each is on the board.
fn texts(design: &Design, model: &ConstraintModel) -> Vec<(String, Point)> {
    let pose = design.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U1").unwrap().clone();
    model.footprint_of(model.part("U1").unwrap()).unwrap().art.texts.iter().map(|t| (t.text.clone(), t.layout.board_position(&pose))).collect()
}

fn write(design: &Design, model: &ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-06", title: "fp keep" }).expect("export")
}

/// The `(footprint ..)` of U1 in a board file.
fn u1(text: &str) -> String {
    let at = text.find("(footprint ").expect("a footprint");
    text[at..].to_string()
}

fn boxes(items: &FootprintItems) -> Vec<&str> {
    items.text_boxes.iter().map(|t| t.text.as_str()).collect()
}

/// Ctrl+E, a text box drawn in the copy, Update Footprint on Board.
fn edited_on_board(design: &Design, model: &ConstraintModel) -> (Design, ConstraintModel) {
    let key = BoardFootprintCopy::key_of("U1");
    let drawn = TextBox::new("F.Fab", Point { x: mm(-4.0), y: mm(4.0) }, Point { x: mm(4.0), y: mm(6.0) }, mm(1.0), mm(0.1));
    let mut drawn = drawn;
    drawn.text = "DRAWN".into();
    apply(
        design,
        model,
        &[
            Cmd::FpEditor(FpEditorCmd::OpenFromBoard { reference: "U1".into() }),
            Cmd::FpItem(FpItemCmd::Add { footprint: key.clone(), item: FpItem::TextBox(drawn) }),
            Cmd::FpEditor(FpEditorCmd::SaveToBoard { footprint: key }),
        ],
    )
}

#[test]
fn edit_footprint_shows_the_footprints_items_and_update_on_board_keeps_them_and_its_record() {
    let (design, intent, _) = import_kicad_pcb(&board_text()).expect("import");
    let before = record(&design);
    assert_eq!((before.items.text_boxes.len(), before.items.images.len(), before.items.barcodes.len(), before.items.points.len()), (1, 1, 1, 1));
    // the copy holds them, ids and all
    let mut b = Board::new(design.clone(), &intent, mm(0.1), mm(0.3));
    b.apply(&Cmd::FpEditor(FpEditorCmd::OpenFromBoard { reference: "U1".into() })).unwrap();
    let copy = b.design().footprint_library.as_ref().unwrap().by_name(&BoardFootprintCopy::key_of("U1")).unwrap().clone();
    assert_eq!(copy.items, before.items, "Ctrl+E's copy shows the footprint's text box, image, barcode and point");
    assert!(copy.texts.iter().any(|t| t.content == "NOTE"));

    let (after, model) = edited_on_board(&design, &intent);
    let e = record(&after);
    assert_eq!(boxes(&e.items), ["REV ${REFERENCE}", "DRAWN"], "the board's text box and the one drawn in the copy");
    assert_eq!((&e.items.images, &e.items.barcodes, &e.items.points), (&before.items.images, &before.items.barcodes, &before.items.points), "the image's pixels too");
    assert_eq!(items(&after, &model), e.items, "the board draws them");
    assert_eq!(texts(&after, &model).iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["NOTE"], "the user text is the copy's");
    // what the clone carries that the copy has no place for: the file's uuid, sheet, classes, description, tags, fields and paste margin
    assert_eq!((e.uuid.clone(), e.sheetname.clone(), e.sheetfile.clone(), e.component_classes.clone()), (before.uuid.clone(), before.sheetname.clone(), before.sheetfile.clone(), before.component_classes.clone()));
    assert_eq!(e.tokens, before.tokens);
    assert_eq!(e.source_name, before.source_name);

    let text = u1(&write(&after, &model));
    for want in ["(fp_text_box \"REV ${REFERENCE}\"", "(fp_text_box \"DRAWN\"", "(image", "(barcode", "(point", "\"NOTE\"", "(descr \"the board's logo\")", "(property \"Datasheet\" \"https://example.com/logo.pdf\"", "00000000-0000-0000-0000-000000000011", "(solder_paste_margin -0.03)"] {
        assert!(text.contains(want), "{want} in\n{text}");
    }
    // read back, the same
    let (back, back_model, _) = import_kicad_pcb(&write(&after, &model)).expect("read back");
    assert_eq!(boxes(&items(&back, &back_model)), ["REV ${REFERENCE}", "DRAWN"]);

    // a second Ctrl+E shows the drawn box, and an edit after a reload keeps them all
    let (again, model2) = apply(&after, &model, &[Cmd::MoveItems { ids: vec!["U1".into()], dx: mm(5.0), dy: mm(0.0) }]);
    assert_eq!(record(&again).items, e.items, "a move carries them, as the footprint's");
    let (twice, model3) = edited_on_board(&again, &model2);
    assert_eq!(boxes(&record(&twice).items), ["REV ${REFERENCE}", "DRAWN", "DRAWN"]);
    assert_eq!(record(&twice).items.images, before.items.images);
    let _ = model3;
}

#[test]
fn a_footprint_made_here_keeps_what_update_on_board_put_on_it_on_either_side() {
    // U1 without the record of a board file: a footprint the board made, on the back
    let (mut design, intent, _) = import_kicad_pcb(&board_text()).expect("import");
    design.drawings.as_mut().unwrap().footprint_extras.clear();
    design.placement.as_mut().unwrap().footprints[0].side = Side::Bottom;
    let pads = |t: &str| u1(t).lines().filter(|l| l.trim_start().starts_with("(pad ")).map(|l| l.split("(uuid").next().unwrap().to_string()).collect::<Vec<_>>();
    let pads_before = pads(&write(&design, &intent));
    // a paste margin of its own (Footprint Properties), which a footprint made here keeps in its edit
    let (design, intent) = apply(&design, &intent, &[Cmd::SetFootprintPasteMargin { part: "U1".into(), margin: Some(mm(-0.04)), ratio: None }]);
    let (after, model) = edited_on_board(&design, &intent);
    assert_eq!(after.footprint_edit("U1").and_then(|e| e.solder_paste_margin), Some(mm(-0.04)), "the copy showed the margin and gave it back");
    let e = record(&after);
    assert_eq!(boxes(&e.items), ["DRAWN"]);
    assert!(!e.holds_file_form() && e.library_frame, "a record of what the board put there, in the library's frame as every footprint made here: {e:?}");
    let text = write(&after, &model);
    assert_eq!(pads(&text), pads_before, "the pads are written where they were (a footprint made here is half turned on the back)");
    assert!(u1(&text).contains("(fp_text_box \"DRAWN\"") && u1(&text).contains("(layer \"B.Fab\")") && u1(&text).contains("(solder_paste_margin -0.04)"), "{}", u1(&text));
    assert_eq!(boxes(&items(&after, &model)), ["DRAWN"], "the board draws it");
}

#[test]
fn update_from_library_makes_the_items_the_librarys_and_keeps_the_user_text_and_the_fields_as_its_options_say() {
    let (design, intent, _) = import_kicad_pcb(&board_text()).expect("import");
    let before = record(&design);
    let note_at = texts(&design, &intent)[0].1;
    // the dialog's Update defaults: texts stay, overrides and models are the library's
    let (d, m) = apply(&design, &intent, &[exchange(None, ExchangeOptions::update(), &[("Lib:Logo", LOGO)])]);
    let e = record(&d);
    assert_eq!(boxes(&items(&d, &m)), ["LIB BOX"], "the library footprint's text box, the board's own gone");
    assert!(e.items.images.is_empty() && e.items.barcodes.is_empty() && e.items.points.is_empty());
    assert_eq!(texts(&d, &m), [("NOTE".to_string(), note_at)], "the user text is kept where it was (no Remove text items)");
    assert_eq!((e.uuid.clone(), e.sheetname.clone(), e.component_classes.clone()), (before.uuid.clone(), before.sheetname.clone(), before.component_classes.clone()));
    assert!(e.tokens.iter().any(|t| t.starts_with("(property \"Datasheet\"")) && e.tokens.iter().any(|t| t.starts_with("(property \"Description\"")), "the fields' text is not reset: {:?}", e.tokens);
    assert!(!e.tokens.iter().any(|t| t.starts_with("(descr") || t.starts_with("(solder_paste_margin")), "the description is the library's, the paste margin reset: {:?}", e.tokens);
    let text = u1(&write(&d, &m));
    assert!(text.contains("(fp_text_box \"LIB BOX\"") && !text.contains("REV ") && !text.contains("(image") && !text.contains("(barcode") && text.contains("\"NOTE\""), "{text}");
    assert!(text.contains("00000000-0000-0000-0000-000000000011") && text.contains("https://example.com/logo.pdf"), "{text}");

    // "Remove text items if not in library footprint"
    let (d, m) = apply(&design, &intent, &[exchange(None, ExchangeOptions { remove_extra_texts: true, ..ExchangeOptions::update() }, &[("Lib:Logo", LOGO)])]);
    assert!(texts(&d, &m).is_empty(), "the user text the library has not is gone");
    // "Update/reset text content": the file's fields go (the writer gives the symbol's), the uuid stays
    let (d, _) = apply(&design, &intent, &[exchange(None, ExchangeOptions { reset_text_content: true, ..ExchangeOptions::update() }, &[("Lib:Logo", LOGO)])]);
    let e = record(&d);
    assert!(e.tokens.is_empty() && !e.holds_file_form() && e.uuid == before.uuid, "{e:?}");
    // "Update/reset clearance overrides" off: the paste margin stays with the file's form
    let (d, m) = apply(&design, &intent, &[exchange(None, ExchangeOptions { reset_clearance_overrides: false, ..ExchangeOptions::update() }, &[("Lib:Logo", LOGO)])]);
    assert!(u1(&write(&d, &m)).contains("(solder_paste_margin -0.03)"));
}

#[test]
fn change_footprint_and_update_pcb_from_schematic_take_the_new_footprints_items() {
    let (design, intent, _) = import_kicad_pcb(&board_text()).expect("import");
    let before = record(&design);
    // Change Footprint's defaults: every text reset, extra texts kept
    let (d, m) = apply(&design, &intent, &[exchange(Some("Lib:Other"), ExchangeOptions::change(), &[("Lib:Other", OTHER)])]);
    let e = record(&d);
    assert_eq!(items(&d, &m).barcodes.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), ["OTHER"], "the new footprint's barcode");
    assert!(e.items.text_boxes.is_empty() && e.items.images.is_empty() && e.items.points.is_empty());
    assert_eq!(texts(&d, &m).iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["NOTE"]);
    assert_eq!((e.uuid.clone(), e.sheetfile.clone()), (before.uuid.clone(), before.sheetfile.clone()));
    assert!(!e.holds_file_form(), "the fields' text is reset: the file's fields go");
    let text = u1(&write(&d, &m));
    assert!(text.starts_with("(footprint \"Lib:Other\"") && text.contains("(barcode") && text.contains("\"OTHER\"") && !text.contains("(fp_text_box") && !text.contains("(image"), "{text}");

    // Update PCB from Schematic's exchange (the symbol's Footprint field names Lib:Other): every option on
    let mut replace = exchange(Some("Lib:Other"), ExchangeOptions::replace(), &[("Lib:Other", OTHER)]);
    if let Cmd::ExchangeFootprints { options, .. } = &mut replace {
        assert_eq!(*options, ExchangeOptions::replace());
    }
    let (d, m) = apply(&design, &intent, &[replace]);
    assert_eq!(items(&d, &m).barcodes.len(), 1);
    assert!(texts(&d, &m).is_empty(), "replaceFootprint removes the extra texts");
    assert_eq!(record(&d).uuid, before.uuid);
}

#[test]
fn a_design_read_again_and_edited_keeps_its_footprints_items() {
    let (design, intent, _) = import_kicad_pcb(&board_text()).expect("import");
    let (after, model) = edited_on_board(&design, &intent);
    // read again twice, then an unrelated edit: the same items, the same file
    let (again, model2) = reload(&after, &intent);
    assert_eq!(record(&again), record(&after));
    assert_eq!(write(&again, &model2), write(&after, &model));
    let (edited, model3) = apply(&again, &model2, &[Cmd::RotateItems { ids: vec!["U1".into()], pivot: Point { x: mm(50.0), y: mm(40.0) }, angle_millideg: -90_000 }]);
    assert_eq!(boxes(&items(&edited, &model3)), ["REV ${REFERENCE}", "DRAWN"]);
}

// ---------------------------------------------------------------------------------------------------------------------------------------- kicad-cli

fn cli() -> Option<PathBuf> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    Some(std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| "kicad-cli".into()))
}

/// kicad-cli's rewrite of `design`'s board (`pcb upgrade --force`), read back.
fn through_kicad_cli(cli: &PathBuf, name: &str, design: &Design, model: &ConstraintModel) -> (String, Design, ConstraintModel) {
    let dir = std::env::temp_dir().join(format!("fp_keep_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(&path, write(design, model)).unwrap();
    let up = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().expect("kicad-cli runs");
    assert!(up.status.success(), "kicad-cli pcb upgrade failed: {}{}", String::from_utf8_lossy(&up.stdout), String::from_utf8_lossy(&up.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let (back, back_model, _) = import_kicad_pcb(&theirs).expect("read kicad-cli's rewrite");
    (theirs, back, back_model)
}

#[test]
fn kicad_cli_keeps_the_footprints_items_after_each_operation() {
    let Some(cli) = cli() else { return };
    let (design, intent, _) = import_kicad_pcb(&board_text()).expect("import");

    // Update Footprint on Board: the board's items and the drawn one, the user text, the fields and the uuid
    let (after, model) = edited_on_board(&design, &intent);
    let (theirs, back, bm) = through_kicad_cli(&cli, "on_board", &after, &model);
    let ours = items(&after, &model);
    let again = items(&back, &bm);
    assert_eq!(boxes(&again), boxes(&ours), "kicad-cli kept both text boxes:\n{}", u1(&theirs));
    assert_eq!((again.images.len(), again.barcodes.len(), again.points.len()), (1, 1, 1));
    assert_eq!(again.images[0].data, ours.images[0].data, "the pixels byte for byte");
    let near = |a: Point, b: Point| (a.x - b.x).abs() <= eda_model::ir::Nm::new(1) && (a.y - b.y).abs() <= eda_model::ir::Nm::new(1);
    let pose = |d: &Design| d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == "U1").unwrap().clone();
    let placed = |i: &FootprintItems, d: &Design| eda_model::fp_items_place::placed(i, &pose(d));
    assert!(placed(&again, &back).text_boxes.iter().zip(&placed(&ours, &after).text_boxes).all(|(a, b)| near(a.start, b.start) && near(a.end, b.end)), "where the studio has them");
    assert_eq!(texts(&back, &bm).iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["NOTE"]);
    let u = u1(&theirs);
    assert!(u.contains("(descr \"the board's logo\")") && u.contains("https://example.com/logo.pdf") && u.contains("00000000-0000-0000-0000-000000000011"), "{u}");

    // Update Footprint from Library: the library's text box, the user text where it was, the board's uuid
    let note_at = texts(&design, &intent)[0].1;
    let (d, m) = apply(&design, &intent, &[exchange(None, ExchangeOptions::update(), &[("Lib:Logo", LOGO)])]);
    let (theirs, back, bm) = through_kicad_cli(&cli, "update", &d, &m);
    assert_eq!(boxes(&items(&back, &bm)), ["LIB BOX"], "{}", u1(&theirs));
    let t = texts(&back, &bm);
    assert_eq!(t.len(), 1);
    assert!(t[0].0 == "NOTE" && near(t[0].1, note_at), "{t:?}");
    assert!(u1(&theirs).contains("00000000-0000-0000-0000-000000000011"));

    // Change Footprint: the new footprint's barcode, the user text kept
    let (d, m) = apply(&design, &intent, &[exchange(Some("Lib:Other"), ExchangeOptions::change(), &[("Lib:Other", OTHER)])]);
    let (theirs, back, bm) = through_kicad_cli(&cli, "change", &d, &m);
    let again = items(&back, &bm);
    assert_eq!((again.barcodes.len(), again.text_boxes.len(), again.images.len()), (1, 0, 0), "{}", u1(&theirs));
    assert_eq!(again.barcodes[0].text, "OTHER");
    assert_eq!(texts(&back, &bm).iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["NOTE"]);
}
