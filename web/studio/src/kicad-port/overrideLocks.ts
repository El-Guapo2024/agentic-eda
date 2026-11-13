// The "Override locks" check box of the board editor's and the schematic editor's top toolbar (`ACTION_TOOLBAR_CONTROLS::overrideLocks`: KiCad 8303b2ad
// common/eda_draw_frame.cpp's `overrideLocksFactory` makes `m_overrideLocksCb = new wxCheckBox( aToolbar, ID_ON_OVERRIDE_LOCKS, _( "Override locks" ) )`, and
// `EDA_DRAW_FRAME::GetOverrideLocks()` reads it, false when the toolbar has none). One per frame, never saved: a new frame's box is unchecked.
//
// Its readers (UI-LEFT-3 P3-3; it fed Auto-Place only):
//   * the board editor's `PCB_SELECTION_TOOL::FilterCollectorForLockedItems` (`if( m_frame->IsType( FRAME_PCB_EDITOR ) && !m_frame->GetOverrideLocks() )`):
//     Move, Drag, Rotate, Flip, Mirror, Delete, Cut, Move Exactly, Position Relative, Swap, Pack and Move, Increment, Justify, the track and
//     shape edits, Align/Distribute's distribute -- `lockedForTools` (pcbTransform.ts);
//   * the schematic editor's `SCH_SELECTION_TOOL::FilterSelectionForLockedItems` (`if( m_frame && m_frame->GetOverrideLocks() ) return;`): Move, Drag,
//     Rotate, Mirror, Swap, Align to Grid and Delete;
//   * `AUTOPLACE_TOOL::autoplace` (`if( !frame()->GetOverrideLocks() )` the locked footprints are left out, UI-LEFT-2 PP20).
// Pure (no React): the kicad-port modules read it; state/overrideLocks.ts adds the hook the toolbar box renders with.

export type OverrideLocksFrame = "pcb" | "schematic";

let current: Record<OverrideLocksFrame, boolean> = { pcb: false, schematic: false };
const listeners = new Set<() => void>();

/** `EDA_DRAW_FRAME::GetOverrideLocks()`. */
export function getOverrideLocks(frame: OverrideLocksFrame): boolean {
  return current[frame];
}

export function setOverrideLocks(frame: OverrideLocksFrame, on: boolean): void {
  if (current[frame] === on) return;
  current = { ...current, [frame]: on };
  for (const l of listeners) l();
}

export function subscribeOverrideLocks(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** `PCB_SELECTION_TOOL::ReportFilteredLockedItems`' info bar warning, word for word. */
export const LOCKED_ITEMS_FILTERED = "Selection contains locked items. Enable 'Override locks' to operate on them.";
