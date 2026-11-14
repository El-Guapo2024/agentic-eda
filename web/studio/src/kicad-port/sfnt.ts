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
//   - A glyph's outline (the 3D view's text, `BOARD_ADAPTER::addText`'s triangles): the `glyf` contours (a composite's components placed), or a CFF font's Type 2
//     charstrings, as FreeType loads them; slanted for a fake italic (`FT_Set_Transform`), emboldened for a fake bold (`FT_Outline_Embolden`), decomposed and flattened as
//     `OUTLINE_DECOMPOSER` does (`BEZIER_POLY::GetPoly( .., m_FontErrorSize )`), sorted into outlines and holes as `getTextAsGlyphsUnlocked` sorts them. FreeType's
//     hinting at KiCad's 358 pixels to the em is not applied (it moves a point by a fraction of a pixel there: under a 1/700 of the size).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import { bezierPolyD } from "./bezierPoly";

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
  /** A glyph's outline as FreeType loads it (`FT_Load_Glyph`, `FT_LOAD_NO_BITMAP`): its contours of tagged points, font units, y up; null for no outline, undefined for a font whose
   *  outlines are not read here (neither `glyf` nor `CFF `). */
  outlineOf(glyph: number): FtContour[] | null | undefined;
}

/** One point of an `FT_Outline` with its tag: on the curve, a conic's control point (`FT_CURVE_TAG_CONIC`) or a cubic's (`FT_CURVE_TAG_CUBIC`). */
export interface FtPoint {
  x: number;
  y: number;
  tag: "on" | "conic" | "cubic";
}

/** One contour of an `FT_Outline`. */
export type FtContour = FtPoint[];

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
    const glyfAt = (g: number): number | null => {
      if (glyf === undefined || loca === undefined) return null;
      const off = longLoca ? v.getUint32(loca + 4 * g) : v.getUint16(loca + 2 * g) * 2;
      const next = longLoca ? v.getUint32(loca + 4 * g + 4) : v.getUint16(loca + 2 * g + 2) * 2;
      return next <= off ? null : glyf + off;
    };
    const cff = tables.get("CFF ");
    const cffFont = glyf === undefined && cff !== undefined ? readCff(v, cff) : null;
    const outlineOf = (g: number): FtContour[] | null | undefined => {
      if (glyf !== undefined && loca !== undefined) return glyfOutline(v, glyfAt, g, 0);
      if (cffFont) return cffFont.outline(g);
      return undefined;
    };
    return { unitsPerEm, ascender, descender, height, glyphOf, advanceOf, boxOf, outlineOf };
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

// ------------------------------------------------------------------------------------------------------------------------------- outlines

/**
 * A `glyf` glyph's contours as FreeType's TrueType loader gives them (`TT_Load_Simple_Glyph`, `TT_Process_Composite_Component`): a simple glyph's points with their
 * on-curve flags (an off-curve point is a conic control point); a composite one its components', each moved (`ARGS_ARE_XY_VALUES`) and scaled (`WE_HAVE_A_SCALE`, an x and
 * y scale, a two by two) as its record says -- a component placed by matching points (no `ARGS_ARE_XY_VALUES`) is placed at its own origin. Null for an empty glyph.
 */
