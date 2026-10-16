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
    ["Do not fill", "Fill with body outline color", "Fill with body background color"]
  );
});

test("the dialog opens on the shape: its border and width, its fill, and whether it is common to all units and body styles", () => {
  const sym = { unit_count: 2, has_alternate_body_style: true };
  assert.deepEqual(shapeFormOf({ stroke_mm: 0.254, fill: "background", unit: 0, body_style: 0 }, sym), { border: true, widthMm: 0.254, fill: "background", commonUnits: true, commonBodyStyles: true });
  assert.deepEqual(shapeFormOf({ stroke_mm: 0.254, fill: "none", unit: 1, body_style: 1 }, sym), { border: true, widthMm: 0.254, fill: "none", commonUnits: false, commonBodyStyles: false });
  // no border: the box is off and the width reads as the default one if it is checked again
  assert.deepEqual(shapeFormOf({ stroke_mm: NO_BORDER_MM, fill: "outline", unit: 1, body_style: 1 }, sym), { border: false, widthMm: 0, fill: "outline", commonUnits: false, commonBodyStyles: false });
  // a symbol of one unit and one body style is never common to them (the boxes are disabled)
  assert.deepEqual(shapeFormOf({ stroke_mm: 0, fill: "none", unit: 0, body_style: 0 }, { unit_count: 1, has_alternate_body_style: false }), { border: true, widthMm: 0, fill: "none", commonUnits: false, commonBodyStyles: false });
});

test("accepting the dialog: the width, the fill, and the unit and body style the editor shows unless the box is common", () => {
  const form = { border: true, widthMm: 0.5, fill: "outline" as const, commonUnits: false, commonBodyStyles: false };
  assert.deepEqual(shapeEditOf(form, { unit: 2, bodyStyle: 2 }), { strokeMm: 0.5, fill: "outline", unit: 2, bodyStyle: 2 });
  assert.deepEqual(shapeEditOf({ ...form, commonUnits: true, commonBodyStyles: true }, { unit: 2, bodyStyle: 2 }), { strokeMm: 0.5, fill: "outline", unit: 0, bodyStyle: 0 });
  // the border off: the width KiCad keeps for none
  assert.equal(shapeEditOf({ ...form, border: false }, { unit: 1, bodyStyle: 1 }).strokeMm, -0.0001);
  // a negative or missing width with the border on is the default one, 0
  assert.equal(strokeOf({ border: true, widthMm: -3 }), 0);
  assert.equal(strokeOf({ border: true, widthMm: NaN }), 0);
});
