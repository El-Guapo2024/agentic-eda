import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_VIEWER3D_PREFS,
  FOLLOW_PCB,
  FOLLOW_PLOT_SETTINGS,
  LEGACY_COLORS,
  afterVisibilityChange,
  applyPreset,
  defaultVisibleLayers,
  deletePreset,
  deleteViewport,
  differentiatePlatedCopper,
  followPcbVisibility,
  kicadViewMatrix,
  killsFollow,
  legacyColorsPreset,
  matchingPreset,
  presetEntries,
  presetFromJson,
  rowShown,
  sanitizeViewer3DPrefs,
  savePreset,
  saveViewport,
  viewportFromJson,
  visibleSnapshot,
  type FollowSources,
} from "./viewer3dSettings";
import { plotFollowVisibility } from "./appearance3d";

const follow = (pcbOff: string[] = [], plotLayers?: string[]): FollowSources => ({
  pcb: followPcbVisibility((l) => !pcbOff.includes(l), (el) => !pcbOff.includes(el)),
  plot: plotFollowVisibility(plotLayers),
});

test("rebuildLayerPresetsWidget: the two Follow presets, the person's after a separator, then Save and Delete", () => {
  assert.deepEqual(presetEntries([]).map((e) => e.kind), ["follow", "follow", "separator", "save", "delete"]);
  const entries = presetEntries([legacyColorsPreset(), { name: "Mine", layers: {}, colors: {} }]);
  assert.deepEqual(entries.map((e) => ("name" in e ? e.name : e.kind)), [FOLLOW_PCB, FOLLOW_PLOT_SETTINGS, "separator", LEGACY_COLORS, "Mine", "separator", "save", "delete"]);
  assert.deepEqual(entries.filter((e) => e.kind === "follow").map((e) => (e as { label: string }).label), ["Follow PCB Editor", "Follow PCB Plot Settings"]);
});

test("GetVisibleLayers: a Follow preset decides the board's rows from the PCB editor or the plot settings, the others stay the viewer's own", () => {
  const f = follow(["F.SilkS", "values"], ["F.Cu", "B.Cu", "F.Mask"]);
  // FOLLOW_PCB: the editor hides F.SilkS and the values.
  assert.equal(rowShown({ silkscreen_top: true }, FOLLOW_PCB, f, "silkscreen_top"), false);
  assert.equal(rowShown({}, FOLLOW_PCB, f, "values"), false);
  assert.equal(rowShown({}, FOLLOW_PCB, f, "references"), true);
  // FOLLOW_PLOT_SETTINGS: the plot's layers (no silkscreen selected).
  assert.equal(rowShown({}, FOLLOW_PLOT_SETTINGS, f, "silkscreen_top"), false);
  assert.equal(rowShown({}, FOLLOW_PLOT_SETTINGS, f, "soldermask_top"), true);
  // User.n is the plot's under FOLLOW_PLOT_SETTINGS, the viewer's own under FOLLOW_PCB.
  assert.equal(rowShown({ user_3: true }, FOLLOW_PCB, f, "user_3"), true);
  assert.equal(rowShown({ user_3: true }, FOLLOW_PLOT_SETTINGS, f, "user_3"), false);
  // Rows no branch sets: the viewer's own.
  assert.equal(rowShown({ th_models: false }, FOLLOW_PCB, f, "th_models"), false);
  assert.equal(rowShown({}, FOLLOW_PLOT_SETTINGS, f, "navigator"), true);
  // A preset of the person's (or none): the rows as set.
  assert.equal(rowShown({ silkscreen_top: true }, "", f, "silkscreen_top"), true);
});

