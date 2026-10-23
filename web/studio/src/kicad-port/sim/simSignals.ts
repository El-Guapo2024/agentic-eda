// The simulator's signals and what a run gives them (eeschema/sim/simulator_frame_ui.cpp, commit 8303b2ad):
//
//   SIMULATOR_FRAME_UI::rebuildSignalsList / rebuildSignalsGrid / vectorNameFromSignalName / updateTrace / UpdateMeasurement / OnSimRefresh (the
//   operating point and noise results in the console) / SetUserDefinedSignals; sortSignals; SIGNALS_GRID_TRICKS::doPopupSelection (the measurement a
//   menu entry adds); SIMULATOR_CONTROL::ExportPlotAsCSV; SPICE_CIRCUIT_MODEL::VectorToSignal.
//
// The circuit is the server's (`POST /api/sim/circuit`: kicad-cli's netlists, crates/cli/src/sim_api.rs) and the vectors are ngspice's raw file
// (`POST /api/sim/run`). A raw file names a node's voltage `v(/out)` and a source's current `i(v1)` where libngspice's memory -- which KiCad reads --
// has `/out` and `v1#branch`; `findVector` takes either.
import {
  OPTION_SAVE_ALL_CURRENTS,
  OPTION_SAVE_ALL_DISSIPATIONS,
  OPTION_SAVE_ALL_VOLTAGES,
  SPT_AC_GAIN,
  SPT_AC_PHASE,
  SPT_CURRENT,
  SPT_POWER,
  SPT_SP_AMP,
  SPT_UNKNOWN,
  SPT_VOLTAGE,
  commandToSimType,
  parseDcCommand,
  parseNoiseCommand,
  shortName,
  type SimType,
} from "./simCommand";
import { naturalCompare } from "../libChooser";
import { SpiceValue, formatFromString, formatToString, formatWithUnits, formatG, type SpiceValueFormat } from "./spiceValue";

export interface CircuitItem {
  ref: string;
  name: string;
  device: string;
  nodes: string[];
  pin_count: number;
  pin_index: Record<string, number>;
  currents: string[];
  tuner: { param: string; value: string } | null;
}

export interface Circuit {
  nets: string[];
  items: CircuitItem[];
  /** `"REF.PIN"` -> SPICE net. */
  pin_nets: Record<string, string>;
  directives: string[];
  sch_sim_command: string;
  messages: string[];
  ngspice: boolean;
  netlist?: string;
}

export interface RawVector {
  name: string;
  kind: string;
  re: number[];
  im?: number[];
}

export interface RawPlot {
  plot_name: string;
  complex: boolean;
  points: number;
  vectors: RawVector[];
}

/** `sortSignals`: voltages first, then `StrNumCmp` ignoring case. */
export function sortSignals(signals: string[]): string[] {
  return [...signals].sort((a, b) => {
    const av = a.toUpperCase().startsWith("V");
    const bv = b.toUpperCase().startsWith("V");
    if (av && !bv) return -1;
    if (!av && bv) return 1;
    return naturalCompare(a, b);
  });
}

/** `EDA_COMBINED_MATCHER( filter.Upper(), CTX_SIGNAL ).Find( signal.Upper() )`: a regular expression, a wildcard pattern or a substring. */
export function signalMatches(filter: string, signal: string): boolean {
  const f = (filter === "" ? "*" : filter).toUpperCase();
  const s = signal.toUpperCase();
  try {
    if (new RegExp(f).test(s)) return true;
  } catch {
    // not a regular expression
  }
  const wild = f.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  try {
    if (new RegExp(wild).test(s)) return true;
  } catch {
    // nothing
  }
  return s.includes(f);
}

/** `SIMULATOR_FRAME_UI::rebuildSignalsList`: the signals of an analysis (`m_signals`) and the nets plotted as voltages (`m_netnames`). */
export function signalsList(simType: SimType, options: number, circuit: Circuit | null): { signals: string[]; netnames: string[] } {
  const signals: string[] = [];
  const netnames: string[] = [];
  const t = simType === "UNKNOWN" ? "TRAN" : simType;
  const add = (name: string) => {
    if (t === "AC") signals.push(`${name} (gain)`, `${name} (phase)`);
    else if (t === "SP") signals.push(`${name} (amplitude)`, `${name} (phase)`);
    else signals.push(name);
  };
  if (!circuit) return { signals, netnames };
  if (options & OPTION_SAVE_ALL_VOLTAGES && (t === "TRAN" || t === "DC" || t === "AC" || t === "FFT")) {
    for (const net of circuit.nets) {
      if (net === "GND" || net === "0" || net.startsWith("unconnected-_")) continue;
      netnames.push(net);
      add(`V(${net})`);
    }
  }
  if (options & OPTION_SAVE_ALL_CURRENTS && (t === "TRAN" || t === "DC" || t === "AC")) {
    for (const item of circuit.items) for (const c of item.currents) add(c);
  }
  if (options & OPTION_SAVE_ALL_DISSIPATIONS && (t === "TRAN" || t === "DC")) {
    for (const item of circuit.items) if (item.pin_count >= 2) add(`P(${item.name})`);
  }
  if (t === "NOISE") {
    add("inoise_spectrum");
    add("onoise_spectrum");
  }
  // .SAVE and .PROBE directives
  for (const d of circuit.directives) {
    for (const raw of d.split(/[\r\n]+/).filter((l) => l !== "")) {
      const line = raw.toUpperCase();
      const params = line.startsWith(".SAVE") ? line.slice(5) : line.startsWith(".PROBE") ? line.slice(6) : null;
      if (params === null) continue;
      for (const p of params.split(/[ \t]+/).filter((x) => x !== "")) add(p);
    }
  }
  return { signals, netnames };
}

