// A plot tab of the simulator (`SIM_PLOT_TAB`, `TRACE`, `CURSOR`; eeschema/sim/sim_plot_tab.cpp, sim_plot_tab.h, commit 8303b2ad): its axes as the
// analysis wants them, its traces with their colours and styles, the data a run gives a trace, the cursors on the traces. Pure model; the canvas
// is SimPlot.tsx's.
import {
  SPT_AC_GAIN,
  SPT_AC_PHASE,
  SPT_CURRENT,
  SPT_POWER,
  SPT_VOLTAGE,
  SPT_Y_AXIS_MASK,
  commandToSimType,
  type SimType,
} from "./simCommand";
import { extendDataRange, generateColor, newScale, newView, resetDataRange, valueAt, type PlotView, type Rgb, type Scale } from "./mathPlot";
import { SpiceValue } from "./spiceValue";

export interface Cursor {
  /** The cursor's x, in the trace's units (`CURSOR::m_coords.x`). */
  x: number;
  /** `CURSOR::GetName`: the signal it was put on as the signals grid names it (`EnableCursor( trace, id, signalName )`: "V(a) (gain)", a
   *  user-defined signal's expression), what the cursors grid's Signal column shows. */
  name?: string;
  /** `CURSOR::m_sweepIndex`: the run of a multi-run trace it reads. */
  sweep?: number;
}

export interface Trace {
  /** The vector the trace plots: `V(/out)`, `I(R1)`, `user0`. */
  vector: string;
  /** `SIM_TRACE_TYPE` bits. */
  type: number;
  /** `TRACE::GetName` (the vector it was made for, or a user-defined signal's text once renamed). */
  name: string;
  color: Rgb;
  /** By cursor number. */
  cursors: Map<number, Cursor>;
  x: number[];
  y: number[];
  sweepCount: number;
  /** Points per sweep; Infinity for one sweep (`std::numeric_limits<size_t>::max()`). */
  sweepSize: number;
  /** `TRACE::IsMultiRun`: the sweeps are the runs of multi-run tuners. */
  multiRun?: boolean;
}

/** `TRACE::GetDisplayName`. */
export function displayName(t: { name: string; type: number }): string {
  if (t.type & SPT_AC_GAIN) return `${t.name} (gain)`;
  if (t.type & SPT_AC_PHASE) return `${t.name} (phase)`;
  return t.name;
}

export interface PlotTab {
  command: string;
  x: Scale | null;
  y1: Scale | null;
  y2: Scale | null;
  y3: Scale | null;
  view: PlotView;
  /** By `getTraceId`: vector name and the trace type's y bits. */
  traces: Map<string, Trace>;
  /** `m_sessionTraceColors`, by trace name. */
  sessionColors: Map<string, Rgb>;
  dotted: boolean;
  legend: { shown: boolean; x: number; y: number };
  /** `(expr, format)` and the value last measured. */
  measurements: { expr: string; format: string }[];
}

/** `SIM_PLOT_TAB::getTraceId`. */
export const traceId = (vector: string, type: number) => `${vector}${type & SPT_Y_AXIS_MASK}`;

export const simTypeOf = (tab: { command: string }): SimType => commandToSimType(tab.command);

/** `SIM_PLOT_TAB::SIM_PLOT_TAB`: margins 30, 70, 45, 70, the axes for the command, the legend hidden at (0, 0). */
export function newPlotTab(command: string): PlotTab {
  const tab: PlotTab = { command, x: null, y1: null, y2: null, y3: null, view: newView(), traces: new Map(), sessionColors: new Map(), dotted: false, legend: { shown: false, x: 0, y: 0 }, measurements: [] };
  updateAxes(tab);
  return tab;
}

/** `SIM_PLOT_TAB::EnsureThirdYAxisExists`. */
export function ensureThirdYAxis(tab: PlotTab): void {
  if (!tab.y3) {
    tab.view.marginRight = 160;
    tab.y3 = newScale("lin", "W", "border_right");
    tab.y3.nameAlign = "border_right";
    tab.y3.master = tab.y1;
  }
  tab.y3.align = "border_right";
  tab.y3.nameAlign = "border_right";
  if (tab.y2) tab.y2.nameAlign = "border_right";
}

