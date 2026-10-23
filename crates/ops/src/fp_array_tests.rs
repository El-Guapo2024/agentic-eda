//! Create Array in the Footprint Editor (`ARRAY_TOOL::CreateArray` with `m_isFootprintEditor`): the copies of pads, graphics and texts, and the numbers
//! KiCad's schemes give the pads of an array.

use super::*;
use eda_model::footprint::PadKind;
use eda_model::ir::{Design, LibraryPadShape};
use eda_model::ConstraintModel;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

fn pad(number: &str, x: Nm, y: Nm) -> LibraryPad {
    LibraryPad {
        id: String::new(),
        number: number.into(),
        at: Point { x, y },
        size: (1_000_000, 600_000),
        offset: Point::default(),
        shape: LibraryPadShape::Rect,
        kind: PadKind::Smd,
        drill: None,
        drill_slot: None,
        rot: 0,
        roundrect_ratio: None,
        trapezoid_delta: None,
        chamfer_ratio: None,
        chamfer_corners: eda_model::ir::ChamferCorners::default(),
        custom: None,
        layers: vec![],
        clearance_override: None,
        thermal_gap_override: None,
        thermal_spoke_width_override: None,
        zone_connection: None,
        thermal_spoke_angle_mdeg: None,
        stack: Default::default(),
        kicad: Default::default(),
    }
}

/// A board that holds the footprint `name` with these pads (ids assigned).
fn with_footprint<'a>(m: &'a ConstraintModel, pads: Vec<LibraryPad>) -> Board<'a> {
    let mut b = Board::new(design(), m, 100_000, 300_000);
    let mut fp = LibraryFootprint::new_empty("Lib:Row");
    fp.pads = pads;
    fp.assign_missing_ids();
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    b
}

fn fp<'b>(b: &'b Board<'_>) -> &'b LibraryFootprint {
    b.design().footprint_library.as_ref().unwrap().by_name("Lib:Row").unwrap()
}

/// The pads as `(number, x, y)`, sorted by position (x, then y) so a test does not depend on the order copies were pushed in.
fn pads_of(b: &Board<'_>) -> Vec<(String, Nm, Nm)> {
    let mut v: Vec<_> = fp(b).pads.iter().map(|p| (p.number.clone(), p.at.x, p.at.y)).collect();
    v.sort_by_key(|(_, x, y)| (*x, *y));
    v
}

fn id_of(b: &Board<'_>, number: &str) -> String {
    fp(b).pads.iter().find(|p| p.number == number).unwrap().id.clone()
}

fn grid(nx: i64, ny: i64, dx: Nm, dy: Nm) -> ArrayGeometry {
    ArrayGeometry::Grid { nx, ny, dx, dy, offset_x: 0, offset_y: 0, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false }
}

fn circle(count: i64, angle_deg: i64, rotate_items: bool) -> ArrayGeometry {
    ArrayGeometry::Circular { center: Point { x: 0, y: 0 }, count, angle_millideg: angle_deg * 1000, angle_offset_millideg: 0, clockwise: true, rotate_items }
}

fn array(ids: Vec<String>, geometry: ArrayGeometry, numbering: Option<PadNumbering>) -> Cmd {
    Cmd::CreateFootprintArray { footprint: "Lib:Row".into(), ids, geometry, arrange: false, numbering }
}

fn from_start(kind: NumberingType, start: &str) -> PadNumbering {
    PadNumbering { start_specified: true, two_d: false, primary: AxisSpec { kind, start: start.into(), step: 1 }, secondary: AxisSpec::default() }
}

fn names(b: &Board<'_>) -> Vec<String> {
    pads_of(b).into_iter().map(|p| p.0).collect()
}

#[test]
fn a_row_of_pads_is_numbered_from_the_start_value_along_the_points() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("1", 0, 0)]);
    let id = id_of(&b, "1");
    b.apply(&array(vec![id], grid(4, 1, 1_270_000, 1_270_000), Some(from_start(NumberingType::Numeric, "1")))).unwrap();
    assert_eq!(pads_of(&b), vec![("1".into(), 0, 0), ("2".into(), 1_270_000, 0), ("3".into(), 2_540_000, 0), ("4".into(), 3_810_000, 0)]);
    assert!(fp(&b).pads.iter().all(|p| !p.id.is_empty()), "every copy has an id");
    let mut ids: Vec<_> = fp(&b).pads.iter().map(|p| p.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4, "and no two are alike");
}

#[test]
fn the_start_value_and_the_step_decide_the_numbers() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("1", 0, 0)]);
    let id = id_of(&b, "1");
    // the odd pins of one side of a connector: 1, 3, 5, 7 -- and a start of 5
    let numbering = PadNumbering { start_specified: true, two_d: false, primary: AxisSpec { kind: NumberingType::Numeric, start: "5".into(), step: 2 }, secondary: AxisSpec::default() };
    b.apply(&array(vec![id], grid(4, 1, 1_000_000, 0), Some(numbering))).unwrap();
    assert_eq!(names(&b), ["5", "7", "9", "11"]);
}

