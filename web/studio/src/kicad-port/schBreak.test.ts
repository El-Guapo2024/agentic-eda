import { test } from "node:test";
import assert from "node:assert/strict";
import { breakCmds, cutPieces, nearestSegment, startBreak, type BreakSource } from "./schBreak";

const wire = (id: string, pts: Array<[number, number]>): BreakSource => ({ id, kind: "wire", pts });

test("a single wire is cut where the cursor is", () => {
  const s = startBreak("break", [wire("w1", [[0, 0], [10000, 0]])], [4000, 0])!;
  assert.equal(s.cuts.length, 1);
  assert.deepEqual(s.cuts[0]!.at, [4000, 0]);
});

test("several wires are each cut at their own midpoint", () => {
  const s = startBreak("break", [wire("w1", [[0, 0], [10000, 0]]), wire("w2", [[0, 5000], [0, 9000]])], [3000, 0])!;
  assert.deepEqual(s.cuts.map((c) => c.at), [[5000, 0], [0, 7000]]);
});

test("a polyline is cut in its segment nearest the cursor", () => {
  const pts: Array<[number, number]> = [[0, 0], [5000, 0], [5000, 5000]];
  assert.equal(nearestSegment(pts, [5000, 4000]), 1);
  assert.equal(nearestSegment(pts, [1000, 100]), 0);
});

test("break: the point the two halves share follows the cursor, so the wire bends", () => {
  const s = startBreak("break", [wire("w1", [[0, 0], [10000, 0]])], [4000, 0])!;
  const pieces = cutPieces(s, s.cuts[0]!, [4000, 2000]);
  assert.deepEqual(pieces, [[[0, 0], [4000, 2000]], [[4000, 2000], [10000, 0]]]);
});

test("slice: the first half's end follows the cursor, the second half stays where it was", () => {
  const s = startBreak("slice", [wire("w1", [[0, 0], [10000, 0]])], [4000, 0])!;
  const pieces = cutPieces(s, s.cuts[0]!, [2000, 3000]);
  assert.deepEqual(pieces, [[[0, 0], [2000, 3000]], [[4000, 0], [10000, 0]]]);
});

test("a piece that collapses to a point is dropped", () => {
  const s = startBreak("break", [wire("w1", [[0, 0], [10000, 0]])], [0, 0])!;
  // The cut is at the very start and the cursor has not moved: the first half has no length.
  assert.deepEqual(cutPieces(s, s.cuts[0]!, [0, 0]), [[[0, 0], [10000, 0]]]);
});

test("the verbs delete each cut line and add its pieces, a bus staying a bus", () => {
  const bus: BreakSource = { id: "b1", kind: "wire", pts: [[0, 0], [8000, 0]], bus: true };
  const s = startBreak("break", [bus], [4000, 0])!;
  assert.deepEqual(breakCmds(s, [4000, 0]), [
    { op: "delete_wire", id: "b1" },
    { op: "add_wire", pts: [{ x: 0, y: 0 }, { x: 4000, y: 0 }], bus: true },
    { op: "add_wire", pts: [{ x: 4000, y: 0 }, { x: 8000, y: 0 }], bus: true },
  ]);
});

test("a graphic line is replaced through the line verbs and keeps its width", () => {
  const line: BreakSource = { id: "ln", kind: "line", pts: [[0, 0], [8000, 0]], widthUm: 300 };
  const s = startBreak("slice", [line], [4000, 0])!;
  const cmds = breakCmds(s, [3000, 0]);
  assert.deepEqual(cmds[0], { op: "delete_sch_line", id: "ln" });
  assert.deepEqual(cmds[1], { op: "add_sch_line", pts: [{ x: 0, y: 0 }, { x: 3000, y: 0 }], width_um: 300 });
  assert.deepEqual(cmds[2], { op: "add_sch_line", pts: [{ x: 4000, y: 0 }, { x: 8000, y: 0 }], width_um: 300 });
});

test("nothing to cut: a selection of lines too short to cut gives no state", () => {
  assert.equal(startBreak("break", [{ id: "x", kind: "wire", pts: [[0, 0]] }], [0, 0]), null);
  assert.equal(startBreak("break", [], [0, 0]), null);
});
