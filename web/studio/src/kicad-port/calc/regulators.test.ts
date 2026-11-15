// kicad-port/calc/regulators.ts: expected values hand-computed from panel_regulator.cpp's formulas (KiCad 25ec2335) for its LM317 defaults, and the data
// file as datafile_read_write.cpp writes and reads it.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  dataFilename,
  formatDataFile,
  parseDataFile,
  regulatorFormValid,
  regulatorFromForm,
  regulatorListAdd,
  regulatorListGet,
  regulatorListRemove,
  regulatorListReplace,
  regulatorToForm,
  regulatorsSolve,
  roundTo,
  type RegulatorData,
  type RegulatorInputs,
} from "./regulators";

const LM317: RegulatorInputs = { type: 1, solveFor: 2, resTol: "1", r1Typ: "0.240", r2Typ: "0.720", vrefMin: "1.20", vrefTyp: "1.25", vrefMax: "1.30", voutTyp: "5", iadjTyp: "50", iadjMax: "100" };

test("3 terminal, Vout: 1.25 * (240 + 720) / 240 + 720 * 50 µA = 5.036 V; min / max over 1 % resistors and Vref / Iadj ranges", () => {
  const s = regulatorsSolve(LM317);
  assert.equal(s.kind, "ok");
  if (s.kind !== "ok") return;
  assert.equal(s.voutTyp, "5.036");
  // 1.2 * (242.4 + 712.8) / 242.4 + 712.8 * 50e-6 = 4.76435; 1.3 * (237.6 + 727.2) / 237.6 + 727.2 * 100e-6 = 5.35151
  assert.equal(s.voutMin, "4.764");
  assert.equal(s.voutMax, "5.352");
  assert.deepEqual([s.r1Min, s.r1Typ, s.r1Max, s.r2Min, s.r2Typ, s.r2Max], ["0.238", "0.24", "0.242", "0.713", "0.72", "0.727"]);
  // (4.76435 - 5.036) / 5.036 = -5.39 %; (5.35151 - 5.036) / 5.35151 = +5.90 %
  assert.equal(s.tolTotalMin, "-5.39");
  assert.equal(s.tolTotalMax, "5.9");
  assert.equal(s.powerComment, "5.04V [4.76V ... 5.35V]");
});

test("standard type: R2 for 3.3 V from Vref 1.25 V and R1 10 k is 10 / (3.3 / 1.25 - 1) = 6.098 k", () => {
  const s = regulatorsSolve({ ...LM317, type: 0, solveFor: 1, r1Typ: "10", voutTyp: "3.3", vrefMin: "1.25", vrefMax: "1.25", resTol: "0" });
  assert.equal(s.kind, "ok");
  if (s.kind === "ok") {
    assert.equal(s.r2Typ, "6.098");
    assert.equal(s.voutMin, "3.3");
  }
});

test("RegulatorsSolve's messages, word for word", () => {
  assert.deepEqual(regulatorsSolve({ ...LM317, solveFor: 0, voutTyp: "1" }), { kind: "message", message: "Vout must be greater than Vref" });
  assert.deepEqual(regulatorsSolve({ ...LM317, vrefMin: "0" }), { kind: "message", message: "Vref set to 0 !" });
  assert.deepEqual(regulatorsSolve({ ...LM317, vrefMin: "1.3", vrefMax: "1.2" }), { kind: "message", message: "Vref must VrefMin < VrefTyp < VrefMax" });
  assert.deepEqual(regulatorsSolve({ ...LM317, r2Typ: "0" }), { kind: "message", message: "Incorrect value for R1 R2" });
  assert.deepEqual(regulatorsSolve({ ...LM317, iadjTyp: "200" }), { kind: "message", message: "Iadj must IadjTyp < IadjMax" });
  assert.deepEqual(regulatorsSolve({ ...LM317, solveFor: 5 }), { kind: "message", message: "Selection error" });
});

test("round_to rounds half away from zero", () => {
  assert.equal(roundTo(-0.0025, 0.001), -0.003);
  assert.equal(roundTo(1.23456), 1.235);
});

