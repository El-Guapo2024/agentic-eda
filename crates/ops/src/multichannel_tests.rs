//! The multichannel tool: Generate Placement Rule Areas, Repeat Layout and Apply Design Block Layout, on a board of identical channels.
//!
//! Each channel is an amplifier U, an input resistor R and a feedback capacitor C on nets of their own, with the ground and the supply shared by all. Channel 0 is laid out
//! (placement, routing, a silkscreen mark); the others are scattered. The channels are known by the sheet their footprints say they are on, or by a group.

use super::*;
use eda_model::ir::{DrawingsSection, FootprintExtra, PlacementSection, Provenance, ZoneBorderStyle};
use eda_model::multichannel::{PlacementArea, PlacementSourceType};
use eda_model::{Footprint, Net, Pad, PadKind, PadShape, Part, Pin, PinKind};

fn p(x: Nm, y: Nm) -> Point {
    Point { x, y }
}

fn pad(n: &str, at: (Nm, Nm)) -> Pad {
    Pad { opposite_side: false, ext: Default::default(), number: n.into(), at, size: (600_000, 600_000), shape: PadShape::Rect, kind: PadKind::Smd, drill: None, drill_slot: None, rot: 0, roundrect_ratio: None }
}

fn footprints() -> Vec<Footprint> {
    vec![
        Footprint { name: "AMP5".into(), pads: vec![pad("1", (-1_500_000, -1_000_000)), pad("2", (-1_500_000, 0)), pad("3", (-1_500_000, 1_000_000)), pad("4", (1_500_000, -1_000_000)), pad("5", (1_500_000, 1_000_000))], courtyard: Some((2_500_000, 2_000_000)), ..Default::default() },
        Footprint { name: "R0603".into(), pads: vec![pad("1", (-800_000, 0)), pad("2", (800_000, 0))], courtyard: Some((1_500_000, 800_000)), ..Default::default() },
        Footprint { name: "C0603".into(), pads: vec![pad("1", (-800_000, 0)), pad("2", (800_000, 0))], courtyard: Some((1_500_000, 800_000)), ..Default::default() },
    ]
}

fn part(reference: &str, footprint: &str, pins: usize) -> Part {
    Part {
        reference: reference.into(),
        mpn: None,
        lcsc: None,
        value: None,
        package: Some(footprint.into()),
        footprint: Some(footprint.into()),
        pins: (1..=pins).map(|n| Pin { number: n.to_string(), name: None, kind: PinKind::Passive }).collect(),
        body_nm: None,
        symbol: None,
        datasheet: None,
        edge: None,
    }
}

/// Channel `c` (0-based) has U, R and C numbered from `c + 1`.
fn names(c: usize) -> (String, String, String) {
    (format!("U{}", c + 1), format!("R{}", c + 1), format!("C{}", c + 1))
}

pub(super) fn model(channels: usize) -> ConstraintModel {
    let mut m = ConstraintModel { footprints: footprints(), ..Default::default() };
    m.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
    let mut gnd: Vec<String> = Vec::new();
    let mut vcc: Vec<String> = Vec::new();
    for c in 0..channels {
        let (u, r, cap) = names(c);
        m.parts.push(part(&u, "AMP5", 5));
        m.parts.push(part(&r, "R0603", 2));
        m.parts.push(part(&cap, "C0603", 2));
        gnd.push(format!("{u}.2"));
        vcc.push(format!("{u}.5"));
        m.nets.push(Net { name: format!("IN{c}"), pins: vec![format!("{u}.1"), format!("{r}.1")] });
        m.nets.push(Net { name: format!("MID{c}"), pins: vec![format!("{u}.3"), format!("{r}.2"), format!("{cap}.1")] });
        m.nets.push(Net { name: format!("OUT{c}"), pins: vec![format!("{u}.4"), format!("{cap}.2")] });
    }
    m.nets.push(Net { name: "GND".into(), pins: gnd });
    m.nets.push(Net { name: "VCC".into(), pins: vcc });
    m
}

fn track(id: &str, net: &str, layer: &str, pts: &[(Nm, Nm)]) -> Track {
    Track { id: id.into(), net: net.into(), pins: vec![], layer: layer.into(), width: 200_000, pts: pts.iter().map(|&(x, y)| p(x, y)).collect(), arc_mid_offset: None }
}

fn pose(id: &str, at: (Nm, Nm), rot: u32) -> FootprintInstance {
    FootprintInstance { id: id.into(), at: p(at.0, at.1), rot, side: Side::Top, label: LabelSide::Above }
}

/// `channels` channels: channel 0 laid out in a row at y = 20 mm with its routing, a silkscreen mark and a text; channel `c` scattered 30 mm below the one before it.
/// Every footprint says what sheet it is on (`/ch1/`, `/ch2/` ...), as a board read from a KiCad file does.
pub(super) fn design(channels: usize) -> Design {
    let mut footprints_: Vec<FootprintInstance> = Vec::new();
    let mut extras: Vec<FootprintExtra> = Vec::new();
    for c in 0..channels {
        let (u, r, cap) = names(c);
        let oy = 20_000_000 + 30_000_000 * c as Nm;
        if c == 0 {
            footprints_.push(pose(&u, (30_000_000, oy), 0));
            footprints_.push(pose(&r, (22_000_000, oy), 90_000));
            footprints_.push(pose(&cap, (38_000_000, oy), 0));
        } else {
            // scattered: other places, other turns, one on the back
            footprints_.push(pose(&u, (31_000_000 + 700_000 * c as Nm, oy + 900_000), 180_000));
            footprints_.push(pose(&r, (21_000_000, oy - 1_500_000), 0));
            let mut f = pose(&cap, (40_000_000, oy + 1_000_000), 270_000);
            f.side = Side::Top;
            footprints_.push(f);
        }
        for reference in [&u, &r, &cap] {
            extras.push(FootprintExtra { id: reference.clone(), sheetname: Some(format!("/ch{}/", c + 1)), sheetfile: Some("channel.kicad_sch".into()), ..Default::default() });
        }
    }
    footprints_.sort_by(|a, b| a.id.cmp(&b.id));
    // channel 0's copper: a track from the resistor to the capacitor, one from the amplifier's pin 4 to the capacitor, a via on the middle net; channel 1 has a track of its
    // own in the way that the copy replaces
    let mut tracks = vec![
        track("trk_mid0", "MID0", "F.Cu", &[(23_000_000, 20_000_000), (28_000_000, 20_000_000), (28_000_000, 21_500_000), (36_000_000, 21_500_000)]),
        track("trk_out0", "OUT0", "F.Cu", &[(33_000_000, 20_500_000), (37_000_000, 20_500_000)]),
        track("trk_gnd0", "GND", "F.Cu", &[(28_000_000, 20_500_000), (28_000_000, 19_000_000)]),
    ];
    let mut vias = vec![Via { id: "via_mid0".into(), net: "MID0".into(), at: p(28_000_000, 21_500_000), drill: 300_000, diameter: 600_000, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }];
    if channels > 1 {
        tracks.push(track("trk_old1", "MID1", "F.Cu", &[(30_500_000, 52_500_000), (32_500_000, 52_500_000)]));
        vias.push(Via { id: "via_old1".into(), net: "MID1".into(), at: p(33_000_000, 52_500_000), drill: 300_000, diameter: 600_000, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() });
    }
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        placement: Some(PlacementSection { outline: vec![p(0, 0), p(100_000_000, 0), p(100_000_000, 100_000_000), p(0, 100_000_000)], footprints: footprints_, modules: vec![] }),
        routing: Some(RoutingSection { tracks, vias, zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
        drawings: Some(DrawingsSection {
            shapes: vec![
                Shape::Segment { id: "shp_mark0".into(), layer: "F.SilkS".into(), stroke_width: 150_000, filled: false, start: p(24_000_000, 18_500_000), end: p(26_000_000, 18_500_000) },
                Shape::Segment { id: "shp_cu0".into(), layer: "F.Cu".into(), stroke_width: 200_000, filled: false, start: p(33_000_000, 22_000_000), end: p(35_000_000, 22_000_000) },
            ],
            texts: vec![Text { id: "txt_ch0".into(), content: "CH1".into(), at: p(30_000_000, 19_500_000), angle: 0, layer: "F.SilkS".into(), size_nm: 800_000, stroke_width: 120_000, justify: eda_model::ir::TextJustify::Center, mirror: false }],
            footprint_extras: extras,
            ..Default::default()
        }),
    }
}

fn board(m: &ConstraintModel, channels: usize) -> Board<'_> {
    Board::new(design(channels), m, 100_000, 300_000)
}

