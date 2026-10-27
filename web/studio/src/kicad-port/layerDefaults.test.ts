import { test } from "node:test";
import assert from "node:assert/strict";
import type { TextGraphicsDefaults } from "../api/types";
import { FACTORY_TEXT_GRAPHICS, STROKE_TOOLS, drawStrokeWidthNm, graphicPolygonStyle, inferBold, isBackLayer, layerClassOf, lineThicknessNm, newTextStyle, steppedDrawStroke, strokeStepKept, textStyleOf } from "./layerDefaults";

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
  assert.deepEqual(newTextStyle(defaults, "F.SilkS"), { sizeNm: 1_300_000, widthNm: 1_200_000, strokeWidthNm: 180, bold: false, italic: true, mirror: false, justify: "left", valign: "bottom" });
  assert.deepEqual(newTextStyle(defaults, "B.SilkS"), { sizeNm: 1_300_000, widthNm: 1_200_000, strokeWidthNm: 180, bold: false, italic: true, mirror: true, justify: "left", valign: "bottom" });
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

// Round 6 A6-1: `m_stroke` belongs to DRAWING_TOOL and is reset only by `Reset()` and when the tool sees a layer other than `m_layer` (at the start of
// drawShape / drawArc / DrawBezier, or on layerChanged in their loops).
test("a stepped width survives re-arming a drawing tool on the same layer, and goes when the tool sees another layer", () => {
  const step = { layer: "F.SilkS", um: 320_000 };
  for (const tool of ["draw_segment", "draw_rect", "draw_circle", "draw_arc", "draw_bezier"]) {
    assert.equal(strokeStepKept(step, tool, "F.SilkS"), step, `${tool} armed again on F.SilkS keeps the step`);
    assert.equal(strokeStepKept(step, tool, "B.SilkS"), null, `${tool} on B.SilkS: m_stroke is B's default again`);
  }
  // A layer change with no stroke tool running is not seen (no layerChanged handler outside the loops), and the graphic polygon is DrawZone's.
  assert.equal(strokeStepKept(step, "select", "B.SilkS"), step);
  assert.equal(strokeStepKept(step, "draw_polygon", "B.SilkS"), step);
  assert.equal(strokeStepKept(null, "draw_segment", "B.SilkS"), null);
  assert.equal(STROKE_TOOLS.has("draw_polygon"), false, "incWidth / decWidth are drawShape's, drawArc's and the Bezier tool's only");
});

test("a board graphic polygon is unfilled on Edge.Cuts and the courtyards, at the layer's own width whatever step the other tools made (A6-2)", () => {
  assert.deepEqual(graphicPolygonStyle(defaults, "Edge.Cuts"), { filled: false, strokeWidthNm: 70_000 });
  assert.deepEqual(graphicPolygonStyle(defaults, "F.CrtYd"), { filled: false, strokeWidthNm: 60_000 });
  assert.deepEqual(graphicPolygonStyle(defaults, "B.CrtYd"), { filled: false, strokeWidthNm: 60_000 });
  assert.deepEqual(graphicPolygonStyle(defaults, "F.SilkS"), { filled: true, strokeWidthNm: 120_000 });
  assert.deepEqual(graphicPolygonStyle(defaults, "F.Cu"), { filled: true, strokeWidthNm: 200_000 });
});

test("a new text is bold when its class's pen is nearer the bold pen than the normal one, as InferBold says (A6-3)", () => {
  // KiCad's factory classes: copper 1.5 mm / 0.3 mm is bold (bold pen 0.3, normal 0.1875); silkscreen 1.0 mm / 0.1 mm is not.
  assert.equal(newTextStyle(undefined, "F.Cu").bold, true);
  assert.equal(newTextStyle(undefined, "In2.Cu").bold, true);
  assert.equal(newTextStyle(undefined, "F.SilkS").bold, false);
  assert.equal(newTextStyle(undefined, "F.Fab").bold, false);
  // The smaller of width and height; a tie is not bold (`<`).
  assert.equal(inferBold(200_000, 1_000_000, 2_000_000), true);
  assert.equal(inferBold(162_500, 1_000_000, 1_000_000), false, "exactly half-way between 0.2 and 0.125");
});
