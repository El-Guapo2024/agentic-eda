import { test } from "node:test";
import assert from "node:assert/strict";
import { angleDegToOrientation, orientationToAngleDeg } from "./pinOrientation";

test("right/left/up/down round-trip through angle_deg", () => {
  for (const o of ["right", "left", "up", "down"] as const) {
    assert.equal(angleDegToOrientation(orientationToAngleDeg(o)), o);
  }
});

test("matches this app's own already-verified builtin symbols (crates/model/src/symbol.rs)", () => {
  // conn_01x's pins sit left of the body (stub pointing left) at angle 0.
  assert.equal(angleDegToOrientation(0), "left");
  // device_d's pin 2 sits right of the body (stub pointing right) at angle 180.
  assert.equal(angleDegToOrientation(180), "right");
  // device_r's pin 1 sits above the body (stub pointing up) at angle 270.
  assert.equal(angleDegToOrientation(270), "up");
  // device_r's pin 2 sits below the body (stub pointing down) at angle 90.
  assert.equal(angleDegToOrientation(90), "down");
});

test("a stray non-cardinal angle snaps to the nearest cardinal orientation", () => {
  assert.equal(angleDegToOrientation(10), "left");
  assert.equal(angleDegToOrientation(100), "down");
});
