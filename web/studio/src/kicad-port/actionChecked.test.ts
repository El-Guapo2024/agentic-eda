import { test } from "node:test";
import assert from "node:assert/strict";
import { actionChecked, FP_TOOL_OF_ACTION, SYM_TOOL_OF_ACTION, type CheckedContext } from "./actionChecked";
import { defaultDockLayout, setColumnCollapsed, togglePane } from "./dockLayout";

const base: CheckedContext = {
  tab: "pcb",
  units: "mm",
  gridVisible: true,
  fpTool: "select",
  symTool: "select",
  sym: { showElectricalTypes: true, showHiddenPins: false, syncPins: false },
  dock: defaultDockLayout(1600),
  rightDockTab: "appearance",
};

test("the grid and the units buttons follow their state, on any tab", () => {
  assert.equal(actionChecked("common.Control.toggleGrid", base), true);
  assert.equal(actionChecked("common.Control.toggleGrid", { ...base, gridVisible: false }), false);
  assert.equal(actionChecked("common.Control.metricUnits", base), true);
  assert.equal(actionChecked("common.Control.mils", base), false);
  assert.equal(actionChecked("common.Control.mils", { ...base, units: "mil" }), true);
  assert.equal(actionChecked("common.Control.imperialUnits", { ...base, units: "in" }), true);
});

test("a pane's button is pressed while the pane is shown and its column is open", () => {
  assert.equal(actionChecked("common.Control.showProperties", base), true);
  const hidden = togglePane(base.dock, "properties");
  assert.equal(actionChecked("common.Control.showProperties", { ...base, dock: hidden }), false);
  const folded = setColumnCollapsed(base.dock, "left", true);
  assert.equal(actionChecked("common.Control.showProperties", { ...base, dock: folded }), false, "a pane inside a folded column is not on screen");
  assert.equal(actionChecked("eeschema.EditorTool.showHierarchy", { ...base, tab: "schematic" }), true);
  assert.equal(actionChecked("pcbnew.Control.showLayersManager", base), true);
  assert.equal(actionChecked("pcbnew.Control.showLayersManager", { ...base, dock: setColumnCollapsed(base.dock, "right", true) }), false);
  assert.equal(actionChecked("pcbnew.Control.showLayersManager", { ...base, rightDockTab: "filter" }), false);
  assert.equal(actionChecked("pcbnew.Control.showLayersManager", { ...base, tab: "schematic" }), undefined, "no Appearance manager on the schematic");
});

test("the library tree button belongs to the two library editors", () => {
  assert.equal(actionChecked("common.Control.showLibraryTree", { ...base, tab: "footprint" }), true);
  assert.equal(actionChecked("common.Control.showLibraryTree", { ...base, tab: "symbol", dock: setColumnCollapsed(base.dock, "tree", true) }), false);
  assert.equal(actionChecked("common.Control.showLibraryTree", base), undefined);
});

test("in the Footprint Editor the button of the active tool is pressed, and only there", () => {
  const fp: CheckedContext = { ...base, tab: "footprint", fpTool: "draw_arc" };
  assert.equal(actionChecked("pcbnew.InteractiveDrawing.arc", fp), true);
  assert.equal(actionChecked("pcbnew.InteractiveDrawing.line", fp), false);
  assert.equal(actionChecked("common.Interactive.selectSetRect", fp), false, "select is not the active tool");
  assert.equal(actionChecked("common.Interactive.selectSetRect", { ...fp, fpTool: "select" }), true);
  assert.equal(actionChecked("pcbnew.InteractiveDrawing.arc", { ...fp, tab: "pcb" }), undefined, "the board's own tool state is not this table's");
  for (const action of Object.keys(FP_TOOL_OF_ACTION)) assert.equal(typeof actionChecked(action, fp), "boolean", action);
});

test("in the Symbol Editor the active tool and the view toggles are pressed", () => {
  const sym: CheckedContext = { ...base, tab: "symbol", symTool: "pin" };
  assert.equal(actionChecked("eeschema.SymbolDrawing.placeSymbolPin", sym), true);
  assert.equal(actionChecked("eeschema.InteractiveDrawing.drawRectangle", sym), false);
  assert.equal(actionChecked("eeschema.SymbolLibraryControl.showElectricalTypes", sym), true);
  assert.equal(actionChecked("eeschema.SymbolLibraryControl.showHiddenPins", sym), false);
  assert.equal(actionChecked("eeschema.SymbolLibraryControl.toggleSyncedPinsMode", { ...sym, sym: { ...sym.sym, syncPins: true } }), true);
  assert.equal(actionChecked("eeschema.SymbolLibraryControl.showHiddenPins", base), undefined, "not on another tab");
  for (const action of Object.keys(SYM_TOOL_OF_ACTION)) assert.equal(typeof actionChecked(action, sym), "boolean", action);
});

test("High Contrast Mode is checked on the board and in the Footprint Editor, which have layers to contrast", () => {
  assert.equal(actionChecked("common.Control.highContrastMode", base), false);
  assert.equal(actionChecked("common.Control.highContrastMode", { ...base, highContrast: true }), true);
  assert.equal(actionChecked("common.Control.highContrastMode", { ...base, tab: "footprint", highContrast: true }), true);
  assert.equal(actionChecked("common.Control.highContrastMode", { ...base, tab: "symbol", highContrast: true }), undefined);
  assert.equal(actionChecked("common.Control.highContrastMode", { ...base, tab: "schematic" }), undefined);
});

test("Polar Coordinates is checked on the board and in the Footprint Editor only", () => {
  assert.equal(actionChecked("common.Control.togglePolarCoords", base), false);
  assert.equal(actionChecked("common.Control.togglePolarCoords", { ...base, polar: true }), true);
  assert.equal(actionChecked("common.Control.togglePolarCoords", { ...base, tab: "footprint", polar: true }), true);
  assert.equal(actionChecked("common.Control.togglePolarCoords", { ...base, tab: "schematic", polar: true }), undefined);
});

test("the measure tool's button is pressed while the tool runs", () => {
  assert.equal(actionChecked("common.Interactive.measureTool", base), false);
  assert.equal(actionChecked("common.Interactive.measureTool", { ...base, tab: "footprint", measureArmed: true }), true);
});

test("the zoom tool's button is pressed while the rubber-band zoom is armed", () => {
  assert.equal(actionChecked("common.Control.zoomTool", base), false);
  assert.equal(actionChecked("common.Control.zoomTool", { ...base, zoomArmed: true }), true);
  assert.equal(actionChecked("common.Control.zoomTool", { ...base, tab: "symbol", zoomArmed: true }), true);
});

test("an action that does not toggle has no pressed state at all", () => {
  assert.equal(actionChecked("common.Interactive.undo", base), undefined);
  assert.equal(actionChecked("common.Control.zoomFitScreen", { ...base, tab: "footprint" }), undefined);
});
