// The Grids pages of KiCad's Preferences (`PANEL_GRID_SETTINGS`, common/dialogs/panel_grid_settings.cpp), one per editor: the PCB Editor, the Footprint Editor, the
// Schematic Editor and the Symbol Editor each have their own list, and OK keeps all of them (state/gridSettings.ts) and makes the row selected on each page the current
// grid of that editor (`last_size_idx`). "Edit Grids..." (`ACTIONS::gridProperties`, `COMMON_TOOLS::GridProperties`) opens the Preferences dialog on the editor's
// Grids page (`ShowPreferences( _( "Grids" ), <the editor> )`, state/preferencesStart.ts; UI-LEFT-3 C3-5), as from the tree (components/PreferencesDialog.tsx).
// The list of grids with its Add / Edit / Remove / Move Up / Move Down buttons (the row selected is the current grid, `last_size_idx`, and the one the buttons act
// on), Fast Grid Switching (Grid 1 and Grid 2, the grids the fast grid hotkeys switch between); the dialog's "Reset Grids to Defaults" is the page's `ResetPanel`.
//
// Add and Edit open Grid Settings (`DIALOG_GRID_SETTINGS`): "Name:" (optional), "X:", "Linked" (on: Y is X; on by default, and when an edited grid is square) and "Y:", in
// the display unit, checked the way it checks them (0.001 to 1000 mm, "Grid size X out of range." / "Grid size Y out of range."; "Grid size '%s' already exists." for a
// grid the list has -- the same name, X and Y). The list shows each grid as "name: X x Y (in the other unit)" (`RebuildGridSizes`). Under the list, "Grid Overrides": a grid
// of its own for each category of item (connected items, wires, vias, text, graphics -- the rows each editor has are `PANEL_GRID_SETTINGS`'s), which the snapping uses while
// Grid Overrides (Ctrl+Shift+G) is on; an override follows its grid when the list is edited (`RebuildGridSizes` keeps the selection by its text). The editors keep their
// current grid by its X size and the row last chosen (`last_size_idx`), so of two grids with the same X the one chosen is current (kicad-port/gridSettings.ts
// `currentGridIndex`; UI-LEFT-3 C3-6).
import { useState } from "react";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useFpDispatch, useFpState } from "../state/footprintEditorStore";
import { useSymDispatch, useSymState } from "../state/symbolEditorStore";
import { currentGridRow, getGridSettings, setGridSettings, setLastGridIndex } from "../state/gridSettings";
import { getGridOverrides, setGridOverrides } from "../state/gridOverrides";
import { formatLength, nmTo, type LengthUnit } from "../state/units";
import { effectiveHotkey, displayHotkey } from "../actions/hotkeys";
import { GRID_EDITORS, gridAt, gridDefs, gridListLabel, gridMessageText, insertGrid, moveGrid, parseGridSize, removeGrid, replaceGrid, resetGrids, sameGrid, type GridDef, type GridEdit, type GridEditor, type GridSettings } from "../kicad-port/gridSettings";
import { overrideRows, rebuildOverrides, safeOverrideIndex, type GridOverrides } from "../kicad-port/gridOverrides";

interface Page {
  settings: GridSettings;
  /** The row selected: the current grid, and the one the buttons act on. */
  row: number;
  /** The Grid Overrides section: which categories have a grid of their own, and which. */
  overrides: GridOverrides;
}


const actionsByName = new Map((actionsData as unknown as ActionsFile).actions.map((a) => [a.name, a]));

/** `(Alt+1)`: the hotkey the Fast Grid boxes show next to their label. */
function hotkeyLabel(name: string): string {
  const a = actionsByName.get(name);
  const hk = a ? displayHotkey(effectiveHotkey(a).hotkey) : "";
  return hk ? `(${hk})` : "";
}

/** A grid as the list shows it (`RebuildGridSizes`: "name: ", the display unit, and in brackets the other system's). */
function gridLabel(g: GridDef, units: LengthUnit): string {
  return gridListLabel(g, (nm) => formatLength(nm, units), (nm) => formatLength(nm, units === "mm" ? "mil" : "mm"));
}

