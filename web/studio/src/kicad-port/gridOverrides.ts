// Grid overrides: a grid of its own for each category of item, used instead of the editor's current grid while overrides are on (`common.Control.toggleGridOverrides`,
// Ctrl+Shift+G). A port of GRID_HELPER_GRIDS (include/tool/grid_helper.h), the GetItemGrid / GetGridSize / GetSelectionGrid of PCB_GRID_HELPER
// (pcbnew/tools/pcb_grid_helper.cpp) and EE_GRID_HELPER (eeschema/tools/ee_grid_helper.cpp), GRID_SETTINGS (include/settings/app_settings.h) with its defaults
// (common/settings/app_settings.cpp) and the Grid Overrides section of PANEL_GRID_SETTINGS (common/dialogs/panel_grid_settings.cpp), commit 8303b2ad.
//
//   category        what it is for                                                        PCB                       schematic
//   "connectable"   footprints and pads / symbols, pins, labels, sheets, no-connects      override_connected        override_connected
//   "wires"         tracks and arcs / wires, buses, junctions and bus entries             override_wires            override_wires
//   "vias"          vias                                                                  override_vias             (none)
//   "text"          text and fields                                                       override_text             override_text
//   "graphics"      shapes, dimensions, images / graphic shapes, text boxes, bitmaps      override_graphics         override_graphics
//   "current"       everything else, and any category whose override is off: the editor's own current grid
//
// An override names a grid by its position in the editor's grid list (`override_*_idx`); a position outside the list means "the current grid" (`GetGridSize` only
// takes `grids[idx]` when `idx >= 0 && idx < grids.size()`).
//
// Units: the studio's um.

/** `GRID_HELPER_GRIDS`. */
export type GridCategory = "current" | "connectable" | "wires" | "vias" | "text" | "graphics";

/** The categories that can be overridden, in the order the Grids page lists them. */
export const OVERRIDE_CATEGORIES: readonly Exclude<GridCategory, "current">[] = ["connectable", "wires", "vias", "text", "graphics"];

/** `GRID_SETTINGS::overrides_enabled` and the five `override_*` / `override_*_idx` pairs. */
export interface GridOverrides {
  /** The master switch the toolbar button and Ctrl+Shift+G toggle (`EDA_DRAW_FRAME::SetGridOverrides`). */
  enabled: boolean;
  connectable: { on: boolean; index: number };
  wires: { on: boolean; index: number };
  vias: { on: boolean; index: number };
  text: { on: boolean; index: number };
  graphics: { on: boolean; index: number };
}

/** The editors whose overrides differ: eeschema and the symbol editor have one set of defaults, pcbnew and the footprint editor the other. */
export type OverrideFamily = "pcb" | "schematic";

/**
 * `APP_SETTINGS_BASE::addParamsForWindow`, "for grid overrides, give just the schematic and symbol editors sane values": connected items and wires on the 50 mil grid
 * (index 1 of 100 / 50 / 25 / 10 mil), text on 10 mil (index 3), graphics on 25 mil (index 2, off), vias off. The board editors start with every override off, pointing at
 * entries of the default PCB list: connected 16 (0.25 mm), wires 19 (0.05 mm), vias 18 (0.1 mm), text 18 (0.1 mm), graphics 15 (0.5 mm).
 */
export function defaultGridOverrides(family: OverrideFamily): GridOverrides {
  if (family === "schematic") {
    return {
      enabled: true,
      connectable: { on: true, index: 1 },
      wires: { on: true, index: 1 },
      vias: { on: false, index: 0 },
      text: { on: true, index: 3 },
      graphics: { on: false, index: 2 },
    };
  }
  return {
    enabled: true,
    connectable: { on: false, index: 16 },
    wires: { on: false, index: 19 },
    vias: { on: false, index: 18 },
    text: { on: false, index: 18 },
    graphics: { on: false, index: 15 },
  };
}

/**
 * `PCB_GRID_HELPER::GetGridSize` / `EE_GRID_HELPER::GetGridSize`: the grid for a category -- its override's grid when overrides are on, the category's is on and its index is
 * in the list, else the current grid. (The schematic helper has no vias category; asking for one gives the current grid there too since its default is off.)
 */
export function gridSizeFor(category: GridCategory, current: number, grids: readonly number[], overrides: GridOverrides | null | undefined): number {
  if (!overrides || !overrides.enabled || category === "current") return current;
  const o = overrides[category];
  if (o.on && o.index >= 0 && o.index < grids.length) return grids[o.index]!;
  return current;
}

/** True when `category` is overridden right now. */
export function isOverridden(category: GridCategory, grids: readonly number[], overrides: GridOverrides | null | undefined): boolean {
  if (!overrides || !overrides.enabled || category === "current") return false;
  const o = overrides[category];
  return o.on && o.index >= 0 && o.index < grids.length;
}

/**
 * `GRID_HELPER::GetSelectionGrid`: "the largest grid of all the items" -- the category of the first item, then any whose grid is coarser (strictly greater). An empty
 * selection is the current grid.
 */
export function selectionGrid(categories: readonly GridCategory[], size: (category: GridCategory) => number): GridCategory {
  if (categories.length === 0) return "current";
  let best = categories[0]!;
  for (const c of categories) if (size(c) > size(best)) best = c;
  return best;
}

