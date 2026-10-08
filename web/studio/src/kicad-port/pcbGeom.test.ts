import { test } from "node:test";
import assert from "node:assert/strict";
import {
  arcCenter,
  arcCentralAngle,
  arcFromFillet,
  arcIsCCW,
  arcRadius,
  circleIntersectSeg,
  computeChamferPoints,
  computeDogbone,
  halfLineIntersect,
  kiRound,
  norm,
  resize,
  rotatePoint,
  segAngle,
  segApproxCollinear,
  segContains,
  segIntersect,
  segIntersectLines,
  segLineProject,
  segNearestPoint,
  segsParallel,
  type Seg,
  type V,
} from "./pcbGeom";

/** mm -> IU (1 nm). */
const mm = (v: number): number => Math.round(v * 1_000_000);
const seg = (ax: number, ay: number, bx: number, by: number): Seg => ({ a: [ax, ay], b: [bx, by] });
const near = (actual: readonly [number, number], expected: readonly [number, number], tol = 2): void => {
  assert.ok(Math.abs(actual[0] - expected[0]) <= tol && Math.abs(actual[1] - expected[1]) <= tol, `expected ~[${expected}] got [${actual}]`);
};

test("kiRound rounds half away from zero", () => {
  assert.equal(kiRound(2.5), 3);
  assert.equal(kiRound(-2.5), -3);
  assert.equal(kiRound(2.4), 2);
  assert.equal(kiRound(-2.4), -2);
});

test("norm: exact on the axes, |x|*sqrt2 on a 45 line, rounded hypot otherwise (VECTOR2I::EuclideanNorm)", () => {
  assert.equal(norm([3, 0]), 3);
  assert.equal(norm([0, -4]), 4);
  assert.equal(norm([10, 10]), 14);
  assert.equal(norm([3, 4]), 5);
  assert.equal(norm([1, 2]), 2);
});

test("resize keeps direction and rounds each component (VECTOR2I::Resize)", () => {
  assert.deepEqual(resize([3, 4], 10), [6, 8]);
  assert.deepEqual(resize([-3, 4], 10), [-6, 8]);
  assert.deepEqual(resize([0, 0], 10), [0, 0]);
  assert.deepEqual(resize([10, 10], 100), [71, 71]);
  assert.deepEqual(resize([3, 4], -10), [-6, -8]);
});

test("rotatePoint is counter-clockwise on screen for positive angles (RotatePoint)", () => {
  assert.deepEqual(rotatePoint([10, 0], [0, 0], 90), [0, -10]);
  assert.deepEqual(rotatePoint([10, 0], [0, 0], 180), [-10, 0]);
  assert.deepEqual(rotatePoint([10, 0], [0, 0], 270), [0, 10]);
  assert.deepEqual(rotatePoint([10, 0], [5, 0], 90), [5, -5]);
  near(rotatePoint([1000, 0], [0, 0], 45), [707, -707], 1);
});

test("segIntersect: crossing segments, endpoint touches, parallel and collinear cases (SEG::intersects)", () => {
  assert.deepEqual(segIntersect(seg(0, 0, 10, 10), seg(0, 10, 10, 0)), [5, 5]);
  assert.equal(segIntersect(seg(0, 0, 10, 0), seg(0, 5, 10, 5)), null); // parallel
  assert.equal(segIntersect(seg(0, 0, 10, 0), seg(20, -5, 20, 5)), null); // lines would meet, segments do not
  assert.deepEqual(segIntersect(seg(0, 0, 10, 0), seg(20, -5, 20, 5), false, true), [20, 0]);
  // touching only at a shared endpoint
  assert.deepEqual(segIntersect(seg(0, 0, 10, 0), seg(10, 0, 10, 10)), [10, 0]);
  assert.equal(segIntersect(seg(0, 0, 10, 0), seg(10, 0, 10, 10), true), null);
  // collinear overlap: the midpoint of the overlapping run
  assert.deepEqual(segIntersect(seg(0, 0, 10, 0), seg(6, 0, 20, 0)), [8, 0]);
  assert.equal(segIntersect(seg(0, 0, 10, 0), seg(11, 0, 20, 0)), null);
  assert.deepEqual(segIntersectLines(seg(0, 0, 10, 0), seg(5, 5, 5, 9)), [5, 0]);
});

