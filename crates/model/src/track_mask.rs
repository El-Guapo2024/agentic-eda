//! A track's solder-mask opening, which [`crate::ir::Track`] has no field for.
//!
//! KiCad 9 lets a track or an arc on an outer copper layer carry its own solder-mask opening (`PCB_TRACK::HasSolderMask`, `GetLocalSolderMaskMargin`): the file writes it as
//! `(layers "F.Cu" "F.Mask")` instead of `(layer "F.Cu")`, and `(solder_mask_margin m)` after it when the track has its own margin (`pcb_io_kicad_sexpr.cpp::format( PCB_TRACK )`;
//! read back by `parseLayersForCuItemWithSoldermask`, which takes one copper layer and at most one mask layer on the same side). A board that came from a KiCad file keeps
//! it here, one [`TrackMask`] per track that has an opening, in `DrawingsSection::track_masks` and keyed by the track's id, as `via_extras` is keyed by a via's. The importer
//! reads them, the writer writes them back, and a track with none is written `(layer ..)` as it always was. Additive: a design without it reads as "no track has a mask
//! opening".

use crate::ir::Nm;
use serde::{Deserialize, Serialize};

/// The solder-mask opening of one track. See the module doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackMask {
    /// The id of the track it belongs to (`Track::id`).
    pub id: String,
    /// `(solder_mask_margin ..)`: the track's own margin, nm. `None` takes the board's (`GetLocalSolderMaskMargin()` has no value).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::units::de")]
    pub margin: Option<Nm>,
}

impl crate::ir::DrawingsSection {
    /// The solder-mask opening of the track `id`, if it has one.
    pub fn track_mask(&self, id: &str) -> Option<&TrackMask> {
        self.track_masks.iter().find(|m| m.id == id)
    }
}
