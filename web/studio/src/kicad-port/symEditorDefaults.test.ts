import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PREFERENCES } from "./preferences";
import { borderOnWidthMm, globalTemplateRows, newTextSizeMm, pinTemplateWithDefaults, withoutEmptyTemplateRows } from "./symEditorDefaults";
import { DEFAULT_PIN_TEMPLATE } from "./symPinTemplate";
import { newUserField } from "./symFieldsGrid";

test("the first pin takes the defaults (GetLastPinLength's delayed initialization), a later one the last pin's", () => {
  const d = { ...DEFAULT_PREFERENCES, symDefaultPinLengthMils: 150, symDefaultPinNameSizeMils: 40, symDefaultPinNumSizeMils: 30 };
  const first = pinTemplateWithDefaults(DEFAULT_PIN_TEMPLATE, d);
  assert.deepEqual([first.length_mm, first.name_size_mm, first.number_size_mm], [3.81, 1.016, 0.762]);
  const edited = { ...first, length_mm: 5.08 };
  assert.equal(pinTemplateWithDefaults(edited, d), edited);
  // KiCad's defaults give the template KiCad starts with
  assert.deepEqual(pinTemplateWithDefaults(DEFAULT_PIN_TEMPLATE, DEFAULT_PREFERENCES), DEFAULT_PIN_TEMPLATE);
});

test("text size and border width in mm", () => {
  assert.equal(newTextSizeMm({ symDefaultTextSizeMils: 60 }), 1.524);
  assert.equal(borderOnWidthMm({ symDefaultLineWidthMils: 0 }), 0);
  assert.equal(borderOnWidthMm({ symDefaultLineWidthMils: 10 }), 0.254);
});

test("Symbol Properties: a row for each global template the symbol lacks, as it is shown; one left empty is not kept", () => {
  let key = 10;
  const rows = [{ ...newUserField(4, 1), name: "MPN", text: "X" }];
  const out = globalTemplateRows(rows, [{ name: "MPN", visible: true, url: false }, { name: "Supplier", visible: true, url: false }], () => ++key);
  assert.deepEqual(out.rows.map((r) => [r.name, r.hidden, r.key]), [["Supplier", false, 11]]);
  assert.deepEqual([...out.added], ["Supplier"]);
  const all = [...rows, ...out.rows];
  assert.deepEqual(withoutEmptyTemplateRows(all, out.added).map((r) => r.name), ["MPN"]);
  assert.deepEqual(withoutEmptyTemplateRows([...rows, { ...out.rows[0]!, text: "Digikey" }], out.added).map((r) => r.name), ["MPN", "Supplier"]);
});
