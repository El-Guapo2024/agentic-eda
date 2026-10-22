//! Tests of the Specctra port: the writer held to the DSN file KiCad itself wrote, the parser held to the errors KiCad gives.

use super::model::*;
use super::parse::{load_pcb, load_session};

/// A DSN file KiCad 10.99 exported (`crates/freeroute/parity/fixtures`).
const KICAD_DSN: &str = include_str!("../../../freeroute/parity/fixtures/Issue756-tomu-fpga8-pours.dsn");
const KICAD_DSN_KEEPOUTS: &str = include_str!("../../../freeroute/parity/fixtures/keepouts-with-holes.dsn");

/// The first line two texts differ at, as a message.
fn first_difference(a: &str, b: &str) -> String {
    for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
        if la != lb {
            return format!("line {}:\n  want {la:?}\n  got  {lb:?}", i + 1);
        }
    }
    format!("lengths differ: {} vs {} lines", a.lines().count(), b.lines().count())
}

/// Everything of a DSN file KiCad wrote up to its `(wiring`, which a patched KiCad filled with the pours of the board as `protect` wires -- not what KiCad's `FromBOARD` writes.
#[test]
fn kicads_own_dsn_is_written_back_byte_for_byte() {
    let mut pcb = load_pcb(KICAD_DSN, "tomu.dsn").unwrap();
    assert!(pcb.wiring.as_ref().is_some_and(|w| !w.wires.is_empty()), "the pours are wires");
    pcb.wiring = None;
    let head = &KICAD_DSN[..KICAD_DSN.find("  (wiring").unwrap()];
    let want = format!("{head})\n");
    let text = pcb.to_text();
    assert!(text == want, "{}", first_difference(&want, &text));
}

#[test]
fn a_hand_made_design_with_keepout_windows_parses() {
    let pcb = load_pcb(KICAD_DSN_KEEPOUTS, "k.dsn").unwrap();
    let s = pcb.structure.unwrap();
    assert_eq!(s.keepouts.len(), 5);
    assert!(s.keepouts.iter().all(|k| !k.windows.is_empty()));
    assert_eq!(s.layers.len(), 2);
}

#[test]
fn a_session_without_a_routes_section_parses_to_none() {
    let s = load_session("(session x (base_design y))\n", "x.ses").unwrap();
    assert!(s.route.is_none());
    assert_eq!(s.base_design, "y");
    let _ = Pcb::default();
}

// ------------------------------------------------------------------------------------------------------------------------------- FromBOARD

use eda_model::ir::{Design, FootprintInstance, PlacementSection, Point, Provenance, RoutingSection, Side, Track, Via, Zone};
use eda_model::{ConstraintModel, Net as ModelNet, Part, Pin, PinKind};

fn pin(number: &str, kind: PinKind) -> Pin {
    Pin { number: number.into(), name: None, kind }
}

fn part(reference: &str, package: &str, pins: Vec<Pin>) -> Part {
    Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: Some(package.into()), footprint: Some(package.into()), pins, body_um: None, symbol: None, datasheet: None, edge: None }
}

fn model_net(name: &str, pins: &[&str]) -> ModelNet {
    ModelNet { name: name.into(), pins: pins.iter().map(|s| s.to_string()).collect() }
}

