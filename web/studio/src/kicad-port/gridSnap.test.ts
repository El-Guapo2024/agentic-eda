import { test } from "node:test";
import assert from "node:assert/strict";
import { computeNearest, alignToGrid } from "./gridSnap";

test("computeNearest rounds to the nearest grid multiple from an arbitrary origin", () => {
  assert.deepEqual(computeNearest({ x: 1040, y: -1040 }, { x: 1000, y: 1000 }, { x: 0, y: 0 }), { x: 1000, y: -1000 });
  assert.deepEqual(computeNearest({ x: 1060, y: 0 }, { x: 1000, y: 1000 }, { x: 0, y: 0 }), { x: 1000, y: 0 });
  // Offset origin: grid lines are at ...,-500,500,1500,... not multiples of 1000.
  assert.deepEqual(computeNearest({ x: 1200, y: 0 }, { x: 1000, y: 1000 }, { x: 500, y: 0 }), { x: 1500, y: 0 });
});

test("computeNearest: a zero grid size means no rounding (passthrough)", () => {
  assert.deepEqual(computeNearest({ x: 1234, y: 5678 }, { x: 0, y: 0 }, { x: 0, y: 0 }), { x: 1234, y: 5678 });
});

test("alignToGrid: Ctrl disables the grid round-off entirely (tool_event.h DisableGridSnapping, Modifier(MD_CTRL))", () => {
  const point = { x: 1040, y: 1040 };
  assert.deepEqual(alignToGrid(point, 1000, { x: 0, y: 0 }, { ctrlOrCmd: false }), { x: 1000, y: 1000 });
  assert.deepEqual(alignToGrid(point, 1000, { x: 0, y: 0 }, { ctrlOrCmd: true }), point, "Ctrl held -> full precision, no rounding");
});
