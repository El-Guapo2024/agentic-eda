// The simulator window's model: its tabs, signals, cursors, measurements, user-defined signals and tuners, and what a run does to them
// (eeschema/sim/simulator_frame_ui.cpp, simulator_frame.cpp, commit 8303b2ad):
//
//   SIMULATOR_FRAME_UI::NewSimTab / GetSimTab / AddTrace / updateTrace / OnSimRefresh / updatePlotCursors / CreateNewCursor / DeleteCursor /
//   onSignalsGridCellChanged / SetUserDefinedSignals / AddMeasurement / DeleteMeasurement / UpdateMeasurement / rebuildMeasurementsGrid /
//   SaveWorkbook / SaveCursorToWorkbook / loadJsonWorkbook / ToggleDarkModePlots, SIMULATOR_FRAME::UpdateTitle,
//   TUNER_SLIDER (its range, value and E-series rounding, eeschema/widgets/tuner_slider.cpp), SIMULATOR_FRAME_UI::calculateMultiRunSteps
//
// Pure: the window (components/simulator/) renders a `SimFrame`, state/simulator.ts talks to the server and puts the replies here.
import {
  OPTION_DEFAULT_FLAGS,
  SPT_AC_GAIN,
  SPT_AC_PHASE,
  SPT_CURRENT,
  SPT_POWER,
  SPT_SP_AMP,
  SPT_Y_AXIS_MASK,
  commandFromCommands,
  commandToSimType,
  commandsOf,
  isPlottable,
  longName,
  shortName,
  xAxisType,
  type SimType,
} from "./simCommand";
import { cssRgb, parseColor, resize, fit, type Rgb } from "./mathPlot";
import {
  deleteTrace,
  displayName,
  getOrAddTrace,
  gridShown,
  labelOf,
  newPlotTab,
  resetScales,
  setGrid,
  setTraceData,
  traceId,
  unitsOf,
  cursorValue,
  cursorStart,
  setTraceColor,
  ensureThirdYAxis,
  type PlotTab,
  type Trace,
} from "./simPlotTab";
import { measurementCommand, noiseSourceOf, signalsList, traceData, userVector, vectorOfSignal, type Circuit, type RawPlot } from "./simSignals";
import { PFX_NONE, SpiceValue, formatFixed, formatFromString, parseSiPrefix, formatToString, formatWithUnits, stripZeros, type SpiceValueFormat } from "./spiceValue";

export interface SimTab {
  id: number;
  /** "Analysis 1 - TRAN" (`NewSimTab`). */
  title: string;
  command: string;
  options: number;
  /** `SIM_TAB::GetLastSchTextSimCommand`. */
  lastSchText: string;
  /** `SIM_TAB::GetSpicePlotName`: set once a run has given the tab its plot. */
  plotName: string;
  plot: PlotTab | null;
  /** The plots of the tab's last run (the analysis's own, and a noise analysis's integrated noise). */
  plots: RawPlot[];
  secondPlots: RawPlot[];
  /** The measurements' results of the last run, by row. */
  measured: Map<number, number>;
}

export interface Tuner {
  ref: string;
  /** The item's name (`XU1` ...) and the parameter tuned. */
  item: string;
  param: string;
  min: SpiceValue;
  max: SpiceValue;
  value: SpiceValue;
  /** "1.5k": `TUNER_SLIDER::updateValueText`. */
  text: string;
  series: "" | "E24" | "E48" | "E96" | "E192";
  multi: boolean;
  steps: number;
}

export interface SimFrame {
  tabs: SimTab[];
  /** The index of the current tab, -1 for none. */
  current: number;
  /** `m_plotNumber`. */
  plotNumber: number;
  circuit: Circuit | null;
  /** `m_signals` and `m_netnames` of the current tab. */
  signals: string[];
  netnames: string[];
  userSignals: Map<number, string>;
  console: string;
  running: boolean;
  /** `SIMULATOR_FRAME::m_simFinished`. */
  finished: boolean;
  dark: boolean;
  consoleShown: boolean;
  sidePanelShown: boolean;
  filter: string;
  /** `m_customCursorsCnt`: 3 with the two fixed cursors. */
  customCursors: number;
  /** `m_cursorFormatsDyn`: cursor 1, cursor 2, the difference, then the custom cursors; x and y. */
  cursorFormats: SpiceValueFormat[][];
  tuners: Tuner[];
  /** `m_workbookModified`. */
  modified: boolean;
}

export function newFrame(dark = true): SimFrame {
  const f = (): SpiceValueFormat[] => [{ precision: 3, range: "~s" }, { precision: 3, range: "~V" }];
  return { tabs: [], current: -1, plotNumber: 0, circuit: null, signals: [], netnames: [], userSignals: new Map(), console: "", running: false, finished: false, dark, consoleShown: true, sidePanelShown: true, filter: "", customCursors: 3, cursorFormats: [f(), f(), f()], tuners: [], modified: false };
}

export const currentTab = (f: SimFrame): SimTab | null => f.tabs[f.current] ?? null;
export const simTypeOf = (t: { command: string } | null): SimType => (t ? commandToSimType(t.command) : "UNKNOWN");

