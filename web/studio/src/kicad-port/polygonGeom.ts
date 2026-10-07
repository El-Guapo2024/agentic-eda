// The click-by-click polygon construction `SCH_DRAWING_TOOLS::DrawRuleArea` runs on -- `POLYGON_GEOM_MANAGER`
// (common/preview_items/polygon_geom_manager.cpp at 8303b2ad), its 45-degree leader (`build45DegLeader`) and the way
// `RULE_AREA_CREATE_HELPER::OnComplete` turns the clicked corners into the final outline.
//
// Locked-in corners are the clicks so far; the leader is the rubber band from the last one to the cursor (a straight line, or a
// bend that keeps every segment on a 45-degree multiple), the loop the path closing the polygon from the cursor back to the first corner.
export type Pt = readonly [number, number];
export type LeaderMode = "direct" | "deg45" | "deg90";

export interface PolyGeom {
  mode: LeaderMode;
  /** `m_lockedPoints`. */
  locked: Pt[];
  /** `m_leaderPts`: last locked corner to the cursor. */
  leader: Pt[];
  /** `m_loopPts`: the cursor back to the first corner (deg45/deg90 only). */
  loop: Pt[];
}

export const newPolyGeom = (mode: LeaderMode): PolyGeom => ({ mode, locked: [], leader: [], loop: [] });

/** `IsPolygonInProgress`. */
export const isPolygonInProgress = (g: PolyGeom): boolean => g.locked.length > 0;

const same = (a: Pt, b: Pt) => a[0] === b[0] && a[1] === b[1];

/** `SHAPE_LINE_CHAIN::Append` without `aAllowDuplication`: a point equal to the last one is not added. */
function append(chain: Pt[], p: Pt): void {
  if (chain.length === 0 || !same(chain[chain.length - 1]!, p)) chain.push(p);
}

/** `KiROUND`: half away from zero. */
const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

const copysign = (magnitude: number, sign: number): number => Math.abs(magnitude) * (sign < 0 || Object.is(sign, -0) ? -1 : 1);

/** `GetVectorSnapped45`: the vector put on the nearest of the eight 45-degree directions (axes included unless `only45`). */
export function snapVector45(v: Pt, only45 = false): [number, number] {
  const ax = Math.abs(v[0]);
  const ay = Math.abs(v[1]);
  if (!only45 && ax > ay * 2) return [v[0], 0];
  if (!only45 && ay > ax * 2) return [0, v[1]];
  if (ax > ay) return [v[0], copysign(v[0], v[1])];
  return [copysign(v[1], v[0]), v[1]];
}

const deg = (v: Pt): number => (Math.atan2(v[1], v[0]) * 180) / Math.PI;
const normalize180 = (a: number): number => {
  while (a <= -180) a += 360;
  while (a > 180) a -= 360;
  return a;
};
const normalize90 = (a: number): number => {
  while (a < -90) a += 180;
  while (a > 90) a -= 180;
  return a;
};

/** `build45DegLeader`: from the last point of `last` to `end` in at most two segments, each on a 45-degree multiple. */
export function build45DegLeader(end: Pt, last: readonly Pt[]): Pt[] {
  if (last.length < 1) return [];
  const lastPt = last[last.length - 1]!;
  const lineVec: Pt = [end[0] - lastPt[0], end[1] - lastPt[1]];
  if (last.length < 2) {
    const s = snapVector45(lineVec);
    return [lastPt, [lastPt[0] + s[0], lastPt[1] + s[1]]];
  }
  const prev = last[last.length - 2]!;
  const lineA = deg(lineVec);
  const prevA = deg(snapVector45([lastPt[0] - prev[0], lastPt[1] - prev[1]]));
  const vertical = Math.abs(lineVec[1]) > Math.abs(lineVec[0]);
  const horizontal = Math.abs(lineVec[1]) < Math.abs(lineVec[0]);
  const angDiff = Math.abs(normalize180(lineA - prevA));
  let bendEnd = angDiff < 45 || (angDiff > 90 && angDiff < 135);
  if (Math.abs(Math.abs(normalize90(prevA)) - 45) < 1e-9) bendEnd = !bendEnd;
  let mid: [number, number] = [end[0], end[1]];
  if (bendEnd) {
    if (vertical) mid = [lastPt[0], lineVec[1] > 0 ? end[1] - Math.abs(lineVec[0]) : end[1] + Math.abs(lineVec[0])];
    else if (horizontal) mid = [lineVec[0] > 0 ? end[0] - Math.abs(lineVec[1]) : end[0] + Math.abs(lineVec[1]), lastPt[1]];
  } else if (vertical) mid = [end[0], lineVec[1] > 0 ? lastPt[1] + Math.abs(lineVec[0]) : lastPt[1] - Math.abs(lineVec[0])];
  else if (horizontal) mid = [lineVec[0] > 0 ? lastPt[0] + Math.abs(lineVec[1]) : lastPt[0] - Math.abs(lineVec[1]), end[1]];
  return [lastPt, [kiRound(mid[0]), kiRound(mid[1])], end];
}

/** `build90DegLeader`: from the last point of `last` to `end` by a horizontal then a vertical segment. */
export function build90DegLeader(end: Pt, last: readonly Pt[]): Pt[] {
  if (last.length < 1) return [];
  const lastPt = last[last.length - 1]!;
  if (lastPt[0] === end[0] || lastPt[1] === end[1]) return [lastPt, end];
  return [lastPt, [end[0], lastPt[1]], end];
}

