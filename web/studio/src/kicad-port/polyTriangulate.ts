// Ear-clipping triangulation of a polygon with holes, for `pcbnew.Control.zoneDisplayTesselation` ("Draw Zone Fill
// Triangulation", `ZONE_DISPLAY_MODE::SHOW_TRIANGULATION`): the display draws the triangles KiCad caches for a zone's fill
// (`SHAPE_POLY_SET::CacheTriangulation`).
//
// KiCad's own is libs/kimath/include/geometry/polygon_triangulation.h (`POLYGON_TRIANGULATION`), which says it is derived from
// Mapbox's earcut (ISC) and K-3D: the polygon's holes are bridged into the outer ring's linked list
// (`eliminateHoles`/`findHoleBridge`), then ears are clipped (`earcutList`/`isEar`), and a ring that stops yielding ears is
// retried after `filterPoints`, `cureLocalIntersections` and finally `splitPolygon`. This is that same earcut skeleton
// (the same passes in the same order) without the Z-order hash `isEarHashed` and KiCad's uniform-subdivision/balanced splitting
// for very large rings, which only make it faster, never different: the display is the debug view of whether the fill
// triangulates, not a mesh to render.
//
// Pure, dependency-free; runs under `npm run test:unit`.

export type Pt = readonly [number, number];

export interface TriPolygon {
  outline: readonly Pt[];
  holes: readonly (readonly Pt[])[];
}

export type Triangle = readonly [Pt, Pt, Pt];

interface Node {
  i: number;
  x: number;
  y: number;
  prev: Node;
  next: Node;
  steiner: boolean;
}

/** Twice the signed area of a ring (positive = clockwise in a y-down frame, like earcut's `signedArea`). */
function signedArea(ring: readonly Pt[]): number {
  let sum = 0;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [px, py] = ring[i]!;
    const [qx, qy] = ring[j]!;
    sum += (qx - px) * (py + qy);
  }
  return sum;
}

function insertNode(i: number, x: number, y: number, last: Node | null): Node {
  const p = { i, x, y, prev: null as unknown as Node, next: null as unknown as Node, steiner: false } as Node;
  if (!last) {
    p.prev = p;
    p.next = p;
  } else {
    p.next = last.next;
    p.prev = last;
    last.next.prev = p;
    last.next = p;
  }
  return p;
}

function removeNode(p: Node): void {
  p.next.prev = p.prev;
  p.prev.next = p.next;
}

function equals(a: Node, b: Node): boolean {
  return a.x === b.x && a.y === b.y;
}

/** Twice the signed area of the triangle p, q, r (earcut's `area`). */
function area(p: Node, q: Node, r: Node): number {
  return (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y);
}

function linkedList(points: readonly Pt[], start: number, clockwise: boolean): Node | null {
  let last: Node | null = null;
  if (clockwise === signedArea(points) > 0) {
    for (let k = 0; k < points.length; k++) last = insertNode(start + k, points[k]![0], points[k]![1], last);
  } else {
    for (let k = points.length - 1; k >= 0; k--) last = insertNode(start + k, points[k]![0], points[k]![1], last);
  }
  if (last && equals(last, last.next)) {
    removeNode(last);
    last = last.next;
  }
  return last;
}

/** `filterPoints`: drop duplicate and collinear points so the ring stays strictly convex-or-reflex. */
function filterPoints(start: Node | null, end?: Node | null): Node | null {
  if (!start) return start;
  if (!end) end = start;
  let p: Node = start;
  let again: boolean;
  do {
    again = false;
    if (!p.steiner && (equals(p, p.next) || area(p.prev, p, p.next) === 0)) {
      removeNode(p);
      p = end = p.prev;
      if (p === p.next) break;
      again = true;
    } else {
      p = p.next;
    }
  } while (again || p !== end);
  return end;
}

function pointInTriangle(ax: number, ay: number, bx: number, by: number, cx: number, cy: number, px: number, py: number): boolean {
  return (cx - px) * (ay - py) >= (ax - px) * (cy - py) && (ax - px) * (by - py) >= (bx - px) * (ay - py) && (bx - px) * (cy - py) >= (cx - px) * (by - py);
}

function pointInTriangleExceptFirst(ax: number, ay: number, bx: number, by: number, cx: number, cy: number, px: number, py: number): boolean {
  return !(ax === px && ay === py) && pointInTriangle(ax, ay, bx, by, cx, cy, px, py);
}