/** `SIMULATOR_FRAME::GetCurrentSimCommand` / `GetCurrentOptions`. */
export function currentCommand(f: SimFrame): { command: string; options: number } {
  const t = currentTab(f);
  return t ? { command: t.command, options: t.options } : { command: f.circuit?.sch_sim_command ?? "", options: OPTION_DEFAULT_FLAGS };
}

/** `SIMULATOR_FRAME_UI::NewSimTab`: a plot tab for a plottable analysis, titled "Analysis N - TYPE", made current. */
export function newSimTab(f: SimFrame, command: string, options = OPTION_DEFAULT_FLAGS): SimTab {
  const type = commandToSimType(command);
  f.plotNumber += 1;
  const tab: SimTab = { id: f.plotNumber, title: `Analysis ${f.plotNumber} - ${shortName(type)}`, command, options, lastSchText: "", plotName: "", plot: isPlottable(type) ? newPlotTab(command) : null, plots: [], secondPlots: [], measured: new Map() };
  f.tabs.push(tab);
  f.current = f.tabs.length - 1;
  rebuildSignals(f);
  return tab;
}

/** `SIMULATOR_FRAME_UI::GetSimTab( type )`: the first tab of the analysis. */
export const simTabOf = (f: SimFrame, type: SimType): SimTab | null => f.tabs.find((t) => simTypeOf(t) === type) ?? null;

