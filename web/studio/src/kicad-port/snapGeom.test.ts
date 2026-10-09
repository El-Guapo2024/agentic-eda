import { test } from "node:test";
import assert from "node:assert/strict";
import {
  OVAL,
  PT,
  arcContainsAngle,
  arcEnd,
  arcStart,
  arcThrough,
  box,
  boxToSegs,
  circle,
  circleKeyPoints,
  clipHalfLineToBox,
  clipLineToBox,
  distanceTo,
  half,
  intersections,
  line,
  nearestOfAny,
  nearestPoint,
  ovalKeyPoints,
  polylineNearest,
  seg,
  type Pt,
} from "./snapGeom";

const near = (a: Pt | undefined, b: Pt, tol = 1e-6, msg = "") => {
  assert.ok(a !== undefined, `${msg} no point`);
  assert.ok(Math.abs(a![0] - b[0]) <= tol && Math.abs(a![1] - b[1]) <= tol, `${msg} expected (${b}) got (${a})`);
};

test("two segments meet where they cross, endpoints included; parallel ones never do", () => {
  near(intersections(seg([0, 0], [10, 10]), seg([0, 10], [10, 0]))[0], [5, 5]);
  near(intersections(seg([0, 0], [10, 0]), seg([10, 0], [10, 10]))[0], [10, 0], 1e-6, "shared end");
  assert.equal(intersections(seg([0, 0], [10, 0]), seg([0, 1], [10, 1])).length, 0);
  assert.equal(intersections(seg([0, 0], [10, 0]), seg([5, 0], [15, 0])).length, 0, "collinear overlap: parallel, like SEG::Intersect");
  assert.equal(intersections(seg([0, 0], [4, 4]), seg([0, 10], [10, 0])).length, 0, "they would cross past the end of the first");
});

test("a segment that ends short of a line does not meet it, a line does meet it", () => {
  assert.equal(intersections(seg([0, 0], [4, 4]), line([0, 10], [10, 0])).length, 0);
  near(intersections(line([0, 0], [4, 4]), line([0, 10], [10, 0]))[0], [5, 5]);
});

test("a ray meets only what is ahead of its start", () => {
  assert.equal(intersections(half([0, 0], [1, 0]), seg([-5, -5], [-5, 5])).length, 0, "behind");
  near(intersections(half([0, 0], [1, 0]), seg([5, -5], [5, 5]))[0], [5, 0]);
  near(intersections(half([0, 0], [1, 0]), half([5, 5], [5, 4]))[0], [5, 0]);
  assert.equal(intersections(half([0, 0], [1, 0]), half([5, 5], [5, 6])).length, 0, "the other ray points away");
  near(intersections(half([0, 0], [1, 0]), line([7, -3], [7, 3]))[0], [7, 0]);
});

test("a circle meets a segment where it is inside the segment, and a line anywhere", () => {
  const c = circle([0, 0], 10);
  const hits = intersections(c, seg([-20, 0], [0, 0]));
  assert.equal(hits.length, 1);
  near(hits[0], [-10, 0]);
  assert.equal(intersections(c, line([-20, 0], [20, 0])).length, 2);
  assert.equal(intersections(c, seg([-20, 11], [20, 11])).length, 0);
  const tangent = intersections(c, line([-20, 10], [20, 10]));
  assert.equal(tangent.length, 1, "a tangent touches once");
  near(tangent[0], [0, 10], 1e-3);
});

test("two circles meet in two points, one (tangent) or none", () => {
  const two = intersections(circle([0, 0], 10), circle([10, 0], 10));
  assert.equal(two.length, 2);
  near(two[0], [5, 8.660254], 1e-4);
  assert.equal(intersections(circle([0, 0], 10), circle([20, 0], 10)).length, 1);
  assert.equal(intersections(circle([0, 0], 10), circle([30, 0], 10)).length, 0);
  assert.equal(intersections(circle([0, 0], 10), circle([0, 0], 5)).length, 0, "concentric");
});

