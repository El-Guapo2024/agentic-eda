import { test } from "node:test";
import assert from "node:assert/strict";
import type { CmdShape, Shape } from "../api/types";
import { arcCenter, toIU } from "./pcbGeom";
import { activeEditPoint, canRemoveCorner, chamferCorner, incrementArcEditMode, moveRingPoint, moveShapePoint, nearestVertex, removeCorner, ringEditPoints, shapeEditPoints } from "./pcbPointEdit";

const base = { id: "s", layer: "F.Fab", stroke_width: 100, filled: false };
const arcShape = (start: [number, number], mid: [number, number], end: [number, number]): Shape => ({ ...base, kind: "arc", start, mid, end });
const pt = (p: { x: number; y: number }): [number, number] => [p.x, p.y];
const center = (a: Extract<CmdShape, { kind: "arc" }>): [number, number] => {
  const c = arcCenter({ start: toIU(pt(a.start)), mid: toIU(pt(a.mid)), end: toIU(pt(a.end)) });
  return [Math.round(c[0] / 1000) + 0, Math.round(c[1] / 1000) + 0];
};
const dist = (a: [number, number], b: [number, number]) => Math.hypot(a[0] - b[0], a[1] - b[1]);

test("edit points: a segment has its ends, a circle its centre and rim point, a curve its four points", () => {
  assert.deepEqual(shapeEditPoints({ ...base, kind: "segment", start: [0, 0], end: [5, 5] }).map((p) => p.id), ["start", "end"]);
  assert.deepEqual(shapeEditPoints({ ...base, kind: "circle", center: [0, 0], end: [5, 0] }).map((p) => p.id), ["center", "end"]);
  assert.deepEqual(shapeEditPoints({ ...base, kind: "bezier", start: [0, 0], c1: [1, 1], c2: [2, 2], end: [3, 3] }).map((p) => p.id), ["start", "c1", "c2", "end"]);
});

test("edit points: a rectangle has four corners, a centre and four edge midpoints", () => {
  const pts = shapeEditPoints({ ...base, kind: "rect", start: [10, 20], end: [0, 0] });
  assert.deepEqual(pts.filter((p) => p.kind === "corner").map((p) => p.id), ["tl", "tr", "br", "bl", "rect-center"]);
  assert.deepEqual(pts.find((p) => p.id === "tl")!.pos, [0, 0], "start and end in any order");
  assert.deepEqual(pts.find((p) => p.id === "edge-top")!.pos, [5, 0]);
  assert.deepEqual(pts.find((p) => p.id === "edge-right")!.pos, [10, 10]);
});

test("edit points: a polygon has its vertices and the midpoint of every edge, the last edge closing the ring", () => {
  const pts = ringEditPoints([[0, 0], [10, 0], [10, 10]]);
  assert.deepEqual(pts.map((p) => p.id), ["v0", "v1", "v2", "m0", "m1", "m2"]);
  assert.deepEqual(pts.find((p) => p.id === "m2")!.pos, [5, 5]);
});

test("the active point is the nearest corner under the pointer, a corner before a midpoint, else a midpoint", () => {
  const pts = ringEditPoints([[0, 0], [1000, 0], [1000, 1000]]);
  assert.equal(activeEditPoint(pts, [20, 10], 100)?.id, "v0");
  assert.equal(activeEditPoint(pts, [500, 30], 100)?.id, "m0");
  assert.equal(activeEditPoint(pts, [500, 300], 100), null);
  // a corner within reach wins over a midpoint that is nearer
  const close = [{ id: "m0", kind: "midpoint" as const, pos: [0, 0] as [number, number] }, { id: "v0", kind: "corner" as const, pos: [50, 0] as [number, number] }];
  assert.equal(activeEditPoint(close, [0, 0], 100)?.id, "v0");
});

test("moving a vertex sets it; moving a midpoint carries both ends of the edge by the same amount", () => {
  const ring: [number, number][] = [[0, 0], [1000, 0], [1000, 1000], [0, 1000]];
  const pts = ringEditPoints(ring);
  assert.deepEqual(moveRingPoint(ring, pts.find((p) => p.id === "v2")!, [1200, 1300]), [[0, 0], [1000, 0], [1200, 1300], [0, 1000]]);
  // the midpoint of the right edge (v1-v2) is (1000, 500): drag it to (1300, 500)
  assert.deepEqual(moveRingPoint(ring, pts.find((p) => p.id === "m1")!, [1300, 500]), [[0, 0], [1300, 0], [1300, 1000], [0, 1000]]);
  // the closing edge (v3-v0)
  assert.deepEqual(moveRingPoint(ring, pts.find((p) => p.id === "m3")!, [-200, 500]), [[-200, 0], [1000, 0], [1000, 1000], [-200, 1000]]);
});

