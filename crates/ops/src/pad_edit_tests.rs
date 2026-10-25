//! Edit Pad as Graphic Shapes and Finish Pad Edit in the Footprint Editor (`PAD_TOOL::EditPad`, `explodePad`, `PAD::Recombine`).

use super::*;
use eda_model::footprint::{CustomZoneMode, PadAnchor};
use eda_model::ir::{Design, LibraryPadShape};
use eda_model::ConstraintModel;
use serde_json::json;
use eda_model::units::mm;

fn design() -> Design {
    serde_json::from_str(r#"{"schema":1,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap()
}

/// A pad made the way the editor's JSON makes one, so a field added later takes its default.
fn pad(v: serde_json::Value) -> LibraryPad {
    serde_json::from_value(v).unwrap()
}

fn line(layer: &str, a: (Nm, Nm), b: (Nm, Nm), width: Nm) -> Shape {
    Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: width, filled: false, start: Point { x: a.0, y: a.1 }, end: Point { x: b.0, y: b.1 } }
}

fn prim(shape: Shape, proxy: bool) -> eda_model::footprint::PadPrimitive {
    eda_model::footprint::PadPrimitive { shape, proxy }
}

fn with_footprint<'a>(m: &'a ConstraintModel, pads: Vec<LibraryPad>, graphics: Vec<Shape>) -> Board<'a> {
    let mut b = Board::new(design(), m, mm(0.1), mm(0.3));
    let mut fp = LibraryFootprint::new_empty("Lib:Row");
    fp.pads = pads;
    fp.graphics = graphics;
    fp.assign_missing_ids();
    b.apply(&Cmd::PutLibraryFootprint { footprint: fp, overwrite: false }).unwrap();
    b
}

fn fp<'b>(b: &'b Board<'_>) -> &'b LibraryFootprint {
    b.design().footprint_library.as_ref().unwrap().by_name("Lib:Row").unwrap()
}

fn custom_pad() -> LibraryPad {
    let mut p = pad(json!({ "number": "1", "at": {"x": 1_000_000, "y": 2_000_000}, "size": [800_000, 800_000], "shape": "custom", "kind": "smd", "rot": 90000 }));
    p.custom = Some(PadCustom {
        anchor: PadAnchor::Circle,
        zone: CustomZoneMode::ConvexHull,
        primitives: vec![prim(line("", (Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO), mm(0.2)), false), prim(line("", (Nm::ZERO, Nm::ZERO), (mm(3.0), Nm::ZERO), mm(0.5)), true)],
    });
    p
}

use eda_model::footprint::PadCustom;

fn explode(b: &mut Board<'_>) -> Result<(), Vec<CheckResult>> {
    let id = fp(b).pads[0].id.clone();
    b.apply(&Cmd::ExplodePad { footprint: "Lib:Row".into(), id })
}

fn recombine(b: &mut Board<'_>) -> Result<(), Vec<CheckResult>> {
    b.apply(&Cmd::RecombinePad { footprint: "Lib:Row".into() })
}

#[test]
fn exploding_a_custom_pad_frees_its_shapes_on_the_pads_layer_turned_and_moved_with_it() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![custom_pad()], vec![]);
    explode(&mut b).unwrap();
    let f = fp(&b);
    // The pad is its anchor now, with no shapes; the footprint is in pad edit mode.
    assert_eq!((f.pads[0].shape, f.pads[0].custom.is_none()), (LibraryPadShape::Circle, true));
    assert_eq!(f.pad_edit.as_ref().map(|m| m.pad.as_str()), Some(f.pads[0].id.as_str()));
    // The line (0,0)-(1000,0) of a pad turned 90 degrees clockwise at (1000,2000) runs from (1000,2000) down to (1000,3000), on F.Cu.
    assert_eq!(f.graphics.len(), 1, "{:?}", f.graphics);
    assert!(matches!(&f.graphics[0], Shape::Segment { layer, stroke_width: v1, start: Point { x: v2, y: v3 }, end: Point { x: v4, y: v5 }, .. } if *v1 == mm(0.2) && *v2 == mm(1.0) && *v3 == mm(2.0) && *v4 == mm(1.0) && *v5 == mm(3.0) && layer == "F.Cu"), "{:?}", f.graphics[0]);
    assert!(!f.graphics[0].id().is_empty(), "the graphic has an id");
    // The spoke template is not copper: set aside, as wide as a thermal spoke, in the footprint's frame; the zone option is kept.
    let mode = f.pad_edit.as_ref().unwrap();
    assert_eq!(mode.zone, CustomZoneMode::ConvexHull);
    assert_eq!(mode.proxies.len(), 1);
    assert!(matches!(&mode.proxies[0].shape, Shape::Segment { layer, stroke_width: v1, start: Point { x: v2, y: v3 }, end: Point { x: v4, y: v5 }, .. } if *v1 == mm(0.5) && *v2 == mm(1.0) && *v3 == mm(2.0) && *v4 == mm(1.0) && *v5 == mm(5.0) && layer == "F.Cu"), "{:?}", mode.proxies[0].shape);
}

