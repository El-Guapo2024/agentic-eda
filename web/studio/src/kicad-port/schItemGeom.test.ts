import { test } from "node:test";
import assert from "node:assert/strict";
import { arcFromThreePoints, directiveShape, distPointSegment, graphicBounds, graphicHit, graphicOutline, pointInPolygon } from "./schItemGeom";
import type { SchGraphic } from "../api/schEditTypes";

const rect = (x0: number, y0: number, x1: number, y1: number, fill: SchGraphic["fill"] = "none"): SchGraphic => ({ id: "r", shape: { type: "rectangle", start: { x: x0, y: y0 }, end: { x: x1, y: y1 } }, fill });

test("distance to a segment clamps to its ends", () => {
  assert.equal(distPointSegment([5, 3], [0, 0], [10, 0]), 3);
  assert.equal(distPointSegment([-4, 3], [0, 0], [10, 0]), 5);
  assert.equal(distPointSegment([13, 4], [0, 0], [10, 0]), 5);
});

test("an unfilled rectangle is hit on its border, not inside; a filled one inside too", () => {
  const open = rect(0, 0, 10_000, 10_000);
  assert.equal(graphicHit(open, [5_000, 5_000], 100), false, "inside an unfilled box");
  assert.equal(graphicHit(open, [10_000, 4_000], 100), true, "on the right edge");
  assert.equal(graphicHit(open, [10_300, 4_000], 100), false, "just outside");
  const filled = rect(0, 0, 10_000, 10_000, "background");
  assert.equal(graphicHit(filled, [5_000, 5_000], 100), true);
});

test("a circle is hit near its circumference; filled, anywhere inside", () => {
  const c: SchGraphic = { id: "c", shape: { type: "circle", center: { x: 0, y: 0 }, radius_um: 5_000 } };
  assert.equal(graphicHit(c, [5_000, 0], 50), true);
  assert.equal(graphicHit(c, [0, 0], 50), false);
  assert.equal(graphicHit({ ...c, fill: "outline" }, [0, 0], 50), true);
});

test("three points give the arc through them, either way round", () => {
  const up = arcFromThreePoints([10, 0], [0, 10], [-10, 0])!;
  assert.ok(Math.abs(up.center[0]) < 1e-9 && Math.abs(up.center[1]) < 1e-9);
  assert.ok(Math.abs(up.radius - 10) < 1e-9);
  const down = arcFromThreePoints([10, 0], [0, -10], [-10, 0])!;
  assert.ok(up.sweep * down.sweep < 0, "opposite directions");
  assert.ok(Math.abs(Math.abs(up.sweep) - Math.PI) < 1e-9);
  assert.equal(arcFromThreePoints([0, 0], [1, 1], [2, 2]), null, "collinear");
});

test("an arc is hit on the curve and not on the chord across it", () => {
  const a: SchGraphic = { id: "a", shape: { type: "arc", start: { x: 10_000, y: 0 }, mid: { x: 0, y: 10_000 }, end: { x: -10_000, y: 0 } } };
  assert.equal(graphicHit(a, [0, 10_000], 100), true);
  assert.equal(graphicHit(a, [0, 5_000], 100), false, "between the chord and the curve");
  const b = graphicBounds(a);
  assert.ok(b.maxY >= 10_000 && b.minY <= 0 && b.minX <= -10_000 && b.maxX >= 10_000);
});

test("a bezier's outline runs from its start to its end", () => {
  const z: SchGraphic = { id: "z", shape: { type: "bezier", start: { x: 0, y: 0 }, c1: { x: 0, y: 10_000 }, c2: { x: 10_000, y: 10_000 }, end: { x: 10_000, y: 0 } } };
  const { pts, closed } = graphicOutline(z);
  assert.equal(closed, false);
  assert.deepEqual(pts[0], [0, 0]);
  assert.deepEqual(pts[pts.length - 1], [10_000, 0]);
});

test("point in polygon, even-odd", () => {
  const tri: [number, number][] = [
    [0, 0],
    [10, 0],
    [0, 10],
  ];
  assert.equal(pointInPolygon([2, 2], tri), true);
  assert.equal(pointInPolygon([8, 8], tri), false);
});

test("a directive flag's pole runs away from the connection point, rotated by its spin", () => {
  const L = 2_540;
  // Spin RIGHT (file angle 0): the pole goes up (-y); UP (90): left; LEFT (180): down; BOTTOM (270): right -- SCH_DIRECTIVE_LABEL::CreateGraphicShape's rotations.
  const end = (deg: number) => directiveShape([0, 0], deg, "round", L).poleEnd.map(Math.round);
  assert.deepEqual(end(0), [0, -L]);
  assert.deepEqual(end(90), [-L, 0]);
  assert.deepEqual(end(180), [0, L]);
  assert.deepEqual(end(270), [L, 0]);
});

test("a directive label is hit on its flag", () => {
  const d: SchGraphic = { id: "d", shape: { type: "directive", at: { x: 0, y: 0 }, orientation: 0, shape: "round", pin_length_um: 2_540 } };
  assert.equal(graphicHit(d, [0, -2_540], 50), true, "on the flag at the pole's end");
  assert.equal(graphicHit(d, [0, -1_000], 50), true, "on the pole");
  assert.equal(graphicHit(d, [8_000, 8_000], 50), false);
});
