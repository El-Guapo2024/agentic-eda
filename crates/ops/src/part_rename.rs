//! A rename that carries the board's part: [`Board::rename_part`], the one place that moves everything the board keeps under a part's reference.
//!
//! A part's reference is the key of its pose, its footprint, the pads its routing ends on, its net pins, its fields, its edits, its zone overrides, its
//! lock and its group membership, so renaming the schematic symbol alone (what `Cmd::RenameSymbol` did) left all of that under the old reference: the next
//! reconcile made a fresh unplaced part for the new one and the footprint, the pose and the copper stayed behind with no symbol. Edit Reference
//! (`U`, `Cmd::RenameSymbol`), Annotate with reset (`Cmd::Annotate`) and Increment Annotations (`Cmd::IncrementAnnotations`) all end here, so a
//! rename is one undo step on the whole design (`Cmd::spans_both_editors`).
//!
//! KiCad has no such step to port: a footprint's reference is a field of the footprint, and the netlist updater (`BOARD_NETLIST_UPDATER::
//! updateFootprintParameters`, "Changed %s reference designator to %s.") sets the board's reference from the schematic's when the PCB is updated. The
//! studio updates the PCB from the schematic on every schematic edit (`board::reconcile_schematic`), and what stays with the part through it is what
//! `ExchangeFootprint` and `SetReference` leave alone: the pose, the footprint, the copper on its pads, its nets, its fields, its lock and its group.

use super::Board;
use eda_model::fp_assign::{compose_renames, renamed_id};
use std::collections::{BTreeMap, BTreeSet};

impl Board<'_> {
    /// Move everything the board keeps under a part's reference to the part's new one, for every `old -> new` of `renames` at once (two parts can swap):
    ///
    /// * the pose (`placement.footprints`) and the floorplan block that names the part (`placement.modules`);
    /// * the pads the routing ends on (`Track::pins`) and the pins of the net list (`Design::nets`);
    /// * the footprint's edits (`drawings.footprint_edits`: its Reference, Value and user fields, attributes and pad edits), the imported mask facts
    ///   (`footprint_extras`), the zone overrides (`zone_overrides`), a footprint that exists only on the board (`board_parts`), the lock
    ///   (`locked_ids`), the groups it is a member of (`groups`) and the DRC waivers that name its pads;
    /// * the intent's part itself: the rename is recorded in `drawings.renamed_parts` (`eda_model::fp_assign`), which the loader lays over the intent,
    ///   so the model has the part, its footprint and its nets under the new reference.
    ///
    /// The caller has checked that no new reference is another part's. A reference the board has nothing under is skipped.
    pub(crate) fn rename_part(&mut self, renames: &BTreeMap<String, String>) {
        let map: BTreeMap<String, String> = renames.iter().filter(|(old, new)| old != new).map(|(old, new)| (old.clone(), new.clone())).collect();
        if map.is_empty() {
            return;
        }
        let new_of = |id: &str| renamed_id(id, &map);

        // The intent's own parts among them, before anything moves: a part the board made (a copy, a footprint placed from a library) is renamed in place.
        let board_made: BTreeSet<String> = self.design.drawings.as_ref().map(|d| d.board_parts.iter().map(|b| b.reference.clone()).collect()).unwrap_or_default();
        let entries = self.design.drawings.as_ref().map(|d| d.renamed_parts.clone()).unwrap_or_default();
        let model = self.model;
        let renamed_parts = compose_renames(&entries, &map, |r| model.part(r).is_some() && !board_made.contains(r));

        if let Some(pl) = self.design.placement.as_mut() {
            for f in pl.footprints.iter_mut() {
                if let Some(n) = map.get(&f.id) {
                    f.id = n.clone();
                }
            }
            pl.footprints.sort_by(|a, b| a.id.cmp(&b.id));
            for module in pl.modules.iter_mut() {
                for r in module.refs.iter_mut() {
                    if let Some(n) = map.get(r) {
                        *r = n.clone();
                    }
                }
            }
        }
        if let Some(rt) = self.design.routing.as_mut() {
            for track in rt.tracks.iter_mut() {
                for pin in track.pins.iter_mut() {
                    if let Some(n) = new_of(pin) {
                        *pin = n;
                    }
                }
            }
        }
        if let Some(nets) = self.design.nets.as_mut() {
            for net in nets.iter_mut() {
                let mut changed = false;
                for pin in net.pins.iter_mut() {
                    if let Some(n) = new_of(pin) {
                        *pin = n;
                        changed = true;
                    }
                }
                if changed {
                    net.pins.sort();
                    net.pins.dedup();
                }
            }
        }
        if let Some(dr) = self.design.drawings.as_mut() {
            for e in dr.footprint_edits.iter_mut() {
                if let Some(n) = map.get(&e.id) {
                    e.id = n.clone();
                }
            }
            dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
            for e in dr.footprint_extras.iter_mut() {
                if let Some(n) = map.get(&e.id) {
                    e.id = n.clone();
                }
            }
            for e in dr.zone_overrides.iter_mut() {
                if let Some(n) = map.get(&e.id) {
                    e.id = n.clone();
                }
            }
            for b in dr.board_parts.iter_mut() {
                if let Some(n) = map.get(&b.reference) {
                    b.reference = n.clone();
                }
            }
            dr.board_parts.sort_by(|a, b| a.reference.cmp(&b.reference));
            for id in dr.locked_ids.iter_mut() {
                if let Some(n) = new_of(id) {
                    *id = n;
                }
            }
            dr.locked_ids.sort();
            dr.locked_ids.dedup();
            for g in dr.groups.iter_mut() {
                for m in g.member_ids.iter_mut() {
                    if let Some(n) = new_of(m) {
                        *m = n;
                    }
                }
            }
            for x in dr.drc_exclusions.iter_mut() {
                for id in x.ids.iter_mut() {
                    if let Some(n) = new_of(id) {
                        *id = n;
                    }
                }
            }
            dr.renamed_parts = renamed_parts;
        }
        // `renamed_parts` is the one place that may need a drawings section of its own.
        else if !renamed_parts.is_empty() {
            self.drawings_mut().renamed_parts = renamed_parts;
        }
    }
}
