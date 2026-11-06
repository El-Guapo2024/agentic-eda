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
//
// KiCad 25ec2335's other four draw modes (`ARC_DRAW_MODE`, include/tool/arc_draw_mode.h; common/preview_items/arc_geom_manager.cpp `acceptExactPoint`,
// `applySolution`, `setDirection`, `SetMode`, `SetTangentSeed`, `ToggleClockwise`, `Reset`; MULTISTEP_GEOM_MANAGER's `rejectRegresses`) ride on the same state as
// optional fields: start-end-mid, start-end-centre, tangent and start-direction-end keep the clicked points exactly (`m_pts`) and describe the arc by the solvers'
// start/mid/end (kicad-port/arcConstruction.ts); a refused click there is ignored instead of stepping back. A state without `mode` is the centre mode, as the
// board's and the Footprint Editor's tools (8303b2ad's manager) keep it. UI-LEFT-2 SS3.

import { arcFromStartEndCenterDrag, arcFromStartEndMidDrag, arcFromStartTangentEnd, INVALID_SOLUTION, type ArcSolution } from "./arcConstruction";

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
  /** KiCad 25ec2335's `m_mode`; absent is `center_start_end`. */
  mode?: ArcDrawMode;
  /** The exact modes' `m_pts`: the points set so far, the last following the cursor. */
  pts?: [ArcPt, ArcPt, ArcPt];
  /** `m_dir`: the departure direction (START_DIR_END's second click, or the tangent seed's). */
  dir?: [number, number];
  /** `m_hasSeed`, `m_seedIsAxis`, `m_axisFlipped`, `m_axis`: TANGENT's seed. */
  hasSeed?: boolean;
  seedIsAxis?: boolean;
  axisFlipped?: boolean;
  axis?: [number, number];
  /** `m_major`: START_END_CENTER's major arc (`/` toggles it). */
  major?: boolean;
  /** `m_solution`: the arc the exact modes describe (it keeps the last valid one while the cursor is on a degenerate place). */
  solution?: ArcSolution;
}

/** KiCad 25ec2335's `ARC_DRAW_MODE`. */
export type ArcDrawMode = "center_start_end" | "start_end_mid" | "start_end_center" | "tangent" | "start_dir_end";

export const ARC_DRAW_MODES: readonly ArcDrawMode[] = ["center_start_end", "start_end_mid", "start_end_center", "tangent", "start_dir_end"];

/** `IncrementArcDrawMode`: the cycle order of `cycleArcDrawMode`. */
export function incrementArcDrawMode(m: ArcDrawMode): ArcDrawMode {
  return ARC_DRAW_MODES[(ARC_DRAW_MODES.indexOf(m) + 1) % ARC_DRAW_MODES.length]!;
}

const modeOf = (g: ArcGeom): ArcDrawMode => g.mode ?? "center_start_end";
/** `UsesExactEndpoints`: every mode but the centre one. */
export const arcUsesExactEndpoints = (g: ArcGeom): boolean => modeOf(g) !== "center_start_end";
/** `HasTangentSeed`. */
export const arcHasTangentSeed = (g: ArcGeom): boolean => modeOf(g) === "tangent" && !!g.hasSeed;
/** `UsesDirectionClick`: START_DIR_END, or a TANGENT without a seed. */
export const arcUsesDirectionClick = (g: ArcGeom): boolean => modeOf(g) === "start_dir_end" || (modeOf(g) === "tangent" && !g.hasSeed);
/** `getMaxStep`: a seeded tangent arc is complete after one click. */
const maxStep = (g: ArcGeom): ArcStep => (arcHasTangentSeed(g) ? ARC_SET_START : ARC_COMPLETE);
/** `GetPointIndex`: the point the next click sets, a seed counting as point 0. */
export const arcPointIndex = (g: ArcGeom): number => g.step + (arcHasTangentSeed(g) ? 1 : 0);
/** `HasPreview`: something to draw, which a seeded tangent arc has before any click. */
export const arcHasPreview = (g: ArcGeom): boolean => g.step !== ARC_SET_ORIGIN || arcHasTangentSeed(g);

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

/** `clearSolution`: nothing drawable. */
function clearSolution(g: ArcGeom): ArcGeom {
  return { ...g, solution: INVALID_SOLUTION, origin: [0, 0], radius: 0, startAngle: 0, endAngle: 0, clockwise: true };
}

