//! The details of the PCB edit tools (GAPS.md package P6): Paste Special's annotation modes and Clear Nets, the items a command makes while a
//! group is entered joining it, a rotation step other than a quarter turn, the flip direction, and the routing a footprint edit leaves be.

use super::pcb_transform_tests::{board, design, drawings, id, ids, model, p, pose, routing, text, track, via};
use super::*;
use eda_model::ir::{PlacementSection, Provenance};
use eda_model::units::{mm, um};

/// A board with the model of [`design`] but nothing placed or drawn yet: U1 and U2 are parts of the intent that are not on the board.
fn blank(m: &ConstraintModel) -> Board<'_> {
    let mut d = design();
    d.placement = Some(PlacementSection { outline: vec![p(Nm::ZERO, Nm::ZERO), p(mm(100.0), Nm::ZERO), p(mm(100.0), mm(100.0)), p(Nm::ZERO, mm(100.0))], footprints: vec![], modules: vec![] });
    d.routing = None;
    d.drawings = None;
    d.provenance = Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] };
    Board::new(d, m, mm(0.1), mm(0.3))
}

/// KiCad's clipboard text of a Copy of `names` on the fixture board, measured from `reference`.
fn copy(names: &[&str], reference: Point) -> String {
    eda_kicad::export_pcb_clipboard(&design(), &model(&["F.Cu", "B.Cu"]), &ids(names), reference).unwrap()
}

/// What the Reference field of `part` says, when the board's edit of it sets a text of its own.
fn shown_reference(b: &Board<'_>, part: &str) -> Option<String> {
    b.design().footprint_edit(part).and_then(|e| e.reference.as_ref()).and_then(|l| l.text.clone())
}

fn special(text: String, at: Point, mode: PasteMode, clear_nets: bool) -> Cmd {
    Cmd::PasteSpecial { text, at, mode, clear_nets }
}

fn board_parts(b: &Board<'_>) -> Vec<String> {
    drawings(b).board_parts.iter().map(|p| p.reference.clone()).collect()
}

// ------------------------------------------------------------- Paste Special

#[test]
fn a_plain_paste_keeps_the_reference_it_brought_even_when_another_footprint_has_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // U1 and U2 are on this board already. KiCad's default (`PASTE_MODE::KEEP_ANNOTATIONS`) leaves their references as the clipboard has them.
    b.apply(&Cmd::PasteClipboard { text: copy(&["U1", "U2"], p(mm(30.0), mm(20.0))), at: p(mm(10.0), mm(80.0)) }).unwrap();

    assert_eq!(board_parts(&b), vec!["U3", "U4"], "a part is named once, so the copies are filed under free names");
    assert_eq!(shown_reference(&b, "U3").as_deref(), Some("U1"), "but they read U1 and U2, as they do after a paste in KiCad");
    assert_eq!(shown_reference(&b, "U4").as_deref(), Some("U2"));
    assert_eq!(shown_reference(&b, "U1"), None, "the board's own footprints are as they were");
}

#[test]
fn paste_special_unique_gives_a_taken_reference_the_next_number() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&special(copy(&["U1", "U2"], p(mm(30.0), mm(20.0))), p(mm(10.0), mm(80.0)), PasteMode::Unique, false)).unwrap();

    assert_eq!(board_parts(&b), vec!["U3", "U4"], "`ReannotateDuplicates`: U1 -> U3 (U2 is taken), U2 -> U4");
    assert_eq!(shown_reference(&b, "U3"), None, "they read what they are called");
    assert_eq!(shown_reference(&b, "U4"), None);
}

#[test]
fn paste_special_remove_sets_every_pasted_reference_to_ref_stars() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&special(copy(&["U1", "U2"], p(mm(30.0), mm(20.0))), p(mm(10.0), mm(80.0)), PasteMode::Remove, false)).unwrap();

    assert_eq!(board_parts(&b), vec!["U3", "U4"]);
    assert_eq!(shown_reference(&b, "U3").as_deref(), Some("REF**"));
    assert_eq!(shown_reference(&b, "U4").as_deref(), Some("REF**"), "all of them, the way `defaultRef` is set on each footprint");
    assert_eq!(shown_reference(&b, "U1"), None, "and nothing that was already on the board");
}

