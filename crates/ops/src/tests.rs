use super::*;
use eda_model::ir::{PlacementSection, Provenance};
use eda_model::{Net, Part, Pin, PinKind, PlacementRule};

fn part(r: &str, pkg: &str) -> Part {
    Part {
        reference: r.into(),
        mpn: None,
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
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
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
    ];
    for c in &part_edit_cmds {
        assert!(c.clears_routing(), "{c:?} must clear routing -- it can move a part out from under a track");
    }
}
