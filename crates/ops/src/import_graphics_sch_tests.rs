//! Import Graphics in the schematic editor and the Symbol Editor: the sheet's graphic lines, shapes and text, and a library symbol's drawn items, made of the
//! small files of `crates/kicad/tests/fixtures` and of a few written here.

use super::pcb_transform_tests::{board, model};
use super::*;
use eda_model::ir::{LibraryFill, LibrarySymbolGraphic, Millideg};
use eda_model::kicad_font;
use eda_model::sch_extras::{SchColor, SchFill, SchGraphic, SchGraphicKind, SchLineStyle};

fn fixture(name: &str) -> String {
    let path = format!("{}/../kicad/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A DXF of millimetres holding `entities`.
fn dxf(entities: &str) -> String {
    format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
}

fn sch_req(format: GraphicsFormat, text: String) -> ImportSchGraphics {
    ImportSchGraphics { format, text, dxf_units: String::new(), scale: 1.0, offset_x_nm: 0, offset_y_nm: 0, line_width_nm: 0 }
}

fn sym_req(text: String) -> ImportSymbolGraphics {
    ImportSymbolGraphics { lib_id: "Test:Sym".into(), unit: 1, body_style: 1, format: GraphicsFormat::Dxf, text, dxf_units: String::new(), scale: 1.0, offset_x_nm: 0, offset_y_nm: 0, line_width_nm: 0 }
}

fn sch<'a>(b: &'a Board<'_>) -> &'a eda_model::ir::SchematicSection {
    b.design().schematic.as_ref().expect("the import made the sheet")
}

fn graphics_of(b: &Board<'_>) -> Vec<SchGraphic> {
    sch(b).extras.graphics.clone()
}

fn symbol_graphics(b: &Board<'_>) -> Vec<LibrarySymbolGraphic> {
    b.design().symbol_library.as_ref().unwrap().by_lib_id("Test:Sym").unwrap().graphics.clone()
}

fn open_symbol(b: &mut Board<'_>) {
    b.apply(&Cmd::OpenSymbolForEdit { lib_id: "Test:Sym".into() }).unwrap();
}

fn bounds(points: impl Iterator<Item = (f64, f64)>) -> (f64, f64, f64, f64) {
    points.fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |(x0, y0, x1, y1), (x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)))
}

// ------------------------------------------------------------------------------------------------------------------------------ the schematic

#[test]
fn a_dxf_outline_becomes_graphic_lines_and_arcs_of_the_sheet() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Dxf, fixture("board_outline_lines_arcs.dxf")))).unwrap();
    let s = sch(&b);
    assert_eq!(s.lines.len(), 4, "{:?}", s.lines);
    let arcs: Vec<&SchGraphic> = s.extras.graphics.iter().filter(|g| matches!(g.shape, SchGraphicKind::Arc { .. })).collect();
    assert_eq!((arcs.len(), s.extras.graphics.len()), (4, 4));
    // No line width in the file, none in the dialog: the sheet's default width (0), the way `MapLineWidth( 0 )` gives it.
    assert!(s.lines.iter().all(|l| l.width_nm == 0 && l.pts.len() == 2 && !l.id.is_empty()), "{:?}", s.lines);
    assert!(arcs.iter().all(|g| g.width_nm == 0 && g.fill == SchFill::None));
    // Y is down on a sheet as on the board: the 40 x 30 outline hangs above the origin.
    let (x0, y0, x1, y1) = bounds(s.lines.iter().flat_map(|l| l.pts.iter().map(|p| (p.x as f64, p.y as f64))));
    assert_eq!((x0, y0, x1, y1), (0.0, -30_000_000.0, 40_000_000.0, 0.0));
    // An arc's middle is on its circle: the corner at the origin side, radius 3 mm.
    let SchGraphicKind::Arc { start, mid, end } = arcs[0].shape else { unreachable!() };
    let (cx, cy) = circle_through(start, mid, end);
    assert!((((start.x as f64 - cx).hypot(start.y as f64 - cy)) - 3_000_000.0).abs() < 2.0, "{start:?} {mid:?} {end:?}");
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
fn the_scale_the_offset_and_the_default_line_width_are_the_dialogs() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut r = sch_req(GraphicsFormat::Dxf, fixture("no_units.dxf"));
    r.dxf_units = "mm".into();
    r.scale = 2.0;
    r.line_width_nm = 150_000;
    r.offset_x_nm = 10_000_000;
    r.offset_y_nm = -5_000_000;
    b.apply(&Cmd::ImportSchGraphics(r)).unwrap();
    let s = sch(&b);
    assert_eq!(s.lines.len(), 4);
    // The 2 x 1 rectangle, doubled and moved by (10, -5) mm, the lines 0.15 mm doubled.
    let (x0, y0, x1, y1) = bounds(s.lines.iter().flat_map(|l| l.pts.iter().map(|p| (p.x as f64, p.y as f64))));
    assert_eq!((x0, x1), (10_000_000.0, 14_000_000.0));
    assert_eq!((y1 - y0).round(), 2_000_000.0);
    assert!(s.lines.iter().all(|l| l.width_nm == 300_000), "{:?}", s.lines);
}

