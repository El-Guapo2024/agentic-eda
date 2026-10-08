import { test } from "node:test";
import assert from "node:assert/strict";
import { alphabeticFromIndex, incrementString, indexFromAlphabetic } from "./stringIncrement";

test("the trailing number goes up and down, keeping its zero padding", () => {
  assert.equal(incrementString("DATA7", 1, 0), "DATA8");
  assert.equal(incrementString("A9", 1, 0), "A10");
  assert.equal(incrementString("R007", 1, 0), "R008");
  assert.equal(incrementString("R007", -1, 0), "R006");
  assert.equal(incrementString("R009", 1, 0), "R010");
  assert.equal(incrementString("R99", 1, 0), "R100");
  assert.equal(incrementString("D10", -3, 0), "D7");
});

test("a number never goes below zero, and a string with nothing to increment is left (null)", () => {
  assert.equal(incrementString("D0", -1, 0), null);
  assert.equal(incrementString("", 1, 0), null);
  assert.equal(incrementString("---", 1, 0), null);
  assert.equal(incrementString("D0", 1, 0), "D1");
});

test("punctuation after the number is skipped and kept", () => {
  assert.equal(incrementString("A1_", 1, 0), "A2_");
  assert.equal(incrementString("D[3]", 1, 0), "D[4]");
});

test("the secondary part is the second incrementable part from the right", () => {
  assert.equal(incrementString("ROW_B3", 1, 1), "ROW_C3");
  assert.equal(incrementString("ROW_B3", 1, 0), "ROW_B4");
  assert.equal(incrementString("AB1", 1, 1), "AC1");
  assert.equal(incrementString("BA1", 1, 1), null, "BA is index 52, past the limit of 50");
  assert.equal(incrementString("5", 1, 1), null, "no second part");
});

test("letters count in capitals or in small, not mixed, and wrap into two letters", () => {
  assert.equal(incrementString("PIN_A", 1, 0), "PIN_B");
  assert.equal(incrementString("pin_a", 1, 0), "pin_b");
  assert.equal(incrementString("Z", 1, 0), "AA");
  assert.equal(incrementString("AY", 1, 0), "AZ");
  assert.equal(incrementString("AZ", 1, 0), null, "AZ is index 51");
  assert.equal(incrementString("AZ", 1, 0, { alphabeticMaxIndex: -1 }), "BA");
  assert.equal(incrementString("Ab", 1, 0), "Ac", "'A' is one part and 'b' another: the last one moves");
  assert.equal(incrementString("Abc", 1, 0), null, "'bc' is index 54, past the limit of 50");
  assert.equal(incrementString("Y", -1, 0), "X");
  assert.equal(incrementString("A", -1, 0), null);
});

test("a long letter run is not incremented (TX, CAN) and a symbol-editor row name skips I O S Q X Z", () => {
  assert.equal(incrementString("TX", 1, 0), null, "TX is index 513, past the limit of 50");
  assert.equal(incrementString("TX", 1, 0, { alphabeticMaxIndex: -1 }), "TY");
  assert.equal(incrementString("H", 1, 0, { skipIOSQXZ: true }), "J");
  assert.equal(incrementString("H", 1, 0), "I");
  assert.equal(incrementString("I", 1, 0, { skipIOSQXZ: true }), "J", "a letter already there is incremented anyway: the full alphabet is used");
});

test("the alphabet helpers agree with each other", () => {
  const abc = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
  // (the C++ indexes before the alphabet for ZA..ZZ, indices 676-701; the default limit of 50 never gets there)
  for (const word of ["A", "Z", "AA", "AZ", "BA", "YZ", "AAA"]) {
    const i = indexFromAlphabetic(word, abc);
    assert.equal(alphabeticFromIndex(i, abc, true), word, word);
  }
  assert.equal(indexFromAlphabetic("a", abc), -1);
});
