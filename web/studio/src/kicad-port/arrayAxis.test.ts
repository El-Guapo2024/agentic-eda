import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ArrayAxis,
  NUMBERING_CHOICES,
  alphabeticFromIndex,
  alphabetOf,
  axisOf,
  gridCoords,
  itemNumber,
  providedNumbers,
  resolveNumbering,
  startAfterAlphabetChange,
  type NumberedArray,
  type NumberingType,
  type PadNumbering,
} from "./arrayAxis";

function axis(kind: NumberingType, start: string, step = 1): ArrayAxis {
  const a = new ArrayAxis();
  a.kind = kind;
  assert.ok(a.setOffsetText(start), `${start} decodes in ${kind}`);
  a.step = step;
  return a;
}
const run = (a: ArrayAxis, n: number) => Array.from({ length: n }, (_, i) => a.itemNumber(i));

// The values below are the ones crates/ops/src/array_numbering.rs asserts: the dialog's preview and the backend's answer are one algorithm.

test("the dialog offers KiCad's four alphabets in its order and words", () => {
  assert.deepEqual(
    NUMBERING_CHOICES.map((c) => c.label),
    ["Numerals (0,1,2,...,9,10)", "Hexadecimal (0,1,...,F,10,...)", "Alphabet, minus IOSQXZ", "Alphabet, full 26 characters"]
  );
  assert.equal(alphabetOf("alpha_no_iosqxz"), "ABCDEFGHJKLMNPRTUVWY");
});

