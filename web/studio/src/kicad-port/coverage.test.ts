import { test } from "node:test";
import assert from "node:assert/strict";
import { footprintHull, padRing, shapeCoverageArea, shapeRings } from "./footprintCoverage";
import type { Pad, Part, Shape } from "../api/types";
import { ARC_LOW_DEF_NM, boundingHull, circleRing, circleSegments, convexHull, coverageRatio, intersectionArea, rectRing, ringArea, stadiumRing, type Ring } from "./coverage";

const sq = (x0: number, y0: number, x1: number, y1: number): Ring => [
  [x0, y0],
  [x1, y0],
  [x1, y1],
  [x0, y1],
];
const near = (a: number, b: number, tol: number) => assert.ok(Math.abs(a - b) <= tol, `${a} is not within ${tol} of ${b}`);

test("ringArea: the shoelace area, whichever way round", () => {
  assert.equal(ringArea(sq(0, 0, 10, 4)), 40);
  assert.equal(ringArea([...sq(0, 0, 10, 4)].reverse()), 40);
  assert.equal(ringArea([[0, 0], [1, 1]]), 0);
});

test("intersectionArea: the union of overlapping polygons counts the overlap once (non-zero), and a clip keeps only what is inside it", () => {
  assert.equal(intersectionArea([sq(0, 0, 10, 10)]), 100);
  assert.equal(intersectionArea([sq(0, 0, 10, 10), sq(5, 5, 15, 15)]), 175);
  // the same polygon twice, one of them the other way round: each ring is a filled outline, so still covered once
  assert.equal(intersectionArea([sq(0, 0, 10, 10), [...sq(0, 0, 10, 10)].reverse()]), 100);
  assert.equal(intersectionArea([sq(0, 0, 10, 10), sq(5, 5, 15, 15)], [sq(0, 0, 8, 8)]), 64);
  assert.equal(intersectionArea([sq(0, 0, 10, 10)], [sq(20, 20, 30, 30)]), 0);
  assert.equal(intersectionArea([], [sq(0, 0, 1, 1)]), 0);
});

test("intersectionArea: edges that cross inside a slab are found (an X of two bars)", () => {
  const a: Ring = [[0, 0], [2, 0], [12, 10], [10, 10]];
  const b: Ring = [[10, 0], [12, 0], [2, 10], [0, 10]];
  const each = ringArea(a);
  const union = intersectionArea([a, b]);
  const both = intersectionArea([a], [b]);
  near(union + both, 2 * each, 1e-9);
  assert.ok(both > 0 && both < each);
  // the overlap: x - y in [0, 2] and x + y in [10, 12], a square of side sqrt(2) -- area 2
  near(both, 2, 1e-9);
});

test("circleRing: chords no further than ARC_LOW_DEF from the circle, outside it", () => {
  const r = 1_000_000;
  const ring = circleRing(0, 0, r);
  assert.equal(ring.length, circleSegments(r));
  for (const [x, y] of ring) assert.ok(Math.hypot(x, y) >= r);
  const area = ringArea(ring);
  assert.ok(area >= Math.PI * r * r && area <= Math.PI * (r + ARC_LOW_DEF_NM) ** 2);
  assert.equal(circleSegments(10), 8, "a tiny circle is an octagon");
});

test("rectRing and stadiumRing: a turned rectangle keeps its area; a stadium is a rectangle and a circle", () => {
  near(ringArea(rectRing(5, 5, 4, 2, 30)), 8, 1e-9);
  const w = 200_000;
  const st = stadiumRing([0, 0], [1_000_000, 0], w);
  const area = ringArea(st);
  assert.ok(area >= 1_000_000 * w + Math.PI * (w / 2) ** 2 * 0.9 && area <= 1_000_000 * w + Math.PI * (w / 2) ** 2 * 1.2, `${area}`);
});

test("convexHull (BuildConvexHull): the hull's corners, counter-clockwise from the lowest x, inner and collinear points dropped", () => {
  const hull = convexHull([[0, 0], [10, 0], [5, 5], [10, 10], [0, 10], [5, 0], [3, 3]]);
  assert.deepEqual(hull, [[0, 0], [10, 0], [10, 10], [0, 10]]);
  assert.deepEqual(convexHull([[1, 1]]), []);
});

test("boundingHull: the hull of the footprint's shapes; with nothing to hull, a 2 mm square on its anchor", () => {
  assert.equal(ringArea(boundingHull([sq(0, 0, 4, 2), sq(6, 0, 10, 2)], [5, 1])), 20);
  assert.equal(ringArea(boundingHull([], [0, 0])), 4e12);
});

test("coverageRatio (FOOTPRINT::CoverageRatio): the share of the hull covered, an overlap counted once, nothing outside the hull, a hull of no area covered", () => {
  const hull = sq(0, 0, 10, 10);
  assert.equal(coverageRatio(hull, [sq(0, 0, 5, 10)]), 0.5);
  assert.equal(coverageRatio(hull, [sq(0, 0, 5, 10), sq(2, 0, 7, 10)]), 0.7);
  assert.equal(coverageRatio(hull, [sq(-10, -10, 20, 20)]), 1);
  assert.equal(coverageRatio([[0, 0], [1, 1]], []), 1);
});

test("padRing and footprintHull: a pad's copper by its shape, and the hull of a footprint's drawings, pads, holes and courtyard box (its fields left out)", () => {
  const rectPad = { num: "1", net: null, x: 0, y: 0, w: 1000, h: 600, round: false, th: false, shape: "rect", size: [1000, 600], rot: 90 } as Pad;
  near(ringArea(padRing(rectPad)), 600_000, 1e-6);
  // a 1 mm round pad: twelve chords at most 0.02 mm outside the circle
  const r = 500_000;
  const roundPad = { num: "2", net: null, x: 0, y: 0, w: 2 * r, h: 2 * r, round: true, th: true, shape: "circle", size: [2 * r, 2 * r], drill: r } as Pad;
  const a = ringArea(padRing(roundPad));
  assert.ok(a >= Math.PI * r * r && a < 1.03 * Math.PI * r * r, `${a}`);
  const part = { ref: "R1", placed: true, at: [0, 0], pads: [{ ...rectPad, x: -2000, y: 0 }, { ...rectPad, x: 2000, y: 0 }], courtyard: [-3000, -1000, 3000, 1000], graphics: [], fields: [{ id: "R1:Reference", x: 0, y: -5000 }] } as unknown as Part;
  assert.equal(ringArea(footprintHull(part, [])), 6000 * 2000, "the courtyard box is the outermost: the hull is it; the field 5 mm away is not in it");
});

test("GetCoverageArea of a shape: a line is width squared however long, an unfilled outline too, a filled one its area", () => {
  const line = { id: "l", layer: "F.SilkS", stroke_width: 200, kind: "segment", start: [0, 0], end: [50_000_000, 0] } as unknown as Shape;
  assert.equal(shapeCoverageArea(line, false), 40_000);
  const rect = { id: "r", layer: "F.SilkS", stroke_width: 100, kind: "rect", start: [0, 0], end: [1000, 2000] } as unknown as Shape;
  assert.equal(shapeCoverageArea(rect, false), 10_000);
  assert.equal(shapeCoverageArea(rect, true), 2_000_000);
  // a filled rectangle's polygon covers its area; an unfilled one only its stroke
  assert.ok(intersectionArea(shapeRings(rect, true)) >= 2_000_000);
  assert.ok(intersectionArea(shapeRings(rect, false)) < 2_000_000 / 2);
});
