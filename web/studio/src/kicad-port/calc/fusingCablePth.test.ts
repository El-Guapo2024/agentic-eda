// Fusing Current, Cable Size and Through-hole Size against KiCad 25ec2335. The PTH cases are KiCad's own (qa/tests/common/test_pth_hole_size.cpp);
// the fusing and cable values were computed by hand (Python, double precision, printed with C's "%f" / "%g") from the panels' formulas.
import { test } from "node:test";
import assert from "node:assert/strict";
import { FUSING_DEFAULTS, FUSING_LONG_TIME_COMMENT, calculateFusing, type FusingInputs } from "./fusingCurrent";
import { CABLE_SIZES, CableSizeModel } from "./cableSize";
import { defaultPcbCalculatorSettings } from "./calcSettings";
import { PTH_DEFAULTS, computeRecommendedPthHoleSize, findPthHoleSizeStandard, iuToMm, mmToIU, pthSummaryCells, recalculatePth } from "./pthSize";

const fusing = (patch: Partial<FusingInputs>): FusingInputs => ({ ...FUSING_DEFAULTS, widthScale: 1e-3, thicknessScale: 1e-3, target: "current", ...patch });

test("fusing current: the defaults give 11.219222 A, 0.012587 s, 0.089133 mm, 0.031196 mm", () => {
  assert.equal(FUSING_DEFAULTS.width, "0.100000");
  assert.deepEqual(calculateFusing(fusing({ target: "current" })), { value: "11.219222", comment: "" });
  assert.equal(calculateFusing(fusing({ target: "time" })).value, "0.012587");
  assert.equal(calculateFusing(fusing({ target: "width" })).value, "0.089133");
  assert.equal(calculateFusing(fusing({ target: "thickness" })).value, "0.031196");
});

test("fusing current: an invalid input is Error; a long time warns", () => {
  assert.equal(calculateFusing(fusing({ ambient: "2000" })).value, "Error");
  assert.equal(calculateFusing(fusing({ current: "-1" })).value, "Error");
  assert.equal(calculateFusing(fusing({ time: "x" })).value, "Error");
  // tau is 0.90 s for the default track: 2 * 0.5 s is past it.
  assert.equal(calculateFusing(fusing({ time: "0.5" })).comment, FUSING_LONG_TIME_COMMENT);
});

test("cable size: LoadSettings over the default (empty) settings ends at copper, 20 deg C, 1 mm", () => {
  const m = new CableSizeModel();
  m.loadSettings(defaultPcbCalculatorSettings().cable_size);
  assert.equal(m.texts.resistivity, "1.72e-08");
  assert.equal(m.texts.conductorTemp, "20");
  assert.equal(m.texts.thermCoef, "3.93e-3");
  assert.equal(m.texts.diameter, "1");
  assert.equal(m.texts.area, "0.785398");
  assert.equal(m.texts.linResistance, "0.0218997");
  assert.equal(m.texts.frequency, "1.74272e-05");
  assert.equal(m.texts.ampacity, "2.35619");
  assert.equal(m.texts.length, "100");
  assert.equal(m.texts.resistanceDc, "0.0218997");
  assert.equal(m.texts.vDrop, "21.8997");
  assert.equal(m.texts.power, "21.8997");
  assert.equal(m.resistivityTip, "Resistivity for 20 deg C is1.72e-08 Ohm*m");
  const saved = defaultPcbCalculatorSettings().cable_size;
  m.saveSettings(saved);
  assert.equal(saved.conductorMaterialResitivity, "1.72e-08");
  assert.equal(saved.currentDensityChoice, 3);
});

test("cable size: AWG10 and typing a diameter", () => {
  const m = new CableSizeModel();
  m.loadSettings(defaultPcbCalculatorSettings().cable_size);
  m.selectSize(CABLE_SIZES.findIndex((s) => s.name === "AWG10"));
  assert.equal(m.texts.diameter, "2.58826");
  assert.equal(m.texts.area, "5.26145");
  assert.equal(m.texts.linResistance, "0.00326906");
  assert.equal(m.texts.ampacity, "15.7844");
  m.setText("diameter", "2");
  assert.equal(m.texts.diameter, "2");
  assert.equal(m.sizeChoice, -1);
  assert.equal(m.texts.area, "3.14159");
  m.setCurrentDensity(6);
  assert.equal(m.texts.ampacity, "18.8496");
  // Area typed: the diameter follows, the area box is left as typed.
  m.setText("area", "1.5");
  assert.equal(m.texts.area, "1.5");
  assert.equal(m.texts.diameter, "1.38198");
});

test("PTH size: KiCad's QA cases (rectangular and square leads, level B)", () => {
  const std = findPthHoleSizeStandard("IPC-2222B")!;
  const rect = std.computeHoleSize(1, { shape: "rectangular", minX: mmToIU(0.46), maxX: mmToIU(0.61), minY: mmToIU(0.2), maxY: mmToIU(0.3) });
  assert.ok(Math.abs(iuToMm(rect.leadMin) - 0.5016) / 0.5016 < 1e-3);
  assert.ok(Math.abs(iuToMm(rect.holeMax) - 1.2016) / 1.2016 < 1e-3);
  assert.equal(iuToMm(computeRecommendedPthHoleSize(rect, mmToIU(0.1), "nearest")), 1.0);
  const sq = std.computeHoleSize(1, { shape: "square", minX: mmToIU(0.6), maxX: mmToIU(0.65), minY: 0, maxY: 0 });
  assert.ok(Math.abs(iuToMm(sq.holeMin) - 1.1192) / 1.1192 < 1e-3);
  assert.equal(iuToMm(computeRecommendedPthHoleSize(sq, mmToIU(0.05), "nearest")), 1.35);
});

test("PTH size: the panel's defaults give a 1.1 mm hole and a 1.9 mm pad", () => {
  const r = recalculatePth(PTH_DEFAULTS)!;
  assert.equal(r.hole, 1100000);
  assert.equal(r.pad, 1900000);
  assert.equal(
    r.report,
    [
      "Standard:                      IPC-2222B",
      "Production level:              B (Moderate)",
      "Lead shape:                    Round",
      "Lead size:                     0.6 – 0.7 mm",
      "Effective lead size:           0.6 – 0.7 mm",
      "",
      "Minimum hole size:             0.9 mm",
      "Maximum hole size:             1.3 mm",
      "Recommended hole size:         1.1 mm",
      "Hole clearance over max. lead: 0.4 mm",
      "Annular ring:                  0.4 mm",
      "Pad diameter:                  1.9 mm",
      "",
    ].join("\n")
  );
  assert.equal(recalculatePth({ ...PTH_DEFAULTS, sizeXMin: "0.8" }), null);
  assert.equal(recalculatePth({ ...PTH_DEFAULTS, rounding: "abc" }), null);
  assert.deepEqual(pthSummaryCells(findPthHoleSizeStandard("IPC-2222B")!)[2], { level: "C (High density)", min: "Maximum lead diameter + 0.15mm", max: "Minimum lead diameter + 0.6mm" });
});
