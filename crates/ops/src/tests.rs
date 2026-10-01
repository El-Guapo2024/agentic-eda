use super::*;
use eda_model::ir::{PlacementSection, Provenance};
use eda_model::{Net, Part, Pin, PinKind, PlacementRule};

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
        body_um: None, symbol: None, datasheet: None,
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
        footprint_library: None, sheet_contents: None, bus_aliases: vec![],
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None, nets: None,
        routing: None,
        placement: Some(PlacementSection {
            outline: vec![
                Point { x: 0, y: 0 },
                Point { x: 100_000, y: 0 },
                Point { x: 100_000, y: 100_000 },
                Point { x: 0, y: 100_000 },
            ],
            footprints: Vec::new(),
            modules: Vec::new(),
        }),
        drawings: None,
    }
}

fn board(m: &ConstraintModel) -> Board<'_> {
    Board::new(empty_design(), m, 100, 300)
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
    let fails: Vec<_> = b
        .checks()
        .into_iter()
        .filter(|c| c.check == "placement_courtyard_overlap" && matches!(c.status, CheckStatus::Fail))
        .collect();
    assert!(fails.is_empty(), "sliding must not stack parts: {fails:?}");
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
    assert!((40_000..=60_000).contains(&at.x), "centre region should be mid-board, got {at:?}");
    assert!((40_000..=60_000).contains(&at.y), "centre region should be mid-board, got {at:?}");
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
    let fails: Vec<_> = b
        .checks()
        .into_iter()
        .filter(|c| c.check == "placement_courtyard_overlap" && matches!(c.status, CheckStatus::Fail))
        .collect();
    assert!(fails.is_empty(), "{fails:?}");
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
    let e = b.apply(&Cmd::AddTrack { net: "NOPE".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_net");

    let e = b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "In1.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_layer");

    let e = b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_track");
}

#[test]
fn add_track_assigns_an_id_that_set_track_width_and_delete_track_can_use() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
    assert!(!id.is_empty(), "a track must get an id the moment it is added");

    b.apply(&Cmd::SetTrackWidth { id: id.clone(), width: 350 }).unwrap();
    let t = &b.design().routing.as_ref().unwrap().tracks[0];
    assert_eq!(t.width, 350);
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
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 2000 }, Point { x: 1000, y: 2000 }] }).unwrap();
    let ids: std::collections::BTreeSet<String> = b.design().routing.as_ref().unwrap().tracks.iter().map(|t| t.id.clone()).collect();
    assert_eq!(ids.len(), 2, "two different tracks must get two different ids");
}

// --------------------------------------------------------------- vias

#[test]
fn add_via_rejects_a_drill_not_smaller_than_its_pad() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::AddVia { net: "GND".into(), x: 5000, y: 5000, drill: 600, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
    let e = b.apply(&Cmd::AddVia { net: "GND".into(), x: 5000, y: 5000, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "F.Cu".into() }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
}

#[test]
fn move_via_changes_position_but_not_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: 5000, y: 5000, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().vias[0].id.clone();

    b.apply(&Cmd::MoveVia { id: id.clone(), x: 9000, y: 9000 }).unwrap();
    let v = &b.design().routing.as_ref().unwrap().vias[0];
    assert_eq!(v.at, Point { x: 9000, y: 9000 });
    assert_eq!(v.id, id);

    b.apply(&Cmd::DeleteVia { id: id.clone() }).unwrap();
    assert!(b.design().routing.as_ref().unwrap().vias.is_empty());
    assert_eq!(b.apply(&Cmd::MoveVia { id, x: 0, y: 0 }).unwrap_err()[0].check, "ops_unknown_via");
}

#[test]
fn edit_via_changes_diameter_and_drill_but_not_net_or_position() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: 5000, y: 5000, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().vias[0].id.clone();

    b.apply(&Cmd::EditVia { id: id.clone(), diameter: 800, drill: 400 }).unwrap();
    let v = &b.design().routing.as_ref().unwrap().vias[0];
    assert_eq!(v.id, id);
    assert_eq!((v.diameter, v.drill), (800, 400));
    assert_eq!(v.net, "GND");
    assert_eq!(v.at, Point { x: 5000, y: 5000 });

    let e = b.apply(&Cmd::EditVia { id: id.clone(), diameter: 400, drill: 400 }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via", "drill not smaller than diameter must be refused, same as AddVia");
    // The refused edit must not have applied halfway.
    assert_eq!(b.design().routing.as_ref().unwrap().vias[0].diameter, 800);

    assert_eq!(b.apply(&Cmd::EditVia { id: "via_nope".into(), diameter: 600, drill: 300 }).unwrap_err()[0].check, "ops_unknown_via");
}

