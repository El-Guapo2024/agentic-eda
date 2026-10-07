// The pad-level rules of the Pad Table (`DIALOG_FP_EDIT_PAD_TABLE`, pcbnew/dialogs/dialog_fp_edit_pad_table.cpp): what the two drill
// columns show for a pad and how an edit of one of them changes the pad.
import type { LibraryPad } from "../api/types";

/** What the Drill X / Drill Y columns show: a round hole is (d, d), a slot (w, h), no hole (0, 0) -- an SMD pad's drill cells are read-only and empty. */
export function drillColumns(pad: Pick<LibraryPad, "kind" | "drill" | "drill_slot">): [number, number] {
  if (pad.kind === "smd") return [0, 0];
  if (pad.drill_slot) return [pad.drill_slot[0], pad.drill_slot[1]];
  return [pad.drill ?? 0, pad.drill ?? 0];
}

/**
 * `OnCellChanged`'s `COL_DRILL_X` / `COL_DRILL_Y` (and `TransferDataFromWindow`): nothing changes unless a side is positive
 * (`if( dx > 0 || dy > 0 )`), a missing side takes the other (`if( dx <= 0 ) dx = dy`), and the pad's hole becomes `{ dx, dy }` -- here a round
 * hole when the sides are equal, a slot when they are not.
 */
export function withDrill(pad: LibraryPad, dx: number, dy: number): LibraryPad {
  if (dx <= 0 && dy <= 0) return pad;
  const x = dx > 0 ? dx : dy;
  const y = dy > 0 ? dy : dx;
  return x === y ? { ...pad, drill: x, drill_slot: null } : { ...pad, drill: null, drill_slot: [x, y] };
}
