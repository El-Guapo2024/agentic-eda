import { test } from "node:test";
import assert from "node:assert/strict";
import {
  TECH_LAYER_NAMES,
  allPresets,
  builtinPresets,
  currentViewport,
  firstLayer,
  hideAllButActive,
  layerGroupOp,
  layerId,
  layerIsVisible,
  layerNameOfKey,
  layerStateKey,
  matchingPreset,
  menuPreset,
  panelLayers,
  presetList,
  savePresetOutcome,
  selectPreset,
  snapshotPreset,
  viewForViewport,
  withPreset,
  withViewport,
  withoutPreset,
  withoutViewport,
  type PresetView,
} from "./layerPresets";
import { DEFAULT_OBJECTS } from "./layerPresets";
import { COPPER2, COPPER4 } from "./appearanceFixture";

function view(copper: string[], over: Partial<PresetView> = {}): PresetView {
  const universe = panelLayers(copper);
  return { universe, visibleLayers: new Set(universe), objects: [...DEFAULT_OBJECTS], activeLayer: "F.Cu", flipBoard: false, activePreset: "All Layers", lastBuiltinObjects: null, boardLayers: new Set(universe), ...over };
}

test("layer state keys: copper by name, the nine painter buckets by bucket name, any other by colour key; and back", () => {
  assert.equal(layerStateKey("F.Cu"), "F.Cu");
  assert.equal(layerStateKey("In1.Cu"), "In1.Cu");
  assert.equal(layerStateKey("F.SilkS"), "f_silks");
  assert.equal(layerStateKey("Edge.Cuts"), "board_edge");
  assert.equal(layerStateKey("F.CrtYd"), "f_courtyard");
  assert.equal(layerStateKey("Dwgs.User"), "Dwgs_User");
  assert.equal(layerStateKey("F.Paste"), "F_Paste");
  for (const n of [...COPPER4, ...TECH_LAYER_NAMES]) assert.equal(layerNameOfKey(layerStateKey(n)), n, n);
});

test("layerIsVisible takes a KiCad layer name or a state key and treats an unknown layer as on", () => {
  const lv = { f_silks: false, "B.Cu": false, Dwgs_User: false };
  assert.equal(layerIsVisible(lv, "F.SilkS"), false);
  assert.equal(layerIsVisible(lv, "B.Cu"), false);
  assert.equal(layerIsVisible(lv, "Dwgs.User"), false);
  assert.equal(layerIsVisible(lv, "F.Cu"), true);
  assert.equal(layerIsVisible(lv, "User.7"), true);
});

test("PCB_LAYER_ID order: F.Cu, F.Mask, B.Cu, B.Mask, In1.Cu, F.SilkS ... -- the first layer of a set is the lowest id", () => {
  assert.deepEqual(["F.Cu", "F.Mask", "B.Cu", "B.Mask", "In1.Cu", "F.SilkS", "In2.Cu", "B.SilkS", "Edge.Cuts"].map(layerId), [0, 1, 2, 3, 4, 5, 6, 7, 25]);
  assert.equal(firstLayer(["B.Mask", "B.Cu", "Edge.Cuts"]), "B.Cu");
  assert.equal(firstLayer(["In2.Cu", "In1.Cu"]), "In1.Cu");
  assert.equal(firstLayer([]), null);
});

test("the built-in presets are eight, read-only and listed alphabetically (a std::map by name)", () => {
  const p = builtinPresets(COPPER4);
  assert.deepEqual(
    p.map((x) => x.name),
    ["All Copper Layers", "All Layers", "Back Assembly View", "Back Layers", "Front Assembly View", "Front Layers", "Inner Copper Layers", "No Layers"]
  );
  assert.ok(p.every((x) => x.readOnly));
  const by = (n: string) => p.find((x) => x.name === n)!;
  assert.deepEqual(by("Front Layers").layers, ["F.Cu", "F.SilkS", "F.Mask", "F.Adhes", "F.Paste", "F.CrtYd", "F.Fab", "Edge.Cuts"]);
  assert.equal(by("Front Layers").flipBoard, false);
  assert.equal(by("Back Layers").flipBoard, true, "presetBack flips the board");
  assert.equal(by("Back Assembly View").flipBoard, true);
  assert.equal(by("Front Assembly View").activeLayer, "F.SilkS");
  assert.equal(by("Back Assembly View").activeLayer, "B.SilkS");
  assert.deepEqual(by("Inner Copper Layers").layers, ["In1.Cu", "In2.Cu", "Edge.Cuts"]);
  assert.deepEqual(by("All Copper Layers").layers, [...COPPER4, "Edge.Cuts"]);
  assert.deepEqual(by("No Layers").layers, []);
  assert.deepEqual(by("All Layers").layers, panelLayers(COPPER4));
  assert.deepEqual(by("Front Layers").renderLayers, [...DEFAULT_OBJECTS], "the built-in presets name GAL_SET::DefaultVisible");
});