fn routing<'a>(b: &'a Board<'_>) -> &'a RoutingSection {
    b.design().routing.as_ref().unwrap()
}

fn drawings<'a>(b: &'a Board<'_>) -> &'a DrawingsSection {
    b.design().drawings.as_ref().unwrap()
}

/// A footprint's pose, to compare (`FootprintInstance` has no equality of its own).
#[derive(Debug, Clone, PartialEq)]
struct Pose {
    at: Point,
    rot: u32,
    side: Side,
    label: LabelSide,
}

fn pose_of(b: &Board<'_>, id: &str) -> Pose {
    let f = b.design().placement.as_ref().unwrap().footprints.iter().find(|f| f.id == id).unwrap();
    Pose { at: f.at, rot: f.rot, side: f.side, label: f.label }
}

fn rule_areas(b: &Board<'_>) -> Vec<Zone> {
    routing(b).zones.iter().filter(|z| z.is_rule_area && z.placement.enabled).cloned().collect()
}

fn generate(picks: &[(AreaKind, &str)], replace: bool, group: bool) -> Cmd {
    Cmd::Multichannel(MultichannelCmd::GenerateRuleAreas { areas: picks.iter().map(|(kind, key)| AreaPick { kind: *kind, key: key.to_string() }).collect(), replace_existing: replace, group_items: group })
}

fn sheets(n: usize) -> Vec<(AreaKind, String)> {
    (1..=n).map(|c| (AreaKind::Sheet, format!("/ch{c}/"))).collect()
}

fn generate_sheets(b: &mut Board<'_>, n: usize) {
    let picks = sheets(n);
    let refs: Vec<(AreaKind, &str)> = picks.iter().map(|(k, s)| (*k, s.as_str())).collect();
    b.apply(&generate(&refs, false, false)).unwrap();
}

fn zone_named<'a>(b: &'a Board<'_>, name: &str) -> &'a Zone {
    routing(b).zones.iter().find(|z| z.name == name).unwrap_or_else(|| panic!("no zone named {name}"))
}

fn centre(z: &Zone) -> Point {
    eda_model::multichannel::outline_centre(&z.outline).unwrap()
}

// ------------------------------------------------------------------------------------------------------------------------------ Generate

#[test]
fn the_potential_areas_are_a_row_for_each_sheet_class_and_group_with_the_footprints_each_holds() {
    let m = model(3);
    let mut d = design(3);
    // a footprint on the root sheet, another that says nothing of its sheet, and one in a group
    d.drawings.as_mut().unwrap().footprint_extras.iter_mut().find(|e| e.id == "C3").unwrap().sheetname = Some("/".into());
    d.drawings.as_mut().unwrap().footprint_extras.iter_mut().find(|e| e.id == "R3").unwrap().component_classes = vec!["Passives".into()];
    d.drawings.as_mut().unwrap().footprint_extras.iter_mut().find(|e| e.id == "R2").unwrap().component_classes = vec!["Passives".into()];
    let mut b = Board::new(d, &m, 100_000, 300_000);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "R1".into()] }).unwrap();
    let gid = drawings(&b).groups[0].id.clone();
    b.apply(&Cmd::EditGroup { id: gid, name: "ch1 core".into(), member_ids: vec!["U1".into(), "R1".into()], design_block: None }).unwrap();

    let areas = b.mc_potential_areas();
    let sheet_keys: Vec<(&str, &str)> = areas.iter().filter(|a| a.kind == AreaKind::Sheet).map(|a| (a.key.as_str(), a.file.as_str())).collect();
    assert_eq!(sheet_keys, vec![("/", "channel.kicad_sch"), ("/ch1/", "channel.kicad_sch"), ("/ch2/", "channel.kicad_sch"), ("/ch3/", "channel.kicad_sch")]);
    let comps = |kind: AreaKind, key: &str| areas.iter().find(|a| a.kind == kind && a.key == key).map(|a| a.components.clone()).unwrap();
    // the root sheet holds everything (queryComponentsInSheet: every path starts with "/"), a channel its own three
    assert_eq!(comps(AreaKind::Sheet, "/").len(), 9);
    assert_eq!(comps(AreaKind::Sheet, "/ch2/"), vec!["C2", "R2", "U2"]);
    assert_eq!(comps(AreaKind::ComponentClass, "Passives"), vec!["R2", "R3"]);
    assert_eq!(comps(AreaKind::Group, "ch1 core"), vec!["R1", "U1"]);
    // the Placement tab's choices
    let s = b.mc_sources();
    assert_eq!(s.groups, vec!["ch1 core"]);
    assert_eq!(s.classes, vec!["Passives"]);
    assert_eq!(s.sheets, vec!["/", "/ch1/", "/ch2/", "/ch3/"]);
}

#[test]
fn generate_makes_a_rule_area_for_each_sheet_ticked_from_the_boxes_of_its_footprints() {
    let m = model(3);
    let mut b = board(&m, 3);
    generate_sheets(&mut b, 3);
    let zones = rule_areas(&b);
    assert_eq!(zones.len(), 3);
    let z = zone_named(&b, "auto-placement-area-/ch1/");
    assert!(z.is_rule_area && z.placement.enabled);
    assert_eq!((z.placement.source_type, z.placement.source.as_str()), (PlacementSourceType::Sheetname, "/ch1/"));
    // all the copper layers, no keepout, no outline hatch, no net
    assert_eq!(z.layer_set(), vec!["F.Cu", "B.Cu"]);
    assert!(!z.keepout_tracks && !z.keepout_vias && !z.keepout_pads && !z.keepout_copper_pour && !z.keepout_footprints);
    assert_eq!(z.border_style, ZoneBorderStyle::None);
    assert!(z.net.is_empty());
    // a rectangle round the three footprints of the row, 0.1 mm clear of their boxes: the courtyards (±2.5 mm × ±2 mm for the amplifier at x = 30 mm; the resistor
    // turned a quarter, ±0.8 × ±1.5 mm at x = 22 mm; the capacitor at x = 38 mm) grown by the stroke of the courtyard (25 um) and the margin
    let bounds = eda_model::multichannel::polygon_box(&z.outline).unwrap();
    assert_eq!(bounds, (22_000_000 - 800_000 - 25_000 - 100_000, 20_000_000 - 2_000_000 - 25_000 - 100_000, 38_000_000 + 1_500_000 + 25_000 + 100_000, 20_000_000 + 2_000_000 + 25_000 + 100_000));
    // the hull of the three boxes, rectified as KiCad does it: a staircase, because the resistor and the capacitor are lower than the amplifier
    assert_eq!(z.outline.len(), 9, "{:?}", z.outline);
    // the second channel's, from where its footprints are
    let z2 = zone_named(&b, "auto-placement-area-/ch2/");
    assert_ne!(centre(z2), centre(z));
    // the rule area is the zone's own: it is in the file as KiCad reads it
    let text = eda_kicad::export_kicad_pcb(b.design(), &m, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(text.contains("(placement (enabled yes) (sheetname \"/ch1/\"))"), "{}", text.lines().filter(|l| l.contains("placement")).collect::<Vec<_>>().join("\n"));
}

#[test]
fn generate_skips_the_areas_that_are_there_already_unless_asked_to_replace_them() {
    let m = model(2);
    let mut b = board(&m, 2);
    generate_sheets(&mut b, 2);
    let before: Vec<String> = rule_areas(&b).iter().map(|z| z.id.clone()).collect();
    assert_eq!(before.len(), 2);
    // the same footprints are in an area already: nothing to make
    let picks = sheets(2);
    let refs: Vec<(AreaKind, &str)> = picks.iter().map(|(k, s)| (*k, s.as_str())).collect();
    assert_eq!(b.apply(&generate(&refs, false, false)).unwrap_err()[0].check, "ops_no_rule_areas");
    assert_eq!(rule_areas(&b).len(), 2);
    // asked to replace, they are made again and the old ones are gone
    b.apply(&generate(&refs, true, false)).unwrap();
    let after = rule_areas(&b);
    assert_eq!(after.len(), 2);
    // an area nobody asked for
    assert_eq!(b.apply(&generate(&[], false, false)).unwrap_err()[0].check, "ops_no_rule_areas");
    assert_eq!(b.apply(&generate(&[(AreaKind::Sheet, "/nowhere/")], false, false)).unwrap_err()[0].check, "ops_unknown_area");
}

#[test]
fn generate_can_group_each_area_with_its_footprints() {
    let m = model(2);
    let mut b = board(&m, 2);
    // the amplifier of channel 1 is in a group already with something else, which falls apart when the footprint leaves it
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "shp_mark0".into()] }).unwrap();
    let picks = sheets(2);
    let refs: Vec<(AreaKind, &str)> = picks.iter().map(|(k, s)| (*k, s.as_str())).collect();
    b.apply(&generate(&refs, false, true)).unwrap();
    let groups = &drawings(&b).groups;
    assert_eq!(groups.len(), 2, "{groups:?}");
    let ch1 = zone_named(&b, "auto-placement-area-/ch1/").id.clone();
    let g1 = groups.iter().find(|g| g.member_ids.contains(&ch1)).unwrap();
    let mut members = g1.member_ids.clone();
    members.sort();
    let mut want = vec![ch1, "C1".to_string(), "R1".to_string(), "U1".to_string()];
    want.sort();
    assert_eq!(members, want);
    assert!(!groups.iter().any(|g| g.member_ids.contains(&"shp_mark0".to_string())), "the old group dissolved: it had one item left");
}

