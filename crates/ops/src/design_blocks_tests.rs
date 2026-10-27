//! Design blocks on the board: Place Design Block (`PCB_CONTROL::AppendDesignBlock` -> `AppendBoard`, with `placeAsGroup`) and the link a saved selection gets.

use super::pcb_transform_tests::{board, design, drawings, ids, model, p};
use super::*;
use eda_model::units::mm;

/// The KiCad text of a Copy of `names` on the fixture board, which is what a design block's board file holds.
fn block_text(names: &[&str]) -> String {
    let m = model(&["F.Cu", "B.Cu"]);
    eda_kicad::export_pcb_clipboard(&design(), &m, &ids(names), p(Nm::ZERO, Nm::ZERO)).unwrap()
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
    b.apply(&place(&text, p(mm(10.0), mm(5.0)), true, "")).unwrap();
    // KiCad has a track to a segment: the polyline of `trk_a` comes back as the segments it was written as
    let came = (routing_len(&b) - tracks, drawings(&b).shapes.len() - shapes, drawings(&b).texts.len() - texts);
    assert!(came.0 >= 1 && came.1 == 1 && came.2 == 1, "{came:?}");
    let g = drawings(&b).groups.iter().find(|g| g.design_block == "Power:Buck").expect("a linked group").clone();
    assert_eq!((g.name.as_str(), g.member_ids.len()), ("Buck", came.0 + came.1 + came.2), "{g:?}");
    // every member is one of the new items, none of the board's own
    assert!(!g.member_ids.iter().any(|m| ["trk_a", "shp_seg", "txt_a"].contains(&m.as_str())), "{:?}", g.member_ids);
    // "Place repeated copies" names the next one
    b.apply(&place(&text, p(mm(40.0), mm(5.0)), true, "Buck2")).unwrap();
    let names: Vec<String> = drawings(&b).groups.iter().filter(|g| g.design_block == "Power:Buck").map(|g| g.name.clone()).collect();
    assert_eq!(names, vec!["Buck", "Buck2"]);
}

#[test]
fn a_design_block_is_not_grouped_when_not_asked_or_when_one_thing_came_in_and_a_bad_block_is_refused() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&place(&block_text(&["trk_a", "shp_seg"]), p(Nm::ZERO, Nm::ZERO), false, "")).unwrap();
    assert!(drawings(&b).groups.iter().all(|g| g.design_block.is_empty()), "Place as group is off");
    // `if( groupableCount >= 2 )`: one item is not a group
    let before = drawings(&b).groups.len();
    b.apply(&place(&block_text(&["shp_seg"]), p(Nm::ZERO, Nm::ZERO), true, "")).unwrap();
    assert_eq!(drawings(&b).groups.len(), before);
    // a block that is not a Lib:Name, and a file that is not a board
    let bad = Cmd::PlaceDesignBlock { text: block_text(&["shp_seg"]), at: p(Nm::ZERO, Nm::ZERO), block: "nonsense".into(), as_group: true, group_name: String::new() };
    assert_eq!(b.apply(&bad).unwrap_err()[0].check, "ops_bad_design_block");
    assert!(b.apply(&place("not a board", p(Nm::ZERO, Nm::ZERO), true, "")).is_err());
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
    // `LIB_ID::Parse( text, true )` never fails: a name with no library is a link too (r2-pcb-edit A9)
    b.apply(&Cmd::EditGroup { id: plain.id.clone(), name: "pair".into(), member_ids: plain.member_ids.clone(), design_block: Some("bogus".into()) }).unwrap();
    assert_eq!(drawings(&b).groups.iter().find(|g| g.id == plain.id).unwrap().design_block, "bogus");
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

/// `AppendBoard` loads the block's file into the board: the copper layers grow to the block's and its nets are the board's by name, so
/// nothing of the block's copper is dropped.
#[test]
fn place_design_block_grows_the_copper_layers_and_keeps_the_nets() {
    // the block: a four-layer board with a track on In1.Cu and a via, both on a net no pad of the block is on
    let mut block_model = model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    block_model.nets.push(eda_model::Net { name: "BLK".into(), pins: vec![] });
    let mut block = design();
    {
        let rt = block.routing.as_mut().unwrap();
        let mut t = super::pcb_transform_tests::track("trk_in", "In1.Cu", &[(mm(10.0), mm(10.0)), (mm(20.0), mm(10.0))]);
        t.net = "BLK".into();
        rt.tracks.push(t);
        let mut v = super::pcb_transform_tests::via("via_in", (mm(20.0), mm(10.0)), "F.Cu", "In2.Cu");
        v.net = "BLK".into();
        rt.vias.push(v);
    }
    let text = eda_kicad::export_pcb_clipboard(&block, &block_model, &ids(&["trk_in", "via_in", "trk_a"]), p(Nm::ZERO, Nm::ZERO)).unwrap();

    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&place(&text, p(Nm::ZERO, Nm::ZERO), true, "")).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.rules.as_ref().and_then(|r| r.stackup.as_ref()).map(|s| s.copper_layers), Some(4), "the board has the block's four copper layers");
    assert_eq!(dr.board_nets, vec!["BLK".to_string()], "the block's net is the board's");
    let rt = super::pcb_transform_tests::routing(&b);
    assert!(rt.tracks.iter().any(|t| t.layer == "In1.Cu" && t.net == "BLK"), "{:?}", rt.tracks);
    assert!(rt.vias.iter().any(|v| v.to_layer == "In2.Cu" && v.net == "BLK"), "{:?}", rt.vias);
    // a net the board has is not added again
    assert!(rt.tracks.iter().filter(|t| t.id != "trk_a").any(|t| t.net == "GND"));

    // a two-layer block leaves a two-layer board as it is
    let mut b = board(&m);
    b.apply(&place(&block_text(&["trk_a", "shp_seg"]), p(Nm::ZERO, Nm::ZERO), true, "")).unwrap();
    assert!(drawings(&b).rules.as_ref().and_then(|r| r.stackup.as_ref()).is_none());
    assert!(drawings(&b).board_nets.is_empty());
}

/// Review item 5: the copper count is the `(layers ..)` table's (`cu.size()`), a pad's `(layers "F.Cu" ..)` is not the table, and a user layer the block
/// enables is enabled on the board (`enabledLayers |= initialEnabledLayers`).
#[test]
fn the_block_layer_table_gives_the_copper_count_and_the_enabled_layers() {
    let text = r#"(kicad_pcb (version 20241229)
	(layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (8 "In3.Cu" signal "Mid") (10 "In4.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (39 "User.1" user))
	(footprint "X" (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Mask")))
)"#;
    assert_eq!(super::design_blocks::layer_table(text), ["F.Cu", "In1.Cu", "In2.Cu", "In3.Cu", "In4.Cu", "B.Cu", "Edge.Cuts", "User.1"]);
    assert!(super::design_blocks::layer_table(r#"(footprint "X" (pad "1" smd rect (layers "F.Cu")))"#).is_empty());
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&place(&text.replace("(footprint \"X\" (pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\")))", "(gr_line (start 0 0) (end 5 0) (stroke (width 0.1) (type solid)) (layer \"User.1\"))"), p(Nm::ZERO, Nm::ZERO), false, "")).unwrap();
    let rules = drawings(&b).rules.clone().unwrap();
    assert_eq!(rules.stackup.map(|s| s.copper_layers), Some(6), "six copper layers in the table, though the block uses none of the inner ones");
    assert!(rules.board_layers.unwrap().enabled.iter().any(|l| l == "User.1"));
}
