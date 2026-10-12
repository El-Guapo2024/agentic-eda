import { test } from "node:test";
import assert from "node:assert/strict";
import { allCategoriesOn, categoryById, DEFAULT_SCH_SELECTION_FILTER, onlyCategory, schSelectable, setAllCategories } from "./schSelectionFilter";
import type { Schematic } from "../api/types";

const sheet = (over: Record<string, unknown>): Schematic =>
  ({ symbols: [], wires: [], labels: [], texts: [], power_symbols: [], no_connects: [], bus_entries: [], sheets: [], junctions: [], lines: [], graphics: [], locked: [], ...over }) as unknown as Schematic;

const sch = sheet({
  symbols: [{ id: "R1" }, { id: "R2", lib_id: "Dev:R", at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, pins: [{ number: "1", name: null, kind: "passive" }] }],
  lib_symbols: { "Dev:R": { graphics: [], pins: [{ number: "1", name: null, electrical_type: "passive", shape: "line", at: [0, 0], angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false }] } },
  power_symbols: [{ id: "#PWR1" }],
  sheets: [{ id: "sh1", pins: [{ id: "shpin1" }] }],
  wires: [{ id: "w1" }],
  junctions: [{ id: "j1" }],
  lines: [{ id: "ln1" }],
  labels: [{ id: "lbl1" }],
  texts: [{ id: "txt1" }],
  no_connects: [{ id: "nc1" }],
  bus_entries: [{ id: "be1" }],
  graphics: [
    { id: "shp1", shape: { type: "rectangle" } },
    { id: "tb1", shape: { type: "text_box" } },
    { id: "ra1", shape: { type: "rule_area" } },
    { id: "dl1", shape: { type: "directive" } },
  ],
});

test("every item is filed under the category itemPassesFilter's switch gives it", () => {
  const c = categoryById(sch);
  const want: Record<string, string> = {
    R1: "symbols",
    "#PWR1": "symbols",
    sh1: "symbols",
    w1: "wires",
    j1: "wires",
    ln1: "graphics",
    lbl1: "labels",
    txt1: "text",
    nc1: "otherItems",
    be1: "otherItems",
    shp1: "graphics",
    tb1: "text",
    ra1: "ruleAreas",
    dl1: "otherItems",
    "pin:R2:1": "pins",
    "shpin1": "pins",
  };
  for (const [id, cat] of Object.entries(want)) assert.equal(c.get(id), cat, id);
});

test("a switched-off Pins category hides the pins of symbols and of sheets, and a pin of a locked symbol is locked with it", () => {
  const withPins = sheet({
    symbols: [{ id: "R2", lib_id: "Dev:R", at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, pins: [{ number: "1", name: null, kind: "passive" }] }],
    lib_symbols: { "Dev:R": { graphics: [], pins: [{ number: "1", name: null, electrical_type: "passive", shape: "line", at: [0, 0], angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false }] } },
    sheets: [{ id: "sh1", pins: [{ id: "shpin1" }] }],
    locked: ["R2"],
  });
  const off = schSelectable(withPins, { ...DEFAULT_SCH_SELECTION_FILTER, lockedItems: true, pins: false });
  assert.equal(off("pin:R2:1"), false);
  assert.equal(off("shpin1"), false);
  assert.equal(off("R2"), true);
  assert.equal(schSelectable(withPins, DEFAULT_SCH_SELECTION_FILTER)("pin:R2:1"), false, "locked with its symbol");
  assert.equal(schSelectable(withPins, { ...DEFAULT_SCH_SELECTION_FILTER, lockedItems: true })("pin:R2:1"), true);
});

test("a switched-off category hides exactly its items", () => {
  const pick = schSelectable(sch, { ...DEFAULT_SCH_SELECTION_FILTER, text: false });
  assert.equal(pick("txt1"), false);
  assert.equal(pick("tb1"), false, "a text box is text");
  assert.equal(pick("lbl1"), true);
  assert.equal(pick("w1"), true);
});

test("locked items are selectable only with 'Locked items' on, whatever their category", () => {
  const locked = { ...sch, locked: ["lbl1", "R1"] } as Schematic;
  const off = schSelectable(locked, DEFAULT_SCH_SELECTION_FILTER);
  assert.equal(off("lbl1"), false);
  assert.equal(off("R1"), false);
  assert.equal(off("w1"), true);
  const on = schSelectable(locked, { ...DEFAULT_SCH_SELECTION_FILTER, lockedItems: true });
  assert.equal(on("lbl1"), true);
  // ... but a locked item of a switched-off category stays out.
  assert.equal(schSelectable(locked, { ...DEFAULT_SCH_SELECTION_FILTER, lockedItems: true, labels: false })("lbl1"), false);
});

test("an id the sheet does not know passes the filter", () => {
  assert.equal(schSelectable(sch, { ...DEFAULT_SCH_SELECTION_FILTER, symbols: false })("somewhere-else"), true);
});

test("All items sweeps the categories but not 'Locked items'; Only <category> leaves one on", () => {
  const f = { ...DEFAULT_SCH_SELECTION_FILTER, lockedItems: true };
  const off = setAllCategories(f, false);
  assert.equal(allCategoriesOn(off), false);
  assert.equal(off.lockedItems, true);
  assert.equal(allCategoriesOn(setAllCategories(off, true)), true);
  const only = onlyCategory(f, "wires");
  assert.deepEqual([only.symbols, only.text, only.wires, only.labels, only.pins, only.graphics, only.ruleAreas, only.otherItems], [false, false, true, false, false, false, false, false]);
  assert.equal(only.lockedItems, true);
});

test("the defaults select everything but locked items", () => {
  assert.equal(DEFAULT_SCH_SELECTION_FILTER.lockedItems, false);
  assert.equal(allCategoriesOn(DEFAULT_SCH_SELECTION_FILTER), true);
});
