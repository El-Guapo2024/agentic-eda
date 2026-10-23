import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, FpGraphic, FpTextInfo, Part } from "../api/types";
import { artBounds, artHitDistance, artLayerShown, artTextAsField, artTextShown, dashPattern, graphicsOf, layerRank, layersOf, unionBox } from "./fpArt";
import { itemBounds } from "./pcbItems";

const line = (layer: string, a: [number, number], b: [number, number], width = 120_000, over: Partial<FpGraphic> = {}): FpGraphic => ({ kind: "segment", layer, stroke_width: width, start: a, end: b, ...over }) as FpGraphic;

const part = (graphics: FpGraphic[], over: Partial<Part> = {}): Part =>
  ({ ref: "U1", value: "MCU", package: null, mpn: null, footprint: null, block: null, placed: true, size: null, at: [10_000_000, 10_000_000], rot: 0, side: "top", courtyard: [9_000_000, 9_000_000, 11_000_000, 11_000_000], graphics, ...over }) as Part;

const text = (over: Partial<FpTextInfo> = {}): FpTextInfo => ({ id: "U1~t0", text: "U1", x: 10_000_000, y: 10_000_000, angle: 0, w: 500_000, h: 500_000, thickness: 75_000, layer: "F.Fab", halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: true, knockout: false, ...over });

test("a graphic from the wire is a complete shape, kept per array", () => {
  const raw = [line("F.SilkS", [0, 0], [1_000_000, 0])];
  const p = part(raw);
  const a = graphicsOf(p);
  assert.equal(a.length, 1);
  assert.deepEqual([a[0]!.id, a[0]!.filled, a[0]!.kind], ["", false, "segment"]);
  assert.equal(graphicsOf(p), a, "the same array is not rebuilt on the next frame");
  assert.deepEqual(graphicsOf({ graphics: undefined }), []);
});

test("a shape on several layers is on each of them", () => {
  const g = graphicsOf(part([line("F.Cu", [0, 0], [1000, 1000], 100_000, { layers: ["F.Cu", "F.Mask"] })]))[0]!;
  assert.deepEqual(layersOf(g), ["F.Cu", "F.Mask"]);
  assert.deepEqual(layersOf({ layer: "F.SilkS" }), ["F.SilkS"]);
});

test("a click on a stroke of the silkscreen is on the footprint, wherever the stroke is; a hidden layer or a hidden side is not", () => {
  // a silkscreen line 1 mm outside the courtyard, 0.12 wide
  const p = part([line("F.SilkS", [12_000_000, 9_000_000], [12_000_000, 11_000_000])]);
  const on = {};
  assert.equal(artHitDistance(p, 12_000_000, 10_000_000, on), 0, "on the stroke");
  assert.equal(artHitDistance(p, 12_200_000, 10_000_000, on), 140_000, "200 um from its axis, the half width (60) off");
  assert.equal(artHitDistance(p, 12_000_000, 13_000_000, on), 1_940_000, "two millimetres past its end, the half width off");
  assert.equal(artHitDistance(p, 12_000_000, 10_000_000, { "F.SilkS": false }), Infinity, "the layer is off");
  assert.equal(artHitDistance(p, 12_000_000, 10_000_000, { "obj:footprints_front": false }), Infinity, "the side is off");
  assert.equal(artHitDistance(part([]), 0, 0, on), Infinity);
});

test("a filled shape is hit anywhere inside, an outline only on its edge", () => {
  const filled: FpGraphic = { kind: "circle", layer: "F.Fab", stroke_width: 100_000, filled: true, center: [10_000_000, 10_000_000], end: [10_500_000, 10_000_000] };
  const hollow: FpGraphic = { ...filled, filled: false };
  assert.equal(artHitDistance(part([filled]), 10_100_000, 10_000_000, {}), 0);
  assert.equal(artHitDistance(part([hollow]), 10_100_000, 10_000_000, {}), 350_000, "400 from the edge, the half width off");
});

