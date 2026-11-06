import { test } from "node:test";
import assert from "node:assert/strict";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_PCB_GRIDS_NM, DEFAULT_GRID_TICK, GRID_THICKNESS_CHOICES, gridLineWidthChoice, gridMarks, gridMinSpacingValue, visibleGridSize2, type GridDrawSpec, type GridMark } from "./grid";

test("computeVisibleGridSize: a comfortably visible grid is left untouched", () => {
  // 100um grid at 1 px/um = 100px apart on screen, well above the 10px threshold.
  assert.equal(computeVisibleGridSize(100, 1, "lines"), 100);
});

test("computeVisibleGridSize: a sub-pixel grid is coarsened by the tick factor until visible", () => {
  // 1um grid at 1 px/um = 1px apart -- below the 10px threshold, so it should
  // multiply by the tick (10) repeatedly: 1 -> 10 -> 100 (100px, first value > 10px threshold... actually 10px == threshold, loop continues while <=).
  const result = computeVisibleGridSize(1, 1, "lines");
  assert.equal(result, 100);
});

test("computeVisibleGridSize: small-cross needs a (strictly) larger margin than lines/dots before it stops coarsening", () => {
  // 20um grid at 1px/um = 20px apart. Lines' threshold is 10px, so 20px is
  // already comfortably above it (no coarsening). Small-cross doubles the
  // threshold to 20px, and the loop condition is "<=" -- so the same 20px
  // spacing is still AT small-cross's threshold and coarsens once more.
  const lines = computeVisibleGridSize(20, 1, "lines");
  const cross = computeVisibleGridSize(20, 1, "small-cross");
  assert.equal(lines, 20, "20px spacing already clears the 10px lines threshold");
  assert.equal(cross, 20 * DEFAULT_GRID_TICK, "20px spacing is exactly at small-cross's doubled (20px) threshold, so it coarsens once more");
});

test("computeVisibleGridSize: zooming in eventually needs no coarsening", () => {
  // 1um grid at 1000 px/um = 1000px apart, nowhere near the 10px threshold.
  assert.equal(computeVisibleGridSize(1, 1000, "lines"), 1);
});

test("isMajorGridLine: every 10th index (from the origin) is major, matching DEFAULT_GRID_TICK", () => {
  assert.equal(DEFAULT_GRID_TICK, 10);
  assert.ok(isMajorGridLine(0));
  assert.ok(!isMajorGridLine(1));
  assert.ok(!isMajorGridLine(9));
  assert.ok(isMajorGridLine(10));
  assert.ok(isMajorGridLine(-10));
  assert.ok(!isMajorGridLine(-1));
});

test("DEFAULT_PCB_GRIDS_NM: the real KiCad default list, unsorted mil-then-mm blocks, 22 entries", () => {
  assert.equal(DEFAULT_PCB_GRIDS_NM.length, 22);
  assert.equal(DEFAULT_PCB_GRIDS_NM[0], 25_400_000); // 1000 mil
  assert.equal(DEFAULT_PCB_GRIDS_NM[11], 25_400); // 1 mil
  assert.equal(DEFAULT_PCB_GRIDS_NM[12], 5_000_000); // 5.0 mm -- the deliberate jump back up in size
  assert.equal(DEFAULT_PCB_GRIDS_NM[DEFAULT_PCB_GRIDS_NM.length - 1], 10_000); // 0.01 mm
  // Each mil-block entry is strictly decreasing; so is the mm block -- but
  // the list as a whole is NOT monotonic across the mil->mm boundary (that
  // jump is intentional, see this module's header comment).
  for (let i = 1; i < 12; i++) assert.ok(DEFAULT_PCB_GRIDS_NM[i]! < DEFAULT_PCB_GRIDS_NM[i - 1]!);
  for (let i = 13; i < DEFAULT_PCB_GRIDS_NM.length; i++) assert.ok(DEFAULT_PCB_GRIDS_NM[i]! < DEFAULT_PCB_GRIDS_NM[i - 1]!);
  assert.ok(DEFAULT_PCB_GRIDS_NM[12]! > DEFAULT_PCB_GRIDS_NM[11]!, "1 mil -> 5.0mm is a jump UP in size, not sorted");
});

test("computeVisibleGridSize: an unfitted view (scale 0) returns instead of looping forever", () => {
  assert.equal(computeVisibleGridSize(1000, 0), 1000);
  assert.equal(computeVisibleGridSize(1000, Number.NaN), 1000);
});

// ---------------------------------------------------------------- UI-LEFT C2: OPENGL_GAL::DrawGrid's three styles

const view1 = { x: 0, y: 0, scale: 1e-5 }; // 1 px = 100 um
const spec = (style: "dots" | "lines" | "small-cross", over: Partial<GridDrawSpec> = {}) => ({ size: { x: 1_000_000, y: 1_000_000 }, origin: [0, 0] as const, style, lineWidthPx: 1, minSpacingPx: 5, ...over });

