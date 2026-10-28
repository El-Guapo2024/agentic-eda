import { test } from "node:test";
import assert from "node:assert/strict";
import { hatchLines } from "./schHatch";

test("r7 M5: a hatch is lines of slope -1, half the stroke wide, 40 widths apart; a cross-hatch has both slopes", () => {
  const h = hatchLines("hatch", [0, 0, 100_000, 100_000], 2_000);
  assert.equal(h.width, 1_000);
  assert.ok(h.segments.length > 0);
  for (const [ax, ay, bx, by] of h.segments) assert.equal((by - ay) / (bx - ax), -1);
  const [first, second] = h.segments;
  assert.ok(first && second);
  const gap = Math.abs(second[1] - first[1]) / Math.SQRT2;
  assert.ok(Math.abs(gap - 40_000) < 1, `the lines are 40 hatch widths apart: ${gap}`);
  const cross = hatchLines("cross_hatch", [0, 0, 100_000, 100_000], 2_000);
  const slopes = new Set(cross.segments.map(([ax, ay, bx, by]) => (by - ay) / (bx - ax)));
  assert.deepEqual([...slopes].sort(), [-1, 1]);
  for (const [ax, ay, bx, by] of hatchLines("reverse_hatch", [0, 0, 100_000, 100_000], 2_000).segments) assert.equal((by - ay) / (bx - ax), 1);
});

test("r7 M5: no more than about 100 lines across the major axis", () => {
  const h = hatchLines("hatch", [0, 0, 10_000_000, 10_000_000], 100);
  assert.ok(h.segments.length <= 202, `${h.segments.length}`);
});
