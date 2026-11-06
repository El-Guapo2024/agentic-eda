// Hop-overs: where two wires cross without joining, one of them jumps the
// other in a small half-circle. Ported from KiCad 25ec2335 (the feature is
// KiCad 10's, newer than the default source):
//   * `SCH_LINE::ShouldHopOver` (eeschema/sch_line.cpp): which of two
//     crossing lines hops -- a horizontal one over a vertical one, else the
//     shallower slope.
//   * `SCH_LINE::BuildWireWithHopShape`: a wire as its straight pieces and
//     its arcs, one hop at each crossing that is no line's end.
//   * `SEG::Intersect( aSeg, true, false )` with `SEG::intersects` and
//     `checkCollinearOverlap` (libs/kimath/src/geometry/seg.cpp), and
//     `rescale` (libs/kimath/src/math/util.cpp): the crossing point.
//   * `SCHEMATIC_SETTINGS::GetHopOverScale` with `hopover_size_mult_list`
//     (eeschema/schematic_settings.cpp): the arc's radius is the default
//     line width (Schematic Setup > Formatting, `m_DefaultLineWidth`) times
//     the "Hop-over size:" choice's multiplier, as `SCH_PAINTER::draw(
//     const SCH_LINE* )` and `SCH_SCREEN::Plot` compute it
//     (`arcRadius = defaultLineWidth * hopOverScale`). It is not the wire's
//     own width: a thick wire hops in the same arc as a thin one.
//   * `SCH_PAINTER::draw( const SCH_LINE* )`'s stroke of the pieces: each
//     straight piece in the line's style, each arc solid ("Hop are a small
//     arc, so use a solid line style gives best results"), the arc's centre
//     by `CalcArcCenter( VECTOR2D, VECTOR2D, VECTOR2D )`
//     (libs/kimath/src/trigo.cpp) and its sweep as the two half angles
//     `Normalize180()`d.
//
// Units: the geometry runs in KiCad's schematic internal unit (100 nm,
// `schIUScale`), on integers, so the crossing, the rounding of the hop's
// ends (`KiROUND`) and of its top (`static_cast<int>`) are kicad-cli's to
// the unit: `sch export svg --draw-hop-over` ends the wire's straight
// pieces at the same points, and its arcs within a unit of them (the SVG
// plotter writes an arc's ends again from its centre and angles). The
// numbers are pinned in `schHopOver.kicadcli.json`, which this module's
// test and crates/cli `sch_setup_tests.rs`' slow kicad-cli proof both
// read. The studio's nanometres are read with `KiROUND( nm / 100 )`, as
// KiCad reads the derived file's millimetres.
//
// Differences from the C++:
//   * The lines a wire is tested against are the sheet's wires and buses in
//     the sheet's order (KiCad: the screen's R-tree, `Items().Overlapping(
//     SCH_LINE_T, GetBoundingBox() )`, whose order only matters for KiCad's
//     own de-duplication of a point three lines cross, which keeps the
//     point once only when it comes twice in a row).
//   * `det2`'s fused multiply-adds are plain products (JS has no fma); the
//     centre is the canvas's, not a plotted value.
//
// Pure: no React or DOM, so `npm run test:unit` compiles it.

import { angleOfVector } from "./arcGeom";
import { kiRound } from "./pcbGeom";

/** `[x, y]` in schematic internal units (100 nm). */
export type Pt = [number, number];

/** One `SCH_LINE`: a wire's or a bus's segment, in internal units. */
export interface SchSeg {
  start: Pt;
  end: Pt;
}

/** `BuildWireWithHopShape`'s list, read as pieces: a straight piece (Z = 0: two points) or a hop's arc (Z = 1: start, middle, end). */
export type WirePiece = { arc: false; start: Pt; end: Pt } | { arc: true; start: Pt; mid: Pt; end: Pt };

/** `schIUScale`: the schematic's internal unit is 100 nm. */
export const SCH_NM_PER_IU = 100;

/** `hopover_size_mult_list` by `m_HopOverSizeChoice`: None, Smallest, Small, Medium, Large, Largest. */
export const HOPOVER_SIZE_MULT: readonly number[] = [0.0, 1.7, 4.0, 6.0, 9.0, 12.0];

/** `DEFAULT_LINE_WIDTH_MILS` (6) in nm: `m_DefaultLineWidth`'s default. */
const DEFAULT_LINE_WIDTH_NM = 152_400;