#[test]
fn track_width_and_via_presets_round_trip_and_reject_bad_entries() {
    let m = net_model();
    let mut b = board(&m);
    assert!(b.design().routing.is_none(), "an empty board starts with no routing section at all");

    b.apply(&Cmd::SetTrackWidthPresets { widths: vec![150, 250, 400] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().track_width_presets, vec![150, 250, 400]);

    b.apply(&Cmd::SetViaPresets { presets: vec![ViaPreset { diameter: 600, drill: 300 }, ViaPreset { diameter: 800, drill: 400 }] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().via_presets.len(), 2);

    let e = b.apply(&Cmd::SetTrackWidthPresets { widths: vec![200, 0] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_track");
    // A refused replace must leave the previous list in place.
    assert_eq!(b.design().routing.as_ref().unwrap().track_width_presets, vec![150, 250, 400]);

    let e = b.apply(&Cmd::SetViaPresets { presets: vec![ViaPreset { diameter: 300, drill: 300 }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
}

// ------------------------------------------- global edit: tracks & vias

#[test]
fn edit_tracks_and_vias_sets_explicit_width_diameter_drill_and_layer() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap();
    b.apply(&Cmd::AddVia { net: "GND".into(), x: 5000, y: 5000, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    let track_id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
    let via_id = b.design().routing.as_ref().unwrap().vias[0].id.clone();

    b.apply(&Cmd::EditTracksAndVias {
        ids: vec![track_id.clone(), via_id.clone()],
        track_width: Some(SizeSpec::Value { um: 350 }),
        via_size: Some(ViaSizeSpec::Value { diameter: 900, drill: 450 }),
        layer: Some("B.Cu".into()),
    })
    .unwrap();

    let t = &b.design().routing.as_ref().unwrap().tracks[0];
    assert_eq!(t.width, 350);
    assert_eq!(t.layer, "B.Cu", "a track's layer is editable; a via's layer span is not touched by this command");
    assert_eq!(t.id, track_id, "style/layer edits must not move the id");
    let v = &b.design().routing.as_ref().unwrap().vias[0];
    assert_eq!((v.diameter, v.drill), (900, 450));
    assert_eq!((v.from_layer.as_str(), v.to_layer.as_str()), ("F.Cu", "B.Cu"));
}

#[test]
fn edit_tracks_and_vias_net_class_resolves_each_items_own_net() {
    let mut m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[("FAST", &["U1"]), ("SLOW", &["U2"])], vec![]);
    m.board.net_classes = vec![eda_model::NetClass { name: "fast".into(), nets: vec!["FAST".into()], track_width: Some(500), clearance: None, via_diameter: Some(1000), via_drill: Some(500), microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }];
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "FAST".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap();
    b.apply(&Cmd::AddTrack { net: "SLOW".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 2000 }, Point { x: 1000, y: 2000 }] }).unwrap();
    let ids: Vec<String> = b.design().routing.as_ref().unwrap().tracks.iter().map(|t| t.id.clone()).collect();

    b.apply(&Cmd::EditTracksAndVias { ids, track_width: Some(SizeSpec::NetClass), via_size: None, layer: None }).unwrap();

    let widths: std::collections::BTreeMap<String, Um> = b.design().routing.as_ref().unwrap().tracks.iter().map(|t| (t.net.clone(), t.width)).collect();
    assert_eq!(widths["FAST"], 500, "FAST is in the fast net class, which overrides track width");
    assert_eq!(widths["SLOW"], m.board.track_width, "SLOW has no class, so it falls back to the board default");
}

#[test]
fn edit_tracks_and_vias_refuses_empty_ids_and_bad_explicit_values() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::EditTracksAndVias { ids: vec![], track_width: Some(SizeSpec::NetClass), via_size: None, layer: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_global_edit");

    let e = b.apply(&Cmd::EditTracksAndVias { ids: vec!["whatever".into()], track_width: Some(SizeSpec::Value { um: 0 }), via_size: None, layer: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_track");

    let e = b.apply(&Cmd::EditTracksAndVias { ids: vec!["whatever".into()], track_width: None, via_size: Some(ViaSizeSpec::Value { diameter: 300, drill: 300 }), layer: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_via");
}

#[test]
fn edit_tracks_and_vias_tolerates_unknown_ids_among_known_ones() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();
    b.apply(&Cmd::EditTracksAndVias { ids: vec![id, "nope".into()], track_width: Some(SizeSpec::Value { um: 500 }), via_size: None, layer: None }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().tracks[0].width, 500);
}

// --------------------------------------------------------------- zones

#[test]
fn add_zone_then_delete_it() {
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }];
    let e = b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 1, y: 1 }] }).unwrap_err();
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
    let outline = vec![Point { x: 0, y: 0 }, Point { x: 5_000, y: 0 }, Point { x: 5_000, y: 5_000 }];
    b.apply(&Cmd::AddZone { net: "".into(), layer: "F.Cu".into(), outline }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().zones[0].net, "");

    let e = b.apply(&Cmd::AddZone { net: "NOPE".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 1, y: 0 }, Point { x: 1, y: 1 }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_net", "a real but unknown net name must still be refused");
}

#[test]
fn edit_zone_keepout_flags_round_trip_and_empty_net_is_allowed() {
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: 0, y: 0 }, Point { x: 5_000, y: 0 }, Point { x: 5_000, y: 5_000 }];
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
    }
}

#[test]
fn edit_zone_replaces_every_setting_but_leaves_the_outline_alone() {
    let m = net_model();
    let mut b = board(&m);
    let outline = vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }];
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: outline.clone() }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();

    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| {
        z.priority = 3;
        z.clearance = 300;
        z.pad_connection = PadConnection::Full;
        z.fill_mode = FillMode::HatchPattern;
        z.hatch_thickness = 1000;
        z.hatch_gap = 1500;
    }))
    .unwrap();

    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!(z.id, id, "id is stable across an edit");
    assert_eq!(z.outline, outline, "EditZone never touches the outline");
    assert_eq!(z.priority, 3);
    assert_eq!(z.clearance, 300);
    assert_eq!(z.pad_connection, PadConnection::Full);
    assert_eq!(z.fill_mode, FillMode::HatchPattern);
}

