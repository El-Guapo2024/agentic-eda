// The RF Attenuators, Wavelength, Via Size and Track Width calculators against KiCad 25ec2335: the expected values are the output of
// tools/calc-golden/rf_power_golden.cpp, which compiles KiCad's common/track_width_calculations.cpp and transcribes the wx-bound panels' arithmetic
// (attenuator_classes.cpp, panel_via_size.cpp, panel_wavelength.cpp). Each value is checked to 1e-12 relative and in the panels' "%g".
import { test } from "node:test";
import assert from "node:assert/strict";
import { AttenuatorBridge, AttenuatorPi, AttenuatorSplitter, AttenuatorTee } from "./attenuators";
import { calculateTrackWidth, currentFromWidth, widthFromCurrent } from "./trackWidth";
import { calculateVia } from "./viaSize";
import { wavelengthValues, frequencyFromWavelengthVacuum, frequencyFromPeriod } from "./wavelength";
import { printfF, printfG } from "./cformat";
import { findMatch, standardRelativeDielectricConstantList } from "./commonData";
import { UNIT_MICRON, UNIT_MIL, UNIT_MM, UNIT_OZSQFT } from "./unitSelectors";

function near(actual: number, expected: number, g?: string): void {
  assert.ok(Math.abs(actual - expected) <= 1e-12 * Math.abs(expected), `${actual} vs ${expected}`);
  if (g !== undefined) assert.equal(printfG(actual), g);
}

test("printf %g / %f as glibc writes them", () => {
  assert.equal(printfG(1e9), "1e+09");
  assert.equal(printfG(141323520.0002555), "1.41324e+08");
  assert.equal(printfG(0.000575362106), "0.000575362");
  assert.equal(printfG(0.0000575362106), "5.75362e-05");
  assert.equal(printfG(37.351877), "37.3519");
  assert.equal(printfG(100000), "100000");
  assert.equal(printfG(1000000), "1e+06");
  assert.equal(printfG(0), "0");
  assert.equal(printfG(1.72e-8), "1.72e-08");
  assert.equal(printfG(Number.NaN), "nan");
  assert.equal(printfF(13.415225844392239), "13.415226");
});

test("track width: the panel's defaults, from the current", () => {
  const r = calculateTrackWidth("current", { current: 1, extWidth: 0, intWidth: 0, extThickness: 35 * UNIT_MICRON, intThickness: 35 * UNIT_MICRON, deltaT_C: 10, rho: 1.72e-8, trackLen: 20 * UNIT_MM });
  near(r.extWidth / UNIT_MM, 0.2772867450210223, "0.277287");
  near(r.intWidth / UNIT_MM, 0.24842296021443036, "0.248423");
  near((r.extWidth * 35 * UNIT_MICRON) / (UNIT_MM * UNIT_MM), 0.0097050360757357807, "0.00970504");
  near(r.extResistance, 0.03544551481473189, "0.0354455");
  near(r.intResistance, 0.039563860844777539, "0.0395639");
  near(r.extV, 0.03544551481473189);
  near(r.extLoss, 0.03544551481473189);
});

test("track width: from an external width; internal copper weights", () => {
  const r = calculateTrackWidth("ext_width", { current: 0, extWidth: 0.5 * UNIT_MM, intWidth: 0, extThickness: 35 * UNIT_MICRON, intThickness: 35 * UNIT_MICRON, deltaT_C: 10, rho: 1.72e-8, trackLen: 20 * UNIT_MM });
  near(r.current, 1.4035358637473274, "1.40354");
  near(r.intWidth / UNIT_MM, 0.44615616017436205, "0.446156");
  near(widthFromCurrent(3, (2 * UNIT_OZSQFT) / UNIT_MIL, 20, true), 19.075472185107039);
  near(widthFromCurrent(3, (3 * UNIT_OZSQFT) / UNIT_MIL, 20, true), 9.032583909852983);
  near(currentFromWidth(10, 0.6, 10, true), 0.69471124275027341);
});

