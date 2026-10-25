//! Tests of the Specctra port: the writer held to the DSN file KiCad itself wrote, the parser held to the errors KiCad gives.

use eda_model::units::mm;
use eda_model::units::Nm;
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
    Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(format!("{reference}_val")), package: Some(package.into()), footprint: Some(package.into()), pins, body_nm: None, symbol: None, datasheet: None, edge: None }
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
        outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(20.0), y: Nm::ZERO }, Point { x: mm(20.0), y: mm(20.0) }, Point { x: Nm::ZERO, y: mm(20.0) }],
        footprints: vec![
            FootprintInstance { id: "U1".into(), at: Point { x: mm(5.0), y: mm(5.0) }, rot: 0, side: Side::Top, label: Default::default() },
            FootprintInstance { id: "C1".into(), at: Point { x: mm(10.0), y: mm(10.0) }, rot: 90_000, side: Side::Bottom, label: Default::default() },
        ],
        modules: Vec::new(),
    };
    let design = Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(placement),
        routing: Some(RoutingSection {
            tracks: vec![Track { id: String::new(), net: "VIN".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.2), pts: vec![Point { x: mm(5.0), y: mm(5.0) }, Point { x: mm(10.0), y: mm(5.0) }, Point { x: mm(10.0), y: mm(8.0) }], arc_mid_offset: None }],
            vias: vec![Via { id: String::new(), net: "VIN".into(), at: Point { x: mm(10.0), y: mm(8.0) }, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }],
            zones: vec![Zone { net: "GND".into(), layer: "B.Cu".into(), outline: vec![Point { x: mm(1.0), y: mm(1.0) }, Point { x: mm(19.0), y: mm(1.0) }, Point { x: mm(19.0), y: mm(19.0) }, Point { x: mm(1.0), y: mm(19.0) }], ..Zone::default() }],
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
    let segs: Vec<(Point, Point, Nm, &str, &str)> = got.tracks.iter().map(|(t, _)| (t.pts[0], t.pts[1], t.width, t.layer.as_str(), t.net.as_str())).collect();
    assert_eq!(
        segs,
        vec![(Point { x: mm(5.0), y: mm(5.0) }, Point { x: mm(10.0), y: mm(5.0) }, mm(0.2), "F.Cu", "VIN"), (Point { x: mm(10.0), y: mm(5.0) }, Point { x: mm(10.0), y: mm(8.0) }, mm(0.2), "F.Cu", "VIN")]
    );
    assert!(got.tracks.iter().all(|(_, locked)| !locked));
    assert_eq!(got.vias.len(), 1);
    let v = &got.vias[0];
    assert_eq!((v.via.at, v.via.diameter, v.via.drill, v.via.from_layer.as_str(), v.via.to_layer.as_str(), v.via.net.as_str()), (Point { x: mm(10.0), y: mm(8.0) }, mm(0.6), mm(0.3), "F.Cu", "B.Cu", "VIN"));
    assert_eq!(v.kind, ViaKind::Through);
    let mut places: Vec<_> = got.placements.iter().map(|p| (p.reference.as_str(), p.at, p.rot, p.side)).collect();
    places.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(places, vec![("C1", Point { x: mm(10.0), y: mm(10.0) }, 90_000, Side::Bottom), ("U1", Point { x: mm(5.0), y: mm(5.0) }, 0, Side::Top)]);
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
    assert_eq!((t.pts[0], t.pts[1], t.width, *locked), (Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(2.54), y: mm(1.27) }, mm(0.254), true));
    assert_eq!(got.tracks[1].0.net, "", "a net the board has not got is no net");
    let v = &got.vias[0].via;
    assert_eq!((v.at, v.diameter, v.drill), (Point { x: mm(2.54), y: mm(1.27) }, Nm::new(609_600), mm(0.4)));
}

