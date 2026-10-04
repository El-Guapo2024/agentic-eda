import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultSheetFile, MIN_SHEET_HEIGHT_UM, MIN_SHEET_WIDTH_UM, sheetSize, uniqueSheetName } from "./schSheet";

const GRID = 1270; // 50 mil

test("sheetSize: the cursor minus the corner, on the grid", () => {
  assert.deepEqual(sheetSize([0, 0], [25_400, 12_700], GRID), [25_400, 12_700]);
  // the far corner snaps to the grid, not the size
  assert.deepEqual(sheetSize([1_270, 2_540], [30_000, 20_000], GRID), [29_210, 17_780]);
});

test("sheetSize: never below 500 x 150 mil -- dragging up or left gives the minimum, not a flipped sheet", () => {
  assert.deepEqual(sheetSize([10_000, 10_000], [0, 0], GRID), [Math.round((10_000 + MIN_SHEET_WIDTH_UM) / GRID) * GRID - 10_000, Math.round((10_000 + MIN_SHEET_HEIGHT_UM) / GRID) * GRID - 10_000]);
  const [w, h] = sheetSize([0, 0], [100, 100], GRID);
  assert.ok(w >= MIN_SHEET_WIDTH_UM - GRID / 2 && h >= MIN_SHEET_HEIGHT_UM - GRID / 2, `${w} x ${h}`);
  assert.ok(w > 0 && h > 0);
});

test("sheetSize: no grid leaves the size as is", () => {
  assert.deepEqual(sheetSize([0, 0], [13_000, 5_000], 0), [13_000, 5_000]);
});

test("uniqueSheetName counts up from the default", () => {
  assert.equal(uniqueSheetName([]), "Untitled Sheet");
  assert.equal(uniqueSheetName(["Untitled Sheet"]), "Untitled Sheet 2");
  assert.equal(uniqueSheetName(["Untitled Sheet", "Untitled Sheet 2"]), "Untitled Sheet 3");
  assert.equal(uniqueSheetName(["Power"], "Power"), "Power 2");
});

test("defaultSheetFile makes a file name from the sheet name", () => {
  assert.equal(defaultSheetFile("Power Supply"), "power_supply.kicad_sch");
  assert.equal(defaultSheetFile("  USB-C / PD  "), "usb_c_pd.kicad_sch");
  assert.equal(defaultSheetFile("???"), "untitled.kicad_sch");
});
