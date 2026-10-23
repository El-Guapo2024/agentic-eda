//! The microwave tools' verbs (`microwave.rs`): what each footprint is, number for number, as `microwave_footprint.cpp`, `microwave_inductor.cpp` and
//! `microwave_polygon.cpp` build it, and how it lands on the board.

use super::microwave::*;
use super::pcb_transform::FlipDirection;
use super::{Board, Cmd};
use eda_model::footprint::PadKind;
use eda_model::ir::{Design, LibraryFootprint, LibraryPadShape, Point, Shape, Side};
use eda_model::ConstraintModel;

fn p(x: i64, y: i64) -> Point {
    Point { x, y }
}

/// A design built from JSON, so these tests do not break when another section is added to `Design`.
fn board(model: &ConstraintModel) -> Board<'_> {
    let design: Design = serde_json::from_str(
        r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]},
            "placement":{"outline":[{"x":0,"y":0},{"x":100000,"y":0},{"x":100000,"y":100000},{"x":0,"y":100000}],"footprints":[],"modules":[]}}"#,
    )
    .unwrap();
    Board::new(design, model, 100, 300)
}

fn gap(b: &mut Board, at: Point, size_nm: i64) {
    b.apply(&Cmd::Microwave(MicrowaveCmd::Gap { at, size_nm, track_width_nm: 250, hand: Hand::default() })).unwrap();
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
    gap(&mut b, p(10_000, 20_000), 500);

    let bp = part(&b, "REF1");
    assert_eq!((bp.footprint.as_str(), bp.value.as_deref()), ("muwave_gap", Some("muwave_gap")), "the name is the footprint's and its value");
    assert!(bp.pad_nets.is_empty(), "the pads are on no net");
    let fp = def(&b, "REF1");
    assert_eq!(fp.pads.len(), 2);
    let (a, c) = (&fp.pads[0], &fp.pads[1]);
    assert_eq!((a.number.as_str(), a.at, a.size, a.shape, a.kind), ("1", p(-375, 0), (250, 250), LibraryPadShape::Rect, PadKind::Smd));
    assert_eq!((c.number.as_str(), c.at, c.size, c.shape, c.kind), ("2", p(375, 0), (250, 250), LibraryPadShape::Rect, PadKind::Smd));
    assert_eq!(a.layers, vec!["F.Cu".to_string()], "`pad->SetLayerSet( {{ F_Cu }} )`: copper and nothing else");
    // the pose is the click
    let f = pose(&b, "REF1");
    assert_eq!((f.at, f.rot, f.side), (p(10_000, 20_000), 0, Side::Top));
    // the gap between the pads' edges is the gap asked for
    assert_eq!((c.at.x - c.size.0 / 2) - (a.at.x + a.size.0 / 2), 500);
}

/// `createBaseFootprint` + `CreateNewFootprint`: Reference `REF**` on the silkscreen half a text height above the origin and Value on the fabrication layer below it, both as big as the
/// track is wide with a thickness of a fifth of it (`text_size = gap_size` is read before the dialog), the `${REFERENCE}` text under them, and the attributes.
#[test]
fn a_gaps_fields_texts_and_attributes_are_createnewfootprints() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(0, 0), 500);
    let dr = b.design().drawings.as_ref().unwrap();
    let edit = dr.footprint_edits.iter().find(|e| e.id == "REF1").expect("the fields are laid out");
    let (r, v) = (edit.reference.as_ref().unwrap(), edit.value.as_ref().unwrap());
    assert_eq!((r.layer.as_str(), r.at, r.size, r.thickness, r.visible), ("F.SilkS", p(0, -500), (250, 250), 50, true));
    assert_eq!((v.layer.as_str(), v.at, v.size, v.thickness, v.visible), ("F.Fab", p(0, 1000), (250, 250), 50, true));
    let fp = def(&b, "REF1");
    assert_eq!(fp.texts.len(), 1);
    let t = &fp.texts[0];
    assert_eq!((t.content.as_str(), t.layer.as_str(), t.at, t.size_nm, t.stroke_width), ("${REFERENCE}", "F.Fab", p(0, 2500), 1000, 150), "the third default text item of the board, at the board's size");
    // `SetAttributes( FP_EXCLUDE_FROM_POS_FILES | FP_EXCLUDE_FROM_BOM )`: neither SMD nor through hole
    let a = edit.attrs.expect("the attributes are set");
    assert_eq!((a.kind, a.exclude_from_pos_files, a.exclude_from_bom, a.board_only, a.dnp), (eda_model::fp_edit::FootprintKind::Unspecified, true, true, false, false));
    // no courtyard, copper only: what a footprint read from a board file with these pads would have
    let extra = dr.footprint_extras.iter().find(|e| e.id == "REF1").expect("the facts of the footprint");
    assert!(extra.no_courtyard);
    assert_eq!(extra.pads.len(), 2);
    assert!(extra.pads.iter().all(|p| p.no_paste && p.no_mask && p.layers == ["F.Cu"]));
    // the engine has no keep-out to enforce for them either: a point, not the box of the pads
    assert_eq!(def(&b, "REF1").to_engine_footprint().courtyard_half(), (1, 1));
}