#[test]
fn paste_special_keep_is_the_default_of_a_command_that_leaves_the_mode_out() {
    let c: Cmd = serde_json::from_str(r#"{"op":"paste_special","text":"(kicad_pcb)","at":{"x":1000,"y":2000}}"#).unwrap();
    assert_eq!(c, special("(kicad_pcb)".into(), p(um(1.0), um(2.0)), PasteMode::Keep, false));
    let c: Cmd = serde_json::from_str(r#"{"op":"paste_special","text":"x","at":{"x":0,"y":0},"mode":"remove","clear_nets":true}"#).unwrap();
    assert_eq!(c, special("x".into(), p(Nm::ZERO, Nm::ZERO), PasteMode::Remove, true));
    assert_eq!(serde_json::to_value(PasteMode::Unique).unwrap(), "unique");
    assert_eq!(c.subjects(), vec!["paste"]);
    assert!(!c.clears_routing());
}

#[test]
fn a_reference_nobody_has_is_the_reference_in_every_mode_and_remove_still_clears_the_text() {
    let m = model(&["F.Cu", "B.Cu"]);
    let text = copy(&["U1"], p(mm(30.0), mm(20.0)));
    for (mode, shown) in [(PasteMode::Keep, None), (PasteMode::Unique, None), (PasteMode::Remove, Some("REF**"))] {
        // U1 is a part of the intent that is not on this board: it is placed again under its own name.
        let mut b = blank(&m);
        b.apply(&special(text.clone(), p(mm(40.0), mm(50.0)), mode, false)).unwrap();
        assert_eq!(pose(&b, "U1").at, p(mm(40.0), mm(50.0)), "{mode:?}");
        assert!(board_parts(&b).is_empty(), "{mode:?}: no second part");
        assert_eq!(shown_reference(&b, "U1").as_deref(), shown, "{mode:?}");
    }
}

#[test]
fn clear_nets_puts_pasted_copper_zones_and_pads_on_no_net() {
    let m = model(&["F.Cu", "B.Cu"]);
    let names = ["trk_a", "via_a", "zone_a", "U1", "U2"];
    // Without it the nets are mapped by name.
    let mut kept = board(&m);
    kept.apply(&special(copy(&names, p(Nm::ZERO, Nm::ZERO)), p(Nm::ZERO, Nm::ZERO), PasteMode::Unique, false)).unwrap();
    assert!(routing(&kept).tracks.iter().filter(|t| t.id != "trk_a").all(|t| t.net == "GND"), "{:?}", routing(&kept).tracks);
    assert!(routing(&kept).vias.iter().any(|v| v.id != "via_a" && v.net == "GND"));
    assert!(routing(&kept).zones.iter().any(|z| z.id != "zone_a" && z.net == "GND"));
    assert_eq!(drawings(&kept).board_parts[0].pad_nets.len(), 2, "U1's pads are on GND and VCC");

    // `item->SetNet( NETINFO_LIST::OrphanedItem() )` for every connected item.
    let mut cleared = board(&m);
    cleared.apply(&special(copy(&names, p(Nm::ZERO, Nm::ZERO)), p(Nm::ZERO, Nm::ZERO), PasteMode::Unique, true)).unwrap();
    let new_tracks: Vec<_> = routing(&cleared).tracks.iter().filter(|t| t.id != "trk_a").collect();
    assert!(!new_tracks.is_empty() && new_tracks.iter().all(|t| t.net.is_empty()), "{new_tracks:?}");
    assert!(routing(&cleared).vias.iter().filter(|v| v.id != "via_a").all(|v| v.net.is_empty()));
    assert!(routing(&cleared).zones.iter().filter(|z| z.id != "zone_a").all(|z| z.net.is_empty()));
    assert!(drawings(&cleared).board_parts.iter().all(|p| p.pad_nets.is_empty()), "and the pads of the footprints brought with them");
    // What was on the board is untouched.
    assert_eq!(routing(&cleared).tracks.iter().find(|t| t.id == "trk_a").unwrap().net, "GND");
}

#[test]
fn clear_nets_changes_nothing_for_a_footprint_copied_alone() {
    let m = model(&["F.Cu", "B.Cu"]);
    let text = copy(&["U1"], p(mm(30.0), mm(20.0)));
    assert!(text.trim_start().starts_with("(footprint"), "a lone footprint is a bare footprint on the clipboard");
    let (mut with, mut without) = (board(&m), board(&m));
    with.apply(&special(text.clone(), p(mm(10.0), mm(80.0)), PasteMode::Keep, true)).unwrap();
    without.apply(&special(text, p(mm(10.0), mm(80.0)), PasteMode::Keep, false)).unwrap();
    assert_eq!(drawings(&with).board_parts, drawings(&without).board_parts);
    assert!(drawings(&with).board_parts[0].pad_nets.is_empty());
}

// ------------------------------------------------------------- items made while a group is entered

fn grouped() -> (ConstraintModel, Design) {
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: "block".into(), member_ids: ids(&["shp_seg", "txt_a"]), design_block: String::new() });
    (model(&["F.Cu", "B.Cu"]), d)
}

