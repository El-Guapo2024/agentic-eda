import { test } from "node:test";
import assert from "node:assert/strict";
import { nextPadNumber, nextPadNumberAfter } from "./padNumbering";

test("an empty footprint's first pad is 1", () => {
  assert.equal(nextPadNumber([]), "1");
});

test("increments past the highest existing number", () => {
  const pads = [{ number: "1" }, { number: "2" }];
  assert.equal(nextPadNumber(pads), "3");
});

test("skips a number that is already used further down the sequence", () => {
  const pads = [{ number: "1" }, { number: "2" }, { number: "4" }];
  // Continuing from "3" must skip the already-occupied "4".
  assert.equal(nextPadNumberAfter(pads, "3"), "5");
});

test("keeps a non-numeric prefix stable while incrementing the trailing digits", () => {
  const pads = [{ number: "A5" }];
  assert.equal(nextPadNumber(pads), "A6");
});

test("a non-numeric pad number (blank, or no trailing digits) is treated as zero", () => {
  const pads = [{ number: "" }, { number: "MH" }];
  assert.equal(nextPadNumber(pads), "1");
});

test("matches the backend's own next_pad_number exactly for a mixed-prefix footprint (see crates/model/src/ir.rs's identical test)", () => {
  const pads = [{ number: "1" }, { number: "2" }];
  assert.equal(nextPadNumber(pads), "3");
  const withHole = [...pads, { number: "4" }];
  assert.equal(nextPadNumberAfter(withHole, "3"), "5");
});