#[test]
fn generate_with_replace_and_group_groups_the_areas_made_again_over_the_same_footprints() {
    // a zone's id is made from its net, layer and outline, so an area made again over the same footprints has the id of the one it replaces: it must still be grouped
    let m = model(2);
    let mut b = board(&m, 2);
    generate_sheets(&mut b, 2);
    let ids: Vec<String> = rule_areas(&b).iter().map(|z| z.id.clone()).collect();
    let picks = sheets(2);
    let refs: Vec<(AreaKind, &str)> = picks.iter().map(|(k, s)| (*k, s.as_str())).collect();
    b.apply(&generate(&refs, true, true)).unwrap();
    assert_eq!(rule_areas(&b).iter().map(|z| z.id.clone()).collect::<Vec<_>>(), ids, "the same areas, the same ids");
    let groups = &drawings(&b).groups;
    assert_eq!(groups.len(), 2, "{groups:?}");
    for id in &ids {
        let g = groups.iter().find(|g| g.member_ids.contains(id)).unwrap();
        assert_eq!(g.member_ids.len(), 4, "the area and the three footprints of its channel: {g:?}");
    }
    // and again: the groups of the first run are not left behind
    b.apply(&generate(&refs, true, true)).unwrap();
    assert_eq!(drawings(&b).groups.len(), 2);
}

#[test]
fn generate_from_a_group_makes_the_area_from_everything_in_the_group() {
    let m = model(2);
    let mut b = board(&m, 2);
    b.apply(&Cmd::Group { ids: vec!["U1".into(), "R1".into(), "C1".into(), "shp_mark0".into(), "txt_ch0".into()] }).unwrap();
    let gid = drawings(&b).groups[0].id.clone();
    b.apply(&Cmd::EditGroup { id: gid, name: "A".into(), member_ids: ["U1", "R1", "C1", "shp_mark0", "txt_ch0"].iter().map(|s| s.to_string()).collect(), design_block: None }).unwrap();
    b.apply(&generate(&[(AreaKind::Group, "A")], false, false)).unwrap();
    let z = zone_named(&b, "auto-placement-area-A");
    assert_eq!((z.placement.source_type, z.placement.source.as_str()), (PlacementSourceType::Group, "A"));
    // the text sticks out above the footprints (y = 17.8 mm - half its height), so the area does too
    let bounds = eda_model::multichannel::polygon_box(&z.outline).unwrap();
    assert!(bounds.1 <= 19_500_000 - 400_000 - 100_000 && bounds.1 < 20_000_000 - 2_000_000 - 25_000 - 100_000 + 1, "{bounds:?}");
}

#[test]
fn the_placement_of_a_rule_area_is_edited_with_the_zone_and_only_a_rule_area_has_one() {
    let m = model(1);
    let mut b = board(&m, 1);
    b.apply(&Cmd::AddZone { net: String::new(), layer: "F.Cu".into(), outline: vec![p(0, 0), p(10_000_000, 0), p(10_000_000, 10_000_000)] }).unwrap();
    let id = routing(&b).zones[0].id.clone();
    let edit = |placement: Option<PlacementArea>, rule: bool| -> Cmd {
        let z = Zone { id: id.clone(), net: String::new(), layer: "F.Cu".into(), is_rule_area: rule, ..Default::default() };
        Cmd::EditZone {
            id: z.id.clone(),
            net: z.net.clone(),
            layer: z.layer.clone(),
            layers: None,
            clearance: z.clearance,
            min_thickness: z.min_thickness,
            thermal_gap: z.thermal_gap,
            thermal_spoke_width: z.thermal_spoke_width,
            pad_connection: z.pad_connection,
            priority: 0,
            island_removal_mode: z.island_removal_mode,
            min_island_area: z.min_island_area,
            fill_mode: z.fill_mode,
            hatch_thickness: z.hatch_thickness,
            hatch_gap: z.hatch_gap,
            hatch_orientation_mdeg: 0,
            hatch_smoothing_level: 0,
            hatch_smoothing_value: z.hatch_smoothing_value,
            hatch_hole_min_area: z.hatch_hole_min_area,
            hatch_border_algorithm: z.hatch_border_algorithm,
            is_rule_area: rule,
            keepout_tracks: false,
            keepout_vias: false,
            keepout_pads: false,
            keepout_copper_pour: false,
            keepout_footprints: false,
            smoothing: None,
            corner_radius: None,
            placement,
        }
    };
    let wanted = PlacementArea { enabled: true, source_type: PlacementSourceType::ComponentClass, source: "Passives".into() };
    b.apply(&edit(Some(wanted.clone()), true)).unwrap();
    assert_eq!(routing(&b).zones[0].placement, wanted);
    // an edit that says nothing of it leaves it; a zone that is no rule area any more has none
    b.apply(&edit(None, true)).unwrap();
    assert_eq!(routing(&b).zones[0].placement, wanted);
    b.apply(&edit(None, false)).unwrap();
    assert!(routing(&b).zones[0].placement.is_default());
}

// ---------------------------------------------------------------------------------------------------------------------------- Repeat Layout

/// The rule area of each channel, generated by sheet, and channel 0 the reference: ids (reference, then the others in order).
fn areas_of(b: &Board<'_>, n: usize) -> Vec<String> {
    (1..=n).map(|c| zone_named(b, &format!("auto-placement-area-/ch{c}/")).id.clone()).collect()
}

fn repeat(reference: &str, targets: &[String], options: RepeatOptions) -> Cmd {
    Cmd::Multichannel(MultichannelCmd::RepeatLayout { reference: reference.into(), targets: targets.to_vec(), options })
}

#[test]
fn identical_channels_match_part_for_part_and_the_dialog_lists_every_other_area() {
    let m = model(3);
    let mut b = board(&m, 3);
    generate_sheets(&mut b, 3);
    let ids = areas_of(&b, 3);
    let plan = b.mc_compat(&ids[0]).unwrap();
    assert_eq!(plan.components, vec!["C1", "R1", "U1"]);
    assert_eq!(plan.targets.len(), 2);
    for t in &plan.targets {
        assert!(t.ok, "{t:?}");
        assert_eq!(t.message, "OK");
        assert_eq!(t.pairs.len(), 3);
    }
    let t2 = plan.targets.iter().find(|t| t.id == ids[1]).unwrap();
    assert!(t2.pairs.contains(&("U1".into(), "U2".into())) && t2.pairs.contains(&("R1".into(), "R2".into())) && t2.pairs.contains(&("C1".into(), "C2".into())), "{:?}", t2.pairs);
    // a zone that is no placement rule area is refused
    assert_eq!(b.mc_compat("nothing").unwrap_err()[0].check, "ops_not_placement_area");
}