/** `SCHEMATIC_SETTINGS::GetHopOverScale`. A choice outside the six is None (`PARAM<int>( .., 0, 5 )` reads its default). */
export function getHopOverScale(choice: number): number {
  return Number.isInteger(choice) ? (HOPOVER_SIZE_MULT[choice] ?? 0) : 0;
}

/** Nanometres to internal units, as KiCad reads a coordinate (`KiROUND`). */
export function nmToIu(nm: number): number {
  return kiRound(nm / SCH_NM_PER_IU);
}

/** The hop's radius in internal units: `defaultLineWidth * hopOverScale`, `m_DefaultLineWidth` an integer of internal units
 *  (`PARAM_SCALED<int>`'s `KiROUND`) and the product kept as the double KiCad keeps (`double arcRadius`). */
export function hopOverArcRadius(defaultLineWidthNm: number | null | undefined, choice: number): number {
  return nmToIu(defaultLineWidthNm || DEFAULT_LINE_WIDTH_NM) * getHopOverScale(choice);
}

const eq = (a: Pt, b: Pt): boolean => a[0] === b[0] && a[1] === b[1];

/** `SCH_LINE::IsEndPoint`. */
const isEndPoint = (l: SchSeg, p: Pt): boolean => eq(p, l.start) || eq(p, l.end);

/** `SCH_LINE::ShouldHopOver( aLine )`: whether `me` hops over `other` where they cross. "Horizontal wires have preference for hop": a vertical
 *  line never hops a slanted or horizontal one, which always hops a vertical one; otherwise the shallower line hops, and of two of the same
 *  steepness (45 degree wires) the one of the smaller slope. The slopes are the C++'s doubles, so a vertical pair compares infinities. */
export function shouldHopOver(me: SchSeg, other: SchSeg): boolean {
  const isMeVertical = me.end[0] === me.start[0];
  const isCandidateVertical = other.end[0] === other.start[0];
  if (isMeVertical && !isCandidateVertical) return false;
  if (isCandidateVertical && !isMeVertical) return true;
  const slopeMe = (me.end[1] - me.start[1]) / (me.end[0] - me.start[0]);
  const slopeCandidate = (other.end[1] - other.start[1]) / (other.end[0] - other.start[0]);
  if (Math.abs(slopeMe) === Math.abs(slopeCandidate)) return slopeMe < slopeCandidate;
  return Math.abs(slopeMe) < Math.abs(slopeCandidate);
}

/** `rescale<int64_t>( numerator, value, denominator )`: `numerator * value / denominator` rounded to nearest, on the 128-bit product
 *  (`__int128_t`), the divisions truncating as C++'s do. BigInt keeps the product exact past 2^53. */
function rescale(numerator: number, value: number, denominator: number): number {
  const n = BigInt(numerator) * BigInt(value);
  const d = BigInt(denominator);
  const half = d / 2n;
  return Number(n < 0n !== d < 0n ? (n - half) / d : (n + half) / d);
}

const cross = (ax: number, ay: number, bx: number, by: number): number => ax * by - ay * bx;

/** `SEG::checkCollinearOverlap`: two collinear segments meet at the middle of their overlap on the axis of `a`'s larger extent; touching
 *  end to end only is no crossing with `ignoreEndpoints`. */
function collinearOverlap(a: SchSeg, b: SchSeg, useXAxis: boolean, ignoreEndpoints: boolean): Pt | null {
  const k = useXAxis ? 0 : 1;
  const o = useXAxis ? 1 : 0;
  const seg1Start = a.start[k];
  const seg1End = a.end[k];
  const coord1Start = a.start[o];
  const coord1End = a.end[o];
  const seg1Min = Math.min(seg1Start, seg1End);
  const seg1Max = Math.max(seg1Start, seg1End);
  const seg2Min = Math.min(b.start[k], b.end[k]);
  const seg2Max = Math.max(b.start[k], b.end[k]);
  if (!(seg1Max >= seg2Min && seg2Max >= seg1Min)) return null;
  const overlapStart = Math.max(seg1Min, seg2Min);
  const overlapEnd = Math.min(seg1Max, seg2Max);
  if (ignoreEndpoints && overlapStart === overlapEnd) {
    if ((overlapStart === seg1Min || overlapStart === seg1Max) && (overlapStart === seg2Min || overlapStart === seg2Max)) return null;
  }
  const proj = Math.trunc((overlapStart + overlapEnd) / 2);
  const other = seg1End !== seg1Start ? coord1Start + rescale(proj - seg1Start, coord1End - coord1Start, seg1End - seg1Start) : coord1Start;
  return useXAxis ? [proj, other] : [other, proj];
}

