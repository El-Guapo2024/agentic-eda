//! The Net Inspector's net edits (`PCB_NET_INSPECTOR_PANEL`, `pcbnew/widgets/pcb_net_inspector_panel.cpp`): Rename Selected Net...
//! (`onRenameSelectedNet`), Delete Selected Net(s) (`onDeleteSelectedNet`) and Add Net... (`onAddNet`).
//!
//! A net of the board, here, is a name of the model the board is loaded with (the schematic's or the intent's nets under KiCad's names, a net a design
//! block or Add Net brought -- `DrawingsSection::board_nets` --, a pad of a footprint that exists only on the board -- `BoardPart::pad_nets`) and the
//! name every copper item of it carries: tracks, arcs and vias, zones and teardrops, copper shapes, a footprint's mask-only and auxiliary pads, the via
//! tenting records (`BOARD::AllConnectedItems`).
//!
//! The pads' nets come from the schematic's netlist, which the panel does not change: KiCad renames or removes the board's `NETINFO_ITEM` alone, and the
//! board disagrees with the schematic until Update PCB from Schematic. Here the edit is kept as an overlay the board's nets are named by on every load,
//! last (`DrawingsSection::net_renames`, laid on by `crates/cli/src/board_net_edits.rs`), so the netlist the schematic owns (`design.nets`, the intent's)
//! is left as it is.
//!
//! * Rename: `net->SetNetname( fullNetName )` on the one `NETINFO_ITEM` every item points at: the pads (through the overlay) and every copper item of the
//!   net take the new name. A rename of a renamed net updates its entry (`A -> B`, then `B -> C`, is `A -> C`); a rename back to the first name drops it.
//!   The panel has already refused an empty name ("Net name cannot be empty.") and a name in use; the verb refuses them again ("Net name '%s' is already
//!   in use.") so that an agent's call cannot merge two nets.
//! * Delete: `BOARD::Remove( NETINFO_ITEM )` -- every connected item of the net is put on the unconnected net (`SetNet( unconnected )`) and the net is gone:
//!   the overlay's entry with an empty name takes the net's pads off it, its copper is on no net (`""`).
//! * Add: `new NETINFO_ITEM( m_board, name, 0 ); m_board->Add( newnet )` -- a net with no item, a name of `board_nets`.
//!
//! Different from KiCad: each is one undo step (`Cmd::RenameNet`, `Cmd::DeleteNets`, `Cmd::AddNet`); KiCad's panel changes the board without a commit
//! (`m_frame->OnModify()` only), so Ctrl+Z does not take them back there. A delete of several nets (several rows, a group) is one step.

use super::Board;
use eda_model::ir::NetRename;
use eda_model::CheckResult;

fn refuse(check: &str, what: &str, message: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, message.into())]
}

