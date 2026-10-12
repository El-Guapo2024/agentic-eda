//! Import Graphics on the board: the layer, the scale, the origin and the line width the shapes are made with, the group, the fix for
//! discontinuities, and an outline that replaces the board's. The files are the small DXFs of `crates/kicad/tests/fixtures`.

use super::pcb_transform_tests::{board, design, drawings, model, p};
use super::*;
use eda_model::ir::Group;

fn fixture(name: &str) -> String {
    let path = format!("{}/../kicad/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn req(text: String, layer: &str) -> ImportGraphics {
    ImportGraphics { format: GraphicsFormat::Dxf, text, layer: layer.into(), dxf_units: String::new(), scale: 1.0, offset_x_nm: 0, offset_y_nm: 0, line_width_um: 200, group: false, fix_tolerance_um: None, replace_outline: false }
}

/// The shapes an import added to the fixture board (it has three of its own).
fn added(b: &Board<'_>) -> Vec<Shape> {
    drawings(b).shapes.iter().filter(|s| !["shp_seg", "shp_rect", "shp_arc"].contains(&s.id())).cloned().collect()
}

fn ends(s: &Shape) -> (Point, Point) {
    match s {
        Shape::Segment { start, end, .. } | Shape::Arc { start, end, .. } => (*start, *end),
        other => panic!("not a segment or an arc: {other:?}"),
    }
}

#[test]
fn a_dxf_outline_becomes_segments_and_arcs_on_the_layer_the_dialog_chose() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportGraphics(req(fixture("board_outline_lines_arcs.dxf"), "Edge.Cuts"))).unwrap();
    let new = added(&b);
    assert_eq!(new.len(), 8, "{new:?}");
    assert!(new.iter().all(|s| s.layer() == "Edge.Cuts" && s.stroke_width() == 200 && !s.is_filled()), "{new:?}");
    assert_eq!(new.iter().filter(|s| matches!(s, Shape::Segment { .. })).count(), 4);
    assert_eq!(new.iter().filter(|s| matches!(s, Shape::Arc { .. })).count(), 4);
    // The DXF has its Y axis up and the board's points down: the 40 x 30 board hangs above the origin, x 0 to 40 mm, y -30 to 0 mm.
    let xs: Vec<Um> = new.iter().flat_map(|s| s.points().into_iter().map(|p| p.x)).collect();
    let ys: Vec<Um> = new.iter().flat_map(|s| s.points().into_iter().map(|p| p.y)).collect();
    assert_eq!((xs.iter().min().copied(), xs.iter().max().copied()), (Some(0), Some(40_000)));
    assert_eq!((ys.iter().min().copied(), ys.iter().max().copied()), (Some(-30_000), Some(0)));
    // An arc's middle is on its circle: the corner at the origin side, radius 3 mm.
    let Shape::Arc { start, mid, end, .. } = new.iter().find(|s| matches!(s, Shape::Arc { .. })).unwrap() else { unreachable!() };
    let (cx, cy) = circle_through(*start, *mid, *end);
    assert!((((start.x as f64 - cx).hypot(start.y as f64 - cy)) - 3_000.0).abs() < 2.0, "radius of the corner: {start:?} {mid:?} {end:?}");
    // The outline makes one closed loop, as kicad-cli will see it.
    let outline = eda_drc::outline::board_outline_of(&new, false);
    assert!(outline.valid && outline.polys.outline_count() == 1, "{:?}", outline.errors);
}

/// The centre of the circle through three points.
fn circle_through(a: Point, b: Point, c: Point) -> (f64, f64) {
    let (ax, ay, bx, by, cx, cy) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64, c.x as f64, c.y as f64);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    let ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
    (ux, uy)
}

