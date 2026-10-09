//! Create Array (`ARRAY_TOOL::CreateArray`): the points of a grid and of a circle, footprints with their references, every other kind
//! of item, arranging a selection, and the checks the dialog makes.

use super::pcb_transform_tests::{board, design, drawings, ids, model, p, pose, routing};
use super::*;
use eda_model::fp_edit::{FieldLayout, FootprintAttrs, FootprintKind, PadEdit};

fn grid(nx: i64, ny: i64, dx: Um, dy: Um) -> ArrayGeometry {
    ArrayGeometry::Grid { nx, ny, dx, dy, offset_x: 0, offset_y: 0, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false }
}

fn circle(count: i64, angle_deg: i64, rotate_items: bool) -> ArrayGeometry {
    ArrayGeometry::Circular { center: p(0, 0), count, angle_millideg: angle_deg * 1000, angle_offset_millideg: 0, clockwise: true, rotate_items }
}


/// Small edits of a geometry (an enum variant cannot be updated field by field).
trait Tweak: Sized {
    fn offsets(self, ox: Um, oy: Um) -> Self;
    fn centred(self) -> Self;
    fn staggered(self, n: i64, rows: bool) -> Self;
    fn column_first(self) -> Self;
    fn serpentine(self) -> Self;
    fn spacing_x(self, dx: Um) -> Self;
    fn counter_clockwise_from(self, offset_deg: i64) -> Self;
    fn turning(self) -> Self;
}

impl Tweak for ArrayGeometry {
    fn offsets(mut self, ox: Um, oy: Um) -> Self {
        if let ArrayGeometry::Grid { offset_x, offset_y, .. } = &mut self {
            (*offset_x, *offset_y) = (ox, oy);
        }
        self
    }
    fn centred(mut self) -> Self {
        if let ArrayGeometry::Grid { centred, .. } = &mut self {
            *centred = true;
        }
        self
    }
    fn staggered(mut self, n: i64, rows: bool) -> Self {
        if let ArrayGeometry::Grid { stagger, stagger_rows, .. } = &mut self {
            (*stagger, *stagger_rows) = (n, rows);
        }
        self
    }
    fn column_first(mut self) -> Self {
        if let ArrayGeometry::Grid { horizontal_then_vertical, .. } = &mut self {
            *horizontal_then_vertical = false;
        }
        self
    }
    fn serpentine(mut self) -> Self {
        if let ArrayGeometry::Grid { reverse_alternate, .. } = &mut self {
            *reverse_alternate = true;
        }
        self
    }
    fn spacing_x(mut self, new_dx: Um) -> Self {
        if let ArrayGeometry::Grid { dx, .. } = &mut self {
            *dx = new_dx;
        }
        self
    }
    fn counter_clockwise_from(mut self, offset_deg: i64) -> Self {
        if let ArrayGeometry::Circular { clockwise, angle_offset_millideg, .. } = &mut self {
            (*clockwise, *angle_offset_millideg) = (false, offset_deg * 1000);
        }
        self
    }
    fn turning(mut self) -> Self {
        if let ArrayGeometry::Circular { rotate_items, .. } = &mut self {
            *rotate_items = true;
        }
        self
    }
}

fn array(ids: &[&str], geometry: ArrayGeometry) -> Cmd {
    Cmd::CreateArray { ids: ids.iter().map(|s| s.to_string()).collect(), geometry, arrange: false, reannotate: true }
}

fn arrange(ids: &[&str], geometry: ArrayGeometry) -> Cmd {
    Cmd::CreateArray { ids: ids.iter().map(|s| s.to_string()).collect(), geometry, arrange: true, reannotate: true }
}