function glyfOutline(v: DataView, glyfAt: (g: number) => number | null, g: number, depth: number): FtContour[] | null {
  const p = glyfAt(g);
  if (p === null || depth > 8) return null;
  const n = v.getInt16(p);
  if (n >= 0) {
    const ends: number[] = [];
    for (let i = 0; i < n; i++) ends.push(v.getUint16(p + 10 + 2 * i));
    const count = n > 0 ? ends[n - 1]! + 1 : 0;
    let at = p + 10 + 2 * n;
    at += 2 + v.getUint16(at);
    const flags: number[] = [];
    while (flags.length < count) {
      const f = v.getUint8(at++);
      flags.push(f);
      if (f & 8) {
        let r = v.getUint8(at++);
        while (r-- > 0) flags.push(f);
      }
    }
    const xs: number[] = [];
    let x = 0;
    for (const f of flags) {
      if (f & 2) {
        const d = v.getUint8(at++);
        x += f & 16 ? d : -d;
      } else if (!(f & 16)) {
        x += v.getInt16(at);
        at += 2;
      }
      xs.push(x);
    }
    const ys: number[] = [];
    let y = 0;
    for (const f of flags) {
      if (f & 4) {
        const d = v.getUint8(at++);
        y += f & 32 ? d : -d;
      } else if (!(f & 32)) {
        y += v.getInt16(at);
        at += 2;
      }
      ys.push(y);
    }
    const out: FtContour[] = [];
    let first = 0;
    for (const end of ends) {
      const c: FtContour = [];
      for (let i = first; i <= end && i < count; i++) c.push({ x: xs[i]!, y: ys[i]!, tag: flags[i]! & 1 ? "on" : "conic" });
      if (c.length > 0) out.push(c);
      first = end + 1;
    }
    return out;
  }
  // a composite glyph
  const out: FtContour[] = [];
  let at = p + 10;
  for (;;) {
    const flags = v.getUint16(at);
    const glyph = v.getUint16(at + 2);
    at += 4;
    let dx = 0;
    let dy = 0;
    if (flags & 1) {
      if (flags & 2) [dx, dy] = [v.getInt16(at), v.getInt16(at + 2)];
      at += 4;
    } else {
      if (flags & 2) [dx, dy] = [v.getInt8(at), v.getInt8(at + 1)];
      at += 2;
    }
    if (!(flags & 2)) [dx, dy] = [0, 0];
    let [a, b, c, d] = [1, 0, 0, 1];
    const f2 = (o: number) => v.getInt16(o) / 16384;
    if (flags & 8) {
      a = d = f2(at);
      at += 2;
    } else if (flags & 0x40) {
      [a, d] = [f2(at), f2(at + 2)];
      at += 4;
    } else if (flags & 0x80) {
      [a, b, c, d] = [f2(at), f2(at + 2), f2(at + 4), f2(at + 6)];
      at += 8;
    }
    for (const contour of glyfOutline(v, glyfAt, glyph, depth + 1) ?? []) out.push(contour.map((pt) => ({ x: pt.x * a + pt.y * c + dx, y: pt.x * b + pt.y * d + dy, tag: pt.tag })));
    if (!(flags & 0x20)) break;
  }
  return out.length > 0 ? out : null;
}

/** A CFF font's charstrings (the `CFF ` table of an OpenType font): its glyphs' outlines from their Type 2 charstrings (`cff_decoder_parse_charstrings`). */
interface CffFont {
  outline(g: number): FtContour[] | null;
}

/** An INDEX of the CFF table: where each of its objects starts and ends, and where the INDEX ends. */
function cffIndex(v: DataView, at: number): { items: Array<[number, number]>; end: number } {
  const count = v.getUint16(at);
  if (count === 0) return { items: [], end: at + 2 };
  const size = v.getUint8(at + 2);
  const off = (i: number) => {
    let r = 0;
    for (let k = 0; k < size; k++) r = r * 256 + v.getUint8(at + 3 + i * size + k);
    return r;
  };
  const base = at + 3 + (count + 1) * size - 1;
  const items: Array<[number, number]> = [];
  for (let i = 0; i < count; i++) items.push([base + off(i), base + off(i + 1)]);
  return { items, end: base + off(count) };
}