#[test]
fn the_import_scale_multiplies_the_coordinates_and_the_default_line_width() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut r = req(fixture("no_units.dxf"), "Dwgs.User");
    r.scale = 2.0;
    r.dxf_units = "mm".into();
    b.apply(&Cmd::ImportGraphics(r)).unwrap();
    let new = added(&b);
    assert_eq!(new.len(), 4);
    // The 2 x 1 rectangle, doubled, with its lines doubled (0.2 mm default width -> 0.4 mm).
    let xs: Vec<Um> = new.iter().flat_map(|s| s.points().into_iter().map(|p| p.x)).collect();
    assert_eq!(xs.iter().max().copied(), Some(4_000));
    assert!(new.iter().all(|s| s.stroke_width() == 400 && s.layer() == "Dwgs.User"), "{new:?}");
}

#[test]
fn the_origin_moves_the_whole_picture_after_the_scale() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut r = req(fixture("no_units.dxf"), "Cmts.User");
    r.scale = 2.0;
    r.dxf_units = "mm".into();
    r.offset_x_nm = 10_000_000; // 10 mm
    r.offset_y_nm = -5_000_000;
    b.apply(&Cmd::ImportGraphics(req_copy(&r))).unwrap();
    let new = added(&b);
    let xs: Vec<Um> = new.iter().flat_map(|s| s.points().into_iter().map(|p| p.x)).collect();
    let ys: Vec<Um> = new.iter().flat_map(|s| s.points().into_iter().map(|p| p.y)).collect();
    assert_eq!((xs.iter().min().copied(), xs.iter().max().copied()), (Some(10_000), Some(14_000)));
    // The picture's y runs 0 to -1 mm in the board's frame; scaled 0 to -2 mm, then 5 mm up.
    assert_eq!((ys.iter().min().copied(), ys.iter().max().copied()), (Some(-7_000), Some(-5_000)));
}

fn req_copy(r: &ImportGraphics) -> ImportGraphics {
    r.clone()
}

#[test]
fn the_units_of_the_file_win_over_the_dialogs_and_a_file_with_none_takes_the_dialogs() {
    let width = |name: &str, units: &str| -> Um {
        let m = model(&["F.Cu", "B.Cu"]);
        let mut b = board(&m);
        let mut r = req(fixture(name), "Dwgs.User");
        r.dxf_units = units.into();
        b.apply(&Cmd::ImportGraphics(r)).unwrap();
        let xs: Vec<Um> = added(&b).iter().flat_map(|s| s.points().into_iter().map(|p| p.x)).collect();
        xs.iter().max().unwrap() - xs.iter().min().unwrap()
    };
    assert_eq!(width("inches.dxf", "mm"), 50_800, "$INSUNITS 1");
    assert_eq!(width("no_units.dxf", "inch"), 50_800);
    assert_eq!(width("no_units.dxf", "mm"), 2_000);
    assert_eq!(width("no_units.dxf", ""), 2_000);
}

#[test]
fn group_imported_items_makes_one_group_of_everything_it_added() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut r = req(fixture("holes.dxf"), "Dwgs.User");
    b.apply(&Cmd::ImportGraphics(req_copy(&r))).unwrap();
    assert!(drawings(&b).groups.is_empty(), "no group unless asked");
    r.group = true;
    b.apply(&Cmd::ImportGraphics(r)).unwrap();
    let groups: &Vec<Group> = &drawings(&b).groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].member_ids.len(), 3);
    let in_group: Vec<&Shape> = drawings(&b).shapes.iter().filter(|s| groups[0].member_ids.iter().any(|m| m == s.id())).collect();
    assert_eq!(in_group.len(), 3);
}

#[test]
fn one_item_is_not_grouped() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n1\n20\n1\n40\n2\n0\nENDSEC\n0\nEOF\n".to_string();
    let mut r = req(dxf, "Dwgs.User");
    r.group = true;
    b.apply(&Cmd::ImportGraphics(r)).unwrap();
    assert!(drawings(&b).groups.is_empty());
}

#[test]
fn a_circle_keeps_its_centre_and_a_point_on_it_for_the_radius() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportGraphics(req(fixture("holes.dxf"), "Edge.Cuts"))).unwrap();
    let mut circles: Vec<(Point, Point)> = added(&b)
        .iter()
        .filter_map(|s| match s {
            Shape::Circle { center, end, .. } => Some((*center, *end)),
            _ => None,
        })
        .collect();
    circles.sort_by_key(|c| c.0.x);
    assert_eq!(circles, vec![(p(5_000, -5_000), p(6_600, -5_000)), (p(20_000, -15_000), p(24_000, -15_000)), (p(35_000, -25_000), p(36_600, -25_000))]);
}

