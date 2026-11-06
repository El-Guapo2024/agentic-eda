// The colour themes of the board editor (colorTheme.ts): KiCad's registry, the built-ins, the Colors page's order, New Theme..., Reset, the swatch menu and the store's text.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  BUILTIN_CLASSIC,
  BUILTIN_DEFAULT,
  DEFAULT_THEME,
  EMPTY_STORED_THEMES,
  PCB_GAL_COLOR_ROWS,
  addTheme,
  builtinThemes,
  dropdownName,
  findTheme,
  parseStoredThemes,
  pcbColorPageKeys,
  pcbColorPageRows,
  resetThemeColors,
  sanitizeThemeName,
  serializeStoredThemes,
  setThemeColors,
  sortedThemes,
  swatchMenu,
  themeColor,
  themeListEntries,
  themeRegistry,
  type ThemeData,
} from "./colorTheme";
import { carryRenamedClass, netclassColorUpdates } from "./appearanceNets";

const colors = JSON.parse(readFileSync("src/kicad/colors.json", "utf8")) as { colors: Record<string, string> };
const themes = JSON.parse(readFileSync("src/kicad/color_themes.json", "utf8")) as { classic: Record<string, string>; boardParams: Array<{ key: string; layer: string }> };
const DATA: ThemeData = { defaults: colors.colors, classic: themes.classic, boardKeys: themes.boardParams.map((p) => p.layer) };

