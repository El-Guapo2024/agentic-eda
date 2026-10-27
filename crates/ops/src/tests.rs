use super::*;
use eda_model::ir::{ArrowDirection, DimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, PlacementSection, Provenance};
use eda_model::{Net, Part, Pin, PinKind, PlacementRule};
use eda_model::units::{mm, um};

fn part(r: &str, pkg: &str) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some(pkg.into()),
        footprint: None,
        pins: vec![
            Pin { number: "1".into(), name: None, kind: PinKind::Passive },
            Pin { number: "2".into(), name: None, kind: PinKind::Passive },
        ],
        body_nm: None, symbol: None, datasheet: None,
        edge: None,
    }
}

fn model(parts: Vec<Part>, nets: &[(&str, &[&str])], rules: Vec<PlacementRule>) -> ConstraintModel {
    ConstraintModel {
        parts,
        nets: nets
            .iter()
            .map(|(n, ps)| Net { name: (*n).into(), pins: ps.iter().map(|p| format!("{p}.1")).collect() })
            .collect(),
        placement_rules: rules,
        ..Default::default()
    }
}

/// An empty 100x100mm board.
fn empty_design() -> Design {
    Design {
        footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        routing: None,
        placement: Some(PlacementSection {
            outline: vec![
                Point { x: Nm::ZERO, y: Nm::ZERO },
                Point { x: mm(100.0), y: Nm::ZERO },
                Point { x: mm(100.0), y: mm(100.0) },
                Point { x: Nm::ZERO, y: mm(100.0) },
            ],
            footprints: Vec::new(),
            modules: Vec::new(),
        }),
        drawings: None,
    }
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(empty_design(), m, mm(0.1), mm(0.3))
}

// ------------------------------------------------------------ refusals

#[test]
fn placing_an_unknown_part_is_refused() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    let e = b.apply(&Cmd::Place { part: "C9".into(), anchor: "U1".into(), side: Dir::East }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_part");
}

#[test]
fn placing_a_part_twice_is_refused() {
    // Silently moving it instead would make a placer's history a lie.
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::East }).unwrap();
    let e = b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::North }).unwrap_err();
    assert_eq!(e[0].check, "ops_already_placed");
}

#[test]
fn anchoring_to_an_unplaced_part_is_refused() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402")], &[], vec![]);
    let mut b = board(&m);
    let e = b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::East }).unwrap_err();
    assert_eq!(e[0].check, "ops_not_placed");
}

#[test]
fn nudging_an_unplaced_part_is_refused() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    let e = b.apply(&Cmd::Nudge { part: "U1".into(), dir: Dir::East, steps: 3 }).unwrap_err();
    assert_eq!(e[0].check, "ops_not_placed");
}

#[test]
fn swapping_a_part_with_itself_is_refused() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    let e = b.apply(&Cmd::Swap { a: "U1".into(), b: "U1".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_swap_self");
}

// ------------------------------------------------------------ geometry

#[test]
fn place_puts_the_part_on_the_side_asked_for() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    let u = b.pose_of("U1").unwrap().at;
    b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::East }).unwrap();
    let c = b.pose_of("C1").unwrap().at;
    assert!(c.x > u.x, "east means greater x: U1 at {u:?}, C1 at {c:?}");

    // North is negative y, as everywhere else here.
    let m2 = model(vec![part("U1", "SOIC-8"), part("C2", "0402")], &[], vec![]);
    let mut b2 = board(&m2);
    b2.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    let u2 = b2.pose_of("U1").unwrap().at;
    b2.apply(&Cmd::Place { part: "C2".into(), anchor: "U1".into(), side: Dir::North }).unwrap();
    assert!(b2.pose_of("C2").unwrap().at.y < u2.y);
}

#[test]
fn a_placed_part_never_overlaps_one_already_down() {
    // Six capacitors all asked for the same side of the same IC. The
    // second onward have to slide, and none may land on another.
    let mut parts = vec![part("U1", "SOIC-8")];
    for i in 1..=6 {
        parts.push(part(&format!("C{i}"), "0402"));
    }
    let m = model(parts, &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    for i in 1..=6 {
        b.apply(&Cmd::Place { part: format!("C{i}"), anchor: "U1".into(), side: Dir::East })
            .unwrap_or_else(|e| panic!("C{i} could not be placed: {e:?}"));
    }
    assert_eq!(crate::repair::courtyard_overlaps(&b), 0, "sliding must not stack parts");
}

#[test]
fn place_is_deterministic() {
    // Two boards built by the same commands must be byte-identical, or
    // every measurement taken on this placer is noise.
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402"), part("C2", "0402")], &[], vec![]);
    let build = || {
        let mut b = board(&m);
        b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
        b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::East }).unwrap();
        b.apply(&Cmd::Place { part: "C2".into(), anchor: "U1".into(), side: Dir::East }).unwrap();
        serde_json::to_string(b.design()).unwrap()
    };
    assert_eq!(build(), build());
}

#[test]
fn rip_takes_a_part_off_so_another_choice_can_be_made() {
    // Backtracking is the thing a one-shot placer cannot do.
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402")], &[], vec![]);
    let mut b = board(&m);
    // Seeded in the middle: an anchor on the west edge has no west side.
    b.apply(&Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre }).unwrap();
    b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::East }).unwrap();
    let east = b.pose_of("C1").unwrap().at;
    b.apply(&Cmd::Rip { part: "C1".into() }).unwrap();
    assert!(b.pose_of("C1").is_none());
    b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::West }).unwrap();
    assert!(b.pose_of("C1").unwrap().at.x < east.x, "the second choice must actually differ");
}

// ------------------------------------------------------------ frontier

#[test]
fn the_frontier_is_only_parts_with_a_placed_neighbour() {
    let m = model(
        vec![part("U1", "SOIC-8"), part("C1", "0402"), part("R9", "0402")],
        &[("N1", &["U1", "C1"])],
        vec![],
    );
    let mut b = board(&m);
    assert!(b.frontier().is_empty(), "nothing is placed, so nothing is anchored");
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    assert_eq!(b.frontier(), vec!["C1".to_string()], "R9 shares nothing with U1");
}

#[test]
fn a_proximity_rule_makes_a_part_a_neighbour() {
    // Rules are intent about adjacency, so they belong in the frontier
    // even when no net connects the two.
    let m = model(
        vec![part("U1", "SOIC-8"), part("R9", "0402")],
        &[],
        vec![PlacementRule::Proximity { a: "U1".into(), b: "R9".into(), max_mm: 2.0, reason: None }],
    );
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    assert_eq!(b.frontier(), vec!["R9".to_string()]);
}

#[test]
fn a_power_net_does_not_make_everything_a_neighbour() {
    // Without the fanout cut, GND makes every part adjacent to every
    // other and the frontier stops carrying information.
    let parts: Vec<Part> = (1..=10).map(|i| part(&format!("C{i}"), "0402")).collect();
    let all: Vec<&str> = ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"].into();
    let m = model(parts, &[("GND", &all)], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "C1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    assert!(b.frontier().is_empty(), "a 10-part net must not anchor the whole board");
}

#[test]
fn a_decoupling_cap_is_a_neighbour_of_its_ic_across_the_fanout_cut() {
    // A decoupling cap sits on nothing but a rail and ground -- the nets
    // the fanout cut drops -- so it had no neighbour at all: never on the
    // frontier, never anchored to its IC, seeded wherever a region had
    // room. It is its IC's neighbour by name now. A cap a proximity rule
    // already places is left to that rule, as the decoupling gate leaves
    // it.
    let mut ic = part("U1", "SOIC-8");
    ic.pins[0].kind = PinKind::Power;
    ic.pins[1].kind = PinKind::Ground;
    let mut parts = vec![ic, part("C1", "0402"), part("C2", "0402"), part("R9", "0402")];
    parts.extend((1..=6).map(|i| part(&format!("R{i}"), "0402")));
    let rail = |pin: &str| parts.iter().filter(|p| p.reference != "R9").map(|p| format!("{}.{pin}", p.reference)).collect::<Vec<_>>();
    let nets = vec![Net { name: "VCC".into(), pins: rail("1") }, Net { name: "GND".into(), pins: rail("2") }];
    let m = ConstraintModel {
        parts,
        nets,
        placement_rules: vec![PlacementRule::Proximity { a: "C2".into(), b: "R9".into(), max_mm: 2.0, reason: None }],
        ..Default::default()
    };
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    assert_eq!(b.frontier(), vec!["C1".to_string()], "C1 decouples U1; C2 has its own rule; the rails anchor nothing");
}

// ------------------------------------------------------------- outcome

#[test]
fn try_apply_reverts_a_move_that_makes_things_worse() {
    let m = model(
        vec![part("U1", "SOIC-8"), part("C1", "0402")],
        &[],
        vec![PlacementRule::Proximity { a: "U1".into(), b: "C1".into(), max_mm: 1.0, reason: None }],
    );
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::East }).unwrap();
    let before = b.pose_of("C1").unwrap().at;
    // Shove it far away; the proximity rule should reject the move.
    let out = b.try_apply(&Cmd::Nudge { part: "C1".into(), dir: Dir::East, steps: 400 }, false).unwrap();
    assert!(!out.improved());
    assert_eq!(b.pose_of("C1").unwrap().at, before, "a rejected move must leave no trace");
}

#[test]
fn a_level_move_is_kept_only_when_the_caller_allows_it() {
    // The repairs this project cannot make need two or three parts moved
    // together, and the first move fixes nothing on its own. A strict
    // rule forbids exactly the sequence that works, so the choice has to
    // be the caller's.
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceEdge { part: "U1".into(), edge: Dir::West, fraction: 0.5 }).unwrap();
    b.apply(&Cmd::Place { part: "C1".into(), anchor: "U1".into(), side: Dir::East }).unwrap();
    let before = b.pose_of("C1").unwrap().at;

    let strict = b.try_apply(&Cmd::Nudge { part: "C1".into(), dir: Dir::East, steps: 2 }, false).unwrap();
    assert!(strict.level());
    assert_eq!(b.pose_of("C1").unwrap().at, before, "strict mode reverts a level move");

    let loose = b.try_apply(&Cmd::Nudge { part: "C1".into(), dir: Dir::East, steps: 2 }, true).unwrap();
    assert!(loose.level());
    assert_ne!(b.pose_of("C1").unwrap().at, before, "level mode keeps it");
}

// -------------------------------------------------------- seed placement

#[test]
fn a_seed_part_can_land_without_an_anchor() {
    // The first part has nothing to hang off, and a board that can only
    // start at an edge cannot put its MCU in the middle.
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre }).unwrap();
    let at = b.pose_of("U1").unwrap().at;
    assert!((mm(40.0)..=mm(60.0)).contains(&at.x), "centre region should be mid-board, got {at:?}");
    assert!((mm(40.0)..=mm(60.0)).contains(&at.y), "centre region should be mid-board, got {at:?}");
}

#[test]
fn seeding_the_same_region_twice_does_not_stack() {
    // The spiral has to find the next free spot, not refuse and not
    // overlap.
    let m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre }).unwrap();
    b.apply(&Cmd::PlaceRegion { part: "U2".into(), region: Region::Centre }).unwrap();
    assert_ne!(b.pose_of("U1").unwrap().at, b.pose_of("U2").unwrap().at);
    assert_eq!(crate::repair::courtyard_overlaps(&b), 0, "two parts seeded into one region must not overlap");
}

#[test]
fn every_region_lands_somewhere_distinct() {
    let parts: Vec<Part> = (0..9).map(|i| part(&format!("U{i}"), "0402")).collect();
    let m = model(parts, &[], vec![]);
    let mut b = board(&m);
    let mut seen = std::collections::BTreeSet::new();
    for (i, r) in Region::ALL.iter().enumerate() {
        b.apply(&Cmd::PlaceRegion { part: format!("U{i}"), region: *r }).unwrap();
        let at = b.pose_of(&format!("U{i}")).unwrap().at;
        assert!(seen.insert((at.x, at.y)), "{} landed on top of an earlier region", r.as_str());
    }
}

// --------------------------------------------------------------- flip

#[test]
fn flip_turns_a_placed_part_to_the_other_side() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre }).unwrap();
    assert_eq!(b.pose_of("U1").unwrap().side, Side::Top);
    b.apply(&Cmd::Flip { part: "U1".into() }).unwrap();
    assert_eq!(b.pose_of("U1").unwrap().side, Side::Bottom);
    b.apply(&Cmd::Flip { part: "U1".into() }).unwrap();
    assert_eq!(b.pose_of("U1").unwrap().side, Side::Top);
}

#[test]
fn flipping_an_unplaced_part_is_refused() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    let e = b.apply(&Cmd::Flip { part: "U1".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_not_placed");
}

#[test]
fn set_label_side_changes_the_refdes_label_side_only() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre }).unwrap();
    assert_eq!(b.pose_of("U1").unwrap().label, LabelSide::Above, "LabelSide::default() is Above");

    let before = b.pose_of("U1").unwrap().clone();
    b.apply(&Cmd::SetLabelSide { part: "U1".into(), side: LabelSide::Left }).unwrap();
    let after = b.pose_of("U1").unwrap();
    assert_eq!(after.label, LabelSide::Left);
    assert_eq!(after.at, before.at, "label side must not move the part");
    assert_eq!(after.rot, before.rot);
    assert_eq!(after.side, before.side);
}

#[test]
fn set_label_side_on_an_unplaced_part_is_refused() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    let e = b.apply(&Cmd::SetLabelSide { part: "U1".into(), side: LabelSide::Below }).unwrap_err();
    assert_eq!(e[0].check, "ops_not_placed");
}

// ------------------------------------------------------------- tracks

fn net_model() -> ConstraintModel {
    model(vec![part("U1", "SOIC-8"), part("C1", "0402")], &[("GND", &["U1", "C1"])], vec![])
}

#[test]
fn add_track_needs_a_real_net_and_a_real_layer() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::AddTrack { net: "NOPE".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_net");

    let e = b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "In1.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_layer");

    let e = b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_track");
}

#[test]
fn add_track_assigns_an_id_that_set_track_width_and_delete_track_can_use() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
    assert!(!id.is_empty(), "a track must get an id the moment it is added");

    b.apply(&Cmd::SetTrackWidth { id: id.clone(), width: mm(0.35) }).unwrap();
    let t = &b.design().routing.as_ref().unwrap().tracks[0];
    assert_eq!(t.width, mm(0.35));
    assert_eq!(t.id, id, "changing width must not move the id");

    b.apply(&Cmd::DeleteTrack { id: id.clone() }).unwrap();
    assert!(b.design().routing.as_ref().unwrap().tracks.is_empty());

    let e = b.apply(&Cmd::DeleteTrack { id }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_track");
}

#[test]
fn adding_a_track_does_not_disturb_an_existing_one() {
    // Two AddTracks on the same net must not collide on id or clobber
    // each other.
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: mm(2.0) }, Point { x: mm(1.0), y: mm(2.0) }] }).unwrap();
    let ids: std::collections::BTreeSet<String> = b.design().routing.as_ref().unwrap().tracks.iter().map(|t| t.id.clone()).collect();
    assert_eq!(ids.len(), 2, "two different tracks must get two different ids");
}

// --------------------------------------------------------------- vias

#[test]
fn add_via_rejects_a_drill_not_smaller_than_its_pad() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: mm(5.0), drill: mm(0.6), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
    let e = b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: mm(5.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "F.Cu".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
}

#[test]
fn move_via_changes_position_but_not_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: mm(5.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().vias[0].id.clone();

    b.apply(&Cmd::MoveVia { id: id.clone(), x: mm(9.0), y: mm(9.0) }).unwrap();
    let v = &b.design().routing.as_ref().unwrap().vias[0];
    assert_eq!(v.at, Point { x: mm(9.0), y: mm(9.0) });
    assert_eq!(v.id, id);

    b.apply(&Cmd::DeleteVia { id: id.clone() }).unwrap();
    assert!(b.design().routing.as_ref().unwrap().vias.is_empty());
    assert_eq!(b.apply(&Cmd::MoveVia { id, x: Nm::ZERO, y: Nm::ZERO }).unwrap_err()[0].check, "ops_unknown_via");
}

