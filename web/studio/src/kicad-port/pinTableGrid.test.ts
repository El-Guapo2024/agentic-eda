import { test } from "node:test";
import assert from "node:assert/strict";
import { COL, COLUMN_LABELS, csvText, DEFAULT_SHOWN_COLUMNS, exportTable, INDETERMINATE_STATE, nextSort, NO_FILTERS, parseTypedLength, pinTableSummary, rebuildRows, renumberGroup, rowValue, updatePin, type GridContext, type TablePin } from "./pinTableGrid";
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

const ctx = (unitCount = 1, demorgan = false): GridContext => ({
  sym: { unit_count: unitCount, has_alternate_body_style: demorgan, kicad: { unit_names: { "2": "Power" } } },
  formatLength: (mm, withUnits) => `${Math.round(mm * 1e4) / 1e4}${withUnits ? " mm" : ""}`,
  parseLength: (t) => parseTypedLength(t, "mm"),
});

const tp = (key: string, over: Partial<LibrarySymbolPin>): TablePin => ({ key, pin: pin({ id: key, ...over }) });

test("KiCad's fourteen columns, eight shown to begin with (ShowHideColumns \"0 1 2 3 4 5 9 10\")", () => {
  assert.equal(COLUMN_LABELS.length, 14);
  assert.deepEqual(
    DEFAULT_SHOWN_COLUMNS.map((c) => COLUMN_LABELS[c]),
    ["Count", "Number", "Name", "Electrical Type", "Graphic Style", "Orientation", "X Position", "Y Position"]
  );
});

test("a cell shows KiCad's names, the frame's lengths, 1 / 0 and the unit's display name (PIN_INFO_FORMATTER::Format, r7 E7-S15)", () => {
  const p = pin({ electrical_type: "power_in", shape: "inverted_clock", angle_deg: 180, at: { x: 1, y: 2.54 }, hidden: true, unit: 2 });
  const c = ctx(2);
  assert.equal(rowValue([p], COL.TYPE, c), "Power input");
  assert.equal(rowValue([p], COL.SHAPE, c), "Inverted clock");
  assert.equal(rowValue([p], COL.ORIENTATION, c), "Left");
  assert.equal(rowValue([p], COL.POSY, c), "-2.54 mm", "KiCad's internal Y, the file's negated");
  assert.equal(rowValue([p], COL.NUMBER_SIZE, c), "1.27 mm");
  assert.equal(rowValue([p], COL.VISIBLE, c), "0");
  assert.equal(rowValue([p], COL.UNIT, c), "Power", "the unit's display name");
  assert.equal(rowValue([pin({ unit: 1 })], COL.UNIT, c), "Unit A");
  assert.equal(rowValue([pin({ unit: 2 })], COL.UNIT, ctx(1)), "ALL", "a symbol of one unit says ALL");
  assert.equal(rowValue([pin({ body_style: 2 })], COL.BODY_STYLE, ctx(1, true)), "Alternate");
  assert.equal(rowValue([p, p], COL.PIN_COUNT, c), "2");
});

test("a grouped row joins its numbers and says -- mixed values -- where its pins differ (GetValue)", () => {
  const a = pin({ number: "1", electrical_type: "input" });
  const b = pin({ number: "3", electrical_type: "output" });
  assert.equal(rowValue([a, b], COL.NUMBER, ctx()), "1, 3");
  assert.equal(rowValue([a, b], COL.TYPE, ctx()), INDETERMINATE_STATE);
  assert.equal(rowValue([a, b], COL.NAME, ctx()), "A");
});

test("rows: Count then Number by PIN_NUMBERS::Compare, Group by name, the filters as KiCad has them (RebuildRows, SortRows)", () => {
  const pins = [tp("a", { number: "10", name: "GND" }), tp("b", { number: "2", name: "VCC" }), tp("c", { number: "1", name: "GND" }), tp("d", { number: "A1", name: "X", unit: 0 })];
  const flat = rebuildRows(pins, false, NO_FILTERS, COL.PIN_COUNT, true, ctx());
  assert.deepEqual(flat, [["c"], ["b"], ["a"], ["d"]], "1, 2, 10, A1: numbers before letters");
  const grouped = rebuildRows(pins, true, NO_FILTERS, COL.PIN_COUNT, true, ctx());
  assert.deepEqual(grouped, [["b"], ["d"], ["c", "a"]], "the one-pin rows first (Count 1, then by Number), the two GND pins one row sorted by number");
  const byName = rebuildRows(pins, false, NO_FILTERS, COL.NAME, false, ctx());
  assert.deepEqual(byName.map((r) => r[0]), ["d", "b", "a", "c"], "Name descending, ties by Number descending");
  // a unit filter keeps that unit and the pins common to all units; "ALL" (index 0) keeps the common ones only
  const units = [tp("u1", { unit: 1 }), tp("u2", { unit: 2, number: "2" }), tp("u0", { unit: 0, number: "3" })];
  assert.deepEqual(rebuildRows(units, false, { ...NO_FILTERS, unit: 2 }, COL.PIN_COUNT, true, ctx(2)).flat(), ["u2", "u0"]);
  assert.deepEqual(rebuildRows(units, false, { ...NO_FILTERS, unit: 0 }, COL.PIN_COUNT, true, ctx(2)).flat(), ["u0"]);
  // a body style filter keeps exactly that style
  const styles = [tp("s1", { body_style: 1 }), tp("s0", { body_style: 0, number: "2" })];
  assert.deepEqual(rebuildRows(styles, false, { ...NO_FILTERS, bodyStyle: 1 }, COL.PIN_COUNT, true, ctx(1, true)).flat(), ["s1"]);
  // the selection filter: a copy whose number, name, unit and body style a selected pin has
  assert.deepEqual(rebuildRows(pins, false, { ...NO_FILTERS, selection: [pin({ number: "2", name: "VCC" })] }, COL.PIN_COUNT, true, ctx()).flat(), ["b"]);
});

