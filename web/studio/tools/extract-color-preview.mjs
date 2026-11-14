#!/usr/bin/env node
// The preview board of Preferences > PCB Editor > Colors (`g_previewBoard`, pcbnew/dialogs/panel_pcbnew_color_settings.cpp at KiCad 8303b2ad): the C string written
// out as the `.kicad_pcb` it is, for `eda import-kicad` to read (`PANEL_PCBNEW_COLOR_SETTINGS::createPreviewItems` loads it with `PCB_IO_KICAD_SEXPR::DoLoad`).
//
//   KICAD_SRC_DIR=$HOME/src/kicad-src-8303b2ad node tools/extract-color-preview.mjs <out.kicad_pcb>
//
// With a second argument it also writes the preview sheet of Preferences > Schematic Editor > Colors (`PANEL_EESCHEMA_COLOR_SETTINGS::createPreviewItems`, which builds
// its items in code -- "It would be nice to parse a schematic file here.") as the `.kicad_sch` of the same items (`previewSheet` below).
//
// The studio's preview data are then what its own server makes of them: `eda import-kicad <file> -o <dir>`, `eda board serve -C <dir>`, and `GET /api/board`
// (the board) or `GET /api/schematic` (the sheet) saved as src/kicad/color_preview_board.json / color_preview_sheet.json (tools/make-color-preview.sh does it all).
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const src = process.env.KICAD_SRC_DIR;
if (!src || !process.argv[2]) {
  console.error("usage: KICAD_SRC_DIR=<kicad source> node tools/extract-color-preview.mjs <out.kicad_pcb> [<out.kicad_sch>]");
  process.exit(2);
}
const cpp = readFileSync(join(src, "pcbnew/dialogs/panel_pcbnew_color_settings.cpp"), "utf8");
const start = cpp.indexOf("std::string g_previewBoard =");
const end = cpp.indexOf(";\n", start);
const body = cpp.slice(start, end);
// the adjacent string literals, joined, their escapes (\" \\ \n \t) undone
const text = [...body.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1].replace(/\\(.)/g, (_, c) => (c === "n" ? "\n" : c === "t" ? "\t" : c))).join("");
writeFileSync(process.argv[2], text);
console.log(`wrote ${process.argv[2]} (${text.length} bytes)`);

// ---------------------------------------------------------------------------------------------- the preview sheet

/** `MILS_POINT`: mils to the file's millimetres. */
const mm = (mils) => +(mils * 0.0254).toFixed(4);
let uuidSeq = 0;
const uuid = () => `00000000-0000-4000-8000-${String(++uuidSeq).padStart(12, "0")}`;
const xy = (x, y) => `(xy ${mm(x)} ${mm(y)})`;
const at = (x, y, a = 0) => `(at ${mm(x)} ${mm(y)} ${a})`;
const font = (mils) => `(effects (font (size ${mm(mils)} ${mm(mils)})))`;

/**
 * `createPreviewItems`, item by item: a 6000 x 5000 mil page titled "Color Preview"; wires 6 mils wide, buses 12, the notes rectangle's four lines (6 mils, the notes'
 * default dash); a no-connect, two bus entries, "PLAIN TEXT", LABEL_{0} (drawn selected: `t2->SetSelected()`) and LABEL_{1}, GLOBAL[0..3] pointing left,
 * HIER_LABEL, a junction; U1, an OPA604 of a filled triangle and three pins ("-", "+", "OUT", 50 mil texts, the pin names at offset 0) at (2625, 1600); the sheet
 * "SHEET" (/path/to/sheet) at (4000, 1300), 800 x 1300, with its "SHEET PIN" at (4500, 1500). The library symbol's own coordinates are relative to U1's position and
 * y up, as a file holds them (`mapLibItemPosition` is a no-op on the preview's in-memory, y-down ones).
 */
