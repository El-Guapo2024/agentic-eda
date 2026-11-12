import { test } from "node:test";
import assert from "node:assert/strict";
import { autoDecodeCsv, colTypeForString, importedPinList, importPins } from "./pinTableImport";
import { COL, COL_COUNT, csvText, exportTable, parseTypedLength, type GridContext } from "./pinTableGrid";
import type { LibrarySymbolPin } from "../api/types";

const ctx: GridContext = {
  sym: { unit_count: 2, has_alternate_body_style: false, kicad: {} },
  formatLength: (mm, withUnits) => `${Math.round(mm * 1e4) / 1e4}${withUnits ? " mm" : ""}`,
  parseLength: (t) => parseTypedLength(t, "mm"),
};

test("AutoDecodeCSV: commas or, with a tab anywhere, tabs; trimmed cells, quotes, # comments and empty lines skipped, rows as wide as the first", () => {
  assert.deepEqual(autoDecodeCsv('Number, Name\n# a comment\n\n"1", "A, B"\n2,"say ""hi"""\n3\n'), [
    ["Number", "Name"],
    ["1", "A, B"],
    ["2", 'say "hi"'],
    ["3", ""],
  ]);
  assert.deepEqual(autoDecodeCsv("Number\tName\r\n1\tX, Y\r\n"), [
    ["Number", "Name"],
    ["1", "X, Y"],
  ]);
  assert.equal(autoDecodeCsv(""), null);
});

test("GetColTypeForString: the Pin Table's labels in any case, else COL_COUNT", () => {
  assert.equal(colTypeForString("electrical type"), COL.TYPE);
  assert.equal(colTypeForString("Y Position"), COL.POSY);
  assert.equal(colTypeForString("Colour"), COL_COUNT);
});

test("OnImportButtonClick: a new pin per row from SCH_PIN's defaults, each known cell put in by UpdatePin, unknown columns and bad booleans reported", () => {
  const table = autoDecodeCsv("Number,Name,Electrical Type,Orientation,Length,X Position,Y Position,Visible,Unit,Colour\n5,CLK,Input,Left,100mil,1.27,-2.54,maybe,Unit B,red\n6,D\n")!;
  const { pins, messages } = importPins(table, ctx);
  // the short row is as wide as the header (AutoDecodeCSV): its empty Visible is reported too, its empty Length is 0 (ValueFromString of "")
  assert.deepEqual(messages, [
    "Unknown columns in data: Colour. These columns will be ignored.",
    "The value 'maybe' can't be converted to boolean correctly, it has been interpreted as 'False'",
    "The value '' can't be converted to boolean correctly, it has been interpreted as 'False'",
  ]);
  assert.equal(pins.length, 2);
  const p = pins[0]!;
  assert.deepEqual([p.number, p.name, p.electrical_type, p.angle_deg, p.length_mm, p.at.x, p.at.y, p.hidden, p.unit], ["5", "CLK", "input", 180, 2.54, 1.27, 2.54, true, 2]);
  const q = pins[1]!;
  assert.deepEqual([q.number, q.name, q.electrical_type, q.shape, q.length_mm, q.unit, q.body_style, q.at.x, q.at.y], ["6", "D", "unspecified", "line", 0, 0, 0, 0, 0]);
  // a header alone imports nothing
  assert.deepEqual(importPins([["Number"]], ctx), { pins: [], messages: [] });
});

test("an exported table imports back to the same pins (PIN_TABLE_EXPORT then PIN_TABLE_IMPORT)", () => {
  const pin: LibrarySymbolPin = { number: "7", name: "~{RST}", electrical_type: "open_collector", shape: "inverted", at: { x: -7.62, y: 5.08 }, angle_deg: 0, length_mm: 3.81, unit: 1, body_style: 0, hidden: true, name_size_mm: 1, number_size_mm: 1.5 };
  const { pins, messages } = importPins(autoDecodeCsv(csvText(exportTable([pin], ctx))), ctx);
  assert.deepEqual(messages, []);
  assert.deepEqual(pins[0], pin);
});

test("Replace all existing pins, or Append to existing pins", () => {
  assert.deepEqual(importedPinList([1, 2], [3], true), [3]);
  assert.deepEqual(importedPinList([1, 2], [3], false), [1, 2, 3]);
});
