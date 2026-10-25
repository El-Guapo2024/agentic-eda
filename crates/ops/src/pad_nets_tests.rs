//! Swap Pad Nets and Swap Gate Nets: the nets of the selected pads rotate, the copper connected to them follows, the pads that were not asked for are asked about.

use super::pcb_transform_tests::{design, p, part};
use super::*;
use eda_model::footprint::{placed_pads, PadKind, PadShape};
use eda_model::fp_units::FootprintUnit;
use eda_model::ir::FootprintExtra;
use eda_model::{ConstraintModel, Footprint, Net, Pad};

/// A four-pad footprint, the pads 2 mm apart in a row: `1 2 3 4` at x = -3, -1, 1, 3 mm.
fn quad() -> Footprint {
    let pad = |n: &str, x: Nm| Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (x, Nm::ZERO), size: (Nm::from_raw_shim(800_000), Nm::from_raw_shim(800_000)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None };
    Footprint { name: "QUAD".into(), pads: vec![pad("1", Nm::from_raw_shim(-3_000_000)), pad("2", Nm::from_raw_shim(-1_000_000)), pad("3", Nm::from_raw_shim(1_000_000)), pad("4", Nm::from_raw_shim(3_000_000))], courtyard: Some((Nm::from_raw_shim(8_000_000), Nm::from_raw_shim(3_000_000))), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }
}

fn quad_part(r: &str) -> eda_model::Part {
    let mut q = part(r);
    q.package = Some("QUAD".into());
    q.footprint = Some("QUAD".into());
    q.pins = ["1", "2", "3", "4"].iter().map(|n| eda_model::Pin { number: (*n).into(), name: None, kind: eda_model::PinKind::Passive }).collect();
    q
}

/// U1 and U2 (QUAD) on a two-layer board; U1's pads are on N1 .. N4, U2's pad 1 and 2 on N1 and N2.
fn model() -> ConstraintModel {
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|p| p.to_string()).collect() };
    let mut m = ConstraintModel {
        parts: vec![quad_part("U1"), quad_part("U2")],
        nets: vec![net("N1", &["U1.1", "U2.1"]), net("N2", &["U1.2", "U2.2"]), net("N3", &["U1.3"]), net("N4", &["U1.4"])],
        footprints: vec![quad()],
        ..Default::default()
    };
    m.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
    m
}

fn nets_of(b: &Board<'_>, pin: &str) -> String {
    let nets = b.design().nets.clone().unwrap_or_default();
    nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default()
}

fn centre(d: &Design, m: &ConstraintModel, reference: &str, number: &str) -> Point {
    let fp = d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == reference).unwrap().clone();
    placed_pads(m, m.part(reference).unwrap(), &fp).unwrap().iter().find(|q| q.number == number).unwrap().center
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> (String, String) {
    let e = b.apply(&cmd).unwrap_err();
    (e[0].check.clone(), e[0].hint.clone().unwrap_or_default())
}

fn swap(ids: &[&str], connected_pads: Option<bool>) -> Cmd {
    Cmd::SwapPadNets { ids: ids.iter().map(|s| s.to_string()).collect(), connected_pads }
}

fn swap_gates(ids: &[&str], target: Option<&str>) -> Cmd {
    Cmd::SwapGateNets { ids: ids.iter().map(|s| s.to_string()).collect(), target: target.map(String::from), connected_pads: Some(true) }
}

/// The design of `pcb_transform_tests` with the nets of `model()` in `design.nets` (the list the swaps edit) and no copper of its own.
fn design_of(m: &ConstraintModel) -> Design {
    let mut d = design();
    d.nets = Some(m.nets.clone());
    let rt = d.routing.as_mut().unwrap();
    rt.tracks.clear();
    rt.vias.clear();
    rt.zones.clear();
    d
}

fn board_of(m: &ConstraintModel) -> Board<'_> {
    Board::new(design_of(m), m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000))
}

/// A track of `net` from one point to another, on the front.
fn track_of(id: &str, net: &str, a: Point, b: Point) -> eda_model::ir::Track {
    let mut t = super::pcb_transform_tests::track(id, "F.Cu", &[(a.x, a.y), (b.x, b.y)]);
    t.net = net.into();
    t
}