test("OnLayerVisibilityChanged: a board row takes the view off a Follow preset with every row as it was shown; a model row does not", () => {
  const f = follow(["F.SilkS"]);
  const off = afterVisibilityChange({ layers: {}, preset: FOLLOW_PCB }, "copper_top", false, { follow: f, presets: [], colors: {}, useStackupColors: false });
  assert.equal(off.preset, "", "no preset matches");
  assert.equal(off.layers.copper_top, false);
  assert.equal(off.layers.silkscreen_top, false, "the followed rows are kept as they were shown");
  const model = afterVisibilityChange({ layers: {}, preset: FOLLOW_PCB }, "th_models", false, { follow: f, presets: [], colors: {}, useStackupColors: false });
  assert.equal(model.preset, FOLLOW_PCB);
  assert.equal(killsFollow("th_models"), false);
  assert.equal(killsFollow("user_12"), true);
  assert.equal(killsFollow("navigator"), false);
  // A change that lands on a saved preset selects it (syncLayerPresetSelection).
  const saved = savePreset([], "Copper only", { ...visibleSnapshot({}, "", f), silkscreen_top: false }, { board: "#332b17e6" });
  const back = afterVisibilityChange({ layers: {}, preset: "" }, "silkscreen_top", false, { follow: f, presets: saved, colors: { board: "#332b17e6" }, useStackupColors: false });
  assert.equal(back.preset, "Copper only");
});

test("syncLayerPresetSelection compares every row and the colours both have; legacy colors never matches with stackup colours on", () => {
  const legacy = legacyColorsPreset();
  assert.equal(matchingPreset([legacy], defaultVisibleLayers(), legacy.colors, false), LEGACY_COLORS);
  assert.equal(matchingPreset([legacy], defaultVisibleLayers(), legacy.colors, true), "");
  assert.equal(matchingPreset([legacy], { ...defaultVisibleLayers(), board: false }, legacy.colors, false), "");
  assert.equal(matchingPreset([legacy], defaultVisibleLayers(), { ...legacy.colors, board: "#ff0000ff" }, false), "");
  // GetDefaultVisibleLayers: the named user layers off, the navigator on, the models not in POS files and the DNP ones off.
  const d = defaultVisibleLayers();
  assert.deepEqual([d.user_drawings, d.navigator, d.not_in_pos_models, d.dnp_models, d.off_board_silk, d.user_7], [false, true, false, false, false, false]);
});

test("doApplyLayerPreset, Save preset... and Delete preset...", () => {
  const legacy = legacyColorsPreset();
  assert.deepEqual(applyPreset([legacy], FOLLOW_PCB), { preset: FOLLOW_PCB });
  const applied = applyPreset([legacy], LEGACY_COLORS)!;
  assert.equal(applied.useStackupColors, false, "legacy colors turns the stackup colours off");
  assert.deepEqual(applied.layers, legacy.layers);
  assert.equal(applyPreset([legacy], "nope"), null);
  // A new name at the end, an existing one overwritten in place.
  const two = savePreset(savePreset([legacy], "A", { board: false }, {}), "B", {}, {});
  assert.deepEqual(two.map((p) => p.name), [LEGACY_COLORS, "A", "B"]);
  assert.deepEqual(savePreset(two, "A", { board: true }, {}).map((p) => [p.name, p.layers.board]), [[LEGACY_COLORS, true], ["A", true], ["B", undefined]]);
  assert.deepEqual(deletePreset(two, "A", "A"), { presets: [two[0], two[2]], current: "" });
  assert.equal(deletePreset(two, "B", "A").current, "A");
  // Stored presets: junk is dropped.
  assert.equal(presetFromJson({ layers: {} }), null);
  assert.deepEqual(presetFromJson({ name: "X", layers: { board: false, bad: 3 }, colors: { board: "#000000", bad: "nope" } }), { name: "X", layers: { board: false }, colors: { board: "#000000" } });
});

test("viewports: Save viewport... adds or updates, Delete viewport... removes, a stored one is read back or dropped", () => {
  const cam = { pos: { x: 0, y: 0, z: -10 }, lookAt: { x: 1, y: 0, z: 2 }, auxDeg: { x: -90, y: 0, z: 0 }, zoom: 0.8, quat: { x: 0, y: 0, z: 0, w: 1 } };
  const one = saveViewport([], "Front", cam);
  const two = saveViewport(one, "Iso", { ...cam, zoom: 1.2 });
  assert.deepEqual(two.map((v) => v.name), ["Front", "Iso"]);
  assert.equal(saveViewport(two, "Front", { ...cam, zoom: 2 })[0]!.camera.zoom, 2);
  assert.deepEqual(deleteViewport(two, "Front").map((v) => v.name), ["Iso"]);
  assert.deepEqual(viewportFromJson(JSON.parse(JSON.stringify(one[0]))), one[0]);
  assert.equal(viewportFromJson({ name: "x", camera: { zoom: 1 } }), null);
});

