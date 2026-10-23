// The browser half of the SPICE simulator's server routes (crates/cli/src/sim_api.rs): the circuit as kicad-cli's netlists give it, a run of ngspice,
// Stop, and the workbook read from and written to KiCad's `.wbk` text.
import { postJson } from "./schControlClient";
import type { Circuit, RawPlot } from "../kicad-port/sim/simSignals";

export interface NgspiceSettings {
  workbook_filename: string;
  fix_include_paths: boolean;
  model_mode: number;
}

export interface CircuitReply extends Circuit {
  ok: boolean;
  message?: string;
  project?: string;
  settings?: { ngspice: NgspiceSettings | null; workbook: string | null };
}

/** `POST /api/sim/circuit`: the circuit; with a command, the netlist Show SPICE Netlist shows for it. */
export function fetchCircuit(command?: string, options?: number): Promise<CircuitReply> {
  return postJson<CircuitReply>("/api/sim/circuit", command === undefined ? {} : { command, options });
}

export interface RunRequest {
  command: string;
  options: number;
  tuners: { ref: string; value: number }[];
  user_signals: { id: number; expr: string }[];
  measurements: { row: number; text: string }[];
  fourier: { fundamental: string; signal: string }[];
  fft_tran?: { command: string; options: number };
}

export interface RunReply {
  ok: boolean;
  message?: string;
  stopped?: boolean;
  timed_out?: boolean;
  console?: string;
  netlist?: string;
  messages?: string[];
  plots?: RawPlot[];
  second_plots?: RawPlot[];
  revision?: string;
}

/** `POST /api/sim/run`: kicad-cli's netlist, then ngspice in batch mode. */
export function runSimulation(req: RunRequest): Promise<RunReply> {
  return postJson<RunReply>("/api/sim/run", req);
}

/** `POST /api/sim/stop`: the run killed. */
export function stopSimulation(): Promise<{ ok: boolean; stopped: boolean }> {
  return postJson("/api/sim/stop", {});
}

/** `POST /api/sim/workbook/parse`: a `.wbk` file's text as KiCad's JSON. */
export function parseWorkbook(text: string): Promise<{ ok: boolean; workbook?: Record<string, unknown>; message?: string }> {
  return postJson("/api/sim/workbook/parse", { text });
}

/** `POST /api/sim/workbook/text`: the bytes KiCad's `SaveWorkbook` writes for a workbook. */
export function workbookText(workbook: Record<string, unknown>): Promise<{ ok: boolean; text?: string; message?: string }> {
  return postJson("/api/sim/workbook/text", { workbook });
}