test("Remove Corner: a polygon keeps at least three corners", () => {
  const square: [number, number][] = [[0, 0], [10, 0], [10, 10], [0, 10]];
  assert.ok(canRemoveCorner(square));
  assert.deepEqual(removeCorner(square, 1), [[0, 0], [10, 10], [0, 10]]);
  assert.ok(!canRemoveCorner([[0, 0], [10, 0], [0, 10]]));
  assert.equal(removeCorner([[0, 0], [10, 0], [0, 10]], 0), null);
  assert.equal(removeCorner(square, 9), null);
});

test("Chamfer Corner: the vertex becomes two points a quarter of the shorter edge away (at most 5 mm)", () => {
  const square: [number, number][] = [[0, 0], [10000, 0], [10000, 10000], [0, 10000]];
  assert.deepEqual(chamferCorner(square, 1), [[0, 0], [7500, 0], [10000, 2500], [10000, 10000], [0, 10000]]);
  // long edges: the 5 mm cap applies
  const big: [number, number][] = [[0, 0], [100000, 0], [100000, 100000], [0, 100000]];
  assert.deepEqual(chamferCorner(big, 2), [[0, 0], [100000, 0], [100000, 95000], [95000, 100000], [0, 100000]]);
  // the first vertex wraps to the last edge
  assert.deepEqual(chamferCorner(square, 0), [[0, 2500], [2500, 0], [10000, 0], [10000, 10000], [0, 10000]]);
  assert.equal(nearestVertex(square, [9000, 800]), 1);
});

test("rectangles: a corner pins its neighbours, an edge moves one side, the centre moves the rectangle", () => {
  const r: Shape = { ...base, kind: "rect", start: [0, 0], end: [4000, 3000] };
  const pts = shapeEditPoints(r);
  const get = (id: string) => pts.find((p) => p.id === id)!;
  assert.deepEqual(moveShapePoint(r, get("br"), [5000, 4000]), { kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: { x: 0, y: 0 }, end: { x: 5000, y: 4000 } });
  assert.deepEqual(moveShapePoint(r, get("tl"), [-500, -200]), { kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: { x: -500, y: -200 }, end: { x: 4000, y: 3000 } });
  assert.deepEqual(moveShapePoint(r, get("edge-right"), [4600, 777]), { kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: { x: 0, y: 0 }, end: { x: 4600, y: 3000 } });
  assert.deepEqual(moveShapePoint(r, get("edge-top"), [123, 400]), { kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: { x: 0, y: 400 }, end: { x: 4000, y: 3000 } });
  assert.deepEqual(moveShapePoint(r, get("rect-center"), [2500, 2000]), { kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: { x: 500, y: 500 }, end: { x: 4500, y: 3500 } });
  // it cannot cross over the opposite corner: 1 mil stays
  const tight = moveShapePoint(r, get("br"), [-100, -100]) as Extract<CmdShape, { kind: "rect" }>;
  assert.deepEqual([tight.start, tight.end], [{ x: 0, y: 0 }, { x: 26, y: 26 }]);
});

test("segments, circles and curves move the one point", () => {
  const s: Shape = { ...base, kind: "segment", start: [0, 0], end: [100, 0] };
  assert.deepEqual(moveShapePoint(s, shapeEditPoints(s)[1]!, [100, 50]), { kind: "segment", layer: "F.Fab", stroke_width: 100, filled: false, start: { x: 0, y: 0 }, end: { x: 100, y: 50 } });
  const c: Shape = { ...base, kind: "circle", center: [0, 0], end: [500, 0] };
  assert.deepEqual(moveShapePoint(c, shapeEditPoints(c)[0]!, [100, 100]), { kind: "circle", layer: "F.Fab", stroke_width: 100, filled: false, center: { x: 100, y: 100 }, end: { x: 500, y: 0 } });
  const b: Shape = { ...base, kind: "bezier", start: [0, 0], c1: [10, 20], c2: [30, 40], end: [50, 0] };
  const moved = moveShapePoint(b, shapeEditPoints(b)[2]!, [35, 45]) as Extract<CmdShape, { kind: "bezier" }>;
  assert.deepEqual([moved.c1, moved.c2], [{ x: 10, y: 20 }, { x: 35, y: 45 }]);
});

// A half circle of radius 10 mm about the origin, from (10, 0) through (0, -10) to (-10, 0).
const half = arcShape([10000, 0], [0, -10000], [-10000, 0]);
const arcPoint = (id: string) => shapeEditPoints(half).find((p) => p.id === id)!;