/** `SEG::Intersect( aSeg, aIgnoreEndpoints, false )` (`SEG::intersects`): where segments `a` and `b` meet, or null. The point is `b.start +
 *  (q / d) * (b.end - b.start)` rounded (`rescale`); with `ignoreEndpoints`, segments that only share an end do not meet. */
export function segIntersect(a: SchSeg, b: SchSeg, ignoreEndpoints: boolean): Pt | null {
  const [ax, ay] = a.start;
  const [bx, by] = a.end;
  const [cx, cy] = b.start;
  const [dx, dy] = b.end;
  if (Math.max(ax, bx) < Math.min(cx, dx) || Math.max(cx, dx) < Math.min(ax, bx) || Math.max(ay, by) < Math.min(cy, dy) || Math.max(cy, dy) < Math.min(ay, by)) return null;
  const dir1x = bx - ax;
  const dir1y = by - ay;
  const dir2x = dx - cx;
  const dir2y = dy - cy;
  const offx = cx - ax;
  const offy = cy - ay;
  const determinant = cross(dir2x, dir2y, dir1x, dir1y);
  if (determinant === 0) {
    if (cross(dir1x, dir1y, offx, offy) !== 0) return null;
    return collinearOverlap(a, b, Math.abs(dir1x) >= Math.abs(dir1y), ignoreEndpoints);
  }
  const param2 = cross(dir2x, dir2y, offx, offy);
  const param1 = cross(dir1x, dir1y, offx, offy);
  if (determinant > 0) {
    if (param1 < 0 || param1 > determinant || param2 < 0 || param2 > determinant) return null;
  } else if (param1 > 0 || param1 < determinant || param2 > 0 || param2 < determinant) {
    return null;
  }
  if (ignoreEndpoints && (param1 === 0 || param1 === determinant) && (param2 === 0 || param2 === determinant)) return null;
  return [cx + rescale(param1, dir2x, determinant), cy + rescale(param1, dir2y, determinant)];
}

/** `SCH_LINE::BuildWireWithHopShape( aScreen, aArcRadius )` of a wire or bus segment `line` among the sheet's wire and bus segments `screen`:
 *  every line `line` hops over (`ShouldHopOver`) that crosses it at a point that is neither line's end gives a hop there; the hops are taken
 *  from the line's start, and the line is its straight pieces between them and one arc per hop. An arc starts and ends `R` along the line
 *  each side of the crossing (`KiROUND`) and passes `R` above it, square to the line's direction folded to [0, pi) so the side does not depend
 *  on which end is the start (truncated, `static_cast<int>`): up for a horizontal wire. A line crossing nothing is the one piece. */
export function buildWireWithHopShape(line: SchSeg, screen: readonly SchSeg[], arcRadius: number): WirePiece[] {
  const intersections: Pt[] = [];
  for (const existing of screen) {
    if (eq(existing.start, line.start) && eq(existing.end, line.end)) continue;
    if (!shouldHopOver(line, existing)) continue;
    const at = segIntersect(line, existing, true);
    if (!at) continue;
    if (isEndPoint(line, at) || isEndPoint(existing, at)) continue;
    // "Ensure intersecting point is not yet entered": more than two wires crossing at one point
    const last = intersections[intersections.length - 1];
    if (!last || !eq(last, at)) intersections.push(at);
  }
  if (intersections.length === 0) return [{ arc: false, start: line.start, end: line.end }];
  const dist = (p: Pt): number => Math.sqrt((line.start[0] - p[0]) ** 2 + (line.start[1] - p[1]) ** 2);
  intersections.sort((a, b) => dist(a) - dist(b));
  const pieces: WirePiece[] = [];
  let currentStart = line.start;
  const R = arcRadius;
  const lineAngle = Math.atan2(line.end[1] - line.start[1], line.end[0] - line.start[0]);
  let arcAngle = lineAngle;
  if (arcAngle < 0) arcAngle += Math.PI;
  else if (arcAngle >= Math.PI) arcAngle -= Math.PI;
  const along: Pt = [kiRound(R * Math.cos(lineAngle)), kiRound(R * Math.sin(lineAngle))];
  for (const hopMid of intersections) {
    const mid: Pt = [hopMid[0] + Math.trunc(R * Math.sin(arcAngle)), hopMid[1] - Math.trunc(R * Math.cos(arcAngle))];
    const beforeHop: Pt = [hopMid[0] - along[0], hopMid[1] - along[1]];
    const afterHop: Pt = [hopMid[0] + along[0], hopMid[1] + along[1]];
    pieces.push({ arc: false, start: currentStart, end: beforeHop });
    pieces.push({ arc: true, start: beforeHop, mid, end: afterHop });
    currentStart = afterHop;
  }
  pieces.push({ arc: false, start: currentStart, end: line.end });
  return pieces;
}

