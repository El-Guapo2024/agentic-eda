// Port of pcbnew's shape-modification routines (pcbnew/tools/
// item_modification_routine.cpp at 8303b2ad, driven by EDIT_TOOL::
// ModifyLines / SimplifyPolygons / HealShapes in edit_tool.cpp, and
// ConnectBoardShapes in pcbnew/fix_board_shape.cpp):
//
//   Fillet Lines / Chamfer Lines / Dogbone Corners / Extend Lines to Meet
//   Simplify Polygons                      Heal Shapes
//
// The functions are pure: they take the selected shapes (the studio's
// `/api/state` view shapes, micrometres) and return what to delete and add;
// the caller sends that as one `batch` Cmd (one undo step, like one
// `BOARD_COMMIT::Push`). Maths run in KiCad's own IU (nm) -- see
// pcbGeom.ts -- and come back rounded to micrometres.
//
// The IR has no way to edit a shape's geometry in place (a shape's id is a
// hash of its geometry), so a line KiCad "modifies" is, here, the old shape
// deleted and the new one added; `ModifyResult.replaced` names the pairs so
// a caller can carry the selection across.

import type { CmdShape, Shape } from "../api/types";
import {
  arcFromFillet,
  clampedCoords,
  computeChamferPoints,
  computeDogbone,
  eq,
  fromIU,
  MIN_PRECISION_IU,
  norm,
  resize,
  segAngle,
  segApproxCollinear,
  segContains,
  segIntersect,
  segIntersectLines,
  segIntersects,
  segLength,
  segNearestPoint,
  segSquaredDistance,
  segsParallel,
  sharedEndpoint,
  squaredNorm,
  toIU,
  vadd,
  vsub,
  type Arc,
  type Seg,
  type V,
} from "./pcbGeom";

type Pt = readonly [number, number];

const toCmdPt = (p: V): { x: number; y: number } => {
  const [x, y] = fromIU(p);
  return { x, y };
};

function cmdSegment(layer: string, width: number, a: V, b: V): CmdShape {
  return { kind: "segment", layer, stroke_width: width, filled: false, start: toCmdPt(a), end: toCmdPt(b) };
}

function cmdArc(layer: string, width: number, arc: Arc): CmdShape {
  return { kind: "arc", layer, stroke_width: width, filled: false, start: toCmdPt(arc.start), mid: toCmdPt(arc.mid), end: toCmdPt(arc.end) };
}

/** What a routine wants done to the board. */
export interface ModifyResult {
  /** Shape ids to delete (decomposed rectangles/polygons, deleted zero-length lines, and the old version of every modified line). */
  remove: string[];
  /** Shapes to add (new arcs/segments, the new version of every modified line, the lines a rectangle/polygon was decomposed into). */
  add: CmdShape[];
  /** Old id -> index into `add` of the shape that replaces it, for every modified (not deleted) line. */
  replaced: { id: string; addIndex: number }[];
  /** Indices into `add` that are NEW items (select them afterwards, `items_to_select_on_success`). */
  created: number[];
  successes: number;
  failures: number;
  /** The info-bar message (`GetStatusMessage`), if any. */
  message: string | null;
  /** False when nothing at all would change (a refusal, e.g. too few lines). */
  ok: boolean;
}

/** One line the pairwise routines act on: a real selected segment, or one conjured from a rectangle/polygon. */
interface WorkLine {
  /** The shape id, or null for a line conjured from a rectangle/polygon. */
  id: string | null;
  seg: Seg;
  layer: string;
  width: number;
  deleted: boolean;
  modified: boolean;
}

export type LineRoutine =
  | { kind: "fillet"; radiusUm: number }
  | { kind: "chamfer"; setbackUm: number }
  | { kind: "dogbone"; radiusUm: number; addSlots: boolean; boardOutline: readonly (readonly Pt[])[] }
  | { kind: "extend" };

/** The unit KiCad's dialogs default to (`pcbIUScale.mmToIU( 1 )`), in micrometres. */
export const DEFAULT_LINE_OP_SIZE_UM = 1000;