test("arc edit points: its ends, its mid point and its centre", () => {
  assert.deepEqual(shapeEditPoints(half).map((p) => [p.id, p.pos]), [["start", [10000, 0]], ["mid", [0, -10000]], ["end", [-10000, 0]], ["center", [0, 0]]]);
});

test("arc, keep centre / adjust radius: moving an end sets the radius and drags the other end to it", () => {
  const out = moveShapePoint(half, arcPoint("start"), [20000, 0], "keep_center_adjust_angle_radius") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual([pt(out.start), pt(out.mid), pt(out.end)], [[20000, 0], [0, -20000], [-20000, 0]]);
  assert.deepEqual(center(out), [0, 0]);
});

test("arc, keep radius and centre / adjust angle: an end slides round the circle", () => {
  const out = moveShapePoint(half, arcPoint("start"), [0, 10000], "keep_center_ends_adjust_angle") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual(pt(out.start), [0, 10000]);
  assert.deepEqual(pt(out.end), [-10000, 0]);
  assert.deepEqual(center(out), [0, 0]);
  assert.ok(Math.abs(dist(pt(out.mid), [0, 0]) - 10000) < 2, "the mid point stays on the circle");
  // an off-circle target is brought onto the circle
  const far = moveShapePoint(half, arcPoint("start"), [0, 30000], "keep_center_ends_adjust_angle") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual(pt(far.start), [0, 10000]);
});

test("arc, keep endpoints: moving the mid point flattens the arc between the same ends", () => {
  const out = moveShapePoint(half, arcPoint("mid"), [0, -5000], "keep_endpoints_or_start_direction") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual([pt(out.start), pt(out.mid), pt(out.end)], [[10000, 0], [0, -5000], [-10000, 0]]);
  // it cannot be pushed through the chord: "we do not allow arc inflection"
  const flipped = moveShapePoint(half, arcPoint("mid"), [0, 8000], "keep_endpoints_or_start_direction") as Extract<CmdShape, { kind: "arc" }>;
  assert.ok(flipped.mid.y < 0, `mid ${flipped.mid.y} stays on its side of the chord`);
});

test("arc, keep centre: moving the mid point sets the radius from the pointer", () => {
  const out = moveShapePoint(half, arcPoint("mid"), [0, -15000], "keep_center_adjust_angle_radius") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual([pt(out.start), pt(out.end)], [[15000, 0], [-15000, 0]]);
  assert.ok(Math.abs(dist(pt(out.mid), [0, 0]) - 15000) < 2);
});

test("arc, keep endpoints: the centre moves along the perpendicular bisector of the chord", () => {
  const out = moveShapePoint(half, arcPoint("center"), [0, 3000], "keep_endpoints_or_start_direction") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual([pt(out.start), pt(out.end)], [[10000, 0], [-10000, 0]]);
  assert.deepEqual(center(out), [0, 3000]);
});

test("arc, the other modes: moving the centre moves the arc", () => {
  const out = moveShapePoint(half, arcPoint("center"), [500, 700], "keep_center_adjust_angle_radius") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual([pt(out.start), pt(out.end)], [[10500, 700], [-9500, 700]]);
  assert.deepEqual(center(out), [500, 700]);
});

test("arc, keep tangent: an end moves while the circle stays tangent to the line it started along", () => {
  // quarter circle about (0, 10 mm): from (0, 0) -- tangent along x -- through (7.071, 2.929) to (10, 10)
  const quarter = arcShape([0, 0], [7071, 2929], [10000, 10000]);
  const end = shapeEditPoints(quarter).find((p) => p.id === "end")!;
  const out = moveShapePoint(quarter, end, [10000, 20000], "keep_endpoints_or_start_direction") as Extract<CmdShape, { kind: "arc" }>;
  assert.deepEqual(pt(out.start), [0, 0]);
  assert.deepEqual(pt(out.end), [10000, 20000]);
  const c = center(out);
  assert.ok(Math.abs(c[0]) <= 1 && Math.abs(c[1] - 12500) <= 1, `centre ${c}`);
});

test("the arc editing modes cycle in KiCad's order", () => {
  assert.equal(incrementArcEditMode("keep_center_adjust_angle_radius"), "keep_center_ends_adjust_angle");
  assert.equal(incrementArcEditMode("keep_center_ends_adjust_angle"), "keep_endpoints_or_start_direction");
  assert.equal(incrementArcEditMode("keep_endpoints_or_start_direction"), "keep_center_adjust_angle_radius");
});
