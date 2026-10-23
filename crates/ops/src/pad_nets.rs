//! Swap Pad Nets and Swap Gate Nets (`pcbnew.InteractiveEdit.swapPadNets`, `pcbnew.InteractiveEdit.swapGateNets`): `EDIT_TOOL::SwapPadNets` and
//! `EDIT_TOOL::SwapGateNets` (`pcbnew/tools/edit_tool_move_fct.cpp`).
//!
//! A pad's net, here, is its pin's place in the net list that is in force (`design.nets`, else the intent's: the one netlist the board, the exports and the checks
//! read), so a swap moves pins between nets. The copper that is connected to a pad follows it, as in KiCad: every track, via and pad of the cluster the pad is in
//! (`CONNECTIVITY_DATA::GetConnectedItems( pad, 0 )`, zones left out) that is on the net the pad had takes the net the pad gets (`eda_connectivity::pad_clusters`).
//! The pads of that copper that were not asked for are KiCad's `PromptConnectedPadDecision`: "N unselected pad(s) are connected to these nets. How do you want to
//! proceed?" -- "Ignore Unselected Pads" leaves their nets, "Swap All Connected Pads" moves them too. The decision is the verb's `connected_pads`; left out, a swap that
//! has such pads is refused with `ops_swap_unselected_pads` (the pads in its `detail`: `{ "pads": ["R2.1", ..] }`) so that the studio can ask, and a swap that has none just happens.
//!
//! * Swap Pad Nets rotates the nets of the selected pads in selection order: the pad `i` gets the net the pad `i + 1` had, the last gets the first's. Two or more pads;
//!   all on one net is nothing to do.
//! * Swap Gate Nets does the same for the pads of whole units of a multi-unit footprint, position by position (`FOOTPRINT::GetUnitInfo`, [`eda_model::fp_units`]): the
//!   units of the selected pads in selection order, or one unit and the name of the unit to swap it with (the context menu's "Swap with B"). The units need as many pins
//!   as each other, and every pad of them must exist.
//!
//! The board then disagrees with the schematic, as it does in KiCad, until Update Schematic from PCB carries the swap back: kicad-cli's `--schematic-parity` reports
//! the pads (`net_conflict`). One undo step each (`BOARD_COMMIT::Push( "Swap Pad Nets" )`, `"Swap Gate Nets"`).

use super::Board;
use eda_connectivity::PadCluster;
use eda_model::fp_edit::{pad_id, parse_pad_id};
use eda_model::fp_units::{unit_info, FootprintUnit};
use eda_model::{CheckResult, ConstraintModel, Net};
use std::collections::{BTreeMap, BTreeSet};

fn refuse(check: &str, what: &str, message: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, message.into())]
}

/// One pad of the board: its footprint, its number and which of the pads with that number it is (1-based).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PadKey {
    reference: String,
    number: String,
    nth: u32,
}

impl PadKey {
    /// `REF.NUM`: the pin of the net list.
    fn pin(&self) -> String {
        format!("{}.{}", self.reference, self.number)
    }

    /// The studio's id of the pad (`REF.NUM`, `REF.NUM#k`).
    fn id(&self) -> String {
        pad_id(&self.reference, &self.number, self.nth)
    }
}

/// The net of a pin in a net list, `""` for none.
fn net_of(nets: &[Net], pin: &str) -> String {
    nets.iter().find(|n| n.pins.iter().any(|p| p == pin)).map(|n| n.name.clone()).unwrap_or_default()
}

/// A pin put into a net's list where it sorts, when the list is sorted, else at the end.
fn insert_pin(pins: &mut Vec<String>, pin: String) {
    if pins.iter().any(|p| *p == pin) {
        return;
    }
    if pins.windows(2).all(|w| w[0] <= w[1]) {
        let at = pins.partition_point(|p| *p < pin);
        pins.insert(at, pin);
    } else {
        pins.push(pin);
    }
}

