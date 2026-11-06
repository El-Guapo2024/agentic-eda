import { test } from "node:test";
import assert from "node:assert/strict";
import { appendBoardCmd, describeAppended, fileStemOf, isKicadBoardName } from "./appendBoard";

test("only KiCad board files are appended", () => {
  assert.ok(isKicadBoardName("other.kicad_pcb"));
  assert.ok(isKicadBoardName("OTHER.KICAD_PCB"));
  assert.ok(!isKicadBoardName("old.brd"));
  assert.ok(!isKicadBoardName("board.kicad_pro"));
  assert.ok(!isKicadBoardName("kicad_pcb"));
});

test("the append puts the file's origin at the cursor less the point the items are held by, so that point lands on the cursor, grouped under the file's stem", () => {
  const cmd = appendBoardCmd("(kicad_pcb)", { x: 10_000, y: -4_000 }, { x: 60_000, y: 70_000 }, "boards/other.v2.kicad_pcb", true);
  assert.deepEqual(cmd, { op: "place_design_block", text: "(kicad_pcb)", at: { x: 50_000, y: 74_000 }, block: "", as_group: true, group_name: "other.v2" });
  // Place as group off: the items come in on their own
  assert.equal((appendBoardCmd("(kicad_pcb)", { x: 0, y: 0 }, { x: 0, y: 0 }, "a.kicad_pcb", false) as { as_group: boolean }).as_group, false);
});

test("the group's name is the file's name without its folder and extension (wxFileName::GetName)", () => {
  assert.equal(fileStemOf("other.kicad_pcb"), "other");
  assert.equal(fileStemOf("C:\\work\\a.b.kicad_pcb"), "a.b");
  assert.equal(fileStemOf("/x/y/noext"), "noext");
  assert.equal(fileStemOf(".hidden"), ".hidden");
});

test("what the file held is said with the right plural", () => {
  assert.equal(describeAppended({ footprints: 1, tracks: 12, vias: 0, zones: 0, shapes: 2, texts: 1, dimensions: 0, groups: 1 }), "1 footprint, 12 tracks, 2 graphics, 1 text");
  assert.equal(describeAppended({}), "nothing");
  assert.equal(describeAppended(undefined), "nothing");
});
