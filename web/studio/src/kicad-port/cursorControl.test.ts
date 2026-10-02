import { test } from "node:test";
import assert from "node:assert/strict";
import { cursorMove, panByGrid, viewCenter, warpViewToInclude, gridPresetIndex, fastGridCycleTarget, zoomToAreaView, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2 } from "./cursorControl";
import { worldToScreen } from "./view";

test("cursorMove: one grid cell per plain key, ten per Fast key (CursorControl fallthrough)", () => {
  const c = { x: 1000, y: 2000 };
  assert.deepEqual(cursorMove(c, 250, "up", false), { x: 1000, y: 1750 });
  assert.deepEqual(cursorMove(c, 250, "down", false), { x: 1000, y: 2250 });
  assert.deepEqual(cursorMove(c, 250, "left", false), { x: 750, y: 2000 });
  assert.deepEqual(cursorMove(c, 250, "right", false), { x: 1250, y: 2000 });
  assert.deepEqual(cursorMove(c, 250, "up", true), { x: 1000, y: -500 });
  assert.deepEqual(cursorMove(c, 250, "right", true), { x: 3500, y: 2000 });
});

test("cursorMove: mirroredX flips only the horizontal sense", () => {
  const c = { x: 0, y: 0 };
  assert.deepEqual(cursorMove(c, 100, "left", false, true), { x: 100, y: 0 });
  assert.deepEqual(cursorMove(c, 100, "right", false, true), { x: -100, y: 0 });
  assert.deepEqual(cursorMove(c, 100, "up", false, true), { x: 0, y: -100 });
});

test("panByGrid moves the view centre by 10 grid cells (PanControl)", () => {
  const view = { scale: 0.5, x: 100, y: 50 };
  const before = viewCenter(view, 800, 600);
  const after = viewCenter(panByGrid(view, 800, 600, 1000, "right"), 800, 600);
  assert.ok(Math.abs(after.x - (before.x + 10000)) < 1e-6);
  assert.ok(Math.abs(after.y - before.y) < 1e-6);
  const down = viewCenter(panByGrid(view, 800, 600, 1000, "down"), 800, 600);
  assert.ok(Math.abs(down.y - (before.y + 10000)) < 1e-6);
});

test("warpViewToInclude: untouched while on screen, re-centres when off screen", () => {
  const view = { scale: 1, x: 0, y: 0 };
  assert.equal(warpViewToInclude(view, 800, 600, { x: 400, y: 300 }), view);
  const moved = warpViewToInclude(view, 800, 600, { x: 5000, y: 5000 });
  const [sx, sy] = worldToScreen(moved, 5000, 5000);
  assert.deepEqual([sx, sy], [400, 300]);
  assert.equal(moved.scale, 1);
});

test("fast grid helpers (GridPreset clamp, GridFastCycle)", () => {
  assert.equal(gridPresetIndex(99, 22), 21);
  assert.equal(gridPresetIndex(-3, 22), 0);
  assert.equal(fastGridCycleTarget(DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2), DEFAULT_FAST_GRID_2);
  assert.equal(fastGridCycleTarget(DEFAULT_FAST_GRID_2, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2), DEFAULT_FAST_GRID_1);
  assert.equal(fastGridCycleTarget(3, DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2), DEFAULT_FAST_GRID_1);
});

test("zoomToAreaView: left button zooms in so the box fills the limiting axis, centred", () => {
  const view = { scale: 1, x: 0, y: 0 };
  // 800x600 screen == 800x600 world; a 200x100 box -> ratio 0.25 -> scale 4.
  const z = zoomToAreaView(view, 800, 600, { x: 100, y: 100 }, { x: 300, y: 200 })!;
  assert.equal(z.scale, 4);
  const c = viewCenter(z, 800, 600);
  assert.deepEqual([c.x, c.y], [200, 150]);
});

test("zoomToAreaView: right button zooms out by the same ratio; degenerate box is a no-op", () => {
  const view = { scale: 2, x: 0, y: 0 };
  // screen world = 400x300; box 200x150 -> ratio 0.5 -> scale * 0.5.
  const out = zoomToAreaView(view, 800, 600, { x: 0, y: 0 }, { x: 200, y: 150 }, true)!;
  assert.equal(out.scale, 2 * 0.5);
  assert.equal(zoomToAreaView(view, 800, 600, { x: 5, y: 5 }, { x: 5, y: 90 }), null);
  assert.equal(zoomToAreaView(view, 800, 600, { x: 5, y: 5 }, { x: 90, y: 5 }), null);
});