/** `SIMULATOR_FRAME_UI::rebuildSignalsGrid`'s list: an FFT's own vectors, else the signals and the user-defined ones (with their gain/phase rows), sorted, then filtered. */
export function signalsGrid(simType: SimType, command: string, signals: string[], userSignals: Map<number, string>, filter: string): string[] {
  let list: string[] = [];
  if (simType === "FFT") {
    const tok = command.split(/[ \t\r\n]+/).filter((s) => s !== "");
    const i = tok.findIndex((x) => x.toLowerCase() === "fft");
    list = i >= 0 ? tok.slice(i + 1) : [];
  } else {
    list = [...signals];
    for (const s of userSignals.values()) {
      if (simType === "AC") list.push(`${s} (gain)`, `${s} (phase)`);
      else if (simType === "SP") list.push(`${s} (amplitude)`, `${s} (phase)`);
      else list.push(s);
    }
    list = sortSignals(list);
  }
  return list.filter((s) => signalMatches(filter, s));
}

/** `vectorNameFromSignalId`. */
export const userVector = (id: number) => `user${id}`;

/** `SIMULATOR_FRAME_UI::vectorNameFromSignalName`: the vector a signal plots and the trace type it is. */
export function vectorOfSignal(signal: string, simType: SimType | null, userSignals: Map<number, string>, noiseSource = ""): { vector: string; type: number } {
  const looksLikePower = (e: string) => {
    const u = e.toUpperCase();
    if (u.includes(":POWER")) return true;
    return u.includes("*") && u.includes("V(") && u.includes("I(");
  };
  let type = SPT_UNKNOWN;
  if (simType === "NOISE") type = noiseSource.toUpperCase().startsWith("I") ? SPT_CURRENT : SPT_VOLTAGE;
  else {
    const c = signal.toUpperCase().charAt(0);
    if (c === "V") type = SPT_VOLTAGE;
    else if (c === "I") type = SPT_CURRENT;
    else if (c === "P") type = SPT_POWER;
  }
  let name = signal;
  // std::map iterates its keys in order: " (amplitude)", " (gain)", " (phase)"
  for (const [suffix, bit] of [[" (amplitude)", SPT_SP_AMP], [" (gain)", SPT_AC_GAIN], [" (phase)", SPT_AC_PHASE]] as const) {
    if (name.endsWith(suffix)) {
      name = name.slice(0, -suffix.length);
      type |= bit;
      break;
    }
  }
  for (const [id, s] of userSignals) {
    if (name === s) {
      if (looksLikePower(s)) type = (type & (SPT_AC_GAIN | SPT_AC_PHASE | SPT_SP_AMP)) | SPT_POWER;
      return { vector: userVector(id), type };
    }
  }
  return { vector: name, type };
}

/** `SIMULATOR_FRAME_UI::getNoiseSource`. */
export function noiseSourceOf(command: string): string {
  return parseNoiseCommand(command)?.source ?? "";
}

/** A vector of a plot by the name KiCad asks libngspice for: as it is, or a raw file's `v(x)` for `x` and `V(x)`, `i(x)` for `I(x)` and `x#branch`. */
export function findVector(plot: RawPlot | undefined, name: string): RawVector | undefined {
  if (!plot) return undefined;
  const want = name.toLowerCase();
  const exact = plot.vectors.find((v) => v.name.toLowerCase() === want);
  if (exact) return exact;
  const alts: string[] = [];
  const m = /^([vi])\((.*)\)$/i.exec(name);
  if (m) {
    alts.push(m[2]!.toLowerCase());
    if (m[1]!.toLowerCase() === "i") alts.push(`${m[2]!.toLowerCase()}#branch`);
  } else {
    alts.push(`v(${want})`);
    if (want.endsWith("#branch")) alts.push(`i(${want.slice(0, -7)})`);
  }
  return plot.vectors.find((v) => alts.includes(v.name.toLowerCase()));
}

