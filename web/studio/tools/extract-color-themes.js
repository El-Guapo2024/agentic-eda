#!/usr/bin/env node
// Extracts what the PCB colour themes need beyond the "KiCad Default" colours of src/kicad/colors.json (tools/extract-colors.js) into
// src/kicad/color_themes.json:
//
//   KICAD_SRC_DIR=/path/to/plain/checkout node tools/extract-color-themes.js [--check]
//
//   * `boardParams`: the board namespace of a theme file, in `COLOR_SETTINGS::COLOR_SETTINGS`' order (common/settings/color_settings.cpp:
//     `CLR( "board.anchor", LAYER_ANCHOR )`, ...): the JSON key KiCad writes a colour under and the layer it is the colour of. These are the
//     colours a PCB theme holds (`SaveColorSettings( cs, "board" )` stores this namespace and nothing else), so they are the layers the board
//     editor's theme decides.
//   * `classic`: the "KiCad Classic" built-in theme (`s_classicTheme`, common/settings/builtin_color_themes.h) for those layers, as `#rrggbbaa`.
//     Its entries name the legacy palette (`COLOR4D( BLUE )`, `COLOR4D( RED ).WithAlpha( 0.5 )`), which is `colorRefs()`'s table
//     (common/gal/color4d.cpp, stored blue, green, red -- `StructColors`' field order) read through `COLOR4D::COLOR4D( EDA_COLOR_T )`
//     (alpha 1); `CSS_COLOR( r, g, b, a )` and `COLOR4D( r, g, b, a )` are read as extract-colors.js reads them. Where the file has a
//     `#ifdef __WXMAC__` branch the Mac one is kept, as extract-colors.js does.
//
// `--check` compares with the file on disk and exits 1 when they differ (nothing is written).
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readFileSync } from "node:fs";
import { readKicadFile, meta, writeJson } from "./lib/kicadSource.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src", "kicad", "color_themes.json");
const THEMES_H = "common/settings/builtin_color_themes.h";
const SETTINGS_CPP = "common/settings/color_settings.cpp";
const COLOR4D_CPP = "common/gal/color4d.cpp";
const COLOR4D_H = "include/gal/color4d.h";

const hex2 = (n) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, "0");

/** The brace-balanced body of `static const std::map<int, COLOR4D> <name> = { ... };`. */
function mapBody(text, name) {
  const m = new RegExp(`std::map<int,\\s*COLOR4D>\\s+${name}\\s*=`).exec(text);
  if (!m) throw new Error(`no ${name} in ${THEMES_H}`);
  const start = text.indexOf("{", m.index);
  let depth = 0;
  for (let i = start; i < text.length; i++) {
    if (text[i] === "{") depth++;
    else if (text[i] === "}" && --depth === 0) return text.slice(start, i + 1);
  }
  throw new Error(`unbalanced ${name}`);
}

