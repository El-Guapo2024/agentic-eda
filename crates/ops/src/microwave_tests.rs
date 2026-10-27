//! The microwave tools' verbs (`microwave.rs`): what each footprint is, number for number, as `microwave_footprint.cpp`, `microwave_inductor.cpp` and
//! `microwave_polygon.cpp` build it, and how it lands on the board.

use eda_model::units::Nm;
use super::microwave::*;
use super::pcb_transform::FlipDirection;
use super::{Board, Cmd};
use eda_model::footprint::PadKind;
use eda_model::ir::{Design, LibraryFootprint, LibraryPadShape, Point, Shape, Side};
use eda_model::ConstraintModel;
use eda_model::units::{mm, um};

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

/// A design built from JSON, so these tests do not break when another section is added to `Design`.
fn board(model: &ConstraintModel) -> Board<'_> {
    let design: Design = serde_json::from_str(
        r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]},
            "placement":{"outline":[{"x":0,"y":0},{"x":100000,"y":0},{"x":100000,"y":100000},{"x":0,"y":100000}],"footprints":[],"modules":[]}}"#,
    )
    .unwrap();
    Board::new(design, model, mm(0.1), mm(0.3))
}

fn gap(b: &mut Board, at: Point, size_nm: Nm) {
    b.apply(&Cmd::Microwave(MicrowaveCmd::Gap { at, size_nm, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
}

fn part<'a>(b: &'a Board, reference: &str) -> &'a eda_model::ir::BoardPart {
    b.design().drawings.as_ref().unwrap().board_parts.iter().find(|p| p.reference == reference).unwrap_or_else(|| panic!("a board part {reference}"))
}

fn def<'a>(b: &'a Board, reference: &str) -> &'a LibraryFootprint {
    part(b, reference).definition.as_ref().expect("a microwave footprint carries its definition")
}

fn pose<'a>(b: &'a Board, reference: &str) -> &'a eda_model::ir::FootprintInstance {
    b.design().placement.as_ref().unwrap().footprints.iter().find(|f| f.id == reference).unwrap_or_else(|| panic!("a pose for {reference}"))
}

// ---------------------------------------------------------------------------------------------------------------------------------- the gap

/// `createFootprint( GAP )` with a 0.25 mm track and a 0.5 mm gap: `offsetX = -( gap + w ) / 2`, the first pad there and the second `gap + w` further.
#[test]
fn a_gap_is_two_square_pads_the_gap_apart_on_a_footprint_of_its_own() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(mm(10.0), mm(20.0)), mm(0.5));

    let bp = part(&b, "REF1");
    assert_eq!((bp.footprint.as_str(), bp.value.as_deref()), ("muwave_gap", Some("muwave_gap")), "the name is the footprint's and its value");
    assert!(bp.pad_nets.is_empty(), "the pads are on no net");
    let fp = def(&b, "REF1");
    assert_eq!(fp.pads.len(), 2);
    let (a, c) = (&fp.pads[0], &fp.pads[1]);
    // `ADD_MODE::INSERT` puts each new pad first: pad 2 is `Pads().begin()`, on the left (round 3 D-7)
    assert_eq!((a.number.as_str(), a.at, a.size, a.shape, a.kind), ("2", p(mm(-0.375), Nm::ZERO), (mm(0.25), mm(0.25)), LibraryPadShape::Rect, PadKind::Smd));
    assert_eq!((c.number.as_str(), c.at, c.size, c.shape, c.kind), ("1", p(mm(0.375), Nm::ZERO), (mm(0.25), mm(0.25)), LibraryPadShape::Rect, PadKind::Smd));
    assert_eq!(a.layers, vec!["F.Cu".to_string()], "`pad->SetLayerSet( {{ F_Cu }} )`: copper and nothing else");
    // the pose is the click
    let f = pose(&b, "REF1");
    assert_eq!((f.at, f.rot, f.side), (p(mm(10.0), mm(20.0)), 0, Side::Top));
    // the gap between the pads' edges is the gap asked for
    assert_eq!((c.at.x - c.size.0 / 2) - (a.at.x + a.size.0 / 2), mm(0.5));
}