/** `NGSPICE::GetGainVector`. */
export function gainOf(v: RawVector): number[] {
  if (!v.im) return v.re.slice();
  const im = v.im;
  return v.re.map((r, i) => Math.hypot(r, im[i] ?? 0));
}

/** `NGSPICE::GetPhaseVector`. */
export function phaseOf(v: RawVector): number[] {
  if (!v.im) return v.re.map(() => 0);
  const im = v.im;
  return v.re.map((r, i) => Math.atan2(im[i] ?? 0, r));
}

/** `NGSPICE::GetXAxis`. */
export function xAxisVector(simType: SimType, plot: RawPlot | undefined): RawVector | undefined {
  switch (simType) {
    case "AC":
    case "SP":
    case "NOISE":
    case "FFT":
      return findVector(plot, "frequency");
    case "DC":
      return plot?.vectors.find((v) => /-sweep\)?$/i.test(v.name)) ?? findVector(plot, "sweep");
    case "TRAN":
      return findVector(plot, "time");
    default:
      return undefined;
  }
}

/** `SIMULATOR_FRAME_UI::updateTrace`'s data: x, y, and the sweeps of a DC sweep with a second source. Null when the plot has no such vector. */
export function traceData(simType: SimType, command: string, plot: RawPlot | undefined, vector: string, type: number): { x: number[]; y: number[]; sweepCount: number; sweepSize: number } | null {
  const xv = xAxisVector(simType, plot);
  if (!xv) return null;
  const x = gainOf(xv);
  let name = vector;
  if (type & SPT_POWER) name = `${vector.slice(vector.indexOf("(") + 1, vector.lastIndexOf(")"))}:power`;
  const v = findVector(plot, name);
  if (!v) return null;
  let y: number[];
  switch (simType) {
    case "AC":
      y = type & SPT_AC_GAIN ? gainOf(v) : phaseOf(v);
      break;
    case "SP":
      y = type & SPT_SP_AMP ? gainOf(v) : phaseOf(v);
      break;
    default:
      y = gainOf(v);
  }
  y = y.slice(0, x.length);
  let sweepCount = 1;
  let sweepSize = Number.POSITIVE_INFINITY;
  const dc = simType === "DC" ? parseDcCommand(command) : null;
  if (dc?.src2 && dc.src2.source !== "") {
    const n = dc.src2.end.sub(dc.src2.start).div(dc.src2.incr);
    sweepCount = Math.round(n.toDouble()) + 1;
    sweepSize = Math.floor(x.length / sweepCount);
  }
  if (y.length < x.length) return null;
  return { x, y, sweepCount, sweepSize };
}

/** `SPICE_CIRCUIT_MODEL::VectorToSignal` of a vector's name (libngspice's or a raw file's): the signal and what it is. */
export function vectorToSignal(vector: string): { signal: string; type: number } {
  const internal = /^@(\w*[.\w+]*)\[(\w*)\]$/.exec(vector);
  if (internal) {
    const param = internal[2] ?? "";
    if (param.toLowerCase().startsWith("i")) return { signal: `I${param.slice(1)}(${(internal[1] ?? "").toUpperCase()})`, type: SPT_CURRENT };
    return { signal: "", type: SPT_UNKNOWN };
  }
  const raw = /^([vi])\((.*)\)$/i.exec(vector);
  if (raw) return raw[1]!.toLowerCase() === "v" ? { signal: `V(${raw[2]})`, type: SPT_VOLTAGE } : { signal: `I(${raw[2]})`, type: SPT_CURRENT };
  if (vector.endsWith("#branch")) return { signal: `I(${vector.slice(0, -7)})`, type: SPT_CURRENT };
  if (vector.endsWith(":power")) return { signal: `P(${vector.slice(0, -6)})`, type: SPT_POWER };
  return { signal: `V(${vector})`, type: SPT_VOLTAGE };
}

/** `wxString::Pad`: `count` copies of `c` appended. */
const pad = (s: string, n: number) => s + " ".repeat(Math.max(0, n));

/** `OnSimRefresh` of an operating point: "Simulation results:" and a line per vector, `signal:` padded to 25 characters, then the value and its
 *  unit; and the values the schematic shows (`SCHEMATIC::SetOperatingPoint`: the net or the device, `:power` for a power). */
