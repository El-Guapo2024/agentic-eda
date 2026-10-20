import { test } from "node:test";
import assert from "node:assert/strict";
import { arcCentre, fpArrayIds, fpItemBounds, fpItemPosition, shapePosition } from "./fpArray";
import type { CmdShape, LibraryFootprint } from "../api/types";

const fp = {
  name: "T",
  pads: [{ id: "pad_a", number: "1", at: { x: 1000, y: 2000 }, size: [1000, 600], offset: { x: 0, y: 0 }, rot: 0, shape: "rect", kind: "smd", drill: null, layers: [] }],
  graphics: [
    { id: "g_line", kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: { x: -500, y: 300 }, end: { x: 500, y: 300 } },
    { id: "g_circle", kind: "circle", layer: "F.SilkS", stroke_width: 150, filled: false, center: { x: 100, y: 200 }, end: { x: 600, y: 200 } },
    { id: "g_arc", kind: "arc", layer: "F.SilkS", stroke_width: 150, filled: false, start: { x: 1000, y: 0 }, mid: { x: 0, y: 1000 }, end: { x: -1000, y: 0 } },
    { id: "g_poly", kind: "polygon", layer: "F.SilkS", stroke_width: 150, filled: true, pts: [{ x: 7, y: 8 }, { x: 70, y: 80 }, { x: 0, y: 90 }] },
  ],
  texts: [{ id: "t_1", content: "T", at: { x: 40, y: 50 }, angle: 0, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "center", mirror: false }],
} as unknown as LibraryFootprint;

test("an item's position is KiCad's GetPosition: a pad's and a text's anchor, a shape's by EDA_SHAPE::getPosition", () => {
  assert.deepEqual(fpItemPosition(fp, "pad_a"), [1000, 2000]);
  assert.deepEqual(fpItemPosition(fp, "t_1"), [40, 50]);
  assert.deepEqual(fpItemPosition(fp, "g_line"), [-500, 300], "a segment: its start");
  assert.deepEqual(fpItemPosition(fp, "g_circle"), [100, 200], "a circle: its centre");
  assert.deepEqual(fpItemPosition(fp, "g_arc"), [0, 0], "an arc: the centre of the circle through its three points");
  assert.deepEqual(fpItemPosition(fp, "g_poly"), [7, 8], "a polygon: its first vertex");
  assert.equal(fpItemPosition(fp, "nope"), null);
});

test("three points on a line have no centre", () => {
  assert.equal(arcCentre([0, 0], [1, 1], [2, 2]), null);
  assert.deepEqual(shapePosition({ kind: "arc", layer: "F.SilkS", stroke_width: 1, filled: false, start: { x: 0, y: 0 }, mid: { x: 1, y: 1 }, end: { x: 2, y: 2 } } as CmdShape), [0, 0]);
});

test("the array works on the ids that are items of the footprint, in the order given, each once", () => {
  assert.deepEqual(fpArrayIds(fp, ["t_1", "nope", "pad_a", "t_1"]), ["t_1", "pad_a"]);
  assert.deepEqual(fpArrayIds(fp, new Set(["g_line"])), ["g_line"]);
  const b = fpItemBounds(fp, "pad_a");
  assert.ok(b && b[0] === 500 && b[2] === 1500, "a pad's box");
});
