import { test } from "node:test";
import assert from "node:assert/strict";
import { BORDER_HELP, FILL_CHOICES, NO_BORDER_MM, shapeEditOf, shapeFormOf, shapeTitle, strokeOf } from "./symShapeProps";

test("the dialog is titled by what the shape is, a polyline being KiCad's polygon", () => {
  assert.deepEqual(
    (["rectangle", "circle", "arc", "polyline"] as const).map((kind) => shapeTitle({ kind })),
    ["Rectangle Properties", "Circle Properties", "Arc Properties", "Polygon Properties"]
  );
  assert.equal(BORDER_HELP, "Set border width to 0 to use schematic's default line width.");
  assert.deepEqual(
    FILL_CHOICES.map((f) => f.label),
    ["Do not fill", "Fill with body outline color", "Fill with body background color", "Fill with:"]
  );
});

const PLAIN = { borderColor: null, style: "solid", fillColor: null, private: false };

test("the dialog opens on the shape: its border and width, its fill, and whether it is common to all units and body styles", () => {
  const sym = { unit_count: 2, has_alternate_body_style: true };
  assert.deepEqual(shapeFormOf({ stroke_mm: 0.254, fill: "background", unit: 0, body_style: 0 }, sym), { border: true, widthMm: 0.254, fill: "background", commonUnits: true, commonBodyStyles: true, ...PLAIN });
  assert.deepEqual(shapeFormOf({ stroke_mm: 0.254, fill: "none", unit: 1, body_style: 1 }, sym), { border: true, widthMm: 0.254, fill: "none", commonUnits: false, commonBodyStyles: false, ...PLAIN });
  // no border: the box is off and the width reads as the default one if it is checked again
  assert.deepEqual(shapeFormOf({ stroke_mm: NO_BORDER_MM, fill: "outline", unit: 1, body_style: 1 }, sym), { border: false, widthMm: 0, fill: "outline", commonUnits: false, commonBodyStyles: false, ...PLAIN });
  // a symbol of one unit and one body style is never common to them (the boxes are disabled)
  assert.deepEqual(shapeFormOf({ stroke_mm: 0, fill: "none", unit: 0, body_style: 0 }, { unit_count: 1, has_alternate_body_style: false }), { border: true, widthMm: 0, fill: "none", commonUnits: false, commonBodyStyles: false, ...PLAIN });
});

test("accepting the dialog: the width, the fill, and the unit and body style the editor shows unless the box is common", () => {
  const form = { border: true, widthMm: 0.5, fill: "outline" as const, commonUnits: false, commonBodyStyles: false, ...PLAIN };
  const pick = (e: ReturnType<typeof shapeEditOf>) => ({ strokeMm: e.strokeMm, fill: e.fill, unit: e.unit, bodyStyle: e.bodyStyle });
  assert.deepEqual(pick(shapeEditOf(form, { unit: 2, bodyStyle: 2 })), { strokeMm: 0.5, fill: "outline", unit: 2, bodyStyle: 2 });
  assert.deepEqual(pick(shapeEditOf({ ...form, commonUnits: true, commonBodyStyles: true }, { unit: 2, bodyStyle: 2 })), { strokeMm: 0.5, fill: "outline", unit: 0, bodyStyle: 0 });
  // the border off: the width KiCad keeps for none
  assert.equal(shapeEditOf({ ...form, border: false }, { unit: 1, bodyStyle: 1 }).strokeMm, -0.0001);
  // a negative or missing width with the border on is the default one, 0
  assert.equal(strokeOf({ border: true, widthMm: -3 }), 0);
  assert.equal(strokeOf({ border: true, widthMm: NaN }), 0);
});

test("UI-LEFT-2 YY2: the border's colour and style, a colour fill of its own and Private open from what the shape keeps beside it and go back on OK", () => {
  const sym = { unit_count: 1, has_alternate_body_style: false };
  const red = { r: 255, g: 0, b: 0, a: 255 };
  const blue = { r: 0, g: 0, b: 255, a: 255 };
  const state = { looks: { private: true, stroke_color: red, fill_color: blue, hyperlink: "x" }, lineStyle: "dash", thicknessMm: null };
  const form = shapeFormOf({ stroke_mm: 0.2, fill: "color", unit: 1, body_style: 1 }, sym, state);
  assert.deepEqual([form.fill, form.borderColor, form.style, form.fillColor, form.private], ["color", red, "dash", blue, true]);
  // a hatch is none of the dialog's fills; OK takes the hatch away
  assert.equal(shapeFormOf({ stroke_mm: 0.2, fill: "color", unit: 1, body_style: 1 }, sym, { ...state, looks: { ...state.looks, hatch: "hatch" } }).fill, "none");
  const e = shapeEditOf({ ...form, style: "dot", private: false, borderColor: null }, { unit: 1, bodyStyle: 1 }, state);
  assert.deepEqual(e.state, { looks: { private: false, stroke_color: null, fill_color: blue, hyperlink: "x", hatch: null }, lineStyle: "dot", thicknessMm: null });
  // the default style opens as Solid
  assert.equal(shapeFormOf({ stroke_mm: 0.2, fill: "none", unit: 1, body_style: 1 }, sym).style, "solid");
});
