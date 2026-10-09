//! The Properties panel's own verbs (`pcb_props.rs`): a track's end points, the net of tracks, vias and zones, a zone's name and a
//! shape's geometry. Everything else the panel edits goes through verbs that had their tests before it.

use super::pcb_transform_tests::{board, design, drawings, ids, model, p, routing, track};
use super::*;

fn with_arc() -> Design {
    let mut d = design();
    let arc = Track::new_arc("GND".into(), "F.Cu".into(), 200, p(10_000, 10_000), p(15_000, 5_000), p(20_000, 10_000));
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_arc".into(), ..arc });
    d
}

// ------------------------------------------------------------ EditTrack

#[test]
fn edit_track_moves_the_first_and_the_last_point_and_nothing_else() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::EditTrack { id: "trk_a".into(), start: Some(p(11_000, 41_000)), end: None }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(11_000, 41_000), p(20_000, 40_000), p(20_000, 50_000)], "only the first vertex moves");
    b.apply(&Cmd::EditTrack { id: "trk_a".into(), start: None, end: Some(p(21_000, 52_000)) }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(11_000, 41_000), p(20_000, 40_000), p(21_000, 52_000)], "and the last one, the bend stays");
    let t = &routing(&b).tracks[0];
    assert_eq!((t.id.as_str(), t.net.as_str(), t.layer.as_str(), t.width), ("trk_a", "GND", "F.Cu", 200), "id, net, layer and width are untouched");
}

#[test]
fn edit_track_keeps_the_mid_point_of_an_arc_and_draws_it_through_the_new_end() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(with_arc(), &m, 100, 300);
    b.apply(&Cmd::EditTrack { id: "trk_arc".into(), start: None, end: Some(p(25_000, 12_000)) }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(10_000, 10_000), p(15_000, 5_000), p(25_000, 12_000))), "still an arc, through the same mid point");
    b.apply(&Cmd::EditTrack { id: "trk_arc".into(), start: Some(p(9_000, 9_000)), end: None }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(9_000, 9_000), p(15_000, 5_000), p(25_000, 12_000))));
}

#[test]
fn edit_track_refuses_what_is_not_a_track_edit() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert_eq!(b.apply(&Cmd::EditTrack { id: "trk_nope".into(), start: Some(p(0, 0)), end: None }).unwrap_err()[0].check, "ops_unknown_track");
    assert_eq!(b.apply(&Cmd::EditTrack { id: "trk_a".into(), start: None, end: None }).unwrap_err()[0].check, "ops_bad_track");
    assert_eq!(b.apply(&Cmd::EditTrack { id: "via_a".into(), start: Some(p(0, 0)), end: None }).unwrap_err()[0].check, "ops_unknown_track", "a via has no end points");

    // A two-point track whose two ends meet is no track.
    let mut d = design();
    d.routing.as_mut().unwrap().tracks.push(track("trk_two", "F.Cu", &[(0, 0), (1_000, 0)]));
    let mut b = Board::new(d, &m, 100, 300);
    assert_eq!(b.apply(&Cmd::EditTrack { id: "trk_two".into(), start: None, end: Some(p(0, 0)) }).unwrap_err()[0].check, "ops_bad_track");
    assert_eq!(routing(&b).tracks.iter().find(|t| t.id == "trk_two").unwrap().pts, vec![p(0, 0), p(1_000, 0)], "a refusal changes nothing");
}

// ----------------------------------------------------------- SetItemNet

#[test]
fn set_item_net_sets_the_net_of_tracks_vias_and_zones_in_one_command() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::SetItemNet { ids: ids(&["trk_a", "via_a", "zone_a"]), net: "VCC".into() }).unwrap();
    assert_eq!(routing(&b).tracks[0].net, "VCC");
    assert_eq!(routing(&b).vias[0].net, "VCC");
    assert_eq!(routing(&b).zones[0].net, "VCC");
    // Everything else of them is as it was.
    assert_eq!(routing(&b).tracks[0].pts.len(), 3);
    assert_eq!(routing(&b).vias[0].diameter, 600);
}