/** `SIMULATOR_FRAME::UpdateTitle`. */
export function frameTitle(f: SimFrame, workbookFilename: string, saved: boolean): string {
  const base = workbookFilename.replace(/^.*\//, "").replace(/\.[^.]*$/, "");
  let title = f.modified ? `*${base}` : base;
  if (!saved) title += " [Unsaved]";
  return `${title} — SPICE Simulator`;
}

/** `rebuildSignalsList` for the current tab. */
export function rebuildSignals(f: SimFrame): void {
  const { command, options } = currentCommand(f);
  const r = signalsList(commandToSimType(command), options, f.circuit);
  f.signals = r.signals;
  f.netnames = r.netnames;
}

/** `vectorNameFromSignalName` for a tab. */
export function vectorOf(f: SimFrame, tab: SimTab | null, signal: string): { vector: string; type: number } {
  const t = simTypeOf(tab);
  return vectorOfSignal(signal, t, f.userSignals, t === "NOISE" && tab ? noiseSourceOf(tab.command) : "");
}

/** The trace a signal is plotted as in the tab, if it is. */
export function traceOfSignal(f: SimFrame, tab: SimTab, signal: string): Trace | null {
  if (!tab.plot) return null;
  const { vector, type } = vectorOf(f, tab, signal);
  return tab.plot.traces.get(traceId(vector, type)) ?? null;
}

/** `SIMULATOR_FRAME_UI::updateTrace`: before a run only the trace; after it, its data from the tab's plot (the trace's type taking the x axis's bits). */
export function updateTrace(f: SimFrame, tab: SimTab, vector: string, traceType: number): Trace | null {
  if (!tab.plot) return null;
  if (!f.finished && !f.running) return getOrAddTrace(tab.plot, vector, traceType, f.dark);
  const t = simTypeOf(tab);
  let type = traceType & SPT_Y_AXIS_MASK;
  type |= xAxisType(t);
  if (!isPlottable(t)) return null;
  const data = traceData(t, tab.command, tab.plots[0], vector, type);
  const tr = getOrAddTrace(tab.plot, vector, type, f.dark);
  if (data) setTraceData(tab.plot, tr, data.x, data.y, data.sweepCount, data.sweepSize);
  return tr;
}

/** `SIMULATOR_FRAME_UI::AddTrace` (a probe): "Error: no current simulation.", "Error: simulation type not defined.", "Error: simulation type doesn't
 *  support plotting." to the console; an AC or SP signal as gain and phase. */
export function addTrace(f: SimFrame, name: string, kind: number): void {
  const tab = currentTab(f);
  if (!tab) {
    f.console += "Error: no current simulation.\n";
    return;
  }
  const t = simTypeOf(tab);
  if (t === "UNKNOWN") {
    f.console += "Error: simulation type not defined.\n";
    return;
  }
  if (!isPlottable(t)) {
    f.console += "Error: simulation type doesn't support plotting.\n";
    return;
  }
  if (t === "AC" || t === "SP") {
    updateTrace(f, tab, name, kind | SPT_AC_GAIN);
    updateTrace(f, tab, name, kind | SPT_AC_PHASE);
  } else updateTrace(f, tab, name, kind);
  f.modified = true;
}

/** `onSignalsGridCellChanged`, Plot column: the signal plotted, or its trace deleted. */
export function setSignalPlotted(f: SimFrame, signal: string, on: boolean): void {
  const tab = currentTab(f);
  if (!tab?.plot) return;
  const { vector, type } = vectorOf(f, tab, signal);
  if (on) updateTrace(f, tab, vector, type);
  else deleteTrace(tab.plot, vector, type) || deleteTrace(tab.plot, vector, type | xAxisType(simTypeOf(tab)));
  f.modified = true;
}

/** The trace of a signal in the current tab, whatever x bits it was made with. */
export function findTrace(f: SimFrame, tab: SimTab, signal: string): Trace | null {
  if (!tab.plot) return null;
  const { vector, type } = vectorOf(f, tab, signal);
  return tab.plot.traces.get(traceId(vector, type)) ?? null;
}

/** `onSignalsGridCellChanged`, a cursor column: the cursor put on this signal's trace and taken off every other's (the columns act as radio buttons). */
export function setCursor(f: SimFrame, signal: string, id: number, on: boolean): void {
  const tab = currentTab(f);
  if (!tab?.plot) return;
  const active = on ? findTrace(f, tab, signal) : null;
  if (active && !active.cursors.has(id)) active.cursors.set(id, { x: cursorStart(tab.plot, id), name: signal });
  for (const tr of tab.plot.traces.values()) if (tr !== active && tr.cursors.has(id)) tr.cursors.delete(id);
  f.modified = true;
}

/** `CreateNewCursor`: "Cursor 3" and on, with time and voltage formats. */
export function createCursor(f: SimFrame): void {
  f.cursorFormats.push([{ precision: 3, range: "~s" }, { precision: 3, range: "~V" }]);
  f.customCursors += 1;
  f.modified = true;
}

/** `DeleteCursor`: the last custom cursor goes, off every trace. */
export function deleteCursor(f: SimFrame): void {
  if (f.customCursors <= 3) return;
  const id = f.customCursors - 1;
  for (const tab of f.tabs) for (const tr of tab.plot?.traces.values() ?? []) tr.cursors.delete(id);
  f.cursorFormats.pop();
  f.customCursors -= 1;
  f.modified = true;
}

/** The cursor-grid columns of the signals grid: "Cursor 1", "Cursor 2", then "Cursor 3" and on. */
export function cursorColumns(f: SimFrame): number[] {
  const ids = [1, 2];
  for (let i = 3; i < f.customCursors; i++) ids.push(i);
  return ids;
}

export interface CursorRow {
  name: string;
  signal: string;
  x: string;
  y: string;
  /** The cursor this row moves, null for the difference. */
  id: number | null;
}

/** `SIMULATOR_FRAME_UI::updatePlotCursors`: the cursors grid's rows and its column labels. */
export function cursorRows(f: SimFrame): { rows: CursorRow[]; xLabel: string; yLabel: string } {
  const tab = currentTab(f);
  const rows: CursorRow[] = [];
  if (!tab?.plot) return { rows, xLabel: "Time", yLabel: "Value" };
  const plot = tab.plot;
  const t = simTypeOf(tab);
  const unitsY = (tr: Trace) => (t === "AC" ? unitsOf(tr.type & SPT_AC_PHASE ? plot.y2 : plot.y1) : unitsOf(tr.type & SPT_POWER ? plot.y3 : tr.type & SPT_CURRENT ? plot.y2 : plot.y1));
  const nameY = (tr: Trace) => (t === "AC" ? labelOf(tr.type & SPT_AC_PHASE ? plot.y2 : plot.y1) : labelOf(tr.type & SPT_POWER ? plot.y3 : tr.type & SPT_CURRENT ? plot.y2 : plot.y1));
  const fmt = (v: number, i: number, col: number) => (!f.finished && col === 1) || Number.isNaN(v) ? "--" : SpiceValue.of(v).toFormatted(f.cursorFormats[i]![col]!);
  const traces = [...plot.traces.entries()].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)).map(([, tr]) => tr);
  const at = (id: number) => {
    for (const tr of traces) {
      const c = tr.cursors.get(id);
      if (c) return { tr, x: c.x, y: cursorValue(tr, c.x, c.sweep ?? 0), name: c.name ?? displayName(tr) };
    }
    return null;
  };
  const c1 = at(1);
  const c2 = at(2);
  let n1 = "";
  let n2 = "";
  let u1 = "";
  let u2 = "";
  const unitsX = unitsOf(plot.x);
  if (c1) {
    n1 = nameY(c1.tr);
    u1 = unitsY(c1.tr);
    f.cursorFormats[0]![0] = formatWithUnits(f.cursorFormats[0]![0]!, unitsX);
    f.cursorFormats[0]![1] = formatWithUnits(f.cursorFormats[0]![1]!, u1);
    rows.push({ name: "1", signal: c1.name, x: fmt(c1.x, 0, 0), y: fmt(c1.y, 0, 1), id: 1 });
  }
  if (c2) {
    n2 = nameY(c2.tr);
    u2 = unitsY(c2.tr);
    f.cursorFormats[1]![0] = formatWithUnits(f.cursorFormats[1]![0]!, unitsX);
    f.cursorFormats[1]![1] = formatWithUnits(f.cursorFormats[1]![1]!, u2);
    rows.push({ name: "2", signal: c2.name, x: fmt(c2.x, 1, 0), y: fmt(c2.y, 1, 1), id: 2 });
  }
  if (c1 && c2 && u1 === u2) {
    f.cursorFormats[2]![0] = formatWithUnits(f.cursorFormats[2]![0]!, unitsX);
    f.cursorFormats[2]![1] = formatWithUnits(f.cursorFormats[2]![1]!, u1);
    const signal = c1.name === c2.name ? `${c2.name}[2 - 1]` : `${c2.name} - ${c1.name}`;
    rows.splice(2, 0, { name: "Diff", signal, x: fmt(c2.x - c1.x, 2, 0), y: fmt(c2.y - c1.y, 2, 1), id: null });
  }
  let yLabel = "Value";
  if (n1 !== "") {
    if (n2 === "" || n1 === n2) yLabel = n1;
  } else if (n2 !== "") yLabel = n2;
  for (let i = 3; i < f.customCursors; i++) {
    const c = at(i);
    if (!c) continue;
    const u = unitsY(c.tr);
    f.cursorFormats[i]![0] = formatWithUnits(f.cursorFormats[i]![0]!, unitsX);
    f.cursorFormats[i]![1] = formatWithUnits(f.cursorFormats[i]![1]!, u);
    rows.push({ name: `${i}`, signal: c.name, x: fmt(c.x, i, 0), y: fmt(c.y, i, 1), id: i });
    const name = nameY(c.tr);
    yLabel = name !== "" && yLabel === name ? name : "Value";
  }
  return { rows, xLabel: labelOf(plot.x), yLabel };
}