/** A CFF DICT: its operators (12 x as 1200 + x) and their operands. */
function cffDict(v: DataView, from: number, to: number): Map<number, number[]> {
  const out = new Map<number, number[]>();
  let ops: number[] = [];
  let at = from;
  while (at < to) {
    const b = v.getUint8(at++);
    if (b <= 21) {
      const op = b === 12 ? 1200 + v.getUint8(at++) : b;
      out.set(op, ops);
      ops = [];
    } else if (b === 28) {
      ops.push(v.getInt16(at));
      at += 2;
    } else if (b === 29) {
      ops.push(v.getInt32(at));
      at += 4;
    } else if (b === 30) {
      // a real: nibbles
      let str = "";
      for (;;) {
        const byte = v.getUint8(at++);
        const nib = [byte >> 4, byte & 15];
        let done = false;
        for (const n of nib) {
          if (n === 15) {
            done = true;
            break;
          }
          str += n <= 9 ? String(n) : n === 10 ? "." : n === 11 ? "E" : n === 12 ? "E-" : n === 14 ? "-" : "";
        }
        if (done) break;
      }
      ops.push(Number(str));
    } else if (b >= 32 && b <= 246) ops.push(b - 139);
    else if (b >= 247 && b <= 250) ops.push((b - 247) * 256 + v.getUint8(at++) + 108);
    else if (b >= 251 && b <= 254) ops.push(-(b - 251) * 256 - v.getUint8(at++) - 108);
  }
  return out;
}

const subrBias = (n: number): number => (n < 1240 ? 107 : n < 33900 ? 1131 : 32768);

/** The CFF table at `at`: the first font of its Top DICT INDEX, its charstrings, its global and local subroutines (a CID font's per Font DICT, by its FDSelect). */
function readCff(v: DataView, at: number): CffFont | null {
  try {
    const hdrSize = v.getUint8(at + 2);
    const names = cffIndex(v, at + hdrSize);
    const tops = cffIndex(v, names.end);
    const strings = cffIndex(v, tops.end);
    const gsubrs = cffIndex(v, strings.end).items;
    const top = tops.items[0];
    if (!top) return null;
    const dict = cffDict(v, top[0], top[1]);
    const cs = dict.get(17)?.[0];
    if (cs === undefined) return null;
    const charstrings = cffIndex(v, at + cs).items;
    const privSubrs = (d: Map<number, number[]>): Array<[number, number]> => {
      const priv = d.get(18);
      if (!priv || priv.length < 2) return [];
      const [size, off] = [priv[0]!, priv[1]!];
      const pd = cffDict(v, at + off, at + off + size);
      const subrs = pd.get(19)?.[0];
      return subrs === undefined ? [] : cffIndex(v, at + off + subrs).items;
    };
    let localOf: (g: number) => Array<[number, number]>;
    const fdArray = dict.get(1236)?.[0];
    const fdSelect = dict.get(1237)?.[0];
    if (fdArray !== undefined && fdSelect !== undefined) {
      const fds = cffIndex(v, at + fdArray).items.map(([a, b]) => privSubrs(cffDict(v, a, b)));
      const sel = at + fdSelect;
      const format = v.getUint8(sel);
      localOf = (g) => {
        if (format === 0) return fds[v.getUint8(sel + 1 + g)] ?? [];
        const nRanges = v.getUint16(sel + 1);
        for (let r = 0; r < nRanges; r++) {
          const first = v.getUint16(sel + 3 + 3 * r);
          const next = v.getUint16(sel + 3 + 3 * (r + 1));
          if (g >= first && g < next) return fds[v.getUint8(sel + 5 + 3 * r)] ?? [];
        }
        return [];
      };
    } else {
      const local = privSubrs(dict);
      localOf = () => local;
    }
    return { outline: (g) => (charstrings[g] ? type2Outline(v, charstrings[g]!, gsubrs, localOf(g)) : null) };
  } catch {
    return null;
  }
}

