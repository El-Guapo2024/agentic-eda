import { test } from "node:test";
import assert from "node:assert/strict";
import { collectBoxSelection } from "./boxSelection";
import type { LibSymbols, Schematic, SchematicSymbol, SchematicWire } from "../../api/types";

// Same minimal "Device:R" fixture wireAttachment.test.ts/ercMarkerPosition.test.ts already use.
const R_LIB: LibSymbols = {
  "Device:R": {
    graphics: [],
    pins: [
      { number: "1", name: null, electrical_type: "passive", shape: "line", at: [0, 2.54], angle_deg: 270, length_mm: 2.54, unit: 1, body_style: 1, hidden: false },
      { number: "2", name: null, electrical_type: "passive", shape: "line", at: [0, -2.54], angle_deg: 90, length_mm: 2.54, unit: 1, body_style: 1, hidden: false },
    ],
  },
};

function resistor(id: string, at: [number, number]): SchematicSymbol {
  return { id, lib_id: "Device:R", at, rot: 0, mirror: null, unit: 1, body_style: 1, value: "10k", mpn: null, package: null, footprint: null, datasheet: null, pins: [] };
}

function wire(id: string, pts: [number, number][]): SchematicWire {
  return { id, net: "N", pins: [], pts, bus: false };
}

function schematic(symbols: SchematicSymbol[], wires: SchematicWire[]): Schematic {
  return { lib_symbols: R_LIB, symbols, power_symbols: [], wires, no_connects: [], labels: [], texts: [], title_block: null, bus_entries: [], sheets: [], sheet_path: [] };
}

test("collectBoxSelection: a symbol fully inside the box is collected either way (crossing or enclosed)", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const sch = schematic([r1], []);
  const box: [number, number, number, number] = [0, 0, 20_000, 20_000];
  assert.deepEqual(collectBoxSelection(sch, box, true), ["R1"]);
  assert.deepEqual(collectBoxSelection(sch, box, false), ["R1"]);
});

test("collectBoxSelection: a wire fully inside the box is collected, same as a symbol", () => {
  const w1 = wire("wire_1", [
    [1_000, 1_000],
    [5_000, 1_000],
  ]);
  const sch = schematic([], [w1]);
  const box: [number, number, number, number] = [0, 0, 10_000, 10_000];
  assert.deepEqual(collectBoxSelection(sch, box, true), ["wire_1"]);
  assert.deepEqual(collectBoxSelection(sch, box, false), ["wire_1"]);
});

test("collectBoxSelection: a wire only partly inside the box is collected when crossing, not when enclosed", () => {
  const w1 = wire("wire_1", [
    [-5_000, 0],
    [5_000, 0],
  ]);
  const sch = schematic([], [w1]);
  const box: [number, number, number, number] = [0, -1_000, 10_000, 1_000];
  assert.deepEqual(collectBoxSelection(sch, box, true), ["wire_1"]);
  assert.deepEqual(collectBoxSelection(sch, box, false), []);
});

test("collectBoxSelection: a wire entirely outside the box is never collected", () => {
  const w1 = wire("wire_1", [
    [100_000, 100_000],
    [200_000, 200_000],
  ]);
  const sch = schematic([], [w1]);
  const box: [number, number, number, number] = [0, 0, 10_000, 10_000];
  assert.deepEqual(collectBoxSelection(sch, box, true), []);
  assert.deepEqual(collectBoxSelection(sch, box, false), []);
});

test("collectBoxSelection: a multi-bend wire's bounds cover every point, not just its endpoints", () => {
  // The bend at (0, 9000) sticks well above the start/end points -- a
  // bounds computation that only looked at pts[0]/pts[last] would miss a
  // selection box drawn around just that bend.
  const w1 = wire("wire_1", [
    [0, 0],
    [0, 9_000],
    [5_000, 9_000],
  ]);
  const sch = schematic([], [w1]);
  const box: [number, number, number, number] = [-1_000, 8_000, 1_000, 10_000];
  assert.deepEqual(collectBoxSelection(sch, box, true), ["wire_1"]);
});

test("collectBoxSelection: symbols and wires both collected together, in sch.symbols-then-sch.wires order", () => {
  const r1 = resistor("R1", [1_000, 1_000]);
  const w1 = wire("wire_1", [
    [5_000, 5_000],
    [6_000, 5_000],
  ]);
  const sch = schematic([r1], [w1]);
  const box: [number, number, number, number] = [0, 0, 10_000, 10_000];
  assert.deepEqual(collectBoxSelection(sch, box, true), ["R1", "wire_1"]);
});

test("collectBoxSelection: a zero-length wire (a single point, e.g. mid-draw) contributes a degenerate box rather than being skipped or crashing", () => {
  const w1 = wire("wire_1", [[5_000, 5_000]]);
  const sch = schematic([], [w1]);
  assert.deepEqual(collectBoxSelection(sch, [0, 0, 10_000, 10_000], true), ["wire_1"]);
  assert.deepEqual(collectBoxSelection(sch, [6_000, 6_000, 10_000, 10_000], true), []);
});
