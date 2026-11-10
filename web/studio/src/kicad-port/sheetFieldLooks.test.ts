import { test } from "node:test";
import assert from "node:assert/strict";
import { newSheetFieldRow, sheetFieldCmds, sheetFieldError, sheetFieldRow, SHEET_FIELD_COLUMNS } from "./sheetFieldLooks";

test("a sheet's user field row reads its looks; a new one is Field<N>, hidden, at the corner, at the Sheetname's angle (OnAddField)", () => {
  const r = sheetFieldRow({ name: "Rev", text: "A", size_nm: 2_540_000, bold: true, name_shown: true }, { dx: 1_270_000, dy: -2_540_000, angle: 90_000, h: "left", v: "top", font: "KiCad Font", color: { r: 255, g: 0, b: 0, a: 255 } });
  assert.deepEqual(r.look, { nameShown: true, h: "left", v: "top", italic: false, bold: true, sizeMm: 2.54, font: "KiCad Font", color: "#ff0000", vertical: true, xMm: 1.27, yMm: -2.54 });
  const n = newSheetFieldRow([r], false);
  assert.equal(n.name, "Field3");
  assert.equal(n.visible, false);
  assert.deepEqual([n.look.xMm, n.look.yMm, n.look.vertical, n.look.h, n.look.v, n.look.font, n.look.color], [0, 0, false, "center", "center", "Default Font", ""]);
  assert.equal(newSheetFieldRow([], true).look.vertical, true);
  assert.ok(!SHEET_FIELD_COLUMNS.some((c) => c.key === "allow_autoplace"));
});

test("OK sends set_sheet_fields for texts and styles, then set_sheet_field_looks for the places, fonts and colours that changed", () => {
  const was = [{ name: "Rev", text: "A" }];
  const rows = [sheetFieldRow(was[0]!, undefined)];
  assert.deepEqual(sheetFieldCmds("s1", rows, was, {}), [], "nothing changed, nothing sent");
  rows[0]!.look = { ...rows[0]!.look, bold: true, xMm: 2.54, h: "right", color: "#00ff00" };
  const cmds = sheetFieldCmds("s1", rows, was, {}) as unknown as Array<Record<string, unknown>>;
  assert.equal(cmds.length, 2);
  assert.equal(cmds[0]!.verb, "set_sheet_fields");
  assert.deepEqual(cmds[0]!.fields, [{ name: "Rev", text: "A", visible: true, italic: false, size_nm: 0, bold: true, name_shown: false }]);
  assert.equal(cmds[1]!.verb, "set_sheet_field_looks");
  assert.deepEqual(cmds[1]!.fields, [{ name: "Rev", dx: 2_540_000, dy: 0, angle: 0, h: "right", v: "center", font: "", color: { r: 0, g: 255, b: 0, a: 255 } }]);
  // a renamed field's looks start from none: a look of its own is sent under the new name
  const renamed = [{ ...sheetFieldRow(was[0]!, { dx: 1_270_000 }), name: "Revision" }];
  const r2 = sheetFieldCmds("s1", renamed, was, { Rev: { dx: 1_270_000 } }) as unknown as Array<Record<string, unknown>>;
  assert.deepEqual(r2.map((c) => c.verb), ["set_sheet_fields", "set_sheet_field_looks"]);
  assert.equal(sheetFieldError([{ ...rows[0]!, look: { ...rows[0]!.look, sizeMm: 0 } }]), "The text size of the Rev field must be between 0.01 and 1000 mm.");
});
