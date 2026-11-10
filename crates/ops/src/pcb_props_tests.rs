//! The Properties panel's own verbs (`pcb_props.rs`): a track's end points, the net of tracks, vias and zones, a zone's name and a
//! shape's geometry. Everything else the panel edits goes through verbs that had their tests before it.

use super::pcb_transform_tests::{board, design, drawings, ids, model, p, routing, track};
use super::*;
use eda_model::ir::TextVAlign;
use eda_model::units::{mm, um};

fn with_arc() -> Design {
    let mut d = design();
    let arc = Track::new_arc("GND".into(), "F.Cu".into(), mm(0.2), p(mm(10.0), mm(10.0)), p(mm(15.0), mm(5.0)), p(mm(20.0), mm(10.0)));
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_arc".into(), ..arc });
    d
}

// ------------------------------------------------------------ EditTrack

#[test]
fn edit_track_moves_the_first_and_the_last_point_and_nothing_else() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::EditTrack { id: "trk_a".into(), start: Some(p(mm(11.0), mm(41.0))), end: None }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(mm(11.0), mm(41.0)), p(mm(20.0), mm(40.0)), p(mm(20.0), mm(50.0))], "only the first vertex moves");
    b.apply(&Cmd::EditTrack { id: "trk_a".into(), start: None, end: Some(p(mm(21.0), mm(52.0))) }).unwrap();
    assert_eq!(routing(&b).tracks[0].pts, vec![p(mm(11.0), mm(41.0)), p(mm(20.0), mm(40.0)), p(mm(21.0), mm(52.0))], "and the last one, the bend stays");
    let t = &routing(&b).tracks[0];
    assert_eq!((t.id.as_str(), t.net.as_str(), t.layer.as_str(), t.width), ("trk_a", "GND", "F.Cu", mm(0.2)), "id, net, layer and width are untouched");
}

#[test]
fn edit_track_keeps_the_mid_point_of_an_arc_and_draws_it_through_the_new_end() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(with_arc(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::EditTrack { id: "trk_arc".into(), start: None, end: Some(p(mm(25.0), mm(12.0))) }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(mm(10.0), mm(10.0)), p(mm(15.0), mm(5.0)), p(mm(25.0), mm(12.0)))), "still an arc, through the same mid point");
    b.apply(&Cmd::EditTrack { id: "trk_arc".into(), start: Some(p(mm(9.0), mm(9.0))), end: None }).unwrap();
    let t = routing(&b).tracks.iter().find(|t| t.id == "trk_arc").unwrap();
    assert_eq!(t.arc(), Some((p(mm(9.0), mm(9.0)), p(mm(15.0), mm(5.0)), p(mm(25.0), mm(12.0)))));
}

#[test]
fn edit_track_refuses_what_is_not_a_track_edit() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert_eq!(b.apply(&Cmd::EditTrack { id: "trk_nope".into(), start: Some(p(Nm::ZERO, Nm::ZERO)), end: None }).unwrap_err()[0].check, "ops_unknown_track");
    assert_eq!(b.apply(&Cmd::EditTrack { id: "trk_a".into(), start: None, end: None }).unwrap_err()[0].check, "ops_bad_track");
    assert_eq!(b.apply(&Cmd::EditTrack { id: "via_a".into(), start: Some(p(Nm::ZERO, Nm::ZERO)), end: None }).unwrap_err()[0].check, "ops_unknown_track", "a via has no end points");

    // A two-point track whose two ends meet is no track.
    let mut d = design();
    d.routing.as_mut().unwrap().tracks.push(track("trk_two", "F.Cu", &[(Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO)]));
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    assert_eq!(b.apply(&Cmd::EditTrack { id: "trk_two".into(), start: None, end: Some(p(Nm::ZERO, Nm::ZERO)) }).unwrap_err()[0].check, "ops_bad_track");
    assert_eq!(routing(&b).tracks.iter().find(|t| t.id == "trk_two").unwrap().pts, vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)], "a refusal changes nothing");
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
    assert_eq!(routing(&b).vias[0].diameter, mm(0.6));
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
    let wider = Shape::Rect { id: String::new(), layer: "F.Fab".into(), stroke_width: mm(0.1), filled: false, start: p(Nm::ZERO, mm(90.0)), end: p(mm(15.0), mm(95.0)) };
    b.apply(&Cmd::ReplaceShape { id: "shp_rect".into(), shape: wider }).unwrap();
    let s = drawings(&b).shapes.iter().find(|s| s.id() == "shp_rect").expect("the id is kept even though the replacement carried none");
    assert_eq!(s.points(), vec![p(Nm::ZERO, mm(90.0)), p(mm(15.0), mm(95.0))]);
    assert!(drawings(&b).locked_ids.iter().any(|i| i.as_str() == "shp_rect"), "the lock stays");
    assert_eq!(drawings(&b).shapes.len(), 3, "replaced, not added");
    assert_eq!(drawings(&b).shapes[1].id(), "shp_rect", "and it keeps its place in the drawing order");
}

