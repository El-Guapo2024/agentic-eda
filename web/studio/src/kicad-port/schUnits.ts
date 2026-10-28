// Place Next Symbol Unit -- `SCH_DRAWING_TOOLS::PlaceNextSymbolUnit` (eeschema/tools/sch_drawing_tools.cpp at 8303b2ad): with one multi-unit symbol
// selected, the lowest unit of it not yet on the sheet is placed next, as a copy of the selected symbol under the same reference.
import type { LibSymbol } from "../api/types";
import { subReference, type SubpartIds } from "./unitLetter";

/** How many units a library symbol has (`LIB_SYMBOL::GetUnitCount`): its `unit_count` when the server sends it, else the highest unit any of its pins or
 *  graphics belongs to (0 means "shared by every unit"). */
export function unitCountOf(lib: (Pick<LibSymbol, "pins" | "graphics"> & { unit_count?: number }) | undefined): number {
  if (!lib) return 1;
  if (lib.unit_count != null && lib.unit_count >= 1) return lib.unit_count;
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

/** The project's unit naming as a sheet response carries it (`Schematic.subpart_id_separator` / `subpart_first_id`). */
export function subpartIdsOf(sch: { subpart_id_separator?: number | null; subpart_first_id?: number | null } | null | undefined): SubpartIds {
  return { separator: sch?.subpart_id_separator ?? null, firstId: sch?.subpart_first_id ?? null };
}

/**
 * What `SCH_SYMBOL::GetRef( &aSheet, true )` puts after the reference (sch_symbol.cpp: `if( aIncludeUnit && GetUnitCount() > 1 ) ref += subRef;`, `subRef =
 * SubReference( instance.m_Unit )`): the placed unit's sub-reference whenever the library symbol has more than one unit, however many of its units are placed
 * (r7 M3), in the project's unit naming (r7 H5). Nothing for a single-unit symbol, or one with no library symbol.
 */
export function referenceUnitSuffix(lib: (Pick<LibSymbol, "pins" | "graphics" | "unit_names"> & { unit_count?: number }) | undefined, unit: number, ids: SubpartIds = {}): string {
  if (!lib || unitCountOf(lib) <= 1) return "";
  return subReference(unit, ids, lib.unit_names?.[String(unit)] ?? null);
}
