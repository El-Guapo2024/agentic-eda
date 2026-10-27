//! The SVG reader on small files (`tests/fixtures/*.svg`): a rect with rounded corners as a board outline, circles and an ellipse, paths with every kind
//! of segment under transforms, and a shape with a hole. Where KiCad's own SVGs are at hand (`KICAD_SRC_DIR`) they are read too.

use eda_kicad::graphics_import::{ImportedShape, V2};
use eda_kicad::svg_import::read_svg;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// nanosvg works in `float`: a few millionths of a millimetre.
fn near(a: V2, x: f64, y: f64) -> bool {
    (a.x - x).abs() < 1e-5 && (a.y - y).abs() < 1e-5
}

#[test]
fn a_rounded_rect_in_millimetres_is_one_closed_polygon_of_40_by_30() {
    let i = read_svg(&fixture("board_outline_rect.svg")).unwrap();
    assert_eq!(i.shapes.len(), 1, "{:?}", i.shapes);
    let ImportedShape::Polygon { pts, filled, stroke, .. } = &i.shapes[0] else { panic!("{:?}", i.shapes) };
    assert!(!filled && (stroke.width - 0.1).abs() < 1e-6, "{stroke:?}");
    let b = i.bbox.unwrap();
    assert!(near(b.min, 0.0, 0.0) && near(b.max, 40.0, 30.0), "{b:?}");
    // The corners are curves cut into segments; the straight sides keep their end points.
    assert!(pts.len() > 20, "{}", pts.len());
    assert!(pts.iter().any(|p| near(*p, 3.0, 0.0)) && pts.iter().any(|p| near(*p, 40.0, 27.0)), "{pts:?}");
    // The ring is a polygon: it closes on itself without repeating its first point.
    assert!(!near(pts[0], pts[pts.len() - 1].x, pts[pts.len() - 1].y));
}

#[test]
fn circles_and_an_ellipse_are_rings_of_points_on_their_curves() {
    let i = read_svg(&fixture("mounting_holes.svg")).unwrap();
    assert_eq!(i.shapes.len(), 3, "{:?}", i.shapes);
    let polys: Vec<&Vec<V2>> = i.shapes.iter().filter_map(|s| if let ImportedShape::Polygon { pts, .. } = s { Some(pts) } else { None }).collect();
    assert_eq!(polys.len(), 3);
    let r = |pts: &Vec<V2>, c: V2| pts.iter().map(|p| ((p.x - c.x).powi(2) + (p.y - c.y).powi(2)).sqrt()).fold((f64::INFINITY, 0.0_f64), |(lo, hi), d| (lo.min(d), hi.max(d)));
    let (lo, hi) = r(polys[0], V2::new(5.0, 5.0));
    assert!((lo - 1.6).abs() < 0.01 && (hi - 1.6).abs() < 0.01, "{lo} {hi}");
    let (lo, hi) = r(polys[1], V2::new(35.0, 25.0));
    assert!((lo - 1.6).abs() < 0.01 && (hi - 1.6).abs() < 0.01, "{lo} {hi}");
    // The ellipse spans 12 x 6 mm about (20, 15).
    let xs: Vec<f64> = polys[2].iter().map(|p| p.x).collect();
    let ys: Vec<f64> = polys[2].iter().map(|p| p.y).collect();
    assert!((xs.iter().cloned().fold(f64::INFINITY, f64::min) - 14.0).abs() < 0.05 && (xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 26.0).abs() < 0.05);
    assert!((ys.iter().cloned().fold(f64::INFINITY, f64::min) - 12.0).abs() < 0.05 && (ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - 18.0).abs() < 0.05);
}

#[test]
fn paths_of_every_kind_under_transforms_land_where_the_file_puts_them() {
    let i = read_svg(&fixture("paths_and_transforms.svg")).unwrap();
    // Text is not imported and says so.
    assert_eq!(i.messages, vec!["SVG text elements are not currently supported.".to_string()]);
    let curves: Vec<(V2, V2)> = i.shapes.iter().filter_map(|s| if let ImportedShape::Spline { start, end, .. } = s { Some((*start, *end)) } else { None }).collect();
    // The open path: 2 line pieces of the polyline, and the open path's own: L, C, S, Q, three arc pieces and the lines: just check its chain.
    let first = curves.iter().find(|c| near(c.0, 5.0, 40.0)).expect("the open path starts at (5, 40)");
    assert!(near(first.1, 15.0, 40.0));
    assert!(curves.iter().any(|c| near(c.1, 65.0, 40.0)), "and ends at (65, 40)");
    // The square under translate(70 10) and rotate(90): (x, y) -> (-y, x) then +(70, 10): the square spans x 60..70, y 10..20.
    let polys: Vec<(&Vec<V2>, bool)> = i.shapes.iter().filter_map(|s| if let ImportedShape::Polygon { pts, filled, .. } = s { Some((pts, *filled)) } else { None }).collect();
    let square = polys.iter().find(|(_, filled)| !filled).expect("the stroked square");
    let (minx, maxx) = square.0.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    let (miny, maxy) = square.0.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    assert!((minx - 60.0).abs() < 1e-6 && (maxx - 70.0).abs() < 1e-6 && (miny - 10.0).abs() < 1e-6 && (maxy - 20.0).abs() < 1e-6, "{minx} {maxx} {miny} {maxy}");
    // The filled shape with a hole is one ring of 300 mm2.
    let filled: Vec<&Vec<V2>> = polys.iter().filter(|(_, f)| *f).map(|(p, _)| *p).collect();
    assert_eq!(filled.len(), 1, "{polys:?}");
    let ring = filled[0];
    let mut a = 0.0;
    for k in 0..ring.len() {
        let (p, q) = (ring[k], ring[(k + 1) % ring.len()]);
        a += p.x * q.y - q.x * p.y;
    }
    assert!((a.abs() / 2.0 - 300.0).abs() < 1e-3, "{}", a.abs() / 2.0);
}

