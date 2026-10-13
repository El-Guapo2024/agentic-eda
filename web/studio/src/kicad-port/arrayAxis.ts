// `ARRAY_AXIS` (common/array_axis.cpp), `AlphabeticFromIndex` (common/increment.cpp) and the numbering entries of `DIALOG_CREATE_ARRAY`
// (`numberingTypeData`, `OnAxisNumberingChange`, `validateAxisOptions`, pcbnew/dialogs/dialog_create_array.cpp) at KiCad 8303b2ad: the numbers the Footprint Editor's
// Create Array gives the pads of an array. The same algorithm as crates/ops/src/array_numbering.rs (the backend's answer is the one that counts; this one lets the
// dialog check its entries and show the numbers before OK, and `arrayAxis.test.ts` holds both to the same table of values).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { AxisSpec, NumberingType, PadNumbering } from "../api/types";

export type { AxisSpec, NumberingType, PadNumbering };

/** `numberingTypeData`: the choices of the axis lists, in the dialog's order and words. */
export const NUMBERING_CHOICES: ReadonlyArray<{ type: NumberingType; label: string }> = [
  { type: "numeric", label: "Numerals (0,1,2,...,9,10)" },
  { type: "hex", label: "Hexadecimal (0,1,...,F,10,...)" },
  { type: "alpha_no_iosqxz", label: "Alphabet, minus IOSQXZ" },
  { type: "alpha_full", label: "Alphabet, full 26 characters" },
];

/** `ARRAY_AXIS::TypeIsNumeric`. */
export function isNumeric(type: NumberingType): boolean {
  return type === "numeric" || type === "hex";
}

/** `ARRAY_AXIS::GetAlphabet`. */
export function alphabetOf(type: NumberingType): string {
  switch (type) {
    case "numeric":
      return "0123456789";
    case "hex":
      return "0123456789ABCDEF";
    case "alpha_no_iosqxz":
      return "ABCDEFGHJKLMNPRTUVWY";
    case "alpha_full":
      return "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
  }
}

/** `schemeNonUnitColsStartAt0`: A, B ... Z, AA (the column of tens starts at the first letter); false for 0 ... 9, 10. */
const nonUnitColsStartAt0 = (type: NumberingType): boolean => type === "alpha_full" || type === "alpha_no_iosqxz";

/**
 * `AlphabeticFromIndex`: the `n`-th name of `alphabet` counted as a spreadsheet counts columns. `n` is KiCad's `size_t`, so a negative index is read as the huge
 * number it wraps to there; where the C++ reads `aAlphabet[-1]` (past 26 * 26 - 1, which it leaves undefined) this takes the last letter.
 */
export function alphabeticFromIndex(n: number | bigint, alphabet: string, zeroBasedNonUnitCols: boolean): string {
  const letters = [...alphabet];
  const radix = BigInt(letters.length);
  let rest = BigInt.asUintN(64, BigInt(n));
  const out: string[] = [];
  let firstRound = true;
  do {
    let m = Number(rest % radix);
    if (zeroBasedNonUnitCols && !firstRound) m -= 1;
    if (m < 0) m += letters.length;
    out.unshift(letters[m]!);
    rest /= radix;
    firstRound = false;
  } while (rest !== 0n);
  return out.join("");
}

const INT_MAX = 2147483647;

/** `ARRAY_AXIS`: one axis of an array and the numbers along it. */
export class ArrayAxis {
  kind: NumberingType = "numeric";
  offset = 0;
  step = 1;
  useLowercase = false;

  /** `getNumberingOffset`: the index a numbering string names in this axis's alphabet, or null (empty, or a character the alphabet lacks). */
  private numberingOffset(text: string): number | null {
    if (text.length === 0) return null;
    const alphabet = [...alphabetOf(this.kind)];
    const radix = alphabet.length;
    const chars = [...text];
    let offset = 0;
    for (let i = 0; i < chars.length; i++) {
      let ch = chars[i]!;
      if (!isNumeric(this.kind) && ch >= "a" && ch <= "z") ch = ch.toUpperCase();
      let index = alphabet.indexOf(ch);
      if (index < 0) return null;
      // eg "AA" is actually index 27, not 26
      if (nonUnitColsStartAt0(this.kind) && i < chars.length - 1) index += 1;
      offset = offset * radix + index;
      if (offset > INT_MAX) return null;
    }
    return offset;
  }

  /** `SetOffset( const wxString& )`: false, and nothing changed, when the start does not decode. A start typed in lowercase makes an alphabetic axis print lowercase. */
  setOffsetText(text: string): boolean {
    const offset = this.numberingOffset(text);
    if (offset === null) return false;
    this.useLowercase = !isNumeric(this.kind) && text[0]! >= "a" && text[0]! <= "z";
    this.offset = offset;
    return true;
  }

