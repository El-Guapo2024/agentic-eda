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

// ------------------------------------------------------------------------------------------------------------------------------- the writer, element by element

use super::format::Out;

fn written(f: impl FnOnce(&mut Out)) -> String {
    let mut out = Out::default();
    f(&mut out);
    out.into_string()
}

/// `PATH::Format`: the points go on one line until it is past 70 characters, then on a line of their own at nest level 6 (or one deeper than the path, if that is more).
#[test]
fn a_path_wraps_past_seventy_characters_at_nest_level_six() {
    let mut p = PathElem::new(false);
    p.layer_id = "F.Cu".into();
    p.aperture_width = 200.0;
    p.points = (0..8).map(|i| Pt::new(10000.0 + i as f64, -20000.5)).collect();
    let text = written(|o| p.format(o, 2));
    assert_eq!(text, "    (path F.Cu 200  10000 -20000.5  10001 -20000.5  10002 -20000.5  10003 -20000.5\n            10004 -20000.5  10005 -20000.5  10006 -20000.5  10007 -20000.5)\n");
    // At nest level 0 (inside a wire) there is no newline after it, and a square aperture is said.
    p.points.truncate(2);
    p.aperture_square = true;
    assert_eq!(written(|o| p.format(o, 0)), "(path F.Cu 200  10000 -20000.5  10001 -20000.5(aperture_type square))");
    p.polygon = true;
    assert!(written(|o| p.format(o, 0)).starts_with("(polygon F.Cu 200"));
}

/// `CIRCLE::Format` writes the centre only when it is not the origin; `QARC::Format` writes its three points.
#[test]
fn a_circle_writes_its_centre_only_off_the_origin_and_a_qarc_its_three_points() {
    let c = Circle { layer_id: "B.Cu".into(), diameter: 600.0, vertex: Pt::default() };
    assert_eq!(written(|o| c.format(o, 3)), "      (circle B.Cu 600)\n");
    let c = Circle { vertex: Pt::new(-1.5, 2.0), ..c };
    assert_eq!(written(|o| c.format(o, 3)), "      (circle B.Cu 600 -1.5 2)\n");
    let q = Qarc { layer_id: "F.Cu".into(), aperture_width: 250.0, vertex: [Pt::new(0.0, 0.0), Pt::new(10.0, 10.0), Pt::new(10.0, 0.0)] };
    assert_eq!(written(|o| q.format(o, 0)), "(qarc F.Cu 250  0 0  10 10  10 0)");
}

/// `WIRE::Format` and `WIRE_VIA::Format`: what `FromBOARD` writes for a routed track and a via, a layer name with a space quoted, and a via's vertexes wrapped past 80
/// characters at one level deeper.
#[test]
fn a_wire_and_a_via_are_written_as_kicad_writes_them() {
    let mut p = PathElem::new(false);
    p.layer_id = "Top Layer".into();
    p.aperture_width = 150.0;
    p.points = vec![Pt::new(1.0, 2.0), Pt::new(3.0, 4.0)];
    let mut w = Wire::new();
    w.shape = Some(ShapeElem::Path(p));
    w.net_id = "/SDA".into();
    w.wire_type = Some(WireType::Fix);
    assert_eq!(written(|o| w.format(o, 2)), "    (wire (path \"Top Layer\" 150  1 2  3 4)(net /SDA)(type fix))\n");

    let mut v = WireVia::new();
    v.padstack_id = "Via[0-1]_600:300_um".into();
    v.vertexes = vec![Pt::new(3292.0, -8374.0)];
    v.net_id = "N1".into();
    v.via_type = Some(WireType::Route);
    assert_eq!(written(|o| v.format(o, 2)), "    (via \"Via[0-1]_600:300_um\"  3292 -8374 (net N1)(type route))\n");
    v.vertexes = (0..7).map(|i| Pt::new(100000.0 + i as f64, -200000.0)).collect();
    v.net_id.clear();
    v.via_type = None;
    assert_eq!(
        written(|o| v.format(o, 2)),
        "    (via \"Via[0-1]_600:300_um\"  100000 -200000  100001 -200000  100002 -200000  100003 -200000\n      100004 -200000  100005 -200000  100006 -200000)\n"
    );
}