test("the 3D viewer's preferences: KiCad's defaults and ranges", () => {
  assert.deepEqual(sanitizeViewer3DPrefs({}), DEFAULT_VIEWER3D_PREFS);
  assert.deepEqual([DEFAULT_VIEWER3D_PREFS.rotationIncrementDeg, DEFAULT_VIEWER3D_PREFS.movingSpeedMultiplier, DEFAULT_VIEWER3D_PREFS.copperThickness, DEFAULT_VIEWER3D_PREFS.antialiasing, DEFAULT_VIEWER3D_PREFS.selectionColor], [10, 3, false, "8x", "#00ff00"]);
  const p = sanitizeViewer3DPrefs({ rotationIncrementDeg: 400, movingSpeedMultiplier: 9.4, antialiasing: "16x", selectionColor: "#ff000080", highlightOnRollover: false });
  assert.deepEqual([p.rotationIncrementDeg, p.movingSpeedMultiplier, p.antialiasing, p.selectionColor, p.highlightOnRollover], [359, 5, "8x", "#ff0000", false]);
});

test("the Render Options and While Moving rows: KiCad's defaults (all off, Realistic) and DifferentiatePlatedCopper", () => {
  const d = DEFAULT_VIEWER3D_PREFS;
  assert.deepEqual(
    [d.clipSilkOnViaAnnuli, d.subtractMaskFromSilk, d.differentiatePlatedCopper, d.materialMode, d.aaDisableOnMove, d.thicknessDisableOnMove, d.microviasDisableOnMove, d.holesDisableOnMove],
    [false, false, false, "realistic", false, false, false, false]
  );
  const p = sanitizeViewer3DPrefs({ clipSilkOnViaAnnuli: true, subtractMaskFromSilk: 1, materialMode: "cad", holesDisableOnMove: true });
  assert.deepEqual([p.clipSilkOnViaAnnuli, p.subtractMaskFromSilk, p.materialMode, p.holesDisableOnMove], [true, false, "cad", true]);
  assert.equal(sanitizeViewer3DPrefs({ materialMode: "glossy" }).materialMode, "realistic");
  // `if( engine == OPENGL && use_board_editor_copper_colors ) return false;`
  assert.equal(differentiatePlatedCopper({ differentiatePlatedCopper: true }, false), true);
  assert.equal(differentiatePlatedCopper({ differentiatePlatedCopper: true }, true), false);
});

test("a saved viewport's KiCad view matrix (`CAMERA::updateViewMatrix`, in 3D units of span / 12.8)", () => {
  // The home top view of a 64 mm board looking at (10, 0, 5) mm: no rotation, the camera 1.25 * 64 = 80 mm back (16 units), the look-at point at (10, -5, 0) mm in
  // KiCad's world (board y inverted), 2 units and -1 unit.
  const top = { pos: { x: 0, y: 0, z: -80 }, lookAt: { x: 10, y: 0, z: 5 }, auxDeg: { x: 0, y: 0, z: 0 }, zoom: 1, quat: { x: 0, y: 0, z: 0, w: 1 } };
  const m = kicadViewMatrix(top, 64);
  const near = (a: number[], b: number[]) => a.every((x, i) => Math.abs(x - b[i]!) < 1e-9);
  assert.ok(near(m, [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -2, 1, -16, 1]), String(m));
  // The front view (aux x -90): the board's thickness axis is the camera's y.
  const front = kicadViewMatrix({ ...top, auxDeg: { x: -90, y: 0, z: 0 } }, 64);
  assert.ok(Math.abs(front[9]! - 1) < 1e-9 || Math.abs(front[9]! + 1) < 1e-9, "z goes to y");
  const vps = saveViewport([], "Top", top, 64);
  assert.equal(vps[0]!.matrix!.length, 16);
  assert.deepEqual(viewportFromJson(JSON.parse(JSON.stringify(vps[0]))), vps[0]);
});