/**
 * The refusal `EDIT_TOOL::ModifyLines` gives before it even asks for parameters: its segment count
 * is the selected segments plus the lines a rectangle (four) or polygon (one per vertex) breaks into;
 * Extend needs exactly two, the others at least two.
 */
export function lineSelectionError(kind: LineRoutine["kind"], shapes: readonly Shape[]): string | null {
  let count = 0;
  for (const s of shapes) {
    if (s.kind === "segment") count++;
    else if (s.kind === "rect") count += 4;
    else if (s.kind === "polygon" && s.pts.length > 1) count += s.pts.length;
  }
  if (kind === "extend" && count !== 2) return "Exactly two lines must be selected to extend them.";
  if (count < 2) return "A shape with at least two lines must be selected.";
  return null;
}

/** `EDIT_TOOL::ModifyLines`'s own selection filter: segments, polygons and rectangles (locked ones dropped by the caller). */
export function isLineModifiable(s: Shape): boolean {
  return s.kind === "segment" || s.kind === "polygon" || s.kind === "rect";
}

/** `PCB_SHAPE::GetRectCorners`'s order for `ModifyLines`: start, (end.x, start.y), end, (start.x, end.y). */
function rectPoints(s: Extract<Shape, { kind: "rect" }>): V[] {
  const a = toIU(s.start);
  const b = toIU(s.end);
  return [a, [b[0], a[1]], b, [a[0], b[1]]];
}

/** The unmodified polygon-style shapes' IU points. */
function polyPoints(s: Extract<Shape, { kind: "polygon" }>): V[] {
  return s.pts.map((p) => toIU(p));
}

/**
 * `ModifyLines` for one routine over the selected `shapes` (in selection
 * order). `shapes` must already be filtered to `isLineModifiable` and
 * unlocked. The per-pair routines are KiCad's `LINE_FILLET_ROUTINE`,
 * `LINE_CHAMFER_ROUTINE`, `DOGBONE_CORNER_ROUTINE` and `LINE_EXTENSION_ROUTINE`.
 */
