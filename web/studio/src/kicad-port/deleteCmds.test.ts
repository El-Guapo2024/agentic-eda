import { test } from "node:test";
import assert from "node:assert/strict";
import { boardDeleteCmds, schematicDeleteCmds } from "./deleteCmds";
import type { BoardState, Schematic } from "../api/types";

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

test("every schematic item has a delete verb", () => {
  const sch = {
    symbols: [{ id: "U1" }],
    wires: [{ id: "w1" }],
    junctions: [{ id: "j1" }],
    lines: [{ id: "l1" }],
    labels: [{ id: "lbl1" }],
    texts: [{ id: "t1" }],
    power_symbols: [{ id: "p1" }],
    no_connects: [{ id: "n1" }],
    bus_entries: [{ id: "e1" }],
  } as unknown as Schematic;
  const cmds = schematicDeleteCmds(sch, ["U1", "w1", "j1", "l1", "lbl1", "t1", "p1", "n1", "e1", "nope"]);
  assert.deepEqual(
    cmds.map((c) => c.op),
    ["delete_symbol", "delete_wire", "delete_junction", "delete_sch_line", "delete_label", "delete_sch_text", "delete_power_symbol", "delete_no_connect", "delete_bus_entry"]
  );
  assert.deepEqual(schematicDeleteCmds(sch, ["nope"]), []);
});
