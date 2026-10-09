// Plain grid round-off with the editor's grid origin: `snap` / `snapPoint`, for the tools that snap a point to the grid and nothing else (the footprint and symbol
// editors' click points, a typed position). The board and schematic editors' placing tools go through their grid helpers (components/canvas/pcbSnap.ts,
// components/schematic/schSnap.ts: `BestSnapAnchor`). Grid snap alone is still what the backend enforces for a part's placement: `board::step()` snaps
// every placement command to `meta.snap_um` server-side (crates/cli/src/board.rs), so this is a *preview* convenience, not the source of truth.

import { NO_ORIGIN, snapAxis, type Origin } from "../../kicad-port/gridOrigin";

export function snap(valueUm: number, gridUm: number): number {
  if (gridUm <= 0) return Math.round(valueUm);
  return Math.round(valueUm / gridUm) * gridUm;
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

export function snapPoint(xUm: number, yUm: number, gridUm: number): [number, number] {
  return [snapAxis(xUm, gridUm, snapOrigin.x), snapAxis(yUm, gridUm, snapOrigin.y)];
}
