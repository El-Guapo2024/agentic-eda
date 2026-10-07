import { test } from "node:test";
import assert from "node:assert/strict";
import { singleNetLabelForPin, swapUnitLabels, type P, type SwapSheet } from "./schSwapLabels";

const wire = (id: string, ...pts: P[]) => ({ id, pts });
const label = (id: string, net: string, at: P) => ({ id, net, at });

// Two units of U1, each with two pins that run out on a wire to a label.
const sheet: SwapSheet = {
  wires: [wire("w1", [0, 0], [5_000, 0]), wire("w2", [0, 2_000], [5_000, 2_000]), wire("w3", [20_000, 0], [25_000, 0]), wire("w4", [20_000, 2_000], [25_000, 2_000])],
  labels: [label("l1", "A", [5_000, 0]), label("l2", "B", [5_000, 2_000]), label("l3", "C", [25_000, 0]), label("l4", "D", [25_000, 2_000])],
  pinPoints: [[0, 0], [0, 2_000], [20_000, 0], [20_000, 2_000]],
};

test("a pin's single net label is found through its wire", () => {
  assert.equal(singleNetLabelForPin(sheet, [0, 0]), "l1");
  assert.equal(singleNetLabelForPin(sheet, [20_000, 2_000]), "l4");
});

test("a pin with no label, two labels or another pin on its connection has no single label", () => {
  assert.equal(singleNetLabelForPin({ ...sheet, labels: sheet.labels.filter((l) => l.id !== "l1") }, [0, 0]), null);
  assert.equal(singleNetLabelForPin({ ...sheet, labels: [...sheet.labels, label("l1b", "A2", [2_000, 0])] }, [0, 0]), null);
  assert.equal(singleNetLabelForPin({ ...sheet, pinPoints: [...sheet.pinPoints, [5_000, 0]] }, [0, 0]), null, "the far end of the wire is a pin too");
});

test("a label right on the pin counts without any wire", () => {
  const bare: SwapSheet = { wires: [], labels: [label("x", "NET", [100, 100])], pinPoints: [[100, 100]] };
  assert.equal(singleNetLabelForPin(bare, [100, 100]), "x");
});

test("two units trade their labels pin by pin", () => {
  const r = swapUnitLabels(sheet, [{ unit: 1, tips: [[0, 0], [0, 2_000]] }, { unit: 2, tips: [[20_000, 0], [20_000, 2_000]] }]);
  assert.deepEqual(r, { ok: true, edits: [{ id: "l1", net: "C" }, { id: "l3", net: "A" }, { id: "l2", net: "D" }, { id: "l4", net: "B" }] });
});

test("three units pass the texts one unit along", () => {
  const three: SwapSheet = {
    wires: [],
    labels: [label("a", "X", [0, 0]), label("b", "Y", [10, 0]), label("c", "Z", [20, 0])],
    pinPoints: [[0, 0], [10, 0], [20, 0]],
  };
  const r = swapUnitLabels(three, [{ unit: 1, tips: [[0, 0]] }, { unit: 2, tips: [[10, 0]] }, { unit: 3, tips: [[20, 0]] }]);
  // The last text goes to the first unit, each other unit takes the previous unit's.
  assert.deepEqual(r, { ok: true, edits: [{ id: "a", net: "Z" }, { id: "b", net: "X" }, { id: "c", net: "Y" }] });
});

test("a pin without a single label stops the swap with KiCad's message", () => {
  const r = swapUnitLabels({ ...sheet, labels: sheet.labels.filter((l) => l.id !== "l3") }, [{ unit: 1, tips: [[0, 0], [0, 2_000]] }, { unit: 2, tips: [[20_000, 0], [20_000, 2_000]] }]);
  assert.equal(r.ok, false);
  assert.match((r as { message: string }).message, /exactly one attached net label/);
});

test("labels that already share a text are left alone", () => {
  const same: SwapSheet = { wires: [], labels: [label("a", "X", [0, 0]), label("b", "X", [10, 0])], pinPoints: [[0, 0], [10, 0]] };
  assert.deepEqual(swapUnitLabels(same, [{ unit: 1, tips: [[0, 0]] }, { unit: 2, tips: [[10, 0]] }]), { ok: true, edits: [] });
});
