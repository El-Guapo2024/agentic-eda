// Which of KiCad 10's item dialogs is open: Grid Properties (`DIALOG_GRID_ITEM_PROPERTIES`), Via Stitching Properties (`DIALOG_VIA_STITCH_PROPERTIES`) and the drill map's and
// chart's. Plain module state with subscribers (like `state/pcbItemDialogs.ts`), so an action of the action runner can open a dialog without a place in the big reducer.
// `components/Kicad10ItemDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";
import type { BoardViaStitch } from "../api/pcbItemTypes";

export interface Kicad10DialogsState {
  /** The grid item Grid Properties edits. */
  gridItem: { id: string } | null;
  /** The via stitching Via Stitching Properties edits: one on the board (`id`), or the one Draw Via Stitch Area has just drawn (`id` null), with the settings it starts from. */
  viaStitch: { id: string | null; stitch: BoardViaStitch } | null;
}

const CLOSED: Kicad10DialogsState = { gridItem: null, viaStitch: null };

let current: Kicad10DialogsState = CLOSED;
const listeners = new Set<() => void>();

function set(next: Kicad10DialogsState): void {
  current = next;
  for (const l of listeners) l();
}

export function getKicad10Dialogs(): Kicad10DialogsState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useKicad10Dialogs(): Kicad10DialogsState {
  return useSyncExternalStore(subscribe, getKicad10Dialogs, getKicad10Dialogs);
}

export function openGridItemDialog(id: string): void {
  set({ ...current, gridItem: { id } });
}

export function closeGridItemDialog(): void {
  set({ ...current, gridItem: null });
}

export function openViaStitchDialog(id: string | null, stitch: BoardViaStitch): void {
  set({ ...current, viaStitch: { id, stitch } });
}

export function closeViaStitchDialog(): void {
  set({ ...current, viaStitch: null });
}