#[test]
fn a_gap_in_the_outline_is_closed_when_fix_discontinuities_has_the_tolerance() {
    // A 10 x 10 mm square whose corners miss by 0.3 mm.
    let dxf = "0\nSECTION\n2\nENTITIES\n".to_string()
        + &[("0", "0", "9.7", "0"), ("10", "0.3", "10", "9.7"), ("9.7", "10", "0.3", "10"), ("0", "9.7", "0", "0.3")]
            .iter()
            .map(|(a, b, c, d)| format!("0\nLINE\n10\n{a}\n20\n{b}\n11\n{c}\n21\n{d}\n"))
            .collect::<String>()
        + "0\nENDSEC\n0\nEOF\n";
    let m = model(&["F.Cu", "B.Cu"]);
    // Without the fix the chaining tolerance of the checker (0.01 mm) does not close it.
    let mut b = board(&m);
    b.apply(&Cmd::ImportGraphics(req(dxf.clone(), "Edge.Cuts"))).unwrap();
    assert!(!eda_drc::outline::board_outline_of(&added(&b), false).valid);
    // With it (the dialog's default tolerance is 1 mm) the four corners meet.
    let mut b = board(&m);
    let mut r = req(dxf, "Edge.Cuts");
    r.fix_tolerance_um = Some(1_000);
    b.apply(&Cmd::ImportGraphics(r)).unwrap();
    let new = added(&b);
    let outline = eda_drc::outline::board_outline_of(&new, false);
    assert!(outline.valid && outline.polys.outline_count() == 1, "{:?} {:?}", outline.errors, new);
    // Each side now ends where the next begins, at a corner of the square.
    let corners: Vec<Point> = new.iter().map(|s| ends(s).1).collect();
    for (i, s) in new.iter().enumerate() {
        assert_eq!(ends(s).1, ends(&new[(i + 1) % 4]).0, "{new:?}");
    }
    assert!(corners.contains(&p(10_000, 0)) || corners.contains(&p(10_000, -0)), "{corners:?}");
}

#[test]
fn an_outline_that_replaces_the_boards_becomes_the_boards_outline() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // The board starts with a 100 x 100 mm polygon outline of the intent.
    assert!(!eda_model::outline::outline_is_shapes(b.design()));
    let mut r = req(fixture("board_outline_lines_arcs.dxf"), "Edge.Cuts");
    r.replace_outline = true;
    b.apply(&Cmd::ImportGraphics(r)).unwrap();
    assert!(eda_model::outline::outline_is_shapes(b.design()), "the imported shapes are the outline now");
    // The summary polygon is the imported board's ring: 40 x 30 mm, with the corners cut.
    let ring = &b.design().placement.as_ref().unwrap().outline;
    let (minx, maxx) = (ring.iter().map(|p| p.x).min().unwrap(), ring.iter().map(|p| p.x).max().unwrap());
    let (miny, maxy) = (ring.iter().map(|p| p.y).min().unwrap(), ring.iter().map(|p| p.y).max().unwrap());
    assert_eq!((minx, maxx, miny, maxy), (0, 40_000, -30_000, 0), "{ring:?}");
    // Edge.Cuts of the board are the imported shapes only: kicad-cli will see no 100 x 100 mm polygon.
    let edge = eda_model::outline::edge_cuts_shapes(b.design());
    assert_eq!(edge.len(), 8);
}

#[test]
fn an_open_outline_cannot_replace_the_boards() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n10\n21\n0\n0\nLINE\n10\n10\n20\n0\n11\n10\n21\n10\n0\nENDSEC\n0\nEOF\n".to_string();
    let mut r = req(dxf, "Edge.Cuts");
    r.replace_outline = true;
    let err = b.apply(&Cmd::ImportGraphics(r)).unwrap_err();
    assert_eq!(err[0].check, "ops_open_outline", "{err:?}");
    assert!(!eda_model::outline::outline_is_shapes(b.design()));
    assert!(added(&b).is_empty(), "a refused command changes nothing");
}

