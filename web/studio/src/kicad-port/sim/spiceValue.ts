// `SPICE_VALUE` and `SPICE_VALUE_FORMAT` (eeschema/sim/spice_value.cpp, commit 8303b2ad): a number with a SPICE unit prefix ("4.7k", "10Meg"),
// read the way `sscanf( "%lf%7s" )` reads one and written the way the simulator's dialogs, cursors and measurements write it. The server has the
// same port in Rust (crates/sim/src/value.rs); the tests hold both to the same outputs.

export const PFX_FEMTO = -15;
export const PFX_PICO = -12;
export const PFX_NANO = -9;
export const PFX_MICRO = -6;
export const PFX_MILI = -3;
export const PFX_NONE = 0;
export const PFX_KILO = 3;
export const PFX_MEGA = 6;
export const PFX_GIGA = 9;
export const PFX_TERA = 12;

/** C's `strtod` on the longest number at the start of `s` (blanks skipped), and the rest. */
function strtod(s: string): { value: number; rest: string } | null {
  const m = /^\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)/.exec(s);
  if (!m || m[1] === undefined) return null;
  return { value: Number(m[1]), rest: s.slice(m[0].length) };
}

/** The exact decimal digits of |x| (finite): the integer part and the fraction (`toFixed( 100 )` is exact to its 100 digits; a double past
 *  1e21 is an integer). */
function exactDigits(x: number): { int: string; frac: string } {
  const a = Math.abs(x);
  if (a >= 1e21) return { int: BigInt(a).toString(), frac: "" };
  const [int = "0", frac = ""] = a.toFixed(100).split(".");
  return { int, frac };
}

/** The first `keep` digits of `digits`, rounded on the rest half to even as glibc's printf rounds an exact decimal expansion (JavaScript's
 *  `toFixed` and `toExponential` round an exact half away from zero: `(0.0625).toFixed( 3 )` is "0.063" where `%.3f` writes "0.062"). The
 *  result has one digit more when the rounding carries. */
function roundDigits(digits: string, keep: number): string {
  if (keep < 0) return "";
  if (keep >= digits.length) return digits + "0".repeat(keep - digits.length);
  const kept = digits.slice(0, keep);
  const rest = digits.slice(keep);
  const first = rest.charCodeAt(0) - 48;
  const tail = /[1-9]/.test(rest.slice(1));
  const last = keep > 0 ? kept.charCodeAt(keep - 1) - 48 : 0;
  const up = first > 5 || (first === 5 && (tail || last % 2 === 1));
  if (!up) return kept;
  const d = kept.split("");
  let i = d.length - 1;
  for (; i >= 0; i--) {
    if (d[i] === "9") d[i] = "0";
    else {
      d[i] = String.fromCharCode(d[i]!.charCodeAt(0) + 1);
      break;
    }
  }
  return (i < 0 ? "1" : "") + d.join("");
}

/** C's `%.<f>f`. */
function cFixed(x: number, f: number): string {
  const neg = x < 0 || Object.is(x, -0);
  const { int, frac } = exactDigits(x);
  const r = roundDigits(int + frac, int.length + f);
  const ip = r.slice(0, r.length - f).replace(/^0+(?=\d)/, "") || "0";
  return `${neg ? "-" : ""}${ip}${f > 0 ? `.${r.slice(r.length - f)}` : ""}`;
}

/** C's `%.<p-1>e` as its mantissa digits (`p` of them) and its exponent. */
function cExp(x: number, p: number): { digits: string; exp: number } {
  const { int, frac } = exactDigits(x);
  const all = int + frac;
  const lead = all.search(/[1-9]/);
  if (lead < 0) return { digits: "0".repeat(p), exp: 0 };
  const r = roundDigits(all, lead + p);
  // a carry adds a digit in front (9.99 -> 10.0) or eats a leading zero (0.0999 -> 0.100)
  const intLen = int.length + (r.length - (lead + p));
  const at = r.search(/[1-9]/);
  return { digits: r.slice(at, at + p).padEnd(p, "0"), exp: intLen - at - 1 };
}

/** C's `%.<p>g`. */
export function formatG(x: number, precision = 6): string {
  if (Number.isNaN(x)) return "nan";
  if (!Number.isFinite(x)) return x > 0 ? "inf" : "-inf";
  const p = Math.max(1, precision);
  if (x === 0) return Object.is(x, -0) ? "-0" : "0";
  const { digits, exp } = cExp(x, p);
  const strip = (t: string) => (t.includes(".") ? t.replace(/0+$/, "").replace(/\.$/, "") : t);
  if (exp < -4 || exp >= p) {
    const mant = strip(digits.length > 1 ? `${digits[0]}.${digits.slice(1)}` : digits);
    return `${x < 0 ? "-" : ""}${mant}e${exp < 0 ? "-" : "+"}${String(Math.abs(exp)).padStart(2, "0")}`;
  }
  return strip(cFixed(x, Math.max(0, p - 1 - exp)));
}

