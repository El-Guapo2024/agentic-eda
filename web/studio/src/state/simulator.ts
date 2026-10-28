// The SPICE simulator window (package P21): `SIMULATOR_FRAME` and `SIMULATOR_CONTROL` (eeschema/sim/simulator_frame.cpp, eeschema/tools/simulator_control.cpp,
// commit 8303b2ad) over the frame model of kicad-port/sim/simFrame.ts. Plain module state with subscribers, like state/designBlocks.ts, so the actions
// (actions/simActions.ts), the window (components/simulator/) and the schematic canvas (the probe, the operating point) see the same simulator.
//
// What differs from KiCad, by the architecture the user chose (docs/HANDOFF.md, "Next tasks", item 3): ngspice runs as a program in batch mode, not as
// libngspice in the background (`NGSPICE::Run`'s `bg_run`). A run is one request (`POST /api/sim/run`): kicad-cli's netlist, ngspice, the plots back.
// So the plot is drawn when the run is over, not live while it runs (`OnSimRefresh` on its 50 ms timer), and Stop Simulation kills ngspice -- what it
// had computed is lost with it (KiCad's `bg_halt` keeps it). What KiCad asks of a loaded simulator after a run -- a measurement added, a Fourier
// analysis, a user-defined signal changed -- is a new run here, with them in it.
import { useSyncExternalStore } from "react";
import { fetchCircuit, parseWorkbook, runSimulation, stopSimulation, workbookText, type CircuitReply, type NgspiceSettings, type RunRequest } from "../api/simClient";
import {
  OPTION_DEFAULT_FLAGS,
  SPT_CURRENT,
  SPT_VOLTAGE,
  commandToSimType,
  isPlottable,
  parseCommand,
  selectDcSources,
  type CommandFields,
} from "../kicad-port/sim/simCommand";
import { SpiceValue } from "../kicad-port/sim/spiceValue";
import { saveTextAs } from "../api/client";
import { fetchSimModelSymbol } from "../api/simModelClient";
import { tunerSaveEdit } from "../kicad-port/sim/simModel";
import {
  addTrace,
  applyRun,
  frameTitle,
  currentTab,
  fromWorkbook,
  measurementRequests,
  newFrame,
  newSimTab,
  rebuildSignals,
  simTabOf,
  simTypeOf,
  toWorkbook,
  tunedValue,
  multiRunSteps,
  newTuner,
  type SimFrame,
  type SimTab,
  type Tuner,
} from "../kicad-port/sim/simFrame";
import { noiseResults, opResults } from "../kicad-port/sim/simSignals";
import { gridShown } from "../kicad-port/sim/simPlotTab";
import type { SpiceValueFormat } from "../kicad-port/sim/spiceValue";

export type SimDialog =
  | { kind: "command"; mode: "new" | "edit"; fields: CommandFields; options: number; command: string }
  | { kind: "userSignals" }
  | { kind: "netlist"; text: string; messages: string[] }
  | { kind: "format"; title: string; format: SpiceValueFormat; apply: (f: SpiceValueFormat) => void }
  | { kind: "fourier"; signals: string[]; title: string }
  | { kind: "saveAs"; name: string; download: boolean }
  | { kind: "confirm"; title: string; message: string; buttons: string[]; resolve: (button: string | null) => void }
  | { kind: "message"; title: string; message: string; detail?: string; error: boolean };

export interface SimUi {
  open: boolean;
  frame: SimFrame;
  /** The project's simulator settings (`NGSPICE_SETTINGS`) as the design holds them. */
  settings: NgspiceSettings;
  /** The workbook's text the design holds. */
  savedWorkbook: string | null;
  project: string;
  dialog: SimDialog | null;
  /** `WX_INFOBAR::ShowMessageFor` above the window. */
  infobar: string | null;
  /** The window has the keyboard (KiCad's frame is active): its hotkeys act. */
  focused: boolean;
  /** The operating point the schematic shows (`SCHEMATIC::SetOperatingPoint`): by net or device. */
  opPoints: Map<string, number>;
  /** View > Show OP Voltages / Show OP Currents (`appearance.show_op_voltages` / `show_op_currents`, both on by default). */
  showOpVoltages: boolean;
  showOpCurrents: boolean;
  /** Bumped on every change, so a canvas redraws. */
  version: number;
}