#[test]
fn use_first_free_number_skips_the_pads_that_are_not_part_of_the_array() {
    let m = ConstraintModel::default();
    // pads 1 and 3 stay; the pad being arrayed is "9", which gives its number up
    let mut b = with_footprint(&m, vec![pad("1", -5_000_000, 0), pad("3", -4_000_000, 0), pad("9", 0, 0)]);
    let id = id_of(&b, "9");
    b.apply(&array(vec![id], grid(3, 1, 1_000_000, 0), Some(PadNumbering::default()))).unwrap();
    let mut by_x = pads_of(&b);
    by_x.retain(|p| p.1 >= 0);
    assert_eq!(by_x.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["2", "4", "5"], "1 and 3 are taken by pads that stay; the selection's own 9 is free, and unused");
    assert_eq!(names(&b).iter().filter(|n| *n == "1" || *n == "3").count(), 2, "the pads that stay keep their numbers");
}

#[test]
fn a_selected_pad_gives_up_its_number_so_the_first_free_run_can_reuse_it() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("1", 0, 0), pad("2", 5_000_000, 0)]);
    let id = id_of(&b, "1");
    b.apply(&array(vec![id], grid(3, 1, 1_000_000, 0), Some(PadNumbering::default()))).unwrap();
    let row: Vec<_> = pads_of(&b).into_iter().filter(|p| p.1 < 5_000_000).collect();
    assert_eq!(row.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["1", "3", "4"], "1 is free again (its pad is in the array), 2 belongs to the pad that stays");
}

#[test]
fn without_a_numbering_the_copies_keep_the_number_of_their_source() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("A7", 0, 0)]);
    let id = id_of(&b, "A7");
    b.apply(&array(vec![id], grid(3, 1, 1_000_000, 0), None)).unwrap();
    assert_eq!(names(&b), ["A7", "A7", "A7"], "\"Renumber pads\" off: `FOOTPRINT::DuplicateItem` copies the pad as it is");
}

#[test]
fn a_coordinate_scheme_names_a_point_by_its_column_and_its_row() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("X", 0, 0)]);
    let id = id_of(&b, "X");
    let numbering = PadNumbering {
        start_specified: true,
        two_d: true,
        primary: AxisSpec { kind: NumberingType::AlphaNoIosqxz, start: "A".into(), step: 1 },
        secondary: AxisSpec { kind: NumberingType::Numeric, start: "1".into(), step: 1 },
    };
    b.apply(&array(vec![id], grid(3, 2, 1_000_000, 1_000_000), Some(numbering))).unwrap();
    assert_eq!(
        pads_of(&b),
        vec![("A1".into(), 0, 0), ("A2".into(), 0, 1_000_000), ("B1".into(), 1_000_000, 0), ("B2".into(), 1_000_000, 1_000_000), ("C1".into(), 2_000_000, 0), ("C2".into(), 2_000_000, 1_000_000)]
    );
}

#[test]
fn the_serpentine_and_the_walking_direction_change_which_point_gets_which_number() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("X", 0, 0)]);
    let id = id_of(&b, "X");
    let mut g = grid(3, 2, 1_000_000, 1_000_000);
    if let ArrayGeometry::Grid { reverse_alternate, .. } = &mut g {
        *reverse_alternate = true; // "Reverse numbering on alternate rows/columns"
    }
    b.apply(&array(vec![id], g, Some(from_start(NumberingType::Numeric, "1")))).unwrap();
    // the first row 1 2 3 runs right, the second runs back: 4 under the 3
    assert_eq!(pads_of(&b), vec![("1".into(), 0, 0), ("6".into(), 0, 1_000_000), ("2".into(), 1_000_000, 0), ("5".into(), 1_000_000, 1_000_000), ("3".into(), 2_000_000, 0), ("4".into(), 2_000_000, 1_000_000)]);

    let mut b = with_footprint(&m, vec![pad("X", 0, 0)]);
    let id = id_of(&b, "X");
    let mut g = grid(3, 2, 1_000_000, 1_000_000);
    if let ArrayGeometry::Grid { horizontal_then_vertical, .. } = &mut g {
        *horizontal_then_vertical = false; // "Vertical, then horizontal"
    }
    b.apply(&array(vec![id], g, Some(from_start(NumberingType::Numeric, "1")))).unwrap();
    // a column before the next column: 1 2 down the first
    assert_eq!(pads_of(&b), vec![("1".into(), 0, 0), ("2".into(), 0, 1_000_000), ("3".into(), 1_000_000, 0), ("4".into(), 1_000_000, 1_000_000), ("5".into(), 2_000_000, 0), ("6".into(), 2_000_000, 1_000_000)]);
}