#[test]
fn text_becomes_a_board_text_on_the_layer() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n5\n20\n5\n40\n2\n1\nREV A\n50\n90\n0\nENDSEC\n0\nEOF\n".to_string();
    b.apply(&Cmd::ImportGraphics(req(dxf, "F.SilkS"))).unwrap();
    let t = drawings(&b).texts.iter().find(|t| t.content == "REV A").unwrap();
    assert_eq!((t.layer.as_str(), t.size_um, t.angle, t.justify), ("F.SilkS", 2_000, 90_000, TextJustify::Left));
    // Baseline justified (bottom): the board's text is centred, so the anchor moved up half a height along the text's own up (here to the left).
    assert_eq!((t.at.x, t.at.y), (4_000, -5_000), "{t:?}");
}

#[test]
fn a_file_that_cannot_be_imported_is_refused_with_the_reason_and_nothing_changes() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let before = b.design().clone();
    // Nothing but a dimension: "No graphic items found in file." and what the plugin could not handle.
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nDIMENSION\n0\nENDSEC\n0\nEOF\n".to_string();
    let e = b.apply(&Cmd::ImportGraphics(req(dxf, "Dwgs.User"))).unwrap_err();
    assert_eq!(e[0].check, "ops_empty_import");
    assert!(e[0].hint.clone().unwrap_or_default().contains("No graphic items found in file.") && e[0].hint.clone().unwrap_or_default().contains("dimensions"), "{e:?}");
    // A binary DXF.
    assert!(b.apply(&Cmd::ImportGraphics(req("AutoCAD Binary DXF\r\n".into(), "Dwgs.User"))).is_err());
    // A layer the board does not have, no layer, no scale.
    assert!(b.apply(&Cmd::ImportGraphics(req(fixture("holes.dxf"), "Nope.Layer"))).is_err());
    assert!(b.apply(&Cmd::ImportGraphics(req(fixture("holes.dxf"), ""))).is_err());
    let mut zero = req(fixture("holes.dxf"), "Dwgs.User");
    zero.scale = 0.0;
    assert!(b.apply(&Cmd::ImportGraphics(zero)).is_err());
    // Too big for KiCad's 32-bit nanometres: refused with the scale that fits.
    let mut big = req(fixture("holes.dxf"), "Dwgs.User");
    big.scale = 1.0e6;
    let e = b.apply(&Cmd::ImportGraphics(big)).unwrap_err();
    assert!(e[0].hint.clone().unwrap_or_default().contains("Maximum scale is"), "{e:?}");
    assert_eq!(serde_json::to_string(b.design()).unwrap(), serde_json::to_string(&before).unwrap(), "a refused command changes nothing");
}

fn svg_req(name: &str, layer: &str) -> ImportGraphics {
    ImportGraphics { format: GraphicsFormat::Svg, ..req(fixture(name), layer) }
}

#[test]
fn an_svg_outline_becomes_a_polygon_on_the_layer_with_the_strokes_width() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportGraphics(svg_req("board_outline_rect.svg", "Dwgs.User"))).unwrap();
    let new = added(&b);
    assert_eq!(new.len(), 1, "{new:?}");
    let Shape::Polygon { layer, stroke_width, filled, pts, .. } = &new[0] else { panic!("{new:?}") };
    // The stroke of the SVG is 0.1 mm; the dialog's DXF line width (0.2) has no say for an SVG. An SVG's Y axis already points down.
    assert_eq!((layer.as_str(), *stroke_width, *filled), ("Dwgs.User", 100, false));
    let (minx, maxx) = (pts.iter().map(|p| p.x).min().unwrap(), pts.iter().map(|p| p.x).max().unwrap());
    let (miny, maxy) = (pts.iter().map(|p| p.y).min().unwrap(), pts.iter().map(|p| p.y).max().unwrap());
    assert_eq!((minx, maxx, miny, maxy), (0, 40_000, 0, 30_000));
    assert!(pts.len() > 20, "the rounded corners are cut into segments: {}", pts.len());
}

