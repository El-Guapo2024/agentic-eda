import { test } from "node:test";
import assert from "node:assert/strict";
import type { Cmd } from "../api/types";
import { repeatCmds, repeatReferences, repeatSource, REPEAT_OFFSET_NM } from "./schRepeat";

const ctx = { cursor: [50_800_000, 25_400_000] as [number, number], nextReference: (id: string) => id.replace(/\d+$/, (n) => String(Number(n) + 1)) };

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
  // a wire repeats through `add_wire_as` (r3 C3-L1: `RepeatDrawItem` does not cut it between a two-pin part's pins), and so does its repeat
  assert.deepEqual(out[0], { op: "sch_edit", verb: "add_wire_as", pts: [{ x: 0, y: 2_540_000 }, { x: 5_080_000, y: 2_540_000 }], bus: true, by: "repeat" });
  assert.deepEqual(repeatCmds([out[0]!], ctx)[0], { op: "sch_edit", verb: "add_wire_as", pts: [{ x: 0, y: 5_080_000 }, { x: 5_080_000, y: 5_080_000 }], bus: true, by: "repeat" });
  assert.deepEqual(repeatSource(out[0]!), [out[0]]);
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

test("repeatCmds: Preferences' Repeated Items -- the horizontal and vertical pitch move the copy, the label increment counts the label on", () => {
  const out = repeatCmds([{ op: "add_label", net: "D2", at: { x: 1_000_000, y: 2_000_000 }, kind: { scope: "local" } }], { ...ctx, offsetNm: [50 * 25_400, -100 * 25_400], labelIncrement: 3 });
  assert.deepEqual(out, [{ op: "add_label", net: "D5", at: { x: 1_000_000 + 1_270_000, y: 2_000_000 - 2_540_000 }, kind: { scope: "local" } }]);
  const down = repeatCmds([{ op: "add_label", net: "D9", at: { x: 0, y: 0 }, kind: { scope: "local" } }], { ...ctx, labelIncrement: -2 });
  assert.equal((down[0] as { net: string }).net, "D7");
});

test("repeatReferences: a repeated symbol takes the server's reference (the hierarchy's numbering), the page's guess when there is none or two would share one", async () => {
  const source: Cmd[] = [
    { op: "add_symbol", id: "R3", lib_id: "Device:R", at: { x: 0, y: 0 }, rot_millideg: 0, value: "10k", footprint: "", unit: 1 },
    { op: "add_symbol", id: "R4", lib_id: "Device:R", at: { x: 0, y: 0 }, rot_millideg: 0, value: "10k", footprint: "", unit: 1 },
    { op: "add_symbol", id: "U1", lib_id: "Amp:Dual", at: { x: 0, y: 0 }, rot_millideg: 0, value: "TL072", footprint: "", unit: 2 },
  ];
  const asked: string[] = [];
  const refs = await repeatReferences(
    source,
    async (q) => {
      asked.push(`${q.prefix}:${q.lib_id}:${q.value}:${q.unit}`);
      return q.prefix === "U" ? null : "R12";
    },
    (id, taken) => `${id}-guess-${taken.length}`,
    (id) => id.replace(/\d+$/, "")
  );
  assert.deepEqual(asked, ["R:Device:R:10k:1", "R:Device:R:10k:1", "U:Amp:Dual:TL072:2"]);
  assert.deepEqual([...refs], [["R3", "R12"], ["R4", "R4-guess-1"], ["U1", "U1-guess-2"]]);
  // and repeatCmds names each copy by it
  const out = repeatCmds(source.slice(0, 1), { ...ctx, nextReference: (id) => refs.get(id) ?? id });
  assert.equal((out[0] as { id: string }).id, "R12");
});