fn add_via(b: &mut Board<'_>, x: Um, y: Um) -> String {
    b.apply(&Cmd::AddVia { net: "GND".into(), x, y, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    routing(b).vias.iter().find(|v| v.at == p(x, y)).unwrap().id.clone()
}

/// The vias the tests added (the fixture's own `via_a` aside).
fn vias(b: &Board<'_>) -> Vec<Via> {
    routing(b).vias.iter().filter(|v| v.id != "via_a").cloned().collect()
}

fn via_positions(b: &Board<'_>) -> BTreeSet<(Um, Um)> {
    vias(b).iter().map(|v| (v.at.x, v.at.y)).collect()
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> String {
    b.apply(&cmd).unwrap_err()[0].check.clone()
}

// ---------------------------------------------------------------------------------------------------- the points

#[test]
fn a_grid_numbers_its_points_across_then_down_and_a_column_first_array_goes_down_then_across() {
    let g = grid(3, 2, 1_000, 2_000);
    let at = |n: i64| g.transform(n, p(5, 5)).0;
    assert_eq!([at(0), at(1), at(2), at(3), at(4), at(5)], [p(0, 0), p(1_000, 0), p(2_000, 0), p(0, 2_000), p(1_000, 2_000), p(2_000, 2_000)]);
    let down = grid(3, 2, 1_000, 2_000).column_first();
    let at = |n: i64| down.transform(n, p(0, 0)).0;
    assert_eq!([at(0), at(1), at(2), at(3), at(4), at(5)], [p(0, 0), p(0, 2_000), p(1_000, 0), p(1_000, 2_000), p(2_000, 0), p(2_000, 2_000)]);
}

#[test]
fn reversing_alternate_rows_makes_the_order_a_serpentine_over_the_same_points() {
    let snake = grid(3, 2, 1_000, 2_000).serpentine();
    let at = |n: i64| snake.transform(n, p(0, 0)).0;
    assert_eq!([at(0), at(1), at(2), at(3), at(4), at(5)], [p(0, 0), p(1_000, 0), p(2_000, 0), p(2_000, 2_000), p(1_000, 2_000), p(0, 2_000)]);
}

#[test]
fn a_skewed_grid_adds_the_offset_per_row_and_per_column() {
    let g = grid(2, 2, 1_000, 2_000).offsets(300, 50);
    // point (1, 1): x = 1*dx + 1*offset_x, y = 1*dy + 1*offset_y
    assert_eq!(g.transform(3, p(0, 0)).0, p(1_300, 2_050));
    let centred = g.centred();
    // the extent is (nx-1)*dx + (ny-1)*offset_x = 1300 across and 2050 down: half of each comes off
    assert_eq!(centred.transform(0, p(0, 0)).0, p(-650, -1_025));
}

#[test]
fn every_other_row_is_staggered_by_a_fraction_of_the_spacing_the_way_kicad_truncates_it() {
    // stagger 2 on rows: odd rows slide by dx/2; stagger 3: rows 1 and 2 slide by dx/3 and 2*dx/3 (integer division)
    let two = grid(2, 3, 1_000, 2_000).staggered(2, true);
    assert_eq!([two.transform(0, p(0, 0)).0, two.transform(2, p(0, 0)).0, two.transform(4, p(0, 0)).0], [p(0, 0), p(500, 2_000), p(0, 4_000)]);
    let three = grid(1, 3, 1_000, 2_000).staggered(3, true);
    assert_eq!([three.transform(1, p(0, 0)).0, three.transform(2, p(0, 0)).0], [p(333, 2_000), p(666, 4_000)], "1000/3 truncates toward zero");
    // a negative stagger slides the other way; by columns it slides vertically
    let back = grid(2, 2, 1_000, 2_000).staggered(-2, true);
    assert_eq!(back.transform(2, p(0, 0)).0, p(-500, 2_000));
    let by_columns = grid(2, 2, 1_000, 2_000).staggered(2, false);
    assert_eq!(by_columns.transform(1, p(0, 0)).0, p(1_000, 1_000));
}

#[test]
fn a_circular_array_walks_the_circle_by_the_angle_clockwise_or_counter_clockwise_from_its_offset() {
    let g = ArrayGeometry::Circular { center: p(0, 0), count: 4, angle_millideg: 90_000, angle_offset_millideg: 0, clockwise: true, rotate_items: false };
    let at = |n: i64| {
        let (o, turn) = g.transform(n, p(1_000, 0));
        assert_eq!(turn, 0, "items are not turned unless asked");
        p(1_000 + o.x, o.y)
    };
    // clockwise on the screen with y down: east, south, west, north
    assert_eq!([at(0), at(1), at(2), at(3)], [p(1_000, 0), p(0, 1_000), p(-1_000, 0), p(0, -1_000)]);
    let ccw = g.counter_clockwise_from(90);
    let (o, _) = ccw.transform(0, p(1_000, 0));
    assert_eq!(p(1_000 + o.x, o.y), p(0, -1_000), "a 90 degree offset, counter-clockwise from east, is north");
    let turned = g.turning();
    assert_eq!(turned.transform(2, p(1_000, 0)).1, 180_000);
}

#[test]
fn an_angle_of_zero_divides_the_circle_evenly() {
    let g = ArrayGeometry::Circular { center: p(0, 0), count: 3, angle_millideg: 0, angle_offset_millideg: 0, clockwise: true, rotate_items: true };
    assert_eq!(g.transform(1, p(1_000, 0)).1, 120_000);
    assert_eq!(g.transform(2, p(1_000, 0)).1, 240_000);
}

// ------------------------------------------------------------------------------------------------ what the dialog refuses

#[test]
fn the_numbers_the_dialog_refuses_are_refused_and_so_is_an_empty_or_unknown_selection() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let v = add_via(&mut b, 0, 0);
    assert_eq!(refusal(&mut b, Cmd::CreateArray { ids: vec![], geometry: grid(2, 2, 1_000, 1_000), arrange: false, reannotate: true }), "ops_bad_array");
    assert_eq!(refusal(&mut b, array(&[&v], grid(2, 2, 0, 1_000))), "ops_bad_array", "horizontal delta of zero with 2 objects");
    assert_eq!(refusal(&mut b, array(&[&v], grid(2, 2, 1_000, 0))), "ops_bad_array");
    assert_eq!(refusal(&mut b, array(&[&v], grid(0, 2, 1_000, 1_000))), "ops_bad_array");
    assert_eq!(refusal(&mut b, array(&[&v], circle(4, 0, false))), "ops_bad_array", "angular delta of zero with 4 objects");
    assert_eq!(refusal(&mut b, array(&["nothing", "here"], grid(2, 2, 1_000, 1_000))), "ops_unknown_array");
    assert_eq!(vias(&b).len(), 1, "a refused array changes nothing");
    // one row and one column need no spacing
    b.apply(&array(&[&v], grid(1, 1, 0, 0))).unwrap();
    assert_eq!(vias(&b).len(), 1);
}

// ------------------------------------------------------------------------------------------------------- copper

#[test]
fn a_grid_of_vias_takes_every_point_once_and_the_original_keeps_the_first() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let v = add_via(&mut b, 0, 0);
    b.apply(&array(&[&v], grid(2, 2, 1_000, 1_000))).unwrap();
    assert_eq!(via_positions(&b), BTreeSet::from([(0, 0), (1_000, 0), (0, 1_000), (1_000, 1_000)]));
    assert_eq!(vias(&b).len(), 4);
    assert_eq!(vias(&b).iter().find(|x| x.id == v).unwrap().at, p(0, 0), "the original stays on the first point");
    assert_eq!(vias(&b).iter().map(|x| x.id.clone()).collect::<BTreeSet<_>>().len(), 4, "every copy has an id of its own");
}

#[test]
fn a_centred_grid_spreads_around_the_original_and_moves_the_original_too() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let v = add_via(&mut b, 0, 0);
    b.apply(&array(&[&v], grid(2, 2, 1_000, 1_000).centred())).unwrap();
    assert_eq!(via_positions(&b), BTreeSet::from([(-500, -500), (500, -500), (-500, 500), (500, 500)]));
}

