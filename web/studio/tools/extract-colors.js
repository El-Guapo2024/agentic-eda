#!/usr/bin/env node
// Extracts the "KiCad Default" PCB color theme into src/kicad/colors.json.
// Run from a terminal with git access to ~/ws/kicad-mirror:
//
//   node tools/extract-colors.js
//
// NOT YET VALIDATED against real KiCad source. This assumes
// common/settings/builtin_color_themes.h defines the default theme as a
// brace-initializer list of `{ LAYER_ID, COLOR4D( r, g, b, a ) }` pairs
// (r/g/b/a as 0..1 floats) -- KiCad's long-standing pattern for this file,
// but not confirmed for the pinned commit, and there may be more than one
// theme in the file (the task wants specifically "KiCad Default", not
// e.g. a "Classic" or high-contrast theme also defined nearby). Run this,
// check `themeName`/the surrounding C++ for which block was actually
// matched, and adjust `THEME_START_PATTERN` below if it grabbed the wrong
// one.

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "colors.json");
const SOURCE_FILE = "common/settings/builtin_color_themes.h";

// Matches the *first* named theme variable/comment in the file that looks
// like the default one. KiCad has named this differently across
// versions ("g_DefaultTheme", "defaultTheme", a COLOR_SETTINGS with
// name "KiCad Default", ...) -- try a few, first match wins.
const THEME_START_PATTERN = /(KiCad Default|g_DefaultTheme|defaultTheme|DEFAULT_THEME)/;

function toHex(floatStr) {
  const v = Math.round(Math.max(0, Math.min(1, parseFloat(floatStr))) * 255);
  return v.toString(16).padStart(2, "0");
}

function parseColors(fileText) {
  const startMatch = THEME_START_PATTERN.exec(fileText);
  const scanFrom = startMatch ? startMatch.index : 0;
  const section = fileText.slice(scanFrom);
  // Stop at the next theme-ish marker so a second theme's colors (e.g. a
  // "Classic" or high-contrast variant) don't get merged in. If none is
  // found, `end` is +Infinity and we just scan to EOF -- acceptable if
  // this file only defines one theme, wrong if it defines several after
  // this point; check the count against pcbnew's known layer count.
  const nextMarker = THEME_START_PATTERN.exec(section.slice(1));
  const end = nextMarker ? nextMarker.index + 1 : section.length;
  const body = section.slice(0, end);

  const colors = {};
  const re = /\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*,\s*COLOR4D\s*\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*\)\s*\}/g;
  let m;
  while ((m = re.exec(body))) {
    const [, id, r, g, b, a] = m;
    colors[id] = `#${toHex(r)}${toHex(g)}${toHex(b)}${toHex(a)}`;
  }
  return colors;
}

function main() {
  const text = readKicadFile(SOURCE_FILE);
  const colors = parseColors(text);
  const count = Object.keys(colors).length;
  writeJson(OUT, {
    meta: meta([SOURCE_FILE], count === 0 ? "No `{ LAYER_ID, COLOR4D(...) }` pairs matched -- the file's actual structure differs from this script's assumption; open it and rewrite parseColors()." : undefined),
    themeName: "KiCad Default",
    colors,
  });
  console.log(`${count} color(s) from ${SOURCE_FILE}`);
}

main();
