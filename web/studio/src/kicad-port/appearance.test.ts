import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_OBJECT_VISIBILITY,
  DEFAULT_OPACITY,
  OBJECT_IDS,
  OBJECT_ROWS,
  defaultAppearance,
  isUnspecified,
  netPalette,
  copperColor,
  drcMarkerObject,
  ratsnestColor,
  nextContrastMode,
  nextNetColorMode,
  objectKey,
  objectOn,
  parseCssColor,
  specifiedColor,
  toCssColor,
  toHex6,
  toHex8,
  visibleObjects,
  withObjectKeys,
  withObjectVisible,
  contrastModeOf,
  netColorModeOf,
  CONTRAST_MODE_NUMBER,
  NET_COLOR_MODE_NUMBER,
} from "./appearance";

test("the colour text of a project file: rgb() when opaque, rgba() with three decimals otherwise (COLOR4D::ToCSSString)", () => {
  assert.equal(toCssColor({ r: 255, g: 0, b: 16, a: 1 }), "rgb(255, 0, 16)");
  assert.equal(toCssColor({ r: 1, g: 2, b: 3, a: 0.5 }), "rgba(1, 2, 3, 0.502)", "alpha is stored as a byte, 128/255");
  assert.equal(toCssColor({ r: 0, g: 0, b: 0, a: 0 }), "rgba(0, 0, 0, 0.000)");
});

test("parseCssColor reads the forms wxColour::Set reads: rgb(), rgba(), #rgb, #rrggbb and #rrggbbaa", () => {
  assert.deepEqual(parseCssColor("rgb(10, 20, 30)"), { r: 10, g: 20, b: 30, a: 1 });
  assert.deepEqual(parseCssColor("rgba(10,20,30,0.25)"), { r: 10, g: 20, b: 30, a: 0.25 });
  assert.deepEqual(parseCssColor("#f80"), { r: 255, g: 136, b: 0, a: 1 });
  assert.deepEqual(parseCssColor("#c83434ff"), { r: 200, g: 52, b: 52, a: 1 });
  assert.equal(parseCssColor("#c8343480")?.a, 128 / 255);
  assert.equal(parseCssColor("not a colour"), null);
  assert.equal(parseCssColor("rgb(1, 2)"), null);
  // Out-of-range channels are clamped, as a colour is.
  assert.deepEqual(parseCssColor("rgb(300, 20, 30)"), { r: 255, g: 20, b: 30, a: 1 });
});

test("a colour survives text and back; hex forms for the canvas and for <input type=color>", () => {
  const c = { r: 12, g: 34, b: 56, a: 1 };
  assert.deepEqual(parseCssColor(toCssColor(c)), c);
  assert.equal(toHex6(c), "#0c2238");
  assert.equal(toHex8({ ...c, a: 0.5 }), "#0c223880");
});

test("COLOR4D::UNSPECIFIED is (0, 0, 0, 0): clearing a swatch writes rgba(0,0,0,0), which means no colour", () => {
  assert.equal(isUnspecified(parseCssColor("rgba(0,0,0,0)")), true);
  assert.equal(specifiedColor("rgba(0,0,0,0)"), null);
  assert.equal(specifiedColor("rgb(0, 0, 0)")?.a, 1, "opaque black is a colour");
  assert.equal(specifiedColor(undefined), null);
  assert.equal(specifiedColor("junk"), null);
});

test("the 22 objects, and what a new project has on (GAL_SET::DefaultVisible): all but DRC exclusions and the board area shadow -- and the drawing sheet, which this studio keeps off", () => {
  assert.equal(OBJECT_IDS.length, 22);
  const off = OBJECT_IDS.filter((id) => !DEFAULT_OBJECT_VISIBILITY[id]);
  assert.deepEqual(off, ["drc_exclusions", "board_outline_area", "drawing_sheet"]);
});

