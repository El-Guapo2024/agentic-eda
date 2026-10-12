import { test } from "node:test";
import assert from "node:assert/strict";
import { clearNetsOffered, DEFAULT_PASTE_MODE, DEFAULT_REFERENCE, isBoardFragment, PCB_PASTE_SPECIAL_OPTIONS, pasteOptions, pasteSource } from "./pcbPasteSpecial";

const BOARD = "(kicad_pcb (version 20240108) (generator pcbnew))";
const FOOTPRINT = '(footprint "R_0603" (layer "F.Cu"))';

test("the dialog offers KiCad's three choices in its order and opens on keeping the references", () => {
  assert.deepEqual(
    PCB_PASTE_SPECIAL_OPTIONS.map((o) => o.mode),
    ["unique", "keep", "remove"]
  );
  assert.equal(DEFAULT_PASTE_MODE, "keep", "`PASTE_MODE mode = PASTE_MODE::KEEP_ANNOTATIONS;`");
  assert.equal(PCB_PASTE_SPECIAL_OPTIONS[1]!.label, "Keep existing reference designators, even if they are duplicated");
  assert.match(PCB_PASTE_SPECIAL_OPTIONS[2]!.tip, /REF\*\*/);
  assert.equal(DEFAULT_REFERENCE, "REF**");
});

test("a paste takes the system clipboard's KiCad text, else its plain text, else the session's copy", () => {
  assert.deepEqual(pasteSource(BOARD, null), { kind: "board", text: BOARD });
  assert.deepEqual(pasteSource(FOOTPRINT, BOARD), { kind: "footprint", text: FOOTPRINT }, "a copy from KiCad wins over this session's");
  assert.deepEqual(pasteSource("hello", BOARD), { kind: "text", text: "hello" }, "plain text is a text item");
  assert.deepEqual(pasteSource("   ", BOARD), { kind: "board", text: BOARD }, "a blank clipboard falls back to the session's copy");
  assert.deepEqual(pasteSource(null, FOOTPRINT), { kind: "footprint", text: FOOTPRINT }, "the browser would not hand the clipboard over");
  assert.equal(pasteSource(null, null), null);
  assert.equal(pasteSource("", "not kicad"), null);
});

test("Clear net assignments is offered for a board fragment only", () => {
  assert.equal(isBoardFragment(`  \n${BOARD}`), true);
  assert.equal(isBoardFragment(FOOTPRINT), false);
  assert.equal(clearNetsOffered({ kind: "board", text: BOARD }), true);
  assert.equal(clearNetsOffered({ kind: "footprint", text: FOOTPRINT }), false);
  assert.deepEqual(pasteOptions("remove", true, { kind: "board", text: BOARD }), { mode: "remove", clearNets: true });
  assert.deepEqual(pasteOptions("remove", true, { kind: "footprint", text: FOOTPRINT }), { mode: "remove", clearNets: false }, "the hidden box counts as unchecked");
});
