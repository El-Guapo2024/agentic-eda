//! Duplicate and Paste for every kind of PCB item, footprints and groups included, and the KiCad clipboard text they paste.

use super::pcb_transform_tests::{board, design, drawings, id, ids, model, p, pose, routing};
use super::*;
use eda_model::ir::{DimensionKind, PlacementSection, Provenance};

/// A board with the same two parts and nets as [`design`] but nothing placed or drawn yet.
fn blank_design() -> Design {
    let mut d = design();
    d.placement = Some(PlacementSection { outline: vec![p(0, 0), p(100_000, 0), p(100_000, 100_000), p(0, 100_000)], footprints: vec![], modules: vec![] });
    d.routing = None;
    d.drawings = None;
    d.provenance = Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] };
    d
}

/// The KiCad clipboard text of a Copy of `names` on the fixture board, measured from `reference`.
fn copy(names: &[&str], reference: Point) -> String {
    let m = model(&["F.Cu", "B.Cu"]);
    eda_kicad::export_clipboard(&design(), &m, &ids(names), reference).unwrap()
}

fn group_of(b: &Board<'_>, member: &str) -> Option<Group> {
    drawings(b).groups.iter().find(|g| g.member_ids.iter().any(|m| m == member)).cloned()
}

// -------------------------------------------------------------- duplicate

#[test]
fn duplicate_copies_a_dimension_in_place_under_a_new_id() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::Duplicate { ids: ids(&["dim_a"]) }).unwrap();
    let dims = &drawings(&b).dimensions;
    assert_eq!(dims.len(), 2);
    let copy = dims.iter().find(|d| d.id != "dim_a").unwrap();
    assert_eq!((copy.start, copy.end, copy.kind), (p(70_000, 50_000), p(80_000, 50_000), DimensionKind::Aligned { height: 2000 }));
    assert!(copy.id.starts_with("dim_"), "{}", copy.id);
}

#[test]
fn duplicating_a_group_copies_its_members_into_a_group_of_their_own() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: "block".into(), member_ids: ids(&["trk_a", "shp_seg", "U1"]) });
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::Duplicate { ids: ids(&["grp_a"]) }).unwrap();

    assert_eq!(drawings(&b).groups.len(), 2, "the original group stays, a second one is made");
    let new_group = drawings(&b).groups.iter().find(|g| g.id != "grp_a").unwrap().clone();
    assert_eq!((new_group.name.as_str(), new_group.member_ids.len()), ("block", 3));
    assert!(new_group.member_ids.iter().all(|m| !["trk_a", "shp_seg", "U1"].contains(&m.as_str())), "its members are copies: {:?}", new_group.member_ids);
    assert_eq!(drawings(&b).groups.iter().find(|g| g.id == "grp_a").unwrap().member_ids, ids(&["trk_a", "shp_seg", "U1"]), "and the original group still holds the originals");
    assert_eq!(routing(&b).tracks.len(), 2);
    assert_eq!(drawings(&b).shapes.len(), 4);
    // The copied footprint is a new part with the next free reference (U1 and U2 are taken).
    assert!(new_group.member_ids.contains(&"U3".to_string()), "{:?}", new_group.member_ids);
}

#[test]
fn the_copy_of_a_member_of_a_group_joins_that_group() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: String::new(), member_ids: ids(&["shp_seg", "txt_a"]) });
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::Duplicate { ids: ids(&["shp_seg"]) }).unwrap();
    let g = drawings(&b).groups.iter().find(|g| g.id == "grp_a").unwrap();
    assert_eq!(g.member_ids.len(), 3, "{g:?}");
    let copy = g.member_ids.iter().find(|m| m.as_str() != "shp_seg" && m.as_str() != "txt_a").unwrap();
    assert!(drawings(&b).shapes.iter().any(|s| s.id() == copy));
    // `group_of` finds the same group for the copy.
    assert_eq!(group_of(&b, copy).unwrap().id, "grp_a");
}

#[test]
fn duplicating_a_footprint_makes_a_new_part_under_the_next_free_reference_on_the_same_nets() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::Duplicate { ids: ids(&["U1"]) }).unwrap();
    b.apply(&Cmd::Duplicate { ids: ids(&["U1"]) }).unwrap();

    let parts = &drawings(&b).board_parts;
    assert_eq!(parts.iter().map(|p| p.reference.as_str()).collect::<Vec<_>>(), vec!["U3", "U4"], "U1 and U2 are in the intent");
    let u3 = &parts[0];
    assert_eq!((u3.footprint.as_str(), u3.value.clone(), u3.definition.is_none()), ("LOPSIDED", None, true), "the model resolves LOPSIDED itself, nothing is stored with the copy");
    assert_eq!(u3.pad_nets, vec![("1".to_string(), "GND".to_string()), ("2".to_string(), "VCC".to_string())], "the copy's pads are on the nets the original's are");
    // In place: the copy sits where the original does, with the same pose.
    let (orig, copy) = (pose(&b, "U1"), pose(&b, "U3"));
    assert_eq!((copy.at, copy.rot, copy.side), (orig.at, orig.rot, orig.side));
    assert_eq!(b.design().placement.as_ref().unwrap().footprints.len(), 4);
}