#[test]
fn a_track_a_zone_and_a_shape_move_whole_to_each_point() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&array(&["trk_a", "zone_a", "shp_seg", "txt_a", "dim_a"], grid(2, 1, 5_000, 0))).unwrap();
    let rt = routing(&b);
    assert_eq!(rt.tracks.len(), 2);
    assert!(rt.tracks.iter().any(|t| t.pts == vec![p(10_000, 40_000), p(20_000, 40_000), p(20_000, 50_000)]) && rt.tracks.iter().any(|t| t.pts == vec![p(15_000, 40_000), p(25_000, 40_000), p(25_000, 50_000)]));
    assert_eq!(rt.zones.len(), 2);
    let d = drawings(&b);
    assert_eq!(d.shapes.iter().filter(|s| s.layer() == "F.SilkS" && s.id() != "shp_arc").count(), 2);
    assert_eq!(d.texts.len(), 2);
    assert_eq!(d.dimensions.len(), 2);
    assert_eq!(d.dimensions.iter().map(|x| x.start.x).collect::<BTreeSet<_>>(), BTreeSet::from([70_000, 75_000]));
}

#[test]
fn a_circular_array_that_turns_its_items_turns_every_kind_about_its_own_position() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().texts.clear();
    d.drawings.as_mut().unwrap().shapes.clear();
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::AddText { text: Text { id: String::new(), content: "1".into(), at: p(1_000, 0), angle: 0, layer: "F.SilkS".into(), size_um: 1_000, stroke_width: 150, justify: TextJustify::Center, mirror: false } }).unwrap();
    b.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: p(1_000, 0), end: p(1_100, 0) } }).unwrap();
    let (text, shape) = (drawings(&b).texts[0].id.clone(), drawings(&b).shapes[0].id().to_string());
    b.apply(&array(&[&text, &shape], circle(4, 90, true))).unwrap();
    let d = drawings(&b);
    assert_eq!(d.texts.len(), 4);
    // The step-one copy sits south of the centre (clockwise) and its text turned clockwise a quarter: KiCad's angle runs the other way.
    let south = d.texts.iter().find(|t| t.at == p(0, 1_000)).unwrap();
    assert_eq!(south.angle, 270_000);
    let south_seg = d.shapes.iter().find(|s| s.points()[0] == p(0, 1_000)).unwrap();
    assert_eq!(south_seg.points()[1], p(0, 1_100), "the segment turned about its start, a quarter clockwise");
    assert_eq!(d.shapes.len(), 4);
}