/** A Type 2 charstring run as FreeType's CFF decoder runs it (`cff_decoder_parse_charstrings`): the contours it draws, a cubic's two control points tagged `cubic`. */
function type2Outline(v: DataView, cs: [number, number], gsubrs: ReadonlyArray<[number, number]>, lsubrs: ReadonlyArray<[number, number]>): FtContour[] | null {
  const out: FtContour[] = [];
  let cur: FtContour | null = null;
  let x = 0;
  let y = 0;
  let stack: number[] = [];
  let nStems = 0;
  let haveWidth = false;
  const close = () => {
    if (cur && cur.length > 1) {
      // the closing point FreeType drops when it is the first one again
      const a = cur[0]!;
      const b = cur[cur.length - 1]!;
      if (b.tag === "on" && a.x === b.x && a.y === b.y) cur.pop();
    }
    if (cur && cur.length > 0) out.push(cur);
    cur = null;
  };
  const moveTo = (dx: number, dy: number) => {
    close();
    x += dx;
    y += dy;
    cur = [{ x, y, tag: "on" }];
  };
  const lineTo = (dx: number, dy: number) => {
    x += dx;
    y += dy;
    cur ??= [];
    cur.push({ x, y, tag: "on" });
  };
  const curveTo = (dx1: number, dy1: number, dx2: number, dy2: number, dx3: number, dy3: number) => {
    cur ??= [];
    const [x1, y1] = [x + dx1, y + dy1];
    const [x2, y2] = [x1 + dx2, y1 + dy2];
    x = x2 + dx3;
    y = y2 + dy3;
    cur.push({ x: x1, y: y1, tag: "cubic" }, { x: x2, y: y2, tag: "cubic" }, { x, y, tag: "on" });
  };
  const stems = () => {
    if (!haveWidth && stack.length % 2 === 1) stack.shift();
    haveWidth = true;
    nStems += stack.length >> 1;
    stack = [];
  };
  let ended = false;
  const run = (from: number, to: number, depth: number) => {
    let at = from;
    while (at < to && !ended) {
      const b = v.getUint8(at++);
      if (b >= 32 || b === 28) {
        if (b === 28) {
          stack.push(v.getInt16(at));
          at += 2;
        } else if (b <= 246) stack.push(b - 139);
        else if (b <= 250) stack.push((b - 247) * 256 + v.getUint8(at++) + 108);
        else if (b <= 254) stack.push(-(b - 251) * 256 - v.getUint8(at++) - 108);
        else {
          stack.push(v.getInt32(at) / 65536);
          at += 4;
        }
        continue;
      }
      switch (b) {
        case 1:
        case 3:
        case 18:
        case 23:
          stems();
          break;
        case 19:
        case 20:
          stems();
          at += (nStems + 7) >> 3;
          break;
        case 21:
          if (!haveWidth && stack.length > 2) stack.shift();
          haveWidth = true;
          moveTo(stack[0] ?? 0, stack[1] ?? 0);
          stack = [];
          break;
        case 22:
          if (!haveWidth && stack.length > 1) stack.shift();
          haveWidth = true;
          moveTo(stack[0] ?? 0, 0);
          stack = [];
          break;
        case 4:
          if (!haveWidth && stack.length > 1) stack.shift();
          haveWidth = true;
          moveTo(0, stack[0] ?? 0);
          stack = [];
          break;
        case 5:
          for (let i = 0; i + 1 < stack.length; i += 2) lineTo(stack[i]!, stack[i + 1]!);
          stack = [];
          break;
        case 6:
        case 7: {
          let horizontal = b === 6;
          for (const d of stack) {
            if (horizontal) lineTo(d, 0);
            else lineTo(0, d);
            horizontal = !horizontal;
          }
          stack = [];
          break;
        }
        case 8:
          for (let i = 0; i + 5 < stack.length; i += 6) curveTo(stack[i]!, stack[i + 1]!, stack[i + 2]!, stack[i + 3]!, stack[i + 4]!, stack[i + 5]!);
          stack = [];
          break;
        case 24: {
          let i = 0;
          for (; i + 5 < stack.length - 2; i += 6) curveTo(stack[i]!, stack[i + 1]!, stack[i + 2]!, stack[i + 3]!, stack[i + 4]!, stack[i + 5]!);
          lineTo(stack[i] ?? 0, stack[i + 1] ?? 0);
          stack = [];
          break;
        }
        case 25: {
          let i = 0;
          for (; i + 1 < stack.length - 6; i += 2) lineTo(stack[i]!, stack[i + 1]!);
          curveTo(stack[i] ?? 0, stack[i + 1] ?? 0, stack[i + 2] ?? 0, stack[i + 3] ?? 0, stack[i + 4] ?? 0, stack[i + 5] ?? 0);
          stack = [];
          break;
        }
        case 26: {
          let i = 0;
          let dx1 = 0;
          if (stack.length % 2 === 1) dx1 = stack[i++]!;
          for (; i + 3 < stack.length; i += 4) {
            curveTo(dx1, stack[i]!, stack[i + 1]!, stack[i + 2]!, 0, stack[i + 3]!);
            dx1 = 0;
          }
          stack = [];
          break;
        }
        case 27: {
          let i = 0;
          let dy1 = 0;
          if (stack.length % 2 === 1) dy1 = stack[i++]!;
          for (; i + 3 < stack.length; i += 4) {
            curveTo(stack[i]!, dy1, stack[i + 1]!, stack[i + 2]!, stack[i + 3]!, 0);
            dy1 = 0;
          }
          stack = [];
          break;
        }
        case 30:
        case 31: {
          // vhcurveto / hvcurveto: the curves alternate, the last one taking an odd argument for its end's other coordinate
          let vertical = b === 30;
          let i = 0;
          while (i + 3 < stack.length) {
            const last = stack.length - (i + 4) === 1 ? stack[i + 4]! : 0;
            if (vertical) curveTo(0, stack[i]!, stack[i + 1]!, stack[i + 2]!, stack[i + 3]!, last);
            else curveTo(stack[i]!, 0, stack[i + 1]!, stack[i + 2]!, last, stack[i + 3]!);
            i += 4;
            vertical = !vertical;
          }
          stack = [];
          break;
        }
        case 10:
        case 29: {
          const subrs = b === 10 ? lsubrs : gsubrs;
          const idx = (stack.pop() ?? 0) + subrBias(subrs.length);
          const s = subrs[idx];
          if (s && depth < 10) run(s[0], s[1], depth + 1);
          break;
        }
        case 11:
          return;
        case 14:
          close();
          ended = true;
          return;
        case 12: {
          const op = v.getUint8(at++);
          const s = stack;
          // the flex operators (Type 2 Charstring Format, 4.1): two curves each
          if (op === 35 && s.length >= 12) {
            curveTo(s[0]!, s[1]!, s[2]!, s[3]!, s[4]!, s[5]!);
            curveTo(s[6]!, s[7]!, s[8]!, s[9]!, s[10]!, s[11]!);
          } else if (op === 34 && s.length >= 7) {
            curveTo(s[0]!, 0, s[1]!, s[2]!, s[3]!, 0);
            curveTo(s[4]!, 0, s[5]!, -s[2]!, s[6]!, 0);
          } else if (op === 36 && s.length >= 9) {
            curveTo(s[0]!, s[1]!, s[2]!, s[3]!, s[4]!, 0);
            curveTo(s[5]!, 0, s[6]!, s[7]!, s[8]!, -(s[1]! + s[3]! + s[7]!));
          } else if (op === 37 && s.length >= 11) {
            const dx = s[0]! + s[2]! + s[4]! + s[6]! + s[8]!;
            const dy = s[1]! + s[3]! + s[5]! + s[7]! + s[9]!;
            curveTo(s[0]!, s[1]!, s[2]!, s[3]!, s[4]!, s[5]!);
            if (Math.abs(dx) > Math.abs(dy)) curveTo(s[6]!, s[7]!, s[8]!, s[9]!, s[10]!, -dy);
            else curveTo(s[6]!, s[7]!, s[8]!, s[9]!, -dx, s[10]!);
          }
          stack = [];
          break;
        }
        default:
          stack = [];
      }
    }
  };
  run(cs[0], cs[1], 0);
  close();
  return out.length > 0 ? out : null;
}

