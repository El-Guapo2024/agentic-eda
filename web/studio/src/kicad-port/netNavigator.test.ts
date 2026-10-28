import { test } from "node:test";
import assert from "node:assert/strict";
import { navSchematic, netNavigatorItems, sheetNets, stepNetItem, type NavSchematic } from "./netNavigator";
import type { Schematic } from "../api/types";

const sch: NavSchematic = {
  pins: [
    { ref: "R2", number: "1", name: "~", tip: [20_000_000, 10_000_000] },
    { ref: "R1", number: "2", name: null, tip: [10_000_000, 10_000_000] },
    { ref: "U1", number: "7", name: "VIN", tip: [30_000_000, 10_000_000] },
    { ref: "C1", number: "1", name: null, tip: [5_000_000, 5_000_000] },
  ],
  wires: [
    { net: "VIN", pins: ["R1.2", "R2.1"] },
    { net: "VIN", pins: ["U1.7"] },
    { net: "GND", pins: ["C1.1"] },
  ],
  labels: [
    { id: "lbl_b", net: "VIN", at: [40_000_000, 12_700_000], scope: "local" },
    { id: "lbl_a", net: "VIN", at: [41_000_000, 12_700_000], scope: "global" },
    { id: "lbl_g", net: "GND", at: [1_000_000, 1_000_000], scope: "local" },
  ],
  powerSymbols: [{ id: "#PWR01", net: "VIN", pin: { number: "1", name: "VIN" } }],
  noConnects: [
    { id: "nc_1", at: [30_000_000, 10_000_000] }, // on U1.7 (a VIN pin)
    { id: "nc_2", at: [99_000_000, 99_000_000] }, // nowhere
  ],
};

test("netNavigatorItems: pins, power symbols, labels and the no-connects on the net's pins, sorted by their description; wires never", () => {
  const items = netNavigatorItems(sch, "VIN", "mm");
  assert.deepEqual(
    items.map((i) => i.text),
    [
      "Global label 'VIN' at (41.0000 mm, 12.7000 mm)",
      "Label 'VIN' at (40.0000 mm, 12.7000 mm)",
      "No-Connect at (30.0000 mm, 10.0000 mm)",
      "Symbol '#PWR01' pin '1' (VIN)",
      "Symbol 'R1' pin '2'",
      "Symbol 'R2' pin '1' (~)",
      "Symbol 'U1' pin '7' (VIN)",
    ],
  );
  // plain string order: uppercase before lowercase, 'G' < 'L' < 'N' < 'S'
  assert.deepEqual(items.map((i) => i.owner), ["lbl_a", "lbl_b", "nc_1", "#PWR01", "R1", "R2", "U1"]);
});

test("netNavigatorItems: another net sees only its own", () => {
  assert.deepEqual(netNavigatorItems(sch, "GND", "mm").map((i) => i.owner), ["lbl_g", "C1"]);
  assert.deepEqual(netNavigatorItems(sch, "NOPE", "mm"), []);
});

test("netNavigatorItems: coordinates follow the units", () => {
  const items = netNavigatorItems(sch, "GND", "mil");
  assert.match(items[0]!.text, /^Label 'GND' at \(39.37 mils, 39.37 mils\)$/);
});

test("stepNetItem: Tab / Shift+Tab walk the list and wrap at both ends", () => {
  const items = netNavigatorItems(sch, "VIN", "mm");
  const last = items.length - 1;
  assert.equal(stepNetItem(items, null, "lbl_a", true)?.owner, "lbl_b");
  assert.equal(stepNetItem(items, null, "lbl_a", false)?.owner, "U1", "before the first wraps to the last");
  assert.equal(stepNetItem(items, null, items[last]!.owner, true)?.owner, "lbl_a", "after the last wraps to the first");
  assert.equal(stepNetItem(items, items[2]!.key, "ignored", true)?.key, items[3]!.key, "the remembered current item wins over the selection");
});

