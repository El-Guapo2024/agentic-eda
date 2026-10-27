//! Update Footprints from Library and Compare Footprint with Library (GAPS.md package P2): the library copy is read through the library tables, the board's footprint is
//! compared with it (`FootprintNeedsUpdate`), and an update puts the library's copy on the part.

use eda_model::units::mm;
use super::tests::{scratch, setup};
use super::{derived_schematic, load, save, step, undo};
use eda_ops::{Cmd, Domain, ExchangeOptions};
use std::path::Path;

fn board_with_library(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    setup(&dir);
    let intent = dir.join("intent.yaml");
    let text = std::fs::read_to_string(&intent).unwrap();
    std::fs::write(&intent, text.replace("2PAD", "Test:2PAD")).unwrap();
    let (_, mut design, model) = load(&dir).unwrap();
    let derived = derived_schematic(&design, &model).unwrap();
    design.schematic = derived.schematic;
    design.sheet_contents = derived.sheet_contents;
    save(&dir, &design).unwrap();
    write_wide(&dir, 1_000_000);
    assert_eq!(crate::library_tables::add_library(&dir, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/Alt.pretty"}"#)["ok"], true);
    dir
}

/// `Alt:WIDE` on disk with pads of `size_nm` square, at +-2 mm.
fn write_wide(dir: &Path, size_nm: i64) {
    std::fs::create_dir_all(dir.join("libs").join("Alt.pretty")).unwrap();
    let size = size_nm as f64 / 1e6;
    std::fs::write(
        dir.join("libs").join("Alt.pretty").join("WIDE.kicad_mod"),
        format!(
            r#"(footprint "WIDE" (version 20240108) (layer "F.Cu") (attr smd)
            (fp_line (start -3 -1) (end 3 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
            (pad "1" smd rect (at -2 0) (size {size} {size}) (layers "F.Cu" "F.Paste" "F.Mask"))
            (pad "2" smd rect (at 2 0) (size {size} {size}) (layers "F.Cu" "F.Paste" "F.Mask")))"#
        ),
    )
    .unwrap();
}

fn plan(dir: &Path, references: &[&str], to: Option<&str>) -> serde_json::Value {
    crate::fp_assign_api::exchange_plan(dir, serde_json::json!({ "references": references, "footprint": to }).to_string().as_bytes())
}

fn pad_sizes(dir: &Path, reference: &str) -> Vec<(eda_model::units::Nm, eda_model::units::Nm)> {
    let (_, _, model) = load(dir).unwrap();
    model.footprint_of(model.part(reference).unwrap()).unwrap().pads.iter().map(|p| p.size).collect()
}

#[test]
fn an_update_reads_the_library_copy_through_the_tables_and_lists_what_differs_before_it_changes_anything() {
    let dir = board_with_library("update_from_library");
    // U1 is put on Alt:WIDE; the board now holds its own copy of it
    step(&dir, Cmd::replace_footprint("U1", "Alt:WIDE"), false, "test").unwrap();
    assert_eq!(pad_sizes(&dir, "U1"), [(mm(1.0), mm(1.0)), (mm(1.0), mm(1.0))]);
    // as the library has it, the board's footprint needs no update
    let report = plan(&dir, &["U1"], None);
    assert_eq!(report["ok"], true, "{report}");
    let e = &report["entries"][0];
    assert_eq!((e["found"].as_bool(), e["differs"].as_bool()), (Some(true), Some(false)), "{e}");
    // the library changes: pads 1.2 mm
    write_wide(&dir, 1_200_000);
    let report = plan(&dir, &["U1"], None);
    let e = &report["entries"][0];
    assert_eq!((e["found"].as_bool(), e["differs"].as_bool()), (Some(true), Some(true)), "{e}");
    let lines: Vec<&str> = e["lines"].as_array().unwrap().iter().filter_map(|l| l.as_str()).collect();
    assert_eq!(lines, ["Pad 1 size differs on layer F.Cu.", "Pad 2 size differs on layer F.Cu."]);
    assert_eq!(pad_sizes(&dir, "U1"), [(mm(1.0), mm(1.0)), (mm(1.0), mm(1.0))], "listing changes nothing");
    // a footprint that is no library's (the intent's own) has no library copy
    let e = &plan(&dir, &["U2"], None)["entries"][0];
    assert_eq!((e["found"].as_bool(), e["differs"].as_bool()), (Some(false), Some(false)), "{e}");

    // the update takes the library's copy; one undo step
    let update = Cmd::ExchangeFootprints { references: vec!["U1".into()], footprint: None, options: ExchangeOptions::update(), library: Default::default() };
    step(&dir, update, false, "test").unwrap();
    assert_eq!(pad_sizes(&dir, "U1"), [(mm(1.2), mm(1.2)), (mm(1.2), mm(1.2))]);
    let e = &plan(&dir, &["U1"], None)["entries"][0];
    assert_eq!(e["differs"], false, "{e}");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(pad_sizes(&dir, "U1"), [(mm(1.0), mm(1.0)), (mm(1.0), mm(1.0))], "and Undo gives the board's copy back");
}

/// `FootprintNeedsUpdate` reports an override only when the board's value differs from the library's. A footprint placed from the Footprint Editor's
/// own library, whose pad says `(zone_connect 2)` and whose file sets `(clearance 0.25)` and `(solder_paste_margin 0.02)`, has those values on the
/// board too (`Design::pad_zone_facts`'s published entry), so an unedited footprint needs no update. Before, every set override was reported and the
/// update list showed a stale footprint.
#[test]
fn a_published_library_footprints_own_overrides_are_not_differences() {
    let dir = board_with_library("published_overrides");
    let text = r#"(footprint "Zone" (version 20240108) (layer "F.Cu") (attr smd) (clearance 0.25) (solder_paste_margin 0.02)
        (pad "1" smd rect (at -1 0) (size 0.8 0.8) (layers "F.Cu" "F.Paste" "F.Mask") (zone_connect 2) (thermal_bridge_angle 90))
        (pad "2" smd rect (at 1 0) (size 0.8 0.8) (layers "F.Cu" "F.Paste" "F.Mask")))"#;
    let mut fp = eda_kicad::parse_library_footprint(text).unwrap().footprint;
    fp.name = "Mine:Zone".into();
    // The Footprint Editor's entry, then the placement that publishes it (as Insert Footprint into PCB sends them)
    step(&dir, Cmd::PutLibraryFootprint { footprint: fp.clone(), overwrite: false }, false, "test").unwrap();
    let place = Cmd::PlaceFootprint { footprint: "Mine:Zone".into(), at: eda_model::ir::Point { x: mm(10.0), y: mm(15.0) }, reference: String::new(), value: "Zone".into(), definition: Some(fp) };
    step(&dir, Cmd::Batch { cmds: vec![place, Cmd::UpdateFootprintOnBoard { name: "Mine:Zone".into() }] }, false, "test").unwrap();
    let reference = {
        let (_, _, model) = load(&dir).unwrap();
        model.parts.iter().find(|p| p.footprint.as_deref() == Some("Mine:Zone")).expect("the placed footprint").reference.clone()
    };
    let diff = crate::fp_assign_api::footprint_diff(&dir, &reference);
    assert_eq!(diff["found"], true, "{diff}");
    assert!(diff["lines"].as_array().unwrap().is_empty(), "an unedited footprint needs no update: {diff}");
    let e = &plan(&dir, &[&reference], None)["entries"][0];
    assert_eq!((e["found"].as_bool(), e["differs"].as_bool()), (Some(true), Some(false)), "{e}");
}

#[test]
fn compare_footprint_with_library_reports_what_differs_and_why_a_library_copy_is_missing() {
    let dir = board_with_library("compare_with_library");
    step(&dir, Cmd::replace_footprint("U1", "Alt:WIDE"), false, "test").unwrap();
    let diff = crate::fp_assign_api::footprint_diff(&dir, "U1");
    assert_eq!(diff["ok"], true, "{diff}");
    assert_eq!((diff["library"].as_str(), diff["item"].as_str(), diff["found"].as_bool()), (Some("Alt"), Some("WIDE"), Some(true)));
    assert!(diff["lines"].as_array().unwrap().is_empty(), "no relevant differences: {diff}");
    assert_eq!(diff["board"]["pads"].as_array().unwrap().len(), 2);
    assert_eq!(diff["library_copy"]["pads"][0]["w"], 1_000_000);
    // the library moves on
    write_wide(&dir, 1_200_000);
    let diff = crate::fp_assign_api::footprint_diff(&dir, "U1");
    assert_eq!(diff["lines"].as_array().unwrap().len(), 2, "{diff}");
    assert_eq!(diff["board"]["pads"][0]["w"], 1_000_000);
    assert_eq!(diff["library_copy"]["pads"][0]["w"], 1_200_000, "the drawings are the two footprints");
    // the library has no such item any more
    std::fs::remove_file(dir.join("libs").join("Alt.pretty").join("WIDE.kicad_mod")).unwrap();
    let diff = crate::fp_assign_api::footprint_diff(&dir, "U1");
    assert_eq!((diff["found"].as_bool(), diff["reason"].as_str()), (Some(false), Some("The library no longer contains the item WIDE.")));
    // a footprint of no library of the tables
    let diff = crate::fp_assign_api::footprint_diff(&dir, "U2");
    assert_eq!((diff["found"].as_bool(), diff["reason"].as_str()), (Some(false), Some("The library is not included in the current configuration.")));
    let diff = crate::fp_assign_api::footprint_diff(&dir, "Z9");
    assert_eq!(diff["ok"], false);
}