test("numerals count from the start by the step", () => {
  assert.deepEqual(run(axis("numeric", "1"), 12), ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"]);
  assert.deepEqual(run(axis("numeric", "1", 2), 4), ["1", "3", "5", "7"]);
  assert.deepEqual(run(axis("numeric", "20", -5), 4), ["20", "15", "10", "5"]);
});

test("hexadecimal runs through F and on to 10, and takes capitals only", () => {
  assert.deepEqual(run(axis("hex", "E"), 4), ["E", "F", "10", "11"]);
  const hex = new ArrayAxis();
  hex.kind = "hex";
  assert.equal(hex.setOffsetText("a"), false);
  assert.equal(hex.setOffsetText("G"), false);
});

test("the full alphabet goes Z, AA: the column of tens starts at the first letter", () => {
  const a = axis("alpha_full", "A");
  assert.deepEqual([a.itemNumber(0), a.itemNumber(25), a.itemNumber(26), a.itemNumber(27), a.itemNumber(51), a.itemNumber(52), a.itemNumber(675)], ["A", "Z", "AA", "AB", "AZ", "BA", "YZ"]);
});

test("the BGA alphabet leaves out I, O, S, Q, X and Z", () => {
  const a = axis("alpha_no_iosqxz", "A");
  assert.equal(run(a, 20).join(""), "ABCDEFGHJKLMNPRTUVWY");
  assert.deepEqual([a.itemNumber(20), a.itemNumber(21), a.itemNumber(40)], ["AA", "AB", "BA"]);
});

test("a start is read in its alphabet: AA is 26, BC is 54, a numeral is no letter, I is not a BGA letter", () => {
  const a = new ArrayAxis();
  a.kind = "alpha_full";
  assert.ok(a.setOffsetText("AA"));
  assert.equal(a.offset, 26);
  assert.ok(a.setOffsetText("BC"));
  assert.equal(a.offset, 54);
  assert.equal(a.itemNumber(0), "BC");
  assert.equal(a.setOffsetText(""), false);
  assert.equal(a.setOffsetText("A1"), false);
  a.kind = "alpha_no_iosqxz";
  assert.equal(a.setOffsetText("I"), false);
  assert.ok(a.setOffsetText("J"));
  assert.equal(a.offset, 8);
});

test("a start typed in lowercase prints lowercase", () => {
  const a = axis("alpha_full", "b");
  assert.deepEqual(run(a, 3), ["b", "c", "d"]);
  assert.equal(a.itemNumber(24), "z");
  assert.equal(a.itemNumber(25), "aa");
  const mixed = new ArrayAxis();
  mixed.kind = "alpha_full";
  assert.ok(mixed.setOffsetText("aB"));
  assert.equal(mixed.itemNumber(0), "ab");
});

test("a negative index wraps as a size_t does", () => {
  const a = axis("numeric", "1", -1);
  assert.deepEqual([a.itemNumber(0), a.itemNumber(1), a.itemNumber(2)], ["1", "0", "18446744073709551615"]);
  assert.equal(alphabeticFromIndex(0, "ABC", true), "A");
});

test("changing the alphabet keeps a start that still decodes and otherwise starts the new alphabet over", () => {
  assert.equal(startAfterAlphabetChange("hex", "12"), "12");
  assert.equal(startAfterAlphabetChange("alpha_full", "12"), "A", "numerals are no letters: the first name of the new alphabet");
  assert.equal(startAfterAlphabetChange("numeric", "B"), "1", "numeric types start at 1");
  assert.equal(startAfterAlphabetChange("alpha_no_iosqxz", "I"), "A");
  assert.equal(startAfterAlphabetChange("alpha_no_iosqxz", "J"), "J");
});

const num = (over: Partial<PadNumbering> = {}): PadNumbering => ({
  start_specified: true,
  two_d: false,
  primary: { kind: "numeric", start: "1", step: 1 },
  secondary: { kind: "numeric", start: "1", step: 1 },
  ...over,
});
const gridOf = (nx: number, ny: number, over: Partial<NumberedArray> = {}): NumberedArray => ({ circular: false, nx, ny, horizontalThenVertical: true, reverseAlternate: false, ...over });

test("the entries are judged like the dialog does, every complaint on a line of its own", () => {
  const bad = resolveNumbering(num({ two_d: true, primary: { kind: "alpha_no_iosqxz", start: "I", step: 1 }, secondary: { kind: "hex", start: "Q", step: 1 } }), false);
  assert.deepEqual(bad, {
    ok: false,
    error:
      "Could not determine numbering start from 'I': expected value consistent with alphabet 'ABCDEFGHJKLMNPRTUVWY'.\nCould not determine numbering start from 'Q': expected value consistent with alphabet '0123456789ABCDEF'.",
  });
  // a circle has the one axis; "Use first free number" ignores the entries
  assert.equal(resolveNumbering(num({ two_d: true, secondary: { kind: "hex", start: "Q", step: 1 } }), true).ok, true);
  assert.equal(resolveNumbering(num({ start_specified: false, primary: { kind: "hex", start: "Q", step: 1 } }), false).ok, true);
  assert.equal(axisOf({ kind: "numeric", start: "x", step: 1 }).ok, false);
});

test("a grid numbered by coordinates names a point by its column and its row, in the order the grid is walked", () => {
  const r = resolveNumbering(num({ two_d: true, primary: { kind: "alpha_full", start: "A", step: 1 } }), false);
  assert.ok(r.ok);
  if (!r.ok) return;
  const names = (a: NumberedArray) => Array.from({ length: 6 }, (_, n) => itemNumber(a, r.axes, n));
  assert.deepEqual(names(gridOf(3, 2)), ["A1", "B1", "C1", "A2", "B2", "C2"]);
  assert.deepEqual(names(gridOf(3, 2, { reverseAlternate: true })), ["A1", "B1", "C1", "C2", "B2", "A2"], "the second row runs back");
  assert.deepEqual(names(gridOf(3, 2, { horizontalThenVertical: false })), ["A1", "B1", "A2", "B2", "A3", "B3"], "vertical first walks an axis of ny");
  assert.deepEqual(gridCoords(4, 3, 2, true, false), [1, 1]);
});

test("use first free number hands out the numerals that are not the number of a pad that stays", () => {
  const free = resolveNumbering(num({ start_specified: false }), false);
  assert.ok(free.ok);
  if (!free.ok) return;
  assert.deepEqual(providedNumbers(gridOf(4, 1), free.axes, new Set(["1", "3"]), 4), ["2", "4", "5", "6"]);
  const specified = resolveNumbering(num(), false);
  assert.ok(specified.ok);
  if (!specified.ok) return;
  assert.deepEqual(providedNumbers(gridOf(4, 1), specified.axes, new Set(["1", "3"]), 4), ["1", "2", "3", "4"], "with a start value the pads outside are not looked at");
});
