import { test } from "node:test";
import assert from "node:assert/strict";
import type { PadLayerShape, PadStack } from "../api/types";
import { shapeFormOf } from "./padShape";
import { BACK, FRONT, INNER, changeMode, orderedStackLayers, checkStack, copperLayersChoices, editLayerChoices, layerFormOf, layerFormOfShape, layerShapeOf, padstackControlsShown, stackDiffers, stackOf, stackStateOf, withLayerForm, type LayerForm } from "./padStack";

const front = (shape: "circle" | "rect" | "oval" = "circle", size: [number, number] = [1600, 1600]): LayerForm => ({ s: shapeFormOf({ shape, size, w: size[0], h: size[1] }), ox: 0, oy: 0, tokens: [] });
const oval: PadLayerShape = { shape: "oval", size: [2400, 1000] };
const square: PadLayerShape = { shape: "rect", size: [3000, 3000] };
const fib: PadStack = { mode: "front_inner_back", inner: square, back: oval };
const four = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];

test("a layer shape goes to a form and back, with its offset and the tokens of its node", () => {
  const shape: PadLayerShape = { shape: "round_rect", size: [2000, 1000], roundrect_ratio: 0.3, offset: [150, -50], tokens: ["(thermal_gap 0.3)"] };
  const form = layerFormOfShape(shape);
  assert.deepEqual([form.s.choice, form.s.w, form.s.h, form.s.ratio, form.ox, form.oy], ["round_rect", 2000, 1000, 0.3, 150, -50]);
  assert.deepEqual(layerShapeOf(form), shape);
  // A circle has one size, and a shape that has no offset has none.
  const circle = layerShapeOf(layerFormOfShape({ shape: "circle", size: [1400, 900] }));
  assert.deepEqual(circle, { shape: "circle", size: [1400, 1400] });
});

test("a stack opens as its mode, with the layers it names and the front's shape for any it does not", () => {
  const normal = stackStateOf(null, front());
  assert.deepEqual([normal.mode, normal.unconnected, normal.editLayer, Object.keys(normal.layers)], ["normal", "keep_all", FRONT, []]);
  const s = stackStateOf({ ...fib, unconnected: "remove_all" }, front());
  assert.deepEqual([s.mode, s.unconnected, Object.keys(s.layers)], ["front_inner_back", "remove_all", [INNER, BACK]]);
  assert.equal(s.layers[INNER]!.s.choice, "rect");
  assert.equal(s.layers[BACK]!.s.choice, "oval");
  // A front/inner/back stack with a layer missing shows the front's shape there.
  const partial = stackStateOf({ mode: "front_inner_back", back: oval }, front("rect", [1000, 600]));
  assert.deepEqual([partial.layers[INNER]!.s.choice, partial.layers[INNER]!.s.w, partial.layers[BACK]!.s.choice], ["rect", 1000, "oval"]);
  const custom = stackStateOf({ mode: "custom", layers: { "In2.Cu": square } }, front());
  assert.deepEqual(Object.keys(custom.layers), ["In2.Cu"]);
});

test("the Edit layer choice is the front, then the inner layers and the back, or every copper layer of the board", () => {
  assert.deepEqual(editLayerChoices("normal", four), [{ key: FRONT, label: "F.Cu" }]);
  assert.deepEqual(editLayerChoices("front_inner_back", four), [
    { key: FRONT, label: "F.Cu" },
    { key: INNER, label: "Inner Layers" },
    { key: BACK, label: "B.Cu" },
  ]);
  assert.deepEqual(editLayerChoices("custom", four).map((c) => c.key), [FRONT, "In1.Cu", "In2.Cu", "B.Cu"]);
  assert.deepEqual(editLayerChoices("custom", []).map((c) => c.key), [FRONT, BACK], "a board that says no layers is a two-layer one");
});

