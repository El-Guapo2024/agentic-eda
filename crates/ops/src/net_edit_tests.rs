//! The Net Inspector's Rename Selected Net, Delete Selected Net(s) and Add Net (`PCB_NET_INSPECTOR_PANEL::onRenameSelectedNet`, `onDeleteSelectedNet`,
//! `onAddNet`): every item and pin of the net follows, a name in use is refused, each is one step.

use super::pcb_transform_tests::{design, part};
use super::*;
use eda_model::ir::{BoardPart, FootprintAuxPad, FootprintExtra, ViaTenting};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Net};

/// U1 and U2 with GND on U1.1 and U2.1, SIG on U1.2; the shared test board's track, via and zone are on GND.
fn model() -> ConstraintModel {
    let net = |name: &str, pins: &[&str]| Net { name: name.into(), pins: pins.iter().map(|p| p.to_string()).collect() };
    let mut m = ConstraintModel { parts: vec![part("U1"), part("U2")], nets: vec![net("GND", &["U1.1", "U2.1"]), net("SIG", &["U1.2"])], ..Default::default() };
    m.board.layers = vec!["F.Cu".into(), "B.Cu".into()];
    m
}

/// The shared design with a copper shape, a footprint's auxiliary pad, a via tenting record, a board part's pad and a design block's net, all on GND.
fn gnd_everywhere() -> Design {
    let mut d = design();
    let dr = d.drawings.get_or_insert_with(Default::default);
    dr.board_items.shape_styles.push(eda_model::pcb_items::ShapeStyle { net: Some("GND".into()), ..eda_model::pcb_items::ShapeStyle::empty("shp_seg") });
    dr.footprint_extras.push(FootprintExtra { id: "U1".into(), aux_pads: vec![FootprintAuxPad { text: "(pad \"\" smd)".into(), net: "GND".into() }], ..Default::default() });
    dr.via_tenting.push(ViaTenting { at: d.routing.as_ref().unwrap().vias[0].at, net: "GND".into(), front: Some(true), back: None });
    dr.board_parts.push(BoardPart { reference: "J9".into(), value: None, footprint: "QUAD".into(), definition: None, pad_nets: vec![("1".into(), "GND".into()), ("2".into(), "SIG".into())] });
    dr.board_nets.push("SPARE".into());
    d
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> (String, String) {
    let e = b.apply(&cmd).unwrap_err();
    (e[0].check.clone(), e[0].hint.clone().unwrap_or_default())
}

fn overlay(d: &Design) -> Vec<(String, String)> {
    d.drawings.as_ref().map(|dr| dr.net_renames.iter().map(|e| (e.from.clone(), e.to.clone())).collect()).unwrap_or_default()
}

/// `onRenameSelectedNet`: the board's net takes the new name -- the overlay the pads are named by on the next load, and its tracks, vias and zones, a copper
/// shape, a footprint's pad that is no pin, the tenting record and a board part's pad; the schematic's netlist is left as it is.
#[test]
fn rename_carries_every_item_of_the_net() {
    let m = model();
    let mut b = Board::new(gnd_everywhere(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::RenameNet { from: "GND".into(), to: "/power/0V".into() }).unwrap();
    let d = b.design();
    assert_eq!(overlay(d), vec![("GND".to_string(), "/power/0V".to_string())]);
    assert!(d.nets.is_none(), "the netlist the schematic owns is not touched");
    let rt = d.routing.as_ref().unwrap();
    assert!(rt.tracks.iter().all(|t| t.net == "/power/0V") && rt.vias.iter().all(|v| v.net == "/power/0V") && rt.zones.iter().all(|z| z.net == "/power/0V"));
    let dr = d.drawings.as_ref().unwrap();
    assert_eq!(dr.board_items.shape_styles.iter().find(|s| s.id == "shp_seg").unwrap().net.as_deref(), Some("/power/0V"));
    assert_eq!(dr.footprint_extras.iter().find(|e| e.id == "U1").unwrap().aux_pads[0].net, "/power/0V");
    assert_eq!(dr.via_tenting[0].net, "/power/0V");
    assert_eq!(dr.board_parts[0].pad_nets, vec![("1".to_string(), "/power/0V".to_string()), ("2".to_string(), "SIG".to_string())]);
    assert_eq!(dr.board_nets, vec!["SPARE".to_string()]);
    // renamed again: one entry from the first name; back to the first name: none
    b.apply(&Cmd::RenameNet { from: "/power/0V".into(), to: "VSS".into() }).unwrap();
    assert_eq!(overlay(b.design()), vec![("GND".to_string(), "VSS".to_string())]);
    b.apply(&Cmd::RenameNet { from: "VSS".into(), to: "GND".into() }).unwrap();
    assert!(overlay(b.design()).is_empty());
    assert!(b.design().routing.as_ref().unwrap().tracks.iter().all(|t| t.net == "GND"));
}

/// "Net name '%s' is already in use." -- a net of the list, a design block's net and a board part's net are all in use; an unknown net and an empty name are refused.
#[test]
fn rename_refuses_a_name_in_use() {
    let m = model();
    let mut b = Board::new(gnd_everywhere(), &m, mm(0.1), mm(0.3));
    assert_eq!(refusal(&mut b, Cmd::RenameNet { from: "GND".into(), to: "SIG".into() }).0, "ops_net_name_in_use");
    assert_eq!(refusal(&mut b, Cmd::RenameNet { from: "GND".into(), to: "SPARE".into() }).0, "ops_net_name_in_use");
    let (check, hint) = refusal(&mut b, Cmd::RenameNet { from: "GND".into(), to: "".into() });
    assert_eq!((check.as_str(), hint.as_str()), ("ops_bad_net_name", "Net name cannot be empty."));
    assert_eq!(refusal(&mut b, Cmd::RenameNet { from: "NOPE".into(), to: "X".into() }).0, "ops_unknown_net");
    assert!(overlay(b.design()).is_empty(), "the design is untouched");
    // after a rename the old name is free and the new one taken
    b.apply(&Cmd::RenameNet { from: "GND".into(), to: "0V".into() }).unwrap();
    assert_eq!(refusal(&mut b, Cmd::RenameNet { from: "SIG".into(), to: "0V".into() }).0, "ops_net_name_in_use");
    assert_eq!(refusal(&mut b, Cmd::RenameNet { from: "GND".into(), to: "X".into() }).0, "ops_unknown_net");
}

/// `BOARD::Remove( NETINFO_ITEM )`: every connected item of the net goes onto the unconnected net, the overlay takes its pads off it; a net with no pin
/// (a design block's) leaves only `board_nets`.
#[test]
fn delete_puts_the_items_of_the_nets_on_no_net() {
    let m = model();
    let mut b = Board::new(gnd_everywhere(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::DeleteNets { names: vec!["GND".into(), "SPARE".into()] }).unwrap();
    let d = b.design();
    assert_eq!(overlay(d), vec![("GND".to_string(), String::new())]);
    let rt = d.routing.as_ref().unwrap();
    assert!(rt.tracks.iter().all(|t| t.net.is_empty()) && rt.vias.iter().all(|v| v.net.is_empty()) && rt.zones.iter().all(|z| z.net.is_empty()));
    let dr = d.drawings.as_ref().unwrap();
    assert!(!dr.board_items.shape_styles.iter().any(|s| s.id == "shp_seg"), "a style that only named the net says nothing now");
    assert!(dr.board_nets.is_empty());
    assert_eq!(dr.board_parts[0].pad_nets, vec![("2".to_string(), "SIG".to_string())]);
    let mut again = Board::new(gnd_everywhere(), &m, mm(0.1), mm(0.3));
    assert_eq!(refusal(&mut again, Cmd::DeleteNets { names: vec!["GND".into(), "NOPE".into()] }).0, "ops_unknown_net");
    assert_eq!(refusal(&mut again, Cmd::DeleteNets { names: vec![] }).0, "ops_nothing_to_change");
    // a renamed net deleted: the one entry, from the first name
    again.apply(&Cmd::RenameNet { from: "GND".into(), to: "0V".into() }).unwrap();
    again.apply(&Cmd::DeleteNets { names: vec!["0V".into()] }).unwrap();
    assert_eq!(overlay(again.design()), vec![("GND".to_string(), String::new())]);
}
/// `onAddNet`: a net with no item, refused when the name is in use.
#[test]
fn add_net_makes_a_net_of_no_items() {
    let m = model();
    let mut b = Board::new(gnd_everywhere(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::AddNet { name: "NEW".into() }).unwrap();
    assert_eq!(b.design().drawings.as_ref().unwrap().board_nets, vec!["NEW".to_string(), "SPARE".to_string()]);
    let (check, hint) = refusal(&mut b, Cmd::AddNet { name: "GND".into() });
    assert_eq!((check.as_str(), hint.as_str()), ("ops_net_name_in_use", "Net name 'GND' is already in use."));
    assert_eq!(refusal(&mut b, Cmd::AddNet { name: "NEW".into() }).0, "ops_net_name_in_use");
}

/// The verbs read from the studio's JSON (`{"op": "rename_net", ..}`) and are the board editor's steps (the schematic is not changed).
#[test]
fn the_verbs_parse_and_are_the_boards() {
    let c: Cmd = serde_json::from_str(r#"{"op":"rename_net","from":"GND","to":"0V"}"#).unwrap();
    assert!(matches!(c, Cmd::RenameNet { .. }) && !c.spans_both_editors() && c.domain() == Domain::Pcb);
    let c: Cmd = serde_json::from_str(r#"{"op":"delete_nets","names":["GND"]}"#).unwrap();
    assert!(matches!(c, Cmd::DeleteNets { .. }) && !c.spans_both_editors());
    let c: Cmd = serde_json::from_str(r#"{"op":"add_net","name":"N"}"#).unwrap();
    assert!(matches!(c, Cmd::AddNet { .. }));
}
