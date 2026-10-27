import { test } from "node:test";
import assert from "node:assert/strict";
import { isTerminalPoint } from "./schTerminalPoint";

const sch = (extra: Record<string, unknown> = {}) => ({ wires: [], labels: [], bus_entries: [], junctions: [], sheets: [], lines: [], ...extra }) as unknown as Parameters<typeof isTerminalPoint>[0];

test("a wire finishes on a pin, a junction, another wire's middle, a connected label (r3 C3-M3: SCH_SCREEN::IsTerminalPoint)", () => {
  const s = sch({ wires: [{ pts: [[0, 0], [100, 0]], bus: false }], junctions: [{ at: [300, 300] }], labels: [{ at: [100, 0] }, { at: [500, 500] }] });
  assert.equal(isTerminalPoint(s, [[700, 700]], [700, 700], "wire"), true, "a pin");
  assert.equal(isTerminalPoint(s, [], [50, 0], "wire"), true, "the middle of a wire (a T)");
  assert.equal(isTerminalPoint(s, [], [300, 300], "wire"), true, "a junction");
  assert.equal(isTerminalPoint(s, [], [500, 500], "wire"), false, "a label nothing reaches is not connected");
  assert.equal(isTerminalPoint(s, [], [50, 50], "wire"), false, "empty space");
});

test("a bus never finishes on a pin; a line finishes on another line", () => {
  const s = sch({ wires: [{ pts: [[0, 0], [100, 0]], bus: true }], lines: [{ pts: [[0, 10], [10, 10]] }] });
  assert.equal(isTerminalPoint(s, [[700, 700]], [700, 700], "bus"), false);
  assert.equal(isTerminalPoint(s, [], [50, 0], "bus"), true);
  assert.equal(isTerminalPoint(s, [], [50, 0], "wire"), false, "a wire does not finish on a bus");
  assert.equal(isTerminalPoint(s, [], [5, 10], "notes"), true);
});

test("a label on a pin end with no wire is connected: a bus finishes on it (r4 C4-L3: SCH_ITEM::IsConnected counts the pin)", () => {
  const s = sch({ labels: [{ at: [700, 700] }] });
  assert.equal(isTerminalPoint(s, [[700, 700]], [700, 700], "bus"), true);
  assert.equal(isTerminalPoint(s, [], [700, 700], "bus"), false, "nothing there but the label");
});