/// `createBaseFootprint` + `CreateNewFootprint`: Reference `REF**` on the silkscreen half a text height above the origin and Value on the fabrication layer below it, both as big as the
/// track is wide with a thickness of a fifth of it (`text_size = gap_size` is read before the dialog), the `${REFERENCE}` text under them, and the attributes.
#[test]
fn a_gaps_fields_texts_and_attributes_are_createnewfootprints() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(Nm::ZERO, Nm::ZERO), mm(0.5));
    let dr = b.design().drawings.as_ref().unwrap();
    let edit = dr.footprint_edits.iter().find(|e| e.id == "REF1").expect("the fields are laid out");
    let (r, v) = (edit.reference.as_ref().unwrap(), edit.value.as_ref().unwrap());
    assert_eq!((r.layer.as_str(), r.at, r.size, r.thickness, r.visible), ("F.SilkS", p(Nm::ZERO, mm(-0.5)), (mm(0.25), mm(0.25)), mm(0.05), true));
    assert_eq!((v.layer.as_str(), v.at, v.size, v.thickness, v.visible), ("F.Fab", p(Nm::ZERO, mm(1.0)), (mm(0.25), mm(0.25)), mm(0.05), true));
    let fp = def(&b, "REF1");
    assert_eq!(fp.texts.len(), 1);
    let t = &fp.texts[0];
    assert_eq!((t.content.as_str(), t.layer.as_str(), t.at, t.size_nm, t.stroke_width), ("${REFERENCE}", "F.Fab", p(Nm::ZERO, mm(2.5)), mm(1.0), mm(0.15)), "the third default text item of the board, at the board's size");
    // `SetAttributes( FP_EXCLUDE_FROM_POS_FILES | FP_EXCLUDE_FROM_BOM )`: neither SMD nor through hole
    let a = edit.attrs.expect("the attributes are set");
    assert_eq!((a.kind, a.exclude_from_pos_files, a.exclude_from_bom, a.board_only, a.dnp), (eda_model::fp_edit::FootprintKind::Unspecified, true, true, false, false));
    // no courtyard, copper only: what a footprint read from a board file with these pads would have
    let extra = dr.footprint_extras.iter().find(|e| e.id == "REF1").expect("the facts of the footprint");
    assert!(extra.no_courtyard);
    assert_eq!(extra.pads.len(), 2);
    assert!(extra.pads.iter().all(|p| p.no_paste && p.no_mask && p.layers == ["F.Cu"]));
    // the engine has no keep-out to enforce for them either: a point, not the box of the pads
    assert_eq!(def(&b, "REF1").to_engine_footprint().courtyard_half(), (Nm::new(1), Nm::new(1)));
}

/// The texts take the board's Text & Graphics defaults per layer: a board with 1.2 mm silkscreen text and 0.8 mm fabrication text puts the Reference half a silkscreen text
/// height above the origin and the Value half a fabrication text height below the end of the Reference's line.
#[test]
fn the_boards_text_defaults_place_and_size_the_fields() {
    let mut model = ConstraintModel::default();
    model.board.text_graphics.silk.text_height_nm = mm(1.2);
    model.board.text_graphics.silk.text_width_nm = mm(1.2);
    model.board.text_graphics.fab.text_height_nm = mm(0.8);
    model.board.text_graphics.fab.text_width_nm = mm(0.8);
    model.board.text_graphics.fab.text_thickness_nm = mm(0.12);
    let mut b = board(&model);
    // A stub of the arc kind keeps the board's sizes (`text_size` 0).
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(Nm::ZERO, Nm::ZERO), radius_nm: mm(1.0), angle_millideg: 90_000, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    let (r, v) = (edit.reference.as_ref().unwrap(), edit.value.as_ref().unwrap());
    // y = -silk/2 = -600; then + silk = 600; + fab/2 = 1000 (the Value); + fab = 1800; + fab/2 = 2200 (the user text)
    assert_eq!((r.at, r.size), (p(Nm::ZERO, mm(-0.6)), (mm(1.2), mm(1.2))));
    assert_eq!((v.at, v.size, v.thickness), (p(Nm::ZERO, mm(1.0)), (mm(0.8), mm(0.8)), mm(0.12)));
    assert_eq!(def(&b, "REF1").texts[0].at, p(Nm::ZERO, mm(2.2)));
}

/// KiCad's integers are nanometres, and `/ 2` truncates there: an odd sum of micrometres leaves half a micrometre, which the IR keeps.
#[test]
fn an_odd_gap_is_divided_in_nanometres() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(Nm::ZERO, Nm::ZERO), mm(0.333));
    let fp = def(&b, "REF1");
    // offsetX = -(333000 + 250000) / 2 = -291500 nm; the second pad at -291500 + 333000 + 250000 = 291500 nm
    assert_eq!((fp.pads[0].at.x, fp.pads[1].at.x), (Nm::new(-291_500), Nm::new(291_500)));
}

#[test]
fn a_gap_needs_a_size_and_a_track() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    for (size, track) in [(Nm::ZERO, mm(0.25)), (um(-5.0), mm(0.25)), (mm(0.5), Nm::ZERO)] {
        let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Gap { at: p(Nm::ZERO, Nm::ZERO), size_nm: size, track_width_nm: track, hand: Hand::default() })).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_microwave", "{size} {track}");
    }
    assert!(b.design().drawings.as_ref().map_or(true, |d| d.board_parts.is_empty()), "a refused command changes nothing");
}

// ---------------------------------------------------------------------------------------------------------------------------------- the stub