/// The texts take the board's Text & Graphics defaults per layer: a board with 1.2 mm silkscreen text and 0.8 mm fabrication text puts the Reference half a silkscreen text
/// height above the origin and the Value half a fabrication text height below the end of the Reference's line.
#[test]
fn the_boards_text_defaults_place_and_size_the_fields() {
    let mut model = ConstraintModel::default();
    model.board.text_graphics.silk.text_height_nm = 1_200;
    model.board.text_graphics.silk.text_width_nm = 1_200;
    model.board.text_graphics.fab.text_height_nm = 800;
    model.board.text_graphics.fab.text_width_nm = 800;
    model.board.text_graphics.fab.text_thickness_nm = 120;
    let mut b = board(&model);
    // A stub of the arc kind keeps the board's sizes (`text_size` 0).
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(0, 0), radius_nm: 1_000, angle_millideg: 90_000, track_width_nm: 250, hand: Hand::default() })).unwrap();
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    let (r, v) = (edit.reference.as_ref().unwrap(), edit.value.as_ref().unwrap());
    // y = -silk/2 = -600; then + silk = 600; + fab/2 = 1000 (the Value); + fab = 1800; + fab/2 = 2200 (the user text)
    assert_eq!((r.at, r.size), (p(0, -600), (1_200, 1_200)));
    assert_eq!((v.at, v.size, v.thickness), (p(0, 1_000), (800, 800), 120));
    assert_eq!(def(&b, "REF1").texts[0].at, p(0, 2_200));
}

/// KiCad's integers are nanometres, and `/ 2` truncates there: an odd sum of micrometres leaves half a micrometre, which the IR rounds away from zero.
#[test]
fn an_odd_gap_is_divided_in_nanometres_and_rounded_to_the_micrometre() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(0, 0), 333);
    let fp = def(&b, "REF1");
    // offsetX = -(333000 + 250000) / 2 = -291500 nm; the second pad at -291500 + 333000 + 250000 = 291500 nm
    assert_eq!((fp.pads[0].at.x, fp.pads[1].at.x), (-292, 292));
}

#[test]
fn a_gap_needs_a_size_and_a_track() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    for (size, track) in [(0, 250), (-5, 250), (500, 0)] {
        let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Gap { at: p(0, 0), size_nm: size, track_width_nm: track, hand: Hand::default() })).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_microwave", "{size} {track}");
    }
    assert!(b.design().drawings.as_ref().map_or(true, |d| d.board_parts.is_empty()), "a refused command changes nothing");
}

// ---------------------------------------------------------------------------------------------------------------------------------- the stub

/// `createFootprint( STUB )`: pad 1 is the square one at the origin, pad 2 is as wide as the track and as long as the stub, `-( stub + h ) / 2` above it.
#[test]
fn a_stub_is_a_square_pad_and_the_stub_above_it() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::Stub { at: p(5_000, 5_000), size_nm: 1_000, track_width_nm: 250, hand: Hand::default() })).unwrap();
    let bp = part(&b, "REF1");
    assert_eq!(bp.footprint, "muwave_stub");
    let fp = def(&b, "REF1");
    let (a, s) = (&fp.pads[0], &fp.pads[1]);
    assert_eq!((a.number.as_str(), a.at, a.size), ("1", p(0, 0), (250, 250)));
    assert_eq!((s.number.as_str(), s.at, s.size), ("2", p(0, -625), (250, 1_000)));
    // the stub's far end is `stub + h / 2` above the origin and its near end touches the square pad
    assert_eq!((s.at.y - s.size.1 / 2, s.at.y + s.size.1 / 2), (-1_125, -125));
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    assert_eq!(edit.reference.as_ref().unwrap().size, (250, 250), "the stub's texts take the track width too");
}