#[test]
fn a_circular_array_that_does_not_turn_its_items_only_translates_them() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::AddShape { shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150, filled: false, start: p(1_000, 0), end: p(1_100, 0) } }).unwrap();
    let seg = drawings(&b).shapes.iter().find(|s| s.points()[0] == p(1_000, 0)).unwrap().id().to_string();
    b.apply(&array(&[&seg], circle(4, 90, false))).unwrap();
    for s in drawings(&b).shapes.iter().filter(|s| s.layer() == "F.SilkS" && s.points()[1].x - s.points()[0].x != 0 || s.points()[0] == p(1_000, 0)) {
        let pts = s.points();
        if pts[0].x.abs() == 1_000 || pts[0].y.abs() == 1_000 {
            assert_eq!((pts[1].x - pts[0].x, pts[1].y - pts[0].y), (100, 0), "{s:?}");
        }
    }
}

// ------------------------------------------------------------------------------------------------------ footprints

fn footprint_ids(b: &Board<'_>) -> Vec<String> {
    b.design().placement.as_ref().unwrap().footprints.iter().map(|f| f.id.clone()).collect()
}

#[test]
fn a_footprint_is_copied_to_every_point_as_a_new_part_on_the_same_nets_with_the_next_free_references() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // U1 and U2 exist: the copies of U1 are U3 and U4, in the order of the points
    b.apply(&array(&["U1"], grid(3, 1, 10_000, 0))).unwrap();
    assert_eq!(footprint_ids(&b), ["U1", "U2", "U3", "U4"]);
    let origin = pose(&b, "U1").at;
    assert_eq!(origin, p(30_000, 20_000), "the original stays on the first point");
    assert_eq!(pose(&b, "U3").at, p(40_000, 20_000));
    assert_eq!(pose(&b, "U4").at, p(50_000, 20_000));
    let parts = &drawings(&b).board_parts;
    assert_eq!(parts.iter().map(|x| x.reference.as_str()).collect::<Vec<_>>(), ["U3", "U4"]);
    assert!(parts.iter().all(|x| x.footprint == "LOPSIDED" && x.pad_nets == vec![("1".to_string(), "GND".to_string()), ("2".to_string(), "VCC".to_string())]), "the pads are on U1's nets: {parts:?}");
}

#[test]
fn copies_take_the_rotation_side_and_edits_of_the_footprint_they_come_from() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut reference = FieldLayout::new(p(1_500, -2_500), "F.Fab");
    reference.size = (900, 900);
    let attrs = FootprintAttrs { kind: FootprintKind::Smd, dnp: true, ..Default::default() };
    b.apply(&Cmd::EditBoardFootprint { part: "U2".into(), reference: Some(reference.clone()), value: None, fields: None, attrs: Some(attrs) }).unwrap();
    let mut pad = PadEdit::none("1", 1);
    pad.size = Some((900, 900));
    b.apply(&Cmd::EditBoardPad { part: "U2".into(), edit: pad.clone() }).unwrap();
    // and how zones connect to it (the zone filler's overlay, beside our edit)
    b.apply(&Cmd::SetFootprintZoneConnection { part: "U2".into(), zone_connection: Some(eda_model::ir::PadConnection::None), clearance: Some(300) }).unwrap();
    b.apply(&array(&["U2"], grid(2, 1, 8_000, 0))).unwrap();
    let copy = pose(&b, "U3");
    assert_eq!((copy.rot, copy.side), (90_000, Side::Top));
    assert_eq!(copy.at, p(68_000, 20_000));
    let edit = b.design().footprint_edit("U3").expect("the copy is edited the way its original is");
    assert_eq!((edit.reference.as_ref().map(|l| (l.at, l.size, l.text.clone())), edit.attrs, edit.pad("1", 1).cloned()), (Some((reference.at, reference.size, None)), Some(attrs), Some(pad)));
    assert_eq!(b.design().footprint_edit("U2").unwrap().reference, Some(reference), "and the original keeps its own");
    let zone = |id: &str| drawings(&b).zone_overrides.iter().find(|z| z.id == id).and_then(|z| z.footprint).map(|f| (f.zone_connection, f.clearance));
    assert_eq!(zone("U3"), Some((Some(eda_model::ir::PadConnection::None), Some(300))), "the copy has its original's zone overrides under its own reference");
    assert_eq!(zone("U2"), zone("U3"), "and the original keeps its own");
}