/// `createFootprint( STUB )`: the square pad at the origin and the stub, as wide as the track and as long as the stub, `-( stub + h ) / 2` above it -- both
/// numbered 1: the first pad (pad 2, `Pads().begin()`) is renamed 1 (round 3 D-7).
#[test]
fn a_stub_is_a_square_pad_and_the_stub_above_it() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::Stub { at: p(mm(5.0), mm(5.0)), size_nm: mm(1.0), track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    let bp = part(&b, "REF1");
    assert_eq!(bp.footprint, "muwave_stub");
    let fp = def(&b, "REF1");
    let (a, s) = (&fp.pads[0], &fp.pads[1]);
    assert_eq!((a.number.as_str(), a.at, a.size), ("1", p(Nm::ZERO, Nm::ZERO), (mm(0.25), mm(0.25))));
    assert_eq!((s.number.as_str(), s.at, s.size), ("1", p(Nm::ZERO, mm(-0.625)), (mm(0.25), mm(1.0))));
    // the stub's far end is `stub + h / 2` above the origin and its near end touches the square pad
    assert_eq!((s.at.y - s.size.1 / 2, s.at.y + s.size.1 / 2), (mm(-1.125), mm(-0.125)));
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    assert_eq!(edit.reference.as_ref().unwrap().size, (mm(0.25), mm(0.25)), "the stub's texts take the track width too");
}

// ------------------------------------------------------------------------------------------------------------------------------ the arc stub

/// The thetas `createFootprint( STUB_ARC )` walks: from `-angle / 2` in steps of 5 degrees, the last clamped to `angle / 2`; the points are `(0, -r)` turned by them.
fn arc_points(b: &Board) -> Vec<Point> {
    match &def(b, "REF1").pads[0].custom.as_ref().expect("a custom pad").primitives[0].shape {
        Shape::Polygon { pts, filled, stroke_width, .. } => {
            assert!(*filled && *stroke_width == Nm::ZERO, "`AddPrimitivePoly( .., 0, true )`");
            pts.clone()
        }
        other => panic!("a polygon, not {other:?}"),
    }
}

#[test]
fn an_arc_stub_is_one_custom_pad_with_a_pie_slice() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(Nm::ZERO, Nm::ZERO), radius_nm: mm(1.0), angle_millideg: 90_000, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    assert_eq!(part(&b, "REF1").footprint, "muwave_arcstub");
    let fp = def(&b, "REF1");
    assert_eq!(fp.pads.len(), 1, "`pad_count = 1`");
    let pad = &fp.pads[0];
    assert_eq!((pad.number.as_str(), pad.shape, pad.size, pad.at), ("1", LibraryPadShape::Custom, (mm(0.25), mm(0.25)), p(Nm::ZERO, Nm::ZERO)));
    let custom = pad.custom.as_ref().unwrap();
    assert_eq!(custom.anchor, eda_model::footprint::PadAnchor::Rect, "`SetAnchorPadShape( .., RECTANGLE )`");
    let pts = arc_points(&b);
    // numPoints = 90 / 5 + 3 = 21: the origin, 19 points from -45 to +45 degrees, and the origin again
    assert_eq!(pts.len(), 21);
    assert_eq!((pts[0], pts[20]), (p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, Nm::ZERO)));
    assert_eq!(pts[1], p(Nm::new(707_107), Nm::new(-707_107)), "theta = -45: RotatePoint turns (0, -r) toward +x");
    assert_eq!(pts[10], p(Nm::ZERO, mm(-1.0)), "the middle of the slice points up");
    assert_eq!(pts[19], p(Nm::new(-707_107), Nm::new(-707_107)));
    // every point of the arc is a radius from the origin, to the nanometre
    for q in &pts[1..20] {
        let r = ((q.x.area(q.x) + q.y.area(q.y)) as f64).sqrt();
        assert!((r - 1_000_000.0).abs() <= 1.0, "{q:?} is {r} from the origin");
    }
}