#[test]
fn the_nets_of_the_selected_pads_rotate_in_selection_order() {
    let m = model();
    let mut b = board_of(&m);
    b.apply(&swap(&["U1.1", "U1.2", "U1.3"], Some(true))).unwrap();
    // The pad takes the net the next one in the selection had; the last takes the first's.
    assert_eq!([nets_of(&b, "U1.1"), nets_of(&b, "U1.2"), nets_of(&b, "U1.3")], ["N2", "N3", "N1"]);
    // Nothing else moved: U1.4, and U2's pins (the copper does not reach them: no track is there).
    assert_eq!([nets_of(&b, "U1.4"), nets_of(&b, "U2.1"), nets_of(&b, "U2.2")], ["N4", "N1", "N2"]);
    // Two pads exchange.
    let mut b = board_of(&m);
    b.apply(&swap(&["U1.4", "U1.3"], Some(true))).unwrap();
    assert_eq!([nets_of(&b, "U1.3"), nets_of(&b, "U1.4")], ["N4", "N3"]);
}

#[test]
fn a_pad_with_no_net_takes_the_net_of_the_one_it_swaps_with_and_leaves_it_without() {
    let mut m = model();
    m.nets.iter_mut().find(|n| n.name == "N4").unwrap().pins.clear();
    let mut b = board_of(&m);
    b.apply(&swap(&["U1.3", "U1.4"], Some(true))).unwrap();
    assert_eq!([nets_of(&b, "U1.3"), nets_of(&b, "U1.4")], ["", "N3"]);
}

