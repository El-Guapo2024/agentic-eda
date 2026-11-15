// C's printf conversions as the Calculator Tools format their values (`wxString::Printf( "%g" )`, `wxString( "" ) << double` -- which is
// `Format( "%g" )` in wxWidgets 3 -- and "%f"), glibc's output in the "C" locale: "%g" with 6 significant digits, trailing zeros dropped, the
// exponent written with a sign and at least two digits ("1e+09"); "nan" and "inf" for the non-finite values.
//
// Pure: no React, no DOM.

function nonFinite(v: number): string | null {
  if (Number.isNaN(v)) return "nan";
  if (!Number.isFinite(v)) return v < 0 ? "-inf" : "inf";
  return null;
}

function stripZeros(s: string): string {
  return s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
}

function cExponent(s: string): string {
  // JS writes "1.5e+9" / "1.5e-7"; C writes at least two exponent digits.
  return s.replace(/e([+-])(\d)$/, "e$10$2");
}

/** `printf( "%.<precision>g", v )`. */
export function printfG(v: number, precision = 6): string {
  const nf = nonFinite(v);
  if (nf !== null) return nf;
  const p = precision === 0 ? 1 : precision;
  if (v === 0) return Object.is(v, -0) ? "-0" : "0";
  const exp = Number(v.toExponential(p - 1).split("e")[1]);
  if (exp < -4 || exp >= p) {
    const [m, e] = v.toExponential(p - 1).split("e");
    return cExponent(`${stripZeros(m!)}e${e!.startsWith("-") ? e : `+${e!.replace("+", "")}`}`);
  }
  return stripZeros(v.toFixed(p - 1 - exp));
}

/** `printf( "%.<precision>f", v )` (`toFixed` switches to exponent form above 1e21, C does not; the calculators' values stay far below). */
export function printfF(v: number, precision = 6): string {
  const nf = nonFinite(v);
  if (nf !== null) return nf;
  return v.toFixed(precision);
}

/** `%.0f`: rounded to an integer, an exact half to the even neighbour (glibc rounds the binary value exactly); "-0" for a negative value that rounds to 0. */
export function formatFixed0(v: number): string {
  if (!Number.isFinite(v)) return nonFinite(v)!;
  const t = Math.trunc(v);
  const frac = Math.abs(v - t);
  let r = frac > 0.5 ? t + Math.sign(v) : frac < 0.5 ? t : t % 2 === 0 ? t : t + Math.sign(v);
  if (r === 0) r = v < 0 || Object.is(v, -0) ? -0 : 0;
  // every digit of the double (String would give the shortest round-trip form above 2^53)
  return Object.is(r, -0) ? "-0" : Math.abs(r) >= 2 ** 53 ? BigInt(r).toString() : String(r);
}

/** `KiROUND` (libs/kimath/include/math/util.h) for a double into an int: half away from zero. */
export function kiRound(v: number): number {
  return Math.trunc(v < 0 ? v - 0.5 : v + 0.5);
}

/**
 * `wxString::ToDouble` in the C locale: the whole text (blanks before it allowed, as strtod skips them) must be a decimal number, else it fails (null).
 * Unlike the panels' `DoubleFromString`, a comma is not a decimal point here.
 */
export function wxToDouble(text: string): number | null {
  const t = text.replace(/^\s+/, "");
  if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(t)) return null;
  return Number(t);
}
