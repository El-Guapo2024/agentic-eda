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

test("deleting a group of groups deletes every item below it", () => {
  const nested = {
    ...board,
    drawings: { ...board.drawings, groups: [{ id: "outer", name: "", member_ids: ["inner", "X1"] }, { id: "inner", name: "", member_ids: ["T1", "S1"] }] },
  } as unknown as BoardState;
  const { cmds } = boardDeleteCmds(nested, ["outer"]);
  assert.deepEqual(cmds, [
    { op: "delete_track", id: "T1" },
    { op: "delete_shape", id: "S1" },
    { op: "delete_text", id: "X1" },
  ]);
});

test("a group with a locked item below it, or that is locked itself, is not deleted at all", () => {
  const lockedDeep = {
    ...board,
    locked: ["V2"],
    drawings: { ...board.drawings, groups: [{ id: "outer", name: "", member_ids: ["inner", "X1"] }, { id: "inner", name: "", member_ids: ["V1", "V2"] }] },
  } as unknown as BoardState;
  const a = boardDeleteCmds(lockedDeep, ["outer", "T1"]);
  assert.equal(a.skippedLocked, 1);
  assert.deepEqual(a.cmds, [{ op: "delete_track", id: "T1" }], "nothing of the group goes");
  const lockedGroup = { ...board, locked: ["grp_1"] } as unknown as BoardState;
  const b = boardDeleteCmds(lockedGroup, ["grp_1"]);
  assert.equal(b.skippedLocked, 1);
  assert.deepEqual(b.cmds, []);
  // a member of a locked group is locked too (`BOARD_ITEM::IsLocked` asks its group)
  assert.equal(boardDeleteCmds(lockedGroup, ["T1"]).skippedLocked, 1);
});

test("a field is hidden, not deleted; one already hidden says to use Footprint Properties (EDIT_TOOL::DeleteItems, PCB_FIELD_T)", () => {
  const field = (name: string, visible: boolean) => ({ id: `R1:${name}`, name, text: name === "Value" ? "10k" : "R1", visible, lx: 0, ly: 0, langle: 0, w: 1_000_000, h: 1_000_000, thickness: 150_000, layer: "F.Fab", halign: "center", valign: "center", mirror: false, bold: false, italic: false, upright: true, knockout: false });
  const b = { ...board, parts: [{ ref: "R1", placed: true, fields: [field("Reference", true), field("Value", false)] }] } as unknown as BoardState;
  const shown = boardDeleteCmds(b, ["R1:Reference"]);
  assert.equal(shown.cmds.length, 1);
  const c = shown.cmds[0]!;
  assert.equal(c.op, "edit_board_field");
  assert.ok(c.op === "edit_board_field" && c.part === "R1" && c.name === "Reference" && c.layout?.visible === false);
  assert.equal(shown.message, undefined);
  const hidden = boardDeleteCmds(b, ["R1:Value"]);
  assert.deepEqual(hidden.cmds, []);
  assert.equal(hidden.message, "Use the Footprint Properties dialog to remove fields.");
  // with the footprint itself, the footprint goes and the field with it
  assert.deepEqual(boardDeleteCmds(b, ["R1", "R1:Reference"]).cmds, [{ op: "rip", part: "R1" }]);
});

test("a lone zone deleted from inside a cutout loses the cutout only (EDIT_TOOL::DeleteItems, ZONE::HitTestCutout)", () => {
  const sq = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
  const zoned = {
    ...board,
    routing: { ...board.routing, zones: [{ id: "Z1", outline: sq(0, 0, 100, 100), holes: [sq(10, 10, 20, 20), sq(50, 50, 60, 60)] }] },
  } as unknown as BoardState;
  const xy = (r: [number, number][]) => r.map(([x, y]) => ({ x, y }));
  const inside = boardDeleteCmds(zoned, ["Z1"], { cursor: { x: 55, y: 55 } });
  assert.deepEqual(inside.cmds, [{ op: "set_zone_outline", id: "Z1", outline: xy(sq(0, 0, 100, 100)), holes: [xy(sq(10, 10, 20, 20))] }]);
  assert.deepEqual(inside.keepSelection, ["Z1"], "the zone stays selected");
  // outside every hole, as a Cut, or with a second item: the whole zone goes
  assert.deepEqual(boardDeleteCmds(zoned, ["Z1"], { cursor: { x: 30, y: 30 } }).cmds, [{ op: "delete_zone", id: "Z1" }]);
  assert.deepEqual(boardDeleteCmds(zoned, ["Z1"], { cursor: { x: 55, y: 55 }, isCut: true }).cmds, [{ op: "delete_zone", id: "Z1" }]);
  const two = boardDeleteCmds(zoned, ["Z1", "T1"], { cursor: { x: 55, y: 55 } }).cmds.map((c) => c.op).sort();
  assert.deepEqual(two, ["delete_track", "delete_zone"]);
});