/// `makeARC`: a quarter arc about its third vertex, the middle on the short way round.
#[test]
fn a_qarc_is_an_arc_through_its_middle() {
    let (design, model) = sample_board();
    let ses = session_with("(net VIN (wire (qarc F.Cu 10 0 0 100 -100 100 0)))");
    let got = from_session(&ses, "s.ses", &design, &model).unwrap();
    let (start, mid, end) = got.tracks[0].0.arc().expect("an arc");
    assert_eq!((start, end), (Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(2.54), y: mm(2.54) }));
    // The centre is (2540000, 0) on the board: the middle of the quarter is at 45 degrees from it, towards the origin's side.
    let r = 2_540_000.0_f64;
    assert!(((mid.x.as_f64()) - (r - r / 2f64.sqrt())).abs() <= 1.0 && ((mid.y.as_f64()) - r / 2f64.sqrt()).abs() <= 1.0, "{mid:?}");
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
    // `GetTokenString( type )` puts the token in quotes.
    assert_eq!(err(&session_with("(net VIN (via Sq 0 0))")), "Unsupported via shape: 'rect'.");
    assert_eq!(err("(session s (placement (component X (place Q9 0 0 front 0))) (routes (library_out)))"), "Reference 'Q9' not found.");
    // What the parser refuses is said as `PARSE_ERROR` says it.
    // `Expecting( T_session )` puts `GetTokenString`'s quoted token in quotes again, as kicad-cli's own board reader says it ("Expecting ''('' in ..").
    assert!(err("(pcb x)").starts_with("Expecting ''session'' in 's.ses', line 1"), "{}", err("(pcb x)"));
}

/// The drill a via's padstack name holds is `UNDEFINED_DRILL_DIAMETER` when it is the Default class's, so the via takes its own net's class drill.
#[test]
fn a_via_drill_equal_to_the_default_is_the_nets_own() {
    let (_, model) = sample_board();
    let pcb_text = crate::export_kicad_pcb(&sample_board().0, &model, &crate::ExportMeta { date: "", title: "" }).unwrap();
    let board = BoardView::parse(&pcb_text).unwrap();
    let session = super::parse::load_session(&session_with("(net VIN (via V:400_um 0 0))"), "s.ses").unwrap();
    let got = from_session_db(&session, &board, 400_000, &|net| if net == "VIN" { 350_000 } else { 0 }).unwrap();
    assert_eq!(got.vias[0].via.drill, mm(0.35));
    let got = from_session_db(&session, &board, 300_000, &|_| 0).unwrap();
    assert_eq!(got.vias[0].via.drill, mm(0.4));
}

// ------------------------------------------------------------------------------------------------------------------------------- held to kicad-cli

/// A board written by hand to hold what a plain design does not (`crates/kicad/tests/fixtures/specctra_parity.kicad_pcb`): a footprint turned by 30 degrees with arcs
/// and a courtyard at nanometres, one on the back turned by -45 degrees with an arc and a rounded rectangle pad off its centre, two custom pads (one turned, with
/// a filled polygon drawn wide, an arc, a circle, a line and a rectangle), and a footprint's rule area on an inner layer and the back with a hole.
const PARITY_PCB: &str = include_str!("../../tests/fixtures/specctra_parity.kicad_pcb");
/// What `kicad-cli pcb export specctra` (10.99, 25ec2335f2) wrote for it.
const PARITY_DSN: &str = include_str!("../../tests/fixtures/specctra_parity.dsn");

/// The element of a DSN text that starts with the line `head` (its indentation included), down to the line that closes it.
fn block<'a>(text: &'a str, head: &str) -> &'a str {
    let start = text.find(&format!("\n{head}")).map(|i| i + 1).unwrap_or_else(|| panic!("no {head:?} in\n{text}"));
    let indent = head.len() - head.trim_start().len();
    let close = format!("\n{})\n", " ".repeat(indent));
    let end = text[start..].find(&close).map(|i| start + i + close.len()).unwrap_or(text.len());
    &text[start..end]
}

/// `FromBOARD` on the hand-made board, read as KiCad reads it: the Default class at KiCad's defaults, the outline its Edge.Cuts rectangle.
fn parity_export() -> String {
    let mut board = BoardView::parse(PARITY_PCB).unwrap();
    let edge = eda_model::ir::Shape::Rect { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: mm(80.0), y: mm(40.0) } };
    let mut outline = eda_drc::outline::build_board_polygon_outlines(&[edge], 5_000.0, mm(0.01), true);
    outline.polys.simplify();
    let default_class = super::export::ClassFacts { name: "Default".into(), track_width: 200_000, clearance: 200_000, via_diameter: 600_000, via_drill: 300_000 };
    let class_of = |_: &str| default_class.clone();
    let st = super::export::Settings {
        pcb_name: "specctra_parity.dsn".into(),
        host_version: super::HOST_VERSION.into(),
        outline: &outline,
        hole_clearance: 0,
        max_error: 5_000,
        default_class: default_class.clone(),
        class_of_net: &class_of,
    };
    super::export::from_board(&mut board, &st).unwrap().0.to_text()
}

