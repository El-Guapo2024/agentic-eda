import { test } from "node:test";
import assert from "node:assert/strict";
import { bareSymbolForms, clipboardTextForSymbols, looksLikeSymbolText } from "./symClipboard";

const LIB = `(kicad_symbol_lib (version 20231120) (generator "eda") (generator_version "1.0")
\t(symbol "R"
\t\t(in_bom yes)
\t)
)
`;

test("bareSymbolForms strips the library wrapper and keeps the symbol forms", () => {
  const bare = bareSymbolForms(LIB);
  assert.ok(bare.startsWith('\t(symbol "R"'));
  assert.ok(!bare.includes("kicad_symbol_lib"));
  assert.equal(bare.trimEnd().endsWith("\t)"), true);
  // balanced
  assert.equal([...bare].filter((c) => c === "(").length, [...bare].filter((c) => c === ")").length);
});

test("clipboardTextForSymbols runs the forms one after the other", () => {
  const two = clipboardTextForSymbols([LIB, LIB.replace('"R"', '"C"')]);
  assert.ok(two.indexOf('(symbol "R"') < two.indexOf('(symbol "C"'));
  assert.equal(two.match(/\(symbol "/g)?.length, 2);
});

test("a text that has no wrapper is returned as it is", () => {
  assert.equal(bareSymbolForms('(symbol "X")'), '(symbol "X")');
});

test("looksLikeSymbolText accepts both forms and refuses other text", () => {
  assert.equal(looksLikeSymbolText('  (symbol "R" (in_bom yes))'), true);
  assert.equal(looksLikeSymbolText("(kicad_symbol_lib (version 1))"), true);
  assert.equal(looksLikeSymbolText("hello"), false);
  assert.equal(looksLikeSymbolText("(footprint \"x\")"), false);
});
