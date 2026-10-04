import { test } from "node:test";
import assert from "node:assert/strict";
import { ercMarkerPosition } from "./ercMarkerPosition";
import { resolveLibSymbol } from "./libSymbol";
import type { LibSymbols, PowerSymbol, Schematic, SchematicLabel, SchematicSymbol, SchematicWire } from "../../api/types";

// Same minimal two-pin "Device:R"-shaped library symbol wireAttachment's
// own test fixture uses -- real pin geometry, not a generic-box fallback,
// so a "REF.PIN" location resolves to a real tip rather than an
// approximate box center.
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

function boxSymbol(id: string, at: [number, number]): SchematicSymbol {
  // `lib_id: null` -- no real geometry resolves, same "no lib_symbols
  // entry yet" gap every other lib_symbols consumer in this app falls
  // back on (layout.ts's generic box).
  return { id, lib_id: null, at, rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] };
}

function powerSymbol(id: string, net: string, at: [number, number]): PowerSymbol {
  return { id, lib_id: "power:GND", at, rot: 0, net, pin: { number: "1", name: net, kind: "power" } };
}

function wire(id: string, net: string, pts: [number, number][]): SchematicWire {
  return { id, net, pins: [], pts, bus: false };
}

function label(id: string, net: string, at: [number, number]): SchematicLabel {
  return { id, net, at, scope: "local", shape: null };
}

function schematic(partial: Partial<Schematic>): Schematic {
  return { lib_symbols: R_LIB, symbols: [], power_symbols: [], wires: [], no_connects: [], labels: [], texts: [], title_block: null, bus_entries: [], sheets: [], sheet_path: [], ...partial };
}

test("ercMarkerPosition: null location or null schematic resolves to null", () => {
  assert.equal(ercMarkerPosition(null, schematic({})), null);
  assert.equal(ercMarkerPosition("U1.1", null), null);
});

test("ercMarkerPosition: bare 'x,y' (no_connect_connected/dangling) is a literal point, no refs", () => {
  const sch = schematic({});
  assert.deepEqual(ercMarkerPosition("12000,34000", sch), { at: [12000, 34000], refs: [] });
  // Negative coordinates (left/above origin) parse too.
  assert.deepEqual(ercMarkerPosition("-500,-250", sch), { at: [-500, -250], refs: [] });
});

test("ercMarkerPosition: 'NET:x,y' (unconnected_wire_endpoint) is the same literal point, net prefix ignored", () => {
  const sch = schematic({});
  assert.deepEqual(ercMarkerPosition("VCC:12000,34000", sch), { at: [12000, 34000], refs: [] });
});

test("ercMarkerPosition: bare 'REF.PIN' (pin_not_connected) resolves to the real pin tip", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const tip = resolveLibSymbol(r1, R_LIB)!.pins[0]!.tip;
  const sch = schematic({ symbols: [r1] });
  assert.deepEqual(ercMarkerPosition("R1.1", sch), { at: tip, refs: ["R1"] });
});

test("ercMarkerPosition: 'NET:REF.PIN' (pin_to_pin/pin_not_driven) resolves the pin half, net ignored", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const tip = resolveLibSymbol(r1, R_LIB)!.pins[1]!.tip;
  const sch = schematic({ symbols: [r1] });
  assert.deepEqual(ercMarkerPosition("VCC:R1.2", sch), { at: tip, refs: ["R1"] });
});

test("ercMarkerPosition: 'REF.PIN' with no real lib_symbols geometry falls back to the symbol's box center", () => {
  const u1 = boxSymbol("U1", [0, 0]);
  const sch = schematic({ symbols: [u1] });
  const resolved = ercMarkerPosition("U1.3", sch);
  assert.ok(resolved);
  assert.deepEqual(resolved!.refs, ["U1"]);
});

test("ercMarkerPosition: 'REF.PIN' with a real symbol but an unknown pin number falls back to the symbol's center, not null", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const sch = schematic({ symbols: [r1] });
  const resolved = ercMarkerPosition("R1.99", sch);
  assert.ok(resolved);
  assert.deepEqual(resolved!.refs, ["R1"]);
});