/** `SIM_PLOT_TAB::updateAxes` and `prepareDCAxes`. */
export function updateAxes(tab: PlotTab, newTraceType = 0): void {
  const t = simTypeOf(tab);
  const named = (s: Scale | null, base: string) => {
    if (s) {
      s.base = base;
      s.name = base;
    }
  };
  switch (t) {
    case "AC":
    case "SP":
      if (!tab.x) {
        tab.x = newScale("log", "Hz", "bottom");
        tab.y1 = newScale("lin", t === "AC" ? "dB" : "", "left");
        tab.y2 = newScale("lin", "°", "right");
        tab.y2.master = tab.y1;
      }
      named(tab.x, "Frequency");
      named(tab.y1, t === "AC" ? "Gain" : "Amplitude");
      named(tab.y2, "Phase");
      break;
    case "DC": {
      const rem = tab.command.toLowerCase().slice(3).replace(/^\s+/, "");
      const ch = rem.charAt(0);
      if (!tab.x) {
        const unit = ch === "i" ? "A" : ch === "r" ? "Ω" : ch === "t" ? "°C" : "V";
        tab.x = newScale("lin", unit, "bottom");
      }
      named(tab.x, ch === "i" ? "Current (swept)" : ch === "r" ? "Resistance (swept)" : ch === "t" ? "Temperature (swept)" : "Voltage (swept)");
      if (!tab.y1) tab.y1 = newScale("lin", "V", "left");
      if (!tab.y2) tab.y2 = newScale("lin", "A", "right");
      named(tab.y1, "Voltage (measured)");
      named(tab.y2, "Current");
      if (newTraceType & SPT_POWER) ensureThirdYAxis(tab);
      named(tab.y3, "Power");
      break;
    }
    case "NOISE":
      if (!tab.x) {
        tab.x = newScale("log", "Hz", "bottom");
        if ((newTraceType & SPT_CURRENT) === 0) tab.y1 = newScale("lin", "", "left");
        else tab.y2 = newScale("lin", "", "right");
      }
      named(tab.x, "Frequency");
      named(tab.y1, "Noise (V/√Hz)");
      named(tab.y2, "Noise (A/√Hz)");
      break;
    case "FFT":
      if (!tab.x) {
        tab.x = newScale("log", "Hz", "bottom");
        tab.y1 = newScale("lin", "dB", "left");
      }
      named(tab.x, "Frequency");
      named(tab.y1, "Intensity");
      break;
    case "TRAN":
      if (!tab.x) {
        tab.x = newScale("time", "s", "bottom");
        tab.y1 = newScale("lin", "V", "left");
        tab.y2 = newScale("lin", "A", "right");
        tab.y2.master = tab.y1;
      }
      named(tab.x, "Time");
      named(tab.y1, "Voltage");
      named(tab.y2, "Current");
      if (newTraceType & SPT_POWER) ensureThirdYAxis(tab);
      named(tab.y3, "Power");
      break;
    default:
      break;
  }
  if (t === "TRAN" || t === "DC") {
    if (tab.y3) {
      tab.view.marginRight = 160;
      if (tab.y2) tab.y2.nameAlign = "border_right";
      tab.y3.align = "border_right";
      tab.y3.nameAlign = "border_right";
    } else {
      tab.view.marginRight = 70;
      if (tab.y2) tab.y2.nameAlign = "right";
    }
  }
  updateAxisVisibility(tab);
}

/** The y scale a trace is on (`SIM_PLOT_TAB::SetTraceData`): phase and (but in an AC plot) currents on the second, power on the third. */
export function scaleOf(tab: PlotTab, trace: { type: number }): Scale | null {
  const t = simTypeOf(tab);
  if (trace.type & SPT_AC_PHASE || (t !== "AC" && trace.type & SPT_CURRENT)) return tab.y2;
  if (trace.type & SPT_POWER) return tab.y3;
  return tab.y1;
}

