import { test } from "node:test";
import assert from "node:assert/strict";
import { splitString, valueStringCompare } from "./valueStringCompare";

test("ValueStringCompare: the number at the end compares as a number, with its modifier", () => {
  assert.deepEqual(splitString("D10"), ["D", "10", ""]);
  // KiCad's own reading: the T before the digits is an old-school decimal separator ("4T7"), so NET10 is NE 0.10 T
  assert.deepEqual(splitString("NET10"), ["NE", ".10", "T"]);
  assert.deepEqual(splitString("4K7"), ["", "4.7", "K"]);
  assert.equal(valueStringCompare("D2", "D10"), -1);
  assert.equal(valueStringCompare("D10", "D2"), 1);
  assert.equal(valueStringCompare("net", "NET"), 0);
  assert.equal(valueStringCompare("470", "4k7"), -1);
  assert.equal(valueStringCompare("A", "B"), -1);
});