#[test]
fn edit_zone_rejects_a_thermal_spoke_narrower_than_the_minimum_width() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();

    let e = b
        .apply(&edit_zone_cmd(id, "GND", |z| {
            z.min_thickness = 500;
            z.thermal_spoke_width = 100;
        }))
        .unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");
}

#[test]
fn edit_zone_rejects_an_unknown_net_or_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }] }).unwrap();
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
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }] }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    b.apply(&edit_zone_cmd(id.clone(), "GND", |z| z.priority = 5)).unwrap();

    let new_outline = vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }];
    b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: new_outline.clone() }).unwrap();
    let z = &b.design().routing.as_ref().unwrap().zones[0];
    assert_eq!(z.id, id, "id is stable across an outline edit");
    assert_eq!(z.outline, new_outline);
    assert_eq!(z.priority, 5, "SetZoneOutline must not touch any other setting");

    let e = b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: vec![Point { x: 0, y: 0 }, Point { x: 1, y: 1 }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_zone");
    assert_eq!(b.apply(&Cmd::SetZoneOutline { id: "zone_nope".into(), outline: new_outline }).unwrap_err()[0].check, "ops_unknown_zone");
}

// ----------------------------------------------------------- teardrops

fn via_and_track_board() -> (ConstraintModel, Point, Point) {
    let m = net_model();
    let via_at = Point { x: 40_000, y: 40_000 };
    let far = Point { x: via_at.x + 5000, y: via_at.y };
    (m, via_at, far)
}

#[test]
fn add_all_teardrops_is_a_no_op_when_disabled() {
    let (m, via_at, far) = via_and_track_board();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: via_at.x, y: via_at.y, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![via_at, far] }).unwrap();

    b.apply(&Cmd::AddAllTeardrops).unwrap();
    assert!(b.design().routing.as_ref().unwrap().zones.is_empty(), "teardrop_settings.enabled defaults false");
}

#[test]
fn add_all_teardrops_generates_once_and_is_idempotent() {
    let (m, via_at, far) = via_and_track_board();
    let mut b = board(&m);
    b.apply(&Cmd::AddVia { net: "GND".into(), x: via_at.x, y: via_at.y, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![via_at, far] }).unwrap();
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
    b.apply(&Cmd::AddVia { net: "GND".into(), x: via_at.x, y: via_at.y, drill: 300, diameter: 800, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![via_at, far] }).unwrap();
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }] }).unwrap();
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
    let shape = Shape::Segment { id: "ignored-on-input".into(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 0 } };
    b.apply(&Cmd::AddShape { shape }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();
    assert_ne!(id, "ignored-on-input", "a caller-supplied id must never be trusted");
    assert!(!id.is_empty());

    b.apply(&Cmd::MoveShape { id: id.clone(), dx: 500, dy: -500 }).unwrap();
    let moved = &b.design().drawings.as_ref().unwrap().shapes[0];
    assert_eq!(moved.points(), vec![Point { x: 500, y: -500 }, Point { x: 1500, y: -500 }]);

    b.apply(&Cmd::DeleteShape { id: id.clone() }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().shapes.is_empty());
    assert_eq!(b.apply(&Cmd::DeleteShape { id }).unwrap_err()[0].check, "ops_unknown_shape");
}

#[test]
fn add_shape_rejects_a_short_polygon() {
    let m = net_model();
    let mut b = board(&m);
    let shape = Shape::Polygon { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: true, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] };
    let e = b.apply(&Cmd::AddShape { shape }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");
}

#[test]
fn edit_shape_changes_layer_width_and_filled_but_not_geometry() {
    let m = net_model();
    let mut b = board(&m);
    let shape = Shape::Rect { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 1000 } };
    b.apply(&Cmd::AddShape { shape }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();

    b.apply(&Cmd::EditShape { id: id.clone(), layer: "F.Fab".into(), stroke_width: 300, filled: true }).unwrap();
    let s = &b.design().drawings.as_ref().unwrap().shapes[0];
    assert_eq!(s.layer(), "F.Fab");
    assert_eq!(s.points(), vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 1000 }], "EditShape never touches geometry");
    let Shape::Rect { stroke_width, filled, .. } = s else { panic!("still a rect") };
    assert_eq!((*stroke_width, *filled), (300, true));

    let e = b.apply(&Cmd::EditShape { id: id.clone(), layer: "".into(), stroke_width: 300, filled: true }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");
    let e = b.apply(&Cmd::EditShape { id, layer: "F.Fab".into(), stroke_width: 0, filled: true }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");

    assert_eq!(b.apply(&Cmd::EditShape { id: "shape_nope".into(), layer: "F.Fab".into(), stroke_width: 100, filled: false }).unwrap_err()[0].check, "ops_unknown_shape");
}

// --------------------------------------------------------------- text

#[test]
fn add_edit_move_delete_text() {
    let m = net_model();
    let mut b = board(&m);
    let text = Text { id: String::new(), content: "REV A".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false };
    b.apply(&Cmd::AddText { text }).unwrap();
    let id = b.design().drawings.as_ref().unwrap().texts[0].id.clone();
    assert!(!id.is_empty());

    b.apply(&Cmd::EditText { id: id.clone(), content: "REV B".into(), angle: 90_000, layer: "F.Fab".into(), size_um: 1200, stroke_width: 200, justify: TextJustify::Left, mirror: true }).unwrap();
    let t = &b.design().drawings.as_ref().unwrap().texts[0];
    assert_eq!(t.content, "REV B");
    assert_eq!(t.layer, "F.Fab");
    assert_eq!(t.justify, TextJustify::Left);
    assert!(t.mirror);
    assert_eq!(t.id, id, "editing style must not move the id");

    b.apply(&Cmd::MoveText { id: id.clone(), x: 4000, y: 4000 }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().texts[0].at, Point { x: 4000, y: 4000 });

    b.apply(&Cmd::DeleteText { id: id.clone() }).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().texts.is_empty());
    assert_eq!(b.apply(&Cmd::EditText { id, content: String::new(), angle: 0, layer: "F.Fab".into(), size_um: 100, stroke_width: 10, justify: TextJustify::Center, mirror: false }).unwrap_err()[0].check, "ops_unknown_text");
}

