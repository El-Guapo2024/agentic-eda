//! Paste Special on the PCB: what happens to the reference designators of the footprints a paste brings, and whether the pasted copper keeps its nets.
//!
//! Ports `DIALOG_PASTE_SPECIAL` (`common/dialogs/dialog_paste_special.cpp`, `PASTE_MODE` in `include/dialogs/dialog_paste_special.h`) and what
//! `PCB_CONTROL::Paste` (`pcbnew/tools/pcb_control.cpp`) does with its answers:
//!
//! * `PASTE_MODE mode = PASTE_MODE::KEEP_ANNOTATIONS;` is the mode of a plain Paste and the one the dialog opens on: a footprint keeps the
//!   reference designator the clipboard has, **even if another footprint of the board has it**.
//! * `UNIQUE_ANNOTATIONS` hands `placeBoardItems( .., aReannotateDuplicates )` a `true`, and `BOARD_REANNOTATE_TOOL::ReannotateDuplicates`
//!   gives a taken reference the next free number (`R1` -> `R2`).
//! * `REMOVE_ANNOTATIONS` sets every pasted footprint's reference to `REF**` (`defaultRef`).
//! * "Clear net assignments" (`clear_nets`, offered only when the clipboard holds a board fragment, `clipItem->Type() == PCB_T`): every
//!   connected item of the fragment -- tracks, vias, zones and the pads of its footprints -- is put on no net
//!   (`item->SetNet( NETINFO_LIST::OrphanedItem() )`) instead of being mapped onto the board's nets by name (`BOARD::MapNets`).
//!
//! # References on this board
//!
//! A footprint of this board is a part, and a part is named by its reference, once. So a copy is always filed under the first free
//! reference (`unique_reference`: the same number-up search the reannotate tool makes) and the text its Reference field shows is what the mode
//! says -- the clipboard's own reference for Keep, `REF**` for Remove. That text is `FieldLayout::text`, the mechanism Create Array's
//! "Keep original reference designators" already uses: it is what the `.kicad_pcb` carries as the `Reference` property, so kicad-cli and
//! KiCad see two `R1`s, as they do after a Paste in KiCad.

use super::Board;
use eda_model::ir::Point;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

/// `PASTE_MODE` (`include/dialogs/dialog_paste_special.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMode {
    /// `UNIQUE_ANNOTATIONS`: "Assign unique reference designators to pasted symbols" -- a taken reference gets the next free number.
    Unique,
    /// `KEEP_ANNOTATIONS`: "Keep existing reference designators, even if they are duplicated" -- what a plain Paste does.
    #[default]
    Keep,
    /// `REMOVE_ANNOTATIONS`: "Clear reference designators on all pasted symbols" -- every footprint becomes [`DEFAULT_REFERENCE`].
    Remove,
}

/// `const wxString defaultRef = wxT( "REF**" );` in `PCB_CONTROL::Paste`.
pub const DEFAULT_REFERENCE: &str = "REF**";

impl Board<'_> {
    /// `Cmd::PasteSpecial` (and, with the defaults, `Cmd::PasteClipboard`): the KiCad clipboard `text`, put on the board with the clipboard's origin at
    /// `at`, its footprints' references handled as `mode` says and, with `clear_nets`, its nets taken off.
    pub(crate) fn paste_special(&mut self, text: &str, at: Point, mode: PasteMode, clear_nets: bool) -> Result<(), Vec<CheckResult>> {
        let clip = eda_kicad::parse_pcb_clipboard(text).map_err(|e| vec![CheckResult::fail("ops_bad_clipboard", "paste", e.iter().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).collect::<Vec<_>>().join("; "))])?;
        let copies = self.copies_from_clipboard(&clip, at, mode, clear_nets);
        if copies.is_empty() {
            return Err(vec![CheckResult::fail("ops_bad_clipboard", "paste", "nothing on the clipboard fits this board: its copper is on layers the board does not have")]);
        }
        self.insert_copies_all(copies)
    }
}
