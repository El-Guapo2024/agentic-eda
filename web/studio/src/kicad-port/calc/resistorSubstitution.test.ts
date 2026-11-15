// kicad-port/calc/resistorSubstitution.ts and eseries.ts against KiCad's own code: resistorSubstitution.golden.json is the output of
// tools/calc-golden/rescalc_golden.cpp, which compiles pcb_calculator/resistor_substitution_utils.cpp and eseries.cpp of KiCad 25ec2335 unchanged.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { R_CALC_MAX_TARGET, R_CALC_MIN_TARGET, ResEquivCalc, solutionTexts, stdSort, strValue } from "./resistorSubstitution";
import { E12, E192, E24, E3, E48, E96, eseriesInDecade, eseriesValues } from "./eseries";

type Num = number | "NaN";
const num = (v: Num) => (v === "NaN" ? Number.NaN : v);
const golden = JSON.parse(readFileSync("src/kicad-port/calc/resistorSubstitution.golden.json", "utf8")) as {
  cases: { series: number; target: Num; exclude: [Num, Num]; results: ({ value: Num; name: string } | null)[] }[];
};

test("every golden case finds KiCad's combinations, names and values", () => {
  const calc = new ResEquivCalc();
  for (const c of golden.cases) {
    calc.setSeries(c.series);
    const t = num(c.target);
    calc.newCalc(t);
    calc.exclude(t);
    calc.exclude(num(c.exclude[0]));
    calc.exclude(num(c.exclude[1]));
    calc.calculate();
    const got = calc.getResults().map((r) => (r ? { value: r.value, name: r.name } : null));
    assert.deepEqual(got, c.results.map((r) => (r ? { value: num(r.value), name: r.name } : null)), `E-series ${c.series}, target ${t}`);
  }
  assert.ok(golden.cases.length > 140);
});

test("strValue: R below 1k, K with one mantissa digit, M", () => {
  assert.equal(strValue(47), "47R");
  assert.equal(strValue(2200), "2K2");
  assert.equal(strValue(10000), "10K");
  assert.equal(strValue(1e6), "1M");
});

test("the panel's error text and its target range", () => {
  const signed = (x: number) => `${x >= 0 ? "+" : ""}${x.toFixed(2)}`;
  assert.deepEqual(solutionTexts(null, 1000, signed), { formula: "Not worth using", error: "" });
  assert.deepEqual(solutionTexts({ value: 1000, name: "1K", parts: [1000] }, 1000, signed), { formula: "1K", error: "Exact" });
  assert.equal(solutionTexts({ value: 1000.05, name: "x", parts: [] }, 1000, signed).error, "<0.01");
  assert.equal(solutionTexts({ value: 1010, name: "x", parts: [] }, 1000, signed).error, "+1.00");
  assert.equal(R_CALC_MIN_TARGET, 2.5);
  assert.equal(R_CALC_MAX_TARGET, 4e6);
});

test("E-series: sizes, KiCad's irregular values, the decades", () => {
  assert.deepEqual(eseriesValues(E3), [100, 220, 470]);
  assert.equal(eseriesValues(E12).length, 12);
  assert.equal(eseriesValues(E24)[15], 430);
  assert.equal(eseriesValues(E48).length, 48);
  assert.equal(eseriesValues(E96).length, 96);
  assert.equal(eseriesValues(E192).length, 192);
  assert.equal(eseriesValues(E96)[1], 102);
  assert.deepEqual(eseriesInDecade(E3, 0), [1, 2.2, 4.7]);
});

test("stdSort sorts (libstdc++'s introsort, past the 16-element insertion threshold)", () => {
  const a = Array.from({ length: 500 }, (_, i) => (i * 7919) % 101);
  stdSort(a, (x, y) => x < y);
  assert.deepEqual(a, [...a].sort((x, y) => x - y));
});
