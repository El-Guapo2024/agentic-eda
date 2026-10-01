import { test } from "node:test";
import assert from "node:assert/strict";
import { ConstantZoomController, AcceleratingZoomController, CONSTANT_SCALE, pickDefaultZoomController } from "./zoomController";

test("ConstantZoomController: zooming in and out by the same rotation are exact inverses", () => {
  const c = new ConstantZoomController(CONSTANT_SCALE.MAC);
  const inFactor = c.getScaleForRotation(120);
  const outFactor = c.getScaleForRotation(-120);
  assert.ok(inFactor > 1, "positive rotation should zoom in (factor > 1)");
  assert.ok(outFactor < 1, "negative rotation should zoom out (factor < 1)");
  assert.ok(Math.abs(inFactor * outFactor - 1) < 1e-9, "in*out should cancel back to 1:1");
});

test("ConstantZoomController: rotation magnitude is clamped to 100", () => {
  const c = new ConstantZoomController(CONSTANT_SCALE.MSW);
  assert.equal(c.getScaleForRotation(1000), c.getScaleForRotation(100));
  assert.equal(c.getScaleForRotation(-1000), c.getScaleForRotation(-100));
});

test("ConstantZoomController: larger platform scale zooms faster per tick", () => {
  const mac = new ConstantZoomController(CONSTANT_SCALE.MAC); // 0.01
  const gtk3 = new ConstantZoomController(CONSTANT_SCALE.GTK3); // 0.002
  assert.ok(mac.getScaleForRotation(120) > gtk3.getScaleForRotation(120));
});

test("AcceleratingZoomController: ticks within the timeout in the same direction accelerate", () => {
  let now = 0;
  const clock = () => now;
  const c = new AcceleratingZoomController(5.0, 500, clock);
  const first = c.getScaleForRotation(120); // no history yet -> MIN_STEP (1.05)
  now += 50; // well within the 500ms timeout
  const second = c.getScaleForRotation(120); // same direction, fast -> accelerated
  assert.ok(second > first, `expected acceleration on a fast repeat tick: ${first} vs ${second}`);
});

test("AcceleratingZoomController: a tick after the timeout resets to the unaccelerated minimum step", () => {
  let now = 0;
  const clock = () => now;
  const c = new AcceleratingZoomController(5.0, 500, clock);
  c.getScaleForRotation(120);
  now += 50;
  const accelerated = c.getScaleForRotation(120);
  now += 1000; // past the 500ms timeout
  const reset = c.getScaleForRotation(120);
  assert.ok(reset < accelerated);
  assert.ok(Math.abs(reset - 1.05) < 1e-9);
});

test("AcceleratingZoomController: reversing direction does not accelerate (falls back to MIN_STEP)", () => {
  let now = 0;
  const clock = () => now;
  const c = new AcceleratingZoomController(5.0, 500, clock);
  c.getScaleForRotation(120);
  now += 50;
  const reversed = c.getScaleForRotation(-120); // direction flipped -> no acceleration
  assert.ok(Math.abs(reversed - 1 / 1.05) < 1e-9);
});

test("pickDefaultZoomController: Mac is always constant; non-Mac is constant unless acceleration is explicitly enabled", () => {
  assert.ok(pickDefaultZoomController(true) instanceof ConstantZoomController);
  assert.ok(pickDefaultZoomController(false) instanceof ConstantZoomController);
  assert.ok(pickDefaultZoomController(false, true) instanceof AcceleratingZoomController);
  // Mac ignores the acceleration setting entirely, same as source's #ifdef __WXMAC__ branch.
  assert.ok(pickDefaultZoomController(true, true) instanceof ConstantZoomController);
});
