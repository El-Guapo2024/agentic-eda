import { test } from "node:test";
import assert from "node:assert/strict";
import { FILTER_ROWS, onlyFilter, onlyLabel, selectionFilterFromJson, selectionFilterJson, type FilterOptions } from "./selectionFilterRows";

const ALL: FilterOptions = { lockedItems: false, footprints: true, text: true, tracks: true, vias: true, pads: true, graphics: true, zones: true, keepouts: true, dimensions: true, points: true, otherItems: true, gridItems: true };

test("the rows that take the right-click menu are the panel's eleven, in its order and words", () => {
  assert.deepEqual(FILTER_ROWS.map((r) => r.label), ["Footprints", "Text", "Tracks", "Vias", "Pads", "Graphics", "Zones", "Rule Areas", "Dimensions", "Points", "Other items"]);
});

test("onlyLabel: `Only %s` of the row's label in lower case", () => {
  assert.equal(onlyLabel("Footprints"), "Only footprints");
  assert.equal(onlyLabel("Rule Areas"), "Only rule areas");
  assert.equal(onlyLabel("Other items"), "Only other items");
});

test("onlyFilter (onPopupSelection): every row off, Grid Items too, then the row; Locked items left as it was", () => {
  const only = onlyFilter({ ...ALL, lockedItems: true }, "vias");
  assert.equal(only.vias, true);
  assert.equal(only.lockedItems, true);
  for (const r of FILTER_ROWS) if (r.key !== "vias") assert.equal(only[r.key], false, r.key);
  assert.equal(only.gridItems, false);
  assert.equal(onlyFilter(ALL, "keepouts").keepouts, true);
});

test("the filter as the project keeps it: every key written, a missing row on; read back with SetIfPresent", () => {
  const json = selectionFilterJson({ ...ALL, keepouts: undefined, gridItems: undefined, footprints: false, lockedItems: true });
  assert.equal(json.footprints, false);
  assert.equal(json.lockedItems, true);
  assert.equal(json.keepouts, true, "absent is on");
  assert.equal(json.gridItems, true);
  const back = selectionFilterFromJson(ALL, json);
  assert.equal(back.footprints, false);
  assert.equal(back.lockedItems, true);
  // a key of the wrong type, or no object at all, leaves the switch as it was
  assert.equal(selectionFilterFromJson(ALL, { tracks: "no", vias: false }).tracks, true);
  assert.equal(selectionFilterFromJson(ALL, { tracks: "no", vias: false }).vias, false);
  assert.deepEqual(selectionFilterFromJson(ALL, null), ALL);
  assert.deepEqual(selectionFilterFromJson(ALL, [1, 2]), ALL);
});
