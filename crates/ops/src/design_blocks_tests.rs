//! Design blocks on the board: Place Design Block (`PCB_CONTROL::AppendDesignBlock` -> `AppendBoard`, with `placeAsGroup`) and the link a saved selection gets.

use super::pcb_transform_tests::{board, design, drawings, ids, model, p};
use super::*;

/// The KiCad text of a Copy of `names` on the fixture board, which is what a design block's board file holds.
fn block_text(names: &[&str]) -> String {
    let m = model(&["F.Cu", "B.Cu"]);
    eda_kicad::export_pcb_clipboard(&design(), &m, &ids(names), p(0, 0)).unwrap()
}

fn place(text: &str, at: Point, as_group: bool, group_name: &str) -> Cmd {
    Cmd::PlaceDesignBlock { text: text.to_string(), at, block: "Power:Buck".into(), as_group, group_name: group_name.into() }
}

#[test]
fn a_design_block_placed_with_place_as_group_is_one_group_named_for_the_block_and_linked() {
    let m = model(&["F.Cu", "B.Cu"]);
    let text = block_text(&["trk_a", "shp_seg", "txt_a"]);
    let mut b = board(&m);
    let (tracks, shapes, texts) = (routing_len(&b), drawings(&b).shapes.len(), drawings(&b).texts.len());
    b.apply(&place(&text, p(10_000, 5_000), true, "")).unwrap();
    // KiCad has a track to a segment: the polyline of `trk_a` comes back as the segments it was written as
    let came = (routing_len(&b) - tracks, drawings(&b).shapes.len() - shapes, drawings(&b).texts.len() - texts);
    assert!(came.0 >= 1 && came.1 == 1 && came.2 == 1, "{came:?}");
    let g = drawings(&b).groups.iter().find(|g| g.design_block == "Power:Buck").expect("a linked group").clone();
    assert_eq!((g.name.as_str(), g.member_ids.len()), ("Buck", came.0 + came.1 + came.2), "{g:?}");
    // every member is one of the new items, none of the board's own
    assert!(!g.member_ids.iter().any(|m| ["trk_a", "shp_seg", "txt_a"].contains(&m.as_str())), "{:?}", g.member_ids);
    // "Place repeated copies" names the next one
    b.apply(&place(&text, p(40_000, 5_000), true, "Buck2")).unwrap();
    let names: Vec<String> = drawings(&b).groups.iter().filter(|g| g.design_block == "Power:Buck").map(|g| g.name.clone()).collect();
    assert_eq!(names, vec!["Buck", "Buck2"]);
}

#[test]
fn a_design_block_is_not_grouped_when_not_asked_or_when_one_thing_came_in_and_a_bad_block_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&place(&block_text(&["trk_a", "shp_seg"]), p(0, 0), false, "")).unwrap();
    assert!(drawings(&b).groups.iter().all(|g| g.design_block.is_empty()), "Place as group is off");
    // `if( groupableCount >= 2 )`: one item is not a group
    let before = drawings(&b).groups.len();
    b.apply(&place(&block_text(&["shp_seg"]), p(0, 0), true, "")).unwrap();
    assert_eq!(drawings(&b).groups.len(), before);
    // a block that is not a Lib:Name, and a file that is not a board
    let bad = Cmd::PlaceDesignBlock { text: block_text(&["shp_seg"]), at: p(0, 0), block: "nonsense".into(), as_group: true, group_name: String::new() };
    assert_eq!(b.apply(&bad).unwrap_err()[0].check, "ops_bad_design_block");
    assert!(b.apply(&place("not a board", p(0, 0), true, "")).is_err());
}

#[test]
fn the_selection_saved_as_a_block_is_grouped_and_linked_or_its_group_takes_the_link() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // not a group: a new group named after the block, even for one item
    b.apply(&Cmd::LinkDesignBlock { ids: ids(&["shp_seg"]), block: "Mine:Marker".into() }).unwrap();
    let g = drawings(&b).groups.iter().find(|g| g.design_block == "Mine:Marker").unwrap().clone();
    assert_eq!((g.name.as_str(), g.member_ids.clone()), ("Marker", ids(&["shp_seg"])));
    // a lone group with no link takes it and keeps its name
    b.apply(&Cmd::Group { ids: ids(&["trk_a", "txt_a"]) }).unwrap();
    let plain = drawings(&b).groups.iter().find(|g| g.design_block.is_empty()).unwrap().clone();
    b.apply(&Cmd::EditGroup { id: plain.id.clone(), name: "pair".into(), member_ids: plain.member_ids.clone(), design_block: None }).unwrap();
    b.apply(&Cmd::LinkDesignBlock { ids: vec![plain.id.clone()], block: "Mine:Pair".into() }).unwrap();
    let g = drawings(&b).groups.iter().find(|g| g.id == plain.id).unwrap().clone();
    assert_eq!((g.name.as_str(), g.design_block.as_str()), ("pair", "Mine:Pair"));
    // Group Properties' link box
    b.apply(&Cmd::EditGroup { id: plain.id.clone(), name: "pair".into(), member_ids: plain.member_ids.clone(), design_block: Some(String::new()) }).unwrap();
    assert!(drawings(&b).groups.iter().find(|g| g.id == plain.id).unwrap().design_block.is_empty());
    assert!(b.apply(&Cmd::EditGroup { id: plain.id.clone(), name: "pair".into(), member_ids: plain.member_ids.clone(), design_block: Some("bogus".into()) }).is_err());
    // an id the board does not have, and a link that is no id
    assert_eq!(b.apply(&Cmd::LinkDesignBlock { ids: ids(&["nothing"]), block: "Mine:X".into() }).unwrap_err()[0].check, "ops_unknown_item");
    assert!(b.apply(&Cmd::LinkDesignBlock { ids: ids(&["shp_seg"]), block: "Mine".into() }).is_err());
    // the link survives a Duplicate of the group, as `PCB_GROUP::DeepDuplicate` copies it
    let linked = drawings(&b).groups.iter().find(|g| g.design_block == "Mine:Marker").unwrap().id.clone();
    b.apply(&Cmd::Duplicate { ids: vec![linked] }).unwrap();
    assert_eq!(drawings(&b).groups.iter().filter(|g| g.design_block == "Mine:Marker").count(), 2);
}

fn routing_len(b: &Board<'_>) -> usize {
    b.design().routing.as_ref().map_or(0, |r| r.tracks.len())
}
