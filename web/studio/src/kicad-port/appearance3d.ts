// The 3D viewer's Appearance manager: what its layer tree lists, what is shown by default, and what colour each row is.
//
// Ported from 3d-viewer/dialogs/appearance_controls_3D.cpp (`APPEARANCE_CONTROLS_3D::s_layerSettings`, the rows and their tooltips; `inStackupColors`, the rows whose
// colour the stackup owns), 3d-viewer/3d_viewer/eda_3d_viewer_settings.cpp (the defaults), 3d-viewer/3d_canvas/board_adapter.cpp (`GetLayerColors`: the theme colours, the
// colours read from the board's physical stackup, the copper finish; the tables `g_SilkColors`, `g_MaskColors`, `g_BoardColors`, `g_FinishColors`) and
// pcbnew/board_stackup_manager/stackup_predefined_prms.cpp (the colour names Board Setup offers). Pure: no three.js, no React.
//
// What the data model has and has not: the board has the copper, silkscreen, mask and paste of `scene.ts`, drawing shapes on any layer, and parts with 3D models;
// it has no "off-board silkscreen", footprint values or text, DNP or not-in-position-file attributes, so those rows of KiCad's tree are not listed (PARITY-3d.md).

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

const c255 = (r: number, g: number, b: number, a = 1): Rgba => ({ r: r / 255, g: g / 255, b: b / 255, a });