#[test]
fn edit_via_changes_diameter_and_drill_but_not_net_or_position() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: mm(5.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().vias[0].id.clone();

    b.apply(&Cmd::EditVia { id: id.clone(), diameter: mm(0.8), drill: mm(0.4) }).unwrap();
    let v = &b.design().routing.as_ref().unwrap().vias[0];
    assert_eq!(v.id, id);
    assert_eq!((v.diameter, v.drill), (mm(0.8), mm(0.4)));
    assert_eq!(v.net, "GND");
    assert_eq!(v.at, Point { x: mm(5.0), y: mm(5.0) });

    let e = b.apply(&Cmd::EditVia { id: id.clone(), diameter: mm(0.4), drill: mm(0.4) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via", "drill not smaller than diameter must be refused, same as AddVia");
    // The refused edit must not have applied halfway.
    assert_eq!(b.design().routing.as_ref().unwrap().vias[0].diameter, mm(0.8));

    assert_eq!(b.apply(&Cmd::EditVia { id: "via_nope".into(), diameter: mm(0.6), drill: mm(0.3) }).unwrap_err()[0].check, "ops_unknown_via");
}

#[test]
fn track_width_and_via_presets_round_trip_and_reject_bad_entries() {
    let m = net_model();
    let mut b = board(&m);
    assert!(b.design().routing.is_none(), "an empty board starts with no routing section at all");

    b.apply(&Cmd::SetTrackWidthPresets { widths: vec![mm(0.15), mm(0.25), mm(0.4)] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().track_width_presets, vec![mm(0.15), mm(0.25), mm(0.4)]);

    b.apply(&Cmd::SetViaPresets { presets: vec![ViaPreset { diameter: mm(0.6), drill: mm(0.3) }, ViaPreset { diameter: mm(0.8), drill: mm(0.4) }] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().via_presets.len(), 2);

    let e = b.apply(&Cmd::SetTrackWidthPresets { widths: vec![mm(0.2), Nm::ZERO] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_track");
    // A refused replace must leave the previous list in place.
    assert_eq!(b.design().routing.as_ref().unwrap().track_width_presets, vec![mm(0.15), mm(0.25), mm(0.4)]);

    let e = b.apply(&Cmd::SetViaPresets { presets: vec![ViaPreset { diameter: mm(0.3), drill: mm(0.3) }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
}

// ------------------------------------------- global edit: tracks & vias

#[test]
fn edit_tracks_and_vias_sets_explicit_width_diameter_drill_and_layer() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: mm(5.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let track_id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
    let via_id = b.design().routing.as_ref().unwrap().vias[0].id.clone();

    b.apply(&Cmd::EditTracksAndVias {
        ids: vec![track_id.clone(), via_id.clone()],
        track_width: Some(SizeSpec::Value { nm: mm(0.35) }),
        via_size: Some(ViaSizeSpec::Value { diameter: mm(0.9), drill: mm(0.45) }),
        layer: Some("B.Cu".into()),
    })
    .unwrap();

    let t = &b.design().routing.as_ref().unwrap().tracks[0];
    assert_eq!(t.width, mm(0.35));
    assert_eq!(t.layer, "B.Cu", "a track's layer is editable; a via's layer span is not touched by this command");
    assert_eq!(t.id, track_id, "style/layer edits must not move the id");
    let v = &b.design().routing.as_ref().unwrap().vias[0];
    assert_eq!((v.diameter, v.drill), (mm(0.9), mm(0.45)));
    assert_eq!((v.from_layer.as_str(), v.to_layer.as_str()), ("F.Cu", "B.Cu"));
}

#[test]
fn edit_tracks_and_vias_net_class_resolves_each_items_own_net() {
    let mut m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[("FAST", &["U1"]), ("SLOW", &["U2"])], vec![]);
    m.board.net_classes = vec![eda_model::NetClass { name: "fast".into(), nets: vec!["FAST".into()], track_width: Some(mm(0.5)), clearance: None, via_diameter: Some(mm(1.0)), via_drill: Some(mm(0.5)), microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }];
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "FAST".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    b.apply(&Cmd::AddTrack { net: "SLOW".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: mm(2.0) }, Point { x: mm(1.0), y: mm(2.0) }] }).unwrap();
    let ids: Vec<String> = b.design().routing.as_ref().unwrap().tracks.iter().map(|t| t.id.clone()).collect();

    b.apply(&Cmd::EditTracksAndVias { ids, track_width: Some(SizeSpec::NetClass), via_size: None, layer: None }).unwrap();

    let widths: std::collections::BTreeMap<String, Nm> = b.design().routing.as_ref().unwrap().tracks.iter().map(|t| (t.net.clone(), t.width)).collect();
    assert_eq!(widths["FAST"], mm(0.5), "FAST is in the fast net class, which overrides track width");
    assert_eq!(widths["SLOW"], m.board.track_width, "SLOW has no class, so it falls back to the board default");
}

/// r2-pcb-edit C3: "Set to net class / custom rule values" is `PCB_EDIT_FRAME::SetTrackSegmentWidth( .., true )`: the width the DRC engine's
/// `TRACK_WIDTH_CONSTRAINT` gives the item (its opt, else its min), so a custom rule wins over the class and a net of a diff pair takes the class's diff-pair
/// width (`drc_engine.cpp`'s `A.hasExactNetclass(..) && A.inDiffPair('*')` rule); a via its `VIA_DIAMETER_CONSTRAINT` and `HOLE_SIZE_CONSTRAINT`.
#[test]
fn edit_tracks_and_vias_net_class_takes_the_diff_pair_width_and_custom_rules() {
    let mut m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[("USB_P", &["U1"]), ("USB_N", &["U2"]), ("VBUS", &["U1"])], vec![]);
    m.board.net_classes = vec![eda_model::NetClass { name: "HS".into(), nets: vec!["USB_P".into(), "USB_N".into()], track_width: Some(mm(0.2)), clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: Some(mm(0.12)), diff_pair_gap: Some(mm(0.15)), diff_pair_via_gap: None, priority: 0 }];
    m.board.custom_rules_text = Some("(version 1)\n(rule w\n  (condition \"A.NetName == 'VBUS'\")\n  (constraint track_width (opt 0.5mm)))\n(rule v\n  (condition \"A.NetName == 'VBUS'\")\n  (constraint via_diameter (opt 0.9mm))\n  (constraint hole_size (opt 0.4mm)))\n".into());
    let mut b = board(&m);
    for (i, net) in ["USB_P", "USB_N", "VBUS"].iter().enumerate() {
        let y = mm(i as f64 * 2.0);
        b.apply(&Cmd::AddTrack { net: (*net).into(), layer: "F.Cu".into(), width: mm(0.3), pts: vec![Point { x: Nm::ZERO, y }, Point { x: mm(5.0), y }] }).unwrap();
    }
    b.apply(&Cmd::AddVia { net: "VBUS".into(), x: mm(8.0), y: mm(4.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let rt = b.design().routing.as_ref().unwrap();
    let mut ids: Vec<String> = rt.tracks.iter().map(|t| t.id.clone()).collect();
    ids.push(rt.vias[0].id.clone());
    b.apply(&Cmd::EditTracksAndVias { ids, track_width: Some(SizeSpec::NetClass), via_size: Some(ViaSizeSpec::NetClass), layer: None }).unwrap();
    let rt = b.design().routing.as_ref().unwrap();
    let widths: std::collections::BTreeMap<String, Nm> = rt.tracks.iter().map(|t| (t.net.clone(), t.width)).collect();
    assert_eq!(widths["USB_P"], mm(0.12), "a diff pair's net takes the class's diff-pair width");
    assert_eq!(widths["USB_N"], mm(0.12));
    assert_eq!(widths["VBUS"], mm(0.5), "the custom rule's opt");
    assert_eq!((rt.vias[0].diameter, rt.vias[0].drill), (mm(0.9), mm(0.4)));
}

#[test]
fn edit_tracks_and_vias_refuses_empty_ids_and_bad_explicit_values() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::EditTracksAndVias { ids: vec![], track_width: Some(SizeSpec::NetClass), via_size: None, layer: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_global_edit");

    let e = b.apply(&Cmd::EditTracksAndVias { ids: vec!["whatever".into()], track_width: Some(SizeSpec::Value { nm: Nm::ZERO }), via_size: None, layer: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_track");

    let e = b.apply(&Cmd::EditTracksAndVias { ids: vec!["whatever".into()], track_width: None, via_size: Some(ViaSizeSpec::Value { diameter: mm(0.3), drill: mm(0.3) }), layer: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
}

#[test]
fn edit_tracks_and_vias_tolerates_unknown_ids_among_known_ones() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
    b.apply(&Cmd::EditTracksAndVias { ids: vec![id, "nope".into()], track_width: Some(SizeSpec::Value { nm: mm(0.5) }), via_size: None, layer: None }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().tracks[0].width, mm(0.5));
}

// --------------------------------------------------------------- zones

#[test]
fn add_zone_then_delete_it() {
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }];
    let e = b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1), y: Nm::new(1) }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");

    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: outline.clone() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    assert!(!id.is_empty());
    b.apply(&Cmd::DeleteZone { id }).unwrap();
    assert!(b.design().routing.as_ref().unwrap().zones.is_empty());
}

#[test]
fn add_zone_allows_an_empty_net_for_a_rule_area() {
    // A keepout normally has no net at all (KiCad's net code 0) -- this
    // must not be refused the way a real but-unknown net name still is.
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }, Point { x: mm(5.0), y: mm(5.0) }];
    b.apply(&Cmd::AddZone { net: "".into(), layer: "F.Cu".into(), outline }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().zones[0].net, "");

    let e = b.apply(&Cmd::AddZone { net: "NOPE".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1), y: Nm::ZERO }, Point { x: Nm::new(1), y: Nm::new(1) }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_net", "a real but unknown net name must still be refused");
}

#[test]
fn edit_zone_keepout_flags_round_trip_and_empty_net_is_allowed() {
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }, Point { x: mm(5.0), y: mm(5.0) }];
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();

    b.apply(&edit_zone_cmd(id.clone(), "", |z| {
        z.is_rule_area = true;
        z.keepout_tracks = true;
        z.keepout_vias = true;
        z.keepout_pads = false;
        z.keepout_copper_pour = true;
        z.keepout_footprints = false;
    }))
    .unwrap();

    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!(z.id, id, "id must not move");
    assert_eq!(z.net, "", "a rule area may drop to no net");
    assert!(z.is_rule_area);
    assert!(z.keepout_tracks && z.keepout_vias && z.keepout_copper_pour);
    assert!(!z.keepout_pads && !z.keepout_footprints);
}

fn edit_zone_cmd(id: String, net: &str, overrides: impl FnOnce(&mut Zone)) -> Cmd {
    // `panel_zone_properties.cpp`'s real defaults (`ZONE_SETTINGS::
    // ZONE_SETTINGS()`), the same ones `Zone::default()` carries -- tests
    // only override the one or two fields they're checking.
    let mut z = Zone { id, net: net.into(), layer: "F.Cu".into(), outline: vec![], ..Default::default() };
    overrides(&mut z);
    Cmd::EditZone {
        id: z.id,
        net: z.net,
        layer: z.layer,
        layers: if z.layers.is_empty() { None } else { Some(z.layers.clone()) },
        clearance: z.clearance,
        min_thickness: z.min_thickness,
        thermal_gap: z.thermal_gap,
        thermal_spoke_width: z.thermal_spoke_width,
        pad_connection: z.pad_connection,
        priority: z.priority,
        island_removal_mode: z.island_removal_mode,
        min_island_area: z.min_island_area,
        fill_mode: z.fill_mode,
        hatch_thickness: z.hatch_thickness,
        hatch_gap: z.hatch_gap,
        hatch_orientation_mdeg: z.hatch_orientation_mdeg,
        hatch_smoothing_level: z.hatch_smoothing_level,
        hatch_smoothing_value: z.hatch_smoothing_value,
        hatch_hole_min_area: z.hatch_hole_min_area,
        hatch_border_algorithm: z.hatch_border_algorithm,
        is_rule_area: z.is_rule_area,
        keepout_tracks: z.keepout_tracks,
        keepout_vias: z.keepout_vias,
        keepout_pads: z.keepout_pads,
        keepout_copper_pour: z.keepout_copper_pour,
        keepout_footprints: z.keepout_footprints,
        smoothing: Some(z.smoothing),
        corner_radius: Some(z.corner_radius),
        placement: (!z.placement.is_default()).then(|| z.placement.clone()),
    }
}

#[test]
fn edit_zone_sets_the_outline_smoothing_and_a_missing_one_leaves_it() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();

    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| {
        z.smoothing = eda_model::ir::ZoneSmoothing::Fillet;
        z.corner_radius = mm(1.5);
    }))
    .unwrap();
    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!((z.smoothing, z.corner_radius), (eda_model::ir::ZoneSmoothing::Fillet, mm(1.5)));

    // a client that does not know about smoothing (the field absent) leaves it as it is
    let mut cmd = edit_zone_cmd(id.clone(), "GND", |z| z.clearance = mm(0.3));
    if let Cmd::EditZone { smoothing, corner_radius, .. } = &mut cmd {
        *smoothing = None;
        *corner_radius = None;
    }
    b.apply(&cmd).unwrap();
    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!((z.clearance, z.smoothing, z.corner_radius), (mm(0.3), eda_model::ir::ZoneSmoothing::Fillet, mm(1.5)));

    let e = b.apply(&edit_zone_cmd(id, "GND", |z| z.corner_radius = Nm::new(-1))).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");
}

/// `ZONE_SETTINGS::m_Layers` (the dialog's layer list): a zone goes on several layers, keeps its id and its first layer, comes back to one, and an edit that does not
/// name the set leaves a set that holds the zone's layer as it is.
#[test]
fn edit_zone_puts_a_zone_on_several_layers_and_back() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    let zone = |b: &Board<'_>| b.design().routing.as_ref().unwrap().zones[0].clone();

    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.layers = vec!["B.Cu".into(), "F.Cu".into()])).unwrap();
    let z = zone(&b);
    assert_eq!((z.id.as_str(), z.layer.as_str(), z.layer_set()), (id.as_str(), "F.Cu", vec!["F.Cu", "B.Cu"]), "the set is kept in the order of the stack, the front first");

    // An edit that does not send a set (an older client, or Zone Manager's priority write) leaves the set while the zone's layer is in it.
    let mut cmd = edit_zone_cmd(id.clone(), "GND", |z| z.priority = 4);
    if let Cmd::EditZone { layers, .. } = &mut cmd {
        *layers = None;
    }
    b.apply(&cmd).unwrap();
    assert_eq!((zone(&b).priority, zone(&b).layer_set()), (4, vec!["F.Cu", "B.Cu"]));

    // ... and moves a zone that is not on that layer to it alone.
    let mut cmd = edit_zone_cmd(id.clone(), "GND", |z| z.layer = "B.Cu".into());
    if let Cmd::EditZone { layers, .. } = &mut cmd {
        *layers = None;
    }
    b.apply(&cmd).unwrap();
    assert_eq!(zone(&b).layer_set(), vec!["F.Cu", "B.Cu"], "B.Cu is in the set: nothing moves");
    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.layers = vec!["B.Cu".into()])).unwrap();
    let z = zone(&b);
    assert_eq!((z.layer.as_str(), z.layers.len()), ("B.Cu", 0), "a set of one layer is the plain layer");

    // The layers have to exist, and a pour is on copper or on technical layers, not both.
    assert_eq!(b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.layers = vec!["F.Cu".into(), "In7.Cu".into()])).unwrap_err()[0].check, "ops_unknown_layer");
    assert_eq!(b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.layers = vec!["F.Cu".into(), "F.SilkS".into()])).unwrap_err()[0].check, "ops_bad_zone");
    // A rule area may span both (its layers are where it keeps things out).
    b.apply(&edit_zone_cmd(id, "", |z| {
        z.is_rule_area = true;
        z.keepout_copper_pour = true;
        z.layers = vec!["F.Cu".into(), "F.SilkS".into(), "B.Cu".into()];
    }))
    .unwrap();
    assert_eq!(zone(&b).layer_set(), vec!["F.Cu", "B.Cu", "F.SilkS"]);
}

/// A zone on no copper layer (`fillNonCopperZone`'s): drawn on a technical layer that is on, with no net, whatever net the dialog sent.
#[test]
fn a_zone_can_be_on_a_technical_layer_and_has_no_net_there() {
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }];
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.SilkS".into(), outline: outline.clone() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    {
        let z = &b.design().routing.as_ref().unwrap().zones[0];
        assert_eq!((z.layer.as_str(), z.net.as_str(), z.is_on_copper_layer()), ("F.SilkS", "", false), "no net on a non-copper layer");
    }
    // A technical layer the board has turned off, and a name that is no layer, take no zone.
    assert_eq!(b.apply(&Cmd::AddZone { net: "".into(), layer: "User.1".into(), outline: outline.clone() }).unwrap_err()[0].check, "ops_layer_off");
    assert_eq!(b.apply(&Cmd::AddZone { net: "".into(), layer: "Nope".into(), outline: outline.clone() }).unwrap_err()[0].check, "ops_unknown_layer");

    // It may have no minimum width (the mask zones of `issue22826` have none) and the spokes of a copper pour are no business of its; a pour needs both.
    b.apply(&edit_zone_cmd(id.clone(), "", |z| {
        z.layer = "F.SilkS".into();
        z.min_thickness = Nm::ZERO;
        z.thermal_spoke_width = Nm::ZERO;
    }))
    .unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().zones[0].min_thickness, Nm::ZERO);
    assert_eq!(b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.min_thickness = Nm::ZERO)).unwrap_err()[0].check, "ops_bad_zone", "a copper pour needs a minimum width");

    // A dialog that sends a net for it still gets none; the same zone moved to copper takes the net.
    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.layers = vec!["F.SilkS".into(), "B.SilkS".into()])).unwrap();
    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!((z.layer_set(), z.net.as_str()), (vec!["F.SilkS", "B.SilkS"], ""));
    b.apply(&edit_zone_cmd(id, "GND", |z| z.layer = "F.Cu".into())).unwrap();
    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!((z.layer_set(), z.net.as_str()), (vec!["F.Cu"], "GND"));
}

#[test]
fn edit_zone_replaces_every_setting_but_leaves_the_outline_alone() {
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }];
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: outline.clone() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();

    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| {
        z.priority = 3;
        z.clearance = mm(0.3);
        z.pad_connection = PadConnection::Full;
        z.fill_mode = FillMode::HatchPattern;
        z.hatch_thickness = mm(1.0);
        z.hatch_gap = mm(1.5);
    }))
    .unwrap();

    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!(z.id, id, "id is stable across an edit");
    assert_eq!(z.outline, outline, "EditZone never touches the outline");
    assert_eq!(z.priority, 3);
    assert_eq!(z.clearance, mm(0.3));
    assert_eq!(z.pad_connection, PadConnection::Full);
    assert_eq!(z.fill_mode, FillMode::HatchPattern);
}

#[test]
fn edit_zone_rejects_a_thermal_spoke_narrower_than_the_minimum_width() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();

    let e = b
        .apply(&edit_zone_cmd(id, "GND", |z| {
            z.min_thickness = mm(0.5);
            z.thermal_spoke_width = mm(0.1);
        }))
        .unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");
}

#[test]
fn edit_zone_rejects_an_unknown_net_or_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();

    let e = b.apply(&edit_zone_cmd(id.clone(), "NOPE", |_| {})).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_net");

    let e = b.apply(&edit_zone_cmd("zone_nope".into(), "GND", |_| {})).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_zone");
}

#[test]
fn set_zone_outline_replaces_the_outline_only() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.priority = 5)).unwrap();

    let new_outline = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(20.0), y: Nm::ZERO }, Point { x: mm(20.0), y: mm(20.0) }, Point { x: Nm::ZERO, y: mm(20.0) }];
    b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: new_outline.clone(), holes: None }).unwrap();
    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!(z.id, id, "id is stable across an outline edit");
    assert_eq!(z.outline, new_outline);
    assert_eq!(z.priority, 5, "SetZoneOutline must not touch any other setting");

    let e = b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1), y: Nm::new(1) }], holes: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");
    assert_eq!(b.apply(&Cmd::SetZoneOutline { id: "zone_nope".into(), outline: new_outline, holes: None }).unwrap_err()[0].check, "ops_unknown_zone");
}

/// A mirror sends the zone's holes with its outline (`ZONE::Mirror`: the whole polygon set); a corner edit sends none and keeps them.
#[test]
fn set_zone_outline_with_holes_replaces_them_and_without_keeps_them() {
    let m = net_model();
    let mut b = board(&m);
    let sq = |x0: Nm, y0: Nm, x1: Nm, y1: Nm| vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }];
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: sq(Nm::ZERO, Nm::ZERO, mm(10.0), mm(10.0)) }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    let hole = sq(mm(2.0), mm(2.0), mm(4.0), mm(4.0));
    b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: sq(Nm::ZERO, Nm::ZERO, mm(10.0), mm(10.0)), holes: Some(vec![hole.clone()]) }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().zones[0].holes, vec![hole.clone()]);
    b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: sq(Nm::ZERO, Nm::ZERO, mm(12.0), mm(10.0)), holes: None }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().zones[0].holes, vec![hole], "a corner edit leaves the holes");
    let mirrored = sq(mm(-4.0), mm(2.0), mm(-2.0), mm(4.0));
    b.apply(&Cmd::SetZoneOutline { id, outline: sq(mm(-12.0), Nm::ZERO, Nm::ZERO, mm(10.0)), holes: Some(vec![mirrored.clone()]) }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().zones[0].holes, vec![mirrored]);
}

