// What the Appearance panel keeps for the project, and the file it keeps it in: `appearance.json` beside `design.json` (crates/cli/src/appearance_api.rs,
// `GET`/`POST /api/appearance`). It is the studio's counterpart of the two files KiCad splits this between, and never part of the design:
//
//   `local`    the project's *local* settings, `<project>.kicad_prl` (`PROJECT_LOCAL_SETTINGS`, common/project/project_local_settings.cpp): which layers and
//              objects are on, the opacities, the inactive-layer and net colour modes, the hidden nets and net classes, the active layer
//   `project`  what KiCad writes to the *project* file `<project>.kicad_pro` (`PROJECT_FILE`, `NET_SETTINGS`): net colours (`net_settings.net_colors`), net class
//              colours (`net_settings.classes[].pcb_color`), the saved layer presets (`board.layer_presets`) and viewports (`board.viewports`)
//
// The keys keep KiCad's names (`board.visible_items`, `board.opacity.*`, `board.hidden_nets` ...), so the derived KiCad project the engine writes
// (crates/kicad-engine) takes them over as they are. Differences, all of them because the studio's data is not KiCad's: layers are written by name where
// KiCad uses a hex mask or numeric ids, and viewports in micrometres where KiCad uses nanometres.
//
// Reading is forgiving, as KiCad's `SetIfPresent` is: a missing or wrongly typed entry leaves the setting as it was.

import { CONTRAST_MODE_NUMBER, DEFAULT_OBJECT_VISIBILITY, DEFAULT_OPACITY, NET_COLOR_MODE_NUMBER, OBJECT_IDS, contrastModeOf, isObjectId, netColorModeOf, specifiedColor, toCssColor, withObjectKeys, type ContrastMode, type LayerPreset, type Opacity, type Viewport } from "./appearance";
import { layerNameOfKey, layerStateKey, panelLayers } from "./layerPresets";
import type { NetsContext } from "./appearanceNets";
import { objectsOf, type ViewSlice } from "./appearanceOps";

export const APPEARANCE_FILE_VERSION = 1;

export interface PresetJson {
  name: string;
  activeLayer: string | null;
  flipBoard: boolean;
  layers: string[];
  renderLayers: string[];
}

export interface AppearanceFile {
  version: number;
  local: {
    /** The layers switched off, by KiCad name (KiCad stores the ones on, as a hex mask). */
    hidden_layers?: string[];
    /** Per layer, the opacity of the ones that are not fully opaque (the studio's own: KiCad has none per layer). */
    layer_opacity?: Record<string, number>;
    /** `board.visible_items`: the objects that are on, by `VISIBILITY_LAYER` name; `["none"]` when none is. */
    visible_items?: string[];
    active_layer?: string | null;
    /** `board.active_layer_preset`. */
    active_layer_preset?: string;
    /** `board.high_contrast_mode`: 0 normal, 1 dimmed, 2 hidden. */
    high_contrast_mode?: number;
    /** `board.net_color_mode`: 0 off, 1 ratsnest, 2 all. */
    net_color_mode?: number;
    /** `board.opacity.*`. */
    opacity?: Partial<Opacity>;
    /** `board.hidden_nets`, `board.hidden_netclasses`. */
    hidden_nets?: string[];
    hidden_netclasses?: string[];
    /** `m_RatsnestMode` (a setting of pcbnew itself in KiCad, kept with the project here). */
    ratsnest_mode?: "all" | "visible";
    /** `m_FlipBoardView`. */
    flip_board?: boolean;
  };
  project: {
    net_colors?: Record<string, string>;
    netclass_colors?: Record<string, string>;
    layer_presets?: PresetJson[];
    viewports?: Viewport[];
  };
}

export const EMPTY_APPEARANCE_FILE: AppearanceFile = { version: APPEARANCE_FILE_VERSION, local: {}, project: {} };

// ------------------------------------------------------------------------------------------------------------------------------------ writing

type FileCtx = Pick<NetsContext, "copper" | "nets">;

