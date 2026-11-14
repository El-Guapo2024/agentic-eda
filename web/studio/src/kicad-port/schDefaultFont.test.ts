import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveFont, setFontResolver, STROKE, withDefaultFace, type KicadFont, type OutlineFace } from "./kicadText";
import { resolveSchFont, schPlainMeasure, setSchDefaultFont } from "./schDefaultFont";

const face: OutlineFace = { family: "t", unitsPerEm: 1000, ascender: 800, descender: -200, height: 1200, fakeBold: false, fakeItalic: false, advance: (t) => Array.from(t).length, ink: () => null };
const outline: KicadFont = { kind: "outline", face };

test("a text with no face of its own takes the default font in force (`GetDrawFont`: `GetDefaultFont( aSettings )`, UI-LEFT-3 S3-29)", () => {
  setFontResolver((f) => (f === "DejaVu Sans" ? outline : STROKE));
  try {
    assert.equal(resolveFont(null), STROKE, "outside a schematic's painting: the stroke font (the board's, the Symbol Editor's)");
    withDefaultFace("DejaVu Sans", () => {
      assert.equal(resolveFont(null), outline, "no face: the default font");
      assert.equal(resolveFont(""), outline);
      assert.equal(resolveFont("Default Font"), outline, "\"Default Font\" is no face of its own");
      assert.equal(resolveFont("KiCad Font"), STROKE, "a text set in the KiCad font keeps it");
    });
    assert.equal(resolveFont(null), STROKE, "the scope ends");
    withDefaultFace("KiCad Font", () => assert.equal(resolveFont(null), STROKE, "Display Options' first choice, KiCad's default"));
    // the sheet's hit tests read the preference outside the painting
    setSchDefaultFont("DejaVu Sans");
    assert.equal(resolveSchFont(null), outline);
    const stroke = (t: string, size: number) => t.length * size * 0.1;
    assert.notEqual(schPlainMeasure(stroke)("AB", 1_000_000), stroke("AB", 1_000_000), "a pin's text measured in the default font");
    assert.equal(schPlainMeasure(stroke, "KiCad Font")("AB", 1_000_000), stroke("AB", 1_000_000), "one in the KiCad font, by the stroke font's measure");
    setSchDefaultFont(null);
    assert.equal(schPlainMeasure(stroke)("AB", 1_000_000), stroke("AB", 1_000_000));
  } finally {
    setFontResolver(() => STROKE);
    setSchDefaultFont(null);
  }
});