#[test]
fn the_arc_stubs_angle_is_taken_as_an_absolute_value_and_clamped_to_a_half_turn() {
    let model = ConstraintModel::default();
    // 270 degrees is 180: 180 / 5 + 3 = 39 points in all, -90 .. +90 degrees
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(Nm::ZERO, Nm::ZERO), radius_nm: mm(2.0), angle_millideg: 270_000, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    let pts = arc_points(&b);
    assert_eq!(pts.len(), 39);
    assert_eq!((pts[1], pts[37]), (p(mm(2.0), Nm::ZERO), p(mm(-2.0), Nm::ZERO)));
    // a negative angle is its absolute value
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(Nm::ZERO, Nm::ZERO), radius_nm: mm(1.0), angle_millideg: -90_000, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    assert_eq!(arc_points(&b).len(), 21);
    // an angle of nothing is the one point the loop makes: origin, (0, -r), origin
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(Nm::ZERO, Nm::ZERO), radius_nm: mm(1.0), angle_millideg: 0, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    assert_eq!(arc_points(&b), vec![p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(-1.0)), p(Nm::ZERO, Nm::ZERO)]);
    // an angle that does not divide by 5: the last step is clamped to angle / 2 (12 degrees: -6, -1, 4, 6 -> numPoints = 12 / 5 + 3 = 5, three points)
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(Nm::ZERO, Nm::ZERO), radius_nm: mm(10.0), angle_millideg: 12_000, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    let pts = arc_points(&b);
    assert_eq!(pts.len(), 5);
    // thetas -6, -1, 4: x = round(r * sin(6)) = 1_045_285, round(r * sin(1)) = 174_524, round(-r * sin(4)) = -697_565 (nm)
    assert_eq!(&pts[1..4], &[p(Nm::new(1_045_285), Nm::new(-9_945_219)), p(Nm::new(174_524), Nm::new(-9_998_477)), p(Nm::new(-697_565), Nm::new(-9_975_641))]);
}

#[test]
fn an_arc_stub_needs_a_radius() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let e = b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(Nm::ZERO, Nm::ZERO), radius_nm: Nm::ZERO, angle_millideg: 90_000, track_width_nm: mm(0.25), hand: Hand::default() })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
}

// ------------------------------------------------------------------------------------------------------------------------ names and references

/// The model finds a footprint by its name, so footprints that differ cannot share one; footprints that are alike do. The references are the next free ones.
#[test]
fn footprints_that_differ_get_a_name_of_their_own_and_those_that_do_not_share_one() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(mm(10.0), mm(10.0)), mm(0.5));
    gap(&mut b, p(mm(20.0), mm(10.0)), mm(0.5));
    gap(&mut b, p(mm(30.0), mm(10.0)), mm(0.8));
    gap(&mut b, p(mm(40.0), mm(10.0)), mm(0.8));
    gap(&mut b, p(mm(50.0), mm(10.0)), mm(1.1));
    let names: Vec<(&str, &str)> = b.design().drawings.as_ref().unwrap().board_parts.iter().map(|p| (p.reference.as_str(), p.footprint.as_str())).collect();
    assert_eq!(names, [("REF1", "muwave_gap"), ("REF2", "muwave_gap"), ("REF3", "muwave_gap_1"), ("REF4", "muwave_gap_1"), ("REF5", "muwave_gap_2")]);
    assert_eq!(def(&b, "REF3").name, "muwave_gap_1", "the definition is named as the part is");
    // each keeps its own pads
    assert_eq!(def(&b, "REF1").pads[1].at.x, mm(0.375));
    assert_eq!(def(&b, "REF3").pads[1].at.x, mm(0.525));
}

#[test]
fn a_reference_taken_is_skipped() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(mm(10.0), mm(10.0)), mm(0.5));
    gap(&mut b, p(mm(20.0), mm(10.0)), mm(0.5));
    // delete the first: the next one takes REF1 back
    b.apply(&Cmd::Rip { part: "REF1".into() }).unwrap();
    gap(&mut b, p(mm(30.0), mm(10.0)), mm(0.5));
    let refs: Vec<&str> = b.design().drawings.as_ref().unwrap().board_parts.iter().map(|p| p.reference.as_str()).collect();
    assert_eq!(refs, ["REF1", "REF2"]);
}

/// Deleting the footprint takes what the tool made with it: the part, its fields' edit and its facts, so a footprint that takes the reference later starts clean.
#[test]
fn deleting_a_microwave_footprint_takes_its_edit_and_facts_with_it() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(mm(10.0), mm(10.0)), mm(0.5));
    b.apply(&Cmd::Rip { part: "REF1".into() }).unwrap();
    let dr = b.design().drawings.as_ref().unwrap();
    assert!(dr.board_parts.is_empty() && dr.footprint_edits.is_empty() && dr.footprint_extras.is_empty() && b.design().placement.as_ref().unwrap().footprints.is_empty());
}

// ------------------------------------------------------------------------------------------------------------------------------------ the hand