/** `onCursorsGridCellChanged`: a cursor's x typed in (a SPICE value); the difference row moves cursor 2 to cursor 1 plus it. */
export function setCursorX(f: SimFrame, row: CursorRow, text: string): void {
  const tab = currentTab(f);
  if (!tab?.plot) return;
  // `SPICE_VALUE( text )`: a text with no number puts the cursor at 0
  const value = SpiceValue.parse(text).toDouble();
  const find = (id: number) => [...tab.plot!.traces.values()].map((tr) => tr.cursors.get(id)).find((c) => c);
  if (row.id !== null) {
    const c = find(row.id);
    if (c) c.x = value;
  } else {
    const c1 = find(1);
    const c2 = find(2);
    if (c1 && c2) c2.x = c1.x + value;
  }
  f.modified = true;
}

/** A cursor dragged on the plot to `x` (data units of its trace's x axis). */
export function moveCursor(f: SimFrame, tr: Trace, id: number, x: number): void {
  const c = tr.cursors.get(id);
  if (c) c.x = x;
  f.modified = true;
}

/** `WX_GRID::OnDeleteRows` (common/widgets/wx_grid.cpp): the selected rows, else the grid cursor's row, are deleted (none: nothing happens), and
 *  the cursor goes to the row above the lowest deleted one when there is one. User-defined Signals' Delete (`onDeleteSignal`). */
export function deleteGridRows<T>(rows: readonly T[], selected: readonly number[], cursor: number): { rows: T[]; cursor: number } {
  const del = [...new Set(selected)];
  if (del.length === 0 && cursor >= 0 && cursor < rows.length) del.push(cursor);
  if (del.length === 0) return { rows: [...rows], cursor };
  del.sort((a, b) => b - a);
  const next = del[del.length - 1]! - 1;
  const gone = new Set(del);
  return { rows: rows.filter((_, i) => !gone.has(i)), cursor: next >= 0 ? next : cursor };
}

/** `SetUserDefinedSignals`: the traces of a signal that went are deleted, those of one that changed renamed. */
export function setUserSignals(f: SimFrame, signals: Map<number, string>): void {
  for (const tab of f.tabs) {
    if (!tab.plot) continue;
    const t = simTypeOf(tab);
    for (const [id, existing] of f.userSignals) {
      const { vector, type } = vectorOf(f, tab, existing);
      const subs = t === "AC" ? [SPT_AC_GAIN, SPT_AC_PHASE] : t === "SP" ? [SPT_SP_AMP, SPT_AC_PHASE] : [0];
      for (const sub of subs) {
        for (const [key, tr] of [...tab.plot.traces]) {
          if (tr.vector !== vector || (tr.type & SPT_Y_AXIS_MASK) !== ((type | sub) & SPT_Y_AXIS_MASK)) continue;
          if (!signals.has(id)) tab.plot.traces.delete(key);
          else tr.name = signals.get(id)!;
        }
      }
    }
  }
  f.userSignals = new Map(signals);
  rebuildSignals(f);
  f.modified = true;
}

/** `AddMeasurement` (no duplicate) and `DeleteMeasurement`. */
export function addMeasurement(f: SimFrame, expr: string): boolean {
  const tab = currentTab(f);
  if (!tab?.plot) return false;
  if (tab.plot.measurements.some((m) => m.expr === expr)) return false;
  tab.plot.measurements.push({ expr, format: measurementCommand(expr, simTypeOf(tab), "").format });
  f.modified = true;
  return true;
}

