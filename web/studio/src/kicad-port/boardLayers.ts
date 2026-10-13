// The logic behind Board Setup > Board Stackup > Board Editor Layers (pcbnew/dialogs/panel_setup_layers.cpp), as pure functions: which layers a board has, what KiCad calls them
// (the name the user gave, else the standard one), the rows of the page in the order its dialog stacks them, the checks the panel makes before OK and the page a draft sends
// (`set_board_layers`, crates/ops/src/board_setup.rs; the backend checks it again, crates/model/src/layers.rs, and the two are held to the same cases in their tests).
//
// The copper layers are the Physical Stackup page's (their number changes there; here they can be named and typed); everything else a board can have is a technical layer
// KiCad starts a board with, or one of up to 45 user layers (`User.1` .. `User.45`). A layer's name in the board file is fixed (`F.SilkS`, `User.3`, `In2.Cu`); what the user
// changes is the name KiCad *shows*. Everything here speaks the file's names ("F.SilkS") and the boundary shows the display names.
//
// Ported from:
//   panel_setup_layers.cpp   initialize_layers_controls (the rows and their order), mandatoryLayerCbSetup, append_user_layer / addUserDefinedLayer, showLayerTypes and
//                            transferDataFromWindow (the types of copper and user layers), testLayerNames (a name is not empty, holds none of the forbidden characters, is not
//                            "signal", is unique), getRemovedLayersWithItems
//   board.cpp                GetLayerName (the user's name, else the standard one), GetLayerType
//   layer_id.cpp             LayerName (the standard names: `F.Silkscreen`, `User.Drawings` ...)
//   lset.cpp                 TechAndUserUIOrder (the order lists show the layers in)
//   layer_ids.h              PCB_LAYER_ID (the order `testLayerNames` walks them in)

import type { BoardLayerType, LayerSetup } from "../api/types";
import { TECH_LAYER_NAMES } from "./layerPresets";

/** The layers the page keeps checked (`mandatoryLayerCbSetup`): a board always has its courtyards, its outline and its margin. */
export const MANDATORY_LAYERS: readonly string[] = ["F.CrtYd", "B.CrtYd", "Edge.Cuts", "Margin"];

/** `User_1` .. `User_45`. */
export const MAX_USER_LAYERS = 45;

/** The standard names KiCad shows for the layers whose board-file name differs (`LayerName`). */
const STANDARD_NAMES: Readonly<Record<string, string>> = {
  "B.Adhes": "B.Adhesive",
  "F.Adhes": "F.Adhesive",
  "B.SilkS": "B.Silkscreen",
  "F.SilkS": "F.Silkscreen",
  "Dwgs.User": "User.Drawings",
  "Cmts.User": "User.Comments",
  "Eco1.User": "User.Eco1",
  "Eco2.User": "User.Eco2",
  "F.CrtYd": "F.Courtyard",
  "B.CrtYd": "B.Courtyard",
};

/** `LayerName( layer )`: the name KiCad shows for a layer nobody renamed, given the name the board file writes. */
export function standardLayerName(file: string): string {
  return STANDARD_NAMES[file] ?? file;
}

export const userLayerName = (n: number): string => `User.${n}`;

/** `User.7` -> 7; null for anything else. */
export function userLayerNumber(file: string): number | null {
  const m = /^User\.([1-9]\d*)$/.exec(file);
  const n = m ? Number(m[1]) : NaN;
  return n >= 1 && n <= MAX_USER_LAYERS ? n : null;
}

/** `F.Cu`, `B.Cu`, `In1.Cu` .. `In30.Cu`. */
export function isCopperLayerName(file: string): boolean {
  if (file === "F.Cu" || file === "B.Cu") return true;
  const m = /^In([1-9]\d*)\.Cu$/.exec(file);
  return !!m && Number(m[1]) <= 30;
}

/** A layer the page can enable: one of the standard technical layers, or `User.N`. */
export function isOptionalLayer(file: string): boolean {
  return TECH_LAYER_NAMES.includes(file) || userLayerNumber(file) !== null;
}

/** `PCB_LAYER_ID`: the order `LSET` walks a board's layers in (front and back interleaved), which is the order `testLayerNames` checks the names in. */
export function pcbLayerId(file: string): number {
  const fixed: Readonly<Record<string, number>> = { "F.Cu": 0, "F.Mask": 1, "B.Cu": 2, "B.Mask": 3, "F.SilkS": 5, "B.SilkS": 7, "F.Adhes": 9, "B.Adhes": 11, "F.Paste": 13, "B.Paste": 15, "Dwgs.User": 17, "Cmts.User": 19, "Eco1.User": 21, "Eco2.User": 23, "Edge.Cuts": 25, Margin: 27, "B.CrtYd": 29, "F.CrtYd": 31, "B.Fab": 33, "F.Fab": 35 };
  const known = fixed[file];
  if (known !== undefined) return known;
  const user = userLayerNumber(file);
  if (user !== null) return 37 + 2 * user;
  const inner = /^In(\d+)\.Cu$/.exec(file);
  return inner ? 2 + 2 * Number(inner[1]) : Number.MAX_SAFE_INTEGER;
}

