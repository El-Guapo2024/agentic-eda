// The simulator's analysis commands and the SPICE Command page of the analysis dialog (commit 8303b2ad):
//
//   eeschema/sim/sim_types.h               SIM_TYPE, SIM_TRACE_TYPE
//   eeschema/sim/spice_circuit_model.cpp   SPICE_CIRCUIT_MODEL::CommandToSimType / ParseDCCommand / ParsePZCommand / ParseNoiseCommand
//   eeschema/sim/spice_simulator.cpp       SPICE_SIMULATOR::TypeToName
//   eeschema/sim/sim_tab.cpp               SIM_TAB::IsPlottable
//   eeschema/dialogs/dialog_sim_command.cpp  DIALOG_SIM_COMMAND::parseCommand / TransferDataFromWindow / evaluateDCControls / updateDCSources /
//                                             updateDCUnits / ApplySettings
//   eeschema/netlist_exporters/netlist_exporter_spice.h   the netlist options
//
// Pure: the dialog component holds a `CommandFields` and asks these functions what command it makes, or which fields a command fills.
import { SpiceValue, spiceString } from "./spiceValue";

export type SimType = "UNKNOWN" | "AC" | "DC" | "DISTO" | "NOISE" | "OP" | "PZ" | "SENS" | "TF" | "TRAN" | "SP" | "FFT";

// SIM_TRACE_TYPE
export const SPT_VOLTAGE = 0x0001;
export const SPT_CURRENT = 0x0002;
export const SPT_AC_PHASE = 0x0004;
export const SPT_AC_GAIN = 0x0008;
export const SPT_POWER = 0x0010;
export const SPT_SP_AMP = 0x0020;
export const SPT_Y_AXIS_MASK = 0x00ff;
export const SPT_TIME = 0x0100;
export const SPT_LIN_FREQUENCY = 0x0200;
export const SPT_LOG_FREQUENCY = 0x0400;
export const SPT_SWEEP = 0x0800;
export const SPT_X_AXIS_MASK = 0xff00;
export const SPT_UNKNOWN = 0x0000;

// NETLIST_EXPORTER_SPICE options
export const OPTION_ADJUST_INCLUDE_PATHS = 0x0010;
export const OPTION_ADJUST_PASSIVE_VALS = 0x0020;
export const OPTION_SAVE_ALL_VOLTAGES = 0x0040;
export const OPTION_SAVE_ALL_CURRENTS = 0x0080;
export const OPTION_SAVE_ALL_DISSIPATIONS = 0x0100;
export const OPTION_SIM_COMMAND = 0x0400;
export const OPTION_SAVE_ALL_EVENTS = 0x0800;
export const OPTION_DEFAULT_FLAGS =
  OPTION_ADJUST_INCLUDE_PATHS | OPTION_ADJUST_PASSIVE_VALS | OPTION_SAVE_ALL_VOLTAGES | OPTION_SAVE_ALL_CURRENTS | OPTION_SAVE_ALL_DISSIPATIONS | OPTION_SAVE_ALL_EVENTS;

/** `SPICE_CIRCUIT_MODEL::CommandToSimType`. */
export function commandToSimType(cmd: string): SimType {
  const c = cmd.toLowerCase().replace(/[\s]+$/, "");
  if (c === ".op") return "OP";
  if (c.startsWith(".ac")) return "AC";
  if (c.startsWith(".dc")) return "DC";
  if (c.startsWith(".tran")) return "TRAN";
  if (c.startsWith(".disto")) return "DISTO";
  if (c.startsWith(".noise")) return "NOISE";
  if (c.startsWith(".pz")) return "PZ";
  if (c.startsWith(".sens")) return "SENS";
  if (c.startsWith(".sp")) return "SP";
  if (c.startsWith(".tf")) return "TF";
  if (c.startsWith("fft") || c.includes("\nfft")) return "FFT";
  return "UNKNOWN";
}

/** `SPICE_SIMULATOR::TypeToName( type, true )`. */
export function shortName(t: SimType): string {
  return t === "UNKNOWN" ? "Custom" : t;
}