/// KiCad's own SVGs, when the source is at hand: nothing in them stops the reader.
#[test]
fn kicads_svg_files_read_when_the_source_is_at_hand() {
    let Some(src) = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from) else {
        eprintln!("KICAD_SRC_DIR not set; skipping");
        return;
    };
    for rel in ["qa/data/common/import_gfx/single_node_closed_path.svg", "qa/data/cli/basic_test/basic_test.svg"] {
        let Ok(text) = std::fs::read_to_string(src.join(rel)) else {
            eprintln!("no {rel}; skipping");
            continue;
        };
        let i = read_svg(&text).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(!i.shapes.is_empty(), "{rel}: nothing came out");
        let b = i.bbox.unwrap();
        assert!(b.width().is_finite() && b.height().is_finite() && b.width() < 10_000.0, "{rel}: {b:?}");
    }
}

/// What a reading comes to, for comparing two readers: the count of each kind of shape, the box, the total polygon area, the messages.
fn summary(i: &eda_kicad::graphics_import::Imported) -> (Vec<(&'static str, usize)>, Option<(f64, f64, f64, f64)>, f64, Vec<String>) {
    let mut kinds: std::collections::BTreeMap<&'static str, usize> = Default::default();
    let mut area = 0.0;
    for s in &i.shapes {
        let k = match s {
            ImportedShape::Line { .. } => "line",
            ImportedShape::Circle { .. } => "circle",
            ImportedShape::Arc { .. } => "arc",
            ImportedShape::Polygon { pts, .. } => {
                let mut a = 0.0;
                for k in 0..pts.len() {
                    let (p, q) = (pts[k], pts[(k + 1) % pts.len()]);
                    a += p.x * q.y - q.x * p.y;
                }
                area += a.abs() / 2.0;
                "polygon"
            }
            ImportedShape::Text { .. } => "text",
            ImportedShape::Spline { .. } => "spline",
        };
        *kinds.entry(k).or_default() += 1;
    }
    (kinds.into_iter().collect(), i.bbox.map(|b| (b.min.x, b.min.y, b.max.x, b.max.y)), area, i.messages.clone())
}

fn same(a: &eda_kicad::graphics_import::Imported, b: &eda_kicad::graphics_import::Imported) -> bool {
    let (sa, sb) = (summary(a), summary(b));
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-4 * (1.0 + x.abs().max(y.abs()));
    sa.0 == sb.0
        && sa.3 == sb.3
        && close(sa.2, sb.2)
        && match (sa.1, sb.1) {
            (Some(p), Some(q)) => close(p.0, q.0) && close(p.1, q.1) && close(p.2, q.2) && close(p.3, q.3),
            (None, None) => true,
            _ => false,
        }
}

/// KiCad's nanosvg (the active reader) against the INACTIVE Rust port of nanosvg on the fixtures (which must read the same: the same shapes,
/// box and area to 1e-4, the same messages) and, when `KICAD_SRC_DIR` is set, on KiCad's own SVGs (compared and listed; the port is known
/// to differ from KiCad's nanosvg where KiCad changed it: `<style>` sheets, the basic colour keywords only, the default stroke width).
#[test]
fn kicads_nanosvg_and_the_inactive_port_read_the_fixtures_alike() {
    for name in ["board_outline_rect.svg", "mounting_holes.svg", "paths_and_transforms.svg"] {
        let text = fixture(name);
        let (a, b) = (read_svg(&text).unwrap(), eda_kicad::svg_import::read_svg_port(&text).unwrap());
        assert!(same(&a, &b), "{name}:\n  nanosvg {:?}\n  port    {:?}", summary(&a), summary(&b));
    }
    let Some(src) = std::env::var_os("KICAD_SRC_DIR").map(PathBuf::from) else { return };
    let mut files = Vec::new();
    for dir in ["qa/data/common/import_gfx", "qa/data/cli/basic_test", "qa/data/cli/artwork_generation_regressions"] {
        if let Ok(rd) = std::fs::read_dir(src.join(dir)) {
            files.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "svg")));
        }
    }
    files.sort();
    let mut alike = 0;
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let (a, b) = (read_svg(&text), eda_kicad::svg_import::read_svg_port(&text));
        match (&a, &b) {
            (Ok(a), Ok(b)) if same(a, b) => alike += 1,
            (Ok(a), Ok(b)) => eprintln!("{}: differs\n  nanosvg {:?}\n  port    {:?}", f.display(), summary(a), summary(b)),
            _ => eprintln!("{}: nanosvg {:?} port {:?}", f.display(), a.as_ref().err(), b.as_ref().err()),
        }
    }
    eprintln!("KiCad's SVGs: {alike} of {} read alike", files.len());
}
