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
  editorThemeName,
  findTheme,
  fpColorPageKeys,
  fpColorPageRows,
  importThemes,
  schColorPageKeys,
  schColorPageRows,
  schSaveColors,
  withEditorTheme,
  withSymbolTheme,
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
const themes = JSON.parse(readFileSync("src/kicad/color_themes.json", "utf8")) as { classic: Record<string, string>; boardParams: Array<{ key: string; layer: string }>; schematicParams: Array<{ key: string; layer: string }> };
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

test("the schematic namespace is COLOR_SETTINGS' `schematic.*` params, each with KiCad's default; Classic's colours where s_classicTheme has one", () => {
  assert.equal(themes.schematicParams.length, 47);
  assert.deepEqual(themes.schematicParams[0], { key: "schematic.anchor", layer: "LAYER_SCHEMATIC_ANCHOR" });
  assert.ok(themes.schematicParams.some((p) => p.key === "schematic.wire" && p.layer === "LAYER_WIRE"));
  assert.ok(themes.schematicParams.some((p) => p.key === "schematic.shadow" && p.layer === "LAYER_SELECTION_SHADOWS"));
  for (const p of themes.schematicParams) assert.match(DATA.defaults[p.layer] ?? "", /^#[0-9a-f]{8}$/, p.layer);
  // CSS_COLOR( 0, 150, 0, 1 ) by default, COLOR4D( GREEN ) -- (0, 132, 0) in colorRefs() -- in Classic; Classic has no page limits and no notes background
  assert.equal(DATA.defaults.LAYER_WIRE, "#009600ff");
  assert.equal(DATA.classic.LAYER_WIRE, "#008400ff");
  assert.equal(DATA.classic.LAYER_SHEET_BACKGROUND, "#ffffff00"); // COLOR4D( WHITE ).WithAlpha( 0.0 )
  assert.equal(DATA.classic.LAYER_SCHEMATIC_PAGE_LIMITS, undefined);
  assert.equal(DATA.classic.LAYER_NOTES_BACKGROUND, undefined);
  // KiCad Default holds both namespaces when the data names the schematic one
  const both: ThemeData = { ...DATA, schematicKeys: themes.schematicParams.map((p) => p.layer) };
  const def = builtinThemes(both)[0]!;
  assert.equal(def.colors.LAYER_WIRE, DATA.defaults.LAYER_WIRE);
  assert.equal(def.colors.F_Cu, DATA.defaults.F_Cu);
});

test("each editor's theme: pcbnew's, eeschema's and the Footprint Editor's own, the Symbol Editor's the schematic editor's unless told otherwise", () => {
  const s0 = EMPTY_STORED_THEMES;
  assert.deepEqual(["pcb", "footprint", "schematic", "symbol"].map((e) => editorThemeName(s0, e as "pcb")), [DEFAULT_THEME, DEFAULT_THEME, DEFAULT_THEME, DEFAULT_THEME]);
  const s1 = withEditorTheme(withEditorTheme(s0, "schematic", BUILTIN_CLASSIC), "footprint", "night");
  assert.equal(editorThemeName(s1, "schematic"), BUILTIN_CLASSIC);
  assert.equal(editorThemeName(s1, "symbol"), BUILTIN_CLASSIC, "m_UseEeschemaColorSettings is on by default");
  assert.equal(editorThemeName(s1, "footprint"), "night");
  assert.equal(editorThemeName(s1, "pcb"), DEFAULT_THEME);
  // "Use theme:" with a theme of its own; back to "Use schematic editor color theme" keeps that m_ColorTheme for later
  const s2 = withSymbolTheme(s1, false, BUILTIN_DEFAULT);
  assert.equal(editorThemeName(s2, "symbol"), BUILTIN_DEFAULT);
  const s3 = withSymbolTheme(s2, true, "ignored");
  assert.equal(editorThemeName(s3, "symbol"), BUILTIN_CLASSIC);
  assert.equal(s3.symTheme, BUILTIN_DEFAULT);
  assert.equal(withEditorTheme(s3, "pcb", DEFAULT_THEME), s3, "nothing changed, the same store");
  // the store's text keeps the new choices; an old store reads as it was written
  assert.deepEqual(parseStoredThemes(serializeStoredThemes(s3)), s3);
  assert.deepEqual(parseStoredThemes(JSON.stringify({ pcbTheme: "x", themes: [] })), { pcbTheme: "x", themes: [] });
});

test("the schematic Colors page: SCH_LAYER_ID_START .. END but the excluded ones, sorted by LayerName(); the sheet's own colours only while overriding", () => {
  const rows = schColorPageRows(false);
  const all = schColorPageRows(true);
  assert.equal(all.length, 48);
  assert.equal(rows.length, 46);
  assert.equal(schColorPageKeys().length, 48);
  assert.ok(!rows.some((r) => r.key === "LAYER_SHEET" || r.key === "LAYER_SHEET_BACKGROUND"));
  assert.deepEqual(all.slice(0, 6).map((r) => r.label), ["Anchors", "Axes (symbol editor only)", "Background", "Bus junctions", "Buses", "Cursor"]);
  assert.ok(all.some((r) => r.label === "Schematic text & graphics" && r.key === "LAYER_NOTES"));
  for (const excluded of ["LAYER_NOTES_BACKGROUND", "LAYER_DANGLING", "LAYER_NET_COLOR_HIGHLIGHT", "LAYER_GROUP"]) assert.ok(!schColorPageKeys().includes(excluded), excluded);
  // "Sheet references" and "Shape fills" have no param: no default to reset to
  for (const k of ["LAYER_INTERSHEET_REFS", "LAYER_SHAPES_BACKGROUND"]) assert.equal(DATA.defaults[k], undefined, k);
  // every other row is a colour of the schematic namespace
  const sch = new Set(themes.schematicParams.map((p) => p.layer));
  assert.deepEqual(schColorPageKeys().filter((k) => !sch.has(k)), ["LAYER_INTERSHEET_REFS", "LAYER_SHAPES_BACKGROUND"]);
  // saveCurrentTheme: no layer but the backgrounds is saved pure white
  assert.deepEqual(schSaveColors({ LAYER_WIRE: "#ffffffff", LAYER_SCHEMATIC_BACKGROUND: "#ffffffff", LAYER_SHEET_BACKGROUND: "#ffffffff", LAYER_BUS: "#ffffff80" }), {
    LAYER_WIRE: "#fcfcfcff",
    LAYER_SCHEMATIC_BACKGROUND: "#ffffffff",
    LAYER_SHEET_BACKGROUND: "#ffffffff",
    LAYER_BUS: "#ffffff80",
  });
});

test("the Footprint Editor's Colors page: F.Cu, Internal Layers, B.Cu, then the GAL layers with a board colour but the vias' and holes', by name", () => {
  const rows = fpColorPageRows();
  assert.deepEqual(rows.slice(0, 3).map((r) => [r.key, r.label]), [["F_Cu", "F.Cu"], ["In1_Cu", "Internal Layers"], ["B_Cu", "B.Cu"]]);
  assert.deepEqual(rows.slice(3).map((r) => r.label), [
    "Anchors", "Background", "Board outline area", "Courtyard collision shadow", "Cursor", "DRC errors", "DRC exclusions", "DRC highlighted", "DRC warnings",
    "Drawing sheet", "Grid", "Grid axes", "Helper items", "Locked item shadow", "Non-plated holes", "Page limits", "Points", "Ratsnest",
  ]);
  const board = new Set(DATA.boardKeys);
  assert.deepEqual(fpColorPageKeys().filter((k) => !board.has(k)), []);
});

test("Import Theme...: a file's theme is the person's under its filename, replacing one of that filename; a built-in's filename is not taken", () => {
  const s0 = { pcbTheme: "user", themes: [{ filename: "nord", name: "Old Nord", colors: {} }] };
  const r = importThemes(s0, [{ filename: "nord", name: "Nord", colors: { F_Cu: "#b3474dff" } }, { filename: BUILTIN_CLASSIC, name: "Mine", colors: {} }]);
  assert.deepEqual(r.filenames, ["nord", `${BUILTIN_CLASSIC}_imported`]);
  assert.deepEqual(r.stored.themes.map((t) => [t.filename, t.name]), [["nord", "Nord"], [`${BUILTIN_CLASSIC}_imported`, "Mine"]]);
  const reg = themeRegistry(DATA, r.stored);
  assert.equal(findTheme(reg, "nord").colors.F_Cu, "#b3474dff");
  assert.equal(findTheme(reg, BUILTIN_CLASSIC).readOnly, true);
});

test("a theme's override flag and the file's other entries are kept by the store and by a colour edit", () => {
  const s0 = { pcbTheme: "user", themes: [{ filename: "t", name: "T", colors: {}, overrideSchItemColors: true, extra: { gerbview: { grid: "rgb(1, 2, 3)" } } }] };
  assert.deepEqual(parseStoredThemes(serializeStoredThemes(s0)), s0);
  const reg = themeRegistry(DATA, s0);
  assert.equal(findTheme(reg, "t").overrideSchItemColors, true);
  const s1 = setThemeColors(s0, reg, "t", { LAYER_WIRE: "#ff0000ff" });
  assert.deepEqual(s1.themes[0], { ...s0.themes[0], colors: { LAYER_WIRE: "#ff0000ff" } });
  const s2 = setThemeColors(s1, themeRegistry(DATA, s1), "t", {}, false);
  assert.equal(s2.themes[0]!.overrideSchItemColors, false);
});
