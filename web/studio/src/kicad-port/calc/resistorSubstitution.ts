// The Resistor Calculator's search (pcb_calculator/resistor_substitution_utils.{h,cpp} at KiCad 25ec2335, `RES_EQUIV_CALC`): the best combination of
// two, three and four E-series resistors (E1..E24, 10 Ω .. 1 MΩ) in series and parallel for a target value, with up to a few values excluded.
// The panel is components/calculator/PanelRCalculator.tsx.
//
// The 2R buffer is sorted with `std::sort`, which is not stable: among combinations of equal value (10R + 10R and 40R | 40R) the one the lookup
// finds depends on where the sort leaves it. `stdSort` below is libstdc++'s introsort (`std::__introsort_loop` with its median-of-three pivot,
// heap-sort fallback and final insertion sort) step for step, so the same combination is picked as in KiCad built with GCC.
// resistorSubstitution.test.ts holds this to tools/calc-golden/rescalc_golden.cpp, KiCad's own file compiled unchanged.
//
// Pure: no React, no DOM.

import { E1, E12, E24, E3, E6, eseriesValues } from "./eseries";

/** Machine epsilon for floating-point equality testing (`epsilon`). */
export const EPSILON = 1e-12;
/** `RES_EQUIV_CALC_FIRST_VALUE` / `RES_EQUIV_CALC_LAST_VALUE`: the range of single resistors considered. */
export const RES_EQUIV_CALC_FIRST_VALUE = 10;
export const RES_EQUIV_CALC_LAST_VALUE = 1e6;

/** `RESISTANCE`: a value with its composition, e.g. {20.0, "10R + 10R"}. */
export interface Resistance {
  value: number;
  name: string;
  parts: number[];
}

const res = (value: number, name: string, parts?: number[]): Resistance => ({ value, name, parts: parts && parts.length ? parts : [value] });

// ------------------------------------------------------------------------------------------------ libstdc++ std::sort

type Less<T> = (a: T, b: T) => boolean;

function lg(n: number): number {
  return 31 - Math.clz32(n);
}

function adjustHeap<T>(a: T[], first: number, holeIndex: number, len: number, value: T, less: Less<T>): void {
  const topIndex = holeIndex;
  let secondChild = holeIndex;
  while (secondChild < Math.trunc((len - 1) / 2)) {
    secondChild = 2 * (secondChild + 1);
    if (less(a[first + secondChild]!, a[first + secondChild - 1]!)) secondChild--;
    a[first + holeIndex] = a[first + secondChild]!;
    holeIndex = secondChild;
  }
  if ((len & 1) === 0 && secondChild === Math.trunc((len - 2) / 2)) {
    secondChild = 2 * (secondChild + 1);
    a[first + holeIndex] = a[first + secondChild - 1]!;
    holeIndex = secondChild - 1;
  }
  // __push_heap
  let parent = Math.trunc((holeIndex - 1) / 2);
  while (holeIndex > topIndex && less(a[first + parent]!, value)) {
    a[first + holeIndex] = a[first + parent]!;
    holeIndex = parent;
    parent = Math.trunc((holeIndex - 1) / 2);
  }
  a[first + holeIndex] = value;
}

function heapSort<T>(a: T[], first: number, last: number, less: Less<T>): void {
  const len = last - first;
  if (len >= 2) {
    // __make_heap
    let parent = Math.trunc((len - 2) / 2);
    for (;;) {
      adjustHeap(a, first, parent, len, a[first + parent]!, less);
      if (parent === 0) break;
      parent--;
    }
  }
  // __sort_heap
  while (last - first > 1) {
    --last;
    const value = a[last]!;
    a[last] = a[first]!;
    adjustHeap(a, first, 0, last - first, value, less);
  }
}

function swap<T>(a: T[], i: number, j: number): void {
  const t = a[i]!;
  a[i] = a[j]!;
  a[j] = t;
}

function moveMedianToFirst<T>(a: T[], result: number, x: number, y: number, z: number, less: Less<T>): void {
  if (less(a[x]!, a[y]!)) {
    if (less(a[y]!, a[z]!)) swap(a, result, y);
    else if (less(a[x]!, a[z]!)) swap(a, result, z);
    else swap(a, result, x);
  } else if (less(a[x]!, a[z]!)) swap(a, result, x);
  else if (less(a[y]!, a[z]!)) swap(a, result, z);
  else swap(a, result, y);
}

