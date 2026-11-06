// KiCad 10's grid items as placement frames (KiCad 25ec2335; the 8303b2ad source the rest of the port reads predates grid items): a footprint carried into a grid that affects
// placement takes that grid's orientation, a quarter turn at most at a time.
//
//   pcbnew/pcb_grid_item.cpp              PCB_GRID_ITEM::GetOrientationAt, FindActiveGridAt, GridFrameAngleAt, GridFrameRotationDelta   [`gridOrientationAt`],
//                                                                                                                                   [`gridFrameAngleAt`], [`gridFrameRotationDelta`]
//   pcbnew/tools/edit_tool_move_fct.cpp   EDIT_TOOL::doMoveSelection's findSingleFp, selectionHasFp, applyMoveFrameOrientation       [`carryFrameTurn`]
//   pcbnew/tools/board_editor_control.cpp BOARD_EDITOR_CONTROL::PlaceFootprint's applyPlacementFrameOrientation                       [`placementFrameTurn`]
//
// The grid's three roles (`PCB_GRID_AFFECTS`) as KiCad 25ec2335 reads them:
//   * Cursor: `PCB_GRID_HELPER::Align` snaps to the grid's points (kicad-port/gridItem.ts `alignWithGridItems`) -- for every tool that asks the grid helper: drawing, the
//     move tool's cursor, and the router's route points, track ends and vias (`snapToItem` / `Align` on the wire and via grids, components/canvas/pcbSnap.ts `routePoint`).
//   * Placement: the frame here -- the move tool and Place Footprint turn a footprint to the orientation of the placement grid under it (`GridFrameAngleAt( .., PLACEMENT )`);
//     the position still snaps through the cursor role.
//   * Routing: declared, read, written and shown in Grid Properties, but nothing of KiCad 25ec2335 asks for it (`PCB_GRID_ROLE::ROUTING` has no caller of
//     `FindActiveGridAt` or `GridFrameAngleAt`), so it changes nothing here either.
//
// Angles are degrees in KiCad's sense: counter-clockwise on the screen (y down). Pure: no store, no DOM. Unit-tested in gridFrame.test.ts.
import type { BoardGridItem } from "../api/pcbItemTypes";
import type { BoardState } from "../api/types";
import { findActiveGridAt, type GridRole } from "./gridItem";
import { opsMatrix, type CarryOp } from "./pcbCarry";
import { padParent } from "./pcbItems";

type P = readonly [number, number];

/**
 * `PCB_GRID_ITEM::GetOrientationAt`: "World-space angle of this grid's local-x axis at `p`": a cartesian grid's orientation; a polar grid's radial direction at `p` (the spoke from
 * its centre through `p`, its orientation at the centre itself). "std::atan2 is math-CCW (y-up); EDA_ANGLE is screen-CCW (y-down) -- negate."
 */
export function gridOrientationAt(g: BoardGridItem, p: P): number {
  if (g.kind !== "polar") return g.angle;
  const [lx, ly] = [p[0] - g.x, p[1] - g.y];
  if (lx === 0 && ly === 0) return g.angle;
  const deg = (-Math.atan2(ly, lx) * 180) / Math.PI;
  return deg === 0 ? 0 : deg; // no -0
}

/** `GridFrameAngleAt( board, pos, role )`: the orientation at `p` of the grid of `role` that owns `p` (`FindActiveGridAt`: a selected grid is left out), 0 where none does. */
export function gridFrameAngleAt(grids: readonly BoardGridItem[], p: P, role: GridRole, selected: ReadonlySet<string> = new Set()): number {
  const g = findActiveGridAt(grids, p, role, selected);
  return g ? gridOrientationAt(g, p) : 0;
}

/** `KiROUND` of a double: half away from zero. */
function kiRound(v: number): number {
  return v < 0 ? -Math.round(-v) : Math.round(v);
}

/**
 * `GridFrameRotationDelta( from, to, rotationStep )`: how far to turn from one frame to the next -- "Cartesian grids repeat every quarter turn, so never rotate by more than that":
 * the difference less its nearest multiple of `min( step, 90 )` (`EDA_ANGLE::Snapped`), 0 when the frames are the same.
 */