#[test]
fn tracks_and_vias_that_end_on_a_pad_take_its_new_net() {
    let m = model();
    let mut d = design_of(&m);
    let (a, c) = (centre(&d, &m, "U1", "1"), centre(&d, &m, "U1", "2"));
    let mid = p(a.x, a.y + Nm::from_raw_shim(3_000_000));
    {
        let rt = d.routing.as_mut().unwrap();
        // N1 leaves pad 1 and ends in a via; N2 leaves pad 2; a track of N1 that is nowhere near stays.
        rt.tracks.push(track_of("t_n1", "N1", a, mid));
        let mut v = super::pcb_transform_tests::via("v_n1", (mid.x, mid.y), "F.Cu", "B.Cu");
        v.net = "N1".into();
        rt.vias.push(v);
        rt.tracks.push(track_of("t_n2", "N2", c, p(c.x, c.y + Nm::from_raw_shim(2_000_000))));
        rt.tracks.push(track_of("t_far", "N1", p(Nm::from_raw_shim(80_000_000), Nm::from_raw_shim(80_000_000)), p(Nm::from_raw_shim(81_000_000), Nm::from_raw_shim(80_000_000))));
    }
    let mut b = Board::new(d, &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    b.apply(&swap(&["U1.1", "U1.2"], Some(true))).unwrap();
    let rt = b.design().routing.as_ref().unwrap();
    let net = |id: &str| rt.tracks.iter().find(|t| t.id == id).map(|t| t.net.clone()).or_else(|| rt.vias.iter().find(|v| v.id == id).map(|v| v.net.clone())).unwrap();
    assert_eq!([net("t_n1"), net("v_n1"), net("t_n2"), net("t_far")], ["N2", "N2", "N1", "N1"], "the copper that reaches a pad goes with it; a track that touches nothing stays");
}

#[test]
fn pads_the_copper_reaches_that_were_not_asked_for_are_asked_about() {
    let m = model();
    let mut d = design_of(&m);
    let (u1, u2) = (centre(&d, &m, "U1", "1"), centre(&d, &m, "U2", "1"));
    // U1.1 and U2.1 are on N1 and a track joins them.
    d.routing.as_mut().unwrap().tracks.push(track_of("t_link", "N1", u1, u2));
    // No answer: the swap is refused with the pads in its detail, and nothing changed.
    let mut b = Board::new(d.clone(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    let e = b.apply(&swap(&["U1.1", "U1.2"], None)).unwrap_err();
    assert_eq!(e[0].check, "ops_swap_unselected_pads");
    assert_eq!(e[0].detail.as_ref().unwrap()["pads"], serde_json::json!(["U2.1"]));
    assert!(e[0].hint.as_deref().unwrap().starts_with("1 unselected pad(s) are connected to these nets"));
    assert_eq!(nets_of(&b, "U1.1"), "N1");
    // "Ignore Unselected Pads": the track goes with the pad, the pad at its far end stays.
    let mut ignore = Board::new(d.clone(), &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    ignore.apply(&swap(&["U1.1", "U1.2"], Some(false))).unwrap();
    assert_eq!([nets_of(&ignore, "U1.1"), nets_of(&ignore, "U2.1")], ["N2", "N1"]);
    assert_eq!(ignore.design().routing.as_ref().unwrap().tracks[0].net, "N2");
    // "Swap All Connected Pads": the far pad changes too.
    b.apply(&swap(&["U1.1", "U1.2"], Some(true))).unwrap();
    assert_eq!([nets_of(&b, "U1.1"), nets_of(&b, "U2.1")], ["N2", "N2"]);
    assert_eq!(b.design().routing.as_ref().unwrap().tracks[0].net, "N2");
}

#[test]
fn a_swap_that_has_nothing_to_swap_or_names_no_pads_is_refused() {
    let m = model();
    let mut b = board_of(&m);
    // U1.1 and U2.1 are on the same net.
    assert_eq!(refusal(&mut b, swap(&["U1.1", "U2.1"], Some(true))).0, "ops_nothing_to_change");
    assert_eq!(refusal(&mut b, swap(&["U1.1"], Some(true))).0, "ops_bad_swap");
    assert_eq!(refusal(&mut b, swap(&[], Some(true))).0, "ops_bad_swap");
    assert_eq!(refusal(&mut b, swap(&["U1.1", "U1"], Some(true))).0, "ops_unknown_item", "a footprint is no pad");
    assert_eq!(refusal(&mut b, swap(&["U1.1", "U9.1"], Some(true))).0, "ops_unknown_item");
    assert_eq!(refusal(&mut b, swap(&["U1.1", "U1.9"], Some(true))).0, "ops_unknown_pad");
    // Nothing changed.
    assert_eq!(nets_of(&b, "U1.1"), "N1");
}

// ------------------------------------------------------------------------------------------------------------------------------ gates

/// The design of `design_of` with U1's gates: A is the pads 1 and 2, B the pads 3 and 4.
fn gate_design(m: &ConstraintModel) -> Design {
    let mut d = design_of(m);
    let units = vec![FootprintUnit { name: "A".into(), pins: vec!["1".into(), "2".into()] }, FootprintUnit { name: "B".into(), pins: vec!["3".into(), "4".into()] }];
    d.drawings.get_or_insert_with(Default::default).footprint_extras.push(FootprintExtra { id: "U1".into(), units, ..Default::default() });
    d
}

fn gate_board(m: &ConstraintModel) -> Board<'_> {
    Board::new(gate_design(m), m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000))
}

#[test]
fn swapping_two_gates_exchanges_their_nets_position_by_position() {
    let m = model();
    let mut b = gate_board(&m);
    b.apply(&swap_gates(&["U1.1", "U1.3"], None)).unwrap();
    // A's pad 1 and B's pad 3 are the first pins of each gate, 2 and 4 the second.
    assert_eq!([nets_of(&b, "U1.1"), nets_of(&b, "U1.2"), nets_of(&b, "U1.3"), nets_of(&b, "U1.4")], ["N3", "N4", "N1", "N2"]);
    // U2, not in the footprint, is as it was.
    assert_eq!([nets_of(&b, "U2.1"), nets_of(&b, "U2.2")], ["N1", "N2"]);
}

#[test]
fn one_selected_gate_is_swapped_with_the_unit_named() {
    let m = model();
    let mut b = gate_board(&m);
    b.apply(&swap_gates(&["U1.2"], Some("B"))).unwrap();
    assert_eq!([nets_of(&b, "U1.1"), nets_of(&b, "U1.3")], ["N3", "N1"]);
    // Without a unit to go to, one gate is not a swap; a unit that does not exist is none either.
    let mut b = gate_board(&m);
    assert_eq!(refusal(&mut b, swap_gates(&["U1.2"], None)).0, "ops_bad_swap");
    assert_eq!(refusal(&mut b, swap_gates(&["U1.2"], Some("Z"))).0, "ops_bad_swap");
}

#[test]
fn gate_swaps_are_refused_for_a_footprint_with_no_gates_pads_of_two_footprints_and_gates_of_other_sizes() {
    let m = model();
    // No units: U2 has none, and neither does U1 on a board nothing gave them to.
    let mut b = board_of(&m);
    let (_, why) = refusal(&mut b, swap_gates(&["U1.1", "U1.3"], None));
    assert_eq!(why, "Gate swapping must be performed on pads within one multi-gate footprint.");
    let mut b = gate_board(&m);
    assert_eq!(refusal(&mut b, swap_gates(&["U1.1", "U2.1"], None)).1, "Gate swapping must be performed on pads within one multi-gate footprint.");
    // Units of different sizes.
    let mut d = gate_design(&m);
    d.drawings.as_mut().unwrap().footprint_extras[0].units = vec![FootprintUnit { name: "A".into(), pins: vec!["1".into(), "2".into()] }, FootprintUnit { name: "B".into(), pins: vec!["3".into()] }];
    let mut uneven = Board::new(d, &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert_eq!(refusal(&mut uneven, swap_gates(&["U1.1", "U1.3"], None)).1, "Gate swapping must be performed on gates with equal pin counts.");
    // A unit with a pad the footprint lacks.
    let mut d = gate_design(&m);
    d.drawings.as_mut().unwrap().footprint_extras[0].units[1].pins[1] = "9".into();
    let mut missing = Board::new(d, &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    assert_eq!(refusal(&mut missing, swap_gates(&["U1.1", "U1.3"], None)).1, "Gate swapping failed: pad in unit missing from footprint.");
    // Gates with the same nets everywhere have nothing to exchange.
    let mut same = ConstraintModel { nets: vec![Net { name: "N1".into(), pins: vec!["U1.1".into(), "U1.3".into(), "U1.2".into(), "U1.4".into()] }], ..m.clone() };
    same.board.layers = m.board.layers.clone();
    let mut b = gate_board(&same);
    assert_eq!(refusal(&mut b, swap_gates(&["U1.1", "U1.3"], None)).1, "Gate swapping has no effect: all selected gates have identical nets.");
}

#[test]
fn the_units_of_a_footprint_come_from_its_symbol_when_the_board_gave_it_none() {
    use eda_model::symbol::{LibPin, SPoint};
    let mut m = model();
    let lib_pin = |n: &str, unit: u32, y: f64| LibPin { number: n.into(), name: String::new(), electrical_type: "passive".into(), shape: "line".into(), at: SPoint::new(0.0, y), angle_deg: 0.0, length_mm: 2.54, unit, hidden: false };
    let mut sym = eda_model::symbol::builtin("Device:R").unwrap();
    sym.lib_id = "Test:Dual".into();
    sym.unit_count = 2;
    sym.pins = vec![lib_pin("1", 1, 0.0), lib_pin("2", 1, 2.54), lib_pin("3", 2, 0.0), lib_pin("4", 2, 2.54)];
    sym.alternate = None;
    m.symbols.push(sym);
    m.parts[0].symbol = Some("Test:Dual".into());
    let mut b = board_of(&m);
    let units = b.unit_info_of("U1");
    assert_eq!(units, vec![FootprintUnit { name: "A".into(), pins: vec!["1".into(), "2".into()] }, FootprintUnit { name: "B".into(), pins: vec!["3".into(), "4".into()] }]);
    assert!(b.unit_info_of("U2").len() < 2, "U2's symbol has one unit");
    b.apply(&swap_gates(&["U1.2", "U1.4"], None)).unwrap();
    assert_eq!([nets_of(&b, "U1.1"), nets_of(&b, "U1.2"), nets_of(&b, "U1.3"), nets_of(&b, "U1.4")], ["N3", "N4", "N1", "N2"]);
}

#[test]
fn the_verbs_take_the_json_the_studio_sends() {
    let c: Cmd = serde_json::from_str(r#"{"op":"swap_pad_nets","ids":["U1.1","U1.2"]}"#).unwrap();
    assert!(matches!(c, Cmd::SwapPadNets { ref ids, connected_pads: None } if ids == &["U1.1".to_string(), "U1.2".to_string()]));
    let c: Cmd = serde_json::from_str(r#"{"op":"swap_gate_nets","ids":["U1.1"],"target":"B","connected_pads":false}"#).unwrap();
    assert!(matches!(c, Cmd::SwapGateNets { ref ids, target: Some(ref t), connected_pads: Some(false) } if ids.len() == 1 && t == "B"));
    let c: Cmd = serde_json::from_str(r#"{"op":"swap_gate_nets","ids":["U1.1","U1.3"]}"#).unwrap();
    assert!(matches!(c, Cmd::SwapGateNets { target: None, connected_pads: None, .. }));
}

#[test]
fn selected_pads_that_the_copper_joins_still_take_the_nets_they_were_given() {
    // U1.1 and U2.1 are on N1 and a track joins them; U1.3 is on N3. All three are asked for: U1.1 stays on N1 (it takes the net of U2.1), U2.1 takes N3, U1.3 takes N1.
    let m = model();
    let mut d = design_of(&m);
    let (a, b) = (centre(&d, &m, "U1", "1"), centre(&d, &m, "U2", "1"));
    d.routing.as_mut().unwrap().tracks.push(track_of("t_link", "N1", a, b));
    let mut board = Board::new(d, &m, Nm::from_raw_shim(100_000), Nm::from_raw_shim(300_000));
    board.apply(&swap(&["U1.1", "U2.1", "U1.3"], None)).unwrap();
    assert_eq!([nets_of(&board, "U1.1"), nets_of(&board, "U2.1"), nets_of(&board, "U1.3")], ["N1", "N3", "N1"], "no question: every pad of the copper is one that was asked for");
    // The track is in the cluster of both pads that join it: the last pad's new net wins, as the map of KiCad's tool is overwritten.
    assert_eq!(board.design().routing.as_ref().unwrap().tracks[0].net, "N3");
}
