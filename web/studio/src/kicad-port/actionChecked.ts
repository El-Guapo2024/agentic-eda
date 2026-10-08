// Which toolbar buttons are drawn pressed. KiCad's toolbar shows a toggle action as pressed while its condition holds (`ACTION_CONDITIONS::Check(...)` in
// each frame's `setupUIConditions`: grid shown, a pane shown, the units, the crosshair mode, ...) and the button of the ACTIVE TOOL as pressed
// (`TOOL_MANAGER` reports the tool in force; `ACTION_TOOLBAR::onToolEvent` toggles the matching button). The extracted toolbars say nothing about which
// actions toggle (`ToolbarState( TOGGLE )` is not in actions.json), so the toggles this studio implements are listed here, per editor.
//
// Pure: the caller (actions/useActionChecked.ts) reads the stores and passes the facts in.
import type { DockLayout } from "./dockLayout";

export interface CheckedContext {
  /** The active editor tab: "pcb" | "schematic" | "footprint" | "symbol" | "3d". */
  tab: string;
  units: "mm" | "mil" | "in";
  /** The active editor canvas shows its grid. */
  gridVisible: boolean;
  /** The Footprint Editor's and the Symbol Editor's active tool ids. */
  fpTool: string;
  symTool: string;
  /** The Symbol Editor's view toggles and Synchronized Pins Mode. */
  sym: { showElectricalTypes: boolean; showHiddenPins: boolean; syncPins: boolean };
  dock: DockLayout;
  /** The board editor's right dock tab. */
  rightDockTab: string;
  /** The active editor's High Contrast Mode (the board's and the Footprint Editor's own switch). */
  highContrast?: boolean;
  /** The zoom tool is the tool in force (the rubber-band zoom is armed). */
  zoomArmed?: boolean;
}

/** The Footprint Editor's tool-arming actions and the tool each arms (`FpToolId`). */
export const FP_TOOL_OF_ACTION: Readonly<Record<string, string>> = {
  "common.Interactive.selectSetRect": "select",
  "pcbnew.PadTool.placePad": "pad",
  "pcbnew.InteractiveDrawing.line": "draw_segment",
  "pcbnew.InteractiveDrawing.arc": "draw_arc",
  "pcbnew.InteractiveDrawing.rectangle": "draw_rect",
  "pcbnew.InteractiveDrawing.circle": "draw_circle",
  "pcbnew.InteractiveDrawing.graphicPolygon": "draw_polygon",
  "pcbnew.InteractiveDrawing.bezier": "draw_bezier",
  "pcbnew.InteractiveDrawing.text": "text",
  "pcbnew.InteractiveDrawing.setAnchor": "anchor",
};

/** The Symbol Editor's tool-arming actions and the tool each arms (`SymToolId`). */
export const SYM_TOOL_OF_ACTION: Readonly<Record<string, string>> = {
  "common.InteractiveSelection.selectionTool": "select",
  "eeschema.SymbolDrawing.placeSymbolPin": "pin",
  "eeschema.SymbolDrawing.placeSymbolText": "text",
  "eeschema.InteractiveDrawing.drawRectangle": "draw_rect",
  "eeschema.InteractiveDrawing.drawCircle": "draw_circle",
  "eeschema.InteractiveDrawing.drawArc": "draw_arc",
  "eeschema.SymbolDrawing.drawSymbolLines": "draw_lines",
  "eeschema.SymbolDrawing.drawSymbolPolygon": "draw_polygon",
  "eeschema.SymbolDrawing.placeSymbolAnchor": "anchor",
};

/** `true` / `false` for an action that toggles (pressed or not), `undefined` for one that does not -- its button never shows a pressed state. */
export function actionChecked(name: string, c: CheckedContext): boolean | undefined {
  switch (name) {
    case "common.Control.toggleGrid":
      return c.gridVisible;
    // `ACTIONS::highContrastMode` is a check item of the board and Footprint Editor menus (the schematic and symbol editors have no layers to contrast).
    case "common.Control.highContrastMode":
      return c.tab === "pcb" || c.tab === "footprint" ? c.highContrast === true : undefined;
    // The zoom tool, like any tool, is drawn pressed while it runs.
    case "common.Control.zoomTool":
      return c.zoomArmed === true;
    case "common.Control.metricUnits":
      return c.units === "mm";
    case "common.Control.imperialUnits":
      return c.units === "in";
    case "common.Control.mils":
      return c.units === "mil";
    // The panes: `ACTIONS::showProperties` / `SCH_ACTIONS::showHierarchy` toggle a pane of the board and schematic frames' left column; the library tree is
    // the library editors' own column; the Appearance manager is the board editor's right dock.
    case "common.Control.showProperties":
      return c.tab === "pcb" || c.tab === "schematic" ? c.dock.shown.properties && !c.dock.leftCollapsed : undefined;
    case "eeschema.EditorTool.showHierarchy":
      return c.dock.shown.hierarchy && !c.dock.leftCollapsed;
    case "pcbnew.Control.showLayersManager":
      return c.tab === "pcb" ? !c.dock.rightCollapsed && c.rightDockTab === "appearance" : undefined;
    case "common.Control.showLibraryTree":
      return c.tab === "footprint" || c.tab === "symbol" ? !c.dock.treeCollapsed : undefined;
    case "eeschema.SymbolLibraryControl.showElectricalTypes":
      return c.tab === "symbol" ? c.sym.showElectricalTypes : undefined;
    case "eeschema.SymbolLibraryControl.showHiddenPins":
      return c.tab === "symbol" ? c.sym.showHiddenPins : undefined;
    case "eeschema.SymbolLibraryControl.toggleSyncedPinsMode":
      return c.tab === "symbol" ? c.sym.syncPins : undefined;
  }
  if (c.tab === "footprint" && name in FP_TOOL_OF_ACTION) return c.fpTool === FP_TOOL_OF_ACTION[name];
  if (c.tab === "symbol" && name in SYM_TOOL_OF_ACTION) return c.symTool === SYM_TOOL_OF_ACTION[name];
  return undefined;
}