// ------------------------------------------------------------------------------------------------------------------------------ the arc stub

/// The thetas `createFootprint( STUB_ARC )` walks: from `-angle / 2` in steps of 5 degrees, the last clamped to `angle / 2`; the points are `(0, -r)` turned by them.
fn arc_points(b: &Board) -> Vec<Point> {
    match &def(b, "REF1").pads[0].custom.as_ref().expect("a custom pad").primitives[0].shape {
        Shape::Polygon { pts, filled, stroke_width, .. } => {
            assert!(*filled && *stroke_width == 0, "`AddPrimitivePoly( .., 0, true )`");
            pts.clone()
        }
        other => panic!("a polygon, not {other:?}"),
    }
}

#[test]
fn an_arc_stub_is_one_custom_pad_with_a_pie_slice() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(0, 0), radius_nm: 1_000, angle_millideg: 90_000, track_width_nm: 250, hand: Hand::default() })).unwrap();
    assert_eq!(part(&b, "REF1").footprint, "muwave_arcstub");
    let fp = def(&b, "REF1");
    assert_eq!(fp.pads.len(), 1, "`pad_count = 1`");
    let pad = &fp.pads[0];
    assert_eq!((pad.number.as_str(), pad.shape, pad.size, pad.at), ("1", LibraryPadShape::Custom, (250, 250), p(0, 0)));
    let custom = pad.custom.as_ref().unwrap();
    assert_eq!(custom.anchor, eda_model::footprint::PadAnchor::Rect, "`SetAnchorPadShape( .., RECTANGLE )`");
    let pts = arc_points(&b);
    // numPoints = 90 / 5 + 3 = 21: the origin, 19 points from -45 to +45 degrees, and the origin again
    assert_eq!(pts.len(), 21);
    assert_eq!((pts[0], pts[20]), (p(0, 0), p(0, 0)));
    assert_eq!(pts[1], p(707, -707), "theta = -45: RotatePoint turns (0, -r) toward +x");
    assert_eq!(pts[10], p(0, -1_000), "the middle of the slice points up");
    assert_eq!(pts[19], p(-707, -707));
    // every point of the arc is a radius from the origin, to the micrometre
    for q in &pts[1..20] {
        let r = ((q.x * q.x + q.y * q.y) as f64).sqrt();
        assert!((r - 1_000.0).abs() <= 1.0, "{q:?} is {r} from the origin");
    }
}

#[test]
fn the_arc_stubs_angle_is_taken_as_an_absolute_value_and_clamped_to_a_half_turn() {
    let model = ConstraintModel::default();
    // 270 degrees is 180: 180 / 5 + 3 = 39 points in all, -90 .. +90 degrees
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(0, 0), radius_nm: 2_000, angle_millideg: 270_000, track_width_nm: 250, hand: Hand::default() })).unwrap();
    let pts = arc_points(&b);
    assert_eq!(pts.len(), 39);
    assert_eq!((pts[1], pts[37]), (p(2_000, 0), p(-2_000, 0)));
    // a negative angle is its absolute value
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(0, 0), radius_nm: 1_000, angle_millideg: -90_000, track_width_nm: 250, hand: Hand::default() })).unwrap();
    assert_eq!(arc_points(&b).len(), 21);
    // an angle of nothing is the one point the loop makes: origin, (0, -r), origin
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(0, 0), radius_nm: 1_000, angle_millideg: 0, track_width_nm: 250, hand: Hand::default() })).unwrap();
    assert_eq!(arc_points(&b), vec![p(0, 0), p(0, -1_000), p(0, 0)]);
    // an angle that does not divide by 5: the last step is clamped to angle / 2 (12 degrees: -6, -1, 4, 6 -> numPoints = 12 / 5 + 3 = 5, three points)
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(0, 0), radius_nm: 10_000, angle_millideg: 12_000, track_width_nm: 250, hand: Hand::default() })).unwrap();
    let pts = arc_points(&b);
    assert_eq!(pts.len(), 5);
    // thetas -6, -1, 4: x = round(r * sin(6)) = 1045, round(r * sin(1)) = 175, round(-r * sin(4)) = -698
    assert_eq!(&pts[1..4], &[p(1_045, -9_945), p(175, -9_998), p(-698, -9_976)]);
}

