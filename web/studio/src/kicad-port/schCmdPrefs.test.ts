import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PREFERENCES, sanitizePreferences } from "./preferences";
import { schCmdPrefsOf } from "./schCmdPrefs";

test("a command carries the Symbol Field Automatic Placement and the new sheets' colours; KiCad's defaults are all on and no colour", () => {
  assert.deepEqual(schCmdPrefsOf(DEFAULT_PREFERENCES), { autoplace: { enable: true, allow_rejustify: true, align_to_grid: true }, sheet_colors: { border: null, background: null } });
  const red = { r: 200, g: 0, b: 0, a: 255 };
  assert.deepEqual(schCmdPrefsOf({ ...DEFAULT_PREFERENCES, schAutoplaceAlign: false, schSheetBorderColor: red }).sheet_colors.border, red);
});

test("the schematic editor's new preferences are KiCad's defaults and survive the store", () => {
  const d = DEFAULT_PREFERENCES;
  assert.deepEqual([d.schDragIsMove, d.schAutoStartWires, d.schLineMode, d.schAutoplaceFields, d.schAutoplaceJustify, d.schAutoplaceAlign, d.schSheetBorderColor, d.schRescueNeverShow], [false, true, 1, true, true, true, null, false]);
  assert.deepEqual([d.symDefaultLineWidthMils, d.symDefaultTextSizeMils, d.symDefaultPinLengthMils, d.symDefaultPinNumSizeMils, d.symDefaultPinNameSizeMils], [0, 50, 100, 50, 50]);
  const back = sanitizePreferences(JSON.parse(JSON.stringify({ ...d, schDragIsMove: true, schLineMode: 2, schSheetBackgroundColor: { r: 1, g: 2, b: 3, a: 255 }, schGlobalFieldNames: [{ name: "MPN", visible: true, url: false }], symDefaultPinLengthMils: 150 })));
  assert.equal(back.schDragIsMove, true);
  assert.equal(back.schLineMode, 2);
  assert.deepEqual(back.schSheetBackgroundColor, { r: 1, g: 2, b: 3, a: 255 });
  assert.deepEqual(back.schGlobalFieldNames, [{ name: "MPN", visible: true, url: false }]);
  assert.equal(back.symDefaultPinLengthMils, 150);
  // anything malformed is the default
  const bad = sanitizePreferences({ schLineMode: 7, schSheetBorderColor: { r: 300 }, schGlobalFieldNames: "x", symDefaultTextSizeMils: -3 });
  assert.deepEqual([bad.schLineMode, bad.schSheetBorderColor, bad.schGlobalFieldNames, bad.symDefaultTextSizeMils], [1, null, [], 1]);
});
