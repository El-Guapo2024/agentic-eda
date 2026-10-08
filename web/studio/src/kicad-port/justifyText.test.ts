import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, CmdText } from "../api/types";
import { boardJustifyCmds, footprintJustifyCmds } from "./justifyText";

function board(texts: { id: string; justify: "left" | "center" | "right" }[], locked: string[] = []): BoardState {
  return {
    locked,
    drawings: {
      texts: texts.map((t) => ({ id: t.id, content: `c${t.id}`, x: 1, y: 2, angle: 90000, layer: "F.SilkS", size: 1000, stroke_width: 150, justify: t.justify, mirror: false })),
      shapes: [],
      dimensions: [],
      groups: [],
    },
  } as unknown as BoardState;
}

test("board justify edits only the selected texts and keeps everything else about them", () => {
  const b = board([
    { id: "a", justify: "center" },
    { id: "b", justify: "center" },
    { id: "c", justify: "center" },
  ]);
  const { cmds, skippedLocked } = boardJustifyCmds(b, ["a", "c", "not-a-text"], "right");
  assert.equal(skippedLocked, 0);
  assert.deepEqual(cmds, [
    { op: "edit_text", id: "a", content: "ca", angle: 90000, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "right", mirror: false },
    { op: "edit_text", id: "c", content: "cc", angle: 90000, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "right", mirror: false },
  ]);
});

test("a text that already has the justification needs no edit, a locked one is skipped and counted", () => {
  const b = board(
    [
      { id: "a", justify: "left" },
      { id: "b", justify: "center" },
    ],
    ["b"]
  );
  const { cmds, skippedLocked } = boardJustifyCmds(b, ["a", "b"], "left");
  assert.deepEqual(cmds, []);
  assert.equal(skippedLocked, 1);
});

test("footprint justify edits the footprint's own texts by id", () => {
  const texts: CmdText[] = [
    { id: "t1", content: "REF**", at: { x: 0, y: 0 }, angle: 0, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "center", mirror: false },
    { id: "t2", content: "VAL", at: { x: 0, y: 0 }, angle: 0, layer: "F.Fab", size_um: 1000, stroke_width: 150, justify: "left", mirror: false },
    { content: "no id", at: { x: 0, y: 0 }, angle: 0, layer: "F.Fab", size_um: 1000, stroke_width: 150, justify: "center", mirror: false },
  ];
  const cmds = footprintJustifyCmds("R_0603", texts, ["t1", "t2"], "left");
  assert.deepEqual(cmds, [{ op: "edit_footprint_text", footprint: "R_0603", id: "t1", content: "REF**", angle: 0, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "left", mirror: false }]);
});
