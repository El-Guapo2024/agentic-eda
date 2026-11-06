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
