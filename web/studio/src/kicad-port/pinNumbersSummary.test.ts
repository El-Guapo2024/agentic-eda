import { test } from "node:test";
import assert from "node:assert/strict";
import { comparePinNumbers, padNumberSummary } from "./pinNumbersSummary";

test("comparePinNumbers: numeric runs compare as numbers, one apart is adjacent", () => {
  assert.equal(comparePinNumbers("1", "2"), -1);
  assert.equal(comparePinNumbers("2", "1"), 1);
  assert.equal(comparePinNumbers("1", "3"), -2);
  assert.equal(comparePinNumbers("10", "9"), 1, "numeric, not alphabetical");
  assert.equal(comparePinNumbers("5", "5"), 0);
  assert.equal(comparePinNumbers("A1", "A2"), -1);
  assert.equal(comparePinNumbers("A1", "A10"), -2);
  assert.equal(comparePinNumbers("1", "A1"), -2, "a number sorts before text");
  assert.equal(comparePinNumbers("A1", "1"), 2);
  assert.equal(comparePinNumbers("1v8", "1.8"), 0, "`v` stands for the decimal point");
});

test("padNumberSummary collapses runs and lists duplicates (PIN_NUMBERS::GetSummary / GetDuplicates)", () => {
  assert.deepEqual(padNumberSummary(["1", "2", "3", "5"]), { summary: "1-3,5", duplicates: "none" });
  assert.deepEqual(padNumberSummary(["3", "1", "2"]), { summary: "1-3", duplicates: "none" }, "sorted first");
  assert.deepEqual(padNumberSummary(["1", "3", "5"]), { summary: "1,3,5", duplicates: "none" });
  assert.deepEqual(padNumberSummary(["7"]), { summary: "7", duplicates: "none" });
  assert.deepEqual(padNumberSummary([]), { summary: "", duplicates: "none" });
  assert.deepEqual(padNumberSummary(["1", "1", "2", "2", "2", "3"]), { summary: "1-3", duplicates: "1,2" });
  assert.deepEqual(padNumberSummary(["", "1", ""]), { summary: "1", duplicates: "none" }, "an empty number is not counted");
});

test("padNumberSummary on alphanumeric numbers (a BGA's A1..B2)", () => {
  assert.equal(padNumberSummary(["A1", "A2", "A3"]).summary, "A1-A3");
  assert.equal(padNumberSummary(["A1", "A3"]).summary, "A1,A3");
  // KiCad's own quirk: two different text prefixes compare as adjacent (`wxString::Cmp` is -1), so A3 -> B1 does not break the range.
  assert.equal(padNumberSummary(["A1", "A2", "B1"]).summary, "A1-B1");
});
