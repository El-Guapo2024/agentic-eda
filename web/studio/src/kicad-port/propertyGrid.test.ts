import { test } from "node:test";
import assert from "node:assert/strict";
import { PropertyManager, type Choice, type PropItem, type PropertyDef } from "./propertyManager";
import { buildGrid, editText, extractValueAndWritability, formatValue, parseValue, planEdit, positiveInt, positiveRatio, rangeInt } from "./propertyGrid";

interface Item extends PropItem {
  id: string;
  x?: number;
  y?: number;
  layer?: string;
  locked?: boolean;
  net?: string;
  freeze?: boolean;
}
type Ctx = { layers: Choice[] };
type Def = PropertyDef<Item, Ctx, string>;

const def = (owner: string, name: string, over: Partial<Def>): Def => ({ owner, name, kind: "int", get: () => 0, ...over });

const copper: Choice[] = [
  { label: "F.Cu", value: "F.Cu" },
  { label: "B.Cu", value: "B.Cu" },
];
const all: Choice[] = [...copper, { label: "F.SilkS", value: "F.SilkS" }];

function manager(): PropertyManager<Item, Ctx, string> {
  const pm = new PropertyManager<Item, Ctx, string>();
  pm.registerType("BOARD_ITEM");
  pm.addProperty(def("BOARD_ITEM", "Position X", { display: "coord", get: (i) => i.x ?? 0, set: (i, v) => (i.x === v ? [] : [`x ${i.id} ${v}`]) }));
  pm.addProperty(def("BOARD_ITEM", "Position Y", { display: "coord", get: (i) => i.y ?? 0, set: (i, v) => [`y ${i.id} ${v}`] }));
  pm.addProperty(def("BOARD_ITEM", "Layer", { kind: "enum", choices: (_i, c) => c.layers, get: (i) => i.layer ?? "", set: (i, v) => [`layer ${i.id} ${v}`] }));
  pm.addProperty(def("BOARD_ITEM", "Locked", { kind: "bool", get: (i) => !!i.locked, set: (i, v) => [`lock ${i.id} ${v}`] }));
  pm.inheritsAfter("TRACK", "BOARD_ITEM");
  pm.addProperty(def("TRACK", "Width", { display: "size", get: () => 200, set: (i, v) => [`width ${i.id} ${v}`], validate: (v) => positiveInt(v) }));
  pm.addProperty(def("TRACK", "Net", { kind: "net", get: (i) => i.net ?? "", set: (i, v) => [`net ${i.id} ${v}`] }), "Electrical");
  pm.inheritsAfter("TEXT", "BOARD_ITEM");
  pm.addProperty(def("TEXT", "Text", { kind: "string", get: () => "hello", set: (i, v) => [`text ${i.id} ${v}`] }), "Text Properties");
  pm.addProperty(def("TEXT", "Parent", { kind: "string", get: () => "p", hidden: true }));
  pm.addProperty(def("TEXT", "Frozen", { kind: "bool", get: (i) => !!i.freeze, set: (i, v) => [`f ${i.id} ${v}`], available: (i) => !!i.freeze }));
  return pm;
}

const ctx: Ctx = { layers: all };
const track = (id: string, over: Partial<Item> = {}): Item => ({ type: "TRACK", id, x: 1, y: 2, layer: "F.Cu", ...over });
const text = (id: string, over: Partial<Item> = {}): Item => ({ type: "TEXT", id, x: 1, y: 2, layer: "F.SilkS", ...over });
const friendly = (i: Item): string => (i.type === "TRACK" ? "Track" : "Text");

const rowNames = (g: ReturnType<typeof buildGrid>): string[] => g.groups.flatMap((x) => x.rows.map((r) => r.name));

test("nothing selected: the caption says so and there is no group", () => {
  const g = buildGrid(manager(), [], ctx, friendly);
  assert.deepEqual(g, { caption: "No objects selected", count: 0, groups: [] });
});

test("one item: its friendly name, its groups in order, the first group is Basic Properties", () => {
  const g = buildGrid(manager(), [track("t1")], ctx, friendly);
  assert.equal(g.caption, "Track");
  assert.deepEqual(
    g.groups.map((x) => [x.caption, x.rows.map((r) => r.name)]),
    [
      ["Basic Properties", ["Position X", "Position Y", "Layer", "Locked", "Width"]],
      ["Electrical", ["Net"]],
    ]
  );
});

test("several items: the caption counts them, a value they share is shown and a value they differ in is unspecified", () => {
  const g = buildGrid(manager(), [track("a", { x: 5 }), track("b", { x: 5, y: 9 })], ctx, friendly);
  assert.equal(g.caption, "2 objects selected");
  const rows = new Map(g.groups.flatMap((x) => x.rows).map((r) => [r.name, r]));
  assert.equal(rows.get("Position X")!.value, 5, "the same X");
  assert.equal(rows.get("Position Y")!.value, null, "different Y: <...>");
  assert.equal(rows.get("Layer")!.value, "F.Cu");
});