export function gridFrameRotationDelta(fromDeg: number, toDeg: number, stepDeg: number): number {
  if (toDeg === fromDeg) return 0;
  const period = Math.min(stepDeg, 90);
  const raw = toDeg - fromDeg;
  const snapped = period <= 0 ? raw : period * kiRound(raw / period);
  return raw - snapped;
}

/** `findSingleFp`: the one footprint of the carried `refs` when the rest are its own pads; null for none, two, or a mix. `selectionHasFp`: any footprint at all. */
export function frameFootprint(board: BoardState, refs: readonly string[]): { single: string | null; any: boolean } {
  const isFp = (id: string) => board.parts.some((p) => p.placed && p.ref === id);
  let single: string | null = null;
  let mixed = false;
  for (const id of refs) {
    if (isFp(id)) {
      if (single) mixed = true;
      single = id;
    } else if (padParent(board, id) === null) mixed = true;
  }
  if (single && refs.some((id) => !isFp(id) && padParent(board, id) !== single)) mixed = true;
  return { single: mixed ? null : single, any: refs.some(isFp) };
}

/** What the carry keeps between pointer moves: the frame angle last applied (`prevFrameAngle`), taken at the pick-up point first. */
export interface FrameTurn {
  ops: CarryOp[];
  frameDeg: number;
}

/**
 * `applyMoveFrameOrientation` for a carry of `refs` that has moved by `(dx, dy)` after the presses `ops`: the frame angle of the placement grid at the pivot -- the one footprint's
 * position (`frameFp->GetPosition()`), or else the cursor (the grab point moved) -- and, when it differs from `prevDeg`, a turn of `GridFrameRotationDelta` about that pivot
 * added to the presses (in the items' frame before the move, where the presses are kept). `prevDeg` undefined: the carry has just picked the items up, and the frame there
 * "already carries that frame's orientation, so the first cursor move must not rotate it again" -- it is taken at the pick-up position. Null when the carry turns nothing: no
 * footprint is in it (`frameRotate` false), or the board has no grid item.
 */
export function carryFrameTurn(
  board: BoardState,
  refs: readonly string[],
  ops: readonly CarryOp[],
  dx: number,
  dy: number,
  grab: P,
  prevDeg: number | undefined,
  stepDeg: number,
  selected: ReadonlySet<string> = new Set(refs)
): FrameTurn | null {
  const grids = board.drawings?.grid_items ?? [];
  // No grid item: every frame is 0, nothing ever turns.
  if (grids.length === 0) return null;
  const { single, any } = frameFootprint(board, refs);
  if (!single && !any) return null;
  const part = single ? board.parts.find((p) => p.ref === single) : undefined;
  // The pivot before the move: the footprint's position as the presses so far have left it, or the grab point.
  const m = opsMatrix(ops);
  const at = part?.at ? [m[0] * part.at[0] + m[2] * part.at[1] + m[4], m[1] * part.at[0] + m[3] * part.at[1] + m[5]] : [grab[0], grab[1]];
  const pivot: [number, number] = [at[0]!, at[1]!];
  const prev = prevDeg ?? gridFrameAngleAt(grids, part?.at ? [part.at[0], part.at[1]] : grab, "placement", selected);
  const next = gridFrameAngleAt(grids, [pivot[0] + dx, pivot[1] + dy], "placement", selected);
  const delta = gridFrameRotationDelta(prev, next, stepDeg);
  if (delta === 0) return { ops: [...ops], frameDeg: next };
  return { ops: [...ops, { kind: "turn", degCcw: delta, pivot }], frameDeg: next };
}

/**
 * `PlaceFootprint`'s `applyPlacementFrameOrientation`: a footprint being placed at `at` turns about its position by `GridFrameRotationDelta( prev, frame at at )`; `prevDeg` is 0
 * for a footprint fresh from the library (`SetOrientation( ANGLE_0 )`, `prevFrameAngle = ANGLE_0`), or the frame at its position for one handed over from another command.
 * For the carry loop of Place Footprint to call on every motion; returns the turn (counter-clockwise degrees) and the frame to keep.
 */
export function placementFrameTurn(grids: readonly BoardGridItem[], at: P, prevDeg: number, stepDeg: number): { turnDeg: number; frameDeg: number } {
  const next = gridFrameAngleAt(grids, at, "placement");
  return { turnDeg: gridFrameRotationDelta(prevDeg, next, stepDeg), frameDeg: next };
}
