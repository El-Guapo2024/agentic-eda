// `HOTKEY_CYCLE_POPUP` (common/dialogs/hotkey_cycle_popup.cpp, KiCad 8303b2ad): the small window a hotkey that cycles a setting shows for half a second -- a title and the
// setting's choices with the new one selected (`Popup( aTitle, aItems, aSelection )`), centred on the frame (`Centre()`), hidden again `SHOW_TIME_MS` (500 ms) after the
// last call (a call while it shows restarts the time and replaces its contents). The callers are the feedback handlers, each of which first asks
// `COMMON_SETTINGS::m_Input.hotkey_feedback` (Preferences > Common's "Show popup indicator when toggling settings with hotkeys", kicad-port/commonSettings.ts):
//   SCH_EDITOR_CONTROL::GridFeedback        "Grid": the schematic's grids (`GRID::UserUnitsMessageText`), at `last_size_idx`, after Next / Previous Grid and the fast grids
//   PCB_CONTROL::ContrastModeFeedback       "Inactive Layer Display": Normal, Dimmed, Hidden, after the inactive-layer cycle (the board and the Footprint Editor)
//   PCB_CONTROL::SnapModeFeedback           "Object Snapping": Active Layer, All Layers, after the magnetic snap actions
// Plain module state with subscribers (components/HotkeyCyclePopup.tsx renders it), so an action handler can show it without a place in the big reducer. UI-LEFT-3 C3-9.
import { useSyncExternalStore } from "react";
import { getCommonOptions } from "./commonOptions";

/** `SHOW_TIME_MS`. */
export const HOTKEY_POPUP_SHOW_MS = 500;

export interface HotkeyPopupState {
  title: string;
  items: string[];
  /** `std::min( aSelection, count - 1 )`. */
  selection: number;
  /** Bumped on every call, so a repeat restarts the time. */
  nonce: number;
}

let current: HotkeyPopupState | null = null;
let nonce = 0;
let timer: ReturnType<typeof setTimeout> | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  for (const l of [...listeners]) l();
}

/** `HOTKEY_CYCLE_POPUP::Popup`, when the person keeps the hotkey feedback on (`if( !Pgm().GetCommonSettings()->m_Input.hotkey_feedback ) return 0;`). */
export function showHotkeyPopup(title: string, items: readonly string[], selection: number): void {
  if (!getCommonOptions().common.hotkeyFeedback) return;
  current = { title, items: [...items], selection: Math.min(selection, items.length - 1), nonce: ++nonce };
  if (typeof window !== "undefined") (window as unknown as { __edaHotkeyPopup?: unknown }).__edaHotkeyPopup = { title, items: [...items], selection: current.selection, at: Date.now() };
  if (timer) clearTimeout(timer);
  // `m_showTimer->StartOnce( SHOW_TIME_MS )`, then `Show( false )`
  timer = setTimeout(() => {
    timer = null;
    current = null;
    emit();
  }, HOTKEY_POPUP_SHOW_MS);
  emit();
}

export function getHotkeyPopup(): HotkeyPopupState | null {
  return current;
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useHotkeyPopup(): HotkeyPopupState | null {
  return useSyncExternalStore(subscribe, getHotkeyPopup, getHotkeyPopup);
}
