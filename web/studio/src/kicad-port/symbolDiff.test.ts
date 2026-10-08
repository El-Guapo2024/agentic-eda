import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, LibrarySymbol, LibSymbol } from "../api/types";
import { diffSymbols } from "./symbolDiff";

const resolvedPin = (over: Partial<LibPin> = {}): LibPin => ({ number: "1", name: null, electrical_type: "passive", shape: "line", at: [0, 3.81], angle_deg: 270, length_mm: 1.27, unit: 1, body_style: 1, hidden: false, ...over });

const resolved = (over: Partial<LibSymbol & { power: boolean }> = {}): LibSymbol & { power: boolean } => ({
  power: false,
  graphics: [{ kind: "rectangle", start: [-1.016, -2.54], end: [1.016, 2.54], stroke_width: 0.254, fill: "none", unit: 1, body_style: 1 }],
  pins: [resolvedPin(), resolvedPin({ number: "2", at: [0, -3.81], angle_deg: 90 })],
  ...over,
});

const library = (over: Partial<LibrarySymbol> = {}): LibrarySymbol => ({
  lib_id: "Device:R",
  reference_prefix: "R",
  description: "",
  keywords: "",
  datasheet: "",
  power: false,
  in_bom: true,
  on_board: true,
  pin_numbers_hidden: false,
  pin_names_hidden: false,
  pin_name_offset_mm: 0,
  unit_count: 1,
  has_alternate_body_style: false,
  footprint_filters: [],
  graphics: [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -1.016, y: -2.54 }, end: { x: 1.016, y: 2.54 }, stroke_mm: 0.254, fill: "none" }],
  pins: [
    { number: "1", name: "", electrical_type: "passive", shape: "line", at: { x: 0, y: 3.81 }, angle_deg: 270, length_mm: 1.27, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null },
    { number: "2", name: "", electrical_type: "passive", shape: "line", at: { x: 0, y: -3.81 }, angle_deg: 90, length_mm: 1.27, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null },
  ],
  published: true,
  ...over,
});

test("the same symbol on both sides has no differences", () => {
  assert.deepEqual(diffSymbols(resolved(), library()), []);
});

test("a power flag, a pin, a graphic and a unit count that differ are each reported", () => {
  assert.deepEqual(diffSymbols(resolved({ power: true }), library()), ["Power flag differs."]);
  const moved = library();
  moved.pins[0] = { ...moved.pins[0]!, length_mm: 2.54 };
  const d = diffSymbols(resolved(), moved);
  assert.equal(d.length, 1);
  assert.match(d[0]!, /^Pin 1 differs: Pin 1 \[passive, line, \(0, 3.81\) mm, length 1.27 mm\]; Pin 1 \[passive, line, \(0, 3.81\) mm, length 2.54 mm\]$/);
  const bigger = library({ graphics: [...library().graphics, { kind: "circle", unit: 1, body_style: 1, center: { x: 0, y: 0 }, radius_mm: 1, stroke_mm: 0.254, fill: "none" }] });
  assert.deepEqual(diffSymbols(resolved(), bigger), ["Graphic item count differs."]);
  const edited = library({ graphics: [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -2, y: -2.54 }, end: { x: 2, y: 2.54 }, stroke_mm: 0.254, fill: "none" }] });
  const g = diffSymbols(resolved(), edited);
  assert.equal(g.length, 1);
  assert.match(g[0]!, /^Graphic item differs: Rectangle \(-1.016, -2.54\) \(1.016, 2.54\) .*; Rectangle \(-2, -2.54\) \(2, 2.54\)/);
  assert.deepEqual(diffSymbols(resolved(), library({ unit_count: 2 })), ["Unit count differs."]);
});

test("a pin only one side has is extra or missing", () => {
  const without = library();
  without.pins = [without.pins[0]!];
  assert.match(diffSymbols(resolved(), without)[0]!, /^Extra pin in schematic symbol: Pin 2/);
  const withThird = library();
  withThird.pins = [...withThird.pins, { ...withThird.pins[0]!, number: "3" }];
  assert.match(diffSymbols(resolved(), withThird)[0]!, /^Missing pin in schematic symbol: Pin 3/);
});