/** `SPICE_SIMULATOR::TypeToName( type, false )`. */
export function longName(t: SimType): string {
  switch (t) {
    case "OP": return "DC Operating Point";
    case "AC": return "Small-Signal Analysis";
    case "DC": return "DC Sweep Analysis";
    case "TRAN": return "Transient Analysis";
    case "DISTO": return "Small-Signal Distortion Analysis";
    case "NOISE": return "Noise Analysis";
    case "PZ": return "Pole-Zero Analysis";
    case "SENS": return "Sensitivity Analysis";
    case "TF": return "Transfer Function Analysis";
    case "SP": return "S-Parameter Analysis";
    case "FFT": return "Frequency Content Analysis";
    default: return "Custom";
  }
}

/** `SIM_TAB::IsPlottable`. */
export function isPlottable(t: SimType): boolean {
  return t === "AC" || t === "DC" || t === "SP" || t === "TRAN" || t === "NOISE" || t === "FFT";
}

/** `SIMULATOR_FRAME_UI::getXAxisType`. */
export function xAxisType(t: SimType): number {
  switch (t) {
    case "AC":
    case "SP":
    case "FFT":
    case "NOISE":
      return SPT_LIN_FREQUENCY;
    case "DC":
      return SPT_SWEEP;
    case "TRAN":
      return SPT_TIME;
    default:
      return SPT_UNKNOWN;
  }
}

const tokens = (s: string, seps = /[ \t]+/) => s.split(seps).filter((t) => t !== "");

export interface DcSource {
  source: string;
  start: SpiceValue;
  end: SpiceValue;
  incr: SpiceValue;
}

const sv = (t: string | undefined) => SpiceValue.parse(t ?? "");

/** `SPICE_CIRCUIT_MODEL::ParseDCCommand`. */
export function parseDcCommand(cmd: string): { src1: DcSource; src2: DcSource | null } | null {
  if (!cmd.toLowerCase().startsWith(".dc")) return null;
  const t = tokens(cmd.slice(3));
  const src1 = { source: t[0] ?? "", start: sv(t[1]), end: sv(t[2]), incr: sv(t[3]) };
  const src2 = t.length > 4 ? { source: t[4] ?? "", start: sv(t[5]), end: sv(t[6]), incr: sv(t[7]) } : null;
  return { src1, src2 };
}

export interface NoiseParams {
  output: string;
  ref: string;
  source: string;
  scale: string;
  pts: SpiceValue;
  fStart: SpiceValue;
  fStop: SpiceValue;
  saveAll: boolean;
}

/** `SPICE_CIRCUIT_MODEL::ParseNoiseCommand`. */
export function parseNoiseCommand(cmd: string): NoiseParams | null {
  if (!cmd.toLowerCase().startsWith(".noise")) return null;
  let rest = cmd.slice(6).replace(/^\s+/, "");
  if (!rest.toLowerCase().startsWith("v(")) return null;
  rest = rest.slice(2);
  const close = rest.indexOf(")");
  const fn = close >= 0 ? rest.slice(0, close) : rest;
  const params = close >= 0 ? rest.slice(close + 1) : "";
  const f = tokens(fn, /[ ,\t]+/);
  const p: NoiseParams = { output: f[0] ?? "", ref: f[1] ?? "", source: "", scale: "", pts: new SpiceValue(), fStart: new SpiceValue(), fStop: new SpiceValue(), saveAll: false };
  const t = tokens(params);
  let i = 0;
  if (t[i] !== undefined) p.source = t[i++] ?? "";
  if (t[i] !== undefined && ["dec", "oct", "lin"].includes((t[i] ?? "").toLowerCase())) p.scale = t[i++] ?? "";
  if (t[i] !== undefined) p.pts = sv(t[i++]);
  if (t[i] !== undefined) p.fStart = sv(t[i++]);
  if (t[i] !== undefined) p.fStop = sv(t[i++]);
  p.saveAll = t[i] !== undefined;
  return p;
}