// ------------------------------------- global edit: text & graphics

#[test]
fn edit_text_and_graphics_touches_only_the_fields_given() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 100, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 0 } } }).unwrap();
    b.apply(&Cmd::AddText { text: Text { id: String::new(), content: "REF".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false } }).unwrap();
    let shape_id = b.design().drawings.as_ref().unwrap().shapes[0].id().to_string();
    let text_id = b.design().drawings.as_ref().unwrap().texts[0].id.clone();

    // Only a line-width change for the shape -- its layer must be untouched.
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![shape_id.clone()], text_ids: vec![], layer: None, line_width: Some(250), text_size: None, text_thickness: None }).unwrap();
    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.shapes[0].layer(), "F.SilkS");
    assert_eq!(dr.shapes[0].id(), shape_id, "a style edit must not move the id");
    match &dr.shapes[0] {
        Shape::Segment { stroke_width, .. } => assert_eq!(*stroke_width, 250),
        other => panic!("expected a Segment, got {other:?}"),
    }

    // Layer + text size/thickness for the text, same command touching both kinds at once.
    b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![shape_id.clone()], text_ids: vec![text_id.clone()], layer: Some("F.Fab".into()), line_width: None, text_size: Some(1200), text_thickness: Some(200) })
        .unwrap();
    let dr = b.design().drawings.as_ref().unwrap();
    assert_eq!(dr.shapes[0].layer(), "F.Fab", "the shared `layer` field must still reach the shape on a mixed call");
    let t = dr.texts.iter().find(|t| t.id == text_id).unwrap();
    assert_eq!(t.layer, "F.Fab");
    assert_eq!(t.size_um, 1200);
    assert_eq!(t.stroke_width, 200);
    assert_eq!(t.content, "REF", "content is not one of this command's fields");
}

#[test]
fn edit_text_and_graphics_refuses_empty_input_and_bad_values() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec![], layer: None, line_width: None, text_size: None, text_thickness: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_global_edit");

    let e = b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec!["x".into()], text_ids: vec![], layer: None, line_width: Some(0), text_size: None, text_thickness: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_shape");

    let e = b.apply(&Cmd::EditTextAndGraphics { shape_ids: vec![], text_ids: vec!["x".into()], layer: None, line_width: None, text_size: Some(-5), text_thickness: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_text");
}

// ---------------------------------------------------- clears_routing

#[test]
fn only_part_edits_and_flip_clear_routing() {
    let copper_and_drawing_cmds = [
        Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![] },
        Cmd::DeleteTrack { id: "x".into() },
        Cmd::SetTrackWidth { id: "x".into(), width: 200 },
        Cmd::AddVia { net: "GND".into(), x: 0, y: 0, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() },
        Cmd::DeleteVia { id: "x".into() },
        Cmd::MoveVia { id: "x".into(), x: 0, y: 0 },
        Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![] },
        Cmd::DeleteZone { id: "x".into() },
        Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 100, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 0, y: 0 } } },
        Cmd::DeleteShape { id: "x".into() },
        Cmd::MoveShape { id: "x".into(), dx: 0, dy: 0 },
        Cmd::AddText { text: Text { id: String::new(), content: String::new(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.SilkS".into(), size_um: 100, stroke_width: 10, justify: TextJustify::Center, mirror: false } },
        Cmd::EditText { id: "x".into(), content: String::new(), angle: 0, layer: "F.SilkS".into(), size_um: 100, stroke_width: 10, justify: TextJustify::Center, mirror: false },
        Cmd::DeleteText { id: "x".into() },
        Cmd::MoveText { id: "x".into(), x: 0, y: 0 },
        Cmd::Duplicate { ids: vec!["x".into()] },
        Cmd::PasteItems { tracks: vec![], vias: vec![], zones: vec![], shapes: vec![], texts: vec![] },
        Cmd::EditTracksAndVias { ids: vec!["x".into()], track_width: None, via_size: None, layer: None },
        Cmd::EditTextAndGraphics { shape_ids: vec!["x".into()], text_ids: vec![], layer: None, line_width: None, text_size: None, text_thickness: None },
    ];
    for c in &copper_and_drawing_cmds {
        assert!(!c.clears_routing(), "{c:?} must not clear routing -- only part edits do");
    }

    let part_edit_cmds = [
        Cmd::Place { part: "U1".into(), anchor: "U2".into(), side: Dir::North },
        Cmd::PlaceEdge { part: "U1".into(), edge: Dir::North, fraction: 0.5 },
        Cmd::PlaceRegion { part: "U1".into(), region: Region::Centre },
        Cmd::PlaceAt { part: "U1".into(), x: 0, y: 0 },
        Cmd::MoveTo { part: "U1".into(), x: 0, y: 0 },
        Cmd::Nudge { part: "U1".into(), dir: Dir::North, steps: 1 },
        Cmd::Rotate { part: "U1".into(), quarter_turns: 1 },
        Cmd::Swap { a: "U1".into(), b: "U2".into() },
        Cmd::Rip { part: "U1".into() },
        Cmd::Flip { part: "U1".into() },
        Cmd::MoveExact { parts: vec!["U1".into()], dx: 0, dy: 0, rotate_millideg: 0, pivot: None },
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
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap();
    b.apply(&Cmd::AddVia { net: "GND".into(), x: 5000, y: 5000, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }] }).unwrap();
    b.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 0 } } }).unwrap();
    b.apply(&Cmd::AddText { text: Text { id: String::new(), content: "REV A".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false } }).unwrap();

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
    assert_eq!(new_track.pts, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }]);
    assert_ne!(new_track.id, track_id);
    assert!(!new_track.id.is_empty());

    let new_via = rt.vias.iter().find(|v| v.id != via_id).unwrap();
    assert_eq!(new_via.at, Point { x: 5000, y: 5000 });

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
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }).unwrap();
    let track_id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();

    b.apply(&Cmd::Duplicate { ids: vec![track_id, "U1".into(), "nonexistent".into()] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().tracks.len(), 2);
}