#[test]
fn circles_are_shapes_with_a_centre_and_a_radius() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Dxf, fixture("holes.dxf")))).unwrap();
    let mut circles: Vec<(Point, Nm)> = graphics_of(&b)
        .into_iter()
        .map(|g| match g.shape {
            SchGraphicKind::Circle { center, radius_nm } => {
                assert_eq!((g.fill, g.width_nm), (SchFill::None, 0));
                (center, radius_nm)
            }
            other => panic!("{other:?}"),
        })
        .collect();
    circles.sort_by_key(|(c, _)| c.x);
    assert_eq!(circles, vec![(Point { x: 5_000_000, y: -5_000_000 }, 1_600_000), (Point { x: 20_000_000, y: -15_000_000 }, 4_000_000), (Point { x: 35_000_000, y: -25_000_000 }, 1_600_000)]);
}

#[test]
fn a_dashed_red_svg_line_keeps_its_style_and_colour_in_the_stroke_table() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><line x1="1" y1="2" x2="11" y2="2" fill="none" stroke="#ff0000" stroke-width="0.3" stroke-dasharray="4 2"/></svg>"##;
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Svg, svg.into()))).unwrap();
    let s = sch(&b);
    assert_eq!(s.lines.len(), 1, "{:?}", s.lines);
    let l = &s.lines[0];
    assert_eq!((l.pts[0], l.pts[1], l.width_nm), (Point { x: 1_000_000, y: 2_000_000 }, Point { x: 11_000_000, y: 2_000_000 }, 300_000));
    let st = s.extras.strokes.get(&l.id).expect("the style and colour are in the stroke table");
    assert_eq!((st.width_nm, st.style, st.color), (0, SchLineStyle::Dash, Some(SchColor { r: 255, g: 0, b: 0, a: 255 })));
}

#[test]
fn a_filled_svg_polygon_is_filled_with_its_colour_and_has_no_outline_to_speak_of() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><polygon points="0,0 10,0 10,10 0,10" fill="#00ff00" stroke="none"/></svg>"##;
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Svg, svg.into()))).unwrap();
    let g = graphics_of(&b);
    assert_eq!(g.len(), 1, "{g:?}");
    let SchGraphicKind::Polygon { pts } = &g[0].shape else { panic!("{g:?}") };
    assert_eq!(pts.len(), 4);
    assert_eq!((g[0].fill, g[0].fill_color), (SchFill::Color, Some(SchColor { r: 0, g: 255, b: 0, a: 255 })));
    // "Historically -1 meant no-stroke": a hairline stands for it.
    assert_eq!(g[0].width_nm, 1_000);
}