export function setMeasurement(f: SimFrame, row: number, expr: string): void {
  const tab = currentTab(f);
  if (!tab?.plot) return;
  const ms = tab.plot.measurements;
  if (row >= ms.length) {
    if (expr !== "") ms.push({ expr, format: measurementCommand(expr, simTypeOf(tab), "").format });
  } else if (expr === "") ms.splice(row, 1);
  else ms[row] = { expr, format: measurementCommand(expr, simTypeOf(tab), ms[row]!.format).format };
  tab.measured.clear();
  f.modified = true;
}

export function deleteMeasurement(f: SimFrame, row: number): void {
  const tab = currentTab(f);
  if (!tab?.plot || row >= tab.plot.measurements.length) return;
  tab.plot.measurements.splice(row, 1);
  tab.measured.clear();
  f.modified = true;
}

/** The measurements a run of the tab asks ngspice for (`UpdateMeasurement`, which `OnSimRefresh` calls for every row): the row and what follows
 *  `meas <TYPE> meas_result_<row>`. Each row's format gets its units again (`fmt.UpdateUnits( units ); SetMeasureFormat( aRow, fmt )`), so a
 *  measurement read from a workbook or kept across a change of analysis is written in the units it has now. */
export function measurementRequests(tab: SimTab): { row: number; text: string }[] {
  const out: { row: number; text: string }[] = [];
  tab.plot?.measurements.forEach((m, row) => {
    const c = measurementCommand(m.expr, simTypeOf(tab), m.format);
    m.format = c.format;
    if (c.text) out.push({ row, text: c.text });
  });
  return out;
}

/** What a finished run gives the frame (`SIMULATOR_FRAME::onSimFinished`, `OnSimRefresh( true )`): the tab's plots, its traces' data (a trace
 *  whose signal the analysis no longer has is deleted), its measurements, its scales reset and the view fitted. */
export function applyRun(f: SimFrame, tab: SimTab, plots: RawPlot[], secondPlots: RawPlot[], console: string): void {
  f.running = false;
  f.finished = true;
  tab.plots = plots;
  tab.secondPlots = secondPlots;
  tab.plotName = plots[0]?.plot_name ?? "";
  f.console = console;
  const t = simTypeOf(tab);
  rebuildSignals(f);
  tab.measured.clear();
  for (const v of plots[0]?.vectors ?? []) {
    const m = /^meas_result_(\d+)$/.exec(v.name);
    if (m && v.re.length > 0) tab.measured.set(Number(m[1]), v.re[0]!);
  }
  if (!isPlottable(t) || !tab.plot) return;
  const plot = tab.plot;
  const known = new Map<string, { vector: string; type: number }>();
  for (const s of f.signals) {
    const v = vectorOf(f, tab, s);
    known.set(traceId(v.vector, v.type), v);
  }
  for (const s of f.userSignals.values()) {
    const base = vectorOf(f, tab, s);
    const subs = t === "AC" ? [SPT_AC_GAIN, SPT_AC_PHASE] : t === "SP" ? [SPT_SP_AMP, SPT_AC_PHASE] : [0];
    for (const sub of subs) {
      const type = (base.type & ~(SPT_AC_GAIN | SPT_AC_PHASE | SPT_SP_AMP)) | sub;
      known.set(traceId(base.vector, type), { vector: base.vector, type });
    }
  }
  for (const [key, tr] of [...plot.traces]) {
    const k = known.get(key) ?? (tr.vector.startsWith("user") ? undefined : known.get(traceId(tr.vector, tr.type)));
    if (!k) {
      plot.traces.delete(key);
      continue;
    }
    const data = traceData(t, tab.command, plots[0], tr.vector, tr.type);
    if (data) setTraceData(plot, tr, data.x, data.y, data.sweepCount, data.sweepSize);
  }
  resetScales(plot, true);
  fit(plot.view);
}

/** `SIMULATOR_FRAME_UI::ToggleDarkModePlots`. */
export function toggleDark(f: SimFrame): void {
  f.dark = !f.dark;
}

// ------------------------------------------------------------------------------------------------------------- tuners

