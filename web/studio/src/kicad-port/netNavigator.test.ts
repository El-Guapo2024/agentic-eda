import { test } from "node:test";
import assert from "node:assert/strict";
import { netNavigatorItems, stepNetItem, type NavSchematic } from "./netNavigator";

const sch: NavSchematic = {
  pins: [
    { ref: "R2", number: "1", name: "~", tip: [20_000, 10_000] },
    { ref: "R1", number: "2", name: null, tip: [10_000, 10_000] },
    { ref: "U1", number: "7", name: "VIN", tip: [30_000, 10_000] },
    { ref: "C1", number: "1", name: null, tip: [5_000, 5_000] },
  ],
  wires: [
    { net: "VIN", pins: ["R1.2", "R2.1"] },
    { net: "VIN", pins: ["U1.7"] },
    { net: "GND", pins: ["C1.1"] },
  ],
  labels: [
    { id: "lbl_b", net: "VIN", at: [40_000, 12_700], scope: "local" },
    { id: "lbl_a", net: "VIN", at: [41_000, 12_700], scope: "global" },
    { id: "lbl_g", net: "GND", at: [1_000, 1_000], scope: "local" },
  ],
  powerSymbols: [{ id: "#PWR01", net: "VIN", pin: { number: "1", name: "VIN" } }],
  noConnects: [
    { id: "nc_1", at: [30_000, 10_000] }, // on U1.7 (a VIN pin)
    { id: "nc_2", at: [99_000, 99_000] }, // nowhere
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
  assert.match(items[0]!.text, /^Label 'GND' at \(39\.37 mils, 39\.37 mils\)$/);
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
