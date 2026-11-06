// The item a schematic placing tool holds (kicad-port/schPlaceTool.ts): a label, directive label, text, power symbol or symbol made by its dialog or chooser,
// riding the cursor until the click that places it. SchematicView draws it in the page (kicad-port/schCarryPreview.ts: `TwoClickPlace`'s `updatePreview`, `PlaceSymbol`'s
// `addSymbol` keep it in the tool's own view), with what only the server knows -- a symbol's autoplaced fields, a global label's Sheet References, a text's variables --
// from the server's drawing of the command the click will send (`POST /api/sch/place_preview`, drawn at `PREVIEW_AT`) once it comes back;
// actions/schPlaceActions.ts sets, turns and places it.
//
// A store of its own (read with `useSyncExternalStore`, written by plain functions), as the paste in the hand is (state/schPasteStore.ts), so
// `store.tsx` -- shared by every editor -- keeps no schematic-only field.
import { useSyncExternalStore } from "react";
import type { LibSymbol, Schematic } from "../api/types";
import { carriedPreview, type ServerDrawing } from "../kicad-port/schCarryPreview";
import { withPaste, type SchPastePreview } from "../kicad-port/schClipboard";
import { PREVIEW_AT, type Carry } from "../kicad-port/schPlaceTool";

export interface SchCarry extends Carry {
  /** The sheet it was made on (path of sheet-instance ids): an item belongs to that sheet. */
  sheet: string[];
  /** Where the item was made (the click, or the cursor for a hotkey): it is drawn there until the pointer moves. */
  at: [number, number];
  /** The carried symbol's or power symbol's library drawing when the sheet has none yet (the one the chooser loaded); the sheet's own wins. */
  lib?: LibSymbol | null;
  /** The server's last drawing of the item (of this state of it, or an earlier one); null until one comes back, and for an item the page draws alone. */
  server: ServerDrawing | null;
  /** Bumped by every change of the item, so a drawing that comes back late is known for an earlier state. */
  seq: number;
  /** Properties (E) has the item's dialog open: OK there changes the carried item instead of making a new one. */
  editing?: boolean;
}

/** The carried item as the page draws it, at `PREVIEW_AT` (kicad-port/schCarryPreview.ts). */
export function carryDrawing(carry: SchCarry, sch: Pick<Schematic, "lib_symbols">): SchPastePreview {
  return carriedPreview(carry.item, { libSymbols: sch.lib_symbols, lib: carry.lib, server: carry.server });
}

/** `sch` with the carried item on it where it was made (or at `at`): what the dialogs opened on the carried item (E, U, V, F) read it from. */
export function schematicWithCarry(sch: Schematic, carry: SchCarry, at: readonly [number, number] = carry.at): Schematic {
  return withPaste(sch, carryDrawing(carry, sch), at[0] - PREVIEW_AT[0], at[1] - PREVIEW_AT[1]);
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

/** Where Place Symbol's first click (or its hotkey) was when it opened the Symbol Chooser: the symbol chosen rides from there. */
let chooserAt: [number, number] | null = null;

export function noteChooserAt(p: [number, number] | null): void {
  chooserAt = p;
}

export const pendingChooserAt = (): [number, number] | null => chooserAt;

/**
 * When the canvas last drew the carried item, and which state of it (`seq`): the test hook's proof that the item appears and turns with no round trip. `requests`
 * counts the server drawings asked for (none for an item the page draws alone).
 */
const timing = { paintedSeq: -1, paintedAt: 0, madeAt: 0, requests: 0 };

export function noteCarryMade(): void {
  timing.madeAt = performance.now();
}

export function noteCarryPainted(seq: number): void {
  if (timing.paintedSeq === seq) return;
  timing.paintedSeq = seq;
  timing.paintedAt = performance.now();
}

export function noteCarryRequest(): void {
  timing.requests++;
}

export const carryTiming = (): { paintedSeq: number; paintMs: number; requests: number } => ({ paintedSeq: timing.paintedSeq, paintMs: Math.max(0, timing.paintedAt - timing.madeAt), requests: timing.requests });
