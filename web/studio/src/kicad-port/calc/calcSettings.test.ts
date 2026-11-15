import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultPcbCalculatorSettings, sanitizePcbCalculatorSettings } from "./calcSettings";
import { UNIT_SELECTOR_THICKNESS, doubleFromString, toDouble, unitScale } from "./unitSelectors";

test("the defaults are PCB_CALCULATOR_SETTINGS' PARAM defaults", () => {
  const s = defaultPcbCalculatorSettings();
  assert.equal(s.last_page, 1);
  assert.equal(s.regulators.type, 1);
  assert.equal(s.regulators.r1, "0.240");
  assert.equal(s.track_width.ext_track_thickness_units, 1);
  assert.equal(s.electrical.iec60664_pcbMaterial, 1);
  assert.equal(s.attenuators.att_bridge.attenuation, 6);
  assert.equal(s.corrosion_table.show_symbols, true);
  assert.deepEqual(s.trans_line.TwistedPair, { values: {}, units: {} });
});

test("a stored object is read onto the defaults: wrong types and unknown keys are dropped", () => {
  const s = sanitizePcbCalculatorSettings({
    last_page: 14,
    board_class_units: "x",
    bogus: 1,
    regulators: { r1: "1.0", type: "2" },
    attenuators: { att_tee: { zin: 75 } },
    trans_line: { Coax: { values: { "3": 1.5, "4": "no" }, units: { "3": 2, "4": 1.5 } } },
  });
  assert.equal(s.last_page, 14);
  assert.equal(s.board_class_units, 0);
  assert.equal((s as unknown as Record<string, unknown>).bogus, undefined);
  assert.equal(s.regulators.r1, "1.0");
  assert.equal(s.regulators.type, 1);
  assert.equal(s.attenuators.att_tee.zin, 75);
  assert.equal(s.attenuators.att_tee.zout, 50);
  assert.deepEqual(s.trans_line.Coax, { values: { "3": 1.5 }, units: { "3": 2 } });
  assert.deepEqual(sanitizePcbCalculatorSettings(null), defaultPcbCalculatorSettings());
});

test("DoubleFromString: comma or point, units after the number ignored, NaN for no number", () => {
  assert.equal(doubleFromString("4.7 K"), 4.7);
  assert.equal(doubleFromString(" 2,5 "), 2.5);
  assert.equal(doubleFromString("1e3"), 1000);
  assert.ok(Number.isNaN(doubleFromString("abc")));
  assert.ok(Number.isNaN(doubleFromString("1e")));
  assert.equal(toDouble("12x"), null);
  assert.equal(toDouble(".5"), 0.5);
});

test("UNIT_SELECTOR_THICKNESS: oz/ft² is 34.4 µm; out of the list is 1", () => {
  assert.equal(unitScale(UNIT_SELECTOR_THICKNESS, 5), 34.4 * 1e-6);
  assert.equal(unitScale(UNIT_SELECTOR_THICKNESS, 9), 1);
});
