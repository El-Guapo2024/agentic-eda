import { test } from "node:test";
import assert from "node:assert/strict";
import { fileStem, schematicSaveNames } from "./saveAs";

test("a board name becomes a safe file stem", () => {
  assert.equal(fileStem("passive_divider_ladder"), "passive_divider_ladder");
  assert.equal(fileStem("  my board  "), "my board");
  assert.equal(fileStem("rev/A:2"), "rev_A_2");
  assert.equal(fileStem("..hidden"), "hidden");
  assert.equal(fileStem(""), "board");
  assert.equal(fileStem("///"), "___");
  assert.equal(fileStem("..."), "board");
});

test("the root sheet takes the board's name and sub-sheets keep theirs", () => {
  assert.deepEqual(schematicSaveNames(["board.kicad_sch"], "blinky"), ["blinky.kicad_sch"]);
  assert.deepEqual(schematicSaveNames(["board.kicad_sch", "power.kicad_sch", "mcu.kicad_sch"], "my/board"), ["my_board.kicad_sch", "power.kicad_sch", "mcu.kicad_sch"]);
  assert.deepEqual(schematicSaveNames([], "x"), []);
});
