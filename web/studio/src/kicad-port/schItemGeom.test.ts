import { test } from "node:test";
import assert from "node:assert/strict";
import { arcFromThreePoints, directiveShape, distPointSegment, graphicBounds, graphicHit, graphicOutline, pointInPolygon } from "./schItemGeom";
import type { SchGraphic } from "../api/schEditTypes";

const rect = (x0: number, y0: number, x1: number, y1: number, fill: SchGraphic["fill"] = "none"): SchGraphic => ({ id: "r", shape: { type: "rectangle", start: { x: x0, y: y0 }, end: { x: x1, y: y1 } }, fill });

test("distance to a segment clamps to its ends", () => {
  assert.equal(distPointSegment([5000, 3000], [0, 0], [10_000, 0]), 3000);
  assert.equal(distPointSegment([-4000, 3000], [0, 0], [10_000, 0]), 5000);
  assert.equal(distPointSegment([13_000, 4000], [0, 0], [10_000, 0]), 5000);
});

test("an unfilled rectangle is hit on its border, not inside; a filled one inside too", () => {
  const open = rect(0, 0, 10_000_000, 10_000_000);
  assert.equal(graphicHit(open, [5_000_000, 5_000_000], 100_000), false, "inside an unfilled box");
  assert.equal(graphicHit(open, [10_000_000, 4_000_000], 100_000), true, "on the right edge");
  assert.equal(graphicHit(open, [10_300_000, 4_000_000], 100_000), false, "just outside");
  const filled = rect(0, 0, 10_000_000, 10_000_000, "background");
  assert.equal(graphicHit(filled, [5_000_000, 5_000_000], 100_000), true);
});

test("a circle is hit near its circumference; filled, anywhere inside", () => {
  const c: SchGraphic = { id: "c", shape: { type: "circle", center: { x: 0, y: 0 }, radius_nm: 5_000_000 } };
  assert.equal(graphicHit(c, [5_000_000, 0], 50_000), true);
  assert.equal(graphicHit(c, [0, 0], 50_000), false);
  assert.equal(graphicHit({ ...c, fill: "outline" }, [0, 0], 50_000), true);
});

test("three points give the arc through them, either way round", () => {
  const up = arcFromThreePoints([10_000, 0], [0, 10_000], [-10_000, 0])!;
  assert.ok(Math.abs(up.center[0]) < 1e-9 && Math.abs(up.center[1]) < 1e-9);
  assert.ok(Math.abs(up.radius - 10_000) < 1e-9);
  const down = arcFromThreePoints([10_000, 0], [0, -10_000], [-10_000, 0])!;
  assert.ok(up.sweep * down.sweep < 0, "opposite directions");
  assert.ok(Math.abs(Math.abs(up.sweep) - Math.PI) < 1e-9);
  assert.equal(arcFromThreePoints([0, 0], [1, 1], [2000, 2000]), null, "collinear");
});

test("an arc is hit on the curve and not on the chord across it", () => {
  const a: SchGraphic = { id: "a", shape: { type: "arc", start: { x: 10_000_000, y: 0 }, mid: { x: 0, y: 10_000_000 }, end: { x: -10_000_000, y: 0 } } };
  assert.equal(graphicHit(a, [0, 10_000_000], 100_000), true);
  assert.equal(graphicHit(a, [0, 5_000_000], 100_000), false, "between the chord and the curve");
  const b = graphicBounds(a);
  assert.ok(b.maxY >= 10_000_000 && b.minY <= 0 && b.minX <= -10_000_000 && b.maxX >= 10_000_000);
});

test("a bezier's outline runs from its start to its end", () => {
  const z: SchGraphic = { id: "z", shape: { type: "bezier", start: { x: 0, y: 0 }, c1: { x: 0, y: 10_000_000 }, c2: { x: 10_000_000, y: 10_000_000 }, end: { x: 10_000_000, y: 0 } } };
  const { pts, closed } = graphicOutline(z);
  assert.equal(closed, false);
  assert.deepEqual(pts[0], [0, 0]);
  assert.deepEqual(pts[pts.length - 1], [10_000, 0]);
});

test("point in polygon, even-odd", () => {
  const tri: [number, number][] = [
    [0, 0],
    [10_000, 0],
    [0, 10_000],
  ];
  assert.equal(pointInPolygon([2000, 2000], tri), true);
  assert.equal(pointInPolygon([8000, 8000], tri), false);
});

test("a directive flag's pole runs away from the connection point, rotated by its spin", () => {
  const L = 2_540_000;
  // Spin RIGHT (file angle 0): the pole goes up (-y); UP (90): left; LEFT (180): down; BOTTOM (270): right -- SCH_DIRECTIVE_LABEL::CreateGraphicShape's rotations.
  const end = (deg: number) => directiveShape([0, 0], deg, "round", L).poleEnd.map(Math.round);
  assert.deepEqual(end(0), [0, -L]);
  assert.deepEqual(end(90), [-L, 0]);
  assert.deepEqual(end(180), [0, L]);
  assert.deepEqual(end(270), [L, 0]);
});

test("a directive label is hit on its flag", () => {
  const d: SchGraphic = { id: "d", shape: { type: "directive", at: { x: 0, y: 0 }, orientation: 0, shape: "round", pin_length_nm: 2_540_000 } };
  assert.equal(graphicHit(d, [0, -2_540_000], 50_000), true, "on the flag at the pole's end");
  assert.equal(graphicHit(d, [0, -1_000_000], 50_000), true, "on the pole");
  assert.equal(graphicHit(d, [8_000_000, 8_000_000], 50_000), false);
});

// ------------------------------------------------------------------------------------------------------------------------- images

test("an image on the sheet is its box: pixels at their definition times the scale, centred, hit anywhere inside", () => {
  // 60 x 40 px at 254 ppi are 6 x 4 mm
  const g = { id: "img_a", shape: { type: "image" as const, at: { x: 10_000, y: 20_000 }, scale_ppm: 1_000_000, px: [60, 40] as [number, number], ppi: 254, hash: "h" } };
  assert.deepEqual(graphicBounds(g), { minX: 7_000_000, minY: 18_000_000, maxX: 13_000_000, maxY: 22_000_000 });
  assert.equal(graphicHit(g, [10_000_000, 20_000_000], 0), true, "the middle of an image is a hit, whatever is drawn there");
  assert.equal(graphicHit(g, [13_000_000, 22_000_000], 0), true, "the corner");
  assert.equal(graphicHit(g, [13_200_000, 20_000_000], 100_000), false);
  assert.equal(graphicHit(g, [13_050_000, 20_000_000], 100_000), true, "within the accuracy of the box");
  assert.deepEqual(graphicOutline(g).pts, [[7_000_000, 18_000_000], [13_000_000, 18_000_000], [13_000_000, 22_000_000], [7_000_000, 22_000_000]]);
  const big = { ...g, shape: { ...g.shape, scale_ppm: 2_500_000_000 } };
  assert.deepEqual(graphicBounds(big), { minX: 2_500_000, minY: 15_000_000, maxX: 17_500_000, maxY: 25_000_000 });
});
