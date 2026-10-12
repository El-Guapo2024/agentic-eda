//! The DXF reader on small files (`tests/fixtures/*.dxf`): a board outline of lines and arcs and the same one as a polyline with bulges,
//! circles, a slot, nested blocks, and the units a file names and the ones it leaves to the caller. Where KiCad's own example files are
//! at hand (`KICAD_SRC_DIR`) they are read too, as a check that nothing in a real DXF stops the reader.

use eda_kicad::dxf_import::{read_dxf, DxfOptions, DxfUnits};
use eda_kicad::graphics_import::{arc_points, ImportedShape, V2};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The two end points of a line or arc, in the order the shape is drawn.
fn ends(s: &ImportedShape) -> Option<(V2, V2)> {
    match s {
        ImportedShape::Line { start, end, .. } => Some((*start, *end)),
        ImportedShape::Arc { center, start, angle_deg, .. } => {
            let (s, _, e) = arc_points(*center, *start, *angle_deg);
            Some((s, e))
        }
        _ => None,
    }
}

fn close(a: V2, b: V2) -> bool {
    (a - b).norm() < 1e-6
}

/// Do the shapes chain end to end into one loop (each one's end is the next one's start or end, whichever way it is drawn)?
fn is_one_closed_loop(shapes: &[ImportedShape]) -> bool {
    let segs: Vec<(V2, V2)> = shapes.iter().filter_map(ends).collect();
    if segs.len() != shapes.len() || segs.is_empty() {
        return false;
    }
    let mut used = vec![false; segs.len()];
    used[0] = true;
    let first = segs[0].0;
    let mut at = segs[0].1;
    for _ in 1..segs.len() {
        let Some(k) = (0..segs.len()).find(|&k| !used[k] && (close(segs[k].0, at) || close(segs[k].1, at))) else { return false };
        used[k] = true;
        at = if close(segs[k].0, at) { segs[k].1 } else { segs[k].0 };
    }
    close(at, first)
}

#[test]
fn a_filleted_board_outline_of_lines_and_arcs_is_one_closed_loop_of_40_by_30() {
    let i = read_dxf(&fixture("board_outline_lines_arcs.dxf"), &DxfOptions::default()).unwrap();
    assert_eq!(i.shapes.iter().filter(|s| matches!(s, ImportedShape::Line { .. })).count(), 4);
    assert_eq!(i.shapes.iter().filter(|s| matches!(s, ImportedShape::Arc { .. })).count(), 4);
    assert!(is_one_closed_loop(&i.shapes), "{:?}", i.shapes);
    // The Y axis points down, so the board hangs below the origin's line... above it: y from -30 to 0.
    let b = i.bbox.unwrap();
    assert!((b.min.x).abs() < 1e-9 && (b.max.x - 40.0).abs() < 1e-9, "{b:?}");
    assert!((b.min.y + 30.0).abs() < 1e-9 && b.max.y.abs() < 1e-9, "{b:?}");
    // Each corner is a quarter circle of radius 3.
    for s in &i.shapes {
        if let ImportedShape::Arc { center, start, angle_deg, .. } = s {
            assert!(((*start - *center).norm() - 3.0).abs() < 1e-9);
            assert!((angle_deg + 90.0).abs() < 1e-9, "{angle_deg}");
        }
    }
    assert!(i.messages.is_empty());
}