#[test]
fn deleting_a_copied_footprint_deletes_its_part_too_but_deleting_an_intent_part_keeps_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::Duplicate { ids: ids(&["U1"]) }).unwrap();
    b.apply(&Cmd::Rip { part: id("U3") }).unwrap();
    assert!(drawings(&b).board_parts.is_empty(), "a part that exists only on the board goes with its footprint");
    assert!(b.design().placement.as_ref().unwrap().footprints.iter().all(|f| f.id != "U3"));
    b.apply(&Cmd::Rip { part: id("U1") }).unwrap();
    assert!(drawings(&b).board_parts.is_empty());
}

#[test]
fn duplicate_with_nothing_duplicable_is_still_refused_and_unknown_ids_are_skipped() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert_eq!(b.apply(&Cmd::Duplicate { ids: ids(&["nope"]) }).unwrap_err()[0].check, "ops_unknown_duplicate");
    assert_eq!(b.apply(&Cmd::Duplicate { ids: vec![] }).unwrap_err()[0].check, "ops_bad_duplicate");
    b.apply(&Cmd::Duplicate { ids: ids(&["nope", "trk_a"]) }).unwrap();
    assert_eq!(routing(&b).tracks.len(), 2);
}

// ------------------------------------------------------------------ paste

fn target(m: &ConstraintModel) -> Board<'_> {
    Board::new(blank_design(), m, 100, 300)
}

#[test]
fn a_paste_puts_the_clipboards_origin_where_it_is_asked_and_unlocked() {
    let m = model(&["F.Cu", "B.Cu"]);
    // trk_a is locked on the source board's lock list in a real session; whatever it is there, a pasted item is free.
    let text = copy(&["trk_a", "via_a", "zone_a", "shp_seg", "txt_a", "dim_a"], p(10_000, 40_000));
    let mut b = target(&m);
    b.apply(&Cmd::PasteClipboard { text, at: p(50_000, 60_000) }).unwrap();

    // The polyline track is two segments in a KiCad file.
    let mut pts: Vec<Vec<Point>> = routing(&b).tracks.iter().map(|t| t.pts.clone()).collect();
    pts.sort();
    assert_eq!(pts, vec![vec![p(50_000, 60_000), p(60_000, 60_000)], vec![p(60_000, 60_000), p(60_000, 70_000)]]);
    assert_eq!(routing(&b).tracks[0].net, "GND");
    assert_eq!(routing(&b).vias[0].at, p(60_000, 70_000));
    assert_eq!(routing(&b).zones[0].outline[0], p(40_000, 80_000));
    assert_eq!(drawings(&b).shapes[0].points(), vec![p(40_000, 100_000), p(50_000, 100_000)]);
    assert_eq!(drawings(&b).texts[0].at, p(110_000, 100_000));
    assert_eq!(drawings(&b).dimensions[0].start, p(110_000, 70_000));
    assert!(drawings(&b).locked_ids.is_empty());
    for t in &routing(&b).tracks {
        assert!(t.id.starts_with("trk_"), "{}", t.id);
    }
}

#[test]
fn copper_on_a_net_this_board_does_not_have_lands_on_no_net() {
    let mut m = model(&["F.Cu", "B.Cu"]);
    m.nets.retain(|n| n.name != "GND");
    let text = copy(&["trk_a", "via_a"], p(0, 0));
    let mut b = target(&m);
    b.apply(&Cmd::PasteClipboard { text, at: p(0, 0) }).unwrap();
    assert!(routing(&b).tracks.iter().all(|t| t.net.is_empty()), "{:?}", routing(&b).tracks);
    assert_eq!(routing(&b).vias[0].net, "");
}

