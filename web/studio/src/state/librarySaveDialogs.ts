// Which library Save Library As is asking about (`SYMBOL_EDIT_FRAME::saveLibrary( aLibrary, true )`'s file dialog, `FOOTPRINT_EDIT_FRAME::SaveLibraryAs`'s
// `CreateNewLibrary`): plain module state with subscribers, like `state/libraryTableDialogs.ts`, so an action of the registry can open it;
// `components/library/SaveLibraryAsDialog.tsx` renders it.
import { useSyncExternalStore } from "react";

export interface SaveLibraryAsUi {
  kind: "symbol" | "footprint";
  /** The library saved as a new one (the project library `eda` too). */
  lib: string;
  /** KiCad 25ec2335's Save Library As Packed / Unpacked; plain Save Library As when absent. */
  mode?: "packed" | "unpacked";
}

let current: SaveLibraryAsUi | null = null;
const listeners = new Set<() => void>();

function set(next: SaveLibraryAsUi | null): void {
  current = next;
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

const snapshot = () => current;

export function useSaveLibraryAs(): SaveLibraryAsUi | null {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

export function openSaveLibraryAs(kind: "symbol" | "footprint", lib: string, mode?: "packed" | "unpacked"): void {
  set(mode ? { kind, lib, mode } : { kind, lib });
}

export function closeSaveLibraryAs(): void {
  set(null);
}