fn members(b: &Board<'_>, group: &str) -> Vec<String> {
    drawings(b).groups.iter().find(|g| g.id == group).map(|g| g.member_ids.clone()).unwrap_or_default()
}

#[test]
fn what_a_command_adds_while_a_group_is_entered_joins_it() {
    let (m, d) = grouped();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let in_group = |cmd: Cmd| Cmd::InGroup { group: id("grp_a"), cmd: Box::new(cmd) };

    b.apply(&in_group(Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: p(mm(1.0), mm(1.0)), end: p(mm(5.0), mm(1.0)) } })).unwrap();
    b.apply(&in_group(Cmd::AddTrack { net: "GND".into(), layer: "F.Cu".into(), width: mm(0.2), pts: vec![p(mm(70.0), mm(70.0)), p(mm(75.0), mm(70.0))] })).unwrap();
    b.apply(&in_group(Cmd::AddVia { net: "GND".into(), x: mm(75.0), y: mm(70.0), drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() })).unwrap();
    b.apply(&in_group(Cmd::AddText { text: text("", "F.SilkS", (mm(5.0), mm(5.0)), 0) })).unwrap();

    let held = members(&b, "grp_a");
    assert_eq!(held.len(), 6, "{held:?}");
    let new_shape = drawings(&b).shapes.iter().map(|s| s.id().to_string()).find(|i| !["shp_seg", "shp_rect", "shp_arc"].contains(&i.as_str())).unwrap();
    let new_track = routing(&b).tracks.iter().map(|t| t.id.clone()).find(|i| i != "trk_a").unwrap();
    let new_via = routing(&b).vias.iter().map(|v| v.id.clone()).find(|i| i != "via_a").unwrap();
    for item in [new_shape, new_track, new_via] {
        assert!(held.contains(&item), "{item} joined: {held:?}");
    }
    assert!(held.contains(&"shp_seg".to_string()) && held.contains(&"txt_a".to_string()), "and the old members stay");
    // The same command with no group entered leaves the new item outside every group.
    let mut free = Board::new(grouped().1, &m, mm(0.1), mm(0.3));
    free.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: p(mm(1.0), mm(1.0)), end: p(mm(5.0), mm(1.0)) } }).unwrap();
    assert_eq!(members(&free, "grp_a").len(), 2);
}