test("attenuators: Pi, Tee, Bridged tee, splitter, and the minimum attenuation", () => {
  const pi = new AttenuatorPi();
  assert.ok(pi.calculate());
  near(pi.r1, 150.47602375372435, "150.476");
  near(pi.r2, 37.351877033540184, "37.3519");
  near(pi.r3, 150.47602375372435);
  Object.assign(pi, { zin: 75, zout: 50, attenuation: 10 });
  assert.ok(pi.calculate());
  near(pi.r1, 207.4348773340985, "207.435");
  near(pi.r2, 87.142125289666879, "87.1421");
  near(pi.r3, 77.107314569517129, "77.1073");
  near(pi.minimumAtt, 5.7194754753335939);
  const tee = new AttenuatorTee();
  assert.ok(tee.calculate());
  near(tee.r1, 16.613942458312223, "16.6139");
  near(tee.r2, 66.931040647706155, "66.931");
  const br = new AttenuatorBridge();
  br.zin = 1234;
  assert.ok(br.calculate());
  assert.equal(br.zin, 50);
  near(br.r1, 49.763115748443973, "49.7631");
  near(br.r2, 50.238011876862267, "50.238");
  const sp = new AttenuatorSplitter();
  sp.zout = 75;
  sp.attenuation = 1;
  assert.ok(sp.calculate());
  assert.deepEqual([sp.attenuation, sp.zin, sp.r1, sp.r2, sp.r3], [6, 75, 25, 25, 25]);
  const bad = new AttenuatorPi();
  Object.assign(bad, { zin: 300, zout: 50, attenuation: 3 });
  assert.equal(bad.calculate(), false);
  assert.ok(bad.error);
  assert.equal(printfF(bad.minimumAtt), "13.415226");
});

test("via size: the panel's defaults", () => {
  const r = calculateVia({
    finishedHoleDia: 0.4 * UNIT_MM,
    platingThickness: 0.035 * UNIT_MM,
    viaLength: 1.6 * UNIT_MM,
    padDia: 0.6 * UNIT_MM,
    clearanceDia: 1.0 * UNIT_MM,
    charImpedance: 50,
    appliedCurrent: 1,
    platingResistivity: 1.72e-8,
    relativePermitivity: 4.5,
    temperatureDiff: 10,
    pulseRiseTime: 1,
  });
  near(r.viaResistance, 0.00057536210625799128, "0.000575362");
  near(r.voltageDrop, 0.00057536210625799128);
  near(r.powerLoss, 0.00057536210625799128);
  near(r.estimatedAmpacity, 2.9993009800491466, "2.9993");
  near(r.thermalResistance, 83.293700266418512, "83.2937");
  near(r.capacitance, 0.59950799999999982, "0.599508");
  near(r.timeDegradation, 32.972939999999994, "32.9729");
  near(r.inductance, 1.2072283911167301, "1.20723");
  near(r.reactance, 3.7926198447373447, "3.79262");
});

test("wavelength: 1 GHz in FR4", () => {
  const w = wavelengthValues(1e9, 4.5, 1);
  near(w.period / 1e-9, 1, "1");
  near(w.wavelengthVacuum / 1e-2, 29.979245799999998, "29.9792");
  near(w.wavelengthMedium / 1e-2, 14.13235200002555, "14.1324");
  near(w.speed, 141323520.0002555, "1.41324e+08");
  near(w.speed / (1 / 3.6), 508764672.00091976, "5.08765e+08");
  near(frequencyFromWavelengthVacuum(0.299792458), 1e9);
  near(frequencyFromPeriod(1e-9), 1e9);
});

test("findMatch: string, then number, else the stop index", () => {
  const list = [...standardRelativeDielectricConstantList(), ""];
  assert.deepEqual(findMatch(list, "4.5"), [true, 0]);
  assert.deepEqual(findMatch(list, "4,04"), [true, 2]);
  assert.deepEqual(findMatch(list, "4.50"), [true, 0]);
  assert.deepEqual(findMatch(list, "4"), [true, 11]);
  assert.deepEqual(findMatch(list, "7"), [false, 17]);
});