function unguardedPartition<T>(a: T[], first: number, last: number, pivot: number, less: Less<T>): number {
  for (;;) {
    while (less(a[first]!, a[pivot]!)) ++first;
    --last;
    while (less(a[pivot]!, a[last]!)) --last;
    if (!(first < last)) return first;
    swap(a, first, last);
    ++first;
  }
}

function introsortLoop<T>(a: T[], first: number, last: number, depthLimit: number, less: Less<T>): void {
  while (last - first > 16) {
    if (depthLimit === 0) {
      heapSort(a, first, last, less);
      return;
    }
    --depthLimit;
    const mid = first + Math.trunc((last - first) / 2);
    moveMedianToFirst(a, first, first + 1, mid, last - 1, less);
    const cut = unguardedPartition(a, first + 1, last, first, less);
    introsortLoop(a, cut, last, depthLimit, less);
    last = cut;
  }
}

function unguardedLinearInsert<T>(a: T[], last: number, less: Less<T>): void {
  const val = a[last]!;
  let next = last - 1;
  while (less(val, a[next]!)) {
    a[last] = a[next]!;
    last = next;
    --next;
  }
  a[last] = val;
}

function insertionSort<T>(a: T[], first: number, last: number, less: Less<T>): void {
  if (first === last) return;
  for (let i = first + 1; i < last; ++i) {
    if (less(a[i]!, a[first]!)) {
      const val = a[i]!;
      for (let k = i; k > first; --k) a[k] = a[k - 1]!;
      a[first] = val;
    } else unguardedLinearInsert(a, i, less);
  }
}

/** libstdc++'s `std::sort( first, last, comp )`, in place. */
export function stdSort<T>(a: T[], less: Less<T>): void {
  const n = a.length;
  if (n === 0) return;
  introsortLoop(a, 0, n, lg(n) * 2, less);
  if (n > 16) {
    insertionSort(a, 0, 16, less);
    for (let i = 16; i < n; ++i) unguardedLinearInsert(a, i, less);
  } else insertionSort(a, 0, n, less);
}

/** `std::lower_bound` over values: the first index whose value is not less than `v`. */
function lowerBound(list: readonly Resistance[], v: number): number {
  let lo = 0;
  let len = list.length;
  while (len > 0) {
    const half = len >> 1;
    if (list[lo + half]!.value < v) {
      lo += half + 1;
      len -= half + 1;
    } else len = half;
  }
  return lo;
}

// ------------------------------------------------------------------------------------------------ names and values

/** `maybeEmbrace`: the text in parentheses when `sym` is one of its top-level operators. */
function maybeEmbrace(text: string, sym: string): string {
  let should = false;
  let level = 0;
  for (const c of text) {
    if (c === "(") level++;
    else if (c === ")") level--;
    else if (c === sym && level === 0) should = true;
  }
  return should ? `(${text})` : text;
}

const serialValue = (r1: number, r2: number) => r1 + r2;
const parallelValue = (r1: number, r2: number) => (r1 * r2) / (r1 + r2);

function serialResistance(r1: Resistance, r2: Resistance): Resistance {
  return res(serialValue(r1.value, r2.value), `${maybeEmbrace(r1.name, "|")} + ${maybeEmbrace(r2.name, "|")}`, [...r1.parts, ...r2.parts]);
}
function parallelResistance(r1: Resistance, r2: Resistance): Resistance {
  return res(parallelValue(r1.value, r2.value), `${maybeEmbrace(r1.name, "+")} | ${maybeEmbrace(r2.name, "+")}`, [...r1.parts, ...r2.parts]);
}
function serialResistanceSimple(r1: Resistance, r2: Resistance): Resistance {
  return res(serialValue(r1.value, r2.value), `${r1.name} + ${r2.name}`, [...r1.parts, ...r2.parts]);
}
function parallelResistanceSimple(r1: Resistance, r2: Resistance): Resistance {
  return res(parallelValue(r1.value, r2.value), `${r1.name} | ${r2.name}`, [...r1.parts, ...r2.parts]);
}

/** `lround`: half away from zero. */
const lround = (x: number) => (x < 0 ? -Math.round(-x) : Math.round(x));

