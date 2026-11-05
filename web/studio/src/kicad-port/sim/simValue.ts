// `SIM_VALUE` (eeschema/sim/sim_value.cpp, sim_value.h, KiCad 8303b2ad): the values of the `Sim.*` fields. KiCad's fields are in "SI" notation
// (`M` mega, `m` milli, a `.` decimal point); SPICE's in its own (`Meg` mega, `m` and `M` milli, case-insensitive). The built-in models' SPICE
// generator (simModelBuiltin.ts) writes every parameter through `ToSpice`.
//
// `SIM_VALUE::ToDouble` and `Normalize` are already in spiceValue.ts (`simValueToDouble`, `simValueNormalize`, round 6); this module adds the
// grammar's whole parse (`SIM_VALUE_PARSER::Parse`, both notations), `ConvertNotation`, `ToSpice`, `ToInt` and `Equal`.
//
// Pure: no React, no DOM.
import { formatG, simValueNormalize, simValueToDouble } from "./spiceValue";

export type Notation = "SI" | "SPICE";

/** `SIM_VALUE_PARSER::PARSE_RESULT`. */
export interface ParseResult {
  isOk: boolean;
  isEmpty: boolean;
  significand: string;
  intPart?: number;
  fracPart?: number;
  exponent?: number;
  unitPrefixExponent?: number;
}

/** `SIM_VALUE_GRAMMAR::unitPrefix< TYPE_FLOAT, SPICE >`: case-insensitive, "Meg" tried before "m". */
const SPICE_PREFIXES = ["f", "p", "n", "u", "meg", "m", "k", "g", "t"];
const SI_PREFIXES = "afpnumkKMGTPE";

/** `SIM_VALUE_PARSER::Parse( aString, aNotation, TYPE_FLOAT )`: `number< TYPE_FLOAT, Notation >` up to the end of the string -- an optional
 * significand (`sor< 1.5, 1., 1, .5, ., - >`), an optional `e`/`E` exponent (its digits optional, but `std::stoi` of none throws: not ok), an
 * optional unit prefix, and for SPICE any letters after it (`garbageSuffix`). A text the grammar refuses is `isOk = false`. */
export function parseSimValue(s: string, notation: Notation = "SI"): ParseResult {
  const r: ParseResult = { isOk: true, isEmpty: true, significand: "" };
  let i = 0;
  const digits = () => {
    const j = i;
    while (i < s.length && s[i]! >= "0" && s[i]! <= "9") i++;
    return s.slice(j, i);
  };
  // significand: intPart = opt<sign> digits
  const start = i;
  const intPart = () => {
    const j = i;
    if (s[i] === "+" || s[i] === "-") i++;
    if (digits() === "") {
      i = j;
      return null;
    }
    return s.slice(j, i);
  };
  let ip = intPart();
  let fp: string | null = null;
  if (ip !== null) {
    if (s[i] === ".") {
      i++;
      const f = digits();
      if (f !== "") fp = f;
    }
  } else if (s[i] === ".") {
    i++;
    const f = digits();
    if (f !== "") fp = f;
  } else if (s[i] === "-") {
    i++;
  }
  if (i > start) {
    r.significand = s.slice(start, i);
    r.isEmpty = false;
    // `std::stoll` of the parts (an out-of-range one makes the parse not ok)
    if (ip !== null) {
      const v = Number(ip);
      if (!Number.isSafeInteger(v) && Math.abs(v) > 9.2e18) r.isOk = false;
      r.intPart = v;
    }
    if (fp !== null) {
      const v = Number(fp);
      if (Math.abs(v) > 9.2e18) r.isOk = false;
      r.fracPart = v;
    }
  }
  ip = null;
  // exponentWithPrefix
  if (s[i] === "e" || s[i] === "E") {
    const j = i;
    i++;
    if (s[i] === "+" || s[i] === "-") i++;
    digits();
    const text = s.slice(j + 1, i);
    r.isEmpty = false;
    if (!/\d/.test(text)) {
      // `std::stoi( "" )` / `( "+" )` throws std::invalid_argument, which Parse catches: not ok
      return { ...r, isOk: false };
    }
    r.exponent = Number.parseInt(text, 10);
  }
  // unitPrefix
  if (notation === "SI") {
    if (i < s.length && SI_PREFIXES.includes(s[i]!)) {
      r.unitPrefixExponent = unitPrefixToExponent(s[i]!, "SI");
      r.isEmpty = false;
      i++;
    }
  } else {
    const rest = s.slice(i).toLowerCase();
    const p = SPICE_PREFIXES.find((x) => rest.startsWith(x));
    if (p) {
      r.unitPrefixExponent = unitPrefixToExponent(p, "SPICE");
      r.isEmpty = false;
      i += p.length;
    }
    while (i < s.length && /[A-Za-z]/.test(s[i]!)) i++; // garbageSuffix
  }
  if (i !== s.length) return { isOk: false, isEmpty: true, significand: "" };
  return r;
}

