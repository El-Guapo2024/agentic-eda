// The shared (`common.*`) KiCad actions the studio wires beyond the ones `useActionRunner.ts` registers inline:
// the view, cursor and display actions of COMMON_TOOLS, the selection tool's modes, groups, the picker, the
// checker markers and the suite-level actions (About, Help, ...). `useActionRunner.ts` calls
// `registerCommonActions` once while it builds its registry.
//
// These tools run in every editor frame in KiCad, so each handler works on whichever of the four canvases is on
// screen -- the board, the schematic, the footprint editor or the symbol editor -- through an `EditorAdapter`
// (editorAdapter.ts), and a handler is only registered on the tabs where it does something (the menus and hotkeys
// read the registry, so an action that is not offered in an editor stays dimmed there, as `actionTabGate.ts` does for
// the editor-specific ones). Each handler cites the KiCad function it ports (commit 8303b2ad).
//
// Not registered, with the reason recorded in `tools/ui-parity-missing.json`: the actions that need a subsystem the
// studio does not have (library tables and on-disk libraries, tables, the Python plugin host, the project manager, the
// simulator's plot zoom).
import type { Dispatch } from "react";
import type { Action as StudioAction, EditorTab, StudioApi, StudioState } from "../state/store";
import type { FootprintEditorApi, FpAction } from "../state/footprintEditorStore";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";
import { isCanvasTab, type EditorAdapter } from "./editorAdapter";
import { getCommonOptions, setCommonOptions } from "../state/commonOptions";
import { crossHairModeForAction } from "../kicad-port/crosshair";
import { LIBRARY_EDITOR_FIT_MARGIN, boxCentre, centerViewOn, setScaleAboutCentre, zoomFitBox, zoomListFor, zoomPresetScale } from "../kicad-port/zoomFit";
import { worldToScreen } from "../kicad-port/view";
import { gridPresetIndex } from "../kicad-port/cursorControl";
import { alignToGrid } from "../kicad-port/gridSnap";
import { formatLength } from "../state/units";

export type ActionHandler = (arg?: unknown) => void;

export interface CommonActionContext {
  tab: EditorTab;
  /** The active editor, read when an action runs (the library editors' stores change without the registry being rebuilt). */
  getAdapter: () => EditorAdapter | null;
  state: StudioState;
  dispatch: Dispatch<StudioAction>;
  api: StudioApi;
  fpApi: FootprintEditorApi;
  fpDispatch: Dispatch<FpAction>;
  symApi: SymbolEditorApi;
  symDispatch: Dispatch<SymAction>;
}

/** The canvas of whichever editor is on screen (they all use this class; only one is mounted at a time). */
export function canvasRect(): DOMRect | null {
  return document.querySelector(".pcb-canvas-container")?.getBoundingClientRect() ?? null;
}

/**
 * Stand-in for the warped pointer / `m_toolMgr->ProcessEvent( TC_MOUSE ... )`: re-dispatches a mouse event on the live
 * canvas at a world position, so every tool that follows the pointer (move preview, route and wire rubber bands, a
 * context menu) sees the keyboard cursor exactly as it would a real mouse at that spot.
 */
export function emitCanvasEvent(adapter: EditorAdapter, type: "pointermove" | "contextmenu", world: { x: number; y: number }): void {
  const el = document.querySelector(".pcb-canvas-container canvas") ?? document.querySelector(".pcb-canvas-container");
  const rect = canvasRect();
  if (!el || !rect) return;
  const [sx, sy] = worldToScreen(adapter.view, world.x, world.y);
  const init = { bubbles: true, cancelable: true, composed: true, clientX: rect.left + sx, clientY: rect.top + sy };
  if (type === "contextmenu") el.dispatchEvent(new MouseEvent("contextmenu", { ...init, button: 2, buttons: 2 }));
  else el.dispatchEvent(new PointerEvent("pointermove", { ...init, button: 0, buttons: 0, pointerId: 1, pointerType: "mouse", isPrimary: true }));
}

