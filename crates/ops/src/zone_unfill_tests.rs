//! An unfill sticks (round 7 R7-Z1, `zone_unfill.rs`), and Zone Properties' display half (UI-LEFT P8): the commands count on the zone the edits that emptied
//! or flipped its fill, so a fill that `eda_kicad::zone_fills` keeps is the zone's only while the zone has the counts it was filled at.

use super::*;
use eda_kicad::zone_fills::{KicadFills, KicadRing, ZoneEntry};
use eda_model::ir::{PlacementSection, Provenance, ZoneBorderStyle};
use eda_model::setup_pages::{HatchOffset, HatchOffsets};
use eda_model::units::mm;
use eda_model::{Net, Part, Pin, PinKind};
use std::collections::BTreeMap;

fn model() -> ConstraintModel {
    let part = |r: &str| Part {
        reference: r.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some("0402".into()),
        footprint: None,
        pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }],
        body_nm: None,
        symbol: None,
        datasheet: None,
        edge: None,
    };
    ConstraintModel { parts: vec![part("C1"), part("C2")], nets: vec![Net { name: "GND".into(), pins: vec!["C1.1".into(), "C2.1".into()] }], ..Default::default() }
}

fn design() -> Design {
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: Some(PlacementSection { outline: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(100.0), y: Nm::ZERO }, Point { x: mm(100.0), y: mm(100.0) }, Point { x: Nm::ZERO, y: mm(100.0) }], footprints: Vec::new(), modules: Vec::new() }),
        drawings: None,
    }
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![Point { x: mm(x0), y: mm(y0) }, Point { x: mm(x1), y: mm(y0) }, Point { x: mm(x1), y: mm(y1) }, Point { x: mm(x0), y: mm(y1) }]
}

/// A GND pour on F.Cu, 10 mm square, and its id.
fn board_with_pour(m: &ConstraintModel) -> (Board<'_>, String) {
    let mut b = Board::new(design(), m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddZone { net: "GND".into(), layer: "F.Cu".into(), outline: square(10.0, 10.0, 20.0, 20.0) }).unwrap();
    let id = b.design().routing.as_ref().unwrap().zones[0].id.clone();
    (b, id)
}

fn zone<'b>(b: &'b Board<'_>, id: &str) -> &'b Zone {
    b.design().routing.as_ref().unwrap().zones.iter().find(|z| z.id == id).unwrap()
}

/// kicad-cli's fill of the zone as it is now: one ring on each of its layers.
fn filled(z: &Zone) -> KicadFills {
    let ring = KicadRing { island: false, pts: vec![[10_100_000, 10_100_000], [19_900_000, 10_100_000], [19_900_000, 19_900_000], [10_100_000, 19_900_000]] };
    let entry = ZoneEntry { layers: z.layer_set().into_iter().map(|l| (l.to_string(), vec![ring.clone()])).collect(), ..ZoneEntry::of(z) };
    KicadFills::new("kicad-cli test".into(), String::new(), 0, BTreeMap::from([(z.id.clone(), entry)]))
}

/// `Cmd::EditZone` with the zone's own settings, on `layers`.
fn edit_layers(z: &Zone, layers: &[&str]) -> Cmd {
    Cmd::EditZone {
        id: z.id.clone(),
        net: z.net.clone(),
        layer: layers[0].into(),
        layers: Some(layers.iter().map(|l| l.to_string()).collect()),
        clearance: z.clearance,
        min_thickness: z.min_thickness,
        thermal_gap: z.thermal_gap,
        thermal_spoke_width: z.thermal_spoke_width,
        pad_connection: z.pad_connection,
        priority: z.priority,
        island_removal_mode: z.island_removal_mode,
        min_island_area: z.min_island_area,
        fill_mode: z.fill_mode,
        hatch_thickness: z.hatch_thickness,
        hatch_gap: z.hatch_gap,
        hatch_orientation_mdeg: z.hatch_orientation_mdeg,
        hatch_smoothing_level: z.hatch_smoothing_level,
        hatch_smoothing_value: z.hatch_smoothing_value,
        hatch_hole_min_area: z.hatch_hole_min_area,
        hatch_border_algorithm: z.hatch_border_algorithm,
        is_rule_area: z.is_rule_area,
        keepout_tracks: false,
        keepout_vias: false,
        keepout_pads: false,
        keepout_copper_pour: false,
        keepout_footprints: false,
        smoothing: None,
        corner_radius: None,
        placement: None,
        border_style: None,
        border_hatch_pitch: None,
        hatch_offsets: None,
    }
}

