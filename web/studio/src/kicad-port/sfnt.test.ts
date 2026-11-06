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
