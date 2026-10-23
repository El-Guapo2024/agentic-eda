import { test } from "node:test";
import assert from "node:assert/strict";
import { nextPadNumber, nextPadNumberAfter } from "./padNumbering";

test("an empty footprint's first pad is 1", () => {
  assert.equal(nextPadNumber([]), "1");
});

test("the pad tool's last number starts as 1: the first free number from 1 up", () => {
  const pads = [{ number: "1" }, { number: "2" }];
  assert.equal(nextPadNumber(pads), "3");
  // `PAD_TOOL::Reset( MODEL_RELOAD )` sets the last number to "1": a footprint whose pad 1 is gone gets it back (it was "6", after the highest)
  assert.equal(nextPadNumber([{ number: "2" }, { number: "5" }]), "1");
});

test("GetNextPadNumber( last ) is last itself when it is free, else the first free number after it", () => {
  const pads = [{ number: "1" }, { number: "2" }, { number: "4" }];
  // "3" is free: it is the answer (it was "5": the number was counted up before it was tried)
  assert.equal(nextPadNumberAfter(pads, "3"), "3");
  assert.equal(nextPadNumberAfter([...pads, { number: "3" }], "3"), "5", "3 and 4 are taken");
  assert.equal(nextPadNumber(pads, "2"), "3", "after the pad placed last");
});

test("the prefix of the last number stays: A5 is followed by A6", () => {
  assert.equal(nextPadNumber([{ number: "A5" }], "A5"), "A6");
});

test("GetRefDesPrefix and GetTrailingInt: trailing ? and digits are not the prefix, and %d drops leading zeros", () => {
  assert.equal(nextPadNumberAfter([], "A07"), "A7");
  assert.equal(nextPadNumberAfter([], "B?"), "B0");
  assert.equal(nextPadNumberAfter([{ number: "MH" }], "MH"), "MH0", "a number with no digits counts from 0");
});