/** The Grid Settings sub-dialog's fields (`DIALOG_GRID_SETTINGS`). */
interface Entry {
  mode: "add" | "edit";
  name: string;
  x: string;
  y: string;
  linked: boolean;
}

/** A size as it is typed into the entry: the number in the display unit, without trailing zeros. */
function entryText(um: number, units: LengthUnit): string {
  return String(Number(nmTo(um, units).toFixed(6)));
}

/** Each editor's current grid size (the row its page starts on). */
function useCurrentGrids(): Record<GridEditor, number> {
  const state = useStudioState();
  const fp = useFpState();
  const sym = useSymState();
  return { pcb: state.gridNm, footprint: fp.gridNm, symbol: sym.gridNm, schematic: state.schGridNm };
}

/** A page as the dialog opens it: the editor's kept list, its current grid's row (`last_size_idx`), its overrides. */
function initialPage(e: GridEditor, currentGrid: Record<GridEditor, number>): Page {
  const settings = getGridSettings(e);
  return { settings, row: Math.max(0, currentGridRow(e, currentGrid[e])), overrides: getGridOverrides(e) };
}

/**
 * The Grids pages' working copies (`PANEL_GRID_SETTINGS`, one per editor), for the Preferences dialog: `commit` is OK (`TransferDataFromWindow`
 * of each page that was shown: its list, its overrides, and the row selected made the editor's current grid), `reset` the page's `ResetPanel`.
 */
export function useGridsPages(initial: GridEditor | null): {
  pages: Record<GridEditor, Page>;
  setPage: (e: GridEditor, next: Page) => void;
  show: (e: GridEditor) => void;
  commit: () => void;
  reset: (e: GridEditor) => void;
  restart: (initial: GridEditor | null) => void;
} {
  const dispatch = useStudioDispatch();
  const fpDispatch = useFpDispatch();
  const symDispatch = useSymDispatch();
  const currentGrid = useCurrentGrids();
  const all = (): Record<GridEditor, Page> => ({ pcb: initialPage("pcb", currentGrid), footprint: initialPage("footprint", currentGrid), symbol: initialPage("symbol", currentGrid), schematic: initialPage("schematic", currentGrid) });
  const [pages, setPages] = useState<Record<GridEditor, Page>>(all);
  /** The pages that were shown: OK makes the row selected on each of them the current grid of its editor (a page never opened is left as it was). */
  const [seen, setSeen] = useState<ReadonlySet<GridEditor>>(new Set(initial ? [initial] : []));
  return {
    pages,
    setPage: (e, next) => setPages((p) => ({ ...p, [e]: next })),
    show: (e) => setSeen((s) => (s.has(e) ? s : new Set(s).add(e))),
    restart: (first) => {
      setPages(all());
      setSeen(new Set(first ? [first] : []));
    },
    commit: () => {
      for (const e of GRID_EDITORS) {
        if (!seen.has(e)) continue;
        const { settings: s, row: r, overrides: o } = pages[e];
        setGridSettings(e, s);
        setGridOverrides(e, o);
        // `TransferDataFromWindow`: `last_size_idx = m_currentGridCtrl->GetSelection()`, the row (not its size) -- of two grids of one X, the one selected
        if (r >= 0 && r < s.grids.length) setLastGridIndex(e, r);
        const um = s.grids[r];
        if (um === undefined || Math.abs(um - currentGrid[e]) < 1e-9) continue;
        if (e === "pcb") dispatch({ type: "SET_GRID_NM", um });
        else if (e === "schematic") dispatch({ type: "SET_SCH_GRID_NM", um });
        else if (e === "footprint") fpDispatch({ type: "SET_GRID_NM", um });
        else symDispatch({ type: "SET_GRID_NM", um });
      }
    },
    reset: (e) =>
      setPages((p) => {
        const page = p[e];
        const next = resetGrids(page.settings, e);
        const keptAt = gridDefs(next).findIndex((g) => sameGrid(g, gridAt(page.settings, page.row)));
        // `ResetPanel` puts the list back (only the list); the overrides keep their grids, as after any edit of the list.
        return { ...p, [e]: { settings: next, row: Math.max(0, keptAt), overrides: rebuildOverrides(page.overrides, gridDefs(page.settings), gridDefs(next), sameGrid) } };
      }),
  };
}

