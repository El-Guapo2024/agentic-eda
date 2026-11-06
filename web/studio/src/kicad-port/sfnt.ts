// What the canvases read of an outline font's file (UI-LEFT-2 CC1): FreeType's face metrics and each glyph's box, from the sfnt tables of the file the server's fontconfig
// matched (`GET /api/fonts/file`).
//
//   - `face->units_per_EM`, `ascender`, `descender`, `height` as FreeType's `sfnt_load_face` (sfnt/sfobjs.c) sets them: the `OS/2` typo metrics when its `fsSelection` has
//     `USE_TYPO_METRICS` (bit 7), else `hhea`'s, else (both zero) the typo ones, else the Windows ones; `height` is ascender - descender + the line gap. KiCad reads
//     `height / units_per_EM` for the interline and the size's ascender and descender for a text's box (`OUTLINE_FONT::GetInterline`, `getTextAsGlyphsUnlocked`).
//   - A glyph's box: its `glyf` header (xMin, yMin, xMax, yMax), the box of the outline FreeType decomposes (`OUTLINE_DECOMPOSER::OutlineToSegments`). The browser's
//     `measureText` rounds a run's ink out to whole pixels of its own glyph size (a few thousandths of the em), so the ink of a run is built from these boxes at the
//     advances the browser shaped (components/text/outlineFonts.ts). A CFF (`.otf`) font has no `glyf`: its glyphs' boxes are not read here.
//   - The `cmap`: format 4 (the Basic Multilingual Plane) and 12 (all of Unicode), the Windows Unicode subtables first.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

export interface SfntFace {
  unitsPerEm: number;
  ascender: number;
  descender: number;
  height: number;
  /** The glyph of a code point (0, `.notdef`, for none). */
  glyphOf(codePoint: number): number;
  /** `hmtx`'s advance of a glyph, font units. */
  advanceOf(glyph: number): number;
  /** A glyph's box `[xMin, yMin, xMax, yMax]` (y up), font units; null for a glyph with no outline (a space), undefined for a font with no `glyf`. */
  boxOf(glyph: number): [number, number, number, number] | null | undefined;
}

