import { test } from "node:test";
import assert from "node:assert/strict";
import { boardDeleteCmds } from "./deleteCmds";
import type { BoardState } from "../api/types";

const board = {
  parts: [
    { ref: "U1", placed: true },
    { ref: "R9", placed: false },
  ],
  locked: ["V2"],
  routing: { tracks: [{ id: "T1" }], vias: [{ id: "V1" }, { id: "V2" }], zones: [{ id: "Z1" }] },
  drawings: {
    shapes: [{ id: "S1" }],
    texts: [{ id: "X1" }],
    dimensions: [{ id: "D1" }],
    groups: [{ id: "grp_1", name: "", member_ids: ["T1", "S1"] }],
  },
} as unknown as BoardState;

test("each board item is deleted by its own verb, a placed footprint is ripped up", () => {
  const { cmds, skippedLocked } = boardDeleteCmds(board, ["T1", "V1", "Z1", "S1", "X1", "D1", "U1"]);
  assert.equal(skippedLocked, 0);
  assert.deepEqual(cmds, [
    { op: "delete_track", id: "T1" },
    { op: "delete_via", id: "V1" },
    { op: "delete_zone", id: "Z1" },
    { op: "delete_shape", id: "S1" },
    { op: "delete_text", id: "X1" },
    { op: "delete_dimension", id: "D1" },
    { op: "rip", part: "U1" },
  ]);
});

test("a locked item is skipped and counted; an unplaced footprint and an unknown id are not deletable", () => {
  const { cmds, skippedLocked } = boardDeleteCmds(board, ["V2", "R9", "nope", "V1"]);
  assert.equal(skippedLocked, 1);
  assert.deepEqual(cmds, [{ op: "delete_via", id: "V1" }]);
});

test("deleting a group deletes its members", () => {
  const { cmds } = boardDeleteCmds(board, ["grp_1"]);
  assert.deepEqual(cmds, [
    { op: "delete_track", id: "T1" },
    { op: "delete_shape", id: "S1" },
  ]);
  // a member named twice (directly and through its group) is deleted once
  assert.equal(boardDeleteCmds(board, ["grp_1", "T1"]).cmds.length, 2);
});
