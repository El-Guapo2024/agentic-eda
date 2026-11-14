//! Swap Pad Nets and Swap Gate Nets (`pcbnew.InteractiveEdit.swapPadNets`, `pcbnew.InteractiveEdit.swapGateNets`): `EDIT_TOOL::SwapPadNets` and
//! `EDIT_TOOL::SwapGateNets` (`pcbnew/tools/edit_tool_move_fct.cpp`).
//!
//! A pad's net, here, is its pin's place in the net list that is in force (`design.nets`, else the intent's: the one netlist the board, the exports and the checks
//! read), so a swap moves pins between nets. The copper that is connected to a pad follows it, as in KiCad: every track, via and pad of the cluster the pad is in
//! (KiCad 25ec2335 `CONNECTIVITY_DATA::GetConnectedItems( pad, EXCLUDE_ZONES )`: zones are left out of the search, so a pad that only a zone fill joins to
//! the swapped one keeps its net, UI-LEFT-2 PP17; 8303b2ad's `GetConnectedItems( pad, 0 )` walked through the fills) that is on the net the pad had takes the net the pad gets (`eda_connectivity::pad_clusters`).
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
//! What KiCad 25ec2335 added is here too (UI-LEFT-3 P3-23): a board graphic on copper with a net that the cluster reaches takes the new net like a track
//! (`PCB_SHAPE_T`); every item is scheduled for one net (`ScheduleNetSwap`), and an item two pads would give two different nets refuses the swap ("Cannot swap
//! nets: connected items would receive conflicting nets."); unless Override locks is on (`Cmd::OverrideLocks`), an item whose net would change and that is
//! locked -- a pad of a locked footprint, a locked track, via or graphic, or one in a locked group -- refuses it ("Cannot swap nets of locked pads or connected
//! copper unless Override Locks is enabled."; `ValidateNetSwapLocks`, after the question, a pad "Ignore Unselected Pads" leaves alone not counted).
//!
//! A pad's net is its own (UI-LEFT-3 P3-35, `PAD::GetNetCode`): the pads of one number on two nets keep each its own (`PadMaskInfo::net`), which the swaps read
//! and set pad by pad ([`Board::pad_net`], [`Board::set_pad_nets`]); a pad named `REF.NUM#k` is the `k`-th pad of the number.
//!
//! The board then disagrees with the schematic, as it does in KiCad, until Update Schematic from PCB carries the swap back (`crate::back_annotate`: the units, the
//! pins or the labels): kicad-cli's `--schematic-parity` reports the pads (`net_conflict`) until then. One undo step each (`BOARD_COMMIT::Push( "Swap Pad Nets" )`, `"Swap Gate Nets"`).

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
pub(crate) struct PadKey {
    pub(crate) reference: String,
    pub(crate) number: String,
    pub(crate) nth: u32,
}

