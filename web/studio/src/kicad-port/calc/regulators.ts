// The Regulators panel's logic (KiCad 25ec2335 pcb_calculator): the regulator list (class_regulator_data.h `REGULATOR_DATA` / `REGULATOR_LIST`), its
// data file (datafile_read_write.cpp `PCB_CALCULATOR_DATAFILE` writer and `PCB_CALCULATOR_DATAFILE_PARSER`, keywords pcb_calculator_datafile.keywords),
// the solver (calculator_panels/panel_regulator.cpp `RegulatorsSolve`) and the Add / Edit dialog's checks (dialogs/dialog_regulator_form.cpp).
// The panel is components/calculator/PanelRegulator.tsx.
//
// The data file is KiCad's s-expression file (`*.pcbcalc`): written byte for byte as `WriteHeader` + `Format` write it, read by a small tokenizer
// in place of KiCad's DSNLEXER (same tokens: parentheses, quoted strings with KiCad's escapes, bare words). A parse error says what was expected and
// where, worded as DSNLEXER's `Expecting` / `Unexpected`, without the source file's path (a browser has none).
//
// Pure: no React, no DOM.

import { formatG } from "../sim/spiceValue";
import { strNumCmp } from "../schSheetPins";
import { doubleFromString } from "./unitSelectors";

/** `REGULATOR_DATA`. `type`: 0 with a separate sense pin ("normal"), 1 adjustable 3 pins ("3terminal"). Iadj in µA, Vref in volts. */
export interface RegulatorData {
  name: string;
  type: number;
  vrefMin: number;
  vrefTyp: number;
  vrefMax: number;
  iadjTyp: number;
  iadjMax: number;
}

/** `wxString::CmpNoCase( .. ) == 0`. */
const sameName = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** `REGULATOR_LIST::Add`: inserted before the first name it sorts before (`StrNumCmp( .., true ) < 0`), so the list stays in natural order. */
export function regulatorListAdd(list: readonly RegulatorData[], item: RegulatorData): RegulatorData[] {
  let ii = 0;
  for (; ii < list.length; ii++) if (strNumCmp(item.name, list[ii]!.name, true) < 0) break;
  return [...list.slice(0, ii), item, ...list.slice(ii)];
}

/** `REGULATOR_LIST::GetReg`: by name, ignoring case. */
export function regulatorListGet(list: readonly RegulatorData[], name: string): RegulatorData | null {
  return list.find((r) => sameName(name, r.name)) ?? null;
}

/** `REGULATOR_LIST::Remove`: the first of that name, ignoring case. */
export function regulatorListRemove(list: readonly RegulatorData[], name: string): RegulatorData[] {
  const i = list.findIndex((r) => sameName(name, r.name));
  return i < 0 ? [...list] : [...list.slice(0, i), ...list.slice(i + 1)];
}

/** `REGULATOR_LIST::Replace`: the entry of the same name (ignoring case) replaced in place. */
export function regulatorListReplace(list: readonly RegulatorData[], item: RegulatorData): RegulatorData[] {
  const i = list.findIndex((r) => sameName(item.name, r.name));
  return i < 0 ? [...list] : list.map((r, k) => (k === i ? item : r));
}

// ------------------------------------------------------------------------------------------------ the data file

/** The data file's extension (`DataFileNameExt`). */
export const DATA_FILE_EXT = "pcbcalc";

/** `PANEL_REGULATOR::SetDataFilename` / `GetDataFilename`: the name with its extension made ".pcbcalc" (`wxFileName::SetExt`); empty stays empty. */
export function dataFilename(name: string): string {
  if (!name) return "";
  const slash = Math.max(name.lastIndexOf("/"), name.lastIndexOf("\\"));
  const dot = name.lastIndexOf(".");
  const base = dot > slash + 1 ? name.slice(0, dot) : name;
  return `${base}.${DATA_FILE_EXT}`;
}