/** `SPICE_CIRCUIT_MODEL::ParsePZCommand`. */
export function parsePzCommand(cmd: string): { tf: string; input: string; inputRef: string; output: string; outputRef: string; poles: boolean; zeros: boolean } | null {
  if (!cmd.toLowerCase().startsWith(".pz")) return null;
  const t = tokens(cmd.slice(3));
  const r = { tf: t[4] ?? "vol", input: t[0] ?? "", inputRef: t[1] ?? "", output: t[2] ?? "", outputRef: t[3] ?? "", poles: true, zeros: true };
  const a = (t[5] ?? "").toLowerCase();
  if (a === "pol") r.zeros = false;
  else if (a === "zer") r.poles = false;
  return r;
}

// ------------------------------------------------------------------------------------------------- the dialog's SPICE Command page

export type Scale = "dec" | "oct" | "lin";

/** The fields of every page of `DIALOG_SIM_COMMAND`, as text, the way its controls hold them. */
export interface CommandFields {
  page: SimType;
  acScale: Scale;
  acPoints: string;
  acStart: string;
  acStop: string;
  spScale: Scale;
  spPoints: string;
  spStart: string;
  spStop: string;
  spNoise: boolean;
  dcType1: "V" | "I" | "R" | "TEMP";
  dcSource1: string;
  dcStart1: string;
  dcStop1: string;
  dcIncr1: string;
  dcEnable2: boolean;
  dcType2: "V" | "I" | "R" | "TEMP";
  dcSource2: string;
  dcStart2: string;
  dcStop2: string;
  dcIncr2: string;
  tranStep: string;
  tranFinal: string;
  tranInitial: string;
  tranMaxStep: string;
  tranUic: boolean;
  fftSignals: string[];
  fftLinearize: boolean;
  noiseMeas: string;
  noiseRef: string;
  noiseSrc: string;
  noiseScale: Scale;
  noisePoints: string;
  noiseStart: string;
  noiseStop: string;
  noiseSaveAll: boolean;
  pzInput: string;
  pzInputRef: string;
  pzOutput: string;
  pzOutputRef: string;
  pzFunction: "vol" | "cur";
  pzAnalyses: "pz" | "pol" | "zer";
  custom: string;
}

/** The controls as `DIALOG_SIM_COMMAND_BASE` makes them (empty texts, the first choices, "Linearize" and "Save contributions" checked). */
export function defaultFields(): CommandFields {
  return {
    page: "OP",
    acScale: "dec",
    acPoints: "",
    acStart: "",
    acStop: "",
    spScale: "dec",
    spPoints: "",
    spStart: "",
    spStop: "",
    spNoise: false,
    dcType1: "V",
    dcSource1: "",
    dcStart1: "",
    dcStop1: "",
    dcIncr1: "",
    dcEnable2: false,
    dcType2: "V",
    dcSource2: "",
    dcStart2: "",
    dcStop2: "",
    dcIncr2: "",
    tranStep: "",
    tranFinal: "",
    tranInitial: "",
    tranMaxStep: "",
    tranUic: false,
    fftSignals: [],
    fftLinearize: true,
    noiseMeas: "",
    noiseRef: "",
    noiseSrc: "",
    noiseScale: "dec",
    noisePoints: "",
    noiseStart: "",
    noiseStop: "",
    noiseSaveAll: true,
    pzInput: "",
    pzInputRef: "",
    pzOutput: "",
    pzOutputRef: "",
    pzFunction: "vol",
    pzAnalyses: "pz",
    custom: "",
  };
}

const scaleOf = (t: string | undefined): Scale | null => {
  const s = (t ?? "").toLowerCase();
  return s === "dec" || s === "oct" || s === "lin" ? s : null;
};

/** `DIALOG_SIM_COMMAND::parseCommand`: the page a command opens on and the fields it fills. `customDirectives` is what `loadDirectives` puts in
 *  the custom text when it is empty (`TransferDataToWindow`: the schematic's analyses). */
