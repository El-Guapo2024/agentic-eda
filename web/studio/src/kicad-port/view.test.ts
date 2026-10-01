import { test } from "node:test";
import assert from "node:assert/strict";
import { worldToScreen, screenToWorld, zoomAbout, fitTransform, boundsOfPoints, panByWorldDelta, MIN_SCALE, MAX_SCALE, type ViewTransform } from "./view";

test("worldToScreen / screenToWorld are inverses", () => {
  const v: ViewTransform = { scale: 2.5, x: 10, y: -4 };
  const [sx, sy] = worldToScreen(v, 1000, -500);
  const [wx, wy] = screenToWorld(v, sx, sy);
  assert.ok(Math.abs(wx - 1000) < 1e-9);
  assert.ok(Math.abs(wy - -500) < 1e-9);
});

test("zoomAbout keeps the anchor screen point fixed (view.cpp SetScale(scale, anchor) contract)", () => {
  const v: ViewTransform = { scale: 1, x: 50, y: 50 };
  const anchorScreen: [number, number] = [200, 150];
  const [wxBefore, wyBefore] = screenToWorld(v, ...anchorScreen);
  const zoomed = zoomAbout(v, anchorScreen[0], anchorScreen[1], 3);
  const [sxAfter, syAfter] = worldToScreen(zoomed, wxBefore, wyBefore);
  assert.ok(Math.abs(sxAfter - anchorScreen[0]) < 1e-9, "anchor x should stay under the cursor");
  assert.ok(Math.abs(syAfter - anchorScreen[1]) < 1e-9, "anchor y should stay under the cursor");
  assert.equal(zoomed.scale, 3);
});

test("zoomAbout clamps to [MIN_SCALE, MAX_SCALE]", () => {
  const v: ViewTransform = { scale: 1, x: 0, y: 0 };
  assert.equal(zoomAbout(v, 0, 0, 1e9).scale, MAX_SCALE);
  assert.equal(zoomAbout(v, 0, 0, 1e-9).scale, MIN_SCALE);
});

test("zoomAbout: two inverse factors in a row return to the original scale", () => {
  const v: ViewTransform = { scale: 4, x: 10, y: 10 };
  const zoomedIn = zoomAbout(v, 100, 100, 1.5);
  const back = zoomAbout(zoomedIn, 100, 100, 1 / 1.5);
  assert.ok(Math.abs(back.scale - v.scale) < 1e-9);
  assert.ok(Math.abs(back.x - v.x) < 1e-9);
  assert.ok(Math.abs(back.y - v.y) < 1e-9);
});

test("fitTransform centers bounds in the viewport with padding", () => {
  const bounds = { minX: 0, minY: 0, maxX: 1000, maxY: 500 };
  const v = fitTransform(bounds, 800, 600, 30);
  const [sx0, sy0] = worldToScreen(v, 0, 0);
  const [sx1, sy1] = worldToScreen(v, 1000, 500);
  assert.ok(sx0 >= 29 && sx0 <= 31 + 1e-6, `left pad ~30px, got ${sx0}`);
  assert.ok(sx1 <= 800 - 29, `right edge should be inside the viewport, got ${sx1}`);
  // Vertical: the 500-tall board is narrower relative to a 600px-tall box than
  // the 1000-wide board is to an 800px-wide box, so scale is bound by width,
  // and the board should be vertically centered rather than edge-padded.
  assert.ok(sy0 > 30, "vertically centered, not just top-padded");
  assert.ok(Math.abs(sy0 + sy1 - 600) < 1e-6, "vertically centered around the viewport middle");
});

test("boundsOfPoints: empty input is null, otherwise a tight box", () => {
  assert.equal(boundsOfPoints([]), null);
  assert.deepEqual(boundsOfPoints([[1, 2], [3, -4], [0, 5]]), { minX: 0, minY: -4, maxX: 3, maxY: 5 });
});

test("panByWorldDelta moves the board point under a fixed screen pixel by exactly the given world delta", () => {
  const v: ViewTransform = { scale: 2, x: 0, y: 0 };
  const screenPoint: [number, number] = [100, 100];
  const [wx0, wy0] = screenToWorld(v, ...screenPoint);
  const panned = panByWorldDelta(v, 50, -20);
  const [wx1, wy1] = screenToWorld(panned, ...screenPoint);
  // view.cpp SetCenter(GetCenter() + delta): the center moves BY +delta,
  // so a fixed screen pixel -- unmoved -- now reads a world point that is
  // also offset by +delta (the same board location is now delta further
  // from that pixel than before, equivalently: that pixel looks at the
  // spot delta-past where it used to).
  assert.ok(Math.abs(wx1 - (wx0 + 50)) < 1e-9);
  assert.ok(Math.abs(wy1 - (wy0 + -20)) < 1e-9);
});
