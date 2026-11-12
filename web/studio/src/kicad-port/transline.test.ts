// kicad-port/transline.ts against KiCad's own calculators: transline.golden.json is the output of tools/transline-golden/transline_golden.cpp, which
// compiles common/transline_calculations/ of KiCad 25ec2335 unchanged and runs each calculator over the parameter sets the tuning profile panel hands it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { CoupledMicrostrip, CoupledStripline, Microstrip, Stripline, type SynthesizeOpts, type TCP, type TranslineCalculationBase } from "./transline";

type Num = number | "NaN" | "Infinity" | "-Infinity";
interface RawCase {
  name: string;
  calc: "microstrip" | "stripline" | "coupled_microstrip" | "coupled_stripline";
  op: "analyse" | "synthesize";
  opts: SynthesizeOpts;
  inputs: Record<string, Num>;
  converged: boolean;
  results: Record<string, [Num, string]>;
}

const golden = JSON.parse(readFileSync("src/kicad-port/transline.golden.json", "utf8")) as { kicad: string; transline: RawCase[] };
const num = (v: Num): number => (v === "NaN" ? Number.NaN : v === "Infinity" ? Infinity : v === "-Infinity" ? -Infinity : v);

/** The tolerance: relative 1e-9 (glibc's libm and V8's `Math` may round the last bit of a transcendental differently). */
const REL = 1e-9;

function close(a: number, b: number): boolean {
  if (Number.isNaN(a) || Number.isNaN(b)) return Number.isNaN(a) && Number.isNaN(b);
  if (!Number.isFinite(a) || !Number.isFinite(b)) return a === b;
  return Math.abs(a - b) <= REL * Math.max(Math.abs(a), Math.abs(b)) || Math.abs(a - b) < 1e-300;
}

function makeCalc(kind: RawCase["calc"]): TranslineCalculationBase {
  switch (kind) {
    case "microstrip":
      return new Microstrip();
    case "stripline":
      return new Stripline();
    case "coupled_microstrip":
      return new CoupledMicrostrip();
    case "coupled_stripline":
      return new CoupledStripline();
  }
}

test("the fixture covers each calculator, analysis and synthesis", () => {
  assert.equal(golden.kicad, "25ec2335");
  for (const calc of ["microstrip", "stripline", "coupled_microstrip", "coupled_stripline"]) {
    assert.ok(golden.transline.some((c) => c.calc === calc && c.op === "analyse"), `${calc} analysed`);
    assert.ok(golden.transline.some((c) => c.calc === calc && c.op === "synthesize"), `${calc} synthesized`);
  }
});

let worst = 0;
for (const c of golden.transline) {
  test(`${c.calc} ${c.op} ${c.opts !== "DEFAULT" ? c.opts + " " : ""}matches KiCad: ${c.name}`, () => {
    const calc = makeCalc(c.calc);
    for (const [p, v] of Object.entries(c.inputs)) calc.SetParameter(p as TCP, num(v));
    let converged = false;
    if (c.op === "synthesize") converged = calc.Synthesize(c.opts);
    else calc.Analyse();
    const results = c.op === "synthesize" ? calc.GetSynthesisResults() : calc.GetAnalysisResults();
    assert.equal(converged, c.converged, "Synthesize's return");
    assert.deepEqual([...results.keys()].sort(), Object.keys(c.results).sort(), "the result map's parameters");
    for (const [p, [v, status]] of Object.entries(c.results)) {
      const got = results.get(p as TCP)!;
      assert.equal(got[1], status, `${p}'s status`);
      const want = num(v);
      assert.ok(close(got[0], want), `${p}: ${got[0]} against KiCad's ${want}`);
      if (Number.isFinite(want) && want !== 0) worst = Math.max(worst, Math.abs(got[0] - want) / Math.abs(want));
    }
  });
}

test("the largest relative difference from KiCad stays within 1e-9", () => {
  assert.ok(worst <= REL, `worst ${worst}`);
});