/// The courtyard of a turned footprint and its arcs (`TransformArcToPolygon`, drawn wide, on the front and turned over to the top from the back) come out as kicad-cli
/// writes them, to the nanometre.
#[test]
fn the_outlines_of_turned_and_flipped_footprints_are_kicads() {
    let ours = parity_export();
    for head in ["    (image T:ARC", "    (image T:BACKARC"] {
        let (want, got) = (block(PARITY_DSN, head), block(&ours, head));
        assert!(want == got, "{head}: {}", first_difference(want, got));
    }
}

/// The padstacks of a rounded rectangle off the pad's centre and of two custom pads (the convex hull of `MergePrimitivesAsPolygon`: the anchor and each basic shape as
/// `TransformShapeToPolygon` makes it) are kicad-cli's shapes. The names of the custom ones end in a hash of KiCad's merged polygon, which is not made here.
#[test]
fn the_padstacks_of_rounded_and_custom_pads_are_kicads() {
    let ours = parity_export();
    let shapes = |text: &str, prefix: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(i) = rest.find(&format!("\n    (padstack {prefix}")) {
            let b = block(rest, &format!("    (padstack {prefix}"));
            out.push(b.lines().skip(1).collect::<Vec<_>>().join("\n"));
            // The block ends with its newline, which the next one's search starts at.
            rest = &rest[i + b.len()..];
        }
        out.sort();
        out
    };
    for prefix in ["RoundRect", "Cust"] {
        let (want, got) = (shapes(PARITY_DSN, prefix), shapes(&ours, prefix));
        assert_eq!(want.len(), 2 - (prefix == "RoundRect") as usize);
        assert!(want == got, "{prefix}:\nwant {want:#?}\ngot  {got:#?}");
    }
    // The name says the pad's size, its bounding box on the board (the turn included; KiCad's is that of the shapes `MakeEffectiveShapes` makes, kicad-cli's
    // 10260 x 9770.001, here that of the hull, 10260 x 9764.944), the hull's corners and a hash in KiCad's form (`HASH_128::ToString`, 32 upper case digits).
    let name = ours.split("(pin ").filter_map(|p| p.split(' ').next()).find(|n| n.starts_with("Cust[T]Pad_2000")).unwrap_or("");
    assert!(name.starts_with("Cust[T]Pad_2000.000000x2000.000000_10260.000000x9764.944000_20_um_"), "{name}");
    let hash = name.rsplit('_').next().unwrap();
    assert!(hash.len() == 32 && hash.chars().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase()), "{hash}");
}

/// A footprint's rule area on In1.Cu and B.Cu: a keepout per layer in the order of the stack (`CuStack`), relative to the footprint; its hole a window on the layer
/// `zone->GetLayer()` gives -- for a zone on more than one layer `UNDEFINED_LAYER`, which `m_kicadLayer2pcb` takes for the first layer of the stack.
#[test]
fn a_footprints_rule_area_is_a_keepout_per_layer_of_the_stack() {
    let ours = parity_export();
    let image = block(&ours, "    (image T:KO");
    let want = "    (image T:KO
      (pin Round[A]Pad_1600.000000_um 1 0 0)
      (keepout \"\" (polygon In1.Cu 0  -3000 3000  3000 3000  3000 -3000  -3000 -3000  -3000 3000)
        (window (polygon F.Cu 0  -1000 1000  1000 1000  1000 -1000  -1000 -1000  -1000 1000))
      )
      (keepout \"\" (polygon B.Cu 0  -3000 3000  3000 3000  3000 -3000  -3000 -3000  -3000 3000)
        (window (polygon F.Cu 0  -1000 1000  1000 1000  1000 -1000  -1000 -1000  -1000 1000))
      )
    )
";
    assert!(image == want, "{}", first_difference(want, image));
}

/// The placement and the network of the hand-made board are kicad-cli's, line for line.
#[test]
fn the_placement_and_network_of_the_hand_made_board_are_kicads() {
    let ours = parity_export();
    for head in ["  (placement", "  (network", "  (structure"] {
        let (want, got) = (block(PARITY_DSN, head), block(&ours, head));
        assert!(want == got, "{head}: {}", first_difference(want, got));
    }
}