test("the Objects rows are s_objectSettings' without Images and Points, and Filled Shapes has a slider and no checkbox", () => {
  const ids = OBJECT_ROWS.filter((r) => r.id !== null).map((r) => r.id);
  assert.deepEqual(ids, [
    "tracks", "vias", "pads", "zones", "shapes", "footprints_front", "footprints_back", "footprint_values", "footprint_references", "footprint_text",
    "ratsnest", "drc_warnings", "drc_errors", "drc_exclusions", "footprint_anchors", "locked_item_shadows", "conflict_shadows", "board_outline_area", "drawing_sheet", "grid",
  ]);
  const shapes = OBJECT_ROWS.find((r) => r.id === "shapes")!;
  assert.equal(shapes.checkbox, false);
  assert.equal(shapes.opacity, "shapes");
  assert.deepEqual(OBJECT_ROWS.filter((r) => r.opacity).map((r) => r.opacity), ["tracks", "vias", "pads", "zones", "shapes"]);
  assert.equal(OBJECT_ROWS.filter((r) => r.id === null).length, 2, "two spacers between the groups");
});

test("the opacities of a new project: zones are 60 %, the rest opaque (PROJECT_LOCAL_SETTINGS)", () => {
  assert.deepEqual(DEFAULT_OPACITY, { tracks: 1, vias: 1, pads: 1, zones: 0.6, images: 0.6, shapes: 1 });
});

test("Footprint Text drags References and Values along; switching either of those on puts it back on (onObjectVisibilityChanged)", () => {
  const v = { ...DEFAULT_OBJECT_VISIBILITY };
  const off = withObjectVisible(v, "footprint_text", false);
  assert.equal(off.footprint_references, false);
  assert.equal(off.footprint_values, false);
  assert.equal(off.footprint_text, false);
  const refOn = withObjectVisible(off, "footprint_references", true);
  assert.equal(refOn.footprint_text, true, "the meta-control comes back");
  assert.equal(refOn.footprint_values, false, "the other field stays off");
  // Switching a field OFF leaves the meta-control alone.
  const valOff = withObjectVisible(v, "footprint_values", false);
  assert.equal(valOff.footprint_text, true);
  assert.equal(valOff.footprint_references, true);
  // A plain object is just itself.
  assert.deepEqual(withObjectVisible(v, "tracks", false), { ...v, tracks: false });
  // Footprint Text back on turns both fields on.
  const back = withObjectVisible(off, "footprint_text", true);
  assert.equal(back.footprint_references && back.footprint_values, true);
});

test("the inactive-layer and net colour modes cycle as PCB_CONTROL does and use the numbers a .kicad_prl stores", () => {
  assert.deepEqual([nextContrastMode("normal"), nextContrastMode("dimmed"), nextContrastMode("hidden")], ["dimmed", "hidden", "normal"]);
  assert.deepEqual([nextNetColorMode("all"), nextNetColorMode("ratsnest"), nextNetColorMode("off")], ["ratsnest", "off", "all"]);
  assert.deepEqual(CONTRAST_MODE_NUMBER, { normal: 0, dimmed: 1, hidden: 2 });
  assert.deepEqual(NET_COLOR_MODE_NUMBER, { off: 0, ratsnest: 1, all: 2 });
  assert.equal(contrastModeOf(2), "hidden");
  assert.equal(contrastModeOf("x"), "normal");
  assert.equal(netColorModeOf(2), "all");
  assert.equal(netColorModeOf(0), "off");
  assert.equal(netColorModeOf(undefined), "ratsnest", "NET_COLOR_MODE::RATSNEST is the default");
  assert.equal(defaultAppearance().netColorMode, "ratsnest");
});

test("visibleObjects merges the ratsnest and the grid switches in, in the order of OBJECT_IDS", () => {
  const a = defaultAppearance();
  assert.equal(visibleObjects(a, true, true).length, 19, "the 22 less the three that are off by default");
  const none = visibleObjects(a, false, false);
  assert.equal(none.includes("ratsnest"), false);
  assert.equal(none.includes("grid"), false);
  assert.equal(none.includes("tracks"), true);
  // The copy of the ratsnest / grid flags inside `visible` is not the authority.
  a.visible.ratsnest = false;
  assert.equal(visibleObjects(a, true, true).includes("ratsnest"), true);
});

test("object keys ride in layerVisible; an opacity of 0 counts as off (Selectable: options.m_TrackOpacity == 0.00)", () => {
  const a = defaultAppearance();
  let lv = withObjectKeys({ "F.Cu": true }, a);
  assert.equal(lv["F.Cu"], true, "layers are kept");
  assert.equal(objectOn(lv, "tracks"), true);
  assert.equal(objectOn({}, "tracks"), true, "an object nobody set is on");
  assert.equal(lv[objectKey("drc_exclusions")], false);
  a.visible.vias = false;
  a.opacity.tracks = 0;
  a.opacity.zones = 0.6;
  lv = withObjectKeys(lv, a);
  assert.equal(objectOn(lv, "vias"), false);
  assert.equal(objectOn(lv, "tracks"), false, "visible but transparent");
  assert.equal(objectOn(lv, "zones"), true);
  a.opacity.shapes = 0;
  assert.equal(objectOn(withObjectKeys(lv, a), "shapes"), false, "Filled Shapes has only the slider");
});

