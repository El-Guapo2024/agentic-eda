import { test } from "node:test";
import assert from "node:assert/strict";
import { handleWheel, computeAutoPanDirection, computeAutoPanStep, DEFAULT_VIEW_CONTROL_SETTINGS, type WheelInput } from "./viewControls";
import { ConstantZoomController, CONSTANT_SCALE } from "./zoomController";
import { screenToWorld, type ViewTransform } from "./view";

function wheel(partial: Partial<WheelInput>): WheelInput {
  return { deltaX: 0, deltaY: 0, shiftKey: false, ctrlOrCmd: false, altKey: false, x: 400, y: 300, ...partial };
}

test("DEFAULT_VIEW_CONTROL_SETTINGS matches KiCad's shipped defaults", () => {
  assert.equal(DEFAULT_VIEW_CONTROL_SETTINGS.autoPanEnabled, false, "auto-pan is OFF out of the box");
  assert.equal(DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin, 0.02);
  assert.equal(DEFAULT_VIEW_CONTROL_SETTINGS.autoPanAcceleration, 5.0);
  assert.equal(DEFAULT_VIEW_CONTROL_SETTINGS.dragMiddle, "pan");
  assert.equal(DEFAULT_VIEW_CONTROL_SETTINGS.dragRight, "pan");
  assert.equal(DEFAULT_VIEW_CONTROL_SETTINGS.scrollModifierZoom, "none");
  assert.equal(DEFAULT_VIEW_CONTROL_SETTINGS.scrollModifierPanH, "ctrl");
});

test("handleWheel: plain wheel zooms about the cursor, scrolling up (negative deltaY) zooms in", () => {
  const view: ViewTransform = { scale: 1, x: 0, y: 0 };
  const zc = new ConstantZoomController(CONSTANT_SCALE.MSW);
  const before = screenToWorld(view, 400, 300);
  const result = handleWheel(view, { width: 800, height: 600 }, wheel({ deltaY: -100 }), DEFAULT_VIEW_CONTROL_SETTINGS, zc);
  assert.equal(result.kind, "zoom");
  assert.ok(result.view.scale > 1, "scrolling up should zoom in");
  const after = screenToWorld(result.view, 400, 300);
  assert.ok(Math.abs(after[0] - before[0]) < 1e-9, "cursor's world point should stay under the cursor");
  assert.ok(Math.abs(after[1] - before[1]) < 1e-9);
});

test("handleWheel: scrolling down (positive deltaY) zooms out", () => {
  const view: ViewTransform = { scale: 1, x: 0, y: 0 };
  const zc = new ConstantZoomController(CONSTANT_SCALE.MSW);
  const result = handleWheel(view, { width: 800, height: 600 }, wheel({ deltaY: 100 }), DEFAULT_VIEW_CONTROL_SETTINGS, zc);
  assert.ok(result.view.scale < 1);
});

test("handleWheel: Ctrl+wheel pans horizontally instead of zooming (scrollModifierPanH default)", () => {
  const view: ViewTransform = { scale: 1, x: 0, y: 0 };
  const zc = new ConstantZoomController(CONSTANT_SCALE.MSW);
  const result = handleWheel(view, { width: 800, height: 600 }, wheel({ deltaY: -100, ctrlOrCmd: true }), DEFAULT_VIEW_CONTROL_SETTINGS, zc);
  assert.equal(result.kind, "pan");
  assert.equal(result.view.scale, 1, "pan must not change zoom");
  assert.notEqual(result.view.x, view.x, "x should move for a horizontal pan");
  assert.equal(result.view.y, view.y, "y should NOT move for a horizontal pan");
});

test("handleWheel: Shift+wheel pans vertically (doesn't match the zoom or pan-h modifier)", () => {
  const view: ViewTransform = { scale: 1, x: 0, y: 0 };
  const zc = new ConstantZoomController(CONSTANT_SCALE.MSW);
  const result = handleWheel(view, { width: 800, height: 600 }, wheel({ deltaY: -100, shiftKey: true }), DEFAULT_VIEW_CONTROL_SETTINGS, zc);
  assert.equal(result.kind, "pan");
  assert.equal(result.view.x, view.x, "x should NOT move for a vertical pan");
  assert.notEqual(result.view.y, view.y, "y should move for a vertical pan");
});