#[test]
fn a_paste_made_while_a_group_is_entered_joins_it_with_a_pasted_group_as_one_member() {
    let (m, mut d) = grouped();
    let mut source = design();
    source.drawings.as_mut().unwrap().groups.push(Group { id: "grp_src".into(), name: "src".into(), member_ids: ids(&["dim_a", "shp_rect"]), design_block: String::new() });
    let text = eda_kicad::export_pcb_clipboard(&source, &m, &ids(&["grp_src", "trk_a"]), p(Nm::ZERO, Nm::ZERO)).unwrap();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_b".into(), name: "other".into(), member_ids: ids(&["shp_arc", "dim_a"]), design_block: String::new() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::InGroup { group: id("grp_a"), cmd: Box::new(Cmd::PasteClipboard { text, at: p(mm(2.0), mm(2.0)) }) }).unwrap();

    let held = members(&b, "grp_a");
    let groups = &drawings(&b).groups;
    let pasted_group = groups.iter().find(|g| g.name == "src").expect("the pasted group is a group again");
    assert!(held.contains(&pasted_group.id), "the pasted group joined as one member: {held:?}");
    assert!(pasted_group.member_ids.iter().all(|m| !held.contains(m)), "its members stay in it, not in the entered group");
    let pasted_tracks: Vec<_> = routing(&b).tracks.iter().filter(|t| t.id != "trk_a").map(|t| t.id.clone()).collect();
    assert!(!pasted_tracks.is_empty() && pasted_tracks.iter().all(|t| held.contains(t)), "the pasted track did: {held:?}");
    assert_eq!(members(&b, "grp_b"), ids(&["shp_arc", "dim_a"]), "another group is not touched");
}

#[test]
fn a_duplicate_of_a_member_of_an_inner_group_joins_that_group_not_the_entered_one() {
    let (m, mut d) = grouped();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_in".into(), name: String::new(), member_ids: ids(&["shp_arc", "dim_a"]), design_block: String::new() });
    d.drawings.as_mut().unwrap().groups[0].member_ids.push(id("grp_in"));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::InGroup { group: id("grp_a"), cmd: Box::new(Cmd::Duplicate { ids: ids(&["shp_arc"]) }) }).unwrap();

    assert_eq!(members(&b, "grp_in").len(), 3, "a duplicate joins its original's group (`BOARD_ITEM::Duplicate( addToParentGroup )`)");
    assert_eq!(members(&b, "grp_a").len(), 3, "so the entered group gained nothing: shp_seg, txt_a and the inner group");
}

#[test]
fn a_group_that_is_gone_is_not_joined_and_a_refused_command_changes_nothing() {
    let (m, d) = grouped();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::InGroup { group: id("grp_gone"), cmd: Box::new(Cmd::AddText { text: text("", "F.SilkS", (mm(5.0), mm(5.0)), 0) }) }).unwrap();
    assert_eq!(drawings(&b).texts.len(), 2, "the command itself ran");
    assert_eq!(members(&b, "grp_a").len(), 2);

    let before = serde_json::to_string(b.design()).unwrap();
    let refused = Cmd::InGroup { group: id("grp_a"), cmd: Box::new(Cmd::DeleteTrack { id: id("nope") }) };
    assert!(b.apply(&refused).is_err());
    assert_eq!(serde_json::to_string(b.design()).unwrap(), before);
}