export function parseCommand(cmd: string, customDirectives = ""): CommandFields {
  const f = defaultFields();
  f.custom = customDirectives;
  const type = commandToSimType(cmd);
  const t = tokens(cmd, /[ \t\r\n]+/);
  const spice = (x: string | undefined) => spiceString(x ?? "");
  switch (type) {
    case "AC":
      f.page = "AC";
      f.acScale = scaleOf(t[1]) ?? "dec";
      f.acPoints = t[2] ?? "";
      f.acStart = spice(t[3]);
      f.acStop = spice(t[4]);
      break;
    case "SP":
      f.page = "SP";
      f.spScale = scaleOf(t[1]) ?? "dec";
      f.spPoints = t[2] ?? "";
      f.spStart = spice(t[3]);
      f.spStop = spice(t[4]);
      if (t[5] !== undefined) f.spNoise = spice(t[5]) === "1";
      break;
    case "DC": {
      f.page = "DC";
      const p = parseDcCommand(cmd);
      if (p) {
        const typeOf = (src: string): CommandFields["dcType1"] => (src.toUpperCase() === "TEMP" ? "TEMP" : (["V", "I", "R"].includes(src.charAt(0).toUpperCase()) ? (src.charAt(0).toUpperCase() as "V" | "I" | "R") : "V"));
        f.dcType1 = typeOf(p.src1.source);
        f.dcSource1 = p.src1.source;
        f.dcStart1 = p.src1.start.toSpiceString();
        f.dcStop1 = p.src1.end.toSpiceString();
        f.dcIncr1 = p.src1.incr.toSpiceString();
        // `src2.m_vincrement = SPICE_VALUE( -1 )` before the parse: a second source whose increment is -1 is no second source
        if (p.src2 && p.src2.incr.toDouble() !== -1) {
          f.dcEnable2 = true;
          f.dcType2 = typeOf(p.src2.source);
          f.dcSource2 = p.src2.source;
          f.dcStart2 = p.src2.start.toSpiceString();
          f.dcStop2 = p.src2.end.toSpiceString();
          f.dcIncr2 = p.src2.incr.toSpiceString();
        }
      }
      break;
    }
    case "PZ": {
      f.page = "PZ";
      const p = parsePzCommand(cmd);
      if (p) {
        f.pzInput = p.input;
        f.pzInputRef = p.inputRef;
        f.pzOutput = p.output;
        f.pzOutputRef = p.outputRef;
        f.pzFunction = p.tf.toLowerCase() === "cur" ? "cur" : "vol";
        f.pzAnalyses = p.poles && p.zeros ? "pz" : p.poles ? "pol" : "zer";
      }
      break;
    }
    case "NOISE": {
      f.page = "NOISE";
      const p = parseNoiseCommand(cmd);
      if (p) {
        f.noiseMeas = p.output;
        f.noiseRef = p.ref;
        f.noiseSrc = p.source;
        f.noiseScale = scaleOf(p.scale) ?? "dec";
        f.noisePoints = p.pts.toSpiceString();
        f.noiseStart = p.fStart.toSpiceString();
        f.noiseStop = p.fStop.toSpiceString();
        f.noiseSaveAll = p.saveAll;
      }
      break;
    }
    case "TRAN":
      f.page = "TRAN";
      f.tranStep = spice(t[1]);
      f.tranFinal = spice(t[2]);
      if (t[3] !== undefined) f.tranInitial = spice(t[3]);
      if (t[4] !== undefined) f.tranMaxStep = spice(t[4]);
      if (t[5] === "uic") f.tranUic = true;
      break;
    case "OP":
      f.page = "OP";
      break;
    case "FFT":
      f.page = "FFT";
      // every token after the first word, as the `std::set` `m_fftInputSignals` collects them (once each)
      f.fftSignals = [...new Set(t.slice(1).filter((x) => x.toLowerCase() !== "fft" && x.toLowerCase() !== "linearize"))];
      break;
    default:
      // `parseCommand` leaves `m_customTxt` alone: `TransferDataToWindow` fills it with the schematic's analyses (`loadDirectives`), so a Custom
      // tab edited shows those, not its own command, as KiCad's does
      f.page = "UNKNOWN";
      break;
  }
  return f;
}

