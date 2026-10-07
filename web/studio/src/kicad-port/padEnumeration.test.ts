import { test } from "node:test";
import assert from "node:assert/strict";
import { enumerateCommit, enumerateHit, enumerateNextValue, enumerateNumber, enumeratePopupText, enumerateShownNumber, enumerateStart, padsUnderSweep, sweepPoints } from "./padEnumeration";

const run = (start = 1, step = 1, prefix = "") => enumerateStart({ start, step, prefix });

test("pads take start, start+step, ... in the order they are hit; the prefix is prepended", () => {
  let s = run(10, 5, "A");
  s = enumerateHit(s, "p1", "7", true);
  s = enumerateHit(s, "p2", "8", true);
  s = enumerateHit(s, "p3", "9", true);
  assert.deepEqual(enumerateCommit(s), [
    ["p1", "A10"],
    ["p2", "A15"],
    ["p3", "A20"],
  ]);
  assert.equal(enumerateNextValue(s), 25);
  assert.equal(enumerateNumber(s.params, 25), "A25");
  assert.match(enumeratePopupText(s), /Click on pad A25/);
});

test("clicking a numbered pad gives its number back, and that number is the next one handed out", () => {
  let s = run();
  s = enumerateHit(s, "a", "x", true); // 1
  s = enumerateHit(s, "b", "y", true); // 2
  s = enumerateHit(s, "c", "z", true); // 3
  s = enumerateHit(s, "b", "2", true); // click b again: it gives 2 back
  assert.deepEqual(enumerateCommit(s), [
    ["a", "1"],
    ["c", "3"],
  ]);
  assert.equal(enumerateNextValue(s), 2, "the given-back value comes before any fresh one");
  s = enumerateHit(s, "d", "w", true);
  assert.deepEqual(enumerateCommit(s), [
    ["a", "1"],
    ["c", "3"],
    ["d", "2"],
  ]);
  assert.equal(enumerateNextValue(s), 4, "then the sequence continues where it was");
});

test("given-back values are handed out oldest first", () => {
  let s = run();
  for (const id of ["a", "b", "c"]) s = enumerateHit(s, id, "", true);
  s = enumerateHit(s, "c", "3", true); // gives 3
  s = enumerateHit(s, "a", "1", true); // gives 1
  assert.deepEqual(s.stored, [3, 1]);
  s = enumerateHit(s, "x", "", true);
  assert.equal(s.assigned.get("x")!.value, 3);
  s = enumerateHit(s, "y", "", true);
  assert.equal(s.assigned.get("y")!.value, 1);
});

test("dragging over a numbered pad does nothing (only a click takes the number back)", () => {
  let s = run();
  s = enumerateHit(s, "a", "q", false);
  const again = enumerateHit(s, "a", "1", false);
  assert.deepEqual(enumerateCommit(again), [["a", "1"]]);
  assert.equal(again.seq, 2, "no number was spent");
});

test("a pad remembers what it was called before the tool touched it", () => {
  let s = run();
  s = enumerateHit(s, "a", "OLD", true);
  assert.equal(s.assigned.get("a")!.oldNumber, "OLD");
  assert.equal(enumerateShownNumber(s, "a", "OLD"), "1");
  assert.equal(enumerateShownNumber(s, "other", "K"), "K");
  s = enumerateHit(s, "a", "1", true);
  assert.equal(enumerateShownNumber(s, "a", "OLD"), "OLD", "back to its own number once given back");
});

test("sweepPoints: one point for a still mouse, a point every 0.1 mm along a move, from the cursor back to where it was", () => {
  assert.deepEqual(sweepPoints(null, { x: 500, y: 500 }), [{ x: 500, y: 500 }]);
  assert.deepEqual(sweepPoints({ x: 500, y: 500 }, { x: 500, y: 500 }), [{ x: 500, y: 500 }]);
  const pts = sweepPoints({ x: 0, y: 0 }, { x: 1000, y: 0 });
  assert.equal(pts.length, 11, "1 mm / 0.1 mm + 1");
  assert.deepEqual(pts[0], { x: 1000, y: 0 });
  // `line_step = ( mouse - old ) / segments` is an integer division (1000 / 11 = 90), so the last point stops short of where the mouse was
  assert.deepEqual(pts[1], { x: 910, y: 0 });
  assert.deepEqual(pts[10], { x: 100, y: 0 });
});

test("padsUnderSweep lists a pad once per consecutive run and keeps sweep order", () => {
  const pads = [
    { id: "a", x: 0 },
    { id: "b", x: 500 },
    { id: "c", x: 1000 },
  ];
  const contains = (p: { x: number }, x: number) => Math.abs(p.x - x) <= 120;
  const pts = sweepPoints({ x: 0, y: 0 }, { x: 1000, y: 0 }); // from x=1000 back to 0
  const hits = padsUnderSweep(pads, pts, contains);
  assert.deepEqual(
    hits.map((p) => p.id),
    ["c", "b", "a"],
    "a fast move across three pads numbers all three, in the order the cursor met them"
  );
});
