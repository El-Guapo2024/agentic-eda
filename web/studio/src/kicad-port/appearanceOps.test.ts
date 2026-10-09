import { test } from "node:test";
import assert from "node:assert/strict";
import { objectChecked, objectIsOn, reduceView, type AppearanceOp, type ViewSlice } from "./appearanceOps";
import { makeCtx, makeSlice, COPPER4 } from "./appearanceFixture";
import { objectOn } from "./appearance";

const ctx = makeCtx();
const run = (s: ViewSlice, ...ops: AppearanceOp[]): ViewSlice => ops.reduce((acc, op) => reduceView(acc, op, ctx), s);

test("hiding an object is seen by the painter and the pickers through layerVisible, and the checkbox follows the switch", () => {
  const s = run(makeSlice(), { op: "object", id: "tracks", visible: false });
  assert.equal(s.appearance.visible.tracks, false);
  assert.equal(objectOn(s.layerVisible, "tracks"), false);
  assert.equal(objectChecked(s, "tracks"), false);
  assert.equal(objectIsOn(s, "tracks"), false);
  assert.equal(objectIsOn(s, "vias"), true);
  assert.equal(s.layerVisible["F.Cu"], true, "layers are untouched");
});

test("the ratsnest and the grid objects are their own switches", () => {
  let s = run(makeSlice(), { op: "object", id: "ratsnest", visible: false }, { op: "object", id: "grid", visible: false });
  assert.equal(s.showRatsnest, false);
  assert.equal(s.gridVisible, false);
  assert.equal(objectChecked(s, "ratsnest"), false);
  assert.equal(objectIsOn(s, "grid"), false);
  s = run(s, { op: "object", id: "grid", visible: true });
  assert.equal(s.gridVisible, true);
});

test("Footprint Text drags References and Values with it; the object keys follow", () => {
  let s = run(makeSlice(), { op: "object", id: "footprint_text", visible: false });
  assert.equal(objectOn(s.layerVisible, "footprint_references"), false);
  assert.equal(objectOn(s.layerVisible, "footprint_values"), false);
  s = run(s, { op: "object", id: "footprint_values", visible: true });
  assert.equal(objectOn(s.layerVisible, "footprint_text"), true);
  assert.equal(objectOn(s.layerVisible, "footprint_references"), false);
});

test("an opacity of 0 takes the object out of drawing and picking while its checkbox stays checked; values are clamped", () => {
  let s = run(makeSlice(), { op: "opacity", key: "tracks", value: 0 });
  assert.equal(s.appearance.opacity.tracks, 0);
  assert.equal(objectChecked(s, "tracks"), true);
  assert.equal(objectIsOn(s, "tracks"), false);
  s = run(s, { op: "opacity", key: "tracks", value: 1.7 }, { op: "opacity", key: "zones", value: -3 });
  assert.equal(s.appearance.opacity.tracks, 1);
  assert.equal(s.appearance.opacity.zones, 0);
  assert.equal(objectIsOn(s, "zones"), false);
  assert.equal(objectIsOn(s, "tracks"), true);
});

test("the inactive-layer mode sets highContrast and the hidden flag", () => {
  let s = run(makeSlice(), { op: "contrast", mode: "dimmed" });
  assert.deepEqual([s.highContrast, s.appearance.contrastHidden], [true, false]);
  s = run(s, { op: "contrast", mode: "hidden" });
  assert.deepEqual([s.highContrast, s.appearance.contrastHidden], [true, true]);
  s = run(s, { op: "contrast", mode: "normal" });
  assert.deepEqual([s.highContrast, s.appearance.contrastHidden], [false, false]);
});