/** `SIM_PLOT_TAB::UpdateAxisVisibility`. */
export function updateAxisVisibility(tab: PlotTab): void {
  let y1 = false;
  let y2 = false;
  let y3 = false;
  const t = simTypeOf(tab);
  for (const tr of tab.traces.values()) {
    if (tr.type & SPT_POWER) y3 = true;
    else if (tr.type & SPT_AC_PHASE || (t !== "AC" && tr.type & SPT_CURRENT)) y2 = true;
    else y1 = true;
  }
  if (tab.y1) tab.y1.visible = y1;
  if (tab.y2) tab.y2.visible = y2;
  if (tab.y3) tab.y3.visible = y3;
}

/** The line a trace is drawn with (`SIM_PLOT_TAB::UpdateTraceStyle`): two pixels, dotted for a phase or a current when the tab says so. */
export function traceDotted(tab: PlotTab, trace: { type: number }): boolean {
  if (trace.type & SPT_AC_GAIN) return false;
  if (trace.type & (SPT_AC_PHASE | SPT_CURRENT)) return tab.dotted;
  return false;
}

/** `SIM_PLOT_TAB::GetOrAddTrace`: a new trace takes the colour its name had in this session, else the first colour no trace has. */
export function getOrAddTrace(tab: PlotTab, vector: string, type: number, dark: boolean): Trace {
  const id = traceId(vector, type);
  let tr = tab.traces.get(id);
  if (tr) return tr;
  updateAxes(tab, type);
  const t = simTypeOf(tab);
  if (t === "TRAN" || t === "DC") {
    if (![...tab.traces.values()].some((c) => c.type & SPT_VOLTAGE)) {
      if (tab.y2) tab.y2.master = null;
      if (tab.y3) tab.y3.master = null;
    }
  }
  const color = tab.sessionColors.get(vector) ?? generateColor([...tab.sessionColors.values()], dark);
  tr = { vector, type, name: vector, color, cursors: new Map(), x: [], y: [], sweepCount: 1, sweepSize: Number.POSITIVE_INFINITY };
  tab.sessionColors.set(tr.name, color);
  tab.traces.set(id, tr);
  updateAxisVisibility(tab);
  return tr;
}

/** A trace's colour changed (`TRACE::SetTraceColour` and `UpdateTraceStyle`, which records it for the session). */
export function setTraceColor(tab: PlotTab, tr: Trace, color: Rgb): void {
  tr.color = color;
  tab.sessionColors.set(tr.name, color);
}

/** `SIM_PLOT_TAB::DeleteTrace`. */
export function deleteTrace(tab: PlotTab, vector: string, type: number): boolean {
  const id = traceId(vector, type);
  if (!tab.traces.delete(id)) return false;
  resetScales(tab, false);
  updateAxisVisibility(tab);
  return true;
}

/** `SIM_PLOT_TAB::SetTraceData`: no 0 Hz point on a log axis, phase in degrees and gain in dB for AC and FFT, then the trace's scales extended. */
export function setTraceData(tab: PlotTab, tr: Trace, xs: number[], ys: number[], sweepCount: number, sweepSize: number): void {
  let x = xs.slice();
  let y = ys.slice();
  if (tab.x?.kind === "log" && x.length > 0 && x[0] === 0) {
    x = x.slice(1);
    y = y.slice(1);
  }
  const t = simTypeOf(tab);
  if (t === "AC" || t === "FFT") {
    if (tr.type & SPT_AC_PHASE) y = y.map((p) => (p * 180.0) / Math.PI);
    else y = y.map((p) => (p !== 0 ? (20 * Math.log(p)) / Math.log(10.0) : p));
  }
  if (x.length !== y.length) return;
  tr.x = x;
  tr.y = y;
  tr.sweepCount = sweepCount;
  tr.sweepSize = sweepSize;
  tr.multiRun = false;
  updateScalesOf(tab, tr);
  updateAxisVisibility(tab);
}

function minMax(v: number[]): [number, number] {
  if (v.length === 0) return [0, 0];
  let lo = v[0]!;
  let hi = v[0]!;
  for (const x of v) {
    if (x < lo) lo = x;
    if (x > hi) hi = x;
  }
  return [lo, hi];
}