test("net colours: the net's own beats its class's, the Default class has none, an unspecified colour is none", () => {
  const classOf = (n: string) => (n.startsWith("USB") ? "usb" : n === "GND" ? "power" : "Default");
  const p = netPalette(
    { netColors: { GND: "rgb(1, 2, 3)", VCC: "rgba(0, 0, 0, 0)" }, netclassColors: { usb: "rgb(9, 9, 9)", power: "rgb(7, 7, 7)", Default: "rgb(5, 5, 5)" } },
    ["GND", "USB_D+", "USB_D-", "VCC", "SCL"],
    classOf
  );
  assert.deepEqual(p.get("GND"), { r: 1, g: 2, b: 3, a: 1 }, "the net's colour beats the power class's");
  assert.deepEqual(p.get("USB_D+"), { r: 9, g: 9, b: 9, a: 1 }, "from the class");
  assert.equal(p.has("VCC"), false, "an unspecified net colour and the Default class: none");
  assert.equal(p.has("SCL"), false, "the Default class never gives a colour");
  assert.equal(p.size, 3);
});

test("a DRC marker is on the object of its severity; a waived one is a DRC exclusion (PCB_MARKER::ViewGetLayers)", () => {
  assert.equal(drcMarkerObject({ severity: "error" }), "drc_errors");
  assert.equal(drcMarkerObject({ severity: "warning" }), "drc_warnings");
  assert.equal(drcMarkerObject({ severity: "error", excluded: true }), "drc_exclusions");
  assert.equal(drcMarkerObject({ severity: "warning", excluded: true }), "drc_exclusions");
  assert.equal(drcMarkerObject({ severity: "exclusion" }), "drc_errors", "anything else is an error, as the switch's default");
});

const palette = new Map([["GND", { r: 0, g: 224, b: 64, a: 1 }], ["SIG", { r: 255, g: 160, b: 0, a: 0.5 }]]);

test("copper takes its net's colour in the All mode only, and keeps its own where the net has none (PCB_RENDER_SETTINGS::GetColor)", () => {
  const all = { netColorMode: "all" as const, palette };
  assert.equal(copperColor("#c83434ff", "GND", all, null), "rgba(0, 224, 64, 1)");
  assert.equal(copperColor("#c83434ff", "VCC", all, null), "#c83434ff", "no colour for the net");
  assert.equal(copperColor("#c83434ff", null, all, null), "#c83434ff", "an item with no net");
  assert.equal(copperColor("#c83434ff", "GND", { netColorMode: "ratsnest", palette }, null), "#c83434ff", "ratsnest mode: copper is not touched");
  assert.equal(copperColor("#c83434ff", "GND", { netColorMode: "off", palette }, null), "#c83434ff");
  assert.equal(copperColor("#c83434ff", "GND", undefined, null), "#c83434ff", "no appearance at all");
});

test("a highlight brightens the highlighted net's colour and darkens every other net's by 0.5, keeping the alpha", () => {
  const all = { netColorMode: "all" as const, palette };
  assert.equal(copperColor("#000", "SIG", all, true), "rgba(255, 208, 128, 0.5)");
  assert.equal(copperColor("#000", "SIG", all, false), "rgba(128, 80, 0, 0.5)");
  assert.equal(copperColor("#000", "GND", all, true), "rgba(128, 240, 160, 1)");
});

test("the ratsnest takes the net's colour in every mode but None", () => {
  assert.equal(ratsnestColor("GND", { netColorMode: "all", palette }), "rgba(0, 224, 64, 1)");
  assert.equal(ratsnestColor("GND", { netColorMode: "ratsnest", palette }), "rgba(0, 224, 64, 1)");
  assert.equal(ratsnestColor("GND", { netColorMode: "off", palette }), null);
  assert.equal(ratsnestColor("VCC", { netColorMode: "all", palette }), null, "no colour: the ratsnest colour");
  assert.equal(ratsnestColor("GND", undefined), null);
});
