//! The Properties panel's rows that were read-only (UI-LEFT-2 PP8): a pad's number (`set_pad_number`), its Pad Type "Edge connector", backdrill and
//! post-machining and a size of 0 (`edit_board_pad`), a copper shape's net (`set_item_net`) and a field's lock (`set_locked`).

use super::pcb_transform_tests::{board, design, drawings, ids, model, p};
use super::*;
use eda_model::fp_edit::PadEdit;
use eda_model::units::mm;
use eda_model::Net;
use eda_model::via_extra::{PadDrillProps, PostMachiningMode, PostMachiningProps, ViaDrillProps};

/// The net list in force: the design's own, else the intent's.
fn nets_of(b: &Board<'_>) -> Vec<Net> {
    b.design().nets.clone().unwrap_or_else(|| b.model.nets.clone())
}

fn net_of(nets: &[Net], pin: &str) -> Option<String> {
    nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone())
}

#[test]
fn a_renumbered_pad_keeps_its_net_and_its_edit_and_its_old_number_leaves_the_net() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut e = PadEdit::none("1", 1);
    e.solder_mask_margin = Some(mm(0.05));
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e }).unwrap();

    b.apply(&Cmd::SetPadNumber { pad: "U1.1".into(), number: "7".into() }).unwrap();
    // The instance's own footprint has the pad renamed; U2's is untouched.
    let def = b.design().footprint_assignment("U1").and_then(|a| a.definition.clone()).expect("U1 has its own copy");
    assert_eq!(def.pads.iter().map(|p| p.number.as_str()).collect::<Vec<_>>(), vec!["7", "2", "3"]);
    assert!(b.design().footprint_assignment("U2").is_none());
    // `PAD::SetNumber` keeps the pad's net: its new pin is on GND, the old pin left it.
    let nets = nets_of(&b);
    assert_eq!(net_of(&nets, "U1.7").as_deref(), Some("GND"));
    assert_eq!(net_of(&nets, "U1.1"), None);
    assert_eq!(net_of(&nets, "U1.2").as_deref(), Some("VCC"), "the other pads keep theirs");
    // The pad's edit followed it.
    let edits = &b.design().footprint_edit("U1").unwrap().pads;
    assert_eq!(edits.iter().map(|e| (e.number.as_str(), e.nth, e.solder_mask_margin)).collect::<Vec<_>>(), vec![("7", 1, Some(mm(0.05)))]);
    // Renamed again, to a number it had: back where it was.
    b.apply(&Cmd::SetPadNumber { pad: "U1.7".into(), number: "1".into() }).unwrap();
    assert_eq!(net_of(&nets_of(&b), "U1.1").as_deref(), Some("GND"));
    assert_eq!(b.apply(&Cmd::SetPadNumber { pad: "U1.9".into(), number: "1".into() }).unwrap_err()[0].check, "ops_unknown_pad");
}

#[test]
fn a_pad_renamed_to_a_shared_number_takes_its_place_among_the_pads_of_that_number() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // Pad 3 (on no net) gets a margin, then becomes the second pad numbered 2: its edit is `2#2`.
    let mut e = PadEdit::none("3", 1);
    e.solder_paste_margin = Some(mm(-0.02));
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e }).unwrap();
    b.apply(&Cmd::SetPadNumber { pad: "U1.3".into(), number: "2".into() }).unwrap();
    let edits = &b.design().footprint_edit("U1").unwrap().pads;
    assert_eq!(edits.iter().map(|e| (e.number.as_str(), e.nth)).collect::<Vec<_>>(), vec![("2", 2)]);
    // `PAD::SetNumber` keeps each pad's net (UI-LEFT-3 P3-9): pad 2 stays on VCC, the renamed pad on none -- each its own (`PadMaskInfo::net`), the net list untouched.
    assert_eq!(net_of(&nets_of(&b), "U1.2").as_deref(), Some("VCC"));
    assert_eq!(net_of(&nets_of(&b), "U1.3"), None);
    let extra = b.design().drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "U1").expect("U1's pads keep their own nets");
    assert_eq!(extra.pads.iter().map(|p| p.net.as_deref()).collect::<Vec<_>>(), vec![None, Some("VCC"), Some("")]);
}

