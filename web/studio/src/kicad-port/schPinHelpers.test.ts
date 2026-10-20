import { test } from "node:test";
import assert from "node:assert/strict";
import { allPinsOrSheetPins, madeByPinHelper, PIN_HELPERS, pinHelperCmd, pinHelperTargets } from "./schPinHelpers";

const sheets = [{ id: "sheet_a", name: "Sub", file: "sub.kicad_sch", at: [0, 0], size: [10, 10], pins: [{ id: "shpin_a", name: "CLK", shape: "input", at: [0, 5] }] }] as never;

test("the submenu is there for a selection of pins and sheet pins and for nothing else", () => {
  assert.ok(allPinsOrSheetPins({ total: 1, pins: 1, sheetPins: 0 }));
  assert.ok(allPinsOrSheetPins({ total: 3, pins: 2, sheetPins: 1 }));
  assert.ok(!allPinsOrSheetPins({ total: 0, pins: 0, sheetPins: 0 }), "an empty selection has no pin");
  assert.ok(!allPinsOrSheetPins({ total: 2, pins: 1, sheetPins: 0 }), "a pin and something else");
  assert.deepEqual(
    PIN_HELPERS.map((h) => [h.kind, h.label]),
    [["wire", "Wire"], ["no_connect", "No Connect"], ["net_label", "Net Label"], ["hier_label", "Hierarchical Label"], ["global_label", "Global Label"]],
    "the five entries in `PIN_TRICKS_MENU::update`'s order",
  );
  assert.ok(PIN_HELPERS.every((h) => h.action.startsWith("studio.PinHelpers.")));
});

test("a helper acts on the pins and sheet pins of the selection, in order, and on nothing else", () => {
  assert.deepEqual(pinHelperTargets({ sheets }, ["R1", "pin:U1:2", "shpin_a", "wire_x", "pin:U1:1", "pin:U1:2"]), ["pin:U1:2", "shpin_a", "pin:U1:1"]);
  assert.deepEqual(pinHelperTargets({ sheets }, ["R1", "wire_x"]), []);
});

test("the command is the backend's verb, with the wire grid whole", () => {
  assert.deepEqual(pinHelperCmd("hier_label", ["pin:U1:1"], 1270), { op: "sch_edit", verb: "pin_helper", what: "hier_label", pins: ["pin:U1:1"], grid: 1270 });
  assert.deepEqual(pinHelperCmd("wire", ["pin:U1:1"], 1270.4), { op: "sch_edit", verb: "pin_helper", what: "wire", pins: ["pin:U1:1"], grid: 1270 });
  assert.equal((pinHelperCmd("wire", [], -5) as unknown as { grid: number }).grid, 0);
});

const wire = (id: string, pts: Array<[number, number]>) => ({ id, net: "", pins: [], pts, bus: false });
const label = (id: string) => ({ id, net: id, at: [0, 0], kind: { scope: "local" } });

test("the new labels are selected, the new no-connects are not, and a stub is selected by its far end", () => {
  const before = { wires: [wire("w_old", [[0, 0], [100, 0]])], labels: [label("l_old")] } as never;
  const afterLabels = { wires: [wire("w_old", [[0, 0], [100, 0]])], labels: [label("l_old"), label("l_new1"), label("l_new2")] } as never;
  assert.deepEqual(madeByPinHelper("net_label", before, afterLabels, []).select, ["l_new1", "l_new2"]);
  assert.deepEqual(madeByPinHelper("global_label", before, afterLabels, []).select, ["l_new1", "l_new2"]);
  assert.deepEqual(madeByPinHelper("no_connect", before, afterLabels, []), { select: [], ends: {}, at: null }, "`ClearSelection()`");
  // a stub from the pin at (500, 500) out to (500 - 1270, 500): the far end, point 1, is the one to drag
  const afterWire = { wires: [wire("w_old", [[0, 0], [100, 0]]), wire("w_new", [[500, 500], [-770, 500]])], labels: [] } as never;
  const made = madeByPinHelper("wire", before, afterWire, [[500, 500]]);
  assert.deepEqual(made.select, ["w_new"]);
  assert.deepEqual(made.ends, { w_new: [1] });
  assert.deepEqual(made.at, [-770, 500]);
  // a wire the cleanup wrote the other way round is read the other way
  const flipped = { wires: [wire("w_new", [[-770, 500], [500, 500]])], labels: [] } as never;
  assert.deepEqual(madeByPinHelper("wire", { wires: [], labels: [] } as never, flipped, [[500, 500]]).ends, { w_new: [0] });
  // nothing new (the stub merged into a wire that was there): nothing to drag
  assert.deepEqual(madeByPinHelper("wire", before, before, [[0, 0]]).select, []);
});