/** `OUTPUTFORMATTER::Quotes`. */
function quotes(s: string): string {
  return `"${s.replace(/[\n\r\\"]/g, (c) => (c === "\n" ? "\\n" : c === "\r" ? "\\r" : c === "\\" ? "\\\\" : '\\"'))}"`;
}

const REGTYPE_STR = ["normal", "3terminal"] as const;

/**
 * `WriteDataFile`: `WriteHeader` (datafile, version 2, the date as `wxDateTime::FormatISOCombined( 'T' )`, the tool), `Format` (the regulators list,
 * numbers as fmt's `{:g}`), then the header's parentheses closed. Indentation is `OUTPUTFORMATTER::Print`'s two spaces per nest level.
 */
export function formatDataFile(list: readonly RegulatorData[], date: string, tool: string): string {
  const ind = (n: number) => "  ".repeat(n);
  let out = "";
  let nest = 0;
  out += `${ind(nest++)}(datafile\n`;
  out += `${ind(nest++)}(version 2)\n`;
  out += `${ind(nest++)}(date ${quotes(date)})\n`;
  out += `${ind(nest++)}(tool ${quotes(tool)})\n`;
  let n = nest;
  out += `${ind(n++)}(regulators\n`;
  for (const item of list) {
    out += `${ind(n)}(regulator ${quotes(item.name)}\n`;
    out += `${ind(n + 1)}(reg_vref_min ${formatG(item.vrefMin)})\n`;
    out += `${ind(n + 1)}(reg_vref_typ ${formatG(item.vrefTyp)})\n`;
    out += `${ind(n + 1)}(reg_vref_max ${formatG(item.vrefMax)})\n`;
    if (item.type === 1) {
      out += `${ind(n + 1)}(reg_iadj_typ ${formatG(item.iadjTyp)})\n`;
      out += `${ind(n + 1)}(reg_iadj_max ${formatG(item.iadjMax)})\n`;
    }
    out += `${ind(n + 1)}(reg_type ${REGTYPE_STR[item.type] ?? REGTYPE_STR[0]})\n`;
    out += `${ind(n)})\n`;
  }
  out += `${ind(--n)})\n`;
  while (nest--) out += `${ind(nest)})\n`;
  return out;
}

