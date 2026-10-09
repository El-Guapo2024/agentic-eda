// `ACTIONS::gridProperties` ("Edit Grids...", common.Control.editGrids) -> `COMMON_TOOLS::GridProperties` -> `ShowPreferences( "Grids", <editor> )`: the Grids page of
// KiCad's Preferences (common/dialogs/panel_grid_settings.cpp), opened on the editor that asked. The list of grids with its Add / Edit / Remove / Move Up / Move Down
// buttons (the row selected is the current grid, `last_size_idx`, and the one the buttons act on), Fast Grid Switching (Grid 1 and Grid 2, the grids the fast grid
// hotkeys switch between) and Reset to Defaults. The editors are the pages of the Preferences tree: the PCB Editor, the Footprint Editor and the Symbol Editor each
// have their own list, and OK keeps all of them (state/gridSettings.ts) and makes the row selected on each page the current grid of that editor.
//
// Add and Edit ask for one size in the display unit, checked the way `DIALOG_GRID_SETTINGS` checks it (0.001 to 1000 mm; "Grid size '%s' already exists." for one in the
// list). Under the list, "Grid Overrides": a grid of its own for each category of item (connected items, wires, vias, text, graphics -- the rows each editor has are
// `PANEL_GRID_SETTINGS`'s), which the snapping uses while Grid Overrides (Ctrl+Shift+G) is on; an override follows its grid when the list is edited (`RebuildGridSizes` keeps
// the selection by name). Not here: the grid's name and a different Y size -- see kicad-port/gridSettings.ts.
import { useState } from "react";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useFpDispatch, useFpState } from "../state/footprintEditorStore";
import { useSymDispatch, useSymState } from "../state/symbolEditorStore";
import { setGridsDialogOpen, useCommonDialogs } from "../state/commonDialogs";
import { getGridSettings, setGridSettings } from "../state/gridSettings";
import { getGridOverrides, setGridOverrides } from "../state/gridOverrides";
import { formatLength, umTo, type LengthUnit } from "../state/units";
import { effectiveHotkey, displayHotkey } from "../actions/hotkeys";
import { GRID_EDITORS, GRID_EDITOR_LABEL, insertGrid, moveGrid, parseGridSize, removeGrid, replaceGrid, resetGrids, type GridEdit, type GridEditor, type GridSettings } from "../kicad-port/gridSettings";
import { overrideRows, rebuildOverrides, safeOverrideIndex, type GridOverrides } from "../kicad-port/gridOverrides";

interface Page {
  settings: GridSettings;
  /** The row selected: the current grid, and the one the buttons act on. */
  row: number;
  /** The Grid Overrides section: which categories have a grid of their own, and which. */
  overrides: GridOverrides;
}

const sameSize = (a: number | undefined, b: number | undefined) => a !== undefined && b !== undefined && Math.abs(a - b) < 1e-9;

const actionsByName = new Map((actionsData as unknown as ActionsFile).actions.map((a) => [a.name, a]));

/** `(Alt+1)`: the hotkey the Fast Grid boxes show next to their label. */
function hotkeyLabel(name: string): string {
  const a = actionsByName.get(name);
  const hk = a ? displayHotkey(effectiveHotkey(a).hotkey) : "";
  return hk ? `(${hk})` : "";
}

/** A grid as the list shows it (`BuildChoiceList`: the display unit, and in brackets the other system's). */
function gridLabel(um: number, units: LengthUnit): string {
  return `${formatLength(um, units)} (${formatLength(um, units === "mm" ? "mil" : "mm")})`;
}

/** A size as it is typed into the entry: the number in the display unit, without trailing zeros. */
function entryText(um: number, units: LengthUnit): string {
  return String(Number(umTo(um, units).toFixed(6)));
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

  const currentGrid: Record<GridEditor, number> = { pcb: state.gridUm, footprint: fp.gridUm, symbol: sym.gridUm, schematic: state.schGridUm };
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
  const [entry, setEntry] = useState<{ mode: "add" | "edit"; text: string } | null>(null);
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
      if (e === "pcb") dispatch({ type: "SET_GRID_UM", um });
      else if (e === "schematic") dispatch({ type: "SET_SCH_GRID_UM", um });
      else if (e === "footprint") fpDispatch({ type: "SET_GRID_UM", um });
      else symDispatch({ type: "SET_GRID_UM", um });
    }
    close();
  };

  /** `OnAddGrid` / `onEditGrid` after the size dialog: the entry typed is checked and applied. */
  const commitEntry = () => {
    if (!entry) return;
    const um = parseGridSize(entry.text, units);
    const result: GridEdit = um === null ? { ok: false, error: "range" } : entry.mode === "add" ? insertGrid(settings, row, um) : replaceGrid(settings, row, um);
    if (!result.ok) {
      setError(result.error === "duplicate" ? `Grid size '${formatLength(um ?? 0, units)}' already exists.` : "Grid size out of range.");
      return;
    }
    setPage({ settings: result.settings, row: result.row, overrides: rebuildOverrides(page.overrides, settings.grids, result.settings.grids, sameSize) });
    setEntry(null);
    setError(null);
  };

  const apply = (next: { settings: GridSettings; row: number }) => {
    setPage({ ...next, overrides: rebuildOverrides(page.overrides, settings.grids, next.settings.grids, sameSize) });
    setError(null);
  };

  const reset = () => {
    const next = resetGrids(settings, editor);
    const keptAt = next.grids.findIndex((g) => Math.abs(g - (settings.grids[row] ?? NaN)) < 1e-9);
    // `ResetPanel` puts the list back (only the list); the overrides keep their grids by size, as after any edit of the list.
    setPage({ settings: next, row: Math.max(0, keptAt), overrides: rebuildOverrides(page.overrides, settings.grids, next.grids, sameSize) });
    setEntry(null);
    setError(null);
  };

  const setOverride = (category: Exclude<keyof GridOverrides, "enabled">, patch: Partial<{ on: boolean; index: number }>) =>
    setPage({ ...page, overrides: { ...page.overrides, [category]: { ...page.overrides[category], ...patch } } });

  const gridOptions = settings.grids.map((um, i) => (
    <option key={`${i}-${um}`} value={i}>
      {gridLabel(um, units)}
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
              {iconButton("Add Grid", "+", () => (setEntry({ mode: "add", text: "" }), setError(null)))}
              {iconButton("Edit Grid", "✎", () => (setEntry({ mode: "edit", text: entryText(settings.grids[row] ?? 0, units) }), setError(null)))}
              {iconButton("Remove Grid", "−", () => apply(removeGrid(settings, row)), settings.grids.length <= 1)}
              {iconButton("Move Grid Up", "▲", () => apply(moveGrid(settings, row, -1)), settings.grids.length <= 1 || row <= 0)}
              {iconButton("Move Grid Down", "▼", () => apply(moveGrid(settings, row, 1)), settings.grids.length <= 1 || row >= settings.grids.length - 1)}
            </div>
          </div>

          {entry && (
            <div style={{ marginTop: 8 }}>
              <div className="kv-grid" style={{ gridTemplateColumns: "70px 1fr 36px auto auto" }}>
                <span>{entry.mode === "add" ? "New grid:" : "Grid size:"}</span>
                <input
                  aria-label="Grid size"
                  autoFocus
                  value={entry.text}
                  onChange={(e) => setEntry({ ...entry, text: e.target.value })}
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
                />
                <span>{units}</span>
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