/** The analysis's item names (`SPICE_CIRCUIT_MODEL::GetItemName`), by reference. */
export type ItemNames = (ref: string) => string;

/** `DIALOG_SIM_COMMAND::evaluateDCControls`: the fields validated (`SPICE_VALIDATOR::Validate`: empty is "Please, fill required fields", a ',' becomes
 *  '.'; `SPICE_VALUE` never fails, so no text is refused), the source named as the exporter names it (`GetItemName`, empty for no item). */
function dcControls(source: string, start: string, stop: string, incr: string, itemName: ItemNames): string | { error: string } {
  if (source === "") return { error: "A DC source must be specified." };
  const e = required(start, stop, incr);
  if (e) return e;
  const src = source === "TEMP" ? source : itemName(source);
  const s = (x: string) => spiceString(x.replace(/,/g, "."));
  return `${src} ${s(start)} ${s(stop)} ${s(incr)}`;
}

/** `SPICE_VALIDATOR::Validate` of required fields: an empty one is refused; any other text is a SPICE value (`SPICE_VALUE` reads no number as 0). */
function required(...vals: string[]): { error: string } | null {
  for (const v of vals) if (v === "") return { error: "Please, fill required fields" };
  return null;
}

function positiveInt(v: string): { error: string } | null {
  const n = Number(v);
  if (v.trim() === "" || !Number.isInteger(n) || n < 1) return { error: `'${v}' is not a valid number of points.` };
  return null;
}

/** `DIALOG_SIM_COMMAND::TransferDataFromWindow`: the command the page makes, or KiCad's message for what is wrong with it. `previous` is the
 *  command the dialog was opened with: a command that changed is trimmed. */
export function buildCommand(f: CommandFields, itemName: ItemNames, previous = "", fftList: string[] = f.fftSignals): { command: string } | { error: string } {
  const s = (x: string) => spiceString(x.replace(/,/g, "."));
  let cmd: string;
  switch (f.page) {
    case "AC": {
      const e = positiveInt(f.acPoints) ?? required(f.acStart, f.acStop);
      if (e) return e;
      cmd = `.ac ${f.acScale} ${f.acPoints} ${s(f.acStart)} ${s(f.acStop)}`;
      break;
    }
    case "SP": {
      const e = positiveInt(f.spPoints) ?? required(f.spStart, f.spStop);
      if (e) return e;
      // `m_spDoNoise ? "1" : ""` tests the check box's pointer, never null: KiCad always writes the 1
      cmd = `.sp ${f.spScale} ${f.spPoints} ${s(f.spStart)} ${s(f.spStop)} 1`;
      break;
    }
    case "DC": {
      const a = dcControls(f.dcSource1, f.dcStart1, f.dcStop1, f.dcIncr1, itemName);
      if (typeof a !== "string") return a;
      cmd = `.dc ${a}`;
      if (f.dcEnable2) {
        const b = dcControls(f.dcSource2, f.dcStart2, f.dcStop2, f.dcIncr2, itemName);
        if (typeof b !== "string") return b;
        cmd += ` ${b}`;
        if (f.dcSource1 === f.dcSource2) return { error: "Source 1 and Source 2 must be different." };
      }
      break;
    }
    case "FFT": {
      // the checked rows of `m_inputSignalsList` (`fftList`, what the filter shows) in its order; `m_fftInputSignals` is a set
      const checked = new Set(f.fftSignals);
      const vectors = fftList.filter((v) => checked.has(v)).map((v) => ` ${v}`).join("");
      cmd = (f.fftLinearize ? `linearize${vectors}\n` : "") + `fft${vectors}`;
      break;
    }
    case "PZ":
      cmd = `.pz ${f.pzInput} ${f.pzInputRef} ${f.pzOutput} ${f.pzOutputRef} ${f.pzFunction} ${f.pzAnalyses}`;
      break;
    case "NOISE": {
      // The page is not validated (KiCad's NOISE branch calls no `Validate`): the points as typed, the frequencies as `SPICE_VALUE` reads them
      // (no ',' made '.'), the reference node through `GetItemName` -- which names items, not nets, so a net gives "v(out,)" as in KiCad.
      if (f.noiseStart === "" || f.noiseStop === "") return { error: "A frequency range must be specified." };
      const ref = f.noiseRef !== "" ? `,${itemName(f.noiseRef)}` : "";
      cmd = `.noise v(${f.noiseMeas}${ref}) ${f.noiseSrc} ${f.noiseScale} ${f.noisePoints} ${spiceString(f.noiseStart)} ${spiceString(f.noiseStop)} ${f.noiseSaveAll ? "1" : ""}`;
      break;
    }
    case "OP":
      cmd = ".op";
      break;
    case "TRAN": {
      const e = required(f.tranStep, f.tranFinal);
      if (e) return e;
      const step = SpiceValue.parse(f.tranStep.replace(/,/g, "."));
      const final = SpiceValue.parse(f.tranFinal.replace(/,/g, "."));
      let start = SpiceValue.of(0);
      if (f.tranInitial !== "") start = SpiceValue.parse(f.tranInitial.replace(/,/g, "."));
      let optionals = f.tranUic ? "uic" : "";
      if (f.tranMaxStep !== "") {
        optionals = `${s(f.tranMaxStep)} ${optionals}`;
      } else if (optionals !== "") {
        let maxStep = final.sub(start).div(SpiceValue.of(50.0));
        if (maxStep.toDouble() > step.toDouble()) maxStep = step;
        optionals = `${maxStep.toSpiceString()} ${optionals}`;
      }
      if (f.tranInitial !== "") optionals = `${start.toSpiceString()} ${optionals}`;
      else if (optionals !== "") optionals = `0 ${optionals}`;
      cmd = `.tran ${step.toSpiceString()} ${final.toSpiceString()} ${optionals}`;
      break;
    }
    default:
      cmd = f.custom;
      break;
  }
  if (cmd !== previous) cmd = cmd.replace(/[ \t\r\n]+$/, "");
  return { command: cmd };
}

