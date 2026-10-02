import { test } from "node:test";
import assert from "node:assert/strict";
import { wrapStep, nextLargerPreset, nextSmallerPreset, rotateQuarter, selectAllIds } from "./editTargets";
import { DEFAULT_SELECTION_FILTER } from "../components/canvas/selectionCandidates";
import type { BoardState } from "../api/types";

test("wrapStep wraps both ways like GridNext/GridPrev and TrackWidthInc/Dec", () => {
  assert.equal(wrapStep(2, 3, 1), 0);
  assert.equal(wrapStep(0, 3, -1), 2);
  assert.equal(wrapStep(0, 3, 1), 1);
  assert.equal(wrapStep(1, 3, -1), 0);
  assert.equal(wrapStep(-1, 4, 1), 0);
  assert.equal(wrapStep(-1, 4, -1), 3);
  assert.equal(wrapStep(0, 1, 1), 0);
  assert.equal(wrapStep(0, 0, 1), -1);
});

test("per-item preset rule: next larger scans up, next smaller scans down, null at the ends", () => {
  const l = [250, 150, 400, 300];
  const id = (n: number) => n;
  assert.equal(nextLargerPreset(l, id, 200), 250, "first in list order above 200 (list is not sorted: source scans in order)");
  assert.equal(nextLargerPreset(l, id, 400), null);
  assert.equal(nextSmallerPreset(l, id, 200), 150);
  assert.equal(nextSmallerPreset(l, id, 150), null);
  assert.equal(nextSmallerPreset(l, id, 300), 150, "scanning from the end: 300 is not < 300, 400 no, 150 yes");
});

test("rotateQuarter turns 90 degrees about a pivot, four turns is identity", () => {
  assert.deepEqual(rotateQuarter(10, 0, 0, 0, 1), { x: 0, y: 10 });
  assert.deepEqual(rotateQuarter(10, 0, 0, 0, 2), { x: -10, y: 0 });
  assert.deepEqual(rotateQuarter(10, 0, 0, 0, 3), { x: 0, y: -10 });
  assert.deepEqual(rotateQuarter(15, 5, 5, 5, 1), { x: 5, y: 15 });
  assert.deepEqual(rotateQuarter(7, 3, 1, 1, 4), { x: 7, y: 3 });
});

const board = {
  parts: [{ ref: "U1", placed: true, courtyard: [0, 0, 1000, 1000] }, { ref: "U2", placed: true, courtyard: [2000, 0, 3000, 1000] }],
  locked: ["U2"],
  routing: { tracks: [{ id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [500, 0]] }, { id: "t2", net: "N", layer: "B.Cu", width: 200, pts: [[0, 0], [500, 0]] }], vias: [{ id: "v1", x: 0, y: 0, d: 600, drill: 300 }], zones: [] },
  drawings: { shapes: [], texts: [], dimensions: [] },
} as unknown as BoardState;

test("selectAllIds honours the selection filter, locked items and layer visibility", () => {
  const all = selectAllIds(board, DEFAULT_SELECTION_FILTER, {}, null, false).sort();
  assert.deepEqual(all, ["U1", "t1", "t2", "v1"], "locked U2 excluded by default");
  assert.deepEqual(selectAllIds(board, { ...DEFAULT_SELECTION_FILTER, lockedItems: true }, {}, null, false).sort(), ["U1", "U2", "t1", "t2", "v1"]);
  assert.deepEqual(selectAllIds(board, { ...DEFAULT_SELECTION_FILTER, tracks: false, vias: false }, {}, null, false), ["U1"], "tracks/vias filtered out");
  assert.deepEqual(selectAllIds(board, DEFAULT_SELECTION_FILTER, { "B.Cu": false }, null, false).sort(), ["U1", "t1", "v1"], "hidden layer's track is not selected");
  assert.deepEqual(selectAllIds(board, { ...DEFAULT_SELECTION_FILTER, footprints: false, tracks: false, vias: false }, {}, null, false, ["X"]), ["X"], "adds to the existing selection");
});