/// R and F pressed with the footprint in the hand: the pose is turned about the footprint's own anchor and then turned over, as `Cmd::RotateItems` and `Cmd::FlipItems` do.
#[test]
fn what_was_done_in_the_hand_is_the_footprints_pose() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::Stub { at: p(mm(10.0), mm(10.0)), size_nm: mm(1.0), track_width_nm: mm(0.25), hand: Hand { rotate_millideg: 90_000, flip: None } })).unwrap();
    let f = pose(&b, "REF1");
    assert_eq!((f.at, f.rot, f.side), (p(mm(10.0), mm(10.0)), 90_000, Side::Top));
    b.apply(&Cmd::Microwave(MicrowaveCmd::Stub { at: p(mm(30.0), mm(10.0)), size_nm: mm(1.0), track_width_nm: mm(0.25), hand: Hand { rotate_millideg: 0, flip: Some(FlipDirection::LeftRight) } })).unwrap();
    let f = pose(&b, "REF2");
    assert_eq!((f.at, f.side), (p(mm(30.0), mm(10.0)), Side::Bottom), "turned over about its own anchor");
    // its fields went to the back with it (`FOOTPRINT::Flip`)
    let edit = b.design().drawings.as_ref().unwrap().footprint_edits.iter().find(|e| e.id == "REF2").unwrap();
    assert_eq!(edit.reference.as_ref().unwrap().layer, "B.SilkS");
    assert_eq!(edit.value.as_ref().unwrap().layer, "B.Fab");
}

// -------------------------------------------------------------------------------------------------------------------------- the wire format

#[test]
fn the_verbs_read_from_and_write_to_the_json_the_studio_sends() {
    let gap = Cmd::Microwave(MicrowaveCmd::Gap { at: p(mm(1.0), mm(2.0)), size_nm: mm(0.5), track_width_nm: mm(0.25), hand: Hand::default() });
    let text = serde_json::to_string(&gap).unwrap();
    assert_eq!(text, r#"{"op":"microwave","verb":"gap","at":{"x":1000000,"y":2000000},"size_nm":500000,"track_width_nm":250000}"#);
    // (a body the studio parsed, as `/api/cmd` reads it)
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::from_value::<Cmd>(value).unwrap(), gap);

    let shape = serde_json::json!({ "op": "microwave", "verb": "function_shape", "at": { "x": 0, "y": 0 }, "file": "XScale=1", "kind": "symmetrical", "size_x_nm": 4_000_000, "track_width_nm": 250_000, "hand": { "rotate_millideg": 90000, "flip": "left_right" } });
    let Cmd::Microwave(MicrowaveCmd::FunctionShape { kind, size_x_nm, size_y_nm, hand, .. }) = serde_json::from_value(shape).unwrap() else { panic!("not a function shape") };
    assert_eq!((kind, size_x_nm, size_y_nm, hand), (ShapeKind::Symmetrical, Some(mm(4.0)), None, Hand { rotate_millideg: 90_000, flip: Some(FlipDirection::LeftRight) }));

    // the inductor's name defaults to the dialog's "L"
    let ind = serde_json::json!({ "op": "microwave", "verb": "inductor", "start": { "x": 0, "y": 0 }, "end": { "x": 0, "y": 10_000_000 }, "length_nm": 12_000_000, "track_width_nm": 250_000 });
    let Cmd::Microwave(MicrowaveCmd::Inductor { name, .. }) = serde_json::from_value(ind).unwrap() else { panic!("not an inductor") };
    assert_eq!(name, "L");
}

// ----------------------------------------------------------------------------------------------------------------------------- the shape file

const SHAPE: &str = "# a shape\nUnit=MM\nXScale=10\nYScale=2\n\n$COORD\n0 0.5\n0.5 1\n1 0.5\n$ENDCOORD\n";

#[test]
fn a_shape_description_file_is_read_as_readdatashapedescr_reads_it() {
    let s = read_shape_file(SHAPE);
    assert_eq!(s.edges, vec![(0.0, 0.5), (0.5, 1.0), (1.0, 0.5)]);
    assert_eq!((s.scale_x, s.scale_y), (10_000_000.0, 2_000_000.0), "the scales times the unit, in nanometres");
    // inches, any case, a blank for the "="; CRLF; a comment; a word after $ENDCOORD is not read
    let s = read_shape_file("UNIT=Inch\r\nxscale 2\r\nYSCALE 0.5\r\n$coord\r\n0\t0.25\r\n# comment\r\n1  0.75\r\n$endcoord\r\nignored\r\n");
    assert_eq!((s.scale_x, s.scale_y), (50_800_000.0, 12_700_000.0));
    assert_eq!(s.edges, vec![(0.0, 0.25), (1.0, 0.75)]);
    // KiCad's strtok quirks, kept: the first token is split at blanks and "=" but not at a tab, so a tab between a keyword and its value makes the value a missing one; a blank on both
    // sides of the "=" makes the "=" the value; and "XScale" is found only at the very start of the line
    let s = read_shape_file("Unit = Inch\nYScale\t3\n  XScale=4\n");
    assert_eq!((s.scale_x, s.scale_y), (1_000_000.0, 0.0), "the unit stayed mm, the tab line has no value, the indented line is not XScale");
    // nothing said: millimetres and a scale of 1; the numbers C's atof reads
    let s = read_shape_file("$COORD\n1e-1 +.5\n-2 x\n");
    assert_eq!((s.scale_x, s.scale_y), (1_000_000.0, 1_000_000.0));
    assert_eq!(s.edges, vec![(0.1, 0.5), (-2.0, 0.0)], "no $ENDCOORD: to the end of the file; atof of a word is 0");
    // the reader stops at $ENDCOORD in the outer loop too
    assert!(read_shape_file("$ENDCOORD\n$COORD\n1 1\n").edges.is_empty());
}

