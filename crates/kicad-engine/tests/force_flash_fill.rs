//! UI-LEFT P3: a zone's force-flash applied after kicad-cli's fill, and a solid zone connection (`ZONE_CONNECTION::FULL`) honoured by it.
//!
//! `ZONE_FILLER::Fill` (pcbnew/zone_filler.cpp at KiCad 8303b2ad) works out, before it fills, which layers of a pad or via that removes its unconnected layers a zone of
//! its own net covers, and sets `ZLO_FORCE_FLASHED` there (`ZLO_FORCE_NO_ZONE_CONNECTION` elsewhere); `PAD::FlashLayer` / `PCB_VIA::FlashLayer` then keep the ring on
//! those layers, and the board is saved with them as `(zone_layer_connections "In1.Cu")`. The studio's Fill All Zones (`refill_zones`) reads them back from kicad-cli's
//! saved board (`KicadFills::forced`), the studio's flashed layers take them (`flashed_layers_with_forced`), and the board written with the fill carries them, so
//! that a DRC that does not refill sees the flashing the fill made.
//!
//! And the fill honours a zone's "Solid" pad connection and a pad's own `ZONE_CONNECTION::FULL` (`ZONE::m_padConnection`, `PAD::GetLocalZoneConnection`): no thermal
//! relief is knocked out of the pour around such a pad, where a thermal one has its gap. Slow tier: kicad-cli fills the board. `EDA_SLOW_TESTS=1`.

use eda_kicad::import_kicad_pcb;
use eda_kicad_engine::{find_cli, refill_zones};