#[test]
fn copper_on_a_layer_the_board_lacks_is_dropped() {
    let src_model = model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    let mut d = design();
    d.routing.as_mut().unwrap().tracks.push(super::pcb_transform_tests::track("trk_in", "In1.Cu", &[(0, 0), (1_000, 0)]));
    let text = eda_kicad::export_clipboard(&d, &src_model, &ids(&["trk_a", "trk_in"]), p(0, 0)).unwrap();
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = target(&m);
    b.apply(&Cmd::PasteClipboard { text, at: p(0, 0) }).unwrap();
    assert_eq!(routing(&b).tracks.len(), 2, "the two segments of trk_a; trk_in was on In1.Cu: {:?}", routing(&b).tracks);
    assert!(routing(&b).tracks.iter().all(|t| t.layer == "F.Cu"));

    // A clipboard with nothing that fits is refused.
    let only_inner = eda_kicad::export_clipboard(&d, &src_model, &ids(&["trk_in"]), p(0, 0)).unwrap();
    let e = target(&m).apply(&Cmd::PasteClipboard { text: only_inner, at: p(0, 0) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_clipboard");
}

#[test]
fn a_pasted_group_is_a_group_again_over_the_new_ids() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: "block".into(), member_ids: ids(&["shp_seg", "txt_a", "dim_a"]) });
    let text = eda_kicad::export_clipboard(&d, &model(&["F.Cu", "B.Cu"]), &ids(&["grp_a"]), p(0, 0)).unwrap();
    let mut b = target(&m);
    b.apply(&Cmd::PasteClipboard { text, at: p(1_000, 1_000) }).unwrap();
    let g = &drawings(&b).groups[0];
    assert_eq!((g.name.as_str(), g.member_ids.len()), ("block", 3));
    assert!(g.id.starts_with("grp_"));
    for member in &g.member_ids {
        assert!(drawings(&b).shapes.iter().any(|s| s.id() == member) || drawings(&b).texts.iter().any(|t| &t.id == member) || drawings(&b).dimensions.iter().any(|d| &d.id == member), "{member} is an item of the board");
    }
}

#[test]
fn a_pasted_footprint_whose_part_is_not_placed_is_placed_again() {
    let m = model(&["F.Cu", "B.Cu"]);
    // U1 and U2 are parts of this board, neither placed: the paste puts U1 back.
    let text = copy(&["U1"], p(30_000, 20_000));
    let mut b = target(&m);
    b.apply(&Cmd::PasteClipboard { text, at: p(40_000, 50_000) }).unwrap();
    assert!(drawings_has_no_board_parts(&b));
    let u1 = pose(&b, "U1");
    assert_eq!((u1.at, u1.rot, u1.side), (p(40_000, 50_000), 0, Side::Top));
    assert!(b.design().placement.as_ref().unwrap().footprints.iter().all(|f| f.id != "U2"));
}

fn drawings_has_no_board_parts(b: &Board<'_>) -> bool {
    b.design().drawings.as_ref().map_or(true, |d| d.board_parts.is_empty())
}

#[test]
fn a_pasted_footprint_whose_reference_is_taken_is_a_new_part_with_its_nets_by_name() {
    let m = model(&["F.Cu", "B.Cu"]);
    // Two footprints copied together keep their pads' nets (a lone footprint's pads travel on no net).
    let text = copy(&["U1", "U2"], p(30_000, 20_000));
    let mut b = board(&m);
    b.apply(&Cmd::PasteClipboard { text, at: p(10_000, 80_000) }).unwrap();

    let refs: Vec<&str> = drawings(&b).board_parts.iter().map(|p| p.reference.as_str()).collect();
    assert_eq!(refs, vec!["U3", "U4"], "U1 and U2 are on the board already: the next free numbers");
    let u3 = &drawings(&b).board_parts[0];
    assert_eq!(u3.pad_nets, vec![("1".to_string(), "GND".to_string()), ("2".to_string(), "VCC".to_string())], "U1's pads are on GND and VCC in the model; the copy's are on the same nets");
    assert!(drawings(&b).board_parts[1].pad_nets.is_empty(), "U2 has no nets");
    // Pasted with the reference point on the cursor: U1 was at (30, 20) mm.
    assert_eq!(pose(&b, "U3").at, p(10_000, 80_000));
    assert_eq!(pose(&b, "U4").at, p(40_000, 80_000));
}

#[test]
fn a_footprint_copied_alone_pastes_with_its_pads_on_no_net() {
    let m = model(&["F.Cu", "B.Cu"]);
    let text = copy(&["U1"], p(30_000, 20_000));
    assert!(text.trim_start().starts_with("(footprint"), "{text}");
    let mut b = board(&m);
    b.apply(&Cmd::PasteClipboard { text, at: p(10_000, 80_000) }).unwrap();
    let new = &drawings(&b).board_parts[0];
    assert_eq!(new.reference, "U3");
    assert!(new.pad_nets.is_empty());
}

#[test]
fn text_that_is_not_kicad_items_is_refused_without_touching_the_board() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let before = b.design().clone();
    let e = b.apply(&Cmd::PasteClipboard { text: "just some words".into(), at: p(0, 0) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_clipboard");
    assert_eq!(serde_json::to_string(b.design()).unwrap(), serde_json::to_string(&before).unwrap());
}

#[test]
fn paste_clipboard_round_trips_through_json_and_everything_has_a_subject() {
    let c = Cmd::PasteClipboard { text: "(kicad_pcb)".into(), at: p(5, 6) };
    let back: Cmd = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back, c);
    assert_eq!(c.subjects(), vec!["paste"]);
    // A paste does not move footprints out from under copper: the routing stays.
    assert!(!c.clears_routing());
    assert_eq!(id("x"), "x");
}