/** `mpFXY::UpdateScales`. */
function updateScalesOf(tab: PlotTab, tr: Trace): void {
  if (tr.x.length === 0) return;
  const [xl, xh] = minMax(tr.x);
  const [yl, yh] = minMax(tr.y);
  if (tab.x) extendDataRange(tab.x, xl, xh);
  const ys = scaleOf(tab, tr);
  if (ys) extendDataRange(ys, yl, yh);
}

/** `SIM_PLOT_TAB::ResetScales`: the data ranges forgotten and taken again from the traces; a transient's x from its command's start and end. */
export function resetScales(tab: PlotTab, includeX: boolean): void {
  if (tab.x && includeX) {
    resetDataRange(tab.x);
    if (simTypeOf(tab) === "TRAN") {
      const t = tab.command.split(/[ \t\r\n]+/).filter((s) => s !== "");
      const end = t[2] !== undefined ? SpiceValue.parse(t[2])?.toDouble() ?? 1.0 : 1.0;
      const start = t[3] !== undefined ? SpiceValue.parse(t[3])?.toDouble() ?? 0.0 : 0.0;
      tab.x.startTime = start;
      tab.x.endTime = end;
      resetDataRange(tab.x);
    }
  }
  for (const s of [tab.y1, tab.y2, tab.y3]) if (s) resetDataRange(s);
  for (const tr of tab.traces.values()) updateScalesOf(tab, tr);
}

/** `GetLabelX` .. `GetLabelY3` and `GetUnitsX` .. `GetUnitsY3`. */
export const labelOf = (s: Scale | null) => s?.name ?? "";
export const unitsOf = (s: Scale | null) => s?.unit ?? "";

/** `CURSOR::doSetCoordX` for a single-sweep trace: the value at the cursor. */
export function cursorValue(tr: Trace, x: number, sweep = 0): number {
  // `CURSOR::doSetCoordX`: a multi-run trace is read on the cursor's run only (the first, or the last when the index is past the runs)
  if (tr.multiRun && tr.sweepCount > 1 && Number.isFinite(tr.sweepSize) && tr.sweepSize > 0 && tr.sweepCount * tr.sweepSize <= tr.x.length) {
    const i = sweep < 0 || sweep >= tr.sweepCount ? Math.max(tr.sweepCount - 1, 0) : sweep;
    const start = i * tr.sweepSize;
    return valueAt(tr.x, tr.y, x, start, Math.min(tr.x.length, start + tr.sweepSize));
  }
  return valueAt(tr.x, tr.y, x);
}

/** `SIM_PLOT_TAB::EnableCursor`: placed at 40 % (cursor 1) or 60 % (the others) of the plot area's width, in data units of the trace's x. */
export function cursorStart(tab: PlotTab, id: number): number {
  const v = tab.view;
  const width = v.scrX - v.marginLeft - v.marginRight;
  const center = v.marginLeft + Math.round(width * (id === 1 ? 0.4 : 0.6));
  const px = v.posX + center / v.scaleX;
  return tab.x ? fromPlotSafe(tab.x, px) : 0;
}

function fromPlotSafe(s: Scale, p: number): number {
  if (s.kind === "log") {
    const lo = Math.log10(s.minV);
    const hi = Math.log10(s.maxV);
    return 10 ** (p * (hi - lo) + lo);
  }
  const scale = 1.0 / (s.maxV - s.minV);
  return p / scale + s.minV;
}

/** `SIM_PLOT_TAB::GetY1Scale` and friends: a locked scale's min and max. */
export function lockedRange(s: Scale | null): [number, number] | null {
  return s && s.locked ? [s.axisMin, s.axisMax] : null;
}

/** `SIM_PLOT_TAB::ShowGrid` / `IsGridShown`: the grid is mathplot's "no ticks". */
export function setGrid(tab: PlotTab, shown: boolean): void {
  for (const s of [tab.x, tab.y1, tab.y2, tab.y3]) if (s) s.ticks = !shown;
}

export function gridShown(tab: PlotTab): boolean {
  if (!tab.x || !tab.y1) return false;
  return !tab.x.ticks;
}
