// Port of KIGFX::PREVIEW::ARC_GEOM_MANAGER (include/preview_items/
// arc_geom_manager.h, common/preview_items/arc_geom_manager.cpp) and
// MULTISTEP_GEOM_MANAGER (multistep_geom_manager.h) -- the construction
// state machine behind pcbnew's "Draw Arc" tool (DRAWING_TOOL::drawArc):
// three clicks, the arc's centre, then its start point (which fixes the
// radius and start angle), then a point along the end radius (the end
// angle). The `/` hotkey (`pcbnew.InteractiveDrawing.arcPosture`,
// `arcManager.ToggleClockwise()`) flips which way round the arc goes.
//
// Everything is plain data in, plain data out (no mutation) so the tool
// state can live in the React store's `drawState` and unit tests need no
// DOM. Angles are degrees in `EDA_ANGLE( VECTOR2 )`'s convention: atan2(y, x)
// of the radius vector (y down on the board), with source's own special
// cases (a vector on the -x axis is -180, not +180).

export type ArcPt = readonly [number, number];

/** `ARC_GEOM_MANAGER::ARC_STEPS`. */
export const ARC_SET_ORIGIN = 0;
export const ARC_SET_START = 1;
export const ARC_SET_ANGLE = 2;
export const ARC_COMPLETE = 3;
export type ArcStep = 0 | 1 | 2 | 3;

export interface ArcGeom {
  /** `MULTISTEP_GEOM_MANAGER::m_step`: which point is being waited for. */
  step: ArcStep;
  /** `m_origin` -- the arc's centre. */
  origin: ArcPt;
  /** `m_radius`. */
  radius: number;
  /** `m_startAngle`, normalised to [0, 360). */
  startAngle: number;
  /** `m_endAngle`, normalised to [0, 360). */
  endAngle: number;
  /** `m_clockwise` -- starts true, exactly as the C++ member initialiser. */
  clockwise: boolean;
  /** `m_directionLocked`: set once the cursor is 90 degrees or more away from the start (or by the posture key); released again when the arc comes back under 90. */
  directionLocked: boolean;
  /** `m_angleSnap`: snap the start/end angle to 45 degrees (`SetAngleSnap`, driven by the angle-snap mode). */
  angleSnap: boolean;
  /** `MULTISTEP_GEOM_MANAGER::m_lastPoint` -- the last raw point added, locked in or not. */
  lastPoint: ArcPt;
}

export function newArcGeom(): ArcGeom {
  return { step: ARC_SET_ORIGIN, origin: [0, 0], radius: 0, startAngle: 0, endAngle: 0, clockwise: true, directionLocked: false, angleSnap: false, lastPoint: [0, 0] };
}

/** `KiROUND`: round half away from zero. */
function kiRound(v: number): number {
  const r = v < 0 ? -Math.round(-v) : Math.round(v);
  return r === 0 ? 0 : r; // never -0
}

/** `EDA_ANGLE( const VECTOR2D& )`, including its exact-axis/diagonal special cases (-180, never +180, on the negative x axis). */
export function angleOfVector(x: number, y: number): number {
  if (x === 0 && y === 0) return 0;
  if (y === 0) return x >= 0 ? 0 : -180;
  if (x === 0) return y >= 0 ? 90 : -90;
  if (x === y) return x >= 0 ? 45 : -180 + 45;
  if (x === -y) return x >= 0 ? -45 : 180 - 45;
  return (Math.atan2(y, x) * 180) / Math.PI;
}

/** `snapAngle`: `ANGLE_45 * KiROUND( aAngle / ANGLE_45 )`. */
function snapAngle(a: number): number {
  return 45 * kiRound(a / 45);
}

function normalize360(a: number): number {
  while (a < 0) a += 360;
  return a;
}

/** `ARC_GEOM_MANAGER::GetSubtended`, in degrees: `-( end - start [+360 if end <= start] [-360 if clockwise] )`. */
export function arcSubtended(g: ArcGeom): number {
  let angle = g.endAngle - g.startAngle;
  if (g.endAngle <= g.startAngle) angle += 360;
  if (g.clockwise) angle -= 360;
  return -angle;
}