const E24 = [1.0, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2.0, 2.2, 2.4, 2.7, 3.0, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1, 5.6, 6.2, 6.8, 7.5, 8.2, 9.1];
const E192 = [
  1.0, 1.01, 1.02, 1.04, 1.05, 1.06, 1.07, 1.09, 1.1, 1.11, 1.13, 1.14, 1.15, 1.17, 1.18, 1.2, 1.21, 1.23, 1.24, 1.26, 1.27, 1.29, 1.3, 1.32, 1.33, 1.35, 1.37, 1.38, 1.4, 1.42, 1.43, 1.45,
  1.47, 1.49, 1.5, 1.52, 1.54, 1.56, 1.58, 1.6, 1.62, 1.64, 1.65, 1.67, 1.69, 1.72, 1.74, 1.76, 1.78, 1.8, 1.82, 1.84, 1.87, 1.89, 1.91, 1.93, 1.96, 1.98, 2.0, 2.03, 2.05, 2.08, 2.1, 2.13,
  2.15, 2.18, 2.21, 2.23, 2.26, 2.29, 2.32, 2.34, 2.37, 2.4, 2.43, 2.46, 2.49, 2.52, 2.55, 2.58, 2.61, 2.64, 2.67, 2.71, 2.74, 2.77, 2.8, 2.84, 2.87, 2.91, 2.94, 2.98, 3.01, 3.05, 3.09,
  3.12, 3.16, 3.2, 3.24, 3.28, 3.32, 3.36, 3.4, 3.44, 3.48, 3.52, 3.57, 3.61, 3.65, 3.7, 3.74, 3.79, 3.83, 3.88, 3.92, 3.97, 4.02, 4.07, 4.12, 4.17, 4.22, 4.27, 4.32, 4.37, 4.42, 4.48, 4.53,
  4.59, 4.64, 4.7, 4.75, 4.81, 4.87, 4.93, 4.99, 5.05, 5.11, 5.17, 5.23, 5.3, 5.36, 5.42, 5.49, 5.56, 5.62, 5.69, 5.76, 5.83, 5.9, 5.97, 6.04, 6.12, 6.19, 6.26, 6.34, 6.42, 6.49, 6.57,
  6.65, 6.73, 6.81, 6.9, 6.98, 7.06, 7.15, 7.23, 7.32, 7.41, 7.5, 7.59, 7.68, 7.77, 7.87, 7.96, 8.06, 8.16, 8.25, 8.35, 8.45, 8.56, 8.66, 8.76, 8.87, 8.98, 9.09, 9.2, 9.31, 9.42, 9.53,
  9.65, 9.76, 9.88,
];

/** `TUNER_SLIDER::updateValueText`: the value with three decimals and its SPICE prefix, or the nearest value of the chosen E series. */
export function tunerText(value: SpiceValue, series: Tuner["series"]): string {
  const { value: v, prefix } = new SpiceValue(value.base, value.prefix).toNormalized();
  if (series !== "") {
    const extended = series !== "E24";
    let table: number[] = [];
    if (extended) {
      const step = series === "E48" ? 4 : series === "E96" ? 2 : 1;
      for (let i = 0; i < E192.length; i += step) table.push(E192[i]!);
    } else table.push(...E24);
    table.push(10.0);
    table = [...new Set(table)].sort((a, b) => a - b);
    for (const decade of [1.0, 10.0, 100.0]) {
      for (let i = 0; i < table.length - 1; i++) {
        if (v < ((table[i]! + table[i + 1]!) * decade) / 2) {
          let precision = 0;
          if (decade === 1.0) precision++;
          if (extended && decade !== 100.0) precision++;
          return `${formatFixed(table[i]! * decade, precision)}${prefix}`;
        }
      }
    }
  }
  return `${stripZeros(formatFixed(v, 3))}${prefix}`;
}

/** `TUNER_SLIDER::TUNER_SLIDER`: the range from half to twice the value; for a potentiometer, 0 to 1. Null with KiCad's reason when the item
 *  is not tunable. */
export function newTuner(circuit: Circuit | null, ref: string): Tuner | { error: string } {
  const item = circuit?.items.find((i) => i.ref === ref);
  if (!item) return { error: `${ref} not found` };
  if (!item.tuner) return { error: `${ref} has simulation model of type '${item.sim_device ?? item.device} ${item.sim_type ?? ""}'.\n\nOnly passive R, L, C models and voltage and current sources may be tuned.` };
  const value = SpiceValue.parse(item.tuner.value);
  const t: Tuner = { ref, item: item.name, param: item.tuner.param, min: SpiceValue.of(0.5).mul(value), max: SpiceValue.of(2.0).mul(value), value, text: "", series: "", multi: false, steps: 5 };
  t.text = tunerText(t.value, t.series);
  return t;
}

/** `DIALOG_SIM_FORMAT_VALUE::DIALOG_SIM_FORMAT_VALUE`: the units the range choices are made of, tested in KiCad's order -- the last letter for
 *  "V", "A", "s" (so an integral's "V.s" is in seconds), two for "Hz", three for "dB" ("~dB" whole), one for "°", then what follows '~', then
 *  the range without its SI prefix. */
export function formatDialogUnits(range: string): string {
  if (range.endsWith("V") || range.endsWith("A") || range.endsWith("s")) return range.slice(-1);
  if (range.endsWith("Hz")) return range.slice(-2);
  if (range.endsWith("dB")) return range.slice(-3);
  if (range.endsWith("°")) return range.slice(-1);
  if (range.startsWith("~")) return range.slice(1);
  return parseSiPrefix(range.charAt(0)) !== PFX_NONE ? range.slice(1) : range;
}

/** `TUNER_SLIDER::onStepsTextEnter` on the spin control (`SetRange( 2, 100 )`): the step count typed, the old one when it is no number. */
export function tunerSteps(text: string, old: number): number {
  const n = /^\s*[+-]?\d+\s*$/.test(text) ? Number.parseInt(text, 10) : old;
  return Math.min(100, Math.max(2, n));
}