const reg = (name: string, type = 0): RegulatorData => ({ name, type, vrefMin: 1.2, vrefTyp: 1.25, vrefMax: 1.3, iadjTyp: type ? 50 : 0, iadjMax: type ? 100 : 0 });

test("the list keeps natural order and finds names ignoring case", () => {
  let l: RegulatorData[] = [];
  for (const n of ["LM1117", "lm317", "LM2596", "AMS1117"]) l = regulatorListAdd(l, reg(n));
  assert.deepEqual(l.map((r) => r.name), ["AMS1117", "lm317", "LM1117", "LM2596"]);
  assert.equal(regulatorListGet(l, "LM317")?.name, "lm317");
  l = regulatorListReplace(l, { ...reg("LM317"), vrefTyp: 2 });
  assert.equal(l[1]!.vrefTyp, 2);
  assert.deepEqual(regulatorListRemove(l, "ams1117").map((r) => r.name), ["LM317", "LM1117", "LM2596"]);
});

test("the data file is written as WriteDataFile writes it and read back", () => {
  const list = [reg("LM317", 1), reg('A "q"')];
  const text = formatDataFile(list, "2026-11-15T10:00:00", "pcb_calculator EDA Studio");
  assert.equal(
    text,
    [
      "(datafile",
      "  (version 2)",
      '    (date "2026-11-15T10:00:00")',
      '      (tool "pcb_calculator EDA Studio")',
      "        (regulators",
      '          (regulator "LM317"',
      "            (reg_vref_min 1.2)",
      "            (reg_vref_typ 1.25)",
      "            (reg_vref_max 1.3)",
      "            (reg_iadj_typ 50)",
      "            (reg_iadj_max 100)",
      "            (reg_type 3terminal)",
      "          )",
      '          (regulator "A \\"q\\""',
      "            (reg_vref_min 1.2)",
      "            (reg_vref_typ 1.25)",
      "            (reg_vref_max 1.3)",
      "            (reg_type normal)",
      "          )",
      "        )",
      "      )",
      "    )",
      "  )",
      ")",
      "",
    ].join("\n")
  );
  assert.deepEqual(parseDataFile(text), [reg('A "q"'), reg("LM317", 1)]);
});

test("legacy reg_vref / reg_iadj entries and errors", () => {
  const l = parseDataFile("(datafile (regulators (regulator LM7805 (reg_vref 5) (reg_type normal)) (regulator X (reg_iadj 2) (reg_type 3TERMINAL))))");
  assert.deepEqual(l[0], { name: "LM7805", type: 0, vrefMin: 5, vrefTyp: 5, vrefMax: 5, iadjTyp: 0, iadjMax: 0 });
  assert.equal(l[1]!.type, 1);
  assert.equal(l[1]!.iadjMax, 2);
  const err = (text: string) => {
    try {
      parseDataFile(text);
      return "";
    } catch (e) {
      return (e as Error).message;
    }
  };
  assert.match(err("(regulators (regulator A (reg_vref x)))"), /^Expecting number/);
  assert.match(err("(regulators (regulator A (reg_foo 1)))"), /^Unexpected 'reg_foo'/);
});

test("data file names get the .pcbcalc extension", () => {
  assert.equal(dataFilename("regs.json"), "regs.pcbcalc");
  assert.equal(dataFilename("regs"), "regs.pcbcalc");
  assert.equal(dataFilename(""), "");
});

test("the regulator dialog: %.3g values, TransferDataFromWindow's checks, Iadj zero for a standard regulator", () => {
  const f = regulatorToForm({ ...reg("LM317", 1), vrefTyp: 1.2345 });
  assert.equal(f.vrefTyp, "1.23");
  assert.equal(regulatorFormValid(f), true);
  assert.equal(regulatorFormValid({ ...f, name: "" }), false);
  assert.equal(regulatorFormValid({ ...f, vrefMin: "0.05" }), false);
  assert.equal(regulatorFormValid({ ...f, iadjTyp: "5.5" }), false);
  assert.equal(regulatorFormValid({ ...f, type: 0, iadjTyp: "" }), true);
  assert.deepEqual(regulatorFromForm({ ...f, type: 0 }).iadjMax, 0);
});