test("an arc through three points keeps its sweep and meets only what lies on it", () => {
  // Quarter arc from (10, 0) through its middle to (0, 10) on a circle about the origin (y down: from east, towards south).
  const a = arcThrough([10, 0], [Math.SQRT1_2 * 10, Math.SQRT1_2 * 10], [0, 10])!;
  near(a.c, [0, 0], 1e-9);
  assert.ok(Math.abs(a.r - 10) < 1e-9);
  near(arcStart(a), [10, 0], 1e-9);
  near(arcEnd(a), [0, 10], 1e-9);
  assert.ok(a.da > 0 && Math.abs(a.da - Math.PI / 2) < 1e-9);
  assert.ok(arcContainsAngle(a, Math.PI / 4));
  assert.ok(!arcContainsAngle(a, -Math.PI / 4));
  // The line y = x meets the circle twice; only the point in the quarter counts.
  const hits = intersections(a, line([-5, -5], [5, 5]));
  assert.equal(hits.length, 1);
  near(hits[0], [Math.SQRT1_2 * 10, Math.SQRT1_2 * 10], 1e-9);
  // The other way round: the three points name which of the two arcs it is.
  const long = arcThrough([10, 0], [-10, 0], [0, 10])!;
  assert.ok(Math.abs(long.da) > Math.PI, "the long way round, through (-10, 0)");
  assert.equal(arcThrough([0, 0], [1, 1], [2, 2]), null, "collinear points are no arc");
  // Arc against circle: the circle about (10, 10) passes through both ends of the arc. Against a circle that cuts the arc's circle only on the other side, none.
  assert.equal(intersections(a, circle([10, 10], 10)).length, 2);
  assert.equal(intersections(a, circle([-10, -10], 10)).length, 0, "its two crossing points are at angles 180 and 270 degrees, off the sweep");
  // Arc against arc: another quarter arc about (10, 10) crosses it at its two ends only.
  const b = arcThrough([0, 10], [10 - Math.SQRT1_2 * 10, 10 - Math.SQRT1_2 * 10], [10, 0])!;
  assert.equal(intersections(a, b).length, 2);
});

test("a box is its four sides", () => {
  const b = box(0, 0, 10, 6);
  assert.equal(boxToSegs(b).length, 4);
  const hits = intersections(b, line([5, -10], [5, 10]));
  assert.equal(hits.length, 2);
  assert.equal(intersections(b, circle([5, 3], 1)).length, 0);
  assert.equal(intersections(circle([5, 3], 2), b).length, 0);
  assert.equal(intersections(b, seg([-5, 3], [20, 3])).length, 2);
});

test("the nearest point of each kind of geometry", () => {
  near(nearestPoint(seg([0, 0], [10, 0]), [4, 3]), [4, 0]);
  near(nearestPoint(seg([0, 0], [10, 0]), [-4, 3]), [0, 0]);
  near(nearestPoint(line([0, 0], [10, 0]), [-4, 3]), [-4, 0]);
  near(nearestPoint(half([0, 0], [10, 0]), [-4, 3]), [0, 0]);
  near(nearestPoint(circle([0, 0], 10), [20, 0]), [10, 0]);
  near(nearestPoint(circle([0, 0], 10), [0, 0]), [10, 0], 1e-9, "the centre has no nearest point: east, by convention");
  const a = arcThrough([10, 0], [Math.SQRT1_2 * 10, Math.SQRT1_2 * 10], [0, 10])!;
  near(nearestPoint(a, [20, 20]), [Math.SQRT1_2 * 10, Math.SQRT1_2 * 10], 1e-9);
  near(nearestPoint(a, [20, -5]), [10, 0], 1e-9, "off the sweep: the nearer end");
  near(nearestPoint(box(0, 0, 10, 10), [20, 5]), [10, 5]);
  near(nearestPoint(box(0, 0, 10, 10), [5, 4]), [5, 0], 1e-9, "inside: the nearest side");
  assert.equal(distanceTo(seg([0, 0], [10, 0]), [4, 3]), 3);
  near(nearestOfAny([seg([0, 0], [10, 0]), circle([100, 0], 5)], [97, 0])!, [95, 0]);
  assert.equal(nearestOfAny([], [0, 0]), null);
  near(polylineNearest([[0, 0], [10, 0], [10, 10]], false, [4, 3]), [4, 0]);
  near(polylineNearest([[0, 0], [10, 0], [10, 10]], true, [2, 8]), [5, 5], 1e-9, "closed: the diagonal back to the start counts");
});

