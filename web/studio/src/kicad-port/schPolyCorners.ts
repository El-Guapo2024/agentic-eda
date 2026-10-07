// Create Corner / Remove Corner on a selected polygon or rule area -- `SCH_POINT_EDITOR::addCorner` / `removeCorner` and their menu conditions
// (`addCornerCondition`, `removeCornerCondition`; eeschema/tools/sch_point_editor.cpp at 8303b2ad).
export type P = readonly [number, number];

/** What the shape is: a rule area keeps at least 3 corners, a polygon shape at least 2 (`removeCorner`'s guards); the studio's polygons are always closed. */
export type PolyKind = "polygon" | "rule_area";

const minCorners = (kind: PolyKind): number => (kind === "rule_area" ? 3 : 2);

function distToSegment(p: P, a: P, b: P): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

/** The outline segment nearest the cursor (the closing one included); ties go to the earlier segment, as `addCorner`'s `<` does. */
export function nearestEdge(pts: readonly P[], cursor: P): { index: number; distance: number } {
  let index = 0;
  let best = Infinity;
  for (let i = 0; i < pts.length; i++) {
    // KiCad compares `SEG::Distance`, an integer distance.
    const d = Math.trunc(distToSegment(cursor, pts[i]!, pts[(i + 1) % pts.length]!));
    if (d < best) {
      best = d;
      index = i;
    }
  }
  return { index, distance: best };
}

/** `addCornerCondition`: the cursor is on the outline (within `tol`), so a corner can be created there. */
export function canAddCorner(pts: readonly P[], cursor: P, tol: number): boolean {
  return pts.length >= 2 && nearestEdge(pts, cursor).distance <= tol;
}

/** `addCorner`: a new corner at the cursor, spliced in after the start of the nearest edge. */
export function addCorner(pts: readonly P[], cursor: P): P[] {
  const { index } = nearestEdge(pts, cursor);
  return [...pts.slice(0, index + 1), cursor, ...pts.slice(index + 1)];
}

/** The corner under the cursor (the edited point), or -1: the nearest one within `tol`. */
export function cornerAt(pts: readonly P[], cursor: P, tol: number): number {
  let best = -1;
  let bestD = tol;
  pts.forEach((p, i) => {
    const d = Math.hypot(p[0] - cursor[0], p[1] - cursor[1]);
    if (d <= bestD) {
      bestD = d;
      best = i;
    }
  });
  return best;
}

/** `removeCornerCondition`: a corner is under the cursor and the shape would keep enough corners. */
export function canRemoveCorner(kind: PolyKind, pts: readonly P[], cursor: P, tol: number): boolean {
  return pts.length > minCorners(kind) && cornerAt(pts, cursor, tol) >= 0;
}

/** `removeCorner`: the outline without the corner under the cursor, or null when the conditions do not hold. */
export function removeCorner(kind: PolyKind, pts: readonly P[], cursor: P, tol: number): P[] | null {
  if (!canRemoveCorner(kind, pts, cursor, tol)) return null;
  const i = cornerAt(pts, cursor, tol);
  return pts.filter((_, j) => j !== i);
}
