// kicad-port/tuningCalculator.ts against KiCad: the "panel" cases of transline.golden.json are tools/transline-golden/transline_golden.cpp's transcription of
// PANEL_SETUP_TUNING_PROFILE_INFO::calculateTrackParametersForCell (KiCad 25ec2335) over KiCad's own calculators -- a stackup, a grid row, a column, and the
// cells it writes or the message it shows.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import type { Stackup, StackupLayer } from "../api/types";
import { boardParametersFor, calculateTrackParametersForCell, isCopperLayerLowerThan, pcbLayerId, toInt, type CalculationType, type TrackCellInput } from "./tuningCalculator";

interface GoldenStackup {
  name: string;
  layers: { name: string; type: string; sublayers: { thickness_nm: number; epsilon_r: number; loss_tangent: number }[] }[];
}
interface PanelCase {
  name: string;
  stackup: number;
  copper: string[];
  differential: boolean;
  target_impedance: string;
  frequency: number;
  model_solder_mask: boolean;
  signal: string;
  top: string;
  bottom: string;
  width_nm: number | null;
  gap_nm: number | null;
  column: CalculationType;
  result: { kind: "none" } | { kind: "error"; message: string } | { kind: "ok"; width_nm?: number; diff_pair_gap_nm?: number; delay_as_per_mm: number };
}

const golden = JSON.parse(readFileSync("src/kicad-port/transline.golden.json", "utf8")) as { stackups: GoldenStackup[]; panel: PanelCase[] };

/** The fixture's stackup as the studio holds it: a layer's first sublayer is its own fields, the rest `sublayers`. */
function studioStackup(s: GoldenStackup): Stackup {
  return {
    layers: s.layers.map((l): StackupLayer => {
      const [first, ...rest] = l.sublayers;
      const layer: StackupLayer = { name: l.name, material: null, thickness_mm: first!.thickness_nm / 1e6, kind: l.type === "dielectric" ? "core" : l.type };
      if (l.type === "dielectric" || l.type === "soldermask") {
        layer.epsilon_r = first!.epsilon_r;
        layer.loss_tangent = first!.loss_tangent;
      }
      if (rest.length > 0) layer.sublayers = rest.map((x) => ({ thickness_mm: x.thickness_nm / 1e6, epsilon_r: x.epsilon_r, loss_tangent: x.loss_tangent }));
      return layer;
    }),
  };
}

/** The page's row and profile for a fixture case: an empty cell is NaN, a target impedance that is no number NaN. */
function inputOf(c: PanelCase): TrackCellInput {
  const z = c.target_impedance.trim() === "" ? Number.NaN : Number(c.target_impedance);
  return {
    profileType: c.differential ? "differential" : "single",
    targetImpedance: z,
    frequencyHz: c.frequency,
    modelSolderMask: c.model_solder_mask,
    copperLayers: c.copper,
    stackup: studioStackup(golden.stackups[c.stackup]!),
    row: {
      signal_layer: c.signal,
      top_reference_layer: c.top === "" ? null : c.top,
      bottom_reference_layer: c.bottom === "" ? null : c.bottom,
      width_nm: c.width_nm ?? Number.NaN,
      diff_pair_gap_nm: c.gap_nm ?? Number.NaN,
    },
    column: c.column,
  };
}

for (const c of golden.panel) {
  test(`the ${c.column} cell of ${c.differential ? "a differential" : "a single"} ${c.signal} row as KiCad works it out: ${c.name}`, () => {
    const got = calculateTrackParametersForCell(inputOf(c));
    if (c.result.kind === "ok") {
      const { kind: _k, ...cells } = c.result;
      assert.deepEqual(got, { kind: "ok", patch: cells });
    } else {
      assert.deepEqual(got, c.result);
    }
  });
}

test("the fixture has each kind of row and column, and each message", () => {
  const ok = golden.panel.filter((c) => c.result.kind === "ok");
  for (const diff of [false, true]) {
    for (const outer of [true, false]) {
      const cols = new Set(ok.filter((c) => c.differential === diff && (c.signal === "F.Cu" || c.signal === "B.Cu") === outer).map((c) => c.column));
      assert.deepEqual([...cols].sort(), diff ? ["delay", "gap", "width"] : ["delay", "width"], `${diff ? "differential" : "single"} ${outer ? "microstrip" : "stripline"}`);
    }
  }
  const messages = new Set(golden.panel.flatMap((c) => (c.result.kind === "error" ? [c.result.message] : [])));
  for (const m of [
    "Error: Target impedance must be greater than 0",
    "Error: Reference layer not found in stackup",
    "Error: Reference layer must be different to signal layer",
    "Error: Top reference layer not found in stackup",
    "Error: Top reference layer must be above signal layer in board stackup",
    "Error: Bottom reference layer not found in stackup",
    "Error: Bottom reference layer must be below signal layer in board stackup",
    "Error: Signal layer thickness must be greater than 0",
    "Error: Dielectric height must be greater than 0",
    "Error: Dielectric heights must be greater than 0",
    "Error: Width calculation failed",
    "Error: Diff pair gap must be greater than 0 to calculate width",
    "Error: Width must be greater than 0 to calculate diff pair gap",
    "Error: Width and diff pair gap must be greater than 0 to calculate delay",
  ])
    assert.ok(messages.has(m), m);
});

