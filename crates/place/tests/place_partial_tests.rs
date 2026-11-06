//! `place_partial`: the placer of `eda board place` run for some footprints among others that stay where they are (Place > Auto-Place Footprints,
//! UI-LEFT P19). The footprints that stay are never moved and never snapped, whatever their angle or side; the ones placed keep clear of them and of
//! each other, inside the board; a part on the back stays on the back; a board too full is refused with the offenders.

use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, Side};
use eda_model::units::{mm, Nm};
use eda_model::{CheckStatus, ConstraintModel, Net, Part, Pin, PinKind};
use eda_place::{place, place_partial, PlaceOptions};
use std::collections::BTreeSet;

fn part(reference: &str, package: &str, npins: usize) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some(package.into()),
        footprint: None,
        pins: (1..=npins).map(|i| Pin { number: i.to_string(), name: None, kind: PinKind::Signal }).collect(),
        body_nm: None,
        symbol: None,
        datasheet: None,
        edge: None,
    }
}

fn model() -> ConstraintModel {
    let mut parts = vec![part("U1", "SOIC-8", 8)];
    let mut nets = vec![];
    for i in 1..=6 {
        parts.push(part(&format!("R{i}"), "0603", 2));
        nets.push(Net { name: format!("S{i}"), pins: vec![format!("R{i}.1"), format!("U1.{}", i)] });
    }
    ConstraintModel { parts, nets, ..Default::default() }
}

fn outline(w: f64, h: f64) -> Vec<Point> {
    vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(w), y: Nm::ZERO }, Point { x: mm(w), y: mm(h) }, Point { x: Nm::ZERO, y: mm(h) }]
}

fn fp(id: &str, x: f64, y: f64, rot: u32, side: Side) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: Point { x: mm(x), y: mm(y) }, rot, side, label: LabelSide::Above }
}

fn design(footprints: Vec<FootprintInstance>, w: f64, h: f64) -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "test".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: outline(w, h), footprints, modules: vec![] }),
        routing: None,
        drawings: None,
    }
}

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// The design with the placed poses written over the movable ones (the caller's half of a partial placement).
fn merged(d: &Design, placed: &[FootprintInstance]) -> Design {
    let mut out = d.clone();
    let fps = &mut out.placement.as_mut().unwrap().footprints;
    for p in placed {
        match fps.iter_mut().find(|f| f.id == p.id) {
            Some(f) => *f = p.clone(),
            None => fps.push(p.clone()),
        }
    }
    fps.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn overlaps(d: &Design, m: &ConstraintModel) -> Vec<String> {
    eda_gates::check_placement(d, m)
        .into_iter()
        .filter(|c| c.status == CheckStatus::Fail && (c.check == "placement_courtyard_overlap" || c.check == "placement_within_outline"))
        .map(|c| format!("{} {:?}", c.check, c.location))
        .collect()
}

#[test]
fn only_the_movable_parts_come_back_and_they_keep_clear_of_the_ones_that_stay() {
    let m = model();
    // U1 turned 45 degrees in the middle and R1..R4 placed; R5 and R6 not on the board yet
    let d = design(vec![fp("U1", 20.0, 15.0, 45_000, Side::Top), fp("R1", 5.0, 5.0, 0, Side::Top), fp("R2", 35.0, 5.0, 90_000, Side::Bottom), fp("R3", 5.0, 25.0, 0, Side::Top), fp("R4", 35.0, 25.0, 0, Side::Top)], 40.0, 30.0);
    let movable = set(&["R3", "R4", "R5", "R6"]);
    let out = place_partial(&d, &m, &PlaceOptions::default(), &movable).expect("places");
    let ids: BTreeSet<String> = out.iter().map(|f| f.id.clone()).collect();
    assert_eq!(ids, movable, "the movable parts, and only they, come back");
    let all = merged(&d, &out);
    assert!(overlaps(&all, &m).is_empty(), "{:?}", overlaps(&all, &m));
    for f in &out {
        assert_eq!(f.at.x.get() % 100_000, 0, "{} on the placement step", f.id);
        assert_eq!(f.rot % 90_000, 0, "{} at a quarter turn", f.id);
    }
}

#[test]
fn a_movable_part_on_the_back_is_placed_on_the_back() {
    let m = model();
    let d = design(vec![fp("U1", 20.0, 15.0, 0, Side::Top), fp("R1", 5.0, 5.0, 0, Side::Bottom)], 40.0, 30.0);
    let out = place_partial(&d, &m, &PlaceOptions::default(), &set(&["R1", "R2"])).expect("places");
    let side = |id: &str| out.iter().find(|f| f.id == id).unwrap().side;
    assert_eq!((side("R1"), side("R2")), (Side::Bottom, Side::Top));
}

#[test]
fn the_same_inputs_place_the_same_way() {
    let m = model();
    let d = design(vec![fp("U1", 20.0, 15.0, 0, Side::Top)], 40.0, 30.0);
    let a = place_partial(&d, &m, &PlaceOptions::default(), &set(&["R1", "R2", "R3"])).unwrap();
    let b = place_partial(&d, &m, &PlaceOptions::default(), &set(&["R1", "R2", "R3"])).unwrap();
    assert_eq!(a.iter().map(|f| (f.at, f.rot)).collect::<Vec<_>>(), b.iter().map(|f| (f.at, f.rot)).collect::<Vec<_>>());
}

#[test]
fn a_board_with_no_room_is_refused_with_the_offenders() {
    let m = model();
    // a 6 x 6 mm board already taken by U1: the resistors cannot go anywhere
    let d = design(vec![fp("U1", 3.0, 3.0, 0, Side::Top)], 6.0, 6.0);
    let e = place_partial(&d, &m, &PlaceOptions::default(), &set(&["R1", "R2", "R3", "R4", "R5", "R6"])).unwrap_err();
    assert_eq!(e[0].check, "place_legalize");
    assert!(e.iter().skip(1).all(|c| c.check == "place_outside" || c.check == "place_overlap"), "{e:#?}");
    assert!(e.iter().skip(1).all(|c| c.location.as_deref().is_some_and(|l| l.contains('R'))), "only placed parts are offenders: {e:#?}");
}

#[test]
fn an_unknown_part_or_no_outline_is_refused() {
    let m = model();
    let d = design(vec![], 40.0, 30.0);
    assert_eq!(place_partial(&d, &m, &PlaceOptions::default(), &set(&["X9"])).unwrap_err()[0].check, "place_precondition");
    let mut bare = d.clone();
    bare.placement.as_mut().unwrap().outline.clear();
    assert_eq!(place_partial(&bare, &m, &PlaceOptions::default(), &set(&["R1"])).unwrap_err()[0].check, "place_precondition");
}

#[test]
fn a_full_placement_still_places_every_part_the_same_way_each_time() {
    // `place` sees every item movable; its moves are drawn the same way they were before the list existed (the picks are the indices themselves).
    let m = model();
    let d = design(vec![], 40.0, 30.0);
    let opts = PlaceOptions { outline: Some(outline(40.0, 30.0)), ..Default::default() };
    let a = place(&d, &m, &opts).expect("places");
    let b = place(&d, &m, &opts).expect("places");
    assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    assert_eq!(a.placement.unwrap().footprints.len(), 7);
}