/** A wire as the painter has it: its polyline in nm (each segment one `SCH_LINE` of the derived file) and whether it is a bus (a bus hops and
 *  is hopped as a wire is: `IsWire() || IsBus()` both ways). */
export interface HopWire {
  id: string;
  pts: ReadonlyArray<readonly [number, number]>;
  bus?: boolean;
}

const cache = new WeakMap<readonly HopWire[], { radius: number; shapes: Map<string, WirePiece[]> }>();

/** The sheet's wires that hop, by id: each one's pieces (in internal units, its segments in order), for the hop-over choice and default line
 *  width of Schematic Setup > Formatting. Only `SCH_PAINTER`'s `hopOverScale > 0.0` builds hops; a wire that crosses nothing it hops over is
 *  not in the map (it is drawn as before). Every wire and bus segment of the sheet is a line a wire may hop (`IsWire() || IsBus()`). Kept per
 *  `wires` array, which the studio replaces, never edits, when the sheet changes. */
export function wireHopShapes(wires: readonly HopWire[], choice: number | null | undefined, defaultLineWidthNm: number | null | undefined): Map<string, WirePiece[]> {
  const radius = hopOverArcRadius(defaultLineWidthNm, choice ?? 0);
  if (!(radius > 0)) return new Map();
  const hit = cache.get(wires);
  if (hit && hit.radius === radius) return hit.shapes;
  const segsOf = (w: HopWire): SchSeg[] => w.pts.slice(1).map((p, i) => ({ start: [nmToIu(w.pts[i]![0]), nmToIu(w.pts[i]![1])], end: [nmToIu(p[0]), nmToIu(p[1])] }));
  const bySeg = wires.map(segsOf);
  const screen = bySeg.flat();
  const shapes = new Map<string, WirePiece[]>();
  wires.forEach((w, i) => {
    const pieces = bySeg[i]!.flatMap((s) => buildWireWithHopShape(s, screen, radius));
    if (pieces.some((p) => p.arc)) shapes.set(w.id, pieces);
  });
  cache.set(wires, { radius, shapes });
  return shapes;
}

// ---- CalcArcCenter( const VECTOR2D&, const VECTOR2D&, const VECTOR2D& ), libs/kimath/src/trigo.cpp

const K_CLUSTER_EXTENT = 5.0;
const K_COINCIDENT_RADIUS_SQUARED = 2.0 * 2.0;
const K_SNAP_RADIUS_TOLERANCE = 1.0;
const K_COLLINEAR_RADIUS = 1e17;
const K_MAX_EXACT_INTEGER = 9007199254740992.0;

/** `CalcArcCenter` on doubles: the clustered and coincident cases, the circumcentre relative to the start, and `snapArcCenter`'s 100 / 10
 *  unit centre when it lies on the circle to within a unit. */
