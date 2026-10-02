import { test } from "node:test";
import assert from "node:assert/strict";
import {
  BEZIER_COMPLETE,
  BEZIER_SET_CONTROL1,
  BEZIER_SET_CONTROL2,
  BEZIER_SET_END,
  BEZIER_SET_START,
  bezierAddPoint,
  bezierChainFrom,
  bezierClick,
  bezierControlC2,
  bezierCurveOf,
  bezierFinishDouble,
  bezierMotion,
  bezierRemoveLastPoint,
  bezierStarted,
  newBezierGeom,
} from "./bezierGeom";
import { bezierPointAt, bezierPolyline } from "./bezierPoly";

// ------------------------------------------------------------- state machine

test("four clicks: start, control 1, end, control 2 -> COMPLETE", () => {
  let g = newBezierGeom();
  assert.equal(g.step, BEZIER_SET_START);
  assert.equal(bezierStarted(g), false);
  g = bezierAddPoint(g, [0, 0], true);
  assert.equal(g.step, BEZIER_SET_CONTROL1);
  assert.equal(bezierStarted(g), true);
  g = bezierAddPoint(g, [10, 0], true);
  assert.equal(g.step, BEZIER_SET_END);
  g = bezierAddPoint(g, [100, 0], true);
  assert.equal(g.step, BEZIER_SET_CONTROL2);
  g = bezierAddPoint(g, [110, 10], true);
  assert.equal(g.step, BEZIER_COMPLETE);
});

test("setStart collapses every control point onto the start (no weird loops before the others are set)", () => {
  const g = bezierAddPoint(newBezierGeom(), [5, 7], true);
  assert.deepEqual([g.start, g.controlC1, g.end, g.controlC2], [[5, 7], [5, 7], [5, 7], [5, 7]]);
});

test("setControlC1 drags end and C2 along with it until they are set", () => {
  let g = bezierAddPoint(newBezierGeom(), [0, 0], true);
  g = bezierAddPoint(g, [30, 40], false);
  assert.equal(g.step, BEZIER_SET_CONTROL1, "motion does not advance");
  assert.deepEqual([g.controlC1, g.end, g.controlC2], [[30, 40], [30, 40], [30, 40]]);
});

test("the end point must differ from the start: a rejected end steps back to control 1", () => {
  let g = bezierAddPoint(bezierAddPoint(newBezierGeom(), [0, 0], true), [10, 0], true);
  assert.equal(g.step, BEZIER_SET_END);
  g = bezierAddPoint(g, [0, 0], true);
  assert.equal(g.step, BEZIER_SET_CONTROL1);
});

test("GetControlC2 is the raw cursor point reflected over the end point", () => {
  let g = newBezierGeom();
  for (const p of [[0, 0], [10, 0], [100, 0], [110, 10]] as [number, number][]) g = bezierAddPoint(g, p, true);
  assert.deepEqual(bezierControlC2(g), [90, -10]);
  assert.deepEqual(bezierCurveOf(g), { start: [0, 0], c1: [10, 0], c2: [90, -10], end: [100, 0] });
});

test("a completed curve chains: the next starts at its end with C1 already the mirror of C2 (the cursor's own point)", () => {
  let geom: ReturnType<typeof newBezierGeom> | null = null;
  let last = null;
  for (const p of [[0, 0], [10, 0], [100, 0], [110, 10]] as [number, number][]) {
    const r = bezierClick(geom, p);
    geom = r.geom;
    last = r.curve;
  }
  assert.deepEqual(last, { start: [0, 0], c1: [10, 0], c2: [90, -10], end: [100, 0] });
  assert.ok(geom);
  assert.equal(geom!.step, BEZIER_SET_END, "primed with start and C1: waiting for the end point");
  assert.deepEqual(geom!.start, [100, 0]);
  assert.deepEqual(geom!.controlC1, [110, 10]);
});

test("a zero-length second control arm chains with only the start point (the user places the new C1)", () => {
  let geom: ReturnType<typeof newBezierGeom> | null = null;
  for (const p of [[0, 0], [10, 0], [100, 0], [100, 0]] as [number, number][]) geom = bezierClick(geom, p).geom;
  assert.ok(geom);
  assert.equal(geom!.step, BEZIER_SET_CONTROL1);
  assert.deepEqual(geom!.start, [100, 0]);
});

test("bezierChainFrom reproduces DrawBezier's priming for a curve value", () => {
  const g = bezierChainFrom({ start: [0, 0], c1: [10, 0], c2: [90, -10], end: [100, 0] });
  assert.equal(g.step, BEZIER_SET_END);
  assert.deepEqual(g.controlC1, [110, 10]);
});

