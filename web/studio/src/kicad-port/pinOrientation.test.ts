import { test } from "node:test";
import assert from "node:assert/strict";
import { angleDegToOrientation, orientationToAngleDeg } from "./pinOrientation";

test("right/left/up/down round-trip through angle_deg", () => {
  for (const o of ["right", "left", "up", "down"] as const) {
    assert.equal(angleDegToOrientation(orientationToAngleDeg(o)), o);
  }
});

test("the file angle as parseSymbolPin reads it: 0 PIN_RIGHT, 90 PIN_UP, 180 PIN_LEFT, 270 PIN_DOWN (r7 E7-S1)", () => {
  assert.equal(angleDegToOrientation(0), "right");
  assert.equal(angleDegToOrientation(90), "up");
  assert.equal(angleDegToOrientation(180), "left");
  assert.equal(angleDegToOrientation(270), "down");
  assert.equal(orientationToAngleDeg("right"), 0);
  assert.equal(orientationToAngleDeg("up"), 90);
  assert.equal(orientationToAngleDeg("left"), 180);
  assert.equal(orientationToAngleDeg("down"), 270);
});

test("a stray non-cardinal angle snaps to the nearest cardinal orientation", () => {
  assert.equal(angleDegToOrientation(10), "right");
  assert.equal(angleDegToOrientation(100), "up");
  assert.equal(angleDegToOrientation(-90), "down");
});
