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
import { canvasRect, emitCanvasEvent } from "./canvasEvents";
import { registerSelectionActions } from "./commonSelectionActions";
import { registerGroupActions } from "./commonGroupActions";
import { registerCheckerActions } from "./commonCheckerActions";
import { registerSuiteActions } from "./commonSuiteActions";
import { registerLibraryActions } from "./commonLibraryActions";
import { registerTextActions } from "./commonTextActions";
import { registerGridActions } from "./commonGridActions";
import { registerGridListActions } from "./commonGridListActions";
import { replaceAll, replaceAndFindNext, updateFind } from "../components/schematic/findReplaceOps";

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

  // Zoom to Fit (COMMON_TOOLS::ZoomFitScreen / ZoomFitObjects) of the board and the schematic is `useActionRunner.ts`'s, and the library editors' is
  // `editorFrameActions.ts`'s. What the zoom list's "Zoom Auto" does in the library editors is the same function: `doZoomFit( ZOOM_FIT_ALL )` frames
  // `GetDocumentExtents()`, with the bigger margin the symbol and footprint editors get ("1.48").
  const boardOrSheetFit = m.get("common.Control.zoomFitScreen");
  const fitAll = (a: EditorAdapter, rect: DOMRect) => {
    const box = a.contentBox();
    const view = zoomFitBox(box ?? a.defaultBox(), a.defaultBox(), rect.width, rect.height, LIBRARY_EDITOR_FIT_MARGIN);
    if (view) a.setView(view);
  };

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
  // (the inactive layers dimmed or not). The board's high contrast is that switch, and so is the Footprint Editor's own (what is not on its active layer is
  // dimmed). The Symbol Editor has no layers.
  if (tab === "pcb") m.set("common.Control.highContrastMode", () => dispatch({ type: "TOGGLE_HIGH_CONTRAST" }));
  else if (tab === "footprint") m.set("common.Control.highContrastMode", () => ctx.fpDispatch({ type: "TOGGLE_HIGH_CONTRAST" }));

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

  // ACTIONS::activatePointEditor -- posted by a tool that has just finished drawing or editing something, so the point editor starts again and
  // `makePoints` builds the handles for what is selected now (PCB_POINT_EDITOR::Main, SCH_POINT_EDITOR::Main). The studio's point editor keeps no
  // running tool: it works out the one selected zone or shape (`pointItemOf`, `SCH_POINT_EDITOR`'s polygon and rule area) from the selection every
  // time it draws its handles or an action reads them, so there is nothing to start again -- the event is satisfied by construction.
  m.set("common.Control.activatePointEditor", () => {});
  // ACTIONS::updateMenu -- `SELECTION_TOOL::UpdateMenu( menu )`: the selection tool re-evaluates a context menu's conditions for the current
  // selection (`CONDITIONAL_MENU::Evaluate`, `ACTION_MENU::UpdateAll`) just before it opens. The studio's menus are built from the selection each
  // time they open (`pcbSweepMenuEntries`, `schContextMenu`) and the menu bar's entries read their state on every render, so no menu holds a stale
  // condition to refresh -- satisfied by construction.
  m.set("common.Interactive.updateMenu", () => {});

  // ACTIONS::showContextMenu -- COMMON_TOOLS::CursorControl( CURSOR_RIGHT_CLICK ): a right-click event at the pointer, which
  // opens the tool's context menu.
  m.set(
    "common.Control.showContextMenu",
    onCanvas((a) => {
      if (a.cursor) emitCanvasEvent(a, "contextmenu", a.cursor);
    })
  );

  // The schematic's Find and Replace actions (eeschema/tools/sch_find_replace_tool.cpp): the dialog's Replace and Replace All buttons run
  // them, they work with the dialog closed (F3 and Shift+F3 are the Find Next / Previous registered in `useActionRunner.ts`), and
  // `updateFind` brightens every match of the search text while the dialog is open (components/schematic/findReplaceOps.ts).
  if (tab === "schematic") {
    m.set("common.Interactive.replaceAndFindNext", () => void replaceAndFindNext(ctx.api, dispatch));
    m.set("common.Interactive.replaceAll", () => void replaceAll(ctx.api, dispatch));
    m.set("common.Control.updateFind", () => void updateFind(ctx.api));
  }

  // The selection tool's modes and events, the interactive delete tool and the picker (commonSelectionActions.ts), the group tool's
  // membership edits (commonGroupActions.ts).
  registerSelectionActions(m, ctx);
  registerGroupActions(m, ctx);
  registerCheckerActions(m, ctx);
  registerSuiteActions(m, ctx);
  registerLibraryActions(m, ctx);
  registerTextActions(m, ctx);
  registerGridActions(m, ctx);
  registerGridListActions(m, ctx);
}