/// R7-Z1: Zone Properties F.Cu -> B.Cu -> F.Cu. `ZONE::SetLayerSet` to another set threw the copper away (`unFillLocked()`, `m_FilledPolysList.clear()`), so the
/// zone back on F.Cu has no fill until the next one; the undo of the first edit (its snapshot, the zone as it was) gives the fill back, as KiCad's undo copy holds it.
#[test]
fn a_layer_change_and_back_leaves_the_zone_unfilled_and_undo_gives_the_fill_back() {
    let m = model();
    let (mut b, id) = board_with_pour(&m);
    let before = b.design().clone();
    let fills = filled(zone(&b, &id));
    assert!(fills.current(zone(&b, &id)).is_some());
    b.apply(&edit_layers(&zone(&b, &id).clone(), &["B.Cu"])).unwrap();
    assert!(fills.current(zone(&b, &id)).is_none(), "on another layer the zone has no fill");
    b.apply(&edit_layers(&zone(&b, &id).clone(), &["F.Cu"])).unwrap();
    let back = zone(&b, &id).clone();
    assert_eq!((back.layer.as_str(), back.outline.clone()), ("F.Cu", before.routing.as_ref().unwrap().zones[0].outline.clone()));
    assert!(fills.current(&back).is_none(), "the unfill sticks: the zone is back on F.Cu, the fill is not");
    assert_eq!(back.fill_epoch, 2);
    // Undo: the design as it was before the first edit.
    let undone = Board::new(before, &m, mm(0.1), mm(0.3));
    assert!(fills.current(zone(&undone, &id)).is_some_and(|e| !e.unfilled), "the undo copy holds the fill");
}

/// R7-Z1: a corner dragged away and back (the point editor's `m_zone.UnFill()` on each drag): no fill.
#[test]
fn a_corner_dragged_away_and_back_leaves_the_zone_unfilled() {
    let m = model();
    let (mut b, id) = board_with_pour(&m);
    let fills = filled(zone(&b, &id));
    let mut away = square(10.0, 10.0, 20.0, 20.0);
    away[2] = Point { x: mm(25.0), y: mm(22.0) };
    b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: away, holes: None }).unwrap();
    assert!(fills.current(zone(&b, &id)).is_none());
    b.apply(&Cmd::SetZoneOutline { id: id.clone(), outline: square(10.0, 10.0, 20.0, 20.0), holes: None }).unwrap();
    assert!(fills.current(zone(&b, &id)).is_none(), "the zone has its old outline again, not its old fill");
    // A move is no unfill: `ZONE::Move` moves the fill with the zone.
    let (mut b, id) = board_with_pour(&m);
    let fills = filled(zone(&b, &id));
    b.apply(&Cmd::MoveItems { ids: vec![id.clone()], dx: mm(5.0), dy: mm(1.0) }).unwrap();
    assert_eq!(zone(&b, &id).fill_epoch, 0);
    let moved = fills.current(zone(&b, &id)).expect("the fill moves with the zone");
    assert_eq!(moved.layers["F.Cu"][0].pts[0], [15_100_000, 11_100_000]);
}

/// R7-Z1: a one-sided zone flipped twice. `ZONE::Flip`'s `SetLayerSet( flipped )` clears `m_isFilled` and `SetFilledPolysList` puts the mirrored copper back,
/// so after the second flip the zone has its copper where it was and is still not filled: written without `(fill yes)`.
#[test]
fn a_zone_flipped_twice_keeps_its_copper_and_is_not_filled() {
    let m = model();
    let (mut b, id) = board_with_pour(&m);
    let fills = filled(zone(&b, &id));
    let flip = Cmd::FlipItems { ids: vec![id.clone()], pivot: Point { x: mm(50.0), y: Nm::ZERO }, direction: FlipDirection::LeftRight };
    b.apply(&flip).unwrap();
    let once = fills.current(zone(&b, &id)).expect("the copper goes with the flip");
    assert!(once.unfilled && once.layers.contains_key("B.Cu"));
    b.apply(&flip).unwrap();
    let z = zone(&b, &id).clone();
    assert_eq!((z.layer.as_str(), z.flip_epoch, z.fill_epoch), ("F.Cu", 2, 0));
    let twice = fills.current(&z).expect("still the copper");
    assert!(twice.unfilled, "a second flip does not make the zone filled again");
    assert_eq!(twice.layers["F.Cu"][0].pts[0], [10_100_000, 10_100_000]);
    // The derived board says so: `(fill (thermal_gap ..))`, no `yes`.
    let text = eda_kicad::export_kicad_pcb_with(b.design(), &m, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }, Some(&fills)).unwrap();
    let zone_text = &text[text.find("(zone").unwrap()..];
    assert!(zone_text.contains("(fill (thermal_gap") && !zone_text.contains("(fill yes"), "{zone_text}");
    assert!(zone_text.contains("(filled_polygon"), "the copper is written: {zone_text}");
}