/** `COLOR4D::SetFromHexString`: `#RRGGBB` (opaque) or `#RRGGBBAA`; anything else is not a colour. */
export function parseColor(text: string): Rgba | null {
  const s = text.trim();
  if (s.length < 7 || !s.startsWith("#") || !/^#[0-9a-fA-F]+$/.test(s)) return null;
  const v = Number.parseInt(s.slice(1), 16);
  if (!Number.isFinite(v)) return null;
  if (s.length >= 9) return { r: ((v >>> 24) & 0xff) / 255, g: ((v >>> 16) & 0xff) / 255, b: ((v >>> 8) & 0xff) / 255, a: (v & 0xff) / 255 };
  return { r: ((v >>> 16) & 0xff) / 255, g: ((v >>> 8) & 0xff) / 255, b: (v & 0xff) / 255, a: 1 };
}

const byte = (v: number) => Math.min(255, Math.max(0, Math.round(v * 255)));
const hex2 = (n: number) => n.toString(16).padStart(2, "0");

/** `#rrggbb` (what an `<input type="color">` holds): the colour without its alpha. */
export function toRgbHex(c: Rgba): string {
  return `#${hex2(byte(c.r))}${hex2(byte(c.g))}${hex2(byte(c.b))}`;
}

/** `#rrggbbaa`, `COLOR4D::ToHexString`'s form. */
export function toHex(c: Rgba): string {
  return `${toRgbHex(c)}${hex2(byte(c.a))}`;
}

// ------------------------------------------------------------------------------------------------------------------------------ the tables

interface NamedColor {
  name: string;
  color: Rgba;
}

/** The name a stackup item has when its colour is not specified (`NotSpecifiedPrm`, stackup_predefined_prms.h). */
export const NOT_SPECIFIED = "Not specified";

/** `BOARD_ADAPTER::g_SilkColors`: the colours of a silkscreen item of the stackup (the first, "Not specified", is white). */
export const SILK_COLORS: readonly NamedColor[] = [
  { name: NOT_SPECIFIED, color: c255(245, 245, 245) },
  { name: "Green", color: c255(20, 51, 36) },
  { name: "Red", color: c255(181, 19, 21) },
  { name: "Blue", color: c255(2, 59, 162) },
  { name: "Black", color: c255(11, 11, 11) },
  { name: "White", color: c255(245, 245, 245) },
  { name: "Purple", color: c255(32, 2, 53) },
  { name: "Yellow", color: c255(194, 195, 0) },
];

/** `BOARD_ADAPTER::g_MaskColors`: a solder mask item (alpha 0.83, `DEFAULT_SOLDERMASK_OPACITY`; "Not specified" is the green of the default theme). */
export const MASK_COLORS: readonly NamedColor[] = [
  { name: NOT_SPECIFIED, color: c255(20, 51, 36, 0.83) },
  { name: "Green", color: c255(20, 51, 36, 0.83) },
  { name: "Light Green", color: c255(91, 168, 12, 0.83) },
  { name: "Saturated Green", color: c255(13, 104, 11, 0.83) },
  { name: "Red", color: c255(181, 19, 21, 0.83) },
  { name: "Light Red", color: c255(210, 40, 14, 0.83) },
  { name: "Red/Orange", color: c255(239, 53, 41, 0.83) },
  { name: "Blue", color: c255(2, 59, 162, 0.83) },
  { name: "Light Blue 1", color: c255(54, 79, 116, 0.83) },
  { name: "Light Blue 2", color: c255(61, 85, 130, 0.83) },
  { name: "Green/Blue", color: c255(21, 70, 80, 0.83) },
  { name: "Black", color: c255(11, 11, 11, 0.83) },
  { name: "White", color: c255(245, 245, 245, 0.83) },
  { name: "Purple", color: c255(32, 2, 53, 0.83) },
  { name: "Light Purple", color: c255(119, 31, 91, 0.83) },
  { name: "Yellow", color: c255(194, 195, 0, 0.83) },
];

/** `BOARD_ADAPTER::g_BoardColors`: a dielectric (core or prepreg) item; the body is their mix. */
export const BOARD_COLORS: readonly NamedColor[] = [
  { name: "FR4 natural, dark", color: c255(51, 43, 22, 0.83) },
  { name: "FR4 natural", color: c255(109, 116, 75, 0.83) },
  { name: "PTFE natural", color: c255(252, 252, 250, 0.9) },
  { name: "Polyimide", color: c255(205, 130, 0, 0.68) },
  { name: "Phenolic natural", color: c255(92, 17, 6, 0.9) },
  { name: "Brown 1", color: c255(146, 99, 47, 0.83) },
  { name: "Brown 2", color: c255(160, 123, 54, 0.83) },
  { name: "Brown 3", color: c255(146, 99, 47, 0.83) },
  { name: "Aluminum", color: c255(213, 213, 213, 1) },
];

/** `BOARD_ADAPTER::g_FinishColors`: the copper finish the surface finish names. */
export const FINISH_COLORS: readonly NamedColor[] = [
  { name: "Copper", color: c255(184, 115, 50) },
  { name: "Gold", color: c255(178, 156, 0) },
  { name: "Silver", color: c255(213, 213, 213) },
  { name: "Tin", color: c255(160, 160, 160) },
];

/**
 * The colour names Board Setup > Physical Stackup offers for a stackup item (`GetStandardColors`, stackup_predefined_prms.cpp): a mask or a silkscreen's, or a
 * dielectric's. "Not specified" writes no colour; a colour typed as `#RRGGBB[AA]` is the "User defined" one.
 */
export const STACKUP_COLOR_NAMES = {
  silk: [NOT_SPECIFIED, "Green", "Red", "Blue", "Purple", "Black", "White", "Yellow"],
  mask: [NOT_SPECIFIED, "Green", "Red", "Blue", "Purple", "Black", "White", "Yellow"],
  dielectric: [NOT_SPECIFIED, "FR4 natural", "PTFE natural", "Polyimide", "Phenolic natural", "Aluminum"],
} as const;

/** Which colour list a stackup item of this KiCad type takes (`BOARD_STACKUP_ITEM::IsColorEditable`), or null when its colour is not editable (copper, paste). */
export function stackupColorKind(itemType: string | undefined): "silk" | "mask" | "dielectric" | null {
  const t = (itemType ?? "").toLowerCase();
  if (t.includes("silk screen")) return "silk";
  if (t.includes("solder mask")) return "mask";
  if (t === "core" || t === "prepreg" || t === "dielectric") return "dielectric";
  return null;
}

/** `findColor` of `GetLayerColors`: a `#...` string is a colour; a name is looked up in `set`; the rest is transparent black (the colour is not set). */
function findColor(name: string | undefined, set: readonly NamedColor[]): Rgba {
  const n = (name ?? "").trim() === "" ? NOT_SPECIFIED : (name ?? "").trim();
  if (n.startsWith("#")) return parseColor(n) ?? { r: 0, g: 0, b: 0, a: 0 };
  return set.find((c) => c.name === n)?.color ?? { r: 0, g: 0, b: 0, a: 0 };
}

const isZero = (c: Rgba) => c.r === 0 && c.g === 0 && c.b === 0 && c.a === 0;

// ------------------------------------------------------------------------------------------------------------------------------ the rows

export type RowGroup = "board" | "user" | "models" | "display";

export interface Row {
  /** `EDA_3D_VIEWER_SETTINGS`'s own names where it has them (`plated_barrels`, `user_drawings`), the layer's name otherwise. */
  id: string;
  label: string;
  /** KiCad's tooltip (the action's own for the model rows). */
  tooltip: string;
  group: RowGroup;
  /** Whether the row carries a colour swatch (`GetLayerColors` has a colour for its `LAYER_3D_*`). */
  color: boolean;
  /** Shown when the person has not said (`EDA_3D_VIEWER_SETTINGS`). */
  visible: boolean;
  /** The one-key shortcut of the row's action (`EDA_3D_ACTIONS`), shown beside it. */
  hotkey?: string;
}

const row = (id: string, label: string, tooltip: string, group: RowGroup, color: boolean, visible = true, hotkey?: string): Row => ({ id, label, tooltip, group, color, visible, ...(hotkey ? { hotkey } : {}) });

export const USER_LAYER_COUNT = 45;

/** KiCad's user-layer rows: the four named ones and `User.1` .. `User.45`, in `s_layerSettings` order. */
export const USER_ROWS: readonly Row[] = [
  row("user_drawings", "User.Drawings", "Show user drawings layer", "user", true),
  row("user_comments", "User.Comments", "Show user comments layer", "user", true),
  row("user_eco1", "User.Eco1", "Show user ECO1 layer", "user", true),
  row("user_eco2", "User.Eco2", "Show user ECO2 layer", "user", true),
  ...Array.from({ length: USER_LAYER_COUNT }, (_, i) => row(`user_${i + 1}`, `User.${i + 1}`, `Show user defined layer ${i + 1}`, "user", true)),
];

/** Every row the data model can show, in KiCad's order (`s_layerSettings`). The user layers are listed by [`rowsFor`] only when the board has something on them. */
export const ROWS: readonly Row[] = [
  row("board", "Board Body", "Show board body", "board", true),
  row("plated_barrels", "Plated Barrels", "Show barrels of plated through-holes and vias", "board", false),
  row("copper_top", "F.Cu", "Show front copper / surface finish color", "board", true),
  row("copper_bottom", "B.Cu", "Show back copper / surface finish color", "board", true),
  row("adhesive", "Adhesive", "Show adhesive", "board", false),
  row("solder_paste", "Solder Paste", "Show solder paste", "board", true),
  row("silkscreen_top", "F.Silkscreen", "Show front silkscreen", "board", true),
  row("silkscreen_bottom", "B.Silkscreen", "Show back silkscreen", "board", true),
  row("soldermask_top", "F.Mask", "Show front solder mask", "board", true),
  row("soldermask_bottom", "B.Mask", "Show back solder mask", "board", true),
  ...USER_ROWS,
  row("th_models", "Through-hole Models", "Show 3D models for 'Through hole' type footprints", "models", false, true, "T"),
  row("smd_models", "SMD Models", "Show 3D models for 'Surface mount' type footprints", "models", false, true, "S"),
  row("virtual_models", "Virtual Models", "Show 3D models for 'Virtual' type footprints", "models", false, true, "V"),
  row("bounding_boxes", "Model Bounding Boxes", "Show a bounding box per model", "models", false, false),
  row("references", "References", "Show footprint references", "display", false),
  row("zones", "Zones", "Show copper zones (Preferences > 3D Viewer > Display Options: Show zones)", "display", false),
  row("background_top", "Background Start", "Background gradient start color", "display", true),
  row("background_bottom", "Background End", "Background gradient end color", "display", true),
];

const ROW_BY_ID = new Map(ROWS.map((r) => [r.id, r]));

export function rowOf(id: string): Row | undefined {
  return ROW_BY_ID.get(id);
}

/** Whether the row is shown: what the person set, else the row's default. */
export function isVisible(layers: Readonly<Record<string, boolean>>, id: string): boolean {
  const set = layers[id];
  if (set !== undefined) return set;
  return ROW_BY_ID.get(id)?.visible ?? true;
}

/** `layers` with the row's visibility flipped (a hotkey, a click on its eye). */
export function toggled(layers: Readonly<Record<string, boolean>>, id: string): Record<string, boolean> {
  return { ...layers, [id]: !isVisible(layers, id) };
}

/** The rows that gate partModels.ts's models (the rest gate the board's own geometry): rebuilding the board for one of these is wasted work. */
export const MODEL_ROWS: ReadonlySet<string> = new Set(["th_models", "smd_models", "virtual_models", "bounding_boxes"]);

/** The user-layer rows a board has something on: the rows its drawings' layers name. */
export function usedUserRows(layerNames: Iterable<string>): Set<string> {
  const used = new Set<string>();
  for (const name of layerNames) {
    const row = userRowOfLayer(name);
    if (row) used.add(row);
  }
  return used;
}

/** The rows the tree lists for a board: every fixed one, and the user layers the board has something on, in KiCad's order. */
export function rowsFor(usedUserRows: ReadonlySet<string>): Row[] {
  return ROWS.filter((r) => r.group !== "user" || usedUserRows.has(r.id));
}

/** The user-layer row a drawing on `layerName` belongs to (`Dwgs.User` -> `user_drawings`, `User.7` -> `user_7`), or null. */
export function userRowOfLayer(layerName: string): string | null {
  switch (layerName) {
    case "Dwgs.User":
      return "user_drawings";
    case "Cmts.User":
      return "user_comments";
    case "Eco1.User":
      return "user_eco1";
    case "Eco2.User":
      return "user_eco2";
  }
  const m = /^User\.(\d+)$/.exec(layerName);
  if (m && Number(m[1]) >= 1 && Number(m[1]) <= USER_LAYER_COUNT) return `user_${m[1]}`;
  return null;
}

/** The 2D editor's colour key of a user row (`colors.json`'s `Dwgs_User`, `User_7`), whose colour KiCad's theme gives the 3D user layer (`s_defaultTheme.at( pcb_layer )`). */
export function userRowEditorKey(id: string): string {
  switch (id) {
    case "user_drawings":
      return "Dwgs_User";
    case "user_comments":
      return "Cmts_User";
    case "user_eco1":
      return "Eco1_User";
    case "user_eco2":
      return "Eco2_User";
    default:
      return `User_${id.replace("user_", "")}`;
  }
}

/** What a drawing on `layerName` is in the 3D view: a silkscreen of a side, an adhesive, a user layer (its row), or nothing KiCad draws (`Edge.Cuts`, courtyard, fab, copper, mask, paste). */
export type ShapeLayer = { kind: "silk"; side: "top" | "bottom" } | { kind: "adhesive"; side: "top" | "bottom" } | { kind: "user"; row: string } | null;

export function shapeLayerOf(layerName: string): ShapeLayer {
  switch (layerName) {
    case "F.SilkS":
      return { kind: "silk", side: "top" };
    case "B.SilkS":
      return { kind: "silk", side: "bottom" };
    case "F.Adhes":
      return { kind: "adhesive", side: "top" };
    case "B.Adhes":
      return { kind: "adhesive", side: "bottom" };
  }
  const user = userRowOfLayer(layerName);
  return user ? { kind: "user", row: user } : null;
}

/** The rows the colour of which the board's stackup owns when "Use board stackup colors" is on (`inStackupColors`): their swatches are read-only then. */
export const STACKUP_ROWS: ReadonlySet<string> = new Set(["board", "copper_top", "copper_bottom", "solder_paste", "silkscreen_top", "silkscreen_bottom", "soldermask_top", "soldermask_bottom"]);

// ----------------------------------------------------------------------------------------------------------------------------- the colours

/** `builtin_color_themes.h`'s default theme for the 3D rows, as `colors.json` carries it (the 2D editor's colours are another set). */
export const THEME_COLORS: Readonly<Record<string, Rgba>> = {
  board: c255(51, 43, 23, 0.9),
  copper_top: c255(179, 156, 0),
  copper_bottom: c255(179, 156, 0),
  silkscreen_top: c255(230, 230, 230),
  silkscreen_bottom: c255(230, 230, 230),
  soldermask_top: c255(20, 51, 36, 0.83),
  soldermask_bottom: c255(20, 51, 36, 0.83),
  solder_paste: c255(128, 128, 128),
  background_top: c255(204, 204, 230),
  background_bottom: c255(102, 102, 128),
};

/** The slice of the board's physical stackup the colours read (`BOARD_STACKUP`). */
export interface StackupColors {
  layers: ReadonlyArray<{ kind?: string; name: string; color?: string }>;
  copper_finish?: string;
}

export interface ColorInputs {
  /** `m_UseStackupColors`: the silkscreen, mask, board body and copper finish come from the board's stackup. */
  useStackupColors: boolean;
  /** `render.use_board_editor_copper_colors`: the copper rows take the 2D editor's colours of F.Cu and B.Cu. */
  useEditorCopperColors: boolean;
  /** What the person set with a swatch: row id -> `#rrggbb[aa]`. */
  overrides: Readonly<Record<string, string>>;
  stackup: StackupColors | null | undefined;
  /** The 2D editor's colour for a layer key (`colors.json`: `F_Cu`, `User_3`, ...), `#rrggbbaa`. */
  editorColor: (key: string) => string | undefined;
}

/** `GetLayerColors`' finish branch: the copper colour a surface finish name gives, or null when the name is none of the four. */
export function finishColor(finish: string | undefined): Rgba | null {
  const f = finish ?? "";
  if (f === "") return null;
  if (f.endsWith("OSP")) return findColor("Copper", FINISH_COLORS);
  if (f.endsWith("IG") || f.endsWith("gold")) return findColor("Gold", FINISH_COLORS);
  if (f.startsWith("HAL") || f.startsWith("HASL") || f.endsWith("tin") || f.endsWith("nickel")) return findColor("Tin", FINISH_COLORS);
  if (f.endsWith("silver")) return findColor("Silver", FINISH_COLORS);
  return null;
}

/**
 * `BOARD_ADAPTER::GetLayerColors`: the colour of every row that has one. The theme's colours; with "Use board stackup colors", the board's physical stackup
 * overrides the silkscreen's, the mask's and the body's (the dielectrics mixed: `body = body.Mix( layer, 1 - layer.a )`, `body.a += ( 1 - body.a ) * layer.a / 2`) and the
 * surface finish the copper's; copper at the bottom is the top's; the person's swatch choices come last, except for a row the stackup owns while it does.
 */
export function resolveColors(inputs: ColorInputs): Record<string, Rgba> {
  const colors: Record<string, Rgba> = { ...THEME_COLORS };
  for (const r of ROWS) {
    if (r.group === "user" && r.color) colors[r.id] = parseColor(inputs.editorColor(userRowEditorKey(r.id)) ?? "") ?? c255(194, 194, 194);
  }

  if (inputs.useStackupColors && inputs.stackup) {
    let body: Rgba = { r: 0, g: 0, b: 0, a: 0 };
    for (const item of inputs.stackup.layers) {
      const kind = stackupColorKind(item.kind);
      if (kind === "silk") colors[item.name.startsWith("B.") ? "silkscreen_bottom" : "silkscreen_top"] = findColor(item.color, SILK_COLORS);
      else if (kind === "mask") colors[item.name.startsWith("B.") ? "soldermask_bottom" : "soldermask_top"] = findColor(item.color, MASK_COLORS);
      else if (kind === "dielectric") {
        const layer = findColor(item.color, BOARD_COLORS);
        if (isZero(body)) body = layer;
        else {
          const f = 1 - layer.a;
          body = { r: layer.r * (1 - f) + body.r * f, g: layer.g * (1 - f) + body.g * f, b: layer.b * (1 - f) + body.b * f, a: body.a };
        }
        body = { ...body, a: body.a + ((1 - body.a) * layer.a) / 2 };
      }
    }
    if (!isZero(body)) colors.board = body;
    const finish = finishColor(inputs.stackup.copper_finish);
    if (finish) colors.copper_top = finish;
  }
  colors.copper_bottom = colors.copper_top!;

  if (inputs.useEditorCopperColors) {
    const top = parseColor(inputs.editorColor("F_Cu") ?? "");
    const bottom = parseColor(inputs.editorColor("B_Cu") ?? "");
    if (top) colors.copper_top = top;
    if (bottom) colors.copper_bottom = bottom;
  }

  for (const [id, text] of Object.entries(inputs.overrides)) {
    if (inputs.useStackupColors && STACKUP_ROWS.has(id)) continue; // read-only: "Uncheck 'Use board stackup colors' to allow color editing."
    const c = parseColor(text);
    if (c && ROW_BY_ID.has(id)) colors[id] = c;
  }
  return colors;
}
