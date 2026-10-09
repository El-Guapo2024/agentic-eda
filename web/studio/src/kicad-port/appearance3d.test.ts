import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  NOT_SPECIFIED,
  ROWS,
  STACKUP_COLOR_NAMES,
  STACKUP_ROWS,
  THEME_COLORS,
  USER_LAYER_COUNT,
  USER_ROWS,
  finishColor,
  isVisible,
  parseColor,
  resolveColors,
  rowOf,
  rowsFor,
  shapeLayerOf,
  stackupColorKind,
  toHex,
  toRgbHex,
  userRowOfLayer,
  type ColorInputs,
  type Rgba,
} from "./appearance3d";

const colorsJson = JSON.parse(readFileSync("src/kicad/colors.json", "utf8")) as { colors: Record<string, string> };
const editorColor = (key: string) => colorsJson.colors[key];

function close(got: Rgba, want: { r: number; g: number; b: number; a?: number }, eps = 1e-6) {
  assert.ok(Math.abs(got.r - want.r / 255) < eps && Math.abs(got.g - want.g / 255) < eps && Math.abs(got.b - want.b / 255) < eps && Math.abs(got.a - (want.a ?? 1)) < eps, `${JSON.stringify(got)} != ${JSON.stringify(want)}`);
}

const base = (over: Partial<ColorInputs> = {}): ColorInputs => ({ useStackupColors: false, useEditorCopperColors: false, overrides: {}, stackup: null, editorColor, ...over });

test("colours read as COLOR4D reads them: #RRGGBB is opaque, #RRGGBBAA has its alpha, nothing else is a colour", () => {
  close(parseColor("#143324")!, { r: 20, g: 51, b: 36, a: 1 });
  close(parseColor("#143324d4")!, { r: 20, g: 51, b: 36, a: 212 / 255 });
  assert.equal(parseColor("red"), null);
  assert.equal(parseColor("#12345"), null, "COLOR4D wants at least 7 characters");
  assert.equal(parseColor("#12345g"), null);
  assert.equal(toRgbHex(parseColor("#143324d4")!), "#143324");
  assert.equal(toHex(parseColor("#143324d4")!), "#143324d4");
});

test("the theme colours are the ones colors.json carries from builtin_color_themes.h", () => {
  const key: Record<string, string> = {
    board: "LAYER_3D_BOARD",
    copper_top: "LAYER_3D_COPPER_TOP",
    silkscreen_top: "LAYER_3D_SILKSCREEN_TOP",
    silkscreen_bottom: "LAYER_3D_SILKSCREEN_BOTTOM",
    soldermask_top: "LAYER_3D_SOLDERMASK_TOP",
    soldermask_bottom: "LAYER_3D_SOLDERMASK_BOTTOM",
    solder_paste: "LAYER_3D_SOLDERPASTE",
    background_top: "LAYER_3D_BACKGROUND_TOP",
    background_bottom: "LAYER_3D_BACKGROUND_BOTTOM",
  };
  for (const [row, json] of Object.entries(key)) {
    const want = parseColor(colorsJson.colors[json]!)!;
    const got = THEME_COLORS[row]!;
    assert.ok(Math.abs(got.r - want.r) < 1 / 255 && Math.abs(got.g - want.g) < 1 / 255 && Math.abs(got.b - want.b) < 1 / 255 && Math.abs(got.a - want.a) < 1 / 255 + 1e-9, `${row}: ${toHex(got)} != ${colorsJson.colors[json]}`);
  }
});