#[test]
fn paste_items_inserts_fresh_copies_and_ignores_incoming_ids() {
    let m = net_model();
    let mut b = board(&m);
    let track = Track { id: "stale-id-from-another-board".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 2000, y: 2000 }, Point { x: 3000, y: 2000 }] };
    let text = Text { id: "also-stale".into(), content: "PASTED".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false };

    b.apply(&Cmd::PasteItems { tracks: vec![track], vias: vec![], zones: vec![], shapes: vec![], texts: vec![text] }).unwrap();

    let rt = b.design().routing.as_ref().unwrap();
    assert_eq!(rt.tracks.len(), 1);
    assert_ne!(rt.tracks[0].id, "stale-id-from-another-board");
    assert!(!rt.tracks[0].id.is_empty());
    assert_eq!(rt.tracks[0].pts, vec![Point { x: 2000, y: 2000 }, Point { x: 3000, y: 2000 }]);

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
    b.apply(&Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 2500, y: -2000 }, Point { x: 2500, y: 2000 }] }).unwrap();
    let old_id = b.design().routing.as_ref().unwrap().tracks[0].id.clone();

    b.apply(&Cmd::CommitRoute {
        remove_track_ids: vec![old_id.clone()],
        remove_via_ids: vec![],
        tracks: vec![
            Track { id: "ignored".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }] },
            Track { id: "ignored2".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 2500, y: -2000 }, Point { x: 2200, y: 0 }, Point { x: 2500, y: 2000 }] },
        ],
        vias: vec![],
    })
    .unwrap();

    let tracks = &b.design().routing.as_ref().unwrap().tracks;
    assert_eq!(tracks.len(), 2, "the old track is gone, both new ones are present");
    assert!(tracks.iter().all(|t| t.id != old_id), "the removed id must not reappear");
    assert!(tracks.iter().all(|t| t.id != "ignored" && t.id != "ignored2"), "incoming ids are always reassigned, same as PasteItems");
    assert!(tracks.iter().any(|t| t.pts[0] == Point { x: 0, y: 0 }));
    assert!(tracks.iter().any(|t| t.pts.len() == 3), "the shoved track's new detour shape must survive");
}

#[test]
fn commit_route_tolerates_an_already_gone_id() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::CommitRoute { remove_track_ids: vec!["trk_doesnotexist".into()], remove_via_ids: vec![], tracks: vec![Track { id: String::new(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }], vias: vec![] }).unwrap();
    assert_eq!(b.design().routing.as_ref().unwrap().tracks.len(), 1);
}

#[test]
fn commit_route_rejects_an_unknown_net() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::CommitRoute { remove_track_ids: vec![], remove_via_ids: vec![], tracks: vec![Track { id: String::new(), net: "NOPE".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }] }], vias: vec![] }).unwrap_err();
    assert!(!e.is_empty());
    assert!(b.design().routing.is_none(), "a refused commit must not partially apply");
}

// ------------------------------------------------------- move exact

#[test]
fn move_exact_with_no_pivot_spins_a_single_part_in_place_around_its_own_anchor() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: 10_000, y: 10_000 }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into()], dx: 0, dy: 0, rotate_millideg: 90_000, pivot: None }).unwrap();
    let pose = b.pose_of("U1").unwrap();
    assert_eq!(pose.at, Point { x: 10_000, y: 10_000 }, "no pivot given: rotating about its own anchor must not move it");
    assert_eq!(pose.rot, 90_000);
}