/** `SIM_VALUE_PARSER::UnitPrefixToExponent`. */
export function unitPrefixToExponent(prefix: string, notation: Notation): number {
  if (notation === "SI") {
    if (prefix === "") return 0;
    switch (prefix[0]) {
      case "a": return -18;
      case "f": return -15;
      case "p": return -12;
      case "n": return -9;
      case "u": return -6;
      case "m": return -3;
      case "k":
      case "K": return 3;
      case "M": return 6;
      case "G": return 9;
      case "T": return 12;
      case "P": return 15;
      case "E": return 18;
    }
    return 0;
  }
  switch (prefix.toLowerCase()) {
    case "f": return -15;
    case "p": return -12;
    case "n": return -9;
    case "u": return -6;
    case "m": return -3;
    case "": return 0;
    case "k": return 3;
    case "meg": return 6;
    case "g": return 9;
    case "t": return 12;
  }
  return 0;
}

/** `SIM_VALUE_PARSER::ExponentToUnitPrefix`: the prefix for a base-10 exponent and the exponent it takes away; SPICE has no `a`, `P` or `E`,
 * and writes mega as `Meg`. */
export function exponentToUnitPrefix(e: number, notation: Notation): { prefix: string; reduction: number } {
  const si = notation === "SI";
  if (si && e >= -18 && e <= -15) return { prefix: "a", reduction: -18 };
  if (e >= -15 && e < -12) return { prefix: "f", reduction: -15 };
  if (e >= -12 && e < -9) return { prefix: "p", reduction: -12 };
  if (e >= -9 && e < -6) return { prefix: "n", reduction: -9 };
  if (e >= -6 && e < -3) return { prefix: "u", reduction: -6 };
  if (e >= -3 && e < 0) return { prefix: "m", reduction: -3 };
  if (e >= 0 && e < 3) return { prefix: "", reduction: 0 };
  if (e >= 3 && e < 6) return { prefix: "k", reduction: 3 };
  if (e >= 6 && e < 9) return { prefix: si ? "M" : "Meg", reduction: 6 };
  if (e >= 9 && e < 12) return { prefix: "G", reduction: 9 };
  if (e >= 12 && e < 15) return { prefix: "T", reduction: 12 };
  if (si && e >= 15 && e < 18) return { prefix: "P", reduction: 15 };
  if (si && e >= 18 && e <= 21) return { prefix: "E", reduction: 18 };
  return { prefix: "", reduction: 0 };
}

/** `fast_float::from_chars` of a significand ("1.5", "-2", "1."; "-" and "." alone leave it 0). */
function significandValue(s: string): number {
  const n = Number(s.endsWith(".") ? s.slice(0, -1) : s);
  return Number.isNaN(n) || s === "-" || s === "." || s === "" ? 0 : n;
}

/** `SIM_VALUE::ConvertNotation`: a `,` read as `.`, then the value written with the other notation's prefix, `{:g}` (so "1M" SI is "1Meg" SPICE,
 * "1e-14" is "10f"); a text that does not parse is returned as it is. */
export function convertNotation(s: string, from: Notation, to: Notation): string {
  const buf = s.replace(/,/g, ".");
  const p = parseSimValue(buf, from);
  if (p.isOk && !p.isEmpty && p.significand !== "") {
    let exponent = (p.exponent ?? 0) + (p.unitPrefixExponent ?? 0);
    const { prefix, reduction } = exponentToUnitPrefix(exponent, to);
    const significand = significandValue(p.significand);
    exponent -= reduction;
    return `${formatG(significand * Math.pow(10, exponent))}${prefix}`;
  }
  return s;
}

/** The "plain number" test `SIM_VALUE::ToSpice` and `SIM_MODEL::SetParamValue` share: only digits and `.` (the empty string too). */
export function isPlainNumber(s: string): boolean {
  for (const c of s) if (c !== "." && (c < "0" || c > "9")) return false;
  return true;
}

/** `SIM_VALUE::ToSpice`: a field's value as SPICE reads it -- a plain number as it is, anything else through `ConvertNotation( SI, SPICE )`. */
export function toSpice(s: string): string {
  return isPlainNumber(s) ? s : convertNotation(s, "SI", "SPICE");
}

/** `SIM_VALUE::ToDouble`. */
export function toDouble(s: string, def = Number.NaN): number {
  return simValueToDouble(s, def);
}

/** `SIM_VALUE::ToInt`: a whole number with no fraction, scaled by a non-negative exponent; else `aDefault`. */
export function toInt(s: string, def = -1): number {
  const p = parseSimValue(s, "SI");
  if (p.isOk && !p.isEmpty && p.intPart !== undefined && (p.fracPart === undefined || p.fracPart === 0)) {
    const exponent = (p.exponent ?? 0) + (p.unitPrefixExponent ?? 0);
    if (exponent >= 0) return Math.trunc(p.intPart) * Math.trunc(Math.pow(10, exponent));
  }
  return def;
}

/** `SIM_VALUE::Normalize`. */
export function normalize(v: number): string {
  return simValueNormalize(v);
}

/** `SIM_VALUE::Equal`: within a double's epsilon. */
export function simValueEqual(lh: number, rh: string): boolean {
  return Math.abs(lh - toDouble(rh)) <= Number.EPSILON;
}