impl Board<'_> {
    /// The net list in force: the design's own, else the intent's.
    fn nets_in_force(&self) -> Vec<Net> {
        self.design.nets.clone().unwrap_or_else(|| self.model.nets.clone())
    }

    /// The pads `ids` name, in order, each once; refused when an id is no pad of a placed footprint.
    fn pad_keys(&self, ids: &[String]) -> Result<Vec<PadKey>, Vec<CheckResult>> {
        let mut keys: Vec<PadKey> = Vec::new();
        for id in ids {
            let Some((reference, number, nth)) = parse_pad_id(id).filter(|(r, _, _)| self.pose_of(r).is_some()) else {
                return Err(refuse("ops_unknown_item", id, "no pad of a footprint on the board with this id"));
            };
            let part = self.model.part(&reference).ok_or_else(|| refuse("ops_unknown_item", id, "no such part"))?;
            let footprint = self.unedited_footprint(part).ok_or_else(|| refuse("ops_no_footprint", &reference, "this part has no footprint geometry"))?;
            if eda_model::fp_edit::pad_index(&footprint, &number, nth).is_none() {
                return Err(refuse("ops_unknown_pad", id, format!("{reference} has no pad {number}")));
            }
            let key = PadKey { reference, number, nth };
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        Ok(keys)
    }

    /// `Cmd::SwapPadNets`.
    pub(crate) fn swap_pad_nets(&mut self, ids: &[String], connected_pads: Option<bool>) -> Result<(), Vec<CheckResult>> {
        let pads = self.pad_keys(ids)?;
        if pads.len() < 2 {
            return Err(refuse("ops_bad_swap", "swap_pad_nets", "Select two or more pads to swap their nets."));
        }
        let nets = self.nets_in_force();
        let from: Vec<String> = pads.iter().map(|p| net_of(&nets, &p.pin())).collect();
        // `allSame`: nothing to do.
        if from.iter().all(|n| *n == from[0]) {
            return Err(refuse("ops_nothing_to_change", "swap_pad_nets", "All the selected pads are on the same net: there is nothing to swap."));
        }
        // The new net of the pad `i` is the net of the pad `i + 1`, the last pad taking the first's.
        let to: Vec<String> = (0..pads.len()).map(|i| from[(i + 1) % pads.len()].clone()).collect();
        self.move_nets(&pads, &from, &to, connected_pads)
    }

    /// The units of a footprint on the board (`FOOTPRINT::GetUnitInfo`): the board file's own `(units ..)`, else what the schematic symbol's library symbol says
    /// (`LIB_SYMBOL::GetUnitPinInfo`, which the netlist hands the footprint).
    pub(crate) fn unit_info_of(&self, reference: &str) -> Vec<FootprintUnit> {
        unit_info(&self.design, self.model, reference)
    }

    /// `Cmd::SwapGateNets`.
    pub(crate) fn swap_gate_nets(&mut self, ids: &[String], target: Option<&str>, connected_pads: Option<bool>) -> Result<(), Vec<CheckResult>> {
        const ONE_FOOTPRINT: &str = "Gate swapping must be performed on pads within one multi-gate footprint.";
        let selected = self.pad_keys(ids).map_err(|_| refuse("ops_bad_swap", "swap_gate_nets", ONE_FOOTPRINT))?;
        let Some(first) = selected.first() else { return Err(refuse("ops_bad_swap", "swap_gate_nets", ONE_FOOTPRINT)) };
        if selected.iter().any(|p| p.reference != first.reference) {
            return Err(refuse("ops_bad_swap", "swap_gate_nets", ONE_FOOTPRINT));
        }
        let reference = first.reference.clone();
        let units = self.unit_info_of(&reference);
        if units.len() < 2 {
            return Err(refuse("ops_bad_swap", "swap_gate_nets", ONE_FOOTPRINT));
        }

        // The units that have a pad selected, in the order the selection reaches them.
        let mut unit_order: Vec<usize> = Vec::new();
        for pad in &selected {
            if let Some(i) = units.iter().position(|u| u.pins.iter().any(|p| *p == pad.number)) {
                if !unit_order.contains(&i) {
                    unit_order.push(i);
                }
            }
        }
        let active: Vec<usize> = if unit_order.len() >= 2 {
            unit_order
        } else if unit_order.len() == 1 && target.is_some() {
            let source = unit_order[0];
            let name = target.unwrap_or_default();
            // The last of the other units that has as many pins and the name (the loop of the source keeps the last match).
            let found = units.iter().enumerate().filter(|(i, u)| *i != source && u.pins.len() == units[source].pins.len() && u.name == name).map(|(i, _)| i).last();
            match found {
                Some(t) => vec![source, t],
                None => return Err(refuse("ops_bad_swap", "swap_gate_nets", ONE_FOOTPRINT)),
            }
        } else {
            return Err(refuse("ops_bad_swap", "swap_gate_nets", ONE_FOOTPRINT));
        };

        let pin_count = units[active[0]].pins.len();
        if active.iter().any(|&u| units[u].pins.len() != pin_count) {
            return Err(refuse("ops_bad_swap", "swap_gate_nets", "Gate swapping must be performed on gates with equal pin counts."));
        }

        // The pads of each unit, by position in the unit (`FindPadByNumber`: the first pad with the number).
        let part = self.model.part(&reference).ok_or_else(|| refuse("ops_unknown_item", &reference, "no such part"))?;
        let footprint = self.unedited_footprint(part).ok_or_else(|| refuse("ops_no_footprint", &reference, "this part has no footprint geometry"))?;
        let nets = self.nets_in_force();
        let mut unit_pads: Vec<Vec<PadKey>> = Vec::new();
        for &u in &active {
            let mut pads = Vec::new();
            for number in &units[u].pins {
                if eda_model::fp_edit::pad_index(&footprint, number, 1).is_none() {
                    return Err(refuse("ops_bad_swap", "swap_gate_nets", "Gate swapping failed: pad in unit missing from footprint."));
                }
                pads.push(PadKey { reference: reference.clone(), number: number.clone(), nth: 1 });
            }
            unit_pads.push(pads);
        }
        let unit_nets: Vec<Vec<String>> = unit_pads.iter().map(|pads| pads.iter().map(|p| net_of(&nets, &p.pin())).collect()).collect();
        // `allSame`: every unit has the nets of the first at every position.
        if (0..pin_count).all(|pi| unit_nets.iter().all(|nets| nets[pi] == unit_nets[0][pi])) {
            return Err(refuse("ops_nothing_to_change", "swap_gate_nets", "Gate swapping has no effect: all selected gates have identical nets."));
        }

        // Position by position, each unit's pad takes the net the next unit's has there.
        let unit_count = active.len();
        let (mut pads, mut from, mut to) = (Vec::new(), Vec::new(), Vec::new());
        for pi in 0..pin_count {
            for ui in 0..unit_count {
                pads.push(unit_pads[ui][pi].clone());
                from.push(unit_nets[ui][pi].clone());
                to.push(unit_nets[(ui + 1) % unit_count][pi].clone());
            }
        }
        self.move_nets(&pads, &from, &to, connected_pads)
    }

    /// The part both swaps share: the pad `pads[i]` goes from the net `from[i]` to `to[i]`, with the copper connected to it that is on `from[i]` -- tracks and vias
    /// always, the other pads of it as `connected_pads` says. See the module doc.
    fn move_nets(&mut self, pads: &[PadKey], from: &[String], to: &[String], connected_pads: Option<bool>) -> Result<(), Vec<CheckResult>> {
        let mut nets = self.nets_in_force();
        // The copper clusters are those of a model whose nets are the ones in force.
        let mut model: ConstraintModel = self.model.clone();
        model.nets = nets.clone();
        let wanted: Vec<(String, String, u32)> = pads.iter().map(|p| (p.reference.clone(), p.number.clone(), p.nth)).collect();
        let clusters: Vec<PadCluster> = eda_connectivity::pad_clusters(&self.design, &model, &wanted);

        let selected: BTreeSet<&PadKey> = pads.iter().collect();
        let mut track_net: BTreeMap<String, String> = BTreeMap::new();
        let mut via_net: BTreeMap<String, String> = BTreeMap::new();
        let mut pad_net: BTreeMap<PadKey, String> = BTreeMap::new();
        let mut unselected: BTreeSet<PadKey> = BTreeSet::new();
        let rt = self.design.routing.as_ref();
        for (i, cluster) in clusters.iter().enumerate() {
            for id in &cluster.tracks {
                if rt.and_then(|r| r.tracks.iter().find(|t| &t.id == id)).is_some_and(|t| t.net == from[i]) {
                    track_net.insert(id.clone(), to[i].clone());
                }
            }
            for id in &cluster.vias {
                if rt.and_then(|r| r.vias.iter().find(|v| &v.id == id)).is_some_and(|v| v.net == from[i]) {
                    via_net.insert(id.clone(), to[i].clone());
                }
            }
            for (reference, number, nth) in &cluster.pads {
                let key = PadKey { reference: reference.clone(), number: number.clone(), nth: *nth };
                if net_of(&nets, &key.pin()) != from[i] {
                    continue;
                }
                // The selected pads get their new nets directly (below), whichever cluster they are found in.
                if selected.contains(&key) {
                    continue;
                }
                unselected.insert(key.clone());
                pad_net.insert(key, to[i].clone());
            }
        }

        // `PromptConnectedPadDecision`.
        let include_connected = match (unselected.is_empty(), connected_pads) {
            (true, _) => true,
            (false, Some(include)) => include,
            (false, None) => {
                let list: Vec<String> = unselected.iter().map(PadKey::id).collect();
                let why = format!("{} unselected pad(s) are connected to these nets. How do you want to proceed?", list.len());
                return Err(vec![CheckResult::fail("ops_swap_unselected_pads", "swap_nets", why).with_detail(serde_json::json!({ "pads": list }))]);
            }
        };

        // The selected pads get their new nets directly (`pads[i]->SetNetCode( newNetForIndex( i ) )`), the pads connected to them when the decision includes them.
        let moves = pads.iter().cloned().zip(to.iter().cloned()).chain(pad_net.into_iter().filter(|_| include_connected));
        for (key, new_net) in moves {
            let pin = key.pin();
            for net in nets.iter_mut() {
                net.pins.retain(|p| *p != pin);
            }
            if !new_net.is_empty() {
                match nets.iter_mut().find(|n| n.name == new_net) {
                    Some(net) => insert_pin(&mut net.pins, pin),
                    None => nets.push(Net { name: new_net, pins: vec![pin] }),
                }
            }
        }
        self.design.nets = Some(nets);
        if let Some(rt) = self.design.routing.as_mut() {
            for t in rt.tracks.iter_mut() {
                if let Some(n) = track_net.get(&t.id) {
                    t.net = n.clone();
                }
            }
            for v in rt.vias.iter_mut() {
                if let Some(n) = via_net.get(&v.id) {
                    v.net = n.clone();
                }
            }
        }
        self.sort_routing();
        Ok(())
    }
}
