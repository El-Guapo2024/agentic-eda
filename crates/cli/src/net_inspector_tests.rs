//! The Net Inspector's numbers (`PCB_NET_INSPECTOR_PANEL::calculateNets`, `LENGTH_DELAY_CALCULATION::CalculateLengthDetails`).

use eda_model::units::{mm, Nm};
use super::*;
use eda_model::ir::{FootprintExtra, FootprintInstance, LabelSide, PadMaskInfo, PlacementSection, Provenance, RoutingSection, Side, Track, Via};
use eda_model::{Footprint, Net, Pad, PadShape, Part, Pin, PinKind};

const MM: i64 = 1_000_000;

fn p(x: f64, y: f64) -> Point {
    Point { x: mm(x), y: mm(y) }
}

fn pad(n: &str, x: Nm) -> Pad {
    Pad { opposite_side: false, ext: Default::default(), number: n.into(), at: (x, Nm::ZERO), size: (mm(0.8), mm(0.8)), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None }
}

fn part(r: &str, pins: usize) -> Part {
    Part { reference: r.into(), mpn: None, lcsc: None, value: None, package: Some("2PAD".into()), footprint: Some("2PAD".into()), pins: (1..=pins).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(), body_nm: None, symbol: None, datasheet: None, edge: None }
}

/// R1 at (10, 10) mm with both pads on `N` (pads at x 9 and 11 mm), U1 at (20, 10) with pad 1 on `N`.
fn model() -> ConstraintModel {
    let fp = Footprint { name: "2PAD".into(), pads: vec![pad("1", mm(-1.0)), pad("2", mm(1.0))], courtyard: Some((mm(1.5), mm(0.6))), ..Default::default() };
    let mut m = ConstraintModel { footprints: vec![fp], parts: vec![part("R1", 2), part("U1", 2)], ..Default::default() };
    m.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
    m.nets = vec![Net { name: "N".into(), pins: vec!["R1.1".into(), "R1.2".into(), "U1.1".into()] }, Net { name: "M".into(), pins: vec!["U1.2".into()] }];
    m
}

fn track(net: &str, layer: &str, pts: &[Point]) -> Track {
    Track { id: format!("trk_{}_{}", layer, pts.len()), net: net.into(), pins: vec![], layer: layer.into(), width: mm(0.2), pts: pts.to_vec(), arc_mid_offset: None }
}

fn design(tracks: Vec<Track>, vias: Vec<Via>) -> Design {
    let fp = |id: &str, at: Point| FootprintInstance { id: id.into(), at, rot: 0, side: Side::Top, label: LabelSide::Above };
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![p(0.0, 0.0), p(40.0, 0.0), p(40.0, 30.0), p(0.0, 30.0)], footprints: vec![fp("R1", p(10.0, 10.0)), fp("U1", p(20.0, 10.0))], modules: vec![] }),
        routing: Some(RoutingSection { tracks, vias, zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() }),
        drawings: None,
    }
}

fn via(at: Point) -> Via {
    Via { id: "via_a".into(), net: "N".into(), at, drill: mm(0.3), diameter: mm(0.6), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }
}

fn row<'a>(rows: &'a [Value], net: &str) -> &'a Value {
    rows.iter().find(|r| r["name"] == net).unwrap_or_else(|| panic!("no row for {net}"))
}

/// P11-1: the Pad Count is the pads of the net (two of R1's, one of U1's), not the footprints that have one.
#[test]
fn net_inspector_counts_pads() {
    let m = model();
    let rows = rows(&design(vec![], vec![]), &m);
    assert_eq!(row(&rows, "N")["pad_count"], 3);
    assert_eq!(row(&rows, "M")["pad_count"], 1);
    assert_eq!(row(&rows, "N")["netclass"], "Default");
    assert_eq!(row(&rows, "N")["total_length"], 0);
    // a net with no track, via or pad has no row (`calculateNets` only finds the nets of the connectivity items)
    let mut bare = model();
    bare.nets.push(Net { name: "EMPTY".into(), pins: vec![] });
    assert!(!rows_of(&design(vec![], vec![]), &bare).iter().any(|r| r["name"] == "EMPTY"));
}