test("ercMarkerPosition: 'NET:ID' (pin_to_pin/pin_not_driven on a power symbol) resolves the power symbol's own position", () => {
  const pwr = powerSymbol("PWR1", "GND", [5_000, 6_000]);
  const sch = schematic({ power_symbols: [pwr] });
  assert.deepEqual(ercMarkerPosition("GND:PWR1", sch), { at: [5_000, 6_000], refs: [] });
});

test("ercMarkerPosition: a bare symbol ref with no pin resolves to the symbol's box center (forward-compat shape, not emitted today)", () => {
  const r1 = resistor("R1", [10_000, 10_000]);
  const sch = schematic({ symbols: [r1] });
  const resolved = ercMarkerPosition("R1", sch);
  assert.ok(resolved);
  assert.deepEqual(resolved!.refs, ["R1"]);
});

test("ercMarkerPosition: a bare net name (wire_dangling) resolves to the net's label anchor first", () => {
  const sch = schematic({
    labels: [label("lbl1", "RESET", [1_000, 2_000])],
    power_symbols: [powerSymbol("PWR2", "RESET", [9_000, 9_000])],
    wires: [wire("w1", "RESET", [[0, 0], [100, 100]])],
  });
  assert.deepEqual(ercMarkerPosition("RESET", sch), { at: [1_000, 2_000], refs: [] });
});

test("ercMarkerPosition: a bare net name with no label falls back to a power symbol anchor, then a wire endpoint", () => {
  const withPower = schematic({ power_symbols: [powerSymbol("PWR2", "RESET", [9_000, 9_000])], wires: [wire("w1", "RESET", [[0, 0], [100, 100]])] });
  assert.deepEqual(ercMarkerPosition("RESET", withPower), { at: [9_000, 9_000], refs: [] });

  const wireOnly = schematic({ wires: [wire("w1", "RESET", [[11, 22], [100, 100]])] });
  assert.deepEqual(ercMarkerPosition("RESET", wireOnly), { at: [11, 22], refs: [] });
});

test("ercMarkerPosition: an unresolvable location (no matching symbol, power symbol, or net anchor) is null", () => {
  const sch = schematic({});
  assert.equal(ercMarkerPosition("NOPE.5", sch), null);
  assert.equal(ercMarkerPosition("NOPE", sch), null);
  assert.equal(ercMarkerPosition("NET:NOPE", sch), null);
});

test("ercMarkerPosition: kicad-cli's ids -- a bare power symbol, wire, label, no-connect or text id", () => {
  const sch = schematic({
    power_symbols: [powerSymbol("pwr_1", "GND", [3000, 4000])],
    wires: [wire("wire_1", "VIN", [[100, 200], [900, 200]])],
    labels: [label("lbl_1", "VIN", [500, 600])],
    no_connects: [{ id: "nc_1", at: [700, 800] }],
    texts: [{ id: "txt_1", content: "note", at: [1100, 1200] } as unknown as Schematic["texts"][number]],
  });
  assert.deepEqual(ercMarkerPosition("pwr_1", sch), { at: [3000, 4000], refs: [] });
  assert.deepEqual(ercMarkerPosition("wire_1", sch), { at: [100, 200], refs: [] }, "a wire resolves to its first point");
  assert.deepEqual(ercMarkerPosition("lbl_1", sch), { at: [500, 600], refs: [] });
  assert.deepEqual(ercMarkerPosition("nc_1", sch), { at: [700, 800], refs: [] });
  assert.deepEqual(ercMarkerPosition("txt_1", sch), { at: [1100, 1200], refs: [] });
  assert.equal(ercMarkerPosition("wire_gone", sch), null, "an id the schematic no longer has resolves to nothing");
});

test("ercMarkerPosition: kicad-cli's 'REF' and 'REF.PIN' ids select the symbol", () => {
  const sch = schematic({ symbols: [resistor("R1", [10_000, 10_000])] });
  const pin = ercMarkerPosition("R1.1", sch);
  assert.deepEqual(pin?.refs, ["R1"]);
  const sym = ercMarkerPosition("R1", sch);
  assert.deepEqual(sym?.refs, ["R1"]);
});