test("the box of the graphics holds the strokes; the footprint's box holds those and the courtyard", () => {
  const p = part([line("F.SilkS", [12_000_000, 9_000_000], [12_000_000, 11_000_000]), line("F.Fab", [8_000_000, 10_000_000], [9_500_000, 10_000_000], 100_000)]);
  assert.deepEqual(artBounds(p), [7_950_000, 8_940_000, 12_060_000, 11_060_000]);
  assert.deepEqual(unionBox([9_000_000, 9_000_000, 11_000_000, 11_000_000], artBounds(p)), [7_950_000, 8_940_000, 12_060_000, 11_060_000]);
  assert.equal(artBounds(part([])), null);
  assert.deepEqual(unionBox(null, [1000, 2000, 3000, 4000]), [1000, 2000, 3000, 4000]);
  assert.equal(unionBox(null, null), null);
  const board = { parts: [p], routing: undefined, drawings: undefined } as unknown as BoardState;
  assert.deepEqual(itemBounds(board, "U1"), [7_950_000, 8_940_000, 12_060_000, 11_060_000], "a footprint's box in the selection tools is its graphics and its courtyard");
  assert.deepEqual(itemBounds({ parts: [part([])] } as unknown as BoardState, "U1"), [9_000_000, 9_000_000, 11_000_000, 11_000_000], "and the courtyard alone when it has none");
});

test("the Appearance rows: the layer, the side, Footprint Text, References and Values", () => {
  const front = { side: "top" } as Pick<Part, "side">;
  const back = { side: "bottom" } as Pick<Part, "side">;
  assert.equal(artLayerShown({}, front, "F.SilkS"), true);
  assert.equal(artLayerShown({ "F.SilkS": false }, front, "F.SilkS"), false);
  assert.equal(artLayerShown({ "obj:footprints_back": false }, back, "B.SilkS"), false);
  assert.equal(artLayerShown({ "obj:footprints_back": false }, front, "F.SilkS"), true);
  assert.equal(artTextShown({}, front, text()), true);
  assert.equal(artTextShown({ "obj:footprint_text": false }, front, text()), false);
  assert.equal(artTextShown({ "obj:footprint_references": false }, front, text({ role: "reference" })), false);
  assert.equal(artTextShown({ "obj:footprint_references": false }, front, text({ role: "value" })), true);
  assert.equal(artTextShown({ "obj:footprint_values": false }, front, text({ role: "value" })), false);
  assert.equal(artTextShown({ "F.Fab": false }, front, text()), false);
});

test("a text is a field to the code that draws fields", () => {
  const f = artTextAsField(text({ h: 700_000, upright: false }));
  assert.deepEqual([f.name, f.visible, f.h, f.upright, f.id], ["", true, 700_000, false, "U1~t0"]);
});

test("dashes are KiCad's: 11 widths long, a dot a fifth of one, a gap four", () => {
  assert.deepEqual(dashPattern("dash", 100_000), [1_100_000, 400_000]);
  assert.deepEqual(dashPattern("dot", 100_000), [20_000, 400_000]);
  assert.deepEqual(dashPattern("dash_dot", 100_000), [1_100_000, 400_000, 20_000, 400_000]);
  assert.deepEqual(dashPattern("dash_dot_dot", 100_000), [1_100_000, 400_000, 20_000, 400_000, 20_000, 400_000]);
  assert.deepEqual(dashPattern(undefined, 100_000), []);
});

test("layers are painted from the user layers up to the silkscreen", () => {
  const order = ["F.Cu", "Dwgs.User", "F.Fab", "F.Mask", "F.SilkS"];
  const ranks = order.map(layerRank);
  assert.deepEqual([...ranks].sort((a, b) => a - b), ranks);
  assert.ok(layerRank("B.Fab") < layerRank("F.Fab") && layerRank("B.SilkS") < layerRank("F.SilkS"));
});
