// The "Override locks" check box of the board editor's and the schematic editor's top toolbar (`ACTION_TOOLBAR_CONTROLS::overrideLocks`: KiCad 8303b2ad
// common/eda_draw_frame.cpp's `overrideLocksFactory` makes `m_overrideLocksCb = new wxCheckBox( aToolbar, ID_ON_OVERRIDE_LOCKS, _( "Override locks" ) )`, and
// `EDA_DRAW_FRAME::GetOverrideLocks()` reads it, false when the toolbar has none). One per frame, never saved: a new frame's box is unchecked.
// Read by Place > Auto-Place Footprints (`AUTOPLACE_TOOL::autoplace`: `if( !frame()->GetOverrideLocks() )` the locked footprints are left out, UI-LEFT-2 PP20).
import { useSyncExternalStore } from "react";

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

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useOverrideLocks(frame: OverrideLocksFrame): boolean {
  return useSyncExternalStore(subscribe, () => current[frame]);
}