/** `isEar`: a convex corner whose triangle holds no other vertex of the ring. */
function isEar(ear: Node): boolean {
  const a = ear.prev;
  const b = ear;
  const c = ear.next;
  if (area(a, b, c) >= 0) return false; // reflex: not an ear
  const x0 = Math.min(a.x, b.x, c.x);
  const y0 = Math.min(a.y, b.y, c.y);
  const x1 = Math.max(a.x, b.x, c.x);
  const y1 = Math.max(a.y, b.y, c.y);
  let p = c.next;
  while (p !== a) {
    if (p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1 && pointInTriangleExceptFirst(a.x, a.y, b.x, b.y, c.x, c.y, p.x, p.y) && area(p.prev, p, p.next) >= 0) return false;
    p = p.next;
  }
  return true;
}

function intersects(p1: Node, q1: Node, p2: Node, q2: Node): boolean {
  const o1 = Math.sign(area(p1, q1, p2));
  const o2 = Math.sign(area(p1, q1, q2));
  const o3 = Math.sign(area(p2, q2, p1));
  const o4 = Math.sign(area(p2, q2, q1));
  if (o1 !== o2 && o3 !== o4) return true;
  if (o1 === 0 && onSegment(p1, p2, q1)) return true;
  if (o2 === 0 && onSegment(p1, q2, q1)) return true;
  if (o3 === 0 && onSegment(p2, p1, q2)) return true;
  if (o4 === 0 && onSegment(p2, q1, q2)) return true;
  return false;
}

function onSegment(p: Node, q: Node, r: Node): boolean {
  return q.x <= Math.max(p.x, r.x) && q.x >= Math.min(p.x, r.x) && q.y <= Math.max(p.y, r.y) && q.y >= Math.min(p.y, r.y);
}

function locallyInside(a: Node, b: Node): boolean {
  return area(a.prev, a, a.next) < 0 ? area(a, b, a.next) >= 0 && area(a, a.prev, b) >= 0 : area(a, b, a.prev) < 0 || area(a, a.next, b) < 0;
}

function middleInside(a: Node, b: Node): boolean {
  let p = a;
  let inside = false;
  const px = (a.x + b.x) / 2;
  const py = (a.y + b.y) / 2;
  do {
    if (p.y > py !== p.next.y > py && p.next.y !== p.y && px < ((p.next.x - p.x) * (py - p.y)) / (p.next.y - p.y) + p.x) inside = !inside;
    p = p.next;
  } while (p !== a);
  return inside;
}

function intersectsPolygon(a: Node, b: Node): boolean {
  let p = a;
  do {
    if (p.i !== a.i && p.next.i !== a.i && p.i !== b.i && p.next.i !== b.i && intersects(p, p.next, a, b)) return true;
    p = p.next;
  } while (p !== a);
  return false;
}

function isValidDiagonal(a: Node, b: Node): boolean {
  return (
    a.next.i !== b.i &&
    a.prev.i !== b.i &&
    !intersectsPolygon(a, b) &&
    ((locallyInside(a, b) && locallyInside(b, a) && middleInside(a, b) && (area(a.prev, a, b.prev) !== 0 || area(a, b.prev, b) !== 0)) || (equals(a, b) && area(a.prev, a, a.next) > 0 && area(b.prev, b, b.next) > 0))
  );
}

/** Split the ring along the diagonal a-b into two rings (earcut's `splitPolygon`). */
function splitPolygon(a: Node, b: Node): Node {
  const a2 = { i: a.i, x: a.x, y: a.y, prev: null as unknown as Node, next: null as unknown as Node, steiner: false } as Node;
  const b2 = { i: b.i, x: b.x, y: b.y, prev: null as unknown as Node, next: null as unknown as Node, steiner: false } as Node;
  const an = a.next;
  const bp = b.prev;
  a.next = b;
  b.prev = a;
  a2.next = an;
  an.prev = a2;
  b2.next = a2;
  a2.prev = b2;
  bp.next = b2;
  b2.prev = bp;
  return b2;
}

function cureLocalIntersections(start: Node, triangles: number[]): Node {
  let p = start;
  do {
    const a = p.prev;
    const b = p.next.next;
    if (!equals(a, b) && intersects(a, p, p.next, b) && locallyInside(a, b) && locallyInside(b, a)) {
      triangles.push(a.i, p.i, b.i);
      removeNode(p);
      removeNode(p.next);
      p = start = b;
    }
    p = p.next;
  } while (p !== start);
  return filterPoints(p) as Node;
}

function splitEarcut(start: Node, triangles: number[]): void {
  let a = start;
  do {
    let b = a.next.next;
    while (b !== a.prev) {
      if (a.i !== b.i && isValidDiagonal(a, b)) {
        let c = splitPolygon(a, b);
        a = filterPoints(a, a.next) as Node;
        c = filterPoints(c, c.next) as Node;
        earcutLinked(a, triangles, 0);
        earcutLinked(c, triangles, 0);
        return;
      }
      b = b.next;
    }
    a = a.next;
  } while (a !== start);
}