#[test]
fn keeping_the_original_references_keeps_the_text_but_not_the_id() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::CreateArray { ids: ids(&["U1"]), geometry: grid(3, 1, 10_000, 0), arrange: false, reannotate: false }).unwrap();
    assert_eq!(footprint_ids(&b), ["U1", "U2", "U3", "U4"], "ids stay unique");
    for copy in ["U3", "U4"] {
        let shown = b.design().footprint_edit(copy).and_then(|e| e.reference.clone()).and_then(|l| l.text);
        assert_eq!(shown.as_deref(), Some("U1"), "{copy} shows the original's reference");
    }
    assert!(b.design().footprint_edit("U1").is_none(), "the original is as it was");
}

#[test]
fn a_pad_or_a_field_in_the_selection_stands_for_its_footprint_once() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&array(&["U1.1", "U1.2", "U1:Reference", "U1"], grid(2, 1, 10_000, 0))).unwrap();
    assert_eq!(footprint_ids(&b), ["U1", "U2", "U3"], "one copy, not four");
}

#[test]
fn a_selection_of_a_footprint_and_copper_moves_together_to_each_point() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&array(&["U1", "trk_a"], grid(2, 1, 0, 0).spacing_x(7_000))).unwrap();
    assert_eq!(pose(&b, "U3").at, p(37_000, 20_000));
    assert!(routing(&b).tracks.iter().any(|t| t.pts[0] == p(17_000, 40_000)), "the copy of the track is on the same point");
}

#[test]
fn a_circular_array_of_a_footprint_turns_the_copies_when_asked() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.placement.as_mut().unwrap().footprints[0].at = p(10_000, 0);
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&array(&["U1"], circle(4, 90, true))).unwrap();
    let u3 = pose(&b, "U3");
    assert_eq!((u3.at, u3.rot), (p(0, 10_000), 90_000), "a quarter clockwise about the origin, and the part turned with it");
    let u5 = pose(&b, "U5");
    assert_eq!((u5.at, u5.rot), (p(0, -10_000), 270_000));
}

// ----------------------------------------------------------------------------------------------------------- groups

#[test]
fn a_group_is_copied_with_copies_of_its_members_and_moves_as_one() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().groups.push(Group { id: "grp_a".into(), name: "block".into(), member_ids: ids(&["trk_a", "shp_seg"]) });
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&array(&["grp_a"], grid(2, 1, 6_000, 0))).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.groups.len(), 2, "a group for the copy");
    let copy = dr.groups.iter().find(|g| g.id != "grp_a").unwrap();
    assert_eq!(copy.name, "block");
    assert_eq!(copy.member_ids.len(), 2);
    let moved_track = routing(&b).tracks.iter().find(|t| copy.member_ids.contains(&t.id)).unwrap();
    assert_eq!(moved_track.pts[0], p(16_000, 40_000));
    let moved_seg = dr.shapes.iter().find(|s| copy.member_ids.contains(&s.id().to_string())).unwrap();
    assert_eq!(moved_seg.points()[0], p(6_000, 80_000));
}