test("a dielectric averages its sublayers by thickness; a stripline's top and bottom heights are each side's", () => {
  const c = golden.panel.find((x) => x.name === "single In4 width, sublayers")!;
  const [bp, r] = boardParametersFor(inputOf(c));
  assert.ok(r.OK && bp);
  // In4.Cu: In3 above over 0.15 mm of 3.5, B.Cu below over 0.12 mm of 3.66 and 0.08 mm of 3.9
  assert.ok(Math.abs(bp.TopDielectricLayerThickness - 0.15e-3) < 1e-15);
  assert.ok(Math.abs(bp.BottomDielectricLayerThickness - 0.2e-3) < 1e-15);
  assert.ok(Math.abs(bp.DielectricConstant - (0.15 * 3.5 + 0.12 * 3.66 + 0.08 * 3.9) / 0.35) < 1e-12);
  assert.ok(Math.abs(bp.SignalLayerThickness - 17.5e-6) < 1e-15);
});

test("a microstrip's mask is the one over its side; an inner layer has none", () => {
  const top = boardParametersFor(inputOf(golden.panel.find((x) => x.name === "single F.Cu width, sublayers")!))[0]!;
  // `IUTomm( thickness ) / 1000.0`, in that order
  assert.deepEqual([top.SolderMaskThickness, top.SolderMaskDielectricConstant, top.SolderMaskLossTangent], [20000 / 1e6 / 1000, 3.8, 0.025]);
  const bottom = boardParametersFor(inputOf(golden.panel.find((x) => x.name === "single B.Cu delay, 70 um")!))[0]!;
  assert.deepEqual([bottom.SolderMaskThickness, bottom.SolderMaskDielectricConstant, bottom.SolderMaskLossTangent], [15000 / 1e6 / 1000, 3.3, 0.02]);
  const inner = boardParametersFor(inputOf(golden.panel.find((x) => x.name === "single In1 width")!))[0]!;
  assert.deepEqual([inner.SolderMaskThickness, inner.SolderMaskDielectricConstant, inner.SolderMaskLossTangent], [0, 0, 0]);
});

test("what the stackup leaves out reads as KiCad's parser leaves it: a dielectric of 4.5 / 0.02, a mask of 3.3, copper 35 um", () => {
  const c = golden.panel.find((x) => x.name === "single F.Cu width")!;
  const full = inputOf(c);
  const bare: Stackup = {
    layers: full.stackup.layers.map((l) => {
      const { epsilon_r: _e, loss_tangent: _t, ...rest } = l;
      return rest.kind === "copper" && (rest.name === "F.Cu" || rest.name === "B.Cu") ? { ...rest, thickness_mm: null } : rest;
    }),
  };
  // the fixture's 4-layer board is FR4 4.5 / 0.02 with 35 um outer copper; only the mask (3.3, 0 in both) and its 10 um match KiCad's defaults too
  assert.deepEqual(calculateTrackParametersForCell({ ...full, stackup: bare }), calculateTrackParametersForCell(full));
});

test("a new KiCad profile page's frequency (1 GHz) and solder mask model (on) are the defaults", () => {
  const c = golden.panel.find((x) => x.name === "single F.Cu width")!;
  const { frequencyHz: _f, modelSolderMask: _m, ...rest } = inputOf(c);
  assert.deepEqual(calculateTrackParametersForCell(rest), calculateTrackParametersForCell(inputOf(c)));
});

test("layer order and ids as layer_ids.h has them", () => {
  assert.deepEqual(["F.Cu", "B.Cu", "In1.Cu", "In30.Cu", "F.Mask", "B.Mask", "User.1"].map(pcbLayerId), [0, 2, 4, 62, 1, 3, -1]);
  assert.equal(isCopperLayerLowerThan(pcbLayerId("B.Cu"), pcbLayerId("In30.Cu")), true);
  assert.equal(isCopperLayerLowerThan(pcbLayerId("In1.Cu"), pcbLayerId("F.Cu")), true);
  assert.equal(isCopperLayerLowerThan(pcbLayerId("F.Cu"), pcbLayerId("In1.Cu")), false);
  assert.equal(isCopperLayerLowerThan(pcbLayerId("In2.Cu"), pcbLayerId("In2.Cu")), false);
});

test("a double is cast to int as x86-64 does: truncated, INT_MIN for NaN and out of range", () => {
  assert.deepEqual([toInt(1.9), toInt(-1.9), toInt(Number.NaN), toInt(3e9), toInt(-Infinity)], [1, -1, -2147483648, -2147483648, -2147483648]);
});