test("double-click: the remaining points all take the current point, then the curve is accepted (no chaining)", () => {
  // first click placed the start; the double-click's second press arrived as an ordinary click (control 1)
  let g = bezierAddPoint(newBezierGeom(), [0, 0], true);
  g = bezierAddPoint(g, [50, 50], true);
  const curve = bezierFinishDouble(g, [50, 50]);
  assert.deepEqual(curve, { start: [0, 0], c1: [50, 50], c2: [50, 50], end: [50, 50] });
});

test("double-click on the start point yields nothing (a zero-length curve is not committed)", () => {
  let g = bezierAddPoint(newBezierGeom(), [7, 7], true);
  g = bezierAddPoint(g, [7, 7], true);
  assert.equal(bezierFinishDouble(g, [7, 7]), null);
});

test("RemoveLastPoint steps back and reprocesses the last point in the earlier step", () => {
  let g = newBezierGeom();
  for (const p of [[0, 0], [10, 0], [100, 0]] as [number, number][]) g = bezierAddPoint(g, p, true);
  g = bezierMotion(g, [120, 5]);
  g = bezierRemoveLastPoint(g);
  assert.equal(g.step, BEZIER_SET_END);
  // [120,5] re-processed as the end point
  assert.deepEqual(g.end, [120, 5]);
});

// ------------------------------------------------------------- flattening

function distToPolyline(p: [number, number], poly: [number, number][]): number {
  let best = Infinity;
  for (let i = 0; i + 1 < poly.length; i++) {
    const [ax, ay] = poly[i]!;
    const [bx, by] = poly[i + 1]!;
    const dx = bx - ax,
      dy = by - ay;
    const len2 = dx * dx + dy * dy;
    const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - ax) * dx + (p[1] - ay) * dy) / len2));
    best = Math.min(best, Math.hypot(p[0] - (ax + t * dx), p[1] - (ay + t * dy)));
  }
  return best;
}

test("BEZIER_POLY: a straight 'curve' flattens to its two end points", () => {
  assert.deepEqual(bezierPolyline([0, 0], [100, 0], [200, 0], [300, 0], 5), [[0, 0], [300, 0]]);
});

test("BEZIER_POLY: a rounded curve stays within maxError of the true curve and keeps its ends", () => {
  const s: [number, number] = [0, 0],
    c1: [number, number] = [0, 10000],
    c2: [number, number] = [10000, 10000],
    e: [number, number] = [10000, 0];
  for (const maxError of [5, 50, 500]) {
    const poly = bezierPolyline(s, c1, c2, e, maxError);
    assert.deepEqual(poly[0], s);
    assert.deepEqual(poly[poly.length - 1], e);
    for (let i = 0; i <= 200; i++) {
      const d = distToPolyline(bezierPointAt(s, c1, c2, e, i / 200), poly);
      assert.ok(d <= maxError + 1.5, `maxError ${maxError}: sample ${i} is ${d.toFixed(2)} from the polyline (rounding slack 1.5)`);
    }
  }
  // a tighter tolerance needs more segments
  assert.ok(bezierPolyline(s, c1, c2, e, 5).length > bezierPolyline(s, c1, c2, e, 500).length);
});

test("BEZIER_POLY: an S-curve (two inflection points handled) is within tolerance", () => {
  const s: [number, number] = [0, 0],
    c1: [number, number] = [8000, 12000],
    c2: [number, number] = [2000, -12000],
    e: [number, number] = [10000, 0];
  const poly = bezierPolyline(s, c1, c2, e, 20);
  assert.deepEqual(poly[0], s);
  assert.deepEqual(poly[poly.length - 1], e);
  for (let i = 0; i <= 400; i++) {
    const d = distToPolyline(bezierPointAt(s, c1, c2, e, i / 400), poly);
    assert.ok(d <= 21.5, `sample ${i} is ${d.toFixed(2)} from the polyline`);
  }
});

test("BEZIER_POLY: a closed loop (start == end) still produces a real polyline", () => {
  const poly = bezierPolyline([0, 0], [5000, 5000], [-5000, 5000], [0, 0], 10);
  assert.ok(poly.length > 4);
  assert.deepEqual(poly[0], [0, 0]);
  assert.deepEqual(poly[poly.length - 1], [0, 0]);
});

test("BEZIER_POLY: maxError <= 0 falls back to 10 like the C++", () => {
  const a = bezierPolyline([0, 0], [0, 4000], [4000, 4000], [4000, 0], 0);
  const b = bezierPolyline([0, 0], [0, 4000], [4000, 4000], [4000, 0], 10);
  assert.deepEqual(a, b);
});