#[test]
fn move_exact_translates_then_rotates_about_its_own_already_moved_anchor() {
    let m = model(vec![part("U1", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: 10_000, y: 10_000 }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into()], dx: 5000, dy: 1000, rotate_millideg: 45_000, pivot: None }).unwrap();
    let pose = b.pose_of("U1").unwrap();
    // Translation lands first, then the (pivot-less) rotation spins it in
    // place at that new position -- the position is exactly the
    // translation, regardless of the rotation angle.
    assert_eq!(pose.at, Point { x: 15_000, y: 11_000 });
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
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: 20_000, y: 10_000 }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into()], dx: 0, dy: 0, rotate_millideg: 90_000, pivot: Some(Point { x: 10_000, y: 10_000 }) }).unwrap();
    let pose = b.pose_of("U1").unwrap();
    assert_eq!(pose.at.x, 10_000, "rotated 90 degrees around the pivot, the +10000 x offset becomes 0 relative to it (within rounding)");
    assert_eq!(pose.at.y, 20_000);
    assert_eq!(pose.rot, 90_000, "orientation still turns by the same angle regardless of the pivot choice");
}

#[test]
fn move_exact_applies_the_same_translation_and_angle_to_every_part_in_the_batch() {
    let m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: 10_000, y: 10_000 }).unwrap();
    b.apply(&Cmd::PlaceAt { part: "U2".into(), x: 30_000, y: 10_000 }).unwrap();

    b.apply(&Cmd::MoveExact { parts: vec!["U1".into(), "U2".into()], dx: 1000, dy: 2000, rotate_millideg: 90_000, pivot: None }).unwrap();
    assert_eq!(b.pose_of("U1").unwrap().at, Point { x: 11_000, y: 12_000 });
    assert_eq!(b.pose_of("U2").unwrap().at, Point { x: 31_000, y: 12_000 });
    assert_eq!(b.pose_of("U1").unwrap().rot, 90_000);
    assert_eq!(b.pose_of("U2").unwrap().rot, 90_000);
}

#[test]
fn move_exact_refuses_an_unplaced_or_unknown_part_without_moving_the_others() {
    let m = model(vec![part("U1", "SOIC-8"), part("U2", "SOIC-8")], &[], vec![]);
    let mut b = board(&m);
    b.apply(&Cmd::PlaceAt { part: "U1".into(), x: 10_000, y: 10_000 }).unwrap();
    // U2 is never placed.
    let e = b.apply(&Cmd::MoveExact { parts: vec!["U1".into(), "U2".into()], dx: 1000, dy: 0, rotate_millideg: 0, pivot: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_not_placed");
    assert_eq!(b.pose_of("U1").unwrap().at, Point { x: 10_000, y: 10_000 }, "the whole batch refuses atomically -- U1 must not have moved either");
}

#[test]
fn move_exact_rejects_an_empty_part_list() {
    let m = net_model();
    let mut b = board(&m);
    let e = b.apply(&Cmd::MoveExact { parts: vec![], dx: 0, dy: 0, rotate_millideg: 0, pivot: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_move_exact");
}

// -------------------------------------------------- footprint editor (GAPS.md #8)

fn fp_pad(number: &str, x: Um, y: Um) -> LibraryPad {
    LibraryPad {
        id: String::new(),
        number: number.into(),
        at: Point { x, y },
        offset: Point { x: 0, y: 0 },
        size: (1000, 1000),
        shape: eda_model::ir::LibraryPadShape::RoundRect,
        kind: eda_model::PadKind::Smd,
        drill: None,
        drill_slot: None,
        rot: 0,
        roundrect_ratio: Some(0.25),
        trapezoid_delta: None,
        chamfer_ratio: None,
        chamfer_corners: eda_model::ir::ChamferCorners::default(),
        layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
        clearance_override: None,
        thermal_gap_override: None,
        thermal_spoke_width_override: None,
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
    b.apply(&Cmd::AddPad { footprint: "Test:Blank".into(), pad: fp_pad("1", 0, 0) }).unwrap();
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:Blank".into() }).unwrap();
    assert_eq!(b.design().footprint_library.as_ref().unwrap().by_name("Test:Blank").unwrap().pads.len(), 1);
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
    let mut pad = fp_pad("1", 0, 0);
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
    let e = b.apply(&Cmd::AddPad { footprint: "Nobody:Opened".into(), pad: fp_pad("1", 0, 0) }).unwrap_err();
    assert_eq!(e[0].check, "ops_unknown_footprint");
}

#[test]
fn add_pad_rejects_a_zero_sized_pad() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    let mut pad = fp_pad("1", 0, 0);
    pad.size = (0, 1000);
    let e = b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_pad");
}

#[test]
fn add_pad_reuses_footprint_validate_for_a_through_hole_with_no_drill() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    let mut pad = fp_pad("1", 0, 0);
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
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", 0, 0) }).unwrap();
    let id = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads[0].id.clone();

    b.apply(&Cmd::MovePad { footprint: "Test:FP".into(), id: id.clone(), x: 500, y: -500 }).unwrap();
    b.apply(&Cmd::RotatePad { footprint: "Test:FP".into(), id: id.clone(), quarter_turns: 1 }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.pads[0].at, Point { x: 500, y: -500 });
    assert_eq!(fp.pads[0].rot, 90_000);

    b.apply(&Cmd::DeletePad { footprint: "Test:FP".into(), id }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads.is_empty());
}

#[test]
fn rotate_pad_wraps_at_a_full_turn() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", 0, 0) }).unwrap();
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
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", 0, 0) }).unwrap();
    let id = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads[0].id.clone();

    let mut edited = fp_pad("1A", 1000, 2000);
    edited.size = (2000, 3000);
    b.apply(&Cmd::EditPad { footprint: "Test:FP".into(), id: id.clone(), pad: edited }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.pads[0].id, id, "the id names which pad to replace -- it never changes underneath the caller");
    assert_eq!(fp.pads[0].number, "1A");
    assert_eq!(fp.pads[0].size, (2000, 3000));
}

#[test]
fn next_pad_number_matches_what_add_pad_actually_produces() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    for n in 1..=3 {
        let next = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().next_pad_number();
        assert_eq!(next, n.to_string());
        b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad(&next, n as Um * 1000, 0) }).unwrap();
    }
}