export const COPPER_LAYER_TYPES: readonly BoardLayerType[] = ["signal", "power", "mixed", "jumper"];
export const USER_LAYER_TYPES: readonly BoardLayerType[] = ["auxiliary", "front", "back"];

/** The choices of the type lists (`initialize_layers_controls`, `append_user_layer`). */
export const LAYER_TYPE_LABEL: Readonly<Record<BoardLayerType, string>> = {
  signal: "signal",
  power: "power plane",
  mixed: "mixed",
  jumper: "jumper",
  auxiliary: "Auxiliary",
  front: "Off-board, front",
  back: "Off-board, back",
};

/** The static description the page puts beside a technical layer. */
const TECH_NOTE: Readonly<Record<string, string>> = {
  "F.CrtYd": "Off-board, testing",
  "B.CrtYd": "Off-board, testing",
  "F.Fab": "Off-board, manufacturing",
  "B.Fab": "Off-board, manufacturing",
  "F.Adhes": "On-board, non-copper",
  "B.Adhes": "On-board, non-copper",
  "F.Paste": "On-board, non-copper",
  "B.Paste": "On-board, non-copper",
  "F.SilkS": "On-board, non-copper",
  "B.SilkS": "On-board, non-copper",
  "F.Mask": "On-board, non-copper",
  "B.Mask": "On-board, non-copper",
  "Edge.Cuts": "Board contour",
  Margin: "Board contour setback",
  "Eco1.User": "Auxiliary",
  "Eco2.User": "Auxiliary",
  "Cmts.User": "Auxiliary",
  "Dwgs.User": "Auxiliary",
};

/** The tooltips of the technical layers' check boxes (`SetToolTip`). */
const TECH_TIP: Readonly<Record<string, string>> = {
  "F.Fab": "If you want a fabrication layer for the front side of the board",
  "F.Adhes": "If you want an adhesive template for the front side of the board",
  "F.Paste": "If you want a solder paste layer for front side of the board",
  "F.SilkS": "If you want a silk screen layer for the front side of the board",
  "F.Mask": "If you want a solder mask layer for the front of the board",
  "Cmts.User": "If you want a separate layer for comments or notes",
  "Dwgs.User": "If you want a layer for documentation drawings",
};

// ------------------------------------------------------------------------------------------------------------------------------------ what the board has

/** The enabled optional layers in `TechAndUserUIOrder`: the technical ones as KiCad lists them, then the user layers by number. */
export function canonicalEnabled(enabled: readonly string[]): string[] {
  const on = new Set(enabled);
  const users = enabled.map(userLayerNumber).filter((n): n is number => n !== null);
  return [...TECH_LAYER_NAMES.filter((l) => on.has(l)), ...[...new Set(users)].sort((a, b) => a - b).map(userLayerName)];
}

/** The optional layers a board has: what its Board Editor Layers page says, else the 18 technical layers KiCad starts a board with. */
export function enabledOptionalLayers(setup: LayerSetup | undefined | null): string[] {
  return setup ? canonicalEnabled(setup.enabled) : [...TECH_LAYER_NAMES];
}

/** The layers a picker offers: the board's copper layers, then the optional ones it has. */
export function boardLayerList(copper: readonly string[], setup: LayerSetup | undefined | null): string[] {
  return [...copper, ...enabledOptionalLayers(setup)];
}

/** `BOARD::GetLayerName`: the name the user gave the layer, else KiCad's standard one. */
export function layerDisplayName(setup: LayerSetup | undefined | null, file: string): string {
  const own = setup?.names?.[file];
  return own ? own : standardLayerName(file);
}

/** `BOARD::GetLayerType`: what the user chose, else the layer's default (a copper layer is a signal layer, a user layer auxiliary). */
export function layerTypeOf(setup: LayerSetup | undefined | null, file: string): BoardLayerType {
  return setup?.types?.[file] ?? defaultLayerType(file);
}

export function defaultLayerType(file: string): BoardLayerType {
  return isCopperLayerName(file) ? "signal" : "auxiliary";
}

/** Whether the board has `file`: a copper layer of its stack, or an optional layer the page has on. */
export function layerIsEnabled(setup: LayerSetup | undefined | null, copper: readonly string[], file: string): boolean {
  return copper.includes(file) || enabledOptionalLayers(setup).includes(file);
}

// ------------------------------------------------------------------------------------------------------------------------------------ the page

