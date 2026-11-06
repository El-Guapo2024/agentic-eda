// The Symbol Editor's Increment (`eeschema.Interactive.increment*`, `SCH_TOOL_BASE::Increment` in eeschema/tools/sch_tool_base.h at KiCad 8303b2ad, which
// `SYMBOL_EDITOR_EDIT_TOOL` runs for `ACTIONS::increment`, `incrementPrimary` / `decrementPrimary` and `incrementSecondary` / `decrementSecondary`; the selection tool
// sends `increment` for Shift+Alt+wheel (the primary part) and Ctrl+Alt+wheel (the secondary one)).
//
// Of the selection's pins and texts (`RequestSelection( incrementable )`; a selection of both kinds does nothing, "Incrementing multiple types at once seems confusing"):
// a pin's number when the cursor is in the number's box (`PIN_LAYOUT_CACHE::GetPinNumberBBox`), else its name when it is in the name's box; a text's text. The string moves
// by `STRING_INCREMENTER` (kicad-port/stringIncrement.ts) with `SetSkipIOSQXZ( m_isSymbolEditor )`: a pin's letters skip I, O, S, Q, X and Z. One commit, "Increment".
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";
import type { Box } from "./schItemGeom";
import { pointInBox } from "./schItemGeom";
import { incrementString } from "./stringIncrement";

/** A pin of the selection with the boxes of its name and number where the canvas draws them (internal nm, +y down), or a text of the selection. */
export type IncrementItem = { kind: "pin"; pin: LibrarySymbolPin; nameBox: Box | null; numberBox: Box | null } | { kind: "text"; text: Extract<LibrarySymbolGraphic, { kind: "text" }> };

/** The pins and texts `Increment` changes, as they are after it; null for a selection of both kinds (nothing is done). An item whose string cannot move is left out. */
export function planSymbolIncrement(items: readonly IncrementItem[], mouseNm: [number, number], delta: number, index: number): { pins: LibrarySymbolPin[]; texts: Extract<LibrarySymbolGraphic, { kind: "text" }>[] } | null {
  if (items.length === 0) return { pins: [], texts: [] };
  if (items.some((i) => i.kind !== items[0]!.kind)) return null;
  const options = { skipIOSQXZ: true };
  const pins: LibrarySymbolPin[] = [];
  const texts: Extract<LibrarySymbolGraphic, { kind: "text" }>[] = [];
  for (const item of items) {
    if (item.kind === "pin") {
      const { pin, numberBox, nameBox } = item;
      if (numberBox && pointInBox(mouseNm, numberBox)) {
        const next = incrementString(pin.number, delta, index, options);
        if (next !== null) pins.push({ ...pin, number: next });
      } else if (nameBox && pointInBox(mouseNm, nameBox)) {
        const next = incrementString(pin.name, delta, index, options);
        if (next !== null) pins.push({ ...pin, name: next });
      }
    } else {
      const next = incrementString(item.text.text, delta, index, options);
      if (next !== null) texts.push({ ...item.text, text: next });
    }
  }
  return { pins, texts };
}