#[test]
fn an_svg_scale_and_origin_work_as_a_dxfs_do() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut r = svg_req("board_outline_rect.svg", "Cmts.User");
    r.scale = 0.5;
    r.offset_x_nm = 10_000_000;
    r.offset_y_nm = 20_000_000;
    b.apply(&Cmd::ImportGraphics(r)).unwrap();
    let Shape::Polygon { stroke_width, pts, .. } = &added(&b)[0] else { panic!() };
    assert_eq!(*stroke_width, 50, "0.1 mm halved");
    assert_eq!((pts.iter().map(|p| p.x).min().unwrap(), pts.iter().map(|p| p.x).max().unwrap()), (10_000, 30_000));
    assert_eq!((pts.iter().map(|p| p.y).min().unwrap(), pts.iter().map(|p| p.y).max().unwrap()), (20_000, 35_000));
}

#[test]
fn an_svg_with_curves_filled_shapes_and_text_keeps_the_curves_and_the_fills() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportGraphics(req_fmt(GraphicsFormat::Svg, fixture("paths_and_transforms.svg"), "F.SilkS"))).unwrap();
    let new = added(&b);
    assert!(new.iter().any(|s| matches!(s, Shape::Bezier { .. })), "the cubic, the quadratic and the arc keep their bend: {new:?}");
    assert!(new.iter().any(|s| matches!(s, Shape::Segment { .. })), "the straight pieces are lines");
    let fills: Vec<&Shape> = new.iter().filter(|s| matches!(s, Shape::Polygon { filled: true, .. })).collect();
    assert_eq!(fills.len(), 1, "the shape with the hole is one filled polygon");
    assert_eq!(fills[0].stroke_width(), 0, "a fill with no stroke has no width");
    assert_eq!(drawings(&b).texts.len(), 1, "text is not imported: only the fixture board's own is there");
}

fn req_fmt(format: GraphicsFormat, text: String, layer: &str) -> ImportGraphics {
    ImportGraphics { format, ..req(text, layer) }
}

#[test]
fn an_svg_outline_can_replace_the_boards_outline_too() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut r = svg_req("board_outline_rect.svg", "Edge.Cuts");
    r.replace_outline = true;
    b.apply(&Cmd::ImportGraphics(r)).unwrap();
    assert!(eda_model::outline::outline_is_shapes(b.design()));
    let ring = &b.design().placement.as_ref().unwrap().outline;
    assert_eq!((ring.iter().map(|p| p.x).min().unwrap(), ring.iter().map(|p| p.x).max().unwrap(), ring.iter().map(|p| p.y).max().unwrap()), (0, 40_000, 30_000), "{}", ring.len());
}

#[test]
fn the_command_reads_from_and_writes_to_the_json_the_studio_sends() {
    let r = req("0\nEOF\n".into(), "Edge.Cuts");
    let cmd = Cmd::ImportGraphics(r);
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("\"op\":\"import_graphics\"") && text.contains("\"format\":\"dxf\""), "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::from_value::<Cmd>(value).unwrap(), cmd);
    // Everything but the format, the text and the layer defaults.
    let minimal = serde_json::json!({ "op": "import_graphics", "format": "svg", "text": "<svg/>", "layer": "F.SilkS" });
    let Cmd::ImportGraphics(r) = serde_json::from_value::<Cmd>(minimal).unwrap() else { panic!() };
    assert_eq!((r.format, r.scale, r.offset_x_nm, r.line_width_um, r.group, r.fix_tolerance_um, r.replace_outline), (GraphicsFormat::Svg, 1.0, 0, 200, false, None, false));
    assert_eq!(GraphicsFormat::from_extension("a/b/Board.DXF"), Some(GraphicsFormat::Dxf));
    assert_eq!(GraphicsFormat::from_extension("logo.svg"), Some(GraphicsFormat::Svg));
    assert_eq!(GraphicsFormat::from_extension("board.kicad_pcb"), None);
    let _ = design();
}