test("the board namespace is COLOR_SETTINGS' `board.*` params, every one with KiCad's default and a Classic colour", () => {
  assert.equal(themes.boardParams.length, 119);
  assert.deepEqual(themes.boardParams[0], { key: "board.anchor", layer: "LAYER_ANCHOR" });
  assert.ok(themes.boardParams.some((p) => p.key === "board.copper.f" && p.layer === "F_Cu"));
  assert.ok(themes.boardParams.some((p) => p.key === "board.track_net_names" && p.layer === "NETNAMES_LAYER_ID_START"));
  for (const p of themes.boardParams) {
    assert.match(DATA.defaults[p.layer] ?? "", /^#[0-9a-f]{8}$/, p.layer);
    assert.match(DATA.classic[p.layer] ?? "", /^#[0-9a-f]{8}$/, p.layer);
  }
  // s_classicTheme in colorRefs()' blue-green-red table: RED is (132, 0, 0), CYAN (0, 132, 132); WithAlpha( 0.6 ) on BLUE
  assert.equal(DATA.classic.F_Cu, "#840000ff");
  assert.equal(DATA.classic.F_SilkS, "#008484ff");
  assert.equal(DATA.classic.LAYER_LOCKED_ITEM_SHADOW, "#00008499");
  assert.equal(DATA.classic.LAYER_PCB_BACKGROUND, "#000000ff");
  assert.equal(DATA.classic.LAYER_VIA_HOLES, "#806600cc"); // COLOR4D( 0.5, 0.4, 0, 0.8 )
});

test("CreateBuiltinColorSettings: KiCad Default and KiCad Classic, read-only; the editor's default theme is 'user', an editable copy named KiCad Default", () => {
  const b = builtinThemes(DATA);
  assert.deepEqual(b.map((t) => [t.filename, t.name, t.readOnly]), [
    [BUILTIN_DEFAULT, "KiCad Default", true],
    [BUILTIN_CLASSIC, "KiCad Classic", true],
  ]);
  const reg = themeRegistry(DATA, EMPTY_STORED_THEMES);
  const user = reg.find((t) => t.filename === DEFAULT_THEME)!;
  assert.equal(user.name, "KiCad Default");
  assert.equal(user.readOnly, false);
  assert.equal(themeColor(user, "F_Cu", DATA), DATA.defaults.F_Cu);
  assert.equal(findTheme(reg, EMPTY_STORED_THEMES.pcbTheme).filename, DEFAULT_THEME);
});

test("GetColorSettings: by filename, then by display name in any case, else the 'user' theme", () => {
  const stored = { pcbTheme: "mine", themes: [{ filename: "mine", name: "My Theme", colors: {} }] };
  const reg = themeRegistry(DATA, stored);
  assert.equal(findTheme(reg, "mine").filename, "mine");
  assert.equal(findTheme(reg, "kicad classic").filename, BUILTIN_CLASSIC);
  assert.equal(findTheme(reg, "gone").filename, DEFAULT_THEME);
});

test("the theme list: sorted by name (case-sensitive), ties in filename order, ' (read-only)' after a built-in, then --- and New Theme...", () => {
  const stored = { pcbTheme: "user", themes: [{ filename: "zeta", name: "Zeta", colors: {} }, { filename: "alpha", name: "alpha", colors: {} }, { filename: "b", name: "Blue", colors: {} }] };
  const reg = themeRegistry(DATA, stored);
  assert.deepEqual(sortedThemes(reg).map((t) => t.filename), ["b", BUILTIN_CLASSIC, BUILTIN_DEFAULT, DEFAULT_THEME, "zeta", "alpha"]);
  assert.deepEqual(themeListEntries(reg).map((e) => e.label), ["Blue", "KiCad Classic (read-only)", "KiCad Default (read-only)", "KiCad Default", "Zeta", "alpha", "---", "New Theme..."]);
  assert.equal(dropdownName(reg.find((t) => t.filename === "zeta")!), "Zeta");
});

test("the Colors page: every copper layer, the technical and user layers, then the GAL layers sorted by LayerName()", () => {
  const rows = pcbColorPageRows((f) => (f === "F.SilkS" ? "F.Silkscreen" : f));
  assert.equal(rows.length, 32 + 18 + 45 + PCB_GAL_COLOR_ROWS.length);
  assert.deepEqual(rows.slice(0, 3).map((r) => r.key), ["F_Cu", "In1_Cu", "In2_Cu"]);
  assert.equal(rows[31]!.key, "B_Cu");
  assert.deepEqual(rows.slice(32, 36).map((r) => r.key), ["F_Adhes", "B_Adhes", "F_Paste", "B_Paste"]);
  assert.equal(rows.find((r) => r.key === "F_SilkS")!.label, "F.Silkscreen");
  assert.equal(rows[50]!.key, "User_1");
  assert.deepEqual(rows.slice(95).map((r) => r.label), [
    "Anchors", "Background", "Board outline area", "Courtyard collision shadow", "Cursor", "DRC errors", "DRC exclusions", "DRC highlighted", "DRC warnings",
    "Drawing sheet", "Grid", "Grid axes", "Helper items", "Locked item shadow", "Non-plated holes", "Pad net names", "Page limits", "Ratsnest",
    "Selection highlight", "Track net names", "Via holes",
  ]);
  // every row but Selection highlight (no `board.*` param in 8303b2ad) is a colour of the board namespace
  const board = new Set(DATA.boardKeys);
  assert.deepEqual(pcbColorPageKeys().filter((k) => !board.has(k)), ["LAYER_SELECT_OVERLAY"]);
});

test("New Theme...: FOOTPRINT_NAME_VALIDATOR's characters dropped, 'Theme already exists!', the current theme's colours copied", () => {
  assert.equal(sanitizeThemeName('my/the:me"%$<>\\'), "mytheme");
  const reg = themeRegistry(DATA, EMPTY_STORED_THEMES);
  const classic = findTheme(reg, BUILTIN_CLASSIC);
  const keys = pcbColorPageKeys();
  const r = addTheme(EMPTY_STORED_THEMES, "Night", classic, keys, DATA);
  assert.equal(r.error, undefined);
  const night = r.stored.themes.find((t) => t.filename === "Night")!;
  assert.equal(night.name, "Night");
  assert.equal(night.colors.F_Cu, DATA.classic.F_Cu);
  assert.equal(night.colors.LAYER_SELECT_OVERLAY, DATA.defaults.LAYER_SELECT_OVERLAY); // the page's row Classic has no colour for: the default
  assert.equal(findTheme(themeRegistry(DATA, r.stored), "Night").readOnly, false);
  assert.equal(addTheme(r.stored, "Night", classic, keys, DATA).error, "Theme already exists!");
  assert.equal(addTheme(r.stored, "user", classic, keys, DATA).error, "Theme already exists!");
});

test("a colour set on an editable theme is kept; a read-only one is left alone; 'user' is written the first time it changes", () => {
  const reg = themeRegistry(DATA, EMPTY_STORED_THEMES);
  assert.equal(setThemeColors(EMPTY_STORED_THEMES, reg, BUILTIN_DEFAULT, { F_Cu: "#00ff00ff" }), EMPTY_STORED_THEMES);
  const s1 = setThemeColors(EMPTY_STORED_THEMES, reg, DEFAULT_THEME, { F_Cu: "#00ff00ff" });
  assert.deepEqual(s1.themes, [{ filename: DEFAULT_THEME, name: "KiCad Default", colors: { F_Cu: "#00ff00ff" } }]);
  const s2 = setThemeColors(s1, themeRegistry(DATA, s1), DEFAULT_THEME, { B_Cu: "#0000ffff" });
  assert.deepEqual(s2.themes[0]!.colors, { F_Cu: "#00ff00ff", B_Cu: "#0000ffff" });
  assert.equal(themeColor(findTheme(themeRegistry(DATA, s2), DEFAULT_THEME), "F_Cu", DATA), "#00ff00ff");
});

test("ResetPanel: every colour of the page back to KiCad's default; nothing on a read-only theme", () => {
  const reg = themeRegistry(DATA, { pcbTheme: "x", themes: [{ filename: "x", name: "X", colors: { F_Cu: "#123456ff", LAYER_GRID: "#ffffffff" } }] });
  const x = findTheme(reg, "x");
  const reset = resetThemeColors(x, pcbColorPageKeys(), DATA);
  assert.equal(reset.colors.F_Cu, DATA.defaults.F_Cu);
  assert.equal(reset.colors.LAYER_GRID, DATA.defaults.LAYER_GRID);
  const classic = findTheme(reg, BUILTIN_CLASSIC);
  assert.equal(resetThemeColors(classic, pcbColorPageKeys(), DATA), classic);
});

test("ShowColorContextMenu: Copy color; Paste color once copied on an editable theme; Revert to saved color when changed", () => {
  assert.deepEqual(swatchMenu(false, null, "#1", "#1"), ["copy"]);
  assert.deepEqual(swatchMenu(false, "#2", "#1", "#1"), ["copy", "paste"]);
  assert.deepEqual(swatchMenu(false, "#2", "#1", "#3"), ["copy", "paste", "revert"]);
  assert.deepEqual(swatchMenu(true, "#2", "#1", "#3"), ["copy"]);
});

test("the store's text round-trips; malformed entries are dropped", () => {
  const s = { pcbTheme: "Night", themes: [{ filename: "Night", name: "Night", colors: { F_Cu: "#840000ff" } }] };
  assert.deepEqual(parseStoredThemes(serializeStoredThemes(s)), s);
  assert.deepEqual(parseStoredThemes(null), EMPTY_STORED_THEMES);
  assert.deepEqual(parseStoredThemes("{not json"), EMPTY_STORED_THEMES);
  const odd = parseStoredThemes(JSON.stringify({ pcbTheme: 3, themes: [{ filename: "", name: "x" }, { filename: "a", name: "A", colors: { F_Cu: "red", B_Cu: "#ABCDEF12" } }, { filename: "a", name: "dup" }, 7] }));
  assert.deepEqual(odd, { pcbTheme: DEFAULT_THEME, themes: [{ filename: "a", name: "A", colors: { B_Cu: "#abcdef12" } }] });
});

test("a net class renamed on the Net Classes page keeps its colour under the new name (the colour is a cell of its row)", () => {
  const stored = { power: "rgb(255, 160, 0)", usb: "rgb(0, 0, 255)" };
  // typed one key at a time: power -> powe -> pwr is two renames of the same row
  let draft = carryRenamedClass(stored, "power", "powe", ["usb"]);
  draft = carryRenamedClass(draft, "powe", "pwr", ["usb"]);
  assert.deepEqual(draft, { pwr: "rgb(255, 160, 0)", usb: "rgb(0, 0, 255)" });
  assert.deepEqual(netclassColorUpdates(stored, draft, ["pwr", "usb"]), [
    { name: "power", color: null },
    { name: "pwr", color: "rgb(255, 160, 0)" },
  ]);
  // another class still named the old name keeps the entry; a removed class loses its colour
  assert.deepEqual(carryRenamedClass(stored, "usb", "usb2", ["usb"]), stored);
  assert.deepEqual(netclassColorUpdates(stored, stored, ["power"]), [{ name: "usb", color: null }]);
  assert.deepEqual(netclassColorUpdates(stored, stored, ["power", "usb"]), []);
});
