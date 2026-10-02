// Port of KIGFX::PREVIEW::BEZIER_GEOM_MANAGER (include/preview_items/
// bezier_geom_manager.h, common/preview_items/bezier_geom_manager.cpp) -- the
// construction state machine behind pcbnew's "Draw Bezier Curve" tool
// (`pcbnew.InteractiveDrawing.bezier`, Ctrl+Shift+B, DRAWING_TOOL::drawOneBezier):
// four points -- the start, the first control point, the end, the second
// control point -- then the next curve of the chain starts at this one's end.
//
// Plain data in / plain data out (no mutation) so the tool state can live in
// the React store, same as arcGeom.ts. `MULTISTEP_GEOM_MANAGER`'s step logic is
// identical to arcGeom.ts's (a rejected point steps back, an accepted one
// forward, clamped to [0, COMPLETE]).

export type BezierPt = readonly [number, number];

/** `BEZIER_GEOM_MANAGER::BEZIER_STEPS`. */
export const BEZIER_SET_START = 0;
export const BEZIER_SET_CONTROL1 = 1;
export const BEZIER_SET_END = 2;
export const BEZIER_SET_CONTROL2 = 3;
export const BEZIER_COMPLETE = 4;
export type BezierStep = 0 | 1 | 2 | 3 | 4;

export interface BezierGeom {
  step: BezierStep;
  start: BezierPt;
  controlC1: BezierPt;
  end: BezierPt;
  /** `m_controlC2`: the RAW second control point -- the one the cursor is on. The curve's real C2 is its reflection over `end` (`bezierControlC2`), "so that the cursor will be on the C1 point of the next bezier". */
  controlC2: BezierPt;
  /** `MULTISTEP_GEOM_MANAGER::m_lastPoint`. */
  lastPoint: BezierPt;
}

export function newBezierGeom(): BezierGeom {
  return { step: BEZIER_SET_START, start: [0, 0], controlC1: [0, 0], end: [0, 0], controlC2: [0, 0], lastPoint: [0, 0] };
}

/** `BEZIER_GEOM_MANAGER::GetControlC2`: `m_end - ( m_controlC2 - m_end )`. */
export function bezierControlC2(g: BezierGeom): [number, number] {
  return [g.end[0] - (g.controlC2[0] - g.end[0]), g.end[1] - (g.controlC2[1] - g.end[1])];
}

/** `BEZIER_GEOM_MANAGER::acceptPoint` -> `setStart`/`setControlC1`/`setEnd`/`setControlC2`. */
function acceptPoint(g: BezierGeom, p: BezierPt): { geom: BezierGeom; accepted: boolean } {
  switch (g.step) {
    case BEZIER_SET_START:
      // "Prevents weird-looking loops if the control points aren't initialized"
      return { geom: { ...g, start: p, end: p, controlC1: p, controlC2: p }, accepted: true };
    case BEZIER_SET_CONTROL1:
      // "It's possible to set the control 1 point to the same as the start point"
      return { geom: { ...g, controlC1: p, end: p, controlC2: p }, accepted: true };
    case BEZIER_SET_END:
      return { geom: { ...g, end: p, controlC2: p }, accepted: p[0] !== g.start[0] || p[1] !== g.start[1] };
    case BEZIER_SET_CONTROL2:
      // "It's possible to set the control 2 point to the same as the end point"
      return { geom: { ...g, controlC2: p }, accepted: true };
    default:
      return { geom: g, accepted: false };
  }
}

/** `MULTISTEP_GEOM_MANAGER::AddPoint( aPt, aLockIn )`. */
export function bezierAddPoint(g: BezierGeom, p: BezierPt, lockIn: boolean): BezierGeom {
  const { geom, accepted } = acceptPoint({ ...g, lastPoint: p }, p);
  if (!lockIn) return geom;
  const step = Math.min(Math.max(geom.step + (accepted ? 1 : -1), 0), BEZIER_COMPLETE) as BezierStep;
  return { ...geom, step };
}