#[test]
fn an_arc_stub_needs_a_radius() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let e = b.apply(&Cmd::Microwave(MicrowaveCmd::ArcStub { at: p(0, 0), radius_nm: 0, angle_millideg: 90_000, track_width_nm: 250, hand: Hand::default() })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
}

// ------------------------------------------------------------------------------------------------------------------------ names and references

/// The model finds a footprint by its name, so footprints that differ cannot share one; footprints that are alike do. The references are the next free ones.
#[test]
fn footprints_that_differ_get_a_name_of_their_own_and_those_that_do_not_share_one() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(10_000, 10_000), 500);
    gap(&mut b, p(20_000, 10_000), 500);
    gap(&mut b, p(30_000, 10_000), 800);
    gap(&mut b, p(40_000, 10_000), 800);
    gap(&mut b, p(50_000, 10_000), 1_100);
    let names: Vec<(&str, &str)> = b.design().drawings.as_ref().unwrap().board_parts.iter().map(|p| (p.reference.as_str(), p.footprint.as_str())).collect();
    assert_eq!(names, [("REF1", "muwave_gap"), ("REF2", "muwave_gap"), ("REF3", "muwave_gap_1"), ("REF4", "muwave_gap_1"), ("REF5", "muwave_gap_2")]);
    assert_eq!(def(&b, "REF3").name, "muwave_gap_1", "the definition is named as the part is");
    // each keeps its own pads
    assert_eq!(def(&b, "REF1").pads[1].at.x, 375);
    assert_eq!(def(&b, "REF3").pads[1].at.x, 525);
}

#[test]
fn a_reference_taken_is_skipped() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(10_000, 10_000), 500);
    gap(&mut b, p(20_000, 10_000), 500);
    // delete the first: the next one takes REF1 back
    b.apply(&Cmd::Rip { part: "REF1".into() }).unwrap();
    gap(&mut b, p(30_000, 10_000), 500);
    let refs: Vec<&str> = b.design().drawings.as_ref().unwrap().board_parts.iter().map(|p| p.reference.as_str()).collect();
    assert_eq!(refs, ["REF1", "REF2"]);
}

/// Deleting the footprint takes what the tool made with it: the part, its fields' edit and its facts, so a footprint that takes the reference later starts clean.
#[test]
fn deleting_a_microwave_footprint_takes_its_edit_and_facts_with_it() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    gap(&mut b, p(10_000, 10_000), 500);
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
    b.apply(&Cmd::Microwave(MicrowaveCmd::Stub { at: p(10_000, 10_000), size_nm: 1_000, track_width_nm: 250, hand: Hand { rotate_millideg: 90_000, flip: None } })).unwrap();
    let f = pose(&b, "REF1");
    assert_eq!((f.at, f.rot, f.side), (p(10_000, 10_000), 90_000, Side::Top));
    b.apply(&Cmd::Microwave(MicrowaveCmd::Stub { at: p(30_000, 10_000), size_nm: 1_000, track_width_nm: 250, hand: Hand { rotate_millideg: 0, flip: Some(FlipDirection::LeftRight) } })).unwrap();
    let f = pose(&b, "REF2");
    assert_eq!((f.at, f.side), (p(30_000, 10_000), Side::Bottom), "turned over about its own anchor");
    // its fields went to the back with it (`FOOTPRINT::Flip`)
    let edit = b.design().drawings.as_ref().unwrap().footprint_edits.iter().find(|e| e.id == "REF2").unwrap();
    assert_eq!(edit.reference.as_ref().unwrap().layer, "B.SilkS");
    assert_eq!(edit.value.as_ref().unwrap().layer, "B.Fab");
}

// -------------------------------------------------------------------------------------------------------------------------- the wire format

