// KiCad's NUMERIC_EVALUATOR (common/libeval/numeric_evaluator.cpp, include/libeval/numeric_evaluator.h and the LEMON grammar common/libeval/grammar.lemon at
// 8303b2ad): the arithmetic a `UNIT_BINDER` box takes -- "1.5mm + 10mil", "2*(0.2+0.05)", "w=0.3; w*2" -- worked out in the box's units when the box loses the
// focus (`UNIT_BINDER::onKillFocus`) and again when the dialog reads it (`UNIT_BINDER::GetValue`: `m_eval.Process( value )`, then `m_eval.Result()`).
//
//     getToken      white space is the space alone; a number takes digits and one decimal separator ('.', ',' or the locale's; in SI units also the IEC 60062
//                   letters, "2K2"); then a unit is looked for before a variable name: `"` `in` `mil` `thou` `mm` `cm` `um` `°` `ps` `fs` `ps/in` `ps/cm`
//                   `ps/mm` (each but `"` and `°` not followed by a letter or digit), scaled to the default unit (a unit of another kind scales by 0);
//                   then a name (a letter, then letters and digits); then one of + - * / ( ) = ;  -- anything else is an error
//     grammar       %nonassoc VAR ASSIGN SEMCOL.  %left PLUS MINUS.  %right UNIT.  %left DIVIDE MULT.  So a unit binds tighter than + and -, looser than * and
//                   / ("2*3mm" is (2*3) mm, "1+2mm" is 1 + (2 mm)); a unary - or + takes what binds tighter than + ("-2*3" is -(2*3), "-1+2" is (-1)+2);
//                   statements are separated by ';' and only the last one's value is the result ("x=2; x*3" is 6); an assignment has no value (NaN)
//     errors        a syntax error, an unknown character, an unknown variable, a division by zero: `Process` answers false
//     the result    `UIDouble2Str`: "%.16f" with the trailing zeros cut for 0 < |v| <= 0.0001, else "%.10g"; "NaN" for no value
//
// The variables a box was given stay with it from one evaluation to the next (`m_varMap` of the box's evaluator).
//
// Pure: no React, no DOM. Unit-tested in numericEvaluator.test.ts.

/** `NUMERIC_EVALUATOR::Unit`. */
export type EvalUnit = "mm" | "mil" | "in" | "deg" | "si" | "ps" | "fs" | "ps/in" | "ps/cm" | "ps/mm";

type Unit = EvalUnit | "um" | "cm" | "invalid";

type TokenKind = "ENDS" | "VALUE" | "UNIT" | "VAR" | "PLUS" | "MINUS" | "MULT" | "DIVIDE" | "PARENL" | "PARENR" | "ASSIGN" | "SEMCOL";

interface Token {
  kind: TokenKind;
  value: number;
  text: string;
}

/** `IsOldSchoolDecimalSeparator`: the IEC 60062 letters and their scale (the two micro signs too). */
function oldSchoolScale(ch: string): number | null {
  switch (ch) {
    case "p":
      return 1e-12;
    case "n":
      return 1e-9;
    case "µ":
    case "μ":
    case "u":
      return 1e-6;
    case "m":
    case "L":
      return 1e-3;
    case "R":
    case "F":
      return 1;
    case "k":
    case "K":
      return 1e3;
    case "M":
      return 1e6;
    case "G":
      return 1e9;
    case "T":
      return 1e12;
    default:
      return null;
  }
}

const isDigit = (ch: string | undefined): boolean => ch !== undefined && ch >= "0" && ch <= "9";
const isAlpha = (ch: string | undefined): boolean => ch !== undefined && /^[A-Za-z]$/.test(ch);
const isAlnum = (ch: string | undefined): boolean => isDigit(ch) || isAlpha(ch);