test("stepNetItem: nothing found, nothing selected", () => {
  const items = netNavigatorItems(sch, "VIN", "mm");
  assert.equal(stepNetItem(items, null, "R9", true), null);
  assert.equal(stepNetItem(items, null, null, true), null);
  assert.equal(stepNetItem([], null, "R1", true), null);
  assert.equal(stepNetItem(items, "pin:gone", "R1", true)?.owner, "R2", "a stale key falls back to the selection");
});

test("r6 C6-M1: a_label_is_an_item_of_the_net_it_is_traced_to, and a pin joined with no wire is too", () => {
  const s: NavSchematic = {
    pins: [
      { ref: "J1", number: "2", name: null, tip: [0, 0], net: "/BAR" },
      // joined to J1.2's net by a label on its end: no wire ends on it
      { ref: "J2", number: "1", name: null, tip: [5_000_000, 0], net: "/BAR" },
      { ref: "J2", number: "2", name: null, tip: [6_000_000, 0], net: null },
    ],
    wires: [{ net: "/BAR", pins: ["J1.2"] }],
    labels: [{ id: "lbl_bar", net: "BAR", net_name: "/BAR", at: [1_000_000, 0], scope: "local" }],
    powerSymbols: [],
    noConnects: [],
  };
  assert.deepEqual(netNavigatorItems(s, "/BAR", "mm").map((i) => i.text), ["Label 'BAR' at (1.0000 mm, 0.0000 mm)", "Symbol 'J1' pin '2'", "Symbol 'J2' pin '1'"]);
  assert.deepEqual(netNavigatorItems(s, "BAR", "mm"), []);
  // Tab from the label walks the traced net's items
  const items = netNavigatorItems(s, "/BAR", "mm");
  assert.equal(stepNetItem(items, null, "lbl_bar", true)?.owner, "J1");
});

test("r6 C6-M1: sheetNets lists a local label's traced net once, not its text beside it", () => {
  const nets = sheetNets({ wires: [{ net: "/BAR" }], labels: [{ net: "BAR", net_name: "/BAR" }], power_symbols: [], symbols: [{ pins: [{ net: "/BAR" }, { net: null }] }] });
  assert.deepEqual(nets, ["/BAR"]);
  // a label on no wire is its own text's net
  assert.deepEqual(sheetNets({ wires: [], labels: [{ net: "FOO", net_name: null }], power_symbols: [], symbols: [] }), ["FOO"]);
});

test("r7 A7-M1: a_pin_of_a_multi_unit_symbol_is_listed_with_its_unit_and_alternate", () => {
  const libPin = (number: string, name: string, unit: number) => ({ number, name, electrical_type: "bidirectional", shape: "line", at: [0, 0], angle_deg: 0, length_mm: 2.54, unit, body_style: 0, hidden: false });
  const sch = {
    lib_symbols: { "MCU:X": { graphics: [], pins: [libPin("1", "PA0", 1), libPin("5", "PB7", 2)], unit_count: 2 } },
    symbols: [{ id: "U1", lib_id: "MCU:X", at: [0, 0], rot: 0, mirror: null, unit: 2, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [{ number: "5", name: "PB7", kind: "signal", net: "/SDA", alt: "SDA" }] }],
    wires: [], labels: [], power_symbols: [], no_connects: [],
  } as unknown as Schematic;
  const items = netNavigatorItems(navSchematic(sch), "/SDA", "mm");
  assert.deepEqual(items.map((i) => [i.text, i.owner]), [["Symbol 'U1B' pin '5' (SDA)", "U1"]]);
  // the project's unit naming names the unit too (H5): separator '.', numbers from '1'
  const dotted = { ...sch, subpart_id_separator: 46, subpart_first_id: 49 } as Schematic;
  assert.equal(netNavigatorItems(navSchematic(dotted), "/SDA", "mm")[0]?.text, "Symbol 'U1.2' pin '5' (SDA)");
});