function previewSheet(date) {
  const root = uuid();
  const lines = [
    ["wire", 1950, 1500, 2325, 1500], ["wire", 1950, 2600, 2350, 2600], ["wire", 2150, 1700, 2325, 1700], ["wire", 2150, 2000, 2150, 1700],
    ["wire", 2925, 1600, 3075, 1600], ["wire", 3075, 1600, 3075, 2000], ["wire", 3075, 1600, 3250, 1600], ["wire", 3075, 2000, 2150, 2000],
    ["bus", 1750, 1300, 1850, 1300], ["bus", 1850, 2500, 1850, 1300],
    ["notes", 2350, 2125, 2350, 2300], ["notes", 2350, 2125, 2950, 2125], ["notes", 2950, 2125, 2950, 2300], ["notes", 2950, 2300, 2350, 2300],
  ];
  const p = [2625, 1600];
  // lib coordinates: relative to U1, y up
  const lib = (x, y) => xy(x - p[0], -(y - p[1]));
  const pin = (type, x, y, angle, name, num) =>
    `      (pin ${type} line (at ${mm(x - p[0])} ${mm(-(y - p[1]))} ${angle}) (length ${mm(100)})\n        (name "${name}" ${font(50)})\n        (number "${num}" ${font(50)})\n      )`;
  const out = [];
  out.push(`(kicad_sch (version 20231120) (generator "eeschema") (generator_version "8.0")`);
  out.push(`  (uuid "${root}")`);
  out.push(`  (paper "User" ${mm(6000)} ${mm(5000)})`);
  out.push(`  (title_block (title "Color Preview") (date "${date}"))`);
  out.push(`  (lib_symbols`);
  out.push(`    (symbol "preview:OPA604" (pin_names (offset 0)) (in_bom yes) (on_board yes)`);
  out.push(`      (property "Reference" "U" (at ${mm(30)} ${mm(260)} 0) (effects (font (size 1.27 1.27)) (justify left)))`);
  out.push(`      (property "Value" "OPA604" (at ${mm(30)} ${mm(180)} 0) (effects (font (size 1.27 1.27)) (justify left)))`);
  out.push(`      (symbol "OPA604_0_0"`);
  out.push(`        (polyline (pts ${lib(p[0] - 200, p[1] + 200)} ${lib(p[0] + 200, p[1])} ${lib(p[0] - 200, p[1] - 200)} ${lib(p[0] - 200, p[1] + 200)}) (stroke (width ${mm(10)}) (type solid)) (fill (type background)))`);
  out.push(pin("input", p[0] - 300, p[1] - 100, 0, "-", "1"));
  out.push(pin("input", p[0] - 300, p[1] + 100, 0, "+", "2"));
  out.push(pin("output", p[0] + 300, p[1], 180, "OUT", "3"));
  out.push(`      )`);
  out.push(`    )`);
  out.push(`  )`);
  out.push(`  (junction ${at(3075, 1600).replace(/ 0\)$/, ")")} (diameter 0) (color 0 0 0 0) (uuid "${uuid()}"))`);
  out.push(`  (no_connect ${at(2350, 2600).replace(/ 0\)$/, ")")} (uuid "${uuid()}"))`);
  for (const [x, y] of [[1850, 1400], [1850, 2500]]) out.push(`  (bus_entry ${at(x, y).replace(/ 0\)$/, ")")} (size ${mm(100)} ${mm(100)}) (stroke (width 0) (type default)) (uuid "${uuid()}"))`);
  for (const [kind, x0, y0, x1, y1] of lines) {
    if (kind === "notes") out.push(`  (polyline (pts ${xy(x0, y0)} ${xy(x1, y1)}) (stroke (width ${mm(6)}) (type default)) (uuid "${uuid()}"))`);
    else out.push(`  (${kind} (pts ${xy(x0, y0)} ${xy(x1, y1)}) (stroke (width ${mm(kind === "bus" ? 12 : 6)}) (type solid)) (uuid "${uuid()}"))`);
  }
  out.push(`  (text "PLAIN TEXT" ${at(2650, 2240)} (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid "${uuid()}"))`);
  out.push(`  (label "LABEL_{0}" ${at(1975, 1500)} (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid "00000000-0000-4000-8000-0000000000aa"))`);
  out.push(`  (label "LABEL_{1}" ${at(1975, 2600)} (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid "${uuid()}"))`);
  out.push(`  (global_label "GLOBAL[0..3]" (shape bidirectional) ${at(1750, 1300, 180)} (effects (font (size 1.27 1.27)) (justify right)) (uuid "${uuid()}"))`);
  out.push(`  (hierarchical_label "HIER_LABEL" (shape input) ${at(3250, 1600)} (effects (font (size 1.27 1.27)) (justify left)) (uuid "${uuid()}"))`);
  out.push(`  (symbol (lib_id "preview:OPA604") ${at(p[0], p[1])} (unit 1) (in_bom yes) (on_board yes) (dnp no) (uuid "${uuid()}")`);
  out.push(`    (property "Reference" "U1" ${at(p[0] + 30, p[1] - 260)} (effects (font (size 1.27 1.27)) (justify left)))`);
  out.push(`    (property "Value" "OPA604" ${at(p[0] + 30, p[1] - 180)} (effects (font (size 1.27 1.27)) (justify left)))`);
  out.push(`    (instances (project "preview" (path "/${root}" (reference "U1") (unit 1))))`);
  out.push(`  )`);
  out.push(`  (sheet ${at(4000, 1300).replace(/ 0\)$/, ")")} (size ${mm(800)} ${mm(1300)}) (fields_autoplaced yes) (stroke (width 0) (type solid)) (fill (color 0 0 0 0.0000)) (uuid "${uuid()}")`);
  out.push(`    (property "Sheetname" "SHEET" ${at(4000, 1300 - 15)} (effects (font (size 1.27 1.27)) (justify left bottom)))`);
  out.push(`    (property "Sheetfile" "/path/to/sheet" ${at(4000, 1300 + 1300 + 15)} (effects (font (size 1.27 1.27)) (justify left top)))`);
  out.push(`    (pin "SHEET PIN" input ${at(4500, 1500, 180)} (effects (font (size 1.27 1.27)) (justify left)) (uuid "${uuid()}"))`);
  out.push(`  )`);
  out.push(`  (sheet_instances (path "/" (page "1")))`);
  out.push(`)`);
  return out.join("\n") + "\n";
}

if (process.argv[3]) {
  writeFileSync(process.argv[3], previewSheet(new Date().toISOString().slice(0, 10)));
  console.log(`wrote ${process.argv[3]}`);
}
