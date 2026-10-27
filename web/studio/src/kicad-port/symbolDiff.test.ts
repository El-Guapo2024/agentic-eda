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
  // a pin is compared by its place only (`SCH_ITEM::compare` under ERC)
  const moved = library();
  moved.pins[0] = { ...moved.pins[0]!, at: { x: 0, y: 5.08 } };
  assert.deepEqual(diffSymbols(resolved(), moved), ["Pin 1 differs: Pin 1 [Passive, Line]; Pin 1 [Passive, Line]"]);
  const bigger = library({ graphics: [...library().graphics, { kind: "circle", unit: 1, body_style: 1, center: { x: 0, y: 0 }, radius_mm: 1, stroke_mm: 0.254, fill: "none" }] });
  assert.deepEqual(diffSymbols(resolved(), bigger), ["Graphic item count differs."]);
  const edited = library({ graphics: [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -2, y: -2.54 }, end: { x: 2, y: 2.54 }, stroke_mm: 0.254, fill: "none" }] });
  assert.deepEqual(diffSymbols(resolved(), edited), ["Graphic item differs: Rectangle, width 2.0320 mm height 5.0800 mm; Rectangle, width 4.0000 mm height 5.0800 mm."]);
  assert.deepEqual(diffSymbols(resolved(), library({ unit_count: 2 })), ["Unit count differs."]);
});

test("a pin only the schematic has is extra", () => {
  const without = library();
  without.pins = [without.pins[0]!];
  assert.deepEqual(diffSymbols(resolved(), without), ["Extra pin in schematic symbol: Pin 2 [Passive, Line]."]);
});

test("r6 C6-M4: a_pin_renamed_in_the_library_is_not_a_difference (nor its type, shape, direction or length)", () => {
  const lib = library();
  lib.pins[0] = { ...lib.pins[0]!, name: "A", electrical_type: "input", shape: "clock", length_mm: 2.54, angle_deg: 0 };
  assert.deepEqual(diffSymbols(resolved(), lib), []);
});

test("r6 C6-M4: a_pin_only_the_library_has_is_not_reported (KiCad looks it up in the library itself)", () => {
  const lib = library();
  lib.pins = [...lib.pins, { ...lib.pins[0]!, number: "3", at: { x: 5, y: 5 } }];
  assert.deepEqual(diffSymbols(resolved(), lib), []);
});

test("r6 C6-M4: a_body_text_is_not_a_graphic_item", () => {
  const lib = library({ graphics: [...library().graphics, { kind: "text", unit: 1, body_style: 1, text: "X", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27 }] });
  assert.deepEqual(diffSymbols(resolved(), lib), []);
  const sch = resolved({ graphics: [...resolved().graphics, { kind: "text", content: "Y", at: [1, 1], angle_deg: 0, size_mm: 1.27, unit: 1, body_style: 1 }] });
  assert.deepEqual(diffSymbols(sch, library()), []);
});

test("r6 C6-M4: shapes compare within EPSILON 2 nm, and two equal shapes are one (`std::set`)", () => {
  const lib = library({ graphics: [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -1.016, y: -2.54 }, end: { x: 1.016000002, y: 2.54 }, stroke_mm: 0.254, fill: "none" }] });
  assert.deepEqual(diffSymbols(resolved(), lib), []);
  const twice = resolved({ graphics: [...resolved().graphics, ...resolved().graphics] });
  assert.deepEqual(diffSymbols(twice, library()), []);
});

test("r6 C6-M4: a_user_field_the_library_lacks_is_an_extra_field; a moved field differs; a library field the placed symbol lacks is not reported", () => {
  const at: [number, number] = [100_000_000, 50_000_000];
  const field = (name: string, text: string, dx: number, dy: number) => ({ name, text, at: [at[0] + dx, at[1] + dy] as [number, number], vertical: false, h: "center" as const, v: "center" as const, visible: true });
  // the library's Reference at (0, 5.08) Y up and Value at (0, 2.54): -5.08 mm and -2.54 mm on the sheet
  const placed = { at, fields: [field("Reference", "R1", 0, -5_080_000), field("Value", "10k", 0, -2_540_000), field("MPN", "RC0603", 0, 0)] };
  assert.deepEqual(diffSymbols(resolved(), library(), placed), ["Extra field in schematic symbol: Field MPN 'RC0603'."]);
  const movedValue = { at, fields: [field("Value", "10k", 1_270_000, -2_540_000)] };
  assert.deepEqual(diffSymbols(resolved(), library(), movedValue), ["Field 'Value' differs: Field Value '10k'; Field Value 'R'."]);
  const libWithUser = library({ fields: [{ name: "Sim.Device", text: "R" }] });
  assert.deepEqual(diffSymbols(resolved(), libWithUser, { at, fields: [] }), []);
});

test("r6 C6-M4: a local power symbol against a global one differs (`m_options`)", () => {
  assert.deepEqual(diffSymbols(resolved({ power: true, power_local: true } as never), library({ power: true })), ["Power flag differs."]);
  assert.deepEqual(diffSymbols(resolved({ power: true, power_local: true } as never), library({ power: true, power_local: true })), []);
  // a schematic side that does not say is taken as the library's kind
  assert.deepEqual(diffSymbols(resolved({ power: true }), library({ power: true, power_local: true })), []);
});