/// Moving a footprint with Edge.Cuts of its own moves the board's outline: the summary (`placement.outline`) follows (`BuildBoardPolygonOutlines`' footprint pass).
#[test]
fn moving_a_footprint_with_edge_cuts_refreshes_the_summary_outline() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
    (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
    (net 0 "")
    (gr_line (start 15 0) (end 0 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e1"))
    (gr_line (start 0 0) (end 0 30) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e2"))
    (gr_line (start 0 30) (end 40 30) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e3"))
    (gr_line (start 40 30) (end 40 0) (stroke (width 0.06) (type default)) (layer "Edge.Cuts") (uuid "e4"))
    (gr_line (start 40 0) (end 25 0) (stroke (width 0.05) (type default)) (layer "Edge.Cuts") (uuid "e5"))
    (footprint "L:Tab" (layer "F.Cu") (uuid "f1") (at 20 0)
        (property "Reference" "J1" (at 0 2 0) (layer "F.SilkS") (uuid "p1"))
        (fp_line (start -5 0) (end -5 -5) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts") (uuid "t1"))
        (fp_line (start -5 -5) (end 5 -5) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts") (uuid "t2"))
        (fp_line (start 5 -5) (end 5 0) (stroke (width 0.05) (type solid)) (layer "Edge.Cuts") (uuid "t3"))
        (pad "1" smd rect (at 0 -3) (size 1 2) (layers "F.Cu" "F.Mask") (uuid "a1")))
)"#;
    let (design, model, _) = eda_kicad::import_kicad_pcb(text).unwrap();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    let min_y = |b: &Board| b.design().placement.as_ref().unwrap().outline.iter().map(|p| p.y).min().unwrap();
    assert_eq!(min_y(&b), mm(-5.0));
    b.apply(&Cmd::MoveItems { ids: vec!["J1".into()], dx: Nm::ZERO, dy: mm(-2.0) }).unwrap();
    // The tab no longer meets the board's lines: KiCad's outline is open, the summary its inferred box; either way the summary changed with the footprint.
    assert_ne!(min_y(&b), mm(-5.0), "the summary followed J1: {:?}", b.design().placement.as_ref().unwrap().outline);
    b.apply(&Cmd::MoveItems { ids: vec!["J1".into()], dx: Nm::ZERO, dy: mm(2.0) }).unwrap();
    assert_eq!(min_y(&b), mm(-5.0));
}

// ----------------------------------------------------------- teardrops

fn via_and_track_board() -> (ConstraintModel, Point, Point) {
    let m = net_model();
    let via_at = Point { x: mm(40.0), y: mm(40.0) };
    let far = Point { x: via_at.x + mm(5.0), y: via_at.y };
    (m, via_at, far)
}

#[test]
fn add_all_teardrops_is_a_no_op_when_disabled() {
    let (m, via_at, far) = via_and_track_board();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: via_at.x, y: via_at.y, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, far] }).unwrap();

    b.apply(&Cmd::AddAllTeardrops).unwrap();
    assert!(b.design().routing.as_ref().unwrap().zones.is_empty(), "teardrop_settings.enabled defaults false");
}

#[test]
fn add_all_teardrops_generates_once_and_is_idempotent() {
    let (m, via_at, far) = via_and_track_board();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: via_at.x, y: via_at.y, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, far] }).unwrap();
    b.apply(&Cmd::SetTeardropSettings { settings: TeardropSettings { enabled: true, ..Default::default() } }).unwrap();

    b.apply(&Cmd::AddAllTeardrops).unwrap();
    let zones = b.design().routing.as_ref().unwrap().zones.clone();
    assert_eq!(zones.len(), 1, "{zones:?}");
    assert!(zones[0].teardrop);
    let first_id = zones[0].id.clone();

    // Running it again must replace, not duplicate, the generated set.
    b.apply(&Cmd::AddAllTeardrops).unwrap();
    let zones2 = b.design().routing.as_ref().unwrap().zones.clone();
    assert_eq!(zones2.len(), 1, "a second run must not accumulate teardrops: {zones2:?}");
    assert_eq!(zones2[0].id, first_id, "regenerating an identical shape must land on the same deterministic id");
}

#[test]
fn add_all_teardrops_never_touches_a_user_drawn_zone() {
    let (m, via_at, far) = via_and_track_board();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: via_at.x, y: via_at.y, drill: mm(0.3), diameter: mm(0.8), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![via_at, far] }).unwrap();
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    let user_zone_id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    b.apply(&Cmd::SetTeardropSettings { settings: TeardropSettings { enabled: true, ..Default::default() } }).unwrap();

    b.apply(&Cmd::AddAllTeardrops).unwrap();
    let zones = b.design().routing.as_ref().unwrap().zones.clone();
    assert_eq!(zones.len(), 2, "{zones:?}");
    assert!(zones.iter().any(|z| z.id == user_zone_id && !z.teardrop), "the hand-drawn zone must survive untouched");

    b.apply(&Cmd::RemoveAllTeardrops).unwrap();
    let zones = b.design().routing.as_ref().unwrap().zones.clone();
    assert_eq!(zones.len(), 1, "{zones:?}");
    assert_eq!(zones[0].id, user_zone_id, "only the generated teardrop should be gone");
}

#[test]
fn set_teardrop_settings_rejects_a_ratio_outside_zero_to_one() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::SetTeardropSettings { settings: TeardropSettings { best_width_ratio: 1.5, ..Default::default() } }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_teardrop_settings");
    assert_eq!(b.design().routing.as_ref().map(|r| r.teardrop_settings), None, "a refused settings change must not touch the board at all (routing section not even created)");
}

// -------------------------------------------------------------- shapes

#[test]
fn add_shape_move_and_delete() {
    let m = net_model();
    let mut b = board(&m);
    let shape = Shape::Segment { id: "ignored-on-input".into(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.0), y: Nm::ZERO } };
    b.apply(&Cmd::AddShape { shape }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();
    assert_ne!(id, "ignored-on-input", "a caller-supplied id must never be trusted");
    assert!(!id.is_empty());

    b.apply(&Cmd::MoveShape { id: id.clone(), dx: mm(0.5), dy: mm(-0.5) }).unwrap();
    let moved = &b.design().drawings.as_ref().unwrap().shapes[0];
    assert_eq!(moved.points(), vec![Point { x: mm(0.5), y: mm(-0.5) }, Point { x: mm(1.5), y: mm(-0.5) }]);

    b.apply(&Cmd::DeleteShape { id: id.clone() }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().shapes.is_empty());
    assert_eq!(b.apply(&Cmd::DeleteShape { id }).unwrap_err()[0].check, "ops_unknown_shape");
}

#[test]
fn the_outline_summary_follows_the_edge_cuts_shapes_when_they_are_the_outline() {
    let m = net_model();
    let b = board(&m);
    let edge = |a: Point, c: Point| Shape::Segment { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, start: a, end: c };
    let p = |x, y| Point { x, y };
    // An imported board whose outline is its shapes: a 10 x 10 mm square, `placement.outline` the ring that stands for it.
    let mut d = b.design().clone();
    d.drawings = Some(eda_model::ir::DrawingsSection {
        shapes: vec![edge(p(Nm::ZERO, Nm::ZERO), p(mm(10.0), Nm::ZERO)), edge(p(mm(10.0), Nm::ZERO), p(mm(10.0), mm(10.0))), edge(p(mm(10.0), mm(10.0)), p(Nm::ZERO, mm(10.0))), edge(p(Nm::ZERO, mm(10.0)), p(Nm::ZERO, Nm::ZERO))],
        outline_is_shapes: true,
        ..Default::default()
    });
    d.drawings.as_mut().unwrap().assign_missing_ids();
    eda_drc::outline::refresh_outline_summary(&mut d, None);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let area = |b: &Board| {
        let o = &b.design().placement.as_ref().unwrap().outline;
        (0..o.len()).map(|i| o[i].x.area(o[(i + 1) % o.len()].y) - o[(i + 1) % o.len()].x.area(o[i].y)).sum::<i128>().abs() / 2
    };
    assert_eq!(area(&b), 100_000_000_000_000);

    // A circular cutout drawn on Edge.Cuts is a hole: the summary ring is the outer ring still, but the shape is in the outline.
    let hole = Shape::Circle { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, center: p(mm(5.0), mm(5.0)), end: p(mm(7.0), mm(5.0)) };
    b.apply(&Cmd::AddShape { shape: hole }).unwrap();
    assert_eq!(area(&b), 100_000_000_000_000, "a cutout does not change the outer ring");
    let outline = eda_drc::outline::board_outline(b.design(), false);
    assert_eq!((outline.polys.outline_count(), outline.polys.hole_count(0)), (1, 1));

    // Taking the right side 2 mm in changes the outline, and the summary follows (the cutout stays well inside).
    let right = b.design().drawings.as_ref().unwrap().shapes.iter().find(|s| matches!(s, Shape::Segment { start, end, .. } if start.x == mm(10.0) && end.x == mm(10.0))).unwrap().id().to_string();
    let top = b.design().drawings.as_ref().unwrap().shapes.iter().find(|s| matches!(s, Shape::Segment { start, end, .. } if start.y == mm(10.0) && end.y == mm(10.0))).unwrap().id().to_string();
    let bottom = b.design().drawings.as_ref().unwrap().shapes.iter().find(|s| matches!(s, Shape::Segment { start, end, .. } if start.y == Nm::ZERO && end.y == Nm::ZERO)).unwrap().id().to_string();
    for id in [&right, &top, &bottom] {
        b.apply(&Cmd::DeleteShape { id: id.clone() }).unwrap();
    }
    b.apply(&Cmd::AddShape { shape: edge(p(Nm::ZERO, Nm::ZERO), p(mm(8.0), Nm::ZERO)) }).unwrap();
    b.apply(&Cmd::AddShape { shape: edge(p(mm(8.0), Nm::ZERO), p(mm(8.0), mm(10.0))) }).unwrap();
    b.apply(&Cmd::AddShape { shape: edge(p(mm(8.0), mm(10.0)), p(Nm::ZERO, mm(10.0))) }).unwrap();
    assert_eq!(area(&b), 80_000_000_000_000, "the summary is the new 8 x 10 mm outline");
    // A cutout that crosses the outline is malformed, as in KiCad; the summary is then the rectangle round the edges.
    assert_eq!(eda_drc::outline::check_board_outline(b.design()).len(), 0);
    b.apply(&Cmd::AddShape { shape: Shape::Circle { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, center: p(mm(8.0), mm(5.0)), end: p(mm(9.5), mm(5.0)) } }).unwrap();
    assert!(!eda_drc::outline::check_board_outline(b.design()).is_empty());
}

#[test]
fn add_shape_accepts_a_bezier_and_moving_it_shifts_all_four_control_points() {
    // `pcbnew.InteractiveDrawing.bezier` commits one `PCB_SHAPE` of type BEZIER: start, C1, C2, end.
    let m = net_model();
    let mut b = board(&m);
    let curve = Shape::Bezier { id: "ignored".into(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, c1: Point { x: Nm::ZERO, y: mm(2.0) }, c2: Point { x: mm(3.0), y: mm(2.0) }, end: Point { x: mm(3.0), y: Nm::ZERO } };
    b.apply(&Cmd::AddShape { shape: curve }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();
    assert!(id.starts_with("shp_"), "{id}");
    assert_ne!(id, "ignored");

    b.apply(&Cmd::MoveShape { id: id.clone(), dx: mm(0.1), dy: mm(0.2) }).unwrap();
    let s = &b.design().drawings.as_ref().unwrap().shapes[0];
    assert_eq!(s.points(), vec![Point { x: mm(0.1), y: mm(0.2) }, Point { x: mm(0.1), y: mm(2.2) }, Point { x: mm(3.1), y: mm(2.2) }, Point { x: mm(3.1), y: mm(0.2) }]);
    // the flattened polyline (what DRC/plot use) starts and ends on the curve's own ends
    let poly = s.bezier_points().unwrap();
    assert_eq!(poly.first(), Some(&Point { x: mm(0.1), y: mm(0.2) }));
    assert_eq!(poly.last(), Some(&Point { x: mm(3.1), y: mm(0.2) }));
    assert!(poly.len() > 3);

    b.apply(&Cmd::EditShape { id, layer: "F.Fab".into(), stroke_width: mm(0.2), filled: false }).unwrap();
    let s = &b.design().drawings.as_ref().unwrap().shapes[0];
    assert_eq!((s.layer(), s.stroke_width()), ("F.Fab", mm(0.2)));
}

#[test]
fn a_bezier_shape_round_trips_through_json_with_its_tagged_kind() {
    let curve = Shape::Bezier { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.12), filled: false, start: Point { x: um(1.0), y: um(2.0) }, c1: Point { x: um(3.0), y: um(4.0) }, c2: Point { x: um(5.0), y: um(6.0) }, end: Point { x: um(7.0), y: um(8.0) } };
    let json = serde_json::to_value(&curve).unwrap();
    assert_eq!(json["kind"], "bezier");
    assert_eq!(json["c2"]["x"], 5000);
    let back: Shape = serde_json::from_value(json).unwrap();
    assert_eq!(back, curve);
}

#[test]
fn add_shape_rejects_a_short_polygon() {
    let m = net_model();
    let mut b = board(&m);
    let shape = Shape::Polygon { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: true, pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] };
    let e = b.apply(&Cmd::AddShape { shape }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");
}

#[test]
fn edit_shape_changes_layer_width_and_filled_but_not_geometry() {
    let m = net_model();
    let mut b = board(&m);
    let shape = Shape::Rect { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.0), y: mm(1.0) } };
    b.apply(&Cmd::AddShape { shape }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();

    b.apply(&Cmd::EditShape { id: id.clone(), layer: "F.Fab".into(), stroke_width: mm(0.3), filled: true }).unwrap();
    let s = &b.design().drawings.as_ref().unwrap().shapes[0];
    assert_eq!(s.layer(), "F.Fab");
    assert_eq!(s.points(), vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: mm(1.0) }], "EditShape never touches geometry");
    let Shape::Rect { stroke_width, filled, .. } = s else { panic!("still a rect") };
    assert_eq!((*stroke_width, *filled), (mm(0.3), true));

    let e = b.apply(&Cmd::EditShape { id: id.clone(), layer: "".into(), stroke_width: mm(0.3), filled: true }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");
    let e = b.apply(&Cmd::EditShape { id, layer: "F.Fab".into(), stroke_width: Nm::ZERO, filled: true }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");

    assert_eq!(b.apply(&Cmd::EditShape { id: "shape_nope".into(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false }).unwrap_err()[0].check, "ops_unknown_shape");
}

// --------------------------------------------------------------- text

#[test]
fn add_edit_move_delete_text() {
    let m = net_model();
    let mut b = board(&m);
    let text = Text { id: String::new(), content: "REV A".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false };
    b.apply(&Cmd::AddText { text }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().texts[0].id.clone();
    assert!(!id.is_empty());

    b.apply(&Cmd::EditText { id: id.clone(), content: "REV B".into(), angle: 90_000, layer: "F.Fab".into(), size_nm: mm(1.2), stroke_width: mm(0.2), justify: TextJustify::Left, mirror: true }).unwrap();
    let t = &b.design().drawings.as_ref().unwrap().texts[0];
    assert_eq!(t.content, "REV B");
    assert_eq!(t.layer, "F.Fab");
    assert_eq!(t.justify, TextJustify::Left);
    assert!(t.mirror);
    assert_eq!(t.id, id, "editing style must not move the id");

    b.apply(&Cmd::MoveText { id: id.clone(), x: mm(4.0), y: mm(4.0) }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().texts[0].at, Point { x: mm(4.0), y: mm(4.0) });

    b.apply(&Cmd::DeleteText { id: id.clone() }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().texts.is_empty());
    assert_eq!(b.apply(&Cmd::EditText { id, content: String::new(), angle: 0, layer: "F.Fab".into(), size_nm: mm(0.1), stroke_width: mm(0.01), justify: TextJustify::Center, mirror: false }).unwrap_err()[0].check, "ops_unknown_text");
}

/// What an imported board text kept beyond `Text`'s fields (`DrawingsSection::text_extras`) follows the text's verbs: it stays through an edit that leaves the size and the
/// stroke width alone, a new size is a square one again, a stroke width given is not the automatic one, and deleting the text takes its extras with it (the next text
/// with the same content at the same place gets the same id, and must not come back bold).
#[test]
fn a_texts_imported_looks_follow_its_edits_and_leave_with_it() {
    use eda_model::board_text::TextExtra;
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddText { text: Text { id: String::new(), content: "REV A".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.125), justify: TextJustify::Center, mirror: false } }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().texts[0].id.clone();
    let mut d = b.design().clone();
    d.drawings.as_mut().unwrap().text_extras = vec![TextExtra { id: id.clone(), width_nm: Some(mm(0.8)), auto_thickness: true, bold: true, ..Default::default() }];
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let extra = |b: &Board| b.design().drawings.as_ref().unwrap().text_extra(&id).cloned();

    // The same size and stroke width: moved, turned, restyled otherwise, and the looks stay.
    b.apply(&Cmd::EditText { id: id.clone(), content: "REV B".into(), angle: 90_000, layer: "F.Fab".into(), size_nm: mm(1.0), stroke_width: mm(0.125), justify: TextJustify::Left, mirror: true }).unwrap();
    assert_eq!(extra(&b).map(|x| (x.width_nm, x.auto_thickness, x.bold)), Some((Some(mm(0.8)), true, true)));
    // A new size is square again.
    b.apply(&Cmd::EditText { id: id.clone(), content: "REV B".into(), angle: 90_000, layer: "F.Fab".into(), size_nm: mm(1.5), stroke_width: mm(0.125), justify: TextJustify::Left, mirror: true }).unwrap();
    assert_eq!(extra(&b).map(|x| (x.width_nm, x.auto_thickness, x.bold)), Some((None, true, true)));
    // A stroke width given is not the automatic one.
    b.apply(&Cmd::EditText { id: id.clone(), content: "REV B".into(), angle: 90_000, layer: "F.Fab".into(), size_nm: mm(1.5), stroke_width: mm(0.2), justify: TextJustify::Left, mirror: true }).unwrap();
    assert_eq!(extra(&b).map(|x| (x.width_nm, x.auto_thickness, x.bold)), Some((None, false, true)));

    b.apply(&Cmd::DeleteText { id: id.clone() }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().text_extras.is_empty(), "the looks leave with the text");
}

/// Set Text and Graphics to Layer Defaults, and a size or a stroke width in Edit Text and Graphics Properties, replace the glyph width and the automatic stroke width an
/// imported text kept; a text that is not in the selection keeps both.
#[test]
fn the_global_text_edit_takes_the_imported_glyph_width_and_automatic_stroke_with_it() {
    use eda_model::board_text::TextExtra;
    let m = net_model();
    let mut b = board(&m);
    for content in ["one", "two", "three"] {
        b.apply(&Cmd::AddText { text: Text { id: String::new(), content: content.into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.125), justify: TextJustify::Center, mirror: false } }).unwrap();
    }
    let ids: Vec<String> = b.design().drawings.as_ref().unwrap().texts.iter().map(|t| t.id.clone()).collect();
    let mut d = b.design().clone();
    d.drawings.as_mut().unwrap().text_extras = ids.iter().map(|id| TextExtra { id: id.clone(), width_nm: Some(mm(0.8)), auto_thickness: true, italic: true, ..Default::default() }).collect();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let edit = |ids: &[String], layer_defaults: bool, text_size: Option<Nm>, text_thickness: Option<Nm>| Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: ids.to_vec(), dimension_ids: vec![], layer_defaults, layer: None, line_width: None, text_size, text_thickness };
    let looks = |b: &Board, id: &str| b.design().drawings.as_ref().unwrap().text_extra(id).map(|x| (x.width_nm, x.auto_thickness, x.italic));

    b.apply(&edit(&ids[..1], false, Some(mm(1.5)), None)).unwrap();
    assert_eq!(looks(&b, &ids[0]), Some((None, true, true)), "a size: square again, the stroke width still automatic");
    b.apply(&edit(&ids[1..2], false, None, Some(mm(0.2)))).unwrap();
    assert_eq!(looks(&b, &ids[1]), Some((Some(mm(0.8)), false, true)), "a stroke width: not automatic, the glyph width stays");
    b.apply(&edit(&ids[2..], true, None, None)).unwrap();
    assert_eq!(looks(&b, &ids[2]), Some((None, false, true)), "layer defaults: both, and what is not size or stroke stays");
    assert_eq!(looks(&b, &ids[1]), Some((Some(mm(0.8)), false, true)), "a text outside the selection is left alone");
}

