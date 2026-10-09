import { test } from "node:test";
import assert from "node:assert/strict";
import { PropertyManager, type PropItem, type PropertyDef } from "./propertyManager";

interface Item extends PropItem {
  v?: number;
}
type Def = PropertyDef<Item, undefined, string>;

const def = (owner: string, name: string, over: Partial<Def> = {}): Def => ({ owner, name, kind: "int", get: () => 0, set: () => [], ...over });

/** The shape of pcbnew's hierarchy: BOARD_ITEM <- BOARD_CONNECTED_ITEM <- TRACK, VIA (masks a layer), and a text built from two bases. */
function manager(): PropertyManager<Item, undefined, string> {
  const pm = new PropertyManager<Item, undefined, string>();
  pm.registerType("BOARD_ITEM");
  pm.addProperty(def("BOARD_ITEM", "Position X"));
  pm.addProperty(def("BOARD_ITEM", "Position Y"));
  pm.addProperty(def("BOARD_ITEM", "Layer"));
  pm.addProperty(def("BOARD_ITEM", "Locked", { kind: "bool" }));

  pm.inheritsAfter("BOARD_CONNECTED_ITEM", "BOARD_ITEM");
  pm.replaceProperty("BOARD_ITEM", "Layer", def("BOARD_CONNECTED_ITEM", "Layer"));
  pm.addProperty(def("BOARD_CONNECTED_ITEM", "Net", { kind: "net" }));
  pm.addProperty(def("BOARD_CONNECTED_ITEM", "Teardrops on", { kind: "bool" }), "Teardrops");

  pm.inheritsAfter("TRACK", "BOARD_CONNECTED_ITEM");
  pm.addProperty(def("TRACK", "Width"));
  pm.replaceProperty("BOARD_ITEM", "Position X", def("TRACK", "Start X"));
  pm.replaceProperty("BOARD_ITEM", "Position Y", def("TRACK", "Start Y"));
  pm.addProperty(def("TRACK", "End X"));
  pm.addProperty(def("TRACK", "End Y"));

  pm.inheritsAfter("VIA", "BOARD_CONNECTED_ITEM");
  pm.mask("VIA", "BOARD_CONNECTED_ITEM", "Layer");
  pm.addProperty(def("VIA", "Diameter"), "Via Properties");
  pm.addProperty(def("VIA", "Hole"), "Via Properties");

  pm.registerType("EDA_TEXT");
  pm.addProperty(def("EDA_TEXT", "Orientation"));
  pm.addProperty(def("EDA_TEXT", "Text", { kind: "string" }), "Text Properties");
  pm.addProperty(def("EDA_TEXT", "Color"), "Text Properties");
  pm.inheritsAfter("TEXT", "BOARD_ITEM");
  pm.inheritsAfter("TEXT", "EDA_TEXT");
  pm.mask("TEXT", "EDA_TEXT", "Color");
  pm.addProperty(def("TEXT", "Knockout", { kind: "bool" }), "Text Properties");
  return pm;
}

const names = (pm: PropertyManager<Item, undefined, string>, type: string, group?: string): string[] => {
  const order = pm.getDisplayOrder(type);
  return pm
    .getProperties(type)
    .filter((p) => group === undefined || (p.group ?? "") === group)
    .sort((a, b) => order.get(a)! - order.get(b)!)
    .map((p) => p.name);
};

test("a base class's properties come before its derived class's, base declared first", () => {
  const pm = manager();
  // BOARD_ITEM's Position X/Y are replaced by the track's Start X/Y, and its Layer by BOARD_CONNECTED_ITEM's, which comes right after Locked.
  assert.deepEqual(names(pm, "TRACK"), ["Locked", "Layer", "Net", "Teardrops on", "Width", "Start X", "Start Y", "End X", "End Y"]);
  assert.deepEqual(names(pm, "BOARD_ITEM"), ["Position X", "Position Y", "Layer", "Locked"]);
});

test("a mask hides a base class's property from the class that masks it", () => {
  const pm = manager();
  assert.deepEqual(names(pm, "VIA"), ["Position X", "Position Y", "Locked", "Net", "Teardrops on", "Diameter", "Hole"]);
  assert.equal(pm.getProperty("VIA", "Layer"), undefined, "masked: the via has no Layer");
  assert.ok(pm.getProperty("TRACK", "Layer"), "and the track still has it");
});

test("a class with two bases lists the one declared first above the one declared last", () => {
  const pm = manager();
  // TEXT inherits BOARD_ITEM then EDA_TEXT: the walk takes EDA_TEXT first, so BOARD_ITEM ends up above it. Color is masked by TEXT.
  assert.deepEqual(names(pm, "TEXT"), ["Position X", "Position Y", "Layer", "Locked", "Orientation", "Text", "Knockout"]);
  assert.deepEqual(names(pm, "TEXT", "Text Properties"), ["Text", "Knockout"]);
});

test("the groups of a class are its own, then those of its bases in order, each once", () => {
  const pm = manager();
  assert.deepEqual(pm.getGroupDisplayOrder("TRACK"), ["", "Teardrops"]);
  assert.deepEqual(pm.getGroupDisplayOrder("VIA"), ["", "Via Properties", "Teardrops"]);
  assert.deepEqual(pm.getGroupDisplayOrder("TEXT"), ["", "Text Properties"]);
});

