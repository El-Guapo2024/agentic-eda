import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd } from "../api/types";
import { repeatCmds, repeatSource, REPEAT_OFFSET_UM } from "./schRepeat";

const ctx = { cursor: [50_800, 25_400] as [number, number], nextReference: (id: string) => id.replace(/\d+$/, (n) => String(Number(n) + 1)) };

test("repeatSource: a placement is remembered, an edit or a deletion is not", () => {
  const label: Cmd = { op: "add_label", net: "D0", at: { x: 0, y: 0 }, kind: { scope: "local" } };
  assert.deepEqual(repeatSource(label), [label]);
  assert.equal(repeatSource({ op: "delete_label", id: "x" }), null);
  assert.equal(repeatSource({ op: "move_symbol", id: "R1", x: 1, y: 2 }), null);
  // a junction is not repeatable (`allowRepeat` is false for it)
  assert.equal(repeatSource({ op: "add_junction", at: { x: 0, y: 0 } }), null);
});

test("repeatSource: a batch of placements (a bus unfold: entry, wire, label) is remembered whole; a mixed batch is not", () => {
  const entry: Cmd = { op: "add_bus_entry", at: { x: 0, y: 0 }, size: { x: 2540, y: 2540 } };
  const wire: Cmd = { op: "add_wire", pts: [{ x: 2540, y: 2540 }, { x: 7620, y: 2540 }] };
  const label: Cmd = { op: "add_label", net: "D2", at: { x: 7620, y: 2540 }, kind: { scope: "local" } };
  assert.deepEqual(repeatSource({ op: "batch", cmds: [entry, wire, label] }), [entry, wire, label]);
  assert.equal(repeatSource({ op: "batch", cmds: [entry, { op: "delete_wire", id: "w" }] }), null);
  assert.equal(repeatSource({ op: "batch", cmds: [] }), null);
});

test("repeatCmds: a label is incremented and moved one grid step down", () => {
  const out = repeatCmds([{ op: "add_label", net: "DATA7", at: { x: 10_000, y: 20_000 }, kind: { scope: "global", shape: "input" } }], ctx);
  assert.deepEqual(out, [{ op: "add_label", net: "DATA8", at: { x: 10_000 + REPEAT_OFFSET_UM[0], y: 20_000 + REPEAT_OFFSET_UM[1] }, kind: { scope: "global", shape: "input" } }]);
  assert.equal(REPEAT_OFFSET_UM[1], 2540, "100 mil");
  // a label with no number repeats as it is
  assert.equal((repeatCmds([{ op: "add_label", net: "RESET", at: { x: 0, y: 0 }, kind: { scope: "local" } }], ctx)[0] as { net: string }).net, "RESET");
});

test("repeatCmds: every point of a wire or line, a text, a no-connect and a bus entry move by the offset", () => {
  const out = repeatCmds(
    [
      { op: "add_wire", pts: [{ x: 0, y: 0 }, { x: 5080, y: 0 }], bus: true },
      { op: "add_sch_line", pts: [{ x: 0, y: 0 }, { x: 1000, y: 1000 }], width_um: 254 },
      { op: "add_sch_text", content: "note", at: { x: 100, y: 200 }, angle_millideg: 0, size_um: 1270 },
      { op: "add_no_connect", at: { x: 5, y: 5 } },
      { op: "add_bus_entry", at: { x: 7, y: 8 }, size: { x: 2540, y: 2540 } },
    ],
    ctx,
  );
  assert.deepEqual(out[0], { op: "add_wire", pts: [{ x: 0, y: 2540 }, { x: 5080, y: 2540 }], bus: true });
  assert.deepEqual(out[1], { op: "add_sch_line", pts: [{ x: 0, y: 2540 }, { x: 1000, y: 3540 }], width_um: 254 });
  assert.deepEqual(out[2], { op: "add_sch_text", content: "note", at: { x: 100, y: 2740 }, angle_millideg: 0, size_um: 1270 });
  assert.deepEqual(out[3], { op: "add_no_connect", at: { x: 5, y: 2545 } });
  assert.deepEqual(out[4], { op: "add_bus_entry", at: { x: 7, y: 2548 }, size: { x: 2540, y: 2540 } });
});

test("repeatCmds: a symbol is cloned at the cursor and takes the next reference; without a cursor it steps down like the rest", () => {
  const sym: Cmd = { op: "add_symbol", id: "R4", lib_id: "Device:R", at: { x: 0, y: 0 }, rot_millideg: 90_000, value: "10k", footprint: "", unit: 1 };
  assert.deepEqual(repeatCmds([sym], ctx), [{ ...sym, id: "R5", at: { x: 50_800, y: 25_400 } }]);
  assert.deepEqual(repeatCmds([sym], { ...ctx, cursor: null }), [{ ...sym, id: "R5", at: { x: 0, y: 2540 } }]);
});

test("repeatCmds: the copies are themselves repeatable, so a second press keeps counting", () => {
  const first = repeatCmds([{ op: "add_label", net: "D0", at: { x: 0, y: 0 }, kind: { scope: "local" } }], ctx);
  const second = repeatCmds(first, ctx);
  assert.equal((second[0] as { net: string }).net, "D2");
  assert.deepEqual((second[0] as { at: { x: number; y: number } }).at, { x: 0, y: 5080 });
});
