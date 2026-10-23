import { test } from "node:test";
import assert from "node:assert/strict";
import { fitView, PREVIEW_SIZE, ringsBounds, ringsPath, worldAt, zoomAbout } from "./zonePreview";

test("ringsBounds: the box of every point of every ring, null for none", () => {
  assert.deepEqual(ringsBounds([[[0, 0], [10_000, 0], [10_000, 5000]], [[-3000, 2000], [4000, 9000], [0, 0]]]), [-3000, 0, 10_000, 9000]);
  assert.equal(ringsBounds([]), null);
  assert.equal(ringsBounds([[]]), null);
});

test("ringsPath: one closed subpath per ring, a ring of fewer than three points left out", () => {
  assert.equal(ringsPath([[[0, 0], [10_000, 0], [10_000, 5000]], [[1, 1], [2000, 2000]]]), "M0 0L10 0L10 5Z");
  assert.equal(ringsPath([[[0, 0], [1, 0], [1, 1]], [[5000, 5000], [6000, 5000], [6000, 6000]]]), "M0 0L1 0L1 1ZM5 5L6 5L6 6Z");
  assert.equal(ringsPath([]), "");
});

test("fitView: the board's box fits the canvas with a margin, centred; no box is the origin", () => {
  const v = fitView([0, 0, 27_600_000, 13_800_000]);
  assert.equal(v.cx, 13_800_000);
  assert.equal(v.cy, 6_900_000);
  assert.ok(Math.abs(v.scale - 27600 / (PREVIEW_SIZE.w * 0.92)) < 1e-9, "the wider side decides: 27.6 mm over 92% of the width");
  // The box's corners are inside the canvas.
  const [wx0] = worldAt(v, 0, PREVIEW_SIZE.h / 2);
  const [wx1] = worldAt(v, PREVIEW_SIZE.w, PREVIEW_SIZE.h / 2);
  assert.ok(wx0 < 0 && wx1 > 27_600_000);
  assert.deepEqual(fitView(null), { cx: 0, cy: 0, scale: 100 });
  assert.equal(fitView([5000, 5000, 5000, 5000]).scale, 1, "a point never asks for less than a micrometre a pixel");
});

test("zoomAbout: the point under the cursor stays under it, and the scale stays between 1 and a million", () => {
  const view = { cx: 1_000_000, cy: 2_000_000, scale: 50 };
  const [px, py] = [60, 40];
  const [wx, wy] = worldAt(view, px, py);
  const zoomed = zoomAbout(view, wx, wy, 0.8);
  assert.equal(zoomed.scale, 40_000);
  const [ax, ay] = worldAt(zoomed, px, py);
  assert.ok(Math.abs(ax - wx) < 1e-6 && Math.abs(ay - wy) < 1e-6, "the world point under the cursor is the same one");
  assert.equal(zoomAbout({ cx: 0, cy: 0, scale: 1 }, 0, 0, 0.5).scale, 1);
  assert.equal(zoomAbout({ cx: 0, cy: 0, scale: 900_000 }, 0, 0, 2000).scale, 1_000_000_000);
});
