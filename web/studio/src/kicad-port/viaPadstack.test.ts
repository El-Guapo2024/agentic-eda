import { test } from "node:test";
import assert from "node:assert/strict";
import { hasLayerDiameters, viaLayerStack, viaMaxDiameter, viaWidthOn } from "./viaPadstack";
import type { Via } from "../api/types";

const MM = 1_000_000;
const COPPER = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const via = (over: Partial<Via> = {}): Via => ({ id: "v", net: "GND", x: 0, y: 0, d: 0.6 * MM, drill: 0.3 * MM, from: "F.Cu", to: "B.Cu", ...over });
const fib = via({ props: { padstack: { mode: "front_inner_back", sizes: { Inner: 1.0 * MM, "B.Cu": 0.8 * MM } } } });

test("PCB_VIA::GetWidth( aLayer ): the front's, Inner for each inner layer of a front/inner/back stack, a custom stack's own layer, else the front's", () => {
  assert.deepEqual(COPPER.map((l) => viaWidthOn(fib, l)), [0.6 * MM, 1.0 * MM, 1.0 * MM, 0.8 * MM]);
  const custom = via({ props: { padstack: { mode: "custom", sizes: { "In2.Cu": 0.4 * MM } } } });
  assert.deepEqual(COPPER.map((l) => viaWidthOn(custom, l)), [0.6 * MM, 0.6 * MM, 0.4 * MM, 0.6 * MM]);
  assert.deepEqual(COPPER.map((l) => viaWidthOn(via(), l)), [0.6 * MM, 0.6 * MM, 0.6 * MM, 0.6 * MM]);
});

test("the widest diameter is the via's box and the router's preview of it (largestDiameter)", () => {
  assert.equal(viaMaxDiameter(fib), 1.0 * MM);
  assert.equal(viaMaxDiameter(via()), 0.6 * MM);
  assert.equal(hasLayerDiameters(fib), true);
  assert.equal(hasLayerDiameters(via()), false);
  // a stack that names layers at the front's size draws as one disc
  assert.equal(hasLayerDiameters(via({ props: { padstack: { mode: "custom", sizes: { "B.Cu": 0.6 * MM } } } })), false);
});

test("the rings stack as the copper layers do, B.Cu at the bottom and the active copper layer on top (SetTopLayer)", () => {
  const shown = () => true;
  assert.deepEqual(viaLayerStack(fib, COPPER, "F.Cu", shown), [
    { layer: "B.Cu", d: 0.8 * MM },
    { layer: "In2.Cu", d: 1.0 * MM },
    { layer: "In1.Cu", d: 1.0 * MM },
    { layer: "F.Cu", d: 0.6 * MM },
  ]);
  assert.deepEqual(viaLayerStack(fib, COPPER, "In2.Cu", shown).map((r) => r.layer), ["B.Cu", "In1.Cu", "F.Cu", "In2.Cu"]);
  // a technical layer active: the copper order as it is
  assert.deepEqual(viaLayerStack(fib, COPPER, "F.SilkS", shown).map((r) => r.layer), ["B.Cu", "In2.Cu", "In1.Cu", "F.Cu"]);
  // a hidden layer and a layer the via does not flash on draw no ring
  assert.deepEqual(viaLayerStack({ ...fib, flashed: ["F.Cu", "In1.Cu", "B.Cu"] }, COPPER, "F.Cu", (l) => l !== "In1.Cu").map((r) => r.layer), ["B.Cu", "F.Cu"]);
  // a blind via spans its own layers
  assert.deepEqual(viaLayerStack({ ...fib, from: "F.Cu", to: "In1.Cu" }, COPPER, "B.Cu", shown).map((r) => r.layer), ["In1.Cu", "F.Cu"]);
});
