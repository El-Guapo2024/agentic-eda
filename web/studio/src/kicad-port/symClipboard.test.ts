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

test("svgCroppedTo: Copy's SVG on the selection's page (`plotSymbolToSvg`), in kicad-cli's mm (UI-LEFT-3 Y3-6)", async () => {
  const { svgCroppedTo } = await import("./symClipboard");
  const svg = `<?xml version="1.0"?>\n<svg\n  xmlns="http://www.w3.org/2000/svg"\n  version="1.1"\n  width="14.0000mm" height="16.0433mm" viewBox="-5.0000 -8.0216 14.0000 16.0433">\n<rect x="-2" y="-2" width="4" height="4"/>\n</svg>\n`;
  const out = svgCroppedTo(svg, [-2_040_000, -2_040_000, 2_040_000, 2_040_000])!;
  assert.match(out, /width="4\.0800mm" height="4\.0800mm" viewBox="-2\.0400 -2\.0400 4\.0800 4\.0800"/);
  assert.ok(out.includes('<rect x="-2" y="-2" width="4" height="4"/>'), "the drawing is as kicad-cli plotted it");
  assert.equal(svgCroppedTo("<svg></svg>", [0, 0, 1, 1]), null, "no size or viewBox, no crop");
});