#[test]
fn a_channel_that_is_wired_differently_does_not_match_and_says_why() {
    let mut m = model(2);
    // channel 2's resistor sits on a net of its own: it is not the channel 1 resistor
    m.nets.iter_mut().find(|n| n.name == "IN1").unwrap().pins = vec!["U2.1".into()];
    m.nets.push(Net { name: "STRAY".into(), pins: vec!["R2.1".into()] });
    let mut b = board(&m, 2);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    let plan = b.mc_compat(&ids[0]).unwrap();
    let t = &plan.targets[0];
    assert!(!t.ok);
    assert!(!t.message.is_empty() && t.message != "OK", "{t:?}");
    assert!(t.pairs.is_empty());
    // the verb leaves a target that does not match alone, and refuses when none does
    let before = b.design().clone();
    let err = b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions::default())).unwrap_err();
    assert_eq!(err[0].check, "ops_topology_mismatch");
    assert!(err[0].hint.as_deref().unwrap().starts_with("Rule Area topologies do not match: "), "{err:?}");
    assert_eq!(serde_json::to_value(b.design()).unwrap(), serde_json::to_value(&before).unwrap(), "nothing was changed");
}

#[test]
fn a_different_number_of_footprints_lists_both_sets_in_the_reasons() {
    let m = model(2);
    let mut d = design(2);
    // channel 2 has its capacitor on another sheet
    d.drawings.as_mut().unwrap().footprint_extras.iter_mut().find(|e| e.id == "C2").unwrap().sheetname = Some("/elsewhere/".into());
    let mut b = Board::new(d, &m, 100_000, 300_000);
    b.apply(&generate(&[(AreaKind::Sheet, "/ch1/"), (AreaKind::Sheet, "/ch2/")], false, false)).unwrap();
    let ids = areas_of(&b, 2);
    let t = b.mc_compat(&ids[0]).unwrap().targets.remove(0);
    assert!(!t.ok);
    assert!(t.reasons.iter().any(|r| r == "Component count mismatch"), "{:?}", t.reasons);
    assert!(t.reasons.iter().any(|r| r == "Reference area total components: 3"), "{:?}", t.reasons);
    assert!(t.reasons.iter().any(|r| r.starts_with("Target area components:\nR2, U2")), "{:?}", t.reasons);
}

#[test]
fn repeat_layout_places_the_footprints_of_the_other_channels_as_the_first_one() {
    let m = model(3);
    let mut b = board(&m, 3);
    generate_sheets(&mut b, 3);
    let ids = areas_of(&b, 3);
    let z0 = zone_named(&b, "auto-placement-area-/ch1/").clone();
    let z1 = zone_named(&b, "auto-placement-area-/ch2/").clone();
    let disp = Point { x: centre(&z1).x - centre(&z0).x, y: centre(&z1).y - centre(&z0).y };
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions::default())).unwrap();
    for (a, c) in [("U1", "U2"), ("R1", "R2"), ("C1", "C2")] {
        let (pa, pc) = (pose_of(&b, a), pose_of(&b, c));
        assert_eq!((pc.rot, pc.side, pc.label), (pa.rot, pa.side, pa.label), "{c}");
        assert_eq!(pc.at, p(pa.at.x + disp.x, pa.at.y + disp.y), "{c} is where {a} is, by the move of the centres");
    }
    // channel 3 is a further 30 mm... its own distance from the first
    let z2 = zone_named(&b, "auto-placement-area-/ch3/").clone();
    let disp3 = Point { x: centre(&z2).x - centre(&z0).x, y: centre(&z2).y - centre(&z0).y };
    // (the areas moved with the copy: the reference's shape at the target)
    let after = zone_named(&b, "auto-placement-area-/ch2/");
    assert_eq!(after.outline, z0.outline.iter().map(|q| p(q.x + disp.x, q.y + disp.y)).collect::<Vec<_>>());
    assert_eq!(pose_of(&b, "U3").at, p(pose_of(&b, "U1").at.x + disp3.x, pose_of(&b, "U1").at.y + disp3.y));
    // the reference stayed
    assert_eq!(pose_of(&b, "U1").at, p(30_000_000, 20_000_000));
}

#[test]
fn repeat_layout_copies_the_routing_onto_the_nets_of_the_target_and_takes_the_old_routing_away() {
    let m = model(2);
    let mut b = board(&m, 2);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    let z0 = zone_named(&b, "auto-placement-area-/ch1/").clone();
    let z1 = zone_named(&b, "auto-placement-area-/ch2/").clone();
    let disp = Point { x: centre(&z1).x - centre(&z0).x, y: centre(&z1).y - centre(&z0).y };
    // channel 2's old copper is in its area
    assert!(routing(&b).tracks.iter().any(|t| t.id == "trk_old1"));
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions::default())).unwrap();
    let rt = routing(&b);
    assert!(!rt.tracks.iter().any(|t| t.id == "trk_old1") && !rt.vias.iter().any(|v| v.id == "via_old1"), "the old routing of the target went");
    // channel 1's three tracks are each copied, on the nets the pads of channel 2 are on
    let nets_of = |layer: &str| -> Vec<(String, Vec<Point>)> { rt.tracks.iter().filter(|t| t.layer == layer).map(|t| (t.net.clone(), t.pts.clone())).collect() };
    let copies = nets_of("F.Cu");
    let shifted = |t: &Track| t.pts.iter().map(|q| p(q.x + disp.x, q.y + disp.y)).collect::<Vec<_>>();
    let mid = rt.tracks.iter().find(|t| t.id == "trk_mid0").unwrap();
    assert!(copies.contains(&("MID1".to_string(), shifted(mid))), "{copies:?}");
    let out = rt.tracks.iter().find(|t| t.id == "trk_out0").unwrap();
    assert!(copies.contains(&("OUT1".to_string(), shifted(out))), "{copies:?}");
    // a net both channels share (GND) is the same net
    let gnd = rt.tracks.iter().find(|t| t.id == "trk_gnd0").unwrap();
    assert!(copies.contains(&("GND".to_string(), shifted(gnd))), "{copies:?}");
    // the via, on its net
    let via = rt.vias.iter().find(|v| v.id == "via_mid0").unwrap();
    assert!(rt.vias.iter().any(|v| v.net == "MID1" && v.at == p(via.at.x + disp.x, via.at.y + disp.y)), "{:?}", rt.vias);
    // the reference's copper is still there, once
    assert_eq!(rt.tracks.iter().filter(|t| t.net == "MID0").count(), 1);
    assert_eq!(rt.tracks.len(), 6, "three of channel 1, three copies: {:?}", rt.tracks.iter().map(|t| &t.id).collect::<Vec<_>>());
}

#[test]
fn repeat_layout_copies_the_other_items_on_the_layers_both_areas_have() {
    let m = model(2);
    let mut b = board(&m, 2);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    // the areas are on copper only: the silkscreen mark and the text are not in them (an item must be on a layer of the area)
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions::default())).unwrap();
    assert_eq!(drawings(&b).shapes.iter().filter(|s| s.layer() == "F.SilkS").count(), 1);
    assert_eq!(drawings(&b).texts.len(), 1);
    // the copper shape is: it is routing, and it comes across
    assert_eq!(drawings(&b).shapes.iter().filter(|s| s.layer() == "F.Cu").count(), 2);

    // with the silkscreen in both areas, they come across and a mark already in the target is replaced
    let m = model(2);
    let mut b = board(&m, 2);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    for id in &ids {
        let z = routing(&b).zones.iter().find(|z| &z.id == id).unwrap().clone();
        let mut layers: Vec<String> = z.layer_set().into_iter().map(String::from).collect();
        layers.push("F.SilkS".into());
        let cmd = Cmd::EditZone {
            id: z.id.clone(),
            net: String::new(),
            layer: "F.Cu".into(),
            layers: Some(layers),
            clearance: z.clearance,
            min_thickness: z.min_thickness,
            thermal_gap: z.thermal_gap,
            thermal_spoke_width: z.thermal_spoke_width,
            pad_connection: z.pad_connection,
            priority: 0,
            island_removal_mode: z.island_removal_mode,
            min_island_area: z.min_island_area,
            fill_mode: z.fill_mode,
            hatch_thickness: z.hatch_thickness,
            hatch_gap: z.hatch_gap,
            hatch_orientation_mdeg: 0,
            hatch_smoothing_level: 0,
            hatch_smoothing_value: z.hatch_smoothing_value,
            hatch_hole_min_area: z.hatch_hole_min_area,
            hatch_border_algorithm: z.hatch_border_algorithm,
            is_rule_area: true,
            keepout_tracks: false,
            keepout_vias: false,
            keepout_pads: false,
            keepout_copper_pour: false,
            keepout_footprints: false,
            smoothing: None,
            corner_radius: None,
            placement: None,
        };
        b.apply(&cmd).unwrap();
    }
    let z0 = zone_named(&b, "auto-placement-area-/ch1/").clone();
    let z1 = zone_named(&b, "auto-placement-area-/ch2/").clone();
    let disp = Point { x: centre(&z1).x - centre(&z0).x, y: centre(&z1).y - centre(&z0).y };
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions::default())).unwrap();
    let dr = drawings(&b);
    assert_eq!(dr.shapes.iter().filter(|s| s.layer() == "F.SilkS").count(), 2, "the mark and its copy");
    let mark = dr.shapes.iter().find(|s| s.id() == "shp_mark0").unwrap();
    let want: Vec<Point> = mark.points().iter().map(|q| p(q.x + disp.x, q.y + disp.y)).collect();
    assert!(dr.shapes.iter().any(|s| s.id() != "shp_mark0" && s.layer() == "F.SilkS" && s.points() == want), "{:?}", dr.shapes);
    assert_eq!(dr.texts.len(), 2);
    assert!(dr.texts.iter().any(|t| t.id != "txt_ch0" && t.at == p(30_000_000 + disp.x, 19_500_000 + disp.y)));
}

