import { test } from "node:test";
import assert from "node:assert/strict";
import type { CmdShape, CmdText, LibraryFootprint, LibraryPad } from "../api/types";
import {
  FP_OBJECT_ROWS,
  FP_SHOWS_INNER_LAYERS_ENTRY,
  fpContrastMode,
  fpLayerLabel,
  fpLayerMenu,
  fpLayersPatch,
  fpLayerTooltip,
  fpObjectChecked,
  fpObjectOpacity,
  fpObjectPatch,
  fpOpacities,
  fpPadOnLayer,
  fpPadShown,
  fpPanelLayers,
  fpShapeOpacity,
  fpTextShown,
  fpUsedLayers,
  type FpVisibility,
} from "./fpAppearance";
import { fpHitTest } from "./fpItems";
import { layerIsVisible } from "./layerPresets";

const pad = (id: string, x: number, layers: string[] = ["F.Cu", "F.Paste", "F.Mask"]): LibraryPad =>
  ({ id, number: id, at: { x, y: 0 }, size: [1_000_000, 600_000], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", drill: null, drill_slot: null, rot: 0, layers, chamfer_corners: {} }) as unknown as LibraryPad;
const text = (id: string, content: string, y: number): CmdText => ({ id, content, at: { x: 0, y }, angle: 0, layer: "F.Fab", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false });
const line = (id: string, y: number, layer = "F.SilkS"): CmdShape => ({ id, kind: "segment", layer, stroke_width: 120_000, filled: false, start: { x: -3_000_000, y }, end: { x: 3_000_000, y } });

function footprint(extra: Partial<LibraryFootprint> = {}): LibraryFootprint {
  return {
    name: "Lib:Part",
    description: "",
    keywords: "",
    attributes: { smd: true } as never,
    reference_visible: true,
    value_visible: true,
    model: null,
    pads: [pad("pad_1", -2_000_000), pad("pad_2", 2_000_000)],
    graphics: [line("fpg_1", -1_000_000), line("fpg_7", -2_000_000, "User.7")],
    texts: [text("fpt_1", "${REFERENCE}", 2_500_000), text("fpt_2", "note", 6_000_000)],
    fields: [{ name: "MPN", value: "X123", visible: true, layout: { at: { x: 0, y: 4_000_000 }, size: [1_000_000, 1_000_000], layer: "F.Fab" } }],
    courtyard: null,
    anchor: { x: 0, y: 0 },
    published: false,
    ...extra,
  } as LibraryFootprint;
}

const TECH = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

test("the Layers tab lists the editor's board: F.Cu, Inner layers, B.Cu, non_cu_seq, User.1 .. User.4 and the user layers the footprint uses (updateEnabledLayers)", () => {
  assert.deepEqual(fpPanelLayers(), ["F.Cu", "In1.Cu", "B.Cu", ...TECH, "User.1", "User.2", "User.3", "User.4"]);
  const used = fpUsedLayers(footprint());
  assert.ok(used.includes("User.7") && used.includes("F.Mask") && used.includes("F.Fab"));
  assert.deepEqual(fpPanelLayers(used).slice(-5), ["User.1", "User.2", "User.3", "User.4", "User.7"]);
  assert.deepEqual(fpPanelLayers(["User.2"], 0), ["F.Cu", "In1.Cu", "B.Cu", ...TECH, "User.2"]);
  assert.deepEqual(fpUsedLayers(null), []);
});

test("a row's name is the board's layer name (In1.Cu is 'Inner layers') or the standard one, its tooltip non_cu_seq's", () => {
  assert.deepEqual(["F.Cu", "In1.Cu", "B.Cu", "F.SilkS", "Dwgs.User", "F.CrtYd", "F.Adhes", "Edge.Cuts", "User.3"].map((n) => fpLayerLabel(n)), ["F.Cu", "Inner layers", "B.Cu", "F.Silkscreen", "User.Drawings", "F.Courtyard", "F.Adhesive", "Edge.Cuts", "User.3"]);
  assert.deepEqual(["F.Cu", "In1.Cu", "B.Cu", "B.Mask", "Edge.Cuts", "Margin", "F.Fab", "User.12"].map(fpLayerTooltip), [
    "Front copper layer",
    "Inner copper layer",
    "Back copper layer",
    "Solder mask on board's back",
    "Board's perimeter definition",
    "Board's edge setback outline",
    "Footprint assembly on board's front",
    "User defined layer 12",
  ]);
});

test("a layer is set under its name and its state key, so an old entry under either does not win", () => {
  const vis = { "F.SilkS": false };
  const next = { ...vis, ...fpLayersPatch(["F.SilkS"], true) };
  assert.equal(layerIsVisible(next, "F.SilkS"), true);
  assert.deepEqual(fpLayersPatch(["F.SilkS", "Dwgs.User", "F.Cu"], false), { "F.SilkS": false, f_silks: false, "Dwgs.User": false, Dwgs_User: false, "F.Cu": false });
});

test("the Objects tab keeps s_allowedInFpEditor's rows in s_objectSettings' order, with no spacer", () => {
  assert.deepEqual(
    FP_OBJECT_ROWS.map((r) => r.id),
    ["tracks", "vias", "pads", "zones", "shapes", "bitmaps", "footprint_values", "footprint_references", "footprint_text", "ly_points", "via_stitching", "grid"]
  );
  assert.deepEqual(
    FP_OBJECT_ROWS.map((r) => r.label),
    ["Tracks", "Vias", "Pads", "Zones", "Filled Shapes", "Images", "Values", "References", "Footprint Text", "Points", "Via Stitching", "Grid"]
  );
  // a slider for the six opacities; Filled Shapes has no eye
  assert.deepEqual(FP_OBJECT_ROWS.filter((r) => r.opacity).map((r) => r.id), ["tracks", "vias", "pads", "zones", "shapes", "bitmaps"]);
  assert.deepEqual(FP_OBJECT_ROWS.filter((r) => !r.checkbox).map((r) => r.id), ["shapes"]);
});

test("Footprint Text turns References and Values with it; References on puts Footprint Text back (onObjectVisibilityChanged); Grid is the frame's grid", () => {
  assert.deepEqual(fpObjectPatch({}, "footprint_text", false), { "obj:footprint_text": false, "obj:footprint_references": false, "obj:footprint_values": false });
  const off = { "obj:footprint_text": false, "obj:footprint_references": false, "obj:footprint_values": false };
  assert.deepEqual(fpObjectPatch(off, "footprint_references", true), { "obj:footprint_references": true, "obj:footprint_text": true });
  assert.deepEqual(fpObjectPatch({}, "pads", false), { "obj:pads": false });
  assert.equal(fpObjectChecked({ layerVisible: {}, gridVisible: false }, "grid"), false);
  assert.equal(fpObjectChecked({ layerVisible: {}, gridVisible: true }, "grid"), true);
  assert.equal(fpObjectChecked({ layerVisible: { "obj:pads": false }, gridVisible: true }, "pads"), false);
});

test("every opacity starts at 1 (PCB_DISPLAY_OPTIONS); a filled shape takes Filled Shapes', a copper line Tracks' (GetColor)", () => {
  assert.deepEqual(fpOpacities({}), { tracks: 1, vias: 1, pads: 1, zones: 1, images: 1, shapes: 1 });
  assert.deepEqual(fpOpacities({ "obj:bitmaps": 0.3, "obj:zones": 0.5 }), { tracks: 1, vias: 1, pads: 1, zones: 0.5, images: 0.3, shapes: 1 });
  assert.equal(fpObjectOpacity(undefined, "pads"), 1);
  const op = { "obj:shapes": 0.4, "obj:tracks": 0.7 };
  assert.equal(fpShapeOpacity({ kind: "rect", filled: true, layer: "F.SilkS" }, op), 0.4);
  assert.equal(fpShapeOpacity({ kind: "segment", filled: true, layer: "F.SilkS" }, op), 1);
  assert.equal(fpShapeOpacity({ kind: "segment", filled: false, layer: "F.Cu" }, op), 0.7);
  assert.equal(fpShapeOpacity({ kind: "circle", filled: false, layer: "F.Fab" }, op), 1);
});

test("Normal / Dim / Hide from the two switches", () => {
  assert.equal(fpContrastMode(false, false), "normal");
  assert.equal(fpContrastMode(false, true), "normal");
  assert.equal(fpContrastMode(true, false), "dimmed");
  assert.equal(fpContrastMode(true, true), "hidden");
});

test("the layer menu (OnLayerContextMenu): the copper entries take Edge.Cuts along, a hiding entry moves the active layer to the first one left", () => {
  const universe = fpPanelLayers();
  const shown = (r: { visible: Record<string, boolean> }) => universe.filter((l) => layerIsVisible(r.visible, l));
  assert.equal(FP_SHOWS_INNER_LAYERS_ENTRY, true);

  const hideCu = fpLayerMenu(universe, {}, "F.Cu", "hide_copper");
  assert.ok(!shown(hideCu).some((l) => /\.Cu$/.test(l) || l === "Edge.Cuts"));
  assert.equal(hideCu.activeLayer, "F.Mask"); // PCB_LAYER_ID order: F.Mask is 1

  const hideNonCu = fpLayerMenu(universe, {}, "F.SilkS", "hide_non_copper");
  assert.deepEqual(shown(hideNonCu), ["F.Cu", "In1.Cu", "B.Cu", "Edge.Cuts"]);
  assert.equal(hideNonCu.activeLayer, "F.Cu");

  const none = fpLayerMenu(universe, {}, "F.SilkS", "no_layers");
  assert.deepEqual(shown(none), []);
  assert.equal(none.activeLayer, "F.SilkS");
  const showCu = fpLayerMenu(universe, none.visible, "F.SilkS", "show_copper");
  assert.deepEqual(shown(showCu), ["F.Cu", "In1.Cu", "B.Cu", "Edge.Cuts"]);
  const showNonCu = fpLayerMenu(universe, none.visible, "F.SilkS", "show_non_copper");
  assert.equal(shown(showNonCu).length, universe.length - 4);

  const butActive = fpLayerMenu(universe, {}, "F.Fab", "hide_all_but_active");
  assert.deepEqual(shown(butActive), ["F.Fab"]);

  const front = fpLayerMenu(universe, {}, "B.SilkS", "front");
  assert.deepEqual(shown(front), ["F.Cu", "F.Adhes", "F.Paste", "F.SilkS", "F.Mask", "Edge.Cuts", "F.CrtYd", "F.Fab"]);
  assert.equal(front.activeLayer, "F.Cu"); // B.SilkS is not in the preset: its first layer
  assert.equal(fpLayerMenu(universe, {}, "F.SilkS", "front_assembly").activeLayer, "F.SilkS");

  const inner = fpLayerMenu(universe, {}, "F.SilkS", "inner_copper");
  assert.deepEqual(shown(inner), ["In1.Cu", "Edge.Cuts"]);
  assert.equal(inner.activeLayer, "In1.Cu");
  assert.equal(shown(fpLayerMenu(universe, none.visible, "F.SilkS", "all_layers")).length, universe.length);
});

test("a pad is drawn while a layer of it is shown (*.Cu is the editor's three copper layers) and is on the active layer by IsOnLayer", () => {
  assert.equal(fpPadOnLayer(["*.Cu", "*.Mask"], "In1.Cu"), true);
  assert.equal(fpPadOnLayer(["*.Cu", "*.Mask"], "B.Mask"), true);
  assert.equal(fpPadOnLayer(["F.Cu", "F.Mask"], "B.Cu"), false);
  assert.equal(fpPadOnLayer([], "F.Cu"), true);
  assert.equal(fpPadShown(["*.Cu"], { "F.Cu": false, "B.Cu": false }), true);
  assert.equal(fpPadShown(["*.Cu"], { "F.Cu": false, "In1.Cu": false, "B.Cu": false }), false);
  assert.equal(fpPadShown(["F.Cu", "F.Mask"], { "F.Cu": false }), true);
  assert.equal(fpPadShown(["F.Cu", "F.Mask"], { "F.Cu": false, f_mask: false }), false);
});

test("a footprint text ${REFERENCE} follows References, ${VALUE} Values, every text Footprint Text (PCB_TEXT::ViewGetLOD)", () => {
  assert.equal(fpTextShown({ "obj:footprint_references": false }, "${REFERENCE}"), false);
  assert.equal(fpTextShown({ "obj:footprint_references": false }, "note"), true);
  assert.equal(fpTextShown({ "obj:footprint_values": false }, "${VALUE}"), false);
  assert.equal(fpTextShown({ "obj:footprint_text": false }, "note"), false);
});

test("fpHitTest with the Appearance manager's switches picks only what Selectable lets through; without them everything", () => {
  const fp = footprint();
  const vis = (v: Partial<FpVisibility> = {}): FpVisibility => ({ layerVisible: {}, ...v });
  // without the argument: as before
  assert.deepEqual(fpHitTest(fp, 1_000_000, -1_000_000, 50_000), { kind: "graphic", id: "fpg_1" });
  assert.deepEqual(fpHitTest(fp, 1_000_000, -1_000_000, 50_000, vis()), { kind: "graphic", id: "fpg_1" });
  // a hidden layer (by its state key or by its name)
  assert.equal(fpHitTest(fp, 1_000_000, -1_000_000, 50_000, vis({ layerVisible: { f_silks: false } })), null);
  assert.equal(fpHitTest(fp, 1_000_000, -2_000_000, 50_000, vis({ layerVisible: { "User.7": false } })), null);
  assert.equal(fpHitTest(fp, 1_000_000, -2_000_000, 50_000, vis())?.id, "fpg_7");
  // pads: not the Pads row (the element visibility `Selectable` asks is not the row's in this editor, UI-LEFT-2 FF5), its opacity, its layers
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, vis())?.id, "pad_1");
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, vis({ layerVisible: { "obj:pads": false } }))?.id, "pad_1");
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, vis({ layerOpacity: { "obj:pads": 0 } })), null);
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, vis({ layerVisible: { "F.Cu": false, f_mask: false, F_Paste: false } })), null);
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, vis({ layerVisible: { "F.Cu": false } }))?.id, "pad_1");
  // the Reference field and a ${REFERENCE} text with References off; a text with Footprint Text off
  assert.equal(fpHitTest(fp, 0, -500_000, 50_000, vis())?.id, "field:Reference");
  assert.equal(fpHitTest(fp, 0, -500_000, 50_000, vis({ layerVisible: { "obj:footprint_references": false } })), null);
  assert.equal(fpHitTest(fp, 0, 2_500_000, 50_000, vis({ layerVisible: { "obj:footprint_references": false } })), null);
  assert.equal(fpHitTest(fp, 0, 6_000_000, 50_000, vis({ layerVisible: { "obj:footprint_references": false } }))?.id, "fpt_2");
  assert.equal(fpHitTest(fp, 0, 6_000_000, 50_000, vis({ layerVisible: { "obj:footprint_text": false } })), null);
  assert.equal(fpHitTest(fp, 0, 4_000_000, 50_000, vis({ layerVisible: { "obj:footprint_text": false } })), null);
  assert.equal(fpHitTest(fp, 0, 1_000_000, 50_000, vis({ layerVisible: { "obj:footprint_values": false } })), null);
  // high contrast: only the active layer's items
  const hc = vis({ highContrast: true, activeLayer: "F.SilkS" });
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, hc), null);
  assert.equal(fpHitTest(fp, 1_000_000, -1_000_000, 50_000, hc)?.id, "fpg_1");
  assert.equal(fpHitTest(fp, -2_000_000, 0, 50_000, vis({ highContrast: true, activeLayer: "F.Cu" }))?.id, "pad_1");
  // a filled shape at Filled Shapes opacity 0
  const filled = footprint({ graphics: [{ id: "fpg_r", kind: "rect", layer: "F.Fab", stroke_width: 100_000, filled: true, start: { x: 5_000_000, y: 5_000_000 }, end: { x: 7_000_000, y: 7_000_000 } }] });
  assert.equal(fpHitTest(filled, 6_000_000, 6_000_000, 50_000, vis())?.id, "fpg_r");
  assert.equal(fpHitTest(filled, 6_000_000, 6_000_000, 50_000, vis({ layerOpacity: { "obj:shapes": 0 } })), null);
});

