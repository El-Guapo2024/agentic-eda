import { test } from "node:test";
import assert from "node:assert/strict";
import { SCREEN_DPI, ZOOM_LIST_EESCHEMA, ZOOM_LIST_PCBNEW, boxCentre, centerViewOn, fitMarginFactor, nearestZoomPreset, scaleToZoomFactor, setScaleAboutCentre, zoomFactorToScale, zoomFitBox, zoomListFor, zoomPresetScale } from "./zoomFit";
import { worldToScreen } from "./view";

test("zoom factor <-> scale: one inch takes factor * 91 pixels", () => {
  assert.equal(SCREEN_DPI, 91);
  // an inch (25 400 um) at zoom factor 1.0 is 91 px
  assert.ok(Math.abs(zoomFactorToScale(1) * 25_400 - 91) < 1e-9);
  assert.ok(Math.abs(scaleToZoomFactor(zoomFactorToScale(3.5)) - 3.5) < 1e-9);
});

test("the editors use their own zoom lists", () => {
  assert.equal(zoomListFor("pcb"), ZOOM_LIST_PCBNEW);
  assert.equal(zoomListFor("footprint"), ZOOM_LIST_PCBNEW);
  assert.equal(zoomListFor("schematic"), ZOOM_LIST_EESCHEMA);
  assert.equal(zoomListFor("symbol"), ZOOM_LIST_EESCHEMA);
  assert.equal(ZOOM_LIST_PCBNEW.length, 18);
  assert.equal(ZOOM_LIST_EESCHEMA.length, 21);
});

test("doZoomFit margin: 1.04, or 1.10 on a canvas shorter than 768 px", () => {
  assert.equal(fitMarginFactor(900), 1.04);
  assert.equal(fitMarginFactor(768), 1.04);
  assert.equal(fitMarginFactor(767), 1.1);
});

test("zoomFitBox frames the box with the margin and centres it", () => {
  // a 2000 x 1000 um box in an 800 x 800 canvas: the width is the tight side
  const v = zoomFitBox([1000, 500, 3000, 1500], [0, 0, 10, 10], 800, 800)!;
  assert.ok(Math.abs(v.scale - 800 / 2000 / 1.04) < 1e-12);
  const [cx, cy] = worldToScreen(v, 2000, 1000);
  assert.ok(Math.abs(cx - 400) < 1e-9 && Math.abs(cy - 400) < 1e-9, "the box centre lands at the canvas centre");
  const [x0] = worldToScreen(v, 1000, 1000);
  const [x1] = worldToScreen(v, 3000, 1000);
  assert.ok(x1 - x0 < 800, "the margin leaves room around the box");
});

test("zoomFitBox: a short canvas uses the bigger margin; the tight side is the height when the box is tall", () => {
  const v = zoomFitBox([0, 0, 100, 1000], [0, 0, 10, 10], 700, 500)!;
  assert.ok(Math.abs(v.scale - 500 / 1000 / 1.1) < 1e-12);
});

test("zoomFitBox: a box with no width or no height falls back to the default view box", () => {
  const v = zoomFitBox([5, 5, 5, 5], [0, 0, 400, 200], 800, 800)!;
  assert.ok(Math.abs(v.scale - 800 / 400 / 1.04) < 1e-12);
  const [cx, cy] = worldToScreen(v, 200, 100);
  assert.ok(Math.abs(cx - 400) < 1e-9 && Math.abs(cy - 400) < 1e-9);
  assert.equal(zoomFitBox([5, 5, 5, 5], [1, 1, 1, 9], 800, 800), null, "a degenerate default box too: nothing to frame");
  assert.equal(zoomFitBox([0, 0, 10, 10], [0, 0, 1, 1], 0, 800), null, "no canvas");
});

test("centerViewOn keeps the zoom and moves the centre", () => {
  const view = { scale: 0.002, x: 10, y: 20 };
  const v = centerViewOn(view, 600, 400, { x: 1234, y: -567 })!;
  assert.equal(v.scale, 0.002);
  const [cx, cy] = worldToScreen(v, 1234, -567);
  assert.ok(Math.abs(cx - 300) < 1e-9 && Math.abs(cy - 200) < 1e-9);
  assert.equal(centerViewOn({ scale: 0, x: 0, y: 0 }, 600, 400, { x: 0, y: 0 }), null, "a view that was never fitted has no scale to keep");
  assert.deepEqual(boxCentre([0, 10, 20, 30]), { x: 10, y: 20 });
});

test("zoomPresetScale: index 0 is Auto, N is the Nth list entry", () => {
  assert.deepEqual(zoomPresetScale(ZOOM_LIST_PCBNEW, 0), { auto: true });
  const p = zoomPresetScale(ZOOM_LIST_PCBNEW, 5);
  assert.equal(p.auto, false);
  if (!p.auto) assert.ok(Math.abs(p.scale - zoomFactorToScale(1.0)) < 1e-12, "the 5th PCB entry is 1.0");
  const last = zoomPresetScale(ZOOM_LIST_PCBNEW, 999);
  if (!last.auto) assert.ok(Math.abs(last.scale - zoomFactorToScale(300)) < 1e-9, "past the end stays on the last entry");
  assert.deepEqual(zoomPresetScale([], 3), { auto: true });
});

test("setScaleAboutCentre keeps the canvas centre fixed", () => {
  const view = { scale: 0.01, x: -50, y: 30 };
  const centre = { x: 400, y: 300 };
  const before = [(centre.x - view.x) / view.scale, (centre.y - view.y) / view.scale];
  const v = setScaleAboutCentre(view, 800, 600, 0.04);
  assert.equal(v.scale, 0.04);
  const after = [(centre.x - v.x) / v.scale, (centre.y - v.y) / v.scale];
  assert.ok(Math.abs(before[0]! - after[0]!) < 1e-9 && Math.abs(before[1]! - after[1]!) < 1e-9);
});

test("nearestZoomPreset picks the entry with the smallest relative error (1-based)", () => {
  assert.equal(nearestZoomPreset(ZOOM_LIST_PCBNEW, 1.0), 5);
  assert.equal(nearestZoomPreset(ZOOM_LIST_PCBNEW, 1.2), 5); // |1.0-1.2|/1.2 = .167, |1.5-1.2|/1.2 = .25
  assert.equal(nearestZoomPreset(ZOOM_LIST_PCBNEW, 1000), 18);
  assert.equal(nearestZoomPreset([], 1), 0);
});