// ------------------------------------- global edit: text & graphics

#[test]
fn edit_text_and_graphics_touches_only_the_fields_given() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.1), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.0), y: Nm::ZERO } } }).unwrap();
    b.apply(&Cmd::AddText { text: Text { id: String::new(), content: "REF".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false } }).unwrap();
    let shape_id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();
    let text_id = b.design().drawings.as_ref().unwrap().texts[0].id.clone();

    // Only a line-width change for the shape -- its layer must be untouched.
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![shape_id.clone()], text_ids: vec![], dimension_ids: vec![], layer_defaults: false, layer: None, line_width: Some(mm(0.25)), text_size: None, text_thickness: None }).unwrap();
    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.shapes[0].layer(), "F.SilkS");
    assert_eq!(dr.shapes[0].id(), shape_id, "a style edit must not move the id");
    match &dr.shapes[0] {
        Shape::Segment { stroke_width, .. } => assert_eq!(*stroke_width, mm(0.25)),
        other => panic!("expected a Segment, got {other:?}"),
    }

    // Layer + text size/thickness for the text, same command touching both kinds at once.
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![shape_id.clone()], text_ids: vec![text_id.clone()], dimension_ids: vec![], layer_defaults: false, layer: Some("F.Fab".into()), line_width: None, text_size: Some(mm(1.2)), text_thickness: Some(mm(0.2)) })
        .unwrap();
    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.shapes[0].layer(), "F.Fab", "the shared `layer` field must still reach the shape on a mixed call");
    let t = dr.texts.iter().find(|t| t.id == text_id).unwrap();
    assert_eq!(t.layer, "F.Fab");
    assert_eq!(t.size_nm, mm(1.2));
    assert_eq!(t.stroke_width, mm(0.2));
    assert_eq!(t.content, "REF", "content is not one of this command's fields");
}

#[test]
fn edit_text_and_graphics_refuses_empty_input_and_bad_values() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], dimension_ids: vec![], layer_defaults: false, layer: None, line_width: None, text_size: None, text_thickness: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_global_edit");

    let e = b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec!["x".into()], text_ids: vec![], dimension_ids: vec![], layer_defaults: false, layer: None, line_width: Some(Nm::ZERO), text_size: None, text_thickness: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");

    let e = b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec!["x".into()], dimension_ids: vec![], layer_defaults: false, layer: None, line_width: None, text_size: Some(um(-5.0)), text_thickness: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_text");
}

// -------------------------------------------------------------- groups

#[test]
fn group_needs_at_least_two_items() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::Group { ids: vec!["U1".into()] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_group");
}

#[test]
fn group_then_ungroup_round_trips() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "C1".into()] }).unwrap();
    let groups = b.design().drawings.as_ref().unwrap().groups.clone();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert!(!groups[0].id.is_empty());
    let mut members = groups[0].member_ids.clone();
    members.sort();
    assert_eq!(members, vec!["C1".to_string(), "U1".to_string()]);

    b.apply(&Cmd::Ungroup { ids: vec![groups[0].id.clone()] }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().groups.is_empty());
}

#[test]
fn grouping_an_existing_group_nests_it() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402"), part("C2", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "C1".into()] }).unwrap();
    let inner_id = b.design().drawings.as_ref().unwrap().groups[0].id.clone();

    b.apply(&Cmd::Group { ids: vec![inner_id.clone(), "C2".into()] }).unwrap();
    let groups = b.design().drawings.as_ref().unwrap().groups.clone();
    assert_eq!(groups.len(), 2, "the inner group stays, inside the new one: {groups:?}");
    let inner = groups.iter().find(|g| g.id == inner_id).expect("the inner group keeps its id");
    let mut inner_members = inner.member_ids.clone();
    inner_members.sort();
    assert_eq!(inner_members, vec!["C1".to_string(), "U1".to_string()]);
    let outer = groups.iter().find(|g| g.id != inner_id).unwrap();
    let mut outer_members = outer.member_ids.clone();
    outer_members.sort();
    let mut want = vec![inner_id, "C2".to_string()];
    want.sort();
    assert_eq!(outer_members, want);
}

#[test]
fn an_item_can_only_belong_to_one_group_at_a_time() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402"), part("C2", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "C1".into()] }).unwrap();
    let first_group_id = b.design().drawings.as_ref().unwrap().groups[0].id.clone();

    // Pulling C1 into a brand new group with C2 removes it from the first group, which keeps U1 alone (PCB_GROUP_TOOL::Group dissolves nothing).
    b.apply(&Cmd::Group { ids: vec!["C1".into(), "C2".into()] }).unwrap();
    let all = b.design().drawings.as_ref().unwrap().groups.clone();
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(all.iter().find(|g| g.id == first_group_id).unwrap().member_ids, vec!["U1".to_string()]);
    let groups: Vec<_> = all.into_iter().filter(|g| g.id != first_group_id).collect();
    let mut members = groups[0].member_ids.clone();
    members.sort();
    assert_eq!(members, vec!["C1".to_string(), "C2".to_string()]);
}

#[test]
fn add_to_group_and_remove_from_group_round_trip() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402"), part("C2", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "C1".into()] }).unwrap();
    let group_id = b.design().drawings.as_ref().unwrap().groups[0].id.clone();

    b.apply(&Cmd::AddToGroup { group_id: group_id.clone(), ids: vec!["C2".into()] }).unwrap();
    let mut members = b.design().drawings.as_ref().unwrap().groups[0].member_ids.clone();
    members.sort();
    assert_eq!(members, vec!["C1".to_string(), "C2".to_string(), "U1".to_string()]);

    let e = b.apply(&Cmd::AddToGroup { group_id: "grp_nope".into(), ids: vec!["U1".into()] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_group");

    // Removing two of the three members leaves only one -- the group must dissolve.
    b.apply(&Cmd::RemoveFromGroup { ids: vec!["C1".into(), "C2".into()] }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().groups.is_empty(), "{:?}", b.design().drawings.as_ref().unwrap().groups);
}

#[test]
fn edit_group_renames_it_and_replaces_its_members() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402"), part("C2", "0402"), part("C3", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "C1".into()] }).unwrap();
    b.apply(&Cmd::Group { ids: vec!["C2".into(), "C3".into()] }).unwrap();
    let first = b.design().drawings.as_ref().unwrap().groups.iter().find(|g| g.member_ids.contains(&"U1".to_string())).unwrap().id.clone();

    // rename it and take C2 from the other group: that group keeps C3 alone (Group Properties dissolves nothing)
    b.apply(&Cmd::EditGroup { id: first.clone(), name: "power".into(), member_ids: vec!["U1".into(), "C1".into(), "C2".into(), "C2".into()], design_block: None }).unwrap();
    let all = b.design().drawings.as_ref().unwrap().groups.clone();
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all.iter().any(|g| g.member_ids == vec!["C3".to_string()]));
    let groups: Vec<_> = all.into_iter().filter(|g| g.id == first).collect();
    assert_eq!(groups[0].name, "power");
    assert_eq!(groups[0].id, first, "the group keeps its id");
    let mut members = groups[0].member_ids.clone();
    members.sort();
    assert_eq!(members, vec!["C1".to_string(), "C2".to_string(), "U1".to_string()], "a member listed twice is one member");

    // a group given one member keeps it; given none it goes
    b.apply(&Cmd::EditGroup { id: first.clone(), name: "x".into(), member_ids: vec!["U1".into()], design_block: None }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().groups.iter().any(|g| g.id == first && g.member_ids == vec!["U1".to_string()]));
    b.apply(&Cmd::EditGroup { id: first.clone(), name: "x".into(), member_ids: vec![], design_block: None }).unwrap();
    assert!(!b.design().drawings.as_ref().unwrap().groups.iter().any(|g| g.id == first));
}

#[test]
fn edit_group_refuses_an_unknown_group_and_a_group_that_would_hold_itself() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402"), part("C2", "0402"), part("C3", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "C1".into()] }).unwrap();
    b.apply(&Cmd::Group { ids: vec!["C2".into(), "C3".into()] }).unwrap();
    let groups = b.design().drawings.as_ref().unwrap().groups.clone();
    let e = b.apply(&Cmd::EditGroup { id: "grp_nope".into(), name: String::new(), member_ids: vec!["U1".into(), "C1".into()], design_block: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_group");
    let e = b.apply(&Cmd::EditGroup { id: groups[0].id.clone(), name: String::new(), member_ids: vec![groups[0].id.clone(), "U1".into()], design_block: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_group", "a group cannot hold itself");
    assert_eq!(b.design().drawings.as_ref().unwrap().groups.len(), 2, "a refused edit changes nothing");

    // another group is a fine member; its holder can then not be put into it
    b.apply(&Cmd::EditGroup { id: groups[0].id.clone(), name: String::new(), member_ids: vec![groups[1].id.clone(), "U1".into(), "C1".into()], design_block: None }).unwrap();
    let e = b.apply(&Cmd::EditGroup { id: groups[1].id.clone(), name: String::new(), member_ids: vec![groups[0].id.clone(), "C2".into()], design_block: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_group", "a group cannot hold a group that holds it");
}

#[test]
fn removing_from_a_group_that_stays_above_two_members_keeps_it_alive() {
    let m = model(vec![part("U1", "SOIC-8"), part("C1", "0402"), part("C2", "0402")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "C1".into(), "C2".into()] }).unwrap();
    b.apply(&Cmd::RemoveFromGroup { ids: vec!["C1".into()] }).unwrap();
    let groups = b.design().drawings.as_ref().unwrap().groups.clone();
    assert_eq!(groups.len(), 1, "{groups:?}");
    let mut members = groups[0].member_ids.clone();
    members.sort();
    assert_eq!(members, vec!["C2".to_string(), "U1".to_string()], "C1 should be gone, the other two still grouped");
}

// ---------------------------------------------------- clears_routing

#[test]
fn only_part_edits_and_flip_clear_routing() {
    let copper_and_drawing_cmds = [
        Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![] },
        Cmd::DeleteTrack { id: "x".into() },
        Cmd::SetTrackWidth { id: "x".into(), width: mm(0.2) },
        Cmd::AddVia { net: "GND".into(), x: Nm::ZERO, y: Nm::ZERO, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() },
        Cmd::DeleteVia { id: "x".into() },
        Cmd::MoveVia { id: "x".into(), x: Nm::ZERO, y: Nm::ZERO },
        Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![] },
        Cmd::DeleteZone { id: "x".into() },
        Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.1), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: Nm::ZERO, y: Nm::ZERO } } },
        Cmd::DeleteShape { id: "x".into() },
        Cmd::MoveShape { id: "x".into(), dx: Nm::ZERO, dy: Nm::ZERO },
        Cmd::AddText { text: Text { id: String::new(), content: String::new(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(0.1), stroke_width: mm(0.01), justify: TextJustify::Center, mirror: false } },
        Cmd::EditText { id: "x".into(), content: String::new(), angle: 0, layer: "F.SilkS".into(), size_nm: mm(0.1), stroke_width: mm(0.01), justify: TextJustify::Center, mirror: false },
        Cmd::DeleteText { id: "x".into() },
        Cmd::MoveText { id: "x".into(), x: Nm::ZERO, y: Nm::ZERO },
        Cmd::Duplicate { ids: vec!["x".into()] },
        Cmd::PasteItems { tracks: vec![], vias: vec![], zones: vec![], shapes: vec![], texts: vec![] },
        Cmd::EditTracksAndVias { ids: vec!["x".into()], track_width: None, via_size: None, layer: None },
        Cmd::EditTextAndGraphics { shape_ids: vec!["x".into()], text_ids: vec![], dimension_ids: vec![], layer_defaults: false, layer: None, line_width: None, text_size: None, text_thickness: None },
    ];
    for c in &copper_and_drawing_cmds {
        assert!(!c.clears_routing(), "{c:?} must not clear routing -- only part edits do");
    }

    let part_edit_cmds = [
        Cmd::Place { part: "U1".into(), anchor: "U2".into(), side: Dir::North },
        Cmd::PlaceEdge { part: "U1".into(), edge: Dir::North, fraction: 0.5 },
        Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre },
        Cmd::PlaceAt { part: "U1".into(), x: Nm::ZERO, y: Nm::ZERO },
        Cmd::MoveTo { part: "U1".into(), x: Nm::ZERO, y: Nm::ZERO },
        Cmd::Nudge { part: "U1".into(), dir: Dir::North, steps: 1 },
        Cmd::Rotate { part: "U1".into(), quarter_turns: 1 },
        Cmd::Swap { a: "U1".into(), b: "U2".into() },
        Cmd::Rip { part: "U1".into() },
        Cmd::Flip { part: "U1".into() },
        Cmd::MoveExact { parts: vec!["U1".into()], dx: Nm::ZERO, dy: Nm::ZERO, rotate_millideg: 0, pivot: None },
        Cmd::CreateArray { ids: vec!["U1".into()], geometry: ArrayGeometry::Grid { nx: 2, ny: 1, dx: mm(1.0), dy: Nm::ZERO, offset_x: Nm::ZERO, offset_y: Nm::ZERO, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false }, arrange: true, reannotate: true },
    ];
    for c in &part_edit_cmds {
        assert!(c.clears_routing(), "{c:?} must clear routing -- it can move a part out from under a track");
    }
}

// ---------------------------------------------------- duplicate / paste

#[test]
fn duplicate_copies_a_track_via_zone_shape_and_text_with_fresh_ids() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: mm(5.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    b.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.0), y: Nm::ZERO } } }).unwrap();
    b.apply(&Cmd::AddText { text: Text { id: String::new(), content: "REV A".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false } }).unwrap();

    let rt = b.design().routing.as_ref().unwrap();
    let (track_id, via_id, zone_id) = (rt.tracks[0].id.clone(), rt.vias[0].id.clone(), rt.zones[0].id.clone());
    let dr = b.design().drawings.as_ref().unwrap();
    let (shape_id, text_id) = (dr.shapes[0].id().to_string(), dr.texts[0].id.clone());

    b.apply(&Cmd::Duplicate { ids: vec![track_id.clone(), via_id.clone(), zone_id.clone(), shape_id.clone(), text_id.clone()] }).unwrap();

    let rt = b.design().routing.as_ref().unwrap();
    assert_eq!(rt.tracks.len(), 2, "the original plus one duplicate");
    assert_eq!(rt.vias.len(), 2);
    assert_eq!(rt.zones.len(), 2);
    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.shapes.len(), 2);
    assert_eq!(dr.texts.len(), 2);

    // Every duplicate landed at the SAME position as its original (KiCad's
    // own Duplicate: an exact copy, handed to the Move tool from there --
    // this app commits move separately, so the backend verb's own job is
    // just the exact copy) but got its own, different id.
    let new_track = rt.tracks.iter().find(|t| t.id != track_id).unwrap();
    assert_eq!(new_track.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }]);
    assert_ne!(new_track.id, track_id);
    assert!(!new_track.id.is_empty());

    let new_via = rt.vias.iter().find(|v| v.id != via_id).unwrap();
    assert_eq!(new_via.at, Point { x: mm(5.0), y: mm(5.0) });

    let new_text = dr.texts.iter().find(|t| t.id != text_id).unwrap();
    assert_eq!(new_text.content, "REV A");
}

#[test]
fn duplicate_rejects_an_empty_or_fully_unmatched_id_list() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::Duplicate { ids: vec![] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_duplicate");

    // A footprint ref (U1) is not a track/via/zone/shape/text id -- it
    // simply never matches anything, same as any other unknown id.
    let e = b.apply(&Cmd::Duplicate { ids: vec!["U1".into(), "nonexistent".into()] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_duplicate");
}

#[test]
fn duplicate_with_a_mix_of_known_and_unknown_ids_copies_only_the_known_ones() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    let track_id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();

    b.apply(&Cmd::Duplicate { ids: vec![track_id, "U1".into(), "nonexistent".into()] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().tracks.len(), 2);
}

#[test]
fn paste_items_inserts_fresh_copies_and_ignores_incoming_ids() {
    let m = net_model();
    let mut b = board(&m);
    let track = Track { id: "stale-id-from-another-board".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(2.0), y: mm(2.0) }, Point { x: mm(3.0), y: mm(2.0) }], arc_mid_offset: None };
    let text = Text { id: "also-stale".into(), content: "PASTED".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false };

    b.apply(&Cmd::PasteItems { tracks: vec![track], vias: vec![], zones: vec![], shapes: vec![], texts: vec![text] }).unwrap();

    let rt = b.design().routing.as_ref().unwrap();
    assert_eq!(rt.tracks.len(), 1);
    assert_ne!(rt.tracks[0].id, "stale-id-from-another-board");
    assert!(!rt.tracks[0].id.is_empty());
    assert_eq!(rt.tracks[0].pts, vec![Point { x: mm(2.0), y: mm(2.0) }, Point { x: mm(3.0), y: mm(2.0) }]);

    let dr = b.design().drawings.as_ref().unwrap();
    assert_ne!(dr.texts[0].id, "also-stale");
    assert_eq!(dr.texts[0].content, "PASTED");
}

#[test]
fn paste_items_with_nothing_in_it_is_a_harmless_no_op() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::PasteItems { tracks: vec![], vias: vec![], zones: vec![], shapes: vec![], texts: vec![] }).unwrap();
    assert!(b.design().routing.is_none());
    assert!(b.design().drawings.is_none());
}

// ------------------------------------------------------- commit route

