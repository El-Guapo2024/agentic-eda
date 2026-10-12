import { test } from "node:test";
import assert from "node:assert/strict";
import { appendBoardCmd, describeAppended, isKicadBoardName } from "./appendBoard";

test("only KiCad board files are appended", () => {
  assert.ok(isKicadBoardName("other.kicad_pcb"));
  assert.ok(isKicadBoardName("OTHER.KICAD_PCB"));
  assert.ok(!isKicadBoardName("old.brd"));
  assert.ok(!isKicadBoardName("board.kicad_pro"));
  assert.ok(!isKicadBoardName("kicad_pcb"));
});

test("the paste puts the file's origin at the cursor less the point the items are held by, so that point lands on the cursor", () => {
  const cmd = appendBoardCmd("(kicad_pcb)", { x: 10_000, y: -4_000 }, { x: 60_000, y: 70_000 });
  assert.deepEqual(cmd, { op: "paste_clipboard", text: "(kicad_pcb)", at: { x: 50_000, y: 74_000 } });
});

test("what the file held is said with the right plural", () => {
  assert.equal(describeAppended({ footprints: 1, tracks: 12, vias: 0, zones: 0, shapes: 2, texts: 1, dimensions: 0, groups: 1 }), "1 footprint, 12 tracks, 2 graphics, 1 text");
  assert.equal(describeAppended({}), "nothing");
  assert.equal(describeAppended(undefined), "nothing");
});