test("the list: built-in presets, then (when there are any) a separator and the user's, a separator, Save and Delete", () => {
  const kinds = (u: Parameters<typeof presetList>[1]) => presetList(COPPER2, u).map((e) => (e.kind === "preset" ? e.preset.name : e.kind));
  assert.deepEqual(kinds([]).slice(-3), ["separator", "save", "delete"]);
  assert.equal(kinds([]).length, 8 + 3);
  const mine = snapshotPreset("Zed", view(COPPER2));
  const other = snapshotPreset("Alpha", view(COPPER2));
  const withMine = kinds([mine, other]);
  assert.deepEqual(withMine.slice(8), ["separator", "Alpha", "Zed", "separator", "save", "delete"]);
  // wxString order: upper case sorts before lower case
  assert.deepEqual(allPresets(COPPER2, [snapshotPreset("alpha", view(COPPER2)), snapshotPreset("Beta", view(COPPER2))]).filter((p) => !p.readOnly).map((p) => p.name), ["Beta", "alpha"]);
  // a user preset cannot shadow a built-in one
  assert.equal(allPresets(COPPER2, [snapshotPreset("All Layers", view(COPPER2))]).filter((p) => p.name === "All Layers").length, 1);
});

test("picking Front Layers shows the front layers and Edge.Cuts only, and the active layer stays when the preset keeps it", () => {
  const all = allPresets(COPPER2, []);
  const front = all.find((p) => p.name === "Front Layers")!;
  const v = selectPreset(view(COPPER2), front, all);
  assert.deepEqual([...v.visibleLayers].sort(), ["Edge.Cuts", "F.Adhes", "F.CrtYd", "F.Cu", "F.Fab", "F.Mask", "F.Paste", "F.SilkS"]);
  assert.equal(v.activeLayer, "F.Cu");
  assert.equal(v.flipBoard, false);
  assert.equal(v.activePreset, "Front Layers");
});

test("picking Back Layers moves the active layer to the preset's first layer and flips the board", () => {
  const all = allPresets(COPPER2, []);
  const back = all.find((p) => p.name === "Back Layers")!;
  const v = selectPreset(view(COPPER2, { activeLayer: "F.Cu" }), back, all);
  assert.equal(v.activeLayer, "B.Cu", "F.Cu is hidden now: the first layer of the set takes over");
  assert.equal(v.flipBoard, true);
  assert.equal(v.visibleLayers.has("F.Cu"), false);
});

test("an assembly view makes its silkscreen the active layer", () => {
  const all = allPresets(COPPER2, []);
  const v = selectPreset(view(COPPER2), all.find((p) => p.name === "Front Assembly View")!, all);
  assert.equal(v.activeLayer, "F.SilkS");
  assert.deepEqual([...v.visibleLayers].sort(), ["Edge.Cuts", "F.CrtYd", "F.Fab", "F.Mask", "F.SilkS"]);
});

test("a layer the board does not have is not made active", () => {
  const all = allPresets(COPPER2, []);
  const inner = all.find((p) => p.name === "Inner Copper Layers")!;
  const v = selectPreset(view(COPPER2), inner, all);
  assert.equal(v.activeLayer, "Edge.Cuts", "a two-layer board has no inner copper: the set is Edge.Cuts alone, the first layer of it takes over");
  assert.deepEqual([...v.visibleLayers], ["Edge.Cuts"]);
  const v4 = selectPreset(view(COPPER4), allPresets(COPPER4, []).find((p) => p.name === "Inner Copper Layers")!, allPresets(COPPER4, []));
  assert.equal(v4.activeLayer, "In1.Cu");
});

test("a built-in preset does not change which objects are on; it restores the ones from when a built-in preset was last left", () => {
  const all = allPresets(COPPER2, []);
  const front = all.find((p) => p.name === "Front Layers")!;
  // The person hides the tracks; the list now shows no preset ("").
  const custom = view(COPPER2, { objects: DEFAULT_OBJECTS.filter((o) => o !== "tracks"), activePreset: "" });
  const v = selectPreset(custom, front, all);
  assert.equal(v.objects.includes("tracks"), false, "the hidden tracks stay hidden");
  assert.equal(v.lastBuiltinObjects?.includes("tracks"), false);
  // A user preset with its own objects takes over ...
  const mine = snapshotPreset("Mine", view(COPPER2, { objects: ["tracks", "pads"] }));
  const all2 = allPresets(COPPER2, [mine]);
  const onMine = selectPreset(v, mine, all2);
  assert.deepEqual(onMine.objects.filter((o) => o !== "ratsnest"), ["tracks", "pads"]);
  assert.equal(onMine.activePreset, "Mine");
  // ... and going back to a built-in one restores what was on when the last built-in was left (no tracks), not the user preset's objects.
  const back = selectPreset(onMine, front, all2);
  assert.equal(back.objects.includes("tracks"), false);
  assert.equal(back.objects.includes("vias"), true);
});