/** One editor's Grids page (`PANEL_GRID_SETTINGS`): the list and its buttons, Grid Settings, Grid Overrides, Fast Grid Switching. */
export function GridsPageBody({ editor, page, setPage }: { editor: GridEditor; page: Page; setPage: (next: Page) => void }) {
  const units = useStudioState().units;
  const [entry, setEntry] = useState<Entry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { settings, row } = page;
  /** `OnAddGrid` / `onEditGrid`: the new grid starts empty with Linked on; an edited one shows its name and X, and its Y unless it is square (`TransferDataToWindow`). */
  const openEntry = (mode: "add" | "edit") => {
    const g = mode === "edit" ? gridAt(settings, row) : undefined;
    const linked = !g || Math.abs(g.x - g.y) < 1e-9;
    setEntry({ mode, name: g?.name ?? "", x: g ? entryText(g.x, units) : "", y: g && !linked ? entryText(g.y, units) : "", linked });
    setError(null);
  };

  /** `DIALOG_GRID_SETTINGS::TransferDataFromWindow`, then `OnAddGrid` / `onEditGrid`: the grid typed is checked and applied. */
  const commitEntry = () => {
    if (!entry) return;
    const x = parseGridSize(entry.x, units);
    const y = entry.linked ? x : parseGridSize(entry.y, units);
    const grid: GridDef | null = x === null ? null : { name: entry.name, x, y: y ?? NaN };
    const result: GridEdit = grid === null ? { ok: false, error: "range" } : entry.mode === "add" ? insertGrid(settings, row, grid) : replaceGrid(settings, row, grid);
    if (!result.ok) {
      setError(result.error === "duplicate" && grid ? `Grid size '${gridMessageText(grid, (nm) => formatLength(nm, units))}' already exists.` : result.error === "range_y" ? "Grid size Y out of range." : "Grid size X out of range.");
      return;
    }
    setPage({ settings: result.settings, row: result.row, overrides: rebuildOverrides(page.overrides, gridDefs(settings), gridDefs(result.settings), sameGrid) });
    setEntry(null);
    setError(null);
  };

  const apply = (next: { settings: GridSettings; row: number }) => {
    setPage({ ...next, overrides: rebuildOverrides(page.overrides, gridDefs(settings), gridDefs(next.settings), sameGrid) });
    setError(null);
  };

  const setOverride = (category: Exclude<keyof GridOverrides, "enabled">, patch: Partial<{ on: boolean; index: number }>) =>
    setPage({ ...page, overrides: { ...page.overrides, [category]: { ...page.overrides[category], ...patch } } });

  const gridOptions = gridDefs(settings).map((g, i) => (
    <option key={`${i}-${g.name}-${g.x}-${g.y}`} value={i}>
      {gridLabel(g, units)}
    </option>
  ));
  const iconButton = (label: string, glyph: string, onClick: () => void, disabled = false) => (
    <button aria-label={label} title={label} disabled={disabled} onClick={onClick} style={{ width: 30 }}>
      {glyph}
    </button>
  );


  return (
    <div data-grids-page={editor}>
      <div style={{ marginTop: 10, fontWeight: 600 }}>Grids</div>
      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        <select aria-label="Grids" size={9} value={row} onChange={(e) => setPage({ ...page, row: Number(e.target.value) })} style={{ flex: 1 }}>
          {gridOptions}
        </select>
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          {iconButton("Add Grid", "+", () => openEntry("add"))}
          {iconButton("Edit Grid", "✎", () => openEntry("edit"))}
          {iconButton("Remove Grid", "−", () => apply(removeGrid(settings, row)), settings.grids.length <= 1)}
          {iconButton("Move Grid Up", "▲", () => apply(moveGrid(settings, row, -1)), settings.grids.length <= 1 || row <= 0)}
          {iconButton("Move Grid Down", "▼", () => apply(moveGrid(settings, row, 1)), settings.grids.length <= 1 || row >= settings.grids.length - 1)}
        </div>
      </div>

      {entry && (
        <div role="group" aria-label="Grid Settings" style={{ marginTop: 8, padding: 8, border: "1px solid var(--chrome-border)" }}>
          <div style={{ fontWeight: 600, marginBottom: 4 }}>Grid Settings</div>
          <div
            className="kv-grid"
            style={{ gridTemplateColumns: "50px 1fr 50px", rowGap: 4 }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commitEntry();
              } else if (e.key === "Escape") {
                e.stopPropagation();
                setEntry(null);
                setError(null);
              }
            }}
          >
            <span>Name:</span>
            <input aria-label="Grid name" autoFocus value={entry.name} onChange={(e) => setEntry({ ...entry, name: e.target.value })} />
            <span style={{ opacity: 0.7 }}>(optional)</span>
            <span>X:</span>
            <input aria-label="Grid size X" value={entry.x} onChange={(e) => setEntry({ ...entry, x: e.target.value })} />
            <span>{units}</span>
            <span />
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input type="checkbox" aria-label="Linked" checked={entry.linked} onChange={(e) => setEntry({ ...entry, linked: e.target.checked })} />
              Linked
            </label>
            <span />
            <span>Y:</span>
            <input aria-label="Grid size Y" disabled={entry.linked} value={entry.linked ? "" : entry.y} onChange={(e) => setEntry({ ...entry, y: e.target.value })} />
            <span>{units}</span>
          </div>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 6 }}>
            <button onClick={() => (setEntry(null), setError(null))}>Cancel</button>
            <button className="primary" onClick={commitEntry}>
              OK
            </button>
          </div>
        </div>
      )}
      {error && (
        <div role="alert" style={{ marginTop: 6, color: "var(--error, #c0392b)", fontSize: 12 }}>
          {error}
        </div>
      )}

      <div style={{ marginTop: 12, fontWeight: 600 }}>Grid Overrides</div>
      <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr", marginTop: 4, rowGap: 4 }}>
        {overrideRows(editor).map(({ category, label }) => (
          <span key={category} style={{ display: "contents" }}>
            <label style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input type="checkbox" aria-label={`${label} grid override`} checked={page.overrides[category].on} onChange={(e) => setOverride(category, { on: e.target.checked })} />
              {label}:
            </label>
            <select aria-label={`${label} override grid`} value={safeOverrideIndex(page.overrides[category].index, settings.grids.length)} disabled={!page.overrides[category].on} onChange={(e) => setOverride(category, { index: Number(e.target.value) })}>
              {gridOptions}
            </select>
          </span>
        ))}
      </div>

      <div style={{ marginTop: 12, fontWeight: 600 }}>Fast Grid Switching</div>
      <div className="kv-grid" style={{ gridTemplateColumns: "70px 1fr 70px", marginTop: 4 }}>
        <span>Grid 1:</span>
        <select aria-label="Grid 1" value={settings.fast1} onChange={(e) => setPage({ ...page, settings: { ...settings, fast1: Number(e.target.value) } })}>
          {gridOptions}
        </select>
        <span style={{ opacity: 0.7 }}>{hotkeyLabel("common.Control.gridFast1")}</span>
        <span>Grid 2:</span>
        <select aria-label="Grid 2" value={settings.fast2} onChange={(e) => setPage({ ...page, settings: { ...settings, fast2: Number(e.target.value) } })}>
          {gridOptions}
        </select>
        <span style={{ opacity: 0.7 }}>{hotkeyLabel("common.Control.gridFast2")}</span>
      </div>
    </div>
  );
}