/** `UIDouble2Str`. */
export function uiDouble2Str(v: number): string {
  if (Number.isNaN(v)) return "NaN";
  if (v !== 0 && Math.abs(v) <= 0.0001) {
    let s = v.toFixed(16);
    s = s.replace(/0+$/, "");
    return s.endsWith(".") ? s.slice(0, -1) : s;
  }
  // "%.10g"
  if (v === 0) return "0";
  if (!Number.isFinite(v)) return v > 0 ? "inf" : "-inf";
  const [mant, expText] = v.toExponential(9).split("e") as [string, string];
  const exp = Number(expText);
  if (exp < -4 || exp >= 10) {
    const m = mant.includes(".") ? mant.replace(/0+$/, "").replace(/\.$/, "") : mant;
    const sign = exp < 0 ? "-" : "+";
    return `${m}e${sign}${String(Math.abs(exp)).padStart(2, "0")}`;
  }
  const fixed = v.toFixed(Math.max(0, 9 - exp));
  return fixed.includes(".") ? fixed.replace(/0+$/, "").replace(/\.$/, "") : fixed;
}

/** The factor that takes a value in `from` to the default unit `def` (`getToken`'s UNIT branch; 0 when the two are not of one kind). */
function unitFactor(def: EvalUnit, from: Unit, siScaler: number): number {
  switch (def) {
    case "mm":
      return { in: 25.4, mil: 25.4 / 1000, um: 1 / 1000, mm: 1, cm: 10 }[from as string] ?? 0;
    case "in":
      return { in: 1, mil: 1 / 1000, um: 1 / 25400, mm: 1 / 25.4, cm: 1 / 2.54 }[from as string] ?? 0;
    case "mil":
      return { in: 1000, mil: 1, um: 1 / 25.4, mm: 1000 / 25.4, cm: 1000 / 2.54 }[from as string] ?? 0;
    case "deg":
      return from === "deg" ? 1 : 0;
    case "si":
      return siScaler;
    case "ps":
      return { ps: 1, fs: 1 / 1000 }[from as string] ?? 0;
    case "fs":
      return { ps: 1000, fs: 1 }[from as string] ?? 0;
    case "ps/in":
      return { "ps/in": 1, "ps/cm": 2.54, "ps/mm": 25.4 }[from as string] ?? 0;
    case "ps/cm":
      return { "ps/in": 1 / 2.54, "ps/cm": 1, "ps/mm": 10 }[from as string] ?? 0;
    case "ps/mm":
      return { "ps/in": 1 / 25.4, "ps/cm": 1 / 10, "ps/mm": 1 }[from as string] ?? 0;
  }
}

/** A box's evaluator: `NUMERIC_EVALUATOR( units )`. */
export class NumericEvaluator {
  private defaultUnits: EvalUnit;
  private vars = new Map<string, number>();
  private resultText = "";
  private original = "";
  private parseError = false;

  constructor(units: EvalUnit, private readonly localeSeparator = ".") {
    this.defaultUnits = units;
  }

  /** `SetDefaultUnits`. */
  setDefaultUnits(units: EvalUnit): void {
    this.defaultUnits = units;
  }

  /** `OriginalText`: the text last processed. */
  originalText(): string {
    return this.original;
  }

  /** `Result`: the value of the last statement, as `UIDouble2Str` writes it ("0" for an empty text). */
  result(): string {
    return this.resultText;
  }

  /** `Clear`. */
  clear(): void {
    this.original = "";
    this.resultText = "";
    this.parseError = true;
  }

  /** `Process`: false on any error. */
  process(text: string): boolean {
    this.original = text;
    this.resultText = "0";
    this.parseError = false;
    if (text === "") return true;
    const tokens = this.tokenize(text);
    if (tokens === null) return false;
    this.parse(tokens);
    return !this.parseError;
  }

