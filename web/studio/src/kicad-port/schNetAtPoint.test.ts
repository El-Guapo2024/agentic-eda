import { test } from "node:test";
import assert from "node:assert/strict";
import { netAtPoint } from "./schNetAtPoint";

const wires = [{ net: "VCC", pts: [[0, 0], [1000, 0]] as const }];
test("netAtPoint: wire hit, anchor hit, miss clears", () => {
  assert.equal(netAtPoint(wires, [], 500, 100, 200), "VCC");
  assert.equal(netAtPoint(wires, [{ net: "GND", at: [0, 900] }], 10, 880, 200), "GND");
  assert.equal(netAtPoint(wires, [], 500, 5000, 200), null);
});