test("the third item can still make the value unspecified, and the first two differing stays so", () => {
  const g = buildGrid(manager(), [track("a", { x: 1 }), track("b", { x: 2 }), track("c", { x: 1 })], ctx, friendly);
  assert.equal(g.groups[0]!.rows.find((r) => r.name === "Position X")!.value, null);
});

test("a property is writeable only if it is for every item; a property with no setter is read-only", () => {
  const pm = manager();
  pm.addProperty(def("BOARD_ITEM", "Tag", { kind: "string", get: () => "t" }));
  pm.addProperty(def("BOARD_ITEM", "Frozen", { kind: "bool", get: (i) => !!i.freeze, set: (i, v) => [`f ${i.id} ${v}`], writeable: (i) => !i.freeze }));
  const one = buildGrid(pm, [track("a")], ctx, friendly).groups[0]!.rows;
  assert.equal(one.find((r) => r.name === "Tag")!.writable, false);
  assert.equal(one.find((r) => r.name === "Frozen")!.writable, true);
  const two = buildGrid(pm, [track("a"), track("b", { freeze: true })], ctx, friendly).groups[0]!.rows;
  assert.equal(two.find((r) => r.name === "Frozen")!.writable, false, "one frozen item freezes the row for the selection");
});

test("items of different classes show the properties every class has, under the groups of the first class then the others'", () => {
  const g = buildGrid(manager(), [track("a"), text("b")], ctx, friendly);
  assert.deepEqual(rowNames(g), ["Position X", "Position Y", "Layer", "Locked"], "Width, Net and Text belong to one class only");
  const g2 = buildGrid(manager(), [text("b"), track("a")], ctx, friendly);
  assert.deepEqual(rowNames(g2), ["Position X", "Position Y", "Layer", "Locked"]);
});

test("a property hidden from the properties manager, or not available for one of the items, is left out", () => {
  const g = buildGrid(manager(), [text("a")], ctx, friendly);
  assert.ok(!rowNames(g).includes("Parent"), "hidden");
  assert.ok(!rowNames(g).includes("Frozen"), "not available for this item");
  const g2 = buildGrid(manager(), [text("a", { freeze: true })], ctx, friendly);
  assert.ok(rowNames(g2).includes("Frozen"), "available for this one");
  const g3 = buildGrid(manager(), [text("a", { freeze: true }), text("b")], ctx, friendly);
  assert.ok(!rowNames(g3).includes("Frozen"), "one item lacking it takes it from the whole selection");
});

test("items that offer different choices for a property do not share it", () => {
  const pm = manager();
  // A track's layer is a copper layer, a text's any layer, as BOARD_CONNECTED_ITEM's Layer is next to BOARD_ITEM's.
  pm.addProperty(def("TRACK", "Side", { kind: "enum", choices: () => copper, get: () => "F.Cu" }));
  pm.addProperty(def("TEXT", "Side", { kind: "enum", choices: () => all, get: () => "F.Cu" }));
  assert.ok(rowNames(buildGrid(pm, [track("a")], ctx, friendly)).includes("Side"));
  assert.ok(rowNames(buildGrid(pm, [track("a"), track("b")], ctx, friendly)).includes("Side"), "same choices");
  assert.ok(!rowNames(buildGrid(pm, [track("a"), text("b")], ctx, friendly)).includes("Side"), "different choices");
});

test("extractValueAndWritability says null for a property a class lacks", () => {
  assert.equal(extractValueAndWritability(manager(), [track("a"), text("b")], "Width", ctx), null);
  assert.equal(extractValueAndWritability(manager(), [track("a"), text("b")], "Locked", ctx)!.value, false);
});

test("planEdit sets the property on every item, in selection order, and skips what the item already has", () => {
  const plan = planEdit(manager(), [track("a", { x: 1 }), track("b", { x: 7 }), track("c", { x: 1 })], "Position X", 7, ctx);
  assert.equal(plan.ok, true);
  assert.deepEqual(plan.ok && plan.cmds, ["x a 7", "x c 7"], "b already has 7: its setter returns nothing");
});

test("planEdit refuses with the property name and the message when a validator does, and sends nothing", () => {
  const plan = planEdit(manager(), [track("a"), track("b")], "Width", -5, ctx);
  assert.equal(plan.ok, false);
  assert.match(!plan.ok ? plan.error : "", /^Width: Value must be greater than or equal to 0 mm$/);
});