export function modifyLines(shapes: readonly Shape[], routine: LineRoutine): ModifyResult {
  const real: WorkLine[] = [];
  const conjured: WorkLine[] = [];
  const removeIds: string[] = [];

  for (const sh of shapes) {
    let pts: V[] = [];
    if (sh.kind === "rect") {
      removeIds.push(sh.id);
      pts = rectPoints(sh);
    } else if (sh.kind === "polygon") {
      removeIds.push(sh.id);
      pts = polyPoints(sh);
    } else if (sh.kind === "segment") {
      real.push({ id: sh.id, seg: { a: toIU(sh.start), b: toIU(sh.end) }, layer: sh.layer, width: sh.stroke_width, deleted: false, modified: false });
    }
    const wrap = (a: V, b: V) => conjured.push({ id: null, seg: { a, b }, layer: sh.layer, width: sh.stroke_width, deleted: false, modified: false });
    for (let j = 1; j < pts.length; j++) wrap(pts[j - 1]!, pts[j]!);
    if (pts.length > 1) wrap(pts[pts.length - 1]!, pts[0]!);
  }

  // `segmentCount = selection.CountType( PCB_SHAPE_LOCATE_SEGMENT_T ) + lines_to_add.size()`
  const segmentCount = real.length + conjured.length;
  const refuse = (message: string): ModifyResult => ({ remove: [], add: [], replaced: [], created: [], successes: 0, failures: 0, message, ok: false });
  if (routine.kind === "extend" && segmentCount !== 2) return refuse("Exactly two lines must be selected to extend them.");
  if (segmentCount < 2) return refuse("A shape with at least two lines must be selected.");

  const lines = [...real, ...conjured];
  const added: CmdShape[] = []; // new shapes created by the routine, in creation order
  let successes = 0;
  let failures = 0;
  let haveNarrowMouths = false;

  /** `ITEM_MODIFICATION_ROUTINE::ModifyLineOrDeleteIfZeroLength`. */
  const modifyOrDelete = (line: WorkLine, seg: Seg | null): void => {
    if (!seg || segLength(seg) === 0) {
      line.deleted = true;
      return;
    }
    line.modified = true;
    line.seg = seg;
  };

  const boardOutline = routine.kind === "dogbone" ? routine.boardOutline.map((ring) => ring.map((p) => toIU(p))) : [];

  const processPair = (la: WorkLine, lb: WorkLine): void => {
    if (segLength(la.seg) === 0 || segLength(lb.seg) === 0) return;
    const segA: Seg = { a: [...la.seg.a], b: [...la.seg.b] };
    const segB: Seg = { a: [...lb.seg.a], b: [...lb.seg.b] };

    switch (routine.kind) {
      case "fillet": {
        const shared = sharedEndpointRefs(segA, segB);
        if (!shared) return;
        if (segsParallel(segA, segB)) return;
        const arc = arcFromFillet(segA, segB, routine.radiusUm * 1000);
        if (!arc) {
          failures++;
          return;
        }
        let t1: V | null = null;
        let t2: V | null = null;
        const setIfOn = (which: 1 | 2, seg: Seg, p: V): boolean => {
          if (norm(vsub(segNearestPoint(seg, p), p)) < MIN_PRECISION_IU) {
            if (which === 1) t1 = [p[0], p[1]];
            else t2 = [p[0], p[1]];
            return true;
          }
          return false;
        };
        // "Do not draw a fillet if the end points of the arc are not within the track segments"
        if (!setIfOn(1, segA, arc.start) && !setIfOn(2, segB, arc.start)) {
          failures++;
          return;
        }
        if (!setIfOn(1, segA, arc.end) && !setIfOn(2, segB, arc.end)) {
          failures++;
          return;
        }
        if (!t1 || !t2) {
          failures++;
          return;
        }
        added.push(cmdArc(la.layer, la.width, arc));
        shared.setA(t1);
        shared.setB(t2);
        modifyOrDelete(la, segA);
        modifyOrDelete(lb, segB);
        successes++;
        return;
      }
      case "chamfer": {
        // "If the segments share an endpoint, we won't try to chamfer them" -- otherwise not an error.
        if (!sharedEndpoint(segA, segB)) return;
        const setback = routine.setbackUm * 1000;
        const res = computeChamferPoints(segA, segB, setback, setback);
        if (!res) {
          failures++;
          return;
        }
        added.push(cmdSegment(la.layer, la.width, res.chamfer.a, res.chamfer.b));
        modifyOrDelete(la, res.updatedA);
        modifyOrDelete(lb, res.updatedB);
        successes++;
        return;
      }
      case "dogbone": {
        if (boardOutline.length === 0) return; // "Skip: board outline unavailable"
        const shared = sharedEndpointRefs(segA, segB);
        if (!shared) return;
        if (segsParallel(segA, segB)) {
          failures++;
          return;
        }
        // Does this corner point into the board outline? Sample just outside the bisector.
        const corner = shared.a();
        const vecA = eq(segA.a, corner) ? vsub(segA.b, corner) : vsub(segA.a, corner);
        const vecB = eq(segB.a, corner) ? vsub(segB.b, corner) : vsub(segB.a, corner);
        const maxLen = Math.max(norm(vecA), norm(vecB));
        if (maxLen === 0) return;
        const bisector = vadd(resize(vecA, maxLen), resize(vecB, maxLen));
        if (norm(bisector) === 0) return;
        const sampleDir = resize([-bisector[0], -bisector[1]], Math.min(1000, routine.radiusUm * 1000));
        const sample = vadd(corner, sampleDir);
        if (!pointInRings(boardOutline, sample)) return; // "corner not inward"
        const res = computeDogbone(segA, segB, routine.radiusUm * 1000, routine.addSlots);
        if (!res) {
          failures++;
          return;
        }
        if (res.smallArcMouth) haveNarrowMouths = true;
        const addSeg = (s: Seg) => {
          if (segLength(s) === 0) return;
          added.push(cmdSegment(la.layer, la.width, s.a, s.b));
        };
        // `tArc` is created first but added last (after the two cap segments).
        addSeg({ a: res.arc.start, b: res.updatedA ? res.updatedA.b : res.arc.start });
        addSeg({ a: res.arc.end, b: res.updatedB ? res.updatedB.b : res.arc.end });
        added.push(cmdArc(la.layer, la.width, res.arc));
        modifyOrDelete(la, res.updatedA);
        modifyOrDelete(lb, res.updatedB);
        successes++;
        return;
      }
      case "extend": {
        if (segIntersects(segA, segB)) return; // already intersecting
        const hit = segIntersectLines(segA, segB);
        if (!hit) return; // parallel
        const extend = (line: WorkLine, seg: Seg): void => {
          if (segContains(seg, hit)) return;
          const distStart = norm(vsub(hit, seg.a));
          const distEnd = norm(vsub(hit, seg.b));
          const furthest = distStart < distEnd ? seg.b : seg.a;
          // "the drawing tool has COORDS_PADDING of 20mm, but we need a larger buffer": 200 mm
          const newEnd = clampedCoords(hit, 200 * 1_000_000);
          line.modified = true;
          line.seg = { a: [furthest[0], furthest[1]], b: newEnd };
        };
        extend(la, segA);
        extend(lb, segB);
        successes++;
        return;
      }
    }
  };

  // `alg::for_all_pairs` over the selection, skipping lines a previous pair deleted (STRUCT_DELETED).
  for (let i = 0; i < lines.length; i++) {
    for (let j = i + 1; j < lines.length; j++) {
      if (lines[i]!.deleted || lines[j]!.deleted) continue;
      processPair(lines[i]!, lines[j]!);
    }
  }

  // ---- assemble the commit
  const remove = [...removeIds];
  const add: CmdShape[] = [];
  const replaced: { id: string; addIndex: number }[] = [];
  const created: number[] = [];
  for (const l of lines) {
    if (l.id !== null) {
      if (l.deleted) remove.push(l.id);
      else if (l.modified) {
        remove.push(l.id);
        replaced.push({ id: l.id, addIndex: add.length });
        add.push(cmdSegment(l.layer, l.width, l.seg.a, l.seg.b));
      }
    } else if (!l.deleted) {
      created.push(add.length); // a rectangle/polygon's own lines come back as new lines
      add.push(cmdSegment(l.layer, l.width, l.seg.a, l.seg.b));
    }
  }
  for (const a of added) {
    created.push(add.length);
    add.push(a);
  }

  return { remove, add, replaced, created, successes, failures, message: statusMessage(routine, successes, failures, segmentCount, haveNarrowMouths), ok: true };
}

