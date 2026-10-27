// The angle constraint `DRAWING_TOOL::drawShape` (pcbnew/tools/drawing_tool.cpp at 8303b2ad) puts on the second point of a line, a rectangle or a circle.
//
// - The mode: the line mode (`GetAngleSnapMode()`, `PCBNEW_SETTINGS::m_AngleSnapMode`) for a line, which Ctrl frees ("All other drawing uses the snap mode,
//   except that is disabled with the modifier key"); a rectangle and a circle are free, and Ctrl gives them `DEG45` ("Drawing rectangles and circles ignore
//   the snap behavior by default, but constrains when the modifier key is pressed").
// - The point: `twoPointMgr.GetOrigin() + GetVectorSnapped45( lineVector, shape == SHAPE_T::RECTANGLE )` (or `GetVectorSnapped90`) on the grid-snapped
//   cursor's vector from the first point. `GetVectorSnapped45` keeps the cursor's x (or y): on the axis when one side is more than twice the other, else the
//   smaller side brought to the larger (a square for a rectangle, `only45`). No second grid snap: the cursor was already on the grid.
import type { AngleSnapMode } from "./pcbParityState";
import { snapVector45 } from "./polygonGeom";

export type DrawShapeKind = "segment" | "rect" | "circle";

/** `GetVectorSnapped90`: the larger side kept, the other zeroed (x on a tie). */
export function snapVector90(v: readonly [number, number]): [number, number] {
  return Math.abs(v[0]) >= Math.abs(v[1]) ? [v[0], 0] : [0, v[1]];
}

/** The mode `drawShape` uses for `kind` under the line mode `mode`, with Ctrl (Cmd on a Mac) held or not. */
export function shapeAngleSnap(kind: DrawShapeKind, mode: AngleSnapMode, ctrl: boolean): AngleSnapMode {
  if (kind === "rect" || kind === "circle") return ctrl ? "45" : "direct";
  return ctrl ? "direct" : mode;
}

/** The second point `drawShape` gives a `kind` drawn from `from` with the (grid-snapped) cursor at `to`. */
export function shapeSecondPoint(kind: DrawShapeKind, mode: AngleSnapMode, ctrl: boolean, from: readonly [number, number], to: readonly [number, number]): [number, number] {
  const snapMode = shapeAngleSnap(kind, mode, ctrl);
  if (snapMode === "direct") return [to[0], to[1]];
  const v: [number, number] = [to[0] - from[0], to[1] - from[1]];
  const s = snapMode === "90" ? snapVector90(v) : snapVector45(v, kind === "rect");
  return [from[0] + s[0], from[1] + s[1]];
}