test("the net colour mode and the ratsnest display (None is the global ratsnest off)", () => {
  let s = run(makeSlice(), { op: "net_color_mode", mode: "all" });
  assert.equal(s.appearance.netColorMode, "all");
  s = run(s, { op: "ratsnest_display", mode: "visible" });
  assert.deepEqual([s.showRatsnest, s.ratsnestMode], [true, "visible"]);
  s = run(s, { op: "ratsnest_display", mode: "none" });
  assert.deepEqual([s.showRatsnest, s.ratsnestMode], [false, "visible"], "None keeps the mode it was in");
  s = run(s, { op: "ratsnest_display", mode: "all" });
  assert.deepEqual([s.showRatsnest, s.ratsnestMode], [true, "all"]);
});

test("a net colour and a net class colour are set and cleared; the nets and classes are named, not numbered", () => {
  let s = run(makeSlice(), { op: "net_color", net: "GND", color: "rgb(1, 2, 3)" }, { op: "netclass_color", name: "usb", color: "rgb(9, 9, 9)" });
  assert.deepEqual(s.appearance.netColors, { GND: "rgb(1, 2, 3)" });
  assert.deepEqual(s.appearance.netclassColors, { usb: "rgb(9, 9, 9)" });
  s = run(s, { op: "net_color", net: "GND", color: null }, { op: "netclass_color", name: "usb", color: null });
  assert.deepEqual(s.appearance.netColors, {});
  assert.deepEqual(s.appearance.netclassColors, {});
});

test("the eye of a net, Show All Nets and Hide All Other Nets edit the hidden ratsnest nets", () => {
  let s = run(makeSlice(), { op: "net_visible", net: "GND", visible: false });
  assert.deepEqual(s.hiddenNets, ["GND"]);
  s = run(s, { op: "hide_other_nets", net: "VCC" });
  assert.deepEqual([...s.hiddenNets].sort(), ["GND", "SCL", "USB_D+", "USB_D-"]);
  s = run(s, { op: "show_all_nets" });
  assert.deepEqual(s.hiddenNets, []);
});

test("the eye of a net class hides every net it owns and is remembered as a class", () => {
  let s = run(makeSlice(), { op: "netclass_visible", name: "usb", visible: false });
  assert.deepEqual([...s.hiddenNets].sort(), ["USB_D+", "USB_D-"]);
  assert.deepEqual(s.appearance.hiddenNetclasses, ["usb"]);
  s = run(s, { op: "hide_other_netclasses", name: "usb" });
  assert.deepEqual(s.appearance.hiddenNetclasses, ["Default"]);
  assert.deepEqual([...s.hiddenNets].sort(), ["GND", "SCL", "VCC"]);
  s = run(s, { op: "show_all_netclasses" });
  assert.deepEqual([s.hiddenNets, s.appearance.hiddenNetclasses], [[], []]);
});

test("toggling a layer moves the preset list to the matching entry, or to the blank one", () => {
  let s = makeSlice();
  assert.equal(s.appearance.activePreset, "All Layers");
  s = run(s, { op: "layer", key: "B.Cu", visible: false });
  assert.equal(s.appearance.activePreset, "", "nothing matches any more");
  s = run(s, { op: "layer", key: "B.Cu", visible: true });
  assert.equal(s.appearance.activePreset, "All Layers", "back to the preset it matches");
});

test("picking a preset changes the layers, the flip, the grid and the active layer together", () => {
  const start = { ...makeSlice(), activeLayer: "F.Cu" };
  const back = run(start, { op: "select_preset", name: "Back Layers" });
  assert.equal(back.layerVisible["F.Cu"], false);
  assert.equal(back.layerVisible["B.Cu"], true);
  assert.equal(back.layerVisible.b_silks, true);
  assert.equal(back.layerVisible.f_silks, false);
  assert.equal(back.boardFlipped, true);
  assert.equal(back.activeLayer, "B.Cu");
  assert.equal(back.appearance.activePreset, "Back Layers");
  const asm = run(start, { op: "select_preset", name: "Front Assembly View" });
  assert.equal(asm.activeLayer, "f_silks", "the state key of F.SilkS");
  assert.equal(run(start, { op: "select_preset", name: "Nope" }), start, "an unknown name does nothing");
});