/** `updateTemporaryLines`: recompute the leader (and loop) for the cursor at `end`. */
function updateTemporaryLines(g: PolyGeom, end: Pt): PolyGeom {
  if (g.locked.length === 0) return g;
  const lastPt = g.locked[g.locked.length - 1]!;
  const reversed = [...g.locked].reverse();
  if (g.mode === "deg45") return { ...g, leader: build45DegLeader(end, g.locked), loop: build45DegLeader(end, reversed).reverse() };
  if (g.mode === "deg90") return { ...g, leader: build90DegLeader(end, g.locked), loop: build90DegLeader(end, reversed).reverse() };
  return { ...g, leader: [lastPt, end], loop: [] };
}

/** `SetLeaderMode`: the mode a new cursor position is laid out with (the tool resets it every event from the sheet's line mode). */
export const withMode = (g: PolyGeom, mode: LeaderMode): PolyGeom => (g.mode === mode ? g : { ...g, mode });

/** `AddPoint`: lock in the next corner -- the end of the leader (its bend and end, in a 45/90-degree mode) or the cursor itself. */
export function addPoint(g: PolyGeom, pt: Pt): PolyGeom {
  const locked = [...g.locked];
  if (g.leader.length > 1) {
    append(locked, g.leader[g.leader.length - 2]!);
    append(locked, g.leader[g.leader.length - 1]!);
  } else {
    append(locked, pt);
  }
  return updateTemporaryLines({ ...g, locked }, pt);
}

/** `SetCursorPosition`. */
export const setCursorPosition = (g: PolyGeom, pos: Pt): PolyGeom => updateTemporaryLines(g, pos);

/** `NewPointClosesOutline`: does a click at `pt` land on the first corner? */
export const newPointClosesOutline = (g: PolyGeom, pt: Pt): boolean => g.locked.length > 0 && same(g.locked[0]!, pt);

/** `DeleteLastCorner`: drop the last locked corner; the corner it was is returned (the tool warps the cursor back onto it). */
export function deleteLastCorner(g: PolyGeom): { geom: PolyGeom; last: Pt | null } {
  if (g.locked.length === 0) return { geom: g, last: null };
  const last = g.locked[g.locked.length - 1]!;
  const locked = g.locked.slice(0, -1);
  // "update the new last segment (was previously locked in), reusing last constraints".
  const geom = locked.length > 0 ? updateTemporaryLines({ ...g, locked }, g.leader[g.leader.length - 1] ?? last) : { ...g, locked, leader: [], loop: [] };
  return { geom, last };
}

/** Is `p` within `tol` of the segment `a`-`b`? (`TestSegmentHit`.) */
function nearSegment(p: Pt, a: Pt, b: Pt, tol: number): boolean {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  let t = len2 === 0 ? 0 : ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy)) <= tol;
}

/** `SHAPE_LINE_CHAIN::Simplify( aTolerance )` on a closed chain of plain points: drop every point that lies within `tolerance` of the line joining its kept neighbours. */
export function simplifyClosed(points: readonly Pt[], tolerance: number): Pt[] {
  const n = points.length;
  if (n < 3) return [...points];
  const out: Pt[] = [];
  let start = 0;
  while (start < n) {
    out.push(points[start]!);
    let end = (start + 2) % n;
    let can = true;
    // (`end_idx > start_idx || m_closed`: the chain is closed, so the end may wrap around.)
    while (can && end !== start) {
      for (let t = (start + 1) % n; t !== end; t = (t + 1) % n) {
        if (!nearSegment(points[t]!, points[start]!, points[end]!, tolerance)) {
          can = false;
          break;
        }
      }
      if (can) end = (end + 1) % n;
    }
    if (end === (start + 2) % n) {
      start++;
    } else {
      const next = (end + n - 1) % n;
      if (next <= start) break;
      start = next;
    }
  }
  return out;
}

/** The distance from `p` to the infinite line through `a` and `b` (`SEG::LineDistance`). */
function lineDistance(a: Pt, b: Pt, p: Pt): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len = Math.hypot(dx, dy);
  return len === 0 ? Math.hypot(p[0] - a[0], p[1] - a[1]) : Math.abs(dx * (p[1] - a[1]) - dy * (p[0] - a[0])) / len;
}

/**
 * `RULE_AREA_CREATE_HELPER::OnComplete`: the closed outline of the corners locked in (plus, in a 45/90-degree mode, the leader's and the loop's own bends,
 * "as they are shown in the preview"), simplified; null when fewer than three corners were locked ("just scrap the rule area in progress").
 */
export function finalOutline(g: PolyGeom): Pt[] | null {
  if (g.locked.length < 3) return null;
  const outline: Pt[] = [];
  for (const p of g.locked) append(outline, p);
  if (g.mode === "deg45" || g.mode === "deg90") {
    for (let i = 1; i < g.leader.length; i++) append(outline, g.leader[i]!);
    for (let i = 1; i < g.loop.length - 1; i++) append(outline, g.loop[i]!);
  }
  let pts = simplifyClosed(outline, 1);
  // "Remove the start point if it lies on the line between neighbouring points. Simplify doesn't handle that currently."
  if (pts.length >= 3 && lineDistance(pts[pts.length - 1]!, pts[1]!, pts[0]!) <= 1) pts = pts.slice(1);
  return pts.length >= 3 ? pts : null;
}
