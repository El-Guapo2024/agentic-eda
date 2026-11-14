// KiCad's colour theme files (colorThemeFile.ts): COLOR4D's CSS strings both ways, and a real theme file (fixtures/nord.json, see nord.SOURCE.txt) read,
// migrated and written back as COLOR_SETTINGS does.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { COLORS_SCHEMA_VERSION, parseColorString, parseThemeFile, themeFilenameOf, toCssString, writeThemeFile, type ThemeParam } from "./colorThemeFile";
import type { ColorTheme, ThemeData } from "./colorTheme";

const colors = JSON.parse(readFileSync("src/kicad/colors.json", "utf8")) as { colors: Record<string, string> };
const themes = JSON.parse(readFileSync("src/kicad/color_themes.json", "utf8")) as { classic: Record<string, string>; boardParams: ThemeParam[]; schematicParams: ThemeParam[] };
const PARAMS: ThemeParam[] = [...themes.boardParams, ...themes.schematicParams];
const DATA: ThemeData = { defaults: colors.colors, classic: themes.classic, boardKeys: themes.boardParams.map((p) => p.layer), schematicKeys: themes.schematicParams.map((p) => p.layer) };
const NORD = readFileSync("src/kicad-port/fixtures/nord.json", "utf8");

type Json = Record<string, unknown>;
/** Every string leaf of a JSON object by its dotted path. */
function leaves(v: unknown, at = "", out = new Map<string, unknown>()): Map<string, unknown> {
  if (v && typeof v === "object" && !Array.isArray(v)) for (const [k, x] of Object.entries(v as Json)) leaves(x, at ? `${at}.${k}` : k, out);
  else out.set(at, v);
  return out;
}
const asTheme = (t: ReturnType<typeof parseThemeFile>["themes"][number]): ColorTheme => ({ ...t, readOnly: false });

test("wxColour::FromString: rgb() and rgba() in any case, clipped, the alpha rounded; #rrggbb, #rrggbbaa, #rgb, #rgba; a name or junk is not read", () => {
  assert.equal(parseColorString("rgb(179, 71, 77)"), "#b3474dff");
  assert.equal(parseColorString("rgba(102, 179, 255, 0.800)"), "#66b3ffcc");
  assert.equal(parseColorString("RGBA(1,2,3,0.5)"), "#01020380"); // wxRound( 127.5 ) = 128
  assert.equal(parseColorString("rgba( 1 , 2 , 3 , 0.85 )"), "#010203d9"); // wxRound( 216.75 ) = 217
  assert.equal(parseColorString("rgb(300, -5, 10)"), "#ff000aff");
  assert.equal(parseColorString("#123456"), "#123456ff");
  assert.equal(parseColorString("#12345678"), "#12345678");
  assert.equal(parseColorString("#abc"), "#aabbccff");
  assert.equal(parseColorString("#abcd"), "#aabbccdd");
  assert.equal(parseColorString("#12345"), null);
  assert.equal(parseColorString("rgba(1, 2, 3, x)"), null);
  assert.equal(parseColorString("rgb (1, 2, 3)"), null); // the bracket must follow "rgb" ("( %d , ..." has no leading blank)
  // a colour name (`wxTheColourDatabase->Find`: any case, GRAY as GREY), opaque; one it has not is nothing
  assert.equal(parseColorString("red"), "#ff0000ff");
  assert.equal(parseColorString("Light Grey"), "#c0c0c0ff");
  assert.equal(parseColorString("dark slate gray"), "#2f4f4fff");
  assert.equal(parseColorString("rebeccapurple"), null);
  assert.equal(parseColorString(""), null);
});

test("COLOR4D::ToCSSString: rgb() when opaque, else rgba() with the alpha to three places", () => {
  assert.equal(toCssString("#b3474dff"), "rgb(179, 71, 77)");
  assert.equal(toCssString("#66b3ffcc"), "rgba(102, 179, 255, 0.800)");
  assert.equal(toCssString("#ffffff00"), "rgba(255, 255, 255, 0.000)");
  assert.equal(toCssString("#010203d9"), "rgba(1, 2, 3, 0.851)");
  // what is written reads back as the same colour
  for (const c of ["#b3474dff", "#66b3ffcc", "#010203d9", "#ffffff00", "#00000001"]) assert.equal(parseColorString(toCssString(c)), c);
});

test("a real theme file: its name, its board and schematic colours, the override flag, and what the migrations from version 3 set", () => {
  const r = parseThemeFile(NORD, "nord", PARAMS);
  assert.equal(r.error, undefined);
  assert.equal(r.themes.length, 1);
  const t = r.themes[0]!;
  assert.equal(t.filename, "nord");
  assert.equal(t.name, "Nord");
  assert.equal(t.colors.F_Cu, "#b3474dff");
  assert.equal(t.colors.LAYER_PCB_BACKGROUND, "#2e3440ff");
  assert.equal(t.colors.LAYER_WIRE, "#a3be8cff");
  assert.equal(t.colors.LAYER_SELECTION_SHADOWS, "#66b3ffcc");
  assert.equal(t.colors.LAYER_SHEET_BACKGROUND, "#ffffff00");
  assert.equal(t.overrideSchItemColors, false);
  // 3 -> 4: the page limits take the grid's colour; 5 -> 6: DRC highlighted is PUREMAGENTA
  assert.equal(t.colors.LAYER_PAGE_LIMITS, t.colors.LAYER_GRID);
  assert.equal(t.colors.LAYER_SCHEMATIC_PAGE_LIMITS, t.colors.LAYER_SCHEMATIC_GRID);
  assert.equal(t.colors.LAYER_DRC_HIGHLIGHTED, "#ff00ffff");
  // a param the file lacks is not a colour of the theme (KiCad's default is used): the file has no Points (8303b2ad's `board.points`)
  assert.equal(t.colors.LAYER_POINTS, undefined);
  // the keys of older versions, which are no param of 8303b2ad, are kept
  const extra = leaves(t.extra);
  assert.equal(extra.get("board.footprint_text_back"), JSON.parse(NORD).board.footprint_text_back);
  assert.equal(extra.get("schematic.net_name"), JSON.parse(NORD).schematic.net_name);
  assert.ok(![...extra.keys()].some((k) => k === "board.copper.f" || k === "meta.name" || k === "meta.version"), "a param or meta key is not kept twice");
});

