import { test } from "node:test";
import assert from "node:assert/strict";
import { fieldsOnShow, hitLibField, libFieldId, libFieldName, movedRows, schFieldOf, unitLetters } from "./symFieldLayout";
import { allFields } from "./symFields";
import type { LibrarySymbol } from "../api/types";

const sym = (over: Partial<LibrarySymbol> = {}) => ({ lib_id: "L:R", reference_prefix: "R", datasheet: "", description: "", fields: [], ...over }) as unknown as LibrarySymbol;
const measure = (text: string, size: number) => text.length * size * 0.6;

test("a field is named for the canvas by fld:lib:<name>", () => {
  assert.equal(libFieldId("Reference"), "fld:lib:Reference");
  assert.equal(libFieldName("fld:lib:MPN"), "MPN");
  assert.equal(libFieldName("fld:U1:Reference"), null, "a sheet's field is not a library field");
  assert.equal(libFieldName("pin1"), null);
});

test("a library field is drawn in the editor's space: mm, +y up, becomes um, +y down, at its size and justification", () => {
  const f = allFields(sym({ fields: [{ name: "Reference", text: "R", at: { x: 1.5, y: 2.54 }, size_mm: 1.0, h_align: "left", v_align: "bottom", bold: true, angle_deg: 90 }] }))[0]!;
  const s = schFieldOf(f);
  assert.deepEqual([s.at, s.size_um, s.h, s.v, s.vertical, s.bold, s.visible, s.id], [[1500, -2540], 1000, "left", "bottom", true, true, true, "fld:lib:Reference"]);
});

test("the Reference of a multi-unit symbol has the unit's letter", () => {
  const f = allFields(sym())[0]!;
  assert.equal(schFieldOf(f, 2, 3).text, "RB");
  assert.equal(schFieldOf(f, 1, 1).text, "R");
  assert.deepEqual([1, 2, 26, 27, 28].map(unitLetters), ["A", "B", "Z", "AA", "AB"]);
});

test("a hidden field is on show only with Show Hidden Fields, and a field with no text never", () => {
  const rows = allFields(sym({ fields: [{ name: "MPN", text: "X", hidden: true }, { name: "Empty", text: "" }] }));
  // the mandatory Footprint, Datasheet and Description are hidden by default and empty: nothing to draw
  assert.deepEqual(fieldsOnShow(rows, false).map((f) => f.name), ["Reference", "Value"]);
  assert.deepEqual(fieldsOnShow(rows, true).map((f) => f.name), ["Reference", "Value", "MPN"]);
});

test("a click picks the field under it, the smallest box of several, and nothing elsewhere", () => {
  const rows = allFields(sym({ fields: [{ name: "Reference", text: "R", at: { x: 0, y: 5.08 } }, { name: "Value", text: "10k", at: { x: 0, y: -5.08 } }] }));
  const shown = fieldsOnShow(rows, false);
  assert.equal(hitLibField(shown, 0, -5080, 100, measure), "fld:lib:Reference");
  assert.equal(hitLibField(shown, 0, 5080, 100, measure), "fld:lib:Value");
  assert.equal(hitLibField(shown, 20000, 0, 100, measure), null);
});

test("a drag moves the named fields by the offset, in the symbol's frame", () => {
  const rows = allFields(sym());
  const moved = movedRows(rows, new Set(["Reference"]), 1.27, -2.54);
  assert.deepEqual([moved[0]!.at, moved[1]!.at], [{ x: 1.27, y: 2.54 }, rows[1]!.at], "Reference was at (0, 5.08)");
});