/** `ARC_GEOM_MANAGER::GetStartAngle`, in degrees. */
export function arcStartAngle(g: ArcGeom): number {
  let angle = g.startAngle;
  if (g.clockwise) angle -= 360;
  return -angle;
}

function radialPoint(g: ArcGeom, angle: number): [number, number] {
  // `vec( radius, 0 ); RotatePoint( vec, -angle ); origin + vec` -- (RotatePoint( p, -a ) of (r, 0) is (r cos a, r sin a)).
  const rad = (angle * Math.PI) / 180;
  return [kiRound(g.origin[0] + g.radius * Math.cos(rad)), kiRound(g.origin[1] + g.radius * Math.sin(rad))];
}

/** `GetStartRadiusEnd`: the point on the start radius line. */
export function arcStartRadiusEnd(g: ArcGeom): [number, number] {
  return radialPoint(g, g.startAngle);
}

/** `GetEndRadiusEnd`: the point on the end radius line. */
export function arcEndRadiusEnd(g: ArcGeom): [number, number] {
  return radialPoint(g, g.endAngle);
}

/** `ARC_GEOM_MANAGER::setOrigin`. */
function setOrigin(g: ArcGeom, p: ArcPt): { geom: ArcGeom; accepted: boolean } {
  return { geom: { ...g, origin: p, startAngle: 0, endAngle: 0 }, accepted: true };
}

/** `ARC_GEOM_MANAGER::setStart`: the start point fixes the radius and the start angle. */
function setStart(g: ArcGeom, p: ArcPt): { geom: ArcGeom; accepted: boolean } {
  const rx = p[0] - g.origin[0];
  const ry = p[1] - g.origin[1];
  const radius = Math.hypot(rx, ry);
  let start = angleOfVector(rx, ry);
  if (g.angleSnap) start = snapAngle(start);
  start = normalize360(start);
  return { geom: { ...g, radius, startAngle: start, endAngle: start }, accepted: radius !== 0 };
}

/** `ARC_GEOM_MANAGER::setEnd`: the end angle, plus the automatic (shorter way round) direction until it locks. */
function setEnd(g: ArcGeom, p: ArcPt): { geom: ArcGeom; accepted: boolean } {
  const rx = p[0] - g.origin[0];
  const ry = p[1] - g.origin[1];
  let end = angleOfVector(rx, ry);
  if (g.angleSnap) end = snapAngle(end);
  end = normalize360(end);
  const next: ArcGeom = { ...g, endAngle: end };
  if (!next.directionLocked) {
    let ccw = next.endAngle - next.startAngle;
    if (next.endAngle <= next.startAngle) ccw += 360;
    const cw = Math.abs(ccw - 360);
    if (Math.min(ccw, cw) >= 90) next.directionLocked = true;
    else next.clockwise = cw < ccw;
  } else if (Math.abs(arcSubtended(next)) < 90) {
    next.directionLocked = false;
  }
  // "if the end is the same as the start, this is a bad point"
  return { geom: next, accepted: next.endAngle !== next.startAngle };
}

function acceptPoint(g: ArcGeom, p: ArcPt): { geom: ArcGeom; accepted: boolean } {
  switch (g.step) {
    case ARC_SET_ORIGIN:
      return setOrigin(g, p);
    case ARC_SET_START:
      return setStart(g, p);
    case ARC_SET_ANGLE:
      return setEnd(g, p);
    default:
      return { geom: g, accepted: false };
  }
}

/**
 * `MULTISTEP_GEOM_MANAGER::AddPoint( aPt, aLockIn )`: update the geometry with
 * `p`; with `lockIn` also advance (the point was accepted) or step back (it was
 * rejected -- a zero radius, or an end on the start radius) one step, clamped to
 * [0, COMPLETE].
 */
export function arcAddPoint(g: ArcGeom, p: ArcPt, lockIn: boolean): ArcGeom {
  const { geom, accepted } = acceptPoint({ ...g, lastPoint: p }, p);
  if (!lockIn) return geom;
  const step = Math.min(Math.max(geom.step + (accepted ? 1 : -1), 0), ARC_COMPLETE) as ArcStep;
  return { ...geom, step };
}