/** `GetStatusMessage( aSegmentCount )` of each pairwise routine. */
function statusMessage(routine: LineRoutine, successes: number, failures: number, segmentCount: number, narrow: boolean): string | null {
  const partial = failures > 0 || successes < segmentCount - 1;
  switch (routine.kind) {
    case "fillet":
      return successes === 0 ? "Unable to fillet the selected lines." : partial ? "Some of the lines could not be filleted." : null;
    case "chamfer":
      return successes === 0 ? "Unable to chamfer the selected lines." : partial ? "Some of the lines could not be chamfered." : null;
    case "extend":
      return successes === 0 ? "Unable to extend the selected lines to meet." : partial ? "Some of the lines could not be extended to meet." : null;
    case "dogbone": {
      let msg = "";
      if (successes === 0) msg += "Unable to add dogbone corners to the selected lines.";
      else if (partial) msg += "Some of the lines could not have dogbone corners added.";
      if (narrow) {
        if (msg) msg += " ";
        msg += "Some of the dogbone corners are too narrow to fit a cutter of the specified radius.";
        msg += routine.addSlots ? " Slots were added." : " Consider enabling the 'Add Slots' option.";
      }
      return msg || null;
    }
  }
}

/** `GetSharedEndpoints( segA, segB )`: handles on the matching end of each segment (the first match in A.A, A.B order of the C++). */
function sharedEndpointRefs(a: Seg, b: Seg): { a: () => V; setA: (p: V) => void; setB: (p: V) => void } | null {
  let ai: "a" | "b" | null = null;
  let bi: "a" | "b" | null = null;
  if (eq(a.a, b.a)) [ai, bi] = ["a", "a"];
  else if (eq(a.a, b.b)) [ai, bi] = ["a", "b"];
  else if (eq(a.b, b.a)) [ai, bi] = ["b", "a"];
  else if (eq(a.b, b.b)) [ai, bi] = ["b", "b"];
  if (!ai || !bi) return null;
  return {
    a: () => a[ai!],
    setA: (p) => {
      a[ai!] = p;
    },
    setB: (p) => {
      b[bi!] = p;
    },
  };
}

