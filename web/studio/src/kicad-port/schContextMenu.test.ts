import { test } from "node:test";
import assert from "node:assert/strict";
import { changeToEntries, emptySummary, schContextMenu, type SchSelectionSummary } from "./schContextMenu";
import type { MenuNode } from "../kicad/types";

const sel = (over: Partial<SchSelectionSummary>): SchSelectionSummary => {
  const s = { ...emptySummary(), ...over };
  // total and the lock counts follow from the kinds unless a test sets them.
  const kinds = s.symbols + s.powerSymbols + s.wires + s.buses + s.lines + s.localLabels + s.globalLabels + s.hierLabels + s.directiveLabels + s.texts + s.textBoxes + s.junctions + s.busEntries + s.noConnects + s.sheets + s.shapes + s.ruleAreas;
  if (over.total === undefined) s.total = kinds;
  if (over.locked === undefined && over.unlocked === undefined) s.unlocked = s.total;
  return s;
};

const actionsOf = (nodes: MenuNode[]): string[] => nodes.flatMap((n) => (n.type === "item" ? [n.action] : n.type === "submenu" ? actionsOf(n.items) : []));

test("a single local label offers every Change To but its own", () => {
  assert.deepEqual(changeToEntries(sel({ localLabels: 1 })), [
    "eeschema.InteractiveEdit.toCLabel",
    "eeschema.InteractiveEdit.toHLabel",
    "eeschema.InteractiveEdit.toGLabel",
    "eeschema.InteractiveEdit.toText",
    "eeschema.InteractiveEdit.toTextBox",
  ]);
});

test("a single text offers every Change To but Text; several text-ish items offer all six", () => {
  assert.equal(changeToEntries(sel({ texts: 1 })).includes("eeschema.InteractiveEdit.toText"), false);
  assert.equal(changeToEntries(sel({ texts: 1 })).length, 5);
  assert.equal(changeToEntries(sel({ localLabels: 1, texts: 1 })).length, 6);
});

test("Change To is not offered when anything else is selected", () => {
  assert.deepEqual(changeToEntries(sel({ localLabels: 1, wires: 1 })), []);
  assert.deepEqual(changeToEntries(sel({})), []);
});

test("a wire selection offers Break and Slice, a connected selection the netclass actions", () => {
  const a = actionsOf(schContextMenu(sel({ wires: 1 })));
  for (const want of ["eeschema.InteractiveEdit.breakWire", "eeschema.InteractiveEdit.slice", "eeschema.InteractiveEdit.assignNetclass", "eeschema.InteractiveEdit.findNetInInspector"]) assert.ok(a.includes(want), want);
  const b = actionsOf(schContextMenu(sel({ symbols: 1 })));
  assert.equal(b.includes("eeschema.InteractiveEdit.breakWire"), false);
  assert.equal(b.includes("eeschema.InteractiveEdit.assignNetclass"), false, "a symbol is not a connected item");
});

test("a single symbol offers Change/Update Symbol, several offer the plural forms", () => {
  const one = actionsOf(schContextMenu(sel({ symbols: 1 })));
  assert.ok(one.includes("eeschema.InteractiveEdit.changeSymbol") && one.includes("eeschema.InteractiveEdit.updateSymbol"));
  assert.equal(one.includes("eeschema.InteractiveEdit.changeSymbols"), false);
  const many = actionsOf(schContextMenu(sel({ symbols: 3 })));
  assert.ok(many.includes("eeschema.InteractiveEdit.changeSymbols") && many.includes("eeschema.InteractiveEdit.updateSymbols"));
  assert.ok(many.includes("eeschema.InteractiveEdit.swap"));
});

test("the Locking submenu offers Lock only when something is unlocked and Unlock only when something is locked", () => {
  const lockItems = (s: SchSelectionSummary) => {
    const m = schContextMenu(s).find((n) => n.type === "submenu" && n.label === "Locking");
    return m && m.type === "submenu" ? actionsOf(m.items) : [];
  };
  assert.deepEqual(lockItems(sel({ symbols: 2 })), ["eeschema.InteractiveEdit.lock", "eeschema.InteractiveEdit.toggleLock"]);
  assert.deepEqual(lockItems(sel({ symbols: 2, locked: 2, unlocked: 0 })), ["eeschema.InteractiveEdit.unlock", "eeschema.InteractiveEdit.toggleLock"]);
  assert.deepEqual(lockItems(sel({ symbols: 2, locked: 1, unlocked: 1 })), ["eeschema.InteractiveEdit.lock", "eeschema.InteractiveEdit.unlock", "eeschema.InteractiveEdit.toggleLock"]);
  assert.deepEqual(lockItems(sel({})), []);
});

test("a single sheet offers its pin tools; Cleanup Sheet Pins only when it has undefined pins", () => {
  const a = actionsOf(schContextMenu(sel({ sheets: 1 })));
  for (const want of ["eeschema.NavigateTool.enterSheet", "eeschema.InteractiveDrawing.placeSheetPin", "eeschema.InteractiveDrawing.autoplaceAllSheetPins", "eeschema.InteractiveDrawing.syncSheetPins"]) assert.ok(a.includes(want), want);
  assert.equal(a.includes("eeschema.InteractiveEdit.cleanupSheetPins"), false);
  assert.ok(actionsOf(schContextMenu(sel({ sheets: 1, sheetHasUndefinedPins: true }))).includes("eeschema.InteractiveEdit.cleanupSheetPins"));
});

test("an empty selection offers Select All and nothing about an item", () => {
  const a = actionsOf(schContextMenu(sel({})));
  assert.deepEqual(a, ["common.Interactive.selectAll"]);
});
