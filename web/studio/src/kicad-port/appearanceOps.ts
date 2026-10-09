// Every edit the Appearance panel makes, as one pure function: `reduceView( slice, op, ctx )` returns the slice after the op. The store's `APPEARANCE`
// action runs it over the fields of `StudioState` it touches (`ViewSlice`), the panel and the scripted test hook (`studio.Appearance.op`) dispatch the
// same ops, and the unit tests call it directly. Ported from the handlers of pcbnew/widgets/appearance_controls.cpp (each op names its handler).
//
// The layer visibility record is keyed as the studio keys it (`layerStateKey`); presets speak KiCad layer names, so the two are converted here.

import { OBJECT_IDS, objectOn, withObjectKeys, withObjectVisible, type AppearanceState, type ContrastMode, type NetColorMode, type ObjectId, type OpacityKey, type RatsnestDisplay } from "./appearance";
import { layerNameOfKey, layerStateKey, matchingPreset, menuPreset, panelLayers, selectPreset, hideAllButActive, layerGroupOp, snapshotPreset, withoutPreset, withoutViewport, withPreset, withViewport, allPresets, type LayerGroupOp, type MenuPresetKind, type PresetView } from "./layerPresets";
import { setNetVisible, showAllNets, hideOtherNets, withNetColor, showNetclass, showAllNetclasses, hideOtherNetclasses, type NetsContext } from "./appearanceNets";

/** The fields of the studio state the Appearance panel edits (`StudioState`'s own names, with the two nested ones lifted out of `bcx`). */
export interface ViewSlice {
  appearance: AppearanceState;
  /** Layer visibility by state key, plus the `obj:<id>` keys (`withObjectKeys`). */
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;
  /** The active layer's state key. */
  activeLayer: string | null;
  highContrast: boolean;
  showRatsnest: boolean;
  gridVisible: boolean;
  /** `bcx.boardFlipped`. */
  boardFlipped: boolean;
  /** `bcx.ratsnestMode`. */
  ratsnestMode: "all" | "visible";
  /** `bcx.hiddenRatsnestNets`. */
  hiddenNets: string[];
}

export type AppearanceOp =
  | { op: "object"; id: ObjectId; visible: boolean }
  | { op: "opacity"; key: OpacityKey; value: number }
  | { op: "contrast"; mode: ContrastMode }
  | { op: "net_color_mode"; mode: NetColorMode }
  | { op: "ratsnest_display"; mode: RatsnestDisplay }
  | { op: "net_color"; net: string; color: string | null }
  | { op: "net_visible"; net: string; visible: boolean }
  | { op: "show_all_nets" }
  | { op: "hide_other_nets"; net: string }
  | { op: "netclass_color"; name: string; color: string | null }
  | { op: "netclass_visible"; name: string; visible: boolean }
  | { op: "show_all_netclasses" }
  | { op: "hide_other_netclasses"; name: string }
  | { op: "layer"; key: string; visible: boolean }
  | { op: "layer_opacity"; key: string; value: number }
  | { op: "layer_group"; group: LayerGroupOp }
  | { op: "menu_preset"; kind: MenuPresetKind }
  | { op: "hide_all_but_active" }
  | { op: "select_preset"; name: string }
  | { op: "save_preset"; name: string }
  | { op: "delete_preset"; name: string }
  | { op: "flip"; flipped: boolean }
  | { op: "save_viewport"; name: string; rect: { x: number; y: number; w: number; h: number } }
  | { op: "delete_viewport"; name: string };

const clamp01 = (v: number): number => Math.max(0, Math.min(1, Number.isFinite(v) ? v : 0));

/** The slice's objects, as the preset code sees them: the 22, with the ratsnest and the grid from their own switches. */
export function objectsOf(s: ViewSlice): ObjectId[] {
  return OBJECT_IDS.filter((id) => (id === "ratsnest" ? s.showRatsnest : id === "grid" ? s.gridVisible : s.appearance.visible[id]));
}