/// `PLACE::Format` on one line (no rules, properties or region): the vertex, the side, the rotation, then the lock type and the part number.
#[test]
fn a_place_is_one_line_with_its_part_number_last() {
    let mut p = Place::new();
    p.component_id = "R 1".into();
    p.set_vertex(Pt::new(2875.0, -11200.0));
    p.side = super::model::Side::Back;
    p.rotation = 270.0;
    p.part_number = "10k".into();
    assert_eq!(written(|o| p.format(o, 3)), "      (place \"R 1\" 2875 -11200 back 270 (PN 10k))\n");
}

/// `PADSTACK::Format` of a via `makeVia` made: a circle on each layer and `(attach off)`.
#[test]
fn a_via_padstack_is_its_circles_and_attach_off() {
    let mut ps = Padstack::new();
    ps.padstack_id = "Via[0-1]_600:300_um".into();
    for layer in ["F.Cu", "B.Cu"] {
        let mut s = ShapeEl::new(false);
        s.shape = Some(ShapeElem::Circle(Circle { layer_id: layer.into(), diameter: 600.0, vertex: Pt::default() }));
        ps.shapes.push(s);
    }
    assert_eq!(written(|o| ps.format(o, 2)), "    (padstack \"Via[0-1]_600:300_um\"\n      (shape (circle F.Cu 600))\n      (shape (circle B.Cu 600))\n      (attach off)\n    )\n");
}

// ------------------------------------------------------------------------------------------------------------------------------- FromSESSION

use super::session::{echo_session, from_session, from_session_db};
use eda_model::via_extra::ViaKind;

/// The sample board's DSN and the session a router that kept its routing would send back.
fn sample_session() -> (Design, ConstraintModel, String) {
    let (design, model) = sample_board();
    let dsn = super::export_dsn(&design, &model, "sample.dsn").unwrap();
    let pcb = load_pcb(&dsn.text, "sample.dsn").unwrap();
    (design, model, echo_session(&pcb, "sample.ses"))
}

/// The routing and the placement of a board come back from the session its own DSN makes: the track's two segments, the via with its drill (which the padstack's name
/// holds), the parts where they were -- the one on the back on the back, turned as it was.
#[test]
fn a_session_brings_back_the_routing_and_the_placement_the_dsn_had() {
    let (design, model, ses) = sample_session();
    let got = from_session(&ses, "sample.ses", &design, &model).unwrap();
    let segs: Vec<(Point, Point, i64, &str, &str)> = got.tracks.iter().map(|(t, _)| (t.pts[0], t.pts[1], t.width, t.layer.as_str(), t.net.as_str())).collect();
    assert_eq!(
        segs,
        vec![(Point { x: 5_000, y: 5_000 }, Point { x: 10_000, y: 5_000 }, 200, "F.Cu", "VIN"), (Point { x: 10_000, y: 5_000 }, Point { x: 10_000, y: 8_000 }, 200, "F.Cu", "VIN")]
    );
    assert!(got.tracks.iter().all(|(_, locked)| !locked));
    assert_eq!(got.vias.len(), 1);
    let v = &got.vias[0];
    assert_eq!((v.via.at, v.via.diameter, v.via.drill, v.via.from_layer.as_str(), v.via.to_layer.as_str(), v.via.net.as_str()), (Point { x: 10_000, y: 8_000 }, 600, 300, "F.Cu", "B.Cu", "VIN"));
    assert_eq!(v.kind, ViaKind::Through);
    let mut places: Vec<_> = got.placements.iter().map(|p| (p.reference.as_str(), p.at, p.rot, p.side)).collect();
    places.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(places, vec![("C1", Point { x: 10_000, y: 10_000 }, 90_000, Side::Bottom), ("U1", Point { x: 5_000, y: 5_000 }, 0, Side::Top)]);
}

fn session_with(routes: &str) -> String {
    format!("(session s.ses (base_design s.dsn)\n (routes (resolution mil 1)\n  (library_out (padstack V:400_um (shape (circle F.Cu 24)) (attach off)) (padstack Sq (shape (rect F.Cu -1 -1 1 1))))\n  (network_out {routes})))\n")
}