/** `DIALOG_SIM_COMMAND::updateDCUnits`. */
export function dcUnit(type: string): string {
  switch (type.charAt(0).toUpperCase()) {
    case "V": return "V";
    case "I": return "A";
    case "R": return "Ω";
    case "T": return "°C";
    default: return "";
  }
}

/** The options the dialog's check boxes make (`DIALOG_SIM_COMMAND::ApplySettings`). */
export function optionsOf(c: { fixIncludePaths: boolean; saveAllVoltages: boolean; saveAllCurrents: boolean; saveAllDissipations: boolean; saveAllEvents: boolean }): number {
  let o = OPTION_DEFAULT_FLAGS;
  if (!c.fixIncludePaths) o &= ~OPTION_ADJUST_INCLUDE_PATHS;
  if (!c.saveAllVoltages) o &= ~OPTION_SAVE_ALL_VOLTAGES;
  if (!c.saveAllCurrents) o &= ~OPTION_SAVE_ALL_CURRENTS;
  if (!c.saveAllDissipations) o &= ~OPTION_SAVE_ALL_DISSIPATIONS;
  if (!c.saveAllEvents) o &= ~OPTION_SAVE_ALL_EVENTS;
  return o;
}

/** `DIALOG_SIM_COMMAND::SetSimOptions`, then `TransferDataToWindow`: the boxes from the tab's option bits, but "Add full path for .include library
 *  directives" from the project's setting (`m_fixIncludePaths->SetValue( m_settings->GetFixIncludePaths() )`), which OK writes back
 *  (`SetFixIncludePaths`) and `ApplySettings` turns into the tab's bit. */
