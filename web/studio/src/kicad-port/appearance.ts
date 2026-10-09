// The Appearance panel's model (pcbnew/widgets/appearance_controls.cpp, pcbnew/pcb_painter.cpp, common/settings/layer_settings_utils.cpp) as pure
// functions: which objects the Objects tab lists and how they depend on each other, their opacities, the three inactive-layer and net-colour modes,
// the colours KiCad writes as CSS strings (`COLOR4D::ToCSSString`) and the net colour a copper item is painted with (`PCB_RENDER_SETTINGS::GetColor`).
// The panel (components/panels/AppearancePanel.tsx) renders it, state/store.tsx holds the `AppearanceState`, components/canvas/painter.ts honours it.
//
// Ported from:
//   appearance_controls.cpp    s_objectSettings (the Objects rows), onObjectVisibilityChanged (Footprint Text drags References and Values along),
//                              onNetColorMode, UpdateDisplayOptions
//   layer_settings_utils.cpp   VISIBILITY_LAYER / UserVisbilityLayers (the 22 objects with visibility, and the names a project file stores them by)
//   lset.cpp                   GAL_SET::DefaultVisible (what is on in a new project)
//   project_local_settings.cpp the defaults of the opacities (zones 0.6, images 0.6, the others 1), HIGH_CONTRAST_MODE and NET_COLOR_MODE
//   pcb_painter.cpp            GetColor's net colour branch: a per-net colour, else the net class colour, else the layer's own, only for copper and
//                              only in the "All" net colour mode
//   color4d.cpp                ToCSSString / SetFromWxString (the colour text of a project file)
//
// What the panel does not list, with the reason: Images (`LAYER_DRAW_BITMAPS`) and Points (`LAYER_POINTS`) -- the board model has no reference images
// and no snap points, so a toggle would switch nothing. Both stay in the model (`OBJECT_IDS`) so a preset or a project file that names them keeps them.

import { brighten, darken, HIGHLIGHT_FACTOR } from "./netHighlight";

// ------------------------------------------------------------------------------------------------------------------------------------ colours

/** A colour as `COLOR4D` holds it: red, green, blue as 0..255 integers and alpha 0..1. */
export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

const clamp255 = (v: number): number => Math.max(0, Math.min(255, Math.round(v)));
const clamp01 = (v: number): number => Math.max(0, Math.min(1, v));

/**
 * `wxColour::Set` for the forms a project file holds: `rgb(r, g, b)`, `rgba(r, g, b, a)` (alpha 0..1), `#rgb`, `#rrggbb` and `#rrggbbaa`.
 * Null for anything else (a colour name, a typo): `COLOR4D::SetFromWxString` then leaves the colour as it was.
 */
export function parseCssColor(text: string): Rgba | null {
  const s = text.trim().toLowerCase();
  const fn = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(s);
  if (fn) {
    const [r, g, b] = [Number(fn[1]), Number(fn[2]), Number(fn[3])];
    const a = fn[4] === undefined ? 1 : Number(fn[4]);
    if (![r, g, b, a].every(Number.isFinite)) return null;
    return { r: clamp255(r), g: clamp255(g), b: clamp255(b), a: clamp01(a) };
  }
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(s);
  if (hex) {
    let h = hex[1]!;
    if (h.length === 3) h = [...h].map((c) => c + c).join("");
    const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
    return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) / 255 : 1 };
  }
  return null;
}