/// `scale` and `mapPt`: a session's own resolution (mils here), the y axis turned down; `(type fix)` is a locked track; a net the board has not got is no net;
/// a via whose padstack's name holds a drill other than the Default class's keeps it, and one with no drill in its name takes its net's.
#[test]
fn a_session_is_read_in_its_own_resolution() {
    let (design, model) = sample_board();
    let ses = session_with("(net VIN (wire (path F.Cu 10 0 0 100 -50) (type fix)) (via V:400_um 100 -50)) (net NOPE (wire (path B.Cu 8 0 0 0 100)))");
    let got = from_session(&ses, "s.ses", &design, &model).unwrap();
    let (t, locked) = &got.tracks[0];
    assert_eq!((t.pts[0], t.pts[1], t.width, *locked), (Point { x: 0, y: 0 }, Point { x: 2_540, y: 1_270 }, 254, true));
    assert_eq!(got.tracks[1].0.net, "", "a net the board has not got is no net");
    let v = &got.vias[0].via;
    assert_eq!((v.at, v.diameter, v.drill), (Point { x: 2_540, y: 1_270 }, 610, 400));
}

/// `makeARC`: a quarter arc about its third vertex, the middle on the short way round.
#[test]
fn a_qarc_is_an_arc_through_its_middle() {
    let (design, model) = sample_board();
    let ses = session_with("(net VIN (wire (qarc F.Cu 10 0 0 100 -100 100 0)))");
    let got = from_session(&ses, "s.ses", &design, &model).unwrap();
    let (start, mid, end) = got.tracks[0].0.arc().expect("an arc");
    assert_eq!((start, end), (Point { x: 0, y: 0 }, Point { x: 2_540, y: 2_540 }));
    // The centre is (2540, 0) on the board: the middle of the quarter is at 45 degrees from it, towards the origin's side.
    let r = 2_540.0_f64;
    assert!(((mid.x as f64) - (r - r / 2f64.sqrt())).abs() <= 1.0 && ((mid.y as f64) - r / 2f64.sqrt()).abs() <= 1.0, "{mid:?}");
}

/// The `IO_ERROR`s of `FromSESSION`, `makeTRACK` and `makeVIA`, in KiCad's words.
#[test]
fn a_bad_session_is_refused_in_kicads_words() {
    let (design, model) = sample_board();
    let err = |ses: &str| from_session(ses, "s.ses", &design, &model).unwrap_err();
    assert_eq!(err("(session s (base_design x))"), "Session file is missing the \"routes\" section");
    assert_eq!(err("(session s (routes (resolution um 10)))"), "Session file is missing the \"library_out\" section");
    assert_eq!(err(&session_with("(net VIN (wire (path In9.Cu 10 0 0 1 1)))")), "Session file uses invalid layer id 'In9.Cu'.");
    assert_eq!(err(&session_with("(net VIN (via Nope 0 0))")), "A wire_via refers to missing padstack 'Nope'.");
    assert_eq!(err(&session_with("(net VIN (via Sq 0 0))")), "Unsupported via shape: rect.");
    assert_eq!(err("(session s (placement (component X (place Q9 0 0 front 0))) (routes (library_out)))"), "Reference 'Q9' not found.");
    // What the parser refuses is said as `PARSE_ERROR` says it.
    assert!(err("(pcb x)").starts_with("Expecting 'session' in 's.ses', line 1"), "{}", err("(pcb x)"));
}

/// The drill a via's padstack name holds is `UNDEFINED_DRILL_DIAMETER` when it is the Default class's, so the via takes its own net's class drill.
#[test]
fn a_via_drill_equal_to_the_default_is_the_nets_own() {
    let (_, model) = sample_board();
    let pcb_text = crate::export_kicad_pcb(&sample_board().0, &model, &crate::ExportMeta { date: "", title: "" }).unwrap();
    let board = BoardView::parse(&pcb_text).unwrap();
    let session = super::parse::load_session(&session_with("(net VIN (via V:400_um 0 0))"), "s.ses").unwrap();
    let got = from_session_db(&session, &board, 400_000, &|net| if net == "VIN" { 350 } else { 0 }).unwrap();
    assert_eq!(got.vias[0].via.drill, 350);
    let got = from_session_db(&session, &board, 300_000, &|_| 0).unwrap();
    assert_eq!(got.vias[0].via.drill, 400);
}
