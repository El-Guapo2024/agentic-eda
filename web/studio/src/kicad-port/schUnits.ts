// Place Next Symbol Unit -- `SCH_DRAWING_TOOLS::PlaceNextSymbolUnit` (eeschema/tools/sch_drawing_tools.cpp at 8303b2ad): with one multi-unit symbol
// selected, the lowest unit of it not yet on the sheet is placed next, as a copy of the selected symbol under the same reference.
import type { LibSymbol } from "../api/types";

/** How many units a library symbol has: the highest unit any of its pins or graphics belongs to (0 means "shared by every unit"). */
export function unitCountOf(lib: Pick<LibSymbol, "pins" | "graphics"> | undefined): number {
  if (!lib) return 1;
  let n = 1;
  for (const p of lib.pins) n = Math.max(n, p.unit);
  for (const g of lib.graphics) n = Math.max(n, g.unit);
  return n;
}

export type NextUnit = { ok: true; unit: number } | { ok: false; message: string };

/**
 * `GetUnplacedUnitsForSymbol` and the checks around it: which unit comes next, or why none does (the info-bar messages).
 * `placedUnits` are the units of this reference already on the sheet.
 */
export function nextUnitToPlace(unitCount: number, placedUnits: Iterable<number>): NextUnit {
  if (unitCount <= 1) return { ok: false, message: "This symbol has only one unit." };
  const placed = new Set(placedUnits);
  for (let u = 1; u <= unitCount; u++) if (!placed.has(u)) return { ok: true, unit: u };
  return { ok: false, message: "All units of this symbol are already placed." };
}