/// `copyRuleAreaContents` runs `fixupNet` on a copied copper shape as on a track: the copy is on the target's net, not the reference channel's.
#[test]
fn repeat_puts_a_copper_shape_on_the_target_net() {
    let m = model(3);
    let mut d = design(3);
    // channel 1's copper segment, on U1.4's net
    d.drawings.as_mut().unwrap().board_items.set_net("shp_cu0", Some("OUT0".into()));
    let mut b = Board::new(d, &m, 100_000, 300_000);
    generate_sheets(&mut b, 3);
    let ids = areas_of(&b, 3);
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions::default())).unwrap();
    let dr = drawings(&b);
    let mut nets: Vec<Option<&str>> = dr.shapes.iter().filter(|s| s.layer() == "F.Cu").map(|s| dr.board_items.net_of(s.id())).collect();
    nets.sort();
    assert_eq!(nets, vec![Some("OUT0"), Some("OUT1"), Some("OUT2")]);

    // "Restrict to routing connected within the area" takes the shape's own net: it is on a channel net, so it is copied
    let m = model(2);
    let mut d = design(2);
    d.drawings.as_mut().unwrap().board_items.set_net("shp_cu0", Some("OUT0".into()));
    let mut b = Board::new(d, &m, 100_000, 300_000);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions { connected_routing_only: true, ..RepeatOptions::default() })).unwrap();
    let dr = drawings(&b);
    assert!(dr.shapes.iter().any(|s| s.layer() == "F.Cu" && dr.board_items.net_of(s.id()) == Some("OUT1")), "{:?}", dr.board_items.shape_styles);
}

/// `RepeatLayout`'s grouping: `group->AddItem( item )` takes each copy out of the group cloned for it, so no item is in two groups.
#[test]
fn repeat_grouping_leaves_no_item_in_two_groups() {
    let m = model(2);
    let mut d = design(2);
    {
        let dr = d.drawings.as_mut().unwrap();
        dr.shapes.push(Shape::Segment { id: "shp_cu0b".into(), layer: "F.Cu".into(), stroke_width: 200_000, filled: false, start: p(33_000_000, 21_800_000), end: p(35_000_000, 21_800_000) });
        dr.groups.push(Group { id: String::new(), name: "pair".into(), member_ids: vec!["shp_cu0".into(), "shp_cu0b".into()], design_block: String::new() });
        eda_model::groups::GroupHost::assign_group_ids(dr);
    }
    let mut b = Board::new(d, &m, 100_000, 300_000);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions { group_items: true, ..RepeatOptions::default() })).unwrap();
    let dr = drawings(&b);
    let mut seen: std::collections::BTreeMap<&str, usize> = Default::default();
    for g in &dr.groups {
        for mbr in &g.member_ids {
            *seen.entry(mbr.as_str()).or_default() += 1;
        }
    }
    assert!(seen.values().all(|&n| n == 1), "{:?}", dr.groups);
    // the reference pair keeps its group; the target's group holds the area, the two copies and the footprints
    assert!(dr.groups.iter().any(|g| g.name == "pair" && g.member_ids.len() == 2 && g.member_ids.contains(&"shp_cu0".to_string())));
    let target = dr.groups.iter().find(|g| g.member_ids.contains(&ids[1])).expect("the target's group");
    assert_eq!(target.member_ids.iter().filter(|m| m.starts_with("shp_")).count(), 2, "{:?}", target.member_ids);
}

/// Review M1: Repeat Layout's grouping prunes only the groups it took items out of (`pruneExistingGroups`); a group of one elsewhere stays.
#[test]
fn repeat_grouping_leaves_an_unrelated_one_member_group_alone() {
    let m = model(2);
    let mut d = design(2);
    {
        let dr = d.drawings.as_mut().unwrap();
        dr.groups.push(Group { id: "grp_one".into(), name: String::new(), member_ids: vec!["shp_mark0".into()], design_block: String::new() });
    }
    let mut b = Board::new(d, &m, 100_000, 300_000);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions { group_items: true, ..RepeatOptions::default() })).unwrap();
    let g = drawings(&b).groups.iter().find(|g| g.id == "grp_one").cloned();
    assert_eq!(g.map(|g| g.member_ids), Some(vec!["shp_mark0".to_string()]));
}

#[test]
fn the_options_of_the_dialog_each_leave_out_their_part_of_the_copy() {
    let run = |options: RepeatOptions| -> Board<'static> {
        let m: &'static ConstraintModel = Box::leak(Box::new(model(2)));
        let mut b = Board::new(design(2), m, 100_000, 300_000);
        generate_sheets(&mut b, 2);
        let ids = areas_of(&b, 2);
        b.apply(&repeat(&ids[0], &ids[1..], options)).unwrap();
        b
    };
    let scattered = pose_of(&Board::new(design(2), &model(2), 100_000, 300_000), "U2");
    // no placement: the footprints stay, the copper comes
    let b = run(RepeatOptions { copy_placement: false, ..Default::default() });
    assert_eq!(pose_of(&b, "U2"), scattered);
    assert!(routing(&b).tracks.len() > 4);
    // no routing: the old copper stays, nothing is added
    let b = run(RepeatOptions { copy_routing: false, ..Default::default() });
    assert!(routing(&b).tracks.iter().any(|t| t.id == "trk_old1"));
    assert_eq!(routing(&b).tracks.len(), 4);
    assert_ne!(pose_of(&b, "U2"), scattered);
    // restricted to the routing connected within the area: the ground track (GND is a net of the matched pads too) comes, a track on a net no matched pad is on does not
    let b = run(RepeatOptions { connected_routing_only: true, ..Default::default() });
    assert_eq!(routing(&b).tracks.len(), 6);
    // group items: the target's area, its footprints and its copies in one group
    let b = run(RepeatOptions { group_items: true, ..Default::default() });
    let ch2 = zone_named(&b, "auto-placement-area-/ch2/").id.clone();
    let g = drawings(&b).groups.iter().find(|g| g.member_ids.contains(&ch2)).expect("a group for channel 2");
    assert!(["U2", "R2", "C2"].iter().all(|r| g.member_ids.contains(&r.to_string())), "{g:?}");
    assert!(g.member_ids.len() > 4, "the copies are in it too: {g:?}");
}