#[test]
fn in_group_wraps_a_command_without_changing_what_it_is() {
    let inner = Cmd::MoveItems { ids: ids(&["U1"]), dx: Nm::new(1), dy: um(2.0) };
    let wrapped = Cmd::InGroup { group: id("g"), cmd: Box::new(inner.clone()) };
    assert_eq!((wrapped.domain(), wrapped.subjects(), wrapped.edits_connectivity()), (inner.domain(), inner.subjects(), inner.edits_connectivity()));
    let back: Cmd = serde_json::from_str(&serde_json::to_string(&wrapped).unwrap()).unwrap();
    assert_eq!(back, wrapped);
    let from_studio: Cmd = serde_json::from_str(r#"{"op":"in_group","group":"g","cmd":{"op":"delete_track","id":"t"}}"#).unwrap();
    assert_eq!(from_studio, Cmd::InGroup { group: id("g"), cmd: Box::new(Cmd::DeleteTrack { id: id("t") }) });
}

// ------------------------------------------------------------- the rotation step and the flip direction

/// Every point of the fixture board's copper and graphics (a footprint lands on the placement grid, so it is compared on its own), to compare two boards.
fn points(b: &Board<'_>) -> Vec<Point> {
    let mut out: Vec<Point> = routing(b).tracks.iter().flat_map(|t| t.pts.clone()).collect();
    out.extend(routing(b).vias.iter().map(|v| v.at));
    out.extend(routing(b).zones.iter().flat_map(|z| z.outline.clone()));
    out.extend(drawings(b).shapes.iter().flat_map(|s| s.points()));
    out.extend(drawings(b).texts.iter().map(|t| t.at));
    out
}

#[test]
fn a_rotation_step_that_is_not_a_quarter_turn_turns_every_kind_by_exactly_that_angle() {
    let m = model(&["F.Cu", "B.Cu"]);
    let pivot = p(mm(50.0), mm(50.0));
    // R presses at a 15 degree step: counter-clockwise on the screen, so the verb (clockwise positive) gets minus the step.
    let turn = |b: &mut Board<'_>, millideg: i64| b.apply(&Cmd::RotateItems { ids: everything_but_the_rect(), pivot, angle_millideg: millideg }).unwrap();
    let mut once = board(&m);
    turn(&mut once, -15_000);
    // The footprints turn by the step, in KiCad's sense, and text angles by it too.
    assert_eq!(pose(&once, "U1").rot, 345_000, "a footprint at 0 turned 15 degrees counter-clockwise reads 345 clockwise");
    assert_eq!(pose(&once, "U2").rot, 75_000);
    assert_eq!(drawings(&once).texts[0].angle, 15_000, "text runs the way KiCad's angles run");
    // 24 presses at 15 degrees are a full turn: every point is where it was (a nanometre of rounding per press at most), a footprint's
    // angle is exactly what it was and its position is within a grid step of it (`Board::set_pose` snaps every pose to the placement grid).
    let mut full = board(&m);
    for _ in 0..24 {
        turn(&mut full, -15_000);
    }
    for (a, b) in points(&full).iter().zip(points(&board(&m)).iter()) {
        assert!((a.x - b.x).abs() <= Nm::new(24) && (a.y - b.y).abs() <= Nm::new(24), "{a:?} vs {b:?}");
    }
    for r in ["U1", "U2"] {
        let (was, now) = (pose(&board(&m), r), pose(&full, r));
        assert_eq!(now.rot, was.rot, "{r}");
        assert!((now.at.x - was.at.x).abs() <= mm(0.3) && (now.at.y - was.at.y).abs() <= mm(0.3), "{r}: {:?} vs {:?}", now.at, was.at);
    }
    // Two presses at 15 are one at 30 (within rounding).
    let (mut two, mut one) = (board(&m), board(&m));
    turn(&mut two, -15_000);
    turn(&mut two, -15_000);
    turn(&mut one, -30_000);
    for (a, b) in points(&two).iter().zip(points(&one).iter()) {
        assert!((a.x - b.x).abs() <= Nm::new(2) && (a.y - b.y).abs() <= Nm::new(2), "{a:?} vs {b:?}");
    }
}

fn everything_but_the_rect() -> Vec<String> {
    ids(&["U1", "U2", "trk_a", "via_a", "zone_a", "shp_seg", "shp_arc", "txt_a", "dim_a"])
}

#[test]
fn a_rectangle_turned_by_a_step_that_is_not_a_quarter_turn_becomes_a_polygon_and_stays_a_rectangle_on_a_quarter() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(mm(5.0), mm(92.5)), angle_millideg: -45_000 }).unwrap();
    assert!(matches!(drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect"), Some(Shape::Polygon { pts, .. }) if pts.len() == 4));
    let mut q = board(&m);
    q.apply(&Cmd::RotateItems { ids: ids(&["shp_rect"]), pivot: p(mm(5.0), mm(92.5)), angle_millideg: -90_000 }).unwrap();
    assert!(matches!(drawings(&q).shapes.iter().find(|s| s.id() == "shp_rect"), Some(Shape::Rect { .. })));
}

#[test]
fn the_flip_direction_decides_which_axis_an_item_is_mirrored_across() {
    let m = model(&["F.Cu", "B.Cu"]);
    let items = ids(&["trk_a", "via_a", "zone_a", "shp_seg", "txt_a", "U1"]);
    let mut lr = board(&m);
    let mut tb = board(&m);
    lr.apply(&Cmd::FlipItems { ids: items.clone(), pivot: p(mm(50.0), mm(50.0)), direction: FlipDirection::LeftRight }).unwrap();
    tb.apply(&Cmd::FlipItems { ids: items, pivot: p(mm(50.0), mm(50.0)), direction: FlipDirection::TopBottom }).unwrap();

    // trk_a ran from (10 000, 40 000): left/right keeps y and mirrors x, top/bottom the other way round.
    assert_eq!(routing(&lr).tracks[0].pts[0], p(mm(90.0), mm(40.0)));
    assert_eq!(routing(&tb).tracks[0].pts[0], p(mm(10.0), mm(60.0)));
    assert_eq!((routing(&lr).vias[0].at, routing(&tb).vias[0].at), (p(mm(80.0), mm(50.0)), p(mm(20.0), mm(50.0))));
    // Either way the layer goes to the other side, and so does the footprint.
    assert_eq!((routing(&lr).tracks[0].layer.as_str(), routing(&tb).tracks[0].layer.as_str()), ("B.Cu", "B.Cu"));
    assert_eq!((pose(&lr, "U1").side, pose(&tb, "U1").side), (Side::Bottom, Side::Bottom));
    // Text: -angle for left/right, 180 - angle for top/bottom (an angle of 30 degrees shows it).
    let mut turned = design();
    turned.drawings.as_mut().unwrap().texts[0].angle = 30_000;
    let (mut a, mut c) = (Board::new(turned.clone(), &m, mm(0.1), mm(0.3)), Board::new(turned, &m, mm(0.1), mm(0.3)));
    a.apply(&Cmd::FlipItems { ids: ids(&["txt_a"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::LeftRight }).unwrap();
    c.apply(&Cmd::FlipItems { ids: ids(&["txt_a"]), pivot: p(Nm::ZERO, Nm::ZERO), direction: FlipDirection::TopBottom }).unwrap();
    assert_eq!((drawings(&a).texts[0].angle, drawings(&c).texts[0].angle), (330_000, 150_000));
}

// ------------------------------------------------------------- the routing stays

#[test]
fn a_routed_footprint_moved_turned_flipped_or_deleted_leaves_the_routing_in_the_design() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.routing.as_mut().unwrap().tracks.push(track("trk_u1", "F.Cu", &[(mm(31.5), mm(20.0)), (mm(40.0), mm(20.0))]));
    d.routing.as_mut().unwrap().vias.push(via("via_u1", (mm(31.5), mm(20.0)), "F.Cu", "B.Cu"));
    let b = Board::new(d, &m, mm(0.1), mm(0.3));
    for cmd in [
        Cmd::MoveItems { ids: ids(&["U1"]), dx: mm(5.0), dy: Nm::ZERO },
        Cmd::RotateItems { ids: ids(&["U1"]), pivot: p(mm(30.0), mm(20.0)), angle_millideg: -15_000 },
        Cmd::FlipItems { ids: ids(&["U1"]), pivot: p(mm(30.0), mm(20.0)), direction: FlipDirection::LeftRight },
        Cmd::Rip { part: id("U1") },
        Cmd::CreateArray { ids: ids(&["U1"]), geometry: ArrayGeometry::Grid { nx: 2, ny: 1, dx: mm(5.0), dy: Nm::ZERO, offset_x: Nm::ZERO, offset_y: Nm::ZERO, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false }, arrange: false, reannotate: true },
    ] {
        assert!(!cmd.clears_routing_in(&b), "{cmd:?}");
    }
    // The verbs of the CLI and the agents answer as they did.
    assert!(Cmd::Nudge { part: id("U1"), dir: Dir::East, steps: 1 }.clears_routing_in(&b));
    assert!(Cmd::PlaceAt { part: id("U1"), x: Nm::ZERO, y: Nm::ZERO }.clears_routing_in(&b));
}
