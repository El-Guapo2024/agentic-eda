import { test } from "node:test";
import assert from "node:assert/strict";
import { attachedWireEndpoints, computeDragAttachment } from "./wireAttachment";
import { resolveLibSymbol } from "./libSymbol";
import type { LibSymbols, Schematic, SchematicSymbol, SchematicWire } from "../../api/types";

// A minimal two-pin "Device:R"-shaped library symbol -- real pin
// geometry (not a generic-box fallback), so resolveLibSymbol actually
// resolves world-space pin tips for attachedWireEndpoints to match
// against. The exact tip coordinates aren't hand-derived here (that's
// transform.ts/libSymbol.ts's own tested concern) -- each test asks
// resolveLibSymbol for the real tip and builds its wire fixture around
// that value, so what's under test is purely "does a wire endpoint that
// coincides with a resolved pin get collected", not the transform math.
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
  return { id, net: "", pins: [], pts };
}

function schematic(symbols: SchematicSymbol[], wires: SchematicWire[]): Schematic {
  return { lib_symbols: R_LIB, symbols, power_symbols: [], wires, no_connects: [], labels: [], title_block: null };
}

test("attachedWireEndpoints: a wire landing exactly on a pin tip is attached at that endpoint", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const tip0 = resolveLibSymbol(r1, R_LIB)!.pins[0]!.tip;
  const sch = schematic([r1], [wire("w1", [[0, 0], tip0])]);
  assert.deepEqual(attachedWireEndpoints(sch, "R1"), [[0, 1]], "wire index 0, point index 1 (the last point) sits on the pin");
});

test("attachedWireEndpoints: a wire's start point can be the attached end too", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const tip1 = resolveLibSymbol(r1, R_LIB)!.pins[1]!.tip;
  const sch = schematic([r1], [wire("w1", [tip1, [40_000, 40_000]])]);
  assert.deepEqual(attachedWireEndpoints(sch, "R1"), [[0, 0]]);
});

test("attachedWireEndpoints: every wire landing on the same pin (a T/+ junction) is attached, not just one -- this app's substitute for sch_move_tool.cpp's ptHasUnselectedJunction stub (see this module's own doc)", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const tip0 = resolveLibSymbol(r1, R_LIB)!.pins[0]!.tip;
  const sch = schematic([r1], [wire("wa", [[0, 0], tip0]), wire("wb", [tip0, [20_000, 20_000]])]);
  const pairs = attachedWireEndpoints(sch, "R1");
  assert.equal(pairs.length, 2);
  assert.ok(pairs.some(([wi, pi]) => wi === 0 && pi === 1), "wa's endpoint");
  assert.ok(pairs.some(([wi, pi]) => wi === 1 && pi === 0), "wb's start point");
});

test("attachedWireEndpoints: an interior bend coinciding with a pin is not attached -- only the wire's own two true ends count", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const tip0 = resolveLibSymbol(r1, R_LIB)!.pins[0]!.tip;
  const sch = schematic([r1], [wire("w1", [[0, 0], tip0, [50_000, 50_000]])]);
  assert.deepEqual(attachedWireEndpoints(sch, "R1"), []);
});

test("attachedWireEndpoints: a wire nowhere near any pin is not attached", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const sch = schematic([r1], [wire("w1", [[0, 0], [1_000, 1_000]])]);
  assert.deepEqual(attachedWireEndpoints(sch, "R1"), []);
});

test("attachedWireEndpoints: an unknown symbol id has nothing attached", () => {
  assert.deepEqual(attachedWireEndpoints(schematic([], []), "nope"), []);
});

test("attachedWireEndpoints: a symbol with no resolved lib_symbols graphics (generic-box fallback) has nothing attached -- same documented gap pinSnapPoints already has for the wire tool", () => {
  const boxy: SchematicSymbol = { id: "U1", lib_id: "Unknown:Part", at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] };
  const sch = schematic([boxy], [wire("w1", [[0, 0], [0, 0]])]);
  assert.deepEqual(attachedWireEndpoints(sch, "U1"), []);
});

test("computeDragAttachment: maps every symbol ref to its own attachedWireEndpoints, keyed by id, and leaves out a ref that isn't a symbol on this sheet", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const r2 = resistor("R2", [50_000, 50_000]);
  const tip0 = resolveLibSymbol(r1, R_LIB)!.pins[0]!.tip;
  const sch = schematic([r1, r2], [wire("w1", [[0, 0], tip0])]);
  const attach = computeDragAttachment(sch, ["R1", "R2", "not-a-symbol"]);
  assert.deepEqual(attach, { R1: [[0, 1]], R2: [] });
  assert.ok(!("not-a-symbol" in attach));
});
