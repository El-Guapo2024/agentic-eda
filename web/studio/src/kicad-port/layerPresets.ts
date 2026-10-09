// Layer presets and viewports of the Appearance panel (pcbnew/widgets/appearance_controls.cpp), as pure functions on KiCad layer names
// ("F.Cu", "In1.Cu", "F.SilkS", "Edge.Cuts"): the built-in presets (`APPEARANCE_CONTROLS::presetFront` ...), what picking one does
// (`onLayerPresetChanged`, `doApplyLayerPreset`), what the layer list's right-click menu does (`OnLayerContextMenu`), which preset the current
// visibility matches (`syncLayerPresetSelection`), saving and deleting the user's own, and the viewports (`VIEWPORT`, `VIEW::SetViewport`).
//
// The studio keeps layer visibility under its own state keys (copper layers by name, nine technical layers by the painter's bucket names --
// `layerStateKey`); everything here speaks KiCad names and the boundary converts.
//
// Ported from:
//   appearance_controls.cpp   presetNoLayers .. presetBackAssembly, loadDefaultLayerPresets, rebuildLayerPresetsWidget (the order of the list),
//                             syncLayerPresetSelection, onLayerPresetChanged, doApplyLayerPreset, OnLayerContextMenu, onViewportChanged
//   lset.cpp                  FrontMask / BackMask / FrontAssembly / BackAssembly / InternalCuMask / AllCuMask
//   layer_ids.h               PCB_LAYER_ID's numbering (the "first layer of a preset" is the lowest id)
//   view.cpp                  VIEW::SetViewport / GetViewport
//
// Not ported: the Ctrl+Tab / Alt+Tab quick switcher (`EDA_VIEW_SWITCHER`): the browser keeps those keys for itself.

import { DEFAULT_OBJECT_VISIBILITY, OBJECT_IDS, type LayerPreset, type ObjectId, type Viewport } from "./appearance";

// ------------------------------------------------------------------------------------------------------------------------------------ layers

/** The technical layers the Layers tab lists after the copper ones, in `LSET::TechAndUserUIOrder` (the rows of `non_cu_seq`, without the user layers). */
export const TECH_LAYER_NAMES: readonly string[] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

/** The painter's bucket names (components/canvas/layers.ts) for the technical layers it paints in a layer of their own. */
const BUCKET_OF: Readonly<Record<string, string>> = {
  "F.SilkS": "f_silks",
  "B.SilkS": "b_silks",
  "F.Mask": "f_mask",
  "B.Mask": "b_mask",
  "F.CrtYd": "f_courtyard",
  "B.CrtYd": "b_courtyard",
  "F.Fab": "f_fab",
  "B.Fab": "b_fab",
  "Edge.Cuts": "board_edge",
};
const NAME_OF_BUCKET: Readonly<Record<string, string>> = Object.fromEntries(Object.entries(BUCKET_OF).map(([name, key]) => [key, name]));

/**
 * The key `StudioState.layerVisible` / `layerOpacity` / `activeLayer` use for a layer: a copper layer is its name, the nine technical layers the
 * painter buckets are their bucket name, any other is its colour key (the name with the dots turned to underscores: "Dwgs_User").
 */
export function layerStateKey(name: string): string {
  return BUCKET_OF[name] ?? (/\.Cu$/.test(name) ? name : name.replace(/\./g, "_"));
}

/** The layer name a state key stands for (the inverse of `layerStateKey` for the keys it produces). */
export function layerNameOfKey(key: string): string {
  if (/\.Cu$/.test(key)) return key;
  const bucket = NAME_OF_BUCKET[key];
  if (bucket) return bucket;
  const known = TECH_LAYER_NAMES.find((n) => n.replace(/\./g, "_") === key);
  return known ?? key;
}

/** True unless the layer is switched off (a layer nobody has set is on). Accepts the KiCad name of any layer an item can be on. */
export function layerIsVisible(layerVisible: Readonly<Record<string, boolean>>, name: string): boolean {
  return layerVisible[layerStateKey(name)] !== false && layerVisible[name] !== false;
}