#[test]
fn push_pad_properties_only_touches_filtered_matches() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    // Pad 1 (source): round, SMD. Pad 2: round, SMD (matches). Pad 3: rotated 90 (orientation mismatch).
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", 0, 0) }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("2", 1000, 0) }).unwrap();
    let mut pad3 = fp_pad("3", 2000, 0);
    pad3.rot = 90_000;
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: pad3 }).unwrap();

    let ids: Vec<String> = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().pads.iter().map(|p| p.id.clone()).collect();
    // Change pad 1's size, then push with the orientation filter on.
    b.apply(&Cmd::EditPad { footprint: "Test:FP".into(), id: ids[0].clone(), pad: { let mut p = fp_pad("1", 0, 0); p.size = (3000, 3000); p } }).unwrap();
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
    assert_eq!(by_id(&ids[1]).size, (3000, 3000), "pad 2 shares the source's orientation (0) -- it must receive the push");
    assert_eq!(by_id(&ids[2]).size, (1000, 1000), "pad 3 is rotated 90 degrees -- the orientation filter must exclude it");
    assert_eq!(by_id(&ids[2]).number, "3", "push never touches number/position");
}

#[test]
fn renumber_pads_orders_by_position_not_by_current_number() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    // Placed right-to-left but numbered in placement order (1, 2, 3) --
    // a reading-order renumber must reverse them to (10, 11, 12).
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", 4000, 0) }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("2", 2000, 0) }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("3", 0, 0) }).unwrap();

    b.apply(&Cmd::RenumberPads { footprint: "Test:FP".into(), start: 10, prefix: String::new(), step: 1 }).unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    let at_x = |x: Um| fp.pads.iter().find(|p| p.at.x == x).unwrap().number.clone();
    assert_eq!(at_x(0), "10");
    assert_eq!(at_x(2000), "11");
    assert_eq!(at_x(4000), "12");
}

#[test]
fn set_footprint_anchor_translates_every_pad_graphic_and_text_so_the_click_point_becomes_zero() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", 1000, 1000) }).unwrap();
    b.apply(&Cmd::AddFootprintGraphic {
        footprint: "Test:FP".into(),
        shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 100, filled: false, start: Point { x: 1000, y: 1000 }, end: Point { x: 2000, y: 1000 } },
    })
    .unwrap();
    b.apply(&Cmd::AddFootprintText { footprint: "Test:FP".into(), text: Text { id: String::new(), content: "REF**".into(), at: Point { x: 1000, y: 500 }, angle: 0, layer: "F.SilkS".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false } })
        .unwrap();

    b.apply(&Cmd::SetFootprintAnchor { name: "Test:FP".into(), at: Point { x: 1000, y: 1000 } }).unwrap();

    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.pads[0].at, Point { x: 0, y: 0 }, "the clicked point is now the origin");
    assert_eq!(fp.graphics[0].points()[0], Point { x: 0, y: 0 });
    assert_eq!(fp.texts[0].at, Point { x: 0, y: -500 });
}

#[test]
fn update_footprint_on_board_only_flips_the_explicit_flag() {
    let m = net_model();
    let mut b = board(&m);
    b.apply(&Cmd::OpenFootprintForEdit { name: "Test:FP".into() }).unwrap();
    assert!(!b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().published);
    b.apply(&Cmd::AddPad { footprint: "Test:FP".into(), pad: fp_pad("1", 0, 0) }).unwrap();
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
    })
    .unwrap();
    let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
    assert_eq!(fp.description, "A test footprint");
    assert!(fp.attributes.smd);
    assert!(!fp.reference_visible);
    assert_eq!(fp.model.as_deref(), Some("${KICAD10_3DMODEL_DIR}/x.step"));
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
        shape: Shape::Rect { id: "ignored".into(), layer: "F.Fab".into(), stroke_width: 100, filled: false, start: Point { x: 0, y: 0 }, end: Point { x: 1000, y: 1000 } },
    })
    .unwrap();
    let gid = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().graphics[0].id().to_string();
    assert_ne!(gid, "ignored");
    b.apply(&Cmd::MoveFootprintGraphic { footprint: "Test:FP".into(), id: gid.clone(), dx: 10, dy: 20 }).unwrap();
    b.apply(&Cmd::EditFootprintGraphic { footprint: "Test:FP".into(), id: gid.clone(), layer: "F.SilkS".into(), stroke_width: 200, filled: true }).unwrap();
    {
        let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
        assert_eq!(fp.graphics[0].layer(), "F.SilkS");
        assert_eq!(fp.graphics[0].points()[0], Point { x: 10, y: 20 });
    }
    b.apply(&Cmd::DeleteFootprintGraphic { footprint: "Test:FP".into(), id: gid }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().graphics.is_empty());

    b.apply(&Cmd::AddFootprintText { footprint: "Test:FP".into(), text: Text { id: String::new(), content: "VAL**".into(), at: Point { x: 0, y: 0 }, angle: 0, layer: "F.Fab".into(), size_um: 1000, stroke_width: 150, justify: TextJustify::Center, mirror: false } }).unwrap();
    let tid = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().texts[0].id.clone();
    b.apply(&Cmd::MoveFootprintText { footprint: "Test:FP".into(), id: tid.clone(), x: 500, y: 500 }).unwrap();
    b.apply(&Cmd::EditFootprintText { footprint: "Test:FP".into(), id: tid.clone(), content: "V2".into(), angle: 900, layer: "F.SilkS".into(), size_um: 800, stroke_width: 120, justify: TextJustify::Left, mirror: true }).unwrap();
    {
        let fp = b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap();
        assert_eq!(fp.texts[0].at, Point { x: 500, y: 500 });
        assert_eq!(fp.texts[0].content, "V2");
        assert!(fp.texts[0].mirror);
    }
    b.apply(&Cmd::DeleteFootprintText { footprint: "Test:FP".into(), id: tid }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().by_name("Test:FP").unwrap().texts.is_empty());
}

