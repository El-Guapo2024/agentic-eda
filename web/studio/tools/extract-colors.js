#!/usr/bin/env node
// Extracts the "KiCad Default" PCB color theme into src/kicad/colors.json.
//
//   KICAD_SRC_DIR=/path/to/plain/checkout node tools/extract-colors.js
//
// common/settings/builtin_color_themes.h defines every built-in theme as
// a `static const std::map<int, COLOR4D> s_<name>Theme = { ... };`
// initializer -- "KiCad Default" is `s_defaultTheme` (the first one in
// the file; "Classic" is `s_classicTheme`, right after it). Each entry is
// `{ LAYER_ID, CSS_COLOR( r, g, b, a ) }` (r/g/b as 0..255 integers, a as
// 0..1) or, less often, `{ LAYER_ID, COLOR4D( r, g, b, a ) }` (all 0..1
// floats) -- both are handled. A couple of entries are wrapped in
// `#ifdef __WXMAC__ ... #else ... #endif` for a platform-specific value;
// since this app targets macOS chrome, the __WXMAC__ branch is kept and
// the #else branch dropped for those.
//
// This app's own semantic layer buckets (background/f_cu/b_cu/silks/...,
// see components/canvas/layers.ts) are NOT the same set as KiCad's
// PCB_LAYER_ID/GAL_LAYER_ID names this file writes -- the mapping
// between them lives in layers.ts, not here. This script's only job is
// a faithful id -> color dump.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "colors.json");
const SOURCE_FILE = "common/settings/builtin_color_themes.h";

/** The exact span of `static const std::map<int, COLOR4D> <mapName> = { ... };`, brace-balanced (entries are themselves `{ ... }` pairs, so a naive first-`}` match would truncate after the first entry). */
function findMapBody(text, mapName) {
  const declRe = new RegExp(`std::map<int,\\s*COLOR4D>\\s+${mapName}\\s*=`);
  const declMatch = declRe.exec(text);
  if (!declMatch) return null;
  const braceStart = text.indexOf("{", declMatch.index);
  if (braceStart === -1) return null;
  let depth = 0;
  for (let i = braceStart; i < text.length; i++) {
    if (text[i] === "{") depth++;
    else if (text[i] === "}") {
      depth--;
      if (depth === 0) return text.slice(braceStart, i + 1);
    }
  }
  return null;
}

/** Keep the __WXMAC__ branch of any `#ifdef __WXMAC__ / #else / #endif` block, dropping the #else branch entirely (this app's chrome targets macOS). Any other #if/#ifdef is left as-is (both branches' entries stay, which just means "last one wins" if they share a key -- rare enough in this file not to special-case). */
function preferMacBranches(text) {
  const lines = text.split("\n");
  const out = [];
  let inMacIf = false;
  let inMacElse = false;
  for (const line of lines) {
    if (/^\s*#ifdef\s+__WXMAC__/.test(line)) {
      inMacIf = true;
      continue;
    }
    if (inMacIf && /^\s*#else\b/.test(line)) {
      inMacIf = false;
      inMacElse = true;
      continue;
    }
    if (inMacElse && /^\s*#endif/.test(line)) {
      inMacElse = false;
      continue;
    }
    if (inMacElse) continue; // drop the non-mac branch's lines
    out.push(line);
  }
  return out.join("\n");
}

function clampByte(n) {
  return Math.max(0, Math.min(255, Math.round(n)));
}
function hex2(n) {
  return clampByte(n).toString(16).padStart(2, "0");
}
function hexFromUnit(f) {
  return hex2(parseFloat(f) * 255);
}

function parseColorEntries(body) {
  const colors = {};
  const reCss = /\{\s*([A-Za-z_]\w*)\s*,\s*CSS_COLOR\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*\)\s*\}/g;
  let m;
  while ((m = reCss.exec(body))) {
    const [, id, r, g, b, a] = m;
    colors[id] = `#${hex2(r)}${hex2(g)}${hex2(b)}${hexFromUnit(a)}`;
  }
  const reColor4d = /\{\s*([A-Za-z_]\w*)\s*,\s*COLOR4D\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*\)\s*\}/g;
  while ((m = reColor4d.exec(body))) {
    const [, id, r, g, b, a] = m;
    if (colors[id]) continue; // shouldn't collide with a CSS_COLOR entry for the same id; if it does, first (CSS_COLOR) wins
    colors[id] = `#${hexFromUnit(r)}${hexFromUnit(g)}${hexFromUnit(b)}${hexFromUnit(a)}`;
  }
  return colors;
}

function main() {
  const text = readKicadFile(SOURCE_FILE);
  const body = findMapBody(text, "s_defaultTheme");
  const notes = [];
  let colors = {};
  if (!body) {
    notes.push("could not find `std::map<int, COLOR4D> s_defaultTheme = { ... }` -- the declaration shape changed; open the file and update findMapBody()'s regex");
  } else {
    colors = parseColorEntries(preferMacBranches(body));
    if (Object.keys(colors).length === 0) notes.push("found s_defaultTheme but matched 0 CSS_COLOR()/COLOR4D() entries inside it -- check parseColorEntries() against the actual entry syntax");
  }
  writeJson(OUT, { meta: meta([SOURCE_FILE], notes.join("; ") || undefined), themeName: "KiCad Default", colors });
  console.log(`${Object.keys(colors).length} color(s) from ${SOURCE_FILE}`);
}

main();