/// Zone Properties' "Outline display", "Outline hatch pitch" and "Hatch offset overrides" (`PANEL_ZONE_PROPERTIES::AcceptOptions`): set by `Cmd::EditZone`, kept
/// by a command that leaves them out, the pitch held to 0.1..2 mm and the offsets to the zone's copper layers.
#[test]
fn edit_zone_sets_the_border_display_and_the_hatch_offsets() {
    let m = model();
    let (mut b, id) = board_with_pour(&m);
    let offsets: HatchOffsets = [("F.Cu".to_string(), HatchOffset { x_nm: mm(0.5), y_nm: Nm::ZERO })].into();
    let mut cmd = edit_layers(&zone(&b, &id).clone(), &["F.Cu"]);
    if let Cmd::EditZone { border_style, border_hatch_pitch, hatch_offsets, .. } = &mut cmd {
        *border_style = Some(ZoneBorderStyle::Full);
        *border_hatch_pitch = Some(mm(1.0));
        *hatch_offsets = Some(offsets.clone());
    }
    b.apply(&cmd).unwrap();
    let z = zone(&b, &id).clone();
    assert_eq!((z.border_style, z.border_hatch_pitch, z.hatch_offsets.clone()), (ZoneBorderStyle::Full, mm(1.0), offsets.clone()));
    // A setting edited without them keeps them.
    b.apply(&edit_layers(&z, &["F.Cu"])).unwrap();
    assert_eq!((zone(&b, &id).border_style, zone(&b, &id).border_hatch_pitch, zone(&b, &id).hatch_offsets.clone()), (ZoneBorderStyle::Full, mm(1.0), offsets));
    // Out of range, and an offset on a layer the zone is not on.
    for bad in [Some(mm(0.05)), Some(mm(2.5))] {
        let mut cmd = edit_layers(&z, &["F.Cu"]);
        if let Cmd::EditZone { border_hatch_pitch, .. } = &mut cmd {
            *border_hatch_pitch = bad;
        }
        assert_eq!(b.apply(&cmd).unwrap_err()[0].check, "ops_bad_zone");
    }
    let mut cmd = edit_layers(&z, &["F.Cu"]);
    if let Cmd::EditZone { hatch_offsets, .. } = &mut cmd {
        *hatch_offsets = Some([("B.Cu".to_string(), HatchOffset::default())].into());
    }
    assert_eq!(b.apply(&cmd).unwrap_err()[0].check, "ops_bad_zone");
    // On both layers, B.Cu may have one; back on F.Cu alone, it is dropped (`SetLayerSet`).
    let mut cmd = edit_layers(&z, &["F.Cu", "B.Cu"]);
    if let Cmd::EditZone { hatch_offsets, .. } = &mut cmd {
        *hatch_offsets = Some([("B.Cu".to_string(), HatchOffset::default()), ("F.Cu".to_string(), HatchOffset::default())].into());
    }
    b.apply(&cmd).unwrap();
    b.apply(&edit_layers(&zone(&b, &id).clone(), &["F.Cu"])).unwrap();
    assert_eq!(zone(&b, &id).hatch_offsets.keys().collect::<Vec<_>>(), ["F.Cu"]);
}

/// `ZONE::Flip` moves each layer's hatch offset to the other side (`m_layerProperties[ FlipLayer( oldLayer ) ]`); Swap Layers (`swapBoardItem`: `SetLayerSet`)
/// drops it.
#[test]
fn a_flip_moves_the_hatch_offsets_and_a_layer_swap_drops_them() {
    let m = model();
    let (mut b, id) = board_with_pour(&m);
    let mut cmd = edit_layers(&zone(&b, &id).clone(), &["F.Cu"]);
    if let Cmd::EditZone { hatch_offsets, .. } = &mut cmd {
        *hatch_offsets = Some([("F.Cu".to_string(), HatchOffset { x_nm: mm(0.2), y_nm: mm(0.3) })].into());
    }
    b.apply(&cmd).unwrap();
    b.apply(&Cmd::FlipItems { ids: vec![id.clone()], pivot: Point { x: mm(50.0), y: Nm::ZERO }, direction: FlipDirection::LeftRight }).unwrap();
    assert_eq!(zone(&b, &id).hatch_offsets.get("B.Cu"), Some(&HatchOffset { x_nm: mm(0.2), y_nm: mm(0.3) }));
    assert_eq!(zone(&b, &id).hatch_offsets.len(), 1);
    b.apply(&Cmd::SwapLayers { mapping: vec![("B.Cu".into(), "F.Cu".into())] }).unwrap();
    assert_eq!(zone(&b, &id).layer, "F.Cu");
    assert!(zone(&b, &id).hatch_offsets.is_empty());
}