fn shape(kind: ShapeKind, file: &str) -> Cmd {
    Cmd::Microwave(MicrowaveCmd::FunctionShape { at: p(mm(10.0), mm(10.0)), file: file.into(), kind, size_x_nm: None, size_y_nm: None, track_width_nm: mm(0.25), hand: Hand::default() })
}

fn polygon(b: &Board) -> Vec<Point> {
    match &def(b, "REF1").graphics[0] {
        Shape::Polygon { pts, layer, filled, stroke_width, .. } => {
            assert_eq!((layer.as_str(), *filled, *stroke_width), ("F.Cu", true, Nm::ZERO), "a filled polygon on the copper layer with no outline");
            pts.clone()
        }
        other => panic!("a polygon, not {other:?}"),
    }
}

/// `createPolygonShape`: two pads `size.x` apart with the anchor between them, the polygon from `(offset.x, 0)` through the points (`x * scaleX`, `-y * scaleY`) and down to the line.
#[test]
fn a_function_shape_is_a_polygon_between_two_pads() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&shape(ShapeKind::Normal, SHAPE)).unwrap();
    assert_eq!(part(&b, "REF1").footprint, "muwave_polygon");
    let fp = def(&b, "REF1");
    assert_eq!((fp.pads.len(), fp.pads[0].at, fp.pads[1].at, fp.pads[0].size), (2, p(mm(-5.0), Nm::ZERO), p(mm(5.0), Nm::ZERO), (mm(0.25), mm(0.25))));
    assert_eq!(polygon(&b), vec![p(mm(-5.0), Nm::ZERO), p(mm(-5.0), mm(-1.0)), p(Nm::ZERO, mm(-2.0)), p(mm(5.0), mm(-1.0)), p(mm(5.0), Nm::ZERO)]);
    // the texts keep the board's sizes (`createBaseFootprint( .., 0, .. )`)
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    assert_eq!(edit.reference.as_ref().unwrap().size, (mm(1.0), mm(1.0)));
}

#[test]
fn a_symmetrical_shape_has_its_mirror_image_and_a_mirrored_one_is_below_the_line() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&shape(ShapeKind::Symmetrical, SHAPE)).unwrap();
    // the five points, then the same five from the last to the first with y negated
    assert_eq!(
        polygon(&b),
        vec![p(mm(-5.0), Nm::ZERO), p(mm(-5.0), mm(-1.0)), p(Nm::ZERO, mm(-2.0)), p(mm(5.0), mm(-1.0)), p(mm(5.0), Nm::ZERO), p(mm(5.0), Nm::ZERO), p(mm(5.0), mm(1.0)), p(Nm::ZERO, mm(2.0)), p(mm(-5.0), mm(1.0)), p(mm(-5.0), Nm::ZERO)]
    );
    let mut b = board(&model);
    b.apply(&shape(ShapeKind::Mirrored, SHAPE)).unwrap();
    assert_eq!(polygon(&b), vec![p(mm(-5.0), Nm::ZERO), p(mm(-5.0), mm(1.0)), p(Nm::ZERO, mm(2.0)), p(mm(5.0), mm(1.0)), p(mm(5.0), Nm::ZERO)]);
}

#[test]
fn a_shape_that_ends_on_the_line_is_not_closed_down_to_it() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    // the last point has y = 0: `if( last_coordinate.y != 0 )` adds nothing
    b.apply(&shape(ShapeKind::Normal, "XScale=4\nYScale=1\n$COORD\n0 0\n0.5 1\n1 0\n$ENDCOORD\n")).unwrap();
    assert_eq!(polygon(&b), vec![p(mm(-2.0), Nm::ZERO), p(mm(-2.0), Nm::ZERO), p(Nm::ZERO, mm(-1.0)), p(mm(2.0), Nm::ZERO)]);
}

#[test]
fn the_size_boxes_of_the_dialog_scale_the_shape() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::FunctionShape { at: p(Nm::ZERO, Nm::ZERO), file: SHAPE.into(), kind: ShapeKind::Normal, size_x_nm: Some(mm(20.0)), size_y_nm: Some(mm(1.0)), track_width_nm: mm(0.25), hand: Hand::default() })).unwrap();
    assert_eq!(def(&b, "REF1").pads[1].at.x, mm(10.0));
    assert_eq!(polygon(&b), vec![p(mm(-10.0), Nm::ZERO), p(mm(-10.0), mm(-0.5)), p(Nm::ZERO, mm(-1.0)), p(mm(10.0), mm(-0.5)), p(mm(10.0), Nm::ZERO)]);
}