const BOARD: &str = r#"(kicad_pcb (version 20260901) (generator "pcbnew") (generator_version "10.99")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (1 "F.Mask" user) (3 "B.Mask" user) (13 "F.Paste" user) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (net 1 "A")
  (net 2 "B")
  (gr_rect (start 0 0) (end 60 40) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-0000000000e1"))
  (footprint "Lib:TH" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000011") (at 10 10 0)
    (property "Reference" "J1" (at 0 -2.5 0) (layer "F.SilkS") (hide yes) (uuid "00000000-0000-0000-0000-000000000012") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "TH" (at 0 2.5 0) (layer "F.Fab") (hide yes) (uuid "00000000-0000-0000-0000-000000000013") (effects (font (size 1 1) (thickness 0.15))))
    (attr through_hole)
    (pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask") (remove_unused_layers yes) (keep_end_layers yes) (zone_layer_connections) (net 1 "A") (uuid "00000000-0000-0000-0000-000000000014"))
  )
  (footprint "Lib:SMD" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000021") (at 40 10 0)
    (property "Reference" "J2" (at 0 -2.5 0) (layer "F.SilkS") (hide yes) (uuid "00000000-0000-0000-0000-000000000022") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "SMD" (at 0 2.5 0) (layer "F.Fab") (hide yes) (uuid "00000000-0000-0000-0000-000000000023") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (pad "1" smd rect (at -5 0) (size 2 2) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-000000000024"))
    (pad "2" smd rect (at 5 0) (size 2 2) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (zone_connect 2) (uuid "00000000-0000-0000-0000-000000000025"))
  )
  (via (at 20 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (remove_unused_layers yes) (keep_end_layers yes) (zone_layer_connections) (net 1) (uuid "00000000-0000-0000-0000-0000000000d1"))
  (zone (net 1) (net_name "A") (layer "In1.Cu") (uuid "00000000-0000-0000-0000-0000000000f1") (hatch edge 0.5)
    (connect_pads (clearance 0.3)) (min_thickness 0.25) (filled_areas_thickness no)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
    (polygon (pts (xy 2 2) (xy 28 2) (xy 28 18) (xy 2 18))))
  (zone (net 1) (net_name "A") (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000f2") (hatch edge 0.5)
    (connect_pads (clearance 0.3)) (min_thickness 0.25) (filled_areas_thickness no)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
    (polygon (pts (xy 31 2) (xy 49 2) (xy 49 18) (xy 31 18))))
  (zone (net 1) (net_name "A") (layer "F.Cu") (uuid "00000000-0000-0000-0000-0000000000f3") (hatch edge 0.5)
    (connect_pads yes (clearance 0.3)) (min_thickness 0.25) (filled_areas_thickness no)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
    (polygon (pts (xy 31 22) (xy 49 22) (xy 49 38) (xy 31 38))))
  (footprint "Lib:SMD" (layer "F.Cu") (uuid "00000000-0000-0000-0000-000000000031") (at 40 30 0)
    (property "Reference" "J3" (at 0 -2.5 0) (layer "F.SilkS") (hide yes) (uuid "00000000-0000-0000-0000-000000000032") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "SMD" (at 0 2.5 0) (layer "F.Fab") (hide yes) (uuid "00000000-0000-0000-0000-000000000033") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (pad "1" smd rect (at -5 0) (size 2 2) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "A") (uuid "00000000-0000-0000-0000-000000000034"))
  )
)"#;

/// The area (mm²) of a ring of nanometre points.
fn ring_area_mm2(pts: &[[i64; 2]]) -> f64 {
    let twice: i128 = (0..pts.len()).map(|i| pts[i][0] as i128 * pts[(i + 1) % pts.len()][1] as i128 - pts[(i + 1) % pts.len()][0] as i128 * pts[i][1] as i128).sum();
    (twice as f64 / 2.0).abs() / 1.0e12
}

#[test]
fn kicad_clis_fill_forces_the_flash_and_honours_a_solid_connection() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let (design, model, _) = import_kicad_pcb(BOARD).expect("import");
    let work = std::env::temp_dir().join(format!("eda_force_flash_{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let fills = refill_zones(&design, &model, &work).expect("kicad-cli fills the board");
    let via = design.routing.as_ref().unwrap().vias[0].id.clone();

    // 1. The pad and the via that remove their unconnected layers are forced to flash on In1.Cu, where the zone of their net covers them (not on In2.Cu,
    //    where none does; their ends are always kept, `keep_end_layers`).
    assert_eq!(fills.forced.get(&via), Some(&vec!["In1.Cu".to_string()]), "{:?}", fills.forced);
    assert_eq!(fills.forced.get("J1.1"), Some(&vec!["In1.Cu".to_string()]), "{:?}", fills.forced);
    let plain = eda_connectivity::flashed_layers(&design, &model);
    let forced = eda_connectivity::flashed_layers_with_forced(&design, &model, &fills.forced);
    assert!(!plain.vias[&via].contains(&"In1.Cu".to_string()), "nothing but the zone connects the via on In1.Cu: {:?}", plain.vias);
    assert_eq!(forced.vias[&via], vec!["F.Cu", "In1.Cu", "B.Cu"]);
    assert_eq!(forced.pads["J1.1"], vec!["F.Cu", "In1.Cu", "B.Cu"]);
    // 2. The board written with the fill carries the overrides, as KiCad's writer does.
    let written = eda_kicad::export_kicad_pcb_with(&design, &model, &eda_kicad::ExportMeta { date: "2026-11-06", title: "force" }, Some(&fills)).expect("export");
    let line = |needle: &str| written.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle}")).to_string();
    assert!(line("(via ").contains("(zone_layer_connections \"In1.Cu\")"), "{}", line("(via "));
    assert!(line("(pad \"1\" thru_hole").contains("(zone_layer_connections \"In1.Cu\")"), "{}", line("(pad \"1\" thru_hole"));

    // 3. The solid connections: the zone around J2 is thermal, but its pad 2 says FULL (`(zone_connect 2)`), so the pour is not cut around it as around pad 1;
    //    the zone around J3 is solid (`(connect_pads yes ..)`), so no gap is cut around its pad at all.
    let zone_area = |uuid_suffix: &str| -> f64 {
        let zone = design.routing.as_ref().unwrap().zones.iter().find(|z| z.uuid.ends_with(uuid_suffix)).expect("zone").clone();
        fills.zones.get(&zone.id).map(|e| e.layers.values().flatten().map(|r| ring_area_mm2(&r.pts)).sum()).unwrap_or(0.0)
    };
    let (thermal, solid) = (zone_area("0f2"), zone_area("0f3"));
    // The outline is 18 x 16 mm = 288 mm². A solid pad leaves the pour whole (its copper overlaps it); a thermal one costs its gap ring less the spokes.
    assert!((solid - 288.0).abs() < 0.5, "a Solid zone keeps all of its area: {solid}");
    // A thermal relief around a 2 mm pad with a 0.5 mm gap takes out the pad and its gap less the spokes: more than the gap's ring (9 - 4 = 5 mm² with square
    // corners, a little less rounded) and less than the whole 3 x 3 mm box. Pad 2, FULL, takes out nothing: so the zone loses one relief's worth, not two.
    let lost = 288.0 - thermal;
    assert!(lost > 4.0 && lost < 9.0, "one thermal pad and one FULL pad: {thermal} mm² ({lost} mm² taken out)");
    let _ = std::fs::remove_dir_all(&work);
}