#[test]
fn a_rectangle_anchor_leaves_a_rectangle_and_a_pad_on_the_back_explodes_to_the_back() {
    let m = ConstraintModel::default();
    let mut p = custom_pad();
    p.custom.as_mut().unwrap().anchor = PadAnchor::Rect;
    p.layers = vec!["B.Cu".into(), "B.Paste".into(), "B.Mask".into()];
    let mut b = with_footprint(&m, vec![p], vec![]);
    explode(&mut b).unwrap();
    assert_eq!(fp(&b).pads[0].shape, LibraryPadShape::Rect);
    assert_eq!(fp(&b).graphics[0].layer(), "B.Cu");
}

#[test]
fn a_pad_that_is_not_custom_is_only_flagged() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![pad(json!({ "number": "1", "at": {"x": 0, "y": 0}, "size": [1_000_000, 600_000], "shape": "rect", "kind": "smd" }))], vec![]);
    explode(&mut b).unwrap();
    assert!(fp(&b).graphics.is_empty());
    assert!(fp(&b).pad_edit.is_some());
}

#[test]
fn exploding_and_recombining_a_custom_pad_gives_the_pad_back_as_it_was() {
    let m = ConstraintModel::default();
    let original = custom_pad();
    let mut b = with_footprint(&m, vec![original.clone()], vec![]);
    explode(&mut b).unwrap();
    recombine(&mut b).unwrap();
    let f = fp(&b);
    assert!(f.pad_edit.is_none() && f.graphics.is_empty(), "{:?}", f.graphics);
    let mut expected = original;
    expected.id = f.pads[0].id.clone();
    assert_eq!(f.pads[0], expected);
}

#[test]
fn recombining_takes_in_the_graphics_that_touch_the_pad_and_leaves_the_others() {
    let m = ConstraintModel::default();
    let circle = pad(json!({ "number": "1", "at": {"x": 0, "y": 0}, "size": [1_000_000, 1_000_000], "shape": "circle", "kind": "smd" }));
    let graphics = vec![
        line("F.Cu", (Nm::ZERO, Nm::ZERO), (mm(2.0), Nm::ZERO), mm(0.3)),          // starts in the pad
        line("F.Cu", (mm(5.0), mm(5.0)), (mm(6.0), mm(5.0)), mm(0.3)), // far away
        line("F.SilkS", (Nm::ZERO, Nm::ZERO), (mm(2.0), Nm::ZERO), mm(0.3)),       // touches, but on another layer
    ];
    let mut b = with_footprint(&m, vec![circle], graphics);
    explode(&mut b).unwrap();
    recombine(&mut b).unwrap();
    let f = fp(&b);
    assert_eq!(f.pads[0].shape, LibraryPadShape::Custom);
    let custom = f.pads[0].custom.as_ref().unwrap();
    assert_eq!(custom.anchor, PadAnchor::Circle);
    assert_eq!(custom.primitives.len(), 1);
    assert!(matches!(&custom.primitives[0].shape, Shape::Segment { layer, stroke_width: v1, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: v2, y: Nm::ZERO }, .. } if *v1 == mm(0.3) && *v2 == mm(2.0) && layer.is_empty()));
    // The far line and the silkscreen line stay graphics of the footprint.
    assert_eq!(f.graphics.len(), 2);
    assert!(f.graphics.iter().any(|g| g.layer() == "F.SilkS") && f.graphics.iter().any(|g| g.layer() == "F.Cu"));
}