#[test]
fn replace_shape_refuses_a_degenerate_or_unknown_shape() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let seg = |layer: &str, w| Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: w, filled: false, start: p(Nm::ZERO, Nm::ZERO), end: p(mm(1.0), Nm::ZERO) };
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: seg("", mm(0.15)) }).unwrap_err()[0].check, "ops_bad_shape");
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: seg("F.SilkS", Nm::new(-1)) }).unwrap_err()[0].check, "ops_bad_shape");
    let poly = Shape::Polygon { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, pts: vec![p(Nm::ZERO, Nm::ZERO), p(mm(1.0), Nm::ZERO)] };
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: poly }).unwrap_err()[0].check, "ops_bad_shape");
    let circle = Shape::Circle { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, center: p(um(5.0), um(5.0)), end: p(um(5.0), um(5.0)) };
    // A circle of radius 0 is the Properties panel's (`SetRadius( 0 )`, UI-LEFT-2 PP8): taken, then put back below.
    b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: circle }).unwrap();
    b.apply(&Cmd::ReplaceShape { id: "shp_seg".into(), shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: mm(0.15), filled: false, start: p(Nm::ZERO, mm(80.0)), end: p(mm(10.0), mm(80.0)) } }).unwrap();
    assert_eq!(b.apply(&Cmd::ReplaceShape { id: "shp_nope".into(), shape: seg("F.SilkS", mm(0.15)) }).unwrap_err()[0].check, "ops_unknown_shape");
    assert_eq!(drawings(&b).shapes[0].points(), vec![p(Nm::ZERO, mm(80.0)), p(mm(10.0), mm(80.0))], "refusals change nothing");
}

// ---------------------------------------------------------------- batch

#[test]
fn a_panel_edit_of_several_items_is_one_batch_that_either_all_lands_or_none_does() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(with_arc(), &m, mm(0.1), mm(0.3));
    let edit = Cmd::Batch {
        cmds: vec![
            Cmd::SetItemNet { ids: ids(&["trk_a"]), net: "VCC".into() },
            Cmd::EditTrack { id: "trk_a".into(), start: Some(p(mm(12.0), mm(40.0))), end: None },
            Cmd::SetZoneName { id: "zone_a".into(), name: "pour".into() },
        ],
    };
    b.apply(&edit).unwrap();
    assert_eq!(routing(&b).tracks.iter().find(|t| t.id == "trk_a").unwrap().net, "VCC");
    assert_eq!(routing(&b).zones[0].name, "pour");

    let bad = Cmd::Batch { cmds: vec![Cmd::SetZoneName { id: "zone_a".into(), name: "other".into() }, Cmd::EditTrack { id: "trk_nope".into(), start: Some(p(Nm::ZERO, Nm::ZERO)), end: None }] };
    assert!(b.apply(&bad).is_err());
    assert_eq!(routing(&b).zones[0].name, "pour", "the first command of a refused batch is taken back");
}

// ------------------------------------------------------------------- the rows of UI-LEFT P2 (task-u1-props)

