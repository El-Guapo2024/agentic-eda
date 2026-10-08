// The schematic paste in progress, and Paste Special's dialog. KiCad adds the pasted items to the sheet and starts the Move tool on them: they
// follow the cursor until a click drops them (`SCH_EDITOR_CONTROL::Paste` -> `SCH_ACTIONS::move`). Here the items are not on the sheet yet:
// what a paste would add is held here, drawn by SchematicView shifted by the cursor, and the click sends the `paste_sch` command.
//
// A store of its own (read with `useSyncExternalStore`, written by plain functions) so the action handlers, the canvas and the dialog share it
// without `store.tsx` -- shared by every editor -- growing a schematic-only field.
import { useSyncExternalStore } from "react";
import type { SchPasteMode, SchPastePreview } from "../kicad-port/schClipboard";

export interface SchPaste {
  /** What the `paste_sch` command carries (the server's `fragment`). */
  fragment: unknown;
  /** The new items, at the position the clipboard has them. */
  preview: SchPastePreview;
  /** The point of the fragment the cursor holds (KiCad's `m_anchorPos`). */
  anchor: [number, number];
  /** The sheet the paste was started on (path of sheet-instance ids); a paste is for that sheet only. */
  sheet: string[];
  mode: SchPasteMode;
  /** Duplicate carries the copy from where it was, Paste from where the clipboard has it. */
  origin: "paste" | "duplicate";
  notes: string[];
}

let current: SchPaste | null = null;
let specialOpen = false;
const listeners = new Set<() => void>();

const emit = () => listeners.forEach((l) => l());
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export const getSchPaste = (): SchPaste | null => current;

export function setSchPaste(next: SchPaste | null): void {
  current = next;
  emit();
}

/** Escape (or leaving the sheet or the tab): throw the paste away. Returns whether there was one, so the caller knows Escape was used up. */
export function cancelSchPaste(): boolean {
  if (!current) return false;
  current = null;
  emit();
  return true;
}

export const useSchPaste = (): SchPaste | null => useSyncExternalStore(subscribe, getSchPaste, getSchPaste);

/** Paste Special's dialog (`DIALOG_PASTE_SPECIAL`). */
export function openPasteSpecial(): void {
  specialOpen = true;
  emit();
}

export function closePasteSpecial(): void {
  specialOpen = false;
  emit();
}

export const usePasteSpecialOpen = (): boolean => useSyncExternalStore(subscribe, () => specialOpen, () => specialOpen);