test("segNearestPoint / segLineProject / segContains", () => {
  const s = seg(0, 0, 100, 0);
  assert.deepEqual(segNearestPoint(s, [50, 30]), [50, 0]);
  assert.deepEqual(segNearestPoint(s, [-20, 5]), [0, 0]);
  assert.deepEqual(segNearestPoint(s, [150, 5]), [100, 0]);
  assert.deepEqual(segLineProject(s, [150, 5]), [150, 0]);
  assert.equal(segContains(s, [50, 1]), true);
  assert.equal(segContains(s, [50, 2]), false);
});

test("segAngle is the absolute angle between the lines and parallel means exactly 0/180", () => {
  assert.equal(segAngle(seg(0, 0, 10, 0), seg(0, 0, 0, 10)), 90);
  assert.equal(segsParallel(seg(0, 0, 10, 0), seg(0, 5, 20, 5)), true);
  assert.equal(segsParallel(seg(0, 0, 10, 0), seg(0, 5, -20, 5)), true);
  assert.equal(segsParallel(seg(0, 0, 10, 0), seg(0, 0, 10, 1)), false);
});

test("segApproxCollinear: both end points of the other segment within 1 IU of this line", () => {
  assert.equal(segApproxCollinear(seg(0, 0, 100, 0), seg(120, 0, 200, 0)), true);
  assert.equal(segApproxCollinear(seg(0, 0, 100, 0), seg(120, 5, 200, 5)), false);
});

test("halfLineIntersect: the ray meets the segment only on its forward side", () => {
  assert.deepEqual(halfLineIntersect([0, 0], [10, 0], seg(20, -5, 20, 5)), [20, 0]);
  assert.equal(halfLineIntersect([0, 0], [-10, 0], seg(20, -5, 20, 5)), null);
});

test("circleIntersectSeg: two crossings, one tangent, none", () => {
  const hits = circleIntersectSeg([0, 0], 1000, seg(-2000, 0, 2000, 0));
  assert.equal(hits.length, 2);
  assert.ok(hits.some((h) => Math.abs(h[0] - 1000) <= 1) && hits.some((h) => Math.abs(h[0] + 1000) <= 1));
  assert.equal(circleIntersectSeg([0, 0], 1000, seg(-2000, 1000, 2000, 1000)).length, 1); // tangent
  assert.equal(circleIntersectSeg([0, 0], 1000, seg(-2000, 2000, 2000, 2000)).length, 0);
  // the chord must lie on the segment itself
  assert.equal(circleIntersectSeg([0, 0], 1000, seg(2000, 0, 3000, 0)).length, 0);
});

test("arcFromFillet: the radius-1mm arc tangent to the two legs of a right angle (SHAPE_ARC(SEG, SEG, radius))", () => {
  const arc = arcFromFillet(seg(0, 0, mm(10), 0), seg(0, 0, 0, mm(10)), mm(1))!;
  near(arc.start, [mm(1), 0]);
  near(arc.end, [0, mm(1)]);
  // the mid point sits on the circle, nearest the corner
  near(arc.mid, [Math.round(mm(1) * (1 - Math.SQRT1_2)), Math.round(mm(1) * (1 - Math.SQRT1_2))]);
  near(arcCenter(arc), [mm(1), mm(1)], 3);
  assert.ok(Math.abs(arcRadius(arc) - mm(1)) < 5);
  assert.ok(Math.abs(Math.abs(arcCentralAngle(arc)) - 90) < 0.01);
});

test("arcFromFillet refuses lines that never meet or a zero-length line", () => {
  assert.equal(arcFromFillet(seg(0, 0, 10, 0), seg(0, 5, 10, 5), 3), null);
  assert.equal(arcFromFillet(seg(0, 0, 0, 0), seg(0, 0, 0, 10), 3), null);
});