  private tokenize(input: string): Token[] | null {
    const allowInfix = this.defaultUnits === "si";
    const sep = this.localeSeparator;
    const isDecimalSeparator = (ch: string | undefined): boolean => ch !== undefined && (ch === sep || ch === "." || ch === "," || (allowInfix && oldSchoolScale(ch) !== null));
    const out: Token[] = [];
    let pos = 0;
    for (;;) {
      while (input[pos] === " ") pos++;
      const ch = input[pos];
      if (ch === undefined) {
        out.push({ kind: "ENDS", value: 0, text: "" });
        return out;
      }
      if (isDigit(ch) || isDecimalSeparator(ch)) {
        // extractNumber
        let haveSeparator = false;
        let siScaler = 1;
        let tok = "";
        let c: string | undefined = ch;
        do {
          if (isDecimalSeparator(c)) {
            if (haveSeparator) break;
            haveSeparator = true;
            const s = oldSchoolScale(c!);
            if (allowInfix && s !== null && c !== sep && c !== "." && c !== ",") siScaler = s;
            tok += ".";
          } else {
            tok += c;
          }
          c = input[++pos];
        } while (isDigit(c) || isDecimalSeparator(c));
        out.push({ kind: "VALUE", value: atof(tok) * siScaler, text: "" });
        continue;
      }
      // checkUnit
      const unit = this.checkUnit(input, pos, allowInfix);
      if (unit) {
        pos = unit.pos;
        out.push({ kind: "UNIT", value: unitFactor(this.defaultUnits, unit.unit, unit.siScaler), text: "" });
        continue;
      }
      if (isAlpha(ch)) {
        let end = pos + 1;
        while (isAlnum(input[end])) end++;
        out.push({ kind: "VAR", value: 0, text: input.slice(pos, end) });
        pos = end;
        continue;
      }
      const single: Record<string, TokenKind> = { "+": "PLUS", "-": "MINUS", "*": "MULT", "/": "DIVIDE", "(": "PARENL", ")": "PARENR", "=": "ASSIGN", ";": "SEMCOL" };
      const kind = single[ch];
      if (!kind) return null; // invalid character
      out.push({ kind, value: 0, text: "" });
      pos++;
    }
  }

  private checkUnit(input: string, pos: number, allowInfix: boolean): { unit: Unit; pos: number; siScaler: number } | null {
    const ch = input[pos]!;
    const at = (k: number) => input[pos + k];
    const word = (w: string) => input.startsWith(w, pos) && !isAlnum(at(w.length));
    if (ch === '"') return { unit: "in", pos: pos + 1, siScaler: 1 };
    if (ch === "°") return { unit: "deg", pos: pos + 1, siScaler: 1 };
    const words: [string, Unit][] = [
      ["um", "um"],
      ["ps/in", "ps/in"],
      ["ps/cm", "ps/cm"],
      ["ps/mm", "ps/mm"],
      ["fs", "fs"],
      ["ps", "ps"],
      ["mm", "mm"],
      ["cm", "cm"],
      ["in", "in"],
      ["mil", "mil"],
      ["thou", "mil"],
    ];
    for (const [w, u] of words) if (word(w)) return { unit: u, pos: pos + w.length, siScaler: 1 };
    const s = oldSchoolScale(ch);
    if (allowInfix && s !== null) return { unit: "si", pos: pos + 1, siScaler: s };
    return null;
  }

  /** The LEMON grammar, by precedence climbing over the same precedences. */
  private parse(tokens: Token[]): void {
    let i = 0;
    const peek = () => tokens[i]!;
    const fail = () => {
      this.parseError = true;
      throw new SyntaxError();
    };
    type V = { v: number; valid: boolean };
    // the binding power of an operator after an expression: PLUS MINUS 1, UNIT 2, MULT DIVIDE 3
    const power = (k: TokenKind): number => (k === "PLUS" || k === "MINUS" ? 1 : k === "UNIT" ? 2 : k === "MULT" || k === "DIVIDE" ? 3 : 0);
    const expr = (min: number): V => {
      let left = prefix();
      for (;;) {
        const t = peek();
        const p = power(t.kind);
        if (p === 0 || p < min) return left;
        i++;
        if (t.kind === "UNIT") {
          // expr ::= expr UNIT
          left = { v: left.v * t.value, valid: left.valid };
          continue;
        }
        // %left: the right operand takes only what binds tighter
        const right = expr(p + 1);
        if (t.kind === "PLUS") left = { v: left.v + right.v, valid: right.valid };
        else if (t.kind === "MINUS") left = { v: left.v - right.v, valid: right.valid };
        else if (t.kind === "MULT") left = { v: left.v * right.v, valid: right.valid };
        else {
          if (right.v !== 0) left = { v: left.v / right.v, valid: right.valid };
          else {
            this.parseError = true; // "Divide by zero"
            left = { v: left.v, valid: right.valid };
          }
        }
      }
    };
    const prefix = (): V => {
      const t = tokens[i++]!;
      switch (t.kind) {
        case "VALUE":
          return { v: t.value, valid: true };
        case "MINUS": {
          const b = expr(2);
          return { v: -b.v, valid: b.valid };
        }
        case "PLUS":
          return expr(2);
        case "PARENL": {
          const b = expr(1);
          if (tokens[i++]?.kind !== "PARENR") fail();
          return b;
        }
        case "VAR": {
          if (peek().kind === "ASSIGN") {
            i++;
            const c = expr(1);
            this.vars.set(t.text, c.v);
            return { v: c.v, valid: false };
          }
          const known = this.vars.get(t.text);
          if (known === undefined) {
            this.parseError = true;
            return { v: 0, valid: true };
          }
          return { v: known, valid: true };
        }
        default:
          return fail();
      }
    };
    try {
      // in ::= stmt | in stmt;  stmt ::= ENDS | expr ENDS | expr SEMCOL
      for (;;) {
        if (peek().kind === "ENDS") return;
        const a = expr(1);
        const t = tokens[i++]!;
        if (t.kind === "SEMCOL") {
          this.resultText = uiDouble2Str(NaN);
          continue;
        }
        if (t.kind !== "ENDS") fail();
        this.resultText = uiDouble2Str(a.valid ? a.v : NaN);
        return;
      }
    } catch (e) {
      if (!(e instanceof SyntaxError)) throw e;
    }
  }
}