export interface SimHost {
  /** One undoable command (`/api/cmd`). */
  cmd: (c: import("../api/types").Cmd) => Promise<boolean>;
  toast: (message: string, kind?: "info" | "error") => void;
  /** Unannotated references on the schematic (`SCH_EDIT_FRAME::ReadyToNetlist`). */
  unannotated: () => boolean;
}

const DEFAULT_SETTINGS: NgspiceSettings = { workbook_filename: "", fix_include_paths: true, model_mode: 4 };
const STORAGE_KEY = "eda-studio.simulator";

/** The viewer's settings KiCad keeps in its own (`simulator.white_background`, `appearance.show_op_voltages`, `show_op_currents`): this browser's. */
interface SavedSettings {
  white_background?: boolean;
  show_op_voltages?: boolean;
  show_op_currents?: boolean;
}

function savedSettings(): SavedSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as SavedSettings) : {};
  } catch {
    return {};
  }
}

function saveSetting(patch: SavedSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...savedSettings(), ...patch }));
  } catch {
    // no storage
  }
}

const savedDark = () => savedSettings().white_background !== true;

let current: SimUi = {
  open: false,
  frame: newFrame(savedDark()),
  settings: DEFAULT_SETTINGS,
  savedWorkbook: null,
  project: "",
  dialog: null,
  infobar: null,
  focused: false,
  opPoints: new Map(),
  showOpVoltages: savedSettings().show_op_voltages !== false,
  showOpCurrents: savedSettings().show_op_currents !== false,
  version: 0,
};
const listeners = new Set<() => void>();
let host: SimHost | null = null;

/** The run in flight, so Stop can tell its reply from a newer run's. */
let runSerial = 0;

function emit(): void {
  current = { ...current, version: current.version + 1 };
  for (const l of listeners) l();
}