#[test]
fn footprint_editor_commands_are_their_own_undo_domain() {
    assert_eq!(Cmd::AddPad { footprint: "x".into(), pad: fp_pad("1", 0, 0) }.domain(), Domain::FootprintEditor);
    assert_eq!(Cmd::OpenFootprintForEdit { name: "x".into() }.domain(), Domain::FootprintEditor);
    assert_eq!(Cmd::UpdateFootprintOnBoard { name: "x".into() }.domain(), Domain::FootprintEditor);
    // Sanity: the other two domains are unaffected by this addition.
    assert_eq!(Cmd::MoveTo { part: "U1".into(), x: 0, y: 0 }.domain(), Domain::Pcb);
    assert_eq!(Cmd::AddWire { pts: vec![], bus: false }.domain(), Domain::Schematic);
}

// ------------------------------------------------------------ multi-unit symbols (GAPS.md #21)

fn add_unit(b: &mut Board<'_>, id: &str, unit: u32, x: Um) {
    b.apply(&Cmd::AddSymbol { id: id.into(), lib_id: "test:DUAL".into(), at: Point { x, y: 0 }, rot_millideg: 0, value: "DUAL".into(), footprint: String::new(), unit }).unwrap();
}

/// Placing a second unit of an *already-placed* reference (same `id`, a
/// different `unit`) is the supported way to add it -- see `Cmd::AddSymbol`'s
/// own doc -- and only an exact `(id, unit)` repeat is refused.
#[test]
fn add_symbol_places_additional_units_of_an_existing_reference() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    add_unit(&mut b, "U1", 1, 0);
    add_unit(&mut b, "U1", 2, 50_000);
    let sch = b.design().schematic.as_ref().unwrap();
    assert_eq!(sch.symbols.len(), 2);
    assert!(sch.symbols.iter().all(|s| s.id == "U1"));
    let units: std::collections::BTreeSet<u32> = sch.symbols.iter().map(|s| s.unit).collect();
    assert_eq!(units, std::collections::BTreeSet::from([1, 2]));

    let e = b.apply(&Cmd::AddSymbol { id: "U1".into(), lib_id: "test:DUAL".into(), at: Point { x: 99_000, y: 0 }, rot_millideg: 0, value: String::new(), footprint: String::new(), unit: 1 }).unwrap_err();
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
    add_unit(&mut b, "U1", 1, 0);
    add_unit(&mut b, "U1", 2, 50_000);

    let e = b.apply(&Cmd::MoveSymbol { id: "U1".into(), x: 1_000, y: 1_000, unit: None }).unwrap_err();
    assert_eq!(e[0].check, "ops_ambiguous_symbol");

    b.apply(&Cmd::MoveSymbol { id: "U1".into(), x: 1_000, y: 2_000, unit: Some(2) }).unwrap();
    let sch = b.design().schematic.as_ref().unwrap();
    let unit1 = sch.symbols.iter().find(|s| s.unit == 1).unwrap();
    let unit2 = sch.symbols.iter().find(|s| s.unit == 2).unwrap();
    assert_eq!(unit1.at, Point { x: 0, y: 0 }, "unit 1 untouched by a move scoped to unit 2");
    assert_eq!(unit2.at, Point { x: 1_000, y: 2_000 });

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
    add_unit(&mut b, "U1", 1, 0);
    b.apply(&Cmd::MoveSymbol { id: "U1".into(), x: 5_000, y: 6_000, unit: None }).unwrap();
    assert_eq!(b.design().schematic.as_ref().unwrap().symbols[0].at, Point { x: 5_000, y: 6_000 });
}

/// Reference/Value/Footprint/Datasheet are part-wide: editing or renaming
/// touches every placed unit together, never just one of them.
#[test]
fn edit_and_rename_apply_to_every_unit_together() {
    let m = model(vec![], &[], vec![]);
    let mut b = board(&m);
    add_unit(&mut b, "U1", 1, 0);
    add_unit(&mut b, "U1", 2, 50_000);

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
