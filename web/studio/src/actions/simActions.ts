// The SPICE simulator's actions (package P21; commit 8303b2ad):
//
//   eeschema/tools/simulator_control.cpp   SIMULATOR_CONTROL::NewAnalysisTab / OpenWorkbook / SaveWorkbook / ExportPlotAsPNG / ExportPlotAsCSV /
//                                          ExportPlotToClipboard / ExportPlotToSchematic / Zoom / UndoZoom / RedoZoom / ToggleSimConsolePanel /
//                                          ToggleSimSidePanel / ToggleGrid / ToggleLegend / ToggleDottedSecondary / ToggleDarkModePlots /
//                                          EditAnalysisTab / RunSimulation / Probe / Tune / EditUserDefinedSignals / ShowNetlist (setTransitions)
//   eeschema/tools/sch_editor_control.cpp  SCH_EDITOR_CONTROL::ShowSimulator (the schematic editor's Inspect > Simulator), SimProbe, SimTune
//   eeschema/sim/simulator_frame.cpp       SIMULATOR_FRAME::setupUIConditions (state/simulator.ts `simEnabled` / `simChecked`)
//
// The window is components/simulator/SimulatorWindow.tsx and its state state/simulator.ts. KiCad's simulator is a frame with its own tools, so its
// hotkeys (Ctrl+N, Ctrl+O, Ctrl+S, R, ...) act in that frame only: here they act while the simulator window has the keyboard (`simEnabled` asks
// that), and the shared zoom actions (Zoom In and Out, Zoom to Fit, the grid) act on the plot then and on the canvas otherwise.
import type { Dispatch } from "react";
import type { Action, StudioApi } from "../state/store";
import type { ActionMap } from "./schActionRegistry";
import {
  editAnalysis,
  getSimUi,
  newAnalysisTab,
  openSimulator,
  openWorkbookFile,
  runOrStop,
  saveWorkbook,
  setSimDialog,
  setSimHost,
  showNetlist,
  stop,
  toggleDarkMode,
  touch,
} from "../state/simulator";
import { currentTab } from "../kicad-port/sim/simFrame";
import { fitWithUndo, zoomIn, zoomOut, zoomRedo, zoomUndo } from "../kicad-port/sim/mathPlot";
import { gridShown, setGrid } from "../kicad-port/sim/simPlotTab";

export interface SimActionContext {
  tab: string;
  dispatch: Dispatch<Action>;
  api: StudioApi;
}

/** A `.wbk` chosen (`wxFileDialog` "Open Simulation Workbook", `FILEEXT::WorkbookFileWildcard`). */
function chooseWorkbook(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".wbk";
    input.style.display = "none";
    let settled = false;
    const done = (f: File | null) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(f);
    };
    input.addEventListener("change", () => done(input.files?.[0] ?? null));
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/** The current tab's plot, when the window has the keyboard. */
const focusedPlot = () => {
  const s = getSimUi();
  return s.open && s.focused ? (currentTab(s.frame)?.plot ?? null) : null;
};

export function registerSimActions(m: ActionMap, ctx: SimActionContext): void {
  const { dispatch, api } = ctx;
  setSimHost({
    cmd: (c) => api.cmd(c),
    toast: (message, kind = "info") => dispatch({ type: "TOAST", message, kind }),
    // `SCH_EDIT_FRAME::ReadyToNetlist`: a reference still ending in '?' is not annotated.
    unannotated: () => (api.getState().schematic?.symbols ?? []).some((s) => s.id.endsWith("?")),
  });

  // SCH_EDITOR_CONTROL::ShowSimulator: the window opened (or brought up).
  m.set("eeschema.EditorControl.showSimulator", () => void openSimulator());

  m.set("eeschema.Simulation.newAnalysisTab", () => void newAnalysisTab());
  m.set("eeschema.Simulation.openWorkbook", () => {
    void chooseWorkbook().then((f) => f && openWorkbookFile(f));
  });
  m.set("eeschema.Simulation.saveWorkbook", () => void saveWorkbook(false));
  m.set("eeschema.Simulation.saveWorkbookAs", () => void saveWorkbook(true));
  m.set("eeschema.Simulation.simAnalysisProperties", () => void editAnalysis());
  m.set("eeschema.Simulation.runSimulation", () => void runOrStop());
  m.set("eeschema.Simulation.stopSimulation", () => {
    if (getSimUi().frame.running) void stop();
  });
  m.set("eeschema.Simulation.editUserDefinedSignals", () => setSimDialog({ kind: "userSignals" }));
  m.set("eeschema.Simulation.showNetlist", () => void showNetlist());

  m.set("eeschema.Simulator.toggleSimSidePanel", () => {
    const f = getSimUi().frame;
    f.sidePanelShown = !f.sidePanelShown;
    touch();
  });
  m.set("eeschema.Simulator.toggleSimConsole", () => {
    const f = getSimUi().frame;
    f.consoleShown = !f.consoleShown;
    touch();
  });
  m.set("eeschema.Simulator.toggleLegend", () => {
    const p = currentTab(getSimUi().frame)?.plot;
    if (!p) return;
    p.legend.shown = !p.legend.shown;
    touch(true);
  });
  m.set("eeschema.Simulator.toggleDottedSecondary", () => {
    const p = currentTab(getSimUi().frame)?.plot;
    if (!p) return;
    p.dotted = !p.dotted;
    touch(true);
  });
  m.set("eeschema.Simulator.toggleDarkModePlots", () => toggleDarkMode());

  // SIMULATOR_CONTROL::Zoom / UndoZoom / RedoZoom on the current plot.
  const plotZoom = (fn: (p: NonNullable<ReturnType<typeof focusedPlot>>) => void) => () => {
    const p = focusedPlot();
    if (!p) return;
    fn(p);
    touch();
  };
  m.set("common.Control.undoZoom", plotZoom((p) => zoomUndo(p.view)));
  m.set("common.Control.redoZoom", plotZoom((p) => zoomRedo(p.view)));
  m.set("common.Control.zoomInHorizontally", plotZoom((p) => zoomIn(p.view, "h")));
  m.set("common.Control.zoomOutHorizontally", plotZoom((p) => zoomOut(p.view, "h")));
  m.set("common.Control.zoomInVertically", plotZoom((p) => zoomIn(p.view, "v")));
  m.set("common.Control.zoomOutVertically", plotZoom((p) => zoomOut(p.view, "v")));
  // The canvases' own zoom and grid actions, which the simulator frame's toolbar and View menu also have: the plot's while the window has the keyboard.
  const shared = (name: string, fn: (p: NonNullable<ReturnType<typeof focusedPlot>>) => void) => {
    const other = m.get(name);
    m.set(name, (arg) => {
      const p = focusedPlot();
      if (p) {
        fn(p);
        touch();
      } else other?.(arg);
    });
  };
  shared("common.Control.zoomInCenter", (p) => zoomIn(p.view));
  shared("common.Control.zoomOutCenter", (p) => zoomOut(p.view));
  shared("common.Control.zoomFitScreen", (p) => fitWithUndo(p.view));
  // SIMULATOR_CONTROL::ToggleGrid
  shared("common.Control.toggleGrid", (p) => {
    setGrid(p, !gridShown(p));
    getSimUi().frame.modified = true;
  });
}