#[test]
fn set_item_net_refuses_an_unknown_net_an_unknown_item_and_no_net_on_copper_that_needs_one() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert_eq!(b.apply(&Cmd::SetItemNet { ids: ids(&["trk_a"]), net: "NOPE".into() }).unwrap_err()[0].check, "ops_unknown_net");
    assert_eq!(b.apply(&Cmd::SetItemNet { ids: ids(&["trk_a", "shp_seg"]), net: "VCC".into() }).unwrap_err()[0].check, "ops_unknown_item", "a shape has no net");
    assert_eq!(routing(&b).tracks[0].net, "GND", "the whole command was refused, the track stays on its net");
    assert_eq!(b.apply(&Cmd::SetItemNet { ids: ids(&["trk_a"]), net: String::new() }).unwrap_err()[0].check, "ops_bad_net");
    assert_eq!(b.apply(&Cmd::SetItemNet { ids: vec![], net: "VCC".into() }).unwrap_err()[0].check, "ops_bad_net");
    // A zone may have none (a rule area has none).
    b.apply(&Cmd::SetItemNet { ids: ids(&["zone_a"]), net: String::new() }).unwrap();
    assert_eq!(routing(&b).zones[0].net, "");
}

// ---------------------------------------------------------- SetZoneName

#[test]
fn set_zone_name_names_the_zone() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::SetZoneName { id: "zone_a".into(), name: "  Ground pour ".into() }).unwrap();
    assert_eq!(routing(&b).zones[0].name, "Ground pour");
    b.apply(&Cmd::SetZoneName { id: "zone_a".into(), name: String::new() }).unwrap();
    assert_eq!(routing(&b).zones[0].name, "", "empty is unnamed");
    assert_eq!(b.apply(&Cmd::SetZoneName { id: "zone_nope".into(), name: "x".into() }).unwrap_err()[0].check, "ops_unknown_zone");
}

// --------------------------------------------------------- ReplaceShape

#[test]
fn replace_shape_changes_the_geometry_in_place_and_keeps_the_id_and_the_lock() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::SetLocked { ids: ids(&["shp_rect"]), locked: true }).unwrap();
    let wider = Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: 100, filled: false, start: p(0, 90_000), end: p(15_000, 95_000) };
    b.apply(&Cmd::ReplaceShape { id: "shp_rect".into(), shape: wider }).unwrap();
    let s = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").expect("the id is kept even though the replacement carried none");
    assert_eq!(s.points(), vec![p(0, 90_000), p(15_000, 95_000)]);
    assert!(drawings(&b).locked_ids.iter().any(|i| i.as_str() == "shp_rect"), "the lock stays");
    assert_eq!(drawings(&b).shapes.len(), 3, "replaced, not added");
    assert_eq!(drawings(&b).shapes[1].id(), "shp_rect", "and it keeps its place in the drawing order");
}

#[test]
fn replace_shape_refuses_a_degenerate_or_unknown_shape() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let seg = |layer: &str, w| Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: w, filled: false, start: p(0, 0), end: p(1_000, 0) };
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: seg("", 150) }).unwrap_err()[0].check, "ops_bad_shape");
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: seg("F.SilkS", -1) }).unwrap_err()[0].check, "ops_bad_shape");
    let poly = Shape::Polygon { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, pts: vec![p(0, 0), p(1_000, 0)] };
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: poly }).unwrap_err()[0].check, "ops_bad_shape");
    let circle = Shape::Circle { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, center: p(5, 5), end: p(5, 5) };
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: circle }).unwrap_err()[0].check, "ops_bad_shape");
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_nope".into(), shape: seg("F.SilkS", 150) }).unwrap_err()[0].check, "ops_unknown_shape");
    assert_eq!(drawings(&b).shapes[0].points(), vec![p(0, 80_000), p(10_000, 80_000)], "refusals change nothing");
}

// ---------------------------------------------------------------- batch

#[test]
fn a_panel_edit_of_several_items_is_one_batch_that_either_all_lands_or_none_does() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(with_arc(), &m, 100, 300);
    let edit = Cmd::Batch {
        cmds: vec![
            Cmd::SetItemNet { ids: ids(&["trk_a"]), net: "VCC".into() },
            Cmd::EditTrack { id: "trk_a".into(), start: Some(p(12_000, 40_000)), end: None },
            Cmd::SetZoneName { id: "zone_a".into(), name: "pour".into() },
        ],
    };
    b.apply(&edit).unwrap();
    assert_eq!(routing(&b).tracks.iter().find(|t| t.id == "trk_a").unwrap().net, "VCC");
    assert_eq!(routing(&b).zones[0].name, "pour");

    let bad = Cmd::Batch { cmds: vec![Cmd::SetZoneName { id: "zone_a".into(), name: "other".into() }, Cmd::EditTrack { id: "trk_nope".into(), start: Some(p(0, 0)), end: None }] };
    assert!(b.apply(&bad).is_err());
    assert_eq!(routing(&b).zones[0].name, "pour", "the first command of a refused batch is taken back");
}