test("arcFromFillet works when the legs are given in either direction", () => {
  const a = arcFromFillet(seg(mm(10), 0, 0, 0), seg(0, mm(10), 0, 0), mm(1))!;
  near(a.start, [mm(1), 0]);
  near(a.end, [0, mm(1)]);
});

test("arcIsCCW / arcCentralAngle agree for a quarter circle both ways round", () => {
  const ccw = { start: [1000, 0] as V, mid: [707, -707] as V, end: [0, -1000] as V };
  assert.equal(arcIsCCW(ccw), false);
  assert.ok(Math.abs(arcCentralAngle(ccw)) < 90.1 && Math.abs(arcCentralAngle(ccw)) > 89.9);
  const full = { start: [1000, 0] as V, mid: [-1000, 0] as V, end: [1000, 0] as V };
  assert.equal(arcCentralAngle(full), 360);
});

test("computeChamferPoints: setback along each leg, the chord between, and the shortened legs (ComputeChamferPoints)", () => {
  const r = computeChamferPoints(seg(0, 0, mm(10), 0), seg(0, 0, 0, mm(10)), mm(2), mm(2))!;
  near(r.chamfer.a, [mm(2), 0]);
  near(r.chamfer.b, [0, mm(2)]);
  near(r.updatedA!.a, [mm(10), 0]);
  near(r.updatedA!.b, [mm(2), 0]);
  near(r.updatedB!.a, [0, mm(10)]);
  near(r.updatedB!.b, [0, mm(2)]);
});

test("computeChamferPoints: a setback eating a whole leg removes it; too long or zero refuses", () => {
  const r = computeChamferPoints(seg(0, 0, mm(2), 0), seg(0, 0, 0, mm(10)), mm(2), mm(2))!;
  assert.equal(r.updatedA, null);
  assert.ok(r.updatedB);
  assert.equal(computeChamferPoints(seg(0, 0, mm(1), 0), seg(0, 0, 0, mm(10)), mm(2), mm(2)), null);
  assert.equal(computeChamferPoints(seg(0, 0, mm(10), 0), seg(0, 0, 0, mm(10)), 0, 0), null);
  assert.equal(computeChamferPoints(seg(0, 0, mm(10), 0), seg(5, 5, 5, mm(10)), mm(2), mm(2)), null); // no shared corner
});

test("computeDogbone: a right-angle corner gets a semicircle whose chord is a diameter (ComputeDogbone)", () => {
  const r = computeDogbone(seg(0, 0, mm(10), 0), seg(0, 0, 0, mm(10)), mm(1), true)!;
  const reach = Math.round(mm(1) * Math.SQRT2);
  near(r.arc.start, [reach, 0], 20);
  near(r.arc.end, [0, reach], 20);
  assert.equal(r.smallArcMouth, false);
  assert.ok(Math.abs(Math.abs(arcCentralAngle(r.arc)) - 180) < 0.5);
  near(r.updatedA!.b, [reach, 0], 20);
  near(r.updatedB!.b, [0, reach], 20);
});

test("computeDogbone: an acute corner is a narrow mouth; slots pull the arc back to a half circle", () => {
  // 60 degree corner at the origin
  const a = seg(0, 0, mm(10), 0);
  const b = seg(0, 0, Math.round(mm(10) * Math.cos(Math.PI / 3)), Math.round(mm(10) * Math.sin(Math.PI / 3)));
  const noSlots = computeDogbone(a, b, mm(1), false)!;
  assert.equal(noSlots.smallArcMouth, true);
  assert.ok(Math.abs(arcCentralAngle(noSlots.arc)) > 180);
  const slots = computeDogbone(a, b, mm(1), true)!;
  assert.equal(slots.smallArcMouth, true);
  assert.ok(Math.abs(Math.abs(arcCentralAngle(slots.arc)) - 180) < 0.5);
});

test("computeDogbone refuses parallel legs and a missing corner", () => {
  assert.equal(computeDogbone(seg(0, 0, mm(10), 0), seg(0, 0, mm(-10), 0), mm(1), true), null);
  assert.equal(computeDogbone(seg(0, 0, mm(10), 0), seg(5, 5, 5, mm(10)), mm(1), true), null);
});
