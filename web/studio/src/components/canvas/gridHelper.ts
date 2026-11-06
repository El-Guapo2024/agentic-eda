// Plain grid round-off with the editor's grid origin: `snap` / `snapPoint`, for the tools that snap a point to the grid and nothing else (the footprint and symbol
// editors' click points, a typed position). The board and schematic editors' placing tools go through their grid helpers (components/canvas/pcbSnap.ts,
// components/schematic/schSnap.ts: `BestSnapAnchor`). Grid snap alone is still what the backend enforces for a part's placement: `board::step()` snaps
// every placement command to `meta.snap_nm` server-side (crates/cli/src/board.rs), so this is a *preview* convenience, not the source of truth.

import { NO_ORIGIN, snapAxis, type Origin } from "../../kicad-port/gridOrigin";

export function snap(valueNm: number, gridNm: number): number {
  if (gridNm <= 0) return Math.round(valueNm);
  return Math.round(valueNm / gridNm) * gridNm;
}

// The point the editing grid of the editor on screen is anchored at (`common.Control.gridSetOrigin`): the board's own in the PCB editor, the
// session's in the Footprint Editor, (0, 0) elsewhere. `actions/useSnapOrigin.ts` keeps it current; every tool that snaps a click through
// `snapPoint` reads it here, so none of them has to be told.
let snapOrigin: Origin = NO_ORIGIN;

export function setSnapOrigin(at: Origin): void {
  snapOrigin = at;
}

export function getSnapOrigin(): Origin {
  return snapOrigin;
}

// `GAL::GetGridSnapping()` of the editor on screen (UI-LEFT-2 CC5): its Display Options' "Snap to grid:" -- Always, When grid shown (while its grid is shown) or
// Never (kicad-port/windowOptions.ts `gridSnappingActive`). `actions/useSnapOrigin.ts` keeps it current with the snap origin; `snapPoint` and the four editors' grid
// helpers (`GridEnv.gridSnapping`, `SetUseGrid( GetGridSnapping() && !DisableGridSnapping() )`) read it, as KiCad's tools and `GetCursorPosition` read the GAL's.
let gridSnapping = true;

export function setGridSnapping(on: boolean): void {
  gridSnapping = on;
}

export function getGridSnapping(): boolean {
  return gridSnapping;
}

/**
 * The grid point of the editor on screen nearest a point (`GetCursorPosition( true )`: `GetGridPoint` when `GetGridSnapping()`), anchored at the snap origin. With the
 * editor's grid snapping off (Snap to grid: Never, or When grid shown with the grid hidden) the point itself, in whole units.
 */
export function snapPoint(xNm: number, yNm: number, gridNm: number): [number, number] {
  if (!gridSnapping) return [Math.round(xNm), Math.round(yNm)];
  return [snapAxis(xNm, gridNm, snapOrigin.x), snapAxis(yNm, gridNm, snapOrigin.y)];
}

/** The grid an editor's own rounding should use: `gridNm` while the editor on screen snaps to its grid, else 0 (no grid: `computeNearest` leaves a point as it is). */
export function snapGridNm(gridNm: number): number {
  return gridSnapping ? gridNm : 0;
}