test("written back as SaveToFile does: every colour the file had at the same path in KiCad's words, the others kept, KiCad's defaults for what it lacked, sorted keys, version 6", () => {
  const original = leaves(JSON.parse(NORD));
  const t = asTheme(parseThemeFile(NORD, "nord", PARAMS).themes[0]!);
  const text = writeThemeFile(t, PARAMS, DATA);
  const written = JSON.parse(text) as Json;
  const out = leaves(written);
  const migrated = new Set(["board.page_limits", "schematic.page_limits", "board.drc_highlighted", "meta.version", "meta.filename"]);
  for (const [path, v] of original) {
    if (migrated.has(path)) continue;
    if (typeof v === "string" && parseColorString(v)) assert.equal(out.get(path), toCssString(parseColorString(v)!), path);
    else assert.deepEqual(out.get(path), v, path);
  }
  // the colour strings of this file are KiCad's own: they come back character for character
  assert.equal(out.get("board.copper.f"), "rgb(179, 71, 77)");
  assert.equal(out.get("schematic.shadow"), "rgba(102, 179, 255, 0.800)");
  assert.equal(out.get("meta.version"), COLORS_SCHEMA_VERSION);
  assert.equal(out.get("meta.filename"), "nord.json");
  assert.equal(out.get("board.drc_highlighted"), "rgb(255, 0, 255)");
  assert.equal(out.get("board.points"), toCssString(DATA.defaults.LAYER_POINTS!));
  for (const p of PARAMS) assert.ok(out.has(p.key), `${p.key} is written`);
  // nlohmann's std::map order, two spaces of indent, a newline at the end
  assert.deepEqual(Object.keys(written), [...Object.keys(written)].sort());
  assert.deepEqual(Object.keys(written.board as Json), [...Object.keys(written.board as Json)].sort());
  assert.match(text, /^\{\n {2}"board": \{\n {4}"anchor": "rgb\(236, 239, 244\)",\n/);
  assert.ok(text.endsWith("}\n"));
  // reading what was written and writing it again changes nothing
  const again = asTheme(parseThemeFile(text, "nord", PARAMS).themes[0]!);
  assert.deepEqual(again.colors, { ...t.colors, ...Object.fromEntries(PARAMS.filter((p) => !(p.layer in t.colors)).map((p) => [p.layer, DATA.defaults[p.layer]!])) });
  assert.equal(writeThemeFile(again, PARAMS, DATA), text);
});

test("migrateSchema0to1: a version-0 file's fpedit namespace becomes a theme of its own, '<name> (Footprints)'", () => {
  const v0 = JSON.stringify({ meta: { name: "Old", version: 0 }, board: { grid: "rgb(1, 2, 3)" }, fpedit: { grid: "rgb(4, 5, 6)", background: "rgb(9, 9, 9)" } });
  const r = parseThemeFile(v0, "old", PARAMS);
  assert.deepEqual(r.themes.map((t) => [t.filename, t.name]), [["old", "Old"], ["_footprints", "Old (Footprints)"]]);
  assert.equal(r.themes[0]!.colors.LAYER_GRID, "#010203ff");
  assert.equal(r.themes[1]!.colors.LAYER_GRID, "#040506ff");
  assert.equal(r.themes[1]!.colors.LAYER_PCB_BACKGROUND, "#090909ff");
  // 1 -> 2 set the via holes on both
  assert.equal(r.themes[0]!.colors.LAYER_VIA_HOLES, "#806600cc");
  assert.equal(r.themes[1]!.colors.LAYER_VIA_HOLES, "#806600cc");
  assert.equal(leaves(r.themes[0]!.extra).has("fpedit.grid"), false);
});

test("a colour string wxColour cannot read is black; a value that is no string, and a file without a name or version, keep KiCad's defaults", () => {
  const r = parseThemeFile(JSON.stringify({ board: { grid: "no such colour", anchor: "light grey", cursor: 7 }, schematic: { override_item_colors: true } }), "x", PARAMS);
  const t = r.themes[0]!;
  assert.equal(t.name, "KiCad Default");
  assert.equal(t.colors.LAYER_GRID, "#000000ff");
  assert.equal(t.colors.LAYER_ANCHOR, "#c0c0c0ff", "a wxWidgets colour name is read (UI-LEFT-3 C3-12)");
  assert.equal(t.colors.LAYER_CURSOR, undefined);
  assert.equal(t.overrideSchItemColors, true);
  assert.equal(t.colors.LAYER_DRC_HIGHLIGHTED, undefined, "no meta.version: not migrated");
  assert.match(parseThemeFile("{ not json", "x", PARAMS).error ?? "", /not a KiCad color theme/);
  assert.match(parseThemeFile("[1, 2]", "x", PARAMS).error ?? "", /not a KiCad color theme/);
  assert.equal(themeFilenameOf("C:\\Users\\me\\colors\\My Theme.json"), "My Theme");
  assert.equal(themeFilenameOf("nord.JSON"), "nord");
});
