import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbol } from "../api/types";
import { withDefaults } from "./libraryDefaults";

// What `GET /api/symbol` sends for a one-unit symbol with nothing in its text fields: no `unit` / `body_style` on the pin, no `body_style` on the
// graphic, no `keywords`, `datasheet` or `footprint_filters`.
const FROM_SERVER = {
  lib_id: "Device:R",
  reference_prefix: "R",
  description: "Resistor",
  graphics: [{ kind: "rectangle", id: "g1", unit: 0, start: { x: -1, y: -2 }, end: { x: 1, y: 2 }, stroke_mm: 0.254, fill: "none" }],
  pins: [{ id: "p1", number: "1", name: "", electrical_type: "passive", shape: "line", at: { x: 0, y: 3.81 }, angle_deg: 270, length_mm: 1.27, hidden: false }],
} as unknown as LibrarySymbol;

test("a pin with no unit or body style is on unit 1, body style 1", () => {
  const s = withDefaults(FROM_SERVER);
  assert.equal(s.pins[0]!.unit, 1);
  assert.equal(s.pins[0]!.body_style, 1);
});

test("a graphic keeps its unit 0 ('every unit') and gets body style 1", () => {
  const s = withDefaults(FROM_SERVER);
  assert.equal(s.graphics[0]!.unit, 0);
  assert.equal(s.graphics[0]!.body_style, 1);
});

test("the text fields, the unit count and the footprint filters the server skipped when empty are back", () => {
  const s = withDefaults(FROM_SERVER);
  assert.equal(s.keywords, "");
  assert.equal(s.datasheet, "");
  assert.equal(s.unit_count, 1);
  assert.deepEqual(s.footprint_filters, []);
  assert.equal(s.description, "Resistor", "a field that was sent is kept");
  assert.equal(s.reference_prefix, "R");
});

test("values the server did send are kept", () => {
  const two = { ...FROM_SERVER, unit_count: 2, keywords: "r res", pins: [{ ...FROM_SERVER.pins[0]!, unit: 2, body_style: 2 }], graphics: [{ ...FROM_SERVER.graphics[0]!, unit: 3, body_style: 2 }] } as LibrarySymbol;
  const s = withDefaults(two);
  assert.equal(s.unit_count, 2);
  assert.equal(s.keywords, "r res");
  assert.equal(s.pins[0]!.unit, 2);
  assert.equal(s.pins[0]!.body_style, 2);
  assert.equal(s.graphics[0]!.unit, 3);
  assert.equal(s.graphics[0]!.body_style, 2);
});

test("a symbol with no pins or graphics list still comes back with both", () => {
  const s = withDefaults({ lib_id: "x" } as unknown as LibrarySymbol);
  assert.deepEqual(s.pins, []);
  assert.deepEqual(s.graphics, []);
});

test("the input is not changed", () => {
  withDefaults(FROM_SERVER);
  assert.equal((FROM_SERVER.pins[0] as { unit?: number }).unit, undefined);
  assert.equal((FROM_SERVER as { keywords?: string }).keywords, undefined);
});
