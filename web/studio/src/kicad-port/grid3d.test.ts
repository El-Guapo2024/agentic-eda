import { test } from "node:test";
import assert from "node:assert/strict";
import { GRID3D_ACTIONS, grid3dLines } from "./grid3d";

test("generate3dGrid: none for NONE; a 10 mm grid of a 40 x 30 mm board spans 120 mm each way around its centre, every fifth line a marker", () => {
  assert.deepEqual(grid3dLines("none", [0, 0, 40, 30], 0.8), []);
  const lines = grid3dLines("10mm", [0, 0, 40, 30], 0.8);
  const flat = lines.filter((l) => l.a[1] === 0.8 && l.b[1] === 0.8);
  // max( size, 100 ) * 1.2 = 120: lines at the centre and +/- 10 .. 60 mm, both ways: 13 + 13, and the vertical grid's line across X at the middle plane
  assert.equal(flat.length, 27);
  const xs = flat.filter((l) => l.a[0] === l.b[0]).map((l) => l.a[0]).sort((a, b) => a - b);
  assert.deepEqual(xs, [-40, -30, -20, -10, 0, 10, 20, 30, 40, 50, 60, 70, 80]);
  assert.ok(flat.find((l) => l.a[0] === 20 && l.b[0] === 20)!.marker, "the centre line is a marker");
  assert.ok(flat.find((l) => l.a[0] === 70)!.marker && !flat.find((l) => l.a[0] === 30)!.marker, "every fifth from the centre");
  const z = flat.find((l) => l.a[0] === 20)!;
  assert.deepEqual([z.a[2], z.b[2]], [-45, 75]);
  // the vertical grid through the centre: from 50 mm under the board's middle to 100 mm over it
  const up = lines.filter((l) => l.a[0] === l.b[0] && l.a[2] === 15 && l.b[2] === 15 && l.a[1] !== l.b[1]);
  assert.ok(up.every((l) => Math.abs(l.a[1] - (0.8 - 50)) < 1e-9 && Math.abs(l.b[1] - 100.8) < 1e-9));
  assert.equal(GRID3D_ACTIONS["3DViewer.Control.show2_5mmGrid"], "2.5mm");
});
