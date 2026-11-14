// The grid lists of the editors, and the fast grids (`common.Control.editGrids`, "Edit Grids..."): the "Grids" page of KiCad's Preferences. Ported from
// common/dialogs/panel_grid_settings.cpp (the list of grids with its Add / Edit / Remove / Move buttons, Fast Grid 1 and 2, Reset to Defaults),
// dialog_grid_settings.cpp (a size is checked against 0.001 .. 1000 mm) and common/settings/app_settings.cpp (`DefaultGridSizeList`, the default
// `fast_grid_1` / `fast_grid_2`: the default grid and the one after it), commit 8303b2ad. Pure: state/gridSettings.ts keeps them, the dialog edits them and the
// toolbars' grid boxes, the grid actions and the fast grid hotkeys read them.
//
// A grid is KiCad's `GRID` (include/settings/grid_settings.h): an optional name and an X and a Y size (`DIALOG_GRID_SETTINGS`: "Name:", "X:", "Linked", "Y:"); two
// grids are the same when name, X and Y all are (`GRID::operator==`), and the list shows "name: X x Y (in the other units)" (`RebuildGridSizes`, `GRID::MessageText`,
// which says one size for a square grid). The X sizes stay `grids` -- what the grid boxes, the actions and the snapping read as the grid -- and the names and the Y
// sizes ride beside them (`names`, `ys`), so a square, unnamed list is what it always was. The grid overrides (connected items, wires, vias, text and graphics each get a
// grid of their own) are kicad-port/gridOverrides.ts, with their own state/gridOverrides.ts.
import { DEFAULT_PCB_GRIDS_NM } from "./grid";
import { DEFAULT_FAST_GRID_1, DEFAULT_FAST_GRID_2 } from "./cursorControl";

/** The editors whose grid is a choice from a list: the board editor, the Schematic Editor, the Footprint Editor and the Symbol Editor. */
export type GridEditor = "pcb" | "footprint" | "symbol" | "schematic";

export const GRID_EDITORS: readonly GridEditor[] = ["pcb", "footprint", "schematic", "symbol"];

/** The page of KiCad's Preferences tree each editor's grids are on. */
export const GRID_EDITOR_LABEL: Record<GridEditor, string> = { pcb: "PCB Editor", footprint: "Footprint Editor", symbol: "Symbol Editor", schematic: "Schematic Editor" };

/** The editor whose grid list a tab uses, or null for the tabs without one (the 3D viewer has none). */
export function gridEditorOfTab(tab: string): GridEditor | null {
  return tab === "pcb" || tab === "footprint" || tab === "symbol" || tab === "schematic" ? tab : null;
}

/** The editors whose grid overrides start the way eeschema's do (`APP_SETTINGS_BASE::addParamsForWindow`: eeschema and the symbol editor); the others start as pcbnew's. */
export const SCHEMATIC_FAMILY: readonly GridEditor[] = ["schematic", "symbol"];

export interface GridSettings {
  /** The grids' X sizes, nm, in the order the list shows them (not sorted: KiCad's own default list jumps from 1 mil to 5 mm). */
  grids: readonly number[];
  /** The grids' Y sizes, beside `grids`; absent (or an entry missing) where a grid is square. */
  ys?: readonly number[];
  /** The grids' names (`GRID::name`, optional), beside `grids`; absent where none is named. */
  names?: readonly string[];
  /** `fast_grid_1` / `fast_grid_2`: indexes into `grids` of the two grids the fast grid hotkeys switch between. */
  fast1: number;
  fast2: number;
}

/** `DefaultGridSizeList()` of the eeschema and symbol editor settings: 100, 50, 25 and 10 mil. */
export const EESCHEMA_GRIDS_NM: readonly number[] = [2_540_000, 1_270_000, 635_000, 254_000];

/** `DIALOG_GRID_SETTINGS::TransferDataFromWindow`: `Validate( 0.001, 1000.0, EDA_UNITS::MM )`. */
export const GRID_MIN_NM = 1_000;
export const GRID_MAX_NM = 1_000_000_000;

/**
 * The settings an editor has until its grids are edited: the PCB list for the board and footprint editors (`defaultGridIdx` 15 -> fast grids 15 and 16), the eeschema list for
 * the schematic and the symbol editor (index 1, 50 mil -> 1 and 2).
 */
