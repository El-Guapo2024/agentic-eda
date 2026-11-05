import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_SYMBOL_TEXT_SIZE_MM, libTextBoxNm, noPrintableChars, textAngleDeg, textFormOf, textFromForm, textSizeError, type LibText } from "./symText";

test("a text of only blanks has no printable characters", () => {
  assert.equal(noPrintableChars(""), true);
  assert.equal(noPrintableChars("  \t "), true);
  assert.equal(noPrintableChars(" A "), false);
});

test("the text size must stay visible: 0.01 mm to 1000 mm", () => {
  assert.equal(textSizeError(DEFAULT_SYMBOL_TEXT_SIZE_MM), null);
  assert.equal(textSizeError(0.01), null);
  assert.equal(textSizeError(1000), null);
  assert.notEqual(textSizeError(0.009), null);
  assert.notEqual(textSizeError(1000.1), null);
  assert.notEqual(textSizeError(Number.NaN), null);
});

test("the dialog offers horizontal and vertical text only", () => {
  assert.equal(textAngleDeg(false), 0);
  assert.equal(textAngleDeg(true), 90);
});

test("DIALOG_TEXT_PROPERTIES of a symbol's text: the form is the text's looks, OK gives them back with the unit and body style the editor shows unless common", () => {
  const t: LibText = { kind: "text", id: "t1", unit: 2, body_style: 1, text: "IN", at: { x: 1, y: 2 }, angle_deg: 90, size_mm: 1.27, bold: true, h_align: "left" };
  const sym = { unit_count: 2, has_alternate_body_style: false };
  const form = textFormOf(t, sym);
  assert.deepEqual(form, { text: "IN", size: "1.27", bold: true, italic: false, h: "left", v: "center", vertical: true, commonUnits: false, commonStyles: false });
  assert.equal(textFormOf({ ...t, unit: 0 }, sym).commonUnits, true);
  assert.equal(textFormOf({ ...t, unit: 0 }, { unit_count: 1, has_alternate_body_style: false }).commonUnits, false, "a symbol of one unit: unchecked");
  const out = textFromForm(t, { ...form, text: "OUT", size: "2", italic: true, h: "right", v: "bottom", vertical: false, commonUnits: true }, 2, 1);
  assert.ok(out.ok);
  if (out.ok) assert.deepEqual(out.text, { ...t, text: "OUT", size_mm: 2, bold: true, italic: true, h_align: "right", v_align: "bottom", angle_deg: 0, unit: 0, body_style: 1 });
  const notCommon = textFromForm({ ...t, unit: 0, body_style: 0 }, form, 2, 1);
  assert.ok(notCommon.ok && notCommon.text.unit === 2 && notCommon.text.body_style === 1, "unchecked: the unit and body style being edited");
  assert.deepEqual(textFromForm(t, { ...form, size: "0" }, 1, 1), { ok: false, error: "Text size must be between 0.01 mm and 1000 mm." });
});

test("SCH_TEXT::GetBoundingBox of a symbol's text: a left-top text lies right of and below its anchor, a vertical one runs up the sheet", () => {
  const measure = (text: string, sizeNm: number) => text.length * sizeNm;
  const t: LibText = { kind: "text", unit: 0, body_style: 1, text: "AB", at: { x: 1, y: 2 }, angle_deg: 0, size_mm: 1, h_align: "left", v_align: "top" };
  const b = libTextBoxNm(t, measure);
  assert.ok(b.minX >= 1_000_000 - 200_000 && b.minX < 1_100_000 && b.maxX > 3_000_000 - 1, `right of the anchor: ${JSON.stringify(b)}`);
  assert.ok(b.minY >= -2_000_000 - 200_000 && b.maxY > -2_000_000 + 900_000, `below it (+y down): ${JSON.stringify(b)}`);
  const v = libTextBoxNm({ ...t, angle_deg: 90 }, measure);
  assert.ok(v.maxY <= -2_000_000 + 200_000 && v.minY < -2_000_000 - 1_900_000, `a vertical text runs up from its anchor: ${JSON.stringify(v)}`);
  const c = libTextBoxNm({ ...t, h_align: "center", v_align: "center" }, measure);
  assert.ok(c.minX < 1_000_000 && c.maxX > 1_000_000 && c.minY < -2_000_000 && c.maxY > -2_000_000, "a centred text is round its anchor");
});