test("the tree lists KiCad's rows in KiCad's order, with its tooltips, and the user layers only when the board has something on them", () => {
  const ids = rowsFor(new Set()).map((r) => r.id);
  assert.deepEqual(ids.slice(0, 10), ["board", "plated_barrels", "copper_top", "copper_bottom", "adhesive", "solder_paste", "silkscreen_top", "silkscreen_bottom", "soldermask_top", "soldermask_bottom"]);
  assert.ok(ids.includes("th_models") && ids.includes("smd_models") && ids.includes("virtual_models") && ids.includes("zones") && ids.includes("bounding_boxes"));
  assert.ok(!ids.some((id) => id.startsWith("user_")), "no user layer in use, none listed");
  const withUser = rowsFor(new Set(["user_comments", "user_7"])).map((r) => r.id);
  assert.deepEqual(withUser.filter((id) => id.startsWith("user_")), ["user_comments", "user_7"], "in KiCad's order: the named ones, then User.1 ..");
  assert.equal(USER_ROWS.length, 4 + USER_LAYER_COUNT);
  assert.equal(rowOf("board")!.tooltip, "Show board body");
  assert.equal(rowOf("th_models")!.hotkey, "T");
  assert.equal(rowOf("smd_models")!.hotkey, "S");
  assert.equal(new Set(ROWS.map((r) => r.id)).size, ROWS.length, "ids are unique");
});

test("what is shown by default is EDA_3D_VIEWER_SETTINGS's: everything but the bounding boxes", () => {
  const shown = ROWS.filter((r) => isVisible({}, r.id)).map((r) => r.id);
  assert.ok(!shown.includes("bounding_boxes"));
  for (const id of ["board", "plated_barrels", "copper_top", "copper_bottom", "adhesive", "solder_paste", "silkscreen_top", "soldermask_bottom", "th_models", "smd_models", "virtual_models", "zones", "references", "user_drawings"]) assert.ok(shown.includes(id), id);
  assert.equal(isVisible({ board: false }, "board"), false);
  assert.equal(isVisible({ bounding_boxes: true }, "bounding_boxes"), true);
  assert.equal(isVisible({}, "no_such_row"), true);
});

test("a drawing on a layer is a silkscreen, an adhesive, a user layer, or nothing the 3D viewer draws", () => {
  assert.deepEqual(shapeLayerOf("F.SilkS"), { kind: "silk", side: "top" });
  assert.deepEqual(shapeLayerOf("B.SilkS"), { kind: "silk", side: "bottom" });
  assert.deepEqual(shapeLayerOf("F.Adhes"), { kind: "adhesive", side: "top" });
  assert.deepEqual(shapeLayerOf("B.Adhes"), { kind: "adhesive", side: "bottom" });
  assert.deepEqual(shapeLayerOf("Dwgs.User"), { kind: "user", row: "user_drawings" });
  assert.deepEqual(shapeLayerOf("Cmts.User"), { kind: "user", row: "user_comments" });
  assert.deepEqual(shapeLayerOf("Eco2.User"), { kind: "user", row: "user_eco2" });
  assert.deepEqual(shapeLayerOf("User.12"), { kind: "user", row: "user_12" });
  for (const none of ["Edge.Cuts", "F.CrtYd", "B.Fab", "F.Cu", "F.Mask", "F.Paste", "User.0", "User.46", "Margin", ""]) assert.equal(shapeLayerOf(none), null, none);
  assert.equal(userRowOfLayer("User.45"), "user_45");
});

test("without the stackup, the colours are the theme's, copper at the bottom is the top's, and a user layer has the 2D editor's colour", () => {
  const c = resolveColors(base());
  assert.equal(toHex(c.board!), toHex(THEME_COLORS.board!));
  assert.equal(toHex(c.copper_bottom!), toHex(c.copper_top!));
  assert.equal(toRgbHex(c.user_drawings!), "#c2c2c2", "Dwgs.User");
  assert.equal(toRgbHex(c.user_comments!), "#5994dc", "Cmts.User");
  assert.equal(toRgbHex(c.user_3!), "#b4dbd2");
});