#[test]
fn a_text_keeps_the_files_size_and_angle_and_stands_at_the_left_end_of_its_baseline() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // Plain text (left, bottom), then text centred on a point, then one standing up.
    let entities = "0\nTEXT\n8\n0\n10\n10\n20\n5\n30\n0\n40\n2.5\n1\nHello\n\
                    0\nTEXT\n8\n0\n10\n0\n20\n0\n30\n0\n11\n10\n21\n20\n31\n0\n40\n2.5\n1\nHello\n72\n1\n73\n0\n\
                    0\nTEXT\n8\n0\n10\n30\n20\n5\n30\n0\n40\n1.5\n1\nUp\n50\n90\n";
    b.apply(&Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Dxf, dxf(entities)))).unwrap();
    let t = &sch(&b).texts;
    assert_eq!(t.len(), 3, "{t:?}");
    let (plain, centred, up) = (&t[0], &t[1], &t[2]);
    assert_eq!((plain.content.as_str(), plain.at, plain.angle, plain.size_nm), ("Hello", Point { x: 10_000_000, y: -5_000_000 }, 0, 2_500_000));
    // Centred: the anchor is half the text's width to the left of the point the file gives.
    let width_nm = kicad_font::text_advance_iu("Hello", kicad_font::mm_to_iu(2.5)) * 100;
    assert!((centred.at.x - (10_000_000 - width_nm / 2)).abs() <= 100 && centred.at.y == -20_000_000, "{centred:?} {width_nm}");
    let angle: Millideg = 90_000;
    assert_eq!((up.content.as_str(), up.at, up.angle, up.size_nm), ("Up", Point { x: 30_000_000, y: -5_000_000 }, angle, 1_500_000));
}

#[test]
fn nothing_to_import_and_a_zero_scale_are_refused_and_leave_the_design_alone() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let e = b.apply(&Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Dxf, dxf("")))).unwrap_err();
    assert_eq!(e[0].check, "ops_empty_import");
    assert!(e[0].hint.as_deref().is_some_and(|h| h.contains("No graphic items found in file.")), "{:?}", e[0].hint);
    let mut r = sch_req(GraphicsFormat::Dxf, fixture("holes.dxf"));
    r.scale = 0.0;
    assert_eq!(b.apply(&Cmd::ImportSchGraphics(r)).unwrap_err()[0].check, "ops_bad_import");
    let mut r = sch_req(GraphicsFormat::Dxf, fixture("holes.dxf"));
    r.dxf_units = "furlongs".into();
    assert_eq!(b.apply(&Cmd::ImportSchGraphics(r)).unwrap_err()[0].check, "ops_bad_import");
    assert_eq!(b.apply(&Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Dxf, "this is not a DXF".into()))).unwrap_err()[0].check, "ops_bad_import");
    assert!(b.design().schematic.is_none(), "a refused import does not even make the sheet");
}

#[test]
fn the_commands_are_the_editors_own_and_read_from_json() {
    let c = Cmd::ImportSchGraphics(sch_req(GraphicsFormat::Dxf, "x".into()));
    assert_eq!(c.domain(), Domain::Schematic);
    assert!(!c.edits_connectivity(), "drawn lines are not wires");
    assert_eq!(Cmd::ImportSymbolGraphics(sym_req("x".into())).domain(), Domain::SymbolEditor);
    let json = serde_json::json!({ "op": "import_sch_graphics", "format": "svg", "text": "<svg/>" });
    assert_eq!(serde_json::from_value::<Cmd>(json).unwrap(), Cmd::ImportSchGraphics(ImportSchGraphics { format: GraphicsFormat::Svg, text: "<svg/>".into(), dxf_units: String::new(), scale: 1.0, offset_x_nm: 0, offset_y_nm: 0, line_width_nm: 0 }));
    let json = serde_json::json!({ "op": "import_symbol_graphics", "lib_id": "Test:Sym", "unit": 2, "format": "dxf", "text": "x", "scale": 0.5, "offset_x_nm": -1500000 });
    let Cmd::ImportSymbolGraphics(r) = serde_json::from_value::<Cmd>(json).unwrap() else { panic!() };
    assert_eq!((r.unit, r.body_style, r.scale, r.offset_x_nm, r.offset_y_nm), (2, 1, 0.5, -1_500_000, 0));
}

