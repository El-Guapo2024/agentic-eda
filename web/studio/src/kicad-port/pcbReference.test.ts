import { test } from "node:test";
import assert from "node:assert/strict";
import type { AnchorBoard } from "./pcbEditActions";
import { interactiveOffsetMove, itemPosition, moveSelectionCommands, offsetVector, pasteMoveOrigin, type PositionBoard } from "./pcbReference";

const board: AnchorBoard = {
  parts: [
    { ref: "U1", placed: true, at: [1000, 2000] },
    { ref: "R1", placed: false },
  ],
  routing: { vias: [{ id: "v1", x: 500, y: 600 }] },
  drawings: {
    shapes: [
      { id: "s1", kind: "segment", start: [0, 0] },
      { id: "s2", kind: "circle", center: [10, 10] },
    ],
    texts: [{ id: "t1", x: 7, y: 8 }],
  },
};

test("the ruler starts as the vector from the second click (origin) to the first (end)", () => {
  assert.deepEqual(offsetVector({ x: 5000, y: 3000 }, { x: 2000, y: 1000 }), { x: 3000, y: 2000 });
});

test("accepting the offset dialog unchanged moves nothing", () => {
  const first = { x: 5000, y: 3000 };
  const second = { x: 2000, y: 1000 };
  assert.deepEqual(interactiveOffsetMove(first, second, offsetVector(first, second)), { x: 0, y: 0 });
});

test("a new offset puts the picked item point at reference + offset", () => {
  const first = { x: 5000, y: 3000 };
  const second = { x: 2000, y: 1000 };
  const edited = { x: 4000, y: 0 };
  const move = interactiveOffsetMove(first, second, edited);
  assert.deepEqual({ x: first.x + move.x, y: first.y + move.y }, { x: second.x + edited.x, y: second.y + edited.y });
});

test("moveSelectionBy: each kind gets its own verb, unplaced parts and unknown ids are skipped", () => {
  const cmds = moveSelectionCommands(board, ["U1", "R1", "v1", "s1", "s2", "t1", "nope"], { x: 100, y: -50 });
  assert.deepEqual(cmds, [
    { op: "move_to", part: "U1", x: 1100, y: 1950 },
    { op: "move_via", id: "v1", x: 600, y: 550 },
    { op: "move_shape", id: "s1", dx: 100, dy: -50 },
    { op: "move_shape", id: "s2", dx: 100, dy: -50 },
    { op: "move_text", id: "t1", x: 107, y: -42 },
  ]);
});

test("moveSelectionBy: a zero vector is no command at all", () => {
  assert.deepEqual(moveSelectionCommands(board, ["U1", "v1"], { x: 0, y: 0 }), []);
});

test("a paste is carried by the clipboard's reference point, else by the cursor", () => {
  assert.deepEqual(pasteMoveOrigin({ x: 1, y: 2 }, { x: 9, y: 9 }), { x: 1, y: 2 });
  assert.deepEqual(pasteMoveOrigin(undefined, { x: 9, y: 9 }), { x: 9, y: 9 });
  assert.equal(pasteMoveOrigin(null, null), null);
});

test("the position of a picked item: footprint anchor, via, shape start, text, track start and zone corner", () => {
  const withRouting: PositionBoard = {
    ...board,
    routing: {
      vias: [{ id: "v1", x: 500, y: 600 }],
      tracks: [{ id: "t", pts: [[1, 2], [3, 4]] }],
      zones: [{ id: "z", outline: [[10, 20], [30, 40], [50, 60]] }],
    },
  };
  assert.deepEqual(itemPosition(withRouting, "U1"), { x: 1000, y: 2000 });
  assert.deepEqual(itemPosition(withRouting, "v1"), { x: 500, y: 600 });
  assert.deepEqual(itemPosition(withRouting, "s2"), { x: 10, y: 10 });
  assert.deepEqual(itemPosition(withRouting, "t1"), { x: 7, y: 8 });
  assert.deepEqual(itemPosition(withRouting, "t"), { x: 1, y: 2 });
  assert.deepEqual(itemPosition(withRouting, "z"), { x: 10, y: 20 });
  assert.equal(itemPosition(withRouting, "R1"), null, "an unplaced footprint has no position");
  assert.equal(itemPosition(withRouting, "nope"), null);
});
