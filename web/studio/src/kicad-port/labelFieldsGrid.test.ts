import { test } from "node:test";
import assert from "node:assert/strict";
import { labelFieldEditOf, labelGridRows, labelLookCmds, labelPlaceCmds, newLabelGridRow, positioningChanged } from "./labelFieldsGrid";
import type { SchLabelField } from "../api/types";

const AT: [number, number] = [10_160_000, 10_160_000];
const isr: SchLabelField = { name: "Intersheetrefs", text: "${INTERSHEET_REFS}", shown: "[1]", visible: false, italic: false, intersheet_refs: true, at: [AT[0] + 8_000_000, AT[1]], vertical: false, h: "left", v: "center", size_nm: 1_270_000, id: "fld:g:Intersheetrefs", owner: "g" };
const nc: SchLabelField = { name: "Netclass", text: "HV", shown: "HV", visible: true, italic: true, intersheet_refs: false, at: [AT[0], AT[1] + 1_270_000], vertical: false, h: "left", v: "center", size_nm: 1_270_000, bold: false, name_shown: false, id: "fld:g:Netclass", owner: "g", allow_autoplace: true };

test("the grid's rows: X / Y from the label's anchor, Orientation, Font, Color, Allow Autoplacement (UI-LEFT-3 S3-31)", () => {
  const [a, b] = labelGridRows([isr, nc], AT);
  assert.equal(a!.isr, true);
  assert.deepEqual([b!.look.xMm, b!.look.yMm, b!.look.vertical, b!.look.font, b!.look.color, b!.look.allowAutoplace], [0, 1.27, false, "Default Font", "", true]);
  assert.deepEqual(labelFieldEditOf(b!), { name: "Netclass", text: "HV", visible: true, italic: true, size_nm: 1_270_000, bold: false, name_shown: false });
  assert.equal(labelFieldEditOf({ ...b!, look: { ...b!.look, allowAutoplace: false } }).no_autoplace, true);
});

test("OK: a moved field's edit_field by the name it had, before edit_label; the Sheet References' looks in theirs", () => {
  const rows = labelGridRows([isr, nc], AT);
  assert.deepEqual(labelPlaceCmds("g", rows), [], "nothing moved");
  const moved = rows.map((r) => (r.isr ? { ...r, look: { ...r.look, bold: true, sizeMm: 2 } } : { ...r, name: "Net Class", look: { ...r.look, xMm: 5.08, vertical: true, h: "right" as const } }));
  assert.equal(positioningChanged(moved[1]!.look, moved[1]!.wasLook), true);
  assert.deepEqual(labelPlaceCmds("g", moved), [
    { op: "sch_edit", verb: "edit_field", id: "fld:g:Intersheetrefs", bold: true, size_nm: 2_000_000 },
    { op: "sch_edit", verb: "edit_field", id: "fld:g:Netclass", local_at: { x: 5_080_000, y: 1_270_000 }, local_vertical: true, h: "right", v: "center" },
  ]);
});

test("OK: fonts and colours after edit_label, by the field's name then; a new field's H / V Align", () => {
  const rows = labelGridRows([isr, nc], AT);
  const changed = [{ ...rows[0]!, look: { ...rows[0]!.look, color: "#ff0000" } }, { ...rows[1]!, name: "Net Class", look: { ...rows[1]!.look, font: "KiCad Font" } }, { ...newLabelGridRow(9, { name: "Note", text: "x", visible: true, italic: false }), look: { ...newLabelGridRow(9, { name: "Note", text: "x", visible: true, italic: false }).look, h: "right" as const } }];
  assert.deepEqual(labelLookCmds("g", changed), [
    { op: "sch_edit", verb: "set_field_looks", ids: ["fld:g:Intersheetrefs"], color: { r: 255, g: 0, b: 0, a: 255 } },
    { op: "sch_edit", verb: "set_field_looks", ids: ["fld:g:Netclass"], font: "KiCad Font" },
    { op: "sch_edit", verb: "edit_field", id: "fld:g:Note", h: "right", v: "center" },
  ]);
  assert.deepEqual(labelPlaceCmds("g", changed), [], "a new row has no place until OK");
});
