// Pure logic of the schematic control actions (`eeschema.EditorControl.*`, `eeschema.NavigateTool.*`, `eeschema.Interactive.increment*`), kept apart from
// React so it can be tested: which state an attribute toggle goes to, what a sheet click or a label hover hits, what the Increment actions change.
import { incrementString, type IncrementOptions } from "./stringIncrement";

// ---------------------------------------------------------------------------------------------------------------------------------- attributes

export type SymbolAttrKey = "dnp" | "exclude_from_bom" | "exclude_from_board" | "exclude_from_sim";

/** The four attribute actions and the field each one flips. */
export const ATTRIBUTE_ACTIONS: Readonly<Record<string, SymbolAttrKey>> = {
  "eeschema.EditorControl.setDNP": "dnp",
  "eeschema.EditorControl.setExcludeFromBOM": "exclude_from_bom",
  "eeschema.EditorControl.setExcludeFromBoard": "exclude_from_board",
  "eeschema.EditorControl.setExcludeFromSimulation": "exclude_from_sim",
};

/**
 * `SCH_EDIT_TOOL::SetAttribute`'s `new_state`: the attribute is set on every selected item when any of them does not have it yet, and cleared
 * (on all of them) when they all do -- one press of the menu item always moves the whole selection to the same state.
 */
export function nextAttributeState(items: ReadonlyArray<Partial<Record<SymbolAttrKey, boolean>>>, key: SymbolAttrKey): boolean {
  return items.some((it) => !it[key]);
}

/** The state a check mark shows for an attribute (`attribDNPCond` ...): checked when every selected item has it. Empty selection: unchecked. */
export function attributeChecked(items: ReadonlyArray<Partial<Record<SymbolAttrKey, boolean>>>, key: SymbolAttrKey): boolean {
  return items.length > 0 && items.every((it) => !!it[key]);
}

// ---------------------------------------------------------------------------------------------------------------------------------- hit tests

export interface SheetBox {
  id: string;
  at: readonly [number, number];
  size: readonly [number, number];
}

/** The sheet whose rectangle holds the point (the last drawn wins where two overlap), else null. */
export function hitSheet(sheets: readonly SheetBox[], x: number, y: number): string | null {
  for (let i = sheets.length - 1; i >= 0; i--) {
    const s = sheets[i]!;
    if (x >= s.at[0] && x <= s.at[0] + s.size[0] && y >= s.at[1] && y <= s.at[1] + s.size[1]) return s.id;
  }
  return null;
}

export interface TextItem {
  id: string;
  at: readonly [number, number];
}

/** The label or text whose anchor is nearest the point, within `tolUm` -- the hover stand-in for `RequestSelection` on a label. */
export function nearestTextItem(items: readonly TextItem[], x: number, y: number, tolUm: number): string | null {
  let best: { id: string; d: number } | null = null;
  for (const it of items) {
    const d = Math.hypot(it.at[0] - x, it.at[1] - y);
    if (d <= tolUm && (!best || d < best.d)) best = { id: it.id, d };
  }
  return best?.id ?? null;
}

// ---------------------------------------------------------------------------------------------------------------------------------- increment

/** What `SCH_TOOL_BASE::Increment` may touch in the schematic: a label of one of the three kinds, or a free text. */
export type IncrementKind = "label:local" | "label:global" | "label:hierarchical" | "text";

export interface IncrementTarget {
  id: string;
  kind: IncrementKind;
  text: string;
}

/**
 * The text changes `Increment` makes. "Incrementing multiple types at once seems confusing though it would work": a selection of more than one
 * kind (a local label next to a global one, a label next to a text) does nothing. Each item whose text can move changes; one that cannot
 * (no number, nothing below zero) stays. Returns the changes, empty when nothing moves, or null for a mixed selection.
 */
export function planIncrement(targets: readonly IncrementTarget[], delta: number, rightIndex: number, options: IncrementOptions = {}): Array<{ id: string; text: string }> | null {
  if (targets.length === 0) return [];
  const kind = targets[0]!.kind;
  if (targets.some((t) => t.kind !== kind)) return null;
  const out: Array<{ id: string; text: string }> = [];
  for (const t of targets) {
    const next = incrementString(t.text, delta, rightIndex, options);
    if (next !== null && next !== t.text) out.push({ id: t.id, text: next });
  }
  return out;
}

/** The (delta, index) each of the five Increment actions passes (`ACTIONS::INCREMENT{ delta, index }`); the generic `increment` takes `{1, 0}` when no event brings one. */
export const INCREMENT_PARAMS: Readonly<Record<string, { delta: number; index: number }>> = {
  "eeschema.Interactive.increment": { delta: 1, index: 0 },
  "eeschema.Interactive.incrementPrimary": { delta: 1, index: 0 },
  "eeschema.Interactive.decrementPrimary": { delta: -1, index: 0 },
  "eeschema.Interactive.incrementSecondary": { delta: 1, index: 1 },
  "eeschema.Interactive.decrementSecondary": { delta: -1, index: 1 },
};