/** `applySolution`: the solution as the canonical arc state (the centre, radius and angles the assistant reads). */
function applySolution(g: ArcGeom, sol: ArcSolution): ArcGeom {
  const sx = sol.start[0] - sol.center[0];
  const sy = sol.start[1] - sol.center[1];
  const ex = sol.end[0] - sol.center[0];
  const ey = sol.end[1] - sol.center[1];
  const mx = sol.mid[0] - sol.center[0];
  const my = sol.mid[1] - sol.center[1];
  // "Raw angles grow toward the start to mid side unless the cross product is negative"
  return { ...g, solution: sol, origin: [kiRound(sol.center[0]), kiRound(sol.center[1])], radius: sol.radius, startAngle: normalize360(angleOfVector(sx, sy)) % 360, endAngle: normalize360(angleOfVector(ex, ey)) % 360, clockwise: sx * my - sy * mx < 0 };
}

/** `setDirection`: the departure direction from the second click, snapped to the 45-degree units when asked. */
function setDirection(g: ArcGeom, p: ArcPt): { geom: ArcGeom; accepted: boolean } {
  const s = g.pts![0];
  const vx = p[0] - s[0];
  const vy = p[1] - s[1];
  const length = Math.hypot(vx, vy);
  if (length === 0) return { geom: g, accepted: false };
  let dir: [number, number] = [vx, vy];
  if (g.angleSnap) {
    // "Table lookup keeps snapped axis directions free of trigonometric rounding"
    const d = Math.SQRT1_2;
    const units: [number, number][] = [[1, 0], [d, d], [0, 1], [-d, d], [-1, 0], [-d, -d], [0, -1], [d, -d]];
    const octant = ((kiRound(angleOfVector(vx, vy) / 45) % 8) + 8) % 8;
    dir = [units[octant]![0] * length, units[octant]![1] * length];
  }
  return { geom: { ...g, dir }, accepted: true };
}

/** `acceptExactPoint`: the point every mode but the centre one sets -- a guide while points are missing, else the arc the mode's solver gives. */
function acceptExactPoint(g0: ArcGeom, p: ArcPt): { geom: ArcGeom; accepted: boolean } {
  if (g0.step === maxStep(g0)) return { geom: g0, accepted: false };
  const index = arcPointIndex(g0);
  const pts = [...(g0.pts ?? [[0, 0], [0, 0], [0, 0]])] as [ArcPt, ArcPt, ArcPt];
  pts[index] = p;
  let g: ArcGeom = { ...g0, pts };
  // "The points before the last only draw guides"
  if (index < 2 && !arcHasTangentSeed(g)) {
    g = clearSolution(g);
    if (index === 0) return { geom: g, accepted: true };
    return arcUsesDirectionClick(g) ? setDirection(g, p) : { geom: g, accepted: p[0] !== pts[0][0] || p[1] !== pts[0][1] };
  }
  if (arcHasTangentSeed(g) && g.seedIsAxis && g.axis) {
    // "The nearer of the two directions along the axis gives the sweep up to a half turn"
    const dot = (p[0] - pts[0][0]) * g.axis[0] + (p[1] - pts[0][1]) * g.axis[1];
    const sign = dot >= 0 !== !!g.axisFlipped ? 1 : -1;
    g = { ...g, dir: [g.axis[0] * sign, g.axis[1] * sign] };
  }
  let sol: ArcSolution = INVALID_SOLUTION;
  switch (modeOf(g)) {
    case "start_end_mid":
      sol = arcFromStartEndMidDrag(pts[0], pts[1], p);
      break;
    case "start_end_center":
      sol = arcFromStartEndCenterDrag(pts[0], pts[1], p, !!g.major);
      break;
    case "tangent":
    case "start_dir_end":
      sol = arcFromStartTangentEnd(pts[0], g.dir ?? [0, 0], p);
      break;
    default:
      break;
  }
  return sol.valid ? { geom: applySolution(g, sol), accepted: true } : { geom: g, accepted: false };
}

function acceptPoint(g: ArcGeom, p: ArcPt): { geom: ArcGeom; accepted: boolean } {
  if (arcUsesExactEndpoints(g)) return acceptExactPoint(g, p);
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
  // `if( aLockIn && ( accepted || rejectRegresses() ) )`: only the centre mode steps back on a refused click
  if (!lockIn || !(accepted || !arcUsesExactEndpoints(g))) return geom;
  const step = Math.min(Math.max(geom.step + (accepted ? 1 : -1), 0), maxStep(geom)) as ArcStep;
  return { ...geom, step };
}

/** `MULTISTEP_GEOM_MANAGER::RemoveLastPoint`: step back, then reprocess the last point in that earlier step (Backspace -> `deleteLastPoint`). */
export function arcRemoveLastPoint(g: ArcGeom): ArcGeom {
  const stepped: ArcGeom = { ...g, step: Math.min(Math.max(g.step - 1, 0), maxStep(g)) as ArcStep };
  return acceptPoint(stepped, g.lastPoint).geom;
}

