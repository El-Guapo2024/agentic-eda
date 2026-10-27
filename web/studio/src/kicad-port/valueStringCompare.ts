// `ValueStringCompare` (common/string_utils.cpp) and what it is made of: `SplitString`, `convertSeparators`, `ApplyModifier`. Two strings are split into a beginning,
// the number at their end and what follows the number; the beginnings compare case-blind, then the numbers as numbers (with an SI or IEC 60062 modifier applied:
// "4k7", "10n"), then the endings. So NET2 sorts before NET10, and 4k7 after 470.

const SEPARATORS = ".,";
/** `NUMERIC_EVALUATOR::IsOldSchoolDecimalSeparator` (the 8-bit chars and the two micro signs). */
const OLD_SCHOOL = new Set(["p", "n", "µ", "μ", "u", "m", "L", "R", "F", "k", "K", "M", "G", "T"]);
const isDigit = (c: string): boolean => c >= "0" && c <= "9";

/** `SplitString`: [beginning, digits, end]; an old-school decimal separator inside the digits ("4K7") becomes '.' and starts the end. */
export function splitString(s: string): [string, string, string] {
  if (s.length === 0) return ["", "", ""];
  let ii = s.length - 1;
  while (ii >= 0 && !isDigit(s[ii]!)) ii--;
  if (ii < 0) return [s, "", ""];
  let end = s.slice(ii + 1);
  const position = ii + 1;
  let infix = "";
  for (; ii >= 0; ii--) {
    const c = s[ii]!;
    if (isDigit(c)) continue;
    if (infix === "" && OLD_SCHOOL.has(c)) {
      infix = c;
      continue;
    }
    if (SEPARATORS.includes(c)) continue;
    break;
  }
  let digits: string;
  let beginning = "";
  if (ii < 0) digits = s.slice(0, position);
  else {
    digits = s.slice(ii + 1, position);
    beginning = s.slice(0, ii + 1);
  }
  if (infix !== "") {
    digits = digits.split(infix).join(".");
    end = infix + end;
  }
  return [beginning, digits, end];
}

/** `convertSeparators`: the number in C-locale form, the thousands separators gone, or null when the separators make no sense. (An ambiguous value takes '.' as the decimal, a C locale's.) */
export function convertSeparators(v: string): string | null {
  const value = v.replace(/ /g, "");
  let ambiguous = "";
  let thousands = "";
  let thousandsFound = false;
  let decimal = "";
  let decimalFound = false;
  let digits = 0;
  for (let ii = value.length - 1; ii >= 0; ii--) {
    const c = value[ii]!;
    if (isDigit(c)) digits++;
    else if (c === "." || c === ",") {
      if (decimal !== "" || thousands !== "") {
        if (c === decimal) {
          if (thousandsFound || decimalFound) return null;
          decimalFound = true;
        } else if (c === thousands) {
          if (digits !== 3) return null;
          thousandsFound = true;
        }
      } else if (ambiguous !== "") {
        if (c === ambiguous) {
          thousands = ambiguous;
          thousandsFound = true;
          decimal = c === "." ? "," : ".";
        } else {
          decimal = ambiguous;
          decimalFound = true;
          thousands = c;
          thousandsFound = true;
        }
      } else if ((ii === 1 && value[0] === "0") || digits !== 3) {
        decimal = c;
        decimalFound = true;
        thousands = c === "." ? "," : ".";
      } else ambiguous = c;
      digits = 0;
    } else digits = 0;
  }
  if (decimal === "" && thousands === "") {
    decimal = ".";
    thousands = ",";
  }
  return value.split(thousands).join("").split(decimal).join(".");
}

/** `ApplyModifier`: the value scaled by a leading modifier of `s` when the rest is a unit KiCad knows (or nothing); false when `s` is no modifier ending. */
export function applyModifier(value: number, s: string): [number, boolean] {
  if (s.length === 0) return [value, false];
  const MODIFIERS = "afpnuµμmLRFkKMGTPE";
  const [modifier, units] = MODIFIERS.includes(s[0]!) ? [s[0]!, s.slice(1).trim()] : [" ", s.trim()];
  if (units.length > 0 && !["f", "hz", "w", "v", "a", "h"].includes(units.toLowerCase())) return [value, false];
  const scale: Record<string, number> = { a: 1e-18, f: 1e-15, p: 1e-12, n: 1e-9, u: 1e-6, "µ": 1e-6, "μ": 1e-6, m: 1e-3, L: 1e-3, R: 1, F: 1, k: 1e3, K: 1e3, M: 1e6, G: 1e9, T: 1e12, P: 1e15, E: 1e18 };
  return [value * (scale[modifier] ?? 1), true];
}

/** `wxString::CmpNoCase` as a sign. */
const cmpNoCase = (a: string, b: string): number => {
  const x = a.toLowerCase();
  const y = b.toLowerCase();
  return x < y ? -1 : x > y ? 1 : 0;
};

/** `ValueStringCompare( strFWord, strSWord )`: -1, 0 or 1. */
export function valueStringCompare(first: string, second: string): number {
  const [fBeg, fMid, fEnd] = splitString(first);
  const [sBeg, sMid, sEnd] = splitString(second);
  const beg = cmpNoCase(fBeg, sBeg);
  if (beg !== 0) return beg;
  // `ToCDouble`: `strtod` in the C locale -- the number its longest prefix reads (`1.2.3` is 1.2, round 3 C-8), 0 when nothing does
  const parse = (s: string): number => {
    const c = convertSeparators(s) ?? s;
    const n = Number.parseFloat(c);
    return Number.isFinite(n) ? n : 0;
  };
  const [a, am] = applyModifier(parse(fMid), fEnd);
  const [b, bm] = applyModifier(parse(sMid), sEnd);
  if (a > b) return 1;
  if (a < b) return -1;
  if (!(am || bm)) return cmpNoCase(fEnd, sEnd);
  return 0;
}