#[test]
fn edit_text_style_keeps_the_texts_italic_bold_valign_width_knockout_and_automatic_pen_in_its_text_extra() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let style = |width_nm, valign, bold, italic, knockout, auto_thickness| Cmd::EditTextStyle { id: "txt_a".into(), width_nm, valign, bold, italic, knockout, auto_thickness, face: None };
    b.apply(&style(Some(mm(1.5)), Some(TextVAlign::Bottom), None, Some(true), Some(true), None)).unwrap();
    let x = drawings(&b).text_extra("txt_a").unwrap().clone();
    assert_eq!((x.width_nm, x.valign, x.italic, x.knockout, x.bold), (Some(mm(1.5)), TextVAlign::Bottom, true, true, false));
    // A field left out is unchanged; the automatic pen is GetPenSizeForBold of the smaller size once the text is bold.
    b.apply(&style(None, None, Some(true), None, None, Some(true))).unwrap();
    let x = drawings(&b).text_extra("txt_a").unwrap().clone();
    assert!(x.italic && x.bold && x.auto_thickness);
    assert_eq!(drawings(&b).texts[0].stroke_width, mm(0.2), "a fifth of the 1 mm height (the width is 1.5 mm)");
    // A width equal to the height keeps no width; nothing left over keeps no entry.
    b.apply(&style(Some(mm(1.0)), Some(TextVAlign::Center), Some(false), Some(false), Some(false), Some(false))).unwrap();
    assert!(drawings(&b).text_extra("txt_a").is_none(), "{:?}", drawings(&b).text_extra("txt_a"));
    assert_eq!(b.apply(&style(None, None, None, None, None, None)).unwrap_err()[0].check, "ops_nothing_to_change");
    assert_eq!(b.apply(&Cmd::EditTextStyle { id: "txt_nope".into(), width_nm: None, valign: None, bold: Some(true), italic: None, knockout: None, auto_thickness: None, face: None }).unwrap_err()[0].check, "ops_unknown_text");
    assert_eq!(b.apply(&style(Some(Nm::ZERO), None, None, None, None, None)).unwrap_err()[0].check, "ops_bad_text");
}

/// The Properties panel's Font row (`EDA_TEXT::SetFont`, UI-LEFT-2 CC1): a face is kept in the text's extra (the file's `(face ..)`); "KiCad Font" or nothing is the stroke
/// font again, which keeps no face; the same face twice changes nothing.
#[test]
fn edit_text_style_sets_and_clears_the_texts_face() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let face = |f: &str| Cmd::EditTextStyle { id: "txt_a".into(), width_nm: None, valign: None, bold: None, italic: None, knockout: None, auto_thickness: None, face: Some(f.into()) };
    b.apply(&face("DejaVu Sans")).unwrap();
    assert_eq!(drawings(&b).text_extra("txt_a").and_then(|x| x.face.clone()).as_deref(), Some("DejaVu Sans"));
    assert_eq!(b.apply(&face("DejaVu Sans")).unwrap_err()[0].check, "ops_nothing_to_change");
    b.apply(&face("KiCad Font")).unwrap();
    assert!(drawings(&b).text_extra("txt_a").is_none(), "the stroke font keeps no face and nothing else is left");
}

#[test]
fn set_solder_mask_opens_the_mask_of_tracks_and_shapes_on_outer_copper_only() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().shapes.push(Shape::Rect { id: "shp_cu".into(), layer: "F.Cu".into(), stroke_width: mm(0.1), filled: true, start: p(Nm::ZERO, Nm::ZERO), end: p(mm(4.0), mm(2.0)) });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SetSolderMask { ids: ids(&["trk_a", "shp_cu"]), solder_mask: true, margin: Some(mm(0.05)) }).unwrap();
    assert_eq!(drawings(&b).track_mask("trk_a").map(|t| t.margin), Some(Some(mm(0.05))));
    let st = drawings(&b).board_items.style_of("shp_cu").unwrap();
    assert_eq!((st.solder_mask, st.solder_mask_margin), (true, Some(mm(0.05))));
    // Taking the opening away takes the margin with it, and an empty style is not kept.
    b.apply(&Cmd::SetSolderMask { ids: ids(&["trk_a", "shp_cu"]), solder_mask: false, margin: Some(mm(0.05)) }).unwrap();
    assert!(drawings(&b).track_mask("trk_a").is_none() && drawings(&b).board_items.style_of("shp_cu").is_none());
    assert_eq!(b.apply(&Cmd::SetSolderMask { ids: ids(&["shp_seg"]), solder_mask: true, margin: None }).unwrap_err()[0].check, "ops_bad_mask", "a silkscreen line has no mask opening");
    assert_eq!(b.apply(&Cmd::SetSolderMask { ids: ids(&["trk_a"]), solder_mask: false, margin: None }).unwrap_err()[0].check, "ops_nothing_to_change");
}

