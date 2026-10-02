import { test } from "node:test";
import assert from "node:assert/strict";
import { uniqueSymbolLibId } from "./symEditActions";

test("uniqueSymbolLibId: Untitled in the project library when free", () => {
  assert.equal(uniqueSymbolLibId([]), "eda:Untitled");
  assert.equal(uniqueSymbolLibId(["Device:R", "Device:C"]), "eda:Untitled");
});

test("uniqueSymbolLibId: counts up past what the project library already has", () => {
  assert.equal(uniqueSymbolLibId(["eda:Untitled"]), "eda:Untitled_1");
  assert.equal(uniqueSymbolLibId(["eda:Untitled", "eda:Untitled_1", "eda:Untitled_2"]), "eda:Untitled_3");
  assert.equal(uniqueSymbolLibId(["eda:Untitled", "eda:Untitled_2"]), "eda:Untitled_1", "first gap wins");
});

test("uniqueSymbolLibId: an Untitled of another library is not a clash", () => {
  assert.equal(uniqueSymbolLibId(["Other:Untitled"]), "eda:Untitled");
});

test("uniqueSymbolLibId: a different base or library", () => {
  assert.equal(uniqueSymbolLibId(["eda:Chip"], "Chip"), "eda:Chip_1");
  assert.equal(uniqueSymbolLibId([], "X", "mylib"), "mylib:X");
});
