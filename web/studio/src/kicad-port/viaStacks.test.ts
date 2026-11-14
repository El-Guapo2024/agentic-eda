import { test } from "node:test";
import assert from "node:assert/strict";
import type { ViaStackPreset } from "../api/types";
import { microviaStackProblem, nameIsFree, nameTakenMessage, netclassViaSize, newViaStackPreset, presetRow, settledPreset, viaStacksProblem } from "./viaStacks";
import { COPPER_FINISH_CHOICES, finishDraftOf, withFinish } from "./boardFinish";

const copper = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const ok: ViaStackPreset = { ...newViaStackPreset("HDI"), via_size_nm: 300_000, via_drill_nm: 150_000 };
const fmt = (nm: number, unit: boolean) => `${nm / 1e6}${unit ? " mm" : ""}`;

test("UI-LEFT-3 B3-8: a new preset is VIA_STACK_PRESET()'s, and a row shows rebuildGrid's cells", () => {
  assert.deepEqual(newViaStackPreset("A"), { name: "A", start_layer: "F.Cu", end_layer: "In1.Cu", staggered: false, via_size_nm: 0, via_drill_nm: 0, use_netclass: false, filled: true, capped: false, pitch_nm: 0 });
  assert.deepEqual(presetRow(ok, (l) => l, fmt), ["HDI", "Stacked", "F.Cu -> In1.Cu", "0.3 / 0.15 mm", "yes", "-"]);
  assert.deepEqual(presetRow({ ...ok, staggered: true, pitch_nm: 400_000, use_netclass: true, filled: false }, (l) => l, fmt), ["HDI", "Staggered", "F.Cu -> In1.Cu", "netclass", "no", "0.4 mm"]);
});

test("UI-LEFT-3 B3-8: names are free without regard to case (nameIsFree)", () => {
  assert.equal(nameIsFree([ok], "hdi", -1), false);
  assert.equal(nameIsFree([ok], "hdi", 0), true, "a row's own name is free for it");
  assert.equal(nameTakenMessage("HDI"), 'A preset named "HDI" already exists.');
});

test("UI-LEFT-3 B3-8: the Microvia Stack dialog's checks, in its order and words", () => {
  const p = (over: Partial<ViaStackPreset>) => microviaStackProblem({ ...ok, ...over }, copper, "25 mm");
  assert.equal(p({}), null);
  assert.equal(p({ end_layer: "F.Cu" }), "The start and end layers must be different.");
  assert.equal(p({ end_layer: "In5.Cu" }), "Select a start and an end layer present on this board.");
  assert.equal(p({ staggered: true }), "Staggered stacks need a pitch greater than zero.");
  assert.equal(p({ staggered: true, pitch_nm: 26_000_000 }), "The pitch must be 25 mm or less.");
  assert.equal(p({ via_size_nm: 0 }), "Via diameter is too small.");
  assert.equal(p({ via_drill_nm: 0 }), "Via drill is too small.");
  assert.equal(p({ via_drill_nm: 300_000 }), "Via hole size must be smaller than via diameter");
  assert.equal(p({ use_netclass: true, via_size_nm: 0, via_drill_nm: 0 }), null, "the class gives the sizes");
  assert.deepEqual(settledPreset({ ...ok, filled: false }).filled, true, "a stacked stack is filled");
  assert.deepEqual(settledPreset({ ...ok, staggered: true, filled: false }).filled, false);
  assert.deepEqual([settledPreset({ ...ok, use_netclass: true }).via_size_nm, settledPreset({ ...ok, use_netclass: true }).via_drill_nm], [0, 0]);
  assert.deepEqual(viaStacksProblem([ok, { ...ok, name: "hdi" }], copper, "25 mm"), { row: 1, message: 'A preset named "hdi" already exists.' });
  assert.deepEqual(netclassViaSize({ name: "Default", nets: [], microvia_diameter: 200_000, microvia_drill: 100_000 } as never, { diameter: 600_000, drill: 300_000 }), { size: 200_000, drill: 100_000 });
  assert.deepEqual(netclassViaSize(undefined, { diameter: 600_000, drill: 300_000 }), { size: 600_000, drill: 300_000 }, "GetCurrentViaSize when the class has none");
});

test("UI-LEFT-3 B3-8: Board Finish shows a finish of the list, an unknown one as User defined, and writes the page's three values", () => {
  assert.deepEqual(finishDraftOf(undefined), { finish: "Not specified", plated: false, edge: 0 });
  assert.deepEqual(finishDraftOf({ copper_finish: "ENIG", edge_plating: true, edge_connector: 2 }), { finish: "ENIG", plated: true, edge: 2 });
  assert.equal(finishDraftOf({ copper_finish: "Gold flash" }).finish, "User defined");
  assert.equal(COPPER_FINISH_CHOICES[0], "Not specified");
  assert.equal(COPPER_FINISH_CHOICES[COPPER_FINISH_CHOICES.length - 1], "User defined");
  const s = withFinish({ layers: [], copper_finish: "ENIG" }, { finish: "Not specified", plated: true, edge: 1 });
  assert.deepEqual(s, { layers: [], edge_plating: true, edge_connector: 1 });
  assert.equal(withFinish({ layers: [] }, { finish: "OSP", plated: false, edge: 0 }).copper_finish, "OSP");
});
