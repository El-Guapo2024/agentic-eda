// The ERC markers Find walks with the items of the sheet in view (UI-LEFT-2 SS10): eeschema's `SCH_FIND_REPLACE_TOOL::nextMatch` walks `aScreen->Items()`,
// the screen's `SCH_MARKER`s among them, each matched by `SCH_MARKER::Matches` on its message (`m_rcItem->GetErrorMessage( true )`). The studio keeps the last
// ERC report in the browser (`state.erc`), so the markers go with the request to `POST /api/sch/find`: each marker the canvas draws on a sheet (its
// `location` resolved here on that sheet's schematic, `ercMarkerPosition`, as painter.ts's `drawErcMarkers` places it), keyed `erc:<index>` into the report;
// a Find over every sheet sends every sheet's (UI-LEFT-3 S3-34), each with its sheet.
import type { ErcViolation, FindMarker, Schematic } from "../api/types";
import { ercMessage } from "./rcItems";
import { ercMarkerPosition } from "../components/schematic/ercMarkerPosition";

export const ERC_MARKER_KEY = "erc:";

/** The markers of `violations` drawn on `sch`, for Find. */
export function ercFindMarkers(violations: readonly ErcViolation[] | null | undefined, sch: Schematic | null | undefined): FindMarker[] {
  if (!violations || !sch) return [];
  const out: FindMarker[] = [];
  violations.forEach((v, i) => {
    const at = ercMarkerPosition(v.location, sch)?.at;
    if (at) out.push({ key: `${ERC_MARKER_KEY}${i}`, text: ercMessage(v), at: [Math.round(at[0]), Math.round(at[1])] });
  });
  return out;
}

/**
 * The markers of `violations` on every sheet path of the hierarchy, for a Find over every sheet (`FindNext`'s `searchAllSheets`: `nextMatch( screen, sheet, .. )`
 * of each sheet in turn, each walking its screen's items, the markers among them): on each sheet the markers the canvas draws there (a marker is an item of its
 * screen, so a screen two sheets show has it on both, as KiCad's), each with its sheet (`"id/id"`, the placement ids from the root).
 */
export function ercFindMarkersOfSheets(violations: readonly ErcViolation[] | null | undefined, sheets: readonly { path: readonly string[]; sch: Schematic }[]): FindMarker[] {
  if (!violations || violations.length === 0) return [];
  return sheets.flatMap(({ path, sch }) => ercFindMarkers(violations, sch).map((m) => ({ ...m, sheet: path.join("/") })));
}

/** The report's index of a marker match's id (`erc:<index>`), or null for any other item. */
export function ercMarkerIndex(id: string): number | null {
  if (!id.startsWith(ERC_MARKER_KEY)) return null;
  const n = Number(id.slice(ERC_MARKER_KEY.length));
  return Number.isInteger(n) && n >= 0 ? n : null;
}
