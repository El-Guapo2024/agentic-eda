// kicad-port/calc/translinePanel.ts (and the coax, coplanar, waveguide, twisted pair calculators and the coupled lines' 2-D synthesis of
// kicad-port/transline.ts) against KiCad: the "calculator" cases of transline.golden.json are KiCad 25ec2335's calculators driven the way the
// Transmission Lines panel drives them (tools/transline-golden/transline_golden.cpp). Each case types its values into the panel model, analyses or
// synthesizes, and compares the calculator's result maps and the mode strings.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import type { TCP, TranslineResults } from "../transline";
import { defaultPcbCalculatorSettings } from "./calcSettings";
import { COAX_TYPE, CPW_TYPE, MICROSTRIP_TYPE, RECTWAVEGUIDE_TYPE, STANDARD_LOSS_TANGENTS, STANDARD_RELATIVE_DIELECTRIC_CONSTANTS, TRANSLINE_TYPES, TranslinePanelModel, findMatch, formatG, soldermaskShown } from "./translinePanel";

type Num = number | "NaN" | "Infinity" | "-Infinity";
interface CalcCase {
  name: string;
  type: number;
  tline: string;
  op: "analyse" | "synthesize";
  radio: 1 | 2;
  typed: Record<string, number>;
  model: number;
  spec_freq: number;
  mask: [number, number, number, number, number];
  converged: boolean;
  te: string;
  tm: string;
  analysis: Record<string, [Num, string]>;
  synthesis: Record<string, [Num, string]>;
}

const golden = JSON.parse(readFileSync("src/kicad-port/transline.golden.json", "utf8")) as { calculator: CalcCase[] };
const num = (v: Num): number => (v === "NaN" ? Number.NaN : v === "Infinity" ? Infinity : v === "-Infinity" ? -Infinity : v);
const REL = 1e-9;
const close = (a: number, b: number) =>
  Number.isNaN(a) || Number.isNaN(b) ? Number.isNaN(a) && Number.isNaN(b) : !Number.isFinite(a) || !Number.isFinite(b) ? a === b : Math.abs(a - b) <= REL * Math.max(Math.abs(a), Math.abs(b)) || Math.abs(a - b) < 1e-300;

function compare(got: TranslineResults | null, want: Record<string, [Num, string]>, what: string): void {
  assert.ok(got, `${what} map`);
  assert.deepEqual([...got.keys()].sort(), Object.keys(want).sort(), `${what}: the parameters`);
  for (const [p, [v, status]] of Object.entries(want)) {
    const [gv, gs] = got.get(p as TCP)!;
    assert.equal(gs, status, `${what}: ${p}'s status`);
    assert.ok(close(gv, num(v)), `${what}: ${p} = ${gv} against KiCad's ${num(v)}`);
  }
}

/** The model with a case typed into it. */
function typeCase(c: CalcCase): TranslinePanelModel {
  const m = new TranslinePanelModel();
  m.loadSettings(defaultPcbCalculatorSettings());
  m.selectType(c.type);
  for (const [key, v] of Object.entries(c.typed)) {
    const slot = Object.values(m.slots).find((s) => s.prm !== null && TRANSLINE_TYPES[c.type]!.prms[s.prm]!.key === key);
    assert.ok(slot, `a control for ${key}`);
    slot.text = String(v);
  }
  m.radio = c.radio;
  m.dielectricModel = c.model;
  m.specFrequencyText = String(c.spec_freq);
  m.specFrequencyUnit = 3; // Hz
  m.soldermaskPresent = c.mask[0] !== 0;
  m.soldermaskThicknessText = String(c.mask[1]);
  m.soldermaskEpsilonRText = String(c.mask[2]);
  m.soldermaskTanDText = String(c.mask[3]);
  m.soldermaskFillsGaps = c.mask[4] !== 0;
  return m;
}

test("the fixture has every line type analysed and synthesized", () => {
  for (const t of TRANSLINE_TYPES) {
    assert.ok(golden.calculator.some((c) => c.tline === t.name && c.op === "analyse"), `${t.name} analysed`);
    assert.ok(golden.calculator.some((c) => c.tline === t.name && c.op === "synthesize"), `${t.name} synthesized`);
  }
});

for (const c of golden.calculator) {
  test(`Transmission Lines ${c.op} matches KiCad: ${c.name}`, () => {
    const m = typeCase(c);
    if (c.op === "synthesize") m.synthesize();
    else m.analyze();
    compare(m.lastAnalysis, c.analysis, "analysis");
    compare(m.lastSynthesis, c.synthesis, "synthesis");
    if (c.type === RECTWAVEGUIDE_TYPE || c.type === COAX_TYPE) {
      const te = m.messages.find((l) => l.label === "TE-modes:")!.value;
      const tm = m.messages.find((l) => l.label === "TM-modes:")!.value;
      assert.equal(te, c.te || "none");
      assert.equal(tm, c.tm || "none");
    }
  });
}

test("%g as printf writes it", () => {
  assert.equal(formatG(66.9548054344515), "66.9548");
  assert.equal(formatG(1e20), "1e+20");
  assert.equal(formatG(0.000355542), "0.000355542");
  assert.equal(formatG(0.0000355542), "3.55542e-05");
  assert.equal(formatG(123456789), "1.23457e+08");
  assert.equal(formatG(100000), "100000");
  assert.equal(formatG(1000000), "1e+06");
  assert.equal(formatG(0.035), "0.035");
  assert.equal(formatG(1.72e-8), "1.72e-08");
  assert.equal(formatG(Number.NaN), "nan");
  assert.equal(formatG(-Infinity), "-inf");
  assert.equal(formatG(0), "0");
});