function viewOf(s: ViewSlice, ctx: Pick<NetsContext, "copper">): PresetView {
  const universe = panelLayers(ctx.copper);
  return {
    universe,
    visibleLayers: new Set(universe.filter((l) => s.layerVisible[layerStateKey(l)] !== false)),
    objects: objectsOf(s),
    activeLayer: s.activeLayer === null ? null : layerNameOfKey(s.activeLayer),
    flipBoard: s.boardFlipped,
    activePreset: s.appearance.activePreset,
    lastBuiltinObjects: s.appearance.lastBuiltinObjects,
    boardLayers: new Set(universe),
  };
}

/** Writes a preset view back into the slice. */
function fromView(s: ViewSlice, v: PresetView): ViewSlice {
  const layerVisible = { ...s.layerVisible };
  for (const l of v.universe) layerVisible[layerStateKey(l)] = v.visibleLayers.has(l);
  const visible = { ...s.appearance.visible };
  for (const id of OBJECT_IDS) if (id !== "ratsnest" && id !== "grid") visible[id] = v.objects.includes(id);
  const appearance: AppearanceState = { ...s.appearance, visible, activePreset: v.activePreset, lastBuiltinObjects: v.lastBuiltinObjects ? [...v.lastBuiltinObjects] : null };
  return {
    ...s,
    appearance,
    layerVisible: withObjectKeys(layerVisible, appearance),
    activeLayer: v.activeLayer === null ? null : layerStateKey(v.activeLayer),
    gridVisible: v.objects.includes("grid"),
    boardFlipped: v.flipBoard,
  };
}

/** `syncLayerPresetSelection`: the list shows the preset that matches what is on screen, or the blank entry. */
function syncPreset(s: ViewSlice, ctx: Pick<NetsContext, "copper">): ViewSlice {
  const v = viewOf(s, ctx);
  const hit = matchingPreset(allPresets(ctx.copper, s.appearance.presets), v);
  const name = hit ? hit.name : "";
  return name === s.appearance.activePreset ? s : { ...s, appearance: { ...s.appearance, activePreset: name } };
}

function setAppearance(s: ViewSlice, patch: Partial<AppearanceState>): ViewSlice {
  const appearance = { ...s.appearance, ...patch };
  return { ...s, appearance, layerVisible: withObjectKeys(s.layerVisible, appearance) };
}

