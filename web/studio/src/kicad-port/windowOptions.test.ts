import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_WINDOW_OPTIONS, EDITOR_WINDOWS, editorWindowOf, parseWindowOptions } from "./windowOptions";

test("each editor keeps KiCad's window defaults: small crosshair always shown, dots, 1 px lines, 10 px spacing, no polar coordinates, snap Always, no axes", () => {
  assert.deepEqual(DEFAULT_WINDOW_OPTIONS, { crossHairMode: "small", alwaysShowCursor: true, gridStyle: "dots", gridLineWidth: 1, gridMinSpacing: 10, polarCoords: false, gridSnap: "always", gridAxes: false, drawBoundingBoxes: false });
  assert.deepEqual(EDITOR_WINDOWS, ["pcb", "schematic", "footprint", "symbol"]);
});

test("a saved window is read field by field, a bad field keeps the base -- an old save's one crosshair stands for every editor", () => {
  const legacy = parseWindowOptions({ crossHairMode: "full", alwaysShowCursor: false });
  assert.equal(legacy.crossHairMode, "full");
  const fp = parseWindowOptions({ gridStyle: "lines", gridLineWidth: 2.4, gridMinSpacing: 3, polarCoords: true, crossHairMode: "sideways", gridSnap: "with_grid", gridAxes: "yes" }, legacy);
  assert.deepEqual(fp, { crossHairMode: "full", alwaysShowCursor: false, gridStyle: "lines", gridLineWidth: 2.5, gridMinSpacing: 5, polarCoords: true, gridSnap: "with_grid", gridAxes: false, drawBoundingBoxes: false });
  assert.equal(parseWindowOptions({ gridSnap: "sometimes" }).gridSnap, "always", "grid.snap: ALWAYS, WITH_GRID or NEVER");
  assert.deepEqual(parseWindowOptions(null), DEFAULT_WINDOW_OPTIONS);
});

test("the tab's window: the four editors their own, the others the board editor's", () => {
  assert.deepEqual(["pcb", "schematic", "footprint", "symbol", "viewer3d"].map(editorWindowOf), ["pcb", "schematic", "footprint", "symbol", "pcb"]);
});

test("Show Bounding Boxes is the frame painter's own and never saved (`RENDER_SETTINGS::m_drawBoundingBoxes`, `COMMON_TOOLS::ToggleBoundingBoxes`)", () => {
  assert.equal(parseWindowOptions({ drawBoundingBoxes: true }).drawBoundingBoxes, false, "a session starts with it off");
  assert.equal(parseWindowOptions({}, { ...DEFAULT_WINDOW_OPTIONS, drawBoundingBoxes: true }).drawBoundingBoxes, false);
});
