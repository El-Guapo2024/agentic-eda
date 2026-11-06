import { test } from "node:test";
import assert from "node:assert/strict";
import { addColumn, availableColumns, cellText, extraFieldsOf, loadShownColumns, moveColumnDown, moveColumnUp, openListDialog, removeColumn, shownColumnTerms, treeColumns } from "./libTreeColumns";

test("loadColumnConfig: Item and Description by default, Item always first", () => {
  assert.deepEqual(loadShownColumns(undefined), ["Item", "Description"]);
  assert.deepEqual(loadShownColumns([]), ["Item", "Description"]);
  assert.deepEqual(loadShownColumns(["Description", "Value"]), ["Item", "Description", "Value"]);
  assert.deepEqual(loadShownColumns(["Item", "Keywords"]), ["Item", "Keywords"]);
});

test("recreateColumns: Item first, then the shown ones once", () => {
  assert.deepEqual(treeColumns(["Value", "Item", "Description"]), ["Item", "Value", "Description"]);
});

test("the available columns: Item, Description, then each library's fields sorted, new ones appended", () => {
  const device: Array<{ fields: Record<string, string> }> = [{ fields: { Reference: "R", Value: "R", Keywords: "res", Description: "x" } }, { fields: { MPN: "a" } }];
  const other = [{ fields: { Value: "U", Manufacturer: "TI" } }];
  assert.deepEqual(availableColumns([extraFieldsOf(device), extraFieldsOf(other)]), ["Item", "Description", "Keywords", "MPN", "Reference", "Value", "Manufacturer"]);
  assert.deepEqual(availableColumns([]), ["Item", "Description"], "a footprint tree offers these two");
});

test("GetValue: Description is the description, a field its text, line breaks as spaces", () => {
  const item = { description: "two\nlines", fields: { Value: "10k" } };
  assert.equal(cellText(item, "Description"), "two lines");
  assert.equal(cellText(item, "Value"), "10k");
  assert.equal(cellText(item, "MPN"), "");
});

test("RebuildSearchTerms: only the shown fields are search words, weighted 4", () => {
  assert.deepEqual(shownColumnTerms({ fields: { Value: "10k", MPN: "RC0603" } }, ["Item", "MPN"]), [{ text: "RC0603", score: 4 }]);
});

test("EDA_REORDERABLE_LIST_DIALOG: add after the selection, Item cannot be removed or passed, up and down swap", () => {
  let d = openListDialog(["Item", "Description", "Keywords", "Value"], ["Item", "Description"]);
  assert.deepEqual(d.available, ["Keywords", "Value"]);
  // `>` with Keywords selected and Item selected on the right: it goes right after Item
  d = addColumn(d);
  assert.deepEqual(d.enabled, ["Item", "Keywords", "Description"]);
  assert.deepEqual(d.available, ["Value"]);
  // Item (the first) is not removed
  assert.deepEqual(removeColumn(d), d);
  d = { ...d, selectedEnabled: 2 };
  d = moveColumnUp(d);
  assert.deepEqual(d.enabled, ["Item", "Description", "Keywords"]);
  assert.equal(d.selectedEnabled, 1);
  d = moveColumnDown(d);
  assert.deepEqual(d.enabled, ["Item", "Keywords", "Description"]);
  assert.deepEqual(moveColumnDown({ ...d, selectedEnabled: 2 }).enabled, d.enabled, "the last does not move down");
  // `<` puts the column back at the selected available place
  d = removeColumn({ ...d, selectedEnabled: 2, selectedAvailable: 0 });
  assert.deepEqual(d.enabled, ["Item", "Keywords"]);
  assert.deepEqual(d.available, ["Description", "Value"]);
});