/** `strValue`: "47R", "2K2", "1M" (one digit after the unit letter: enough for series up to E24). */
export function strValue(value: number): string {
  if (value < 1000.0) return `${Math.trunc(value)}R`;
  let div = 1e3;
  let unit = "K";
  if (value >= 1e6) {
    div = 1e6;
    unit = "M";
  }
  value /= div;
  const asInt = Math.trunc(value);
  let result = `${asInt}${unit}`;
  const mantissa = value - asInt;
  if (mantissa > 0) result += String(lround(mantissa * 10));
  return result;
}

// ------------------------------------------------------------------------------------------------ SolutionCollector

class SolutionCollector {
  private bestDeviation = Infinity;
  private best: Resistance | null = null;
  constructor(private readonly target: number) {}

  add2RLookupResults(found: [Resistance, Resistance], valueFunc: (v: number) => number, resultFunc: (r: Resistance) => Resistance): void {
    this.consider(valueFunc(found[0].value), found[0], resultFunc);
    this.consider(valueFunc(found[1].value), found[1], resultFunc);
  }

  getBest(): Resistance {
    if (!this.best) throw new Error("Empty solution collector");
    return this.best;
  }

  private consider(value: number, found: Resistance, resultFunc: (r: Resistance) => Resistance): void {
    const deviation = Math.abs(value - this.target);
    if (deviation + EPSILON < this.bestDeviation) {
      this.bestDeviation = deviation;
      this.best = resultFunc(found);
    } else if (Math.abs(deviation - this.bestDeviation) < EPSILON) {
      const candidate = resultFunc(found);
      if (!this.best || betterCandidate(candidate, this.best)) this.best = candidate;
    }
  }
}

function uniqueCount(r: Resistance): number {
  const parts = [...r.parts].sort((a, b) => a - b);
  let count = 0;
  let last = 0;
  let first = true;
  for (const v of parts) {
    if (first || Math.abs(v - last) > EPSILON) {
      count++;
      last = v;
      first = false;
    }
  }
  return count;
}

function betterCandidate(cand: Resistance, best: Resistance): boolean {
  const cu = uniqueCount(cand);
  const bu = uniqueCount(best);
  if (cu !== bu) return cu < bu;
  if (cand.parts.length !== best.parts.length) return cand.parts.length < best.parts.length;
  return cand.name < best.name;
}

// ------------------------------------------------------------------------------------------------ RES_EQUIV_CALC

/** `RES_EQUIV_CALC::S2R / S3R / S4R`. */
export const S2R = 0;
export const S3R = 1;
export const S4R = 2;

/** `buildSeriesData`: the series' values over every decade from 10 Ω up to and including 1 MΩ. */
function buildSeriesData(list: readonly number[]): Resistance[] {
  const out: Resistance[] = [];
  for (let decade = RES_EQUIV_CALC_FIRST_VALUE; ; decade *= 10.0) {
    const multiplier = decade / list[0]!;
    for (const lv of list) {
      const value = multiplier * lv;
      out.push(res(value, strValue(value)));
      if (value >= RES_EQUIV_CALC_LAST_VALUE) return out;
    }
  }
}

/** `RES_EQUIV_CALC`. */
export class ResEquivCalc {
  private readonly eSeries: Resistance[][] = [E1, E3, E6, E12, E24].map((s) => buildSeriesData(eseriesValues(s)));
  private excludeMask: boolean[] = [];
  private buffer1R: Resistance[] = [];
  private buffer2R: Resistance[] = [];
  private series = E6;
  private target = 0;
  private results: (Resistance | null)[] = [null, null, null];

  /** `SetSeries`: 0..4 for E1, E3, E6, E12, E24. */
  setSeries(series: number): void {
    this.series = series;
  }

  newCalc(target: number): void {
    this.target = target;
    this.excludeMask = new Array<boolean>(this.eSeries[this.series]!.length).fill(false);
    this.results = [null, null, null];
  }

  exclude(value: number): void {
    if (Number.isNaN(value)) return;
    const series = this.eSeries[this.series]!;
    const it = lowerBound(series, value - EPSILON);
    if (it !== series.length && Math.abs(series[it]!.value - value) < EPSILON) this.excludeMask[it] = true;
  }

  calculate(): void {
    this.prepare1RBuffer();
    this.prepare2RBuffer();
    const s2 = this.calculate2RSolution();
    this.results[S2R] = s2;
    if (Math.abs(s2.value - this.target) > EPSILON) {
      const s3 = this.calculate3RSolution();
      this.results[S3R] = s3;
      if (Math.abs(s3.value - this.target) > EPSILON) this.results[S4R] = this.calculate4RSolution();
    }
  }

