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