test("handleWheel: two modifiers at once is left unhandled (forwarded to tools in source)", () => {
  const view: ViewTransform = { scale: 1, x: 0, y: 0 };
  const zc = new ConstantZoomController(CONSTANT_SCALE.MSW);
  const result = handleWheel(view, { width: 800, height: 600 }, wheel({ deltaY: -100, shiftKey: true, ctrlOrCmd: true }), DEFAULT_VIEW_CONTROL_SETTINGS, zc);
  assert.equal(result.kind, "unhandled");
  assert.deepEqual(result.view, view);
});

test("handleWheel: a dominant horizontal delta always pans horizontally, regardless of modifiers", () => {
  const view: ViewTransform = { scale: 1, x: 0, y: 0 };
  const zc = new ConstantZoomController(CONSTANT_SCALE.MSW);
  const result = handleWheel(view, { width: 800, height: 600 }, wheel({ deltaX: -50, ctrlOrCmd: true, shiftKey: true }), DEFAULT_VIEW_CONTROL_SETTINGS, zc);
  assert.equal(result.kind, "pan-horizontal");
  assert.notEqual(result.view.x, view.x);
  assert.equal(result.view.y, view.y);
});

test("handleWheel: reverseScrollZoom flips the zoom direction", () => {
  const view: ViewTransform = { scale: 1, x: 0, y: 0 };
  const zc = new ConstantZoomController(CONSTANT_SCALE.MSW);
  const settings = { ...DEFAULT_VIEW_CONTROL_SETTINGS, reverseScrollZoom: true };
  const result = handleWheel(view, { width: 800, height: 600 }, wheel({ deltaY: -100 }), settings, zc);
  assert.ok(result.view.scale < 1, "reversed: scrolling up now zooms out");
});

test("computeAutoPanDirection: center of the screen is outside the autopan border", () => {
  const dir = computeAutoPanDirection({ x: 400, y: 300 }, { width: 800, height: 600 });
  assert.deepEqual(dir, { x: 0, y: 0 });
});

test("computeAutoPanDirection: near the left/top edge gives a negative direction sized to the penetration depth", () => {
  // margin 0.02 * min(800,600)=600 -> 12, but border = max(min(0.02*800, 0.02*600), 2) = max(min(16,12),2) = 12
  const dir = computeAutoPanDirection({ x: 5, y: 300 }, { width: 800, height: 600 });
  assert.equal(dir.x, -(12 - 5));
  assert.equal(dir.y, 0);
});

test("computeAutoPanDirection: near the right/bottom edge gives a positive direction", () => {
  const dir = computeAutoPanDirection({ x: 795, y: 300 }, { width: 800, height: 600 });
  assert.equal(dir.x, 795 - (800 - 12));
});

test("computeAutoPanStep: no direction means no pan", () => {
  assert.equal(computeAutoPanStep({ x: 0, y: 0 }, { width: 800, height: 600 }, 1), null);
});

test("computeAutoPanStep: far past the border is accelerated to borderSize*accel", () => {
  const screenSize = { width: 800, height: 600 };
  const borderSize = Math.min(0.02 * 800, 0.02 * 600); // 12
  const accel = 0.5 + 5.0 / 5.0; // 1.5 at the default acceleration setting
  const step = computeAutoPanStep({ x: -borderSize * 10, y: 0 }, screenSize, 1); // length 120 >= borderSize(12)
  assert.ok(step);
  const pixelMagnitude = Math.hypot(step!.x, step!.y) * 1; // scalePxPerUm=1, so world delta == pixel magnitude
  assert.ok(Math.abs(pixelMagnitude - borderSize * accel) < 1e-9);
});

test("computeAutoPanStep: a world-scale > 1 shrinks the resulting world-space pan", () => {
  const screenSize = { width: 800, height: 600 };
  const atScale1 = computeAutoPanStep({ x: -100, y: 0 }, screenSize, 1)!;
  const atScale2 = computeAutoPanStep({ x: -100, y: 0 }, screenSize, 2)!;
  assert.ok(Math.abs(atScale2.x) < Math.abs(atScale1.x));
  assert.ok(Math.abs(Math.abs(atScale1.x) / Math.abs(atScale2.x) - 2) < 1e-9);
});

test("computeAutoPanStep: just barely into the border (<= half border) uses the raw distance, unaccelerated", () => {
  const screenSize = { width: 800, height: 600 };
  const borderSize = 12;
  const tiny = borderSize * 0.3; // < borderSize/2
  const step = computeAutoPanStep({ x: -tiny, y: 0 }, screenSize, 1)!;
  assert.ok(Math.abs(Math.abs(step.x) - tiny) < 1e-9);
});
