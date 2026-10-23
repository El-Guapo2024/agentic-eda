import { test } from "node:test";
import assert from "node:assert/strict";
import { cursorMove, panByGrid, viewCenter, warpViewToInclude, gridPresetIndex, fastGridCycleTarget, zoomToAreaView, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2 } from "./cursorControl";
import { worldToScreen } from "./view";

test("cursorMove: one grid cell per plain key, ten per Fast key (CursorControl fallthrough)", () => {
  const c = { x: 1_000_000, y: 2_000_000 };
  assert.deepEqual(cursorMove(c, 250_000, "up", false), { x: 1_000_000, y: 1_750_000 });
  assert.deepEqual(cursorMove(c, 250_000, "down", false), { x: 1_000_000, y: 2250 });
  assert.deepEqual(cursorMove(c, 250_000, "left", false), { x: 750_000, y: 2_000_000 });
  assert.deepEqual(cursorMove(c, 250_000, "right", false), { x: 1_250_000, y: 2_000_000 });
  assert.deepEqual(cursorMove(c, 250_000, "up", true), { x: 1_000_000, y: -500_000 });
  assert.deepEqual(cursorMove(c, 250_000, "right", true), { x: 3_500_000, y: 2_000_000 });
});

test("cursorMove: mirroredX flips only the horizontal sense", () => {
  const c = { x: 0, y: 0 };
  assert.deepEqual(cursorMove(c, 100_000, "left", false, true), { x: 100_000, y: 0 });
  assert.deepEqual(cursorMove(c, 100_000, "right", false, true), { x: -100_000, y: 0 });
  assert.deepEqual(cursorMove(c, 100_000, "up", false, true), { x: 0, y: -100_000 });
});

test("panByGrid moves the view centre by 10 grid cells (PanControl)", () => {
  const view = { scale: 0.5, x: 100_000, y: 50_000 };
  const before = viewCenter(view, 800_000, 600_000);
  const after = viewCenter(panByGrid(view, 800_000, 600_000, 1_000_000, "right"), 800_000, 600_000);
  assert.ok(Math.abs(after.x - (before.x + 10_000_000)) < 1e-6);
  assert.ok(Math.abs(after.y - before.y) < 1e-6);
  const down = viewCenter(panByGrid(view, 800_000, 600_000, 1_000_000, "down"), 800_000, 600_000);
  assert.ok(Math.abs(down.y - (before.y + 10_000_000)) < 1e-6);
});

test("warpViewToInclude: untouched while on screen, re-centres when off screen", () => {
  const view = { scale: 1, x: 0, y: 0 };
  assert.equal(warpViewToInclude(view, 800_000, 600_000, { x: 400_000, y: 300_000 }), view);
  const moved = warpViewToInclude(view, 800_000, 600_000, { x: 5_000_000, y: 5_000_000 });
  const [sx, sy] = worldToScreen(moved, 5_000_000, 5_000_000);
  assert.deepEqual([sx, sy], [400_000, 300_000]);
  assert.equal(moved.scale, 1);
});

test("fast grid helpers (GridPreset clamp, GridFastCycle)", () => {
  assert.equal(gridPresetIndex(99, 22_000), 21_000);
  assert.equal(gridPresetIndex(-3, 22_000), 0);
  assert.equal(fastGridCycleTarget(DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2), DEFAULT_FAST_GRID_2);
  assert.equal(fastGridCycleTarget(DEFAULT_FAST_GRID_2, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2), DEFAULT_FAST_GRID_1);
  assert.equal(fastGridCycleTarget(3000, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2), DEFAULT_FAST_GRID_1);
});

test("zoomToAreaView: left button zooms in so the box fills the limiting axis, centred", () => {
  const view = { scale: 1, x: 0, y: 0 };
  // 800x600 screen == 800x600 world; a 200x100 box -> ratio 0.25 -> scale 4.
  const z = zoomToAreaView(view, 800_000, 600_000, { x: 100_000, y: 100_000 }, { x: 300_000, y: 200_000 })!;
  assert.equal(z.scale, 4000);
  const c = viewCenter(z, 800_000, 600_000);
  assert.deepEqual([c.x, c.y], [200_000, 150_000]);
});

test("zoomToAreaView: right button zooms out by the same ratio; degenerate box is a no-op", () => {
  const view = { scale: 2, x: 0, y: 0 };
  // screen world = 400x300; box 200x150 -> ratio 0.5 -> scale * 0.5.
  const out = zoomToAreaView(view, 800_000, 600_000, { x: 0, y: 0 }, { x: 200_000, y: 150_000 }, true)!;
  assert.equal(out.scale, 2 * 0.5);
  assert.equal(zoomToAreaView(view, 800_000, 600_000, { x: 5000, y: 5000 }, { x: 5000, y: 90 }), null);
  assert.equal(zoomToAreaView(view, 800_000, 600_000, { x: 5000, y: 5000 }, { x: 90, y: 5000 }), null);
});
