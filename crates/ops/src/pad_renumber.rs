//! The Properties panel's "Pad Number" of a pad on the board (`PAD_DESC`'s `PROPERTY<PAD, wxString>( "Pad Number", &PAD::SetNumber, &PAD::GetNumber )`, available on a
//! copper pad; UI-LEFT-2 PP8): `PAD::SetNumber` renames the one pad, in one commit ("Edit Properties", `PCB_PROPERTIES_PANEL::valueChanged`).
//!
//! In KiCad a pad holds its own net, so a renamed pad keeps the net it had (`SetNumber` touches nothing else) until Update PCB from Schematic matches pads to pins
//! by number again. Here a pad's net is its pin's place in the net list in force (`REF.NUM`, see `pad_nets.rs`), and much of what the board says of a pad is keyed
//! by its number, so the verb carries them along:
//!
//! * the footprint's pads: the instance gets its own copy of the footprint it is on with the pad renamed (a [`FootprintAssignment`] with a definition, as the
//!   footprint's own texts and Swap Layers make one: `fp_texts.rs`), its pad edits re-keyed to the pads' new numbers and places among the pads that share them
//!   (`REF.NUM#k`);
//! * the pad's net: `REF.NEW` joins the net `REF.OLD` is on, and `REF.OLD` leaves it once no pad of the footprint is numbered so -- the net list in force
//!   becomes the design's own, as a swap of pad nets makes it; a board-only part's `pad_nets` the same;
//! * a pad renamed to a number another pad of the footprint already has keeps its own net, and those pads keep theirs (UI-LEFT-3 P3-9): when the nets differ,
//!   every pad of that number gets its own (`PadMaskInfo::net`, the per-`PAD` net an imported board keeps where a number is on two nets, UI-LEFT-2 PP21),
//!   which the connectivity, the writer and the studio read over the number's; the net list is left as it is. Update PCB from Schematic then matches the
//!   pads to the pins again, as in KiCad;
//! * the zone overrides of the old number (`set_pad_zone_overrides`, which are per number here, per pad in KiCad) are the new number's too.
//!
//! A pad's number on an NPTH pad is not offered (`isCopperPad`), as in KiCad.

use super::Board;
use eda_model::fp_assign::FootprintAssignment;
use eda_model::fp_edit::{parse_pad_id, PadEdit};
use eda_model::CheckResult;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// `(number, nth)` of every pad of a footprint, in pad order: the k-th pad numbered so is `nth` k (1-based), as `REF.NUM#k` counts them.
fn keys_of(numbers: &[String]) -> Vec<(String, u32)> {
    let mut seen: std::collections::BTreeMap<&str, u32> = Default::default();
    numbers
        .iter()
        .map(|n| {
            let k = seen.entry(n.as_str()).or_insert(0);
            *k += 1;
            (n.clone(), *k)
        })
        .collect()
}

