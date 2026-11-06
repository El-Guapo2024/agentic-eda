// `ACTIONS::gridProperties` ("Edit Grids...", common.Control.editGrids) -> `COMMON_TOOLS::GridProperties` -> `ShowPreferences( "Grids", <editor> )`: the Grids page of
// KiCad's Preferences (common/dialogs/panel_grid_settings.cpp), opened on the editor that asked. The list of grids with its Add / Edit / Remove / Move Up / Move Down
// buttons (the row selected is the current grid, `last_size_idx`, and the one the buttons act on), Fast Grid Switching (Grid 1 and Grid 2, the grids the fast grid
// hotkeys switch between) and Reset to Defaults. The editors are the pages of the Preferences tree: the PCB Editor, the Footprint Editor and the Symbol Editor each
// have their own list, and OK keeps all of them (state/gridSettings.ts) and makes the row selected on each page the current grid of that editor.
//
// Add and Edit open Grid Settings (`DIALOG_GRID_SETTINGS`): "Name:" (optional), "X:", "Linked" (on: Y is X; on by default, and when an edited grid is square) and "Y:", in
// the display unit, checked the way it checks them (0.001 to 1000 mm, "Grid size X out of range." / "Grid size Y out of range."; "Grid size '%s' already exists." for a
// grid the list has -- the same name, X and Y). The list shows each grid as "name: X x Y (in the other unit)" (`RebuildGridSizes`). Under the list, "Grid Overrides": a grid
// of its own for each category of item (connected items, wires, vias, text, graphics -- the rows each editor has are `PANEL_GRID_SETTINGS`'s), which the snapping uses while
// Grid Overrides (Ctrl+Shift+G) is on; an override follows its grid when the list is edited (`RebuildGridSizes` keeps the selection by its text). The editors keep their
// current grid by its X size, so of two grids with the same X the first is the one chosen (kicad-port/gridSettings.ts `gridYOf`).
import { useState } from "react";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useFpDispatch, useFpState } from "../state/footprintEditorStore";
import { useSymDispatch, useSymState } from "../state/symbolEditorStore";
import { setGridsDialogOpen, useCommonDialogs } from "../state/commonDialogs";
import { getGridSettings, setGridSettings } from "../state/gridSettings";
import { getGridOverrides, setGridOverrides } from "../state/gridOverrides";
import { formatLength, nmTo, type LengthUnit } from "../state/units";
import { effectiveHotkey, displayHotkey } from "../actions/hotkeys";
import { GRID_EDITORS, GRID_EDITOR_LABEL, gridAt, gridDefs, gridListLabel, gridMessageText, insertGrid, moveGrid, parseGridSize, removeGrid, replaceGrid, resetGrids, sameGrid, type GridDef, type GridEdit, type GridEditor, type GridSettings } from "../kicad-port/gridSettings";
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

export function GridsDialog() {
  const editor = useCommonDialogs().grids;
  if (!editor) return null;
  return <GridsDialogBody initial={editor} />;
}

function GridsDialogBody({ initial }: { initial: GridEditor }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const fp = useFpState();
  const fpDispatch = useFpDispatch();
  const sym = useSymState();
  const symDispatch = useSymDispatch();
  const units = state.units;

  const currentGrid: Record<GridEditor, number> = { pcb: state.gridNm, footprint: fp.gridNm, symbol: sym.gridNm, schematic: state.schGridNm };
  const [pages, setPages] = useState<Record<GridEditor, Page>>(() => {
    const page = (e: GridEditor): Page => {
      const settings = getGridSettings(e);
      return { settings, row: Math.max(0, settings.grids.findIndex((g) => Math.abs(g - currentGrid[e]) < 1e-9)), overrides: getGridOverrides(e) };
    };
    return { pcb: page("pcb"), footprint: page("footprint"), symbol: page("symbol"), schematic: page("schematic") };
  });
  const [editor, setEditor] = useState<GridEditor>(initial);
  /** The pages that were shown: OK makes the row selected on each of them the current grid of its editor (a page never opened is left as it was). */
  const [seen, setSeen] = useState<ReadonlySet<GridEditor>>(new Set([initial]));
  const [entry, setEntry] = useState<Entry | null>(null);
  const [error, setError] = useState<string | null>(null);

  const page = pages[editor];
  const { settings, row } = page;
  const setPage = (next: Page) => setPages((p) => ({ ...p, [editor]: next }));
  const close = () => setGridsDialogOpen(null);

  const showEditor = (e: GridEditor) => {
    setEditor(e);
    setSeen((s) => new Set(s).add(e));
    setEntry(null);
    setError(null);
  };

  const ok = () => {
    for (const e of GRID_EDITORS) {
      if (!seen.has(e)) continue;
      const { settings: s, row: r, overrides: o } = pages[e];
      setGridSettings(e, s);
      setGridOverrides(e, o);
      const um = s.grids[r];
      if (um === undefined || Math.abs(um - currentGrid[e]) < 1e-9) continue;
      if (e === "pcb") dispatch({ type: "SET_GRID_NM", um });
      else if (e === "schematic") dispatch({ type: "SET_SCH_GRID_NM", um });
      else if (e === "footprint") fpDispatch({ type: "SET_GRID_NM", um });
      else symDispatch({ type: "SET_GRID_NM", um });
    }
    close();
  };

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

  const reset = () => {
    const next = resetGrids(settings, editor);
    const keptAt = gridDefs(next).findIndex((g) => sameGrid(g, gridAt(settings, row)));
    // `ResetPanel` puts the list back (only the list); the overrides keep their grids, as after any edit of the list.
    setPage({ settings: next, row: Math.max(0, keptAt), overrides: rebuildOverrides(page.overrides, gridDefs(settings), gridDefs(next), sameGrid) });
    setEntry(null);
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
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && close()} role="dialog" aria-label="Preferences - Grids">
        <div className="dialog-header">
          <span>Preferences - Grids</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "70px 1fr" }}>
            <span>Editor:</span>
            <select aria-label="Editor" value={editor} onChange={(e) => showEditor(e.target.value as GridEditor)}>
              {GRID_EDITORS.map((e) => (
                <option key={e} value={e}>
                  {GRID_EDITOR_LABEL[e]}
                </option>
              ))}
            </select>
          </div>

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
        <div className="dialog-footer" style={{ justifyContent: "space-between" }}>
          <button onClick={reset}>Reset to Defaults</button>
          <span style={{ display: "flex", gap: 8 }}>
            <button onClick={close}>Cancel</button>
            <button className="primary" onClick={ok}>
              OK
            </button>
          </span>
        </div>
      </div>
    </div>
  );
}