#[test]
fn set_corner_radius_rounds_a_rectangle_up_to_half_its_shorter_side() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::SetCornerRadius { ids: ids(&["shp_rect"]), radius: mm(1.0) }).unwrap();
    assert_eq!(drawings(&b).board_items.corner_radius_of("shp_rect"), mm(1.0));
    assert_eq!(b.apply(&Cmd::SetCornerRadius { ids: ids(&["shp_rect"]), radius: mm(2.6) }).unwrap_err()[0].check, "ops_bad_shape", "the rectangle is 5 mm tall");
    assert_eq!(b.apply(&Cmd::SetCornerRadius { ids: ids(&["shp_seg"]), radius: mm(1.0) }).unwrap_err()[0].check, "ops_bad_shape");
    b.apply(&Cmd::SetCornerRadius { ids: ids(&["shp_rect"]), radius: Nm::ZERO }).unwrap();
    assert!(drawings(&b).board_items.style_of("shp_rect").is_none());
}

#[test]
fn set_via_props_writes_the_tokens_kicad_writes_and_the_drcs_tenting_follows() {
    use eda_model::via_extra::{PostMachiningMode, PostMachiningProps, ViaDrillProps, ViaProps};
    let m = model(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
    let mut b = board(&m);
    let props = ViaProps {
        front_tenting: Some(true),
        back_tenting: Some(false),
        filling: Some(true),
        backdrill: Some(ViaDrillProps { size: mm(0.45), start: "B.Cu".into(), end: "In2.Cu".into() }),
        back_post_machining: Some(PostMachiningProps { mode: PostMachiningMode::Counterbore, size: mm(0.9), depth: mm(0.3), angle_ddeg: 0 }),
        ..Default::default()
    };
    b.apply(&Cmd::SetViaProps { id: "via_a".into(), props: props.clone() }).unwrap();
    let e = drawings(&b).via_extra("via_a").unwrap();
    assert_eq!(e.props(), props);
    assert_eq!(e.tokens, ["(backdrill (size 0.45) (layers \"B.Cu\" \"In2.Cu\"))", "(back_post_machining counterbore (size 0.9) (depth 0.3))", "(tenting (front yes) (back no))", "(filling yes)"]);
    let t = &drawings(&b).via_tenting;
    assert_eq!((t.len(), t[0].front, t[0].back), (1, Some(true), Some(false)));
    // Back to the board's stackup for all of it: no entry, no tenting row.
    b.apply(&Cmd::SetViaProps { id: "via_a".into(), props: ViaProps::default() }).unwrap();
    assert!(drawings(&b).via_extra("via_a").is_none() && drawings(&b).via_tenting.is_empty());
    let bad = ViaProps { backdrill: Some(ViaDrillProps { size: mm(0.45), start: "B.Cu".into(), end: "In7.Cu".into() }), ..Default::default() };
    assert_eq!(b.apply(&Cmd::SetViaProps { id: "via_a".into(), props: bad }).unwrap_err()[0].hint.as_deref(), Some("Via layer is outside the board stack."));
    assert_eq!(b.apply(&Cmd::SetViaProps { id: "via_a".into(), props: ViaProps::default() }).unwrap_err()[0].check, "ops_nothing_to_change");
    assert_eq!(b.apply(&Cmd::SetViaProps { id: "via_nope".into(), props: props.clone() }).unwrap_err()[0].check, "ops_unknown_via");
}

#[test]
fn set_teardrop_params_gives_vias_and_pads_their_own_and_none_gives_them_back_to_board_setup() {
    use eda_model::teardrop_params::TeardropParams;
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let on = TeardropParams { enabled: true, best_length_ratio: 0.8, max_length: mm(2.0), ..Default::default() };
    b.apply(&Cmd::SetTeardropParams { ids: ids(&["via_a", "U1.1"]), params: Some(on) }).unwrap();
    assert_eq!(drawings(&b).via_extra("via_a").and_then(|e| e.teardrops()), Some(on), "the via's (teardrops ..) token");
    assert_eq!(b.design().footprint_edit("U1").and_then(|e| e.pad("1", 1)).and_then(|p| p.teardrops), Some(on), "the pad's edit");
    // KiCad's defaults (off) are still the item's own: Board Setup does not serve it any more.
    b.apply(&Cmd::SetTeardropParams { ids: ids(&["via_a"]), params: Some(TeardropParams::default()) }).unwrap();
    assert_eq!(drawings(&b).via_extra("via_a").and_then(|e| e.teardrops()), Some(TeardropParams::default()));
    // None takes them away.
    b.apply(&Cmd::SetTeardropParams { ids: ids(&["via_a", "U1.1"]), params: None }).unwrap();
    assert!(drawings(&b).via_extra("via_a").is_none(), "a via left with nothing of its own keeps no extra");
    assert!(b.design().footprint_edit("U1").is_none(), "nor a footprint an edit");
    assert_eq!(b.apply(&Cmd::SetTeardropParams { ids: ids(&["via_a"]), params: None }).unwrap_err()[0].check, "ops_nothing_to_change");
    assert_eq!(b.apply(&Cmd::SetTeardropParams { ids: ids(&["U1.9"]), params: Some(on) }).unwrap_err()[0].check, "ops_unknown_pad");
    assert_eq!(b.apply(&Cmd::SetTeardropParams { ids: ids(&["nothing"]), params: Some(on) }).unwrap_err()[0].check, "ops_unknown_item");
    let bad = TeardropParams { best_length_ratio: -0.1, ..on };
    assert_eq!(b.apply(&Cmd::SetTeardropParams { ids: ids(&["via_a"]), params: Some(bad) }).unwrap_err()[0].check, "ops_bad_teardrops");
}

#[test]
fn set_footprint_paste_margin_edits_a_board_files_footprint_tokens_and_a_footprint_placed_from_the_intent_its_edit() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    // (a record that holds the file's form: read from a footprint of that name)
    d.drawings.as_mut().unwrap().footprint_extras.push(eda_model::ir::FootprintExtra { id: "U1".into(), tokens: vec!["(descr \"x\")".into(), "(solder_paste_margin -0.1)".into()], source_name: "Lib:U1".into(), ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SetFootprintPasteMargin { part: "U1".into(), margin: Some(um(-50.0)), ratio: Some(-0.05) }).unwrap();
    let e = drawings(&b).footprint_extras.iter().find(|e| e.id == "U1").unwrap();
    assert_eq!(e.tokens, ["(descr \"x\")", "(solder_paste_margin -0.05)", "(solder_paste_margin_ratio -0.05)"], "the margin stays where the file had it");
    let facts = eda_kicad_facts(&e.tokens);
    assert_eq!(facts, (Some(um(-50.0)), Some(-0.05)));
    b.apply(&Cmd::SetFootprintPasteMargin { part: "U1".into(), margin: None, ratio: None }).unwrap();
    assert_eq!(drawings(&b).footprint_extras[0].tokens, ["(descr \"x\")"]);
    // UI-LEFT P14: a footprint placed from the intent keeps them in its edit, which the writer writes
    b.apply(&Cmd::SetFootprintPasteMargin { part: "U2".into(), margin: Some(um(10.0)), ratio: Some(-0.1) }).unwrap();
    let e = b.design().footprint_edit("U2").unwrap();
    assert_eq!((e.solder_paste_margin, e.solder_paste_ratio), (Some(um(10.0)), Some(-0.1)));
    assert_eq!(b.apply(&Cmd::SetFootprintPasteMargin { part: "U2".into(), margin: Some(um(10.0)), ratio: Some(-0.1) }).unwrap_err()[0].check, "ops_nothing_to_change");
    assert_eq!(b.apply(&Cmd::SetFootprintPasteMargin { part: "U2".into(), margin: None, ratio: Some(-0.6) }).unwrap_err()[0].check, "ops_bad_footprint");
    b.apply(&Cmd::SetFootprintPasteMargin { part: "U2".into(), margin: None, ratio: None }).unwrap();
    assert!(b.design().footprint_edit("U2").is_none(), "nothing left of the edit");
}

#[test]
fn set_footprint_solder_mask_sets_the_records_expansion_and_bridging_and_makes_a_bare_record_for_a_footprint_without_one() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().footprint_extras.push(eda_model::ir::FootprintExtra { id: "U1".into(), solder_mask_margin: Some(um(50.0)), source_name: "Lib:U1".into(), ..Default::default() });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    // a negative expansion is taken (`Validate` checks only the pad clearance)
    b.apply(&Cmd::SetFootprintSolderMask { part: "U1".into(), margin: Some(um(-20.0)), allow_bridges: true }).unwrap();
    let e = drawings(&b).footprint_extras.iter().find(|e| e.id == "U1").unwrap();
    assert_eq!((e.solder_mask_margin, e.allow_soldermask_bridges, e.source_name.as_str()), (Some(um(-20.0)), true, "Lib:U1"), "the rest of the record stays");
    assert_eq!(b.apply(&Cmd::SetFootprintSolderMask { part: "U1".into(), margin: Some(um(-20.0)), allow_bridges: true }).unwrap_err()[0].check, "ops_nothing_to_change");
    b.apply(&Cmd::SetFootprintSolderMask { part: "U1".into(), margin: None, allow_bridges: false }).unwrap();
    let e = drawings(&b).footprint_extras.iter().find(|e| e.id == "U1").unwrap();
    assert_eq!((e.solder_mask_margin, e.allow_soldermask_bridges), (None, false), "blank is the board's");
    // a footprint placed from the intent has no record: it gets the bare one a footprint made here gets
    assert!(!drawings(&b).footprint_extras.iter().any(|e| e.id == "U2"));
    assert_eq!(b.apply(&Cmd::SetFootprintSolderMask { part: "U2".into(), margin: None, allow_bridges: false }).unwrap_err()[0].check, "ops_nothing_to_change", "nothing to change makes no record");
    assert!(!drawings(&b).footprint_extras.iter().any(|e| e.id == "U2"));
    b.apply(&Cmd::SetFootprintSolderMask { part: "U2".into(), margin: Some(um(30.0)), allow_bridges: false }).unwrap();
    let e = drawings(&b).footprint_extras.iter().find(|e| e.id == "U2").unwrap();
    assert_eq!(e, &eda_model::ir::FootprintExtra { id: "U2".into(), library_frame: true, solder_mask_margin: Some(um(30.0)), ..Default::default() });
    assert!(b.apply(&Cmd::SetFootprintSolderMask { part: "U9".into(), margin: None, allow_bridges: true }).is_err(), "no such footprint");
    // the verb as the studio sends it
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "set_footprint_solder_mask", "part": "U2", "margin": null, "allow_bridges": true })).unwrap();
    assert_eq!(cmd, Cmd::SetFootprintSolderMask { part: "U2".into(), margin: None, allow_bridges: true });
}

/// `(solder_paste_margin ..)` and `(solder_paste_margin_ratio ..)` read back the way `eda_kicad::footprint_file_facts` reads them (millimetres to nanometres).
fn eda_kicad_facts(tokens: &[String]) -> (Option<Nm>, Option<f64>) {
    let num = |name: &str| tokens.iter().find_map(|t| t.strip_prefix(&format!("({name} ")).and_then(|r| r.strip_suffix(')')).and_then(|v| v.parse::<f64>().ok()));
    (num("solder_paste_margin").map(|v| Nm::kiround(v * 1_000_000.0)), num("solder_paste_margin_ratio"))
}