  /** `GetResults`: null means that the exact value can be achieved using fewer resistors. */
  getResults(): readonly (Resistance | null)[] {
    return this.results;
  }

  private prepare1RBuffer(): void {
    const series = this.eSeries[this.series]!;
    this.buffer1R = series.filter((_, i) => !this.excludeMask[i]);
  }

  private prepare2RBuffer(): void {
    const b: Resistance[] = [];
    const one = this.buffer1R;
    for (let i1 = 0; i1 < one.length; i1++) {
      for (let i2 = i1; i2 < one.length; i2++) {
        b.push(serialResistanceSimple(one[i1]!, one[i2]!));
        b.push(parallelResistanceSimple(one[i1]!, one[i2]!));
      }
    }
    stdSort(b, (x, y) => x.value < y.value);
    this.buffer2R = b;
  }

  private findIn2RBuffer(target: number): [Resistance, Resistance] {
    const b = this.buffer2R;
    if (Number.isNaN(target)) return [b[0]!, b[0]!];
    if (target <= b[0]!.value || target >= b[b.length - 1]!.value) return [b[0]!, b[b.length - 1]!];
    const it = lowerBound(b, target);
    if (it === 0) return [b[0]!, b[0]!];
    if (it === b.length) return [b[it - 1]!, b[it - 1]!];
    return [b[it - 1]!, b[it]!];
  }

  private calculate2RSolution(): Resistance {
    const sol = new SolutionCollector(this.target);
    sol.add2RLookupResults(this.findIn2RBuffer(this.target), (v) => v, (r) => r);
    return sol.getBest();
  }

  private calculate3RSolution(): Resistance {
    const t = this.target;
    const sol = new SolutionCollector(t);
    for (const r of this.buffer1R) {
      sol.add2RLookupResults(this.findIn2RBuffer(t - r.value), (v) => serialValue(v, r.value), (f) => serialResistance(f, r));
      sol.add2RLookupResults(this.findIn2RBuffer((t * r.value) / (r.value - t)), (v) => parallelValue(v, r.value), (f) => parallelResistance(f, r));
    }
    return sol.getBest();
  }

  private calculate4RSolution(): Resistance {
    const t = this.target;
    const sol = new SolutionCollector(t);
    for (const rr of this.buffer2R) {
      sol.add2RLookupResults(this.findIn2RBuffer(t - rr.value), (v) => serialValue(v, rr.value), (f) => serialResistance(f, rr));
      sol.add2RLookupResults(this.findIn2RBuffer((t * rr.value) / (rr.value - t)), (v) => parallelValue(v, rr.value), (f) => parallelResistance(f, rr));
    }
    for (const r1 of this.buffer1R) {
      for (const r2 of this.buffer1R) {
        sol.add2RLookupResults(
          this.findIn2RBuffer(((t - r1.value) * r2.value) / (r1.value + r2.value - t)),
          (v) => serialValue(r1.value, parallelValue(r2.value, v)),
          (f) => serialResistance(r1, parallelResistance(r2, f))
        );
        sol.add2RLookupResults(
          this.findIn2RBuffer((t * r1.value) / (r1.value - t) - r2.value),
          (v) => parallelValue(r1.value, serialValue(r2.value, v)),
          (f) => parallelResistance(r1, serialResistance(r2, f))
        );
      }
    }
    return sol.getBest();
  }
}

/** `min_target_value` / `max_target_value` of panel_r_calculator.cpp: a parallel pair of the smallest to a series of four of the largest. */
export const R_CALC_MIN_TARGET = RES_EQUIV_CALC_FIRST_VALUE / 4;
export const R_CALC_MAX_TARGET = RES_EQUIV_CALC_LAST_VALUE * 4;

/** What a solution row shows (`showResult`): the formula and the error text ("Exact", "<0.01", "+1.23"), or "Not worth using". */
export function solutionTexts(result: Resistance | null, reqr: number, formatSigned2: (x: number) => string): { formula: string; error: string } {
  if (!result) return { formula: "Not worth using", error: "" };
  const error = (result.value / reqr - 1) * 100;
  let es: string;
  if (Math.abs(error) < EPSILON) es = "Exact";
  else if (Math.abs(error) < 0.01) es = "<0.01";
  else es = formatSigned2(error);
  return { formula: result.name, error: es };
}