/** C's `atof` on the number text the lexer kept (digits and one '.'). */
function atof(s: string): number {
  const n = Number.parseFloat(s === "." ? "0" : s.startsWith(".") ? `0${s}` : s);
  return Number.isFinite(n) ? n : 0;
}

// ------------------------------------------------------------------------------------------------------------------------------ a length box

/** A length unit of the editor (`EDA_UNITS::MM`, `MILS`, `INCH`). */
export type LengthUnits = "mm" | "mil" | "in";

const NM_PER: Record<LengthUnits | "um" | "cm", number> = { mm: 1e6, mil: 25_400, in: 25_400_000, um: 1e3, cm: 1e7 };

/**
 * `EDA_UNIT_UTILS::UI::ValueFromString( pcbIUScale, units, text, DISTANCE )` (`DoubleValueFromString`, common/eda_units.cpp): the number the text starts with
 * (digits, a decimal separator ('.' or ','), signs; 0 when there is none), in the unit its next two letters name (`um`, `mm`, `cm`, `mi` / `th`, `in` / `"`,
 * `oz` as 1.37 mil), else in `units`; nanometres, rounded (`KiROUND`).
 */
export function lengthFromString(text: string, units: LengthUnits): number {
  const buf = text.trim().replace(/,/g, ".");
  let brk = 0;
  while (brk < buf.length && /[0-9.+-]/.test(buf[brk]!)) brk++;
  let value = Number.parseFloat(buf.slice(0, brk));
  if (!Number.isFinite(value)) value = 0;
  const unit = buf.slice(brk).trimStart().slice(0, 2).toLowerCase();
  let u: LengthUnits | "um" | "cm" = units;
  if (unit === "um" || unit === "µm" || unit === "μm") u = "um";
  else if (unit === "mm") u = "mm";
  else if (unit === "cm") u = "cm";
  else if (unit === "mi" || unit === "th") u = "mil";
  else if (unit === "in" || unit.startsWith('"')) u = "in";
  else if (unit === "oz") {
    u = "mil";
    value *= 1.37;
  }
  return Math.round(value * NM_PER[u]);
}

/**
 * What a `UNIT_BINDER` length box holds, in nanometres (`UNIT_BINDER::GetValue`): the box's text worked out by its evaluator (`m_eval.Process( value )`, then
 * `m_eval.Result()`), else the text as it is, read by [`lengthFromString`]. `null` for an empty box ("-- leave unchanged --" here).
 */
export function lengthBoxValue(text: string, units: LengthUnits, evaluator: NumericEvaluator = new NumericEvaluator(units)): number | null {
  if (text.trim() === "") return null;
  evaluator.setDefaultUnits(units);
  const value = evaluator.process(text) ? evaluator.result() : text;
  return lengthFromString(value, units);
}

/** `UNIT_BINDER::onKillFocus`: the text the box shows once it loses the focus -- the result when the text works out, else the text as typed. */
export function evaluatedBoxText(text: string, units: EvalUnit, evaluator: NumericEvaluator = new NumericEvaluator(units)): string {
  evaluator.setDefaultUnits(units);
  return evaluator.process(text) && text !== "" ? evaluator.result() : text;
}
