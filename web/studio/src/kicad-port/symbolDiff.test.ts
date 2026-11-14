import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin, LibrarySymbol, LibSymbol } from "../api/types";
import { diffSymbols } from "./symbolDiff";
import { DEFAULT_SYMBOL_PARITY } from "./schSetup";

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
  // a pin moved
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

test("UI-LEFT-3 B3-9 (25ec2335): a pin's name is no difference, its type, shape, direction or length is", () => {
  const renamed = library();
  renamed.pins[0] = { ...renamed.pins[0]!, name: "A" };
  assert.deepEqual(diffSymbols(resolved(), renamed), []);
  for (const over of [{ electrical_type: "input" as const }, { shape: "clock" as const }, { length_mm: 2.54 }, { angle_deg: 0 }]) {
    const lib = library();
    lib.pins[0] = { ...lib.pins[0]!, ...over };
    assert.equal(diffSymbols(resolved(), lib).length, 1, JSON.stringify(over));
  }
});

test("UI-LEFT-3 B3-9 (25ec2335): a pin only the library has is missing from the schematic's symbol", () => {
  const lib = library();
  lib.pins = [...lib.pins, { ...lib.pins[0]!, number: "3", at: { x: 5, y: 5 } }];
  assert.deepEqual(diffSymbols(resolved(), lib), ["Missing pin in schematic symbol: Pin 3 [Passive, Line]."]);
});

test("UI-LEFT-3 B3-9: a hidden pin and the alternates count by the page's boxes", () => {
  const hidden = library();
  hidden.pins[0] = { ...hidden.pins[0]!, hidden: true };
  assert.deepEqual(diffSymbols(resolved(), hidden), [], "pin visibilities are off by default");
  assert.equal(diffSymbols(resolved(), hidden, undefined, { ...DEFAULT_SYMBOL_PARITY, pin_visibilities: true }).length, 1);
  const alt = library();
  alt.pins[0] = { ...alt.pins[0]!, alternates: [{ name: "SDA", electrical_type: "bidirectional", shape: "line" }] };
  assert.equal(diffSymbols(resolved(), alt).length, 1, "alternate pin functions are compared by default");
  assert.deepEqual(diffSymbols(resolved(), alt, undefined, { ...DEFAULT_SYMBOL_PARITY, pin_alt_functions: false }), []);
  assert.deepEqual(diffSymbols(resolved(), library({ pin_names_hidden: true }), undefined, { ...DEFAULT_SYMBOL_PARITY, pin_visibilities: true }), ["Show pin names settings differ."]);
  assert.deepEqual(diffSymbols({ ...resolved(), in_bom: false }, library()), [], "exclude from BOM is off by default");
  assert.deepEqual(diffSymbols({ ...resolved(), on_board: false }, library()), ["Exclude from board settings differ."], "exclude from board is on");
});

test("r6 C6-M4: a_body_text_is_not_a_graphic_item", () => {
  const lib = library({ graphics: [...library().graphics, { kind: "text", unit: 1, body_style: 1, text: "X", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27 }] });
  assert.deepEqual(diffSymbols(resolved(), lib), []);
  const sch = resolved({ graphics: [...resolved().graphics, { kind: "text", content: "Y", at: [1, 1], angle_deg: 0, size_mm: 1.27, unit: 1, body_style: 1 }] });
  assert.deepEqual(diffSymbols(sch, library()), []);
});

test("r6 C6-M4: shapes compare within EPSILON (2 IU), and two equal shapes are one (`std::set`)", () => {
  const lib = library({ graphics: [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -1.016, y: -2.54 }, end: { x: 1.016000002, y: 2.54 }, stroke_mm: 0.254, fill: "none" }] });
  assert.deepEqual(diffSymbols(resolved(), lib), []);
  const twice = resolved({ graphics: [...resolved().graphics, ...resolved().graphics] });
  assert.deepEqual(diffSymbols(twice, library()), []);
});