/**
 * What the page edits: the layers that are checked and a name and a type for every layer it shows (a layer that was turned off keeps what it had, as its row does in KiCad's
 * dialog until it closes). `shown` is the user layers that have a row: those that are on, and the ones added since.
 */
export interface LayerDraft {
  enabled: string[];
  shown: string[];
  names: Record<string, string>;
  types: Record<string, BoardLayerType>;
}

/** The draft of a board's page: its layers, a name for each (the standard one where the user gave none) and a type for each copper and user layer. */
export function layerDraft(setup: LayerSetup | undefined | null, copper: readonly string[]): LayerDraft {
  const enabled = enabledOptionalLayers(setup);
  const shown = enabled.filter((l) => userLayerNumber(l) !== null);
  const names: Record<string, string> = {};
  const types: Record<string, BoardLayerType> = {};
  for (const l of [...copper, ...TECH_LAYER_NAMES, ...shown]) {
    names[l] = layerDisplayName(setup, l);
    if (isCopperLayerName(l) || userLayerNumber(l) !== null) types[l] = layerTypeOf(setup, l);
  }
  return { enabled, shown, names, types };
}

/** One row of the page. */
export interface LayerRow {
  /** The name in the board file. */
  layer: string;
  kind: "copper" | "tech" | "user";
  checked: boolean;
  /** The check box cannot be changed: a copper layer (the Physical Stackup page owns those) or one of the layers a board always has. */
  locked: boolean;
  /** Why it is locked. */
  lockedBecause?: string;
  name: string;
  /** The choices of the type list (copper and user layers), else none. */
  typeChoices: readonly BoardLayerType[];
  type: BoardLayerType | null;
  /** The static description of a technical layer. */
  note: string;
  tip: string;
}

/**
 * The rows in the order the dialog stacks them (`initialize_layers_controls`): the front technical layers, the copper layers front to back, the back technical layers, the outline
 * and the margin, the auxiliary layers, then the user layers that have a row.
 */
export function layerRows(draft: LayerDraft, copper: readonly string[]): LayerRow[] {
  const order: Array<[string, LayerRow["kind"]]> = [
    ...["F.CrtYd", "F.Fab", "F.Adhes", "F.Paste", "F.SilkS", "F.Mask"].map((l): [string, LayerRow["kind"]] => [l, "tech"]),
    ...copper.map((l): [string, LayerRow["kind"]] => [l, "copper"]),
    ...["B.Mask", "B.SilkS", "B.Paste", "B.Adhes", "B.Fab", "B.CrtYd", "Edge.Cuts", "Margin", "Eco1.User", "Eco2.User", "Cmts.User", "Dwgs.User"].map((l): [string, LayerRow["kind"]] => [l, "tech"]),
    ...[...new Set(draft.shown)]
      .filter((l) => userLayerNumber(l) !== null)
      .sort((a, b) => (userLayerNumber(a) ?? 0) - (userLayerNumber(b) ?? 0))
      .map((l): [string, LayerRow["kind"]] => [l, "user"]),
  ];
  return order.map(([layer, kind]) => {
    const mandatory = MANDATORY_LAYERS.includes(layer);
    return {
      layer,
      kind,
      checked: kind === "copper" || mandatory || draft.enabled.includes(layer),
      locked: kind === "copper" || mandatory,
      lockedBecause: kind === "copper" ? "Use the Physical Stackup page to change the number of copper layers." : mandatory ? "This layer is required and cannot be disabled." : undefined,
      name: draft.names[layer] ?? standardLayerName(layer),
      typeChoices: kind === "copper" ? COPPER_LAYER_TYPES : kind === "user" ? USER_LAYER_TYPES : [],
      type: kind === "tech" ? null : (draft.types[layer] ?? defaultLayerType(layer)),
      note: kind === "tech" ? (TECH_NOTE[layer] ?? "") : "",
      tip: kind === "copper" ? "Copper layer type for Freerouter and other external routers.\nPower plane layers are removed from Freerouter's layer menus." : kind === "user" ? "Auxiliary layers do not flip with board side, while back and front layers do." : (TECH_TIP[layer] ?? ""),
    };
  });
}

/** The user layers `addUserDefinedLayer` offers: the ones that have no row yet. */
export function availableUserLayers(draft: LayerDraft): string[] {
  const have = new Set(draft.shown);
  return Array.from({ length: MAX_USER_LAYERS }, (_, i) => userLayerName(i + 1)).filter((l) => !have.has(l));
}

/** `addUserDefinedLayer`: the layer gets a row, is checked, and is called what KiCad calls it. */
export function withUserLayer(draft: LayerDraft, layer: string): LayerDraft {
  if (userLayerNumber(layer) === null || draft.shown.includes(layer)) return draft;
  return {
    ...draft,
    enabled: canonicalEnabled([...draft.enabled, layer]),
    shown: [...draft.shown, layer],
    names: { ...draft.names, [layer]: draft.names[layer] ?? standardLayerName(layer) },
    types: { ...draft.types, [layer]: draft.types[layer] ?? "auxiliary" },
  };
}

