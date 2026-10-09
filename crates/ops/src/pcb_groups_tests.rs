//! Groups that nest: what a group of groups moves, turns, flips and takes with it when it goes, and how a group stays honest when
//! its members leave the board.

use super::pcb_transform_tests::{design, drawings, ids, model, p, pose, routing};
use super::*;

fn group(id: &str, members: &[&str]) -> Group {
    Group { id: id.into(), name: String::new(), member_ids: ids(members) }
}

/// The fixture board with `inner = {trk_a, via_a}` held by `outer = {inner, shp_seg}`; `U1`, `U2`, the zone and the rest stand outside.
fn nested() -> Design {
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_outer", &["grp_inner", "shp_seg"]), group("grp_inner", &["trk_a", "via_a"])];
    d
}

fn members(b: &Board<'_>, id: &str) -> Vec<String> {
    let mut m = drawings(b).group(id).map(|g| g.member_ids.clone()).unwrap_or_default();
    m.sort();
    m
}

// ------------------------------------------------------------------- transforms

#[test]
fn a_group_of_groups_moves_everything_below_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(nested(), &m, 100, 300);
    b.apply(&Cmd::MoveItems { ids: ids(&["grp_outer"]), dx: 1_000, dy: 2_000 }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts[0], p(11_000, 42_000), "a track of the inner group");
    assert_eq!(routing(&b).vias[0].at, p(21_000, 52_000), "a via of the inner group");
    assert_eq!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_seg").unwrap().points()[0], p(1_000, 82_000), "a shape of the outer group");
    assert_eq!(pose(&b, "U1").at, p(30_000, 20_000), "a footprint outside every group stays");
    assert_eq!(routing(&b).zones[0].outline[0], p(0, 60_000));
}

#[test]
fn a_group_of_groups_turns_and_flips_as_its_items_do() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut by_group = Board::new(nested(), &m, 100, 300);
    let mut by_items = Board::new(design(), &m, 100, 300);
    let turn = |ids: Vec<String>| Cmd::RotateItems { ids, pivot: p(20_000, 50_000), angle_millideg: 90_000 };
    by_group.apply(&turn(ids(&["grp_outer"]))).unwrap();
    by_items.apply(&turn(ids(&["trk_a", "via_a", "shp_seg"]))).unwrap();
    assert_eq!(routing(&by_group).tracks, routing(&by_items).tracks);
    assert_eq!(routing(&by_group).vias, routing(&by_items).vias);
    assert_eq!(drawings(&by_group).shapes, drawings(&by_items).shapes);

    let flip = |ids: Vec<String>| Cmd::FlipItems { ids, pivot: p(20_000, 50_000), direction: FlipDirection::LeftRight };
    by_group.apply(&flip(ids(&["grp_outer"]))).unwrap();
    by_items.apply(&flip(ids(&["trk_a", "via_a", "shp_seg"]))).unwrap();
    assert_eq!(routing(&by_group).tracks, routing(&by_items).tracks);
    assert_eq!(drawings(&by_group).shapes, drawings(&by_items).shapes);
}

#[test]
fn a_footprint_inside_a_nested_group_moves_with_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_outer", &["grp_inner", "shp_seg"]), group("grp_inner", &["U1", "trk_a"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::MoveItems { ids: ids(&["grp_outer"]), dx: 2_000, dy: 0 }).unwrap();
    assert_eq!(pose(&b, "U1").at, p(32_000, 20_000));
    assert_eq!(pose(&b, "U2").at, p(60_000, 20_000));
}

// ------------------------------------------------------------------- nesting