#[test]
fn commit_route_replaces_a_shoved_track_and_adds_the_new_route_in_one_step() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(2.5), y: mm(-2.0) }, Point { x: mm(2.5), y: mm(2.0) }] }).unwrap();
    let old_id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();

    b.apply(&Cmd::CommitRoute {
        remove_track_ids: vec![old_id.clone()],
        remove_via_ids: vec![],
        tracks: vec![
            Track { id: "ignored".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }], arc_mid_offset: None },
            Track { id: "ignored2".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(2.5), y: mm(-2.0) }, Point { x: mm(2.2), y: Nm::ZERO }, Point { x: mm(2.5), y: mm(2.0) }], arc_mid_offset: None },
        ],
        vias: vec![],
        lock_tracks: vec![],
        via_kinds: vec![],
    })
    .unwrap();

    let tracks = &b.design().routing.as_ref().unwrap().tracks;
    assert_eq!(tracks.len(), 2, "the old track is gone, both new ones are present");
    assert!(tracks.iter().all(|t| t.id != old_id), "the removed id must not reappear");
    assert!(tracks.iter().all(|t| t.id != "ignored" && t.id != "ignored2"), "incoming ids are always reassigned, same as PasteItems");
    assert!(tracks.iter().any(|t| t.pts[0] == Point { x: Nm::ZERO, y: Nm::ZERO }));
    assert!(tracks.iter().any(|t| t.pts.len() == 3), "the shoved track's new detour shape must survive");
}

#[test]
fn commit_route_tolerates_an_already_gone_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::CommitRoute { remove_track_ids: vec!["trk_doesnotexist".into()], remove_via_ids: vec![], tracks: vec![Track { id: String::new(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }], arc_mid_offset: None }], vias: vec![], lock_tracks: vec![], via_kinds: vec![] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().tracks.len(), 1);
}

#[test]
fn commit_route_rejects_an_unknown_net() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::CommitRoute { remove_track_ids: vec![], remove_via_ids: vec![], tracks: vec![Track { id: String::new(), net: "NOPE".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }], arc_mid_offset: None }], vias: vec![], lock_tracks: vec![], via_kinds: vec![] }).unwrap_err();
    assert!(!e.is_empty());
    assert!(b.design().routing.is_none(), "a refused commit must not partially apply");
}

// ------------------------------------------------------- move exact

#[test]
fn move_exact_with_no_pivot_spins_a_single_part_in_place_around_its_own_anchor() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(10.0), y: mm(10.0) }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into()], dx: Nm::ZERO, dy: Nm::ZERO, rotate_millideg: 90_000, pivot: None }).unwrap();
    let pose = b.pose_of("U1").unwrap();
    assert_eq!(pose.at, Point { x: mm(10.0), y: mm(10.0) }, "no pivot given: rotating about its own anchor must not move it");
    assert_eq!(pose.rot, 90_000);
}

#[test]
fn move_exact_translates_then_rotates_about_its_own_already_moved_anchor() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(10.0), y: mm(10.0) }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into()], dx: mm(5.0), dy: mm(1.0), rotate_millideg: 45_000, pivot: None }).unwrap();
    let pose = b.pose_of("U1").unwrap();
    // Translation lands first, then the (pivot-less) rotation spins it in
    // place at that new position -- the position is exactly the
    // translation, regardless of the rotation angle.
    assert_eq!(pose.at, Point { x: mm(15.0), y: mm(11.0) });
    assert_eq!(pose.rot, 45_000);
}

#[test]
fn move_exact_with_a_shared_pivot_orbits_the_part_around_it() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    // Start 10mm due east of the pivot at (10mm, 10mm); a 90 degree
    // rotation around that pivot (this crate's rotation matrix: no
    // Y-axis flip, same as `to_board`) must swing it to due "south" of
    // the pivot in this app's Y-down board coordinates.
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(20.0), y: mm(10.0) }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into()], dx: Nm::ZERO, dy: Nm::ZERO, rotate_millideg: 90_000, pivot: Some(Point { x: mm(10.0), y: mm(10.0) }) }).unwrap();
    let pose = b.pose_of("U1").unwrap();
    assert_eq!(pose.at.x, mm(10.0), "rotated 90 degrees around the pivot, the +10000 x offset becomes 0 relative to it (within rounding)");
    assert_eq!(pose.at.y, mm(20.0));
    assert_eq!(pose.rot, 90_000, "orientation still turns by the same angle regardless of the pivot choice");
}

#[test]
fn move_exact_applies_the_same_translation_and_angle_to_every_part_in_the_batch() {
    let m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(10.0), y: mm(10.0) }).unwrap();
    b.apply(&Cmd::PlaceAt { part: "U2".into(), x: mm(30.0), y: mm(10.0) }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into(), "U2".into()], dx: mm(1.0), dy: mm(2.0), rotate_millideg: 90_000, pivot: None }).unwrap();
    assert_eq!(b.pose_of("U1").unwrap().at, Point { x: mm(11.0), y: mm(12.0) });
    assert_eq!(b.pose_of("U2").unwrap().at, Point { x: mm(31.0), y: mm(12.0) });
    assert_eq!(b.pose_of("U1").unwrap().rot, 90_000);
    assert_eq!(b.pose_of("U2").unwrap().rot, 90_000);
}

#[test]
fn move_exact_refuses_an_unplaced_or_unknown_part_without_moving_the_others() {
    let m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(10.0), y: mm(10.0) }).unwrap();
    // U2 is never placed.
    let e = b.apply(&Cmd::MoveExact { parts: vec!["U1".into(), "U2".into()], dx: mm(1.0), dy: Nm::ZERO, rotate_millideg: 0, pivot: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_not_placed");
    assert_eq!(b.pose_of("U1").unwrap().at, Point { x: mm(10.0), y: mm(10.0) }, "the whole batch refuses atomically -- U1 must not have moved either");
}

#[test]
fn move_exact_rejects_an_empty_part_list() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::MoveExact { parts: vec![], dx: Nm::ZERO, dy: Nm::ZERO, rotate_millideg: 0, pivot: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_move_exact");
}

// -------------------------------------------------- footprint editor (GAPS.md #8)

fn fp_pad(number: &str, x: Nm, y: Nm) -> LibraryPad {
    LibraryPad {
        id: String::new(),
        number: number.into(),
        at: Point { x, y },
        offset: Point { x: Nm::ZERO, y: Nm::ZERO },
        size: (mm(1.0), mm(1.0)),
        shape: eda_model::ir::LibraryPadShape::RoundRect,
        kind: eda_model::PadKind::Smd,
        drill: None,
        drill_slot: None,
        rot: 0,
        roundrect_ratio: Some(0.25),
        trapezoid_delta: None,
        chamfer_ratio: None,
        chamfer_corners: eda_model::ir::ChamferCorners::default(),
        custom: None,
        layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        clearance_override: None,
        thermal_gap_override: None,
        thermal_spoke_width_override: None,
        zone_connection: None,
        thermal_spoke_angle_mdeg: None,
        stack: Default::default(),
        kicad: Default::default(),
    }
}

#[test]
fn opening_a_new_name_starts_blank_and_is_idempotent() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:Blank".into() }).unwrap();
    let lib = b.design().footprint_library.as_ref().unwrap();
    let fp = lib.by_name("Test:Blank").unwrap();
    assert!(fp.pads.is_empty());
    assert!(!fp.published);

    // Re-opening is a no-op even after an edit -- it must never reset progress.
    b.apply(&Cmd::AddPad { footprint: "Test:Blank".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }).unwrap();
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:Blank".into() }).unwrap();
    assert_eq!(b.design().footprint_library.as_ref().unwrap().by_name("Test:Blank").unwrap().pads.len(), 1);
}

#[test]
fn new_footprint_creates_an_empty_smd_footprint_and_refuses_a_name_already_taken() {
    // `CreateNewFootprint`: `int footprintAttrs = FP_SMD;` and a name the caller made unique first.
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::NewFootprint { name: "Untitled".into() }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Untitled").unwrap();
    assert!(fp.pads.is_empty() && fp.graphics.is_empty());
    assert!(fp.attributes.smd && !fp.attributes.through_hole, "a new footprint defaults to SMD");
    assert!(!fp.published);
    // E1 (r2-editors-exports): the Footprint Editor's third default text item, `${REFERENCE}` on F.Fab at (0, 2.5 mm), 1 mm / 0.15 mm.
    assert_eq!(fp.texts.iter().map(|t| (t.content.as_str(), t.layer.as_str(), t.at.y, t.size_nm, t.stroke_width)).collect::<Vec<_>>(), [("${REFERENCE}", "F.Fab", mm(2.5), mm(1.0), mm(0.15))]);
    let text = eda_kicad::export_kicad_mod(fp);
    assert!(text.contains("(property \"Reference\" \"REF**\" (at 0 -0.5 0) (layer \"F.SilkS\")") && text.contains("(property \"Value\" \"Untitled\" (at 0 1 0) (layer \"F.Fab\")"), "{text}");
    assert!(text.contains("(fp_text user \"${REFERENCE}\" (at 0 2.5") , "{text}");

    // already in the project library
    let e = b.apply(&Cmd::NewFootprint { name: "Untitled".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_footprint_exists");
    // already defined by the builtin table
    let e = b.apply(&Cmd::NewFootprint { name: "0603".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_footprint_exists");
    assert_eq!(b.apply(&Cmd::NewFootprint { name: String::new() }).unwrap_err()[0].check, "ops_bad_footprint");
    // its own undo domain, like every footprint-editor verb
    assert_eq!(Cmd::NewFootprint { name: "x".into() }.domain(), Domain::FootprintEditor);
    // a second "Untitled_1" is fine; it takes the attributes of the library's last footprint (`fpnames.back()`), here a through-hole one
    if let Some(l) = b.design.footprint_library.as_mut() {
        l.footprints[0].attributes = eda_model::ir::FootprintAttributes { through_hole: true, ..Default::default() };
    }
    b.apply(&Cmd::NewFootprint { name: "Untitled_1".into() }).unwrap();
    assert_eq!(b.design().footprint_library.as_ref().unwrap().footprints.len(), 2);
    let second = b.design().footprint_library.as_ref().unwrap().by_name("Untitled_1").unwrap();
    assert!(second.attributes.through_hole && !second.attributes.smd, "{:?}", second.attributes);
}

#[test]
fn opening_a_builtin_name_materializes_its_real_pads() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "0603".into() }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("0603").unwrap();
    assert_eq!(fp.pads.len(), 2, "0603 is a two-pad passive");
    assert!(fp.pads.iter().all(|p| !p.id.is_empty()));
}

#[test]
fn add_pad_assigns_an_id_and_ignores_a_caller_supplied_one() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    let mut pad = fp_pad("1", Nm::ZERO, Nm::ZERO);
    pad.id = "ignored-on-input".into();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.pads.len(), 1);
    assert_ne!(fp.pads[0].id, "ignored-on-input");
    assert!(!fp.pads[0].id.is_empty());
}

#[test]
fn add_pad_against_an_unopened_footprint_is_refused() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::AddPad { footprint: "Nobody:Opened".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_footprint");
}

#[test]
fn add_pad_rejects_a_zero_sized_pad() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    let mut pad = fp_pad("1", Nm::ZERO, Nm::ZERO);
    pad.size = (Nm::ZERO, mm(1.0));
    let e = b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_pad");
}

#[test]
fn add_pad_reuses_footprint_validate_for_a_through_hole_with_no_drill() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    let mut pad = fp_pad("1", Nm::ZERO, Nm::ZERO);
    pad.kind = eda_model::PadKind::ThroughHole;
    pad.drill = None;
    let e = b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad }).unwrap_err();
    assert_eq!(e[0].check, "footprint", "reused straight from Footprint::validate, not a second copy of the rule");
}

#[test]
fn move_rotate_and_delete_pad_round_trip_by_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }).unwrap();
    let id = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads[0].id.clone();

    b.apply(&Cmd::MovePad { footprint: "Test:FP".into(), id: id.clone(), x: mm(0.5), y: mm(-0.5) }).unwrap();
    b.apply(&Cmd::RotatePad { footprint: "Test:FP".into(), id: id.clone(), quarter_turns: 1 }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.pads[0].at, Point { x: mm(0.5), y: mm(-0.5) });
    assert_eq!(fp.pads[0].rot, 90_000);

    b.apply(&Cmd::DeletePad { footprint: "Test:FP".into(), id }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads.is_empty());
}

#[test]
fn rotate_pad_wraps_at_a_full_turn() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }).unwrap();
    let id = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads[0].id.clone();
    for _ in 0..4 {
        b.apply(&Cmd::RotatePad { footprint: "Test:FP".into(), id: id.clone(), quarter_turns: 1 }).unwrap();
    }
    assert_eq!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads[0].rot, 0, "four quarter turns is a full turn, back to 0");
}

#[test]
fn edit_pad_replaces_every_field_but_keeps_the_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }).unwrap();
    let id = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads[0].id.clone();

    let mut edited = fp_pad("1A", mm(1.0), mm(2.0));
    edited.size = (mm(2.0), mm(3.0));
    b.apply(&Cmd::EditPad { footprint: "Test:FP".into(), id: id.clone(), pad: edited }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.pads[0].id, id, "the id names which pad to replace -- it never changes underneath the caller");
    assert_eq!(fp.pads[0].number, "1A");
    assert_eq!(fp.pads[0].size, (mm(2.0), mm(3.0)));
}

#[test]
fn next_pad_number_matches_what_add_pad_actually_produces() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    for n in 1..=3 {
        let next = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().next_pad_number();
        assert_eq!(next, n.to_string());
        b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad(&next, n as i64 * mm(1.0), Nm::ZERO) }).unwrap();
    }
}

#[test]
fn push_pad_properties_only_touches_filtered_matches() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    // Pad 1 (source): round, SMD. Pad 2: round, SMD (matches). Pad 3: rotated 90 (orientation mismatch).
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("2", mm(1.0), Nm::ZERO) }).unwrap();
    let mut pad3 = fp_pad("3", mm(2.0), Nm::ZERO);
    pad3.rot = 90_000;
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: pad3 }).unwrap();

    let ids: Vec<String> = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads.iter().map(|p| p.id.clone()).collect();
    // Change pad 1's size, then push with the orientation filter on.
    b.apply(&Cmd::EditPad { footprint: "Test:FP".into(), id: ids[0].clone(), pad: { let mut p = fp_pad("1", Nm::ZERO, Nm::ZERO); p.size = (mm(3.0), mm(3.0)); p } }).unwrap();
    b.apply(&Cmd::PushPadProperties {
        footprint: "Test:FP".into(),
        source_pad_id: ids[0].clone(),
        filter_shape: false,
        filter_orientation: true,
        filter_layers: false,
        filter_type: false,
    })
    .unwrap();

    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    let by_id = |id: &str| fp.pads.iter().find(|p| p.id == id).unwrap();
    assert_eq!(by_id(&ids[1]).size, (mm(3.0), mm(3.0)), "pad 2 shares the source's orientation (0) -- it must receive the push");
    assert_eq!(by_id(&ids[2]).size, (mm(1.0), mm(1.0)), "pad 3 is rotated 90 degrees -- the orientation filter must exclude it");
    assert_eq!(by_id(&ids[2]).number, "3", "push never touches number/position");
}

#[test]
fn renumber_pads_orders_by_position_not_by_current_number() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    // Placed right-to-left but numbered in placement order (1, 2, 3) --
    // a reading-order renumber must reverse them to (10, 11, 12).
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", mm(4.0), Nm::ZERO) }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("2", mm(2.0), Nm::ZERO) }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("3", Nm::ZERO, Nm::ZERO) }).unwrap();

    b.apply(&Cmd::RenumberPads { footprint: "Test:FP".into(), start: 10, prefix: String::new(), step: 1 }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    let at_x = |x: Nm| fp.pads.iter().find(|p| p.at.x == x).unwrap().number.clone();
    assert_eq!(at_x(Nm::ZERO), "10");
    assert_eq!(at_x(mm(2.0)), "11");
    assert_eq!(at_x(mm(4.0)), "12");
}

#[test]
fn set_footprint_anchor_translates_every_pad_graphic_and_text_so_the_click_point_becomes_zero() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", mm(1.0), mm(1.0)) }).unwrap();
    b.apply(&Cmd::AddFootprintGraphic {
        footprint: "Test:FP".into(),
        shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.1), filled: false, start: Point { x: mm(1.0), y: mm(1.0) }, end: Point { x: mm(2.0), y: mm(1.0) } },
    })
    .unwrap();
    b.apply(&Cmd::AddFootprintText { footprint: "Test:FP".into(), text: Text { id: String::new(), content: "REF**".into(), at: Point { x: mm(1.0), y: mm(0.5) }, angle: 0, layer: "F.SilkS".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false } })
        .unwrap();

    b.apply(&Cmd::SetFootprintAnchor { name: "Test:FP".into(), at: Point { x: mm(1.0), y: mm(1.0) } }).unwrap();

    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.pads[0].at, Point { x: Nm::ZERO, y: Nm::ZERO }, "the clicked point is now the origin");
    assert_eq!(fp.graphics[0].points()[0], Point { x: Nm::ZERO, y: Nm::ZERO });
    assert_eq!(fp.texts[0].at, Point { x: Nm::ZERO, y: mm(-0.5) });
}

#[test]
fn update_footprint_on_board_only_flips_the_explicit_flag() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    assert!(!b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().published);
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }).unwrap();
    assert!(!b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().published, "editing pads must never auto-publish");

    b.apply(&Cmd::UpdateFootprintOnBoard { name: "Test:FP".into() }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().published);
}

#[test]
fn edit_footprint_properties_replaces_the_whole_panel() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::EditFootprintProperties {
        name: "Test:FP".into(),
        description: "A test footprint".into(),
        keywords: "test smd".into(),
        attributes: FootprintAttributes { smd: true, ..Default::default() },
        reference_visible: false,
        value_visible: true,
        model: Some("${KICAD10_3DMODEL_DIR}/x.step".into()),
        models3d: None,
        zone_connection: Some(PadConnection::Full),
        reference: None,
        value: None,
        fields: None,
        datasheet: None,
        description_field: None,
    })
    .unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.zone_connection, Some(PadConnection::Full), "the Clearances tab's zone connection is part of the panel");
    assert_eq!(fp.description, "A test footprint");
    assert!(fp.attributes.smd);
    assert!(!fp.reference_visible);
    assert_eq!(fp.model.as_deref(), Some("${KICAD10_3DMODEL_DIR}/x.step"));
}