#[test]
fn the_verbs_read_from_and_write_to_the_json_the_studio_sends() {
    let gap = Cmd::Microwave(MicrowaveCmd::Gap { at: p(1_000, 2_000), size_nm: 500, track_width_nm: 250, hand: Hand::default() });
    let text = serde_json::to_string(&gap).unwrap();
    assert_eq!(text, r#"{"op":"microwave","verb":"gap","at":{"x":1000,"y":2000},"size_nm":500,"track_width_nm":250}"#);
    // (a body the studio parsed, as `/api/cmd` reads it)
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::from_value::<Cmd>(value).unwrap(), gap);

    let shape = serde_json::json!({ "op": "microwave", "verb": "function_shape", "at": { "x": 0, "y": 0 }, "file": "XScale=1", "kind": "symmetrical", "size_x_nm": 4000, "track_width_nm": 250, "hand": { "rotate_millideg": 90000, "flip": "left_right" } });
    let Cmd::Microwave(MicrowaveCmd::FunctionShape { kind, size_x_nm, size_y_nm, hand, .. }) = serde_json::from_value(shape).unwrap() else { panic!("not a function shape") };
    assert_eq!((kind, size_x_nm, size_y_nm, hand), (ShapeKind::Symmetrical, Some(4_000), None, Hand { rotate_millideg: 90_000, flip: Some(FlipDirection::LeftRight) }));

    // the inductor's name defaults to the dialog's "L"
    let ind = serde_json::json!({ "op": "microwave", "verb": "inductor", "start": { "x": 0, "y": 0 }, "end": { "x": 0, "y": 10000 }, "length_nm": 12000, "track_width_nm": 250 });
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
    Cmd::Microwave(MicrowaveCmd::FunctionShape { at: p(10_000, 10_000), file: file.into(), kind, size_x_nm: None, size_y_nm: None, track_width_nm: 250, hand: Hand::default() })
}

fn polygon(b: &Board) -> Vec<Point> {
    match &def(b, "REF1").graphics[0] {
        Shape::Polygon { pts, layer, filled, stroke_width, .. } => {
            assert_eq!((layer.as_str(), *filled, *stroke_width), ("F.Cu", true, 0), "a filled polygon on the copper layer with no outline");
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
    assert_eq!((fp.pads.len(), fp.pads[0].at, fp.pads[1].at, fp.pads[0].size), (2, p(-5_000, 0), p(5_000, 0), (250, 250)));
    assert_eq!(polygon(&b), vec![p(-5_000, 0), p(-5_000, -1_000), p(0, -2_000), p(5_000, -1_000), p(5_000, 0)]);
    // the texts keep the board's sizes (`createBaseFootprint( .., 0, .. )`)
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    assert_eq!(edit.reference.as_ref().unwrap().size, (1_000, 1_000));
}

#[test]
fn a_symmetrical_shape_has_its_mirror_image_and_a_mirrored_one_is_below_the_line() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&shape(ShapeKind::Symmetrical, SHAPE)).unwrap();
    // the five points, then the same five from the last to the first with y negated
    assert_eq!(
        polygon(&b),
        vec![p(-5_000, 0), p(-5_000, -1_000), p(0, -2_000), p(5_000, -1_000), p(5_000, 0), p(5_000, 0), p(5_000, 1_000), p(0, 2_000), p(-5_000, 1_000), p(-5_000, 0)]
    );
    let mut b = board(&model);
    b.apply(&shape(ShapeKind::Mirrored, SHAPE)).unwrap();
    assert_eq!(polygon(&b), vec![p(-5_000, 0), p(-5_000, 1_000), p(0, 2_000), p(5_000, 1_000), p(5_000, 0)]);
}

#[test]
fn a_shape_that_ends_on_the_line_is_not_closed_down_to_it() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    // the last point has y = 0: `if( last_coordinate.y != 0 )` adds nothing
    b.apply(&shape(ShapeKind::Normal, "XScale=4\nYScale=1\n$COORD\n0 0\n0.5 1\n1 0\n$ENDCOORD\n")).unwrap();
    assert_eq!(polygon(&b), vec![p(-2_000, 0), p(-2_000, 0), p(0, -1_000), p(2_000, 0)]);
}

