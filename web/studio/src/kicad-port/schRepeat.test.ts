import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd } from "../api/types";
import { repeatCmds, repeatSource, REPEAT_OFFSET_NM } from "./schRepeat";

const ctx = { cursor: [50_800, 25_400] as [number, number], nextReference: (id: string) => id.replace(/\d+$/, (n) => String(Number(n) + 1)) };

test("repeatSource: a placement is remembered, an edit or a deletion is not", () => {
  const label: Cmd = { op: "add_label", net: "D0", at: { x: 0, y: 0 }, kind: { scope: "local" } };
  assert.deepEqual(repeatSource(label), [label]);
  assert.equal(repeatSource({ op: "delete_label", id: "x" }), null);
  assert.equal(repeatSource({ op: "move_symbol", id: "R1", x: 1, y: 2000 }), null);
  // a junction is not repeatable (`allowRepeat` is false for it)
  assert.equal(repeatSource({ op: "add_junction", at: { x: 0, y: 0 } }), null);
});

test("repeatSource: a batch of placements (a bus unfold: entry, wire, label) is remembered whole; a mixed batch is not", () => {
  const entry: Cmd = { op: "add_bus_entry", at: { x: 0, y: 0 }, size: { x: 2_540_000, y: 2_540_000 } };
  const wire: Cmd = { op: "add_wire", pts: [{ x: 2_540_000, y: 2_540_000 }, { x: 7_620_000, y: 2_540_000 }] };
  const label: Cmd = { op: "add_label", net: "D2", at: { x: 7_620_000, y: 2_540_000 }, kind: { scope: "local" } };
  assert.deepEqual(repeatSource({ op: "batch", cmds: [entry, wire, label] }), [entry, wire, label]);
  assert.equal(repeatSource({ op: "batch", cmds: [entry, { op: "delete_wire", id: "w" }] }), null);
  assert.equal(repeatSource({ op: "batch", cmds: [] }), null);
});

test("repeatCmds: a label is incremented and moved one grid step down", () => {
  const out = repeatCmds([{ op: "add_label", net: "DATA7", at: { x: 10_000_000, y: 20_000_000 }, kind: { scope: "global", shape: "input" } }], ctx);
  assert.deepEqual(out, [{ op: "add_label", net: "DATA8", at: { x: 10_000_000 + REPEAT_OFFSET_NM[0], y: 20_000_000 + REPEAT_OFFSET_NM[1] }, kind: { scope: "global", shape: "input" } }]);
  assert.equal(REPEAT_OFFSET_NM[1], 2_540_000, "100 mil");
  // a label with no number repeats as it is
  assert.equal((repeatCmds([{ op: "add_label", net: "RESET", at: { x: 0, y: 0 }, kind: { scope: "local" } }], ctx)[0] as { net: string }).net, "RESET");
});

test("repeatCmds: every point of a wire or line, a text, a no-connect and a bus entry move by the offset", () => {
  const out = repeatCmds(
    [
      { op: "add_wire", pts: [{ x: 0, y: 0 }, { x: 5_080_000, y: 0 }], bus: true },
      { op: "add_sch_line", pts: [{ x: 0, y: 0 }, { x: 1_000_000, y: 1_000_000 }], width_nm: 254_000 },
      { op: "add_sch_text", content: "note", at: { x: 100_000, y: 200_000 }, angle_millideg: 0, size_nm: 1_270_000 },
      { op: "add_no_connect", at: { x: 5000, y: 5000 } },
      { op: "add_bus_entry", at: { x: 7000, y: 8000 }, size: { x: 2_540_000, y: 2_540_000 } },
    ],
    ctx,
  );
  assert.deepEqual(out[0], { op: "add_wire", pts: [{ x: 0, y: 2_540_000 }, { x: 5_080_000, y: 2_540_000 }], bus: true });
  assert.deepEqual(out[1], { op: "add_sch_line", pts: [{ x: 0, y: 2_540_000 }, { x: 1_000_000, y: 3_540_000 }], width_nm: 254_000 });
  assert.deepEqual(out[2], { op: "add_sch_text", content: "note", at: { x: 100_000, y: 2_740_000 }, angle_millideg: 0, size_nm: 1_270_000 });
  assert.deepEqual(out[3], { op: "add_no_connect", at: { x: 5000, y: 2_545_000 } });
  assert.deepEqual(out[4], { op: "add_bus_entry", at: { x: 7000, y: 2_548_000 }, size: { x: 2_540_000, y: 2_540_000 } });
});

test("repeatCmds: a symbol is cloned at the cursor and takes the next reference; without a cursor it steps down like the rest", () => {
  const sym: Cmd = { op: "add_symbol", id: "R4", lib_id: "Device:R", at: { x: 0, y: 0 }, rot_millideg: 90_000, value: "10k", footprint: "", unit: 1 };
  assert.deepEqual(repeatCmds([sym], ctx), [{ ...sym, id: "R5", at: { x: 50_800_000, y: 25_400_000 } }]);
  assert.deepEqual(repeatCmds([sym], { ...ctx, cursor: null }), [{ ...sym, id: "R5", at: { x: 0, y: 2_540_000 } }]);
});

test("repeatCmds: the copies are themselves repeatable, so a second press keeps counting", () => {
  const first = repeatCmds([{ op: "add_label", net: "D0", at: { x: 0, y: 0 }, kind: { scope: "local" } }], ctx);
  const second = repeatCmds(first, ctx);
  assert.equal((second[0] as { net: string }).net, "D2");
  assert.deepEqual((second[0] as { at: { x: number; y: number } }).at, { x: 0, y: 5_080_000 });
});

test("a label or text placed with its dialog's properties repeats with them: the label's text steps on, both move one grid step down", () => {
  const out = repeatCmds(
    [
      { op: "sch_edit", verb: "place_label", net: "CLK0", at: { x: 1_000_000, y: 2_000_000 }, kind: { scope: "global", shape: "output" }, spin: "left", auto_rotate: true, bold: true },
      { op: "sch_edit", verb: "place_text", content: "note", at: { x: 100_000, y: 200_000 }, angle: 90_000, size_nm: 1_270_000, h: "right", v: "top" },
    ],
    ctx,
  );
  assert.deepEqual(out[0], { op: "sch_edit", verb: "place_label", net: "CLK1", at: { x: 1_000_000, y: 2_000_000 + REPEAT_OFFSET_NM[1] }, kind: { scope: "global", shape: "output" }, spin: "left", auto_rotate: true, bold: true });
  assert.deepEqual(out[1], { op: "sch_edit", verb: "place_text", content: "note", at: { x: 100_000, y: 200_000 + REPEAT_OFFSET_NM[1] }, angle: 90_000, size_nm: 1_270_000, h: "right", v: "top" });
  // they are what the repeat source keeps; an edit of a label is not
  assert.notEqual(repeatSource({ op: "sch_edit", verb: "place_label", net: "A", at: { x: 0, y: 0 }, kind: { scope: "local" } }), null);
  assert.notEqual(repeatSource({ op: "sch_edit", verb: "place_text", content: "t", at: { x: 0, y: 0 }, size_nm: 1_270_000 }), null);
  assert.equal(repeatSource({ op: "sch_edit", verb: "edit_label", id: "lbl_a", text: "X" }), null);
});