/// `PAD::Recombine`'s `findNext` takes the pad as it has grown, so a chain of graphics is merged whole, in either list order.
#[test]
fn recombining_follows_a_chain_of_shapes_that_touch_through_each_other() {
    let m = ConstraintModel::default();
    let circle = || pad(json!({ "number": "1", "at": {"x": 0, "y": 0}, "size": [500_000, 500_000], "shape": "circle", "kind": "smd" }));
    let a = line("F.Cu", (Nm::ZERO, Nm::ZERO), (mm(2.0), Nm::ZERO), mm(0.2));
    let b = line("F.Cu", (mm(2.0), Nm::ZERO), (mm(2.0), mm(2.0)), mm(0.2));
    for graphics in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
        let mut bd = with_footprint(&m, vec![circle()], graphics);
        explode(&mut bd).unwrap();
        recombine(&mut bd).unwrap();
        let f = fp(&bd);
        let custom = f.pads[0].custom.as_ref().unwrap();
        assert_eq!(custom.primitives.len(), 2, "{:?}", custom.primitives);
        assert!(f.graphics.is_empty(), "{:?}", f.graphics);
        // The segment that touches the anchor is merged first, the one that touches only it second (`AddPrimitive`'s order).
        assert!(matches!(&custom.primitives[0].shape, Shape::Segment { end: Point { x: v1, y: Nm::ZERO }, .. } if *v1 == mm(2.0)), "{:?}", custom.primitives[0].shape);
    }
}

/// The finding's repro: an L-shaped custom pad (anchor, then a segment off it, then a segment off that) explodes and
/// recombines to itself, with no loose copper left in the footprint.
#[test]
fn recombining_merges_shapes_that_touch_the_pad_only_through_another_merged_shape() {
    let m = ConstraintModel::default();
    let mut p = pad(json!({ "number": "1", "at": {"x": 0, "y": 0}, "size": [800_000, 800_000], "shape": "custom", "kind": "smd" }));
    p.custom = Some(PadCustom {
        anchor: PadAnchor::Circle,
        zone: CustomZoneMode::Outline,
        primitives: vec![prim(line("", (Nm::ZERO, Nm::ZERO), (mm(1.0), Nm::ZERO), mm(0.2)), false), prim(line("", (mm(1.0), Nm::ZERO), (mm(1.0), mm(3.0)), mm(0.2)), false)],
    });
    let mut b = with_footprint(&m, vec![p.clone()], vec![]);
    explode(&mut b).unwrap();
    recombine(&mut b).unwrap();
    let f = fp(&b);
    assert!(f.graphics.is_empty(), "{:?}", f.graphics);
    assert_eq!(f.pads[0].custom.as_ref().unwrap().primitives.len(), 2);
    let mut expected = p;
    expected.id = f.pads[0].id.clone();
    assert_eq!(f.pads[0], expected);
}

#[test]
fn a_rectangle_pad_is_its_own_anchor_and_a_rounded_one_becomes_a_polygon_round_a_circular_anchor() {
    let m = ConstraintModel::default();
    let touching = vec![line("F.Cu", (Nm::ZERO, Nm::ZERO), (mm(2.0), Nm::ZERO), mm(0.1))];

    let rect = pad(json!({ "number": "1", "at": {"x": 0, "y": 0}, "size": [1_000_000, 600_000], "shape": "rect", "kind": "smd" }));
    let mut b = with_footprint(&m, vec![rect], touching.clone());
    explode(&mut b).unwrap();
    recombine(&mut b).unwrap();
    let c = fp(&b).pads[0].custom.clone().unwrap();
    assert_eq!((fp(&b).pads[0].shape, c.anchor, c.primitives.len(), fp(&b).pads[0].size), (LibraryPadShape::Custom, PadAnchor::Rect, 1, (mm(1.0), mm(0.6))));

    let rounded = pad(json!({ "number": "1", "at": {"x": 0, "y": 0}, "size": [1_000_000, 600_000], "shape": "round_rect", "roundrect_ratio": 0.25, "kind": "smd" }));
    let mut b = with_footprint(&m, vec![rounded], touching);
    explode(&mut b).unwrap();
    recombine(&mut b).unwrap();
    let p = &fp(&b).pads[0];
    let c = p.custom.clone().unwrap();
    assert_eq!((p.shape, c.anchor, p.size, p.roundrect_ratio), (LibraryPadShape::Custom, PadAnchor::Circle, (mm(0.6), mm(0.6)), None));
    // The old outline first, as a filled polygon of no width, then the line.
    assert_eq!(c.primitives.len(), 2);
    assert!(matches!(&c.primitives[0].shape, Shape::Polygon { filled: true, stroke_width: Nm::ZERO, pts, .. } if pts.len() >= 8));
    assert!(matches!(&c.primitives[1].shape, Shape::Segment { .. }));
}