/** `earcutList`: clip ears; when none is left, retry after `filterPoints`, then `cureLocalIntersections`, then split the ring. */
function earcutLinked(first: Node | null, triangles: number[], pass: number): void {
  if (!first) return;
  let ear: Node = first;
  let stop: Node = first;
  while (ear.prev !== ear.next) {
    const prev: Node = ear.prev;
    const next: Node = ear.next;
    if (isEar(ear)) {
      triangles.push(prev.i, ear.i, next.i);
      removeNode(ear);
      ear = next.next;
      stop = next.next;
      continue;
    }
    ear = next;
    if (ear === stop) {
      if (pass === 0) earcutLinked(filterPoints(ear), triangles, 1);
      else if (pass === 1) earcutLinked(cureLocalIntersections(filterPoints(ear) as Node, triangles), triangles, 2);
      else if (pass === 2) splitEarcut(ear, triangles);
      break;
    }
  }
}

function getLeftmost(start: Node): Node {
  let p = start;
  let leftmost = start;
  do {
    if (p.x < leftmost.x || (p.x === leftmost.x && p.y < leftmost.y)) leftmost = p;
    p = p.next;
  } while (p !== start);
  return leftmost;
}

function sectorContainsSector(m: Node, p: Node): boolean {
  return area(m.prev, m, p.prev) < 0 && area(p.next, m, m.next) < 0;
}

/** `findHoleBridge`: the outer vertex a hole's leftmost point can be joined to (David Eberly's algorithm, as in earcut). */
function findHoleBridge(hole: Node, outerNode: Node): Node | null {
  let p = outerNode;
  const hx = hole.x;
  const hy = hole.y;
  let qx = -Infinity;
  let m: Node | null = null;
  do {
    if (hy <= p.y && hy >= p.next.y && p.next.y !== p.y) {
      const x = p.x + ((hy - p.y) * (p.next.x - p.x)) / (p.next.y - p.y);
      if (x <= hx && x > qx) {
        qx = x;
        m = p.x < p.next.x ? p : p.next;
        if (x === hx) return m;
      }
    }
    p = p.next;
  } while (p !== outerNode);
  if (!m) return null;
  const stop = m;
  const mx = m.x;
  const my = m.y;
  let tanMin = Infinity;
  p = m;
  do {
    if (hx >= p.x && p.x >= mx && hx !== p.x && pointInTriangle(hy < my ? hx : qx, hy, mx, my, hy < my ? qx : hx, hy, p.x, p.y)) {
      const tan = Math.abs(hy - p.y) / (hx - p.x);
      if (locallyInside(p, hole) && (tan < tanMin || (tan === tanMin && (p.x > m.x || (p.x === m.x && sectorContainsSector(m, p)))))) {
        m = p;
        tanMin = tan;
      }
    }
    p = p.next;
  } while (p !== stop);
  return m;
}

/** `eliminateHoles`: bridge every hole, leftmost first, into the outer ring so one ring remains. */
function eliminateHoles(holes: Node[], outerNode: Node): Node {
  const queue = holes.map(getLeftmost).sort((a, b) => a.x - b.x);
  for (const hole of queue) {
    const bridge = findHoleBridge(hole, outerNode);
    if (!bridge) continue;
    const bridgeReverse = splitPolygon(bridge, hole);
    filterPoints(bridgeReverse, bridgeReverse.next);
    outerNode = filterPoints(bridge, bridge.next) as Node;
  }
  return outerNode;
}

/**
 * Triangulate `poly` (an outline and its holes): triangles whose union is the outline minus the holes. A ring that does not
 * triangulate (fewer than 3 distinct points, fully collinear) contributes nothing, as in `TesselatePolygon`'s early returns.
 */
export function triangulate(poly: TriPolygon): Triangle[] {
  const all: Pt[] = [...poly.outline, ...poly.holes.flat()];
  const outer = linkedList(poly.outline, 0, true);
  if (!outer || outer.next === outer.prev) return [];
  let start = 0 + poly.outline.length;
  const holeNodes: Node[] = [];
  for (const hole of poly.holes) {
    const ring = linkedList(hole, start, false);
    start += hole.length;
    if (!ring) continue;
    if (ring === ring.next) ring.steiner = true;
    // A hole that collapsed below three vertices is rejected (the `holeRing->prev != holeRing->next` guard).
    if (ring.prev !== ring.next) holeNodes.push(ring);
  }
  const ring = holeNodes.length > 0 ? eliminateHoles(holeNodes, outer) : outer;
  const indexes: number[] = [];
  earcutLinked(ring, indexes, 0);
  const out: Triangle[] = [];
  for (let k = 0; k + 2 < indexes.length; k += 3) out.push([all[indexes[k]!]!, all[indexes[k + 1]!]!, all[indexes[k + 2]!]!]);
  return out;
}
