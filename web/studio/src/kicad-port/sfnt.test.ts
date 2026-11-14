import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { readSfnt, sfntInk } from "./sfnt";

const DEJAVU = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";

test("readSfnt: DejaVu Sans as FreeType reads it -- 2048 to the em, hhea's 1901 / -483 and no line gap, H's advance and box (skipped without the file)", () => {
  if (!existsSync(DEJAVU)) return;
  const f = readSfnt(readFileSync(DEJAVU))!;
  assert.deepEqual([f.unitsPerEm, f.ascender, f.descender, f.height], [2048, 1901, -483, 2384]);
  const h = f.glyphOf("H".codePointAt(0)!);
  assert.equal(f.advanceOf(h), 1540);
  assert.deepEqual(f.boxOf(h), [201, 0, 1339, 1493]);
  // a space has no outline; "HH": the second H one advance on
  assert.equal(f.boxOf(f.glyphOf(32)), null);
  const ink = sfntInk(f, "HH", (p) => (p.length * 1540) / 2048)!;
  assert.deepEqual(ink.map((v) => Math.round(v * 2048) + 0), [201, -1493, 1540 + 1339, 0]);
});

test("readSfnt: not a font", () => {
  assert.equal(readSfnt(new Uint8Array([1, 2, 3, 4, 5, 6])), null);
});

import { FAKE_BOLD_EM, ftDecompose, ftEmbolden, ftOrientation, glyphPolys, sfntGlyphPolys, type FtContour } from "./sfnt";
import { bezierPolyD } from "./bezierPoly";

const LOMA = "/usr/share/fonts/opentype/tlwg/Loma-Bold.otf";

const boxOf = (polys: ReadonlyArray<{ outer: ReadonlyArray<readonly [number, number]> }>) => {
  const xs = polys.flatMap((p) => p.outer.map((q) => q[0]));
  const ys = polys.flatMap((p) => p.outer.map((q) => q[1]));
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
};

test("the glyphs of DejaVu Sans as KiCad's OUTLINE_FONT makes them: 'o' one outline with its hole, 'H' one outline, within the glyf box (skipped without the file)", () => {
  if (!existsSync(DEJAVU)) return;
  const f = readSfnt(readFileSync(DEJAVU))!;
  const adv = (p: string) => (Array.from(p).length * 1540) / 2048;
  const o = sfntGlyphPolys(f, "o", adv)!;
  assert.equal(o.length, 1, "one outline");
  assert.equal(o[0]!.holes.length, 1, "with the counter as its hole");
  const h = sfntGlyphPolys(f, "H", adv)!;
  assert.deepEqual([h.length, h[0]!.holes.length], [1, 0]);
  // y down from the baseline: the box is the glyf box flipped
  assert.deepEqual(boxOf(h).map((v) => Math.round(v * 2048) + 0), [201, -1493, 1339, 0]);
  // the second glyph of "HH" is one advance on; a space has none
  assert.equal(sfntGlyphPolys(f, "H H", adv)!.length, 2);
  // a fake italic leans the top right by tan 12 degrees of its height (x' = x cos 12 + y sin 12)
  const it = sfntGlyphPolys(f, "H", adv, true)!;
  assert.ok(Math.abs(boxOf(it)[2]! - (1339 * Math.cos((12 * Math.PI) / 180) + 1493 * Math.sin((12 * Math.PI) / 180)) / 2048) < 1e-3);
  // a fake bold is one of FreeType's pixels at KiCad's size wider (FT_Outline_Embolden: half on each side, then moved up and right by half)
  const bold = sfntGlyphPolys(f, "H", adv, false, true)!;
  assert.ok(Math.abs(boxOf(bold)[2]! - boxOf(bold)[0]! - (boxOf(h)[2]! - boxOf(h)[0]!) - FAKE_BOLD_EM) < 1e-4, "one strength wider");
});

test("a CFF font's glyphs come from its Type 2 charstrings (skipped without the file)", () => {
  if (!existsSync(LOMA)) return;
  const f = readSfnt(readFileSync(LOMA))!;
  assert.equal(f.boxOf(f.glyphOf(72)), undefined, "no glyf table");
  const o = sfntGlyphPolys(f, "o", () => 0)!;
  assert.equal(o.length, 1);
  assert.equal(o[0]!.holes.length, 1);
  const [x0, y0, x1, y1] = boxOf(sfntGlyphPolys(f, "H", () => 0)!) as [number, number, number, number];
  assert.ok(x1 > x0 && y0 < -0.5 && Math.abs(y1) < 0.01, `an H on the baseline about the cap height: ${[x0, y0, x1, y1]}`);
});

test("FT_Outline_Decompose through OUTLINE_DECOMPOSER: implied on-curve points between two conic points, a start on a conic point, the closing point dropped", () => {
  const sq: FtContour = [
    { x: 0, y: 0, tag: "on" },
    { x: 0, y: 1, tag: "on" },
    { x: 1, y: 1, tag: "on" },
    { x: 1, y: 0, tag: "on" },
  ];
  assert.deepEqual(ftDecompose([sq], (c) => bezierPolyD(c, 0.01)), [[[0, 0], [0, 1], [1, 1], [1, 0]]]);
  assert.equal(ftOrientation([sq]), "truetype", "clockwise with y up");
  // a circle-ish contour of four conic points only: it starts at the middle of the last and the first
  const round: FtContour = [
    { x: 1, y: 1, tag: "conic" },
    { x: -1, y: 1, tag: "conic" },
    { x: -1, y: -1, tag: "conic" },
    { x: 1, y: -1, tag: "conic" },
  ];
  const flat = ftDecompose([round], (c) => bezierPolyD(c, 0.001))[0]!;
  assert.deepEqual(flat[0], [1, 0]);
  assert.ok(flat.length > 12 && flat.every(([x, y]) => Math.hypot(x, y) <= Math.hypot(0.75, 0.75) + 1e-9 && Math.hypot(x, y) >= 1 - 1e-9));
  // an outline and its hole, the hole in the other winding
  const hole: FtContour = sq.map((p) => ({ x: 0.25 + p.x / 2, y: 0.25 + p.y / 2, tag: p.tag })).reverse();
  const polys = glyphPolys(ftDecompose([sq, hole], (c) => bezierPolyD(c, 0.01)), ftOrientation([sq, hole]));
  assert.deepEqual([polys.length, polys[0]!.holes.length], [1, 1]);
  // emboldened, the square grows by the strength and moves half of it up and right
  const big = ftEmbolden([sq], 0.1)[0]!;
  assert.deepEqual(big.map((p) => [+p.x.toFixed(6) + 0, +p.y.toFixed(6) + 0]), [[0, 0], [0, 1.1], [1.1, 1.1], [1.1, 0]]);
});

test("BEZIER_POLY::getQuadPoly: the ends, and points within the error of the curve", () => {
  const pts = bezierPolyD([[0, 0], [5, 10], [10, 0]], 0.05);
  assert.deepEqual([pts[0], pts[pts.length - 1]], [[0, 0], [10, 0]]);
  assert.ok(pts.length > 4);
  for (const [x, y] of pts) assert.ok(Math.abs(y - (x * (10 - x)) / 5) < 1e-9, "on the parabola y = x (10 - x) / 5");
});