#[test]
fn the_size_boxes_of_the_dialog_scale_the_shape() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    b.apply(&Cmd::Microwave(MicrowaveCmd::FunctionShape { at: p(0, 0), file: SHAPE.into(), kind: ShapeKind::Normal, size_x_nm: Some(20_000), size_y_nm: Some(1_000), track_width_nm: 250, hand: Hand::default() })).unwrap();
    assert_eq!(def(&b, "REF1").pads[1].at.x, 10_000);
    assert_eq!(polygon(&b), vec![p(-10_000, 0), p(-10_000, -500), p(0, -1_000), p(10_000, -500), p(10_000, 0)]);
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

fn inductor(start: Point, end: Point, length_nm: i64) -> Cmd {
    Cmd::Microwave(MicrowaveCmd::Inductor { start, end, length_nm, name: "L".into(), track_width_nm: 250 })
}

/// The segments of the coil as `(start, end)` pairs, in the footprint's frame.
fn coil(b: &Board, reference: &str) -> Vec<(Point, Point)> {
    def(b, reference)
        .graphics
        .iter()
        .map(|g| match g {
            Shape::Segment { layer, stroke_width, start, end, .. } => {
                assert_eq!((layer.as_str(), *stroke_width), ("F.Cu", 250), "a segment of the track's width on the footprint's layer");
                (*start, *end)
            }
            other => panic!("a segment, not {other:?}"),
        })
        .collect()
}

fn path_length(segments: &[(Point, Point)]) -> f64 {
    segments.iter().map(|(a, b)| (((b.x - a.x) as f64).powi(2) + ((b.y - a.y) as f64).powi(2)).sqrt()).sum()
}

/// `createMicrowaveInductor`: the footprint is anchored at the end point, the coil runs from the start (pad 2) to the end (pad 1), is as long as asked (the chords that stand for its arcs
/// are corrected by `ADJUST_SIZE`) and sits between the two points.
#[test]
fn an_inductor_is_an_s_shaped_coil_between_the_two_points() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let (start, end) = (p(20_000, 20_000), p(20_000, 30_000));
    b.apply(&inductor(start, end, 30_000)).unwrap();
    let bp = part(&b, "REF1");
    assert_eq!((bp.footprint.as_str(), bp.value.as_deref()), ("mw_inductor", Some("L")), "`LIB_ID( wxEmptyString, \"mw_inductor\" )` and the value the dialog asked for");
    let f = pose(&b, "REF1");
    assert_eq!((f.at, f.rot, f.side), (end, 0, Side::Top), "`footprint->SetPosition( aInductorPattern.m_End )`");
    let fp = def(&b, "REF1");
    // two round pads of the track width: 1 at the end, 2 at the start
    assert_eq!(fp.pads.len(), 2);
    assert_eq!((fp.pads[0].number.as_str(), fp.pads[0].at, fp.pads[0].shape, fp.pads[0].size), ("1", p(0, 0), LibraryPadShape::Circle, (250, 250)));
    assert_eq!((fp.pads[1].number.as_str(), fp.pads[1].at), ("2", p(0, -10_000)));
    assert!(fp.pads.iter().all(|p| p.layers == ["F.Cu"] && p.kind == PadKind::Smd));
    // the coil
    let segs = coil(&b, "REF1");
    assert!(segs.len() > 50, "arcs are many chords");
    assert_eq!(segs[0].0, p(0, -10_000), "it starts at pad 2");
    assert_eq!(segs.last().unwrap().1, p(0, 0), "and ends at pad 1");
    assert!(segs.windows(2).all(|w| w[0].1 == w[1].0), "one connected path");
    let length = path_length(&segs);
    assert!((length - 30_000.0).abs() < 0.01 * 30_000.0, "the coil is {length} um long for 30000");
    assert!(segs.iter().all(|(a, b)| (-10_000..=0).contains(&a.y) && (-10_000..=0).contains(&b.y)), "between the two points");
    assert!(segs.iter().all(|(a, _)| a.x.abs() < 4_000), "within the room of half the span");
    // the texts: a text height above and below the middle of the coil
    let edit = &b.design().drawings.as_ref().unwrap().footprint_edits[0];
    assert_eq!(edit.reference.as_ref().unwrap().at, p(0, -5_000 - 1_000), "the Reference above the middle");
    assert_eq!(edit.value.as_ref().unwrap().at, p(0, -5_000 + 1_000), "the Value below it");
    assert_eq!(fp.texts[0].at, p(0, 2_500), "the `${{REFERENCE}}` text keeps its place at the anchor");
}