/** `MULTISTEP_GEOM_MANAGER::RemoveLastPoint` (Backspace -> `deleteLastPoint`). */
export function bezierRemoveLastPoint(g: BezierGeom): BezierGeom {
  const stepped: BezierGeom = { ...g, step: Math.min(Math.max(g.step - 1, 0), BEZIER_COMPLETE) as BezierStep };
  return acceptPoint(stepped, g.lastPoint).geom;
}

export function bezierIsComplete(g: BezierGeom): boolean {
  return g.step === BEZIER_COMPLETE;
}

/** `started()` in drawOneBezier: past the first point. */
export function bezierStarted(g: BezierGeom | null | undefined): boolean {
  return !!g && g.step > BEZIER_SET_START;
}

export interface BezierCurve {
  start: [number, number];
  c1: [number, number];
  c2: [number, number];
  end: [number, number];
}

/** `bezier->SetStart/SetBezierC1/SetEnd/SetBezierC2( GetStart(), GetControlC1(), GetEnd(), GetControlC2() )`. */
export function bezierCurveOf(g: BezierGeom): BezierCurve {
  return { start: [g.start[0], g.start[1]], c1: [g.controlC1[0], g.controlC1[1]], c2: bezierControlC2(g), end: [g.end[0], g.end[1]] };
}

/**
 * A left click while "Draw Bezier" is armed (`drawOneBezier`'s `IsClick( BUT_LEFT )`
 * branch): lock the point in. `geom` is the tool state to keep; `curve` is set when
 * this click completed a curve. A completed curve CHAINS (`DrawBezier`): the next
 * one starts at its end, with the first control point already placed as the mirror
 * of the finished curve's second (a smooth join) unless that control arm has zero
 * length, in which case the user places the new C1 themselves.
 */
export function bezierClick(prev: BezierGeom | null | undefined, p: BezierPt): { geom: BezierGeom | null; curve: BezierCurve | null } {
  const next = bezierAddPoint(prev ?? newBezierGeom(), p, true);
  if (!bezierIsComplete(next)) return { geom: next.step === BEZIER_SET_START ? null : next, curve: null };
  const curve = bezierCurveOf(next);
  return { geom: bezierChainFrom(curve), curve };
}

/** The `startingPoint` / `startingC1` priming `DrawBezier` hands the next `drawOneBezier`: `bezierManager.AddPoint( *start, true ); [AddPoint( *c1, true )]`. */
export function bezierChainFrom(curve: BezierCurve): BezierGeom {
  let g = bezierAddPoint(newBezierGeom(), curve.end, true);
  if (curve.end[0] !== curve.c2[0] || curve.end[1] !== curve.c2[1]) {
    // "Mirror the control point across the end point to get a tangent control point"
    g = bezierAddPoint(g, [curve.end[0] - (curve.c2[0] - curve.end[0]), curve.end[1] - (curve.c2[1] - curve.end[1])], true);
  }
  return g;
}

/**
 * A double-click (`drawOneBezier`: `AddPoint( cursorPos, true ); while( step < SET_END )
 * AddPoint( cursorPos, true )`): "use the current point for all remaining points",
 * then the curve is accepted AND the tool resets (`ACCEPTED_AND_RESET`: no chaining).
 * In the browser the double-click's second press has already been delivered as an
 * ordinary click (`bezierClick`), so `prev` already holds that point and only the
 * "remaining points" fill is left here. `null` when there is nothing to commit (the
 * curve would have no length).
 */
export function bezierFinishDouble(prev: BezierGeom, p: BezierPt): BezierCurve | null {
  let g = prev;
  while (g.step < BEZIER_SET_END) g = bezierAddPoint(g, p, true);
  if (g.step < BEZIER_SET_END) return null;
  const curve = bezierCurveOf(g);
  return curve.start[0] === curve.end[0] && curve.start[1] === curve.end[1] ? null : curve;
}

/** Cursor motion: `bezierManager.AddPoint( cursorPos, false )` -- update the geometry only. */
export function bezierMotion(prev: BezierGeom, p: BezierPt): BezierGeom {
  return bezierAddPoint(prev, p, false);
}