test("a preset leaves the ratsnest to its own switch, but sets the grid", () => {
  const mine = snapshotPreset("Mine", view(COPPER2, { objects: ["tracks", "grid", "ratsnest"] }));
  const all = allPresets(COPPER2, [mine]);
  const off = selectPreset(view(COPPER2, { objects: ["tracks", "pads"] }), mine, all);
  assert.equal(off.objects.includes("ratsnest"), false, "off stays off, whatever the preset holds");
  assert.equal(off.objects.includes("grid"), true, "the grid is part of the preset's objects");
  const on = selectPreset(view(COPPER2, { objects: ["tracks", "ratsnest"] }), snapshotPreset("Bare", view(COPPER2, { objects: ["tracks"] })), allPresets(COPPER2, []));
  assert.equal(on.objects.includes("ratsnest"), true, "on stays on");
  // A built-in preset brings the objects back as they were: the grid that was off stays off.
  const builtin = selectPreset(view(COPPER2, { objects: DEFAULT_OBJECTS.filter((o) => o !== "grid") }), all.find((p) => p.name === "Front Layers")!, all);
  assert.equal(builtin.objects.includes("grid"), false);
});

test("the list shows the preset the visibility matches, or the blank entry (syncLayerPresetSelection)", () => {
  const all = allPresets(COPPER2, []);
  const v = view(COPPER2);
  assert.equal(matchingPreset(all, v)?.name, "All Layers");
  const hidden = { ...v, visibleLayers: new Set([...v.visibleLayers].filter((l) => l !== "B.SilkS")) };
  assert.equal(matchingPreset(all, hidden), null, "one layer off and nothing matches");
  const front = all.find((p) => p.name === "Front Layers")!;
  assert.equal(matchingPreset(all, { ...v, visibleLayers: new Set(front.layers) })?.name, "Front Layers");
  assert.equal(matchingPreset(all, { ...v, visibleLayers: new Set(front.layers), flipBoard: true }), null, "the flip is part of the match");
  assert.equal(matchingPreset(all, { ...v, objects: v.objects.filter((o) => o !== "pads") }), null, "so are the objects");
});

test("the right-click menu's presets keep the objects and the flip, and leave the list blank", () => {
  const v = view(COPPER4, { objects: DEFAULT_OBJECTS.filter((o) => o !== "zones"), flipBoard: false });
  const front = menuPreset(v, "front", COPPER4);
  assert.equal(front.objects.includes("zones"), false);
  assert.equal(front.flipBoard, false);
  assert.equal(front.activePreset, "");
  const back = menuPreset(v, "back", COPPER4);
  assert.equal(back.flipBoard, false, "Show Only Back Layers does not flip (the preset's flip is not copied)");
  assert.equal(back.activeLayer, "B.Cu");
  const asm = menuPreset(v, "front_assembly", COPPER4);
  assert.equal(asm.activeLayer, "F.Mask", "F.Cu is hidden, so the first layer of the set takes over; the assembly entries do not pick F.SilkS (only the layers are copied)");
  const none = menuPreset(v, "no_layers", COPPER4);
  assert.equal(none.visibleLayers.size, 0);
  assert.equal(none.activeLayer, "F.Cu", "no layers: nothing to move the active layer to");
  assert.equal(menuPreset(none, "all_layers", COPPER4).visibleLayers.size, panelLayers(COPPER4).length);
});

test("Hide All Layers But Active keeps the active layer only", () => {
  const v = hideAllButActive(view(COPPER2, { activeLayer: "B.Cu" }));
  assert.deepEqual([...v.visibleLayers], ["B.Cu"]);
  assert.deepEqual([...hideAllButActive(view(COPPER2, { activeLayer: null })).visibleLayers], []);
});