/** `SaveProjectLocalSettings` and the net / preset / viewport part of `SaveProject`. Nets the board no longer has are left out (once the board's nets are known). */
export function toAppearanceFile(s: ViewSlice, ctx: FileCtx): AppearanceFile {
  const a = s.appearance;
  const universe = panelLayers(ctx.copper);
  const known = ctx.nets.length > 0 ? new Set(ctx.nets) : null;
  const keepNet = (n: string): boolean => known === null || known.has(n);
  const on = objectsOf(s);

  const layerOpacity: Record<string, number> = {};
  for (const [key, v] of Object.entries(s.layerOpacity)) if (v !== 1 && !key.startsWith("obj:")) layerOpacity[layerNameOfKey(key)] = v;

  const netColors: Record<string, string> = {};
  for (const [net, css] of Object.entries(a.netColors)) if (keepNet(net) && specifiedColor(css)) netColors[net] = css;
  const classColors: Record<string, string> = {};
  for (const [name, css] of Object.entries(a.netclassColors)) if (specifiedColor(css)) classColors[name] = css;

  return {
    version: APPEARANCE_FILE_VERSION,
    local: {
      hidden_layers: universe.filter((l) => s.layerVisible[layerStateKey(l)] === false),
      layer_opacity: layerOpacity,
      visible_items: on.length > 0 ? [...on] : ["none"],
      active_layer: s.activeLayer === null ? null : layerNameOfKey(s.activeLayer),
      active_layer_preset: a.activePreset,
      high_contrast_mode: CONTRAST_MODE_NUMBER[contrastOf(s)],
      net_color_mode: NET_COLOR_MODE_NUMBER[a.netColorMode],
      opacity: { ...a.opacity },
      hidden_nets: s.hiddenNets.filter(keepNet),
      hidden_netclasses: [...a.hiddenNetclasses],
      ratsnest_mode: s.ratsnestMode,
      flip_board: s.boardFlipped,
    },
    project: {
      net_colors: netColors,
      netclass_colors: classColors,
      layer_presets: a.presets.map(presetToJson),
      viewports: a.viewports.map((v) => ({ ...v })),
    },
  };
}

function contrastOf(s: ViewSlice): ContrastMode {
  return !s.highContrast ? "normal" : s.appearance.contrastHidden ? "hidden" : "dimmed";
}

export function presetToJson(p: LayerPreset): PresetJson {
  return { name: p.name, activeLayer: p.activeLayer, flipBoard: p.flipBoard, layers: [...p.layers], renderLayers: [...p.renderLayers] };
}

// ------------------------------------------------------------------------------------------------------------------------------------ reading

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const strings = (v: unknown): string[] | null => (Array.isArray(v) && v.every((x) => typeof x === "string") ? (v as string[]) : null);

/** `PARAM_LAYER_PRESET::jsonToPresets`: an entry without a name is skipped, a field of the wrong type keeps its default. */
export function presetFromJson(j: unknown): LayerPreset | null {
  if (!isRecord(j) || typeof j.name !== "string" || j.name === "") return null;
  const layers = strings(j.layers);
  const render = strings(j.renderLayers);
  return {
    name: j.name,
    layers: layers ?? [],
    renderLayers: render ? render.filter(isObjectId) : OBJECT_IDS.filter((id) => DEFAULT_OBJECT_VISIBILITY[id]),
    flipBoard: typeof j.flipBoard === "boolean" ? j.flipBoard : false,
    activeLayer: typeof j.activeLayer === "string" ? j.activeLayer : null,
    readOnly: false,
  };
}

function viewportFromJson(j: unknown): Viewport | null {
  if (!isRecord(j) || typeof j.name !== "string" || j.name === "") return null;
  const n = (k: string): number => (typeof j[k] === "number" && Number.isFinite(j[k] as number) ? (j[k] as number) : 0);
  return { name: j.name, x: n("x"), y: n("y"), w: n("w"), h: n("h") };
}

function colorMap(j: unknown): Record<string, string> | null {
  if (!isRecord(j)) return null;
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(j)) if (typeof v === "string" && specifiedColor(v)) out[k] = toCssColor(specifiedColor(v)!);
  return out;
}

