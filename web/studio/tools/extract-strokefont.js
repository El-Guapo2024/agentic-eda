#!/usr/bin/env node
// Extracts KiCad's Newstroke font glyph data into src/kicad/strokeFont.json.
//
//   KICAD_SRC_DIR=/path/to/plain/checkout node tools/extract-strokefont.js
//
// common/newstroke_font.cpp defines `const char* const newstroke_font[] =
// { ... };`, one C string literal per glyph, covering dozens of Unicode
// blocks back-to-back (Basic Latin, Latin-1, Greek, Cyrillic, Arabic,
// most of the world's scripts, and finally a huge CJK Unicode Ideographs
// tail) -- 65000+ lines, almost all of it scripts this app has no use
// for (no CJK/RTL/Indic text appears anywhere in this app's data model).
// This script keeps only the ranges an English-language PCB/schematic
// tool actually needs: Basic Latin (0020-007F), Latin-1 Supplement
// (0080-00FF -- degree/plus-minus/micro/multiply/divide signs used in
// real component values like "10µF" or "±5%"), and Greek and Coptic
// (0370-03FF, for Ω -- the ohm sign, U+03A9, is genuinely common in
// resistor values). Every block in the source file is preceded by a
// `/* // NAME (START-END) */` comment giving its own Unicode range in
// hex, which this script reads to know each entry's real codepoint
// (rather than assuming the whole array is contiguous from U+0020,
// which stops being true after Basic Latin -- see e.g. "Cyrillic
// Supplement (500-52F)" starting well past where 0x1xx-range entries
// would put it if codepoints were just "start at 0x20 and count up").
//
// Output shape: { meta, glyphs: { "<decimal codepoint>": "<raw stroke
// string, exactly as it appears in the C array>" } }. Decoding that
// string into strokes (the `<startX><endX>` pair, then coordinate pairs
// separated by a " R" pen-lift) is src/components/text/strokeFont.ts's
// job, ported from common/font/stroke_font.cpp's loadNewStrokeFont --
// this script does not interpret the glyph data at all, only extracts
// the raw per-codepoint strings, so the decoder has exactly one place
// to live and one thing to be tested against.
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "strokeFont.json");
const SOURCE_FILE = "common/newstroke_font.cpp";

// [start, end] inclusive, hex codepoints -- see header comment for why these three.
const KEEP_RANGES = [
  [0x0020, 0x007f], // Basic Latin
  [0x0080, 0x00ff], // Latin-1 Supplement
  [0x0370, 0x03ff], // Greek and Coptic
];

function keepCodepoint(cp) {
  return KEEP_RANGES.some(([lo, hi]) => cp >= lo && cp <= hi);
}

/** One C string literal's content, unescaped (this font data only ever uses `\\` for a literal backslash -- e.g. some glyphs' own stroke coordinates -- so that's the only escape handled; anything else throws rather than silently mis-decoding). */
function decodeCString(literal) {
  return literal.replace(/\\(.)/g, (whole, ch) => {
    if (ch === "\\") return "\\";
    if (ch === '"') return '"';
    throw new Error(`unhandled C string escape '\\${ch}' in: ${literal}`);
  });
}

function extractArrayBody(text) {
  const declRe = /const\s+char\*\s+const\s+newstroke_font\[\]\s*=\s*\n?\s*\{/;
  const m = declRe.exec(text);
  if (!m) throw new Error("could not find `const char* const newstroke_font[] = {` -- the declaration shape changed");
  const braceStart = text.indexOf("{", m.index);
  // Brace-depth match, same technique as extract-colors.js's findMapBody --
  // the glyph strings themselves can contain '{'/'}' as stroke coordinate
  // characters (e.g. the placeholder glyph "F^K[KFYFY[K[" doesn't, but
  // several real letters like '{' '[' do use those literal characters as
  // *data*, inside a quoted string) -- so this must skip over quoted
  // string contents entirely while counting braces, not just count every
  // '{'/'}' byte in the file.
  let depth = 0;
  let inString = false;
  for (let i = braceStart; i < text.length; i++) {
    const c = text[i];
    if (inString) {
      if (c === "\\") i++; // skip the escaped character
      else if (c === '"') inString = false;
      continue;
    }
    if (c === '"') inString = true;
    else if (c === "{") depth++;
    else if (c === "}") {
      depth--;
      if (depth === 0) return text.slice(braceStart + 1, i);
    }
  }
  throw new Error("unbalanced braces in newstroke_font[] array body");
}

function parseGlyphs(body) {
  const glyphs = {};
  let currentCp = null;
  let count = 0;
  let kept = 0;
  const lines = body.split("\n");
  const sectionRe = /^\s*\/\*\s*\/\/\s*.*?\(([0-9A-Fa-f]+)[-–]([0-9A-Fa-f]+)\)\s*\*\/\s*$/;
  const stringRe = /^\s*"((?:[^"\\]|\\.)*)"\s*,?\s*(?:\/\*.*\*\/)?\s*$/;

  for (const line of lines) {
    if (!line.trim()) continue;
    const section = sectionRe.exec(line);
    if (section) {
      currentCp = Number.parseInt(section[1], 16);
      continue;
    }
    const str = stringRe.exec(line);
    if (str) {
      if (currentCp === null) throw new Error(`glyph string before any section header: ${line}`);
      if (keepCodepoint(currentCp)) {
        glyphs[String(currentCp)] = decodeCString(str[1]);
        kept++;
      }
      currentCp++;
      count++;
      continue;
    }
    // A handful of section markers don't carry a "(START-END)" hex range
    // (e.g. "/* // Skip to CJK */", "/* // - (18B0-18FF) */" has one but
    // some others are prose-only) -- harmless to skip since they only
    // ever occur well past every KEEP_RANGES block this script cares
    // about, but still asserted to be a comment line (not silently
    // swallowing a mis-parsed glyph string) rather than unconditionally
    // ignored.
    if (/^\s*\/\*.*\*\/\s*$/.test(line)) continue;
    // Anything else is unexpected inside the array body -- fail loudly
    // instead of silently mis-numbering every codepoint after it.
    throw new Error(`unrecognized line inside newstroke_font[] body: ${JSON.stringify(line)}`);
  }
  return { glyphs, count, kept };
}

function main() {
  const text = readKicadFile(SOURCE_FILE);
  const body = extractArrayBody(text);
  const { glyphs, count, kept } = parseGlyphs(body);

  const expectedKept = KEEP_RANGES.reduce((sum, [lo, hi]) => sum + (hi - lo + 1), 0);
  const notes = [];
  if (kept !== expectedKept) {
    notes.push(`expected ${expectedKept} glyphs across the kept ranges, got ${kept} -- a range in KEEP_RANGES fell partly outside what the source file actually defines, or the section headers shifted; check the gap`);
  }

  writeJson(OUT, {
    meta: meta([SOURCE_FILE], notes.join("; ") || undefined),
    keepRanges: KEEP_RANGES.map(([lo, hi]) => [lo, hi]),
    glyphs,
  });
  console.log(`${kept} glyph(s) kept out of ${count} total entries in newstroke_font[]`);
}

main();
