// The grid lists of the editors, and the fast grids (`common.Control.editGrids`, "Edit Grids..."): the "Grids" page of KiCad's Preferences. Ported from
// common/dialogs/panel_grid_settings.cpp (the list of grids with its Add / Edit / Remove / Move buttons, Fast Grid 1 and 2, Reset to Defaults),
// dialog_grid_settings.cpp (a size is checked against 0.001 .. 1000 mm) and common/settings/app_settings.cpp (`DefaultGridSizeList`, the default
// `fast_grid_1` / `fast_grid_2`: the default grid and the one after it), commit 8303b2ad. Pure: state/gridSettings.ts keeps them, the dialog edits them and the
// toolbars' grid boxes, the grid actions and the fast grid hotkeys read them.
//
// Not here: a grid's name, a grid with different X and Y sizes (the studio snaps to one square grid, so the dialog asks for one size) and the grid overrides
// (connected items, wires, vias, text and graphics each get a grid of their own -- which is also why `common.Control.toggleGridOverrides` has a reason in
// tools/ui-parity-missing.json).
import { DEFAULT_PCB_GRIDS_UM } from "./grid";
import { DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2 } from "./cursorControl";

/** The editors whose grid is a choice from a list: the board editor, the Footprint Editor and the Symbol Editor (the schematic's is fixed at 50 mil). */
export type GridEditor = "pcb" | "footprint" | "symbol";

export const GRID_EDITORS: readonly GridEditor[] = ["pcb", "footprint", "symbol"];

/** The page of KiCad's Preferences tree each editor's grids are on. */
export const GRID_EDITOR_LABEL: Record<GridEditor, string> = { pcb: "PCB Editor", footprint: "Footprint Editor", symbol: "Symbol Editor" };

/** The editor whose grid list a tab uses, or null for the tabs without one (the schematic's grid is the fixed 50 mil; the 3D viewer has none). */
export function gridEditorOfTab(tab: string): GridEditor | null {
  return tab === "pcb" || tab === "footprint" || tab === "symbol" ? tab : null;
}

export interface GridSettings {
  /** The grid sizes, um, in the order the list shows them (not sorted: KiCad's own default list jumps from 1 mil to 5 mm). */
  grids: readonly number[];
  /** `fast_grid_1` / `fast_grid_2`: indexes into `grids` of the two grids the fast grid hotkeys switch between. */
  fast1: number;
  fast2: number;
}

/** `DefaultGridSizeList()` of the eeschema and symbol editor settings: 100, 50, 25 and 10 mil. */
export const EESCHEMA_GRIDS_UM: readonly number[] = [2540, 1270, 635, 254];

/** `DIALOG_GRID_SETTINGS::TransferDataFromWindow`: `Validate( 0.001, 1000.0, EDA_UNITS::MM )`. */
export const GRID_MIN_UM = 1;
export const GRID_MAX_UM = 1_000_000;

/** The settings an editor has until its grids are edited: the PCB list for the board and footprint editors (`defaultGridIdx` 15 -> fast grids 15 and 16), the eeschema list for the symbol editor (index 1, 50 mil -> 1 and 2). */
export function defaultGridSettings(editor: GridEditor): GridSettings {
  return editor === "symbol" ? { grids: EESCHEMA_GRIDS_UM, fast1: 1, fast2: 2 } : { grids: DEFAULT_PCB_GRIDS_UM, fast1: DEFAULT_FAST_GRID_1, fast2: DEFAULT_FAST_GRID_2 };
}

const sameSize = (a: number | undefined, b: number | undefined) => a !== undefined && b !== undefined && Math.abs(a - b) < 1e-9;

/** `TransferDataToWindow`'s `safeGrid`: an index outside the list is the first grid; the list itself is never empty. */
export function normalizeGridSettings(s: GridSettings): GridSettings {
  const grids = s.grids.length > 0 ? s.grids : defaultGridSettings("pcb").grids;
  const safe = (i: number) => (Number.isFinite(i) && i >= 0 && i < grids.length ? Math.trunc(i) : 0);
  return { grids, fast1: safe(s.fast1), fast2: safe(s.fast2) };
}

/** A message when a saved list cannot be used: it needs a grid, every size above zero. */
export function gridListError(grids: readonly number[]): string | null {
  if (grids.length === 0) return "There must be at least one grid.";
  if (grids.some((g) => !Number.isFinite(g) || g <= 0)) return "Every grid needs a size above zero.";
  return null;
}

/**
 * `RebuildGridSizes` after the list changed: a fast grid stays on the grid it named (found again by its size); one whose grid is gone goes to the first
 * entry (Grid 1) or the last (Grid 2).
 */
function rebuilt(old: GridSettings, grids: readonly number[]): GridSettings {
  const keep = (idx: number, fallback: number) => {
    const at = grids.findIndex((g) => sameSize(g, old.grids[idx]));
    return at >= 0 ? at : fallback;
  };
  return { grids, fast1: keep(old.fast1, 0), fast2: keep(old.fast2, grids.length - 1) };
}

/** What an edit of the list gives: the new settings and the row selected afterwards, or why the size was refused. */
export type GridEdit = { ok: true; settings: GridSettings; row: number } | { ok: false; error: "range" | "duplicate" };

function sizeError(um: number): "range" | null {
  return Number.isFinite(um) && um >= GRID_MIN_UM && um <= GRID_MAX_UM ? null : "range";
}