/** `FT_Outline_Get_Orientation`: the sign of the area the control points span (nonzero winding), y up -- "truetype" clockwise, "postscript" counter-clockwise. */
export function ftOrientation(contours: readonly FtContour[]): "truetype" | "postscript" | "none" {
  let area = 0;
  for (const c of contours) {
    let prev = c[c.length - 1];
    for (const cur of c) {
      area += (cur.y - prev!.y) * (cur.x + prev!.x);
      prev = cur;
    }
  }
  return area > 0 ? "postscript" : area < 0 ? "truetype" : "none";
}

/**
 * `FT_Outline_EmboldenXY( outline, strength, strength )` (FreeType's src/base/ftoutln.c, what `FT_Outline_Embolden( &face->glyph->outline, 1 << 6 )` calls for KiCad's fake
 * bold): every point moved half the strength up and right and along the bisector of its two edges, outwards, by up to half the strength.
 */
export function ftEmbolden(contours: FtContour[], strength: number): FtContour[] {
  const xs = strength / 2;
  const ys = strength / 2;
  const orientation = ftOrientation(contours);
  if (orientation === "none" || strength === 0) return contours;
  return contours.map((c) => {
    const pts = c.map((p) => ({ ...p }));
    const n = pts.length;
    const last = n - 1;
    let lIn = 0;
    let inV = { x: 0, y: 0 };
    let anchor = { x: 0, y: 0 };
    let lAnchor = 0;
    const src = c;
    for (let i = last, j = 0, k = -1; j !== i && i !== k; j = j < last ? j + 1 : 0) {
      let out: { x: number; y: number };
      let lOut: number;
      if (j !== k) {
        const ox = src[j]!.x - src[i]!.x;
        const oy = src[j]!.y - src[i]!.y;
        lOut = Math.hypot(ox, oy);
        if (lOut === 0) continue;
        out = { x: ox / lOut, y: oy / lOut };
      } else {
        out = anchor;
        lOut = lAnchor;
      }
      if (lIn !== 0) {
        if (k < 0) {
          k = i;
          anchor = inV;
          lAnchor = lIn;
        }
        let d = inV.x * out.x + inV.y * out.y;
        let sx = 0;
        let sy = 0;
        // "shift only if turn is less than ~160 degrees"
        if (d > -0.9375) {
          d += 1;
          sx = inV.y + out.y;
          sy = inV.x + out.x;
          if (orientation === "truetype") sx = -sx;
          else sy = -sy;
          let q = out.x * inV.y - out.y * inV.x;
          if (orientation === "truetype") q = -q;
          const l = Math.min(lIn, lOut);
          sx = xs * q <= l * d ? (sx * xs) / d : (sx * l) / q;
          sy = ys * q <= l * d ? (sy * ys) / d : (sy * l) / q;
        }
        for (; i !== j; i = i < last ? i + 1 : 0) {
          pts[i]!.x = src[i]!.x + xs + sx;
          pts[i]!.y = src[i]!.y + ys + sy;
        }
      } else i = j;
      inV = out;
      lIn = lOut;
    }
    return pts;
  });
}