/// The Footprint Editor's Footprint Properties sends its whole panel, 3D Models tab included (`dialog_footprint_properties_fp_editor.cpp`): the list is the footprint's, in order,
/// with the placement of each; the one `model` path follows its first model; an empty list is no model.
#[test]
fn edit_footprint_properties_takes_the_whole_list_of_3d_models() {
    use eda_model::footprint::Model3d;
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    let props = |model: Option<&str>, models3d: Option<Vec<Model3d>>| Cmd::EditFootprintProperties {
        name: "Test:FP".into(),
        description: "d".into(),
        keywords: String::new(),
        attributes: FootprintAttributes::default(),
        reference_visible: true,
        value_visible: true,
        model: model.map(str::to_string),
        models3d,
        zone_connection: None,
        reference: None,
        value: None,
        fields: None,
        datasheet: None,
        description_field: None,
    };
    let two = vec![Model3d { offset: [1.0, 2.0, 0.5], rotate: [90.0, 0.0, 0.0], opacity: 0.5, ..Model3d::identity("a.step") }, Model3d { show: false, scale: [2.0, 2.0, 2.0], ..Model3d::identity("b.step") }];
    b.apply(&props(None, Some(two.clone()))).unwrap();
    let fp = |b: &Board<'_>| b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().clone();
    assert_eq!((fp(&b).models3d.clone(), fp(&b).model.clone()), (two.clone(), Some("a.step".to_string())), "the list is the footprint's and `model` is its first path");

    // an old-style call that echoes the path it was given leaves the list alone
    b.apply(&props(Some("a.step"), None)).unwrap();
    assert_eq!(fp(&b).models3d, two);
    // one that names another path makes it the footprint's one model, as before the list existed
    b.apply(&props(Some("c.step"), None)).unwrap();
    assert_eq!((fp(&b).models3d.len(), fp(&b).model), (0, Some("c.step".to_string())));
    // none at all
    b.apply(&props(None, Some(two.clone()))).unwrap();
    b.apply(&props(None, Some(vec![]))).unwrap();
    assert_eq!((fp(&b).models3d.len(), fp(&b).model), (0, None), "an empty list is no model, and the path goes with it");
    b.apply(&props(None, Some(two.clone()))).unwrap();
    b.apply(&props(None, None)).unwrap();
    assert_eq!((fp(&b).models3d.len(), fp(&b).model), (0, None), "no list and no path: the one model is taken away");

    // a row the tab would not take is refused, named, and nothing of the panel is taken
    b.apply(&props(None, Some(two.clone()))).unwrap();
    let mut changed = props(None, Some(vec![Model3d::identity("ok.step"), Model3d::identity("bad?.step")]));
    if let Cmd::EditFootprintProperties { description, .. } = &mut changed {
        *description = "changed".into();
    }
    let e = b.apply(&changed).unwrap_err();
    assert_eq!((e[0].check.as_str(), e[0].hint.as_deref()), ("ops_bad_model", Some("model 2: Illegal filename: \"bad?.step\"")));
    assert_eq!((fp(&b).description, fp(&b).models3d), ("d".to_string(), two));
}

#[test]
fn delete_library_footprint_removes_it_and_refuses_an_unknown_name() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::DeleteLibraryFootprint { name: "Test:FP".into() }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").is_none());
    let e = b.apply(&Cmd::DeleteLibraryFootprint { name: "Test:FP".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_footprint");
}

#[test]
fn footprint_graphic_and_text_add_move_edit_delete_round_trip() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::AddFootprintGraphic {
        footprint: "Test:FP".into(),
        shape: Shape::Rect { id: "ignored".into(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.0), y: mm(1.0) } },
    })
    .unwrap();
    let gid = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().graphics[0].id().to_string();
    assert_ne!(gid, "ignored");
    b.apply(&Cmd::MoveFootprintGraphic { footprint: "Test:FP".into(), id: gid.clone(), dx: mm(0.01), dy: mm(0.02) }).unwrap();
    b.apply(&Cmd::EditFootprintGraphic { footprint: "Test:FP".into(), id: gid.clone(), layer: "F.SilkS".into(), stroke_width: mm(0.2), filled: true }).unwrap();
    {
        let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
        assert_eq!(fp.graphics[0].layer(), "F.SilkS");
        assert_eq!(fp.graphics[0].points()[0], Point { x: mm(0.01), y: mm(0.02) });
    }
    b.apply(&Cmd::DeleteFootprintGraphic { footprint: "Test:FP".into(), id: gid }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().graphics.is_empty());

    b.apply(&Cmd::AddFootprintText { footprint: "Test:FP".into(), text: Text { id: String::new(), content: "VAL**".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.Fab".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false } }).unwrap();
    let tid = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().texts[0].id.clone();
    b.apply(&Cmd::MoveFootprintText { footprint: "Test:FP".into(), id: tid.clone(), x: mm(0.5), y: mm(0.5) }).unwrap();
    b.apply(&Cmd::EditFootprintText { footprint: "Test:FP".into(), id: tid.clone(), content: "V2".into(), angle: 900, layer: "F.SilkS".into(), size_nm: mm(0.8), stroke_width: mm(0.12), justify: TextJustify::Left, mirror: true }).unwrap();
    {
        let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
        assert_eq!(fp.texts[0].at, Point { x: mm(0.5), y: mm(0.5) });
        assert_eq!(fp.texts[0].content, "V2");
        assert!(fp.texts[0].mirror);
    }
    b.apply(&Cmd::DeleteFootprintText { footprint: "Test:FP".into(), id: tid }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().texts.is_empty());
}

#[test]
fn footprint_editor_commands_are_their_own_undo_domain() {
    assert_eq!(Cmd::AddPad { footprint: "x".into(), pad: fp_pad("1", Nm::ZERO, Nm::ZERO) }.domain(), Domain::FootprintEditor);
    assert_eq!(Cmd::OpenFootprintForEdit { name: "x".into() }.domain(), Domain::FootprintEditor);
    assert_eq!(Cmd::UpdateFootprintOnBoard { name: "x".into() }.domain(), Domain::FootprintEditor);
    // Sanity: the other two domains are unaffected by this addition.
    assert_eq!(Cmd::MoveTo { part: "U1".into(), x: Nm::ZERO, y: Nm::ZERO }.domain(), Domain::Pcb);
    assert_eq!(Cmd::AddWire { pts: vec![], bus: false }.domain(), Domain::Schematic);
}

// ------------------------------------------------------------ multi-unit symbols (GAPS.md #21)

fn add_unit(b: &mut Board<'_>, id: &str, unit: u32, x: Nm) {
    b.apply(&Cmd::AddSymbol { id: id.into(), lib_id: "test:DUAL".into(), at: Point { x, y: Nm::ZERO }, rot_millideg: 0, value: "DUAL".into(), footprint: String::new(), unit }).unwrap();
}

/// Placing a second unit of an *already-placed* reference (same `id`, a
/// different `unit`) is the supported way to add it -- see `Cmd::AddSymbol`'s
/// own doc -- and only an exact `(id, unit)` repeat is refused.
#[test]
fn add_symbol_places_additional_units_of_an_existing_reference() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    add_unit(&mut b, "U1", 1, Nm::ZERO);
    add_unit(&mut b, "U1", 2, mm(50.0));
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.symbols.len(), 2);
    assert!(sch.symbols.iter().all(|s| s.id == "U1"));
    let units: std::collections::BTreeSet<u32> = sch.symbols.iter().map(|s| s.unit).collect();
    assert_eq!(units, std::collections::BTreeSet::from([1, 2]));

    let e = b.apply(&Cmd::AddSymbol { id: "U1".into(), lib_id: "test:DUAL".into(), at: Point { x: mm(99.0), y: Nm::ZERO }, rot_millideg: 0, value: String::new(), footprint: String::new(), unit: 1 }).unwrap_err();
    assert_eq!(e[0].check, "ops_duplicate_symbol");
}

/// Move/rotate/mirror/delete on a reference with more than one placed unit
/// refuse as ambiguous without a `unit` to pick between them, same as a
/// caller that predates multi-unit support would see on a reference that
/// somehow already had two placements -- and succeed, touching only the
/// named unit, once one is given.
#[test]
fn per_instance_verbs_require_a_unit_once_more_than_one_is_placed() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    add_unit(&mut b, "U1", 1, Nm::ZERO);
    add_unit(&mut b, "U1", 2, mm(50.0));

    let e = b.apply(&Cmd::MoveSymbol { id: "U1".into(), x: mm(1.0), y: mm(1.0), unit: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_ambiguous_symbol");

    b.apply(&Cmd::MoveSymbol { id: "U1".into(), x: mm(1.0), y: mm(2.0), unit: Some(2) }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let unit1 = sch.symbols.iter().find(|s| s.unit == 1).unwrap();
    let unit2 = sch.symbols.iter().find(|s| s.unit == 2).unwrap();
    assert_eq!(unit1.at, Point { x: Nm::ZERO, y: Nm::ZERO }, "unit 1 untouched by a move scoped to unit 2");
    assert_eq!(unit2.at, Point { x: mm(1.0), y: mm(2.0) });

    b.apply(&Cmd::RotateSymbol { id: "U1".into(), quarter_turns: 1, unit: Some(1) }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.symbols.iter().find(|s| s.unit == 1).unwrap().rot, 90_000);
    assert_eq!(sch.symbols.iter().find(|s| s.unit == 2).unwrap().rot, 0, "unit 2 untouched by a rotate scoped to unit 1");

    b.apply(&Cmd::DeleteSymbol { id: "U1".into(), unit: Some(1) }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.symbols.len(), 1, "only unit 1 removed");
    assert_eq!(sch.symbols[0].unit, 2);
}

/// `unit: None` still means exactly what it always did when there is only
/// one placed instance -- no behavior change for every single-unit part.
#[test]
fn per_instance_verbs_with_no_unit_still_work_for_a_single_unit_part() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    add_unit(&mut b, "U1", 1, Nm::ZERO);
    b.apply(&Cmd::MoveSymbol { id: "U1".into(), x: mm(5.0), y: mm(6.0), unit: None }).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().symbols[0].at, Point { x: mm(5.0), y: mm(6.0) });
}

/// Reference/Value/Footprint/Datasheet are part-wide: editing or renaming
/// touches every placed unit together, never just one of them.
#[test]
fn edit_and_rename_apply_to_every_unit_together() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    add_unit(&mut b, "U1", 1, Nm::ZERO);
    add_unit(&mut b, "U1", 2, mm(50.0));

    b.apply(&Cmd::EditSymbolFields { id: "U1".into(), value: Some("74HC00".into()), footprint: None, datasheet: None }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert!(sch.symbols.iter().all(|s| s.value == "74HC00"));

    b.apply(&Cmd::RenameSymbol { id: "U1".into(), new_id: "U5".into() }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.symbols.len(), 2);
    assert!(sch.symbols.iter().all(|s| s.id == "U5"));
    let units: std::collections::BTreeSet<u32> = sch.symbols.iter().map(|s| s.unit).collect();
    assert_eq!(units, std::collections::BTreeSet::from([1, 2]));
}

// ------------------------------------------------------------ eeschema hotkey sweep: junction, lines, sheet, swap

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

#[test]
fn a_junction_is_added_once_per_point_deleted_by_id_and_belongs_to_the_schematic_domain() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    assert_eq!(Cmd::AddJunction { at: p(Nm::ZERO, Nm::ZERO) }.domain(), Domain::Schematic);
    assert_eq!(Cmd::DeleteJunction { id: "x".into() }.domain(), Domain::Schematic);
    b.apply(&Cmd::AddJunction { at: p(mm(2.54), mm(5.08)) }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.junctions.len(), 1);
    assert!(sch.junctions[0].id.starts_with("jct_"), "{}", sch.junctions[0].id);
    // a second one at the same point is a no-op in source (a click on a junction does nothing): refused here so undo never records nothing
    assert_eq!(b.apply(&Cmd::AddJunction { at: p(mm(2.54), mm(5.08)) }).unwrap_err()[0].check, "ops_junction_exists");
    b.apply(&Cmd::AddJunction { at: p(Nm::ZERO, Nm::ZERO) }).unwrap();
    let id = b.design().schematic.as_ref().unwrap().junctions.iter().find(|j| j.at == p(mm(2.54), mm(5.08))).unwrap().id.clone();
    b.apply(&Cmd::DeleteJunction { id: id.clone() }).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().junctions.len(), 1);
    assert_eq!(b.apply(&Cmd::DeleteJunction { id }).unwrap_err()[0].check, "ops_unknown_junction");
}

#[test]
fn a_graphic_line_needs_two_points_and_a_sane_width() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    assert_eq!(b.apply(&Cmd::AddSchLine { pts: vec![p(Nm::ZERO, Nm::ZERO)], width_nm: Nm::ZERO }).unwrap_err()[0].check, "ops_bad_line");
    assert_eq!(b.apply(&Cmd::AddSchLine { pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(0.01), Nm::ZERO)], width_nm: Nm::new(-1) }).unwrap_err()[0].check, "ops_bad_line");
    // a line whose points all coincide has no segment left once zero-length ones are dropped
    assert_eq!(b.apply(&Cmd::AddSchLine { pts: vec![p(um(5.0), um(5.0)), p(um(5.0), um(5.0))], width_nm: Nm::ZERO }).unwrap_err()[0].check, "ops_bad_line");
    // a double-click lands its last point twice: the repeat is dropped
    b.apply(&Cmd::AddSchLine { pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(10.0), Nm::ZERO), p(mm(10.0), mm(5.0)), p(mm(10.0), mm(5.0))], width_nm: mm(0.254) }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.lines.len(), 1);
    assert_eq!(sch.lines[0].pts.len(), 3);
    assert_eq!(sch.lines[0].width_nm, mm(0.254));
    assert!(sch.lines[0].id.starts_with("sln_"), "{}", sch.lines[0].id);
    assert!(sch.wires.is_empty(), "a notes-layer line is never a wire");
    let id = sch.lines[0].id.clone();
    b.apply(&Cmd::DeleteSchLine { id: id.clone() }).unwrap();
    assert!(b.design().schematic.as_ref().unwrap().lines.is_empty());
    assert_eq!(b.apply(&Cmd::DeleteSchLine { id }).unwrap_err()[0].check, "ops_unknown_line");
}

#[test]
fn a_new_sheet_gets_its_own_empty_screen_and_a_shared_file_is_reused() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::AddSheet { name: "Power".into(), file: "power".into(), at: p(mm(10.0), mm(10.0)), size: (mm(30.0), mm(20.0)) }).unwrap();
    let d = b.design();
    let sheet = &d.schematic.as_ref().unwrap().sheets[0];
    assert_eq!((sheet.name.as_str(), sheet.file.as_str(), sheet.size), ("Power", "power.kicad_sch", (mm(30.0), mm(20.0))), "the extension is added");
    assert!(sheet.id.starts_with("sheet_"), "{}", sheet.id);
    let screens = d.sheet_contents.as_ref().expect("a screen for the new file");
    assert!(screens.get("power.kicad_sch").unwrap().symbols.is_empty());

    // a second placement of the same file shares the screen (and keeps what is already drawn on it)
    b.apply(&Cmd::AddSheet { name: "Power 2".into(), file: "power.kicad_sch".into(), at: p(mm(50.0), mm(10.0)), size: (mm(30.0), mm(20.0)) }).unwrap();
    assert_eq!(b.design().sheet_contents.as_ref().unwrap().len(), 1, "same file, same screen");
    assert_eq!(b.design().schematic.as_ref().unwrap().sheets.len(), 2);
}

#[test]
fn a_sheet_is_refused_without_a_name_with_a_folder_in_its_file_with_a_taken_name_or_with_no_size() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    let add = |name: &str, file: &str, size: (Nm, Nm)| Cmd::AddSheet { name: name.into(), file: file.into(), at: p(Nm::ZERO, Nm::ZERO), size };
    assert_eq!(b.apply(&add("", "a", (Nm::new(1), Nm::new(1)))).unwrap_err()[0].check, "ops_bad_sheet");
    assert_eq!(b.apply(&add("S", "", (Nm::new(1), Nm::new(1)))).unwrap_err()[0].check, "ops_bad_sheet");
    assert_eq!(b.apply(&add("S", "sub/a.kicad_sch", (Nm::new(1), Nm::new(1)))).unwrap_err()[0].check, "ops_bad_sheet");
    assert_eq!(b.apply(&add("S", "a", (Nm::ZERO, mm(5.0)))).unwrap_err()[0].check, "ops_bad_sheet");
    b.apply(&add("S", "a", (mm(5.0), mm(5.0)))).unwrap();
    assert_eq!(b.apply(&add("S", "b", (mm(5.0), mm(5.0)))).unwrap_err()[0].check, "ops_sheet_name_taken");
}

fn place_sym(b: &mut Board<'_>, id: &str, lib_id: &str, at: Point, rot: Millideg) {
    b.apply(&Cmd::AddSymbol { id: id.into(), lib_id: lib_id.into(), at, rot_millideg: rot, value: String::new(), footprint: String::new(), unit: 1 }).unwrap();
}

#[test]
fn swap_exchanges_positions_and_the_orientation_only_for_the_same_library_symbol() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    place_sym(&mut b, "R1", "Device:R", p(mm(10.0), mm(10.0)), 0);
    place_sym(&mut b, "R2", "Device:R", p(mm(30.0), mm(20.0)), 90_000);
    place_sym(&mut b, "D1", "Device:LED", p(mm(50.0), mm(30.0)), 180_000);
    b.apply(&Cmd::SwapSchItems { a: "R1".into(), b: "R2".into() }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let get = |id: &str| sch.symbols.iter().find(|s| s.id == id).unwrap();
    assert_eq!((get("R1").at, get("R1").rot), (p(mm(30.0), mm(20.0)), 90_000), "R1 takes R2's place and orientation");
    assert_eq!((get("R2").at, get("R2").rot), (p(mm(10.0), mm(10.0)), 0));
    // a resistor and an LED: positions swap, orientations stay (different default orientations)
    b.apply(&Cmd::SwapSchItems { a: "R2".into(), b: "D1".into() }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let get = |id: &str| sch.symbols.iter().find(|s| s.id == id).unwrap();
    assert_eq!((get("R2").at, get("R2").rot), (p(mm(50.0), mm(30.0)), 0));
    assert_eq!((get("D1").at, get("D1").rot), (p(mm(10.0), mm(10.0)), 180_000));
}

#[test]
fn swap_reaches_labels_texts_and_power_symbols_and_a_chain_rotates_the_positions() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::AddLabel { net: "A".into(), at: p(mm(1.0), mm(1.0)), kind: LabelKind::Local }).unwrap();
    b.apply(&Cmd::AddSchText { content: "note".into(), at: p(mm(2.0), mm(2.0)), angle_millideg: 0, size_nm: mm(1.27) }).unwrap();
    b.apply(&Cmd::AddPowerSymbol { lib_id: "power:GND".into(), at: p(mm(3.0), mm(3.0)), rot_millideg: 0, net: "GND".into(), pin: "1".into() }).unwrap();
    // labels and texts get their ids when the design is next loaded (`board::load`'s back-fill) -- do that here
    let mut d = b.into_design();
    d.assign_missing_ids();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let sch = b.design().schematic.as_ref().unwrap();
    let (lbl, txt, pwr) = (sch.labels[0].id.clone(), sch.texts[0].id.clone(), sch.power_symbols[0].id.clone());
    // sorted = [label, text, power]: swap(label, text), then swap(text, power) -- Swap's own loop over `sorted[i]`, `sorted[i + 1]`
    b.apply(&Cmd::Batch { cmds: vec![Cmd::SwapSchItems { a: lbl.clone(), b: txt.clone() }, Cmd::SwapSchItems { a: txt.clone(), b: pwr.clone() }] }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.labels[0].at, p(mm(2.0), mm(2.0)), "label takes the text's place");
    assert_eq!(sch.texts[0].at, p(mm(3.0), mm(3.0)), "text takes the power symbol's");
    assert_eq!(sch.power_symbols[0].at, p(mm(1.0), mm(1.0)), "power symbol ends where the label started");
}