export function getSimUi(): SimUi {
  return current;
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useSimUi(): SimUi {
  return useSyncExternalStore(subscribe, getSimUi, getSimUi);
}

/** The host the actions run in (the store's command and toast). */
export function setSimHost(h: SimHost): void {
  host = h;
}

/** Any change to the frame made by a component: redraw and mark. */
export function touch(modified = false): void {
  if (modified) current.frame.modified = true;
  emit();
}

export function setSimFocused(focused: boolean): void {
  if (current.focused !== focused) {
    current.focused = focused;
    emit();
  }
}

export function clearInfobar(): void {
  current.infobar = null;
  emit();
}

/** `SIMULATOR_FRAME::UpdateTitle`: "*name [Unsaved] — SPICE Simulator". */
export function frameTitleOf(s: SimUi): string {
  return frameTitle(s.frame, s.settings.workbook_filename, s.savedWorkbook !== null && s.settings.workbook_filename !== "");
}

export function setSimDialog(d: SimDialog | null): void {
  current.dialog = d;
  emit();
}

function message(title: string, text: string, error = true, detail?: string): void {
  setSimDialog({ kind: "message", title, message: text, error, detail });
}

/** `DisplayErrorMessage` / `IsOK` / `HandleUnsavedChanges`: a question in the window. */
function ask(title: string, text: string, buttons: string[]): Promise<string | null> {
  return new Promise((resolve) => setSimDialog({ kind: "confirm", title, message: text, buttons, resolve: (b) => {
    setSimDialog(null);
    resolve(b);
  } }));
}

// Don't let the dialog grow too tall: `SIMULATOR_FRAME::showNetlistErrors`' MAX_MESSAGES
const MAX_MESSAGES = 20;

/** `SIMULATOR_FRAME::showNetlistErrors`: kicad-cli's and the netlist's messages, "Errors during netlist generation." */
function showNetlistErrors(messages: string[] | undefined): void {
  if (!messages || messages.length === 0) return;
  let lines = messages.flatMap((m) => m.split("\n"));
  if (lines.length > MAX_MESSAGES) lines = [...lines.slice(0, MAX_MESSAGES), "..."];
  const tuned = lines.some((l) => l.startsWith("Could not apply tuned value(s):"));
  message(tuned ? "Simulator" : "Errors during netlist generation.", tuned ? "Could not apply tuned value(s):" : "Errors during netlist generation.", true, lines.join("\n"));
}

/** `SIMULATOR_FRAME::LoadSimulator`: the circuit read again ("Simulator requires a fully annotated schematic." first). */
async function loadCircuit(command?: string, options?: number): Promise<CircuitReply | null> {
  if (host?.unannotated()) {
    message("Simulator", "Simulator requires a fully annotated schematic.");
    return null;
  }
  let reply: CircuitReply;
  try {
    reply = await fetchCircuit(command, options);
  } catch (e) {
    message("Simulator", String(e instanceof Error ? e.message : e));
    return null;
  }
  if (!reply.ok) {
    message("Errors during netlist generation.", reply.message ?? "kicad-cli could not export the netlist");
    return null;
  }
  current.frame.circuit = reply;
  current.project = reply.project ?? current.project;
  current.settings = reply.settings?.ngspice ?? DEFAULT_SETTINGS;
  current.savedWorkbook = reply.settings?.workbook ?? null;
  rebuildSignals(current.frame);
  return reply;
}

/** `SIMULATOR_FRAME::SIMULATOR_FRAME` and `SIMULATOR_FRAME_UI::InitWorkbook`: the window opened on the workbook the project names, else on the
 *  schematic's own analysis. A window already open is brought up. */
export async function openSimulator(): Promise<void> {
  if (current.open) {
    current.focused = true;
    emit();
    return;
  }
  current.open = true;
  current.focused = true;
  current.frame = newFrame(current.frame.dark);
  current.infobar = null;
  emit();
  const reply = await loadCircuit();
  if (!reply) {
    emit();
    return;
  }
  const name = current.settings.workbook_filename;
  let fromSchematic = true;
  if (name !== "") {
    if (current.savedWorkbook === null) {
      current.infobar = `Workbook file '${name}' not found. Loading simulation settings from schematic.`;
      current.settings = { ...current.settings, workbook_filename: "" };
    } else {
      fromSchematic = !(await loadWorkbookText(current.savedWorkbook, name, false));
    }
  }
  if (fromSchematic && reply.sch_sim_command !== "") {
    newSimTab(current.frame, reply.sch_sim_command, OPTION_DEFAULT_FLAGS);
  }
  current.frame.modified = false;
  emit();
}

/** `SIMULATOR_FRAME::canCloseWindow` and `doCloseWindow`: "Save changes to workbook?", the run stopped, the operating point taken off the schematic. */
export async function closeSimulator(): Promise<boolean> {
  if (current.frame.modified) {
    const b = await ask("Save Changes?", "Save changes to workbook?", ["Save", "Discard Changes", "Cancel"]);
    if (b === "Cancel" || b === null) return false;
    if (b === "Save" && !(await saveWorkbook(false))) return false;
  }
  if (current.frame.running) await stop();
  current.open = false;
  current.focused = false;
  current.opPoints = new Map();
  current.dialog = null;
  emit();
  return true;
}

/** `SIMULATOR_CONTROL::NewAnalysisTab`: the analysis dialog on "*", a new tab with what it says. */
export async function newAnalysisTab(): Promise<void> {
  runAfterNewTab = false;
  await loadCircuit();
  const fields = parseCommand("*", current.frame.circuit?.sch_sim_command ?? "");
  setSimDialog({ kind: "command", mode: "new", fields, options: OPTION_DEFAULT_FLAGS, command: "*" });
}

/** `SIMULATOR_FRAME::EditAnalysis` (Edit Analysis Tab...): the dialog on the current tab's command, options and plot setup. */
export async function editAnalysis(): Promise<void> {
  const tab = currentTab(current.frame);
  if (!tab) return;
  await loadCircuit();
  // `parseCommand`'s DC sources: selected only where `updateDCSources` lists them
  const fields = selectDcSources(parseCommand(tab.command, current.frame.circuit?.sch_sim_command ?? ""), current.frame.circuit?.items ?? []);
  setSimDialog({ kind: "command", mode: "edit", fields, options: tab.options, command: tab.command });
}

/** The dialog's OK: `NewSimTab` / `SetSimCommand`, then `DIALOG_SIM_COMMAND::ApplySettings` (done by the dialog on the plot) and the settings it changed. */
export async function applyAnalysis(mode: "new" | "edit", command: string, options: number, settings: NgspiceSettings, applyPlot: (tab: SimTab) => void): Promise<void> {
  let tab: SimTab;
  if (mode === "new") tab = newSimTab(current.frame, command, options);
  else {
    const t = currentTab(current.frame);
    if (!t) return;
    tab = t;
    if (tab.command !== command) {
      tab.command = command;
      tab.plotName = "";
    }
    tab.options = options;
  }
  applyPlot(tab);
  rebuildSignals(current.frame);
  current.frame.modified = true;
  setSimDialog(null);
  await saveSettings(settings);
  // `RunSimulation` with no tab: `NewAnalysisTab`, then the run when the dialog made one
  if (mode === "new" && runAfterNewTab) {
    runAfterNewTab = false;
    await startSimulation();
  }
}

/** Run Simulation was asked with no tab: the tab the analysis dialog makes is run (`SIMULATOR_CONTROL::RunSimulation`). */
let runAfterNewTab = false;

/** `SIMULATOR_FRAME::SaveSettings`: the project's simulator settings written when they changed (`NGSPICE_SETTINGS::SaveToFile`, the schematic marked modified):
 *  one command, with the workbook the design holds. */
async function saveSettings(settings: NgspiceSettings, workbook: string | null = current.savedWorkbook): Promise<boolean> {
  const same = (a: NgspiceSettings, b: NgspiceSettings) => a.workbook_filename === b.workbook_filename && a.fix_include_paths === b.fix_include_paths && a.model_mode === b.model_mode;
  if (same(settings, current.settings) && workbook === current.savedWorkbook) return true;
  const isDefault = same(settings, DEFAULT_SETTINGS);
  const ok = (await host?.cmd({ op: "set_simulator", ngspice: isDefault ? null : settings, workbook })) ?? false;
  if (ok) {
    current.settings = settings;
    current.savedWorkbook = workbook;
    emit();
  }
  return ok;
}

/** `SIMULATOR_CONTROL::RunSimulation` / `SIMULATOR_FRAME::StartSimulation`: Stop while a run goes; a new tab first when there is none; an FFT on the TRAN
 *  tab's results; the first tab offered the schematic's changed analysis. */
export async function runOrStop(): Promise<void> {
  if (current.frame.running) {
    await stop();
    return;
  }
  if (!currentTab(current.frame)) {
    await newAnalysisTab();
    runAfterNewTab = true;
    return;
  }
  await startSimulation();
}

/** The run in progress, so that a tuner's change can wait for the run it stopped. */
let runInProgress: Promise<void> | null = null;
/** Whether the last run was stopped (a multi-run goes no further). */
let lastRunStopped = false;

/** `SIMULATOR_FRAME::StartSimulation`. `fromTuner`: `onUpdateSim`'s run, the loaded circuit run again with the tuned values -- no FFT or schematic
 *  checks (here the batch run reads the schematic again, without asking). */
export async function startSimulation(overrides?: Map<string, number>, fromTuner = false): Promise<void> {
  const run = startSimulationInner(overrides, fromTuner);
  runInProgress = run;
  try {
    await run;
  } finally {
    if (runInProgress === run) runInProgress = null;
  }
}

async function startSimulationInner(overrides: Map<string, number> | undefined, fromTuner: boolean): Promise<void> {
  const f = current.frame;
  const tab = currentTab(f);
  if (!tab) return;
  const type = simTypeOf(tab);
  let fftTran: RunRequest["fft_tran"];
  if (type === "FFT") {
    const tran = simTabOf(f, "TRAN");
    if (!tran || tran.plotName === "") {
      message("Simulator", "You must run a TRAN simulation first; its results will be used for the fast Fourier transform.");
      return;
    }
    fftTran = { command: tran.command, options: tran.options };
  } else if (f.tabs.indexOf(tab) === 0 && !fromTuner) {
    // busy from here: the schematic's analysis is read first (kicad-cli)
    f.running = true;
    emit();
    const sch = (await loadCircuit())?.sch_sim_command ?? null;
    f.running = false;
    if (sch === null) {
      emit();
      return;
    }
    if (sch !== tab.lastSchText) {
      const yes = tab.lastSchText === "" || (await ask("Confirmation", "Schematic sheet simulation command directive has changed.  Do you wish to update the Simulation Command?", ["Yes", "No"])) === "Yes";
      if (yes) {
        tab.command = sch;
        tab.lastSchText = tab.command;
        f.modified = true;
      }
    }
  }
  if (host?.unannotated()) {
    message("Simulator", "Simulator requires a fully annotated schematic.");
    emit();
    return;
  }
  // `OnSimUpdate`: the scales reset, the console cleared, the tuners applied.
  f.console = "";
  f.running = true;
  f.finished = false;
  current.opPoints = new Map();
  emit();
  const serial = ++runSerial;
  lastRunStopped = false;
  const req: RunRequest = {
    command: tab.command,
    options: tab.options,
    tuners: f.tuners.map((t) => ({ ref: t.ref, value: overrides?.get(t.ref) ?? tunedValue(t) })),
    user_signals: [...f.userSignals.entries()].map(([id, expr]) => ({ id, expr })),
    measurements: measurementRequests(tab),
    fourier: pendingFourier.splice(0),
    fft_tran: fftTran,
  };
  let reply;
  try {
    reply = await runSimulation(req);
  } catch (e) {
    reply = { ok: false, message: String(e instanceof Error ? e.message : e) };
  }
  if (serial !== runSerial) return;
  f.running = false;
  showNetlistErrors(reply.messages);
  if (reply.stopped) {
    // `NGSPICE::Stop` ends the run and `onSimFinished` follows it (`m_simFinished = true`: Probe and Tune stay enabled); the killed batch run
    // leaves no vectors
    f.console = reply.console ?? "";
    f.finished = true;
    lastRunStopped = true;
    emit();
    return;
  }
  if (!reply.plots) {
    f.console = reply.console ?? "";
    if (reply.message) message("Simulator", reply.message, true);
    emit();
    return;
  }
  applyRun(f, tab, reply.plots, reply.second_plots ?? [], reply.console ?? "");
  if (type === "OP") {
    const op = opResults(reply.plots[0]);
    f.console += op.console;
    current.opPoints = op.points;
  } else if (type === "NOISE") {
    f.console += noiseResults(reply.second_plots?.[0]);
  }
  emit();
}

/** `SIMULATOR_CONTROL::RunSimulation` while running: `NGSPICE::Stop`, here the ngspice process killed. */
export async function stop(): Promise<void> {
  try {
    await stopSimulation();
  } catch {
    // the run's own reply says how it ended
  }
}

/** The Fourier analyses asked for by "Perform Fourier Analysis..." (`DoFourier`), sent with the next run. */
const pendingFourier: { fundamental: string; signal: string }[] = [];

/** `SIMULATOR_FRAME_UI::DoFourier`: `fourier <fundamental> <signal>` into the console; a run that has it. */
export async function doFourier(signals: string[], fundamental: string): Promise<void> {
  const f = SpiceValue.parse(fundamental).toSpiceString();
  for (const s of signals) pendingFourier.push({ fundamental: f, signal: s });
  setSimDialog(null);
  await startSimulation();
}

/** What changed after a run that KiCad asks the loaded simulator for (a measurement, a user-defined signal, a tuner): the run made again with it. */
export async function rerunIfFinished(): Promise<void> {
  if (current.frame.finished && !current.frame.running) await startSimulation();
  else emit();
}

/** `SIMULATOR_FRAME::AddVoltageTrace` / `AddCurrentTrace` (the probe). */
export function probeTrace(name: string, current_: boolean): void {
  addTrace(current.frame, name, current_ ? SPT_CURRENT : SPT_VOLTAGE);
  emit();
}

// ----------------------------------------------------------------------------------------------------------- the workbook

/** `SIMULATOR_CONTROL::getDefaultFilename`: the workbook's name, else the project's with `.wbk`, else "noname.wbk". */
export function defaultWorkbookName(): string {
  const name = current.settings.workbook_filename.replace(/^.*\//, "");
  if (name !== "") return name;
  return `${current.project !== "" ? current.project : "noname"}.wbk`;
}

/** `SIMULATOR_FRAME_UI::LoadWorkbook` from a file's text: the frame made from it, the project naming it. Loading is no modification. */
export async function loadWorkbookText(text: string, name: string, store = true): Promise<boolean> {
  const r = await parseWorkbook(text).catch(() => null);
  if (!r?.ok || !r.workbook) {
    message("Simulator", `Unable to load or parse file ${name}`, true, r?.message);
    return false;
  }
  const dark = current.frame.dark;
  const circuit = current.frame.circuit;
  const frame = newFrame(dark);
  frame.circuit = circuit;
  fromWorkbook(frame, r.workbook);
  current.frame = frame;
  if (store) await saveSettings({ ...current.settings, workbook_filename: name }, text);
  current.frame.modified = false;
  emit();
  return true;
}

/** `SIMULATOR_CONTROL::OpenWorkbook`: a `.wbk` the user picks. */
export function openWorkbookFile(file: File): Promise<boolean> {
  return file.text().then((t) => loadWorkbookText(t, file.name));
}

/** `SIMULATOR_FRAME_UI::SaveWorkbook`: KiCad's bytes for the workbook, kept in the design as the project's workbook (one undo step) under `name`. */
export async function saveWorkbookAs(name: string, download: boolean): Promise<boolean> {
  const file = name.toLowerCase().endsWith(".wbk") ? name : `${name.replace(/\.[^.]*$/, "")}.wbk`;
  const r = await workbookText(toWorkbook(current.frame)).catch(() => null);
  if (!r?.ok || r.text === undefined) {
    message("Simulator", r?.message ?? "the workbook could not be written");
    return false;
  }
  const ok = await saveSettings({ ...current.settings, workbook_filename: file }, r.text);
  if (!ok) return false;
  if (download) {
    saveTextAs(r.text, file);
  }
  current.frame.modified = false;
  setSimDialog(null);
  emit();
  return true;
}

/** `SIMULATOR_CONTROL::SaveWorkbook`: Save Workbook under the workbook's name; with none, and Save Workbook As, "Save Simulation Workbook As". */
export async function saveWorkbook(asNew: boolean): Promise<boolean> {
  const name = asNew ? "" : current.settings.workbook_filename;
  if (name === "") {
    setSimDialog({ kind: "saveAs", name: defaultWorkbookName(), download: asNew });
    return false;
  }
  return saveWorkbookAs(name, false);
}

// ----------------------------------------------------------------------------------------------------------- tuners

/** `SIMULATOR_FRAME_UI::AddTuner`: "The current analysis must have a plot in order to tune ..."; no second tuner for a symbol. */
export async function addTuner(ref: string): Promise<void> {
  const tab = currentTab(current.frame);
  if (!tab?.plot) {
    message("Simulator", "The current analysis must have a plot in order to tune the value of a passive R, L, C model or voltage or current source.");
    return;
  }
  if (current.frame.tuners.some((t) => t.ref === ref)) return;
  const t = newTuner(current.frame.circuit, ref);
  if ("error" in t) {
    if (!t.error.endsWith("not found")) message("Simulator", t.error);
    return;
  }
  current.frame.tuners.push(t);
  current.frame.modified = true;
  emit();
}

/** `SIMULATOR_FRAME_UI::RemoveTuner`. */
export function removeTuner(t: Tuner): void {
  current.frame.tuners = current.frame.tuners.filter((x) => x !== t);
  current.frame.modified = true;
  emit();
}

/** `TUNER_SLIDER::updateComponentValue` (`EVT_SIM_UPDATE`): the simulation run again with the tuned values -- every step of the multi-run tuners, one run each
 *  (`prepareMultiRunState`), the traces drawn as sweeps of the runs. */
export async function tunerChanged(): Promise<void> {
  const f = current.frame;
  // `onUpdateSim`: a run going is stopped, and the simulation run again
  if (f.running) {
    await stop();
    if (runInProgress) await runInProgress;
  }
  const multi = f.tuners.filter((t) => t.multi);
  if (multi.length === 0) {
    await startSimulation(undefined, true);
    return;
  }
  const steps = multiRunSteps(multi);
  if (steps.length < 2) {
    await startSimulation(undefined, true);
    return;
  }
  const tab = currentTab(f);
  if (!tab?.plot) return;
  // One run per step, the y of every trace recorded per step (`recordMultiRunData`), then each trace drawn with a sweep per run.
  const record = new Map<string, { x: number[]; ys: number[][] }>();
  for (const step of steps) {
    await startSimulation(step, true);
    if (!f.finished || lastRunStopped) return;
    for (const [key, tr] of tab.plot.traces) {
      const r = record.get(key) ?? { x: tr.x.slice(), ys: [] };
      if (r.x.length === tr.x.length) r.ys.push(tr.y.slice());
      record.set(key, r);
    }
  }
  for (const [key, r] of record) {
    const tr = tab.plot.traces.get(key);
    if (!tr || r.ys.length === 0) continue;
    const x = r.ys.flatMap(() => r.x);
    const y = r.ys.flat();
    // already converted (dB, degrees) by the single runs: set the data as it is
    tr.x = x;
    tr.y = y;
    tr.sweepCount = r.ys.length;
    tr.sweepSize = r.x.length;
    tr.multiRun = true;
  }
  emit();
}

/** `SIMULATOR_FRAME_UI::UpdateTunerValue` (the tuner's Save): the value written to the symbol -- its `Sim.Params` parameter when the symbol has one, else its
 *  Value field -- one undo step. The fields are read from the server (`POST /api/sim/model/symbol`: the instance's over the library symbol's), so a sheet
 *  read from a KiCad file, whose symbols the studio lists without fields, is saved too (r7 R7-SM-2); `symbolFields` is the caller's copy, used when the
 *  server cannot be asked. */
export async function saveTunerValue(t: Tuner, symbolFields: Record<string, string> | null): Promise<void> {
  let fields = symbolFields ?? {};
  try {
    const r = await fetchSimModelSymbol(t.ref);
    if (r.ok) fields = r.fields;
  } catch {
    /* the caller's copy */
  }
  await host?.cmd({ op: "set_symbol_fields", edits: [tunerSaveEdit(t.ref, t.param, t.text, fields)] });
}

/** The simulator's checks for the menus and the toolbar (`SIMULATOR_FRAME::setupUIConditions`). */
export function simEnabled(name: string, s: SimUi = current): boolean | undefined {
  const tab = currentTab(s.frame);
  const havePlot = !!tab?.plot;
  switch (name) {
    case "eeschema.EditorControl.showSimulator":
      return true;
    case "eeschema.Simulation.probe":
    case "eeschema.Simulation.tune":
      return s.open && s.frame.finished;
    case "eeschema.Simulation.openWorkbook":
    case "eeschema.Simulation.saveWorkbook":
    case "eeschema.Simulation.saveWorkbookAs":
    case "eeschema.Simulation.newAnalysisTab":
    case "eeschema.Simulation.showNetlist":
    case "eeschema.Simulation.editUserDefinedSignals":
    case "eeschema.Simulator.toggleSimSidePanel":
    case "eeschema.Simulator.toggleSimConsole":
    case "eeschema.Simulator.toggleDarkModePlots":
      return s.open && s.focused;
    case "eeschema.Simulator.exportPNG":
    case "eeschema.Simulator.exportCSV":
    case "eeschema.Simulator.exportToClipboard":
    case "eeschema.Simulator.exportPlotToSchematic":
    case "eeschema.Simulator.toggleLegend":
    case "eeschema.Simulator.toggleDottedSecondary":
    case "common.Control.zoomInHorizontally":
    case "common.Control.zoomOutHorizontally":
    case "common.Control.zoomInVertically":
    case "common.Control.zoomOutVertically":
      return s.open && s.focused && havePlot;
    case "common.Control.undoZoom":
      return s.open && s.focused && havePlot && tab!.plot!.view.undo.length > 0;
    case "common.Control.redoZoom":
      return s.open && s.focused && havePlot && tab!.plot!.view.redo.length > 0;
    case "eeschema.Simulation.simAnalysisProperties":
      return s.open && s.focused && !!tab;
    case "eeschema.Simulation.runSimulation":
      return s.open && s.focused && !s.frame.running;
    case "eeschema.Simulation.stopSimulation":
      return s.open && s.focused && s.frame.running;
    default:
      return undefined;
  }
}

/** The simulator's toggles (`setupUIConditions`' CHECK conditions). */
export function simChecked(name: string, s: SimUi = current): boolean | undefined {
  const plot = currentTab(s.frame)?.plot ?? null;
  switch (name) {
    case "eeschema.Simulator.toggleSimSidePanel":
      return s.frame.sidePanelShown;
    case "eeschema.Simulator.toggleSimConsole":
      return s.frame.consoleShown;
    case "eeschema.Simulator.toggleLegend":
      return !!plot?.legend.shown;
    case "eeschema.Simulator.toggleDottedSecondary":
      return !!plot?.dotted;
    case "eeschema.Simulator.toggleDarkModePlots":
      return s.frame.dark;
    case "eeschema.EditorControl.showOperatingPointVoltages":
      return s.showOpVoltages;
    case "eeschema.EditorControl.showOperatingPointCurrents":
      return s.showOpCurrents;
    case "common.Control.toggleGrid":
      return s.open && s.focused && plot ? gridShown(plot) : undefined;
    default:
      return undefined;
  }
}

/** `SIMULATOR_FRAME_UI::ToggleDarkModePlots`, remembered as the setting `simulator.white_background`. */
export function toggleDarkMode(): void {
  current.frame.dark = !current.frame.dark;
  saveSetting({ white_background: !current.frame.dark });
  emit();
}

/** `SCH_EDITOR_CONTROL::ToggleOPVoltages` / `ToggleOPCurrents`. */
export function toggleOp(which: "voltages" | "currents"): void {
  if (which === "voltages") {
    current.showOpVoltages = !current.showOpVoltages;
    saveSetting({ show_op_voltages: current.showOpVoltages });
  } else {
    current.showOpCurrents = !current.showOpCurrents;
    saveSetting({ show_op_currents: current.showOpCurrents });
  }
  emit();
}

/** Switch tabs (`onPlotChanged`). */
export function selectTab(i: number): void {
  current.frame.current = i;
  rebuildSignals(current.frame);
  emit();
}

/** Close a tab (`onPlotClose` / `onPlotClosed`: modified, and the operating point off the schematic unless the tab left is an OP). */
export function closeTab(i: number): void {
  const f = current.frame;
  f.tabs.splice(i, 1);
  if (f.current >= f.tabs.length) f.current = f.tabs.length - 1;
  f.modified = true;
  rebuildSignals(f);
  if (simTypeOf(currentTab(f)) !== "OP") current.opPoints = new Map();
  emit();
}

/** `SIMULATOR_CONTROL::ShowNetlist`: the netlist the current tab's command makes, with what the netlister said. */
export async function showNetlist(): Promise<void> {
  const f = current.frame;
  const tab = currentTab(f);
  const r = await loadCircuit(tab ? tab.command : (f.circuit?.sch_sim_command ?? ""), tab ? tab.options : OPTION_DEFAULT_FLAGS);
  if (!r) return;
  setSimDialog({ kind: "netlist", text: r.netlist ?? "", messages: r.messages ?? [] });
}

/** Whether the tab can show a plot: `SIM_TAB::IsPlottable`. */
export const plottable = (tab: SimTab | null) => !!tab && isPlottable(commandToSimType(tab.command));