#[test]
fn a_group_made_of_items_that_share_a_group_takes_their_place_in_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_a", &["trk_a", "via_a", "shp_seg"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::Group { ids: ids(&["trk_a", "via_a"]) }).unwrap();
    let groups = &drawings(&b).groups;
    assert_eq!(groups.len(), 2, "{groups:?}");
    let inner = groups.iter().find(|g| g.id != "grp_a").unwrap();
    assert_eq!(members(&b, &inner.id), ids(&["trk_a", "via_a"]));
    assert_eq!(members(&b, "grp_a").len(), 2);
    assert!(members(&b, "grp_a").contains(&inner.id) && members(&b, "grp_a").contains(&"shp_seg".to_string()), "{:?}", members(&b, "grp_a"));
}

#[test]
fn grouping_everything_a_group_holds_leaves_one_group() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_a", &["trk_a", "via_a"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::Group { ids: ids(&["trk_a", "via_a"]) }).unwrap();
    let groups = &drawings(&b).groups;
    assert_eq!(groups.len(), 1, "the old group is left empty-handed and goes: {groups:?}");
    assert_eq!(members(&b, &groups[0].id), ids(&["trk_a", "via_a"]));
}

#[test]
fn ungrouping_a_nested_group_frees_its_members_and_the_group_above_loses_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_outer", &["grp_inner", "shp_seg", "txt_a"]), group("grp_inner", &["trk_a", "via_a"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::Ungroup { ids: ids(&["grp_inner"]) }).unwrap();
    assert!(!drawings(&b).is_group("grp_inner"));
    assert_eq!(members(&b, "grp_outer"), ids(&["shp_seg", "txt_a"]));
    assert!(drawings(&b).parent_group("trk_a").is_none() && drawings(&b).parent_group("via_a").is_none(), "its members are free of every group");
}

#[test]
fn removing_a_member_from_a_nested_group_dissolves_what_falls_under_two_all_the_way_up() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(nested(), &m, 100, 300);
    // inner = {trk_a, via_a} -> {via_a}: gone; outer = {inner, shp_seg} -> {shp_seg}: gone too.
    b.apply(&Cmd::RemoveFromGroup { ids: ids(&["trk_a"]) }).unwrap();
    assert!(drawings(&b).groups.is_empty(), "{:?}", drawings(&b).groups);
}

#[test]
fn a_group_cannot_be_added_to_itself_or_to_a_group_below_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(nested(), &m, 100, 300);
    let e = b.apply(&Cmd::AddToGroup { group_id: "grp_inner".into(), ids: ids(&["grp_outer"]) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_group");
    let e = b.apply(&Cmd::AddToGroup { group_id: "grp_inner".into(), ids: ids(&["grp_inner"]) }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_group");
    assert_eq!(drawings(&b).groups, nested().drawings.unwrap().groups, "a refused command changes nothing");
}

#[test]
fn a_group_can_be_added_to_another_group() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_a", &["trk_a", "via_a"]), group("grp_b", &["shp_seg", "txt_a"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::AddToGroup { group_id: "grp_a".into(), ids: ids(&["grp_b"]) }).unwrap();
    assert_eq!(members(&b, "grp_a"), ids(&["grp_b", "trk_a", "via_a"]));
    assert_eq!(drawings(&b).group_leaves("grp_a"), ids(&["trk_a", "via_a", "shp_seg", "txt_a"]));
}

// ------------------------------------------------------------------- items that leave the board

#[test]
fn deleting_a_member_takes_it_out_of_its_group() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_a", &["trk_a", "via_a", "shp_seg"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::DeleteShape { id: "shp_seg".into() }).unwrap();
    assert_eq!(members(&b, "grp_a"), ids(&["trk_a", "via_a"]));
    b.apply(&Cmd::DeleteVia { id: "via_a".into() }).unwrap();
    assert!(drawings(&b).groups.is_empty(), "a group with one member left is no group: {:?}", drawings(&b).groups);
}

#[test]
fn deleting_every_member_of_a_nested_group_leaves_no_group_behind() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(nested(), &m, 100, 300);
    b.apply(&Cmd::Batch { cmds: vec![Cmd::DeleteTrack { id: "trk_a".into() }, Cmd::DeleteVia { id: "via_a".into() }, Cmd::DeleteShape { id: "shp_seg".into() }] }).unwrap();
    assert!(drawings(&b).groups.is_empty(), "{:?}", drawings(&b).groups);
}