#[test]
fn locked_items_are_left_alone_unless_the_dialog_includes_them() {
    let run = |include_locked_items: bool| -> (Board<'static>, Pose) {
        let m: &'static ConstraintModel = Box::leak(Box::new(model(2)));
        let mut b = Board::new(design(2), m, 100_000, 300_000);
        generate_sheets(&mut b, 2);
        let ids = areas_of(&b, 2);
        // the old track of channel 2 and the footprint R2 are locked
        b.apply(&Cmd::SetLocked { ids: vec!["trk_old1".into(), "R2".into()], locked: true }).unwrap();
        let r2 = pose_of(&b, "R2");
        b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions { include_locked_items, ..Default::default() })).unwrap();
        (b, r2)
    };
    let (b, r2) = run(false);
    assert!(routing(&b).tracks.iter().any(|t| t.id == "trk_old1"), "a locked track is not taken away");
    assert_eq!(pose_of(&b, "R2"), r2, "a locked footprint is not moved");
    assert_ne!(pose_of(&b, "U2").at, p(31_700_000, 51_900_000), "the rest of the channel is");
    // the locked copper of the reference area is not copied either
    let (b, r2) = run(true);
    assert!(!routing(&b).tracks.iter().any(|t| t.id == "trk_old1"));
    assert_ne!(pose_of(&b, "R2"), r2);
}

#[test]
fn an_anchor_footprint_turns_and_moves_the_copy_to_lie_where_the_anchor_is() {
    let m = model(2);
    let mut b = board(&m, 2);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    // U2 is where it is, turned half a turn from U1: with U1 as the anchor the whole channel is turned half a turn about it and U2 stays
    let u2 = pose_of(&b, "U2");
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions { anchor: Some("U1".into()), ..Default::default() })).unwrap();
    assert_eq!(pose_of(&b, "U2").at, u2.at);
    assert_eq!(pose_of(&b, "U2").rot, u2.rot);
    // the resistor, 8 mm to the left of the amplifier and turned a quarter, is 8 mm to the right of it now and turned three quarters: half a turn about U
    let r2 = pose_of(&b, "R2");
    assert_eq!(r2.at, p(u2.at.x + 8_000_000, u2.at.y));
    assert_eq!(r2.rot, (90_000 + 180_000) % 360_000);
    // a copper track went with it: the middle net's first point was 7 mm left of U1, it is 7 mm right of U2 now
    assert!(routing(&b).tracks.iter().any(|t| t.net == "MID1" && t.pts.first() == Some(&p(u2.at.x + 7_000_000, u2.at.y))), "{:?}", routing(&b).tracks);
    // an anchor that is not in the reference area is refused
    assert_eq!(b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions { anchor: Some("U9".into()), ..Default::default() })).unwrap_err()[0].check, "ops_unknown_anchor");
}

#[test]
fn copper_that_ends_on_a_pad_ends_on_the_matching_pad_of_a_target_turned_a_quarter_about_the_anchor() {
    // A half turn does not tell a turn one way from the other way round, a quarter turn does: with the amplifier of channel 2 a quarter from the one of channel 1 and the
    // amplifier as the anchor, a stub that starts on a pad of channel 1 starts on the matching pad of channel 2, whichever footprint carries it -- the copper turns the way the
    // footprints do.
    let m = model(2);
    let mut d = design(2);
    d.placement.as_mut().unwrap().footprints.iter_mut().find(|f| f.id == "U2").unwrap().rot = 90_000;
    let pad_centre = |d: &Design, reference: &str, number: &str| -> Point {
        let f = d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == reference).unwrap();
        eda_model::footprint::placed_pads(&m, m.part(reference).unwrap(), f).unwrap().into_iter().find(|p| p.number == number).unwrap().center
    };
    // a stub of 0.3 mm into the footprint from the amplifier's pad 4 and from the resistor's pad 2 and the capacitor's pad 2, each on the net of its pad
    let stubs = [("U1", "4", "OUT0"), ("R1", "2", "MID0"), ("C1", "2", "OUT0")];
    for (reference, number, net) in stubs {
        let at = pad_centre(&d, reference, number);
        let inwards = if at.x >= d.placement.as_ref().unwrap().footprints.iter().find(|f| f.id == reference).unwrap().at.x { -300_000 } else { 300_000 };
        d.routing.as_mut().unwrap().tracks.push(track(&format!("stub_{reference}"), net, "F.Cu", &[(at.x, at.y), (at.x + inwards, at.y)]));
    }
    let mut b = Board::new(d, &m, 100_000, 300_000);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    let area = zone_named(&b, "auto-placement-area-/ch1/").outline.clone();
    for (reference, _, _) in stubs {
        let t = routing(&b).tracks.iter().find(|t| t.id == format!("stub_{reference}")).unwrap();
        assert!(t.pts.iter().all(|q| eda_model::multichannel::point_inside(&area, *q)), "the stub of {reference} is in the area: {:?}", t.pts);
    }
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions { anchor: Some("U1".into()), ..Default::default() })).unwrap();
    assert_eq!(pose_of(&b, "U2").rot, 90_000, "the counterpart of the anchor stays as it is");
    assert_ne!(pose_of(&b, "R2").rot, pose_of(&b, "R1").rot, "the others turned with it");
    for (reference, target, number, net) in [("U1", "U2", "4", "OUT1"), ("R1", "R2", "2", "MID1"), ("C1", "C2", "2", "OUT1")] {
        let want = pad_centre(b.design(), target, number);
        let copies: Vec<&Track> = routing(&b).tracks.iter().filter(|t| t.net == net && t.pts.len() == 2 && t.id != format!("stub_{reference}")).collect();
        // (a micrometre of rounding: the pad's place and the copy's are each rounded to it)
        assert!(copies.iter().any(|t| (t.pts[0].x - want.x).abs() <= 1 && (t.pts[0].y - want.y).abs() <= 1), "the copy of the stub of {reference}'s pad {number} starts on pad {number} of {target} at {want:?}: {:?}", copies.iter().map(|t| &t.pts).collect::<Vec<_>>());
    }
}

#[test]
fn the_text_fields_and_the_models_of_the_footprints_follow_the_placement() {
    let m = model(2);
    let mut d = design(2);
    // the reference of R1 is laid out by hand, the value hidden
    let dr = d.drawings.as_mut().unwrap();
    let mut e = eda_model::fp_edit::FootprintEdit::new("R1");
    let mut layout = eda_model::fp_edit::FieldLayout::new(p(0, -2_500_000), "F.SilkS");
    layout.size = (800_000, 800_000);
    e.reference = Some(layout.clone());
    let mut hidden = eda_model::fp_edit::FieldLayout::new(p(0, 1_000_000), "F.Fab");
    hidden.visible = false;
    e.value = Some(hidden.clone());
    dr.footprint_edits.push(e);
    let mut b = Board::new(d, &m, 100_000, 300_000);
    generate_sheets(&mut b, 2);
    let ids = areas_of(&b, 2);
    b.apply(&repeat(&ids[0], &ids[1..], RepeatOptions::default())).unwrap();
    let edit = b.design().footprint_edit("R2").expect("R2 has the layout now").clone();
    assert_eq!(edit.reference.as_ref().map(|l| (l.at, l.size, l.layer.as_str())), Some((layout.at, layout.size, "F.SilkS")));
    assert_eq!(edit.value.as_ref().map(|l| l.visible), Some(false));
}

#[test]
fn the_whole_repeat_is_one_command_and_one_undo_step() {
    let m = model(3);
    let mut b = board(&m, 3);
    generate_sheets(&mut b, 3);
    let ids = areas_of(&b, 3);
    let before = serde_json::to_value(b.design()).unwrap();
    let mut fork = b.fork();
    let cmd = repeat(&ids[0], &ids[1..], RepeatOptions::default());
    fork.apply(&cmd).unwrap();
    assert_ne!(serde_json::to_value(fork.design()).unwrap(), before);
    // the command is serialisable the way the studio sends it, and reads back
    let wire = serde_json::to_value(&cmd).unwrap();
    assert_eq!(wire["op"], "multichannel");
    assert_eq!(wire["verb"], "repeat_layout");
    let back: Cmd = serde_json::from_value(wire).unwrap();
    assert_eq!(back, cmd);
    assert_eq!(cmd.domain(), Domain::Pcb);
    assert!(!cmd.clears_routing());
}

// -------------------------------------------------------------------------------------------------------------- Apply Design Block Layout

/// The `.kicad_pcb` of a design block that holds channel 1 as it is laid out: the clipboard text of its footprints and its copper.
fn block_text(channels: usize) -> String {
    let m = model(channels);
    let d = design(channels);
    let ids: Vec<String> = ["U1", "R1", "C1", "trk_mid0", "trk_out0", "trk_gnd0", "via_mid0", "shp_cu0"].iter().map(|s| s.to_string()).collect();
    eda_kicad::export_pcb_clipboard(&d, &m, &ids, p(0, 0)).unwrap()
}