export function commandDialogOptions(options: number, settings: { fix_include_paths: boolean }): { fixIncludePaths: boolean; saveAllVoltages: boolean; saveAllCurrents: boolean; saveAllDissipations: boolean; saveAllEvents: boolean } {
  return {
    fixIncludePaths: !!settings.fix_include_paths,
    saveAllVoltages: !!(options & OPTION_SAVE_ALL_VOLTAGES),
    saveAllCurrents: !!(options & OPTION_SAVE_ALL_CURRENTS),
    saveAllDissipations: !!(options & OPTION_SAVE_ALL_DISSIPATIONS),
    saveAllEvents: !!(options & OPTION_SAVE_ALL_EVENTS),
  };
}

/** `DIALOG_SIM_COMMAND::updateDCSources`: the sources a DC sweep's choice lists for a type letter, the sorted unique references of the items of
 *  that device (`std::set<wxString>`), or "TEMP". The letter is compared as given (`aType == 'R'`), so a lower-case letter lists nothing. */
export function dcSourceChoices(type: string, items: readonly { ref: string; device: string }[]): string[] {
  if (type === "T") return ["TEMP"];
  if (type !== "V" && type !== "I" && type !== "R") return [];
  return [...new Set(items.filter((i) => i.device === type).map((i) => i.ref))].sort();
}

/** `DIALOG_SIM_COMMAND::parseCommand`'s DC sources: `updateDCSources( src.m_source.GetChar( 0 ), ... )` then `SetStringSelection( src.m_source )`
 *  (`wxChoice::FindString`, case-insensitive). A source the list does not hold is no selection (""), which OK refuses with "A DC source must be
 *  specified." -- e.g. the exporter's name `VBAT1` of the reference `BAT1`, or `temp` in lower case; under 'T' the list's "TEMP" stays selected
 *  (`prevSelection`) whatever the text. */
export function selectDcSources(f: CommandFields, items: readonly { ref: string; device: string }[]): CommandFields {
  if (f.page !== "DC") return f;
  const pick = (source: string) => {
    const list = dcSourceChoices(source.charAt(0), items);
    const found = list.find((x) => x.toLowerCase() === source.toLowerCase());
    return found ?? (source.charAt(0) === "T" ? "TEMP" : "");
  };
  const out = { ...f, dcSource1: pick(f.dcSource1) };
  if (f.dcEnable2) out.dcSource2 = pick(f.dcSource2);
  return out;
}

/** The order `parseCommand` lists the analyses in for a new tab (`m_commandType`). */
export const NEW_TAB_TYPES: SimType[] = ["OP", "DC", "AC", "TRAN", "PZ", "NOISE", "SP", "FFT", "UNKNOWN"];

/** A `.wbk`'s `commands` of a tab (`SaveWorkbook`). */
export function commandsOf(command: string, options: number): string[] {
  const v = [command];
  if (options & OPTION_ADJUST_INCLUDE_PATHS) v.push(".kicad adjustpaths");
  if (options & OPTION_SAVE_ALL_VOLTAGES) v.push(".save all");
  if (options & OPTION_SAVE_ALL_CURRENTS) v.push(".probe alli");
  if (options & OPTION_SAVE_ALL_DISSIPATIONS) v.push(".probe allp");
  if (!(options & OPTION_SAVE_ALL_EVENTS)) v.push(".kicad esavenone");
  return v;
}

/** `loadJsonWorkbook`: a tab's command and options from its `commands`. */
export function commandFromCommands(commands: string[]): { command: string; options: number } {
  let options = OPTION_ADJUST_PASSIVE_VALS | OPTION_SAVE_ALL_EVENTS;
  let command = "";
  for (const c of commands) {
    if (c === ".kicad adjustpaths") options |= OPTION_ADJUST_INCLUDE_PATHS;
    else if (c === ".save all") options |= OPTION_SAVE_ALL_VOLTAGES;
    else if (c === ".probe alli") options |= OPTION_SAVE_ALL_CURRENTS;
    else if (c === ".probe allp") options |= OPTION_SAVE_ALL_DISSIPATIONS;
    else if (c === ".kicad esavenone") options &= ~OPTION_SAVE_ALL_EVENTS;
    else command += c.replace(/\s+$/, "");
  }
  return { command, options };
}
