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

test("UI-LEFT-2 YY5: Copy's image box -- the selection's boxes merged and grown by 2% a side; the bitmap at the view's scale, at most 4096 px", async () => {
  const { clipboardBox, bitmapSize } = await import("./symClipboard");
  assert.deepEqual(clipboardBox([[0, 0, 1000, 500], null, [500, -500, 2000, 0]]), [-40, -520, 2040, 520]);
  assert.equal(clipboardBox([]), null);
  assert.equal(clipboardBox([[0, 0, 0, 100]]), null, "no width: no image");
  assert.deepEqual(bitmapSize([0, 0, 10_000_000, 5_000_000], 1e-4), { width: 1000, height: 500, scale: 1e-4 });
  const big = bitmapSize([0, 0, 100_000_000, 50_000_000], 1e-4);
  assert.deepEqual([big.width, big.height], [4096, 2048]);
});
