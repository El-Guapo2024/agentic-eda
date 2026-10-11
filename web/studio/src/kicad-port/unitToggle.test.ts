import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_IMPERIAL_UNIT, rememberedImperial, toggledUnits, type ImperialUnit, type ToggledUnit } from "./unitToggle";

test("Ctrl+U goes from millimetres to inches until mils were chosen", () => {
  assert.equal(DEFAULT_IMPERIAL_UNIT, "in");
  assert.equal(toggledUnits("mm", DEFAULT_IMPERIAL_UNIT), "in");
  assert.equal(toggledUnits("in", "in"), "mm");
});

test("after mils were chosen, Ctrl+U returns to mils, not to inches", () => {
  let units: ToggledUnit = "mm";
  let last: ImperialUnit = DEFAULT_IMPERIAL_UNIT;
  const pick = (u: ToggledUnit) => {
    last = rememberedImperial(last, u);
    units = u;
  };
  pick("mil");
  assert.equal(units, "mil");
  units = toggledUnits(units, last);
  assert.equal(units, "mm", "from mils to millimetres");
  units = toggledUnits(units, last);
  assert.equal(units, "mil", "and back to mils");
  pick("mm");
  assert.equal(last, "mil", "picking millimetres does not forget the imperial unit");
  pick("in");
  units = toggledUnits(toggledUnits(units, last), last);
  assert.equal(units, "in", "inches chosen again: Ctrl+U goes back and forth to inches");
});