test("the stackup's colours: a silkscreen and a mask by name, 'Not specified' and a missing colour the table's first entry, a typed colour as typed", () => {
  const stackup = {
    layers: [
      { name: "F.SilkS", kind: "Top Silk Screen", color: "Black" },
      { name: "F.Mask", kind: "Top Solder Mask", color: "Red" },
      { name: "F.Cu", kind: "copper" },
      { name: "dielectric 1", kind: "core" },
      { name: "B.Mask", kind: "Bottom Solder Mask", color: "#336699" },
      { name: "B.SilkS", kind: "Bottom Silk Screen", color: NOT_SPECIFIED },
    ],
  };
  const off = resolveColors(base({ stackup }));
  assert.equal(toHex(off.silkscreen_top!), toHex(THEME_COLORS.silkscreen_top!), "off: the theme");
  const c = resolveColors(base({ useStackupColors: true, stackup }));
  close(c.silkscreen_top!, { r: 11, g: 11, b: 11 });
  close(c.soldermask_top!, { r: 181, g: 19, b: 21, a: 0.83 });
  close(c.soldermask_bottom!, { r: 0x33, g: 0x66, b: 0x99, a: 1 }); // a typed #RRGGBB is opaque
  close(c.silkscreen_bottom!, { r: 245, g: 245, b: 245 }); // not specified: white
  assert.equal(toHex(c.board!), toHex(THEME_COLORS.board!), "a dielectric with no colour leaves the body the theme's");
});

test("the body is the dielectrics mixed (Mix( layer, 1 - layer.a ), alpha += (1 - alpha) * layer.a / 2), once per dielectric", () => {
  const one = resolveColors(base({ useStackupColors: true, stackup: { layers: [{ name: "d1", kind: "core", color: "FR4 natural" }] } }));
  close(one.board!, { r: 109, g: 116, b: 75, a: 0.83 + (0.17 * 0.83) / 2 });
  const two = resolveColors(base({ useStackupColors: true, stackup: { layers: [{ name: "d1", kind: "core", color: "FR4 natural" }, { name: "d2", kind: "prepreg", color: "FR4 natural" }] } }));
  const a1 = 0.83 + (0.17 * 0.83) / 2;
  close(two.board!, { r: 109, g: 116, b: 75, a: a1 + ((1 - a1) * 0.83) / 2 });
  const mixed = resolveColors(base({ useStackupColors: true, stackup: { layers: [{ name: "d1", kind: "core", color: "PTFE natural" }, { name: "d2", kind: "prepreg", color: "Polyimide" }] } }));
  // layer 2 (Polyimide, alpha 0.68) over the body (PTFE, alpha 0.9): colour = layer * 0.68 + body * 0.32.
  const f = 1 - 0.68;
  close(mixed.board!, { r: (205 * (1 - f) + 252 * f), g: (130 * (1 - f) + 252 * f), b: (0 * (1 - f) + 250 * f), a: 0.9 + ((1 - 0.9) * 0.9) / 2 + ((1 - (0.9 + (0.1 * 0.9) / 2)) * 0.68) / 2 });
});

test("the copper finish colours the copper: OSP copper, IG and gold gold, HAL/HASL and tin and nickel tin, silver silver", () => {
  const want: Array<[string, string | null]> = [
    ["ENIG", "Gold"], ["ENEPIG", "Gold"], ["Hard gold", "Gold"], ["Immersion gold", "Gold"],
    ["HAL SnPb", "Tin"], ["HAL lead-free", "Tin"], ["Immersion tin", "Tin"], ["Immersion nickel", "Tin"],
    ["OSP", "Copper"], ["HT_OSP", "Copper"], ["Immersion silver", "Silver"], ["None", null], ["", null], ["User defined", null],
  ];
  const table: Record<string, Rgba> = { Copper: { r: 184 / 255, g: 115 / 255, b: 50 / 255, a: 1 }, Gold: { r: 178 / 255, g: 156 / 255, b: 0, a: 1 }, Silver: { r: 213 / 255, g: 213 / 255, b: 213 / 255, a: 1 }, Tin: { r: 160 / 255, g: 160 / 255, b: 160 / 255, a: 1 } };
  for (const [finish, name] of want) {
    const got = finishColor(finish);
    if (name === null) assert.equal(got, null, finish);
    else assert.deepEqual(got, table[name], finish);
  }
  const c = resolveColors(base({ useStackupColors: true, stackup: { layers: [], copper_finish: "ENIG" } }));
  close(c.copper_top!, { r: 178, g: 156, b: 0 });
  close(c.copper_bottom!, { r: 178, g: 156, b: 0 });
  assert.equal(toHex(resolveColors(base({ stackup: { layers: [], copper_finish: "ENIG" } })).copper_top!), toHex(THEME_COLORS.copper_top!), "the finish counts only with 'Use board stackup colors'");
});