test("a header click sorts by it, a second click turns the order round (OnColSort)", () => {
  assert.deepEqual(nextSort(null, COL.NAME), { col: COL.NAME, ascending: true });
  assert.deepEqual(nextSort({ col: COL.NAME, ascending: true }, COL.NAME), { col: COL.NAME, ascending: false });
  assert.deepEqual(nextSort({ col: COL.NAME, ascending: false }, COL.LENGTH), { col: COL.LENGTH, ascending: true });
});

test("the summary: Pin numbers, Pin count and Duplicate pins over every pin (updateSummary)", () => {
  const s = pinTableSummary([tp("a", { number: "1" }), tp("b", { number: "2" }), tp("c", { number: "3" }), tp("d", { number: "3" }), tp("e", { number: "" }), tp("f", { number: "A1" })]);
  assert.deepEqual(s, { numbers: "1-3,A1", count: "6", duplicates: "3" });
  assert.deepEqual(pinTableSummary([tp("a", { number: "1" })]).duplicates, "none");
});

test("an edit goes into each pin as UpdatePin reads it: names, units, a length that keeps the body end", () => {
  const c = ctx(2, true);
  assert.equal(updatePin(pin(), "power output", COL.TYPE, c).electrical_type, "power_out", "the name, any case");
  assert.equal(updatePin(pin(), "nonsense", COL.TYPE, c).electrical_type, "input", "a name that is no choice changes nothing");
  assert.equal(updatePin(pin(), "Up", COL.ORIENTATION, c).angle_deg, 90);
  assert.equal(updatePin(pin(), "Power", COL.UNIT, c).unit, 2, "the unit's display name");
  assert.equal(updatePin(pin(), "A", COL.UNIT, c).unit, 1, "or its letter");
  assert.equal(updatePin(pin(), "ALL", COL.UNIT, c).unit, 0);
  assert.equal(updatePin(pin({ unit: 1 }), "Unit B", COL.UNIT, ctx(1)).unit, 1, "a symbol of one unit takes nothing");
  assert.equal(updatePin(pin(), "Alternate", COL.BODY_STYLE, c).body_style, 2);
  assert.equal(updatePin(pin(), "0", COL.VISIBLE, c).hidden, true);
  assert.equal(updatePin(pin(), "True", COL.VISIBLE, c).hidden, false);
  assert.equal(updatePin(pin(), "-3 mm", COL.POSY, c).at.y, 3, "a typed internal Y is stored negated");
  assert.equal(updatePin(pin(), "100 mil", COL.NAME_SIZE, c).name_size_mm, 2.54);
  const longer = updatePin(pin({ at: { x: -5.08, y: 0 }, angle_deg: 0, length_mm: 2.54 }), "5.08", COL.LENGTH, c);
  assert.equal(longer.length_mm, 5.08);
  assert.ok(Math.abs(longer.at.x - -7.62) < 1e-9, `ChangeLength keeps the body end: the connection point moves out (${longer.at.x})`);
});

test("a grouped row's Number renumbers its pins, adds a pin per extra number and drops the rest (SetValue with IsDisplayGrouped)", () => {
  let n = 0;
  const key = () => `new${++n}`;
  const row = [tp("a", { number: "1", name: "GND", unit: 2, body_style: 1 }), tp("b", { number: "2", name: "GND", at: { x: -5.08, y: -2.54 }, unit: 2, body_style: 1 })];
  const grown = renumberGroup(row, "4, 5,6", key);
  assert.deepEqual(grown.row.map((t) => t.pin.number), ["4", "5", "6"]);
  assert.equal(grown.added.length, 1);
  const added = grown.added[0]!.pin;
  assert.deepEqual([added.name, added.unit, added.body_style, added.at.x, added.at.y], ["GND", 2, 0, -5.08, 0], "the last pin's name and unit, a pin step on (KiCad's y up the file), body style 0");
  assert.deepEqual(grown.removed, []);
  const shrunk = renumberGroup(row, "7,", key);
  assert.deepEqual(shrunk.row.map((t) => t.pin.number), ["7"], "the empty token after a last comma is not one (wxTOKEN_RET_EMPTY)");
  assert.deepEqual(shrunk.removed, ["b"]);
  assert.deepEqual(renumberGroup(row, "7,,8", key).row.map((t) => t.pin.number), ["7", "", "8"], "an empty token in the middle is");
});

test("Export: every column but Count, no units, True / False, quoted CSV (PIN_TABLE_EXPORT::ExportData, CSV_WRITER)", () => {
  const t = exportTable([pin({ name: 'A"B' })], ctx());
  assert.equal(t[0]!.length, 13);
  assert.equal(t[0]![0], "Number");
  assert.deepEqual(t[1]!.slice(0, 5), ["1", 'A"B', "Input", "Line", "Right"]);
  assert.equal(t[1]![8], "-5.08", "X Position without its unit");
  assert.equal(t[1]![10], "True");
  assert.equal(csvText([["a", 'b"c']]), '"a","b""c"\n');
});