#[test]
fn swap_refuses_the_same_item_twice_unknown_ids_and_an_ambiguous_multi_unit_reference() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    add_unit(&mut b, "U1", 1, Nm::ZERO);
    add_unit(&mut b, "U1", 2, mm(50.0));
    place_sym(&mut b, "R1", "Device:R", p(Nm::new(1), Nm::new(1)), 0);
    assert_eq!(b.apply(&Cmd::SwapSchItems { a: "R1".into(), b: "R1".into() }).unwrap_err()[0].check, "ops_bad_swap");
    assert_eq!(b.apply(&Cmd::SwapSchItems { a: "R1".into(), b: "nope".into() }).unwrap_err()[0].check, "ops_unknown_item");
    assert_eq!(b.apply(&Cmd::SwapSchItems { a: "R1".into(), b: "U1".into() }).unwrap_err()[0].check, "ops_ambiguous_symbol");
}

#[test]
fn a_new_symbol_is_an_empty_project_library_entry_and_a_taken_name_is_refused() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    assert_eq!(Cmd::NewSymbol { lib_id: "eda:Untitled".into() }.domain(), Domain::SymbolEditor);
    b.apply(&Cmd::NewSymbol { lib_id: "eda:Untitled".into() }).unwrap();
    let lib = b.design().symbol_library.as_ref().unwrap();
    let sym = lib.by_lib_id("eda:Untitled").expect("created");
    assert!(sym.pins.is_empty() && sym.graphics.is_empty());
    assert_eq!(b.apply(&Cmd::NewSymbol { lib_id: "eda:Untitled".into() }).unwrap_err()[0].check, "ops_symbol_exists");
    assert_eq!(b.apply(&Cmd::NewSymbol { lib_id: String::new() }).unwrap_err()[0].check, "ops_bad_symbol");
    // a name a builtin library symbol already answers to is taken too
    assert_eq!(b.apply(&Cmd::NewSymbol { lib_id: "Device:R".into() }).unwrap_err()[0].check, "ops_symbol_exists");
}

// Create Array's tests are in `array_tests.rs`.

// ------------------------------------------------------------ dimensions (task item 7)

fn aligned_dim(start: Point, end: Point) -> Dimension {
    Dimension {
        id: String::new(),
        layer: "Dwgs.User".into(),
        kind: DimensionKind::Aligned { height: mm(1.0) },
        start,
        end,
        prefix: String::new(),
        suffix: String::new(),
        override_text: None,
        units: DimensionUnits::Mm,
        units_format: DimensionUnitsFormat::NoSuffix,
        precision: 2,
        suppress_trailing_zeros: true,
        text_position: DimensionTextPosition::Outside,
        keep_text_aligned: true,
        text_angle: 0,
        text_size_nm: mm(1.0),
        stroke_width: mm(0.15),
        arrow_length: mm(1.0),
        extension_offset: mm(0.2),
        extension_height: mm(0.5),
        arrow_direction: ArrowDirection::Outward,
        text_thickness_nm: None,
    }
}

#[test]
fn add_dimension_needs_a_layer_and_two_distinct_points() {
    let m = net_model();
    let mut b = board(&m);

    let mut no_layer = aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO });
    no_layer.layer = String::new();
    let e = b.apply(&Cmd::AddDimension { dimension: no_layer }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_dimension");

    let same_point = aligned_dim(Point { x: mm(0.5), y: mm(0.5) }, Point { x: mm(0.5), y: mm(0.5) });
    let e = b.apply(&Cmd::AddDimension { dimension: same_point }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_dimension");

    assert!(b.design().drawings.as_ref().map(|d| d.dimensions.is_empty()).unwrap_or(true));
}

#[test]
fn add_dimension_assigns_an_id_ignoring_any_the_caller_sent() {
    let m = net_model();
    let mut b = board(&m);
    let mut dim = aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO });
    dim.id = "caller-supplied".into();
    b.apply(&Cmd::AddDimension { dimension: dim }).unwrap();

    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.dimensions.len(), 1);
    assert!(!dr.dimensions[0].id.is_empty());
    assert_ne!(dr.dimensions[0].id, "caller-supplied");
    assert!(dr.dimensions[0].id.starts_with("dim_"));
}

#[test]
fn delete_dimension_removes_by_id_and_refuses_an_unknown_one() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddDimension { dimension: aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }) }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().dimensions[0].id.clone();

    b.apply(&Cmd::DeleteDimension { id: id.clone() }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().dimensions.is_empty());

    let e = b.apply(&Cmd::DeleteDimension { id }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_dimension");
}

#[test]
fn move_dimension_translates_both_feature_points_but_not_the_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddDimension { dimension: aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }) }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().dimensions[0].id.clone();

    b.apply(&Cmd::MoveDimension { id: id.clone(), dx: mm(0.5), dy: mm(-0.2) }).unwrap();

    let dim = &b.design().drawings.as_ref().unwrap().dimensions[0];
    assert_eq!(dim.id, id);
    assert_eq!(dim.start, Point { x: mm(0.5), y: mm(-0.2) });
    assert_eq!(dim.end, Point { x: mm(1.5), y: mm(-0.2) });
}

#[test]
fn edit_dimension_replaces_every_field_but_keeps_the_original_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddDimension { dimension: aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }) }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().dimensions[0].id.clone();

    let mut edited = aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(2.0), y: Nm::ZERO });
    edited.id = "ignored-too".into();
    edited.kind = DimensionKind::Orthogonal { height: mm(0.5), horizontal: true };
    edited.prefix = "L=".into();
    b.apply(&Cmd::EditDimension { id: id.clone(), dimension: edited }).unwrap();

    let dim = &b.design().drawings.as_ref().unwrap().dimensions[0];
    assert_eq!(dim.id, id, "editing must never move the id");
    assert_eq!(dim.end, Point { x: mm(2.0), y: Nm::ZERO });
    assert_eq!(dim.prefix, "L=");
    assert!(matches!(dim.kind, DimensionKind::Orthogonal { horizontal: true, .. }));
}

#[test]
fn edit_dimension_on_an_unknown_id_is_refused() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::EditDimension { id: "nope".into(), dimension: aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_dimension");
}

#[test]
fn set_dimension_settings_replaces_the_whole_struct() {
    let m = net_model();
    let mut b = board(&m);
    let settings = DimensionSettings { precision: 1, arrow_length: mm(2.0), ..Default::default() };
    b.apply(&Cmd::SetDimensionSettings { settings }).unwrap();

    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.dimension_settings.precision, 1);
    assert_eq!(dr.dimension_settings.arrow_length, mm(2.0));
}

#[test]
fn dimension_commands_never_clear_routing() {
    let id = "x".to_string();
    let cmds = [
        Cmd::AddDimension { dimension: aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }) },
        Cmd::DeleteDimension { id: id.clone() },
        Cmd::MoveDimension { id: id.clone(), dx: Nm::ZERO, dy: Nm::ZERO },
        Cmd::EditDimension { id, dimension: aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }) },
        Cmd::SetDimensionSettings { settings: DimensionSettings::default() },
    ];
    for c in &cmds {
        assert!(!c.clears_routing(), "{c:?} must not clear routing -- it never moves a part");
    }
}

// ------------------------------------------------------- swap layers (task item 8)

#[test]
fn swap_layers_remaps_every_kind_but_leaves_vias_and_unmapped_layers_alone() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }] }).unwrap();
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(10.0), y: Nm::ZERO }, Point { x: mm(10.0), y: mm(10.0) }] }).unwrap();
    b.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.0), y: Nm::ZERO } } }).unwrap();
    b.apply(&Cmd::AddText { text: Text { id: String::new(), content: "REV A".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: 0, layer: "F.Cu".into(), size_nm: mm(1.0), stroke_width: mm(0.15), justify: TextJustify::Center, mirror: false } }).unwrap();
    b.apply(&Cmd::AddDimension { dimension: aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO }) }).unwrap(); // aligned_dim's own layer is "Dwgs.User", not swapped by this test's mapping
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: mm(5.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();

    b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "B.Cu".into())] }).unwrap();

    let rt = b.design().routing.as_ref().unwrap();
    assert_eq!(rt.tracks[0].layer, "B.Cu", "a track on a mapped layer must move");
    assert_eq!(rt.zones[0].layer, "B.Cu", "a zone on a mapped layer must move");
    assert_eq!(rt.vias[0].from_layer, "F.Cu", "vias are never remapped -- every via here is source's own skipped THROUGH case");
    assert_eq!(rt.vias[0].to_layer, "B.Cu");

    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.shapes[0].layer(), "F.SilkS", "a layer absent from the mapping must be left untouched");
    assert_eq!(dr.texts[0].layer, "B.Cu", "a text on a mapped layer must move");
    assert_eq!(dr.dimensions[0].layer, "Dwgs.User", "a dimension on a layer absent from the mapping must be left untouched");
}

#[test]
fn swap_layers_remaps_a_dimension_on_a_mapped_layer() {
    let m = net_model();
    let mut b = board(&m);
    let mut dim = aligned_dim(Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.0), y: Nm::ZERO });
    dim.layer = "F.Cu".into();
    b.apply(&Cmd::AddDimension { dimension: dim }).unwrap();

    b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "B.Cu".into())] }).unwrap();

    assert_eq!(b.design().drawings.as_ref().unwrap().dimensions[0].layer, "B.Cu");
}

#[test]
fn swap_layers_remaps_a_blind_via_but_never_a_through_via() {
    // `GLOBAL_EDIT_TOOL::SwapLayers`: THROUGH vias are skipped, any other
    // via gets `SetLayerPair( map[top], map[bottom] )`.
    let mut m = net_model();
    m.board.layers = vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: Nm::ZERO, y: Nm::ZERO, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "In1.Cu".into() }).unwrap();
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(5.0), y: Nm::ZERO, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();

    b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "B.Cu".into()), ("In1.Cu".into(), "In2.Cu".into())] }).unwrap();

    let vias = &b.design().routing.as_ref().unwrap().vias;
    let blind = vias.iter().find(|v| v.at.x == Nm::ZERO).unwrap();
    let through = vias.iter().find(|v| v.at.x == mm(5.0)).unwrap();
    assert_eq!((blind.from_layer.as_str(), blind.to_layer.as_str()), ("B.Cu", "In2.Cu"));
    assert_eq!((through.from_layer.as_str(), through.to_layer.as_str()), ("F.Cu", "B.Cu"));
}

#[test]
fn swap_layers_rejects_an_unknown_destination_layer() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::SwapLayers { mapping: vec![("F.Cu".into(), "In1.Cu".into())] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_layer");
}

// ------------------------------------------------------------ symbol editor

fn sym_pin(number: &str, x: f64, y: f64) -> LibrarySymbolPin {
    LibrarySymbolPin {
        id: String::new(),
        number: number.into(),
        name: String::new(),
        electrical_type: "passive".into(),
        shape: "line".into(),
        at: eda_model::symbol::SPoint::new(x, y),
        angle_deg: 270.0,
        length_mm: 2.54,
        unit: 1,
        body_style: 1,
        hidden: false,
        name_size_mm: Some(1.27),
        number_size_mm: Some(1.27),
        alternates: Vec::new(),
    }
}

#[test]
fn opening_a_new_lib_id_starts_blank_and_is_idempotent() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Blank".into() }).unwrap();
    let lib = b.design().symbol_library.as_ref().unwrap();
    let sym = lib.by_lib_id("Test:Blank").unwrap();
    assert!(sym.pins.is_empty());
    assert!(!sym.published);

    // Re-opening is a no-op even after an edit -- it must never reset progress.
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Blank".into(), pin: sym_pin("1", 0.0, 3.81) }).unwrap();
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Blank".into() }).unwrap();
    assert_eq!(b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Blank").unwrap().pins.len(), 1);
}

#[test]
fn opening_a_builtin_lib_id_materializes_its_real_pins() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Device:R".into() }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Device:R").unwrap();
    assert_eq!(sym.pins.len(), 2, "Device:R is a two-pin passive");
    assert!(sym.pins.iter().all(|p| !p.id.is_empty()));
    assert_eq!(sym.reference_prefix, "R");
}

#[test]
fn add_pin_assigns_an_id_and_ignores_a_caller_supplied_one() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    let mut pin = sym_pin("1", 0.0, 3.81);
    pin.id = "ignored-on-input".into();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    assert_eq!(sym.pins.len(), 1);
    assert_ne!(sym.pins[0].id, "ignored-on-input");
    assert!(!sym.pins[0].id.is_empty());
}

#[test]
fn add_pin_against_an_unopened_symbol_is_refused() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::AddSymbolPin { lib_id: "Nobody:Opened".into(), pin: sym_pin("1", 0.0, 0.0) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_symbol");
}

#[test]
fn add_pin_takes_an_empty_number_as_createpin_makes_it() {
    // r2-editors-exports E2: `CreatePin` leaves the number empty and Pin Properties' OK does not ask for one.
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: sym_pin("", 0.0, 0.0) }).unwrap();
}

#[test]
fn move_edit_and_delete_pin_round_trip() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: sym_pin("1", 0.0, 3.81) }).unwrap();
    let id = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().pins[0].id.clone();

    b.apply(&Cmd::MoveSymbolPin { lib_id: "Test:Sym".into(), id: id.clone(), x: 1.0, y: 2.0 }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    assert_eq!(sym.pins[0].at, eda_model::symbol::SPoint::new(1.0, 2.0));

    let mut edited = sym_pin("2", 1.0, 2.0);
    edited.electrical_type = "output".into();
    b.apply(&Cmd::EditSymbolPin { lib_id: "Test:Sym".into(), id: id.clone(), pin: edited }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    assert_eq!(sym.pins[0].number, "2");
    assert_eq!(sym.pins[0].electrical_type, "output");
    assert_eq!(sym.pins[0].id, id, "edit must keep the pin's own id, not the caller's pin.id");

    b.apply(&Cmd::DeleteSymbolPin { lib_id: "Test:Sym".into(), id }).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().pins.is_empty());
}

#[test]
fn next_pin_number_is_used_for_auto_increment_while_placing_in_sequence() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    for n in 1..=4 {
        let next = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().next_pin_number();
        b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: sym_pin(&next, 0.0, n as f64 * 2.54) }).unwrap();
    }
    let numbers: Vec<String> = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().pins.iter().map(|p| p.number.clone()).collect();
    assert_eq!(numbers, vec!["1", "2", "3", "4"]);
}

#[test]
fn push_pin_property_skips_length_across_different_body_styles_but_not_text_sizes() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: { let mut p = sym_pin("1", 0.0, 3.81); p.length_mm = 5.08; p.name_size_mm = Some(2.0); p } }).unwrap();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: { let mut p = sym_pin("2", 0.0, -3.81); p.body_style = 2; p } }).unwrap();
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: sym_pin("3", 2.54, 0.0) }).unwrap();

    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    let source_id = sym.pins.iter().find(|p| p.number == "1").unwrap().id.clone();

    b.apply(&Cmd::PushPinProperty { lib_id: "Test:Sym".into(), source_pin_id: source_id, field: PushPinField::Length, body_style: None }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    let by_num = |n: &str| sym.pins.iter().find(|p| p.number == n).unwrap();
    assert_eq!(by_num("3").length_mm, 5.08, "same body style -- length pushed");
    assert_eq!(by_num("2").length_mm, 2.54, "different body style -- length NOT pushed");
}

#[test]
fn add_edit_and_delete_symbol_graphic_round_trip() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    b.apply(&Cmd::AddSymbolGraphic {
        lib_id: "Test:Sym".into(),
        graphic: LibrarySymbolGraphic::Rectangle { id: "ignored".into(), unit: 1, body_style: 1, start: eda_model::symbol::SPoint::new(-1.0, -1.0), end: eda_model::symbol::SPoint::new(1.0, 1.0), stroke_mm: 0.254, fill: LibraryFill::None },
    })
    .unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    assert_eq!(sym.graphics.len(), 1);
    let id = sym.graphics[0].id().to_string();
    assert_ne!(id, "ignored");

    b.apply(&Cmd::MoveSymbolGraphic { lib_id: "Test:Sym".into(), id: id.clone(), dx_mm: 1.0, dy_mm: 1.0 }).unwrap();
    b.apply(&Cmd::EditSymbolGraphic { lib_id: "Test:Sym".into(), id: id.clone(), stroke_mm: 0.5, fill: LibraryFill::Background, unit: None, body_style: None }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    match &sym.graphics[0] {
        LibrarySymbolGraphic::Rectangle { start, stroke_mm, fill, .. } => {
            assert_eq!(*start, eda_model::symbol::SPoint::new(0.0, 0.0));
            assert_eq!(*stroke_mm, 0.5);
            assert_eq!(*fill, LibraryFill::Background);
        }
        other => panic!("expected a rectangle, got {other:?}"),
    }

    b.apply(&Cmd::DeleteSymbolGraphic { lib_id: "Test:Sym".into(), id }).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().graphics.is_empty());
}

#[test]
fn edit_symbol_text_changes_content_angle_and_size_but_not_position() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Test:Sym".into(), graphic: LibrarySymbolGraphic::Text { id: String::new(), unit: 0, body_style: 0, text: "note".into(), at: eda_model::symbol::SPoint::new(0.0, 0.0), angle_deg: 0.0, size_mm: 1.27 } }).unwrap();
    let id = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().graphics[0].id().to_string();

    b.apply(&Cmd::EditSymbolText { lib_id: "Test:Sym".into(), id: id.clone(), text: "renamed".into(), angle_deg: 90.0, size_mm: 2.0 }).unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    match &sym.graphics[0] {
        LibrarySymbolGraphic::Text { text, angle_deg, size_mm, at, .. } => {
            assert_eq!(text, "renamed");
            assert_eq!(*angle_deg, 90.0);
            assert_eq!(*size_mm, 2.0);
            assert_eq!(*at, eda_model::symbol::SPoint::new(0.0, 0.0), "position is untouched by edit_symbol_text");
        }
        other => panic!("expected text, got {other:?}"),
    }

    // Editing a non-text graphic's text fields (and vice versa) is refused,
    // not a silent no-op or a panic.
    b.apply(&Cmd::AddSymbolGraphic { lib_id: "Test:Sym".into(), graphic: LibrarySymbolGraphic::Circle { id: String::new(), unit: 1, body_style: 1, center: eda_model::symbol::SPoint::new(0.0, 0.0), radius_mm: 1.0, stroke_mm: 0.254, fill: LibraryFill::None } }).unwrap();
    let circle_id = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().graphics.iter().find(|g| matches!(g, LibrarySymbolGraphic::Circle { .. })).unwrap().id().to_string();
    let e = b.apply(&Cmd::EditSymbolText { lib_id: "Test:Sym".into(), id: circle_id, text: "x".into(), angle_deg: 0.0, size_mm: 1.0 }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");
}

