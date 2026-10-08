import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd } from "../api/types";
import { onCurrentSheet } from "./schSheetCmd";

const move: Cmd = { op: "move_symbol", id: "U1", x: 1, y: 2 };

test("on the root a schematic command is sent as it is", () => {
  assert.deepEqual(onCurrentSheet(move, []), move);
});

test("on a nested sheet a schematic command is addressed to that sheet by its path of sheet ids", () => {
  assert.deepEqual(onCurrentSheet(move, ["sheetA", "sheetB"]), { op: "on_sheet", sheet: "sheetA/sheetB", cmd: move });
});

test("a batch of schematic commands is one command on the sheet, so it stays one undo step", () => {
  const batch: Cmd = { op: "batch", cmds: [move, { op: "add_wire", pts: [], bus: false }] };
  assert.deepEqual(onCurrentSheet(batch, ["s1"]), { op: "on_sheet", sheet: "s1", cmd: batch });
});

test("a command that is already on a sheet and Reorganize are never re-addressed", () => {
  const already: Cmd = { op: "on_sheet", sheet: "s2", cmd: move };
  assert.deepEqual(onCurrentSheet(already, ["s1"]), already);
  assert.deepEqual(onCurrentSheet({ op: "reorganize_sheets" }, ["s1"]), { op: "reorganize_sheets" });
});

test("the commands the schematic control adds (attributes, page numbers, text increments) are addressed too, whatever their name", () => {
  const attrs = { op: "set_symbol_attrs", ids: ["R1"], dnp: true } as unknown as Cmd;
  assert.deepEqual(onCurrentSheet(attrs, ["s1"]), { op: "on_sheet", sheet: "s1", cmd: attrs });
});