/** Keeps the `#ifdef __WXMAC__` branch of a `#ifdef __WXMAC__ / #else / #endif` block (extract-colors.js' rule). */
function preferMac(text) {
  const out = [];
  let mode = "";
  for (const line of text.split("\n")) {
    if (/^\s*#ifdef\s+__WXMAC__/.test(line)) mode = "mac";
    else if (mode === "mac" && /^\s*#else\b/.test(line)) mode = "else";
    else if (mode && /^\s*#endif/.test(line)) mode = "";
    else if (mode !== "else") out.push(line);
  }
  return out.join("\n");
}

/** `enum EDA_COLOR_T`'s names in order (BLACK = 0, ...). */
function edaColorNames() {
  const h = readKicadFile(COLOR4D_H);
  const body = /enum EDA_COLOR_T\s*\{([\s\S]*?)\};/.exec(h)?.[1];
  if (!body) throw new Error("no enum EDA_COLOR_T");
  const names = [];
  for (const raw of body.split("\n")) {
    const line = raw.replace(/\/\/.*$/, "").trim();
    const m = /^([A-Z_]+)\s*(?:=\s*(-?\d+))?\s*,?/.exec(line);
    if (!m) continue;
    if (m[1] === "UNSPECIFIED_COLOR") continue;
    if (m[1] === "NBCOLORS") break;
    names.push(m[1]);
  }
  return names;
}

/** `colorRefs()`: EDA_COLOR_T name -> `#rrggbb` (the table lists blue, green, red). */
function colorRefs() {
  const cpp = readKicadFile(COLOR4D_CPP);
  const table = /s_ColorRefs\[NBCOLORS\]\s*=\s*\{([\s\S]*?)\n\s*\};/.exec(cpp)?.[1];
  if (!table) throw new Error("no s_ColorRefs");
  const out = {};
  const re = /\{\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([A-Z_]+)\s*,/g;
  let m;
  while ((m = re.exec(table))) out[m[4]] = `#${hex2(+m[3])}${hex2(+m[2])}${hex2(+m[1])}`;
  return out;
}

/** One theme entry's colour as `#rrggbbaa`, or null when it is `UNSPECIFIED_COLOR` or a form this reader does not know. */
function entryColor(expr, refs) {
  let m = /^CSS_COLOR\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*\)$/.exec(expr);
  if (m) return `#${hex2(+m[1])}${hex2(+m[2])}${hex2(+m[3])}${hex2(+m[4] * 255)}`;
  m = /^COLOR4D\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*\)$/.exec(expr);
  if (m) return `#${hex2(+m[1] * 255)}${hex2(+m[2] * 255)}${hex2(+m[3] * 255)}${hex2(+m[4] * 255)}`;
  m = /^COLOR4D\(\s*([A-Z_]+)\s*\)(?:\.WithAlpha\(\s*([\d.]+)\s*\))?$/.exec(expr);
  if (m) {
    if (m[1] === "UNSPECIFIED_COLOR") return null;
    const rgb = refs[m[1]];
    if (!rgb) throw new Error(`unknown EDA_COLOR_T ${m[1]}`);
    return `${rgb}${hex2((m[2] === undefined ? 1 : +m[2]) * 255)}`;
  }
  return null;
}

/** `CLR( "board.xxx", LAYER )` of `COLOR_SETTINGS::COLOR_SETTINGS`, in order. */
function boardParams() {
  const cpp = readKicadFile(SETTINGS_CPP);
  const out = [];
  const re = /CLR\(\s*"(board\.[\w.]+)"\s*,\s*([A-Za-z_]\w*)\s*\)/g;
  let m;
  while ((m = re.exec(cpp))) out.push({ key: m[1], layer: m[2] });
  return out;
}

function main() {
  const check = process.argv.includes("--check");
  const params = boardParams();
  const wanted = new Set(params.map((p) => p.layer));
  const refs = colorRefs();
  const names = edaColorNames();
  for (const n of names) if (!refs[n]) throw new Error(`colorRefs() has no ${n}`);
  const body = preferMac(mapBody(readKicadFile(THEMES_H), "s_classicTheme"));
  const classic = {};
  const skipped = [];
  const re = /\{\s*([A-Za-z_]\w*)\s*,\s*((?:CSS_COLOR|COLOR4D)\([^{}]*?\)(?:\.WithAlpha\(\s*[\d.]+\s*\))?)\s*\}/g;
  let m;
  while ((m = re.exec(body))) {
    if (!wanted.has(m[1])) continue;
    const c = entryColor(m[2].replace(/\s+/g, " ").replace(/\( /g, "( ").trim(), refs);
    if (c) classic[m[1]] = c;
    else skipped.push(`${m[1]}: ${m[2]}`);
  }
  const notes = skipped.length ? `not read: ${skipped.join("; ")}` : undefined;
  const data = { meta: meta([SETTINGS_CPP, THEMES_H, COLOR4D_CPP, COLOR4D_H], notes), classicName: "KiCad Classic", boardParams: params, classic };
  if (check) {
    const disk = JSON.parse(readFileSync(OUT, "utf8"));
    const same = JSON.stringify({ ...disk, meta: undefined }) === JSON.stringify({ ...data, meta: undefined });
    console.log(same ? "color_themes.json is current" : "color_themes.json differs from the source");
    process.exit(same ? 0 : 1);
  }
  writeJson(OUT, data);
  console.log(`${params.length} board colour keys, ${Object.keys(classic).length} KiCad Classic colours${notes ? ` (${notes})` : ""}`);
}

main();