test("the four copper / non-copper entries; hiding the active layer's group moves the active layer to the lowest id left on", () => {
  const v = view(COPPER4, { activeLayer: "F.Cu" });
  const noCopper = layerGroupOp(v, "hide_copper");
  assert.deepEqual([...noCopper.visibleLayers].filter((l) => l.endsWith(".Cu")), []);
  assert.equal(noCopper.activeLayer, "F.Mask", "F.Mask is id 1, the lowest of what is left");
  assert.equal(layerGroupOp(noCopper, "show_copper").visibleLayers.size, panelLayers(COPPER4).length);
  const noTech = layerGroupOp(v, "hide_non_copper");
  assert.deepEqual([...noTech.visibleLayers].sort(), [...COPPER4].sort());
  assert.equal(noTech.activeLayer, "F.Cu", "the active layer is still on");
  assert.equal(layerGroupOp(noTech, "show_non_copper").visibleLayers.size, panelLayers(COPPER4).length);
  // Hiding every layer leaves the active layer as it is (nothing to move to).
  const empty = layerGroupOp(layerGroupOp(v, "hide_copper"), "hide_non_copper");
  assert.equal(empty.visibleLayers.size, 0);
  assert.equal(empty.activeLayer, "F.Mask");
});

test("Save preset: a new name, a user preset's name (asks first), a built-in's name (refused), nothing", () => {
  const mine = snapshotPreset("Mine", view(COPPER2));
  const all = allPresets(COPPER2, [mine]);
  assert.deepEqual(savePresetOutcome("Fresh", all), { kind: "new" });
  assert.deepEqual(savePresetOutcome("Mine", all), { kind: "overwrite" });
  const refused = savePresetOutcome("Front Layers", all);
  assert.equal(refused.kind, "refused");
  assert.match((refused as { message: string }).message, /Default presets cannot be modified/);
  assert.deepEqual(savePresetOutcome("   ", all), { kind: "empty" });
});

test("a saved preset holds the layers on, the objects on and the flip; saving over a name replaces it; delete removes only the user's", () => {
  const v = view(COPPER2, { visibleLayers: new Set(["F.Cu", "Edge.Cuts"]), objects: ["tracks", "grid"], flipBoard: true });
  const p = snapshotPreset("Routing", v);
  assert.deepEqual(p, { name: "Routing", layers: ["F.Cu", "Edge.Cuts"], renderLayers: ["tracks", "grid"], flipBoard: true, activeLayer: null, readOnly: false });
  const list = withPreset([], p);
  const again = withPreset(list, { ...p, layers: ["B.Cu"] });
  assert.equal(again.length, 1);
  assert.deepEqual(again[0]!.layers, ["B.Cu"]);
  assert.deepEqual(withoutPreset(again, "Routing"), []);
  assert.deepEqual(withoutPreset(again, "All Layers"), again, "a built-in name is not in the user's list, so nothing goes");
  // Applying it brings back exactly that.
  const all = allPresets(COPPER2, again);
  const onIt = selectPreset(view(COPPER2), all.find((x) => x.name === "Routing")!, all);
  assert.deepEqual([...onIt.visibleLayers], ["B.Cu"]);
  assert.equal(onIt.flipBoard, true);
});

test("a viewport is the part of the board on screen; recalling it centres it and fits the larger ratio (VIEW::GetViewport / SetViewport)", () => {
  const view0 = { scale: 0.01, x: -500, y: -200 }; // 100 x 50 px canvas shows x 50000..60000 um, y 20000..25000 um
  const rect = currentViewport(view0, 100, 50);
  assert.deepEqual(rect, { x: 50000, y: 20000, w: 10000, h: 5000 });
  const back = viewForViewport(rect, 100, 50);
  assert.ok(Math.abs(back.scale - 0.01) < 1e-12 && Math.abs(back.x + 500) < 1e-9 && Math.abs(back.y + 200) < 1e-9, "the same canvas gets the same view back");
  // A taller canvas: the width decides, the rectangle stays centred.
  const tall = viewForViewport(rect, 100, 200);
  assert.ok(Math.abs(tall.scale - 0.01) < 1e-12);
  assert.ok(Math.abs(tall.y - (100 - 22500 * 0.01)) < 1e-9, "centre row of the canvas");
  // A wider canvas: the height decides.
  assert.ok(Math.abs(viewForViewport(rect, 400, 50).scale - 0.01) < 1e-12);
  assert.ok(Math.abs(viewForViewport(rect, 200, 100).scale - 0.02) < 1e-12);
});

test("saving a viewport under an existing name replaces it; they are listed by name; delete removes one", () => {
  let v = withViewport([], "b", { x: 0, y: 0, w: 10, h: 10 });
  v = withViewport(v, "a", { x: 1, y: 1, w: 2, h: 2 });
  assert.deepEqual(v.map((x) => x.name), ["a", "b"]);
  v = withViewport(v, "a", { x: 5, y: 5, w: 6, h: 6 });
  assert.equal(v.length, 2);
  assert.deepEqual(v[0], { name: "a", x: 5, y: 5, w: 6, h: 6 });
  assert.deepEqual(withoutViewport(v, "a").map((x) => x.name), ["b"]);
});
