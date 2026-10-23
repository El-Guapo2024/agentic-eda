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
import { addTuner, probeTrace, toggleOp } from "../state/simulator";
import { picker } from "./pcbPicker";
import { hitPin, pinItems } from "../kicad-port/schPins";
import { hitSymbol } from "../components/schematic/schHit";
import { measureStrokeText } from "../components/text/strokeFont";
import type { Schematic } from "../api/types";
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

/** The wire nearest a point, within `tol` (`SCH_SELECTION_TOOL::GetNode` for `SCH_ITEM_LOCATE_WIRE_T`); a bus is no node. */
function wireAt(sch: Schematic, x: number, y: number, tol: number): Schematic["wires"][number] | null {
  let best: { w: Schematic["wires"][number]; d: number } | null = null;
  for (const w of sch.wires) {
    if (w.bus) continue;
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const [x1, y1] = w.pts[i]!;
      const [x2, y2] = w.pts[i + 1]!;
      const dx = x2 - x1;
      const dy = y2 - y1;
      const k = Math.max(0, Math.min(1, ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy || 1)));
      const d = Math.hypot(x - (x1 + k * dx), y - (y1 + k * dy));
      if (d <= tol && (!best || d < best.d)) best = { w, d };
    }
  }
  return best?.w ?? null;
}

/** The SPICE net a net of the sheet is: through a pin the wire lands on, else any pin on the net (kicad-cli's XML netlist names the pins' nets). */
function spiceNetOf(sch: Schematic, wire: Schematic["wires"][number]): string | null {
  const nets = getSimUi().frame.circuit?.pin_nets ?? {};
  for (const p of wire.pins) if (nets[p]) return nets[p]!;
  for (const p of pinItems(sch)) if (p.net === wire.net && nets[`${p.reference}.${p.number}`]) return nets[`${p.reference}.${p.number}`]!;
  return null;
}

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
  // SIMULATOR_CONTROL::Probe -> SCH_EDITOR_CONTROL::SimProbe: a pick on the sheet until Escape. A pin is a current (`SPICE_GENERATOR::CurrentNames`, the one
  // of the model pin the symbol pin is when the device has several), a wire or a junction the voltage of its net (`V(<net>)`).
  m.set("eeschema.Simulation.probe", () => {
    picker.start({
      kind: "point",
      prompt: "Probe: a wire for its voltage, a pin for its current",
      onPoint: (at) => {
        const s = api.getState();
        const sch = s.schematic;
        if (!sch) return true;
        const tol = Math.max(5 / (s.schematicView.scale || 1), 254);
        const pin = pinItems(sch).find((p) => hitPin(p, [at.x, at.y], tol / 2, measureStrokeText));
        if (pin) {
          const item = getSimUi().frame.circuit?.items.find((i) => i.ref === pin.reference);
          if (!item || item.currents.length === 0) return true;
          if (item.currents.length === 1) probeTrace(item.currents[0]!, true);
          else {
            const idx = item.pin_index[pin.number];
            if (idx !== undefined && item.currents[idx]) probeTrace(item.currents[idx]!, true);
          }
          return true;
        }
        const wire = wireAt(sch, at.x, at.y, tol);
        if (wire) {
          const net = spiceNetOf(sch, wire);
          if (net) probeTrace(`V(${net})`, false);
        }
        return true;
      },
    });
  });
  // SIMULATOR_CONTROL::Tune -> SCH_EDITOR_CONTROL::SimTune: a symbol picked (or one of its fields), a tuner added for it.
  m.set("eeschema.Simulation.tune", () => {
    picker.start({
      kind: "point",
      prompt: "Tune: a symbol whose value to tune",
      onPoint: (at) => {
        const sch = api.getState().schematic;
        if (!sch) return true;
        const ref = hitSymbol(sch, at.x, at.y);
        if (ref) void addTuner(ref);
        return true;
      },
    });
  });
  // SCH_EDITOR_CONTROL::ToggleOPVoltages / ToggleOPCurrents
  m.set("eeschema.EditorControl.showOperatingPointVoltages", () => toggleOp("voltages"));
  m.set("eeschema.EditorControl.showOperatingPointCurrents", () => toggleOp("currents"));
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