/** Every layer the panel lists: the board's copper layers (front to back, as `board.layers` has them), then the technical layers. */
export function panelLayers(copper: readonly string[]): string[] {
  return [...copper, ...TECH_LAYER_NAMES];
}

/** `PCB_LAYER_ID`'s numbers for the layers a preset can name (F.Cu 0, F.Mask 1, B.Cu 2, ..., In1.Cu 4, F.SilkS 5 ...); the order "first layer of a set" means. */
const FIXED_LAYER_ID: Readonly<Record<string, number>> = {
  "F.Cu": 0,
  "F.Mask": 1,
  "B.Cu": 2,
  "B.Mask": 3,
  "F.SilkS": 5,
  "B.SilkS": 7,
  "F.Adhes": 9,
  "B.Adhes": 11,
  "F.Paste": 13,
  "B.Paste": 15,
  "Dwgs.User": 17,
  "Cmts.User": 19,
  "Eco1.User": 21,
  "Eco2.User": 23,
  "Edge.Cuts": 25,
  Margin: 27,
  "B.CrtYd": 29,
  "F.CrtYd": 31,
  "B.Fab": 33,
  "F.Fab": 35,
};

export function layerId(name: string): number {
  const fixed = FIXED_LAYER_ID[name];
  if (fixed !== undefined) return fixed;
  const inner = /^In(\d+)\.Cu$/.exec(name);
  if (inner) return 2 + 2 * Number(inner[1]);
  return 1000;
}

/** The set's first layer in `PCB_LAYER_ID` order (`*aPreset.layers.Seq().begin()`), or null for an empty set. */
export function firstLayer(names: Iterable<string>): string | null {
  let best: string | null = null;
  for (const n of names) if (best === null || layerId(n) < layerId(best)) best = n;
  return best;
}

export const isCopper = (name: string): boolean => /\.Cu$/.test(name);

// ------------------------------------------------------------------------------------------------------------------------------------ presets

const FRONT_TECH = ["F.SilkS", "F.Mask", "F.Adhes", "F.Paste", "F.CrtYd", "F.Fab"];
const BACK_TECH = ["B.SilkS", "B.Mask", "B.Adhes", "B.Paste", "B.CrtYd", "B.Fab"];

/** `GAL_SET::DefaultVisible` as a list: what the objects are set to by a preset that does not say. */
export const DEFAULT_OBJECTS: readonly ObjectId[] = OBJECT_IDS.filter((id) => DEFAULT_OBJECT_VISIBILITY[id]);

/**
 * The eight built-in presets (`loadDefaultLayerPresets`, all read-only), in the order the list shows them: `m_layerPresets` is a `std::map` keyed by name,
 * so alphabetical. `copper` is the board's copper layers (the masks `AllCuMask` and `InternalCuMask` cover 32 layers; a board has these).
 */
export function builtinPresets(copper: readonly string[]): LayerPreset[] {
  const inner = copper.filter((l) => /^In\d+\.Cu$/.test(l));
  const all = panelLayers(copper);
  const mk = (name: string, layers: string[], flipBoard: boolean, activeLayer: string | null = null): LayerPreset => ({ name, layers, renderLayers: [...DEFAULT_OBJECTS], flipBoard, activeLayer, readOnly: true });
  const list = [
    mk("No Layers", [], false),
    mk("All Layers", all, false),
    mk("All Copper Layers", [...copper, "Edge.Cuts"], false),
    mk("Inner Copper Layers", [...inner, "Edge.Cuts"], false),
    mk("Front Layers", ["F.Cu", ...FRONT_TECH, "Edge.Cuts"], false),
    mk("Front Assembly View", ["F.SilkS", "F.Mask", "F.Fab", "F.CrtYd", "Edge.Cuts"], false, "F.SilkS"),
    mk("Back Layers", ["B.Cu", ...BACK_TECH, "Edge.Cuts"], true),
    mk("Back Assembly View", ["B.SilkS", "B.Mask", "B.Fab", "B.CrtYd", "Edge.Cuts"], true, "B.SilkS"),
  ];
  return list.sort((a, b) => compareNames(a.name, b.name));
}