export function calcArcCenter(s: Pt, m: Pt, e: Pt): Pt {
  const minX = Math.min(s[0], m[0], e[0]);
  const maxX = Math.max(s[0], m[0], e[0]);
  const minY = Math.min(s[1], m[1], e[1]);
  const maxY = Math.max(s[1], m[1], e[1]);
  if (maxX - minX < K_CLUSTER_EXTENT && maxY - minY < K_CLUSTER_EXTENT) return [(s[0] + m[0] + e[0]) / 3, (s[1] + m[1] + e[1]) / 3];
  const coincident = (a: Pt, b: Pt): boolean => (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 < K_COINCIDENT_RADIUS_SQUARED;
  if (coincident(s, m) || coincident(m, e)) return [(s[0] + e[0]) / 2, (s[1] + e[1]) / 2];
  if (coincident(s, e)) return [(s[0] + m[0]) / 2, (s[1] + m[1]) / 2];
  const bx = m[0] - s[0];
  const by = m[1] - s[1];
  const cx = e[0] - s[0];
  const cy = e[1] - s[1];
  const b2 = bx * bx + by * by;
  const c2 = cx * cx + cy * cy;
  const d = 2.0 * (bx * cy - by * cx);
  if (d === 0) {
    // collinearArcCenter: far off square to the chord
    const chord: Pt = [e[0] - s[0], e[1] - s[1]];
    const len = Math.hypot(chord[0], chord[1]) || 1;
    return [(s[0] + e[0]) / 2 + (chord[1] / len) * K_COLLINEAR_RADIUS, (s[1] + e[1]) / 2 - (chord[0] / len) * K_COLLINEAR_RADIUS];
  }
  const centre: Pt = [s[0] + (b2 * cy - c2 * by) / d, s[1] + (c2 * bx - b2 * cx) / d];
  if (!(Math.abs(centre[0]) < K_MAX_EXACT_INTEGER) || !(Math.abs(centre[1]) < K_MAX_EXACT_INTEGER)) return centre;
  const radiiAgree = (c: Pt): boolean => {
    const r = [s, m, e].map((p) => Math.hypot(c[0] - p[0], c[1] - p[1]));
    return Math.max(...r) - Math.min(...r) <= K_SNAP_RADIUS_TOLERANCE;
  };
  for (const grid of [100.0, 10.0]) {
    const candidate: Pt = [Math.floor(centre[0] / grid + 0.5) * grid, Math.floor(centre[1] / grid + 0.5) * grid];
    if (radiiAgree(candidate)) return candidate;
  }
  return centre;
}

/** `EDA_ANGLE::Normalize180` in degrees: (-180, 180]. */
function normalize180(a: number): number {
  let v = a % 720;
  while (v <= -180) v += 360;
  while (v > 180) v -= 360;
  return v;
}

/** How `SCH_PAINTER::draw( const SCH_LINE* )` draws a hop's arc (`m_gal->DrawArc( center, radius, startAngle, angle )`): the centre
 *  (`CalcArcCenter`), the radius to the start, the start angle (`EDA_ANGLE( start - center )`) and the sweep, the start-to-middle and
 *  middle-to-end angles each `Normalize180()`d and added; degrees, y down. */
export function hopArcDraw(a: { start: Pt; mid: Pt; end: Pt }): { center: Pt; radius: number; startAngle: number; sweep: number } {
  const center = calcArcCenter(a.start, a.mid, a.end);
  const ang = (p: Pt): number => angleOfVector(p[0] - center[0], p[1] - center[1]);
  const startAngle = ang(a.start);
  const sweep = normalize180(ang(a.mid) - startAngle) + normalize180(ang(a.end) - ang(a.mid));
  return { center, radius: Math.hypot(a.start[0] - center[0], a.start[1] - center[1]), startAngle, sweep };
}

/** The canvas calls a wire's pieces take. */
export interface WireShapeCanvas {
  setLineDash(segments: number[]): void;
  beginPath(): void;
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  arc(x: number, y: number, radius: number, startAngle: number, endAngle: number, counterclockwise?: boolean): void;
  stroke(): void;
}

/** Strokes a hopping wire's pieces in nm, in the stroke style the caller set: the straight pieces in `dash` (each piece starts its own
 *  dashes, as each is its own `drawLine`), then the arcs solid. Leaves the dash solid, as the painter's wire loop does. */
export function strokeWireShape(ctx: WireShapeCanvas, pieces: readonly WirePiece[], dash: number[]): void {
  const k = SCH_NM_PER_IU;
  ctx.setLineDash(dash);
  ctx.beginPath();
  for (const p of pieces) {
    if (p.arc) continue;
    ctx.moveTo(p.start[0] * k, p.start[1] * k);
    ctx.lineTo(p.end[0] * k, p.end[1] * k);
  }
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.beginPath();
  for (const p of pieces) {
    if (!p.arc) continue;
    const { center, radius, startAngle, sweep } = hopArcDraw(p);
    const a0 = (startAngle * Math.PI) / 180;
    // a subpath of its own: the arc's own start, so no line joins it to the arc before
    ctx.moveTo((center[0] + radius * Math.cos(a0)) * k, (center[1] + radius * Math.sin(a0)) * k);
    ctx.arc(center[0] * k, center[1] * k, radius * k, a0, a0 + (sweep * Math.PI) / 180, sweep < 0);
  }
  ctx.stroke();
}
