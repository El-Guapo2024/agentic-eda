import { test } from "node:test";
import assert from "node:assert/strict";
import { commonChecked, type ToggleSnapshot } from "./commonChecked";

const base: ToggleSnapshot = { crossHairMode: "small", alwaysShowCursor: true, drawBoundingBoxes: false, selectionMode: "rect" };

test("the crosshair modes are a radio group: exactly the current one is checked", () => {
  const modes = ["common.Control.cursorSmallCrosshairs", "common.Control.cursorFullCrosshairs", "common.Control.cursor45Crosshairs"];
  assert.deepEqual(modes.map((m) => commonChecked(m, base)), [true, false, false]);
  assert.deepEqual(modes.map((m) => commonChecked(m, { ...base, crossHairMode: "full" })), [false, true, false]);
  assert.deepEqual(modes.map((m) => commonChecked(m, { ...base, crossHairMode: "diag45" })), [false, false, true]);
});

test("the display toggles show their setting", () => {
  assert.equal(commonChecked("common.Control.toggleCursor", base), true);
  assert.equal(commonChecked("common.Control.toggleCursor", { ...base, alwaysShowCursor: false }), false);
  assert.equal(commonChecked("common.Control.toggleBoundingBoxes", base), false);
  assert.equal(commonChecked("common.Control.toggleBoundingBoxes", { ...base, drawBoundingBoxes: true }), true);
});

test("the selection modes: rectangle and lasso are one choice", () => {
  assert.equal(commonChecked("common.Interactive.selectSetRect", base), true);
  assert.equal(commonChecked("common.Interactive.selectSetLasso", base), false);
  assert.equal(commonChecked("common.Interactive.selectSetLasso", { ...base, selectionMode: "lasso" }), true);
});

test("an action with no check says so", () => {
  assert.equal(commonChecked("common.Control.zoomIn", base), undefined);
  assert.equal(commonChecked("common.Control.showLibraryTree", base), undefined, "the library tree's check is the dock layout's");
});
