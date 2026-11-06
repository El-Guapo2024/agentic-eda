//! Place > Auto-Place Footprints through a board directory (UI-LEFT P19): the autoplacer's command is one undo step, it is judged by the placement gates
//! even with Strict off (a placement that adds a gate failure is not applied, and the refusal names the failures), the placer's own refusal leaves the
//! board as it was, and an editor's placement keeps the point it was given (the server no longer rounds a footprint's position to its 0.1 mm step).

use super::tests::{scratch, setup};
use super::{load, meta_path, redo, save, step, undo, Meta};
use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, Side};
use eda_model::units::{mm, Nm};
use eda_model::{ConstraintModel, Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind, PlacementRule};
use eda_ops::autoplace::AutoplaceCmd;
use eda_ops::{Cmd, Domain};
use std::path::Path;

fn pose(dir: &Path, id: &str) -> Option<(Point, u32)> {
    load(dir).unwrap().1.placement.unwrap().footprints.iter().find(|f| f.id == id).map(|f| (f.at, f.rot))
}

/// Three 2-pad parts on a 20 x 20 mm board: U1 and U3 placed, U2 not on the board; `rules` are the intent's placement rules.
fn setup_three(dir: &Path, rules: Vec<PlacementRule>) {
    let pad = |n: &str, x: f64| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (mm(x), Nm::ZERO), size: (mm(0.8), mm(0.8)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
    let fp = Footprint { name: "2PAD".into(), pads: vec![pad("1", -1.0), pad("2", 1.0)], courtyard: Some((mm(2.0), mm(1.0))), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() };
    let part = |r: &str| Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some("2PAD".into()),
        footprint: Some("2PAD".into()),
        pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
        body_nm: None,
        symbol: None,
        datasheet: None,
        edge: None,
    };
    let model = ConstraintModel {
        parts: vec![part("U1"), part("U2"), part("U3")],
        nets: vec![Net { name: "A".into(), pins: vec!["U1.1".into(), "U2.1".into()] }, Net { name: "B".into(), pins: vec!["U2.2".into(), "U3.2".into()] }],
        footprints: vec![fp],
        placement_rules: rules,
        ..Default::default()
    };
    let intent_path = dir.join("intent.yaml");
    std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();
    let at = |id: &str, x: f64, y: f64| FootprintInstance { id: id.into(), at: Point { x: mm(x), y: mm(y) }, rot: 0, side: Side::Top, label: LabelSide::default() };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection {
            outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(20.0), y: Nm::ZERO }, Point { x: mm(20.0), y: mm(20.0) }, Point { x: Nm::ZERO, y: mm(20.0) }],
            footprints: vec![at("U1", 5.0, 5.0), at("U3", 15.0, 15.0)],
            modules: vec![],
        }),
        routing: None,
        drawings: None,
    };
    save(dir, &design).unwrap();
    let meta = Meta { intent: intent_path.display().to_string(), snap_nm: mm(0.1), spacing_nm: mm(0.3) };
    std::fs::write(meta_path(dir), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
}

#[test]
fn place_off_board_footprints_is_one_undo_step_and_leaves_the_others_where_they_are() {
    let dir = scratch("autoplace_offboard_undo");
    setup_three(&dir, vec![]);
    let u1 = pose(&dir, "U1");
    assert!(pose(&dir, "U2").is_none());
    step(&dir, Cmd::Autoplace(AutoplaceCmd { offboard: true, grid_nm: Some(mm(0.05)), ..Default::default() }), false, "ui").unwrap();
    let (at, _) = pose(&dir, "U2").expect("U2 is on the board now");
    assert!(at.x > Nm::ZERO && at.x < mm(20.0) && at.y > Nm::ZERO && at.y < mm(20.0), "{at:?}");
    assert_eq!((at.x.get() % 50_000, at.y.get() % 50_000), (0, 0), "on the editor's grid");
    assert_eq!(pose(&dir, "U1"), u1);
    undo(&dir, "ui", Some(Domain::Pcb)).unwrap();
    assert!(pose(&dir, "U2").is_none(), "one undo takes the whole placement back");
    redo(&dir, "ui", Some(Domain::Pcb)).unwrap();
    assert_eq!(pose(&dir, "U2").map(|p| p.0), Some(at));
    let log = std::fs::read_to_string(dir.join("activity.jsonl")).unwrap_or_default();
    assert!(log.contains("autoplace --offboard"), "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_placement_the_gates_refuse_is_not_applied_even_with_strict_off() {
    // U2 must stay 50 mm from U1 and from U3 on a 20 mm board: placing it clears one gate failure (U2 not placed) and adds two (the separations).
    let dir = scratch("autoplace_gated");
    let sep = |b: &str| PlacementRule::Separation { a: "U2".into(), b: b.into(), min_mm: 50.0, reason: None };
    setup_three(&dir, vec![sep("U1"), sep("U3")]);
    let before = std::fs::read_to_string(dir.join("design.json")).unwrap();
    let e = step(&dir, Cmd::Autoplace(AutoplaceCmd { offboard: true, ..Default::default() }), false, "ui").unwrap_err();
    assert_eq!(e[0].check, "board_worse", "{e:?}");
    let why = e[0].hint.clone().unwrap_or_default();
    assert!(why.contains("placement_separation"), "the refusal names the failures: {why}");
    assert_eq!(std::fs::read_to_string(dir.join("design.json")).unwrap(), before, "nothing applied");
    // the same refusal with Strict on, of course
    assert_eq!(step(&dir, Cmd::Autoplace(AutoplaceCmd { offboard: true, ..Default::default() }), true, "ui").unwrap_err()[0].check, "board_worse");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_to_place_is_refused() {
    let dir = scratch("autoplace_no_room");
    setup(&dir);
    // a selection of nothing that can be placed: U9 is no footprint, and nothing else is asked for
    let e = step(&dir, Cmd::Autoplace(AutoplaceCmd { footprints: vec!["U9".into()], ..Default::default() }), false, "ui").unwrap_err();
    assert_eq!(e[0].check, "ops_autoplace_nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_editor_placement_keeps_its_point_on_any_grid() {
    // UI-LEFT P19: placing on a 0.05 mm (or 25 mil) grid stays on that grid; the server used to round it to the board's 0.1 mm step.
    let dir = scratch("autoplace_exact_place");
    setup_three(&dir, vec![]);
    step(&dir, Cmd::PlaceAt { part: "U2".into(), x: mm(10.05), y: Nm::new(10_160_000) }, false, "ui").unwrap();
    assert_eq!(pose(&dir, "U2").unwrap().0, Point { x: mm(10.05), y: Nm::new(10_160_000) });
    let _ = std::fs::remove_dir_all(&dir);
}