/** Face `index` of an sfnt file or collection; null when it is no TrueType or OpenType font. */
export function readSfnt(bytes: Uint8Array, index = 0): SfntFace | null {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  try {
    let at = 0;
    if (v.getUint32(0) === 0x74746366 /* ttcf */) {
      if (index >= v.getUint32(8)) return null;
      at = v.getUint32(12 + 4 * index);
    }
    const tables = new Map<string, number>();
    for (let i = 0; i < v.getUint16(at + 4); i++) {
      const rec = at + 12 + 16 * i;
      tables.set(String.fromCharCode(v.getUint8(rec), v.getUint8(rec + 1), v.getUint8(rec + 2), v.getUint8(rec + 3)), v.getUint32(rec + 8));
    }
    const head = tables.get("head");
    const hhea = tables.get("hhea");
    if (head === undefined || hhea === undefined) return null;
    const unitsPerEm = v.getUint16(head + 18);
    const longLoca = v.getInt16(head + 50) === 1;
    let ascender = v.getInt16(hhea + 4);
    let descender = v.getInt16(hhea + 6);
    let height = ascender - descender + v.getInt16(hhea + 8);
    const os2 = tables.get("OS/2");
    if (os2 !== undefined) {
      const fsSelection = v.getUint16(os2 + 62);
      const [typoAsc, typoDesc, typoGap] = [v.getInt16(os2 + 68), v.getInt16(os2 + 70), v.getInt16(os2 + 72)];
      if (fsSelection & 128) [ascender, descender, height] = [typoAsc, typoDesc, typoAsc - typoDesc + typoGap];
      else if (!ascender && !descender) {
        if (typoAsc || typoDesc) [ascender, descender, height] = [typoAsc, typoDesc, typoAsc - typoDesc + typoGap];
        else {
          const [winAsc, winDesc] = [v.getUint16(os2 + 74), v.getUint16(os2 + 76)];
          [ascender, descender, height] = [winAsc, -winDesc, winAsc + winDesc];
        }
      }
    }
    const metrics = v.getUint16(hhea + 34);
    const hmtx = tables.get("hmtx");
    const loca = tables.get("loca");
    const glyf = tables.get("glyf");
    const cmap = tables.get("cmap");
    // the Unicode subtable: Windows full Unicode (format 12) over Windows BMP (format 4), then the Unicode platform's
    let sub = -1;
    let rank = -1;
    if (cmap !== undefined) {
      for (let i = 0; i < v.getUint16(cmap + 2); i++) {
        const rec = cmap + 4 + 8 * i;
        const [platform, encoding] = [v.getUint16(rec), v.getUint16(rec + 2)];
        const off = cmap + v.getUint32(rec + 4);
        const format = v.getUint16(off);
        const r = platform === 3 && encoding === 10 && format === 12 ? 4 : platform === 3 && encoding === 1 && format === 4 ? 3 : platform === 0 && format === 12 ? 2 : platform === 0 && format === 4 ? 1 : -1;
        if (r > rank) [sub, rank] = [off, r];
      }
    }
    const glyphOf = (c: number): number => {
      if (sub < 0) return 0;
      if (v.getUint16(sub) === 12) {
        const groups = v.getUint32(sub + 12);
        for (let g = 0; g < groups; g++) {
          const rec = sub + 16 + 12 * g;
          const [start, end] = [v.getUint32(rec), v.getUint32(rec + 4)];
          if (c >= start && c <= end) return v.getUint32(rec + 8) + (c - start);
        }
        return 0;
      }
      const segs = v.getUint16(sub + 6) / 2;
      for (let s = 0; s < segs; s++) {
        if (c > v.getUint16(sub + 14 + 2 * s)) continue;
        const start = v.getUint16(sub + 16 + 2 * segs + 2 * s);
        if (c < start) return 0;
        const delta = v.getInt16(sub + 16 + 4 * segs + 2 * s);
        const rangeAt = sub + 16 + 6 * segs + 2 * s;
        const range = v.getUint16(rangeAt);
        if (range === 0) return (c + delta) & 0xffff;
        const g = v.getUint16(rangeAt + range + 2 * (c - start));
        return g ? (g + delta) & 0xffff : 0;
      }
      return 0;
    };
    const advanceOf = (g: number): number => (hmtx === undefined || metrics === 0 ? 0 : v.getUint16(hmtx + 4 * Math.min(g, metrics - 1)));
    const boxOf = (g: number): [number, number, number, number] | null | undefined => {
      if (glyf === undefined || loca === undefined) return undefined;
      const off = longLoca ? v.getUint32(loca + 4 * g) : v.getUint16(loca + 2 * g) * 2;
      const next = longLoca ? v.getUint32(loca + 4 * g + 4) : v.getUint16(loca + 2 * g + 2) * 2;
      if (next <= off) return null;
      const p = glyf + off;
      return [v.getInt16(p + 2), v.getInt16(p + 4), v.getInt16(p + 6), v.getInt16(p + 8)];
    };
    return { unitsPerEm, ascender, descender, height, glyphOf, advanceOf, boxOf };
  } catch {
    return null;
  }
}

/**
 * The ink of `text` at an em of 1, y down (`[left, top, right, bottom]` from the run's origin): each glyph's box at the advance `advanceTo` gives the text before it
 * (the browser's shaping, or the sum of `hmtx`'s advances); null for a text with no outline, undefined when the font has no glyph boxes.
 */
export function sfntInk(f: SfntFace, text: string, advanceTo: (prefix: string) => number): readonly [number, number, number, number] | null | undefined {
  const chars = Array.from(text);
  let box: [number, number, number, number] | null = null;
  for (let i = 0; i < chars.length; i++) {
    const b = f.boxOf(f.glyphOf(chars[i]!.codePointAt(0)!));
    if (b === undefined) return undefined;
    if (!b) continue;
    const x = advanceTo(chars.slice(0, i).join(""));
    const g: [number, number, number, number] = [x + b[0] / f.unitsPerEm, -b[3] / f.unitsPerEm, x + b[2] / f.unitsPerEm, -b[1] / f.unitsPerEm];
    box = box ? [Math.min(box[0], g[0]), Math.min(box[1], g[1]), Math.max(box[2], g[2]), Math.max(box[3], g[3])] : g;
  }
  return box;
}
