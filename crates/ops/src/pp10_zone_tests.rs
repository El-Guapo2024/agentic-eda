//! UI-LEFT-2 PP10's backend: a zone with holes made a polygon keeps them (`ShapeStyle::holes`, `poly_holes.rs`) through the commands and back to a zone, and Zone
//! Properties' OK makes the board's default zone settings (`set_zone_defaults`, `SetDefaultZoneSettings`).

use super::convert::{polys_from_items, ConvertStrategy};
use super::pcb_transform_tests::{board, drawings, ids, model, p};
use super::*;
use eda_model::units::mm;

fn sq(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![p(mm(x0), mm(y0)), p(mm(x1), mm(y0)), p(mm(x1), mm(y1)), p(mm(x0), mm(y1))]
}

fn area(c: &[Point]) -> f64 {
    let mut a = 0.0;
    for i in 0..c.len() {
        let (p, q) = (c[i], c[(i + 1) % c.len()]);
        a += p.x.as_f64() * q.y.as_f64() - q.x.as_f64() * p.y.as_f64();
    }
    (a / 2.0).abs()
}

#[test]
fn a_zone_with_a_hole_made_a_polygon_keeps_it_through_a_move_and_back_to_a_zone() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: sq(0.0, 0.0, 10.0, 10.0) }).unwrap();
    let zid = b.design().routing.as_ref().unwrap().zones.last().unwrap().id.clone();
    b.design.routing.as_mut().unwrap().zones.iter_mut().find(|z| z.id == zid).unwrap().holes = vec![sq(3.0, 3.0, 6.0, 6.0)];
    // Create Polygon from Selection sends the conversion's ring: the hole bridged in.
    let ring = polys_from_items(b.design(), &[zid.clone()], ConvertStrategy::Centerline, Nm::ZERO).rings.remove(0);
    b.apply(&Cmd::AddShape { shape: Shape::Polygon { id: String::new(), layer: "F.Cu".into(), stroke_width: mm(0.1), filled: true, pts: ring } }).unwrap();
    let sid = drawings(&b).shapes.last().unwrap().id().to_string();
    let Shape::Polygon { pts, .. } = drawings(&b).shapes.last().unwrap() else { panic!() };
    assert_eq!(pts.len(), 4, "the outline alone");
    assert_eq!(drawings(&b).board_items.holes_of(&sid).len(), 1, "and its hole beside it");
    // A move takes the hole with it.
    b.apply(&Cmd::MoveItems { ids: ids(&[sid.as_str()]), dx: mm(20.0), dy: Nm::ZERO }).unwrap();
    let hole = &drawings(&b).board_items.holes_of(&sid)[0];
    assert!(hole.iter().all(|q| q.x >= mm(23.0) && q.x <= mm(26.0)), "{hole:?}");
    // The board file: KiCad writes the outline alone; here the hole is bridged into the one ring, so kicad-cli sees the copper drawn.
    let text = eda_kicad::export_kicad_pcb(b.design(), &m, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    let poly = text.lines().find(|l| l.contains("(gr_poly") && l.contains("F.Cu")).expect("the polygon");
    assert!(poly.matches("(xy").count() >= 9, "outline, bridge and hole: {poly}");
    // Made a zone again: the outline and its hole (`polys.back().AddHole( .. )`).
    let r = polys_from_items(b.design(), &[sid.clone()], ConvertStrategy::Centerline, Nm::ZERO);
    assert_eq!(r.polys.len(), 1);
    assert!((area(&r.polys[0].0) - 100e12).abs() < 1e6 && r.polys[0].1.len() == 1 && (area(&r.polys[0].1[0]) - 9e12).abs() < 1e6, "{:?}", r.polys);
    // An edit of the outline alone keeps nothing it was not given: a plain polygon has none.
    b.apply(&Cmd::ReplaceShape { id: sid.clone(), shape: Shape::Polygon { id: String::new(), layer: "F.Cu".into(), stroke_width: mm(0.1), filled: true, pts: sq(20.0, 0.0, 31.0, 10.0) } }).unwrap();
    assert!(drawings(&b).board_items.holes_of(&sid).is_empty());
}

#[test]
fn zone_properties_ok_makes_the_boards_default_zone_settings_without_its_net_or_outline() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let z = Zone { id: "zone_x".into(), net: "GND".into(), layer: "F.Cu".into(), outline: sq(0.0, 0.0, 1.0, 1.0), clearance: mm(1.0), name: "Pour".into(), layers: vec!["F.Cu".into(), "B.Cu".into()], ..Default::default() };
    b.apply(&Cmd::SetZoneDefaults { zone: Box::new(z) }).unwrap();
    let d = drawings(&b).zone_defaults.clone().expect("the defaults");
    assert_eq!((d.clearance, d.name.as_str(), d.net.as_str(), d.outline.len(), d.id.as_str()), (mm(1.0), "Pour", "", 0, ""));
    assert_eq!(d.layer_set(), vec!["F.Cu", "B.Cu"]);
    // what the studio sends: the zone's JSON with no outline or net
    let json = serde_json::json!({ "op": "set_zone_defaults", "zone": { "net": "", "layer": "F.Cu", "outline": [], "clearance": 200000, "fill_mode": "HatchPattern" } });
    let cmd: Cmd = serde_json::from_value(json).expect("the studio's command reads");
    b.apply(&cmd).unwrap();
    assert_eq!(drawings(&b).zone_defaults.as_ref().unwrap().clearance, Nm::new(200_000));
}