/// A board whose channel 2 is a group of the three footprints, linked to the design block.
fn board_with_group(m: &ConstraintModel) -> (Board<'_>, String) {
    let mut b = Board::new(design(2), m, 100_000, 300_000);
    b.apply(&Cmd::Group { ids: vec!["U2".into(), "R2".into(), "C2".into()] }).unwrap();
    let gid = drawings(&b).groups[0].id.clone();
    b.apply(&Cmd::EditGroup { id: gid.clone(), name: "ch2".into(), member_ids: vec!["U2".into(), "R2".into(), "C2".into()], design_block: Some("Amps:Channel".into()) }).unwrap();
    (b, gid)
}

fn apply_block(group: &str, text: String) -> Cmd {
    Cmd::Multichannel(MultichannelCmd::ApplyDesignBlockLayout { group: group.into(), block: "Amps:Channel".into(), text })
}

#[test]
fn the_layout_of_a_design_block_is_applied_to_the_footprints_of_a_group_matched_by_topology() {
    let m = model(2);
    let (mut b, gid) = board_with_group(&m);
    let before: Vec<(String, Pose)> = ["U2", "R2", "C2"].iter().map(|r| (r.to_string(), pose_of(&b, r))).collect();
    b.apply(&apply_block(&gid, block_text(2))).unwrap();
    // The block's footprints are placed as a rigid copy of the layout of the block, anchored on the first pair: that footprint of the group stays where it was and the layout
    // is turned to its orientation. So one of the three has not moved, and the three are the block's poses (U 30/20 mm, R 22/20 turned a quarter, C 38/20) turned by one
    // angle about the origin and moved by one vector.
    let block = [("U2", p(30_000_000, 20_000_000), 0u32), ("R2", p(22_000_000, 20_000_000), 90_000), ("C2", p(38_000_000, 20_000_000), 0)];
    let unmoved: Vec<&str> = before.iter().filter(|(r, was)| &pose_of(&b, r) == was).map(|(r, _)| r.as_str()).collect();
    assert_eq!(unmoved.len(), 1, "the anchor stays: {unmoved:?}");
    let (anchor_ref, anchor_block, anchor_rot) = block.iter().find(|(r, _, _)| *r == unmoved[0]).unwrap();
    let anchor_after = pose_of(&b, anchor_ref);
    let turn = (anchor_after.rot as i64 - *anchor_rot as i64).rem_euclid(360_000);
    let moved_anchor = super::rotate_point_about(*anchor_block, p(0, 0), turn);
    let by = p(anchor_after.at.x - moved_anchor.x, anchor_after.at.y - moved_anchor.y);
    for (r, at, rot) in block {
        let got = pose_of(&b, r);
        let want_at = super::rotate_point_about(at, p(0, 0), turn);
        assert_eq!(got.at, p(want_at.x + by.x, want_at.y + by.y), "{r}");
        assert_eq!(got.rot as i64, (rot as i64 + turn).rem_euclid(360_000), "{r}");
    }
    // the copper of the block came across onto the group's nets, and joined the group
    let rt = routing(&b);
    assert!(rt.tracks.iter().any(|t| t.net == "MID1" && t.id != "trk_old1"), "{:?}", rt.tracks.iter().map(|t| (&t.id, &t.net)).collect::<Vec<_>>());
    assert!(rt.tracks.iter().any(|t| t.net == "OUT1"));
    assert!(rt.vias.iter().any(|v| v.net == "MID1" && v.id != "via_old1"));
    let g = drawings(&b).groups.iter().find(|g| g.id == gid).unwrap();
    assert!(g.member_ids.len() > 3 && g.design_block == "Amps:Channel", "{g:?}");
    // nothing of the board's own was taken away (the group's temporary area is not on the board)
    assert!(rt.tracks.iter().any(|t| t.id == "trk_old1"));
    // no rule area was added
    assert!(rule_areas(&b).is_empty());
}

#[test]
fn a_group_that_does_not_match_the_block_or_is_locked_is_refused_in_kicads_words() {
    let m = model(2);
    // a block of channel 1 against a group of two footprints
    let (mut b, gid) = board_with_group(&m);
    b.apply(&Cmd::RemoveFromGroup { ids: vec!["C2".into()] }).unwrap();
    let err = b.apply(&apply_block(&gid, block_text(2))).unwrap_err();
    assert_eq!(err[0].check, "ops_topology_mismatch");
    assert!(err[0].hint.as_deref().unwrap().starts_with("Rule Area topologies do not match: "), "{err:?}");
    // a locked footprint
    let (mut b, gid) = board_with_group(&m);
    b.apply(&Cmd::SetLocked { ids: vec!["R2".into()], locked: true }).unwrap();
    let err = b.apply(&apply_block(&gid, block_text(2))).unwrap_err();
    assert_eq!(err[0].hint.as_deref(), Some("Footprint R2 is locked and cannot be placed."));
    // a group with no link, one with no footprints and one that is not there
    let mut plain = Board::new(design(2), &m, 100_000, 300_000);
    plain.apply(&Cmd::Group { ids: vec!["U2".into(), "R2".into()] }).unwrap();
    let pid = drawings(&plain).groups[0].id.clone();
    assert_eq!(plain.apply(&apply_block(&pid, block_text(2))).unwrap_err()[0].check, "ops_no_design_block_link");
    assert_eq!(plain.apply(&apply_block("grp_none", block_text(2))).unwrap_err()[0].check, "ops_unknown_group");
    let (mut b, gid) = board_with_group(&m);
    assert_eq!(b.apply(&apply_block(&gid, "not a board".into())).unwrap_err()[0].check, "ops_bad_clipboard");
}

// ------------------------------------------------------------------------------------------------------------------- where the sheets come from

#[test]
fn a_board_with_no_stored_schematic_is_on_the_module_sheets_the_studio_derives() {
    // no footprint says what sheet it is on: the module sheets of the intent are the sheets (`infer_modules` and the names `derive_hierarchy` gives them), one per amplifier here
    let m = model(3);
    let mut d = design(3);
    d.drawings.as_mut().unwrap().footprint_extras.clear();
    let mut b = Board::new(d, &m, 100_000, 300_000);
    let sheets = b.mc_part_sheets();
    for (reference, module) in [("U1", "U1"), ("R2", "U2"), ("C3", "U3")] {
        assert_eq!((sheets[reference].name.as_str(), sheets[reference].file.as_str()), (format!("/{module}/").as_str(), format!("{}.kicad_sch", module.to_lowercase()).as_str()), "{reference}");
    }
    // so the sheets are rows of the Generate dialog, and the channels they make match
    let rows = b.mc_potential_areas();
    let keys: Vec<&str> = rows.iter().filter(|r| r.kind == AreaKind::Sheet).map(|r| r.key.as_str()).collect();
    assert_eq!(keys, vec!["/U1/", "/U2/", "/U3/"]);
    b.apply(&generate(&[(AreaKind::Sheet, "/U1/"), (AreaKind::Sheet, "/U2/"), (AreaKind::Sheet, "/U3/")], false, false)).unwrap();
    let ids: Vec<String> = ["/U1/", "/U2/", "/U3/"].iter().map(|k| zone_named(&b, &format!("auto-placement-area-{k}")).id.clone()).collect();
    assert!(b.mc_compat(&ids[0]).unwrap().targets.iter().all(|t| t.ok));
}

