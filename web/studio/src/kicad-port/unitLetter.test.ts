import { test } from "node:test";
import assert from "node:assert/strict";
import { letterSubReference, subReference } from "./unitLetter";

test("LetterSubReference counts like a spreadsheet from the initial letter", () => {
  assert.deepEqual([1, 2, 26, 27, 28].map((u) => letterSubReference(u)), ["A", "B", "Z", "AA", "AB"]);
  assert.equal(letterSubReference(3, 97), "c");
});

test("SCHEMATIC_SETTINGS::SubReference: separator, then the number for a digit first id, else letters; nothing below unit 1", () => {
  assert.equal(subReference(2), "B");
  assert.equal(subReference(2, { separator: 46 }), ".B");
  assert.equal(subReference(2, { separator: 46 }, null, false), "B", "aAddSeparator false");
  assert.equal(subReference(12, { firstId: 49 }), "12");
  assert.equal(subReference(12, { separator: 95, firstId: 48 }), "_12");
  assert.equal(subReference(0, { separator: 46 }), "");
  assert.equal(subReference(1, { separator: 45 }, "Left"), "-Left", "a display name after the separator");
});