/// The coil is worked out for a vertical one and turned to where the two points are: a horizontal span and a slanted one give a coil of the same length that starts and ends where they do.
#[test]
fn an_inductor_turns_to_where_its_two_points_are() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let cases = [(p(10_000, 10_000), p(20_000, 10_000)), (p(30_000, 30_000), p(20_000, 30_000)), (p(10_000, 40_000), p(16_000, 48_000))];
    for (i, (s, e)) in cases.iter().enumerate() {
        let min = (((e.x - s.x) as f64).powi(2) + ((e.y - s.y) as f64).powi(2)).sqrt();
        let want = (min * 3.0).round() as i64;
        b.apply(&inductor(*s, *e, want)).unwrap();
        let segs = coil(&b, &format!("REF{}", i + 1));
        assert_eq!((segs[0].0, segs.last().unwrap().1), (p(s.x - e.x, s.y - e.y), p(0, 0)), "case {i}");
        let length = path_length(&segs);
        assert!((length - want as f64).abs() < 0.015 * want as f64, "case {i}: {length} for {want}");
    }
}

/// The four ways `createMicrowaveInductor` says no, in KiCad's words, and nothing placed.
#[test]
fn a_length_the_coil_cannot_have_is_refused_with_kicads_message() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let (start, end) = (p(20_000, 20_000), p(20_000, 30_000));
    for (length, message) in [
        (9_000, "Requested length < minimum length"),
        (11_000, "Requested length too small"),
        (15_000, "Requested length can't be represented"),
        (100_000, "Requested length too large"),
    ] {
        let e = b.apply(&inductor(start, end, length)).unwrap_err();
        assert_eq!(e[0].hint.as_deref(), Some(message), "length {length}");
    }
    // the shortest that works is the same as the longest that does not
    assert!(b.apply(&inductor(start, end, 20_000)).is_ok());
    assert_eq!(b.design().drawings.as_ref().unwrap().board_parts.len(), 1, "only the one that worked");
}

#[test]
fn an_inductor_needs_two_points_a_name_and_a_track() {
    let model = ConstraintModel::default();
    let mut b = board(&model);
    let e = b.apply(&inductor(p(1_000, 1_000), p(1_000, 1_000), 5_000)).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
    let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(0, 0), end: p(0, 10_000), length_nm: 20_000, name: "  ".into(), track_width_nm: 250 })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
    let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(0, 0), end: p(0, 10_000), length_nm: 20_000, name: "L".into(), track_width_nm: 0 })).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_microwave");
    // a value with a character `FOOTPRINT_NAME_VALIDATOR` does not take
    for bad in ["L/1", "L:2", "L%", "a\"b"] {
        let e = b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(0, 0), end: p(0, 10_000), length_nm: 20_000, name: bad.into(), track_width_nm: 250 })).unwrap_err();
        assert_eq!(e[0].check, "ops_bad_microwave", "{bad}");
    }
    // the value is what the dialog's "Component value:" holds
    b.apply(&Cmd::Microwave(MicrowaveCmd::Inductor { start: p(0, 0), end: p(0, 10_000), length_nm: 20_000, name: "L22".into(), track_width_nm: 250 })).unwrap();
    assert_eq!(part(&b, "REF1").value.as_deref(), Some("L22"));
}

/// The S shape itself, in KiCad's nanometres: the corners start at the start point and end at the end point, and there are no more strands than the room allows.
#[test]
fn the_s_shape_runs_from_the_start_to_the_end() {
    let SShape::Corners(c) = build_corners_s_shape((0, 0), (0, 10_000_000), 30_000_000, 250_000) else { panic!("a coil") };
    assert_eq!((c[0], *c.last().unwrap()), ((0, 0), (0, 10_000_000)));
    // the first stub is straight down from the start by `stubs_len` (a tenth of the span), before the first arc
    assert_eq!((c[1].0, c[1].1 > 0), (0, true));
    assert_eq!(build_corners_s_shape((0, 0), (0, 10_000_000), 9_000_000, 250_000), SShape::TooShort, "length is checked against the stubs and four quarter circles");
}
