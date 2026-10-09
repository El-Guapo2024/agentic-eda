import { test } from "node:test";
import assert from "node:assert/strict";
import { APPEARANCE_FILE_VERSION, applyAppearanceFile, presetFromJson, toAppearanceFile } from "./appearanceFile";
import { reduceView, type AppearanceOp, type ViewSlice } from "./appearanceOps";
import { makeCtx, makeSlice } from "./appearanceFixture";
import { objectOn } from "./appearance";

const ctx = makeCtx();
const run = (s: ViewSlice, ...ops: AppearanceOp[]): ViewSlice => ops.reduce((acc, op) => reduceView(acc, op, ctx), s);

test("a fresh project writes a file that says nothing is hidden and everything default is on", () => {
  const f = toAppearanceFile(makeSlice(), ctx);
  assert.equal(f.version, APPEARANCE_FILE_VERSION);
  assert.deepEqual(f.local.hidden_layers, []);
  assert.equal(f.local.visible_items?.includes("tracks"), true);
  assert.equal(f.local.visible_items?.includes("drc_exclusions"), false);
  assert.equal(f.local.high_contrast_mode, 0);
  assert.equal(f.local.net_color_mode, 1, "KiCad's default: colours on the ratsnest");
  assert.deepEqual(f.local.opacity, { tracks: 1, vias: 1, pads: 1, zones: 0.6, images: 0.6, shapes: 1 });
  assert.deepEqual(f.project, { net_colors: {}, netclass_colors: {}, layer_presets: [], viewports: [] });
});

test("everything the panel edits survives the file and back", () => {
  let s = makeSlice();
  s = run(
    s,
    { op: "object", id: "tracks", visible: false },
    { op: "opacity", key: "zones", value: 0.25 },
    { op: "contrast", mode: "hidden" },
    { op: "net_color_mode", mode: "all" },
    { op: "net_color", net: "GND", color: "rgb(1, 2, 3)" },
    { op: "netclass_color", name: "usb", color: "rgba(9, 9, 9, 0.502)" },
    { op: "net_visible", net: "VCC", visible: false },
    { op: "netclass_visible", name: "usb", visible: false },
    { op: "layer", key: "B.Cu", visible: false },
    { op: "layer", key: "f_fab", visible: false },
    { op: "layer_opacity", key: "F.Cu", value: 0.5 },
    { op: "ratsnest_display", mode: "visible" },
    { op: "flip", flipped: true },
    { op: "object", id: "grid", visible: false },
    { op: "save_preset", name: "Mine" },
    { op: "save_viewport", name: "Home", rect: { x: 1, y: 2, w: 30, h: 40 } }
  );
  s = { ...s, activeLayer: "f_silks" };
  const file = toAppearanceFile(s, ctx);
  // The file is plain JSON.
  const text = JSON.parse(JSON.stringify(file));
  const back = applyAppearanceFile(makeSlice(), text, ctx);
  assert.deepEqual(back.appearance.visible.tracks, false);
  assert.equal(objectOn(back.layerVisible, "tracks"), false, "the object keys are rebuilt");
  assert.equal(back.appearance.opacity.zones, 0.25);
  assert.deepEqual([back.highContrast, back.appearance.contrastHidden], [true, true]);
  assert.equal(back.appearance.netColorMode, "all");
  assert.deepEqual(back.appearance.netColors, { GND: "rgb(1, 2, 3)" });
  assert.deepEqual(back.appearance.netclassColors, { usb: "rgba(9, 9, 9, 0.502)" });
  assert.deepEqual([...back.hiddenNets].sort(), ["USB_D+", "USB_D-", "VCC"]);
  assert.deepEqual(back.appearance.hiddenNetclasses, ["usb"]);
  assert.equal(back.layerVisible["B.Cu"], false);
  assert.equal(back.layerVisible.f_fab, false);
  assert.equal(back.layerVisible["F.Cu"], true);
  assert.equal(back.layerOpacity["F.Cu"], 0.5);
  assert.equal(back.activeLayer, "f_silks");
  assert.deepEqual([back.showRatsnest, back.ratsnestMode], [true, "visible"]);
  assert.equal(back.boardFlipped, true);
  assert.equal(back.gridVisible, false);
  assert.deepEqual(back.appearance.presets, s.appearance.presets);
  assert.deepEqual(back.appearance.viewports, [{ name: "Home", x: 1, y: 2, w: 30, h: 40 }]);
  assert.equal(back.appearance.activePreset, s.appearance.activePreset);
  // And writing what was read changes nothing.
  assert.deepEqual(toAppearanceFile(back, ctx), file);
});

