import { test } from "node:test";
import assert from "node:assert/strict";
import { EDGE_CUTS_DIM_FACTOR, HI_CONTRAST_FACTOR, hiContrastDim, highlightDarken, mix, parseColor, ratsnestCurveControl, ratsnestLineColor } from "./color4d";
import { padContrast, padHoleContrast, viaCopperContrast, viaHoleContrast } from "./hiContrast";

const close = (a: number, b: number, eps = 1e-9) => assert.ok(Math.abs(a - b) <= eps, `${a} vs ${b}`);

test("COLOR4D::Mix keeps the given share of the colour and its alpha", () => {
  const m = mix({ r: 1, g: 0, b: 0, a: 0.5 }, { r: 0, g: 0, b: 1, a: 1 }, 0.2);
  close(m.r, 0.2);
  close(m.b, 0.8);
  close(m.a, 0.5);
});

test("the high-contrast factor is 1 - hicontrast_dimming_factor (0.8), Edge.Cuts at least 0.3", () => {
  close(HI_CONTRAST_FACTOR, 0.2, 1e-12);
  close(EDGE_CUTS_DIM_FACTOR, 0.3, 1e-12);
});

test("Dim mixes an item's colour into the background and keeps it opaque where it was", () => {
  // KiCad Default's F.Cu over its background
  assert.equal(hiContrastDim("#c83434ff", "#001023ff"), "rgba(40, 23, 38, 1)");
  // the alpha of a translucent layer colour is kept, not multiplied
  assert.equal(hiContrastDim("#ffffff80", "#000000"), "rgba(51, 51, 51, 0.502)");
  assert.equal(hiContrastDim("not a colour", "#000000"), "not a colour");
});

test("the net highlight darkens what is not on the net by 1 - m_highlightFactor", () => {
  assert.equal(highlightDarken("#ffffff"), "rgba(128, 128, 128, 1)");
  assert.equal(highlightDarken("#f2eda180"), "rgba(121, 119, 81, 0.502)");
});

test("RATSNEST_VIEW_ITEM::ViewDraw: dynamic lines brighten by 0.5 and gain 0.3 alpha on a dark background", () => {
  const c = parseColor(ratsnestLineColor("#00f8ff59", "#001023ff", { dynamic: true, highlighted: false, dimStatic: true }))!;
  close(c.r, 0.5, 0.003);
  close(c.a, 0x59 / 255 + 0.3, 0.001);
});

test("RATSNEST_VIEW_ITEM::ViewDraw: dimStatic halves a static line's alpha, a highlighted net brightens by 0.8 and gains 0.4", () => {
  const plain = ratsnestLineColor("#00f8ff59", "#001023ff", { dynamic: false, highlighted: false, dimStatic: false });
  assert.equal(plain, "rgba(0, 248, 255, 0.349)");
  const dimmed = parseColor(ratsnestLineColor("#00f8ff59", "#001023ff", { dynamic: false, highlighted: false, dimStatic: true }))!;
  close(dimmed.a, 0x59 / 255 / 2, 0.001);
  close(dimmed.g, 248 / 255, 0.003);
  const lit = parseColor(ratsnestLineColor("#00f8ff59", "#001023ff", { dynamic: false, highlighted: true, dimStatic: true }))!;
  close(lit.r, 0.8, 0.003);
  close(lit.a, 0x59 / 255 / 2 + 0.4, 0.001);
});

test("RATSNEST_VIEW_ITEM::ViewDraw: a light background darkens instead of brightening, the alpha at most 1", () => {
  const c = parseColor(ratsnestLineColor("#8080ffe0", "#ffffff", { dynamic: true, highlighted: false, dimStatic: false }))!;
  close(c.b, 0.5, 0.003);
  close(c.a, 1);
});

test("the curved ratsnest's control point is 0.1 of the length off the middle, both control points the same", () => {
  assert.deepEqual(ratsnestCurveControl([0, 0], [1000, 0]), [500, 100]);
  assert.deepEqual(ratsnestCurveControl([0, 0], [0, 1000]), [-100, 500]);
  assert.deepEqual(ratsnestCurveControl([10, 10], [13, 10]), [11, 10], "VECTOR2I truncates");
});

test("High Contrast Mode: a pad is active on its copper layer, a through-hole pad on a copper layer it flashes on", () => {
  assert.equal(padContrast("F.Cu", { th: false, copperLayer: "F.Cu", flashesOnPrimary: false }, false), "active");
  assert.equal(padContrast("B.Cu", { th: false, copperLayer: "F.Cu", flashesOnPrimary: false }, false), "dim");
  assert.equal(padContrast("B.Cu", { th: false, copperLayer: "F.Cu", flashesOnPrimary: false }, true), "hide");
  assert.equal(padContrast("In1.Cu", { th: true, copperLayer: "F.Cu", flashesOnPrimary: true }, false), "active");
  assert.equal(padContrast("In1.Cu", { th: true, copperLayer: "F.Cu", flashesOnPrimary: false }, false), "dim");
  assert.equal(padContrast("F.SilkS", { th: true, copperLayer: "F.Cu", flashesOnPrimary: false }, false), "dim");
  // `LAYER_PAD_PLATEDHOLES` is high contrast with a copper layer primary only: a technical layer primary dims the holes
  assert.equal(padHoleContrast("In1.Cu", false), "active");
  assert.equal(padHoleContrast("F.SilkS", false), "dim");
  assert.equal(padHoleContrast("F.Fab", true), "hide");
});

test("High Contrast Mode: a via's copper is active on the primary layer only, its hole with a copper layer primary (a blind via's on its span)", () => {
  const copper = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
  assert.equal(viaCopperContrast("In1.Cu", "In1.Cu", false), "active");
  assert.equal(viaCopperContrast("In1.Cu", "F.Cu", false), "dim");
  assert.equal(viaCopperContrast("F.SilkS", "F.Cu", true), "hide");
  assert.equal(viaHoleContrast("In2.Cu", { from: "F.Cu", to: "B.Cu" }, copper, false), "active");
  assert.equal(viaHoleContrast("F.SilkS", { from: "F.Cu", to: "B.Cu" }, copper, false), "dim");
  assert.equal(viaHoleContrast("In1.Cu", { type: "blind", from: "F.Cu", to: "In1.Cu" }, copper, false), "active");
  assert.equal(viaHoleContrast("B.Cu", { type: "blind", from: "F.Cu", to: "In1.Cu" }, copper, false), "dim");
});