impl PadKey {
    /// `REF.NUM`: the pin of the net list.
    pub(crate) fn pin(&self) -> String {
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
        // `pads[i]->GetNetCode()`: each pad's own net (UI-LEFT-3 P3-35)
        let from: Vec<String> = pads.iter().map(|p| self.pad_net(&nets, p)).collect();
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
        let unit_nets: Vec<Vec<String>> = unit_pads.iter().map(|pads| pads.iter().map(|p| self.pad_net(&nets, p)).collect()).collect();
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

    /// The part both swaps share (KiCad 25ec2335 `EDIT_TOOL::SwapPadNets` / `SwapGateNets` from "Accumulate changes" on): the pad `pads[i]` goes from the net
    /// `from[i]` to `to[i]`, with the copper connected to it that is on `from[i]` -- tracks, arcs, vias, the board graphics on copper and the other pads of it
    /// (`GetConnectedItems( pad, EXCLUDE_ZONES )`, `PCB_TRACE_T`, `PCB_ARC_T`, `PCB_VIA_T`, `PCB_PAD_T`, `PCB_SHAPE_T`). Every item is scheduled once
    /// (`ScheduleNetSwap`: the selected pads first, then what each one reaches); an item scheduled for two different nets refuses the whole swap ("Cannot swap
    /// nets: connected items would receive conflicting nets."). Then the pads not asked for (`PromptConnectedPadDecision`, `connected_pads`), then the locks
    /// (`ValidateNetSwapLocks`: unless Override locks is on, an item whose net would change and is locked -- a pad of a locked footprint, a locked track, via or
    /// graphic, or one in a locked group -- refuses it, a pad left out by "Ignore Unselected Pads" not counted). See the module doc.
    fn move_nets(&mut self, pads: &[PadKey], from: &[String], to: &[String], connected_pads: Option<bool>) -> Result<(), Vec<CheckResult>> {
        const CONFLICT: &str = "Cannot swap nets: connected items would receive conflicting nets.";
        let mut nets = self.nets_in_force();
        // The copper clusters are those of a model whose nets are the ones in force.
        let mut model: ConstraintModel = self.model.clone();
        model.nets = nets.clone();
        let wanted: Vec<(String, String, u32)> = pads.iter().map(|p| (p.reference.clone(), p.number.clone(), p.nth)).collect();
        let clusters: Vec<PadCluster> = eda_connectivity::pad_clusters(&self.design, &model, &wanted);

        // `itemNewNets`: each item and the net it is to get, in the order it was scheduled
        let mut scheduled: Vec<(SwapItem, String)> = Vec::new();
        let schedule = |scheduled: &mut Vec<(SwapItem, String)>, item: SwapItem, net: &str| -> Result<(), Vec<CheckResult>> {
            match scheduled.iter().find(|(i, _)| *i == item) {
                Some((_, n)) if n != net => Err(refuse("ops_swap_conflict", "swap_nets", CONFLICT)),
                Some(_) => Ok(()),
                None => {
                    scheduled.push((item, net.to_string()));
                    Ok(())
                }
            }
        };
        // "Include the direct pad changes so connectivity cannot assign them a conflicting net."
        for (pad, net) in pads.iter().zip(to) {
            schedule(&mut scheduled, SwapItem::Pad(pad.clone()), net)?;
        }
        let selected: BTreeSet<&PadKey> = pads.iter().collect();
        let mut unselected: BTreeSet<PadKey> = BTreeSet::new();
        let rt = self.design.routing.as_ref();
        let shape_net = |id: &str| self.design.drawings.as_ref().and_then(|d| d.board_items.net_of(id)).unwrap_or("").to_string();
        for (i, cluster) in clusters.iter().enumerate() {
            let mut reached: Vec<(SwapItem, String)> = Vec::new();
            for id in &cluster.tracks {
                if let Some(t) = rt.and_then(|r| r.tracks.iter().find(|t| &t.id == id)) {
                    reached.push((SwapItem::Track(id.clone()), t.net.clone()));
                }
            }
            for id in &cluster.vias {
                if let Some(v) = rt.and_then(|r| r.vias.iter().find(|v| &v.id == id)) {
                    reached.push((SwapItem::Via(id.clone()), v.net.clone()));
                }
            }
            for id in &cluster.shapes {
                reached.push((SwapItem::Shape(id.clone()), shape_net(id)));
            }
            for (reference, number, nth) in &cluster.pads {
                let key = PadKey { reference: reference.clone(), number: number.clone(), nth: *nth };
                // (`GetConnectedItems` leaves out the item asked about)
                if key == pads[i] {
                    continue;
                }
                let net = self.pad_net(&nets, &key);
                reached.push((SwapItem::Pad(key), net));
            }
            for (item, net) in reached {
                if net != from[i] {
                    continue;
                }
                schedule(&mut scheduled, item.clone(), &to[i])?;
                if let SwapItem::Pad(key) = item {
                    if !selected.contains(&key) {
                        unselected.insert(key);
                    }
                }
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

        // The net each scheduled item has now.
        let current = |item: &SwapItem| -> String {
            match item {
                SwapItem::Pad(key) => self.pad_net(&nets, key),
                SwapItem::Track(id) => rt.and_then(|r| r.tracks.iter().find(|t| &t.id == id)).map(|t| t.net.clone()).unwrap_or_default(),
                SwapItem::Via(id) => rt.and_then(|r| r.vias.iter().find(|v| &v.id == id)).map(|v| v.net.clone()).unwrap_or_default(),
                SwapItem::Shape(id) => shape_net(id),
            }
        };
        // `ValidateNetSwapLocks`
        if !self.override_locks {
            for (item, net) in &scheduled {
                if current(item) == *net {
                    continue;
                }
                let locked = match item {
                    SwapItem::Pad(key) if !include_connected && !selected.contains(key) => false,
                    // `PAD::IsLocked`: the footprint's lock (or its group's)
                    SwapItem::Pad(key) => self.locked_with_group(&key.reference),
                    SwapItem::Track(id) | SwapItem::Via(id) | SwapItem::Shape(id) => self.locked_with_group(id),
                };
                if locked {
                    return Err(refuse("ops_locked", "swap_nets", "Cannot swap nets of locked pads or connected copper unless Override Locks is enabled."));
                }
            }
        }

        // Apply: the selected pads get their new nets directly, the items connected to them theirs, the pads among them as the decision says.
        let mut pad_moves: Vec<(PadKey, String)> = Vec::new();
        let (mut track_net, mut via_net, mut shape_nets): (BTreeMap<String, String>, BTreeMap<String, String>, BTreeMap<String, String>) = Default::default();
        for (item, net) in scheduled {
            if current(&item) == net {
                continue;
            }
            match item {
                SwapItem::Pad(key) => {
                    if selected.contains(&key) || include_connected {
                        pad_moves.push((key, net));
                    }
                }
                SwapItem::Track(id) => {
                    track_net.insert(id, net);
                }
                SwapItem::Via(id) => {
                    via_net.insert(id, net);
                }
                SwapItem::Shape(id) => {
                    shape_nets.insert(id, net);
                }
            }
        }
        self.set_pad_nets(&mut nets, &pad_moves);
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
        if !shape_nets.is_empty() {
            let items = &mut self.drawings_mut().board_items;
            for (id, n) in shape_nets {
                items.set_net(&id, Some(n).filter(|n| !n.is_empty()));
            }
        }
        self.sort_routing();
        Ok(())
    }

    /// The board's footprint of `reference` (`model.footprint_of`, whose pads the footprint's pad records follow; `footprint` when the command has put another one on
    /// the part, which the model does not know yet) and its record's pads, when the two line up.
    fn pad_records(&self, reference: &str, footprint: Option<&eda_model::Footprint>) -> Option<(eda_model::Footprint, Option<&[eda_model::ir::PadMaskInfo]>)> {
        let footprint = match footprint {
            Some(f) => f.clone(),
            None => self.model.footprint_of(self.model.part(reference)?)?,
        };
        let records = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)).map(|e| e.pads.as_slice()).filter(|p| p.len() == footprint.pads.len());
        Some((footprint, records))
    }

    /// `PAD::GetNetname` of one pad of the board (UI-LEFT-3 P3-35): its own net where its number is on two (`PadMaskInfo::net`, `""` none), else its number's
    /// in the net list `nets`. KiCad keeps a net per `PAD`; the model one per `REF.NUMBER`, with the pads of a split number holding their own.
    pub(crate) fn pad_net(&self, nets: &[Net], key: &PadKey) -> String {
        self.own_pad_net(key, None).unwrap_or_else(|| net_of(nets, &key.pin()))
    }

    /// The pad's own net (`PadMaskInfo::net`, `Some("")` for none), `None` where it has its number's; `footprint` as for [`Self::pad_records`].
    pub(crate) fn own_pad_net(&self, key: &PadKey, footprint: Option<&eda_model::Footprint>) -> Option<String> {
        let (footprint, records) = self.pad_records(&key.reference, footprint)?;
        let k = footprint.pads.iter().enumerate().filter(|(_, p)| p.number == key.number).nth(key.nth.saturating_sub(1) as usize)?.0;
        records?.get(k)?.net.clone()
    }

    /// `PAD::SetNetCode` for each of `moves` (UI-LEFT-3 P3-35): each pad takes its net, the other pads keep theirs. Where the pads of one number then share one
    /// net, that is the number's in the net list `nets` and none keeps a net of its own; where they differ, each keeps its own (`PadMaskInfo::net`, the record
    /// made where the footprint has none, as a renamed pad's is) and the number is on each of their nets, as a board read from a file has it.
    pub(crate) fn set_pad_nets(&mut self, nets: &mut Vec<Net>, moves: &[(PadKey, String)]) {
        self.set_pad_nets_on(nets, moves, None);
    }

    /// [`Self::set_pad_nets`] with the footprint the command has put the part on (`footprint`, for moves of that one part), which the model does not know yet.
    pub(crate) fn set_pad_nets_on(&mut self, nets: &mut Vec<Net>, moves: &[(PadKey, String)], footprint: Option<&eda_model::Footprint>) {
        let mut by_footprint: BTreeMap<&str, Vec<&(PadKey, String)>> = BTreeMap::new();
        for m in moves {
            by_footprint.entry(m.0.reference.as_str()).or_default().push(m);
        }
        for (reference, moves) in by_footprint {
            let Some((footprint, _)) = self.pad_records(reference, footprint) else {
                // no geometry: the number's net, as before
                for (key, net) in moves {
                    move_pin(nets, &key.pin(), net);
                }
                continue;
            };
            // every pad's net now, then the moves
            let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
            let keys: Vec<PadKey> = footprint
                .pads
                .iter()
                .map(|p| {
                    let n = seen.entry(p.number.as_str()).or_insert(0);
                    *n += 1;
                    PadKey { reference: reference.to_string(), number: p.number.clone(), nth: *n }
                })
                .collect();
            let mut now: Vec<String> = keys.iter().map(|k| self.own_pad_net(k, Some(&footprint)).unwrap_or_else(|| net_of(nets, &k.pin()))).collect();
            let mut numbers: BTreeSet<String> = BTreeSet::new();
            for (key, net) in moves {
                if let Some(k) = keys.iter().position(|k| k == key) {
                    now[k] = net.clone();
                    numbers.insert(key.number.clone());
                } else {
                    move_pin(nets, &key.pin(), net);
                }
            }
            for number in numbers {
                let idx: Vec<usize> = (0..keys.len()).filter(|&k| keys[k].number == number && footprint.pads[k].kind != eda_model::PadKind::NonPlatedHole).collect();
                let distinct: BTreeSet<&String> = idx.iter().map(|&k| &now[k]).collect();
                let pin = format!("{reference}.{number}");
                for n in nets.iter_mut() {
                    n.pins.retain(|p| *p != pin);
                }
                for net in distinct.iter().filter(|n| !n.is_empty()) {
                    match nets.iter_mut().find(|n| n.name == **net) {
                        Some(n) => insert_pin(&mut n.pins, pin.clone()),
                        None => nets.push(Net { name: (*net).clone(), pins: vec![pin.clone()] }),
                    }
                }
                let split = distinct.len() > 1;
                let own: Vec<(usize, Option<String>)> = idx.iter().map(|&k| (k, split.then(|| now[k].clone()))).collect();
                self.set_own_pad_nets(reference, &footprint, &own);
            }
        }
    }

    /// The pad records' own nets (`PadMaskInfo::net`) of `reference`'s pads `own` (index in the footprint, net or `None` for the number's). A record is made for a
    /// footprint that has none only when a pad needs a net of its own; a footprint made here gets each pad's pin function and type in it, as `pad_renumber` does.
    fn set_own_pad_nets(&mut self, reference: &str, footprint: &eda_model::Footprint, own: &[(usize, Option<String>)]) {
        let needed = own.iter().any(|(_, n)| n.is_some());
        let pad_count = footprint.pads.len();
        let pins: Vec<(String, String)> = footprint.pads.iter().map(|p| self.model.symbol_links.get(reference).and_then(|l| l.pins.get(&p.number)).cloned().unwrap_or_default()).collect();
        let has = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)).is_some_and(|e| e.pads.len() == pad_count);
        if !has && !needed {
            return;
        }
        let dr = self.drawings_mut();
        if !dr.footprint_extras.iter().any(|e| e.id == reference) {
            dr.footprint_extras.push(eda_model::ir::FootprintExtra { id: reference.to_string(), ..Default::default() });
        }
        let extra = dr.footprint_extras.iter_mut().find(|e| e.id == reference).expect("just made");
        if extra.pads.len() != pad_count {
            let made_here = !extra.from_library;
            extra.pads = pins
                .into_iter()
                .map(|(pin_function, pin_type)| if made_here { eda_model::ir::PadMaskInfo { pin_function, pin_type, ..Default::default() } } else { Default::default() })
                .collect();
        }
        for (k, net) in own {
            if let Some(p) = extra.pads.get_mut(*k) {
                p.net = net.clone();
            }
        }
    }
}

