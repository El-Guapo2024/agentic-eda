import { test } from "node:test";
import assert from "node:assert/strict";
import { borderHatchPitchError, clampBorderHatchPitch, generateHatchLines, pointInRing, polygonContains, rescaleInt, segIntersectsLine, zoneHatchLines } from "./zoneHatch";

const square = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [
  [x0, y0],
  [x1, y0],
  [x1, y1],
  [x0, y1],
];

test("rescale rounds the 64-bit product to nearest, half away from zero, beyond 2^53 too", () => {
  assert.equal(rescaleInt(7, 3, 2), 11); // 21 / 2 = 10.5 -> 11
  assert.equal(rescaleInt(-7, 3, 2), -11);
  assert.equal(rescaleInt(7, -3, -2), 11);
  assert.equal(rescaleInt(400_000_000, 300_000_000, 7), Number((400_000_000n * 300_000_000n + 3n) / 7n));
});

test("PointInside's crossing test, and Contains leaves the holes out", () => {
  const outer = square(0, 0, 100, 100);
  const hole = square(40, 40, 60, 60);
  assert.equal(pointInRing(outer, [5, 5]), true);
  assert.equal(pointInRing(outer, [150, 5]), false);
  assert.equal(polygonContains(outer, [hole], [50, 50]), false, "inside a hole is outside the zone");
  assert.equal(polygonContains(outer, [hole], [20, 50]), true);
});

test("SEG::IntersectsLine: an oblique segment, a vertical one and a parallel one", () => {
  assert.deepEqual(segIntersectsLine([0, 0], [10, 0], -1, 4), [4, 0]);
  assert.deepEqual(segIntersectsLine([0, 10], [0, 0], -1, 4), [0, 4], "vertical: the line's y at the segment's x");
  assert.equal(segIntersectsLine([10, 0], [10, 10], -1, 4), null, "the line's y at x = 10 is -6, off the segment");
  assert.equal(segIntersectsLine([10, 10], [0, 10], -1, 4), null, "t = 1.6 is past the end");
  assert.deepEqual(segIntersectsLine([0, 4], [4, 0], -1, 4), [2, 2], "on the line: its middle");
  assert.equal(segIntersectsLine([0, 5], [5, 0], -1, 4), null, "parallel, off the line");
});

test("GenerateHatchLines over a square: whole diagonals for a full hatch, edge stubs for an edge hatch", () => {
  assert.deepEqual(generateHatchLines(square(0, 0, 10, 10), [], [-1], 4, -1), [
    [[4, 0], [0, 4]],
    [[8, 0], [0, 8]],
    [[10, 2], [2, 10]],
    [[10, 6], [6, 10]],
  ]);
  // A piece of 8 along x is not shorter than twice the line length 4: its two ends, 4 long each.
  const edge = generateHatchLines(square(0, 0, 10, 10), [], [-1], 4, 4);
  assert.deepEqual(edge[1], [[8, 0], [4, 4]]);
  assert.deepEqual(edge[2], [[0, 8], [4, 4]]);
  assert.deepEqual(edge[0], [[4, 0], [0, 4]], "a piece of 4 is drawn whole");
});

test("the hatch stops at a hole", () => {
  const lines = generateHatchLines(square(0, 0, 100, 100), [square(40, 40, 60, 60)], [-1], 10, -1);
  const at90 = lines.filter(([a, b]) => a[0] + a[1] === 90 && b[0] + b[1] === 90);
  assert.deepEqual(at90, [
    [[90, 0], [50, 40]],
    [[40, 50], [0, 90]],
  ]);
  for (const [a, b] of lines) assert.equal(polygonContains(square(40, 40, 60, 60), [], [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]), false, "no piece in the hole");
});

test("HatchBorder: the style, the pitch and the layer's slope", () => {
  const z = { layer: "F.Cu", outline: square(0, 0, 4_000_000, 4_000_000) };
  assert.deepEqual(zoneHatchLines({ ...z, border_style: "none" }), [], "Line: no hatch");
  assert.deepEqual(zoneHatchLines({ ...z, border_style: "edge", border_hatch_pitch: 0 }), [], "a pitch of 0: no hatch");
  const full = zoneHatchLines({ ...z, border_style: "full", border_hatch_pitch: 1_000_000 });
  // Fully hatched: twice the pitch apart, slope -1 on copper (even PCB_LAYER_ID).
  assert.ok(full.length > 0 && full.every(([a, b]) => a[0] + a[1] === b[0] + b[1] && (a[0] + a[1]) % 2_000_000 === 0));
  const silk = zoneHatchLines({ ...z, layer: "F.SilkS", border_style: "full", border_hatch_pitch: 1_000_000 });
  assert.ok(silk.length > 0 && silk.every(([a, b]) => a[1] - a[0] === b[1] - b[0]), "slope 1 on a technical layer (odd id)");
  const edge = zoneHatchLines({ ...z, border_hatch_pitch: 1_000_000 });
  assert.ok(edge.length > full.length, "the default (Hatched) has a stub at each end of the long diagonals");
});

test("the hatch pitch is 0.1 to 2 mm: clamped on read, refused by the dialog", () => {
  assert.equal(clampBorderHatchPitch(10_000), 100_000);
  assert.equal(clampBorderHatchPitch(5_000_000), 2_000_000);
  const fmt = (nm: number) => `${nm / 1_000_000} mm`;
  assert.equal(borderHatchPitchError(50_000, fmt), "Outline hatch pitch must be at least 0.1 mm.");
  assert.equal(borderHatchPitchError(2_500_000, fmt), "Outline hatch pitch must be less than 2 mm.");
  assert.equal(borderHatchPitchError(500_000, fmt), null);
});
