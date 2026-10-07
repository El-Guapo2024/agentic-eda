import { test } from "node:test";
import assert from "node:assert/strict";
import { ELECTRICAL_TYPE_NAMES, electricalTypeLayout, pinLabelsShown, pinShown, pinTextOffsetUm, TARGET_PIN_RADIUS_UM } from "./symPinText";

const near = (a: number, b: number) => assert.ok(Math.abs(a - b) < 1e-6, `${a} != ${b}`);
const pin = { electrical_type: "passive" as const, name_size_mm: 1.27 };

test("the text offset is KiROUND( 24 * 0.15 ) = 4 mil", () => {
  near(pinTextOffsetUm(), 101.6);
});

test("the electrical type text sits beyond the connection point of a pin whose body points right", () => {
  // body points right => the way it sticks out (body -> connection point) is -x
  const l = electricalTypeLayout(pin, [0, 0], [-1, 0]);
  assert.equal(l.text, "Passive");
  near(l.sizeUm, 952.5); // 1.27 mm * 3/4
  near(l.thicknessUm, 952.5 / 8);
  near(l.at[0], -(101.6 + 952.5 / 16 + TARGET_PIN_RADIUS_UM + TARGET_PIN_RADIUS_UM / 2));
  assert.equal(l.at[1], 0);
  assert.equal(l.vertical, false);
  assert.equal(l.justify, "right");
});

test("a pin whose body points left puts the text on the right, left-aligned", () => {
  const l = electricalTypeLayout(pin, [1000, 500], [1, 0]);
  assert.ok(l.at[0] > 1000);
  assert.equal(l.at[1], 500);
  assert.equal(l.justify, "left");
  assert.equal(l.vertical, false);
});

test("vertical pins get vertical text, mirrored alignment between up and down", () => {
  const up = electricalTypeLayout(pin, [0, 0], [0, 1]); // body points up, text below
  assert.equal(up.vertical, true);
  assert.ok(up.at[1] > 0);
  assert.equal(up.justify, "right");
  const down = electricalTypeLayout(pin, [0, 0], [0, -1]);
  assert.equal(down.vertical, true);
  assert.ok(down.at[1] < 0);
  assert.equal(down.justify, "left");
});

test("the text is never smaller than 0.7 mm", () => {
  near(electricalTypeLayout({ electrical_type: "input", name_size_mm: 0.5 }, [0, 0], [-1, 0]).sizeUm, 700);
});

test("a pin that is connected loses the extra half radius", () => {
  const dangling = electricalTypeLayout(pin, [0, 0], [-1, 0], true);
  const connected = electricalTypeLayout(pin, [0, 0], [-1, 0], false);
  near(dangling.at[0] - connected.at[0], -TARGET_PIN_RADIUS_UM / 2);
});

test("every electrical type has KiCad's name", () => {
  assert.equal(Object.keys(ELECTRICAL_TYPE_NAMES).length, 12);
  assert.equal(ELECTRICAL_TYPE_NAMES.tri_state, "Tri-state");
  assert.equal(ELECTRICAL_TYPE_NAMES.no_connect, "Unconnected");
});

test("a hidden pin is drawn and picked only while hidden pins are shown", () => {
  assert.equal(pinShown(false, false), true);
  assert.equal(pinShown(true, false), false);
  assert.equal(pinShown(true, true), true);
});

test("pin labels follow the symbol, and Show Pin Numbers forces the numbers on", () => {
  assert.deepEqual(pinLabelsShown({ pin_names_hidden: false, pin_numbers_hidden: false }, false), { names: true, numbers: true });
  assert.deepEqual(pinLabelsShown({ pin_names_hidden: true, pin_numbers_hidden: true }, false), { names: false, numbers: false });
  assert.deepEqual(pinLabelsShown({ pin_names_hidden: true, pin_numbers_hidden: true }, true), { names: false, numbers: true });
});
