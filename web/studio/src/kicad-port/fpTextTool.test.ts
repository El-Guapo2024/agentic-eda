import { test } from "node:test";
import assert from "node:assert/strict";
import { fpNewText, noPrintableChars } from "./fpTextTool";

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
