// Which of KiCad 10's item dialogs is open: Grid Properties (`DIALOG_GRID_ITEM_PROPERTIES`), Via Stitching Properties (`DIALOG_VIA_STITCH_PROPERTIES`) and the drill map's and
// chart's. Plain module state with subscribers (like `state/pcbItemDialogs.ts`), so an action of the action runner can open a dialog without a place in the big reducer.
// `components/Kicad10ItemDialogs.tsx` renders them.
import { useSyncExternalStore } from "react";

export interface Kicad10DialogsState {
  /** The grid item Grid Properties edits. */
  gridItem: { id: string } | null;
}

const CLOSED: Kicad10DialogsState = { gridItem: null };

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
