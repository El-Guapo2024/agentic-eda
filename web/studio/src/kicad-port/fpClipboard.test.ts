import { test } from "node:test";
import assert from "node:assert/strict";
import { fpClipboardKind, fpPlainTextPaste } from "./fpClipboard";

test("KiCad's clipboard texts are a footprint's, a KiCad 6 module's or a board's; anything else is pasted as a text", () => {
  assert.equal(fpClipboardKind('(footprint "clipboard:6c7e3ce6" (version 20250114) (layer "F.Cu"))'), "kicad");
  assert.equal(fpClipboardKind("\n  (module Foo (layer F.Cu))"), "kicad");
  assert.equal(fpClipboardKind("(kicad_pcb (version 20250114))"), "kicad");
  assert.equal(fpClipboardKind("(footprints are nice)"), "text");
  assert.equal(fpClipboardKind("R1"), "text");
  assert.equal(fpClipboardKind(""), "none");
});

test("a plain text goes in as it is, on the active layer, with EDA_TEXT's defaults; a long one asks first", () => {
  const p = fpPlainTextPaste("hello\nworld", { x: 1, y: 2 }, "F.Fab")!;
  assert.deepEqual(p.text, { content: "hello\nworld", at: { x: 1, y: 2 }, angle: 0, layer: "F.Fab", size_nm: 1_270_000, stroke_width: 158_750, justify: "center", mirror: false });
  assert.equal(p.longText, false);
  assert.equal(fpPlainTextPaste("x".repeat(101), { x: 0, y: 0 }, "F.SilkS")!.longText, true);
  assert.equal(fpPlainTextPaste("", { x: 0, y: 0 }, "F.SilkS"), null);
});

test("UI-LEFT-3 F3-10: Copy as Text takes texts, fields, text boxes, dimensions and tables as KiCad shows them in the Footprint Editor (no variable resolved)", async () => {
  const { fpCanCopyAsText, fpSelectionAsText } = await import("./fpClipboard");
  const fp = {
    name: "Lib:X",
    pads: [{ id: "pad_1", number: "1" }],
    graphics: [],
    texts: [{ id: "t1", content: "  ${REFERENCE}  ", at: { x: 0, y: 0 }, angle: 0, layer: "F.Fab", size_nm: 1, stroke_width: 1, justify: "center", mirror: false }],
    fields: [{ name: "MPN", value: "X123", visible: true }],
    reference: { text: "REF**", at: { x: 0, y: 0 }, size: [1_000_000, 1_000_000], layer: "F.SilkS" },
    display: {
      dimensions: [{ id: "d1", text: "2.00 mm" }],
      zones: [],
      text_boxes: [{ id: "b1", text: "box text" }],
      images: [],
      points: [],
      barcodes: [],
      tables: [{ id: "tb1", rows: 2, cols: 2, cells: [{ text: "a" }, { text: "b" }, { text: "c" }, { text: "d" }] }],
    },
  } as never;
  assert.equal(fpSelectionAsText(fp, ["t1", "field:Reference", "field:MPN", "d1", "b1", "tb1"]), "${REFERENCE}\nREF**\nX123\n2.00 mm\nbox text\na\tb\nc\td");
  assert.equal(fpCanCopyAsText(fp, ["t1", "d1"]), true);
  assert.equal(fpCanCopyAsText(fp, ["t1", "pad_1"]), false, "a pad among them: no Copy as Text in the menu");
  assert.equal(fpCanCopyAsText(fp, []), false);
  assert.equal(fpSelectionAsText(fp, ["pad_1"]), "");
});
