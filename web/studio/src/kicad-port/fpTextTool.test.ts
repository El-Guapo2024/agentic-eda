import { test } from "node:test";
import assert from "node:assert/strict";
import { fpNewText, fpTextSizeMessage, noPrintableChars } from "./fpTextTool";

test("a placed text takes the layer's defaults: size, pen, left/bottom, mirrored on a back layer", () => {
  assert.deepEqual(fpNewText("B.SilkS", { x: 1, y: 2 }), { content: "", at: { x: 1, y: 2 }, angle: 0, layer: "B.SilkS", size_nm: 1_000_000, stroke_width: 100_000, justify: "left", mirror: true });
  const cu = fpNewText("F.Cu", { x: 0, y: 0 });
  assert.deepEqual([cu.size_nm, cu.stroke_width, cu.mirror], [1_500_000, 300_000, false]);
  const fab = fpNewText("F.Fab", { x: 0, y: 0 });
  assert.deepEqual([fab.size_nm, fab.stroke_width], [1_000_000, 150_000]);
  const crt = fpNewText("B.CrtYd", { x: 0, y: 0 });
  assert.deepEqual([crt.size_nm, crt.stroke_width, crt.mirror], [1_000_000, 150_000, true]);
});

test("an empty or blank text is dropped (NoPrintableChars)", () => {
  assert.equal(noPrintableChars(""), true);
  assert.equal(noPrintableChars("  \n\t"), true);
  assert.equal(noPrintableChars(" a "), false);
});

// E5-1: UNIT_BINDER::Validate's own words, the bound printed by StringFromValue (mm, trailing zeros off) and the label without its colon.
test("a size outside the text limits gets UNIT_BINDER::Validate's message for the Width field", () => {
  assert.equal(fpTextSizeMessage(0), "Width must be at least 0.001 mm.");
  assert.equal(fpTextSizeMessage(300_000_000), "Width must be less than 250 mm.");
  assert.equal(fpTextSizeMessage(999), "Width must be at least 0.001 mm.");
  for (const nm of [1_000, 1_000_000, 250_000_000]) assert.equal(fpTextSizeMessage(nm), null, `${nm} is in range`);
});
