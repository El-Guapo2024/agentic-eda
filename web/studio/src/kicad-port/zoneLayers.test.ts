import { test } from "node:test";
import assert from "node:assert/strict";
import { orderedLayerSet, zoneCopperLayers, zoneDialogKind, zoneDialogNet, zoneIsOnCopper, zoneIsOnLayer, zoneLayerChoices, zoneLayers, zonePickLayer, convertZoneTitle } from "./zoneLayers";

test("a zone from a backend without layer sets is on its layer", () => {
  assert.deepEqual(zoneLayers({ layer: "F.Cu" }), ["F.Cu"]);
  assert.deepEqual(zoneLayers({ layer: "F.Cu", layers: [] }), ["F.Cu"]);
  assert.deepEqual(zoneLayers({ layer: "F.Cu", layers: ["F.Cu", "B.Cu"] }), ["F.Cu", "B.Cu"]);
});

test("a zone is on a layer of its set, and is a copper zone when the set has a copper layer", () => {
  const both = { layer: "F.Cu", layers: ["F.Cu", "B.Cu"] };
  assert.ok(zoneIsOnLayer(both, "B.Cu") && !zoneIsOnLayer(both, "In1.Cu"));
  assert.ok(zoneIsOnCopper(both));
  const silk = { layer: "F.SilkS", layers: ["F.SilkS", "F.Mask"] };
  assert.ok(!zoneIsOnCopper(silk));
  assert.deepEqual(zoneCopperLayers({ layer: "F.Cu", layers: ["F.Cu", "In1.Cu", "F.SilkS"] }), ["F.Cu", "In1.Cu"]);
});

test("the layer that decides whether a zone can be picked is the active one it is on, else the first one shown", () => {
  const z = { layer: "F.Cu", layers: ["F.Cu", "B.Cu"] };
  assert.equal(zonePickLayer(z, {}, "B.Cu"), "B.Cu");
  assert.equal(zonePickLayer(z, {}, "In1.Cu"), "F.Cu", "an active layer the zone is not on does not matter");
  assert.equal(zonePickLayer(z, { "F.Cu": false }, null), "B.Cu", "the front is hidden: the back stands for it");
  assert.equal(zonePickLayer(z, { "F.Cu": false, "B.Cu": false }, null), "F.Cu", "nothing shown: its first layer, which is hidden");
  assert.equal(zonePickLayer({ layer: "F.SilkS", layers: ["F.SilkS", "F.Mask"] }, {}, "f_mask"), "F.Mask", "an active layer is a state key");
});

test("the dialog a zone gets follows its layers", () => {
  assert.equal(zoneDialogKind(false, ["F.Cu"]), "copper");
  assert.equal(zoneDialogKind(false, ["F.SilkS", "B.SilkS"]), "non-copper");
  assert.equal(zoneDialogKind(true, ["F.Cu"]), "rule-area");
  const copper = ["F.Cu", "B.Cu"];
  const optional = ["F.SilkS", "F.Mask"];
  assert.deepEqual(zoneLayerChoices("copper", copper, optional), copper);
  assert.deepEqual(zoneLayerChoices("non-copper", copper, optional), optional);
  assert.deepEqual(zoneLayerChoices("rule-area", copper, optional), ["F.Cu", "B.Cu", "F.SilkS", "F.Mask"]);
  assert.deepEqual(orderedLayerSet(["B.Cu", "F.Cu"], copper), ["F.Cu", "B.Cu"], "the dialog lists the layers in the board's order");
});

test("R5-Z3: OK on a copper zone on no net keeps it on no net; a new zone starts on the first net; <no net> can be chosen", () => {
  const nets = ["+3V3", "GND"];
  assert.equal(zoneDialogNet("copper", "", nets), "", "an existing zone on no net (Edit_Zone_Params keeps net 0)");
  assert.equal(zoneDialogNet("copper", "GND", nets), "GND");
  assert.equal(zoneDialogNet("copper", null, nets), "+3V3", "a new zone nothing was picked for");
  assert.equal(zoneDialogNet("copper", null, []), "");
  assert.equal(zoneDialogNet("rule-area", null, nets), "", "a rule area has no net unless it holds one");
  assert.equal(zoneDialogNet("non-copper", "GND", nets), "");
});

test("Create Zone / Rule Area from Selection is one dialog, titled for the conversion (round 6 D6-2)", () => {
  assert.equal(convertZoneTitle("copper"), "Convert to Copper Zone");
  assert.equal(convertZoneTitle("non-copper"), "Convert to Non Copper Zone");
  assert.equal(convertZoneTitle("rule-area"), "Convert to Rule Area");
});