#[test]
fn the_same_outline_as_a_closed_lwpolyline_with_bulges_has_the_same_ends() {
    let a = read_dxf(&fixture("board_outline_lines_arcs.dxf"), &DxfOptions::default()).unwrap();
    let p = read_dxf(&fixture("board_outline_lwpolyline.dxf"), &DxfOptions::default()).unwrap();
    assert_eq!(p.shapes.len(), 8, "four sides and four corners: {:?}", p.shapes);
    assert!(is_one_closed_loop(&p.shapes), "{:?}", p.shapes);
    let bb = p.bbox.unwrap();
    assert!((bb.max.x - 40.0).abs() < 1e-6 && (bb.min.y + 30.0).abs() < 1e-6, "{bb:?}");
    // Every corner arc of the polyline has the radius and sweep of the ARC entities (bulge tan(22.5 degrees) is a quarter circle).
    let arcs = |i: &eda_kicad::graphics_import::Imported| -> Vec<(f64, f64)> {
        let mut v: Vec<(f64, f64)> = i
            .shapes
            .iter()
            .filter_map(|s| match s {
                ImportedShape::Arc { center, start, angle_deg, .. } => Some(((*start - *center).norm(), angle_deg.abs())),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    };
    for (x, y) in arcs(&a).iter().zip(arcs(&p).iter()) {
        assert!((x.0 - y.0).abs() < 1e-6 && (x.1 - y.1).abs() < 1e-5, "{x:?} {y:?}");
    }
}

#[test]
fn circles_keep_their_centres_and_radii() {
    let i = read_dxf(&fixture("holes.dxf"), &DxfOptions::default()).unwrap();
    let mut c: Vec<(V2, f64)> = i
        .shapes
        .iter()
        .filter_map(|s| match s {
            ImportedShape::Circle { center, radius, filled: false, .. } => Some((*center, *radius)),
            _ => None,
        })
        .collect();
    c.sort_by(|a, b| a.0.x.partial_cmp(&b.0.x).unwrap());
    assert_eq!(c, vec![(V2::new(5.0, -5.0), 1.6), (V2::new(20.0, -15.0), 4.0), (V2::new(35.0, -25.0), 1.6)]);
}

#[test]
fn a_slot_of_two_half_circle_bulges_is_two_half_circles_and_two_lines() {
    let i = read_dxf(&fixture("slot_bulges.dxf"), &DxfOptions::default()).unwrap();
    let lines = i.shapes.iter().filter(|s| matches!(s, ImportedShape::Line { .. })).count();
    let arcs: Vec<f64> = i
        .shapes
        .iter()
        .filter_map(|s| match s {
            ImportedShape::Arc { angle_deg, .. } => Some(angle_deg.abs()),
            _ => None,
        })
        .collect();
    assert_eq!((lines, arcs.len()), (2, 2), "{:?}", i.shapes);
    assert!(arcs.iter().all(|a| (a - 180.0).abs() < 1e-9));
    assert!(is_one_closed_loop(&i.shapes));
    // 10 mm long and 4 mm across: the caps bow outwards (a positive bulge bows to the right of the way the polyline runs).
    let b = i.bbox.unwrap();
    assert!((b.width() - 10.0).abs() < 1e-6 && (b.height() - 4.0).abs() < 1e-6, "{b:?}");
    let xs: Vec<f64> = i.shapes.iter().filter_map(|s| match s {
        ImportedShape::Arc { center, start, angle_deg, .. } => Some(arc_points(*center, *start, *angle_deg).1.x),
        _ => None,
    }).collect();
    assert!(xs.iter().any(|x| (x - 0.0).abs() < 1e-6) && xs.iter().any(|x| (x - 10.0).abs() < 1e-6), "the caps' middles are at x 0 and 10: {xs:?}");
}

#[test]
fn nested_blocks_put_a_pad_where_each_row_of_each_insert_has_one() {
    let i = read_dxf(&fixture("nested_blocks.dxf"), &DxfOptions::default()).unwrap();
    // 2 inserts of a row of 3 pads of 4 lines: 24 lines, and nothing at the blocks' own places.
    assert_eq!(i.shapes.len(), 24, "{:?}", i.messages);
    let xs = |sel: &dyn Fn(&V2) -> bool| -> usize {
        i.shapes
            .iter()
            .filter(|s| match s {
                ImportedShape::Line { start, .. } => sel(start),
                _ => false,
            })
            .count()
    };
    // The first row: pads at x 10, 13, 16, y -20 to -21.
    assert_eq!(xs(&|p| (p.y + 20.0).abs() < 1e-9 && p.x >= 10.0 - 1e-9 && p.x <= 18.0), 6, "the pads' bottom and top-left starts");
    // The second row is turned a quarter counter-clockwise about its insertion point (30, 20): the pads climb the drawing's Y axis, so
    // they go up the screen (negative y) from y -20 in steps of 3.
    let near_x30 = |p: &V2| p.x >= 28.0 && p.x <= 31.0;
    let ys: Vec<f64> = i
        .shapes
        .iter()
        .filter_map(|s| match s {
            ImportedShape::Line { start, .. } if near_x30(start) => Some(start.y),
            _ => None,
        })
        .collect();
    assert!(!ys.is_empty());
    let miny = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!((miny + 28.0).abs() < 1e-6, "the third pad of the turned row reaches y -26 to -28: {miny}");
}

#[test]
fn a_file_in_inches_is_25_4_times_a_file_in_millimetres_and_no_unit_takes_the_callers() {
    let width = |name: &str, units: DxfUnits| -> f64 {
        let i = read_dxf(&fixture(name), &DxfOptions { units, line_width_mm: 0.2 }).unwrap();
        i.bbox.unwrap().width()
    };
    assert!((width("inches.dxf", DxfUnits::Mm) - 50.8).abs() < 1e-9, "$INSUNITS 1 wins over the caller's millimetres");
    assert!((width("no_units.dxf", DxfUnits::Inch) - 50.8).abs() < 1e-9);
    assert!((width("no_units.dxf", DxfUnits::Mm) - 2.0).abs() < 1e-9);
    assert!((width("no_units.dxf", DxfUnits::Mils) - 0.0508).abs() < 1e-9);
    assert!((width("no_units.dxf", DxfUnits::Cm) - 20.0).abs() < 1e-9);
}

/// KiCad's own example files, when the source is at hand: nothing in them stops the reader, and the counts are the files'.
#[test]
fn kicads_example_files_read_when_the_source_is_at_hand() {
    let Some(src) = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from) else {
        eprintln!("KICAD_SRC_DIR not set; skipping");
        return;
    };
    let examples = src.join("common/import_gfx/examples");
    let Ok(outlines) = std::fs::read_to_string(examples.join("test_outlines.dxf")) else {
        eprintln!("no examples under {}; skipping", examples.display());
        return;
    };
    let i = read_dxf(&outlines, &DxfOptions::default()).unwrap();
    let count = |f: &dyn Fn(&ImportedShape) -> bool| i.shapes.iter().filter(|s| f(s)).count();
    assert_eq!(count(&|s| matches!(s, ImportedShape::Line { .. })), 94);
    assert_eq!(count(&|s| matches!(s, ImportedShape::Circle { .. })), 121);
    assert_eq!(count(&|s| matches!(s, ImportedShape::Arc { .. })), 28);
    if let Ok(ellipses) = std::fs::read_to_string(examples.join("basic_ellipses.dxf")) {
        let e = read_dxf(&ellipses, &DxfOptions::default()).unwrap();
        assert!(e.shapes.len() >= 127 && e.messages.is_empty(), "{} shapes, {:?}", e.shapes.len(), e.messages);
    }
}