/// The track length: the segments of a polyline merged into one line (`mergeLines`), its end on a pad's position and the one in a via measured to their
/// centres straight from where the line leaves them (`clipLineToPad`, `clipLineToVia`); a line that starts inside the via off its centre is measured from the
/// centre. The via's height counts, and the via, when the board uses the stackup height.
#[test]
fn the_lengths_are_kicads_merged_and_clipped_lines_and_the_stackup_height_of_the_vias() {
    let m = model();
    let tracks = vec![
        // from R1.2's centre (11, 10) to the via at (15, 12): 4 + 2 mm, its two ends clipped where they leave the pad and the via: the same length
        track("N", "F.Cu", &[p(11.0, 10.0), p(15.0, 10.0), p(15.0, 12.0)]),
        // on the back, from 0.2 mm off the via's centre: measured from the centre, 4 mm and not 3.8
        track("N", "B.Cu", &[p(15.2, 12.0), p(19.0, 12.0)]),
    ];
    let d = design(tracks, vec![via(p(15.0, 12.0))]);
    let rows = rows(&d, &m);
    let n = row(&rows, "N");
    let track = n["track_length"].as_i64().unwrap();
    assert!((track - 10 * MM).abs() <= 2, "{n}");
    assert_eq!(n["via_count"], 1);
    let stackup = eda_model::rules::default_stackup(2, m.board.board_thickness_nm);
    let height = eda_pns::tuning_path::stackup_distance(&stackup, &m.board.layers, "F.Cu", "B.Cu").get();
    assert!(height > MM, "{height}");
    assert_eq!(n["via_length"], height);
    assert_eq!(n["total_length"].as_i64().unwrap(), track + height);
    assert!((n["layer_lengths"]["F.Cu"].as_i64().unwrap() - 6 * MM).abs() <= 2);

    // without "Use stackup height for track length calculations" the via has no length and is not counted (KiCad counts it in that branch only)
    let mut flat = m.clone();
    flat.board.use_height_for_length_calcs = false;
    let rows = rows_of(&d, &flat);
    assert_eq!((row(&rows, "N")["via_count"].clone(), row(&rows, "N")["via_length"].clone()), (json!(0), json!(0)));
}

fn rows_of(d: &Design, m: &ConstraintModel) -> Vec<Value> {
    rows(d, m)
}

/// `optimiseVias`: a via that only lines on one layer end in is a stub, with no height.
#[test]
fn a_stub_via_has_no_height() {
    let m = model();
    let d = design(vec![track("N", "F.Cu", &[p(11.0, 10.0), p(15.0, 10.0)])], vec![via(p(15.0, 10.0))]);
    let rows = rows(&d, &m);
    assert_eq!(row(&rows, "N")["via_count"], 1);
    assert_eq!(row(&rows, "N")["via_length"], 0);
}

/// The Die Length is the `(die_length ..)` a board file's pad had, and joins the Total Length.
#[test]
fn the_die_length_is_the_pads_own() {
    let m = model();
    let mut d = design(vec![], vec![]);
    let pad_info = |tokens: Vec<String>| PadMaskInfo { tokens, ..Default::default() };
    d.drawings = Some(eda_model::ir::DrawingsSection {
        footprint_extras: vec![FootprintExtra { id: "R1".into(), pads: vec![pad_info(vec!["(die_length 1.5)".into()]), pad_info(vec![])], ..Default::default() }],
        ..Default::default()
    });
    let rows = rows(&d, &m);
    assert_eq!(row(&rows, "N")["die_length"], 1_500_000);
    assert_eq!(row(&rows, "N")["total_length"], 1_500_000);
    assert_eq!(die_length_of(&["(die_length 0.25)".to_string()]), mm(0.25));
}

#[test]
fn a_composite_net_class_has_kicads_human_readable_name() {
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(human_class_name(&names(&["Power"])), "Power");
    assert_eq!(human_class_name(&names(&["Power", "Default"])), "Power and Default");
    assert_eq!(human_class_name(&names(&["A", "B", "C"])), "A, B and C");
    assert_eq!(human_class_name(&names(&["A", "B", "C", "D"])), "A, B and 2 more");
}