test("visibleGridSize2: each axis at least 100, both multiplied by the tick while the smaller is closer than the minimum spacing (twice that for small crosses)", () => {
  assert.deepEqual(visibleGridSize2({ x: 1_000_000, y: 2_000_000 }, 1e-5, "dots", 10), { x: 1_000_000, y: 2_000_000 }, "10 px apart: drawn as it is");
  assert.deepEqual(visibleGridSize2({ x: 1_000_000, y: 2_000_000 }, 1e-5, "dots", 11), { x: 10_000_000, y: 20_000_000 });
  assert.deepEqual(visibleGridSize2({ x: 1_000_000, y: 1_000_000 }, 1e-5, "small-cross", 6), { x: 10_000_000, y: 10_000_000 }, "a small cross needs twice the room");
  assert.deepEqual(visibleGridSize2({ x: 1_000_000, y: 1_000_000 }, 1e-5, "dots", 6), { x: 1_000_000, y: 1_000_000 });
  assert.deepEqual(visibleGridSize2({ x: 0, y: 50 }, 1, "dots", 10), { x: 100, y: 100 });
});

test("gridMarks DOTS: one rectangle per crossing, as wide as its column's line and as high as its row's (the stencil of the two passes)", () => {
  // a 100 x 100 px screen of a 1 mm grid at 10 px: KiROUND( 0 / 1 mm ) = 0 .. KiROUND( 10 mm / 1 mm ) = 10, one more each side -> -1 .. 11
  const marks = gridMarks(view1, 100, 100, spec("dots"));
  assert.equal(marks.length, 13 * 13);
  assert.ok(marks.every((m) => m.kind === "rect"));
  const at = (x: number, y: number) => marks.find((m) => m.kind === "rect" && Math.abs(m.x + m.w / 2 - x) < 1 && Math.abs(m.y + m.h / 2 - y) < 1) as Extract<GridMark, { kind: "rect" }>;
  const minor = 1 / 1e-5; // 1 px in nm
  assert.deepEqual([at(1_000_000, 1_000_000).w, at(1_000_000, 1_000_000).h], [minor, minor]);
  assert.deepEqual([at(0, 0).w, at(0, 0).h], [2 * minor, 2 * minor], "the tick at the origin is a major line both ways");
  assert.deepEqual([at(0, 1_000_000).w, at(0, 1_000_000).h], [2 * minor, minor], "a major column crossing a minor row");
});

test("gridMarks LINES: every row and column across the index range, every tenth (from the origin) twice as wide; the thickness setting sets the minor width, at least 1 px", () => {
  const marks = gridMarks(view1, 100, 100, spec("lines", { lineWidthPx: 2 }));
  assert.equal(marks.length, 26);
  const lines = marks.filter((m): m is Extract<GridMark, { kind: "line" }> => m.kind === "line");
  const row0 = lines.find((l) => l.y0 === 0 && l.y1 === 0)!;
  assert.deepEqual([row0.x0, row0.x1, row0.width], [-1_000_000, 11_000_000, 4 / 1e-5]);
  const row3 = lines.find((l) => l.y0 === 3_000_000 && l.y1 === 3_000_000)!;
  assert.equal(row3.width, 2 / 1e-5);
  assert.equal(gridMarks(view1, 100, 100, spec("lines", { lineWidthPx: 0.5 })).find((m) => m.kind === "line")!.kind, "line");
  const thin = gridMarks(view1, 100, 100, spec("lines", { lineWidthPx: 0.5 })).filter((m): m is Extract<GridMark, { kind: "line" }> => m.kind === "line");
  assert.equal(Math.min(...thin.map((l) => l.width)), 1 / 1e-5, "max( 1.0f, m_gridLineWidth )");
});

test("gridMarks SMALL_CROSS: a cross at each crossing, 2 x the width each way, major only where both lines are ticks; the origin offsets the grid", () => {
  const marks = gridMarks(view1, 100, 100, spec("small-cross", { size: { x: 2_000_000, y: 2_000_000 }, minSpacingPx: 5, origin: [500_000, 0] }));
  const crosses = marks.filter((m): m is Extract<GridMark, { kind: "cross" }> => m.kind === "cross");
  assert.equal(crosses.length, marks.length);
  const origin = crosses.find((c) => c.x === 500_000 && c.y === 0)!;
  assert.deepEqual([origin.width, origin.len], [2 / 1e-5, 4 / 1e-5]);
  const other = crosses.find((c) => c.x === 2_500_000 && c.y === 0)!;
  assert.deepEqual([other.width, other.len], [1 / 1e-5, 2 / 1e-5]);
  // a grid that is not square: rows 1 mm apart, columns 2 mm
  const xy = gridMarks(view1, 100, 100, spec("dots", { size: { x: 2_000_000, y: 1_000_000 } }));
  const xs = new Set(xy.map((m) => (m.kind === "rect" ? m.x + m.w / 2 : NaN)));
  const ys = new Set(xy.map((m) => (m.kind === "rect" ? m.y + m.h / 2 : NaN)));
  assert.ok(xs.has(2_000_000) && !xs.has(1_000_000) && ys.has(1_000_000));
});

test("the Display Options choices: thickness 0.5 .. 10 by 0.5, minimum spacing 5 .. 200", () => {
  assert.equal(GRID_THICKNESS_CHOICES.length, 20);
  assert.deepEqual([GRID_THICKNESS_CHOICES[0], GRID_THICKNESS_CHOICES[19]], [0.5, 10]);
  assert.equal(gridLineWidthChoice(1.2), 1);
  assert.equal(gridLineWidthChoice(99), 10);
  assert.equal(gridMinSpacingValue(2), 5);
  assert.equal(gridMinSpacingValue(250), 200);
  assert.equal(gridMinSpacingValue(12.4), 12);
});
