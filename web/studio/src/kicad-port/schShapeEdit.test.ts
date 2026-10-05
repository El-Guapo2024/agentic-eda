import { test } from "node:test";
import assert from "node:assert/strict";
import { arcMid, beginEdit, calcEdit, continueEdit, toShape } from "./schShapeEdit";

test("a rectangle is two clicks: the first corner, then the opposite one", () => {
  let e = beginEdit("rectangle", [0, 0]);
  e = calcEdit(e, [4000, 2000]);
  assert.equal(continueEdit(e).more, false, "the second click finishes it");
  assert.deepEqual(toShape(e), { type: "rectangle", start: { x: 0, y: 0 }, end: { x: 4000, y: 2000 } });
});

test("a circle is its centre and a point on it", () => {
  let e = beginEdit("circle", [1000, 1000]);
  e = calcEdit(e, [1000 + 3000, 1000 + 4000]);
  assert.deepEqual(toShape(e), { type: "circle", center: { x: 1000, y: 1000 }, radius_um: 5000 });
});

test("an untouched shape is degenerate and not committed", () => {
  assert.equal(toShape(beginEdit("rectangle", [5, 5])), null);
  assert.equal(toShape(beginEdit("circle", [5, 5])), null);
  assert.equal(toShape(beginEdit("arc", [5, 5])), null);
  assert.equal(toShape(beginEdit("bezier", [5, 5])), null);
});

test("an arc is two clicks and subtends 90 degrees, turning clockwise from the start to the end", () => {
  // Start (0,0), end (100,0): the chord is 100, the radius 70.71, the centre (50,50) -- the arc bulges to the top of the sheet (negative y).
  let e = beginEdit("arc", [0, 0]);
  e = calcEdit(e, [100, 0]);
  assert.deepEqual(e.center, [50, 50]);
  assert.equal(continueEdit(e).more, false);
  const shape = toShape(e);
  assert.equal(shape?.type, "arc");
  if (shape?.type !== "arc") return;
  assert.deepEqual(shape.start, { x: 0, y: 0 });
  assert.deepEqual(shape.end, { x: 100, y: 0 });
  assert.deepEqual(shape.mid, { x: 50, y: -21 }); // 50 - 70.71, rounded
});

test("drawing the arc the other way round bulges the other way", () => {
  let e = beginEdit("arc", [100, 0]);
  e = calcEdit(e, [0, 0]);
  // Right to left over the bottom: still clockwise on the sheet.
  assert.deepEqual(e.center, [50, -50]);
  assert.equal(arcMid(e.start, e.end, e.center)[1], 21);
});

test("a vertical chord puts the centre to its left or right of it, always keeping 90 degrees", () => {
  let e = beginEdit("arc", [0, 0]);
  e = calcEdit(e, [0, 100]);
  // Down the sheet, clockwise: the arc bulges to the right (positive x), so the centre is on the left.
  assert.deepEqual(e.center, [-50, 50]);
  assert.equal(arcMid(e.start, e.end, e.center)[0], 21);
});

test("a Bezier curve is four clicks: start, end, control point 1, control point 2", () => {
  let e = beginEdit("bezier", [0, 0]);
  e = calcEdit(e, [3000, 0]); // state 1: the end (and control point 2) follow the cursor
  assert.deepEqual([e.end, e.c2], [[3000, 0], [3000, 0]]);
  let step = continueEdit(e); // second click fixes the end
  assert.equal(step.more, true);
  e = calcEdit(step.edit, [1000, 2000]); // state 2: control point 1
  assert.deepEqual(e.c1, [1000, 2000]);
  assert.deepEqual(e.end, [3000, 0]);
  step = continueEdit(e); // third click fixes control point 1
  assert.equal(step.more, true);
  e = calcEdit(step.edit, [2000, 2000]); // state 3: control point 2
  assert.deepEqual(e.c2, [2000, 2000]);
  step = continueEdit(e); // the fourth click finishes
  assert.equal(step.more, false);
  assert.deepEqual(toShape(step.edit), { type: "bezier", start: { x: 0, y: 0 }, c1: { x: 1000, y: 2000 }, c2: { x: 2000, y: 2000 }, end: { x: 3000, y: 0 } });
});

test("a text box is drawn like a rectangle but has no shape until it has its text", () => {
  let e = beginEdit("text_box", [0, 0]);
  e = calcEdit(e, [4000, 1000]);
  assert.deepEqual([e.start, e.end], [[0, 0], [4000, 1000]]);
  assert.equal(toShape(e), null);
});
