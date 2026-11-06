import { test } from "node:test";
import assert from "node:assert/strict";
import { copperChoices, holeShapeSelected, layersOfForm, padLayerForm, padToDieOf, withPadFab } from "./padLayers";

test("the copper choices are UpdateLayersDropdown's for each pad type", () => {
  assert.deepEqual(copperChoices("pth"), ["All copper layers", "F.Cu, B.Cu and connected layers", "Connected layers only", "None"]);
  assert.deepEqual(copperChoices("npth"), ["F.Cu and B.Cu", "F.Cu", "B.Cu", "None"]);
  assert.deepEqual(copperChoices("smd"), ["F.Cu", "B.Cu"]);
  assert.deepEqual(copperChoices("aperture"), ["None"]);
});

test("a pad's layers show as updatePadLayersList shows them and go back as transferDataToPad and formatLayers write them", () => {
  const pth = padLayerForm("pth", ["*.Cu", "*.Mask"], "keep_all");
  assert.equal(pth.copper, 0);
  assert.deepEqual([...pth.tech].sort(), ["B.Mask", "F.Mask"]);
  assert.deepEqual(layersOfForm("pth", pth), { layers: ["*.Cu", "*.Mask"], mode: "keep_all" });
  // the plated hole's copper choice is its unconnected-layer mode
  assert.equal(padLayerForm("pth", ["*.Cu", "*.Mask"], "remove_except_start_and_end").copper, 1);
  assert.deepEqual(layersOfForm("pth", { copper: 2, tech: new Set(["F.Mask"]) }), { layers: ["*.Cu", "F.Mask"], mode: "remove_all" });
  assert.deepEqual(layersOfForm("pth", { copper: 3, tech: new Set(["F.Mask", "B.Mask", "F.Paste"]) }).layers, ["*.Mask", "F.Paste"]);
  // an SMD pad on the back, with paste and silkscreen
  const smd = padLayerForm("smd", ["B.Cu", "B.Paste", "B.Mask"], "keep_all");
  assert.equal(smd.copper, 1);
  assert.deepEqual(layersOfForm("smd", { copper: 0, tech: new Set(["F.Paste", "F.Mask", "F.SilkS"]) }).layers, ["F.Cu", "F.Mask", "F.SilkS", "F.Paste"]);
  // an SMD pad with no copper falls back to the type's mask ("no "no copper" radio choice")
  assert.equal(padLayerForm("smd", ["F.Paste"], "keep_all").copper, 0);
  // a non-plated hole: front only
  // the wildcards first, then each layer (`formatLayers`)
  assert.deepEqual(layersOfForm("npth", { copper: 1, tech: new Set(["F.Mask", "B.Mask"]) }).layers, ["*.Mask", "F.Cu"]);
  assert.equal(padLayerForm("npth", ["*.Cu", "*.Mask"], "keep_all").copper, 0);
  assert.equal(padLayerForm("npth", ["*.Mask"], "keep_all").copper, 3);
});

test("the fabrication property and the pad-to-die length and delay are the pad's kept forms", () => {
  assert.deepEqual(padToDieOf({ tokens: ["(die_length 1.2)", "(pintype \"x\")"] }), { length: 1_200_000, delay: null });
  const k = withPadFab({ tokens: ["(pintype \"x\")"], property: "pad_prop_bga" }, "pad_prop_testpoint", 2_500_000, 12.5);
  assert.equal(k.property, "pad_prop_testpoint");
  assert.deepEqual(k.tokens, ["(die_length 2.5)", "(die_delay 12.5)", "(pintype \"x\")"]);
  const off = withPadFab(k, null, null, null);
  assert.equal(off.property, undefined);
  assert.deepEqual(off.tokens, ["(pintype \"x\")"]);
  assert.deepEqual(withPadFab(undefined, null, null, null), {});
});

test("switching the hole between Round and Oblong keeps its size (E7-F6)", () => {
  assert.deepEqual(holeShapeSelected(800_000, null, "oblong", null), { drill: null, drill_slot: [800_000, 800_000] });
  assert.deepEqual(holeShapeSelected(800_000, null, "oblong", 1_200_000), { drill: null, drill_slot: [800_000, 1_200_000] });
  assert.deepEqual(holeShapeSelected(null, [600_000, 1_000_000], "round", null), { drill: 600_000, drill_slot: null });
});
