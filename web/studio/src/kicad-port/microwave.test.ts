import { test } from "node:test";
import assert from "node:assert/strict";
import {
  arcStubAngleMillideg,
  lengthFromText,
  lengthText,
  parseDouble,
} from "./microwave";

test("the angle is read as wxString::ToDouble reads it: all of the text, one number", () => {
  assert.equal(parseDouble("0.0"), 0);
  assert.equal(parseDouble("  90"), 90);
  assert.equal(parseDouble("-45.5"), -45.5);
  assert.equal(parseDouble("+.5"), 0.5);
  assert.equal(parseDouble("1e2"), 100);
  assert.equal(parseDouble(""), null);
  assert.equal(parseDouble("abc"), null);
  assert.equal(parseDouble("12 "), null, "a trailing blank is not part of a number");
  assert.equal(parseDouble("12deg"), null);
  assert.equal(parseDouble("1,5"), null, "the C locale has no decimal comma");
});

test("an arc stub's angle is its absolute value, at most a half turn", () => {
  assert.equal(arcStubAngleMillideg(90), 90_000);
  assert.equal(arcStubAngleMillideg(-90), 90_000);
  assert.equal(arcStubAngleMillideg(270), 180_000);
  assert.equal(arcStubAngleMillideg(37.5), 37_500);
  assert.equal(arcStubAngleMillideg(0), 0);
});

test("lengths are shown to the digits of the units and read with or without a unit name", () => {
  assert.equal(lengthText(250, "mm"), "0.25");
  assert.equal(lengthText(250, "mil"), "9.84");
  assert.equal(lengthText(254, "in"), "0.01");
  assert.equal(lengthFromText("0.25", "mm"), 250);
  assert.equal(lengthFromText("10", "mil"), 254);
  assert.equal(lengthFromText("0.5 mm", "mil"), 500);
  assert.equal(lengthFromText('0.01"', "mm"), 254);
  assert.equal(lengthFromText("nonsense", "mm"), 0, "`ValueFromString` reads what is not a length as nothing");
});