/** `%.<n>f` (`wxString::FromCDouble( x, n )`), rounded as glibc rounds. */
export function formatFixed(x: number, digits: number): string {
  if (!Number.isFinite(x)) return Number.isNaN(x) ? "nan" : x > 0 ? "inf" : "-inf";
  return cFixed(x, Math.max(0, Math.min(100, digits)));
}

/** `SPICE_VALUE::StripZeros`. */
export function stripZeros(s: string): string {
  if (s.includes(",") || s.includes(".")) {
    s = s.replace(/0+$/, "");
    if (s.endsWith(".") || s.endsWith(",")) s = s.slice(0, -1);
  }
  return s;
}

/** `spice_prefix()`. */
export function spicePrefix(p: number): string {
  switch (p) {
    case PFX_FEMTO: return "f";
    case PFX_PICO: return "p";
    case PFX_NANO: return "n";
    case PFX_MICRO: return "u";
    case PFX_MILI: return "m";
    case PFX_KILO: return "k";
    case PFX_MEGA: return "Meg";
    case PFX_GIGA: return "G";
    case PFX_TERA: return "T";
    default: return "";
  }
}

/** `si_prefix()`: KiCad writes kilo as 'K'. */
export function siPrefix(p: number): string {
  switch (p) {
    case PFX_FEMTO: return "f";
    case PFX_PICO: return "p";
    case PFX_NANO: return "n";
    case PFX_MICRO: return "u";
    case PFX_MILI: return "m";
    case PFX_KILO: return "K";
    case PFX_MEGA: return "M";
    case PFX_GIGA: return "G";
    case PFX_TERA: return "T";
    default: return "";
  }
}

/** `SPICE_VALUE::ParseSIPrefix`. */
export function parseSiPrefix(c: string): number {
  switch (c) {
    case "f": return PFX_FEMTO;
    case "p": return PFX_PICO;
    case "n": return PFX_NANO;
    case "u": return PFX_MICRO;
    case "m": return PFX_MILI;
    case "K": return PFX_KILO;
    case "M": return PFX_MEGA;
    case "G": return PFX_GIGA;
    case "T": return PFX_TERA;
    default: return PFX_NONE;
  }
}

/** `SPICE_VALUE_FORMAT`: significant digits and a range, a prefix and a unit ("mV") or '~' and a unit. */
export interface SpiceValueFormat {
  precision: number;
  range: string;
}

/** `SPICE_VALUE_FORMAT::FromString`. */
export function formatFromString(s: string): SpiceValueFormat {
  if (s === "") return { precision: 3, range: "~V" };
  const d = Number.parseInt(s.charAt(0), 10);
  return { precision: Number.isNaN(d) ? 0 : d, range: s.slice(1) };
}

/** `SPICE_VALUE_FORMAT::ToString`. */
export function formatToString(f: SpiceValueFormat): string {
  return `${Math.max(0, Math.min(9, f.precision))}${f.range}`;
}

/** `SPICE_VALUE_FORMAT::UpdateUnits`. */
export function formatWithUnits(f: SpiceValueFormat, units: string): SpiceValueFormat {
  const c = f.range.charAt(0);
  if (c === "~") return { ...f, range: `~${units}` };
  if (c !== "" && parseSiPrefix(c) !== PFX_NONE) return { ...f, range: `${c}${units}` };
  return { ...f, range: units };
}

export class SpiceValue {
  base: number;
  prefix: number;
  spiceStr: boolean;

  constructor(base = 0, prefix = PFX_NONE, spiceStr = false) {
    this.base = base;
    this.prefix = prefix;
    this.spiceStr = spiceStr;
  }

  /** `SPICE_VALUE( double )`: normalized. */
  static of(x: number, prefix = PFX_NONE): SpiceValue {
    const v = new SpiceValue(x, prefix);
    v.normalize();
    return v;
  }

  /** `SPICE_VALUE( const wxString& )`: `sscanf( "%lf%7s" )`, which never fails -- a text with no number at its start is zero (KiCad 8303b2ad's
   *  constructor throws nothing, so `SPICE_VALIDATOR`'s "'%s' is not a valid SPICE value." is never shown). An empty string is zero. */
  static parse(s: string): SpiceValue {
    const v = new SpiceValue();
    if (s === "") return v;
    const n = strtod(s);
    if (!n) {
      v.normalize();
      return v;
    }
    v.base = n.value;
    const units = (/^\s*(\S{0,7})/.exec(n.rest)?.[1] ?? "").toLowerCase();
    if (units === "") {
      v.normalize();
      return v;
    }
    v.spiceStr = true;
    if (units === "meg") v.prefix = PFX_MEGA;
    else
      v.prefix =
        ({ f: PFX_FEMTO, p: PFX_PICO, n: PFX_NANO, u: PFX_MICRO, m: PFX_MILI, k: PFX_KILO, g: PFX_GIGA, t: PFX_TERA } as Record<string, number>)[units.charAt(0)] ?? PFX_NONE;
    v.normalize();
    return v;
  }