#[test]
fn several_selected_pads_are_numbered_point_by_point_in_the_order_of_the_selection() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("a", 0, 0), pad("b", 0, 700_000)]);
    let (a, c) = (id_of(&b, "a"), id_of(&b, "b"));
    // selected in the order b, a: the number goes to b first at every point
    b.apply(&array(vec![c, a], grid(3, 1, 2_000_000, 0), Some(from_start(NumberingType::Numeric, "1")))).unwrap();
    assert_eq!(
        pads_of(&b),
        vec![("2".into(), 0, 0), ("1".into(), 0, 700_000), ("4".into(), 2_000_000, 0), ("3".into(), 2_000_000, 700_000), ("6".into(), 4_000_000, 0), ("5".into(), 4_000_000, 700_000)]
    );
}

#[test]
fn a_pad_that_cannot_have_a_number_keeps_none_and_takes_none() {
    let m = ConstraintModel::default();
    let mut hole = pad("", 0, 700_000);
    hole.kind = PadKind::NonPlatedHole;
    hole.drill = Some(800_000);
    let mut b = with_footprint(&m, vec![pad("x", 0, 0), hole]);
    let (p, h) = (id_of(&b, "x"), id_of(&b, ""));
    b.apply(&array(vec![p, h], grid(2, 1, 3_000_000, 0), Some(from_start(NumberingType::Numeric, "1")))).unwrap();
    let mut got = pads_of(&b);
    got.sort();
    assert_eq!(got, vec![("".into(), 0, 700_000), ("".into(), 3_000_000, 700_000), ("1".into(), 0, 0), ("2".into(), 3_000_000, 0)], "the mounting hole is copied, not numbered, and does not use up a number");
}

#[test]
fn a_circular_array_numbers_around_the_circle_and_turns_the_pads_when_it_rotates_its_items() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("x", 5_000_000, 0)]);
    let id = id_of(&b, "x");
    let numbering = PadNumbering { start_specified: true, two_d: false, primary: AxisSpec { kind: NumberingType::AlphaFull, start: "A".into(), step: 1 }, secondary: AxisSpec::default() };
    b.apply(&array(vec![id], circle(4, 90, true), Some(numbering))).unwrap();
    let mut pads: Vec<_> = fp(&b).pads.iter().map(|p| (p.number.clone(), p.at.x, p.at.y, p.rot)).collect();
    pads.sort();
    // clockwise on the screen from (5 mm, 0): then (0, 5 mm) below it (y grows downward), (-5 mm, 0), (0, -5 mm); each pad turned with its point (KiCad's angle runs the other way)
    assert_eq!(pads, vec![("A".into(), 5_000_000, 0, 0), ("B".into(), 0, 5_000_000, 270_000), ("C".into(), -5_000_000, 0, 180_000), ("D".into(), 0, -5_000_000, 90_000)]);
}

#[test]
fn a_circular_array_that_does_not_rotate_its_items_leaves_their_orientation_alone() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("x", 5_000_000, 0)]);
    let id = id_of(&b, "x");
    b.apply(&array(vec![id], circle(3, 120, false), Some(from_start(NumberingType::Hex, "A")))).unwrap();
    assert!(fp(&b).pads.iter().all(|p| p.rot == 0));
    let mut numbers = names(&b);
    numbers.sort();
    assert_eq!(numbers, ["A", "B", "C"], "hexadecimal A, B, C");
}

#[test]
fn graphics_and_texts_are_copied_and_moved_with_their_pads() {
    use eda_model::ir::{Text, TextJustify};
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("1", 0, 0)]);
    b.apply(&Cmd::AddFootprintGraphic { footprint: "Lib:Row".into(), shape: Shape::Segment { id: String::new(), layer: "F.SilkS".into(), stroke_width: 150_000, filled: false, start: Point { x: -500_000, y: -800_000 }, end: Point { x: 500_000, y: -800_000 } } }).unwrap();
    b.apply(&Cmd::AddFootprintText {
        footprint: "Lib:Row".into(),
        text: Text { id: String::new(), content: "T".into(), at: Point { x: 0, y: 1_500_000 }, angle: 0, layer: "F.SilkS".into(), size_nm: 1_000_000, stroke_width: 150_000, justify: TextJustify::Center, mirror: false },
    })
    .unwrap();
    let ids: Vec<String> = {
        let f = fp(&b);
        vec![f.pads[0].id.clone(), f.graphics[0].id().to_string(), f.texts[0].id.clone()]
    };
    b.apply(&array(ids, grid(3, 1, 2_540_000, 0), Some(from_start(NumberingType::Numeric, "1")))).unwrap();
    let f = fp(&b);
    assert_eq!((f.pads.len(), f.graphics.len(), f.texts.len()), (3, 3, 3));
    let mut starts: Vec<Nm> = f.graphics.iter().map(|g| g.points()[0].x).collect();
    starts.sort();
    assert_eq!(starts, vec![-500_000, 2_040_000, 4_580_000], "the silkscreen line moved with each pad");
    let mut xs: Vec<Nm> = f.texts.iter().map(|t| t.at.x).collect();
    xs.sort();
    assert_eq!(xs, vec![0, 2_540_000, 5_080_000]);
    let mut all: Vec<String> = f.graphics.iter().map(|g| g.id().to_string()).chain(f.texts.iter().map(|t| t.id.clone())).chain(f.pads.iter().map(|p| p.id.clone())).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 9, "nine items, nine ids");
}