#[test]
fn update_symbol_on_board_only_flips_the_explicit_flag() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    assert!(!b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().published);
    b.apply(&Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: sym_pin("1", 0.0, 3.81) }).unwrap();
    assert!(!b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().published, "editing pins must never auto-publish");

    b.apply(&Cmd::UpdateSymbolOnBoard { lib_id: "Test:Sym".into() }).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().published);
}

#[test]
fn edit_symbol_properties_replaces_the_whole_panel() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    b.apply(&Cmd::EditSymbolProperties {
        lib_id: "Test:Sym".into(),
        reference_prefix: "U".into(),
        description: "A test symbol".into(),
        keywords: "test".into(),
        datasheet: "http://example.com".into(),
        power: false,
        in_bom: true,
        on_board: true,
        pin_numbers_hidden: true,
        pin_names_hidden: false,
        pin_name_offset_mm: 0.254,
        unit_count: 2,
        has_alternate_body_style: true,
        footprint_filters: vec!["SOIC*".into()],
        units_locked: None,
        power_local: None,
        exclude_from_sim: None,
        exclude_from_pos_files: None,
    })
    .unwrap();
    let sym = b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap();
    assert_eq!(sym.description, "A test symbol");
    assert_eq!(sym.unit_count, 2);
    assert!(sym.has_alternate_body_style);
    assert!(sym.pin_numbers_hidden);
    assert_eq!(sym.footprint_filters, vec!["SOIC*".to_string()]);
}

#[test]
fn edit_symbol_properties_rejects_zero_units() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    let e = b
        .apply(&Cmd::EditSymbolProperties {
            lib_id: "Test:Sym".into(),
            reference_prefix: "U".into(),
            description: String::new(),
            keywords: String::new(),
            datasheet: String::new(),
            power: false,
            in_bom: true,
            on_board: true,
            pin_numbers_hidden: false,
            pin_names_hidden: false,
            pin_name_offset_mm: 0.508,
            unit_count: 0,
            has_alternate_body_style: false,
            footprint_filters: vec![],
            units_locked: None,
            power_local: None,
            exclude_from_sim: None,
            exclude_from_pos_files: None,
        })
        .unwrap_err();
    assert_eq!(e[0].check, "ops_bad_symbol");
}

#[test]
fn delete_library_symbol_removes_the_entry_but_never_a_placed_instance() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
    b.apply(&Cmd::DeleteLibrarySymbol { lib_id: "Test:Sym".into() }).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").is_none());

    let e = b.apply(&Cmd::DeleteLibrarySymbol { lib_id: "Test:Sym".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_symbol", "deleting an already-gone symbol is refused, not a silent no-op");
}

#[test]
fn symbol_editor_commands_are_scoped_to_the_symbol_editor_domain() {
    for cmd in [
        Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() },
        Cmd::AddSymbolPin { lib_id: "Test:Sym".into(), pin: sym_pin("1", 0.0, 0.0) },
        Cmd::DeleteLibrarySymbol { lib_id: "Test:Sym".into() },
    ] {
        assert_eq!(cmd.domain(), Domain::SymbolEditor);
    }
}

// -------------------------------------------- pcbnew action-parity cmds

#[test]
fn set_locked_locks_and_unlocks_any_item_kind_and_prunes_dead_ids() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.25), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }] }).unwrap();
    let tid = b.design().routing.as_ref().unwrap().tracks[0].id.clone();

    b.apply(&Cmd::SetLocked { ids: vec![tid.clone()], locked: true }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().locked_ids, vec![tid.clone()]);
    // locking twice is idempotent (a set, not a counter)
    b.apply(&Cmd::SetLocked { ids: vec![tid.clone()], locked: true }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().locked_ids.len(), 1);

    // an unknown id refuses the whole Cmd -- nothing is half-applied
    let e = b.apply(&Cmd::SetLocked { ids: vec![tid.clone(), "nope".into()], locked: false }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_item");
    assert_eq!(b.design().drawings.as_ref().unwrap().locked_ids.len(), 1);

    // deleting the item and then touching the lock set prunes the dead id
    b.apply(&Cmd::DeleteTrack { id: tid }).unwrap();
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(1.0), y: mm(1.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let vid = b.design().routing.as_ref().unwrap().vias[0].id.clone();
    b.apply(&Cmd::SetLocked { ids: vec![vid.clone()], locked: true }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().locked_ids, vec![vid.clone()]);
    b.apply(&Cmd::SetLocked { ids: vec![vid], locked: false }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().locked_ids.is_empty());

    assert_eq!(b.apply(&Cmd::SetLocked { ids: vec![], locked: true }).unwrap_err()[0].check, "ops_bad_lock");
}

#[test]
fn set_locked_accepts_a_placed_part_but_not_an_unplaced_one() {
    let m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre }).unwrap();
    b.apply(&Cmd::SetLocked { ids: vec!["U1".into()], locked: true }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().locked_ids, vec!["U1".to_string()]);
    assert_eq!(b.apply(&Cmd::SetLocked { ids: vec!["U2".into()], locked: true }).unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn swap_chain_shifts_every_pose_one_step_including_rotation_and_side() {
    let m = model(vec![part("R1", "0603"), part("R2", "0603"), part("R3", "0603")], &[], vec![]);
    let mut b = board(&m);
    for (r, x) in [("R1", mm(10.0)), ("R2", mm(30.0)), ("R3", mm(50.0))] {
        b.apply(&Cmd::PlaceAt { part: r.into(), x, y: mm(20.0) }).unwrap();
    }
    b.apply(&Cmd::Rotate { part: "R2".into(), quarter_turns: 1 }).unwrap();
    b.apply(&Cmd::Flip { part: "R3".into() }).unwrap();
    let before: Vec<_> = ["R1", "R2", "R3"].iter().map(|r| b.pose_of(r).unwrap().clone()).collect();

    b.apply(&Cmd::SwapChain { parts: vec!["R1".into(), "R2".into(), "R3".into()] }).unwrap();
    // source: swap(R1,R2) then swap(R2,R3) -> R1 takes R2's pose, R2 takes R3's, R3 takes R1's.
    for (r, src) in [("R1", 1), ("R2", 2), ("R3", 0)] {
        let p = b.pose_of(r).unwrap();
        assert_eq!((p.at, p.rot, p.side), (before[src].at, before[src].rot, before[src].side), "{r}");
    }
}

#[test]
fn swap_chain_of_two_is_a_plain_pose_swap_and_refuses_bad_input() {
    let m = model(vec![part("R1", "0603"), part("R2", "0603")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "R1".into(), x: mm(10.0), y: mm(10.0) }).unwrap();
    b.apply(&Cmd::PlaceAt { part: "R2".into(), x: mm(40.0), y: mm(30.0) }).unwrap();
    let (a, c) = (b.pose_of("R1").unwrap().at, b.pose_of("R2").unwrap().at);
    b.apply(&Cmd::SwapChain { parts: vec!["R1".into(), "R2".into()] }).unwrap();
    assert_eq!((b.pose_of("R1").unwrap().at, b.pose_of("R2").unwrap().at), (c, a));

    assert_eq!(b.apply(&Cmd::SwapChain { parts: vec!["R1".into()] }).unwrap_err()[0].check, "ops_swap_chain");
    assert_eq!(b.apply(&Cmd::SwapChain { parts: vec!["R1".into(), "R1".into()] }).unwrap_err()[0].check, "ops_swap_self");
    assert_eq!(b.apply(&Cmd::SwapChain { parts: vec!["R1".into(), "R9".into()] }).unwrap_err()[0].check, "ops_not_placed");
}

fn square(x0: Nm, y0: Nm, x1: Nm, y1: Nm) -> Vec<Point> {
    vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }]
}

fn zone_area(z: &Zone) -> f64 {
    let n = z.outline.len();
    (0..n).map(|i| (z.outline[i].x.as_f64()) * (z.outline[(i + 1) % n].y.as_f64()) - (z.outline[(i + 1) % n].x.as_f64()) * (z.outline[i].y.as_f64())).sum::<f64>().abs() / 2.0
}

#[test]
fn zone_cutout_across_an_edge_trims_the_zone_and_keeps_its_settings() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: square(Nm::ZERO, Nm::ZERO, mm(10.0), mm(10.0)) }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.clearance = mm(0.777))).unwrap();

    b.apply(&Cmd::ZoneCutout { id, cutout: square(mm(6.0), mm(-1.0), mm(11.0), mm(11.0)) }).unwrap();
    let zones = &b.design().routing.as_ref().unwrap().zones;
    assert_eq!(zones.len(), 1);
    assert!((zone_area(&zones[0]) - 60_000_000_000_000.0).abs() < 1e6, "10x10mm minus the right 4mm strip");
    assert_eq!((zones[0].net.as_str(), zones[0].layer.as_str(), zones[0].clearance), ("GND", "F.Cu", mm(0.777)));
}

#[test]
fn zone_cutout_that_splits_the_zone_makes_one_zone_per_piece() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: square(Nm::ZERO, Nm::ZERO, mm(10.0), mm(10.0)) }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    b.apply(&Cmd::ZoneCutout { id, cutout: square(mm(4.0), mm(-1.0), mm(6.0), mm(11.0)) }).unwrap();
    let zones = &b.design().routing.as_ref().unwrap().zones;
    assert_eq!(zones.len(), 2, "source makes one new ZONE per main outline");
    assert!(zones.iter().all(|z| (zone_area(z) - 40_000_000_000_000.0).abs() < 1e6));
    assert_ne!(zones[0].id, zones[1].id);
}

#[test]
fn zone_cutout_inside_the_zone_is_a_hole_of_the_zone_with_the_right_area() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: square(Nm::ZERO, Nm::ZERO, mm(10.0), mm(10.0)) }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    b.apply(&Cmd::ZoneCutout { id, cutout: square(mm(3.0), mm(3.0), mm(5.0), mm(5.0)) }).unwrap();
    let zones = &b.design().routing.as_ref().unwrap().zones;
    assert_eq!(zones.len(), 1);
    // `performZoneCutout`: the zone keeps the outline, and the cutout is a hole of it (`newZoneOutline->AddHole`), not fractured into the outline.
    assert_eq!(zones[0].outline.len(), 4, "the outline as it was: {:?}", zones[0].outline);
    assert_eq!(zones[0].holes.len(), 1);
    let hole = Zone { outline: zones[0].holes[0].clone(), ..Default::default() };
    assert!((zone_area(&zones[0]) - zone_area(&hole) - 96_000_000_000_000.0).abs() < 1e6, "100 - 4 mm^2");
}

#[test]
fn zone_cutout_refuses_a_disjoint_cutout_a_degenerate_one_and_an_unknown_zone() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: square(Nm::ZERO, Nm::ZERO, mm(10.0), mm(10.0)) }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    let e = b.apply(&Cmd::ZoneCutout { id: id.clone(), cutout: square(mm(20.0), mm(20.0), mm(30.0), mm(30.0)) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");
    let e = b.apply(&Cmd::ZoneCutout { id: id.clone(), cutout: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1), y: Nm::new(1) }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");
    let e = b.apply(&Cmd::ZoneCutout { id: "zone_nope".into(), cutout: square(Nm::ZERO, Nm::ZERO, mm(1.0), mm(1.0)) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_zone");
    assert_eq!(b.design().routing.as_ref().unwrap().zones.len(), 1, "a refused cutout leaves the zone alone");
}

#[test]
fn batch_applies_every_sub_command_and_rolls_back_whole_on_a_refusal() {
    let m = model(vec![part("R1", "0603"), part("R2", "0603")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "R1".into(), x: mm(10.0), y: mm(10.0) }).unwrap();
    b.apply(&Cmd::PlaceAt { part: "R2".into(), x: mm(40.0), y: mm(30.0) }).unwrap();
    b.apply(&Cmd::Batch { cmds: vec![Cmd::Rotate { part: "R1".into(), quarter_turns: 1 }, Cmd::Rotate { part: "R2".into(), quarter_turns: 1 }] }).unwrap();
    assert_eq!(b.pose_of("R1").unwrap().rot, b.pose_of("R2").unwrap().rot);
    let r1 = b.pose_of("R1").unwrap().clone();
    // second sub-command is refused -> the first one must not stick
    let e = b.apply(&Cmd::Batch { cmds: vec![Cmd::Rotate { part: "R1".into(), quarter_turns: 1 }, Cmd::Rotate { part: "R9".into(), quarter_turns: 1 }] });
    assert!(e.is_err());
    assert_eq!(b.pose_of("R1").unwrap().rot, r1.rot, "a refused batch changes nothing");
    assert!(Cmd::Batch { cmds: vec![Cmd::Flip { part: "R1".into() }] }.clears_routing());
    let j: Cmd = serde_json::from_str(r#"{"op":"batch"}"#).unwrap();
    assert!(matches!(j, Cmd::Batch { ref cmds } if cmds.is_empty()));
}

#[test]
fn a_placement_snaps_to_the_grid_anchored_at_the_grid_origin() {
    // The board snaps to 100 um steps. With no grid origin a position lands on whole steps from (0, 0); with one at (50, 30) it lands on whole steps
    // from there, so a point the editor snapped to the grid it draws is the point the verb keeps.
    let m = model(vec![part("R1", "0603")], &[], vec![]);
    let mut plain = board(&m);
    plain.apply(&Cmd::PlaceAt { part: "R1".into(), x: mm(10.234), y: mm(10.678) }).unwrap();
    assert_eq!((plain.pose_of("R1").unwrap().at.x, plain.pose_of("R1").unwrap().at.y), (mm(10.2), mm(10.7)), "no grid origin: whole steps from (0, 0)");

    let mut b = board(&m);
    b.apply(&Cmd::SetGridOrigin { at: Some(Point { x: mm(0.05), y: mm(0.03) }) }).unwrap();
    b.apply(&Cmd::PlaceAt { part: "R1".into(), x: mm(10.234), y: mm(10.678) }).unwrap();
    assert_eq!((b.pose_of("R1").unwrap().at.x, b.pose_of("R1").unwrap().at.y), (mm(10.25), mm(10.63)), "a grid origin at (50, 30): 10234 -> 10250, 10678 -> 10630");
    b.apply(&Cmd::SetGridOrigin { at: None }).unwrap();
    assert!(b.design().drawings.as_ref().and_then(|d| d.grid_origin).is_none());
}

/// `Cmd::CommitRoute`'s `via_kinds` read from the JSON the studio and the route API send (`[[index, "blind"]]`, numbers inside a tagged enum: the
/// `arbitrary_precision` reader), and the via it names gets its type in `via_extras`; a removed via's extras go with it.
#[test]
fn commit_route_gives_its_vias_their_type_and_drops_the_extras_of_removed_vias() {
    let m = net_model();
    let mut b = board(&m);
    let cmd: Cmd = serde_json::from_value(serde_json::json!({
        "op": "commit_route",
        "vias": [
            { "net": "GND", "at": { "x": 1_000_000, "y": 0 }, "drill": 300_000, "diameter": 600_000, "from_layer": "F.Cu", "to_layer": "B.Cu" },
            { "net": "GND", "at": { "x": 3_000_000, "y": 0 }, "drill": 100_000, "diameter": 300_000, "from_layer": "F.Cu", "to_layer": "B.Cu" }
        ],
        "via_kinds": [[1, "micro"]]
    }))
    .expect("the JSON reads");
    b.apply(&cmd).unwrap();
    let vias = b.design().routing.as_ref().unwrap().vias.clone();
    let micro = vias.iter().find(|v| v.at.x == mm(3.0)).unwrap().id.clone();
    let through = vias.iter().find(|v| v.at.x == mm(1.0)).unwrap().id.clone();
    let extras = &b.design().drawings.as_ref().unwrap().via_extras;
    assert_eq!(extras.len(), 1);
    assert_eq!((extras[0].id.as_str(), extras[0].kind), (micro.as_str(), eda_model::via_extra::ViaKind::Micro));
    assert!(b.design().drawings.as_ref().unwrap().via_extra(&through).is_none());
    b.apply(&Cmd::CommitRoute { remove_track_ids: vec![], remove_via_ids: vec![micro], tracks: vec![], vias: vec![], lock_tracks: vec![], via_kinds: vec![] }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().via_extras.is_empty(), "the removed via's type went with it");
}

// ------------------------------------------------------- the board's real outline (p12 O10)

/// `AR_AUTOPLACER` works from `GetBoardPolygonOutlines`: a part is not put in a cutout, and a board of two pieces takes a part on either.
#[test]
fn place_at_refuses_a_cutout() {
    let m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[], vec![]);
    let mut d = empty_design();
    d.drawings = Some(eda_model::ir::DrawingsSection {
        shapes: vec![
            Shape::Rect { id: "shp_edge".into(), layer: "Edge.Cuts".into(), stroke_width: mm(0.1), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(100.0), y: mm(100.0) } },
            // a round cutout of 10 mm radius in the middle
            Shape::Circle { id: "shp_hole".into(), layer: "Edge.Cuts".into(), stroke_width: mm(0.1), filled: false, center: Point { x: mm(50.0), y: mm(50.0) }, end: Point { x: mm(60.0), y: mm(50.0) } },
        ],
        // the shapes are the outline (`placement.outline` is their summary, the box)
        outline_is_shapes: true,
        ..Default::default()
    });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let e = b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(50.0), y: mm(50.0) }).unwrap_err();
    assert_eq!(e[0].check, "ops_outside_board");
    // half over the cutout's edge is refused too
    assert!(b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(60.0), y: mm(50.0) }).is_err());
    // clear of it, it lands
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(20.0), y: mm(20.0) }).unwrap();
    // Place in the centre region spirals out of the cutout
    b.apply(&Cmd::PlaceRegion { part: "U2".into(), region: Region::Centre }).unwrap();
    let at = b.pose_of("U2").unwrap().at;
    let (dx, dy) = ((at.x - mm(50.0)).as_f64(), (at.y - mm(50.0)).as_f64());
    assert!(dx.hypot(dy) > 10_000_000.0, "{at:?} is in the cutout");
}

#[test]
fn a_board_of_two_pieces_takes_a_part_on_its_smaller_piece() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut d = empty_design();
    let rect = |id: &str, x0, y0, x1, y1| Shape::Rect { id: id.into(), layer: "Edge.Cuts".into(), stroke_width: mm(0.1), filled: false, start: Point { x: x0, y: y0 }, end: Point { x: x1, y: y1 } };
    d.drawings = Some(eda_model::ir::DrawingsSection { shapes: vec![rect("shp_a", Nm::ZERO, Nm::ZERO, mm(60.0), mm(100.0)), rect("shp_b", mm(70.0), Nm::ZERO, mm(100.0), mm(30.0))], outline_is_shapes: true, ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    // between the pieces is off the board
    assert!(b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(65.0), y: mm(15.0) }).is_err());
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: mm(85.0), y: mm(15.0) }).unwrap();
}