/** `TUNER_SLIDER::SetValue`: within the range. */
export function setTunerValue(t: Tuner, v: SpiceValue): void {
  t.value = v.toDouble() > t.max.toDouble() ? t.max : v.toDouble() < t.min.toDouble() ? t.min : v;
  t.text = tunerText(t.value, t.series);
}

/** `TUNER_SLIDER::SetMin` / `SetMax`: refused when it would cross the other end; the value kept inside. */
export function setTunerRange(t: Tuner, which: "min" | "max", v: SpiceValue): boolean {
  if (which === "min") {
    if (v.toDouble() >= t.max.toDouble()) return false;
    t.min = v;
    if (t.value.toDouble() < v.toDouble()) setTunerValue(t, v);
  } else {
    if (v.toDouble() <= t.min.toDouble()) return false;
    t.max = v;
    if (t.value.toDouble() > v.toDouble()) setTunerValue(t, v);
  }
  return true;
}

/** `TUNER_SLIDER::updateSlider` (0..100) and `onSliderChanged`. */
export const sliderOf = (t: Tuner) => Math.round(((t.value.toDouble() - t.min.toDouble()) / (t.max.toDouble() - t.min.toDouble())) * 100);
export function setTunerSlider(t: Tuner, pos: number): void {
  t.value = t.min.add(t.max.sub(t.min).mul(SpiceValue.of(pos / 100.0)));
  t.text = tunerText(t.value, t.series);
}

/** The value a run tunes to: `applyTuners`' `tuner->GetValue().ToDouble()`, the tuner's `m_value` -- not its text, which `updateValueText`
 *  rounds to three digits or to the E series for show only. */
export const tunedValue = (t: Tuner) => t.value.toDouble();

/** `SIMULATOR_FRAME_UI::calculateMultiRunSteps`: every combination of the multi-run tuners' steps, at most `limit` (KiCad's advanced setting
 *  `SimulatorMultiRunCombinationLimit`, 12). */
export function multiRunSteps(tuners: Tuner[], limit = 12): Map<string, number>[] {
  const steps: Map<string, number>[] = [];
  if (tuners.length === 0) return steps;
  const values = tuners.map((t) => {
    const a = t.min.toDouble();
    const b = t.max.toDouble();
    const n = Math.max(2, t.steps);
    const inc = (b - a) / (n - 1);
    return Array.from({ length: n }, (_, i) => a + inc * i);
  });
  const cur = new Array<number>(tuners.length).fill(0);
  const gen = (depth: number): void => {
    if (steps.length >= Math.max(1, limit)) return;
    if (depth === tuners.length) {
      steps.push(new Map(tuners.map((t, i) => [t.ref, cur[i]!])));
      return;
    }
    for (const v of values[depth]!) {
      cur[depth] = v;
      gen(depth + 1);
      if (steps.length >= Math.max(1, limit)) return;
    }
  };
  gen(0);
  return steps;
}

// ------------------------------------------------------------------------------------------------------------- the workbook

/** `SIMULATOR_FRAME_UI::SaveWorkbook`: the workbook as KiCad's JSON (the server writes it out byte for byte, `/api/sim/workbook/text`). */
export function toWorkbook(f: SimFrame): Record<string, unknown> {
  const tabs = f.tabs.map((tab) => {
    const t = simTypeOf(tab);
    const js: Record<string, unknown> = { analysis: shortName(t), commands: commandsOf(tab.command, tab.options) };
    if (tab.plot) {
      const plot = tab.plot;
      const findSignalName = (display: string) => {
        let suffix = "";
        if (display.endsWith(" (phase)")) suffix = " (phase)";
        else if (display.endsWith(" (gain)")) suffix = " (gain)";
        const v = display.slice(0, display.length - suffix.length);
        for (const [id, s] of f.userSignals) if (v === userVector(id)) return s + suffix;
        return display;
      };
      const traces = [...plot.traces.entries()]
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
        .map(([, tr]) => {
          const o: Record<string, unknown> = { trace_type: tr.type, signal: findSignalName(displayName(tr)), color: cssRgb(tr.color) };
          for (let ii = 1; ii <= f.customCursors; ii++) {
            const after = ii > 3 ? ii - 1 : ii;
            const c = tr.cursors.get(ii);
            const fm = f.cursorFormats[after] ?? f.cursorFormats[0]!;
            if (c) o[`cursor${ii}`] = { position: c.x, x_format: formatToString(fm[0]!), y_format: formatToString(fm[1]!) };
            if (after < 3 && (tr.cursors.has(1) || tr.cursors.has(2))) o.cursorD = { x_format: formatToString(f.cursorFormats[2]![0]!), y_format: formatToString(f.cursorFormats[2]![1]!) };
          }
          return o;
        });
      js.traces = traces;
      js.measurements = plot.measurements.map((m) => ({ expr: m.expr, format: m.format }));
      js.dottedSecondary = plot.dotted;
      js.showGrid = gridShown(plot);
      const lock = (s: PlotTab["y1"]) => (s && s.locked ? { min: s.axisMin, max: s.axisMax } : null);
      if (lock(plot.y1)) js.fixedY1scale = lock(plot.y1);
      if (lock(plot.y2)) js.fixedY2scale = lock(plot.y2);
      if (lock(plot.y3)) js.fixedY3scale = lock(plot.y3);
      if (plot.legend.shown) js.legend = { x: plot.legend.x, y: plot.legend.y };
      js.margins = { left: plot.view.marginLeft, right: plot.view.marginRight, top: plot.view.marginTop, bottom: plot.view.marginBottom };
    }
    return js;
  });
  const js: Record<string, unknown> = { version: 7, tabs, user_defined_signals: [...f.userSignals.entries()].sort(([a], [b]) => a - b).map(([, s]) => s), custom_cursors: f.customCursors - 1 };
  if (f.tabs.length > 0) js.last_sch_text_sim_command = f.tabs[0]!.lastSchText;
  return js;
}