#[test]
fn arrange_selection_puts_the_items_on_the_points_and_numbers_nothing() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("1", 0, 0), pad("2", 400_000, 9_000_000), pad("3", 800_000, -9_000_000)]);
    let ids = vec![id_of(&b, "1"), id_of(&b, "2"), id_of(&b, "3")];
    b.apply(&Cmd::CreateFootprintArray { footprint: "Lib:Row".into(), ids, geometry: grid(3, 1, 2_000_000, 0), arrange: true, numbering: Some(from_start(NumberingType::Numeric, "7")) }).unwrap();
    assert_eq!(pads_of(&b), vec![("1".into(), 0, 0), ("2".into(), 2_000_000, 0), ("3".into(), 4_000_000, 0)], "all three are in a row now, with the numbers they had");
    assert_eq!(fp(&b).pads.len(), 3, "nothing is made");
}

#[test]
fn a_bad_start_a_zero_spacing_and_unknown_things_are_refused_and_change_nothing() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad("1", 0, 0)]);
    let id = id_of(&b, "1");
    let before = fp(&b).clone();

    let e = b.apply(&array(vec![id.clone()], grid(2, 1, 1_000_000, 0), Some(from_start(NumberingType::Hex, "G")))).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_array");
    assert!(e[0].hint.as_deref().unwrap_or("").contains("Could not determine numbering start from 'G'") && e[0].hint.as_deref().unwrap_or("").contains("0123456789ABCDEF"), "{:?}", e[0].hint);

    let e = b.apply(&array(vec![id.clone()], grid(3, 1, 0, 0), None)).unwrap_err();
    assert!(e[0].hint.as_deref().unwrap_or("").contains("horizontal delta of zero with 3 objects"), "{:?}", e[0].hint);
    let e = b.apply(&array(vec![id.clone()], circle(3, 0, false), None)).unwrap_err();
    assert!(e[0].hint.as_deref().unwrap_or("").contains("angular delta of zero with 3 objects"), "{:?}", e[0].hint);

    assert_eq!(b.apply(&array(vec![], grid(2, 1, 1_000_000, 0), None)).unwrap_err()[0].check, "ops_bad_array");
    assert_eq!(b.apply(&array(vec!["nope".into()], grid(2, 1, 1_000_000, 0), None)).unwrap_err()[0].check, "ops_unknown_array");
    let missing = Cmd::CreateFootprintArray { footprint: "Lib:Nope".into(), ids: vec![id], geometry: grid(2, 1, 1_000_000, 0), arrange: false, numbering: None };
    assert_eq!(b.apply(&missing).unwrap_err()[0].check, "ops_unknown_footprint");
    assert_eq!(*fp(&b), before, "nothing changed");
}

#[test]
fn the_verb_runs_in_the_footprint_editors_undo_domain_and_reads_from_json() {
    let cmd = Cmd::CreateFootprintArray { footprint: "F".into(), ids: vec!["a".into()], geometry: grid(2, 1, 1_000_000, 0), arrange: false, numbering: None };
    assert_eq!(cmd.domain(), Domain::FootprintEditor);
    let json = serde_json::json!({
        "op": "create_footprint_array",
        "footprint": "F",
        "ids": ["a"],
        "geometry": { "kind": "grid", "nx": 2, "ny": 1, "dx": 1_000_000, "dy": 0 },
        "numbering": { "start_specified": true, "two_d": true, "primary": { "kind": "alpha_full", "start": "A" }, "secondary": {} }
    });
    match serde_json::from_value::<Cmd>(json).unwrap() {
        Cmd::CreateFootprintArray { arrange, numbering: Some(n), .. } => {
            assert!(!arrange);
            assert!(n.start_specified && n.two_d);
            assert_eq!((n.primary.kind, n.primary.start.as_str(), n.primary.step), (NumberingType::AlphaFull, "A", 1));
            assert_eq!((n.secondary.kind, n.secondary.start.as_str()), (NumberingType::Numeric, "1"), "an axis left out is numerals from 1");
        }
        other => panic!("{other:?}"),
    }
}