test("the defaults: microstrip analysed gives KiCad's results lines and fills Z0 and Ang_l", () => {
  const m = new TranslinePanelModel();
  m.loadSettings(defaultPcbCalculatorSettings());
  assert.equal(m.current, MICROSTRIP_TYPE);
  assert.equal(m.slots.subs0.label, "εr:");
  assert.equal(m.slots.subs0.text, "4.5");
  assert.equal(m.slots.subs4.text, "1e+20");
  assert.equal(m.slots.elec1.enabled, false);
  m.analyze();
  assert.equal(m.slots.elec0.text, "66.9548");
  assert.equal(m.messages[0]!.label, "Effective εr:");
  assert.match(m.messages[1]!.value, / ps\/cm$/);
  assert.match(m.messages[4]!.value, / µm$/);
  assert.equal(m.messages[5]!.label, "");
});

test("statuses: an impossible stripline paints Z0 red, a coax with Dout below Din paints both diameters", () => {
  const m = new TranslinePanelModel();
  m.loadSettings(defaultPcbCalculatorSettings());
  m.selectType(2);
  m.analyze();
  assert.equal(m.slots.elec0.text, "nan");
  assert.equal(m.slots.elec0.status, "error");
  m.selectType(COAX_TYPE);
  const din = Object.values(m.slots).find((s) => s.label === "Din:")!;
  const dout = Object.values(m.slots).find((s) => s.label === "Dout:")!;
  din.text = "3";
  dout.text = "2";
  m.analyze();
  assert.equal(din.status, "warning");
  assert.equal(dout.status, "warning");
  assert.equal(m.slots.elec0.status, "error");
});

test("checkProperties: a negative tan δ gets the warning background", () => {
  const m = new TranslinePanelModel();
  m.loadSettings(defaultPcbCalculatorSettings());
  m.slots.subs1.text = "-1";
  m.analyze();
  assert.equal(m.slots.subs1.status, "warning");
});

test("switching types keeps each type's values; Reset to Defaults restores them", () => {
  const m = new TranslinePanelModel();
  m.loadSettings(defaultPcbCalculatorSettings());
  m.slots.phys0.text = "0.33";
  m.selectType(CPW_TYPE);
  assert.equal(m.slots.phys0.label, "W:");
  assert.equal(m.slots.phys1.label, "S:");
  m.selectType(MICROSTRIP_TYPE);
  assert.equal(m.slots.phys0.text, "0.33");
  m.reset();
  assert.equal(m.slots.phys0.text, "0.2");
});

test("SaveSettings / LoadSettings round trip: keywords, units, the panel-level fields", () => {
  const m = new TranslinePanelModel();
  const cfg = defaultPcbCalculatorSettings();
  m.loadSettings(cfg);
  m.selectType(COAX_TYPE);
  const din = Object.values(m.slots).find((s) => s.label === "Din:")!;
  din.text = "0.9";
  din.unit = 3; // mil
  m.dielectricModel = 1;
  m.specFrequencyText = "2.5";
  m.soldermaskPresent = true;
  m.soldermaskThicknessText = "25";
  m.saveSettings(cfg);
  assert.equal(cfg.translines.type, COAX_TYPE);
  assert.equal(cfg.translines.dielectric_model, 1);
  assert.equal(cfg.translines.spec_frequency, "2.5");
  assert.equal(cfg.translines.soldermask_present, 1);
  assert.equal(cfg.translines.soldermask_thickness, 25 * 1.0e-6);
  assert.equal(cfg.trans_line.Coax.values.Din, 0.9);
  assert.equal(cfg.trans_line.Coax.units.Din, 3);
  assert.equal(cfg.trans_line.MicroStrip.values.H_t, 1e20);
  assert.equal(cfg.trans_line.MicroStrip.values[""], 0, "the DUMMY line is written under an empty keyword, its NaN as 0");
  const m2 = new TranslinePanelModel();
  m2.loadSettings(cfg);
  assert.equal(m2.current, COAX_TYPE);
  const din2 = Object.values(m2.slots).find((s) => s.label === "Din:")!;
  assert.equal(din2.text, "0.9");
  assert.equal(din2.unit, 3);
  assert.equal(m2.soldermaskThicknessText, "25");
  assert.equal(m2.specFrequencyEnabled, true);
});

test("a stored type out of range is the microstrip", () => {
  const cfg = defaultPcbCalculatorSettings();
  cfg.translines.type = 42;
  const m = new TranslinePanelModel();
  m.loadSettings(cfg);
  assert.equal(m.current, MICROSTRIP_TYPE);
});

test("the soldermask group is shown for microstrip, coupled microstrip and the CPWs; fills-gaps for the CPWs", () => {
  assert.deepEqual([0, 1, 2, 3, 4, 5, 6, 7, 8].map((t) => soldermaskShown(t).group), [true, true, false, false, true, true, false, false, false]);
  assert.deepEqual([0, 1, 4, 5].map((t) => soldermaskShown(t).fillsGaps), [false, false, true, true]);
});

test("findMatch: as text, then as a number; past the list when nothing matches", () => {
  assert.equal(findMatch(STANDARD_RELATIVE_DIELECTRIC_CONSTANTS, "4.5"), 0);
  assert.equal(findMatch(STANDARD_RELATIVE_DIELECTRIC_CONSTANTS, "4"), 11);
  assert.equal(findMatch(STANDARD_LOSS_TANGENTS, "0.0003"), 7);
  assert.equal(findMatch(STANDARD_LOSS_TANGENTS, "7"), STANDARD_LOSS_TANGENTS.length);
});