/** `wxDateTime::Now().FormatISOCombined( 'T' )`: local time, "YYYY-MM-DDTHH:MM:SS". */
export function isoCombinedNow(d = new Date()): string {
  const p = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

type Tok = { kind: "(" | ")" | "str" | "sym"; text: string; line: number; offset: number };

function tokenize(text: string): Tok[] {
  const toks: Tok[] = [];
  let line = 1;
  let lineStart = 0;
  let i = 0;
  while (i < text.length) {
    const c = text[i]!;
    if (c === "\n") {
      line++;
      lineStart = ++i;
      continue;
    }
    if (c === " " || c === "\t" || c === "\r") {
      i++;
      continue;
    }
    const offset = i - lineStart + 1;
    if (c === "(" || c === ")") {
      toks.push({ kind: c, text: c, line, offset });
      i++;
    } else if (c === '"') {
      let s = "";
      i++;
      while (i < text.length && text[i] !== '"') {
        if (text[i] === "\\" && i + 1 < text.length) {
          const e = text[i + 1]!;
          s += e === "n" ? "\n" : e === "r" ? "\r" : e === "t" ? "\t" : e;
          i += 2;
        } else s += text[i++];
      }
      if (i >= text.length) throw new Error(`Unterminated delimited string, line ${line}, offset ${offset}`);
      i++;
      toks.push({ kind: "str", text: s, line, offset });
    } else {
      let s = "";
      while (i < text.length && !/[\s()"]/.test(text[i]!)) s += text[i++];
      toks.push({ kind: "sym", text: s, line, offset });
    }
  }
  return toks;
}

const isNumber = (t: Tok) => t.kind === "sym" && /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(t.text);

/**
 * `ReadDataFile` / `PCB_CALCULATOR_DATAFILE_PARSER::Parse`: the regulators of the file, each added with `REGULATOR_LIST::Add` (so in natural order).
 * Legacy `reg_vref` / `reg_iadj` set min, typ and max at once. Throws the parser's message on a malformed file.
 */
export function parseDataFile(text: string): RegulatorData[] {
  const toks = tokenize(text);
  let p = 0;
  const where = (t: Tok | undefined) => (t ? `line ${t.line}, offset ${t.offset}` : "end of file");
  const next = (): Tok | undefined => toks[p++];
  const unexpected = (t: Tok | undefined): never => {
    throw new Error(t ? `Unexpected '${t.text}', ${where(t)}` : `Unexpected end of file`);
  };
  const needRight = () => {
    const t = next();
    if (t?.kind !== ")") throw new Error(`Expecting ')', ${where(t)}`);
  };
  let list: RegulatorData[] = [];

  const parseNumber = (): number => {
    const t = next();
    if (!t || !isNumber(t)) throw new Error(`Expecting number, ${where(t)}`);
    const v = Number(t.text.trim());
    needRight();
    return v;
  };

  const parseRegulators = () => {
    for (;;) {
      let t = next();
      if (!t) unexpected(t);
      if (t!.kind === ")") return;
      if (t!.kind === "(") t = next();
      if (t?.kind === "sym" && t.text === "regulator") {
        let type = 0;
        let vrefmin = 0.0;
        let vreftyp = 0.0;
        let vrefmax = 0.0;
        let iadjtyp = 0.0;
        let iadjmax = 0.0;
        const nameTok = next();
        if (!nameTok) unexpected(nameTok);
        const name = nameTok!.text;
        for (;;) {
          let k = next();
          if (!k) unexpected(k);
          if (k!.kind === ")") break;
          if (k!.kind === "(") k = next();
          switch (k?.kind === "sym" ? k.text : "") {
            case "reg_vref":
              vreftyp = parseNumber();
              vrefmin = vreftyp;
              vrefmax = vreftyp;
              break;
            case "reg_vref_min":
              vrefmin = parseNumber();
              break;
            case "reg_vref_typ":
              vreftyp = parseNumber();
              break;
            case "reg_vref_max":
              vrefmax = parseNumber();
              break;
            case "reg_iadj":
              iadjtyp = parseNumber();
              iadjmax = iadjtyp;
              break;
            case "reg_iadj_typ":
              iadjtyp = parseNumber();
              break;
            case "reg_iadj_max":
              iadjmax = parseNumber();
              break;
            case "reg_type": {
              const v = next();
              const s = (v?.text ?? "").toLowerCase();
              if (s === REGTYPE_STR[0]) type = 0;
              else if (s === REGTYPE_STR[1]) type = 1;
              else unexpected(v);
              needRight();
              break;
            }
            default:
              unexpected(k);
          }
        }
        if (name !== "") list = regulatorListAdd(list, { name, type, vrefMin: vrefmin, vrefTyp: vreftyp, vrefMax: vrefmax, iadjTyp: iadjtyp, iadjMax: iadjmax });
      }
    }
  };

  for (let t = next(); t; t = next()) {
    if (t.kind === "(") {
      const k = next();
      if (k?.kind === "sym" && k.text === "regulators") parseRegulators();
    }
  }
  return list;
}

// ------------------------------------------------------------------------------------------------ the solver

/** `PANEL_REGULATOR::round_to`: `std::round( value / precision ) * precision` (half away from zero). */
export function roundTo(value: number, precision = 0.001): number {
  const q = value / precision;
  return (q < 0 ? -Math.round(-q) : Math.round(q)) * precision;
}

/** The panel's text boxes RegulatorsSolve reads, as typed. */
export interface RegulatorInputs {
  /** 0 Standard Type, 1 3 Terminal Type. */
  type: number;
  /** The radio button chosen: 0 R1, 1 R2, 2 Vout is calculated. */
  solveFor: number;
  resTol: string;
  r1Typ: string;
  r2Typ: string;
  vrefMin: string;
  vrefTyp: string;
  vrefMax: string;
  voutTyp: string;
  iadjTyp: string;
  iadjMax: string;
}

/** What RegulatorsSolve writes back to the panel (`txt.Printf( "%g", .. )`), or the message it shows (`m_RegulMessage`). */
export type RegulatorSolution =
  | { kind: "message"; message: string }
  | {
      kind: "ok";
      r1Min: string;
      r1Typ: string;
      r1Max: string;
      r2Min: string;
      r2Typ: string;
      r2Max: string;
      voutMin: string;
      voutTyp: string;
      voutMax: string;
      tolTotalMin: string;
      tolTotalMax: string;
      powerComment: string;
    };

/** `PANEL_REGULATOR::RegulatorsSolve`. R1 and R2 are typed in kΩ, Iadj in µA, the tolerance in %. */
export function regulatorsSolve(inp: RegulatorInputs): RegulatorSolution {
  const id = inp.solveFor;
  if (id !== 0 && id !== 1 && id !== 2) return { kind: "message", message: "Selection error" };
  const r1scale = 1000;
  const r2scale = 1000;
  const restol = doubleFromString(inp.resTol) / 100;
  let r1typ = doubleFromString(inp.r1Typ) * r1scale;
  let r2typ = doubleFromString(inp.r2Typ) * r2scale;
  const vrefmin = doubleFromString(inp.vrefMin);
  const vreftyp = doubleFromString(inp.vrefTyp);
  const vrefmax = doubleFromString(inp.vrefMax);
  let vouttyp = doubleFromString(inp.voutTyp);

  if ((vouttyp < vrefmin || vouttyp < vreftyp || vouttyp < vrefmax) && id !== 2) return { kind: "message", message: "Vout must be greater than Vref" };
  if (vrefmin === 0.0 || vreftyp === 0.0 || vrefmax === 0.0) return { kind: "message", message: "Vref set to 0 !" };
  if (vrefmin > vreftyp || vreftyp > vrefmax) return { kind: "message", message: "Vref must VrefMin < VrefTyp < VrefMax" };
  if ((r1typ < 0 && id !== 0) || (r2typ <= 0 && id !== 1)) return { kind: "message", message: "Incorrect value for R1 R2" };

  let r1min: number, r1max: number, r2min: number, r2max: number, voutmin: number, voutmax: number;
  if (inp.type === 1) {
    let iadjtyp = doubleFromString(inp.iadjTyp);
    let iadjmax = doubleFromString(inp.iadjMax);
    if (iadjtyp > iadjmax) return { kind: "message", message: "Iadj must IadjTyp < IadjMax" };
    iadjtyp /= 1000000;
    iadjmax /= 1000000;
    switch (id) {
      case 0:
        r1typ = (vreftyp * r2typ) / (vouttyp - vreftyp - r2typ * iadjtyp);
        break;
      case 1:
        r2typ = (vouttyp - vreftyp) / (iadjtyp + vreftyp / r1typ);
        break;
      case 2:
        vouttyp = (vreftyp * (r1typ + r2typ)) / r1typ;
        vouttyp += r2typ * iadjtyp;
        break;
    }
    r1min = r1typ - r1typ * restol;
    r1max = r1typ + r1typ * restol;
    r2min = r2typ - r2typ * restol;
    r2max = r2typ + r2typ * restol;
    voutmin = (vrefmin * (r1max + r2min)) / r1max;
    voutmin += r2min * iadjtyp;
    voutmax = (vrefmax * (r1min + r2max)) / r1min;
    voutmax += r2max * iadjmax;
  } else {
    switch (id) {
      case 0:
        r1typ = (vouttyp / vreftyp - 1) * r2typ;
        break;
      case 1:
        r2typ = r1typ / (vouttyp / vreftyp - 1);
        break;
      case 2:
        vouttyp = (vreftyp * (r1typ + r2typ)) / r2typ;
        break;
    }
    r1min = r1typ - r1typ * restol;
    r1max = r1typ + r1typ * restol;
    r2min = r2typ - r2typ * restol;
    r2max = r2typ + r2typ * restol;
    voutmin = (vrefmin * (r1min + r2max)) / r2max;
    voutmax = (vrefmax * (r1max + r2min)) / r2min;
  }
  const toltotalmin = ((voutmin - vouttyp) / vouttyp) * 100.0;
  const toltotalmax = ((voutmax - vouttyp) / voutmax) * 100.0;
  const g = (v: number, p?: number) => formatG(roundTo(v, p));
  return {
    kind: "ok",
    r1Min: g(r1min / r1scale),
    r1Typ: g(r1typ / r1scale),
    r1Max: g(r1max / r1scale),
    r2Min: g(r2min / r2scale),
    r2Typ: g(r2typ / r2scale),
    r2Max: g(r2max / r2scale),
    voutMin: g(voutmin),
    voutTyp: g(vouttyp),
    voutMax: g(voutmax),
    tolTotalMin: g(toltotalmin, 0.01),
    tolTotalMax: g(toltotalmax, 0.01),
    powerComment: `${g(vouttyp, 0.01)}V [${g(voutmin, 0.01)}V ... ${g(voutmax, 0.01)}V]`,
  };
}

/** `RegulatorPageUpdate`'s formula label for the type. */
export function regulatorFormula(type: number): string {
  return type === 1 ? "Vout = Vref * (R1 + R2) / R1 + Iadj * R2" : "Vout = Vref * (R1 + R2) / R2";
}

// ------------------------------------------------------------------------------------------------ DIALOG_REGULATOR_FORM

/** The dialog's text boxes. */
export interface RegulatorForm {
  name: string;
  vrefMin: string;
  vrefTyp: string;
  vrefMax: string;
  type: number;
  iadjTyp: string;
  iadjMax: string;
}

/** `CopyRegulatorDataToDialog`: the values as `%.3g`. */
export function regulatorToForm(item: RegulatorData): RegulatorForm {
  const g = (v: number) => formatG(v, 3);
  return { name: item.name, vrefMin: g(item.vrefMin), vrefTyp: g(item.vrefTyp), vrefMax: g(item.vrefMax), type: item.type, iadjTyp: g(item.iadjTyp), iadjMax: g(item.iadjMax) };
}

/**
 * `TransferDataFromWindow`: a name, every Vref given and at least 0.1 V in size, and for a 3 terminal regulator both Iadj whole numbers of at least
 * 1 µA in size (`ToInt`). KiCad's OK does nothing when this fails (no message).
 */
export function regulatorFormValid(f: RegulatorForm): boolean {
  let ok = true;
  if (f.name === "") ok = false;
  for (const v of [f.vrefMin, f.vrefTyp, f.vrefMax]) {
    if (v === "") ok = false;
    // `fabs( vref ) < 0.1`: a text that is no number (NaN) passes, as in KiCad
    else if (Math.abs(doubleFromString(v)) < 0.1) ok = false;
  }
  if (f.type === 1) {
    for (const v of [f.iadjTyp, f.iadjMax]) {
      if (v === "") ok = false;
      else {
        const isInt = /^\s*[+-]?\d+$/.test(v);
        const iadj = isInt ? parseInt(v, 10) : 0;
        if (!isInt || Math.abs(iadj) < 1) ok = false;
      }
    }
  }
  return ok;
}

/** `BuildRegulatorFromData`: Iadj zero for a standard regulator. */
export function regulatorFromForm(f: RegulatorForm): RegulatorData {
  let iadjTyp = doubleFromString(f.iadjTyp);
  let iadjMax = doubleFromString(f.iadjMax);
  if (f.type !== 1) {
    iadjTyp = 0.0;
    iadjMax = 0.0;
  }
  return { name: f.name, type: f.type, vrefMin: doubleFromString(f.vrefMin), vrefTyp: doubleFromString(f.vrefTyp), vrefMax: doubleFromString(f.vrefMax), iadjTyp, iadjMax };
}