#[test]
fn a_footprint_taken_off_the_board_leaves_its_group() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_a", &["U1", "U2", "shp_seg"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::Rip { part: "U1".into() }).unwrap();
    assert_eq!(members(&b, "grp_a"), ids(&["U2", "shp_seg"]));
}

#[test]
fn a_member_the_board_never_had_stays_in_its_group() {
    // The group tests have always grouped parts nothing placed; a command that removes nothing leaves such a group alone.
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups = vec![group("grp_a", &["never_placed", "trk_a", "via_a"])];
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::MoveItems { ids: ids(&["trk_a"]), dx: 100, dy: 0 }).unwrap();
    assert_eq!(members(&b, "grp_a"), ids(&["never_placed", "trk_a", "via_a"]));
}

// ------------------------------------------------------------------- copies

#[test]
fn duplicating_a_group_of_groups_copies_the_whole_tree() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(nested(), &m, 100, 300);
    b.apply(&Cmd::Duplicate { ids: ids(&["grp_outer"]) }).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.groups.len(), 4, "the two originals stay and two copies are made: {:?}", dr.groups);
    assert_eq!(members(&b, "grp_outer"), ids(&["grp_inner", "shp_seg"]), "the originals are untouched");
    assert_eq!(members(&b, "grp_inner"), ids(&["trk_a", "via_a"]));
    let outer_copy = dr.groups.iter().find(|g| !["grp_outer", "grp_inner"].contains(&g.id.as_str()) && g.member_ids.iter().any(|m| dr.is_group(m))).expect("a copy of the outer group");
    let inner_copy = dr.group(outer_copy.member_ids.iter().find(|m| dr.is_group(m)).unwrap()).unwrap();
    assert_ne!(inner_copy.id, "grp_inner");
    assert_eq!(inner_copy.member_ids.len(), 2);
    assert!(inner_copy.member_ids.iter().all(|m| m != "trk_a" && m != "via_a"), "its members are copies: {:?}", inner_copy.member_ids);
    assert_eq!(dr.group_leaves(&outer_copy.id).len(), 3, "a track, a via and a shape");
    assert_eq!((routing(&b).tracks.len(), routing(&b).vias.len(), dr.shapes.len()), (2, 2, 4));
}

#[test]
fn the_copy_of_a_nested_group_joins_the_group_above_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(nested(), &m, 100, 300);
    b.apply(&Cmd::Duplicate { ids: ids(&["grp_inner"]) }).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.groups.len(), 3, "{:?}", dr.groups);
    let copy = dr.groups.iter().find(|g| !["grp_outer", "grp_inner"].contains(&g.id.as_str())).unwrap();
    assert!(dr.group("grp_outer").unwrap().member_ids.contains(&copy.id), "the copy sits beside the original in the outer group");
}

#[test]
fn a_pasted_group_of_groups_is_nested_again() {
    let m = model(&["F.Cu", "B.Cu"]);
    let text = eda_kicad::export_pcb_clipboard(&nested(), &m, &ids(&["grp_outer"]), p(0, 0)).unwrap();
    let mut b = Board::new(design(), &m, 100, 300);
    b.apply(&Cmd::PasteClipboard { text, at: p(5_000, 5_000) }).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.groups.len(), 2, "{:?}", dr.groups);
    let outer = dr.groups.iter().find(|g| g.member_ids.iter().any(|m| dr.is_group(m))).expect("the outer group");
    // The track has three points, which KiCad's clipboard writes as two segments: two tracks, a via and a shape.
    assert_eq!(dr.group_leaves(&outer.id).len(), 4, "{:?}", dr.group_leaves(&outer.id));
    assert_eq!(outer.member_ids.len(), 2);
}