export function opResults(plot: RawPlot | undefined): { console: string; points: Map<string, number> } {
  let console = "\n\nSimulation results:\n\n";
  const points = new Map<string, number>();
  for (const v of plot?.vectors ?? []) {
    const val = v.re[0];
    if (val === undefined) continue;
    let value = SpiceValue.of(val).toSpiceString();
    const { signal, type } = vectorToSignal(v.name);
    const tab = 25;
    const padding = signal.length < tab ? tab - signal.length : 1;
    value += type === SPT_VOLTAGE ? "V" : type === SPT_CURRENT ? "A" : type === SPT_POWER ? "W" : "?";
    console += `${pad(`${signal}:`, padding)}${value}\n`;
    let key = signal;
    if (type === SPT_VOLTAGE || type === SPT_CURRENT || type === SPT_POWER) key = signal.slice(2, signal.length - 1);
    if (type === SPT_POWER) key += ":power";
    points.set(key, val);
  }
  return { console, points };
}

/** `OnSimRefresh` of a noise analysis: the integrated noise's vectors, "%s: %sV". */
export function noiseResults(plot: RawPlot | undefined): string {
  let s = "\n\nSimulation results:\n\n";
  for (const v of plot?.vectors ?? []) {
    const val = v.re[0];
    if (val === undefined) continue;
    s += `${v.name}: ${SpiceValue.of(val).toSpiceString()}V\n`;
  }
  return s;
}

/** The measurement a signals-grid menu entry adds (`SIGNALS_GRID_TRICKS::doPopupSelection`): none for a phase; a gain's or amplitude's suffix
 *  dropped, and a voltage then measured in dB (`vdb(...)`). */
export function measurementFor(func: string, signal: string): string | null {
  if (signal.endsWith(" (phase)")) return null;
  let s = signal;
  if (s.endsWith(" (gain)") || s.endsWith(" (amplitude)")) {
    s = s.slice(0, s.length - 7);
    if (s.toUpperCase().startsWith("V(")) s = `vdb${s.slice(1)}`;
  }
  return `${func} ${s}`;
}

/** `SIMULATOR_FRAME_UI::UpdateMeasurement`: the units of a measurement's result, the text ngspice's `meas` is given (a power is `func dev:power`),
 *  and the format with its units. Null text for an empty measurement. */
export function measurementCommand(text: string, simType: SimType, format: string): { text: string | null; format: string; units: string | null } {
  if (text === "") return { text: null, format, units: null };
  const m = /^ *([a-zA-Z_]+) +([a-zA-Z]*)\(([^)]+)\)/.exec(text);
  if (!m) return { text, format, units: null };
  const func = (m[1] ?? "").toUpperCase();
  const signalType = (m[2] ?? "").toUpperCase();
  const device = m[3] ?? "";
  let units: string;
  let cmd = text;
  if (signalType.endsWith("DB")) units = "dB";
  else if (signalType.startsWith("I")) units = "A";
  else if (signalType.startsWith("P")) {
    units = "W";
    cmd = `${func} ${device}:power`;
  } else units = "V";
  if (func.endsWith("_AT")) units = simType === "AC" || simType === "SP" ? "Hz" : "s";
  else if (func.startsWith("INTEG")) {
    switch (simType) {
      case "TRAN":
        units = signalType.startsWith("P") ? "J" : `${units}.s`;
        break;
      case "AC":
      case "SP":
      case "DISTO":
      case "NOISE":
      case "FFT":
      case "SENS":
        units += "·Hz";
        break;
      default:
        units += "·?";
    }
  }
  const f = formatWithUnits(formatFromString(format), units);
  return { text: cmd, format: formatToString(f), units };
}

/** A measurement's result as the grid shows it: `SPICE_VALUE( result ).ToString( format )`, "?" without one. */
export function measurementValue(result: number | undefined, format: string): string {
  if (result === undefined || Number.isNaN(result)) return "?";
  return SpiceValue.of(result).toFormatted(formatFromString(format));
}

/** The `meas` analysis name of a tab (`TypeToName( type, true )`). */
export const measType = (command: string) => shortName(commandToSimType(command));

/** `SIMULATOR_CONTROL::ExportPlotAsCSV`: the x axis's name and every trace's name, then a row per point, each value `%g` followed by ';', rows
 *  ending "\r\n". The traces are in the tab's `std::map` order (by trace id). */
export function plotCsv(xAxisName: string, traces: { id: string; display: string; x: number[]; y: number[] }[]): string | null {
  if (traces.length === 0) return null;
  const sorted = [...traces].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const first = sorted[0]!;
  let out = `${xAxisName};`;
  for (const t of sorted) out += `${t.display};`;
  out += "\r\n";
  for (let r = 0; r < first.x.length; r++) {
    out += `${formatG(first.x[r]!, 6)};`;
    for (const t of sorted) out += `${formatG(t.y[r] ?? Number.NaN, 6)};`;
    out += "\r\n";
  }
  return out;
}

export type { SpiceValueFormat };