export function defaultGridSettings(editor: GridEditor): GridSettings {
  return SCHEMATIC_FAMILY.includes(editor) ? { grids: EESCHEMA_GRIDS_NM, fast1: 1, fast2: 2 } : { grids: DEFAULT_PCB_GRIDS_NM, fast1: DEFAULT_FAST_GRID_1, fast2: DEFAULT_FAST_GRID_2 };
}

const sameSize = (a: number | undefined, b: number | undefined) => a !== undefined && b !== undefined && Math.abs(a - b) < 1e-9;

/** One grid of the list: KiCad's `GRID` (`name`, `x`, `y`), sizes in nm. */
export interface GridDef {
  name: string;
  x: number;
  y: number;
}

/** A square, unnamed grid of `nm`. */
export const squareGrid = (nm: number): GridDef => ({ name: "", x: nm, y: nm });

/** `GRID::operator==`: the same name, X and Y. */
export function sameGrid(a: GridDef | undefined, b: GridDef | undefined): boolean {
  return a !== undefined && b !== undefined && a.name === b.name && sameSize(a.x, b.x) && sameSize(a.y, b.y);
}

/** The grid at `i` of the list. */
export function gridAt(s: GridSettings, i: number): GridDef | undefined {
  const x = s.grids[i];
  if (x === undefined) return undefined;
  return { name: s.names?.[i] ?? "", x, y: s.ys?.[i] ?? x };
}

/** The whole list as grids. */
export function gridDefs(s: GridSettings): GridDef[] {
  return s.grids.map((_, i) => gridAt(s, i)!);
}

/** Settings holding `defs` (the fast grids as given): `ys` and `names` only when a grid is not square or has a name. */
export function withGrids(s: GridSettings, defs: readonly GridDef[], fast1 = s.fast1, fast2 = s.fast2): GridSettings {
  const out: GridSettings = { grids: defs.map((d) => d.x), fast1, fast2 };
  if (defs.some((d) => !sameSize(d.x, d.y))) out.ys = defs.map((d) => d.y);
  if (defs.some((d) => d.name !== "")) out.names = defs.map((d) => d.name);
  return out;
}

/**
 * The row of the editor's current grid (`GRID_SETTINGS::last_size_idx`, common/settings/app_settings.cpp): the editors keep their grid's X size (`gridNm`) and the
 * row last chosen (`lastIdx`, state/gridSettings.ts), so of two grids with the same X the one chosen stays current (UI-LEFT-3 C3-6). A row whose X is no longer the
 * current size (the list was edited, the size set some other way) gives way to the first grid of that X; -1 for a size the list does not have.
 */
export function currentGridIndex(grids: readonly number[], xNm: number, lastIdx?: number): number {
  if (lastIdx !== undefined && Number.isInteger(lastIdx) && sameSize(grids[lastIdx], xNm)) return lastIdx;
  return grids.findIndex((g) => sameSize(g, xNm));
}

/** The Y size of the editor's current grid (`currentGridIndex`): its row's Y, and the X again for a size the list does not have. */
export function gridYOf(s: GridSettings, xNm: number, lastIdx?: number): number {
  const at = currentGridIndex(s.grids, xNm, lastIdx);
  return at >= 0 ? (s.ys?.[at] ?? xNm) : xNm;
}

/** `GRID::MessageText` with `format` the display of one length: one size for a square grid, else "X x Y". */
export function gridMessageText(g: GridDef, format: (nm: number) => string): string {
  const xs = format(g.x);
  const ys = format(g.y);
  return xs === ys ? xs : `${xs} x ${ys}`;
}

/** `RebuildGridSizes`: "%s%s (%s)" -- the name and ": " when it has one, the grid in the display unit, and in brackets in the other system's unit. */
export function gridListLabel(g: GridDef, primary: (nm: number) => string, secondary: (nm: number) => string): string {
  return `${g.name ? `${g.name}: ` : ""}${gridMessageText(g, primary)} (${gridMessageText(g, secondary)})`;
}

/** `TransferDataToWindow`'s `safeGrid`: an index outside the list is the first grid; the list itself is never empty. */
export function normalizeGridSettings(s: GridSettings): GridSettings {
  const defs = s.grids.length > 0 ? gridDefs(s) : defaultGridSettings("pcb").grids.map(squareGrid);
  const safe = (i: number) => (Number.isFinite(i) && i >= 0 && i < defs.length ? Math.trunc(i) : 0);
  return withGrids(s, defs, safe(s.fast1), safe(s.fast2));
}