// ------------------------------------------------------------------------------------------------------------------------------ the symbol

#[test]
fn a_dxf_outline_in_a_symbol_is_polylines_and_arcs_in_millimetres_the_right_way_up() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    open_symbol(&mut b);
    b.apply(&Cmd::ImportSymbolGraphics(sym_req(fixture("board_outline_lines_arcs.dxf")))).unwrap();
    let g = symbol_graphics(&b);
    let lines: Vec<&LibrarySymbolGraphic> = g.iter().filter(|x| matches!(x, LibrarySymbolGraphic::Polyline { .. })).collect();
    let arcs = g.iter().filter(|x| matches!(x, LibrarySymbolGraphic::Arc { .. })).count();
    assert_eq!((lines.len(), arcs, g.len()), (4, 4, 8), "{g:?}");
    for x in &g {
        let (id, unit, style) = match x {
            LibrarySymbolGraphic::Polyline { id, unit, body_style, .. } | LibrarySymbolGraphic::Arc { id, unit, body_style, .. } => (id, unit, body_style),
            other => panic!("{other:?}"),
        };
        assert!(!id.is_empty() && *unit == 1 && *style == 1, "{x:?}");
    }
    // A library's Y axis points up: the outline of the file stands the way up it was drawn, y 0 to 30 mm.
    let pts = lines.iter().flat_map(|x| match x {
        LibrarySymbolGraphic::Polyline { pts, .. } => pts.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>(),
        _ => vec![],
    });
    let (x0, y0, x1, y1) = bounds(pts);
    assert_eq!((x0, y0, x1, y1), (0.0, 0.0, 40.0, 30.0));
    assert!(lines.iter().all(|x| matches!(x, LibrarySymbolGraphic::Polyline { stroke_mm, fill, .. } if *stroke_mm == 0.0 && *fill == LibraryFill::None)));
}

#[test]
fn the_unit_the_body_style_the_scale_and_the_offset_are_the_dialogs() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    open_symbol(&mut b);
    let mut r = sym_req(fixture("holes.dxf"));
    r.unit = 2;
    r.body_style = 2;
    r.scale = 0.5;
    r.offset_x_nm = 10_000_000;
    r.offset_y_nm = 5_000_000;
    b.apply(&Cmd::ImportSymbolGraphics(r)).unwrap();
    let mut circles: Vec<(f64, f64, f64)> = symbol_graphics(&b)
        .into_iter()
        .map(|x| match x {
            LibrarySymbolGraphic::Circle { unit, body_style, center, radius_mm, fill, .. } => {
                assert_eq!((unit, body_style, fill), (2, 2, LibraryFill::None));
                (center.x, center.y, radius_mm)
            }
            other => panic!("{other:?}"),
        })
        .collect();
    circles.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The file's (5, 5) is (5, -5) on the drawing (Y down), (2.5, -2.5) at half scale, (12.5, 2.5) moved by (10, 5), and (12.5, -2.5) with the library's Y up;
    // the radii halve.
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let want = [(12.5, -2.5, 0.8), (20.0, 2.5, 2.0), (27.5, 7.5, 0.8)];
    for (got, want) in circles.iter().zip(want) {
        assert!(near(got.0, want.0) && near(got.1, want.1) && near(got.2, want.2), "{circles:?}");
    }
}