/** Even-odd point-in-rings test (outline with holes): `SHAPE_POLY_SET::Contains` for a set of nested rings. */
export function pointInRings(rings: readonly (readonly V[])[], p: V): boolean {
  let inside = false;
  for (const ring of rings) {
    const n = ring.length;
    for (let i = 0, j = n - 1; i < n; j = i++) {
      const [xi, yi] = ring[i]!;
      const [xj, yj] = ring[j]!;
      if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
    }
  }
  return inside;
}

// ------------------------------------------------------------------ Simplify

/** `TestSegmentHit( ref, start, end, dist )` (geometry_utils.cpp). */
export function testSegmentHit(ref: V, start: V, end: V, dist: number): boolean {
  let xmin = start[0];
  let xmax = end[0];
  let ymin = start[1];
  let ymax = end[1];
  const deltaX = start[0] - ref[0];
  const deltaY = start[1] - ref[1];
  if (xmax < xmin) [xmax, xmin] = [xmin, xmax];
  if (ymax < ymin) [ymax, ymin] = [ymin, ymax];
  if (ymin - ref[1] > dist || ref[1] - ymax > dist) return false;
  if (xmin - ref[0] > dist || ref[0] - xmax > dist) return false;
  if (start[0] === end[0] && ref[1] > ymin && ref[1] < ymax) return Math.abs(deltaX) <= dist;
  if (start[1] === end[1] && ref[0] > xmin && ref[0] < xmax) return Math.abs(deltaY) <= dist;
  return segSquaredDistance({ a: start, b: end }, ref) < (dist + 1) * (dist + 1);
}

/** `SHAPE_LINE_CHAIN::Simplify( tolerance )` (shape_line_chain.cpp), point-only (no arcs): drop vertices a longer straight run covers within `tolerance`. */
export function simplifyChain(points: readonly V[], closed: boolean, tolerance: number): V[] {
  const n = points.length;
  if (n < 3) return points.map((p) => [p[0], p[1]] as V);
  const out: V[] = [];
  let start = 0;
  while (start < n) {
    out.push([points[start]![0], points[start]![1]]);
    // If the line is not closed, we need at least 3 points before simplifying
    if (!closed && start === n - 2) break;
    let end = (start + 2) % n;
    let can = true;
    while (can && end !== start && (end > start || closed)) {
      for (let test = (start + 1) % n; test !== end; test = (test + 1) % n) {
        if (!testSegmentHit(points[test]!, points[start]!, points[end]!, tolerance)) {
          can = false;
          break;
        }
      }
      if (can) end = (end + 1) % n;
    }
    if (end === (start + 2) % n) {
      start++;
    } else {
      const newStart = (end + n - 1) % n;
      if (newStart <= start) break;
      start = newStart;
    }
  }
  if (out.length === 1) out.push([points[n - 1]![0], points[n - 1]![1]]);
  if (!closed) {
    const last = points[n - 1]!;
    if (!eq(last, out[out.length - 1]!)) out.push([last[0], last[1]]);
  }
  return out;
}