test("choosing a mode starts the new layers as copies of the front, or of the inner layers for an inner one, and drops the ones the mode has not", () => {
  const f = front("circle", [1600, 1600]);
  let s = stackStateOf(null, f);
  s = changeMode(s, "front_inner_back", f, four);
  assert.deepEqual(Object.keys(s.layers), [INNER, BACK]);
  assert.equal(s.layers[INNER]!.s.choice, "circle");
  assert.equal(s.editLayer, FRONT, "the front stays the layer being edited");
  // The inner layers get their own shape, then the mode becomes custom: In1 takes it, and so does In2 (a new inner layer starts from the inner layers' shape), the back keeps its own.
  const inner = layerFormOfShape(square);
  ({ state: s } = withLayerForm(s, f, INNER, inner));
  s = changeMode(s, "custom", f, four);
  assert.deepEqual(Object.keys(s.layers), ["In1.Cu", "In2.Cu", "B.Cu"]);
  assert.equal(s.layers["In1.Cu"]!.s.choice, "rect");
  assert.equal(s.layers["In2.Cu"]!.s.choice, "rect");
  assert.equal(s.layers["B.Cu"]!.s.choice, "circle");
  // Back to front/inner/back: the inner layers' shape is In1's, the back's its own.
  s = changeMode(s, "front_inner_back", f, four);
  assert.deepEqual([s.layers[INNER]!.s.choice, s.layers[BACK]!.s.choice], ["rect", "circle"]);
  // Normal drops the layers; the unconnected-layer mode stays.
  s = changeMode({ ...s, unconnected: "remove_all" }, "normal", f, four);
  assert.deepEqual([Object.keys(s.layers), s.unconnected, s.editLayer], [[], "remove_all", FRONT]);
  // The same mode is nothing to do.
  assert.equal(changeMode(s, "normal", f, four), s);
});

test("the layer being edited is the front's form, the dialog's own, or the stack's", () => {
  const f = front();
  const s = stackStateOf(fib, f);
  assert.equal(layerFormOf(s, f, FRONT), f);
  assert.equal(layerFormOf(s, f, INNER).s.choice, "rect");
  assert.equal(layerFormOf(s, f, "In9.Cu"), f, "a layer the stack does not have is the front's");
  const next = layerFormOfShape({ shape: "oval", size: [900, 500] });
  const r = withLayerForm(s, f, BACK, next);
  assert.equal(r.state.layers[BACK], next);
  assert.equal(r.front, f);
  const r2 = withLayerForm(s, f, FRONT, next);
  assert.equal(r2.front, next);
  assert.equal(r2.state, s);
});

test("OK sends the stack: no layers for a normal one, the inner and back for front/inner/back, the layers for a custom one", () => {
  const f = front();
  assert.deepEqual(stackOf(stackStateOf(null, f)), {});
  assert.deepEqual(stackOf({ ...stackStateOf(null, f), unconnected: "remove_except_start_and_end" }), { unconnected: "remove_except_start_and_end" });
  assert.deepEqual(stackOf(stackStateOf(fib, f)), fib);
  const custom: PadStack = { mode: "custom", layers: { "In1.Cu": square, "B.Cu": oval }, unconnected: "remove_all" };
  assert.deepEqual(stackOf(stackStateOf(custom, f)), custom);
});

test("a stack that says the same as another is the same, whatever the keys that say nothing", () => {
  assert.equal(stackDiffers(null, {}), false);
  assert.equal(stackDiffers(undefined, { mode: "normal", unconnected: "keep_all", layers: {} }), false);
  assert.equal(stackDiffers(fib, { ...fib, inner: { ...square, tokens: [], offset: null } }), false);
  assert.equal(stackDiffers(fib, { ...fib, back: { ...oval, size: [2400, 1100] } }), true);
  assert.equal(stackDiffers(fib, { ...fib, unconnected: "remove_all" }), true);
  assert.equal(stackDiffers({ mode: "custom", layers: {} }, null), true);
});

test("the padstack controls are the plated through-hole pad's, with the three Copper layers choices", () => {
  assert.equal(padstackControlsShown("through_hole"), true);
  assert.equal(padstackControlsShown("smd"), false);
  assert.equal(padstackControlsShown("non_plated_hole"), false);
  assert.deepEqual(copperLayersChoices().map((c) => c.label), ["All copper layers", "F.Cu, B.Cu and connected layers", "Connected layers only"]);
});

test("a layer is checked as a pad is, and named in the message", () => {
  const f = front();
  const s = stackStateOf(fib, f);
  assert.equal(checkStack(s), null);
  const bad = withLayerForm(s, f, INNER, layerFormOfShape({ shape: "trapezoid", size: [1000, 600], trapezoid_delta: [0, 1100] })).state;
  assert.match(checkStack(bad) ?? "", /trapezoid delta is too large.*the inner layers/);
});

test("a padstack pad is drawn from the inner layers out, its own side last", () => {
  const groups = [{ layers: ["F.Cu"] }, { layers: ["In1.Cu", "In2.Cu"] }, { layers: ["B.Cu"] }];
  assert.deepEqual(orderedStackLayers(groups, "top").map((g) => g.layers[0]), ["In1.Cu", "B.Cu", "F.Cu"]);
  assert.deepEqual(orderedStackLayers(groups, "bottom").map((g) => g.layers[0]), ["In1.Cu", "F.Cu", "B.Cu"]);
});