// ----------------------------------------------------------------------------------------------------------------------------------------- item kinds

/** What the board editor's grid helper calls an item (`PCB_GRID_HELPER::GetItemGrid`: by `Type()`). */
export type PcbItemKind = "footprint" | "pad" | "track" | "via" | "shape" | "dimension" | "text" | "field" | "zone" | "group" | "other";

/** `PCB_GRID_HELPER::GetItemGrid`. */
export function pcbItemGrid(kind: PcbItemKind | null | undefined): GridCategory {
  switch (kind) {
    case "footprint":
    case "pad":
      return "connectable";
    // A footprint's field (`PCB_FIELD_T`) is text.
    case "text":
    case "field":
      return "text";
    case "shape":
    case "dimension":
      return "graphics";
    case "track":
      return "wires";
    case "via":
      return "vias";
    default:
      return "current";
  }
}

/** What the schematic editor's grid helper calls an item (`EE_GRID_HELPER::GetItemGrid`: by `Type()`). */
export type SchItemKind =
  | "symbol"
  | "pin"
  | "sheet"
  | "sheet_pin"
  | "no_connect"
  | "label"
  | "global_label"
  | "hier_label"
  | "directive_label"
  | "rule_area"
  | "field"
  | "text"
  | "shape"
  | "text_box"
  | "bitmap"
  | "junction"
  | "wire"
  | "bus"
  | "graphic_line"
  | "bus_entry"
  | "other";

/** `EE_GRID_HELPER::GetItemGrid`: a wire or bus is on the wire grid, a graphic line on the graphics grid. */
export function schItemGrid(kind: SchItemKind | null | undefined): GridCategory {
  switch (kind) {
    case "symbol":
    case "pin":
    case "sheet_pin":
    case "sheet":
    case "no_connect":
    case "global_label":
    case "hier_label":
    case "label":
    case "directive_label":
    case "rule_area":
      return "connectable";
    case "field":
    case "text":
      return "text";
    case "shape":
    case "text_box":
    case "bitmap":
    case "graphic_line":
      return "graphics";
    case "junction":
    case "wire":
    case "bus":
    case "bus_entry":
      return "wires";
    default:
      return "current";
  }
}

// ----------------------------------------------------------------------------------------------------------------------------------------- the page

/** The rows the Grids page shows for an editor, with the label `PANEL_GRID_SETTINGS`' constructor gives each (a row it hides is not listed). */
export function overrideRows(editor: "pcb" | "footprint" | "schematic" | "symbol"): { category: Exclude<GridCategory, "current">; label: string }[] {
  switch (editor) {
    case "pcb":
      return [
        { category: "connectable", label: "Footprints/pads" },
        { category: "wires", label: "Tracks" },
        { category: "vias", label: "Vias" },
        { category: "text", label: "Text" },
        { category: "graphics", label: "Graphics" },
      ];
    case "footprint":
      return [
        { category: "connectable", label: "Pads" },
        { category: "text", label: "Text" },
        { category: "graphics", label: "Graphics" },
      ];
    default:
      return [
        { category: "connectable", label: "Connected items" },
        { category: "wires", label: "Wires" },
        { category: "text", label: "Text" },
        { category: "graphics", label: "Graphics" },
      ];
  }
}

/** Keeps an override's grid when the list is rebuilt: found again by size (`RebuildGridSizes` keeps the string selection), else the first entry. */
export function rebuildOverrides(old: GridOverrides, oldGrids: readonly number[], newGrids: readonly number[], same: (a: number, b: number) => boolean): GridOverrides {
  const keep = (index: number): number => {
    const at = newGrids.findIndex((g) => oldGrids[index] !== undefined && same(g, oldGrids[index]!));
    return at >= 0 ? at : 0;
  };
  const next = { ...old };
  for (const c of OVERRIDE_CATEGORIES) next[c] = { on: old[c].on, index: keep(old[c].index) };
  return next;
}

/** `safeGrid`: an index outside the list is the first grid. */
export function safeOverrideIndex(index: number, size: number): number {
  return Number.isFinite(index) && index >= 0 && index < size ? Math.trunc(index) : 0;
}

/** Overrides read back from storage (`null`, or the wrong shape: the family's defaults), every field checked. */
export function parseGridOverrides(raw: unknown, family: OverrideFamily, size: number): GridOverrides {
  const d = defaultGridOverrides(family);
  const o = raw as Partial<Record<string, unknown>> | null;
  if (!o || typeof o !== "object") return d;
  const pick = (key: Exclude<GridCategory, "current">): { on: boolean; index: number } => {
    const v = o[key] as { on?: unknown; index?: unknown } | undefined;
    return { on: typeof v?.on === "boolean" ? v.on : d[key].on, index: typeof v?.index === "number" ? safeOverrideIndex(v.index, size) : d[key].index };
  };
  return { enabled: typeof o.enabled === "boolean" ? o.enabled : d.enabled, connectable: pick("connectable"), wires: pick("wires"), vias: pick("vias"), text: pick("text"), graphics: pick("graphics") };
}

/** Two override sets are the same. */
export function sameOverrides(a: GridOverrides, b: GridOverrides): boolean {
  return a.enabled === b.enabled && OVERRIDE_CATEGORIES.every((c) => a[c].on === b[c].on && a[c].index === b[c].index);
}
