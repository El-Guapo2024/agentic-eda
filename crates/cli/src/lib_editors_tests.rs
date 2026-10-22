//! The library editors' leftovers (GAPS.md package P17, items 19 and 22) through a board directory: what an array of pads writes into the project and into the derived
//! `.kicad_mod`, and (the slow tier, `EDA_SLOW_TESTS=1`) what kicad-cli makes of the files this studio writes.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_model::ir::{Point, Shape};
use eda_ops::{ArrayGeometry, AxisSpec, Cmd, Domain, NumberingType, PadNumbering};
use std::path::Path;

/// kicad-cli on `args`, in `work`; what it said, or what it said on failure. Launched as the studio launches it (`cli_command`: an empty KiCad config, not the user's).
fn kicad_cli(work: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let cli = eda_kicad_engine::find_cli().ok_or("no kicad-cli")?;
    let out = eda_kicad_engine::cli_command(&cli, None).args(args).current_dir(work).output().map_err(|e| e.to_string())?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(said)
    } else {
        Err(said)
    }
}

fn slow() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    !off
}

fn library_pad(number: &str, x: i64, y: i64) -> eda_model::ir::LibraryPad {
    use eda_model::footprint::PadKind;
    use eda_model::ir::{ChamferCorners, LibraryPadShape};
    eda_model::ir::LibraryPad {
        id: String::new(),
        number: number.into(),
        at: Point { x, y },
        size: (900, 600),
        offset: Point::default(),
        shape: LibraryPadShape::RoundRect,
        kind: PadKind::Smd,
        drill: None,
        drill_slot: None,
        rot: 0,
        roundrect_ratio: Some(0.25),
        trapezoid_delta: None,
        chamfer_ratio: None,
        chamfer_corners: ChamferCorners::default(),
        custom: None,
        layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        clearance_override: None,
        thermal_gap_override: None,
        thermal_spoke_width_override: None,
        zone_connection: None,
        thermal_spoke_angle_mdeg: None,
        stack: Default::default(),
    }
}

/// A BGA-like footprint made the way a person makes it: one pad drawn, then Create Array on it with the coordinate scheme of the BGA alphabet (A1, A2 ... B1 ...), then a
/// silkscreen corner mark. The board directory is `setup`'s, so every verb is `step`ped through the project file and its undo history.
fn bga_in(dir: &Path) -> String {
    let name = "Test:BGA_3x4";
    step(dir, Cmd::NewFootprint { name: name.into() }, false, "test").unwrap();
    step(dir, Cmd::AddPad { footprint: name.into(), pad: library_pad("X", 0, 0) }, false, "test").unwrap();
    let (_, design, _) = load(dir).unwrap();
    let id = design.footprint_library.as_ref().unwrap().by_name(name).unwrap().pads[0].id.clone();
    let numbering = PadNumbering {
        start_specified: true,
        two_d: true,
        primary: AxisSpec { kind: NumberingType::AlphaNoIosqxz, start: "A".into(), step: 1 },
        secondary: AxisSpec { kind: NumberingType::Numeric, start: "1".into(), step: 1 },
    };
    let geometry = ArrayGeometry::Grid { nx: 3, ny: 4, dx: 800, dy: 800, offset_x: 0, offset_y: 0, centred: true, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false };
    step(dir, Cmd::CreateFootprintArray { footprint: name.into(), ids: vec![id], geometry, arrange: false, numbering: Some(numbering) }, false, "test").unwrap();
    step(
        dir,
        Cmd::AddFootprintGraphic { footprint: name.into(), shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 120, filled: false, start: Point { x: -1500, y: -1700 }, end: Point { x: -900, y: -2300 } } },
        false,
        "test",
    )
    .unwrap();
    name.into()
}

