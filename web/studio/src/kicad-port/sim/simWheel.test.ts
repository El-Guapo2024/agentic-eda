import { test } from "node:test";
import assert from "node:assert/strict";
import { fit, newView } from "./mathPlot";
import { DEFAULT_SIM_WHEEL_ACTIONS, performSimWheelAction, sanitizeSimWheelActions, simWheelAction, TRACKPAD_SIM_WHEEL_ACTIONS } from "./simWheel";

const ev = (o: Partial<{ horizontal: boolean; ctrl: boolean; shift: boolean; alt: boolean }> = {}) => ({ horizontal: false, ctrl: false, shift: false, alt: false, ...o });

test("onMouseWheel: the row of the one modifier held, the horizontal row for a sideways wheel, nothing with two modifiers", () => {
  const a = DEFAULT_SIM_WHEEL_ACTIONS;
  assert.equal(simWheelAction(a, ev()), "zoom");
  assert.equal(simWheelAction(a, ev({ ctrl: true })), "pan_left_right");
  assert.equal(simWheelAction(a, ev({ shift: true })), "pan_up_down");
  assert.equal(simWheelAction(a, ev({ alt: true })), "none");
  assert.equal(simWheelAction(a, ev({ horizontal: true, ctrl: true })), "none");
  assert.equal(simWheelAction(a, ev({ ctrl: true, shift: true })), null, "event.Skip()");
  assert.equal(simWheelAction(TRACKPAD_SIM_WHEEL_ACTIONS, ev()), "pan_up_down");
  assert.equal(simWheelAction(TRACKPAD_SIM_WHEEL_ACTIONS, ev({ horizontal: true })), "pan_left_right");
});

test("PerformMouseWheelAction: the pans move the view by change / scale, the one-way zooms leave the other axis", () => {
  const v = newView();
  fit(v);
  // zoomed in, so the view has room to pan inside its limits (`LimitView( true )`)
  for (let i = 0; i < 5; i++) performSimWheelAction(v, "zoom", 1, { x: 320, y: 200 });
  const x0 = v.posX;
  performSimWheelAction(v, "pan_left_right", 10, { x: 0, y: 0 });
  const x1 = v.posX;
  assert.ok(Math.abs(x1 - x0 - 10 / v.scaleX) < 1e-9, `${x0} -> ${x1}`);
  performSimWheelAction(v, "pan_right_left", 10, { x: 0, y: 0 });
  assert.ok(Math.abs(v.posX - x0) < 1e-9);
  const before = { x: v.desiredXmax - v.desiredXmin, y: v.desiredYmax - v.desiredYmin };
  performSimWheelAction(v, "zoom_horizontally", 1, { x: 320, y: 200 });
  assert.ok(v.desiredXmax - v.desiredXmin < before.x, "narrower in x");
  assert.equal(v.desiredYmax - v.desiredYmin, before.y, "y untouched");
  performSimWheelAction(v, "none", 1, { x: 0, y: 0 });
});

test("the stored actions: KiCad's defaults for anything unknown; the horizontal row takes only its three", () => {
  assert.deepEqual(sanitizeSimWheelActions(undefined), DEFAULT_SIM_WHEEL_ACTIONS);
  assert.deepEqual(sanitizeSimWheelActions({ verticalUnmodified: "zoom_vertically", verticalWithAlt: "bogus", horizontal: "pan_up_down" }), { ...DEFAULT_SIM_WHEEL_ACTIONS, verticalUnmodified: "zoom_vertically", horizontal: "none" });
});
