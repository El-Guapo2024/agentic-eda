//! The upkeep that makes an unfill stick (round 7 R7-Z1, `docs/parity/findings/round7/r7-route-rules.md`).
//!
//! A zone's fill is KiCad's, made by kicad-cli, and kept beside `design.json` as derived data (`eda_kicad::zone_fills`): it is the zone's while the zone is the
//! one it was made for, moved with it when the zone is moved, turned, mirrored or flipped. KiCad throws a zone's copper away on some edits
//! (`ZONE::SetLayerSet` to another set: `unFillLocked()`, then `m_FilledPolysList.clear()`; the point editor's `m_zone.UnFill()`) and clears the filled flag
//! on a flip (`ZONE::Flip`: `SetLayerSet( flipped )`, then `SetFilledPolysList` puts the mirrored copper back), and the copper stays gone, or the flag
//! cleared, whatever later edits do: dragging the corner back, setting the old layer again, flipping again. Here the zone carries how many such edits it
//! has been through (`Zone::fill_epoch`, `Zone::flip_epoch`), counted after every command by comparing each zone with itself before it
//! ([`eda_kicad::zone_fills::fill_after_edit`]); a fill made at other counts is not the zone's (`KicadFills::current`). The counts are part of the design,
//! so an undo, which restores the zone as it was, brings the fill back with it, as KiCad's undo copy of the zone holds its fill.

use crate::Board;
use eda_kicad::zone_fills::{fill_after_edit, FillAfterEdit};
use eda_model::ir::Zone;
use std::collections::HashMap;

impl Board<'_> {
    /// The zones before a command, for [`Board::tidy_zone_fills`]; `None` when the board has none.
    pub(crate) fn zones_before(&self) -> Option<HashMap<String, Zone>> {
        let zones = &self.design.routing.as_ref()?.zones;
        (!zones.is_empty()).then(|| zones.iter().filter(|z| !z.id.is_empty()).map(|z| (z.id.clone(), z.clone())).collect())
    }

    /// Counts, on each zone the command changed, an edit that emptied its fill (`Zone::fill_epoch`) or flipped it to the other side's layers
    /// (`Zone::flip_epoch`), from the zone as it was before the command (`before`).
    pub(crate) fn tidy_zone_fills(&mut self, before: Option<HashMap<String, Zone>>) {
        let Some(before) = before else { return };
        let Some(rt) = self.design.routing.as_mut() else { return };
        for z in rt.zones.iter_mut() {
            let Some(was) = before.get(&z.id) else { continue };
            match fill_after_edit(was, z) {
                FillAfterEdit::Kept => {}
                FillAfterEdit::Emptied => z.fill_epoch = was.fill_epoch + 1,
                FillAfterEdit::Flipped => z.flip_epoch = was.flip_epoch + 1,
            }
        }
    }
}
