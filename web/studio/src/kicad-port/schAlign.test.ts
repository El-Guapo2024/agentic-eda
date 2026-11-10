import { test } from "node:test";
import assert from "node:assert/strict";
import { alignMoves, type SchAlignItem } from "./schAlign";

const item = (id: string, box: [number, number, number, number], locked = false): SchAlignItem => ({ id, box, locked });
const A = item("A", [100, 0, 300, 50]);
const B = item("B", [400, 100, 500, 400]);
const C = item("C", [250, 60, 330, 90]);

test("Align Left puts every left edge on the leftmost item's, Align Right on the rightmost's", () => {
  assert.deepEqual(alignMoves([A, B, C], "left", null), [{ id: "A", dx: 0, dy: 0 }, { id: "C", dx: -150, dy: 0 }, { id: "B", dx: -300, dy: 0 }]);
  assert.deepEqual(alignMoves([A, B, C], "right", null), [{ id: "B", dx: 0, dy: 0 }, { id: "C", dx: 170, dy: 0 }, { id: "A", dx: 200, dy: 0 }]);
});

test("Align Top, Bottom and the two centres work along the other axis", () => {
  assert.deepEqual(alignMoves([A, B, C], "top", null).map((m) => [m.id, m.dy]), [["A", 0], ["C", -60], ["B", -100]]);
  assert.deepEqual(alignMoves([A, B, C], "bottom", null).map((m) => [m.id, m.dy]), [["B", 0], ["C", 310], ["A", 350]]);
  assert.deepEqual(alignMoves([A, B], "centerX", null).map((m) => [m.id, m.dx]), [["A", 0], ["B", -250]]);
  assert.deepEqual(alignMoves([A, B], "centerY", null).map((m) => [m.id, m.dy]), [["A", 0], ["B", -225]]);
});

test("the item under the cursor is the target when nothing is locked", () => {
  // B is under the cursor: A and C move to B's left edge (the items come back in the sort order, leftmost first)
  assert.deepEqual(alignMoves([A, B, C], "left", [450, 200]), [{ id: "A", dx: 300, dy: 0 }, { id: "C", dx: 150, dy: 0 }, { id: "B", dx: 0, dy: 0 }]);
});

test("a locked item is the target and never moves", () => {
  const locked = item("L", [700, 0, 800, 50], true);
  const out = alignMoves([A, B, locked], "left", null);
  assert.deepEqual(out, [{ id: "A", dx: 600, dy: 0 }, { id: "B", dx: 300, dy: 0 }]);
  assert.deepEqual(alignMoves([locked, item("L2", [0, 0, 5, 5], true)], "left", null), [], "nothing can move");
  assert.deepEqual(alignMoves([A], "left", null), [], "a lone item has nothing to align to");
});

test("a group is one item, its offset snapped by its centre to the connection grid on both axes (UI-LEFT-2 SS18)", () => {
  // the group's box [1000, 0, 3100, 1300] (centre 2050, 650); the item to its left at 0. Align Left moves the group by -1000, its centre to 1050, which
  // the grid of 100 takes to 1100 (and 650 to 700: off the grid across the line too)
  const left = item("A", [0, 2000, 100, 2100], false);
  const group = { ...item("grp", [1000, 0, 3100, 1300], false), groupGridNm: 100 };
  assert.deepEqual(alignMoves([left, group], "left", null), [{ id: "A", dx: 0, dy: 0 }, { id: "grp", dx: -950, dy: 50 }]);
  // a group not on the connection grid (graphics alone) is moved as measured
  assert.deepEqual(alignMoves([left, item("grp", [1000, 0, 3100, 1300], false)], "left", null)[1], { id: "grp", dx: -1000, dy: 0 });
});
