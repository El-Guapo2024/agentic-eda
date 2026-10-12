import { test } from "node:test";
import assert from "node:assert/strict";
import type { LayerSetup } from "../api/types";
import {
  availableUserLayers,
  boardLayerList,
  canonicalEnabled,
  checkLayerDraft,
  enabledOptionalLayers,
  isCopperLayerName,
  isOptionalLayer,
  itemsPerLayer,
  layerDisplayName,
  layerDraft,
  layerDraftToSend,
  layerIsEnabled,
  layerRows,
  layerTypeOf,
  pcbLayerId,
  sameLayerSetup,
  standardLayerName,
  userLayerNumber,
  withLayerChecked,
  withUserLayer,
} from "./boardLayers";
import { TECH_LAYER_NAMES } from "./layerPresets";

const COPPER4 = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const COPPER2 = ["F.Cu", "B.Cu"];

// The same cases as crates/model/src/layers.rs: the page checks before Apply what the backend refuses after.
test("a board nobody set up has the eighteen technical layers KiCad starts a board with, and the standard names", () => {
  assert.deepEqual(enabledOptionalLayers(undefined), [...TECH_LAYER_NAMES]);
  assert.equal(TECH_LAYER_NAMES.length, 18);
  assert.deepEqual(boardLayerList(COPPER2, null).slice(0, 3), ["F.Cu", "B.Cu", "F.Adhes"]);
  assert.deepEqual(["F.SilkS", "Dwgs.User", "F.Mask", "In1.Cu", "User.3", "F.CrtYd"].map((l) => layerDisplayName(undefined, l)), ["F.Silkscreen", "User.Drawings", "F.Mask", "In1.Cu", "User.3", "F.Courtyard"]);
  assert.equal(standardLayerName("B.Adhes"), "B.Adhesive");
});

test("the names and numbers of layers are the file's", () => {
  assert.deepEqual(["User.1", "User.45", "User.46", "User.0", "User.07", "Users.1", "User."].map(userLayerNumber), [1, 45, null, null, null, null, null]);
  assert.ok(["F.Cu", "B.Cu", "In1.Cu", "In30.Cu"].every(isCopperLayerName));
  assert.ok(!["In0.Cu", "In31.Cu", "F.Mask", "In01.Cu", "Cu"].some(isCopperLayerName));
  assert.ok(isOptionalLayer("F.Fab") && isOptionalLayer("User.12") && !isOptionalLayer("F.Cu") && !isOptionalLayer("Rescue"));
  assert.deepEqual(["F.Cu", "F.Mask", "B.Cu", "B.Mask", "In1.Cu", "F.SilkS", "In2.Cu", "B.SilkS", "Edge.Cuts", "Margin", "B.CrtYd", "F.CrtYd", "B.Fab", "F.Fab", "User.1", "User.45", "In30.Cu"].map(pcbLayerId), [0, 1, 2, 3, 4, 5, 6, 7, 25, 27, 29, 31, 33, 35, 39, 127, 62]);
});

test("two pages that mean the same are the same page", () => {
  const a: LayerSetup = { enabled: ["User.2", "User.1", "B.Fab", ...TECH_LAYER_NAMES.filter((l) => l !== "B.Fab")], names: { "F.SilkS": "F.Silkscreen", "User.1": "Notes" }, types: { "In1.Cu": "signal", "User.2": "front" } };
  assert.deepEqual(enabledOptionalLayers(a).slice(16), ["F.Fab", "B.Fab", "User.1", "User.2"], "the technical layers in KiCad's order, then the user layers by number");
  assert.ok(sameLayerSetup(a, { enabled: [...TECH_LAYER_NAMES, "User.1", "User.2"], names: { "User.1": "Notes" }, types: { "User.2": "front" } }));
  assert.ok(!sameLayerSetup(a, { enabled: [...TECH_LAYER_NAMES, "User.1", "User.2"], names: { "User.1": "Notes" } }));
  assert.ok(sameLayerSetup(undefined, { enabled: [...TECH_LAYER_NAMES].reverse() }), "the order of the list is not part of the page");
  assert.deepEqual(canonicalEnabled(["User.3", "Edge.Cuts", "User.1", "User.3"]), ["Edge.Cuts", "User.1", "User.3"]);
});

test("the dialog stacks the rows front to back, with the copper layers in the middle", () => {
  const rows = layerRows(layerDraft({ enabled: [...TECH_LAYER_NAMES, "User.2", "User.1"] }, COPPER4), COPPER4).map((r) => r.layer);
  assert.deepEqual(rows, ["F.CrtYd", "F.Fab", "F.Adhes", "F.Paste", "F.SilkS", "F.Mask", "F.Cu", "In1.Cu", "In2.Cu", "B.Cu", "B.Mask", "B.SilkS", "B.Paste", "B.Adhes", "B.Fab", "B.CrtYd", "Edge.Cuts", "Margin", "Eco1.User", "Eco2.User", "Cmts.User", "Dwgs.User", "User.1", "User.2"]);
  const row = (l: string) => layerRows(layerDraft(undefined, COPPER4), COPPER4).find((r) => r.layer === l)!;
  assert.deepEqual([row("F.CrtYd").locked, row("Margin").locked, row("In1.Cu").locked, row("F.SilkS").locked], [true, true, true, false], "the courtyards, the outline, the margin and the copper layers cannot be switched");
  assert.match(row("In1.Cu").lockedBecause ?? "", /Physical Stackup/);
  assert.deepEqual([row("In1.Cu").typeChoices, row("F.SilkS").typeChoices], [["signal", "power", "mixed", "jumper"], []]);
  assert.equal(row("F.Fab").note, "Off-board, manufacturing");
  assert.equal(row("Edge.Cuts").note, "Board contour");
});