test("the keys are KiCad's: board.visible_items names, board.opacity.*, hidden_nets, net_colors, layer presets by name", () => {
  const s = run(makeSlice(), { op: "object", id: "pads", visible: false }, { op: "net_color", net: "GND", color: "rgb(1, 2, 3)" }, { op: "layer", key: "f_fab", visible: false }, { op: "save_preset", name: "P" });
  const f = toAppearanceFile(s, ctx);
  assert.equal(f.local.visible_items?.includes("pads"), false);
  assert.deepEqual(f.local.hidden_layers, ["F.Fab"], "layers are written by KiCad name, not by the painter's bucket");
  assert.deepEqual(f.project.net_colors, { GND: "rgb(1, 2, 3)" });
  assert.deepEqual(Object.keys(f.project.layer_presets![0]!).sort(), ["activeLayer", "flipBoard", "layers", "name", "renderLayers"]);
});

test("an unreadable file changes nothing; a missing key leaves its setting alone (SetIfPresent)", () => {
  const s = run(makeSlice(), { op: "object", id: "tracks", visible: false });
  assert.equal(applyAppearanceFile(s, null, ctx), s);
  assert.equal(applyAppearanceFile(s, "junk", ctx), s);
  assert.equal(applyAppearanceFile(s, [], ctx), s);
  const empty = applyAppearanceFile(s, { version: 1, local: {}, project: {} }, ctx);
  assert.equal(empty.appearance.visible.tracks, false);
  assert.deepEqual(empty.appearance.opacity, s.appearance.opacity);
  // Wrongly typed entries are skipped one by one.
  const odd = applyAppearanceFile(makeSlice(), { local: { high_contrast_mode: "dimmed", opacity: { tracks: "no", vias: 0.3, pads: 7 }, hidden_nets: [1, 2], net_color_mode: 2 }, project: { net_colors: { GND: "red", VCC: "rgb(1, 2, 3)" } } }, ctx);
  assert.equal(odd.highContrast, false);
  assert.equal(odd.appearance.opacity.tracks, 1);
  assert.equal(odd.appearance.opacity.vias, 0.3);
  assert.equal(odd.appearance.opacity.pads, 1, "out of range");
  assert.deepEqual(odd.hiddenNets, []);
  assert.equal(odd.appearance.netColorMode, "all");
  assert.deepEqual(odd.appearance.netColors, { VCC: "rgb(1, 2, 3)" }, "a colour that is not one is dropped");
});

test("board.visible_items: nothing usable means everything on, \"none\" is the way to say nothing is", () => {
  const hideAll = (items: unknown) => applyAppearanceFile(makeSlice(), { local: { visible_items: items } }, ctx);
  const corrupt = hideAll(["something", 7]);
  assert.equal(corrupt.appearance.visible.drc_exclusions, true, "restore corrupted state: all on");
  const none = hideAll(["none"]);
  assert.equal(none.appearance.visible.tracks, false);
  assert.equal(none.showRatsnest, false);
  assert.equal(none.gridVisible, false);
  const some = hideAll(["tracks", "grid"]);
  assert.deepEqual([some.appearance.visible.tracks, some.appearance.visible.vias, some.gridVisible, some.showRatsnest], [true, false, true, false]);
  // Written back: hiding everything is "none".
  assert.deepEqual(toAppearanceFile(none, ctx).local.visible_items, ["none"]);
});

test("nets the board no longer has are left out of the file once the board's nets are known", () => {
  const s = run(makeSlice(), { op: "net_color", net: "GND", color: "rgb(1, 2, 3)" }, { op: "net_color", net: "Gone", color: "rgb(4, 5, 6)" }, { op: "net_visible", net: "Gone", visible: false });
  const f = toAppearanceFile(s, ctx);
  assert.deepEqual(f.project.net_colors, { GND: "rgb(1, 2, 3)" });
  assert.deepEqual(f.local.hidden_nets, []);
  // With no nets known yet (the board has not loaded), nothing is dropped.
  const early = toAppearanceFile(s, { copper: ctx.copper, nets: [] });
  assert.deepEqual(Object.keys(early.project.net_colors!).sort(), ["GND", "Gone"]);
});

test("a preset entry without a name is skipped, a field of the wrong type keeps its default (jsonToPresets)", () => {
  assert.equal(presetFromJson({ layers: [] }), null);
  assert.equal(presetFromJson("x"), null);
  const p = presetFromJson({ name: "A", layers: "no", flipBoard: 1, activeLayer: 4, renderLayers: ["tracks", "nope"] })!;
  assert.deepEqual(p, { name: "A", layers: [], renderLayers: ["tracks"], flipBoard: false, activeLayer: null, readOnly: false });
});