/**
 * `ARC_GEOM_MANAGER::ToggleClockwise` -- the `/` hotkey: in the centre mode reverse the direction and lock it; in START_END_CENTER the minor and the major arc swap (the
 * arc under the cursor rebuilt); a tangent arc seeded by an axis takes the other way along it. The other modes' points fix the direction.
 */
export function arcToggleClockwise(g: ArcGeom): ArcGeom {
  switch (modeOf(g)) {
    case "center_start_end":
      return { ...g, clockwise: !g.clockwise, directionLocked: true };
    case "start_end_center": {
      const next = { ...g, major: !g.major };
      return g.step === ARC_SET_ANGLE ? acceptPoint(next, g.lastPoint).geom : next;
    }
    case "tangent":
      return arcHasTangentSeed(g) && g.seedIsAxis ? acceptPoint({ ...g, axisFlipped: !g.axisFlipped }, g.lastPoint).geom : g;
    default:
      return g;
  }
}

/** `ARC_GEOM_MANAGER::Reset`: ready for the next arc of a chain, in the same mode; the seed goes too. */
export function arcReset(g: ArcGeom): ArcGeom {
  return g.mode && g.mode !== "center_start_end" ? { ...newArcGeom(), mode: g.mode } : newArcGeom();
}

/** A new construction in `mode` (`StartArcDrawMode` -> `SetMode` of a manager that is reset). */
export function newArcGeomIn(mode: ArcDrawMode): ArcGeom {
  return mode === "center_start_end" ? newArcGeom() : { ...newArcGeom(), mode, solution: INVALID_SOLUTION };
}

/**
 * `ARC_GEOM_MANAGER::SetMode`: with nothing locked the mode just changes (a seed set beforehand stays); else the construction restarts, keeping the first point when it
 * is an arc's start in both modes (every mode but the centre one), with the cursor where it was.
 */
export function arcSetMode(g: ArcGeom, mode: ArcDrawMode): ArcGeom {
  if (mode === modeOf(g)) return g;
  if (g.step === ARC_SET_ORIGIN) return clearSolution({ ...g, mode });
  const keepStart = modeOf(g) !== "center_start_end" && mode !== "center_start_end";
  const start = g.pts?.[0] ?? [0, 0];
  const cursor = g.lastPoint;
  const fresh = { ...newArcGeomIn(mode), angleSnap: g.angleSnap };
  if (!keepStart) return newArcGeomIn(mode);
  return arcAddPoint(arcAddPoint(fresh, start, true), cursor, false);
}

/** `SetTangentSeed`: the start and departure direction of a TANGENT arc, before its first click; `directionIsAxis` when only the line is known. */
export function arcSetTangentSeed(g: ArcGeom, start: ArcPt, direction: readonly [number, number], directionIsAxis = false): ArcGeom {
  if (g.step !== ARC_SET_ORIGIN || (direction[0] === 0 && direction[1] === 0)) return g;
  const pts: [ArcPt, ArcPt, ArcPt] = [start, start, start];
  return clearSolution({ ...g, pts, dir: [direction[0], direction[1]], axis: [direction[0], direction[1]], seedIsAxis: directionIsAxis, axisFlipped: false, hasSeed: true });
}

/** `ARC_GEOM_MANAGER::SetClockwise`. */
export function arcSetClockwise(g: ArcGeom, clockwise: boolean): ArcGeom {
  return { ...g, clockwise, directionLocked: true };
}

export function arcIsComplete(g: ArcGeom): boolean {
  return g.step === maxStep(g);
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
  // nothing in progress once the manager is back at its first step -- unless a tangent seed waits for the click that ends the arc
  return { geom: next.step === ARC_SET_ORIGIN && !arcHasTangentSeed(next) ? null : next, arc: null };
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
  if (arcUsesExactEndpoints(g)) {
    // `ApplyToShape`: "Winding follows the midpoint, and the clicked endpoints are kept exactly" (`SetArcGeometry( start, mid, end )`)
    const sol = g.solution;
    if (!sol?.valid) return null;
    const c: [number, number] = [sol.center[0], sol.center[1]];
    const a = (q: readonly [number, number]) => Math.atan2(q[1] - c[1], q[0] - c[0]);
    const norm = (x: number) => ((x % 360) + 360) % 360;
    const half = norm(((a(sol.mid) - a(sol.start)) * 180) / Math.PI);
    return { center: c, radius: sol.radius, start: [sol.start[0], sol.start[1]], mid: [sol.mid[0], sol.mid[1]], end: [sol.end[0], sol.end[1]], sweepDeg: Math.min(360, 2 * Math.min(half, 360 - half)) };
  }
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
