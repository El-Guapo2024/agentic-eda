import { test } from "node:test";
import assert from "node:assert/strict";
import { netAtPoint } from "./schNetAtPoint";

const wires = [{ net: "VCC", pts: [[0, 0], [1000, 0]] as const }];
test("netAtPoint: wire hit, anchor hit, miss clears", () => {
  assert.equal(netAtPoint(wires, [], 500, 100, 200), "VCC");
  assert.equal(netAtPoint(wires, [{ net: "GND", at: [0, 900] }], 10, 880, 200), "GND");
  assert.equal(netAtPoint(wires, [], 500, 5000, 200), null);
});

test("netAtPoint: the accuracy grows in steps and the first that finds an item decides; a pin is hit through its box", () => {
  // a wire 120 um away is not found at 0 or at a quarter of 400 (100), but at a half (200)
  assert.equal(netAtPoint(wires, [], 500, 120, 400), "VCC");
  assert.equal(netAtPoint(wires, [], 500, 120, 300), "VCC", "found at the whole threshold");
  assert.equal(netAtPoint(wires, [], 500, 500, 400), null);
  // the nearest at the first step that finds anything: the wire (50 away) before an anchor (150 away)
  assert.equal(netAtPoint(wires, [{ net: "GND", at: [500, 200] }], 500, 50, 400), "VCC");
  // a pin whose box the point is in is found at accuracy 0; its net is the highlighted one
  assert.equal(netAtPoint([], [], 10, 10, 400, [{ net: "SDA", distUm: 0 }]), "SDA");
  // a pin 100 um from the point is within the floor (158.75 um), one 200 um from it needs the threshold to reach
  assert.equal(netAtPoint([], [], 0, 0, 100, [{ net: "SDA", distUm: 100 }]), "SDA");
  assert.equal(netAtPoint([], [], 0, 0, 100, [{ net: "SDA", distUm: 200 }]), null);
  assert.equal(netAtPoint([], [], 0, 0, 400, [{ net: "SDA", distUm: 200 }]), "SDA");
  // a pin on no net found: the highlight is cleared, not kept
  assert.equal(netAtPoint(wires, [], 500, 100, 400, [{ net: null, distUm: 0 }]), null);
});