/** `COLOR4D::ToCSSString`: `rgb(r, g, b)` when opaque, else `rgba(r, g, b, a)` with the alpha to three decimals. */
export function toCssColor(c: Rgba): string {
  const [r, g, b] = [clamp255(c.r), clamp255(c.g), clamp255(c.b)];
  const alpha = Math.round(clamp01(c.a) * 255);
  return alpha === 255 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${(alpha / 255).toFixed(3)})`;
}

/** `COLOR4D::UNSPECIFIED` is (0, 0, 0, 0): "no colour of its own", which is how a swatch is cleared (`rgba(0,0,0,0)`). */
export function isUnspecified(c: Rgba | null | undefined): boolean {
  return !c || (c.r === 0 && c.g === 0 && c.b === 0 && c.a === 0);
}

/** "#rrggbb" (the form an `<input type="color">` reads). */
export function toHex6(c: Rgba): string {
  const h = (v: number) => clamp255(v).toString(16).padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

/** "#rrggbbaa", the form `layerColor` returns. */
export function toHex8(c: Rgba): string {
  return `${toHex6(c)}${clamp255(c.a * 255).toString(16).padStart(2, "0")}`;
}

/** The colour a stored colour text means, or null when it is not a colour or is `COLOR4D::UNSPECIFIED`. */
export function specifiedColor(text: string | undefined | null): Rgba | null {
  if (!text) return null;
  const c = parseCssColor(text);
  return isUnspecified(c) ? null : c;
}

// ------------------------------------------------------------------------------------------------------------------------------------ objects

/**
 * The objects with a visibility of their own, by the names a project file stores them under (`VISIBILITY_LAYER`, lower case; they are the
 * `board.visible_items` of the `.kicad_prl` and the `renderLayers` of a layer preset).
 */
export const OBJECT_IDS = [
  "tracks",
  "vias",
  "pads",
  "zones",
  "shapes",
  "bitmaps",
  "footprints_front",
  "footprints_back",
  "footprint_values",
  "footprint_references",
  "footprint_text",
  "footprint_anchors",
  "ly_points",
  "ratsnest",
  "drc_warnings",
  "drc_errors",
  "drc_exclusions",
  "locked_item_shadows",
  "conflict_shadows",
  "board_outline_area",
  "drawing_sheet",
  "grid",
] as const;
export type ObjectId = (typeof OBJECT_IDS)[number];

export function isObjectId(s: unknown): s is ObjectId {
  return typeof s === "string" && (OBJECT_IDS as readonly string[]).includes(s);
}

/**
 * `GAL_SET::DefaultVisible` restricted to the 22: everything is on except the DRC exclusions ("DRC exclusions hidden by default") and the board area
 * shadow ("currently hidden by default") -- and, unlike KiCad, the drawing sheet: the placer starts a board at the page's corner, where the sheet's frame (ten
 * and twelve millimetres in from the paper's edge) would cut straight through it, and the studio has never drawn one. One click on Drawing Sheet shows it.
 */
export const DEFAULT_OBJECT_VISIBILITY: Readonly<Record<ObjectId, boolean>> = {
  tracks: true,
  vias: true,
  pads: true,
  zones: true,
  shapes: true,
  bitmaps: true,
  footprints_front: true,
  footprints_back: true,
  footprint_values: true,
  footprint_references: true,
  footprint_text: true,
  footprint_anchors: true,
  ly_points: true,
  ratsnest: true,
  drc_warnings: true,
  drc_errors: true,
  drc_exclusions: false,
  locked_item_shadows: true,
  conflict_shadows: true,
  board_outline_area: false,
  drawing_sheet: false,
  grid: true,
};

/** The six opacities (`PCB_DISPLAY_OPTIONS::m_TrackOpacity` ...), 0..1. */
export interface Opacity {
  tracks: number;
  vias: number;
  pads: number;
  zones: number;
  images: number;
  shapes: number;
}
export type OpacityKey = keyof Opacity;

/** `PROJECT_LOCAL_SETTINGS`' defaults: zones and images are 60 % opaque, everything else fully. */
export const DEFAULT_OPACITY: Readonly<Opacity> = { tracks: 1, vias: 1, pads: 1, zones: 0.6, images: 0.6, shapes: 1 };

/** One row of the Objects tab (`APPEARANCE_CONTROLS::s_objectSettings`; a null id is the spacer `RR()` between groups). */
export interface ObjectRow {
  id: ObjectId | null;
  label: string;
  tooltip: string;
  /** The opacity slider, when the row has one. */
  opacity?: OpacityKey;
  /** False for the one row that has a slider and no checkbox ("Filled Shapes": `can_control_visibility` false). */
  checkbox: boolean;
}

/** The rows, in `s_objectSettings`' order, minus Images and Points (see the header). */
export const OBJECT_ROWS: readonly ObjectRow[] = [
  { id: "tracks", label: "Tracks", tooltip: "Show tracks", opacity: "tracks", checkbox: true },
  { id: "vias", label: "Vias", tooltip: "Show all vias", opacity: "vias", checkbox: true },
  { id: "pads", label: "Pads", tooltip: "Show all pads", opacity: "pads", checkbox: true },
  { id: "zones", label: "Zones", tooltip: "Show copper zones", opacity: "zones", checkbox: true },
  { id: "shapes", label: "Filled Shapes", tooltip: "Opacity of filled shapes", opacity: "shapes", checkbox: false },
  { id: null, label: "", tooltip: "", checkbox: false },
  { id: "footprints_front", label: "Footprints Front", tooltip: "Show footprints that are on board's front", checkbox: true },
  { id: "footprints_back", label: "Footprints Back", tooltip: "Show footprints that are on board's back", checkbox: true },
  { id: "footprint_values", label: "Values", tooltip: "Show footprint values", checkbox: true },
  { id: "footprint_references", label: "References", tooltip: "Show footprint references", checkbox: true },
  { id: "footprint_text", label: "Footprint Text", tooltip: "Show all footprint text", checkbox: true },
  { id: null, label: "", tooltip: "", checkbox: false },
  { id: "ratsnest", label: "Ratsnest", tooltip: "Show unconnected nets as a ratsnest", checkbox: true },
  { id: "drc_warnings", label: "DRC Warnings", tooltip: "DRC violations with a Warning severity", checkbox: true },
  { id: "drc_errors", label: "DRC Errors", tooltip: "DRC violations with an Error severity", checkbox: true },
  { id: "drc_exclusions", label: "DRC Exclusions", tooltip: "DRC violations which have been individually excluded", checkbox: true },
  { id: "footprint_anchors", label: "Anchors", tooltip: "Show footprint and text origins as a cross", checkbox: true },
  { id: "locked_item_shadows", label: "Locked Item Shadow", tooltip: "Show a shadow on locked items", checkbox: true },
  { id: "conflict_shadows", label: "Colliding Courtyards", tooltip: "Show colliding footprint courtyards", checkbox: true },
  { id: "board_outline_area", label: "Board Area Shadow", tooltip: "Show board area shadow", checkbox: true },
  { id: "drawing_sheet", label: "Drawing Sheet", tooltip: "Show drawing sheet borders and title block", checkbox: true },
  { id: "grid", label: "Grid", tooltip: "Show the (x,y) grid dots", checkbox: true },
];

/**
 * `onObjectVisibilityChanged`: "Footprint Text is a meta-control that also can disable values/references, drag them along here" -- switching it
 * (by its own checkbox) sets both; and switching References or Values ON puts the meta-control back on. Every other object is just itself.
 */
export function withObjectVisible(visible: Readonly<Record<ObjectId, boolean>>, id: ObjectId, on: boolean): Record<ObjectId, boolean> {
  const next = { ...visible, [id]: on };
  if (id === "footprint_text") {
    next.footprint_references = on;
    next.footprint_values = on;
  } else if ((id === "footprint_references" || id === "footprint_values") && on) {
    next.footprint_text = true;
  }
  return next;
}

/**
 * The object a DRC marker belongs to (`PCB_MARKER::ViewGetLayers`): a violation the person waived is a DRC exclusion, otherwise its severity decides --
 * a warning is a DRC warning, everything else (an error, and any severity KiCad does not name) a DRC error.
 */
export function drcMarkerObject(v: { excluded?: boolean; severity: string }): ObjectId {
  return v.excluded === true ? "drc_exclusions" : v.severity === "warning" ? "drc_warnings" : "drc_errors";
}

// ------------------------------------------------------------------------------------------------------------------------------------ modes

/** `HIGH_CONTRAST_MODE`: what the layers other than the active one look like. */
export type ContrastMode = "normal" | "dimmed" | "hidden";
export const CONTRAST_MODES: readonly ContrastMode[] = ["normal", "dimmed", "hidden"];

/** `PCB_CONTROL::HighContrastModeCycle`: normal -> dimmed -> hidden -> normal. */
export function nextContrastMode(m: ContrastMode): ContrastMode {
  return m === "normal" ? "dimmed" : m === "dimmed" ? "hidden" : "normal";
}

/** `NET_COLOR_MODE`: where the net and net class colours show -- on every copper item, on the ratsnest only (KiCad's default), or nowhere. */
export type NetColorMode = "all" | "ratsnest" | "off";
export const NET_COLOR_MODES: readonly NetColorMode[] = ["all", "ratsnest", "off"];

/** `PCB_CONTROL::NetColorModeCycle`: all -> ratsnest -> off -> all. */
export function nextNetColorMode(m: NetColorMode): NetColorMode {
  return m === "all" ? "ratsnest" : m === "ratsnest" ? "off" : "all";
}

/** The numbers a `.kicad_prl` stores the two modes as (`board.high_contrast_mode`, `board.net_color_mode`: `NET_COLOR_MODE::OFF` is 0). */
export const CONTRAST_MODE_NUMBER: Readonly<Record<ContrastMode, number>> = { normal: 0, dimmed: 1, hidden: 2 };
export const NET_COLOR_MODE_NUMBER: Readonly<Record<NetColorMode, number>> = { off: 0, ratsnest: 1, all: 2 };

export function contrastModeOf(n: unknown): ContrastMode {
  return n === 1 ? "dimmed" : n === 2 ? "hidden" : "normal";
}
export function netColorModeOf(n: unknown): NetColorMode {
  return n === 0 ? "off" : n === 2 ? "all" : "ratsnest";
}

/** The ratsnest display of the Net Display Options: `m_ShowGlobalRatsnest` off is "none", else `m_RatsnestMode` (all layers, or visible ones). */
export type RatsnestDisplay = "all" | "visible" | "none";

// ------------------------------------------------------------------------------------------------------------------------------------ state

/**
 * What the Appearance panel keeps beyond the older fields of `StudioState` (`layerVisible` for the layers, `showRatsnest`, `gridVisible`,
 * `highContrast`, `bcx.hiddenRatsnestNets`, `bcx.ratsnestMode`, `bcx.boardFlipped`, which the panel reads and writes as they are).
 */
export interface AppearanceState {
  /** Object visibility. The `ratsnest` and `grid` entries are not authoritative: `StudioState.showRatsnest` and `gridVisible` are (`objectsVisible` merges them in). */
  visible: Record<ObjectId, boolean>;
  opacity: Opacity;
  /** HIDDEN contrast mode (inactive layers not drawn at all) -- `StudioState.highContrast` is "not NORMAL", this says which of the two non-normal modes. */
  contrastHidden: boolean;
  netColorMode: NetColorMode;
  /** `NET_SETTINGS::m_NetColorAssignments` (`net_settings.net_colors` of the project): net name -> colour text. A net with no entry has no colour of its own. */
  netColors: Record<string, string>;
  /** `NETCLASS::GetPcbColor` of each class that has one (`net_settings.classes[].pcb_color`): class name -> colour text. */
  netclassColors: Record<string, string>;
  /** `PROJECT_LOCAL_SETTINGS::m_HiddenNetclasses`: the classes whose nets' ratsnest is hidden. */
  hiddenNetclasses: string[];
  /** The layer presets the user saved (`PROJECT_FILE::m_LayerPresets`); the built-in ones are `builtinPresets`. */
  presets: LayerPreset[];
  /** `m_currentPreset->name`: the preset the combo shows ("" none, shown as "---"). */
  activePreset: string;
  /** `PROJECT_FILE::m_Viewports`. */
  viewports: Viewport[];
  /** `m_lastBuiltinPreset.renderLayers`: the objects as they were when a built-in preset (or none) was last left, which the next built-in preset restores. Not saved. */
  lastBuiltinObjects: ObjectId[] | null;
}

/** A saved set of visible layers and objects (`LAYER_PRESET`). Layers are KiCad layer names ("F.Cu", "F.SilkS", "Edge.Cuts"). */
export interface LayerPreset {
  name: string;
  layers: string[];
  renderLayers: ObjectId[];
  flipBoard: boolean;
  /** The layer made active when the preset is applied (null = keep, unless it is not in the preset). */
  activeLayer: string | null;
  readOnly: boolean;
}

/** A saved view (`VIEWPORT`): the part of the board on screen, as a rectangle in micrometres. */
export interface Viewport {
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export function defaultAppearance(): AppearanceState {
  return {
    visible: { ...DEFAULT_OBJECT_VISIBILITY },
    opacity: { ...DEFAULT_OPACITY },
    contrastHidden: false,
    netColorMode: "ratsnest",
    netColors: {},
    netclassColors: {},
    hiddenNetclasses: [],
    presets: [],
    activePreset: "All Layers",
    viewports: [],
    lastBuiltinObjects: null,
  };
}

export const DEFAULT_APPEARANCE: AppearanceState = defaultAppearance();

/** The ids of the objects that are on, in `OBJECT_IDS`' order, with the two older toggles merged in. */
export function visibleObjects(a: AppearanceState, showRatsnest: boolean, gridVisible: boolean): ObjectId[] {
  return OBJECT_IDS.filter((id) => (id === "ratsnest" ? showRatsnest : id === "grid" ? gridVisible : a.visible[id]));
}

/** The keys under which object visibility rides in `StudioState.layerVisible`, so every picker that already gets that record honours it. */
export const OBJECT_KEY_PREFIX = "obj:";
export const objectKey = (id: ObjectId): string => OBJECT_KEY_PREFIX + id;

/** True unless the object is switched off in `layerVisible` (an object nobody has set is on). */
export function objectOn(layerVisible: Readonly<Record<string, boolean>>, id: ObjectId): boolean {
  return layerVisible[OBJECT_KEY_PREFIX + id] !== false;
}

/**
 * `layerVisible` with the `obj:<id>` keys for the current appearance. An object with an opacity that is 0 counts as off, as it does for
 * `PCB_SELECTION_TOOL::Selectable` ("options.m_TrackOpacity == 0.00" -> not selectable) and for the painter (nothing is drawn).
 */
export function withObjectKeys(layerVisible: Readonly<Record<string, boolean>>, a: AppearanceState): Record<string, boolean> {
  const out = { ...layerVisible };
  const zero = (k: OpacityKey) => a.opacity[k] <= 0;
  const set = (id: ObjectId, on: boolean) => {
    out[objectKey(id)] = on;
  };
  for (const id of OBJECT_IDS) set(id, a.visible[id]);
  set("tracks", a.visible.tracks && !zero("tracks"));
  set("vias", a.visible.vias && !zero("vias"));
  set("pads", a.visible.pads && !zero("pads"));
  set("zones", a.visible.zones && !zero("zones"));
  set("shapes", a.visible.shapes && !zero("shapes"));
  return out;
}

// ------------------------------------------------------------------------------------------------------------------------------------ net colours

/** The colour of each net that has one under the current net colour mode, as the painter looks it up. */
export type NetPalette = ReadonlyMap<string, Rgba>;

/**
 * Each net's own colour: the one assigned to the net, else its net class's (`NETCLASS::HasPcbColor`), else none -- `PCB_RENDER_SETTINGS::GetColor`'s
 * order for copper and `RATSNEST_VIEW_ITEM::ViewDraw`'s for the ratsnest. `classOf` names the class of a net ("Default" owns the nets no class
 * claims; it never has a colour -- "Default netclass can't have an override color").
 */
export function netPalette(a: Pick<AppearanceState, "netColors" | "netclassColors">, nets: Iterable<string>, classOf: (net: string) => string): Map<string, Rgba> {
  const out = new Map<string, Rgba>();
  for (const net of nets) {
    const own = specifiedColor(a.netColors[net]);
    if (own) {
      out.set(net, own);
      continue;
    }
    const cls = classOf(net);
    const viaClass = cls === "Default" ? null : specifiedColor(a.netclassColors[cls]);
    if (viaClass) out.set(net, viaClass);
  }
  return out;
}

/** Whether the mode puts net colours on copper (`NET_COLOR_MODE::ALL`) or on the ratsnest (anything but `OFF`). */
export const netColorsOnCopper = (m: NetColorMode): boolean => m === "all";
export const netColorsOnRatsnest = (m: NetColorMode): boolean => m !== "off";

// -------------------------------------------------------------------------------------------------------------------------- colours of copper and ratsnest

/** What the painter reads to colour a net: the net colour mode and each net's own colour (`netPalette`). */
export interface NetColorView {
  netColorMode: NetColorMode;
  palette: ReadonlyMap<string, Rgba>;
}

const rgbaText = (c: Rgba): string => `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})`;

/**
 * `PCB_RENDER_SETTINGS::GetColor`'s net branch for copper: the item's colour is its net's (or its net class's) when the net colour mode is "All" and the net
 * has one, else `base` (null: nothing to change). With a net highlight (`highlighted` true on the highlighted net, false on every other, null for none) the colour
 * is brightened or darkened by the same 0.5 (`m_highlightFactor`), its alpha kept.
 */
export function copperColor(base: string, net: string | null | undefined, view: NetColorView | undefined, highlighted: boolean | null): string {
  const own = net && view && netColorsOnCopper(view.netColorMode) ? view.palette.get(net) : undefined;
  if (!own) return base;
  if (highlighted === null) return rgbaText(own);
  const rgb = highlighted ? brighten(own, HIGHLIGHT_FACTOR) : darken(own, HIGHLIGHT_FACTOR);
  return rgbaText({ ...rgb, a: own.a });
}

/** The ratsnest line's colour: the net's (or its class's) in every mode but "None" (`RATSNEST_VIEW_ITEM::ViewDraw`'s `colorByNet`), else null (the ratsnest colour). */
export function ratsnestColor(net: string, view: NetColorView | undefined): string | null {
  const own = view && netColorsOnRatsnest(view.netColorMode) ? view.palette.get(net) : undefined;
  return own ? rgbaText(own) : null;
}