#[test]
fn a_shape_with_no_points_or_no_size_is_refused_with_kicads_words() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let e = b.apply(&shape(ShapeKind::Normal, "Unit=MM\nXScale=10\nYScale=2\n")).unwrap_err();
    assert_eq!(e[0].hint.as_deref(), Some("Shape has no points."));
    // the size is checked first
    let e = b.apply(&shape(ShapeKind::Normal, "XScale=0\nYScale=2\n$COORD\n0 0\n1 1\n$ENDCOORD\n")).unwrap_err();
    assert_eq!(e[0].hint.as_deref(), Some("Shape has a null size."));
    let e = b.apply(&shape(ShapeKind::Normal, "")).unwrap_err();
    assert_eq!(e[0].hint.as_deref(), Some("Shape has no points."));
    assert!(b.design().drawings.as_ref().map_or(true, |d| d.board_parts.is_empty()));
}

// ----------------------------------------------------------------------------------------------------------------------------- the inductor

fn inductor(start: Point, end: Point, length_nm: Nm) -> Cmd {
    Cmd::Microwave(MicrowaveCmd::Inductor { start, end, length_nm, name: "L".into(), track_width_nm: mm(0.25) })
}

/// The segments of the coil as `(start, end)` pairs, in the footprint's frame.
fn coil(b: &Board, reference: &str) -> Vec<(Point, Point)> {
    def(b, reference)
        .graphics
        .iter()
        .map(|g| match g {
            Shape::Segment { layer, stroke_width, start, end, .. } => {
                assert_eq!((layer.as_str(), *stroke_width), ("F.Cu", mm(0.25)), "a segment of the track's width on the footprint's layer");
                (*start, *end)
            }
            other => panic!("a segment, not {other:?}"),
        })
        .collect()
}

fn path_length(segments: &[(Point, Point)]) -> f64 {
    segments.iter().map(|(a, b)| ((b.x - a.x).as_f64().powi(2) + (b.y - a.y).as_f64().powi(2)).sqrt()).sum()
}

/// `createMicrowaveInductor`: the footprint is anchored at the end point, the coil runs from the start (pad 2) to the end (pad 1), is as long as asked (the chords that stand for its arcs
/// are corrected by `ADJUST_SIZE`) and sits between the two points.
#[test]
fn an_inductor_is_an_s_shaped_coil_between_the_two_points() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let (start, end) = (p(mm(20.0), mm(20.0)), p(mm(20.0), mm(30.0)));
    b.apply(&inductor(start, end, mm(30.0))).unwrap();
    let bp = part(&b, "REF1");
    assert_eq!((bp.footprint.as_str(), bp.value.as_deref()), ("mw_inductor", Some("L")), "`LIB_ID( wxEmptyString, \"mw_inductor\" )` and the value the dialog asked for");
    let f = pose(&b, "REF1");
    assert_eq!((f.at, f.rot, f.side), (end, 0, Side::Top), "`footprint->SetPosition( aInductorPattern.m_End )`");
    let fp = def(&b, "REF1");
    // two round pads of the track width: 1 at the end, 2 at the start
    assert_eq!(fp.pads.len(), 2);
    assert_eq!((fp.pads[0].number.as_str(), fp.pads[0].at, fp.pads[0].shape, fp.pads[0].size), ("1", p(Nm::ZERO, Nm::ZERO), LibraryPadShape::Circle, (mm(0.25), mm(0.25))));
    assert_eq!((fp.pads[1].number.as_str(), fp.pads[1].at), ("2", p(Nm::ZERO, mm(-10.0))));
    assert!(fp.pads.iter().all(|p| p.layers == ["F.Cu"] && p.kind == PadKind::Smd));
    // the coil
    let segs = coil(&b, "REF1");
    assert!(segs.len() > 50, "arcs are many chords");
    assert_eq!(segs[0].0, p(Nm::ZERO, mm(-10.0)), "it starts at pad 2");
    assert_eq!(segs.last().unwrap().1, p(Nm::ZERO, Nm::ZERO), "and ends at pad 1");
    assert!(segs.windows(2).all(|w| w[0].1 == w[1].0), "one connected path");
    let length = path_length(&segs);
    assert!((length - 30_000_000.0).abs() < 0.01 * 30_000_000.0, "the coil is {length} nm long for 30 mm");
    assert!(segs.iter().all(|(a, b)| (mm(-10.0)..=Nm::ZERO).contains(&a.y) && (mm(-10.0)..=Nm::ZERO).contains(&b.y)), "between the two points");
    assert!(segs.iter().all(|(a, _)| a.x.abs() < mm(4.0)), "within the room of half the span");
    // the texts: a text height above and below the middle of the coil
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    assert_eq!(edit.reference.as_ref().unwrap().at, p(Nm::ZERO, mm(-5.0) - mm(1.0)), "the Reference above the middle");
    assert_eq!(edit.value.as_ref().unwrap().at, p(Nm::ZERO, mm(-5.0) + mm(1.0)), "the Value below it");
    assert_eq!(fp.texts[0].at, p(Nm::ZERO, mm(2.5)), "the `${{REFERENCE}}` text keeps its place at the anchor");
}

