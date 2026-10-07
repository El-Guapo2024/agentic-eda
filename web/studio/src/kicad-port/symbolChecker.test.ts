import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibrarySymbol, LibrarySymbolPin } from "../api/types";
import { checkLibSymbol, messageText, MIN_PIN_GRID_IU } from "./symbolChecker";

const mm = (v: number) => `${v} mm`;
const GRID_50_MIL = 1_270_000;

function pin(over: Partial<LibrarySymbolPin> = {}): LibrarySymbolPin {
  return { number: "1", name: "", electrical_type: "passive", shape: "line", at: { x: 0, y: 3.81 }, angle_deg: 270, length_mm: 1.27, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null, ...over };
}

function sym(over: Partial<LibrarySymbol> = {}): LibrarySymbol {
  return {
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
    graphics: [],
    pins: [pin({ number: "1" }), pin({ number: "2", at: { x: 0, y: -3.81 }, angle_deg: 90 })],
    published: false,
    ...over,
  };
}

const texts = (s: LibrarySymbol, grid = GRID_50_MIL) => checkLibSymbol(s, grid, mm).map(messageText);

test("a clean resistor has nothing to report", () => {
  assert.deepEqual(texts(sym()), []);
});

test("an empty reference prefix, and one that ends in a digit or '?', are warned about", () => {
  assert.deepEqual(texts(sym({ reference_prefix: "" })), ["Warning: reference is empty"]);
  assert.deepEqual(texts(sym({ reference_prefix: "R?" })), ["Warning: reference prefix\nprefix ending by '0123456789?' can create issues if saved in a symbol library"]);
  assert.deepEqual(texts(sym({ reference_prefix: "R2" })).length, 1);
});

test("two pins with one number in the same unit and body style are duplicates, worded with their locations and names", () => {
  const s = sym({ pins: [pin({ number: "1", name: "A", at: { x: 0, y: 2.54 } }), pin({ number: "1", name: "B", at: { x: 0, y: -2.54 } })] });
  const [m] = texts(s);
  assert.equal(m, "Duplicate pin 1  'B' at location (0 mm, -2.54 mm) conflicts with pin 1 'A' at location (0 mm, 2.54 mm) in units A and A.");
  assert.equal(texts(s).length, 1);
});

test("one pin number in two units is a duplicate (a package has each pin once), in two body styles it is not; a unit 0 (common) pin drops the units from the message", () => {
  const other = sym({ unit_count: 2, pins: [pin({ number: "1", unit: 1 }), pin({ number: "1", unit: 2 })] });
  assert.deepEqual(texts(other), ["Duplicate pin 1  at location (0 mm, 3.81 mm) conflicts with pin 1 at location (0 mm, 3.81 mm) in units B and A."]);
  const common = sym({ unit_count: 2, pins: [pin({ number: "1", unit: 0 }), pin({ number: "1", unit: 2, at: { x: 2.54, y: 0 } })] });
  const [m] = texts(common);
  assert.match(m!, /conflicts with pin 1 at location \(0 mm, 3.81 mm\)\.$/);
  const styles = sym({ has_alternate_body_style: true, pins: [pin({ number: "1", body_style: 1 }), pin({ number: "1", body_style: 2 })] });
  assert.deepEqual(texts(styles), []);
});

test("a stacked pin number counts every pin it stands for", () => {
  const s = sym({ pins: [pin({ number: "[1-3]" }), pin({ number: "2", at: { x: 2.54, y: 0 } })] });
  const msgs = texts(s);
  assert.equal(msgs.length, 1);
  assert.match(msgs[0]!, /^Duplicate pin 2/);
});

test("a pin that is off the grid is reported once with its location, by unit when there are several", () => {
  const s = sym({ unit_count: 2, pins: [pin({ number: "1", unit: 2, at: { x: 0.5, y: 0 } })] });
  assert.deepEqual(texts(s), ["Off grid pin 1  at location (0.5 mm, 0 mm) in unit B."]);
  const named = sym({ pins: [pin({ number: "1", name: "IN", at: { x: 1.27, y: 0.635 } })] });
  assert.deepEqual(texts(named), ["Off grid pin 1 'IN' at location (1.27 mm, 0.635 mm)."], "0.635 mm is half of a 50 mil grid");
  // a coarser grid than 25 mil is kept; a finer one is raised to 25 mil
  assert.deepEqual(texts(sym({ pins: [pin({ at: { x: 0.635, y: 0 } })] }), 1), []);
  assert.equal(MIN_PIN_GRID_IU, 635_000);
});

test("a symbol with an alternate body style words the location by body style", () => {
  const s = sym({ has_alternate_body_style: true, pins: [pin({ number: "1", body_style: 2, at: { x: 0.1, y: 0 } })] });
  assert.deepEqual(texts(s), ["Off grid pin 1  at location (0.1 mm, 0 mm) of alternate body style."]);
});

test("a power symbol needs one unit, one pin, and a power pin", () => {
  const good = sym({ power: true, reference_prefix: "#PWR", pins: [pin({ electrical_type: "power_in", hidden: false })] });
  assert.deepEqual(texts(good), []);
  const bad = sym({ power: true, reference_prefix: "#PWR", unit_count: 2, pins: [pin({ electrical_type: "passive" }), pin({ number: "2", electrical_type: "passive" })] });
  assert.deepEqual(texts(bad), ["A Power Symbol should have only one unit", "A Power Symbol should have only one pin", "Suspicious Power Symbol\nOnly an input or output power pin has meaning"]);
  const hidden = sym({ power: true, reference_prefix: "#PWR", pins: [pin({ electrical_type: "power_in", hidden: true })] });
  assert.deepEqual(texts(hidden), ["Suspicious Power Symbol\nInvisible input power pins are no longer required"]);
});

test("a hidden power input pin on an ordinary symbol is an info line that names what it will drive", () => {
  const s = sym({ pins: [pin({ number: "8", name: "VCC", electrical_type: "power_in", hidden: true })] });
  assert.deepEqual(texts(s), ["Info: Hidden power pin 8 'VCC' at location (0 mm, 3.81 mm).\n(Hidden power pins will drive their pin names on to any connected nets.)"]);
});

test("a circle without a radius and a rectangle of one point are reported with where they are", () => {
  const s = sym({
    graphics: [
      { kind: "circle", unit: 0, body_style: 0, center: { x: 1, y: 2 }, radius_mm: 0, stroke_mm: 0.254, fill: "none" },
      { kind: "rectangle", unit: 0, body_style: 0, start: { x: 3, y: 4 }, end: { x: 3, y: 4 }, stroke_mm: 0.254, fill: "none" },
      { kind: "rectangle", unit: 0, body_style: 0, start: { x: 0, y: 0 }, end: { x: 1, y: 0 }, stroke_mm: 0.254, fill: "none" },
    ],
  });
  assert.deepEqual(texts(s), ["Graphic circle has radius = 0 at location (1 mm, 2 mm).", "Graphic rectangle has size 0 at location (3 mm, 4 mm)."]);
});