test("Save preset keeps what is on screen; the preset comes back after the layers change; delete empties the list's choice", () => {
  let s = run(makeSlice(), { op: "layer", key: "f_fab", visible: false }, { op: "object", id: "vias", visible: false });
  s = run(s, { op: "save_preset", name: "No fab" });
  assert.equal(s.appearance.activePreset, "No fab");
  assert.equal(s.appearance.presets.length, 1);
  assert.equal(s.appearance.presets[0]!.layers.includes("F.Fab"), false);
  assert.equal(s.appearance.presets[0]!.renderLayers.includes("vias"), false);
  s = run(s, { op: "select_preset", name: "All Layers" });
  assert.equal(s.layerVisible.f_fab, true);
  s = run(s, { op: "select_preset", name: "No fab" });
  assert.equal(s.layerVisible.f_fab, false);
  assert.equal(objectOn(s.layerVisible, "vias"), false, "the user preset brings its own objects");
  // A built-in name is refused.
  assert.equal(run(s, { op: "save_preset", name: "Front Layers" }), s);
  s = run(s, { op: "delete_preset", name: "No fab" });
  assert.deepEqual(s.appearance.presets, []);
  assert.equal(s.appearance.activePreset, "");
});

test("the layer list's menu: copper off moves the active layer; a preset from the menu keeps the objects", () => {
  const four = makeSlice(COPPER4);
  const c4 = makeCtx(COPPER4);
  let s = reduceView({ ...four, activeLayer: "In1.Cu" }, { op: "layer_group", group: "hide_copper" }, c4);
  assert.equal(s.layerVisible["In1.Cu"], false);
  assert.equal(s.activeLayer, "f_mask", "the state key of F.Mask, the lowest id still on");
  s = reduceView(four, { op: "object", id: "zones", visible: false }, c4);
  s = reduceView(s, { op: "menu_preset", kind: "front" }, c4);
  assert.equal(s.layerVisible["B.Cu"], false);
  assert.equal(objectOn(s.layerVisible, "zones"), false, "objects stay");
  assert.equal(s.appearance.activePreset, "");
  s = reduceView({ ...four, activeLayer: "B.Cu" }, { op: "hide_all_but_active" }, c4);
  assert.deepEqual(Object.entries(s.layerVisible).filter(([k, v]) => v && !k.startsWith("obj:")).map(([k]) => k), ["B.Cu"]);
});

test("flipping the board is part of what a preset matches", () => {
  let s = makeSlice();
  s = run(s, { op: "flip", flipped: true });
  assert.equal(s.boardFlipped, true);
  assert.equal(s.appearance.activePreset, "", "All Layers is not flipped");
  s = run(s, { op: "flip", flipped: false });
  assert.equal(s.appearance.activePreset, "All Layers");
});

test("viewports are saved over a rectangle and deleted by name", () => {
  let s = run(makeSlice(), { op: "save_viewport", name: "USB corner", rect: { x: 1, y: 2, w: 3, h: 4 } });
  assert.deepEqual(s.appearance.viewports, [{ name: "USB corner", x: 1, y: 2, w: 3, h: 4 }]);
  s = run(s, { op: "save_viewport", name: "  ", rect: { x: 0, y: 0, w: 1, h: 1 } });
  assert.equal(s.appearance.viewports.length, 1, "a blank name saves nothing");
  s = run(s, { op: "delete_viewport", name: "USB corner" });
  assert.deepEqual(s.appearance.viewports, []);
});

test("a layer's own opacity is kept apart from the objects'", () => {
  const s = run(makeSlice(), { op: "layer_opacity", key: "F.Cu", value: 0.5 });
  assert.equal(s.layerOpacity["F.Cu"], 0.5);
  assert.equal(s.appearance.opacity.tracks, 1);
});
