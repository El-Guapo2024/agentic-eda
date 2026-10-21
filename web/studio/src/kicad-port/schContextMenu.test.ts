import { test } from "node:test";
import assert from "node:assert/strict";
import { changeToEntries, emptySummary, schContextMenu, type SchSelectionSummary } from "./schContextMenu";
import type { MenuNode } from "../kicad/types";

const sel = (over: Partial<SchSelectionSummary>): SchSelectionSummary => {
  const s = { ...emptySummary(), ...over };
  // total and the lock counts follow from the kinds unless a test sets them.
  const kinds = s.symbols + s.powerSymbols + s.wires + s.buses + s.lines + s.localLabels + s.globalLabels + s.hierLabels + s.directiveLabels + s.texts + s.textBoxes + s.junctions + s.busEntries + s.noConnects + s.sheets + s.shapes + s.ruleAreas + s.tables + s.tableCells;
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

test("a selected polygon or rule area offers Create / Remove Corner only under the pointer conditions", () => {
  const none = actionsOf(schContextMenu(sel({ ruleAreas: 1 })));
  assert.equal(none.includes("eeschema.PointEditor.addCorner") || none.includes("eeschema.PointEditor.removeCorner"), false);
  const onEdge = actionsOf(schContextMenu(sel({ ruleAreas: 1, canAddCorner: true })));
  assert.ok(onEdge.includes("eeschema.PointEditor.addCorner") && !onEdge.includes("eeschema.PointEditor.removeCorner"));
  const onCorner = actionsOf(schContextMenu(sel({ shapes: 1, canAddCorner: true, canRemoveCorner: true })));
  assert.ok(onCorner.includes("eeschema.PointEditor.addCorner") && onCorner.includes("eeschema.PointEditor.removeCorner"));
  // Count( 1 ): not with several items selected.
  assert.equal(actionsOf(schContextMenu(sel({ ruleAreas: 2, canAddCorner: true }))).includes("eeschema.PointEditor.addCorner"), false);
});

test("an empty selection offers Paste, Paste Special and Select All and nothing about an item", () => {
  const a = actionsOf(schContextMenu(sel({})));
  assert.deepEqual(a, ["common.Interactive.paste", "common.Interactive.pasteSpecial", "common.Interactive.selectAll"]);
});

test("a selection offers the clipboard group in KiCad's order: Cut, Copy, Paste, Paste Special, Delete, Duplicate", () => {
  const a = actionsOf(schContextMenu(sel({ symbols: 1 })));
  const tail = a.slice(a.indexOf("common.Interactive.cut"));
  assert.deepEqual(tail, ["common.Interactive.cut", "common.Interactive.copy", "common.Interactive.paste", "common.Interactive.pasteSpecial", "common.Interactive.delete", "common.Interactive.duplicate", "common.Interactive.selectAll"]);
});

test("a single power symbol offers Properties, as the one symbol it is in KiCad does; two of them do not", () => {
  assert.ok(actionsOf(schContextMenu(sel({ powerSymbols: 1 }))).includes("eeschema.InteractiveEdit.properties"));
  assert.ok(actionsOf(schContextMenu(sel({ symbols: 1 }))).includes("eeschema.InteractiveEdit.properties"));
  assert.ok(!actionsOf(schContextMenu(sel({ powerSymbols: 2 }))).includes("eeschema.InteractiveEdit.properties"));
  assert.ok(!actionsOf(schContextMenu(sel({ powerSymbols: 1, symbols: 1 }))).includes("eeschema.InteractiveEdit.properties"));
});

// `SCH_EDIT_TABLE_TOOL::Init` (`EDIT_TABLE_TOOL_BASE::addMenus`) and `SCH_SELECTION_TOOL::Init`'s Select Column(s)/Row(s)/Table.
test("a selection of table cells offers the selections and the structure edits of a table", () => {
  const a = actionsOf(schContextMenu(sel({ tableCells: 1 })));
  for (const want of [
    "common.InteractiveSelection.SelectColumns",
    "common.InteractiveSelection.Rows",
    "common.InteractiveSelection.SelectTable",
    "common.TableEditor.addRowAbove",
    "common.TableEditor.addRowBelow",
    "common.TableEditor.addColBefore",
    "common.TableEditor.addColAfter",
    "common.TableEditor.deleteRows",
    "common.TableEditor.deleteColumns",
    "pcbnew.TableEditor.editTable",
    "common.TableEditor.exportTableCSV",
    "eeschema.InteractiveEdit.properties",
  ])
    assert.ok(a.includes(want), want);
  // a single cell is no block and not merged: neither Merge nor Unmerge
  assert.ok(!a.includes("common.TableEditor.mergeCells") && !a.includes("common.TableEditor.unmergeCell"));
});

test("Merge Cells is offered for a block of cells and Unmerge Cell when one of them is merged", () => {
  assert.ok(actionsOf(schContextMenu(sel({ tableCells: 2, cellBlock: true }))).includes("common.TableEditor.mergeCells"));
  const merged = actionsOf(schContextMenu(sel({ tableCells: 1, mergedCells: true })));
  assert.ok(merged.includes("common.TableEditor.unmergeCell") && !merged.includes("common.TableEditor.mergeCells"));
});

test("a table that is not a selection of cells has no table editing entries, and a cell with something else has none either", () => {
  const whole = actionsOf(schContextMenu(sel({ tables: 1 })));
  assert.ok(!whole.includes("common.TableEditor.addRowAbove") && !whole.includes("common.InteractiveSelection.SelectTable"));
  assert.ok(whole.includes("eeschema.InteractiveEdit.properties"), "a table has Properties");
  const mixed = actionsOf(schContextMenu(sel({ tableCells: 1, symbols: 1 })));
  assert.ok(!mixed.includes("common.TableEditor.addRowAbove") && !mixed.includes("common.InteractiveSelection.Rows"));
  // neither is offered when nothing is selected
  assert.ok(!actionsOf(schContextMenu(sel({}))).includes("common.TableEditor.deleteRows"));
});
