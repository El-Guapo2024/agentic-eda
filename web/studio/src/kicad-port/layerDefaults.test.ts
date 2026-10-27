import { test } from "node:test";
import assert from "node:assert/strict";
import type { TextGraphicsDefaults } from "../api/types";
import { FACTORY_TEXT_GRAPHICS, drawStrokeWidthNm, isBackLayer, layerClassOf, lineThicknessNm, newTextStyle, steppedDrawStroke, textStyleOf } from "./layerDefaults";

// The same cases as crates/model/src/rules.rs `a_new_item_takes_the_default_of_its_layer_class`.
const defaults: TextGraphicsDefaults = {
  ...FACTORY_TEXT_GRAPHICS,
  silk: { line_width_nm: 120_000, text_width_nm: 1_200_000, text_height_nm: 1_300_000, text_thickness_nm: 180, italic: true, upright: false },
  edge_cuts_line_width_nm: 70_000,
  courtyard_line_width_nm: 60_000,
};

test("a layer belongs to the class KiCad's GetLayerClass gives it", () => {
  const expected: Record<string, string> = { "F.SilkS": "silk", "B.SilkS": "silk", "F.Cu": "copper", "In3.Cu": "copper", "Edge.Cuts": "edges", "B.CrtYd": "courtyard", "F.Fab": "fab", "Dwgs.User": "others", "User.7": "others", "F.Mask": "others" };
  for (const [layer, cls] of Object.entries(expected)) assert.equal(layerClassOf(layer), cls, layer);
});

test("a new line takes the line thickness of its layer class, and the factory table is KiCad's", () => {
  assert.deepEqual([lineThicknessNm(defaults, "F.SilkS"), lineThicknessNm(defaults, "B.SilkS")], [120_000, 120_000]);
  assert.deepEqual([lineThicknessNm(defaults, "Edge.Cuts"), lineThicknessNm(defaults, "F.CrtYd"), lineThicknessNm(defaults, "F.Cu"), lineThicknessNm(defaults, "Dwgs.User")], [70_000, 60_000, 200_000, 100_000]);
  assert.deepEqual([lineThicknessNm(undefined, "F.SilkS"), lineThicknessNm(undefined, "F.Cu"), lineThicknessNm(undefined, "Edge.Cuts"), lineThicknessNm(undefined, "F.Fab")], [100_000, 200_000, 50_000, 100_000], "no table from the backend: KiCad's own");
});

test("a new text takes the size, thickness and style of its layer class, is mirrored on a back layer and left aligned", () => {
  assert.deepEqual(textStyleOf(defaults, "F.SilkS"), { widthNm: 1_200_000, heightNm: 1_300_000, thicknessNm: 180, italic: true, upright: false });
  assert.deepEqual([textStyleOf(defaults, "B.Cu").heightNm, textStyleOf(defaults, "In1.Cu").thicknessNm], [1_500_000, 300_000]);
  assert.deepEqual([textStyleOf(defaults, "Edge.Cuts").heightNm, textStyleOf(defaults, "F.CrtYd").thicknessNm], [1_000_000, 150_000], "the classes with no text cells keep KiCad's own constants");
  // The class's width and italic too, and the bottom justification (round 5 B5-2).
  assert.deepEqual(newTextStyle(defaults, "F.SilkS"), { sizeNm: 1_300_000, widthNm: 1_200_000, strokeWidthNm: 180, italic: true, mirror: false, justify: "left", valign: "bottom" });
  assert.deepEqual(newTextStyle(defaults, "B.SilkS"), { sizeNm: 1_300_000, widthNm: 1_200_000, strokeWidthNm: 180, italic: true, mirror: true, justify: "left", valign: "bottom" });
  assert.deepEqual(["B.Cu", "B.Fab", "B.CrtYd", "B.Mask", "B.Paste", "B.Adhes"].map(isBackLayer), [true, true, true, true, true, true]);
  assert.deepEqual(["F.Cu", "F.SilkS", "In1.Cu", "Edge.Cuts", "Dwgs.User"].map(isBackLayer), [false, false, false, false, false]);
});

test("the width being drawn is the layer class's until incWidth or decWidth steps it, and a layer change drops the step", () => {
  assert.equal(drawStrokeWidthNm(defaults, "F.SilkS", null), 120_000);
  assert.equal(drawStrokeWidthNm(defaults, null, null), 120_000, "no active layer: the silkscreen, as the tools always assumed");
  const up = steppedDrawStroke(defaults, "F.SilkS", null, 1);
  assert.deepEqual(up, { layer: "F.SilkS", um: 220_000 }, "one WIDTH_STEP (0.1 mm) more than the class's width");
  assert.equal(drawStrokeWidthNm(defaults, "F.SilkS", up), 220_000);
  assert.equal(drawStrokeWidthNm(defaults, "F.Cu", up), 200_000, "on another layer the step is gone");
  assert.equal(steppedDrawStroke(defaults, "F.SilkS", up, 1).um, 320_000);
  assert.equal(steppedDrawStroke(defaults, "F.SilkS", up, -1).um, 120_000);
  const thin = { layer: "F.SilkS", um: 100_000 };
  assert.equal(steppedDrawStroke(defaults, "F.SilkS", thin, -1).um, 100_000, "never down to nothing: the decrement is refused at one step");
});

test("the active layer is read as the state holds it: a state key like \"f_silks\" is the silkscreen, not a layer of its own", () => {
  assert.equal(drawStrokeWidthNm(defaults, "f_silks", null), 120_000);
  assert.equal(drawStrokeWidthNm(defaults, "f_fab", null), 100_000);
  assert.equal(drawStrokeWidthNm(defaults, "board_edge", null), 70_000);
  assert.equal(drawStrokeWidthNm(defaults, "Dwgs_User", null), 100_000);
  const up = steppedDrawStroke(defaults, "f_silks", null, 1);
  assert.deepEqual(up, { layer: "F.SilkS", um: 220_000 }, "the step is kept under the layer's name");
  assert.equal(drawStrokeWidthNm(defaults, "f_silks", up), 220_000);
  assert.equal(drawStrokeWidthNm(defaults, "F.SilkS", up), 220_000, "and read the same by name");
  assert.equal(drawStrokeWidthNm(defaults, "b_silks", up), 120_000, "the back silkscreen is another layer");
});