/** What `LoadProjectSettings` does with a file: lays its settings over the slice, leaving alone whatever the file does not say. */
export function applyAppearanceFile(s: ViewSlice, file: unknown, ctx: Pick<NetsContext, "copper">): ViewSlice {
  if (!isRecord(file)) return s;
  const local = isRecord(file.local) ? file.local : {};
  const project = isRecord(file.project) ? file.project : {};
  let a = { ...s.appearance };
  let layerVisible = { ...s.layerVisible };
  const layerOpacity = { ...s.layerOpacity };
  let next = { ...s };

  const hiddenLayers = strings(local.hidden_layers);
  if (hiddenLayers) {
    for (const l of panelLayers(ctx.copper)) layerVisible[layerStateKey(l)] = true;
    for (const l of hiddenLayers) layerVisible[layerStateKey(l)] = false;
  }
  if (isRecord(local.layer_opacity)) for (const [name, v] of Object.entries(local.layer_opacity)) if (typeof v === "number" && v >= 0 && v <= 1) layerOpacity[layerStateKey(name)] = v;

  // `board.visible_items`: nothing usable means everything on; "none" is the one way to say nothing is.
  // (An entry that is not a string is skipped, as the `catch( ... )` around each entry does.)
  const items = Array.isArray(local.visible_items) ? local.visible_items.filter((x): x is string => typeof x === "string") : null;
  if (items) {
    const named = items.filter(isObjectId);
    const none = items.includes("none");
    if (named.length === 0 && !none) {
      // "Restore corrupted state": all on.
      a.visible = Object.fromEntries(OBJECT_IDS.map((id) => [id, true])) as typeof a.visible;
    } else {
      const set = new Set(named);
      a.visible = Object.fromEntries(OBJECT_IDS.map((id) => [id, set.has(id)])) as typeof a.visible;
    }
    next.showRatsnest = a.visible.ratsnest;
    next.gridVisible = a.visible.grid;
  }
  if (typeof local.active_layer === "string") next.activeLayer = layerStateKey(local.active_layer);
  else if (local.active_layer === null) next.activeLayer = null;
  if (typeof local.active_layer_preset === "string") a.activePreset = local.active_layer_preset;
  if (typeof local.high_contrast_mode === "number") {
    const mode = contrastModeOf(local.high_contrast_mode);
    next.highContrast = mode !== "normal";
    a.contrastHidden = mode === "hidden";
  }
  if (typeof local.net_color_mode === "number") a.netColorMode = netColorModeOf(local.net_color_mode);
  if (isRecord(local.opacity)) {
    const op = { ...a.opacity };
    for (const k of Object.keys(DEFAULT_OPACITY) as (keyof Opacity)[]) {
      const v = (local.opacity as Record<string, unknown>)[k];
      if (typeof v === "number" && v >= 0 && v <= 1) op[k] = v;
    }
    a.opacity = op;
  }
  const hiddenNets = strings(local.hidden_nets);
  if (hiddenNets) next.hiddenNets = [...hiddenNets];
  const hiddenClasses = strings(local.hidden_netclasses);
  if (hiddenClasses) a.hiddenNetclasses = [...hiddenClasses];
  if (local.ratsnest_mode === "all" || local.ratsnest_mode === "visible") next.ratsnestMode = local.ratsnest_mode;
  if (typeof local.flip_board === "boolean") next.boardFlipped = local.flip_board;

  const nc = colorMap(project.net_colors);
  if (nc) a.netColors = nc;
  const cc = colorMap(project.netclass_colors);
  if (cc) a.netclassColors = cc;
  if (Array.isArray(project.layer_presets)) a.presets = project.layer_presets.map(presetFromJson).filter((p): p is LayerPreset => p !== null);
  if (Array.isArray(project.viewports)) a.viewports = project.viewports.map(viewportFromJson).filter((v): v is Viewport => v !== null);

  layerVisible = withObjectKeys(layerVisible, a);
  next = { ...next, appearance: a, layerVisible, layerOpacity };
  return next;
}