/// An item a swap of nets changes (`BOARD_CONNECTED_ITEM*` of `itemNewNets`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum SwapItem {
    Pad(PadKey),
    Track(String),
    Via(String),
    Shape(String),
}

/// The pin `pin` taken off every net and put on `net` (none for `""`).
fn move_pin(nets: &mut Vec<Net>, pin: &str, net: &str) {
    for n in nets.iter_mut() {
        n.pins.retain(|p| p != pin);
    }
    if !net.is_empty() {
        match nets.iter_mut().find(|n| n.name == net) {
            Some(n) => insert_pin(&mut n.pins, pin.to_string()),
            None => nets.push(Net { name: net.to_string(), pins: vec![pin.to_string()] }),
        }
    }
}

impl Board<'_> {
    /// `Cmd::TrackViaNets`: `DIALOG_TRACK_VIA_PROPERTIES::TransferDataFromWindow` from `std::set<int> shortingNets` on, on the board as the batch's earlier edits left it.
    ///
    /// `connected_tracks` are the tracks and vias of the copper the selected ones are in (`GetConnectedItems`, here the same-net flood of the connectivity graph, with
    /// zones that have no fill computed: they connect nothing). For each of them, every other track or via whose net is neither its own nor the new one, and every pad
    /// not on the new net, that its copper touches (`collide`, on a layer both have) is a short (`shortingNets`) -- or, for a pad of its own net, a pad the net
    /// change takes along (`changingPads`). A short asks first (`confirmShortingNets`; Cancel Changes is `commit.Revert()`: the studio sends nothing); then, when the
    /// net is set, the pads (`confirmPadChange`): Change Nets sets the selected tracks and vias and those pads to the net (a pad's net is its pin's place in the net
    /// list in force), Leave Nets Unchanged leaves every net.
    pub(crate) fn track_via_nets(&mut self, ids: &[String], net: Option<&str>, apply_anyway: Option<bool>, change_pads: Option<bool>) -> Result<(), Vec<CheckResult>> {
        use eda_connectivity::ItemRef;
        let rt = self.design.routing.as_ref();
        let selected_tracks: BTreeSet<&str> = ids.iter().map(String::as_str).filter(|id| rt.is_some_and(|r| r.tracks.iter().any(|t| t.id == *id))).collect();
        let selected_vias: BTreeSet<&str> = ids.iter().map(String::as_str).filter(|id| rt.is_some_and(|r| r.vias.iter().any(|v| v.id == *id))).collect();
        if let Some(n) = net {
            self.known_net(n)?;
        }
        let nets = self.nets_in_force();
        let mut model: ConstraintModel = self.model.clone();
        model.nets = nets.clone();
        let graph = eda_connectivity::build_graph(&self.design, &model, &eda_drc::zone_copper::ZoneCopper::default());
        let pad_of = eda_connectivity::flash::pad_of_items(&graph);
        // the graph's items of a track or via
        let key = |i: usize| -> Option<(bool, &str)> {
            match &graph.items[i].item {
                ItemRef::TrackSeg { track_id, .. } => Some((true, track_id.as_str())),
                ItemRef::Via { via_id } => Some((false, via_id.as_str())),
                _ => None,
            }
        };
        // `connected_tracks`: the same-net flood from the selected tracks and vias
        let mut in_cluster = vec![false; graph.items.len()];
        let mut queue: Vec<usize> = (0..graph.items.len()).filter(|&i| key(i).is_some_and(|(t, id)| if t { selected_tracks.contains(id) } else { selected_vias.contains(id) })).collect();
        for &i in &queue {
            in_cluster[i] = true;
        }
        while let Some(cur) = queue.pop() {
            for &nb in &graph.items[cur].connected {
                if !in_cluster[nb] && graph.items[nb].net == graph.items[cur].net && !matches!(graph.items[nb].item, ItemRef::ZoneOutline { .. } | ItemRef::Shape { .. }) {
                    in_cluster[nb] = true;
                    queue.push(nb);
                }
            }
        }
        let new_net = net.unwrap_or("\u{0}indeterminate");
        let mut shorting: BTreeSet<String> = BTreeSet::new();
        let mut changing: Vec<(String, String, u32)> = Vec::new();
        for i in (0..graph.items.len()).filter(|&i| in_cluster[i] && key(i).is_some()) {
            let mine = &graph.items[i].net;
            for &o in &graph.items[i].connected {
                let other = &graph.items[o];
                match &other.item {
                    ItemRef::TrackSeg { .. } | ItemRef::Via { .. } => {
                        if other.net != *mine && other.net != new_net {
                            shorting.insert(other.net.clone());
                        }
                    }
                    ItemRef::Pad { .. } => {
                        if other.net == new_net {
                            continue;
                        }
                        if other.net == *mine {
                            if let Some(p) = pad_of[o].clone().filter(|p| !changing.contains(p)) {
                                changing.push(p);
                            }
                        } else {
                            shorting.insert(other.net.clone());
                        }
                    }
                    // the loop asks the board's tracks and pads only
                    ItemRef::ZoneOutline { .. } | ItemRef::Shape { .. } => {}
                }
            }
        }
        // `m_netSelector->GetValue()`: the net's name, `<no net>` for none, `INDETERMINATE_STATE` ("-- mixed values --") when the box is indeterminate
        let shown = |n: &str| if n.is_empty() { "<no net>".to_string() } else { n.to_string() };
        let selector = net.map(shown).unwrap_or_else(|| "-- mixed values --".to_string());
        if !shorting.is_empty() && apply_anyway != Some(true) {
            let message = if shorting.len() == 1 {
                // `FindNet( *shortingNets.begin() )->GetNetname()`: net 0's name is "".
                format!("Applying these changes will short net {selector} with {}.", shorting.iter().next().unwrap())
            } else {
                format!("Applying these changes will short net {selector} with other nets.")
            };
            return Err(vec![CheckResult::fail("ops_net_change_shorts", "track_via_nets", message).with_detail(serde_json::json!({ "nets": shorting.iter().collect::<Vec<_>>() }))]);
        }
        let Some(net) = net else { return Ok(()) };
        if !changing.is_empty() {
            let pad_ids: Vec<String> = changing.iter().map(|(r, n, k)| PadKey { reference: r.clone(), number: n.clone(), nth: *k }.id()).collect();
            match change_pads {
                None => {
                    let message = match changing.as_slice() {
                        [(r, n, _)] => format!("Changing the net will also update {r} pad {n} to {selector}."),
                        [(r1, n1, _), (r2, n2, _)] => format!("Changing the net will also update {r1} pad {n1} and {r2} pad {n2} to {selector}."),
                        _ => format!("Changing the net will also update {} connected pads to {selector}.", changing.len()),
                    };
                    return Err(vec![CheckResult::fail("ops_net_change_pads", "track_via_nets", message).with_detail(serde_json::json!({ "pads": pad_ids }))]);
                }
                Some(false) => return Ok(()),
                Some(true) => {}
            }
        }
        // `track->SetNetCode( newNetCode )` for the selected ones, `pad->SetNetCode( newNetCode )` for the pads
        if let Some(rt) = self.design.routing.as_mut() {
            for t in rt.tracks.iter_mut().filter(|t| selected_tracks.contains(t.id.as_str())) {
                t.net = net.to_string();
            }
            for v in rt.vias.iter_mut().filter(|v| selected_vias.contains(v.id.as_str())) {
                v.net = net.to_string();
            }
        }
        if !changing.is_empty() {
            // each pad its own net (`pad->SetNetCode( newNetCode )`, UI-LEFT-3 P3-35)
            let mut nets = nets;
            let moves: Vec<(PadKey, String)> = changing.iter().map(|(r, n, k)| (PadKey { reference: r.clone(), number: n.clone(), nth: *k }, net.to_string())).collect();
            self.set_pad_nets(&mut nets, &moves);
            self.design.nets = Some(nets);
        }
        self.sort_routing();
        Ok(())
    }
}