/** A layer checked or unchecked. */
export function withLayerChecked(draft: LayerDraft, layer: string, checked: boolean): LayerDraft {
  if (MANDATORY_LAYERS.includes(layer) || isCopperLayerName(layer) || !isOptionalLayer(layer)) return draft;
  const without = draft.enabled.filter((l) => l !== layer);
  return { ...draft, enabled: canonicalEnabled(checked ? [...without, layer] : without) };
}

/** What `testLayerNames` and the backend refuse, per layer (the file's name), in the page's words; and a problem with the page as a whole. */
export interface LayerCheck {
  errors: Record<string, string>;
  page: string | null;
}

/** `wxFileName::GetForbiddenChars( wxPATH_DOS )` and `%`. */
export const FORBIDDEN_LAYER_NAME_CHARS = "*?\\/:\"<>|%";

/** `PANEL_SETUP_LAYERS::testLayerNames`: the layers the board has, walked in `PCB_LAYER_ID` order; the first problem of a layer is the one it is marked with. Every layer with one is marked. */
export function checkLayerDraft(draft: LayerDraft, copper: readonly string[]): LayerCheck {
  const errors: Record<string, string> = {};
  const have = [...copper, ...draft.enabled.filter((l) => isOptionalLayer(l))].sort((a, b) => pcbLayerId(a) - pcbLayerId(b));
  const used = new Set<string>();
  for (const layer of have) {
    const name = draft.names[layer] ?? standardLayerName(layer);
    let problem: string | null = null;
    if (name === "") problem = "Layer must have a name.";
    else if ([...name].some((c) => FORBIDDEN_LAYER_NAME_CHARS.includes(c) || /[\u0000-\u001f\u007f-\u009f]/.test(c))) problem = `${FORBIDDEN_LAYER_NAME_CHARS} are forbidden in layer names.`;
    else if (name === "signal") problem = 'Layer name "signal" is reserved.';
    else if (used.has(name)) problem = `Layer name '${name}' already in use.`;
    if (problem) errors[layer] = problem;
    else used.add(name);
  }
  for (const m of MANDATORY_LAYERS) if (!draft.enabled.includes(m)) errors[m] = "This layer is required and cannot be disabled.";
  return { errors, page: null };
}

/** The page a draft sends: the layers that are on, the names that are not the standard ones and the types that are not the default's, for the layers the board has. */
export function layerDraftToSend(draft: LayerDraft, copper: readonly string[]): LayerSetup {
  const enabled = canonicalEnabled([...draft.enabled, ...MANDATORY_LAYERS]);
  const have = [...copper, ...enabled];
  const names: Record<string, string> = {};
  const types: Record<string, BoardLayerType> = {};
  for (const l of have) {
    const name = draft.names[l];
    if (name !== undefined && name !== standardLayerName(l)) names[l] = name;
    const type = draft.types[l];
    if (type !== undefined && type !== defaultLayerType(l) && (isCopperLayerName(l) || userLayerNumber(l) !== null)) types[l] = type;
  }
  return { enabled, ...(Object.keys(names).length > 0 ? { names } : {}), ...(Object.keys(types).length > 0 ? { types } : {}) };
}

/** Two pages that mean the same thing (the order of the list and the entries that are the defaults are not part of the page). */
export function sameLayerSetup(a: LayerSetup | undefined | null, b: LayerSetup | undefined | null): boolean {
  const norm = (s: LayerSetup | undefined | null) => {
    const enabled = enabledOptionalLayers(s);
    const names = Object.entries(s?.names ?? {}).filter(([l, n]) => n !== "" && n !== standardLayerName(l)).sort(([x], [y]) => (x < y ? -1 : 1));
    const types = Object.entries(s?.types ?? {}).filter(([l, t]) => t !== defaultLayerType(l)).sort(([x], [y]) => (x < y ? -1 : 1));
    return JSON.stringify({ enabled, names, types });
  };
  return norm(a) === norm(b);
}

/** How many board-level items (graphics, texts, dimensions) sit on each layer: the layers a page may not turn off while they hold something (`getRemovedLayersWithItems`). */
export function itemsPerLayer(drawings: { shapes?: ReadonlyArray<{ layer: string }>; texts?: ReadonlyArray<{ layer: string }>; dimensions?: ReadonlyArray<{ layer: string }> } | null | undefined): Record<string, number> {
  const out: Record<string, number> = {};
  for (const list of [drawings?.shapes, drawings?.texts, drawings?.dimensions]) for (const item of list ?? []) out[item.layer] = (out[item.layer] ?? 0) + 1;
  return out;
}
