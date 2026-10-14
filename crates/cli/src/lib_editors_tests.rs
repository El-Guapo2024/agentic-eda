//! The library editors' leftovers (GAPS.md package P17, items 19 and 22) through a board directory: what an array of pads writes into the project and into the derived
//! `.kicad_mod`, and (the slow tier, `EDA_SLOW_TESTS=1`) what kicad-cli makes of the files this studio writes.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_model::ir::{Point, Shape};
use eda_ops::{ArrayGeometry, AxisSpec, Cmd, Domain, NumberingType, PadNumbering};
use std::path::Path;

/// kicad-cli on `args`, in `work`; what it said, or what it said on failure.
fn kicad_cli(work: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let cli = eda_kicad_engine::find_cli().ok_or("no kicad-cli")?;
    let out = std::process::Command::new(cli).args(args).current_dir(work).output().map_err(|e| e.to_string())?;
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