test("the first property registered under a name keeps it", () => {
  const pm = manager();
  const again = pm.addProperty(def("BOARD_ITEM", "Locked", { kind: "string" }));
  assert.equal(again.kind, "bool", "the duplicate is dropped and the first returned");
  assert.equal(pm.getProperties("BOARD_ITEM").filter((p) => p.name === "Locked").length, 1);
});

test("a name is found without regard to case", () => {
  const pm = manager();
  assert.equal(pm.getProperty("TRACK", "start x")?.name, "Start X");
});

test("availability: the property's own test, then the item's class's override -- overrides are not inherited", () => {
  const pm = manager();
  pm.addProperty(def("BOARD_ITEM", "Soldermask", { kind: "bool", available: (i) => i.v !== 0 }));
  pm.overrideAvailability("TRACK", "BOARD_ITEM", "Soldermask", (i) => (i.v ?? 0) > 5);
  const soldermask = pm.getProperty("TRACK", "Soldermask")!;
  assert.equal(pm.isAvailableFor("TRACK", soldermask, { type: "TRACK", v: 3 }, undefined), false, "the override says no");
  assert.equal(pm.isAvailableFor("TRACK", soldermask, { type: "TRACK", v: 7 }, undefined), true);
  assert.equal(pm.isAvailableFor("TRACK", soldermask, { type: "TRACK", v: 0 }, undefined), false, "the property's own test still has to pass");
  // VIA inherits BOARD_CONNECTED_ITEM and so BOARD_ITEM, but it has no override of its own.
  assert.equal(pm.isAvailableFor("VIA", pm.getProperty("VIA", "Soldermask")!, { type: "VIA", v: 3 }, undefined), true);
});

test("writeability: no setter is read-only, the property's test and the class's override both have to allow", () => {
  const pm = manager();
  const ro = pm.addProperty(def("BOARD_ITEM", "Parent name", { kind: "string", set: undefined }));
  assert.equal(pm.isWriteableFor("BOARD_ITEM", ro, { type: "BOARD_ITEM" }, undefined), false);
  pm.addProperty(def("BOARD_ITEM", "Fill", { writeable: (i) => i.v === 1 }));
  pm.overrideWriteability("TRACK", "BOARD_ITEM", "Fill", (i) => i.v !== 2);
  const fill = pm.getProperty("TRACK", "Fill")!;
  assert.equal(pm.isWriteableFor("TRACK", fill, { type: "TRACK", v: 1 }, undefined), true);
  assert.equal(pm.isWriteableFor("TRACK", fill, { type: "TRACK", v: 2 }, undefined), false);
  assert.equal(pm.isWriteableFor("TRACK", fill, { type: "TRACK", v: 0 }, undefined), false);
});

test("a mask only reaches what is walked after it: a class reached first by another path keeps the property", () => {
  // KiCad's own dimension classes: PCB_DIMENSION_BASE inherits PCB_TEXT, BOARD_ITEM, EDA_TEXT; PCB_TEXT masks EDA_TEXT's Color. Walking the last base first reaches
  // EDA_TEXT before PCB_TEXT's mask has been seen, so a dimension shows a text colour that a text does not.
  const pm = new PropertyManager<Item, undefined, string>();
  pm.registerType("EDA_TEXT");
  pm.addProperty(def("EDA_TEXT", "Text", { kind: "string" }), "Text Properties");
  pm.addProperty(def("EDA_TEXT", "Color"), "Text Properties");
  pm.registerType("BOARD_ITEM");
  pm.addProperty(def("BOARD_ITEM", "Locked", { kind: "bool" }));
  pm.inheritsAfter("PCB_TEXT", "BOARD_ITEM");
  pm.inheritsAfter("PCB_TEXT", "EDA_TEXT");
  pm.mask("PCB_TEXT", "EDA_TEXT", "Color");
  pm.inheritsAfter("DIM", "PCB_TEXT");
  pm.inheritsAfter("DIM", "BOARD_ITEM");
  pm.inheritsAfter("DIM", "EDA_TEXT");
  pm.addProperty(def("DIM", "Prefix", { kind: "string" }), "Dimension Properties");
  assert.equal(pm.getProperty("PCB_TEXT", "Color"), undefined, "a text has no Color");
  assert.ok(pm.getProperty("DIM", "Color"), "the dimension reaches EDA_TEXT through the last-declared base before PCB_TEXT's mask is known");
});

test("of two properties of one name the one the item has is found", () => {
  const pm = new PropertyManager<Item, undefined, string>();
  pm.registerType("EDA_TEXT");
  pm.addProperty(def("EDA_TEXT", "Text", { kind: "string" }));
  pm.inheritsAfter("DIM", "EDA_TEXT");
  pm.addProperty(def("DIM", "Text", { kind: "string", available: (i) => i.v === 1 }));
  pm.overrideAvailability("DIM", "EDA_TEXT", "Text", () => false);
  const leader = { type: "DIM", v: 1 };
  const other = { type: "DIM", v: 0 };
  assert.equal(pm.getProperty("DIM", "Text", leader, undefined)?.owner, "DIM");
  assert.equal(pm.getProperty("DIM", "Text", other, undefined)?.owner, "DIM", "none is available: the first");
});

test("isOfType follows the inheritance", () => {
  const pm = manager();
  assert.equal(pm.isOfType("TRACK", "BOARD_ITEM"), true);
  assert.equal(pm.isOfType("BOARD_ITEM", "TRACK"), false);
  assert.equal(pm.isOfType("TEXT", "EDA_TEXT"), true);
});
