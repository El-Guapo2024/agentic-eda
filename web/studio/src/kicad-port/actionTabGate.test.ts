import { test } from "node:test";
import assert from "node:assert/strict";
import { isActionEnabledForTab } from "./actionTabGate";

test("isActionEnabledForTab: an unregistered action is never enabled, regardless of tab or prefix", () => {
  assert.equal(isActionEnabledForTab("pcbnew.InteractiveEdit.flip", "pcb", false), false);
  assert.equal(isActionEnabledForTab("eeschema.InteractiveMove.move", "schematic", false), false);
});

test("isActionEnabledForTab: a pcbnew.* action is only enabled on the pcb tab", () => {
  assert.equal(isActionEnabledForTab("pcbnew.InteractiveEdit.rotateCcw", "pcb", true), true);
  assert.equal(isActionEnabledForTab("pcbnew.InteractiveEdit.rotateCcw", "schematic", true), false);
  assert.equal(isActionEnabledForTab("pcbnew.InteractiveEdit.rotateCcw", "3d", true), false);
});

test("isActionEnabledForTab: an eeschema.* action is only enabled on the schematic tab", () => {
  assert.equal(isActionEnabledForTab("eeschema.InteractiveEdit.rotateCCW", "schematic", true), true);
  assert.equal(isActionEnabledForTab("eeschema.InteractiveEdit.rotateCCW", "pcb", true), false);
});

test("isActionEnabledForTab: a common.* (or any other non-pcbnew/eeschema) action is tab-agnostic once registered", () => {
  assert.equal(isActionEnabledForTab("common.Interactive.cancel", "pcb", true), true);
  assert.equal(isActionEnabledForTab("common.Interactive.cancel", "schematic", true), true);
  assert.equal(isActionEnabledForTab("common.Interactive.cancel", "3d", true), true);
});

test("isActionEnabledForTab: the R-key collision this module exists for -- only one of the pair is ever enabled per tab", () => {
  const candidates = ["eeschema.InteractiveEdit.rotateCCW", "eeschema.Simulation.runSimulation", "pcbnew.InteractiveEdit.rotateCcw"];
  const onPcb = candidates.find((n) => isActionEnabledForTab(n, "pcb", true));
  const onSchematic = candidates.find((n) => isActionEnabledForTab(n, "schematic", true));
  assert.equal(onPcb, "pcbnew.InteractiveEdit.rotateCcw", "the PCB tab must resolve R to its own rotate, not have it shadowed by eeschema's same-keyed action coming first in the list");
  assert.equal(onSchematic, "eeschema.InteractiveEdit.rotateCCW");
});
