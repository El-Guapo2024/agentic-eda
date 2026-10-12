import { test } from "node:test";
import assert from "node:assert/strict";
import type { TextGraphicsDefaults } from "../api/types";
import { FACTORY_TEXT_GRAPHICS, drawStrokeWidthUm, isBackLayer, layerClassOf, lineThicknessUm, newTextStyle, steppedDrawStroke, textStyleOf } from "./layerDefaults";

// The same cases as crates/model/src/rules.rs `a_new_item_takes_the_default_of_its_layer_class`.
const defaults: TextGraphicsDefaults = {
  ...FACTORY_TEXT_GRAPHICS,
  silk: { line_width_um: 120, text_width_um: 1200, text_height_um: 1300, text_thickness_um: 180, italic: true, upright: false },
  edge_cuts_line_width_um: 70,
  courtyard_line_width_um: 60,
};

test("a layer belongs to the class KiCad's GetLayerClass gives it", () => {
  const expected: Record<string, string> = { "F.SilkS": "silk", "B.SilkS": "silk", "F.Cu": "copper", "In3.Cu": "copper", "Edge.Cuts": "edges", "B.CrtYd": "courtyard", "F.Fab": "fab", "Dwgs.User": "others", "User.7": "others", "F.Mask": "others" };
  for (const [layer, cls] of Object.entries(expected)) assert.equal(layerClassOf(layer), cls, layer);
});

test("a new line takes the line thickness of its layer class, and the factory table is KiCad's", () => {
  assert.deepEqual([lineThicknessUm(defaults, "F.SilkS"), lineThicknessUm(defaults, "B.SilkS")], [120, 120]);
  assert.deepEqual([lineThicknessUm(defaults, "Edge.Cuts"), lineThicknessUm(defaults, "F.CrtYd"), lineThicknessUm(defaults, "F.Cu"), lineThicknessUm(defaults, "Dwgs.User")], [70, 60, 200, 100]);
  assert.deepEqual([lineThicknessUm(undefined, "F.SilkS"), lineThicknessUm(undefined, "F.Cu"), lineThicknessUm(undefined, "Edge.Cuts"), lineThicknessUm(undefined, "F.Fab")], [100, 200, 50, 100], "no table from the backend: KiCad's own");
});

test("a new text takes the size, thickness and style of its layer class, is mirrored on a back layer and left aligned", () => {
  assert.deepEqual(textStyleOf(defaults, "F.SilkS"), { widthUm: 1200, heightUm: 1300, thicknessUm: 180, italic: true, upright: false });
  assert.deepEqual([textStyleOf(defaults, "B.Cu").heightUm, textStyleOf(defaults, "In1.Cu").thicknessUm], [1500, 300]);
  assert.deepEqual([textStyleOf(defaults, "Edge.Cuts").heightUm, textStyleOf(defaults, "F.CrtYd").thicknessUm], [1000, 150], "the classes with no text cells keep KiCad's own constants");
  assert.deepEqual(newTextStyle(defaults, "F.SilkS"), { sizeUm: 1300, strokeWidthUm: 180, mirror: false, justify: "left" });
  assert.deepEqual(newTextStyle(defaults, "B.SilkS"), { sizeUm: 1300, strokeWidthUm: 180, mirror: true, justify: "left" });
  assert.deepEqual(["B.Cu", "B.Fab", "B.CrtYd", "B.Mask", "B.Paste", "B.Adhes"].map(isBackLayer), [true, true, true, true, true, true]);
  assert.deepEqual(["F.Cu", "F.SilkS", "In1.Cu", "Edge.Cuts", "Dwgs.User"].map(isBackLayer), [false, false, false, false, false]);
});

test("the width being drawn is the layer class's until incWidth or decWidth steps it, and a layer change drops the step", () => {
  assert.equal(drawStrokeWidthUm(defaults, "F.SilkS", null), 120);
  assert.equal(drawStrokeWidthUm(defaults, null, null), 120, "no active layer: the silkscreen, as the tools always assumed");
  const up = steppedDrawStroke(defaults, "F.SilkS", null, 1);
  assert.deepEqual(up, { layer: "F.SilkS", um: 220 }, "one WIDTH_STEP (0.1 mm) more than the class's width");
  assert.equal(drawStrokeWidthUm(defaults, "F.SilkS", up), 220);
  assert.equal(drawStrokeWidthUm(defaults, "F.Cu", up), 200, "on another layer the step is gone");
  assert.equal(steppedDrawStroke(defaults, "F.SilkS", up, 1).um, 320);
  assert.equal(steppedDrawStroke(defaults, "F.SilkS", up, -1).um, 120);
  const thin = { layer: "F.SilkS", um: 100 };
  assert.equal(steppedDrawStroke(defaults, "F.SilkS", thin, -1).um, 100, "never down to nothing: the decrement is refused at one step");
});

test("the active layer is read as the state holds it: a state key like \"f_silks\" is the silkscreen, not a layer of its own", () => {
  assert.equal(drawStrokeWidthUm(defaults, "f_silks", null), 120);
  assert.equal(drawStrokeWidthUm(defaults, "f_fab", null), 100);
  assert.equal(drawStrokeWidthUm(defaults, "board_edge", null), 70);
  assert.equal(drawStrokeWidthUm(defaults, "Dwgs_User", null), 100);
  const up = steppedDrawStroke(defaults, "f_silks", null, 1);
  assert.deepEqual(up, { layer: "F.SilkS", um: 220 }, "the step is kept under the layer's name");
  assert.equal(drawStrokeWidthUm(defaults, "f_silks", up), 220);
  assert.equal(drawStrokeWidthUm(defaults, "F.SilkS", up), 220, "and read the same by name");
  assert.equal(drawStrokeWidthUm(defaults, "b_silks", up), 120, "the back silkscreen is another layer");
});