impl Board<'_> {
    /// `Cmd::SetPadNumber`: `PAD::SetNumber( number )` of the pad `id` (`REF.NUM` / `REF.NUM#k`). See the module doc. Refused for a pad that is not on the board.
    pub(crate) fn set_pad_number(&mut self, id: &str, number: &str) -> Result<(), Vec<CheckResult>> {
        let Some((reference, old, nth)) = parse_pad_id(id) else {
            return Err(fail("ops_unknown_pad", id, "no pad of a footprint on the board with this id"));
        };
        self.require_placed(&reference)?;
        let part = self.model.part(&reference).ok_or_else(|| fail("ops_unknown_part", &reference, "no part with this reference exists in the model"))?;
        // (the pads as the board has them, those taken off left out: `pads_footprint`; the copy is made of the whole footprint, so the removals' places stay true)
        let base = self.unedited_footprint(part).ok_or_else(|| fail("ops_no_footprint", &reference, "this part has no footprint geometry to edit"))?;
        let before = self.pads_footprint(part).ok_or_else(|| fail("ops_no_footprint", &reference, "this part has no footprint geometry to edit"))?;
        let removed = self.design.footprint_edit(&reference).map(|e| e.removed_pads.clone()).unwrap_or_default();
        let Some(index) = eda_model::fp_edit::pad_index(&before, &old, nth) else {
            return Err(fail("ops_unknown_pad", id, format!("{reference} has no pad {old}")));
        };
        if old == number {
            return Ok(());
        }
        let name = self.design.footprint_assignment(&reference).map(|a| a.footprint.clone()).unwrap_or_else(|| crate::fp_exchange::footprint_name_of(part));

        // The pads' keys before and after.
        let numbers_before: Vec<String> = before.pads.iter().map(|p| p.number.clone()).collect();
        let mut numbers_after = numbers_before.clone();
        numbers_after[index] = number.to_string();
        let (keys_before, keys_after) = (keys_of(&numbers_before), keys_of(&numbers_after));
        let old_kept = numbers_after.iter().any(|n| *n == old);
        // `PAD::SetNumber` touches nothing but the number: each pad's net before the rename, its own (`PadMaskInfo::net`) or its number's.
        let nets_before = self.design.nets.clone().unwrap_or_else(|| self.model.nets.clone());
        let own_before: Vec<Option<String>> = {
            let infos = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| e.id == reference)).map(|e| &e.pads).filter(|p| p.len() == numbers_before.len());
            (0..numbers_before.len()).map(|j| infos.and_then(|p| p[j].net.clone())).collect()
        };
        let board_pad_nets: Vec<(String, String)> = self.design.drawings.as_ref().and_then(|d| d.board_parts.iter().find(|p| p.reference == reference)).map(|p| p.pad_nets.clone()).unwrap_or_default();
        let net_before = |j: usize| -> String {
            own_before[j].clone().unwrap_or_else(|| {
                let pin = format!("{reference}.{}", numbers_before[j]);
                nets_before
                    .iter()
                    .find(|n| n.pins.contains(&pin))
                    .map(|n| n.name.clone())
                    .or_else(|| board_pad_nets.iter().find(|(n, _)| *n == numbers_before[j]).map(|(_, net)| net.clone()))
                    .unwrap_or_default()
            })
        };
        // Renamed to a number another pad has: every pad keeps the net it had (UI-LEFT-3 P3-9).
        let shares = !number.is_empty() && numbers_before.iter().enumerate().any(|(j, n)| j != index && n == number);
        let kept_nets: Option<Vec<(usize, String)>> = shares.then(|| (0..numbers_after.len()).filter(|&j| numbers_after[j] == number).map(|j| (j, net_before(j))).collect());
        let split = kept_nets.as_ref().is_some_and(|k| k.iter().any(|(_, n)| *n != k[0].1));

        // The footprint's own copy, the pad renamed.
        let mut fp = base.clone();
        fp.pads[eda_model::fp_edit::base_pad_index(&removed, index)].number = number.to_string();
        self.drawings_mut().set_footprint_assignment(FootprintAssignment { reference: reference.clone(), footprint: name, definition: Some(fp) });

        // The pad edits, each to the pad it was for.
        if let Some(edit) = self.design.drawings.as_mut().and_then(|d| d.footprint_edits.iter_mut().find(|e| e.id == reference)) {
            let pads: Vec<PadEdit> = std::mem::take(&mut edit.pads);
            for mut e in pads {
                if let Some(j) = keys_before.iter().position(|(n, k)| *n == e.number && *k == e.nth.max(1)) {
                    e.number = keys_after[j].0.clone();
                    e.nth = keys_after[j].1;
                }
                edit.set_pad(e);
            }
        }

        // The zone overrides of the old number are the new number's too (and the old number's while a pad keeps it).
        if let Some(over) = self.design.drawings.as_mut().and_then(|d| d.zone_overrides.iter_mut().find(|o| o.id == reference)) {
            if let Some(o) = over.pads.iter().find(|o| o.number == old).cloned() {
                if !over.pads.iter().any(|p| p.number == number) {
                    over.pads.push(eda_model::ir::PadZoneOverride { number: number.to_string(), ..o });
                }
                if !old_kept {
                    over.pads.retain(|p| p.number != old);
                }
                over.pads.sort_by(|a, b| a.number.cmp(&b.number));
            }
        }

        // Each pad of a shared number keeps its own net where they differ (`PadMaskInfo::net`, in the footprint's pad order).
        if split {
            let pad_count = numbers_after.len();
            // A footprint made here has no pad records: each new one takes the pin function and type its pad is written with now (its schematic pin's, as
            // `updateComponentPadConnections` set them), since the writer reads a record's own over the pin's.
            let pins: Vec<(String, String)> = numbers_before
                .iter()
                .map(|n| self.model.symbol_links.get(&reference).and_then(|l| l.pins.get(n)).cloned().unwrap_or_default())
                .collect();
            let dr = self.drawings_mut();
            if !dr.footprint_extras.iter().any(|e| e.id == reference) {
                dr.footprint_extras.push(eda_model::ir::FootprintExtra { id: reference.clone(), ..Default::default() });
            }
            let extra = dr.footprint_extras.iter_mut().find(|e| e.id == reference).expect("just made");
            if extra.pads.len() != pad_count {
                let made_here = !extra.from_library;
                extra.pads = pins
                    .into_iter()
                    .map(|(pin_function, pin_type)| if made_here { eda_model::ir::PadMaskInfo { pin_function, pin_type, ..Default::default() } } else { Default::default() })
                    .collect();
            }
            for (j, net) in kept_nets.iter().flatten() {
                extra.pads[*j].net = Some(net.clone());
            }
        }

        // The pad keeps its net: its new pin joins the net its old pin is on (a number another pad has keeps its pin where it is, above).
        let (old_pin, new_pin) = (format!("{reference}.{old}"), format!("{reference}.{number}"));
        let mut nets = self.design.nets.clone().unwrap_or_else(|| self.model.nets.clone());
        let was_on = nets.iter().position(|n| n.pins.contains(&old_pin));
        let new_is_on = nets.iter().any(|n| n.pins.contains(&new_pin));
        if let Some(i) = was_on {
            if !new_is_on && !number.is_empty() && !shares {
                let pins = &mut nets[i].pins;
                let sorted = pins.windows(2).all(|w| w[0] <= w[1]);
                pins.push(new_pin.clone());
                if sorted {
                    pins.sort();
                }
            }
            if !old_kept {
                nets[i].pins.retain(|p| *p != old_pin);
            }
            self.design.nets = Some(nets);
        }
        if let Some(bp) = self.design.drawings.as_mut().and_then(|d| d.board_parts.iter_mut().find(|p| p.reference == reference)) {
            if let Some(net) = bp.pad_nets.iter().find(|(n, _)| *n == old).map(|(_, net)| net.clone()) {
                if !bp.pad_nets.iter().any(|(n, _)| n == number) && !number.is_empty() && !shares {
                    bp.pad_nets.push((number.to_string(), net));
                }
                if !old_kept {
                    bp.pad_nets.retain(|(n, _)| *n != old);
                }
                bp.pad_nets.sort();
            }
        }
        self.tidy_edit(&reference);
        Ok(())
    }
}
