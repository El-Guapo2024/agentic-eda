import { test } from "node:test";
import assert from "node:assert/strict";
import { padAt } from "./boardControlPick";

const parts = [
  {
    ref: "U1",
    placed: true,
    pads: [
      { num: "1", x: 0, y: 0, w: 1000, h: 600 },
      { num: "2", x: 1200, y: 0, w: 1000, h: 600 },
    ],
  },
  { ref: "R1", placed: true, pads: [{ num: "1", x: 5000, y: 5000, w: 800, h: 800 }] },
  { ref: "R2", placed: false, pads: [{ num: "1", x: 9000, y: 9000, w: 800, h: 800 }] },
  { ref: "H1", placed: true },
];

test("padAt: a point inside a pad's box finds it, named REF.NUMBER", () => {
  assert.deepEqual(padAt(parts, 10, 10), { ref: "U1", id: "U1.1", num: "1" });
  assert.deepEqual(padAt(parts, 1200, 250), { ref: "U1", id: "U1.2", num: "2" });
  assert.deepEqual(padAt(parts, 5390, 4610), { ref: "R1", id: "R1.1", num: "1" });
});

test("padAt: outside every pad, on an unplaced part, or on a part with no pads finds nothing", () => {
  assert.equal(padAt(parts, 600, 0), null, "the gap between the two pads of U1");
  assert.equal(padAt(parts, 0, 400), null, "just above a 600-high pad");
  assert.equal(padAt(parts, 9000, 9000), null, "R2 is not placed");
  assert.equal(padAt([], 0, 0), null);
});

test("padAt: where two pads' boxes overlap the nearer centre wins", () => {
  const overlap = [{ ref: "J1", placed: true, pads: [{ num: "1", x: 0, y: 0, w: 2000, h: 2000 }, { num: "2", x: 1500, y: 0, w: 2000, h: 2000 }] }];
  assert.equal(padAt(overlap, 400, 0)?.id, "J1.1");
  assert.equal(padAt(overlap, 1000, 0)?.id, "J1.2");
});

test("padAt: a round pad is hit by its circle, so a click in its box's corner falls through to the footprint (r7 C7-10)", () => {
  const round = [{ ref: "J1", placed: true, pads: [{ num: "1", x: 0, y: 0, w: 1000, h: 1000, round: true }] }];
  assert.deepEqual(padAt(round, 300, 300), { ref: "J1", id: "J1.1", num: "1" }, "inside the circle (r = 500)");
  assert.equal(padAt(round, 450, 450), null, "in the box's corner, outside the circle");
  const oval = [{ ref: "J2", placed: true, pads: [{ num: "1", x: 0, y: 0, w: 2000, h: 1000, round: true }] }];
  assert.deepEqual(padAt(oval, 900, 0)?.id, "J2.1", "the stadium's end");
  assert.equal(padAt(oval, 950, 450), null, "outside the stadium's round end, inside its box");
});