#[test]
fn a_group_inside_a_group_is_copied_whole_and_everything_in_it_moves_as_one() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    let dr = d.drawings.as_mut().unwrap();
    dr.groups.push(Group { id: "grp_in".into(), name: "inner".into(), member_ids: ids(&["trk_a", "shp_seg"]) });
    dr.groups.push(Group { id: "grp_out".into(), name: "outer".into(), member_ids: ids(&["grp_in", "txt_a"]) });
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&array(&["grp_out"], grid(2, 1, 6_000, 0))).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.groups.len(), 4, "a group of each kind for the copy");
    let outer = dr.groups.iter().find(|g| g.name == "outer" && g.id != "grp_out").expect("the copy of the outer group");
    let inner = dr.groups.iter().find(|g| g.name == "inner" && g.id != "grp_in").expect("the copy of the inner group");
    assert!(outer.member_ids.contains(&inner.id) && outer.member_ids.len() == 2, "the outer copy holds the inner copy and a text: {outer:?}");
    assert_eq!(inner.member_ids.len(), 2);
    let track = routing(&b).tracks.iter().find(|t| inner.member_ids.contains(&t.id)).unwrap();
    assert_eq!(track.pts[0], p(16_000, 40_000), "the track in the inner copy moved by the array's step");
    let seg = dr.shapes.iter().find(|s| inner.member_ids.contains(&s.id().to_string())).unwrap();
    assert_eq!(seg.points()[0], p(6_000, 80_000));
    let text = dr.texts.iter().find(|t| outer.member_ids.contains(&t.id)).unwrap();
    assert_eq!(text.at, p(76_000, 80_000), "the text the outer copy holds moved with it");
    assert_eq!(routing(&b).tracks.iter().find(|t| t.id == "trk_a").unwrap().pts[0], p(10_000, 40_000), "the original tree stays where it was");
}

// -------------------------------------------------------------------------------------------------------- arrange

#[test]
fn arranging_puts_the_items_on_the_points_starting_from_the_first_ones_position() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let v1 = add_via(&mut b, 0, 0);
    let v2 = add_via(&mut b, 9_999, 9_999);
    let v3 = add_via(&mut b, 4_000, 8_000);
    b.apply(&arrange(&[&v1, &v2, &v3], grid(2, 1, 5_000, 0))).unwrap();
    let at = |id: &String| routing(&b).vias.iter().find(|v| &v.id == id).unwrap().at;
    assert_eq!(vias(&b).len(), 3, "arranging makes nothing");
    assert_eq!(at(&v1), p(0, 0));
    assert_eq!(at(&v2), p(5_000, 0), "stacked on the first item's position, then offset by the second point");
    assert_eq!(at(&v3), p(4_000, 8_000), "there are only two points: the third item is left alone");
}

#[test]
fn arranging_footprints_moves_them_and_an_unknown_id_does_not_use_up_a_point() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let first = pose(&b, "U1").at;
    b.apply(&arrange(&["ignored", "U1", "ignored too", "U2"], grid(2, 1, 5_000, 0))).unwrap();
    assert_eq!(pose(&b, "U1").at, first);
    assert_eq!(pose(&b, "U2").at, p(first.x + 5_000, first.y), "U2 is on the second point, the unknown ids took none");
    assert_eq!(footprint_ids(&b), ["U1", "U2"]);
}

#[test]
fn arranging_on_a_circle_turns_a_footprint_when_asked() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.placement.as_mut().unwrap().footprints[0].at = p(10_000, 0);
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&arrange(&["U1", "U2"], circle(2, 180, true))).unwrap();
    assert_eq!(pose(&b, "U1").at, p(10_000, 0));
    let u2 = pose(&b, "U2");
    assert_eq!((u2.at, u2.rot), (p(-10_000, 0), (90_000 + 180_000) % 360_000), "on the far side of the circle, turned half a turn more");
}

// ------------------------------------------------------------------------------------------------------ the routing

#[test]
fn an_array_of_copper_leaves_the_routing_be_and_one_of_a_routed_footprint_clears_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let b = board(&m);
    // trk_a is not on a pad of any footprint, so arraying it or U2 (no copper on its pads) leaves the routing alone
    assert!(!array(&["trk_a"], grid(2, 1, 5_000, 0)).clears_routing_in(&b));
    assert!(!array(&["U2"], grid(2, 1, 5_000, 0)).clears_routing_in(&b));
    // a track that ends on U1's pad 1 is routed to it
    let mut d = design();
    let pad = eda_model::footprint::placed_pads(&m, m.part("U1").unwrap(), &d.placement.as_ref().unwrap().footprints[0].clone()).unwrap();
    let on_pad = pad[0].center;
    d.routing.as_mut().unwrap().tracks.push(Track { id: "trk_pad".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![on_pad, p(on_pad.x, on_pad.y + 5_000)], arc_mid_offset: None });
    let routed = Board::new(d, &m, 100, 300);
    assert!(array(&["U1"], grid(2, 1, 5_000, 0)).clears_routing_in(&routed));
    assert!(array(&["U1.1"], grid(2, 1, 5_000, 0)).clears_routing_in(&routed), "a pad stands for its footprint");
}