test("a swatch the person set wins, but not on a row the stackup owns while 'Use board stackup colors' is on; the editor's copper colours replace the copper's", () => {
  const overrides = { board: "#ff0000", user_drawings: "#00ff00", background_top: "#0000ff80", nope: "#ffffff" };
  const free = resolveColors(base({ overrides }));
  assert.equal(toRgbHex(free.board!), "#ff0000");
  assert.equal(toRgbHex(free.user_drawings!), "#00ff00");
  close(free.background_top!, { r: 0, g: 0, b: 255, a: 128 / 255 });
  assert.ok(!("nope" in free), "only rows have colours");
  const stackup = resolveColors(base({ overrides, useStackupColors: true, stackup: { layers: [] } }));
  assert.equal(toHex(stackup.board!), toHex(THEME_COLORS.board!), "the board's colour is the stackup's now, read-only");
  assert.equal(toRgbHex(stackup.user_drawings!), "#00ff00", "a user layer is not the stackup's");
  assert.ok(STACKUP_ROWS.has("board") && STACKUP_ROWS.has("soldermask_top") && !STACKUP_ROWS.has("user_drawings") && !STACKUP_ROWS.has("background_top"));
  const editor = resolveColors(base({ useEditorCopperColors: true }));
  assert.equal(toRgbHex(editor.copper_top!), "#c83434");
  assert.equal(toRgbHex(editor.copper_bottom!), "#4d7fc4");
});

test("Board Setup offers KiCad's colour names for a mask, a silkscreen and a dielectric, and none for copper or paste", () => {
  assert.equal(stackupColorKind("Top Silk Screen"), "silk");
  assert.equal(stackupColorKind("Bottom Solder Mask"), "mask");
  assert.equal(stackupColorKind("core"), "dielectric");
  assert.equal(stackupColorKind("prepreg"), "dielectric");
  assert.equal(stackupColorKind("copper"), null);
  assert.equal(stackupColorKind("Top Solder Paste"), null);
  assert.equal(stackupColorKind(undefined), null);
  assert.deepEqual(STACKUP_COLOR_NAMES.mask, [NOT_SPECIFIED, "Green", "Red", "Blue", "Purple", "Black", "White", "Yellow"]);
  assert.deepEqual(STACKUP_COLOR_NAMES.dielectric, [NOT_SPECIFIED, "FR4 natural", "PTFE natural", "Polyimide", "Phenolic natural", "Aluminum"]);
  // Every name Board Setup offers is a name the 3D viewer knows how to colour.
  for (const kind of ["silk", "mask", "dielectric"] as const) {
    const type = kind === "silk" ? "Top Silk Screen" : kind === "mask" ? "Top Solder Mask" : "core";
    for (const name of STACKUP_COLOR_NAMES[kind]) {
      if (name === NOT_SPECIFIED) continue;
      const c = resolveColors(base({ useStackupColors: true, stackup: { layers: [{ name: kind === "dielectric" ? "d" : "F.X", kind: type, color: name }] } }));
      const row = kind === "silk" ? "silkscreen_top" : kind === "mask" ? "soldermask_top" : "board";
      assert.ok(c[row]!.a > 0, `${kind}: ${name}`);
    }
  }
});