const mid = (a: { x: number; y: number }, b: { x: number; y: number }): [number, number] => [(a.x + b.x) / 2, (a.y + b.y) / 2];

/**
 * `FT_Outline_Decompose` through KiCad's `OUTLINE_DECOMPOSER` (common/font/outline_decomposer.cpp): each contour from its first on-curve point (or the middle of its first
 * and last conic points), a line to every on-curve point, a conic or cubic flattened by `BEZIER_POLY::GetPoly( result, m_FontErrorSize )` (`flatten`), a repeated point
 * left out; the contour closed.
 */
export function ftDecompose(contours: readonly FtContour[], flatten: (ctrl: Array<[number, number]>) => Array<[number, number]>): Array<Array<[number, number]>> {
  const out: Array<Array<[number, number]>> = [];
  for (const c of contours) {
    if (c.length === 0 || c[0]!.tag === "cubic") continue;
    const pts: Array<[number, number]> = [];
    const add = (p: [number, number]) => {
      const b = pts[pts.length - 1];
      if (!b || b[0] !== p[0] || b[1] !== p[1]) pts.push(p);
    };
    let first = 0;
    let limit = c.length - 1;
    let start: [number, number] = [c[0]!.x, c[0]!.y];
    if (c[0]!.tag === "conic") {
      const lastP = c[limit]!;
      if (lastP.tag === "on") {
        start = [lastP.x, lastP.y];
        limit--;
      } else start = mid(c[0]!, lastP);
      first = -1;
    }
    add(start);
    let last: [number, number] = start;
    let i = first;
    let closed = false;
    while (i < limit) {
      i++;
      const p = c[i]!;
      if (p.tag === "on") {
        last = [p.x, p.y];
        add(last);
        continue;
      }
      if (p.tag === "conic") {
        let control: [number, number] = [p.x, p.y];
        for (;;) {
          if (i < limit) {
            i++;
            const q = c[i]!;
            if (q.tag === "on") {
              for (const r of flatten([last, control, [q.x, q.y]])) add(r);
              last = [q.x, q.y];
              break;
            }
            const m: [number, number] = [(control[0] + q.x) / 2, (control[1] + q.y) / 2];
            for (const r of flatten([last, control, m])) add(r);
            last = m;
            control = [q.x, q.y];
            continue;
          }
          for (const r of flatten([last, control, start])) add(r);
          closed = true;
          break;
        }
        if (closed) break;
        continue;
      }
      // a cubic: two control points
      const c1: [number, number] = [p.x, p.y];
      const p2 = c[i + 1];
      if (!p2) break;
      const c2: [number, number] = [p2.x, p2.y];
      i += 2;
      if (i <= limit) {
        const e: [number, number] = [c[i]!.x, c[i]!.y];
        for (const r of flatten([last, c1, c2, e])) add(r);
        last = e;
        continue;
      }
      for (const r of flatten([last, c1, c2, start])) add(r);
      closed = true;
      break;
    }
    if (!closed) add(start);
    // the closing point is the first one again
    if (pts.length > 1 && pts[0]![0] === pts[pts.length - 1]![0] && pts[0]![1] === pts[pts.length - 1]![1]) pts.pop();
    if (pts.length > 0) out.push(pts);
  }
  return out;
}