test("a user layer is added with the lowest number you pick, auxiliary, called what KiCad calls it, and can be turned off again", () => {
  let d = layerDraft(undefined, COPPER2);
  assert.equal(availableUserLayers(d).length, 45);
  d = withUserLayer(d, "User.3");
  assert.deepEqual([d.enabled.includes("User.3"), d.shown, d.names["User.3"], d.types["User.3"]], [true, ["User.3"], "User.3", "auxiliary"]);
  assert.equal(availableUserLayers(d).length, 44);
  assert.equal(withUserLayer(d, "User.3"), d, "a layer that has a row is not added twice");
  assert.equal(withUserLayer(d, "User.46"), d);
  d = withLayerChecked(d, "User.3", false);
  assert.ok(!d.enabled.includes("User.3") && d.shown.includes("User.3"), "the row stays, unchecked");
  assert.equal(withLayerChecked(d, "Edge.Cuts", false), d, "a mandatory layer stays");
  assert.equal(withLayerChecked(d, "F.Cu", false), d, "copper is the stackup's");
  assert.ok(!withLayerChecked(d, "Eco1.User", false).enabled.includes("Eco1.User"), "a technical layer can be turned off");
});

test("the checks are the panel's: a name is not empty, has none of the forbidden characters, is not 'signal' and is unique", () => {
  const check = (f: (d: ReturnType<typeof layerDraft>) => void, copper = COPPER4) => {
    const d = layerDraft(undefined, copper);
    f(d);
    return checkLayerDraft(d, copper).errors;
  };
  assert.deepEqual(check(() => {}), {});
  assert.deepEqual(check((d) => (d.names["F.SilkS"] = "")), { "F.SilkS": "Layer must have a name." });
  assert.match(check((d) => (d.names["F.SilkS"] = "a/b"))["F.SilkS"] ?? "", /are forbidden in layer names\.$/);
  for (const bad of ['a"b', "a:b", "a*b", "a%b", "a|b", "tab\t"]) assert.ok("F.SilkS" in check((d) => (d.names["F.SilkS"] = bad)), bad);
  assert.deepEqual(check((d) => (d.names["In2.Cu"] = "signal")), { "In2.Cu": 'Layer name "signal" is reserved.' });
  // The later of two clashing names in PCB_LAYER_ID order is the one marked: B.Fab is 33, F.Fab 35.
  assert.deepEqual(check((d) => (d.names["F.Fab"] = "B.Fab")), { "F.Fab": "Layer name 'B.Fab' already in use." });
  assert.deepEqual(check((d) => (d.names["F.Mask"] = "B.Mask")), { "B.Mask": "Layer name 'B.Mask' already in use." });
  // A name another layer gave up is free.
  assert.deepEqual(check((d) => { d.names["F.Fab"] = "Assembly"; d.names["B.Fab"] = "F.Fab"; }), {});
  // A layer that is off does not count.
  assert.deepEqual(check((d) => { d.names["Eco1.User"] = ""; d.enabled = d.enabled.filter((l) => l !== "Eco1.User"); }), {});
});

test("the page sends the layers that are on and only the names and types that are not the defaults", () => {
  let d = layerDraft(undefined, COPPER4);
  assert.deepEqual(layerDraftToSend(d, COPPER4), { enabled: [...TECH_LAYER_NAMES] });
  d = withUserLayer(withUserLayer(d, "User.2"), "User.1");
  d.names["User.1"] = "Notes";
  d.names["In1.Cu"] = "GND plane";
  d.names["F.SilkS"] = "F.Silkscreen";
  d.types["In1.Cu"] = "power";
  d.types["In2.Cu"] = "signal";
  d.types["User.2"] = "back";
  d = withLayerChecked(d, "Eco2.User", false);
  const sent = layerDraftToSend(d, COPPER4);
  assert.deepEqual(sent, { enabled: [...TECH_LAYER_NAMES.filter((l) => l !== "Eco2.User"), "User.1", "User.2"], names: { "In1.Cu": "GND plane", "User.1": "Notes" }, types: { "In1.Cu": "power", "User.2": "back" } });
  assert.ok(sameLayerSetup(layerDraftToSend(layerDraft(sent, COPPER4), COPPER4), sent), "the page reads back as it was sent");
  // The names of layers the board does not have are not sent (a 2-layer board keeps no In1.Cu name).
  assert.equal(layerDraftToSend({ ...d, names: { ...d.names, "In1.Cu": "Gone" } }, COPPER2).names?.["In1.Cu"], undefined);
});

test("what a board has follows its stackup and its page", () => {
  const s: LayerSetup = { enabled: ["Edge.Cuts", "User.4"] };
  assert.ok(layerIsEnabled(s, COPPER4, "In2.Cu") && !layerIsEnabled(s, COPPER2, "In2.Cu"));
  assert.ok(layerIsEnabled(s, COPPER4, "User.4") && !layerIsEnabled(s, COPPER4, "User.5") && !layerIsEnabled(s, COPPER4, "F.Fab"));
  assert.deepEqual([layerTypeOf({ enabled: [], types: { "In1.Cu": "mixed" } }, "In1.Cu"), layerTypeOf(undefined, "F.Cu"), layerTypeOf(undefined, "User.1")], ["mixed", "signal", "auxiliary"]);
});

test("the items on each layer are counted so the page can say what a layer holds before it is turned off", () => {
  assert.deepEqual(itemsPerLayer({ shapes: [{ layer: "Eco1.User" }, { layer: "F.Fab" }], texts: [{ layer: "Eco1.User" }], dimensions: [{ layer: "Dwgs.User" }] }), { "Eco1.User": 2, "F.Fab": 1, "Dwgs.User": 1 });
  assert.deepEqual(itemsPerLayer(null), {});
});