test("the Footprint Editor's presets: what is on screen as a preset view, the matching preset, a preset applied (UI-LEFT-2 FF5)", async () => {
  const { fpPresetView, fpMatchingPreset, fpApplyPresetView, fpPassesFilter, DEFAULT_FP_SELECTION_FILTER } = await import("./fpAppearance");
  const { allPresets, selectPreset } = await import("./layerPresets");
  const universe = ["F.Cu", "B.Cu", "F.SilkS", "B.SilkS", "F.Fab", "Edge.Cuts"];
  const st = { layerVisible: {}, gridVisible: true, activeLayer: "F.SilkS" };
  const presets = allPresets(["F.Cu", "B.Cu"], [], ["F.SilkS", "B.SilkS", "F.Fab", "Edge.Cuts"]);
  assert.equal(fpMatchingPreset(presets, fpPresetView(universe, st)), "All Layers");
  const front = selectPreset(fpPresetView(universe, st), presets.find((p) => p.name === "Front Layers")!, presets);
  const out = fpApplyPresetView(universe, {}, front);
  assert.equal(out.visible["B.Cu"] ?? out.visible["b_cu"], false);
  assert.equal(fpMatchingPreset(presets, fpPresetView(universe, { ...st, layerVisible: out.visible })), "Front Layers");
  // the filter's rows: a rule area is Rule Areas, a barcode Other items, a locked pad clicked alone passes
  assert.equal(fpPassesFilter({ ...DEFAULT_FP_SELECTION_FILTER, keepouts: false }, "zone", false, true), false);
  assert.equal(fpPassesFilter({ ...DEFAULT_FP_SELECTION_FILTER, keepouts: false }, "zone", false, false), true);
  assert.equal(fpPassesFilter({ ...DEFAULT_FP_SELECTION_FILTER, otherItems: false }, "barcode", false), false);
  assert.equal(fpPassesFilter(DEFAULT_FP_SELECTION_FILTER, "pad", true), true);
  assert.equal(fpPassesFilter(DEFAULT_FP_SELECTION_FILTER, "pad", true, false, true), false);
});