test("UI-LEFT-3 B3-9: the placed fields are compared as the page's boxes say", () => {
  const at: [number, number] = [100_000_000, 50_000_000];
  const field = (name: string, text: string, dx: number, dy: number, over: Record<string, unknown> = {}) => ({ name, text, at: [at[0] + dx, at[1] + dy] as [number, number], vertical: false, h: "center" as const, v: "center" as const, visible: true, ...over });
  // the library's Reference at (0, 5.08) Y up and Value at (0, 2.54): -5.08 mm and -2.54 mm on the sheet; its Value text is "R"
  const placed = { at, fields: [field("Reference", "R1", 0, -5_080_000), field("Value", "R", 0, -2_540_000), field("MPN", "RC0603", 0, 0)] };
  assert.deepEqual(diffSymbols(resolved(), library(), placed), [], "extra fields are not checked by default, a Reference's text never");
  assert.deepEqual(diffSymbols(resolved(), library(), placed, { ...DEFAULT_SYMBOL_PARITY, extra_fields: true }), ["Extra field in schematic symbol: MPN."]);
  const tenK = { at, fields: [field("Value", "10k", 0, -2_540_000)] };
  assert.deepEqual(diffSymbols(resolved(), library(), tenK), ["Field 'Value' differs: 10k; R."], "non-reference field text is compared by default");
  assert.deepEqual(diffSymbols(resolved(), library(), tenK, { ...DEFAULT_SYMBOL_PARITY, field_texts: false }), []);
  const moved = { at, fields: [field("Value", "R", 1_270_000, -2_540_000)] };
  assert.deepEqual(diffSymbols(resolved(), library(), moved), [], "positions are off by default");
  assert.deepEqual(diffSymbols(resolved(), library(), moved, { ...DEFAULT_SYMBOL_PARITY, field_positions: true }), ["Field 'Value' positions differ."]);
  const hiddenValue = { at, fields: [field("Value", "R", 0, -2_540_000, { visible: false })] };
  assert.deepEqual(diffSymbols(resolved(), library(), hiddenValue, { ...DEFAULT_SYMBOL_PARITY, field_visibilities: true }), ["Field 'Value' visibility flags differ."]);
  const big = { at, fields: [field("Value", "R", 0, -2_540_000, { size_nm: 2_000_000, bold: true })] };
  assert.deepEqual(diffSymbols(resolved(), library(), big, { ...DEFAULT_SYMBOL_PARITY, field_styles: true }), ["Field 'Value' text sizes differ.", "Field 'Value' text styles differ."]);
  // a library user field the placed symbol lacks is missing (checked by default)
  const libWithUser = library({ fields: [{ name: "Sim.Device", text: "R" }] });
  assert.deepEqual(diffSymbols(resolved(), libWithUser, { at, fields: [] }), ["Missing field in schematic symbol: Sim.Device."]);
  assert.deepEqual(diffSymbols(resolved(), libWithUser, { at, fields: [] }, { ...DEFAULT_SYMBOL_PARITY, missing_fields: false }), []);
});

test("r6 C6-M4: a local power symbol against a global one differs (`m_options`)", () => {
  assert.deepEqual(diffSymbols(resolved({ power: true, power_local: true } as never), library({ power: true })), ["Power flag differs."]);
  assert.deepEqual(diffSymbols(resolved({ power: true, power_local: true } as never), library({ power: true, power_local: true })), []);
  // a schematic side that does not say is taken as the library's kind
  assert.deepEqual(diffSymbols(resolved({ power: true }), library({ power: true, power_local: true })), []);
});

test("r7 A7-L1: shapes_150_nm_apart_are_the_same (EPSILON is 2 IU of 100 nm), a pin 50 nm away is not moved", () => {
  const lib = library({ graphics: [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -1.016, y: -2.54 }, end: { x: 1.01615, y: 2.54 }, stroke_mm: 0.254, fill: "none" }] });
  assert.deepEqual(diffSymbols(resolved(), lib), []);
  const far = library({ graphics: [{ kind: "rectangle", unit: 1, body_style: 1, start: { x: -1.016, y: -2.54 }, end: { x: 1.0163, y: 2.54 }, stroke_mm: 0.254, fill: "none" }] });
  assert.equal(diffSymbols(resolved(), far).length, 1, "300 nm is past it");
  const nudged = library();
  nudged.pins[0] = { ...nudged.pins[0]!, at: { x: 0.00004, y: 3.81 } };
  assert.deepEqual(diffSymbols(resolved(), nudged), []);
});

test("r7 A7-L1: two_polylines_with_one_first_point_pair_by_vertex_count (a POLY's start and end are (0, 0))", () => {
  const poly = (pts: [number, number][]) => ({ kind: "polyline" as const, pts, stroke_width: 0.254, fill: "none" as const, unit: 1, body_style: 1 });
  const lpoly = (pts: [number, number][]) => ({ kind: "polyline" as const, unit: 1, body_style: 1, pts: pts.map(([x, y]) => ({ x, y })), stroke_mm: 0.254, fill: "none" as const });
  // same first point; a 2-point line ending far right and a 3-point line ending far left. By the last point the 3-point one sorts first; by KiCad's
  // vertex count the 2-point one does. With the library's 3-point line moved, only that pair is reported.
  const sch = resolved({ graphics: [poly([[0, 0], [10, 0]]), poly([[0, 0], [1, 1], [-10, 0]])] });
  const lib = library({ graphics: [lpoly([[0, 0], [10, 0]]), lpoly([[0, 0], [1, 2], [-10, 0]])] });
  assert.deepEqual(diffSymbols(sch, lib), ["Graphic item differs: Polyline, 3 points; Polyline, 3 points."]);
});
