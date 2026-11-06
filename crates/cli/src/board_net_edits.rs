//! The Net Inspector's renames and deletes of the board's nets, laid over the model each time the board is loaded (`DrawingsSection::net_renames`; the
//! verbs are `crates/ops/src/net_edit.rs`).
//!
//! KiCad's Rename Selected Net and Delete Selected Net(s) (`PCB_NET_INSPECTOR_PANEL::onRenameSelectedNet`, `onDeleteSelectedNet`) change the board's
//! `NETINFO_ITEM` and nothing else: the schematic keeps its name for the net, and the next Update PCB from Schematic would bring it back. Here the board's
//! nets are named from the schematic (or the intent) on every load (`net_names::kicad_names`), so the edit is laid on after that, last:
//!
//! * a rename gives the net its new name (the pads on it follow; the copper the verb renamed already); a net the board has under the new name too (a
//!   footprint that exists only on the board, whose pads the verb renamed) is one net with the pins of both. The net keeps the intent name it is drawn
//!   from (`intent_net_names`), so a design routed under the intent's names still finds its copper (`net_names::rename_design`).
//! * a delete takes every pin off the net (`BOARD::Remove` puts its pads on the unconnected net); the name stays only when Add Net made it again
//!   (`board_nets`), with no pin.
//!
//! The net classes are the patterns' (a renamed net is in the classes whose patterns its new name matches, as after KiCad's
//! `SynchronizeNetsAndNetClasses`), and the custom rules that name a net are left as they are.

use eda_model::ir::Design;
use eda_model::ConstraintModel;

/// Lays `design`'s net edits over `model`'s nets. An edit whose net the model does not have does nothing.
pub(crate) fn apply(design: &Design, model: &mut ConstraintModel) {
    let Some(d) = design.drawings.as_ref() else { return };
    for e in &d.net_renames {
        let Some(i) = model.nets.iter().position(|n| n.name == e.from) else { continue };
        if e.to.is_empty() {
            if d.board_nets.iter().any(|n| *n == e.from) {
                model.nets[i].pins.clear();
            } else {
                model.nets.remove(i);
            }
            continue;
        }
        let net = model.nets.remove(i);
        // what the schematic is drawn from: the name the intent gave, through the rename
        if let Some(intent) = model.intent_net_names.remove(&e.from) {
            model.intent_net_names.insert(e.to.clone(), intent);
        }
        for pour in model.board.pours.iter_mut().filter(|p| p.net == e.from) {
            pour.net = e.to.clone();
        }
        match model.nets.iter_mut().find(|n| n.name == e.to) {
            Some(same) => {
                for p in net.pins {
                    if !same.pins.contains(&p) {
                        same.pins.push(p);
                    }
                }
            }
            None => model.nets.insert(i.min(model.nets.len()), eda_model::Net { name: e.to.clone(), pins: net.pins }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{DrawingsSection, NetRename};
    use eda_model::Net;

    fn names(m: &ConstraintModel) -> Vec<(String, Vec<String>)> {
        m.nets.iter().map(|n| (n.name.clone(), n.pins.clone())).collect()
    }

    fn net(name: &str, pins: &[&str]) -> Net {
        Net { name: name.into(), pins: pins.iter().map(|p| p.to_string()).collect() }
    }

    fn design(renames: &[(&str, &str)], board_nets: &[&str]) -> Design {
        let drawings = DrawingsSection {
            net_renames: renames.iter().map(|(a, b)| NetRename { from: a.to_string(), to: b.to_string() }).collect(),
            board_nets: board_nets.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let mut d: Design = serde_json::from_str(r#"{"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap();
        d.drawings = Some(drawings);
        d
    }

    /// A rename names the net anew with its pins and the intent name it is drawn from; a delete takes its pins off; an edit of a net the model has not is nothing.
    #[test]
    fn renames_and_deletes_are_laid_over_the_nets() {
        let mut m = ConstraintModel { nets: vec![net("GND", &["R1.1", "U1.4"]), net("/s/A", &["R1.2"]), net("B", &["U1.1"])], ..Default::default() };
        m.intent_net_names.insert("GND".into(), "GND".into());
        apply(&design(&[("GND", "0V"), ("B", ""), ("NOPE", "X")], &[]), &mut m);
        assert_eq!(names(&m), vec![("0V".to_string(), vec!["R1.1".to_string(), "U1.4".to_string()]), ("/s/A".to_string(), vec!["R1.2".to_string()])]);
        assert_eq!(m.intent_net_names.get("0V").map(String::as_str), Some("GND"));
        assert!(!m.intent_net_names.contains_key("GND"));
    }

    /// A deleted net that Add Net made again stays, with no pin; a renamed net meeting the same name (a board part's pads) is one net.
    #[test]
    fn a_net_added_again_stays_and_a_rename_onto_a_board_part_net_merges() {
        let mut m = ConstraintModel { nets: vec![net("A", &["R1.1"]), net("N", &["R2.1"]), net("M", &["J9.1"])], ..Default::default() };
        apply(&design(&[("A", ""), ("N", "M")], &["A"]), &mut m);
        assert_eq!(names(&m), vec![("A".to_string(), vec![]), ("M".to_string(), vec!["J9.1".to_string(), "R2.1".to_string()])]);
    }
}