#[test]
fn a_shape_edited_while_the_pad_was_exploded_comes_back_where_it_was_drawn() {
    let m = ConstraintModel::default();
    let mut p = custom_pad();
    p.rot = 0;
    let mut b = with_footprint(&m, vec![p], vec![]);
    explode(&mut b).unwrap();
    // Move the freed line 400 um to the right: it is the pad's primitive moved by 400 when the pad is recombined.
    let gid = fp(&b).graphics[0].id().to_string();
    b.apply(&Cmd::MoveFootprintGraphic { footprint: "Lib:Row".into(), id: gid, dx: mm(0.4), dy: Nm::ZERO }).unwrap();
    recombine(&mut b).unwrap();
    let custom = fp(&b).pads[0].custom.clone().unwrap();
    let real: Vec<_> = custom.primitives.iter().filter(|p| !p.proxy).collect();
    assert!(matches!(&real[0].shape, Shape::Segment { start: Point { x: v1, y: Nm::ZERO }, end: Point { x: v2, y: Nm::ZERO }, .. } if *v1 == mm(0.4) && *v2 == mm(1.4)));
}

#[test]
fn the_edit_mode_is_one_pad_at_a_time_and_a_recombine_needs_one() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![custom_pad()], vec![]);
    assert!(recombine(&mut b).is_err(), "nothing is being edited");
    explode(&mut b).unwrap();
    assert!(explode(&mut b).is_err(), "a pad is being edited already");
    let id = "no-such-pad".to_string();
    assert!(b.apply(&Cmd::ExplodePad { footprint: "Lib:Row".into(), id }).is_err());
    recombine(&mut b).unwrap();
    assert!(fp(&b).pad_edit.is_none());
}

#[test]
fn deleting_the_pad_that_is_being_edited_ends_the_mode_and_publishing_the_footprint_recombines_it() {
    let m = ConstraintModel::default();
    let mut b = with_footprint(&m, vec![custom_pad()], vec![]);
    explode(&mut b).unwrap();
    let id = fp(&b).pads[0].id.clone();
    b.apply(&Cmd::DeletePad { footprint: "Lib:Row".into(), id }).unwrap();
    assert!(fp(&b).pad_edit.is_none());

    // `SaveFootprint` recombines first: updating the footprint on the board does it.
    let mut b = with_footprint(&m, vec![custom_pad()], vec![]);
    explode(&mut b).unwrap();
    b.apply(&Cmd::UpdateFootprintOnBoard { name: "Lib:Row".into() }).unwrap();
    assert!(fp(&b).pad_edit.is_none() && fp(&b).graphics.is_empty());
    assert_eq!(fp(&b).pads[0].shape, LibraryPadShape::Custom);
}

#[test]
fn a_rectangle_turned_by_a_pad_that_is_not_square_to_the_footprint_becomes_a_polygon() {
    let m = ConstraintModel::default();
    let mut p = custom_pad();
    p.rot = 45_000;
    p.custom.as_mut().unwrap().primitives = vec![prim(Shape::Rect { id: String::new(), layer: String::new(), stroke_width: Nm::ZERO, filled: true, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(1.0), y: mm(0.5) } }, false)];
    let mut b = with_footprint(&m, vec![p], vec![]);
    explode(&mut b).unwrap();
    assert!(matches!(&fp(&b).graphics[0], Shape::Polygon { pts, filled: true, .. } if pts.len() == 4), "{:?}", fp(&b).graphics[0]);
    recombine(&mut b).unwrap();
    // Back in the pad it is the polygon it became: the four corners of the rectangle.
    let c = fp(&b).pads[0].custom.clone().unwrap();
    assert!(matches!(&c.primitives[0].shape, Shape::Polygon { pts, .. } if pts.len() == 4));
}