/** `OUTLINE_DECOMPOSER::winding`: 1 for a clockwise contour (y up), -1 counter-clockwise, 0 for none. */
function winding(c: ReadonlyArray<[number, number]>): number {
  if (c.length < 2) return 0;
  let sum = 0;
  for (let i = 0; i < c.length; i++) {
    const p1 = c[i]!;
    const p2 = c[(i + 1) % c.length]!;
    sum += (p2[0] - p1[0]) * (p2[1] + p1[1]);
  }
  return sum > 0 ? 1 : sum < 0 ? -1 : 0;
}

function pointInside(c: ReadonlyArray<[number, number]>, p: [number, number]): boolean {
  let inside = false;
  for (let i = 0, j = c.length - 1; i < c.length; j = i++) {
    const [xi, yi] = c[i]!;
    const [xj, yj] = c[j]!;
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** One glyph polygon: an outline and its holes. */
export interface GlyphPoly {
  outer: Array<[number, number]>;
  holes: Array<Array<[number, number]>>;
}

/**
 * `OUTLINE_FONT::getTextAsGlyphsUnlocked`' polygons of one glyph's contours (y up): a contour is filled when its winding agrees with the outline's orientation
 * (`contourIsFilled`), a hole otherwise; each hole goes to the first outline its first point is inside, and one inside none is an outline of its own ("Some lovely TTF
 * fonts decided that winding didn't matter for outlines that don't have holes").
 */
export function glyphPolys(contours: ReadonlyArray<Array<[number, number]>>, orientation: "truetype" | "postscript" | "none"): GlyphPoly[] {
  const polys: GlyphPoly[] = [];
  const holes: Array<Array<[number, number]>> = [];
  for (const c of contours) {
    const w = winding(c);
    const filled = orientation === "truetype" ? w === 1 : orientation === "postscript" ? w === -1 : false;
    if (filled) polys.push({ outer: c, holes: [] });
    else holes.push(c);
  }
  for (const h of holes) {
    if (h.length === 0) continue;
    const owner = polys.find((p) => pointInside(p.outer, h[0]!));
    if (owner) owner.holes.push(h);
    else polys.push({ outer: h, holes: [] });
  }
  return polys;
}

/** `OUTLINE_FONT::faceSize()`: 16 points times `m_charSizeScaler` (64) times `m_outlineFontSizeCompensation` (1.4), an int: the em in `OUTLINE_DECOMPOSER`'s units. */
const FACE_SIZE = Math.trunc(16 * 64 * 1.4);
/** `ADVANCED_CFG::m_FontErrorSize` (2), in those units: the error a glyph's curves are flattened to, as a fraction of the em. */
export const FONT_ERROR_EM = 2 / FACE_SIZE;
/** `FT_Outline_Embolden( .., 1 << 6 )`: one pixel at `FT_Set_Char_Size( face, 0, faceSize(), GLYPH_RESOLUTION (1152), 0 )`, `faceSize() / 64 * 1152 / 72` pixels to the em. */
export const FAKE_BOLD_EM = 1 / ((FACE_SIZE / 64) * (1152 / 72));
const ITALIC = (12 * Math.PI) / 180;

/**
 * The polygons of `text`'s glyphs at an em of 1, from the run's origin on the baseline, y down: each glyph's outline (`outlineOf`) at the advance `advanceTo` gives the text
 * before it, slanted by 12 degrees for a fake italic, emboldened for a fake bold, flattened and sorted into outlines with their holes. Undefined when the font's outlines are
 * not read here.
 */
export function sfntGlyphPolys(f: SfntFace, text: string, advanceTo: (prefix: string) => number, fakeItalic = false, fakeBold = false): GlyphPoly[] | undefined {
  const chars = Array.from(text);
  const out: GlyphPoly[] = [];
  const upem = f.unitsPerEm || 1000;
  const [sin, cos] = [Math.sin(ITALIC), Math.cos(ITALIC)];
  for (let i = 0; i < chars.length; i++) {
    let contours = f.outlineOf(f.glyphOf(chars[i]!.codePointAt(0)!));
    if (contours === undefined) return undefined;
    if (!contours) continue;
    // in ems, y up
    contours = contours.map((c) => c.map((p) => ({ x: p.x / upem, y: p.y / upem, tag: p.tag })));
    if (fakeItalic) contours = contours.map((c) => c.map((p) => ({ x: p.x * cos + p.y * sin, y: p.y, tag: p.tag })));
    if (fakeBold) contours = ftEmbolden(contours, FAKE_BOLD_EM);
    const flat = ftDecompose(contours, (ctrl) => bezierPolyD(ctrl, FONT_ERROR_EM));
    const x = advanceTo(chars.slice(0, i).join(""));
    const place = (c: Array<[number, number]>) => c.map(([u, v]) => [x + u, -v] as [number, number]);
    for (const g of glyphPolys(flat, ftOrientation(contours))) out.push({ outer: place(g.outer), holes: g.holes.map(place) });
  }
  return out;
}
