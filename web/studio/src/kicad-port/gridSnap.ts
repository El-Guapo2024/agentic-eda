// The grid half of snapping: round to the grid from an arbitrary origin, with Ctrl (Cmd on macOS) turning the grid off. A port of common/tool/grid_helper.cpp
// (GRID_HELPER::computeNearest and the grid half of Align).
//
// The anchors -- pads, track ends, corners, centres, intersections, midpoints, the snap hysteresis, the snap lines and the construction geometry -- are
// kicad-port/pcbGridHelper.ts (PCB_GRID_HELPER::BestSnapAnchor) and kicad-port/schGridHelper.ts (EE_GRID_HELPER::BestSnapAnchor), which replaced the pure
// nearest-anchor function that used to be here.
//
// A real, source-verified correction to how this behavior is often described: it is NOT "Shift disables the grid." Per pcbnew/tools/edit_tool_move_fct.cpp:
//   grid.SetSnap( !evt->Modifier( MD_SHIFT ) );                    // Shift -> no ANCHOR snap
//   grid.SetUseGrid( ... && !evt->DisableGridSnapping() );          // Ctrl  -> no GRID round
// and tool_event.h: `DisableGridSnapping() { return Modifier(MD_CTRL); }`. So Ctrl disables the plain grid round-off (move at full precision) and Shift disables
// snapping to nearby item anchors (the grid round-off still applies).

export interface Point {
  x: number;
  y: number;
}

/** grid_helper.cpp GRID_HELPER::computeNearest, byte-for-byte (KiROUND(x) == Math.round(x) for the magnitudes a board ever has). */
export function computeNearest(point: Point, grid: Point, origin: Point): Point {
  return {
    x: grid.x > 0 ? Math.round((point.x - origin.x) / grid.x) * grid.x + origin.x : point.x,
    y: grid.y > 0 ? Math.round((point.y - origin.y) / grid.y) * grid.y + origin.y : point.y,
  };
}

export interface GridSnapModifiers {
  /** tool_event.h TOOL_EVENT::DisableGridSnapping(): Ctrl (Cmd on macOS, same substitution as everywhere else in this app) disables the grid round-off entirely. */
  ctrlOrCmd: boolean;
  /** edit_tool_move_fct.cpp: Shift disables snapping to nearby item anchors. Does NOT affect the grid round-off. */
  shiftKey: boolean;
}

/** grid_helper.cpp GRID_HELPER::Align minus the aux-axis special case (this app has no "auxiliary axis" origin marker) -- the grid half of the decision, before any anchor is considered. */
export function alignToGrid(point: Point, gridUm: number, origin: Point, modifiers: Pick<GridSnapModifiers, "ctrlOrCmd">): Point {
  if (modifiers.ctrlOrCmd || gridUm <= 0) return point;
  return computeNearest(point, { x: gridUm, y: gridUm }, origin);
}