export function registerCommonActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const { dispatch } = ctx;
  const tab = isCanvasTab(ctx.tab) ? ctx.tab : null;
  /** A handler that needs the active editor and its canvas. */
  const onCanvas =
    (fn: (a: EditorAdapter, rect: DOMRect, arg?: unknown) => void): ActionHandler =>
    (arg) => {
      const adapter = ctx.getAdapter();
      const rect = canvasRect();
      if (adapter && rect) fn(adapter, rect, arg);
    };
  const toast = (message: string, kind: "error" | "info" = "info") => dispatch({ type: "TOAST", message, kind });

  // ---------------------------------------------------------------------------------------- framing the view

  // ACTIONS::zoomFitSelection -- COMMON_TOOLS::ZoomFitSelection -> doZoomFit( ZOOM_FIT_SELECTION ): frame the selection's
  // bounding box (an empty selection does nothing: `if( selection.Empty() ) return 0`).
  m.set(
    "common.Control.zoomFitSelection",
    onCanvas((a, rect) => {
      const box = a.selectionBox();
      if (!box) return;
      const view = zoomFitBox(box, a.defaultBox(), rect.width, rect.height);
      if (view) a.setView(view);
    })
  );

  // ACTIONS::centerSelection -- COMMON_TOOLS::CenterSelection -> doCenter( CENTER_SELECTION ): pan so the selection's
  // centre is the view centre, keeping the zoom.
  m.set(
    "common.Control.centerSelection",
    onCanvas((a, rect) => {
      const box = a.selectionBox();
      if (!box) return;
      const view = centerViewOn(a.view, rect.width, rect.height, boxCentre(box));
      if (view) a.setView(view);
    })
  );

  // ACTIONS::centerContents -- doCenter( CENTER_CONTENTS ): the centre of the document's own bounding box, or of the
  // default view box when that has no area.
  m.set(
    "common.Control.centerContents",
    onCanvas((a, rect) => {
      let box = a.contentBox();
      if (!box || box[2] - box[0] === 0 || box[3] - box[1] === 0) box = a.defaultBox();
      const view = centerViewOn(a.view, rect.width, rect.height, boxCentre(box));
      if (view) a.setView(view);
    })
  );

  // The library editors do not register Zoom to Fit in `useActionRunner.ts` (that is the board's and the schematic's):
  // COMMON_TOOLS::ZoomFitScreen / ZoomFitObjects -> doZoomFit( ZOOM_FIT_ALL / ZOOM_FIT_OBJECTS ) frames
  // `GetDocumentExtents()`, with the bigger margin the symbol and footprint editors get ("1.48").
  const boardOrSheetFit = m.get("common.Control.zoomFitScreen");
  const fitAll = (a: EditorAdapter, rect: DOMRect) => {
    const box = a.contentBox();
    const view = zoomFitBox(box ?? a.defaultBox(), a.defaultBox(), rect.width, rect.height, LIBRARY_EDITOR_FIT_MARGIN);
    if (view) a.setView(view);
  };
  if (tab === "footprint" || tab === "symbol") {
    m.set("common.Control.zoomFitScreen", onCanvas(fitAll));
    m.set("common.Control.zoomFitObjects", onCanvas(fitAll));
  }

  // ACTIONS::zoomPreset -- COMMON_TOOLS::ZoomPreset( idx ) -> doZoomToPreset( idx, false ): entry `idx` of the editor's
  // zoom list, set about the view centre, or Zoom Auto (zoom to fit the page / document) for 0 -- the action's default
  // parameter. The studio's zoom box picks the entry; `run( name, idx )` carries it.
  m.set(
    "common.Control.zoomPreset",
    onCanvas((a, rect, arg) => {
      const idx = typeof arg === "number" ? Math.round(arg) : 0;
      const preset = zoomPresetScale(zoomListFor(a.tab), idx);
      if (preset.auto) {
        if (tab === "footprint" || tab === "symbol") fitAll(a, rect);
        else boardOrSheetFit?.();
        return;
      }
      a.setView(setScaleAboutCentre(a.view, rect.width, rect.height, preset.scale));
    })
  );

  // ---------------------------------------------------------------------------------------- grid

  // ACTIONS::gridPreset -- COMMON_TOOLS::GridPreset( idx, false ) -> OnGridChanged: `currentGrid = clamp( idx, 0, size - 1 )`,
  // the grid becomes that entry of the editor's grid list and the cursor is put on the new grid
  // (`SetCrossHairCursorPosition( GetCursorPosition( true ) )`). The schematic's grid is the fixed 50 mil, so it has no list
  // to index and the action is not offered there.
  if (tab && tab !== "schematic") {
    m.set("common.Control.gridPreset", (arg) => {
      const adapter = ctx.getAdapter();
      const list = adapter?.gridList;
      if (!adapter || !list) return;
      const um = list[gridPresetIndex(typeof arg === "number" ? Math.round(arg) : 0, list.length)]!;
      adapter.setGridUm(um);
      if (adapter.cursor) {
        const p = alignToGrid({ x: adapter.cursor.x, y: adapter.cursor.y }, um, { x: 0, y: 0 }, { ctrlOrCmd: false });
        adapter.setCursor({ x: p.x, y: p.y });
      }
      toast(`Grid: ${formatLength(um, ctx.state.units)}`);
    });
  }

  // ---------------------------------------------------------------------------------------- cursor and display options

  // ACTIONS::cursorSmallCrosshairs / cursorFullCrosshairs / cursor45Crosshairs -- COMMON_TOOLS::CursorSmallCrosshairs etc.:
  // `galOpts.SetCursorMode( ... )`, kept per window and saved. Here one setting for the four canvases
  // (state/commonOptions.ts); `CommonOverlay` draws the mode.
  const setCrossHair = (action: "cursorSmallCrosshairs" | "cursorFullCrosshairs" | "cursor45Crosshairs"): ActionHandler => () => {
    const crossHairMode = crossHairModeForAction(action);
    setCommonOptions({ crossHairMode });
    dispatch({ type: "SET_FULLSCREEN_CROSSHAIR", value: crossHairMode === "full" });
  };
  m.set("common.Control.cursorSmallCrosshairs", setCrossHair("cursorSmallCrosshairs"));
  m.set("common.Control.cursorFullCrosshairs", setCrossHair("cursorFullCrosshairs"));
  m.set("common.Control.cursor45Crosshairs", setCrossHair("cursor45Crosshairs"));

  // ACTIONS::toggleCursor -- COMMON_TOOLS::ToggleCursor: `galOpts.m_forceDisplayCursor = !galOpts.m_forceDisplayCursor`
  // ("Always Show Crosshairs"). Off, the crosshair is drawn only while a tool is running.
  m.set("common.Control.toggleCursor", () => setCommonOptions({ alwaysShowCursor: !getCommonOptions().alwaysShowCursor }));

  // ACTIONS::toggleBoundingBoxes -- COMMON_TOOLS::ToggleBoundingBoxes: `rs->SetDrawBoundingBoxes( !rs->GetDrawBoundingBoxes() )`.
  m.set("common.Control.toggleBoundingBoxes", () => setCommonOptions({ drawBoundingBoxes: !getCommonOptions().drawBoundingBoxes }));

  // ACTIONS::highContrastMode -- PCB_CONTROL::HighContrastMode: `m_ContrastModeDisplay = NORMAL ? DIMMED : NORMAL`
  // (the inactive layers dimmed or not). The board's high contrast is that switch.
  if (tab === "pcb") m.set("common.Control.highContrastMode", () => dispatch({ type: "TOGGLE_HIGH_CONTRAST" }));

  // PCB_ACTIONS::magneticSnapActiveLayer / magneticSnapAllLayers -- PCB_CONTROL::SnapMode: `settings.allLayers = false / true`
  // (the third, magneticSnapToggle, is registered in `useActionRunner.ts`); SnapModeFeedback says which.
  if (tab === "pcb") {
    const snapMode = (allLayers: boolean): ActionHandler => () => {
      dispatch({ type: "SET_MAGNETIC_ALL_LAYERS", value: allLayers });
      toast(`Object Snapping: ${allLayers ? "All Layers" : "Active Layer"}`);
    };
    m.set("common.Control.magneticSnapActiveLayer", snapMode(false));
    m.set("common.Control.magneticSnapAllLayers", snapMode(true));
  }

  // ---------------------------------------------------------------------------------------- events between tools

  // ACTIONS::refreshPreview -- posted after the cursor moved (CursorControl) or an item changed, so a tool that follows the
  // pointer recomputes what it shows at the cursor. The stand-in for a motion event at the cursor, as the cursor keys use.
  const refreshPreview = onCanvas((a) => {
    if (a.cursor) emitCanvasEvent(a, "pointermove", a.cursor);
  });
  m.set("common.Control.refreshPreview", refreshPreview);
  // ACTIONS::updateUnits / updatePreferences -- `EDA_DRAW_FRAME::ChangeUserUnits` / `CommonSettingsChanged` tell the running
  // tool the units or the settings changed, and it redraws its live assistant in the new units (the measure tool's ruler,
  // the two-point assistant of the drawing tools). Everything on this canvas reads the units and the preferences from
  // state, so the assistants already follow them on the next paint; what is left of the event is that recompute at the cursor.
  m.set("common.Control.updateUnits", refreshPreview);
  m.set("common.Control.updatePreferences", refreshPreview);

  // ACTIONS::showContextMenu -- COMMON_TOOLS::CursorControl( CURSOR_RIGHT_CLICK ): a right-click event at the pointer, which
  // opens the tool's context menu. The schematic has no context menu here.
  if (tab === "pcb" || tab === "footprint" || tab === "symbol") {
    m.set(
      "common.Control.showContextMenu",
      onCanvas((a) => {
        if (a.cursor) emitCanvasEvent(a, "contextmenu", a.cursor);
      })
    );
  }
}