#[test]
fn a_symbols_text_stands_at_the_middle_of_its_baseline() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    open_symbol(&mut b);
    // Left-aligned at (10, 5), centred on (10, 20), and 90 degrees.
    let entities = "0\nTEXT\n8\n0\n10\n10\n20\n5\n30\n0\n40\n2.5\n1\nHello\n\
                    0\nTEXT\n8\n0\n10\n0\n20\n0\n30\n0\n11\n10\n21\n20\n31\n0\n40\n2.5\n1\nHello\n72\n1\n73\n0\n\
                    0\nTEXT\n8\n0\n10\n30\n20\n5\n30\n0\n40\n1.5\n1\nUp\n50\n90\n";
    b.apply(&Cmd::ImportSymbolGraphics(sym_req(dxf(entities)))).unwrap();
    let texts: Vec<(String, f64, f64, f64, f64)> = symbol_graphics(&b)
        .into_iter()
        .filter_map(|x| match x {
            LibrarySymbolGraphic::Text { text, at, angle_deg, size_mm, .. } => Some((text, at.x, at.y, angle_deg, size_mm)),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 3, "{texts:?}");
    let w = kicad_font::text_advance_iu("Hello", kicad_font::mm_to_iu(2.5)) as f64 / kicad_font::IU_PER_MM;
    // Left-aligned text: its middle is half its width to the right of the anchor. Y is up in the library.
    let left = &texts[0];
    assert!((left.1 - (10.0 + w / 2.0)).abs() < 1e-3 && left.2 == 5.0 && left.3 == 0.0 && left.4 == 2.5, "{left:?}");
    // Centred text is at the point the file gives.
    let centred = &texts[1];
    assert!((centred.1 - 10.0).abs() < 1e-9 && centred.2 == 20.0, "{centred:?}");
    // Standing up, its middle is above the anchor by half its width.
    let up = &texts[2];
    let w_up = kicad_font::text_advance_iu("Up", kicad_font::mm_to_iu(1.5)) as f64 / kicad_font::IU_PER_MM;
    assert!((up.1 - 30.0).abs() < 1e-9 && (up.2 - (5.0 + w_up / 2.0)).abs() < 1e-3 && (up.3, up.4) == (90.0, 1.5) && up.0 == "Up", "{up:?}");
}

#[test]
fn a_symbol_has_no_curves_so_an_svg_curve_becomes_segments_that_follow_it() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><path d="M 0 0 C 0 10 10 10 10 0" fill="none" stroke="black" stroke-width="0.2"/></svg>"##;
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    open_symbol(&mut b);
    let mut r = sym_req(svg.into());
    r.format = GraphicsFormat::Svg;
    b.apply(&Cmd::ImportSymbolGraphics(r)).unwrap();
    let g = symbol_graphics(&b);
    assert_eq!(g.len(), 1, "{g:?}");
    let LibrarySymbolGraphic::Polyline { pts, stroke_mm, .. } = &g[0] else { panic!("{g:?}") };
    assert!(pts.len() > 4, "{} points", pts.len());
    assert!((*stroke_mm - 0.2).abs() < 1e-9);
    // The curve leaves (0, 0), peaks at 7.5 below the line in the file (up in the picture's own frame, so -7.5 here) and comes back to (10, 0).
    assert_eq!((pts[0].x, pts[0].y), (0.0, 0.0));
    let last = pts[pts.len() - 1];
    assert!((last.x - 10.0).abs() < 1e-3 && last.y.abs() < 1e-3, "{last:?}");
    let low = pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    assert!((low + 7.5).abs() < 0.02, "{low}");
}

#[test]
fn a_symbol_import_needs_the_symbol_to_be_open_and_something_in_the_file() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert_eq!(b.apply(&Cmd::ImportSymbolGraphics(sym_req(fixture("holes.dxf")))).unwrap_err()[0].check, "ops_unknown_symbol");
    open_symbol(&mut b);
    assert_eq!(b.apply(&Cmd::ImportSymbolGraphics(sym_req(dxf("")))).unwrap_err()[0].check, "ops_empty_import");
    assert!(symbol_graphics(&b).is_empty());
}
