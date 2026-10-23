import { test } from "node:test";
import assert from "node:assert/strict";
import { computeVisibleGridSize, isMajorGridLine, DEFAULT_PCB_GRIDS_NM, DEFAULT_GRID_TICK } from "./grid";

test("computeVisibleGridSize: a comfortably visible grid is left untouched", () => {
  // 100um grid at 1 px/um = 100px apart on screen, well above the 10px threshold.
  assert.equal(computeVisibleGridSize(100_000, 1, "lines"), 100_000);
});

test("computeVisibleGridSize: a sub-pixel grid is coarsened by the tick factor until visible", () => {
  // 1um grid at 1 px/um = 1px apart -- below the 10px threshold, so it should
  // multiply by the tick (10) repeatedly: 1 -> 10 -> 100 (100px, first value > 10px threshold... actually 10px == threshold, loop continues while <=).
  const result = computeVisibleGridSize(1, 1, "lines");
  assert.equal(result, 100_000);
});

test("computeVisibleGridSize: small-cross needs a (strictly) larger margin than lines/dots before it stops coarsening", () => {
  // 20um grid at 1px/um = 20px apart. Lines' threshold is 10px, so 20px is
  // already comfortably above it (no coarsening). Small-cross doubles the
  // threshold to 20px, and the loop condition is "<=" -- so the same 20px
  // spacing is still AT small-cross's threshold and coarsens once more.
  const lines = computeVisibleGridSize(20_000, 1, "lines");
  const cross = computeVisibleGridSize(20_000, 1, "small-cross");
  assert.equal(lines, 20_000, "20px spacing already clears the 10px lines threshold");
  assert.equal(cross, 20 * DEFAULT_GRID_TICK, "20px spacing is exactly at small-cross's doubled (20px) threshold, so it coarsens once more");
});

test("computeVisibleGridSize: zooming in eventually needs no coarsening", () => {
  // 1um grid at 1000 px/um = 1000px apart, nowhere near the 10px threshold.
  assert.equal(computeVisibleGridSize(1, 1_000_000, "lines"), 1);
});

test("isMajorGridLine: every 10th index (from the origin) is major, matching DEFAULT_GRID_TICK", () => {
  assert.equal(DEFAULT_GRID_TICK, 10_000);
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
  assert.equal(DEFAULT_PCB_GRIDS_NM[11], 25.4); // 1 mil
  assert.equal(DEFAULT_PCB_GRIDS_NM[12], 5_000_000); // 5.0 mm -- the deliberate jump back up in size
  assert.equal(DEFAULT_PCB_GRIDS_NM[DEFAULT_PCB_GRIDS_NM.length - 1], 10); // 0.01 mm
  // Each mil-block entry is strictly decreasing; so is the mm block -- but
  // the list as a whole is NOT monotonic across the mil->mm boundary (that
  // jump is intentional, see this module's header comment).
  for (let i = 1; i < 12_000; i++) assert.ok(DEFAULT_PCB_GRIDS_NM[i]! < DEFAULT_PCB_GRIDS_NM[i - 1]!);
  for (let i = 13; i < DEFAULT_PCB_GRIDS_NM.length; i++) assert.ok(DEFAULT_PCB_GRIDS_NM[i]! < DEFAULT_PCB_GRIDS_NM[i - 1]!);
  assert.ok(DEFAULT_PCB_GRIDS_NM[12]! > DEFAULT_PCB_GRIDS_NM[11]!, "1 mil -> 5.0mm is a jump UP in size, not sorted");
});

test("computeVisibleGridSize: an unfitted view (scale 0) returns instead of looping forever", () => {
  assert.equal(computeVisibleGridSize(1_000_000, 0), 1_000_000);
  assert.equal(computeVisibleGridSize(1_000_000, Number.NaN), 1_000_000);
});
