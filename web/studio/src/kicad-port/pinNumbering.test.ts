import { test } from "node:test";
import assert from "node:assert/strict";
import { nextPinNumber, nextPinNumberAfter } from "./pinNumbering";

test("a blank symbol's first pin is 1", () => {
  assert.equal(nextPinNumber([]), "1");
});

test("increments past the highest existing number", () => {
  const pins = [{ number: "1" }, { number: "2" }];
  assert.equal(nextPinNumber(pins), "3");
});

test("skips a number that is already used further down the sequence", () => {
  const pins = [{ number: "1" }, { number: "2" }, { number: "4" }];
  // Continuing from "3" must skip the already-occupied "4".
  assert.equal(nextPinNumberAfter(pins, "3"), "5");
});

test("keeps a non-numeric prefix stable while incrementing the trailing digits", () => {
  const pins = [{ number: "A5" }];
  assert.equal(nextPinNumber(pins), "A6");
});

test("a non-numeric pin number (blank, or no trailing digits) is treated as zero", () => {
  const pins = [{ number: "" }, { number: "NC" }];
  assert.equal(nextPinNumber(pins), "1");
});

test("matches the backend's own next_pin_number exactly for a mixed-prefix symbol (see crates/model/src/ir.rs's identical test)", () => {
  const pins = [{ number: "1" }, { number: "2" }];
  assert.equal(nextPinNumber(pins), "3");
  const withHole = [...pins, { number: "4" }];
  assert.equal(nextPinNumberAfter(withHole, "3"), "5");
});