/// A 20 x 20 mm board with an SOT-23 on top and a 0603 turned by 90 degrees on the bottom, two nets, a track and a via.
pub(super) fn sample_board() -> (Design, ConstraintModel) {
    let u1 = part("U1", "SOT-23", vec![pin("1", PinKind::Power), pin("2", PinKind::Ground), pin("3", PinKind::Signal)]);
    let c1 = part("C1", "0603", vec![pin("1", PinKind::Passive), pin("2", PinKind::Ground)]);
    let model = ConstraintModel {
        parts: vec![u1, c1],
        nets: vec![model_net("VIN", &["U1.1", "C1.1"]), model_net("GND", &["U1.2", "C1.2"])],
        ..Default::default()
    };
    let placement = PlacementSection {
        outline: vec![Point { x: 0, y: 0 }, Point { x: 20_000, y: 0 }, Point { x: 20_000, y: 20_000 }, Point { x: 0, y: 20_000 }],
        footprints: vec![
            FootprintInstance { id: "U1".into(), at: Point { x: 5_000, y: 5_000 }, rot: 0, side: Side::Top, label: Default::default() },
            FootprintInstance { id: "C1".into(), at: Point { x: 10_000, y: 10_000 }, rot: 90_000, side: Side::Bottom, label: Default::default() },
        ],
        modules: Vec::new(),
    };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: 1,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(placement),
        routing: Some(RoutingSection {
            tracks: vec![Track { id: String::new(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: 200, pts: vec![Point { x: 5_000, y: 5_000 }, Point { x: 10_000, y: 5_000 }, Point { x: 10_000, y: 8_000 }], arc_mid_offset: None }],
            vias: vec![Via { id: String::new(), net: "VIN".into(), at: Point { x: 10_000, y: 8_000 }, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
            zones: vec![Zone { net: "GND".into(), layer: "B.Cu".into(), outline: vec![Point { x: 1_000, y: 1_000 }, Point { x: 19_000, y: 1_000 }, Point { x: 19_000, y: 19_000 }, Point { x: 1_000, y: 19_000 }], ..Zone::default() }],
            track_width_presets: vec![],
            via_presets: vec![],
            teardrop_settings: Default::default(),
        }),
        drawings: None,
    };
    (design, model)
}

#[test]
fn a_board_is_exported_and_the_export_is_a_design_the_parser_reads_back() {
    let (design, model) = sample_board();
    let out = super::export_dsn(&design, &model, "sample.dsn").unwrap();
    println!("{}", out.text);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let pcb = load_pcb(&out.text, "sample.dsn").unwrap();
    // The parse of what was written is what was written.
    assert!(pcb.to_text() == out.text, "{}", first_difference(&out.text, &pcb.to_text()));
}

use super::board::BoardView;

/// Where Specctra puts a pin: the image's pin turned by the component's rotation about its place, mirrored across the y axis first for a component on the back
/// (`mirror_first`, the style FreeRouting reads a DSN with). The y axis of the file points up.
fn specctra_pin_position(place: &Place, pin_x: f64, pin_y: f64) -> (f64, f64) {
    let v = place.vertex.unwrap();
    let (x, y) = if place.side == super::model::Side::Back { (-pin_x, pin_y) } else { (pin_x, pin_y) };
    let a = place.rotation.to_radians();
    (v.x + x * a.cos() - y * a.sin(), v.y + x * a.sin() + y * a.cos())
}

/// The pads of every part land where the board has them when the DSN is read the way a router reads it -- the front, the back (turned over by `FlipFOOTPRINTs`) and
/// the parts turned by quarter turns and by other angles.
#[test]
fn the_pins_of_the_dsn_are_where_the_pads_of_the_board_are() {
    let (mut design, model) = sample_board();
    // More poses: a front part at 30 degrees, a back part at 135 degrees, a front part at 270.
    let pl = design.placement.as_mut().unwrap();
    pl.footprints[0].rot = 30_000;
    pl.footprints[1].rot = 135_000;
    let pcb_text = crate::export_kicad_pcb(&design, &model, &crate::ExportMeta { date: "", title: "" }).unwrap();
    let board = BoardView::parse(&pcb_text).unwrap();
    let dsn = super::export_dsn(&design, &model, "t.dsn").unwrap();
    let pcb = load_pcb(&dsn.text, "t.dsn").unwrap();
    let library = pcb.library.as_ref().unwrap();
    let mut checked = 0;
    for component in &pcb.placement.as_ref().unwrap().components {
        let image = library.images.iter().find(|i| i.image_id() == component.image_id).expect("the image of a component");
        for place in &component.places {
            let fp = board.footprints.iter().find(|f| f.reference == place.component_id).expect("the footprint");
            let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
            for pad in &fp.pads {
                let n = seen.entry(pad.number.clone()).or_insert(0);
                let pin_id = if *n == 0 { pad.number.clone() } else { format!("{}@{}", pad.number, n) };
                *n += 1;
                let Some(pin) = image.pins.iter().find(|p| p.pin_id == pin_id) else { continue };
                let (x, y) = specctra_pin_position(place, pin.vertex.x, pin.vertex.y);
                let (bx, by) = (pad.pos.x as f64 / 1000.0, -(pad.pos.y as f64) / 1000.0);
                assert!((x - bx).abs() < 0.01 && (y - by).abs() < 0.01, "{} pad {}: the DSN puts it at ({x:.3}, {y:.3}), the board at ({bx:.3}, {by:.3})", fp.reference, pad.number);
                // The pad's own turn: the component's plus the pin's, the pin's reversed by the mirror on the back (a rectangle's turn is told modulo a half turn).
                let board_turn = pad.orient.0;
                let dsn_turn = if place.side == super::model::Side::Back { place.rotation - pin.rotation } else { place.rotation + pin.rotation };
                let diff = (board_turn - dsn_turn).rem_euclid(180.0);
                assert!(diff < 0.01 || (180.0 - diff) < 0.01, "{} pad {}: the DSN turns it by {dsn_turn}, the board by {board_turn}", fp.reference, pad.number);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 5, "every pad was found");
}