/** `OnAddGrid`: the new grid goes in before the selected row, which becomes the new grid. A size that is in the list already is refused ("Grid size '%s' already exists."). */
export function insertGrid(s: GridSettings, row: number, um: number): GridEdit {
  const bad = sizeError(um);
  if (bad) return { ok: false, error: bad };
  if (s.grids.some((g) => sameSize(g, um))) return { ok: false, error: "duplicate" };
  const at = Math.max(0, Math.min(s.grids.length, Math.trunc(row)));
  const grids = [...s.grids.slice(0, at), um, ...s.grids.slice(at)];
  return { ok: true, settings: rebuilt(s, grids), row: at };
}

/** `onEditGrid`: the selected grid gets a new size (unchanged is not an error). A fast grid that pointed at the old size goes back to the first / last entry. */
export function replaceGrid(s: GridSettings, row: number, um: number): GridEdit {
  const bad = sizeError(um);
  if (bad) return { ok: false, error: bad };
  if (row < 0 || row >= s.grids.length) return { ok: false, error: "range" };
  if (sameSize(s.grids[row], um)) return { ok: true, settings: s, row };
  if (s.grids.some((g, i) => i !== row && sameSize(g, um))) return { ok: false, error: "duplicate" };
  const grids = s.grids.map((g, i) => (i === row ? um : g));
  return { ok: true, settings: rebuilt(s, grids), row };
}

/** `OnRemoveGrid`: the last grid cannot be removed; the row above the removed one is selected (the first stays the first). */
export function removeGrid(s: GridSettings, row: number): { settings: GridSettings; row: number } {
  if (s.grids.length <= 1 || row < 0 || row >= s.grids.length) return { settings: s, row };
  return { settings: rebuilt(s, s.grids.filter((_, i) => i !== row)), row: row === 0 ? 0 : row - 1 };
}

/** `OnMoveGridUp` (`delta` -1) / `OnMoveGridDown` (+1): the grid swaps places with its neighbour, and the selection goes with it. */
export function moveGrid(s: GridSettings, row: number, delta: -1 | 1): { settings: GridSettings; row: number } {
  const to = row + delta;
  if (s.grids.length <= 1 || row < 0 || row >= s.grids.length || to < 0 || to >= s.grids.length) return { settings: s, row };
  const grids = [...s.grids];
  [grids[row], grids[to]] = [grids[to]!, grids[row]!];
  return { settings: rebuilt(s, grids), row: to };
}

/** `ResetPanel`: the editor's default list; the fast grids keep their size if the default list has it, and go to the first / last entry if not. */
export function resetGrids(s: GridSettings, editor: GridEditor): GridSettings {
  return rebuilt(s, defaultGridSettings(editor).grids);
}

/**
 * A grid size typed in the dialog: a number, in `unit` (mm, mil or in) unless it carries its own ("0.25 mm", "10 mil", "0.1in"), as um -- or null when
 * it is not a number. A comma works as the decimal point. Whether it is a size KiCad accepts is `insertGrid`'s question.
 */
export function parseGridSize(text: string, unit: "mm" | "mil" | "in"): number | null {
  const m = /^\s*([0-9]*[.,]?[0-9]+(?:e[+-]?\d+)?)\s*(mm|mil|mils|in|inch|")?\s*$/i.exec(text);
  if (!m) return null;
  const value = Number(m[1]!.replace(",", "."));
  const u = (m[2] ?? unit).toLowerCase();
  const um = u === "mm" ? value * 1000 : u === "in" || u === "inch" || u === '"' ? value * 25_400 : value * 25.4;
  return Number.isFinite(um) ? Math.round(um * 1e6) / 1e6 : null;
}

/** Reads one editor's saved settings back (`null`, nothing saved or damaged: the defaults), checked the way the dialog checks them. */
export function parseGridSettings(raw: unknown, editor: GridEditor): GridSettings {
  const fallback = defaultGridSettings(editor);
  const o = raw as Partial<{ grids: unknown; fast1: unknown; fast2: unknown }> | null;
  if (!o || typeof o !== "object" || !Array.isArray(o.grids)) return fallback;
  const grids = o.grids.filter((g): g is number => typeof g === "number");
  if (gridListError(grids) !== null) return fallback;
  return normalizeGridSettings({ grids, fast1: typeof o.fast1 === "number" ? o.fast1 : fallback.fast1, fast2: typeof o.fast2 === "number" ? o.fast2 : fallback.fast2 });
}

/** The settings of every editor from what the browser kept (`null`, or text that is not JSON: all defaults). */
export function parseStoredGridSettings(text: string | null): Record<GridEditor, GridSettings> {
  let saved: Record<string, unknown> | null = null;
  try {
    saved = text ? (JSON.parse(text) as Record<string, unknown>) : null;
  } catch {
    saved = null;
  }
  return {
    pcb: parseGridSettings(saved?.pcb, "pcb"),
    footprint: parseGridSettings(saved?.footprint, "footprint"),
    symbol: parseGridSettings(saved?.symbol, "symbol"),
  };
}

/** True when the settings are the defaults of the editor (nothing to save). */
export function isDefaultGridSettings(s: GridSettings, editor: GridEditor): boolean {
  const d = defaultGridSettings(editor);
  return s.fast1 === d.fast1 && s.fast2 === d.fast2 && s.grids.length === d.grids.length && s.grids.every((g, i) => g === d.grids[i]);
}

/** `GridNext` / `GridPrev`: the grid after / before the current one in the list, wrapping round; a current grid that is not in the list goes to the first / last. */
export function stepGrid(grids: readonly number[], currentUm: number, dir: 1 | -1): number {
  const at = grids.findIndex((g) => sameSize(g, currentUm));
  const next = at + dir;
  return grids[next < 0 ? grids.length - 1 : next >= grids.length ? 0 : next]!;
}