type Js = Record<string, unknown>;
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const num = (v: unknown, d = 0): number => (typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" && !Number.isNaN(Number(v)) ? Number(v) : d);
const str = (v: unknown, d = ""): string => (typeof v === "string" ? v : d);

/** `SIMULATOR_FRAME_UI::loadJsonWorkbook`: the frame made from a workbook (KiCad's JSON, as `/api/sim/workbook/parse` gives it). */
export function fromWorkbook(f: SimFrame, js: Js): void {
  f.tabs = [];
  f.current = -1;
  f.userSignals = new Map();
  const traceInfo: [SimTab, Js[]][] = [];
  for (const tabJs of arr(js.tabs) as Js[]) {
    const { command, options } = commandFromCommands(arr(tabJs.commands).map((c) => str(c)));
    const tab = newSimTab(f, command, options);
    const plot = tab.plot;
    if (!plot) continue;
    if (Array.isArray(tabJs.traces)) traceInfo.push([tab, tabJs.traces as Js[]]);
    for (const m of arr(tabJs.measurements) as Js[]) plot.measurements.push({ expr: str(m.expr), format: str(m.format) });
    plot.dotted = tabJs.dottedSecondary === true;
    setGrid(plot, tabJs.showGrid !== false);
    for (const [key, scale] of [["fixedY1scale", "y1"], ["fixedY2scale", "y2"], ["fixedY3scale", "y3"]] as const) {
      const s = tabJs[key] as Js | undefined;
      if (!s) continue;
      if (scale === "y3") ensureThirdYAxis(plot);
      const sc = plot[scale];
      if (sc) {
        sc.locked = true;
        sc.axisMin = num(s.min);
        sc.axisMax = num(s.max);
      }
      plot.view.yLocked = true;
    }
    const legend = tabJs.legend as Js | undefined;
    if (legend) plot.legend = { shown: true, x: num(legend.x), y: num(legend.y) };
    const margins = tabJs.margins as Js | undefined;
    if (margins) {
      plot.view.marginTop = num(margins.top, 30);
      plot.view.marginRight = num(margins.right, 70);
      plot.view.marginBottom = num(margins.bottom, 45);
      plot.view.marginLeft = num(margins.left, 70);
    }
  }
  arr(js.user_defined_signals).forEach((s, i) => f.userSignals.set(i, str(s)));
  if (f.tabs[0]) f.tabs[0].lastSchText = str(js.last_sch_text_sim_command);
  let temp = js.custom_cursors !== undefined ? num(js.custom_cursors, 2) : 2;
  if (temp > f.customCursors && f.customCursors > 2) temp = 2 * temp - f.customCursors;
  const base = f.customCursors;
  for (let yy = 0; yy <= temp - base; yy++) createCursor(f);
  for (const [tab, traces] of traceInfo) {
    for (const trJs of traces) {
      const signal = str(trJs.signal);
      const { vector } = vectorOf(f, tab, signal);
      const tr = getOrAddTrace(tab.plot!, vector, num(trJs.trace_type), f.dark);
      const addCursor = (id: number, c: Js) => {
        if (id >= 1) tr.cursors.set(id, { x: num(c.position), name: signal });
        const i = id === -1 ? 2 : id < 3 ? id - 1 : id;
        const fm = f.cursorFormats[i];
        if (fm) {
          fm[0] = formatFromString(str(c.x_format));
          fm[1] = formatFromString(str(c.y_format));
        }
      };
      if (trJs.cursorD) addCursor(-1, trJs.cursorD as Js);
      for (let i = 1; i <= temp; i++) if (trJs[`cursor${i}`]) addCursor(i, trJs[`cursor${i}`] as Js);
      const color = typeof trJs.color === "string" ? parseColor(trJs.color) : null;
      if (color) setTraceColor(tab.plot!, tr, color);
    }
  }
  if (f.tabs.length > 0) f.current = 0;
  rebuildSignals(f);
  f.modified = false;
}

/** The plot area sized (the canvas's size), fitted when it never was. */
export function sizePlot(tab: SimTab, w: number, h: number): void {
  if (!tab.plot) return;
  resize(tab.plot.view, w, h);
}

export { longName, shortName };
export type { Rgb };
