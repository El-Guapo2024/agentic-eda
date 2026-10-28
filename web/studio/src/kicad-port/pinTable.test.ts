import { test } from "node:test";
import assert from "node:assert/strict";
import { addedPin, bodyStyleChoices, displayY, pinTableCommands, storedY, unitCellText, unitChoices } from "./pinTable";
import { changePinLength } from "./symPinSync";
import type { LibrarySymbolPin } from "../api/types";

const pin = (over: Partial<LibrarySymbolPin> = {}): LibrarySymbolPin => ({
  id: "p1",
  number: "1",
  name: "A",
  electrical_type: "input",
  shape: "line",
  at: { x: -5.08, y: 0 },
  angle_deg: 0,
  length_mm: 2.54,
  unit: 1,
  body_style: 1,
  hidden: false,
  name_size_mm: 1.27,
  number_size_mm: 1.27,
  ...over,
});

test("the Unit and Body Style cells offer ALL and what the symbol has (r7 E7-S10)", () => {
  assert.deepEqual(unitChoices({ unit_count: 2, kicad: { unit_names: { "2": "Power" } } }).map((c) => c.label), ["ALL", "Unit A", "Power"]);
  assert.deepEqual(unitChoices({ unit_count: 1 }).map((c) => c.value), [0, 1]);
  assert.deepEqual(bodyStyleChoices({ has_alternate_body_style: true }).map((c) => c.label), ["ALL", "Standard", "Alternate"]);
  assert.deepEqual(bodyStyleChoices({ has_alternate_body_style: false }).map((c) => c.label), ["ALL"]);
  assert.equal(unitCellText({ unit_count: 1 }, { unit: 1 }), "ALL", "a symbol of one unit shows ALL");
  assert.equal(unitCellText({ unit_count: 3 }, { unit: 2 }), "Unit B");
});

test("addRow copies the last pin and steps 100 mil (OnAddRow, r7 E7-S13)", () => {
  const empty = addedPin([]);
  assert.deepEqual([empty.number, empty.name, empty.electrical_type, empty.angle_deg, empty.unit, empty.body_style, empty.at.x, empty.at.y], ["", "", "unspecified", 0, 0, 0, 0, 0]);
  const left = addedPin([pin({ angle_deg: 0, electrical_type: "output", shape: "clock", unit: 2, body_style: 0, at: { x: -5.08, y: 2.54 } })]);
  assert.deepEqual([left.angle_deg, left.electrical_type, left.shape, left.unit, left.body_style, left.number], [0, "output", "clock", 2, 0, ""]);
  assert.deepEqual(left.at, { x: -5.08, y: 5.08 }, "`pos.y -= step` on KiCad's y-down axis is up the file's");
  const up = addedPin([pin({ angle_deg: 90, at: { x: 0, y: -7.62 } })]);
  assert.deepEqual(up.at, { x: 2.54, y: -7.62 });
});

test("Y is shown as KiCad's internal Y, the file's negated (r7 E7-S3)", () => {
  assert.equal(displayY(2.54), -2.54);
  assert.equal(storedY(5), -5);
  assert.equal(displayY(0), 0);
});

test("a pin length edit keeps the body end (ChangeLength, r7 E7-S2)", () => {
  const p = changePinLength(pin({ at: { x: -5.08, y: 0 }, angle_deg: 0, length_mm: 2.54 }), 5.08);
  assert.deepEqual(p.at, { x: -7.62, y: 0 });
});

test("OK is one batch of the verbs that make the pins the rows, with no sync (r7 E7-S11, S12)", () => {
  const a = pin({ id: "a", number: "1" });
  const b = pin({ id: "b", number: "2" });
  const c = pin({ id: "c", number: "3" });
  const rows = [{ ...a, name: "X" }, c, { ...addedPin([c]), number: "4" }];
  const cmds = pinTableCommands("L:S", [a, b, c], rows);
  assert.deepEqual(cmds.map((x) => x.op), ["delete_symbol_pin", "edit_symbol_pin", "add_symbol_pin"]);
  assert.equal((cmds[1] as { id: string }).id, "a");
  assert.deepEqual(pinTableCommands("L:S", [a, b], [a, b]), [], "an unchanged table sends nothing");
});