#[test]
fn an_array_of_pads_is_one_undo_step_of_the_footprint_editor_and_reaches_the_derived_kicad_mod() {
    let dir = scratch("p17_pad_array");
    setup(&dir);
    let name = bga_in(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let fp = design.footprint_library.as_ref().unwrap().by_name(&name).unwrap();
    let mut numbers: Vec<_> = fp.pads.iter().map(|p| p.number.clone()).collect();
    numbers.sort();
    assert_eq!(numbers, ["A1", "A2", "A3", "A4", "B1", "B2", "B3", "B4", "C1", "C2", "C3", "C4"]);
    let a1 = fp.pads.iter().find(|p| p.number == "A1").unwrap();
    let c4 = fp.pads.iter().find(|p| p.number == "C4").unwrap();
    assert_eq!((a1.at.x, a1.at.y, c4.at.x, c4.at.y), (-800, -1200, 800, 1200), "centred on the source pad: the 3 x 4 grid is 1.6 mm by 2.4 mm across");

    // what the file the studio exports says: a pad per point, with its number
    let text = eda_kicad::export_kicad_mod(fp);
    for n in &numbers {
        assert!(text.contains(&format!("(pad \"{n}\" smd roundrect")), "pad {n} is written:\n{text}");
    }
    assert_eq!(text.matches("(pad \"").count(), 12);

    // one undo step of the Footprint Editor takes the whole array away -- not the pad, not the footprint
    undo(&dir, "test", Some(Domain::FootprintEditor)).unwrap();
    let (_, after, _) = load(&dir).unwrap();
    // the graphic was added after the array: that is what the first undo took
    assert!(after.footprint_library.as_ref().unwrap().by_name(&name).unwrap().graphics.is_empty());
    undo(&dir, "test", Some(Domain::FootprintEditor)).unwrap();
    let (_, after, _) = load(&dir).unwrap();
    assert_eq!(after.footprint_library.as_ref().unwrap().by_name(&name).unwrap().pads.len(), 1, "the array was one step");
}

/// Slow tier (kicad-cli): a footprint with a numbered pad array, written by this studio, loads in kicad-cli, and `kicad-cli fp upgrade` -- which reads it and writes it out
/// again in the current format -- keeps every pad: the same numbers on the same places with the same copper, and the same plot of the front copper and the silkscreen.
#[test]
fn kicad_cli_loads_a_numbered_pad_array_and_keeps_it_through_fp_upgrade() {
    if !slow() {
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    let dir = scratch("p17_pad_array_kicad_cli");
    setup(&dir);
    let name = bga_in(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let fp = design.footprint_library.as_ref().unwrap().by_name(&name).unwrap();
    let ours_text = eda_kicad::export_kicad_mod(fp);

    let work = dir.join("kicad");
    let (ours_lib, theirs_lib) = (work.join("ours.pretty"), work.join("theirs.pretty"));
    for lib in [&ours_lib, &theirs_lib] {
        std::fs::create_dir_all(lib).unwrap();
        std::fs::write(lib.join("BGA_3x4.kicad_mod"), &ours_text).unwrap();
    }
    kicad_cli(&work, &["fp".as_ref(), "upgrade".as_ref(), theirs_lib.as_os_str()]).unwrap_or_else(|e| panic!("kicad-cli fp upgrade: {e}"));
    let theirs_text = std::fs::read_to_string(theirs_lib.join("BGA_3x4.kicad_mod")).unwrap();
    assert!(theirs_text.contains("(footprint \"BGA_3x4\""), "kicad-cli wrote the footprint back out:\n{theirs_text}");

    // 1. the same pads, read back by this studio's reader from KiCad's own text
    let pads = |text: &str| -> Vec<(String, i64, i64, i64, i64)> {
        let parsed = eda_kicad::parse_library_footprint(text).expect("the reader takes it");
        let mut v: Vec<_> = parsed.footprint.pads.iter().map(|p| (p.number.clone(), p.at.x, p.at.y, p.size.0, p.size.1)).collect();
        v.sort();
        v
    };
    assert_eq!(pads(&theirs_text), pads(&ours_text), "kicad-cli keeps every pad: number, place and size");
    assert_eq!(pads(&theirs_text).len(), 12);
    assert_eq!(pads(&theirs_text)[0].0, "A1");

    // 2. and draws the same: its plot of the front copper and of the silkscreen of the two libraries is one
    let mut plots = Vec::new();
    for (label, lib) in [("ours", &ours_lib), ("theirs", &theirs_lib)] {
        let out_dir = work.join(format!("{label}_svg"));
        std::fs::create_dir_all(&out_dir).unwrap();
        kicad_cli(&work, &["fp".as_ref(), "export".as_ref(), "svg".as_ref(), "--layers".as_ref(), "F.Cu,F.SilkS".as_ref(), "-o".as_ref(), out_dir.as_os_str(), lib.as_os_str()])
            .unwrap_or_else(|e| panic!("kicad-cli fp export svg {label}: {e}"));
        plots.push(std::fs::read_to_string(out_dir.join("BGA_3x4.svg")).expect("an svg"));
    }
    let shapes = |svg: &str| svg.matches("<path").count();
    assert!(shapes(&plots[0]) >= 12, "kicad-cli drew the 12 pads: {} paths", shapes(&plots[0]));
    assert_eq!(shapes(&plots[0]), shapes(&plots[1]), "the same number of shapes before and after fp upgrade");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A footprint made the way a person makes it: a pad, then the General tab of Footprint Properties with the fields grid in it -- the Reference moved and renamed, the Value hidden, a
/// shown user field `MPN` and a hidden one `Vendor` -- as one `edit_footprint_properties`.
fn footprint_with_fields_in(dir: &Path) -> String {
    use eda_model::fp_edit::FieldLayout;
    use eda_model::ir::FootprintField;
    let name = "Test:Fielded";
    step(dir, Cmd::NewFootprint { name: name.into() }, false, "test").unwrap();
    step(dir, Cmd::AddPad { footprint: name.into(), pad: library_pad("1", 0, 0) }, false, "test").unwrap();
    let mut reference = FieldLayout::new(Point { x: 500, y: -2500 }, "F.Fab");
    reference.size = (1200, 1000);
    reference.text = Some("J**".into());
    let mut mpn = FieldLayout::new(Point { x: 1500, y: 3000 }, "F.Fab");
    mpn.bold = true;
    mpn.halign = -1;
    step(
        dir,
        Cmd::EditFootprintProperties {
            name: name.into(),
            description: "with fields".into(),
            keywords: String::new(),
            attributes: Default::default(),
            reference_visible: true,
            value_visible: false,
            model: None,
            models3d: None,
            zone_connection: None,
            reference: Some(reference),
            value: Some(FieldLayout::new(Point { x: 0, y: 2200 }, "F.Fab")),
            fields: Some(vec![
                FootprintField { name: "MPN".into(), value: "ABC-1".into(), visible: true, layout: Some(mpn) },
                FootprintField { name: "Vendor".into(), value: "ACME".into(), visible: false, layout: None },
            ]),
        },
        false,
        "test",
    )
    .unwrap();
    name.into()
}

#[test]
fn the_fields_grid_is_one_undo_step_and_reaches_the_derived_kicad_mod() {
    let dir = scratch("p17_footprint_fields");
    setup(&dir);
    let name = footprint_with_fields_in(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let fp = design.footprint_library.as_ref().unwrap().by_name(&name).unwrap();
    assert_eq!((fp.fields.len(), fp.value_visible, fp.reference.as_ref().and_then(|l| l.text.clone())), (2, false, Some("J**".to_string())));

    // what the file this studio exports says: KiCad 8's properties with their places, and reading it back gives the same grid
    let text = eda_kicad::export_kicad_mod(fp);
    for line in ["(property \"Reference\" \"J**\" (at 0.5 -2.5 0)", "(property \"Value\" \"Test:Fielded\" (at 0 2.2 0)", "(property \"MPN\" \"ABC-1\" (at 1.5 3 0)", "(property \"Vendor\" \"ACME\" (at 0 0 0)"] {
        assert!(text.contains(line), "{line} is written:\n{text}");
    }
    let back = eda_kicad::parse_library_footprint(&text).unwrap().footprint;
    assert_eq!(back.fields.iter().map(|f| (f.name.as_str(), f.value.as_str(), f.visible)).collect::<Vec<_>>(), [("MPN", "ABC-1", true), ("Vendor", "ACME", false)]);
    assert!(!back.value_visible && back.reference_visible);
    assert_eq!(crate::library_api::footprint_kicad_mod_any(&dir, &name).unwrap(), text, "the route that serves the file writes the same text");

    // one undo step of the Footprint Editor takes the whole panel away: the grid and the description with it
    undo(&dir, "test", Some(Domain::FootprintEditor)).unwrap();
    let (_, after, _) = load(&dir).unwrap();
    let fp = after.footprint_library.as_ref().unwrap().by_name(&name).unwrap();
    assert!(fp.fields.is_empty() && fp.reference.is_none() && fp.description.is_empty() && fp.value_visible, "the panel was one step");
}

/// Slow tier (kicad-cli): a footprint with a Reference that has moved, a hidden Value and a shown and a hidden user field, written by this studio, loads in kicad-cli, and
/// `kicad-cli fp upgrade` keeps every field: the same names, texts and shown or hidden, on the same places and layers -- and its plot of the fabrication layer draws the shown user
/// field and the Reference and leaves the hidden ones out, as ours does.
#[test]
fn kicad_cli_loads_a_footprint_with_fields_and_keeps_them_through_fp_upgrade() {
    if !slow() {
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    let dir = scratch("p17_footprint_fields_kicad_cli");
    setup(&dir);
    let name = footprint_with_fields_in(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let ours_text = eda_kicad::export_kicad_mod(design.footprint_library.as_ref().unwrap().by_name(&name).unwrap());

    let work = dir.join("kicad");
    let (ours_lib, theirs_lib) = (work.join("ours.pretty"), work.join("theirs.pretty"));
    for lib in [&ours_lib, &theirs_lib] {
        std::fs::create_dir_all(lib).unwrap();
        std::fs::write(lib.join("Fielded.kicad_mod"), &ours_text).unwrap();
    }
    kicad_cli(&work, &["fp".as_ref(), "upgrade".as_ref(), theirs_lib.as_os_str()]).unwrap_or_else(|e| panic!("kicad-cli fp upgrade: {e}"));
    let theirs_text = std::fs::read_to_string(theirs_lib.join("Fielded.kicad_mod")).unwrap();
    assert!(theirs_text.contains("(footprint \"Fielded\""), "kicad-cli wrote the footprint back out:\n{theirs_text}");

    // 1. the fields, read back by this studio's reader from KiCad's own text, are the ones written
    let summary = |text: &str| {
        let fp = eda_kicad::parse_library_footprint(text).expect("the reader takes it").footprint;
        let layout = |l: &Option<eda_model::fp_edit::FieldLayout>| l.as_ref().map(|l| (l.at, l.layer.clone(), l.size, l.visible, l.bold, l.halign));
        (
            (fp.reference_visible, fp.value_visible),
            layout(&fp.reference),
            fp.reference.as_ref().and_then(|l| l.text.clone()),
            fp.fields.iter().map(|f| (f.name.clone(), f.value.clone(), f.visible, layout(&f.layout))).collect::<Vec<_>>(),
        )
    };
    assert_eq!(summary(&theirs_text), summary(&ours_text), "kicad-cli keeps every field: its text, its place, its layer, its size, shown or hidden");
    let theirs = summary(&theirs_text);
    assert_eq!((theirs.2.as_deref(), theirs.3.len()), (Some("J**"), 2));
    assert_eq!((theirs.3[0].0.as_str(), theirs.3[0].2, theirs.3[1].2), ("MPN", true, false));

    // 2. and draws the same: the fabrication layer of the two libraries is one plot, and it holds the shown fields' text (the plot of the footprint without its fields is smaller)
    let mut plots = Vec::new();
    for (label, lib) in [("ours", &ours_lib), ("theirs", &theirs_lib)] {
        let out_dir = work.join(format!("{label}_svg"));
        std::fs::create_dir_all(&out_dir).unwrap();
        kicad_cli(&work, &["fp".as_ref(), "export".as_ref(), "svg".as_ref(), "--layers".as_ref(), "F.Fab".as_ref(), "-o".as_ref(), out_dir.as_os_str(), lib.as_os_str()])
            .unwrap_or_else(|e| panic!("kicad-cli fp export svg {label}: {e}"));
        plots.push(std::fs::read_to_string(out_dir.join("Fielded.svg")).expect("an svg"));
    }
    let bare_lib = work.join("bare.pretty");
    std::fs::create_dir_all(&bare_lib).unwrap();
    let mut bare = design.footprint_library.as_ref().unwrap().by_name(&name).unwrap().clone();
    bare.fields.clear();
    bare.reference_visible = false;
    std::fs::write(bare_lib.join("Fielded.kicad_mod"), eda_kicad::export_kicad_mod(&bare)).unwrap();
    let bare_out = work.join("bare_svg");
    std::fs::create_dir_all(&bare_out).unwrap();
    kicad_cli(&work, &["fp".as_ref(), "export".as_ref(), "svg".as_ref(), "--layers".as_ref(), "F.Fab".as_ref(), "-o".as_ref(), bare_out.as_os_str(), bare_lib.as_os_str()]).unwrap_or_else(|e| panic!("kicad-cli fp export svg bare: {e}"));
    let bare_plot = std::fs::read_to_string(bare_out.join("Fielded.svg")).expect("an svg");
    let shapes = |svg: &str| svg.matches("<path").count();
    assert_eq!(shapes(&plots[0]), shapes(&plots[1]), "the same number of shapes before and after fp upgrade");
    assert!(shapes(&plots[0]) > shapes(&bare_plot), "the shown fields are drawn: {} paths against {} without them", shapes(&plots[0]), shapes(&bare_plot));
    let _ = std::fs::remove_dir_all(&dir);
}

// ===================================================================================================================================================== derived symbols

use eda_model::ir::{LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin};
use eda_model::sym_fields::{FieldHAlign, LibrarySymbolField};
use eda_model::symbol::SPoint;

/// `Lib:Base`: a rectangle and two pins, the five fields of a real library symbol and a visible user field `MPN`.
fn base_symbol() -> LibrarySymbol {
    let mut s = LibrarySymbol::new_empty("Lib:Base");
    s.reference_prefix = "U".into();
    s.description = "the base".into();
    s.keywords = "base chip".into();
    s.footprint_filters = vec!["SOIC*".into()];
    s.graphics.push(LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 1, body_style: 1, start: SPoint { x: -2.54, y: -2.54 }, end: SPoint { x: 2.54, y: 2.54 }, stroke_mm: 0.254, fill: eda_model::ir::LibraryFill::None });
    for (n, y) in [("1", 1.27), ("2", -1.27)] {
        s.pins.push(LibrarySymbolPin {
            id: String::new(),
            number: n.into(),
            name: format!("P{n}"),
            electrical_type: "passive".into(),
            shape: "line".into(),
            at: SPoint { x: -5.08, y },
            angle_deg: 0.0,
            length_mm: 2.54,
            unit: 1,
            body_style: 1,
            hidden: false,
            name_size_mm: None,
            number_size_mm: None,
            alternates: Vec::new(),
        });
    }
    let mut f = s.all_fields();
    f[1].text = "BASE".into();
    f[3].text = "http://base/ds.pdf".into();
    f.push(LibrarySymbolField { name: "MPN".into(), text: "X-1".into(), at: SPoint { x: 0.0, y: -5.0 }, size_mm: 1.0, h_align: FieldHAlign::Left, ..Default::default() });
    s.set_fields(f);
    s.assign_missing_ids();
    s
}

/// Three symbols made through the verbs: `Base`, `Mid` derived from it (with its user fields and their text) and given a Value of its own, `Leaf` derived from `Mid` with a Value,
/// a Datasheet and an `MPN` of its own.
fn symbol_chain_in(dir: &Path) {
    step(dir, Cmd::PutLibrarySymbol { symbol: base_symbol(), overwrite: false }, false, "test").unwrap();
    step(dir, Cmd::DeriveSymbol { parent: "Lib:Base".into(), lib_id: "Lib:Mid".into(), keep_footprint: false, keep_datasheet: false, transfer_user_fields: true, keep_content_user_fields: true, overwrite: false }, false, "test").unwrap();
    let (_, design, _) = load(dir).unwrap();
    let mut f = design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Mid").unwrap().all_fields();
    f[1].text = "MID".into();
    f[1].at = SPoint { x: 2.0, y: -6.5 };
    f[1].h_align = FieldHAlign::Right;
    f.iter_mut().find(|x| x.name == "MPN").unwrap().text = "MID-1".into();
    step(dir, Cmd::EditLibrarySymbolFields { lib_id: "Lib:Mid".into(), fields: f }, false, "test").unwrap();
    step(dir, Cmd::DeriveSymbol { parent: "Lib:Mid".into(), lib_id: "Lib:Leaf".into(), keep_footprint: false, keep_datasheet: false, transfer_user_fields: false, keep_content_user_fields: false, overwrite: false }, false, "test").unwrap();
    let (_, design, _) = load(dir).unwrap();
    let mut f = design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Leaf").unwrap().all_fields();
    f[1].text = "LEAF".into();
    f[1].size_mm = 1.0;
    f[3].text = "http://leaf/ds.pdf".into();
    f.push(LibrarySymbolField { name: "MPN".into(), text: "LEAF-7".into(), at: SPoint { x: -3.0, y: 5.0 }, size_mm: 0.8, bold: true, ..Default::default() });
    step(dir, Cmd::EditLibrarySymbolFields { lib_id: "Lib:Leaf".into(), fields: f }, false, "test").unwrap();
}

#[test]
fn a_derived_symbol_is_placed_exported_and_undone_as_the_flat_symbol_it_stands_for() {
    let dir = scratch("p17_derived_symbols");
    setup(&dir);
    symbol_chain_in(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let lib = design.symbol_library.as_ref().unwrap();
    assert_eq!(lib.parent_chain("Lib:Leaf").parents, ["Lib:Mid", "Lib:Base"]);

    // placed: the schematic resolves a published derived symbol as the flat one (the root's drawing, the datasheet of the nearest step that has one)
    step(&dir, Cmd::UpdateSymbolOnBoard { lib_id: "Lib:Leaf".into() }, false, "test").unwrap();
    let (_, _, model) = load(&dir).unwrap();
    let placed = model.symbol_of("Lib:Leaf").expect("the published derived symbol resolves");
    assert_eq!((placed.pins.len(), placed.graphics.len()), (2, 1));
    assert_eq!(placed.datasheet, "http://leaf/ds.pdf");
    assert_eq!(placed.description, "the base", "its Description is the parent's");

    // exported on its own (Export, Copy): flat, with nothing left to look up; the whole library keeps the derivation
    let alone = crate::library_api::symbol_kicad_sym_any(&dir, "Lib:Leaf").unwrap();
    assert!(!alone.contains("extends") && alone.contains("(pin passive") && alone.contains("\"LEAF\""), "{alone}");
    let all: Vec<&LibrarySymbol> = lib.symbols.iter().collect();
    let whole = eda_kicad::export_kicad_sym_library(&all);
    assert_eq!(whole.matches("(extends ").count(), 2, "Mid and Leaf");
    assert_eq!(whole.matches("(pin passive").count(), 2, "the drawing is written once, under the base");
    // and what is written reads back as the same library
    let back = eda_kicad::parse_library_symbols(&whole).unwrap();
    let section = eda_model::ir::SymbolLibrarySection { symbols: back.symbols.into_iter().map(|mut s| { s.lib_id = format!("Lib:{}", s.lib_id); s.extends = s.extends.map(|p| format!("Lib:{p}")); s }).collect() };
    assert_eq!(section.flatten("Lib:Leaf").unwrap().all_fields(), lib.flatten("Lib:Leaf").unwrap().all_fields());
    assert_eq!(section.flatten("Lib:Leaf").unwrap().pins.len(), 2);

    // one undo of the Symbol Editor takes the last verb away -- not the derivation before it
    undo(&dir, "test", Some(Domain::SymbolEditor)).unwrap(); // update_symbol_on_board
    undo(&dir, "test", Some(Domain::SymbolEditor)).unwrap(); // the Leaf's fields
    let (_, design, _) = load(&dir).unwrap();
    assert_eq!(design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Leaf").unwrap().field("Value").unwrap().text, "MID", "the Value Leaf was given is gone with the step");
    undo(&dir, "test", Some(Domain::SymbolEditor)).unwrap(); // derive Leaf
    let (_, design, _) = load(&dir).unwrap();
    assert!(design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Leaf").is_none(), "one undo takes the derive away");
    assert!(design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Mid").is_some());
}

/// `GET /api/symbol_editor/names` says which symbols of the project library are derived and from what -- the library tree offers Flatten Symbol and the related fields table from it --
/// and `GET /api/symbol?flatten=1` is the symbol the editor draws.
#[test]
fn the_symbol_editor_names_route_says_which_symbols_are_derived_and_the_flat_route_draws_them() {
    let dir = scratch("p17_names_derived");
    setup(&dir);
    symbol_chain_in(&dir);
    let names = crate::studio::symbol_editor_names_json(&dir).unwrap();
    assert_eq!(names["derived"]["Lib:Leaf"], "Lib:Mid");
    assert_eq!(names["derived"]["Lib:Mid"], "Lib:Base");
    assert!(names["derived"].get("Lib:Base").is_none(), "a root symbol is not listed");
    let project: Vec<&str> = names["project"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(project.contains(&"Lib:Base") && project.contains(&"Lib:Leaf"), "{project:?}");

    let flat = crate::library_api::symbol_flattened(&dir, "Lib:Leaf").unwrap();
    assert!(flat.extends.is_none(), "the flat symbol stands alone");
    assert_eq!((flat.pins.len(), flat.graphics.len()), (2, 1), "and draws the root's pins and body");
    let own = crate::library_api::symbol_any(&dir, "Lib:Leaf").unwrap().0;
    assert_eq!(own.extends.as_deref(), Some("Lib:Mid"));
    assert!(own.pins.is_empty() && own.graphics.is_empty(), "the symbol itself keeps no drawing");
}

/// kicad-cli's SVG plot of `symbol` in `library`, without its title line (which holds the date).
fn plotted_symbol(work: &Path, library: &Path, symbol: &str, out_name: &str, extra: &[&str]) -> String {
    let out_dir = work.join(out_name);
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut args: Vec<&std::ffi::OsStr> = vec!["sym".as_ref(), "export".as_ref(), "svg".as_ref(), "--symbol".as_ref(), symbol.as_ref(), "-o".as_ref(), out_dir.as_os_str()];
    args.extend(extra.iter().map(|a| std::ffi::OsStr::new(*a)));
    args.push(library.as_os_str());
    kicad_cli(work, &args).unwrap_or_else(|e| panic!("kicad-cli sym export svg {out_name}: {e}"));
    let file = std::fs::read_dir(&out_dir).unwrap().filter_map(Result::ok).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "svg")).expect("an svg");
    std::fs::read_to_string(file).unwrap().lines().filter(|l| !l.starts_with("<title>")).collect::<Vec<_>>().join("\n")
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): a library with derived symbols (`Base`, `Mid` derived from it, `Leaf` derived from that) written by this studio loads in kicad-cli, and
/// (1) `kicad-cli sym upgrade` -- which reads the library and writes it out again in the current format -- keeps every symbol the same: the same parent, the same fields with their text,
/// place and look, the keywords and the filters; (2) the plot kicad-cli makes of `Leaf` (it flattens a derived symbol itself) is the plot it makes of the flat symbol this studio's `Flatten`
/// gives, written as a symbol of its own: the root's drawing and every field the chain overrides, the user fields included, hidden ones too.
#[test]
fn kicad_cli_loads_derived_symbols_keeps_them_through_sym_upgrade_and_plots_a_flat_symbol_as_it_plots_the_derived_one() {
    if !slow() {
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    let dir = scratch("p17_derived_symbols_kicad_cli");
    setup(&dir);
    symbol_chain_in(&dir);
    let (_, design, _) = load(&dir).unwrap();
    let lib = design.symbol_library.as_ref().unwrap();
    let bare = |s: &LibrarySymbol| {
        let mut b = s.clone();
        b.lib_id = eda_model::sym_fields::bare_name(&b.lib_id).to_string();
        b.extends = b.extends.map(|p| eda_model::sym_fields::bare_name(&p).to_string());
        b
    };
    let ours: Vec<LibrarySymbol> = ["Lib:Base", "Lib:Mid", "Lib:Leaf"].iter().map(|id| bare(lib.by_lib_id(id).unwrap())).collect();
    let refs: Vec<&LibrarySymbol> = ours.iter().collect();
    let text = eda_kicad::export_kicad_sym_library(&refs);

    let work = dir.join("kicad");
    std::fs::create_dir_all(&work).unwrap();
    let (derived_path, upgraded_path, flat_path) = (work.join("derived.kicad_sym"), work.join("upgraded.kicad_sym"), work.join("flat.kicad_sym"));
    std::fs::write(&derived_path, &text).unwrap();
    std::fs::write(&upgraded_path, &text).unwrap();
    kicad_cli(&work, &["sym".as_ref(), "upgrade".as_ref(), upgraded_path.as_os_str()]).unwrap_or_else(|e| panic!("kicad-cli sym upgrade: {e}"));
    let theirs_text = std::fs::read_to_string(&upgraded_path).unwrap();
    assert!(theirs_text.contains("(extends \"Base\")") && theirs_text.contains("(extends \"Mid\")"), "kicad-cli kept the derivation:\n{theirs_text}");

    // 1. the same symbols, read back by this studio's reader from KiCad's own text
    let theirs = eda_kicad::parse_library_symbols(&theirs_text).expect("the reader takes KiCad's text");
    assert_eq!(theirs.symbols.iter().map(|s| s.lib_id.as_str()).collect::<Vec<_>>(), ["Base", "Mid", "Leaf"]);
    for (mine, kicad) in ours.iter().zip(&theirs.symbols) {
        assert_eq!(kicad.extends, mine.extends, "{}: the parent", mine.lib_id);
        assert_eq!(kicad.all_fields(), mine.all_fields(), "{}: the fields, with their text, place and look", mine.lib_id);
        assert_eq!((kicad.pins.len(), kicad.graphics.len()), (mine.pins.len(), mine.graphics.len()), "{}: the drawing", mine.lib_id);
    }
    // KiCad writes a derived symbol's footprint filters out in full when it saves (the ones it inherits too: `saveDcmInfoAsFields` asks `GetFPFilters()`), so the keywords and the
    // filters are compared as the symbols have them once flattened -- what each one stands for
    let (mine, kicad) = (eda_model::ir::SymbolLibrarySection { symbols: ours.clone() }, eda_model::ir::SymbolLibrarySection { symbols: theirs.symbols.clone() });
    for name in ["Base", "Mid", "Leaf"] {
        let (a, b) = (mine.flatten(name).unwrap(), kicad.flatten(name).unwrap());
        assert_eq!((&b.keywords, &b.footprint_filters), (&a.keywords, &a.footprint_filters), "{name}: keywords and filters once flattened");
        assert_eq!(b.all_fields(), a.all_fields(), "{name}: the flat fields");
        assert_eq!((b.in_bom, b.on_board, b.pins.len(), b.graphics.len()), (a.in_bom, a.on_board, a.pins.len(), a.graphics.len()), "{name}: the flat flags and drawing");
    }

    // 2. kicad-cli plots Leaf the way it plots the flat Leaf of this studio's Flatten
    let flat = bare(&lib.flatten("Lib:Leaf").unwrap());
    std::fs::write(&flat_path, eda_kicad::export_kicad_sym_library(&[&flat])).unwrap();
    for (label, extra) in [("plain", &[][..]), ("hidden_fields", &["--include-hidden-fields", "--include-hidden-pins"][..])] {
        let theirs_svg = plotted_symbol(&work, &derived_path, "Leaf", &format!("derived_{label}"), extra);
        let ours_svg = plotted_symbol(&work, &flat_path, "Leaf", &format!("flat_{label}"), extra);
        assert!(theirs_svg.contains("<rect") && theirs_svg.matches("stroked-text").count() >= 4, "kicad-cli drew the body and the texts of the derived symbol ({label})");
        assert_eq!(ours_svg, theirs_svg, "kicad-cli plots the flat symbol this studio makes as it plots the derived one ({label})");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ============================================================================================================================================= pin alternate functions

fn mcu_with_alternates() -> LibrarySymbol {
    use eda_model::ir::PinAlternate;
    let alt = |name: &str, t: &str, shape: &str| PinAlternate { name: name.into(), electrical_type: t.into(), shape: shape.into() };
    let mut s = base_symbol();
    s.lib_id = "Lib:Mcu".into();
    s.pins[0].name = "PA0".into();
    s.pins[0].electrical_type = "bidirectional".into();
    s.pins[0].alternates = vec![alt("UART_TX", "output", "line"), alt("ADC1", "input", "line"), alt("GPIO0", "bidirectional", "inverted"), alt("TIM2_CH1", "output", "clock")];
    s
}

/// The alternate functions of a pin are one undo step of the symbol editor (the pin's own verb carries them) and reach the derived library file by name, in the map's order.
#[test]
fn pin_alternates_are_one_undo_step_and_reach_the_derived_kicad_sym() {
    let dir = scratch("p17_pin_alternates");
    setup(&dir);
    step(&dir, Cmd::PutLibrarySymbol { symbol: mcu_with_alternates(), overwrite: false }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let pin = design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Mcu").unwrap().pins[0].clone();
    // Pin Properties' grid: a row removed, a row changed, one added -- one verb
    let mut edited = pin.clone();
    edited.alternates.retain(|a| a.name != "TIM2_CH1");
    edited.alternates.iter_mut().find(|a| a.name == "ADC1").unwrap().shape = "clock_low".into();
    edited.alternates.push(eda_model::ir::PinAlternate { name: "SPI_MISO".into(), electrical_type: "input".into(), shape: "line".into() });
    step(&dir, Cmd::EditSymbolPin { lib_id: "Lib:Mcu".into(), id: pin.id.clone(), pin: edited }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let names = |d: &eda_model::ir::Design| d.symbol_library.as_ref().unwrap().by_lib_id("Lib:Mcu").unwrap().pins[0].alternates.iter().map(|a| format!("{} {} {}", a.name, a.electrical_type, a.shape)).collect::<Vec<_>>();
    assert_eq!(names(&design), ["ADC1 input clock_low", "GPIO0 bidirectional inverted", "SPI_MISO input line", "UART_TX output line"]);
    let text = crate::library_api::symbol_kicad_sym_any(&dir, "Lib:Mcu").unwrap();
    assert!(text.contains("(alternate \"ADC1\" input clock_low)") && text.contains("(alternate \"SPI_MISO\" input line)") && !text.contains("TIM2_CH1"), "{text}");
    // one undo of the Symbol Editor brings the whole first list back
    undo(&dir, "test", Some(Domain::SymbolEditor)).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    assert_eq!(names(&design), ["UART_TX output line", "ADC1 input line", "GPIO0 bidirectional inverted", "TIM2_CH1 output clock"], "as imported");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): a symbol with alternate pin functions written by this studio loads in kicad-cli, `kicad-cli sym upgrade` keeps every alternate (name, type,
/// style) of every pin, and a plot of the symbol is the plot of the same symbol without them (the alternates are the pin's menu in the editor, not part of the drawing).
#[test]
fn kicad_cli_loads_pin_alternates_and_keeps_them_through_sym_upgrade() {
    if !slow() {
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    let dir = scratch("p17_pin_alternates_kicad_cli");
    setup(&dir);
    let mut mine = mcu_with_alternates();
    mine.lib_id = "Mcu".into();
    mine.assign_missing_ids();
    let mut plain = mine.clone();
    plain.lib_id = "Plain".into();
    plain.pins.iter_mut().for_each(|p| p.alternates.clear());
    let text = eda_kicad::export_kicad_sym_library(&[&mine, &plain]);
    let work = dir.join("kicad");
    std::fs::create_dir_all(&work).unwrap();
    let (path, upgraded) = (work.join("mcu.kicad_sym"), work.join("upgraded.kicad_sym"));
    std::fs::write(&path, &text).unwrap();
    std::fs::write(&upgraded, &text).unwrap();
    kicad_cli(&work, &["sym".as_ref(), "upgrade".as_ref(), upgraded.as_os_str()]).unwrap_or_else(|e| panic!("kicad-cli sym upgrade: {e}"));
    let theirs_text = std::fs::read_to_string(&upgraded).unwrap();
    assert!(theirs_text.contains("(alternate \"UART_TX\" output line)"), "kicad-cli kept the alternates:\n{theirs_text}");
    let theirs = eda_kicad::parse_library_symbols(&theirs_text).expect("the reader takes KiCad's text");
    let kicad_mcu = theirs.symbols.iter().find(|s| s.lib_id == "Mcu").unwrap();
    for (a, b) in mine.pins.iter().zip(&kicad_mcu.pins) {
        // KiCad keeps a pin's alternates in a map by name, so they come back in the name's order
        let mut sorted = a.alternates.clone();
        sorted.sort_by(|x, y| x.name.cmp(&y.name));
        assert_eq!((&a.number, &sorted), (&b.number, &b.alternates), "pin {}: the alternate functions, by name", a.number);
    }
    assert!(!kicad_mcu.pins[0].alternates.is_empty());
    // loads and plots (kicad-cli reads the file with the alternates and draws the symbol as it draws the one without them)
    let with = plotted_symbol(&work, &path, "Mcu", "with", &[]);
    let without = plotted_symbol(&work, &path, "Plain", "without", &[]);
    assert_eq!(with.matches("<path").count(), without.matches("<path").count(), "the alternates add nothing to the plot");
    let _ = std::fs::remove_dir_all(&dir);
}

// ======================================================================================================================================== shape properties and text boxes

fn text_box_at(text: &str, start: (f64, f64), end: (f64, f64)) -> LibrarySymbolGraphic {
    use eda_model::sym_fields::{FieldHAlign, FieldVAlign};
    LibrarySymbolGraphic::TextBox {
        id: String::new(),
        unit: 1,
        body_style: 1,
        text: text.into(),
        start: SPoint { x: start.0, y: start.1 },
        end: SPoint { x: end.0, y: end.1 },
        angle_deg: 0.0,
        size_mm: 1.27,
        bold: false,
        italic: false,
        h_align: FieldHAlign::Left,
        v_align: FieldVAlign::Top,
        margin_mm: 0.0,
        stroke_mm: 0.254,
        fill: eda_model::ir::LibraryFill::None,
    }
}

/// A shape's properties dialog and a text box are each one undo step of the Symbol Editor, and the derived `.kicad_sym` has the text box as KiCad writes one: the start and the size, the
/// line break as `\n` (a raw one is a library kicad-cli cannot load), the margins and the stroke.
#[test]
fn a_shapes_properties_and_a_text_box_are_one_undo_step_each_and_reach_the_derived_kicad_sym() {
    let dir = scratch("p17_text_boxes");
    setup(&dir);
    step(&dir, Cmd::PutLibrarySymbol { symbol: base_symbol(), overwrite: false }, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let rect_id = design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Base").unwrap().graphics[0].id().to_string();
    // Shape Properties: a width of 0.5, the body background colour, common to all units
    step(&dir, Cmd::EditSymbolGraphic { lib_id: "Lib:Base".into(), id: rect_id.clone(), stroke_mm: 0.5, fill: eda_model::ir::LibraryFill::Background, unit: Some(0), body_style: None }, false, "test").unwrap();
    step(&dir, Cmd::AddSymbolGraphic { lib_id: "Lib:Base".into(), graphic: text_box_at("10V\nREF", (-17.78, 6.35), (-10.16, -1.27)) }, false, "test").unwrap();
    let text = crate::library_api::symbol_kicad_sym_any(&dir, "Lib:Base").unwrap();
    assert!(
        text.contains("(text_box \"10V\\nREF\" (at -17.78 6.35 0) (size 7.62 -7.62) (margins 1.016 1.016 1.016 1.016) (stroke (width 0.254) (type default)) (fill (type none)) (effects (font (size 1.27 1.27)) (justify left top)))"),
        "{text}"
    );
    assert!(text.contains("(stroke (width 0.5) (type default)) (fill (type background))"), "the shape's edit is written too: {text}");
    // the box is on the unit the editor was on; one undo takes it away, the next the shape's edit
    undo(&dir, "test", Some(Domain::SymbolEditor)).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let sym = design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Base").unwrap();
    assert!(!sym.graphics.iter().any(|g| matches!(g, LibrarySymbolGraphic::TextBox { .. })), "one undo takes the text box");
    assert_eq!(sym.graphics[0].unit(), 0, "and leaves the shape's edit");
    undo(&dir, "test", Some(Domain::SymbolEditor)).unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let g = &design.symbol_library.as_ref().unwrap().by_lib_id("Lib:Base").unwrap().graphics[0];
    assert!(matches!(g, LibrarySymbolGraphic::Rectangle { stroke_mm, fill: eda_model::ir::LibraryFill::None, .. } if *stroke_mm == 0.254) && g.unit() == 1, "one more undo takes the whole dialog back: {g:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): a library with text boxes written by this studio loads in kicad-cli, and
/// (1) a text box of no text plots exactly as the rectangle of its two corners (the start, the size and the Y-up mm are the file's own);
/// (2) `kicad-cli sym upgrade` keeps every text box -- text, corners, angle, size, bold, italic, alignment, margin, border and fill;
/// (3) kicad-cli draws the text of each: the plot has its stroked lines, and the upgraded file plots as the one this studio wrote.
#[test]
fn kicad_cli_loads_text_boxes_keeps_them_through_sym_upgrade_and_draws_their_text() {
    use eda_model::ir::LibraryFill;
    use eda_model::sym_fields::{FieldHAlign, FieldVAlign};
    if !slow() {
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: no kicad-cli");
        return;
    }
    let dir = scratch("p17_text_boxes_kicad_cli");
    setup(&dir);
    let work = dir.join("kicad");
    std::fs::create_dir_all(&work).unwrap();
    let write = |name: &str, graphics: Vec<LibrarySymbolGraphic>| -> std::path::PathBuf {
        let mut s = LibrarySymbol::new_empty("Boxed");
        s.graphics = graphics;
        s.assign_missing_ids();
        let path = work.join(name);
        std::fs::write(&path, eda_kicad::export_kicad_sym_library(&[&s])).unwrap();
        path
    };

    // (1) the geometry: a box with no text is a rectangle, to the 0.1 um
    let empty = write("empty_box.kicad_sym", vec![text_box_at("", (-5.0, 5.0), (5.0, -5.0))]);
    let rect = write("rect.kicad_sym", vec![LibrarySymbolGraphic::Rectangle { id: String::new(), unit: 1, body_style: 1, start: SPoint { x: -5.0, y: 5.0 }, end: SPoint { x: 5.0, y: -5.0 }, stroke_mm: 0.254, fill: LibraryFill::None }]);
    assert_eq!(plotted_symbol(&work, &empty, "Boxed", "empty_box", &[]), plotted_symbol(&work, &rect, "Boxed", "rect", &[]), "a text box of no text is the rectangle of its corners");

    // (2) and (3): three boxes of different looks
    let mut multi = text_box_at("10V\nREF", (-17.78, 6.35), (-10.16, -1.27));
    let mut tall = text_box_at("5k", (-13.335, -8.89), (-12.065, -12.7));
    let mut plain = text_box_at("a longer text that has to wrap inside its box", (2.54, 7.62), (12.7, 0.0));
    if let LibrarySymbolGraphic::TextBox { bold, .. } = &mut multi {
        *bold = true;
    }
    if let LibrarySymbolGraphic::TextBox { angle_deg, size_mm, italic, h_align, v_align, stroke_mm, fill, .. } = &mut tall {
        (*angle_deg, *size_mm, *italic, *h_align, *v_align, *stroke_mm, *fill) = (90.0, 0.635, true, FieldHAlign::Center, FieldVAlign::Center, -0.0001, LibraryFill::Background);
    }
    if let LibrarySymbolGraphic::TextBox { margin_mm, h_align, .. } = &mut plain {
        (*margin_mm, *h_align) = (0.5, FieldHAlign::Right);
    }
    let ours = vec![multi, tall, plain];
    let path = write("boxes.kicad_sym", ours.clone());
    let upgraded = work.join("boxes_upgraded.kicad_sym");
    std::fs::copy(&path, &upgraded).unwrap();
    kicad_cli(&work, &["sym".as_ref(), "upgrade".as_ref(), upgraded.as_os_str()]).unwrap_or_else(|e| panic!("kicad-cli sym upgrade: {e}"));
    let theirs_text = std::fs::read_to_string(&upgraded).unwrap();
    assert_eq!(theirs_text.matches("(text_box ").count(), 3, "kicad-cli kept the three boxes:\n{theirs_text}");
    let theirs = eda_kicad::parse_library_symbols(&theirs_text).expect("the reader takes KiCad's text");
    assert!(theirs.warnings.is_empty(), "{:?}", theirs.warnings);
    let theirs_boxes: Vec<&LibrarySymbolGraphic> = theirs.symbols[0].graphics.iter().collect();
    assert_eq!(theirs_boxes.len(), 3);
    for (mine, kicad) in ours.iter().zip(&theirs_boxes) {
        let (LibrarySymbolGraphic::TextBox { text: t0, start: s0, end: e0, angle_deg: a0, size_mm: z0, bold: b0, italic: i0, h_align: h0, v_align: v0, margin_mm: m0, stroke_mm: k0, fill: f0, .. }, LibrarySymbolGraphic::TextBox { text: t1, start: s1, end: e1, angle_deg: a1, size_mm: z1, bold: b1, italic: i1, h_align: h1, v_align: v1, margin_mm: m1, stroke_mm: k1, fill: f1, .. }) = (mine, *kicad) else {
            panic!("{mine:?} {kicad:?}")
        };
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert_eq!(t0, t1, "the text, line breaks included");
        assert!(near(s0.x, s1.x) && near(s0.y, s1.y) && near(e0.x, e1.x) && near(e0.y, e1.y), "{t0:?}: the corners {s0:?} {e0:?} against {s1:?} {e1:?}");
        assert_eq!((a0, b0, i0, h0, v0, f0), (a1, b1, i1, h1, v1, f1), "{t0:?}: the way it runs and looks");
        assert!(near(*z0, *z1), "{t0:?}: the text size");
        assert!(near(text_margin(*z0, *m0), text_margin(*z1, *m1)), "{t0:?}: the margin ({m0} against {m1})");
        assert!(near(*k0, *k1) || (*k0 < 0.0 && *k1 < 0.0), "{t0:?}: the border width {k0} against {k1}");
    }
    let (ours_svg, theirs_svg) = (plotted_symbol(&work, &path, "Boxed", "boxes_ours", &[]), plotted_symbol(&work, &upgraded, "Boxed", "boxes_upgraded", &[]));
    assert!(ours_svg.matches("stroked-text").count() >= 4, "kicad-cli draws the text of every box ({} stroked texts)", ours_svg.matches("stroked-text").count());
    assert_eq!(ours_svg, theirs_svg, "the library this studio wrote plots as KiCad's own rewrite of it");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A text box's margin as the file has it: its own, or the default four fifths of the text height.
fn text_margin(size_mm: f64, margin_mm: f64) -> f64 {
    eda_model::ir::text_box_margin_mm(size_mm, margin_mm)
}
