//! The Net Inspector's Rename, Delete and Add Net through a board directory (`PCB_NET_INSPECTOR_PANEL::onRenameSelectedNet`, `onDeleteSelectedNet`,
//! `onAddNet`): the board's nets are named anew on the next load (`board_net_edits`), with the gates on (the studio's strict mode), its copper with them,
//! the derived `.kicad_pcb` agrees, and one undo step takes each back.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_model::ir::{RoutingSection, Track};
use eda_model::units::mm;
use eda_ops::{Cmd, Domain};
use std::path::Path;

fn net_of(dir: &Path, pin: &str) -> String {
    let (_, _, model) = load(dir).unwrap();
    model.nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default()
}

/// `setup`'s board with a GND track from U1's pad 1 to U2's.
fn routed(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    setup(&dir);
    let (_, mut design, _) = load(&dir).unwrap();
    let p = |x: f64, y: f64| eda_model::ir::Point { x: mm(x), y: mm(y) };
    let track = Track { id: "trk_gnd".into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: mm(0.25), pts: vec![p(4.0, 5.0), p(4.0, 8.0), p(14.0, 8.0), p(14.0, 5.0)], arc_mid_offset: None };
    design.routing = Some(RoutingSection { tracks: vec![track], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() });
    super::save(&dir, &design).unwrap();
    dir
}

#[test]
fn a_renamed_net_keeps_its_pads_and_copper_under_the_gates_and_undo_takes_it_back() {
    let dir = routed("net_inspector_rename_dir");
    step(&dir, Cmd::RenameNet { from: "GND".into(), to: "0V".into() }, true, "test").unwrap();
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U2.1"), net_of(&dir, "U1.2")], ["0V", "0V", "VCC"], "the pads take the new name, the other net is as it was");
    let (_, design, model) = load(&dir).unwrap();
    assert_eq!(design.routing.as_ref().unwrap().tracks[0].net, "0V");
    assert!(design.nets.is_none(), "the netlist is the schematic's");
    let pcb = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(pcb.contains("(net \"0V\")") || pcb.contains("\"0V\""), "the derived board names the net");
    assert!(!pcb.contains("\"GND\""), "and not the old one");
    // the old name is free again, the new one taken
    let e = step(&dir, Cmd::RenameNet { from: "VCC".into(), to: "0V".into() }, true, "test").unwrap_err();
    assert!(super::reasons(&e).contains("Net name '0V' is already in use."), "{}", super::reasons(&e));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(net_of(&dir, "U1.1"), "GND");
    assert_eq!(load(&dir).unwrap().1.routing.unwrap().tracks[0].net, "GND");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_deleted_net_leaves_its_pads_and_copper_on_no_net_and_add_net_makes_one_of_no_items() {
    let dir = routed("net_inspector_delete_dir");
    step(&dir, Cmd::DeleteNets { names: vec!["GND".into()] }, false, "test").unwrap();
    assert_eq!([net_of(&dir, "U1.1"), net_of(&dir, "U2.1"), net_of(&dir, "U1.2")], ["", "", "VCC"]);
    assert_eq!(load(&dir).unwrap().1.routing.unwrap().tracks[0].net, "");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(net_of(&dir, "U1.1"), "GND");
    // Add Net: a net of no item, which the next load has; the same name again is refused
    step(&dir, Cmd::AddNet { name: "SPARE".into() }, true, "test").unwrap();
    assert!(load(&dir).unwrap().2.nets.iter().any(|n| n.name == "SPARE" && n.pins.is_empty()));
    let e = step(&dir, Cmd::AddNet { name: "SPARE".into() }, true, "test").unwrap_err();
    assert!(super::reasons(&e).contains("Net name 'SPARE' is already in use."));
    let _ = std::fs::remove_dir_all(&dir);
}
