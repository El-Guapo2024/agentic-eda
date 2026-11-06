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

test("the design variants' list is the project's: its verbs are never addressed to a sheet (KiCad 25ec2335 `SCHEMATIC::AddVariant` ...)", () => {
  for (const cmd of [
    { op: "add_design_variant", name: "Lite", description: "" },
    { op: "remove_design_variant", name: "Lite" },
    { op: "edit_design_variant_description", name: "Lite", description: "d" },
    { op: "rename_design_variant", name: "Lite", new_name: "Max" },
    { op: "copy_design_variant", name: "Lite", new_name: "Lite_copy" },
  ] as const) {
    assert.deepEqual(onCurrentSheet(cmd, ["sheetA"]), cmd);
  }
  // a symbol's record of a variant is the sheet's symbol's: addressed like any edit
  const set: Cmd = { op: "set_symbol_variant", variant: "Lite", ids: ["R1"], dnp: true };
  assert.deepEqual(onCurrentSheet(set, ["sheetA"]), { op: "on_sheet", sheet: "sheetA", cmd: set });
});
