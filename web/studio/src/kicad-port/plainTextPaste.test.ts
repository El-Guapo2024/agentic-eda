import { test } from "node:test";
import assert from "node:assert/strict";
import { plainTextPaste } from "./plainTextPaste";

test("a plain-text paste is the clipboard text as is, 1.27 mm, the automatic pen (PCB_CONTROL::Paste)", () => {
  const r = plainTextPaste("  hello\n", { x: 1, y: 2 }, "F.SilkS")!;
  assert.deepEqual(r.cmd, { op: "add_text", text: { content: "  hello\n", at: { x: 1, y: 2 }, angle: 0, layer: "F.SilkS", size_nm: 1_270_000, stroke_width: 158_750, justify: "center", mirror: false } });
  assert.equal(r.longText, false);
  assert.equal(plainTextPaste("x".repeat(101), { x: 0, y: 0 }, "F.Cu")!.longText, true);
  assert.equal(plainTextPaste("", { x: 0, y: 0 }, "F.Cu"), null);
});

test("the long-text question counts code points, as a wxString does (round 3 B-5)", () => {
  const emoji = "\u{1F600}".repeat(60);
  assert.equal(emoji.length, 120);
  assert.equal(plainTextPaste(emoji, { x: 0, y: 0 }, "F.SilkS")!.longText, false);
  assert.equal(plainTextPaste("  ", { x: 0, y: 0 }, "F.SilkS")!.cmd.op, "add_text", "blanks are a text");
});
