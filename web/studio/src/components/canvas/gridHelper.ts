// Snap helpers for the move/place/draw tools.
//
// `snap`/`snapPoint` are the plain grid-round-off every tool still uses
// for its own click points (route/zone/shape placement) -- unchanged.
// Grid snap alone is still what the backend enforces anyway: `board::
// step()` snaps every placement command to `meta.snap_um` server-side
// (crates/cli/src/board.rs), so this is a *preview* convenience, not the
// source of truth.
//
// `snapWithAnchors` is the real PCB_GRID_HELPER port (pcbnew/tools/
// pcb_grid_helper.cpp's BestSnapAnchor, common/tool/grid_helper.cpp's
// Align/computeNearest -- see kicad-port/gridSnap.ts for the exact
// ported algorithm and its documented gaps) -- anchor snapping to pad
// centers, track ends/midpoints, via centers, and footprint origins,
// plus the real Ctrl-disables-grid/Shift-disables-anchor-snap modifier
// split. Wired into Canvas.tsx's move-drag preview; other tools (route/
// zone/shape placement) still use plain `snapPoint` for now.

import { computeNearest, bestSnapPoint, collectAnchors, DEFAULT_MAGNETIC_SETTINGS, type AnchorSourceBoard, type GridSnapModifiers, type SnapAnchor } from "../../kicad-port/gridSnap";
import { computeVisibleGridSize } from "../../kicad-port/grid";

export function snap(valueUm: number, gridUm: number): number {
  if (gridUm <= 0) return Math.round(valueUm);
  return Math.round(valueUm / gridUm) * gridUm;
}

export function snapPoint(xUm: number, yUm: number, gridUm: number): [number, number] {
  return [snap(xUm, gridUm), snap(yUm, gridUm)];
}

export type { GridSnapModifiers, SnapAnchor };

/**
 * The move tool's real snap decision: grid round-off (unless Ctrl),
 * then prefer a nearby pad/track-end/track-mid/via/footprint-origin
 * anchor (unless Shift) when one is within `kicad-port/gridSnap.ts`'s
 * ported snap radius. `excludeOwnerId` should be the ref/id of whatever
 * is being dragged, so it never snaps to its own anchors.
 */
export function snapWithAnchors(xUm: number, yUm: number, gridUm: number, scalePxPerUm: number, board: AnchorSourceBoard, modifiers: GridSnapModifiers, excludeOwnerId?: string): { x: number; y: number; snappedTo: SnapAnchor | null } {
  const anchors = collectAnchors(board, DEFAULT_MAGNETIC_SETTINGS, excludeOwnerId);
  const visibleGridUm = computeVisibleGridSize(gridUm, scalePxPerUm);
  const { point, snappedTo } = bestSnapPoint({ x: xUm, y: yUm }, gridUm, { x: 0, y: 0 }, scalePxPerUm, visibleGridUm, anchors, modifiers);
  return { x: point.x, y: point.y, snappedTo };
}

// Re-exported for anything that just wants the grid-only half (e.g. a
// future caller with no board/anchors at hand) without pulling in the
// anchor machinery above.
export { computeNearest };
