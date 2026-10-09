import { test } from "node:test";
import assert from "node:assert/strict";
import { footprintPreviewBounds, symbolPreviewBounds } from "./libChooserPreview";
import type { LibraryPad, LibrarySymbolGraphic, LibrarySymbolPin } from "../api/types";

const pin = (n: string, x: number, y: number, angle: number, len: number, unit = 1, style = 1, hidden = false): LibrarySymbolPin => ({
  number: n, name: "", electrical_type: "passive", shape: "line", at: { x, y }, angle_deg: angle, length_mm: len, unit, body_style: style, hidden, name_size_mm: null, number_size_mm: null,
});

test("a symbol's box is its graphics and its pins from tip to root, Y flipped to the painters' down", () => {
  const graphics: LibrarySymbolGraphic[] = [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -1.016, y: 2.54 }, end: { x: 1.016, y: -2.54 }, stroke_mm: 0.254, fill: "none" }];
  const pins = [pin("1", 0, 3.81, 270, 1.27), pin("2", 0, -3.81, 90, 1.27)];
  const b = symbolPreviewBounds({ graphics, pins }, 1, 1)!;
  // the pins reach (0, 3.81) down to the body at 2.54 (angle 270: toward -y in the file, i.e. toward the body)... and the tips are the extremes
  assert.deepEqual([b.minX, b.maxX], [-1016, 1016]);
  assert.deepEqual([b.minY, b.maxY], [-3810, 3810]);
  assert.equal(symbolPreviewBounds({ graphics: [], pins: [] }, 1, 1), null);
});

test("only the unit and body style shown are framed; a shared item (0) is in every one", () => {
  const rect = (unit: number, style: number, x: number): LibrarySymbolGraphic => ({ kind: "rectangle", unit, body_style: style, start: { x: x, y: 1 }, end: { x: x + 1, y: -1 }, stroke_mm: 0.1, fill: "none" });
  const graphics = [rect(1, 1, 0), rect(2, 1, 10), rect(0, 1, -5), rect(1, 2, 20)];
  assert.deepEqual(symbolPreviewBounds({ graphics, pins: [] }, 1, 1), { minX: -5000, minY: -1000, maxX: 1000, maxY: 1000 });
  assert.deepEqual(symbolPreviewBounds({ graphics, pins: [] }, 2, 1), { minX: -5000, minY: -1000, maxX: 11000, maxY: 1000 });
  assert.deepEqual(symbolPreviewBounds({ graphics, pins: [] }, 1, 2), { minX: 20000, minY: -1000, maxX: 21000, maxY: 1000 }, "the alternate style's own graphics");
  // a hidden pin draws nothing
  const hidden = symbolPreviewBounds({ graphics: [], pins: [pin("1", 100, 100, 0, 1, 1, 1, true)] }, 1, 1);
  assert.equal(hidden, null);
});

test("a footprint's box holds its pads at their full size, its graphics and its courtyard", () => {
  const pad = (x: number, y: number, w: number, h: number): LibraryPad => ({ id: "", number: "1", at: { x, y }, size: [w, h], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", drill: null, rot: 0, layers: [], chamfer_corners: { top_left: false, top_right: false, bottom_left: false, bottom_right: false } } as unknown as LibraryPad);
  const pads = [pad(-1000, 0, 800, 600), pad(1000, 0, 800, 600)];
  assert.deepEqual(footprintPreviewBounds({ pads, graphics: [] }), { minX: -1400, minY: -400, maxX: 1400, maxY: 400 });
  const graphics = [{ kind: "segment", start: { x: -2000, y: -1500 }, end: { x: 2000, y: -1500 } }, { kind: "circle", center: { x: 0, y: 0 }, end: { x: 3000, y: 0 } }];
  assert.deepEqual(footprintPreviewBounds({ pads, graphics }), { minX: -3000, minY: -3000, maxX: 3000, maxY: 3000 });
  assert.deepEqual(footprintPreviewBounds({ pads, graphics: [], courtyard: [2500, 1500] }), { minX: -2500, minY: -1500, maxX: 2500, maxY: 1500 });
  assert.equal(footprintPreviewBounds({ pads: [], graphics: [] }), null);
});