/// `BuiltBoardOutlines` is in nanometres: a round cutout of the board is a keepout whose corners are where KiCad's polygon of the circle puts them, not rounded to
/// micrometres (kicad-cli's DSN of the same Edge.Cuts circle).
#[test]
fn a_round_cutout_is_kicads_polygon_to_the_nanometre() {
    let (mut design, model) = sample_board();
    // the sample board's outline is a 20 mm square here, so that the 1.5 mm hole at (15, 15) mm is inside it
    design.placement.as_mut().unwrap().outline = vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(20.0), y: Nm::ZERO }, Point { x: mm(20.0), y: mm(20.0) }, Point { x: Nm::ZERO, y: mm(20.0) }];
    design.drawings = Some(eda_model::ir::DrawingsSection {
        shapes: vec![eda_model::ir::Shape::Circle { id: "shp_hole".into(), layer: "Edge.Cuts".into(), stroke_width: mm(0.05), filled: false, center: Point { x: mm(15.0), y: mm(15.0) }, end: Point { x: mm(16.5), y: mm(15.0) } }],
        ..Default::default()
    });
    let out = super::export_dsn(&design, &model, "t.dsn").unwrap();
    let pcb = load_pcb(&out.text, "t.dsn").unwrap();
    let keepouts = &pcb.structure.as_ref().unwrap().keepouts;
    let Some(ShapeElem::Path(hole)) = &keepouts[0].shape else { panic!("{}", out.text) };
    // kicad-cli 10.99's polygon of `(gr_circle (center 15 15) (end 16.5 15))`.
    let kicad = "15247.3 -13517.9  15487.9 -13578.8  15715.1 -13678.5 15922.9 -13814.3  16105.5 -13982.3  16257.9 -14178.2  16376 -14396.4 \
                 16456.6 -14631.1  16497.4 -14875.9  16500 -15000  16497.4 -15124.1 16456.6 -15368.9  16376 -15603.6  16257.9 -15821.8  16105.5 -16017.7 \
                 15922.9 -16185.7  15715.1 -16321.5  15487.9 -16421.2  15247.3 -16482.1 15000 -16502.6  14752.7 -16482.1  14512.1 -16421.2  14284.9 -16321.5 \
                 14077.1 -16185.7  13894.5 -16017.7  13742.1 -15821.8  13624 -15603.6 13543.4 -15368.9  13502.6 -15124.1  13502.6 -14875.9  13543.4 -14631.1 \
                 13624 -14396.4  13742.1 -14178.2  13894.5 -13982.3  14077.1 -13814.3 14284.9 -13678.5  14512.1 -13578.8  14752.7 -13517.9  15000 -13497.4";
    let nums: Vec<&str> = kicad.split_whitespace().collect();
    let mut want: Vec<String> = nums.chunks(2).map(|c| format!("{} {}", c[0], c[1])).collect();
    let mut got: Vec<String> = hole.points.iter().map(|p| format!("{} {}", super::format::g6(p.x), super::format::g6(p.y))).collect();
    assert_eq!(got.first(), got.last(), "the polygon is closed");
    got.pop();
    want.sort();
    got.sort();
    assert_eq!(got, want);
}

/// `Expecting( "front|back" )` and `Unexpected( tok )` name the token they got as `GetTokenString` does -- a keyword by its spelling, any other bare word as
/// `'symbol'` -- in a second pair of quotes, as kicad-cli 10.99's board reader words it (`Expecting start, end, .. or locked. Got ''symbol'' in ..`).
/// `Unexpected( CurText() )` gives the word as it is.
#[test]
fn a_parse_error_names_a_keyword_by_its_spelling_and_another_word_as_a_symbol() {
    let problem = |ses: &str| load_session(ses, "s.ses").unwrap_err().problem;
    assert_eq!(problem("(session s (placement (component X (place R1 0 0 sideways 0))))"), "Expecting front|back. Got ''symbol''");
    assert_eq!(problem("(session s (placement (component X (place R1 0 0 pins 0))))"), "Expecting front|back. Got ''pins''");
    assert_eq!(problem("(session s (placement (component X (pins))))"), "Unexpected ''pins''");
    assert_eq!(problem("(session s (placement (component X (wobble))))"), "Unexpected ''symbol''");
    assert_eq!(problem("(session s (placement (component X (place R1 0 0 front 0 x))))"), "Expecting ''(''");
    assert_eq!(problem("(session s (wobble))"), "Unexpected wobble");
}
