import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd } from "../api/types";
import { isSchematicCmd, onCurrentSheet } from "./schSheetCmd";

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

test("a PCB command, a command already on a sheet and Reorganize are never re-addressed", () => {
  const pcb: Cmd = { op: "move_to", part: "R1", x: 0, y: 0 };
  assert.deepEqual(onCurrentSheet(pcb, ["s1"]), pcb);
  const already: Cmd = { op: "on_sheet", sheet: "s2", cmd: move };
  assert.deepEqual(onCurrentSheet(already, ["s1"]), already);
  assert.deepEqual(onCurrentSheet({ op: "reorganize_sheets" }, ["s1"]), { op: "reorganize_sheets" });
});

test("the schematic ops are the ones Cmd::domain files under the schematic editor", () => {
  assert.equal(isSchematicCmd({ op: "add_label", net: "A", at: { x: 0, y: 0 }, kind: { scope: "local" } } as Cmd), true);
  assert.equal(isSchematicCmd({ op: "sch_edit", verb: "set_locked", ids: [], locked: true } as unknown as Cmd), true);
  assert.equal(isSchematicCmd({ op: "route" } as unknown as Cmd), false);
  assert.equal(isSchematicCmd({ op: "batch", cmds: [] }), false);
});
