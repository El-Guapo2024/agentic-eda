import { test } from "node:test";
import assert from "node:assert/strict";
import { newSymbolNameCheck, symbolExistsMessage, uniqueSymbolLibId } from "./symEditActions";

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

test("newSymbolNameCheck: GetName trims, underscores spaces and escapes a LIB_ID's specials", () => {
  assert.deepEqual(newSymbolNameCheck("  My Symbol ", []), { name: "My_Symbol", libId: "eda:My_Symbol", error: null, exists: false });
  assert.equal(newSymbolNameCheck("A:B", []).name, "A{colon}B", "a colon becomes {colon}, as EscapeString( CTX_LIBID )");
  assert.equal(newSymbolNameCheck('a"b', []).name, "a{dblquote}b");
  assert.equal(newSymbolNameCheck("a<b>c", []).error, null, "the escaped name is a legal lib_id");
});

test("newSymbolNameCheck: a name is required, in KiCad's words", () => {
  assert.equal(newSymbolNameCheck("", []).error, "Symbol must have a name.");
  assert.equal(newSymbolNameCheck("   ", []).error, "Symbol must have a name.");
  assert.equal(newSymbolNameCheck("   ", []).exists, false);
});

test("symbolExistsMessage: KiCad's taken-name words, the name unescaped, no Overwrite", () => {
  assert.equal(symbolExistsMessage("R", "eda"), "Symbol 'R' already exists in library 'eda'.");
  assert.equal(symbolExistsMessage("A{colon}B", "eda"), "Symbol 'A:B' already exists in library 'eda'.");
  assert.ok(!/Overwrite/.test(symbolExistsMessage("R", "eda")), "the verb cannot overwrite, so the dialog doesn't offer it");
});

test("newSymbolNameCheck: an existing name is reported for the dialog's own library only", () => {
  assert.equal(newSymbolNameCheck("R", ["eda:R", "Device:C"]).exists, true);
  assert.equal(newSymbolNameCheck("R", ["Device:R"]).exists, false, "another library's R is no clash");
  assert.equal(newSymbolNameCheck("R", ["eda:R"], "mylib").exists, false);
  assert.deepEqual(newSymbolNameCheck("R", ["eda:R"]), { name: "R", libId: "eda:R", error: null, exists: true });
});