/** A message when a saved list cannot be used: it needs a grid, every size above zero. */
export function gridListError(grids: readonly number[]): string | null {
  if (grids.length === 0) return "There must be at least one grid.";
  if (grids.some((g) => !Number.isFinite(g) || g <= 0)) return "Every grid needs a size above zero.";
  return null;
}

/**
 * `RebuildGridSizes` after the list changed: a fast grid stays on the grid it named (found again by its name and sizes, as the choice keeps its string); one whose
 * grid is gone goes to the first entry (Grid 1) or the last (Grid 2).
 */
function rebuilt(old: GridSettings, defs: readonly GridDef[]): GridSettings {
  const keep = (idx: number, fallback: number) => {
    const was = gridAt(old, idx);
    const at = defs.findIndex((g) => sameGrid(g, was));
    return at >= 0 ? at : fallback;
  };
  return withGrids(old, defs, keep(old.fast1, 0), keep(old.fast2, defs.length - 1));
}

/**
 * What an edit of the list gives: the new settings and the row selected afterwards, or why the grid was refused -- `range` / `range_y` "Grid size X / Y out of range."
 * (`DIALOG_GRID_SETTINGS::TransferDataFromWindow`), `duplicate` "Grid size '%s' already exists." (`OnAddGrid`, `onEditGrid`).
 */
export type GridEdit = { ok: true; settings: GridSettings; row: number } | { ok: false; error: "range" | "range_y" | "duplicate" };

const inRange = (nm: number) => Number.isFinite(nm) && nm >= GRID_MIN_NM && nm <= GRID_MAX_NM;

/** `m_gridSizeX.Validate( 0.001, 1000.0, EDA_UNITS::MM )`, then the same for Y when it is not linked. */
function sizeError(g: GridDef): "range" | "range_y" | null {
  if (!inRange(g.x)) return "range";
  if (!inRange(g.y)) return "range_y";
  return null;
}

const asGrid = (g: GridDef | number): GridDef => (typeof g === "number" ? squareGrid(g) : g);

/** `OnAddGrid`: the new grid goes in before the selected row, which becomes the new grid. A grid that is in the list already is refused ("Grid size '%s' already exists."). */
export function insertGrid(s: GridSettings, row: number, grid: GridDef | number): GridEdit {
  const g = asGrid(grid);
  const bad = sizeError(g);
  if (bad) return { ok: false, error: bad };
  const defs = gridDefs(s);
  if (defs.some((d) => sameGrid(d, g))) return { ok: false, error: "duplicate" };
  const at = Math.max(0, Math.min(defs.length, Math.trunc(row)));
  return { ok: true, settings: rebuilt(s, [...defs.slice(0, at), g, ...defs.slice(at)]), row: at };
}

/** `onEditGrid`: the selected grid becomes `grid` ("If the user just clicked OK without changing anything", nothing happens). A fast grid that named the old grid goes to the first / last entry. */
export function replaceGrid(s: GridSettings, row: number, grid: GridDef | number): GridEdit {
  const g = asGrid(grid);
  const bad = sizeError(g);
  if (bad) return { ok: false, error: bad };
  if (row < 0 || row >= s.grids.length) return { ok: false, error: "range" };
  const defs = gridDefs(s);
  if (sameGrid(defs[row], g)) return { ok: true, settings: s, row };
  if (defs.some((d) => sameGrid(d, g))) return { ok: false, error: "duplicate" };
  return { ok: true, settings: rebuilt(s, defs.map((d, i) => (i === row ? g : d))), row };
}

/** `OnRemoveGrid`: the last grid cannot be removed; the row above the removed one is selected (the first stays the first). */
export function removeGrid(s: GridSettings, row: number): { settings: GridSettings; row: number } {
  if (s.grids.length <= 1 || row < 0 || row >= s.grids.length) return { settings: s, row };
  return { settings: rebuilt(s, gridDefs(s).filter((_, i) => i !== row)), row: row === 0 ? 0 : row - 1 };
}