/** `MULTISTEP_GEOM_MANAGER::RemoveLastPoint`: step back, then reprocess the last point in that earlier step (Backspace -> `deleteLastPoint`). */
export function arcRemoveLastPoint(g: ArcGeom): ArcGeom {
  const stepped: ArcGeom = { ...g, step: Math.min(Math.max(g.step - 1, 0), ARC_COMPLETE) as ArcStep };
  return acceptPoint(stepped, g.lastPoint).geom;
}

/** `ARC_GEOM_MANAGER::ToggleClockwise` -- the `/` hotkey: reverse the direction and lock it. */
export function arcToggleClockwise(g: ArcGeom): ArcGeom {
  return { ...g, clockwise: !g.clockwise, directionLocked: true };
}

/** `ARC_GEOM_MANAGER::SetClockwise`. */
export function arcSetClockwise(g: ArcGeom, clockwise: boolean): ArcGeom {
  return { ...g, clockwise, directionLocked: true };
}

export function arcIsComplete(g: ArcGeom): boolean {
  return g.step === ARC_COMPLETE;
}

/**
 * The arc as it is stored (`updateArcFromConstructionMgr`): its own start angle, end
 * angle and positive sweep, in the direction of increasing `atan2` angle. When
 * `GetSubtended() < 0` the stored start is the start radius end; otherwise the two
 * ends are swapped. `null` until a radius and a non-zero subtended angle exist.
 */
export function arcSweep(g: ArcGeom): { startAngle: number; endAngle: number; sweep: number } | null {
  if (g.step < ARC_SET_ANGLE || !(g.radius > 0)) return null;
  const sub = arcSubtended(g);
  if (sub === 0) return null;
  const swapped = sub >= 0; // start = the END radius end
  return { startAngle: swapped ? g.endAngle : g.startAngle, endAngle: swapped ? g.startAngle : g.endAngle, sweep: Math.abs(sub) };
}

/**
 * A left click while "Draw Arc" is armed: `arcManager.AddPoint( cursorPos, true )`
 * (`drawArc`'s `IsClick( BUT_LEFT )` branch). `geom` is the tool state to keep
 * (`null` once the manager is back at its first step, i.e. nothing is in
 * progress); `arc` is set when this click completed the arc, ready to commit.
 */
export function arcClick(prev: ArcGeom | null | undefined, p: ArcPt, angleSnap: boolean): { geom: ArcGeom | null; arc: ThreePointArc | null } {
  const next = arcAddPoint({ ...(prev ?? newArcGeom()), angleSnap }, p, true);
  if (arcIsComplete(next)) return { geom: null, arc: arcToThreePoints(next) };
  return { geom: next.step === ARC_SET_ORIGIN ? null : next, arc: null };
}

/** Cursor motion: `arcManager.SetAngleSnap( angleSnap ); arcManager.AddPoint( cursorPos, false )` -- update the geometry, never the step. */
export function arcMotion(prev: ArcGeom, p: ArcPt, angleSnap: boolean): ArcGeom {
  return arcAddPoint({ ...prev, angleSnap }, p, false);
}

export interface ThreePointArc {
  center: [number, number];
  radius: number;
  start: [number, number];
  mid: [number, number];
  end: [number, number];
  /** The arc's own sweep, degrees (always positive), from `start` to `end` in the direction of increasing `atan2` angle -- `EDA_SHAPE::GetArcAngle`. */
  sweepDeg: number;
}

/**
 * `updateArcFromConstructionMgr` (drawing_tool.cpp) + `EDA_SHAPE::GetArcMid`: the
 * arc the manager currently describes as the IR's `start`/`mid`/`end` triple. When
 * `GetSubtended() < 0` the arc runs from the start radius end to the end radius
 * end; otherwise the two are swapped -- an arc always runs from its `start` to its
 * `end` in the direction of increasing angle, which is what `mid` (the point half
 * way round that sweep) records. `null` until a radius and a distinct end angle exist.
 */
export function arcToThreePoints(g: ArcGeom): ThreePointArc | null {
  const s = arcSweep(g);
  if (!s) return null;
  const { startAngle, endAngle, sweep } = s;
  const mid = radialPoint(g, startAngle + sweep / 2);
  return {
    center: [g.origin[0], g.origin[1]],
    radius: g.radius,
    start: radialPoint(g, startAngle),
    mid,
    end: radialPoint(g, endAngle),
    sweepDeg: sweep,
  };
}