  /** `GetItemNumber`: the name of the `n`-th point of the axis. */
  itemNumber(n: number): string {
    // `n = m_offset + m_step * n;` on ints, then handed to a `size_t`
    const index = (this.offset + this.step * n) | 0;
    const result = alphabeticFromIndex(index, alphabetOf(this.kind), nonUnitColsStartAt0(this.kind));
    return this.useLowercase ? result.toLowerCase() : result;
  }
}

/** The text of the start entry the dialog shows after the alphabet changes (`OnAxisNumberingChange`): the old one if it still decodes, else the first name of the new alphabet. */
export function startAfterAlphabetChange(kind: NumberingType, current: string): string {
  const axis = new ArrayAxis();
  axis.kind = kind;
  if (axis.setOffsetText(current)) return current;
  axis.offset = isNumeric(kind) ? 1 : 0;
  return axis.itemNumber(0);
}

/** `validateAxisOptions`: the axis these entries make, or the dialog's complaint about the start. */
export function axisOf(spec: AxisSpec): { ok: true; axis: ArrayAxis } | { ok: false; error: string } {
  const axis = new ArrayAxis();
  axis.kind = spec.kind;
  if (!axis.setOffsetText(spec.start)) return { ok: false, error: `Could not determine numbering start from '${spec.start}': expected value consistent with alphabet '${alphabetOf(spec.kind)}'.` };
  axis.step = spec.step | 0;
  return { ok: true, axis };
}

/** The numbering entries as the backend takes them. */
export interface NumberingAxes {
  startSpecified: boolean;
  twoD: boolean;
  primary: ArrayAxis;
  secondary: ArrayAxis;
}

/**
 * `PadNumbering::resolve`: "Use first free number" is an artificial linear scheme of numerals from 1; with a start value each axis in use must decode. Every
 * complaint comes back, one to a line, as the dialog's message box has them.
 */
export function resolveNumbering(n: PadNumbering, circular: boolean): { ok: true; axes: NumberingAxes } | { ok: false; error: string } {
  if (!n.start_specified) {
    const primary = new ArrayAxis();
    primary.offset = 1;
    return { ok: true, axes: { startSpecified: false, twoD: false, primary, secondary: new ArrayAxis() } };
  }
  const twoD = !circular && n.two_d;
  const errors: string[] = [];
  const primary = axisOf(n.primary);
  if (!primary.ok) errors.push(primary.error);
  const secondary = twoD ? axisOf(n.secondary) : ({ ok: true, axis: new ArrayAxis() } as const);
  if (!secondary.ok) errors.push(secondary.error);
  if (!primary.ok || !secondary.ok) return { ok: false, error: errors.join("\n") };
  return { ok: true, axes: { startSpecified: true, twoD, primary: primary.axis, secondary: secondary.axis } };
}

/** `ARRAY_GRID_OPTIONS::getGridCoords`: the column and row of the `n`-th point, in the order the grid is numbered. */
export function gridCoords(n: number, nx: number, ny: number, horizontalThenVertical: boolean, reverseAlternate: boolean): [number, number] {
  const axisSize = Math.max(1, horizontalThenVertical ? nx : ny);
  let x = n % axisSize;
  const y = Math.floor(n / axisSize);
  if (reverseAlternate && y % 2 === 1) x = axisSize - x - 1;
  return [x, y];
}

/** What a grid or a circular array needs to name its `n`-th point. */
export interface NumberedArray {
  circular: boolean;
  nx: number;
  ny: number;
  horizontalThenVertical: boolean;
  reverseAlternate: boolean;
}

/** `ARRAY_OPTIONS::GetItemNumber( n )`. */
export function itemNumber(a: NumberedArray, axes: NumberingAxes, n: number): string {
  if (!a.circular && axes.twoD) {
    const [x, y] = gridCoords(n, a.nx, a.ny, a.horizontalThenVertical, a.reverseAlternate);
    return `${axes.primary.itemNumber(x)}${axes.secondary.itemNumber(y)}`;
  }
  return axes.primary.itemNumber(n);
}

/**
 * `ARRAY_PAD_NUMBER_PROVIDER`: the first `count` numbers it hands out, none of which is in `reserved` (the numbers of the pads that stay where they are; with a start
 * value nothing is reserved). Stops early rather than looping for ever when the entries make a run that never leaves the reserved numbers.
 */
export function providedNumbers(a: NumberedArray, axes: NumberingAxes, reserved: ReadonlySet<string>, count: number): string[] {
  const skip = axes.startSpecified ? new Set<string>() : reserved;
  const out: string[] = [];
  let index = 0;
  for (let tries = 0; out.length < count && tries < 100000; tries++) {
    const number = itemNumber(a, axes, index++);
    if (!skip.has(number)) out.push(number);
  }
  return out;
}
