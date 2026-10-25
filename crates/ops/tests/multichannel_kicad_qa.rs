//! KiCad's own multichannel board (`qa/data/pcbnew/issue21739/topology_mismatch.kicad_pcb`, two channels of one sheet with a rule area for each, written by KiCad's Generate Placement
//! Rule Areas): the studio reads the areas and the footprints' sheets from it, finds the same two channels in the Generate dialog's rows, makes the same two areas from the same
//! footprints -- the corners KiCad wrote, to the micron -- and writes the board back with the same placement sources.
//!
//! The file is KiCad's, in its source tree: the test runs when `KICAD_SRC_DIR` names that tree (as the parity audit's does) and says so when it does not.

use eda_model::units::Nm;
use eda_kicad::{export_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, Point, Zone};
use eda_model::multichannel::PlacementSourceType;
use eda_ops::{AreaKind, AreaPick, Board, Cmd, MultichannelCmd};
use std::path::Path;

fn qa_board() -> Option<String> {
    let src = std::env::var_os("KICAD_SRC_DIR")?;
    let path = Path::new(&src).join("qa/data/pcbnew/issue21739/topology_mismatch.kicad_pcb");
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) => {
            eprintln!("{}: {e}; skipping", path.display());
            None
        }
    }
}

fn rule_areas(d: &Design) -> Vec<Zone> {
    d.routing.as_ref().map(|r| r.zones.iter().filter(|z| z.is_rule_area && z.placement.enabled).cloned().collect()).unwrap_or_default()
}

fn area_of<'a>(areas: &'a [Zone], source: &str) -> &'a Zone {
    areas.iter().find(|z| z.placement.source == source).unwrap_or_else(|| panic!("no rule area for {source}: {:?}", areas.iter().map(|z| (&z.name, &z.placement)).collect::<Vec<_>>()))
}

/// Two lists of corners are one polygon when they are the same cycle: the one may start at another corner than the other.
fn same_cycle(a: &[Point], b: &[Point]) -> bool {
    a.len() == b.len() && (0..a.len()).any(|shift| (0..a.len()).all(|i| a[i] == b[(i + shift) % b.len()]))
}

#[test]
fn the_rule_areas_of_kicads_multichannel_board_are_read_made_again_and_written_back_as_they_are() {
    let Some(text) = qa_board() else {
        eprintln!("KICAD_SRC_DIR is not set: skipping the board of KiCad's own tests");
        return;
    };
    let (design, model, _notes) = eda_kicad::import_kicad_pcb(&text).expect("KiCad's board imports");

    // the two areas the file has, each with the sheet it stands for
    let areas = rule_areas(&design);
    assert_eq!(areas.len(), 2, "{:?}", areas.iter().map(|z| (&z.name, &z.placement)).collect::<Vec<_>>());
    assert!(areas.iter().all(|z| z.placement.source_type == PlacementSourceType::Sheetname));
    let (ch0, ch1) = (area_of(&areas, "/i2c_thingy_ch0/").clone(), area_of(&areas, "/i2c_thingy_ch1/").clone());
    assert_eq!(ch0.layer_set(), vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"], "the four copper layers of the board, in stack order");

    // the Generate dialog's rows: the two channel sheets, each holding its own footprints (KiCad's file also has a footprint on the root sheet, which makes it a row too)
    let mut board = Board::new(design.clone(), &model, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let rows = board.mc_potential_areas();
    let row = |key: &str| rows.iter().find(|r| r.kind == AreaKind::Sheet && r.key == key).unwrap_or_else(|| panic!("no row for {key}: {rows:?}"));
    assert_eq!(row("/i2c_thingy_ch0/").components.len(), 2, "{rows:?}");
    assert_eq!(row("/i2c_thingy_ch1/").components.len(), 2, "{rows:?}");
    assert_eq!(row("/i2c_thingy_ch0/").file, "i2c_thingy.kicad_sch");

    // made again over the same footprints (Replace existing)
    board
        .apply(&Cmd::Multichannel(MultichannelCmd::GenerateRuleAreas {
            areas: ["/i2c_thingy_ch0/", "/i2c_thingy_ch1/"].iter().map(|k| AreaPick { kind: AreaKind::Sheet, key: k.to_string() }).collect(),
            replace_existing: true,
            group_items: false,
        }))
        .expect("Generate applies");
    let made = rule_areas(board.design());
    assert_eq!(made.len(), 2, "the areas of the file are replaced, not added to");
    // the area of the first channel has the corners KiCad wrote, to the micron: the rectified hull of the two footprints' boxes and 0.1 mm
    let ours = area_of(&made, "/i2c_thingy_ch0/");
    assert_eq!(ours.name, "auto-placement-area-/i2c_thingy_ch0/");
    assert!(same_cycle(&ours.outline, &ch0.outline), "the corners of the area KiCad made\n  KiCad: {:?}\n  here:  {:?}", ch0.outline, ours.outline);
    assert_eq!(ours.layer_set(), ch0.layer_set(), "on the copper layers of the board");
    // The file's other area is not the hull of its sheet's footprints: its polygon is the first one's, moved (what a Repeat Layout leaves of the area it copies to), and the footprints
    // of its sheet are elsewhere. Made from them, the area is a staircase round both footprints.
    let moved: Vec<Point> = ch0.outline.iter().map(|p| Point { x: p.x + Nm::from_raw_shim(12_155_000), y: p.y + Nm::from_raw_shim(30_000) }).collect();
    assert!(same_cycle(&moved, &ch1.outline), "KiCad's second area is the first one moved by 12.155 mm and 0.03 mm");
    let theirs = area_of(&made, "/i2c_thingy_ch1/");
    assert_eq!(theirs.name, "auto-placement-area-/i2c_thingy_ch1/");
    assert!(theirs.outline.len() >= 4 && theirs.outline.iter().enumerate().all(|(i, p)| { let q = theirs.outline[(i + 1) % theirs.outline.len()]; p.x == q.x || p.y == q.y }), "{:?}", theirs.outline);
    for reference in &row("/i2c_thingy_ch1/").components {
        let at = board.design().placement.as_ref().unwrap().footprints.iter().find(|f| &f.id == reference).unwrap().at;
        assert!(eda_model::multichannel::point_inside(&theirs.outline, at), "{reference} at {at:?} is inside the area made from it: {:?}", theirs.outline);
    }

    // and the file written back says the same of each area's placement
    let written = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-10-21", title: "qa" }).expect("the board exports");
    for source in ["/i2c_thingy_ch0/", "/i2c_thingy_ch1/"] {
        assert!(written.contains(&format!("(placement (enabled yes) (sheetname \"{source}\"))")), "{source}: {}", written.lines().filter(|l| l.contains("placement")).collect::<Vec<_>>().join("\n"));
    }
    // the footprints keep the sheet they are on
    assert!(written.matches("(sheetname \"/i2c_thingy_ch0/\")").count() >= 3 && written.matches("(sheetname \"/i2c_thingy_ch1/\")").count() >= 3, "the footprints and the areas name the sheets");
}