test("clipping a line or a ray to a box", () => {
  const b = box(0, 0, 100, 100);
  const l = clipLineToBox(line([10, 50], [20, 50]), b)!;
  near(l[0], [0, 50]);
  near(l[1], [100, 50]);
  assert.equal(clipLineToBox(line([0, 200], [10, 200]), b), null);
  const r = clipHalfLineToBox(half([10, 50], [20, 50]), b)!;
  near(r[0], [10, 50]);
  near(r[1], [100, 50]);
  assert.equal(clipHalfLineToBox(half([200, 50], [300, 50]), b), null, "ray pointing away from the box");
});

test("a circle's key points are its four quadrants, and its centre when asked", () => {
  const pts = circleKeyPoints([10, 20], 5, false);
  assert.equal(pts.length, 4);
  assert.ok(pts.every((p) => p.types === PT.QUADRANT));
  near(pts[0]!.pt, [10, 25]);
  near(pts[1]!.pt, [15, 20]);
  const withCentre = circleKeyPoints([10, 20], 5, true);
  assert.equal(withCentre.length, 5);
  assert.equal(withCentre[0]!.types, PT.CENTER);
});

test("an oval's key points: centre, side middles, tips (quadrants when square-on) and no extra extremes", () => {
  const flags = OVAL.CENTER | OVAL.CAP_TIPS | OVAL.SIDE_MIDPOINTS | OVAL.CARDINAL_EXTREMES;
  // A wide pad, 4 x 2: tips on the x axis at +-2, side middles at y = +-1.
  const wide = ovalKeyPoints([100, 100], 4, 2, 0, flags);
  assert.equal(wide.length, 5);
  const byType = (t: number) => wide.filter((p) => p.types === t).map((p) => p.pt);
  assert.equal(byType(PT.CENTER).length, 1);
  near(byType(PT.CENTER)[0]!, [100, 100]);
  const tips = byType(PT.QUADRANT).sort((a, b) => a[0] - b[0]);
  assert.equal(tips.length, 2);
  near(tips[0]!, [98, 100], 1e-9);
  near(tips[1]!, [102, 100], 1e-9);
  const mids = byType(PT.MID).sort((a, b) => a[1] - b[1]);
  near(mids[0]!, [100, 99], 1e-9);
  near(mids[1]!, [100, 101], 1e-9);
  // A tall pad, 2 x 4: the tips are on the y axis.
  const tall = ovalKeyPoints([0, 0], 2, 4, 0, flags);
  const tallTips = tall.filter((p) => p.types === PT.QUADRANT).map((p) => p.pt).sort((a, b) => a[1] - b[1]);
  near(tallTips[0]!, [0, -2], 1e-9);
  near(tallTips[1]!, [0, 2], 1e-9);
  // Turned 45 degrees it has real cap ends and four quadrant points beyond the centre, tips and side middles.
  const turned = ovalKeyPoints([0, 0], 8, 2, Math.PI / 4, flags);
  assert.equal(turned.filter((p) => p.types === PT.END).length, 2);
  assert.equal(turned.filter((p) => p.types === PT.QUADRANT).length, 4);
});

test("an oval's cap centres and side ends are asked for by flag", () => {
  const pts = ovalKeyPoints([0, 0], 10, 4, 0, OVAL.CAP_CENTERS | OVAL.SIDE_ENDS);
  assert.equal(pts.filter((p) => p.types === PT.CENTER).length, 2);
  assert.equal(pts.filter((p) => p.types === PT.END).length, 4);
  const centres = pts.filter((p) => p.types === PT.CENTER).map((p) => p.pt).sort((a, b) => a[0] - b[0]);
  near(centres[0]!, [-3, 0], 1e-9);
  near(centres[1]!, [3, 0], 1e-9);
});