/** `EDIT_TOOL::SimplifyPolygons` on one polygon ring in micrometres: the simplified ring, or null if it did not change. */
export function simplifyPolygonRing(pts: readonly Pt[], toleranceUm: number): [number, number][] | null {
  const simplified = simplifyChain(
    pts.map((p) => toIU(p)),
    true,
    toleranceUm * 1000
  ).map((p) => fromIU(p));
  if (simplified.length === pts.length && simplified.every((p, i) => p[0] === pts[i]![0] && p[1] === pts[i]![1])) return null;
  return simplified;
}

// ---------------------------------------------------------------------- Heal

type HealShape =
  | { kind: "segment"; id: string; start: V; end: V }
  | { kind: "arc"; id: string; start: V; mid: V; end: V }
  | { kind: "bezier"; id: string; start: V; c1: V; c2: V; end: V };

function toHeal(s: Shape): HealShape | null {
  if (s.kind === "segment") return { kind: "segment", id: s.id, start: toIU(s.start), end: toIU(s.end) };
  if (s.kind === "arc") return { kind: "arc", id: s.id, start: toIU(s.start), mid: toIU(s.mid), end: toIU(s.end) };
  if (s.kind === "bezier") return { kind: "bezier", id: s.id, start: toIU(s.start), c1: toIU(s.c1), c2: toIU(s.c2), end: toIU(s.end) };
  return null;
}

const sq = (a: V, b: V): number => squaredNorm(vsub(a, b));

/** Index of the smallest of four squared distances (`std::min_element`: the first on ties). */
function argMin4(d: readonly number[]): number {
  let best = 0;
  for (let i = 1; i < 4; i++) if (d[i]! < d[best]!) best = i;
  return best;
}