  /** `SPICE_VALUE::Normalize`. */
  normalize(): void {
    while (Math.abs(this.base) >= 1000.0) {
      if (this.prefix === PFX_TERA) break;
      this.base *= 0.001;
      this.prefix += 3;
    }
    while (this.base !== 0.0 && Math.abs(this.base) < 1.0) {
      if (this.prefix === PFX_FEMTO) break;
      this.base *= 1000.0;
      this.prefix -= 3;
    }
  }

  /** `SPICE_VALUE::ToDouble`. */
  toDouble(): number {
    return this.prefix !== PFX_NONE ? this.base * 10 ** this.prefix : this.base;
  }

  /** `SPICE_VALUE::ToNormalizedDouble`: the base and its SPICE prefix. */
  toNormalized(): { value: number; prefix: string } {
    this.normalize();
    return { value: this.base, prefix: spicePrefix(this.prefix) };
  }

  /** `SPICE_VALUE::ToString()`. */
  toPlainString(): string {
    return stripZeros(formatFixed(this.toDouble(), 3));
  }

  /** `SPICE_VALUE::ToSpiceString`. */
  toSpiceString(): string {
    return stripZeros(formatG(this.base, 6)) + spicePrefix(this.prefix);
  }

  /** `SPICE_VALUE::ToOrigString`. */
  toOrigString(): string {
    return this.spiceStr ? this.toSpiceString() : this.toPlainString();
  }

  /** `SPICE_VALUE::ToString( const SPICE_VALUE_FORMAT& )`. */
  toFormatted(format: SpiceValueFormat): string {
    let base = this.base;
    let prefix = this.prefix;
    let range = format.range;
    if (range.endsWith("°")) {
      let deg = base * 10 ** prefix;
      while (deg <= -180) deg += 360;
      while (deg > 180) deg -= 360;
      return `${formatFixed(deg, format.precision)}°`;
    }
    if (range.startsWith("~")) {
      const v = new SpiceValue(base, prefix);
      v.normalize();
      base = v.base;
      prefix = v.prefix;
      range = siPrefix(prefix) + range.slice(1);
    } else {
      const rp = parseSiPrefix(range.charAt(0));
      base = base * 10 ** (prefix - rp);
      prefix = rp;
    }
    let mantissa = base;
    let scale = 0;
    while (Math.abs(mantissa) >= 10.0) {
      mantissa *= 0.1;
      scale += 1;
    }
    while (mantissa !== 0.0 && Math.abs(mantissa) < 1.0) {
      mantissa *= 10;
      scale -= 1;
    }
    const p = format.precision;
    mantissa = kiRound(mantissa * 10 ** (p - 1));
    mantissa *= 10 ** (scale - p + 1);
    let res = formatFixed(mantissa, Math.max(0, p - scale - 1));
    if (res.length > p + scale + 1) res = formatG(mantissa, 6);
    return res + range;
  }

  private combine(other: SpiceValue, f: (a: number, b: number) => number): SpiceValue {
    const diff = this.prefix - other.prefix;
    const r = new SpiceValue(0, PFX_NONE, this.spiceStr || other.spiceStr);
    if (diff > 0) {
      r.base = f(this.base * 10 ** diff, other.base);
      r.prefix = other.prefix;
    } else if (diff < 0) {
      r.base = f(this.base, other.base * 10 ** -diff);
      r.prefix = this.prefix;
    } else {
      r.base = f(this.base, other.base);
      r.prefix = this.prefix;
    }
    r.normalize();
    return r;
  }

  add(o: SpiceValue): SpiceValue {
    return this.combine(o, (a, b) => a + b);
  }

  sub(o: SpiceValue): SpiceValue {
    return this.combine(o, (a, b) => a - b);
  }

  mul(o: SpiceValue): SpiceValue {
    const r = SpiceValue.of(this.base * o.base, this.prefix + o.prefix);
    r.spiceStr = this.spiceStr || o.spiceStr;
    return r;
  }

  div(o: SpiceValue): SpiceValue {
    const r = SpiceValue.of(this.base / o.base, this.prefix - o.prefix);
    r.spiceStr = this.spiceStr || o.spiceStr;
    return r;
  }

  equals(o: SpiceValue): boolean {
    return this.prefix === o.prefix && this.base === o.base;
  }
}

/** `KiROUND`: half away from zero. */
export function kiRound(x: number): number {
  return x < 0 ? -Math.round(-x) : Math.round(x);
}

/** `SPICE_VALUE( aString ).ToSpiceString()` of a field ("0" for a text with no number). */
export function spiceString(s: string): string {
  return SpiceValue.parse(s).toSpiceString();
}

/** `SPICE_VALUE( s ).ToDouble()` (0 for a text with no number). */
export function spiceDouble(s: string): number {
  return SpiceValue.parse(s).toDouble();
}