/// The coil is worked out for a vertical one and turned to where the two points are: a horizontal span and a slanted one give a coil of the same length that starts and ends where they do.
#[test]
fn an_inductor_turns_to_where_its_two_points_are() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let cases = [(p(mm(10.0), mm(10.0)), p(mm(20.0), mm(10.0))), (p(mm(30.0), mm(30.0)), p(mm(20.0), mm(30.0))), (p(mm(10.0), mm(40.0)), p(mm(16.0), mm(48.0)))];
    for (i, (s, e)) in cases.iter().enumerate() {
        let min = ((e.x - s.x).as_f64().powi(2) + (e.y - s.y).as_f64().powi(2)).sqrt();
        let want = Nm::kiround(min * 3.0);
        b.apply(&inductor(*s, *e, want)).unwrap();
        let segs = coil(&b, &format!("REF{}", i + 1));
        assert_eq!((segs[0].0, segs.last().unwrap().1), (p(s.x - e.x, s.y - e.y), p(Nm::ZERO, Nm::ZERO)), "case {i}");
        let length = path_length(&segs);
        assert!((length - want.as_f64()).abs() < 0.015 * want.as_f64(), "case {i}: {length} for {want}");
    }
}

/// The four ways `createMicrowaveInductor` says no, in KiCad's words, and nothing placed.
#[test]
fn a_length_the_coil_cannot_have_is_refused_with_kicads_message() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let (start, end) = (p(mm(20.0), mm(20.0)), p(mm(20.0), mm(30.0)));
    for (length, message) in [
        (mm(9.0), "Requested length < minimum length"),
        (mm(11.0), "Requested length too small"),
        (mm(15.0), "Requested length can't be represented"),
        (mm(100.0), "Requested length too large"),
    ] {
        let e = b.apply(&inductor(start, end, length)).unwrap_err();
        assert_eq!(e[0].hint.as_deref(), Some(message), "length {length}");
    }
    // the shortest that works is the same as the longest that does not
    assert!(b.apply(&inductor(start, end, mm(20.0))).is_ok());
    assert_eq!(b.design().drawings.as_ref().unwrap().board_parts.len(), 1, "only the one that worked");
}

#[test]
fn an_inductor_needs_two_points_a_name_and_a_track() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let e = b.apply(&inductor(p(mm(1.0), mm(1.0)), p(mm(1.0), mm(1.0)), mm(5.0))).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
    let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(Nm::ZERO, Nm::ZERO), end: p(Nm::ZERO, mm(10.0)), length_nm: mm(20.0), name: "  ".into(), track_width_nm: mm(0.25) })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
    let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(Nm::ZERO, Nm::ZERO), end: p(Nm::ZERO, mm(10.0)), length_nm: mm(20.0), name: "L".into(), track_width_nm: Nm::ZERO })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
    // a value with a character `FOOTPRINT_NAME_VALIDATOR` does not take
    for bad in ["L/1", "L:2", "L%", "a\"b"] {
        let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(Nm::ZERO, Nm::ZERO), end: p(Nm::ZERO, mm(10.0)), length_nm: mm(20.0), name: bad.into(), track_width_nm: mm(0.25) })).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_microwave", "{bad}");
    }
    // the value is what the dialog's "Component value:" holds
    b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(Nm::ZERO, Nm::ZERO), end: p(Nm::ZERO, mm(10.0)), length_nm: mm(20.0), name: "L22".into(), track_width_nm: mm(0.25) })).unwrap();
    assert_eq!(part(&b, "REF1").value.as_deref(), Some("L22"));
}

/// The S shape itself (a 30 mm coil of 0.25 mm track over 10 mm): the corners start at the start point and end at the end point, and there are no more strands than the room allows.
#[test]
fn the_s_shape_runs_from_the_start_to_the_end() {
    let (start, end) = (p(Nm::ZERO, Nm::ZERO), p(Nm::ZERO, mm(10.0)));
    let SShape::Corners(c) = build_corners_s_shape(start, end, mm(30.0), mm(0.25)) else { panic!("a coil") };
    assert_eq!((c[0], *c.last().unwrap()), (start, end));
    // the first stub is straight down from the start by `stubs_len` (a tenth of the span), before the first arc
    assert_eq!((c[1].x, c[1].y > Nm::ZERO), (Nm::ZERO, true));
    assert_eq!(build_corners_s_shape(start, end, mm(9.0), mm(0.25)), SShape::TooShort, "length is checked against the stubs and four quarter circles");
}