#[test]
fn a_pad_renamed_to_another_pads_number_keeps_its_own_net_on_the_board_and_in_the_file() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    // Pad 1 (GND) becomes a second pad 2 (VCC): KiCad's `PAD::SetNumber` touches nothing else, so it stays on GND until Update PCB from Schematic.
    b.apply(&Cmd::SetPadNumber { pad: "U1.1".into(), number: "2".into() }).unwrap();
    let extra = b.design().drawings.as_ref().unwrap().footprint_extras.iter().find(|e| e.id == "U1").unwrap();
    assert_eq!(extra.pads.iter().map(|p| p.net.as_deref()).collect::<Vec<_>>(), vec![Some("GND"), Some("VCC"), None]);
    // The connectivity reads each pad's own net.
    let d = b.design().clone();
    // (the model the server builds: the board's copy of U1's footprint, `fold_footprint_edits`)
    let mut m2 = m.clone();
    m2.instance_footprints.insert("U1".into(), d.footprint_assignment("U1").and_then(|a| a.definition.clone()).unwrap());
    let items = eda_connectivity::items::build_items(&d, &m2, &Default::default());
    let mut pad_nets: Vec<(String, String)> = items
        .iter()
        .filter(|i| i.pad_piece_of.is_none())
        .filter_map(|i| match &i.item {
            eda_connectivity::ItemRef::Pad { reference, number } if reference == "U1" => Some((format!("U1.{number}"), i.net.clone())),
            _ => None,
        })
        .collect();
    pad_nets.sort();
    pad_nets.dedup();
    assert_eq!(pad_nets, vec![("U1.2".to_string(), "GND".to_string()), ("U1.2".to_string(), "VCC".to_string()), ("U1.3".to_string(), String::new())]);
    // The board file writes each pad's own net.
    let pcb = eda_kicad::export_kicad_pcb(&d, &m2, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    let u1 = &pcb[pcb.find("\"U1\"").unwrap()..];
    let pad_nets: Vec<&str> = u1.split("(pad ").skip(1).take(3).map(|p| p.split("(net ").nth(1).map_or("", |n| n.split(')').next().unwrap())).collect();
    assert!(pad_nets[0].contains("GND") && pad_nets[1].contains("VCC") && pad_nets[2].is_empty(), "{pad_nets:?}");
    // A pad renamed to a number nobody has: its pin joins the net it was on (the pads of a shared number keep theirs).
    b.apply(&Cmd::SetPadNumber { pad: "U1.3".into(), number: "9".into() }).unwrap();
    assert_eq!(net_of(&nets_of(&b), "U1.9"), None);
}

#[test]
fn a_pad_takes_a_size_of_zero_an_edge_connector_type_and_its_backdrill_and_post_machining() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let mut e = PadEdit::none("2", 1);
    e.size = Some((Nm::ZERO, mm(0.6)));
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: e }).expect("the Properties panel takes a size of 0");
    let mut c = PadEdit::none("1", 1);
    c.connector = Some(true);
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: c }).unwrap();
    let drill = PadDrillProps {
        backdrill: Some(ViaDrillProps { size: mm(0.5), start: "B.Cu".into(), end: "In1.Cu".into() }),
        front_post_machining: Some(PostMachiningProps { mode: PostMachiningMode::Countersink, size: mm(1.2), depth: Nm::ZERO, angle_ddeg: 900 }),
        ..Default::default()
    };
    let mut d = PadEdit::none("3", 1);
    d.drill_props = Some(drill.clone());
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: d }).unwrap();
    let edit = b.design().footprint_edit("U1").unwrap();
    assert_eq!(edit.pad("1", 1).and_then(|e| e.connector), Some(true));
    assert_eq!(edit.pad("3", 1).and_then(|e| e.drill_props.clone()), Some(drill.clone()));
    // The tokens a pad edit writes, and what they read back as.
    let mut tokens = vec!["(property pad_prop_heatsink)".to_string(), "(backdrill (size 0.3) (layers \"F.Cu\" \"In1.Cu\"))".to_string()];
    drill.set_in_tokens(&mut tokens);
    assert_eq!(tokens, vec!["(property pad_prop_heatsink)".to_string(), "(backdrill (size 0.5) (layers \"B.Cu\" \"In1.Cu\"))".into(), "(front_post_machining countersink (size 1.2) (angle 90))".into()]);
    assert_eq!(PadDrillProps::from_tokens(&tokens), drill);
    PadDrillProps::default().set_in_tokens(&mut tokens);
    assert_eq!(tokens, vec!["(property pad_prop_heatsink)".to_string()], "no backdrill, no post-machining: nothing written");
}

#[test]
fn a_copper_shape_takes_a_net_and_gives_it_back() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut d = design();
    d.drawings.as_mut().unwrap().shapes.push(Shape::Rect { id: "shp_cu".into(), layer: "F.Cu".into(), stroke_width: mm(0.1), filled: true, start: p(Nm::ZERO, Nm::ZERO), end: p(mm(4.0), mm(2.0)) });
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::SetItemNet { ids: ids(&["shp_cu", "trk_a"]), net: "VCC".into() }).unwrap();
    assert_eq!(drawings(&b).board_items.net_of("shp_cu"), Some("VCC"));
    // A copper shape may be on no net (a track may not), a silkscreen line has no net at all.
    b.apply(&Cmd::SetItemNet { ids: ids(&["shp_cu"]), net: String::new() }).unwrap();
    assert_eq!(drawings(&b).board_items.net_of("shp_cu"), None);
    assert_eq!(b.apply(&Cmd::SetItemNet { ids: ids(&["shp_seg"]), net: "VCC".into() }).unwrap_err()[0].check, "ops_unknown_item");
}

#[test]
fn a_footprints_field_is_locked_on_its_own() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    b.apply(&Cmd::SetLocked { ids: ids(&["U1:Reference"]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.iter().any(|i| i == "U1:Reference"));
    assert!(!drawings(&b).locked_ids.iter().any(|i| i == "U1"), "the footprint is not locked with it");
    // Another lock keeps it.
    b.apply(&Cmd::SetLocked { ids: ids(&["trk_a"]), locked: true }).unwrap();
    assert!(drawings(&b).locked_ids.iter().any(|i| i == "U1:Reference"));
    b.apply(&Cmd::SetLocked { ids: ids(&["U1:Reference"]), locked: false }).unwrap();
    assert!(!drawings(&b).locked_ids.iter().any(|i| i == "U1:Reference"));
    assert_eq!(b.apply(&Cmd::SetLocked { ids: ids(&["U9:Reference"]), locked: true }).unwrap_err()[0].check, "ops_unknown_item");
}