/** `wxString::operator<`: by character code, so upper case sorts before lower case. */
export function compareNames(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Every preset by name, the built-in ones and the user's, as the `std::map` iterates them (a user preset cannot take a built-in name). */
export function allPresets(copper: readonly string[], user: readonly LayerPreset[]): LayerPreset[] {
  const builtin = builtinPresets(copper);
  const names = new Set(builtin.map((p) => p.name));
  const mine = user.filter((p) => !names.has(p.name) && !p.readOnly);
  return [...builtin, ...mine].sort((a, b) => compareNames(a.name, b.name));
}

/** The entries of the preset list in `rebuildLayerPresetsWidget`: the built-in ones, a separator and the user's when there are any, a separator, then the two commands. */
export type PresetListEntry = { kind: "preset"; preset: LayerPreset } | { kind: "separator" } | { kind: "save" } | { kind: "delete" };
export function presetList(copper: readonly string[], user: readonly LayerPreset[]): PresetListEntry[] {
  const all = allPresets(copper, user);
  const out: PresetListEntry[] = all.filter((p) => p.readOnly).map((preset) => ({ kind: "preset", preset }));
  const mine = all.filter((p) => !p.readOnly);
  if (mine.length > 0) {
    out.push({ kind: "separator" });
    for (const preset of mine) out.push({ kind: "preset", preset });
  }
  out.push({ kind: "separator" }, { kind: "save" }, { kind: "delete" });
  return out;
}

/** What a preset acts on: the layers on, the objects on (all 22, with the ratsnest and the grid), the active layer, the flip. A layer is a KiCad name. */
export interface PresetView {
  /** Every layer the panel lists, KiCad names. */
  universe: readonly string[];
  visibleLayers: ReadonlySet<string>;
  /** The objects that are on, with the ratsnest and the grid. */
  objects: readonly ObjectId[];
  activeLayer: string | null;
  flipBoard: boolean;
  /** The preset the list shows ("" = the blank entry). */
  activePreset: string;
  /** `m_lastBuiltinPreset.renderLayers`; null until a built-in preset was left. */
  lastBuiltinObjects: readonly ObjectId[] | null;
  /** The board's layers: a layer a preset names that the board does not have cannot become active (`boardLayers.Contains`). */
  boardLayers: ReadonlySet<string>;
}

/**
 * `syncLayerPresetSelection`: the first preset (in the list's order) whose layers, objects and flip are exactly what is on screen, else null
 * (the blank entry). Layers outside the panel's universe do not count either way.
 */
export function matchingPreset(presets: readonly LayerPreset[], v: Pick<PresetView, "universe" | "visibleLayers" | "objects" | "flipBoard">): LayerPreset | null {
  const universe = new Set(v.universe);
  const sameLayers = (p: LayerPreset): boolean => {
    const have = new Set(p.layers.filter((l) => universe.has(l)));
    const want = [...v.visibleLayers].filter((l) => universe.has(l));
    return have.size === want.length && want.every((l) => have.has(l));
  };
  const sameObjects = (p: LayerPreset): boolean => {
    const have = new Set(p.renderLayers);
    return have.size === v.objects.length && v.objects.every((o) => have.has(o));
  };
  return presets.find((p) => sameLayers(p) && sameObjects(p) && p.flipBoard === v.flipBoard) ?? null;
}

/**
 * `doApplyLayerPreset`: the preset's layers become the visible ones, its objects the visible objects (the ratsnest is the one object a preset leaves
 * alone -- "ratsnest visibility is controlled by the ratsnest option, and not by the preset"), and the board is flipped as the preset says. The
 * active layer is the preset's own, else it stays unless the preset hides it, when it becomes the preset's first layer; a layer the board does
 * not have is not made active.
 */
function applyTo(v: PresetView, layers: readonly string[], objects: readonly ObjectId[], flipBoard: boolean, presetActive: string | null): PresetView {
  const visibleLayers = new Set(layers.filter((l) => v.universe.includes(l)));
  const keepRatsnest = v.objects.includes("ratsnest");
  const nextObjects = OBJECT_IDS.filter((id) => (id === "ratsnest" ? keepRatsnest : objects.includes(id)));
  let activeLayer = v.activeLayer;
  let wanted: string | null = null;
  if (presetActive) wanted = presetActive;
  else if (layers.length > 0 && !(v.activeLayer !== null && layers.includes(v.activeLayer))) wanted = firstLayer(layers);
  if (wanted !== null && v.boardLayers.has(wanted)) activeLayer = wanted;
  return { ...v, visibleLayers, objects: nextObjects, activeLayer, flipBoard };
}

/**
 * `onLayerPresetChanged` for a preset picked in the list. A built-in preset does not say which objects are on: it restores the ones that were on when a
 * built-in preset (or none) was last left; leaving one for a user preset stores them. `presets` is every preset (for the current-preset test).
 */
export function selectPreset(v: PresetView, preset: LayerPreset, presets: readonly LayerPreset[]): PresetView {
  const current = presets.find((p) => p.name === v.activePreset) ?? null;
  const lastBuiltinObjects = !current || current.readOnly ? [...v.objects] : v.lastBuiltinObjects;
  const objects = preset.readOnly ? (lastBuiltinObjects ?? DEFAULT_OBJECTS) : preset.renderLayers;
  const applied = applyTo(v, preset.layers, objects, preset.flipBoard, preset.activeLayer);
  return { ...applied, activePreset: preset.name, lastBuiltinObjects };
}

/** The entries of the layer list's right-click menu that are presets (`ID_PRESET_*`), by the layers they show. */
export type MenuPresetKind = "all_layers" | "no_layers" | "front" | "front_assembly" | "inner_copper" | "back" | "back_assembly";

/**
 * `OnLayerContextMenu`'s presets: "Show All Layers", "Show Only Front Layers" ... apply a built-in preset's layers but keep the objects that are on and
 * the flip as they are, and leave the list on its blank entry (the temporary preset has no name).
 */
export function menuPreset(v: PresetView, kind: MenuPresetKind, copper: readonly string[]): PresetView {
  const find = (name: string) => builtinPresets(copper).find((p) => p.name === name)!;
  const p = find({ all_layers: "All Layers", no_layers: "No Layers", front: "Front Layers", front_assembly: "Front Assembly View", inner_copper: "Inner Copper Layers", back: "Back Layers", back_assembly: "Back Assembly View" }[kind]);
  // Only the layers are copied from the built-in preset (`preset.layers = presetFront.layers`): its active layer is not, so the assembly entries do not pick F.SilkS.
  const applied = applyTo(v, p.layers, v.objects, v.flipBoard, null);
  return { ...applied, activePreset: "" };
}

/** "Hide All Layers But Active" (`ID_HIDE_ALL_BUT_ACTIVE`): only the active layer stays on. */
export function hideAllButActive(v: PresetView): PresetView {
  const layers = v.activeLayer !== null ? [v.activeLayer] : [];
  return { ...applyTo(v, layers, v.objects, v.flipBoard, null), activePreset: "" };
}

export type LayerGroupOp = "show_copper" | "hide_copper" | "show_non_copper" | "hide_non_copper";

/**
 * The four copper / non-copper entries of the layer list's menu. Hiding copper or non-copper layers moves the active layer to the first layer
 * left on when it was one of those hidden (`SetActiveLayer( *visible.Seq().begin() )`).
 */
export function layerGroupOp(v: PresetView, op: LayerGroupOp): PresetView {
  const visible = new Set(v.visibleLayers);
  for (const l of v.universe) {
    if (op === "show_copper" && isCopper(l)) visible.add(l);
    else if (op === "hide_copper" && isCopper(l)) visible.delete(l);
    else if (op === "show_non_copper" && !isCopper(l)) visible.add(l);
    else if (op === "hide_non_copper" && !isCopper(l)) visible.delete(l);
  }
  let activeLayer = v.activeLayer;
  if ((op === "hide_copper" || op === "hide_non_copper") && (activeLayer === null || !visible.has(activeLayer)) && visible.size > 0) activeLayer = firstLayer(visible);
  return { ...v, visibleLayers: visible, activeLayer };
}

/** A preset made of what is on screen (`LAYER_PRESET( name, getVisibleLayers(), getVisibleObjects(), UNSELECTED_LAYER, flip )`). */
export function snapshotPreset(name: string, v: Pick<PresetView, "universe" | "visibleLayers" | "objects" | "flipBoard">): LayerPreset {
  return { name, layers: v.universe.filter((l) => v.visibleLayers.has(l)), renderLayers: [...v.objects], flipBoard: v.flipBoard, activeLayer: null, readOnly: false };
}

/** What saving under `name` does (`onLayerPresetChanged`, "Save preset..."): add a new preset, replace one of the user's after the person confirms, or refuse a built-in's name. */
export type SaveOutcome = { kind: "new" } | { kind: "overwrite" } | { kind: "refused"; message: string } | { kind: "empty" };
export function savePresetOutcome(name: string, existing: readonly LayerPreset[]): SaveOutcome {
  if (name.trim() === "") return { kind: "empty" };
  const hit = existing.find((p) => p.name === name);
  if (!hit) return { kind: "new" };
  if (hit.readOnly) return { kind: "refused", message: "Default presets cannot be modified.\nPlease use a different name." };
  return { kind: "overwrite" };
}

/** The user's presets after saving `preset` (replacing the one of that name). */
export function withPreset(user: readonly LayerPreset[], preset: LayerPreset): LayerPreset[] {
  return [...user.filter((p) => p.name !== preset.name), { ...preset, readOnly: false }].sort((a, b) => compareNames(a.name, b.name));
}

/** The user's presets without `name`: the built-in ones cannot be deleted. */
export function withoutPreset(user: readonly LayerPreset[], name: string): LayerPreset[] {
  return user.filter((p) => p.name !== name);
}

// ------------------------------------------------------------------------------------------------------------------------------------ viewports

/** `VIEW::GetViewport`: the part of the board the canvas shows, as a rectangle in micrometres. */
export function currentViewport(view: { scale: number; x: number; y: number }, widthPx: number, heightPx: number): { x: number; y: number; w: number; h: number } {
  return { x: -view.x / view.scale, y: -view.y / view.scale, w: widthPx / view.scale, h: heightPx / view.scale };
}

/**
 * `VIEW::SetViewport`: the view centred on the rectangle, zoomed so the whole of it fits (the larger of the two ratios decides). The result is the
 * studio's view transform (`screen = world * scale + (x, y)`); the caller clamps the scale to the zoom limits.
 */
export function viewForViewport(vp: { x: number; y: number; w: number; h: number }, widthPx: number, heightPx: number): { scale: number; x: number; y: number } {
  const scale = Math.min(widthPx / Math.max(vp.w, 1e-9), heightPx / Math.max(vp.h, 1e-9));
  const [cx, cy] = [vp.x + vp.w / 2, vp.y + vp.h / 2];
  return { scale, x: widthPx / 2 - cx * scale, y: heightPx / 2 - cy * scale };
}

/** The viewports with `name` saved over the rectangle (a new one, or the one of that name replaced); `std::map` order. */
export function withViewport(list: readonly Viewport[], name: string, rect: { x: number; y: number; w: number; h: number }): Viewport[] {
  return [...list.filter((v) => v.name !== name), { name, ...rect }].sort((a, b) => compareNames(a.name, b.name));
}

export function withoutViewport(list: readonly Viewport[], name: string): Viewport[] {
  return list.filter((v) => v.name !== name);
}