/** `OnMoveGridUp` (`delta` -1) / `OnMoveGridDown` (+1): the grid swaps places with its neighbour, and the selection goes with it. */
export function moveGrid(s: GridSettings, row: number, delta: -1 | 1): { settings: GridSettings; row: number } {
  const to = row + delta;
  if (s.grids.length <= 1 || row < 0 || row >= s.grids.length || to < 0 || to >= s.grids.length) return { settings: s, row };
  const defs = gridDefs(s);
  [defs[row], defs[to]] = [defs[to]!, defs[row]!];
  return { settings: rebuilt(s, defs), row: to };
}

/** `ResetPanel`: the editor's default list; the fast grids keep their size if the default list has it, and go to the first / last entry if not. */
export function resetGrids(s: GridSettings, editor: GridEditor): GridSettings {
  return rebuilt(s, defaultGridSettings(editor).grids.map(squareGrid));
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
  const um = u === "mm" ? value * 1e6 : u === "in" || u === "inch" || u === '"' ? value * 25_400_000 : value * 25_400;
  return Number.isFinite(um) ? Math.round(um * 1e3) / 1e3 : null;
}

/** Reads one editor's saved settings back (`null`, nothing saved or damaged: the defaults), checked the way the dialog checks them. */
export function parseGridSettings(raw: unknown, editor: GridEditor): GridSettings {
  const fallback = defaultGridSettings(editor);
  const o = raw as Partial<{ grids: unknown; ys: unknown; names: unknown; fast1: unknown; fast2: unknown }> | null;
  if (!o || typeof o !== "object" || !Array.isArray(o.grids)) return fallback;
  const grids = o.grids.filter((g): g is number => typeof g === "number");
  if (gridListError(grids) !== null) return fallback;
  // the Y sizes and names beside them: with an entry dropped, a list of the wrong length or a bad value, the grids are read square and unnamed
  const whole = grids.length === o.grids.length;
  const ys = whole && Array.isArray(o.ys) && o.ys.length === grids.length && o.ys.every((y) => typeof y === "number" && Number.isFinite(y) && y > 0) ? (o.ys as number[]) : undefined;
  const names = whole && Array.isArray(o.names) && o.names.length === grids.length && o.names.every((n) => typeof n === "string") ? (o.names as string[]) : undefined;
  return normalizeGridSettings({ grids, ys, names, fast1: typeof o.fast1 === "number" ? o.fast1 : fallback.fast1, fast2: typeof o.fast2 === "number" ? o.fast2 : fallback.fast2 });
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
    schematic: parseGridSettings(saved?.schematic, "schematic"),
  };
}

/** True when the settings are the defaults of the editor (nothing to save). */
export function isDefaultGridSettings(s: GridSettings, editor: GridEditor): boolean {
  const d = defaultGridSettings(editor);
  return s.fast1 === d.fast1 && s.fast2 === d.fast2 && s.grids.length === d.grids.length && gridDefs(s).every((g, i) => sameGrid(g, squareGrid(d.grids[i]!)));
}

/** `GridNext` / `GridPrev`: the row after / before the current one (`currentGridIndex`) in the list, wrapping round; a current grid that is not in the list goes to the first / last. */
export function stepGridIndex(grids: readonly number[], currentNm: number, dir: 1 | -1, lastIdx?: number): number {
  const at = currentGridIndex(grids, currentNm, lastIdx);
  const next = at + dir;
  return next < 0 ? grids.length - 1 : next >= grids.length ? 0 : next;
}

/** `stepGridIndex`'s grid size. */
export function stepGrid(grids: readonly number[], currentNm: number, dir: 1 | -1, lastIdx?: number): number {
  return grids[stepGridIndex(grids, currentNm, dir, lastIdx)]!;
}

/**
 * `GRID_MENU::update` / `BuildChoiceList` (the context menus' Grid submenu, the toolbar's grid box): each grid as `gridListLabel` says, the editor's current one checked --
 * `last_size_idx` (see `currentGridIndex`).
 */
export function gridMenuEntries(s: GridSettings, currentNm: number, primary: (nm: number) => string, secondary: (nm: number) => string, lastIdx?: number): { label: string; checked: boolean }[] {
  const current = currentGridIndex(s.grids, currentNm, lastIdx);
  return gridDefs(s).map((g, i) => ({ label: gridListLabel(g, primary, secondary), checked: i === current }));
}
