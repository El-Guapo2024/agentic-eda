// The actions that work on the editor's list of grids: Edit Grids..., the grid presets and Next / Previous Grid, the fast grids, and Grid Overrides. The board editor, the
// Schematic Editor, the Footprint Editor and the Symbol Editor each have a list (state/gridSettings.ts, edited in components/GridsDialog.tsx) and their grid overrides
// (state/gridOverrides.ts). `registerCommonActions` (commonActions.ts) calls `registerGridListActions` once while the registry is built.
//
//   common/tool/common_tools.cpp   COMMON_TOOLS::GridProperties / GridPreset / GridNext / GridPrev / GridFast1 / GridFast2 / GridFastCycle / OnGridChanged
//   common/eda_draw_frame.cpp      EDA_DRAW_FRAME::OnSelectGrid ("Edit Grids..." at the end of the grid box -> `ACTIONS::gridProperties`)
//
// `OnGridChanged( aFromHotkey )` puts the cursor on the new grid and, when it came from a hotkey (Next / Previous Grid, the fast grids), shows which grid it is
// (`EVENTS::GridChangedByKeyEvent`); the grid box and `gridPreset` do not.
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { currentGridRow, getGridSettings, getLastGridIndex, setLastGridIndex } from "../state/gridSettings";
import { toggleGridOverrides } from "../state/gridOverrides";
import { setPreferencesStartPage } from "../state/preferencesStart";
import { gridEditorOfTab, stepGridIndex } from "../kicad-port/gridSettings";
import { fastGridCycleTarget, gridPresetIndex } from "../kicad-port/cursorControl";
import { alignToGrid } from "../kicad-port/gridSnap";
import { getSnapOrigin } from "../components/canvas/gridHelper";
import { formatLength } from "../state/units";

export function registerGridListActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const editor = gridEditorOfTab(ctx.tab);
  if (!editor) return;

  /**
   * `currentGrid = idx` (`last_size_idx`, so of two grids with the same X the one chosen is current: UI-LEFT-3 C3-6), then `OnGridChanged`: the editor's grid becomes
   * that row's, the cursor goes to the nearest point of it, and a hotkey says which grid it is.
   */
  const changeGrid = (idx: number, fromHotkey: boolean) => {
    const adapter = ctx.getAdapter();
    const um = getGridSettings(editor).grids[idx];
    if (!adapter || um === undefined) return;
    setLastGridIndex(editor, idx);
    adapter.setGridNm(um);
    if (adapter.cursor) {
      const p = alignToGrid({ x: adapter.cursor.x, y: adapter.cursor.y }, um, getSnapOrigin(), { ctrlOrCmd: false });
      adapter.setCursor({ x: p.x, y: p.y });
    }
    if (fromHotkey) ctx.dispatch({ type: "TOAST", message: `Grid: ${formatLength(um, ctx.state.units)}`, kind: "info" });
  };

  /** `GridPreset( idx, fromHotkey )`: `currentGrid = clamp( idx, 0, size - 1 )`. */
  const preset = (idx: number, fromHotkey: boolean) => {
    changeGrid(gridPresetIndex(idx, getGridSettings(editor).grids.length), fromHotkey);
  };

  // ACTIONS::gridProperties ("Edit Grids...", common.Control.editGrids) -- COMMON_TOOLS::GridProperties: `ShowPreferences( _( "Grids" ), <the editor's name> )`, the
  // Preferences dialog opened on the editor's Grids page (state/preferencesStart.ts, components/PreferencesDialog.tsx; UI-LEFT-3 C3-5) -- the end of the grid box,
  // and the right-click menu of the Show Grid button, run it.
  m.set("common.Control.editGrids", () => {
    setPreferencesStartPage({ grids: editor });
    ctx.dispatch({ type: "SET_PREFERENCES_DIALOG_OPEN", open: true });
  });

  // ACTIONS::toggleGridOverrides (Ctrl+Shift+G) -- COMMON_TOOLS::ToggleGridOverrides: `m_frame->SetGridOverrides( !m_frame->IsGridOverridden() )`. With overrides on, connected
  // items, wires, vias, text and graphics snap to the grid the Grids page names for them; off, everything snaps to the current grid.
  m.set("common.Control.toggleGridOverrides", () => void toggleGridOverrides(editor));

  // ACTIONS::gridPreset -- COMMON_TOOLS::GridPreset( aEvent.Parameter<int>(), false ): entry `idx` of the editor's grid list.
  m.set("common.Control.gridPreset", (arg) => preset(typeof arg === "number" ? Math.round(arg) : 0, false));

  // ACTIONS::gridNext / gridPrev -- COMMON_TOOLS::GridNext / GridPrev: `currentGrid++; if( >= size ) = 0` / `--; if( < 0 ) = size - 1`, wrapping round.
  const step = (dir: 1 | -1): ActionHandler => () => {
    const adapter = ctx.getAdapter();
    if (adapter) changeGrid(stepGridIndex(getGridSettings(editor).grids, adapter.gridNm, dir, getLastGridIndex(editor)), true);
  };
  m.set("common.Control.gridNext", step(1));
  m.set("common.Control.gridPrev", step(-1));

  // ACTIONS::gridFast1 / gridFast2 / gridFastCycle -- COMMON_TOOLS::GridFast1 / GridFast2 / GridFastCycle: `GridPreset( fast_grid_1 / fast_grid_2, true )`; the cycle
  // goes to Grid 2 when the current grid is Grid 1, and to Grid 1 from any other.
  m.set("common.Control.gridFast1", () => preset(getGridSettings(editor).fast1, true));
  m.set("common.Control.gridFast2", () => preset(getGridSettings(editor).fast2, true));
  m.set("common.Control.gridFastCycle", () => {
    const adapter = ctx.getAdapter();
    if (!adapter) return;
    const { fast1, fast2 } = getGridSettings(editor);
    preset(fastGridCycleTarget(currentGridRow(editor, adapter.gridNm), fast1, fast2), true);
  });
}