/** `ConnectBoardShapes`'s `connectPair`: move the nearest pair of end points of two shapes together. */
function connectPair(p0: HealShape, p1: HealShape): boolean {
  const s0 = p0.kind;
  const s1 = p1.kind;
  if (s0 === "segment" && s1 === "segment") {
    const seg0: Seg = { a: p0.start, b: p0.end };
    const seg1: Seg = { a: p1.start, b: p1.end };
    const d = [sq(seg0.a, seg1.a), sq(seg0.a, seg1.b), sq(seg0.b, seg1.a), sq(seg0.b, seg1.b)];
    const idx = argMin4(d);
    const i0 = idx >> 1;
    const i1 = idx % 2;
    if (segIntersects(seg0, seg1) || segAngle(seg0, seg1) > 45) {
      const inter = segIntersectLines(seg0, seg1);
      if (inter) {
        if (i0 === 0) p0.start = inter;
        else p0.end = inter;
        if (i1 === 0) p1.start = inter;
        else p1.end = inter;
        return true;
      }
    }
    return false;
  }
  if ((s0 === "arc" && s1 === "segment") || (s0 === "segment" && s1 === "arc") || (s0 === "bezier" && s1 === "segment") || (s0 === "segment" && s1 === "bezier")) {
    const other = (s0 === "segment" ? p1 : p0) as Exclude<HealShape, { kind: "segment" }>;
    const seg = (s0 === "segment" ? p0 : p1) as Extract<HealShape, { kind: "segment" }>;
    const pts: V[] = [other.start, other.end];
    const segPts: V[] = [seg.start, seg.end];
    const d = [sq(segPts[0]!, pts[0]!), sq(segPts[0]!, pts[1]!), sq(segPts[1]!, pts[0]!), sq(segPts[1]!, pts[1]!)];
    const idx = argMin4(d);
    if (idx === 0) seg.start = pts[0]!;
    else if (idx === 1) seg.start = pts[1]!;
    else if (idx === 2) seg.end = pts[0]!;
    else seg.end = pts[1]!;
    return true;
  }
  if (s0 === "arc" && s1 === "arc") {
    const a0 = p0 as Extract<HealShape, { kind: "arc" }>;
    const a1 = p1 as Extract<HealShape, { kind: "arc" }>;
    const pts0: V[] = [a0.start, a0.end];
    const pts1: V[] = [a1.start, a1.end];
    const d = [sq(pts0[0]!, pts1[0]!), sq(pts0[0]!, pts1[1]!), sq(pts0[1]!, pts1[0]!), sq(pts0[1]!, pts1[1]!)];
    const idx = argMin4(d);
    const i0 = idx >> 1;
    const i1 = idx % 2;
    const middle: V = [Math.trunc((pts0[i0]![0] + pts1[i1]![0]) / 2), Math.trunc((pts0[i0]![1] + pts1[i1]![1]) / 2)];
    if (i0 === 0) a0.start = middle;
    else a0.end = middle;
    if (i1 === 0) a1.start = middle;
    else a1.end = middle;
    return true;
  }
  if ((s0 === "bezier" && s1 === "arc") || (s0 === "arc" && s1 === "bezier")) {
    const bez = (s0 === "bezier" ? p0 : p1) as Extract<HealShape, { kind: "bezier" }>;
    const arc = (s0 === "arc" ? p0 : p1) as Extract<HealShape, { kind: "arc" }>;
    const bezPts: V[] = [bez.start, bez.end];
    const arcPts: V[] = [arc.start, arc.end];
    const d = [sq(bezPts[0]!, arcPts[0]!), sq(bezPts[0]!, arcPts[1]!), sq(bezPts[1]!, arcPts[0]!), sq(bezPts[1]!, arcPts[1]!)];
    const idx = argMin4(d);
    const target = arcPts[idx % 2]!;
    if (idx < 2) {
      const delta = vsub(target, bez.start);
      bez.start = target;
      bez.c1 = vadd(bez.c1, delta);
    } else {
      const delta = vsub(target, bez.end);
      bez.end = target;
      bez.c2 = vadd(bez.c2, delta);
    }
    return true;
  }
  if (s0 === "bezier" && s1 === "bezier") {
    const b0 = p0 as Extract<HealShape, { kind: "bezier" }>;
    const b1 = p1 as Extract<HealShape, { kind: "bezier" }>;
    const pts0: V[] = [b0.start, b0.end];
    const pts1: V[] = [b1.start, b1.end];
    const d = [sq(pts0[0]!, pts1[0]!), sq(pts0[0]!, pts1[1]!), sq(pts0[1]!, pts1[0]!), sq(pts0[1]!, pts1[1]!)];
    const idx = argMin4(d);
    const i0 = idx >> 1;
    const i1 = idx % 2;
    const middle: V = [Math.trunc((pts0[i0]![0] + pts1[i1]![0]) / 2), Math.trunc((pts0[i0]![1] + pts1[i1]![1]) / 2)];
    const move = (b: typeof b0, atStart: boolean) => {
      if (atStart) {
        const delta = vsub(middle, b.start);
        b.start = middle;
        b.c1 = vadd(b.c1, delta);
      } else {
        const delta = vsub(middle, b.end);
        b.end = middle;
        b.c2 = vadd(b.c2, delta);
      }
    };
    move(b0, i0 === 0);
    move(b1, i1 === 0);
    return true;
  }
  return false;
}

/**
 * `ConnectBoardShapes( shapeList, epsilon )` (fix_board_shape.cpp), the body of
 * `EDIT_TOOL::HealShapes`: walk the selected segments/arcs/curves end to end
 * and snap each pair of neighbouring end points (closer than `toleranceUm`)
 * together. The k-d tree search is a brute-force scan returning the same two
 * nearest end points.
 */
