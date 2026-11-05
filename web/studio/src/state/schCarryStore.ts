// The item a schematic placing tool holds (kicad-port/schPlaceTool.ts): a label, directive label, text or power symbol made by its dialog or chooser,
// riding the cursor until the click that places it. SchematicView draws it from the server's preview of the command that click will send (`POST
// /api/sch/place_preview`, drawn at `PREVIEW_AT`) shifted to the cursor; actions/schPlaceActions.ts sets, turns and places it.
//
// A store of its own (read with `useSyncExternalStore`, written by plain functions), as the paste in the hand is (state/schPasteStore.ts), so
// `store.tsx` -- shared by every editor -- keeps no schematic-only field.
import { useSyncExternalStore } from "react";
import type { SchPastePreview } from "../kicad-port/schClipboard";
import type { Carry } from "../kicad-port/schPlaceTool";

export interface SchCarry extends Carry {
  /** The sheet it was made on (path of sheet-instance ids): an item belongs to that sheet. */
  sheet: string[];
  /** Where the item was made (the click, or the cursor for a hotkey): it is drawn there until the pointer moves. */
  at: [number, number];
  /** The server's drawing of the item at `PREVIEW_AT`; null until it arrives. */
  preview: SchPastePreview | null;
  /** Bumped by every change of the item, so a preview that comes back late is dropped. */
  seq: number;
  /** Properties (E) has the item's dialog open: OK there changes the carried item instead of making a new one. */
  editing?: boolean;
}

let current: SchCarry | null = null;
/** The point the canvas last snapped the cursor to for a placing tool (`BestSnapAnchor`): where the tool's hotkey places the item. */
let cursor: [number, number] | null = null;
const listeners = new Set<() => void>();

const emit = () => listeners.forEach((l) => l());
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export const getSchCarry = (): SchCarry | null => current;

export function setSchCarry(next: SchCarry | null): void {
  current = next;
  emit();
}

/** Drop what the tool holds (Esc, Undo, Delete, leaving the sheet, the tool or the tab). Returns whether there was something, so Esc is used up. */
export function cancelSchCarry(): boolean {
  if (!current) return false;
  current = null;
  emit();
  return true;
}

export const useSchCarry = (): SchCarry | null => useSyncExternalStore(subscribe, getSchCarry, getSchCarry);

export function noteCarryCursor(p: [number, number] | null): void {
  cursor = p;
}

export const carryCursor = (): [number, number] | null => cursor;
