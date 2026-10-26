import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PIN_TEMPLATE, newPinFromClick, pinTemplateOf, samePinTemplate } from "./symPinTemplate";
import type { LibrarySymbolPin } from "../api/types";

const template = { ...DEFAULT_PIN_TEMPLATE };

test("newPinFromClick: the sticky defaults and the click's place, in mm and +y up", () => {
  const pin = newPinFromClick(template, 2, 1, "5", { x: -5.08, y: 2.54 });
  assert.deepEqual(pin, { ...template, name: "", number: "5", at: { x: -5.08, y: 2.54 }, unit: 2, body_style: 1 });
  assert.equal(pin.name, "", "g_LastPin* holds no name: a new pin starts without one");
  assert.equal(pin.length_mm, 2.54, "the default 100 mil pin");
});

test("newPinFromClick: a pin made common to all units (or styles) keeps the next one common too -- g_LastPinCommonUnit", () => {
  const pin = newPinFromClick({ ...template, unit: 0, body_style: 0 }, 2, 2, "1", { x: 0, y: 0 });
  assert.equal(pin.unit, 0);
  assert.equal(pin.body_style, 0);
  const other = newPinFromClick({ ...template, unit: 1, body_style: 1 }, 2, 2, "2", { x: 0, y: 0 });
  assert.equal(other.unit, 2, "an ordinary pin takes the unit being edited");
  assert.equal(other.body_style, 2);
});

test("pinTemplateOf: what the next pin carries, and what it does not", () => {
  const pin: LibrarySymbolPin = {
    id: "pin-1",
    number: "7",
    name: "VIN",
    electrical_type: "power_in",
    shape: "inverted",
    at: { x: 3.1, y: 0.7 },
    angle_deg: 90,
    length_mm: 3.81,
    unit: 0,
    body_style: 2,
    hidden: true,
    name_size_mm: 1.5,
    number_size_mm: null,
    alternates: [{ name: "UART", electrical_type: "output", shape: "line" }],
  };
  assert.deepEqual(pinTemplateOf(pin), {
    electrical_type: "power_in",
    shape: "inverted",
    angle_deg: 90,
    length_mm: 3.81,
    hidden: true,
    name_size_mm: 1.5,
    number_size_mm: null,
    unit: 0,
    body_style: 2,
  });
  assert.ok(!("alternates" in pinTemplateOf(pin)), "the alternate functions are the pin's own, not a default for the next");
  assert.ok(!("name" in pinTemplateOf(pin)), "the name is the pin's own too: KiCad's g_LastPin* has no name");
});

test("samePinTemplate: what the poll may re-set without a re-render", () => {
  assert.equal(samePinTemplate(template, { ...template }), true);
  assert.equal(samePinTemplate(template, { ...template, length_mm: 3.81 }), false);
  assert.equal(samePinTemplate(template, { ...template, unit: 0 }), false);
});