test("planEdit leaves a read-only property alone", () => {
  const plan = planEdit(manager(), [text("a")], "Parent", "x", ctx);
  assert.deepEqual(plan, { ok: true, cmds: [] });
  assert.deepEqual(planEdit(manager(), [], "Width", 1, ctx), { ok: false, error: "Nothing is selected." });
});

test("a one-item edit and a three-item edit differ only in how many commands they carry", () => {
  const pm = manager();
  const one = planEdit(pm, [track("a")], "Layer", "B.Cu", ctx);
  const three = planEdit(pm, [track("a"), track("b"), track("c")], "Layer", "B.Cu", ctx);
  assert.deepEqual(one.ok && one.cmds, ["layer a B.Cu"]);
  assert.deepEqual(three.ok && three.cmds, ["layer a B.Cu", "layer b B.Cu", "layer c B.Cu"]);
});

// ------------------------------------------------------------------------------------------------------------------------------- cells

const size = { kind: "int", display: "size", choices: null } as const;

test("lengths show the number and the display unit; angles the degree sign; choices their label", () => {
  assert.equal(formatValue(size, 1500, "mm"), "1.5 mm");
  assert.equal(formatValue(size, 25400, "mil"), "1000 mil");
  assert.equal(formatValue(size, 25400, "in"), "1 in");
  assert.equal(formatValue({ kind: "double", display: "degree", choices: null }, 90, "mm"), "90°");
  assert.equal(formatValue({ kind: "double", display: "degree", choices: null }, 12.34567, "mm"), "12.3457°");
  assert.equal(formatValue({ kind: "enum", display: "default", choices: copper }, "B.Cu", "mm"), "B.Cu");
  assert.equal(formatValue({ kind: "enum", display: "default", choices: [{ label: "Solid fill", value: 0 }, { label: "Hatch pattern", value: 1 }] }, 1, "mm"), "Hatch pattern");
  assert.equal(formatValue({ kind: "int", display: "area", choices: null }, 10_000_000, "mm"), "10 mm²");
  assert.equal(formatValue({ kind: "bool", display: "default", choices: null }, true, "mm"), "true");
});

test("editing starts from the number without its unit", () => {
  assert.equal(editText(size, 1500, "mm"), "1.5");
  assert.equal(editText({ kind: "double", display: "degree", choices: null }, 90, "mm"), "90");
  assert.equal(editText({ kind: "string", display: "default", choices: null }, "R1", "mm"), "R1");
});

test("a length typed without a unit is in the display unit; a unit suffix wins", () => {
  assert.deepEqual(parseValue(size, "1.5", "mm"), { ok: true, value: 1500 });
  assert.deepEqual(parseValue(size, "10", "mil"), { ok: true, value: 254 });
  assert.deepEqual(parseValue(size, "0.5 in", "mm"), { ok: true, value: 12700 });
  assert.deepEqual(parseValue(size, "2mm", "mil"), { ok: true, value: 2000 });
  assert.deepEqual(parseValue(size, "1,25", "mm"), { ok: true, value: 1250 }, "a comma is a decimal point");
  assert.deepEqual(parseValue(size, "-3", "mm"), { ok: true, value: -3000 });
  const bad = parseValue(size, "abc", "mm");
  assert.equal(bad.ok, false);
  assert.match(!bad.ok ? bad.error : "", /is not a length/);
});

test("angles take a degree sign, whole numbers must be whole, a string is taken as typed", () => {
  const angle = { kind: "double", display: "degree" } as const;
  assert.deepEqual(parseValue(angle, "45°", "mm"), { ok: true, value: 45 });
  assert.deepEqual(parseValue(angle, "-90.5", "mm"), { ok: true, value: -90.5 });
  assert.equal(parseValue({ kind: "int", display: "default" }, "3.5", "mm").ok, false);
  assert.deepEqual(parseValue({ kind: "int", display: "default" }, "7", "mm"), { ok: true, value: 7 });
  assert.deepEqual(parseValue({ kind: "string", display: "default" }, "  keep spaces ", "mm"), { ok: true, value: "  keep spaces " });
  assert.deepEqual(parseValue({ kind: "double", display: "ratio" }, "0.25", "mm"), { ok: true, value: 0.25 });
});

test("the validators use KiCad's wording", () => {
  assert.equal(positiveInt(-1, "size", "mm"), "Value must be greater than or equal to 0 mm");
  assert.equal(positiveInt(0), null);
  assert.equal(rangeInt(250, 5000)(100), "Value must be greater than or equal to 0.25 mm");
  assert.equal(rangeInt(250, 5000)(9000), "Value must be less than or equal to 5 mm");
  assert.equal(rangeInt(250, 5000)(300), null);
  assert.equal(positiveRatio(1.5), "Value must be less than or equal to 1");
  assert.equal(positiveRatio(-0.1), "Value must be greater than or equal to 0");
  assert.equal(positiveRatio(0.5), null);
});