export function reduceView(s: ViewSlice, op: AppearanceOp, ctx: NetsContext): ViewSlice {
  switch (op.op) {
    case "object": {
      // `onObjectVisibilityChanged`: the ratsnest and the grid have switches of their own; the rest live in the appearance.
      if (op.id === "ratsnest") return syncPreset({ ...s, showRatsnest: op.visible }, ctx);
      if (op.id === "grid") return syncPreset({ ...s, gridVisible: op.visible }, ctx);
      return syncPreset(setAppearance(s, { visible: withObjectVisible(s.appearance.visible, op.id, op.visible) }), ctx);
    }
    case "opacity": {
      const value = clamp01(op.value);
      return setAppearance(s, { opacity: { ...s.appearance.opacity, [op.key]: value } });
    }
    case "contrast":
      // `m_ContrastModeDisplay`; `StudioState.highContrast` is "not normal".
      return { ...setAppearance(s, { contrastHidden: op.mode === "hidden" }), highContrast: op.mode !== "normal" };
    case "net_color_mode":
      return setAppearance(s, { netColorMode: op.mode });
    case "ratsnest_display": {
      // `onRatsnestMode`: "None" is the global ratsnest off; All and Visible layers turn it on and choose the mode.
      if (op.mode === "none") return syncPreset({ ...s, showRatsnest: false }, ctx);
      return syncPreset({ ...s, showRatsnest: true, ratsnestMode: op.mode }, ctx);
    }
    case "net_color":
      return setAppearance(s, { netColors: withNetColor(s.appearance.netColors, op.net, op.color) });
    case "net_visible":
      return { ...s, hiddenNets: setNetVisible(s.hiddenNets, op.net, op.visible) };
    case "show_all_nets":
      return { ...s, hiddenNets: showAllNets(s.hiddenNets, ctx.nets) };
    case "hide_other_nets":
      return { ...s, hiddenNets: hideOtherNets(s.hiddenNets, ctx.nets, op.net) };
    case "netclass_color": {
      const colors = { ...s.appearance.netclassColors };
      if (op.color === null) delete colors[op.name];
      else colors[op.name] = op.color;
      return setAppearance(s, { netclassColors: colors });
    }
    case "netclass_visible":
    case "show_all_netclasses":
    case "hide_other_netclasses": {
      const cur = { hiddenNets: s.hiddenNets, hiddenNetclasses: s.appearance.hiddenNetclasses };
      const next = op.op === "netclass_visible" ? showNetclass(cur, ctx, op.name, op.visible) : op.op === "show_all_netclasses" ? showAllNetclasses(cur, ctx) : hideOtherNetclasses(cur, ctx, op.name);
      return { ...setAppearance(s, { hiddenNetclasses: next.hiddenNetclasses }), hiddenNets: next.hiddenNets };
    }
    case "layer": {
      // `onLayerVisibilityToggled`
      const layerVisible = { ...s.layerVisible, [op.key]: op.visible };
      return syncPreset({ ...s, layerVisible }, ctx);
    }
    case "layer_opacity":
      return { ...s, layerOpacity: { ...s.layerOpacity, [op.key]: clamp01(op.value) } };
    case "layer_group":
      return syncPreset(fromView(s, layerGroupOp(viewOf(s, ctx), op.group)), ctx);
    case "menu_preset":
      // The temporary preset has no name, so the list goes blank -- unless what it left on screen happens to match one (`syncLayerPresetSelection`).
      return syncPreset(fromView(s, menuPreset(viewOf(s, ctx), op.kind, ctx.copper)), ctx);
    case "hide_all_but_active":
      return syncPreset(fromView(s, hideAllButActive(viewOf(s, ctx))), ctx);
    case "select_preset": {
      const all = allPresets(ctx.copper, s.appearance.presets);
      const preset = all.find((p) => p.name === op.name);
      if (!preset) return s;
      return fromView(s, selectPreset(viewOf(s, ctx), preset, all));
    }
    case "save_preset": {
      // "Save preset...": a new preset (or one of the user's replaced -- the caller asks first) made of what is on screen, which becomes the current one.
      const name = op.name.trim();
      if (name === "" || allPresets(ctx.copper, []).some((p) => p.name === name)) return s;
      const preset = snapshotPreset(name, viewOf(s, ctx));
      return setAppearance(s, { presets: withPreset(s.appearance.presets, preset), activePreset: name });
    }
    case "delete_preset": {
      // `m_currentPreset = nullptr`: the list goes blank whichever preset was deleted.
      return setAppearance(s, { presets: withoutPreset(s.appearance.presets, op.name), activePreset: "" });
    }
    case "flip":
      return syncPreset({ ...s, boardFlipped: op.flipped }, ctx);
    case "save_viewport": {
      const name = op.name.trim();
      return name === "" ? s : setAppearance(s, { viewports: withViewport(s.appearance.viewports, name, op.rect) });
    }
    case "delete_viewport":
      return setAppearance(s, { viewports: withoutViewport(s.appearance.viewports, op.name) });
  }
}

/** The state of an object's checkbox in the panel (the switch itself; an opacity of 0 does not uncheck it). */
export function objectChecked(s: Pick<ViewSlice, "appearance" | "showRatsnest" | "gridVisible">, id: ObjectId): boolean {
  return id === "ratsnest" ? s.showRatsnest : id === "grid" ? s.gridVisible : s.appearance.visible[id];
}

/** Whether an object is on as far as drawing and picking go: switched on and not at opacity 0 (the answer the painter and the pickers get). */
export function objectIsOn(s: Pick<ViewSlice, "layerVisible" | "showRatsnest" | "gridVisible">, id: ObjectId): boolean {
  return id === "ratsnest" ? s.showRatsnest : id === "grid" ? s.gridVisible : objectOn(s.layerVisible, id);
}