impl Board<'_> {
    /// `BOARD::FindNet( name )`: a net of the board by its (escaped) name -- the model's, `board_nets`', a board part's pad's, or one only a copper item names.
    pub(crate) fn board_has_net(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let edits = self.design.drawings.as_ref().map(|d| d.net_renames.as_slice()).unwrap_or(&[]);
        // a net the overlay renamed or deleted since the model was loaded is no longer under its old name, and one it renamed to is
        let renamed_away = edits.iter().any(|e| e.from == name && e.to != name) && !edits.iter().any(|e| e.to == name);
        if self.model.nets.iter().any(|n| n.name == name) && !renamed_away {
            return true;
        }
        if edits.iter().any(|e| e.to == name) {
            return true;
        }
        if let Some(d) = self.design.drawings.as_ref() {
            if d.board_nets.iter().any(|n| n == name) || d.board_parts.iter().any(|p| p.pad_nets.iter().any(|(_, n)| n == name)) {
                return true;
            }
        }
        let mut found = false;
        self.visit_item_nets_ref(&mut |n: &str| found |= n == name);
        found
    }

    /// Every net name a copper item of the board carries, read (`AllConnectedItems`).
    fn visit_item_nets_ref(&self, f: &mut dyn FnMut(&str)) {
        if let Some(rt) = self.design.routing.as_ref() {
            rt.tracks.iter().for_each(|t| f(&t.net));
            rt.vias.iter().for_each(|v| f(&v.net));
            rt.zones.iter().filter(|z| !z.is_rule_area).for_each(|z| f(&z.net));
        }
        if let Some(d) = self.design.drawings.as_ref() {
            d.board_items.shape_styles.iter().filter_map(|s| s.net.as_deref()).for_each(&mut *f);
            for e in &d.footprint_extras {
                e.graphics.iter().for_each(|g| f(&g.net));
                e.aux_pads.iter().for_each(|p| f(&p.net));
            }
        }
    }

    /// Every net name a copper item of the board carries, to change (`AllConnectedItems`, `BOARD_CONNECTED_ITEM::SetNet`), with the via tenting
    /// records kept beside them (keyed by a via's position and net) and the pads of the footprints that exist only on the board.
    fn rename_item_nets(&mut self, from: &str, to: &str) {
        let mut f = |n: &mut String| {
            if n == from {
                *n = to.to_string();
            }
        };
        if let Some(rt) = self.design.routing.as_mut() {
            rt.tracks.iter_mut().for_each(|t| f(&mut t.net));
            rt.vias.iter_mut().for_each(|v| f(&mut v.net));
            rt.zones.iter_mut().filter(|z| !z.is_rule_area).for_each(|z| f(&mut z.net));
        }
        if let Some(d) = self.design.drawings.as_mut() {
            for s in d.board_items.shape_styles.iter_mut() {
                if let Some(n) = s.net.as_mut() {
                    f(n);
                }
            }
            // a copper shape on no net says nothing (`ShapeStyle::net` `None`)
            for s in d.board_items.shape_styles.iter_mut().filter(|s| s.net.as_deref() == Some("")) {
                s.net = None;
            }
            d.board_items.shape_styles.retain(|s| !s.is_empty());
            for e in d.footprint_extras.iter_mut() {
                e.graphics.iter_mut().for_each(|g| f(&mut g.net));
                e.aux_pads.iter_mut().for_each(|p| f(&mut p.net));
            }
            d.via_tenting.iter_mut().for_each(|t| f(&mut t.net));
            for p in d.board_parts.iter_mut() {
                if to.is_empty() {
                    p.pad_nets.retain(|(_, n)| n != from);
                } else {
                    p.pad_nets.iter_mut().for_each(|(_, n)| f(n));
                }
            }
            if to.is_empty() {
                d.board_nets.retain(|n| n != from);
            } else {
                d.board_nets.iter_mut().for_each(&mut f);
                d.board_nets.sort();
                d.board_nets.dedup();
            }
        }
    }

    /// The overlay after the board's net `from` is called `to` (`""`: deleted): an entry that already leads to `from` now leads to `to`, else a new one; an
    /// entry that leads back to its own name says nothing and goes.
    fn record_net_rename(&mut self, from: &str, to: &str) {
        let d = self.design.drawings.get_or_insert_with(Default::default);
        match d.net_renames.iter_mut().find(|e| e.to == from) {
            Some(e) => e.to = to.to_string(),
            None => d.net_renames.push(NetRename { from: from.to_string(), to: to.to_string() }),
        }
        d.net_renames.retain(|e| e.from != e.to);
    }

    /// `Cmd::RenameNet`: `onRenameSelectedNet`'s `net->SetNetname( fullNetName )`.
    pub(crate) fn rename_net(&mut self, from: &str, to: &str) -> Result<(), Vec<CheckResult>> {
        if to.is_empty() {
            return Err(refuse("ops_bad_net_name", "rename_net", "Net name cannot be empty."));
        }
        if !self.board_has_net(from) {
            return Err(refuse("ops_unknown_net", from, format!("the board has no net named '{from}'")));
        }
        if from == to {
            return Err(refuse("ops_nothing_to_change", "rename_net", "The net already has this name."));
        }
        if self.board_has_net(to) {
            return Err(refuse("ops_net_name_in_use", to, format!("Net name '{}' is already in use.", eda_model::text_vars::unescape_string(to))));
        }
        self.record_net_rename(from, to);
        self.rename_item_nets(from, to);
        self.sort_routing();
        Ok(())
    }

    /// `Cmd::DeleteNets`: `onDeleteSelectedNet`'s `m_board->Remove( net )` for each of `names` -- every item of the net onto no net, the net gone.
    pub(crate) fn delete_nets(&mut self, names: &[String]) -> Result<(), Vec<CheckResult>> {
        let mut gone: Vec<&str> = names.iter().map(String::as_str).filter(|n| !n.is_empty()).collect();
        gone.sort();
        gone.dedup();
        if gone.is_empty() {
            return Err(refuse("ops_nothing_to_change", "delete_nets", "No net to delete."));
        }
        if let Some(missing) = gone.iter().find(|n| !self.board_has_net(n)) {
            return Err(refuse("ops_unknown_net", missing, format!("the board has no net named '{missing}'")));
        }
        for name in gone {
            // a net only Add Net or a design block made has no pads to take off it: the name goes from `board_nets`, nothing else to remember
            let has_pins = self.model.nets.iter().any(|n| n.name == name && !n.pins.is_empty()) || self.design.drawings.as_ref().is_some_and(|d| d.net_renames.iter().any(|e| e.to == name));
            if has_pins {
                self.record_net_rename(name, "");
            }
            self.rename_item_nets(name, "");
        }
        self.sort_routing();
        Ok(())
    }

    /// `Cmd::AddNet`: `onAddNet`'s `m_board->Add( new NETINFO_ITEM( m_board, name, 0 ) )`, refused for a name in use ("Net name '%s' is already in use.").
    pub(crate) fn add_net(&mut self, name: &str) -> Result<(), Vec<CheckResult>> {
        if name.is_empty() {
            return Err(refuse("ops_bad_net_name", "add_net", "Net name cannot be empty."));
        }
        if self.board_has_net(name) {
            return Err(refuse("ops_net_name_in_use", name, format!("Net name '{name}' is already in use.")));
        }
        let d = self.design.drawings.get_or_insert_with(Default::default);
        d.board_nets.push(name.to_string());
        d.board_nets.sort();
        d.board_nets.dedup();
        Ok(())
    }
}