/// A schematic whose root places the one sheet file twice, `ch1` and `ch2`, the symbols of the file being U1/R1 on the first and U2/R2 on the second (`instance_overrides`).
fn schematic_with_a_sheet_placed_twice(d: &mut Design) {
    use eda_model::ir::{SchematicSection, SheetInstance, SymbolInstance, SymbolPathOverride};
    let symbol = |id: &str, x: Nm| -> SymbolInstance { serde_json::from_value(serde_json::json!({ "id": id, "at": { "x": x, "y": 10_000_000 }, "rot": 0 })).unwrap() };
    let sheet = |id: &str, name: &str| SheetInstance { id: id.into(), name: name.into(), file: "channel.kicad_sch".into(), at: p(0, 0), size: (10_000_000, 10_000_000), pins: vec![], page: String::new() };
    let over = |parent: &str, x: Nm, reference: &str| SymbolPathOverride { at: p(x, 10_000_000), parent_sheet_instance_id: parent.into(), reference: reference.into(), unit: 1 };
    d.schematic = Some(SchematicSection { sheets: vec![sheet("sh_a", "ch1"), sheet("sh_b", "ch2")], ..Default::default() });
    let screen = SchematicSection {
        symbols: vec![symbol("U1", 10_000_000), symbol("R1", 20_000_000), symbol("C1", 30_000_000)],
        instance_overrides: vec![over("sh_a", 10_000_000, "U1"), over("sh_a", 20_000_000, "R1"), over("sh_a", 30_000_000, "C1"), over("sh_b", 10_000_000, "U2"), over("sh_b", 20_000_000, "R2"), over("sh_b", 30_000_000, "C2")],
        ..Default::default()
    };
    d.sheet_contents = Some([("channel.kicad_sch".to_string(), screen)].into_iter().collect());
}

#[test]
fn a_sheet_placed_twice_puts_the_parts_of_each_placement_on_the_sheet_of_that_placement() {
    let m = model(2);
    let mut d = design(2);
    d.drawings.as_mut().unwrap().footprint_extras.clear();
    schematic_with_a_sheet_placed_twice(&mut d);
    let mut b = Board::new(d, &m, 100_000, 300_000);
    let sheets = b.mc_part_sheets();
    assert_eq!((sheets["U1"].name.as_str(), sheets["U1"].file.as_str()), ("/ch1/", "channel.kicad_sch"));
    assert_eq!(sheets["R2"].name, "/ch2/");
    // both placements are of one symbol of the file: the footprints that stand for it are tied together, which is how the matcher breaks a tie
    assert_eq!(sheets["U1"].symbol, sheets["U2"].symbol);
    assert!(sheets["U1"].symbol.is_some() && sheets["U1"].symbol != sheets["R1"].symbol);
    b.apply(&generate(&[(AreaKind::Sheet, "/ch1/"), (AreaKind::Sheet, "/ch2/")], false, false)).unwrap();
    let ids = areas_of(&b, 2);
    let plan = b.mc_compat(&ids[0]).unwrap();
    assert!(plan.targets[0].ok, "{:?}", plan.targets[0]);
}

// ----------------------------------------------------------------------------------------------------------------------- component classes

#[test]
fn the_classes_the_project_gives_a_footprint_are_those_its_assignment_rules_match() {
    use eda_model::component_classes::{ClassAssignment, ClassCondition, ComponentClassSettings, ConditionKind};
    let cond = |kind: ConditionKind, primary: &str, secondary: &str| ClassCondition { kind, primary: primary.into(), secondary: secondary.into() };
    let rule = |class: &str, all: bool, conditions: Vec<ClassCondition>| ClassAssignment { class: class.into(), all, conditions };
    let mut m = model(2);
    m.board.component_classes = ComponentClassSettings {
        sheet_classes: false,
        assignments: vec![
            // the amplifiers (a wildcard on the reference), turned half a turn from the first channel's
            rule("Amplifiers", true, vec![cond(ConditionKind::Reference, "U*", "")]),
            rule("Resistors", true, vec![cond(ConditionKind::Reference, "R1,R2", "")]),
            // all of the conditions, or any of them
            rule("TurnedAmplifiers", true, vec![cond(ConditionKind::Reference, "U?", ""), cond(ConditionKind::Rotation, "180", "")]),
            rule("EitherOne", false, vec![cond(ConditionKind::Reference, "C1", ""), cond(ConditionKind::Side, "Back", "")]),
            // a rule with no condition that says anything is for every footprint
            rule("Everything", true, vec![cond(ConditionKind::Side, "Any", "")]),
            // the sheet, and a field
            rule("OnChannelTwo", true, vec![cond(ConditionKind::SheetName, "/ch2/", "")]),
            rule("ByField", true, vec![cond(ConditionKind::FootprintField, "Value", "10k")]),
            // an expression of the user's own is not evaluated here
            rule("Custom", true, vec![cond(ConditionKind::Custom, "A.Layer == 'F.Cu'", "")]),
        ],
    };
    let b = Board::new(design(2), &m, 100_000, 300_000);
    let mc = b.mc_board();
    let classes = |r: &str| -> Vec<String> { mc.fps[mc.index_of(r).unwrap()].classes.iter().cloned().collect() };
    assert_eq!(classes("U1"), vec!["Amplifiers", "Everything"]);
    assert_eq!(classes("U2"), vec!["Amplifiers", "Everything", "OnChannelTwo", "TurnedAmplifiers"], "U2 is turned half a turn");
    assert_eq!(classes("R2"), vec!["Everything", "OnChannelTwo", "Resistors"]);
    assert_eq!(classes("C1"), vec!["EitherOne", "Everything"]);
    // sheet classes: one per sheet but the root, named by the sheet's path
    let mut m2 = model(2);
    m2.board.component_classes.sheet_classes = true;
    let mut d = design(2);
    d.drawings.as_mut().unwrap().footprint_extras.iter_mut().find(|e| e.id == "C2").unwrap().sheetname = Some("/".into());
    let b2 = Board::new(d, &m2, 100_000, 300_000);
    let mc2 = b2.mc_board();
    assert_eq!(mc2.fps[mc2.index_of("U1").unwrap()].classes.iter().cloned().collect::<Vec<_>>(), vec!["/ch1/"]);
    assert!(mc2.fps[mc2.index_of("C2").unwrap()].classes.is_empty(), "the root sheet has no class");
    // a footprint's own classes, from the file
    let mut d3 = design(2);
    d3.drawings.as_mut().unwrap().footprint_extras.iter_mut().find(|e| e.id == "U1").unwrap().component_classes = vec!["Stage A".into()];
    let m3 = model(2);
    let b3 = Board::new(d3, &m3, 100_000, 300_000);
    let rows = b3.mc_potential_areas();
    assert_eq!(rows.iter().find(|r| r.kind == AreaKind::ComponentClass).map(|r| (r.key.as_str(), r.components.clone())), Some(("Stage A", vec!["U1".to_string()])));
    // a rule area for the class finds the footprints that have it
    let mut b3 = b3;
    b3.apply(&generate(&[(AreaKind::ComponentClass, "Stage A")], false, false)).unwrap();
    let z = zone_named(&b3, "auto-placement-area-Stage A");
    assert_eq!(z.placement.source_type, PlacementSourceType::ComponentClass);
    let mc3 = b3.mc_board();
    assert_eq!(Board::mc_components_of(&mc3, &z.placement), vec![mc3.index_of("U1").unwrap()]);
}

#[test]
fn the_sheet_tests_are_the_rule_languages() {
    use super::multichannel_facts::{in_sheet_or_below, member_of_group, member_of_sheet, member_of_sheet_or_children};
    // memberOfSheet: the sheet itself, wildcards allowed, the closing slash ignored
    assert!(member_of_sheet("/ch1/", "/ch1"));
    assert!(member_of_sheet("/ch1/", "/ch?/"));
    assert!(!member_of_sheet("/ch1/sub/", "/ch1/"));
    assert!(member_of_sheet("", ""), "the root is no sheet name, or all of them");
    // memberOfSheetOrChildren: the sheet and the ones below it, level by level
    assert!(member_of_sheet_or_children("/ch1/sub/", "/ch1/"));
    assert!(member_of_sheet_or_children("/ch1/sub/", "/*/sub"));
    assert!(!member_of_sheet_or_children("/ch1/", "/ch1/sub/"));
    assert!(!member_of_sheet_or_children("/ch2/", "/ch1/"));
    // queryComponentsInSheet: a sheet and its children by the prefix; the root holds every path
    assert!(in_sheet_or_below("/ch1/sub/", "/ch1/"));
    assert!(!in_sheet_or_below("/ch10/", "/ch1/"));
    assert!(in_sheet_or_below("/anything/", "/"));
    assert!(in_sheet_or_below("", "/") && in_sheet_or_below("/", "/"));
    // memberOfGroup: the group or any above it, by name
    assert!(member_of_group(&["inner".into(), "outer".into()], "outer"));
    assert!(member_of_group(&["inner".into()], "in*"));
    assert!(!member_of_group(&[], "x"));
}