export function healShapes(shapes: readonly Shape[], toleranceUm: number): ModifyResult {
  const work = shapes.map(toHeal).filter((s): s is HealShape => s !== null);
  const before = work.map((w) => JSON.stringify(w));
  const eps = toleranceUm * 1000;

  // Every end point of every shape (two per shape), in the adaptor's order.
  const endpoints = (): { p: V; shape: HealShape }[] => work.flatMap((s) => [{ p: s.start, shape: s }, { p: s.end, shape: s }]);
  const skip = new Set<HealShape>();

  /** `findNext`: of the two nearest end points to `p`, the closest valid one of another shape within `eps`. */
  const findNext = (curr: HealShape, p: V): HealShape | null => {
    const pts = endpoints();
    const order = pts.map((e, i) => ({ i, d: sq(p, e.p) })).sort((a, b) => a.d - b.d || a.i - b.i);
    let closest: HealShape | null = null;
    let closestSq = eps * eps;
    for (const { i, d } of order.slice(0, 2)) {
      const cand = pts[i]!.shape;
      if (cand === curr || skip.has(cand)) continue;
      if (d < closestSq) {
        closestSq = d;
        closest = cand;
      }
    }
    return closest;
  };

  const closerToFirst = (ref: V, first: V, second: V): boolean => sq(ref, first) < sq(ref, second);
  const minDistanceSq = (ref: V, first: V, second: V): number => Math.min(sq(ref, first), sq(ref, second));

  const startCandidates = [...work];
  while (startCandidates.length > 0) {
    const graphic = startCandidates[0]!;
    const walkFrom = (start: HealShape, startPt: V): void => {
      let curr = start;
      let prevPt = startPt;
      for (;;) {
        const next = findNext(curr, prevPt);
        if (!next) break;
        connectPair(curr, next);
        prevPt = closerToFirst(prevPt, next.start, next.end) ? next.end : next.start;
        curr = next;
        skip.add(curr);
        const at = startCandidates.indexOf(curr);
        if (at >= 0) startCandidates.splice(at, 1);
      }
    };

    const ptEnd = graphic.end;
    const ptStart = graphic.start;
    const grAtEnd = findNext(graphic, ptEnd);
    const grAtStart = findNext(graphic, ptStart);
    let beginFromEnd = true;
    if (grAtEnd && grAtStart) {
      beginFromEnd = minDistanceSq(ptEnd, grAtEnd.start, grAtEnd.end) <= minDistanceSq(ptStart, grAtStart.start, grAtStart.end);
    } else if (grAtEnd) beginFromEnd = true;
    else if (grAtStart) beginFromEnd = false;

    if (beginFromEnd) {
      walkFrom(graphic, graphic.end);
      walkFrom(graphic, graphic.start);
    } else {
      walkFrom(graphic, graphic.start);
      walkFrom(graphic, graphic.end);
    }
    const at = startCandidates.indexOf(graphic);
    if (at >= 0) startCandidates.splice(at, 1);
  }

  const remove: string[] = [];
  const add: CmdShape[] = [];
  const replaced: { id: string; addIndex: number }[] = [];
  const byId = new Map(shapes.map((s) => [s.id, s]));
  work.forEach((w, i) => {
    if (JSON.stringify(w) === before[i]) return;
    const orig = byId.get(w.id)!;
    remove.push(w.id);
    replaced.push({ id: w.id, addIndex: add.length });
    const base = { layer: orig.layer, stroke_width: orig.stroke_width, filled: orig.filled };
    if (w.kind === "segment") add.push({ kind: "segment", ...base, start: toCmdPt(w.start), end: toCmdPt(w.end) });
    else if (w.kind === "arc") add.push({ kind: "arc", ...base, start: toCmdPt(w.start), mid: toCmdPt(w.mid), end: toCmdPt(w.end) });
    else add.push({ kind: "bezier", ...base, start: toCmdPt(w.start), c1: toCmdPt(w.c1), c2: toCmdPt(w.c2), end: toCmdPt(w.end) });
  });
  return { remove, add, replaced, created: [], successes: add.length, failures: 0, message: null, ok: true };
}

/** `segApproxCollinear` re-exported for FilletTracks' "ignore collinear tracks" check. */
export { segApproxCollinear, segIntersect };
