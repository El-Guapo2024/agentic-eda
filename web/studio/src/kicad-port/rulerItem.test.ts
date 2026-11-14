import { test } from "node:test";
import assert from "node:assert/strict";
import { constantGlyph, dimensionLabel, rotatePoint, rulerDrawing, rulerStrings, shadowColorOf, tickFormatForScale, vectorAngleDeg, type Vec } from "./rulerItem";

const width = (s: string, size: number) => s.length * size * 0.6;
// 1 px = 10 um, a 1000 x 800 px screen from (0, 0)
const view = { scale: 1e-4, viewport: [0, 0, 10_000_000, 8_000_000] as const };

test("GetDimensionStrings: signed x and y, r, and θ counterclockwise from +X, at DimensionLabel's precision", () => {
  assert.deepEqual(rulerStrings([0, 0], [3_000_000, -4_000_000], { units: "mm" }), ["x: 3.000 mm", "y: -4.000 mm", "r: 5.000 mm", "θ: 53.1°"]);
  // pointing left: EDA_ANGLE says -180, negated 180
  assert.equal(rulerStrings([0, 0], [-1_000_000, 0], { units: "mm" })[3], "θ: 180.0°");
  // pointing down the screen is -90
  assert.equal(rulerStrings([0, 0], [0, 2_540_000], { units: "mil" })[3], "θ: -90.0°");
  assert.equal(rulerStrings([0, 0], [0, 2_540_000], { units: "mil" })[1], "y: 100.0 mils");
  // the inverted axes of the display options flip the signs of x and y only
  assert.deepEqual(rulerStrings([0, 0], [1_000_000, 1_000_000], { units: "in", flipX: true, flipY: true }).slice(0, 2), ["x: -0.0394 in", "y: -0.0394 in"]);
  assert.equal(dimensionLabel("", 2_500_000, "mm", false), "2.500");
});

test("EDA_ANGLE( VECTOR2D ) and RotatePoint keep KiCad's exact cases", () => {
  assert.deepEqual([[1, 0], [-1, 0], [0, 1], [0, -1], [2, 2], [-2, -2], [3, -3], [-3, 3]].map((v) => vectorAngleDeg(v as Vec)), [0, -180, 90, -90, 45, -135, -45, 135]);
  assert.deepEqual(rotatePoint([1, 0], 90), [0, -1], "RotatePoint by 90: ( y, -x )");
  assert.deepEqual(rotatePoint([1, 0], -90), [-0, 1]);
  const r = rotatePoint([1, 0], 30);
  assert.ok(Math.abs(r[0] - Math.cos(Math.PI / 6)) < 1e-12 && Math.abs(r[1] + 0.5) < 1e-12);
});

test("getTickFormatForScale: a 1-2-5 tick space of at least 10 px, from round millimetres or mils", () => {
  // 1e-4 px/nm: 10 px is 100 um -> 1, 2, 4, 10, 20, 40, 100 um ...: the first >= 100 um is 100 um, after six steps (format 0: major every 10, mid every 5)
  assert.deepEqual(tickFormatForScale(1e-4, "mm"), { tickSpace: 100_000, majorStep: 10, midStep: 5 });
  // from 2.54 nm by 2, 2.5, 2, 2, 2.5 ...: 50.8 um is under 10 px, 127 um (5 mils) is not -- format 2, major every 2nd
  const imp = tickFormatForScale(1e-4, "mil");
  assert.ok(Math.abs(imp.tickSpace - 127_000) < 1e-6, `5 mil: ${imp.tickSpace}`);
  assert.deepEqual([imp.majorStep, imp.midStep], [2, 0]);
});

test("the Measure tool's ruler: the line, ticks with labels every mid and major step, three back ticks, the stroke back from the origin, no arrowhead; the strings next to the end", () => {
  const origin: Vec = [1_000_000, 4_000_000];
  const end: Vec = [4_000_000, 4_000_000];
  const d = rulerDrawing(origin, end, view, { units: "mm" }, width);
  const g = constantGlyph(1e-4);
  assert.deepEqual(d.segs[0], { a: origin, b: end, width: g.stroke * 0.8 });
  // 3 mm at a 0.1 mm tick: 30 ticks, labelled at 0, 0.5, 1.0 .. 2.5 mm
  const labels = d.texts.filter((t) => !t.text.includes(":")).map((t) => t.text);
  assert.deepEqual(labels, ["0.000", "0.500", "1.000", "1.500", "2.000", "2.500"]);
  const tickSegs = d.segs.slice(1, 31);
  assert.equal(tickSegs.length, 30);
  // the ticks stand on the side `RotatePoint( rulerVec, 90 )` gives -- up the screen for a ruler to the right -- 5, 7.5 and 12.5 px long
  assert.ok(Math.abs(tickSegs[1]!.b[1] - (4_000_000 - 5 / 1e-4)) < 1e-6);
  assert.ok(Math.abs(tickSegs[5]!.b[1] - (4_000_000 - 7.5 / 1e-4)) < 1e-6);
  assert.ok(Math.abs(tickSegs[10]!.b[1] - (4_000_000 - 12.5 / 1e-4)) < 1e-6);
  const back = d.segs.slice(31, 34);
  assert.deepEqual(back.map((s) => s.a[0]), [1_000_000, 2_500_000, 4_000_000], "the ruler halved");
  // `drawBacksideTicks`' `getTickLineWidth( GetConstantGlyphHeight( gal, -1 ) )`: the small glyph's, as the stroke back from the origin after them
  const small = constantGlyph(1e-4, -1);
  assert.ok(back.every((s) => Math.abs(s.width - small.stroke * 0.8) < 1e-9));
  assert.ok(back.every((s) => Math.abs(s.b[1] - (4_000_000 + 12.5 / 1e-4)) < 1e-6), "on the other side, the major length");
  const last = d.segs[d.segs.length - 1]!;
  assert.deepEqual(last.a, origin);
  assert.ok(Math.abs(last.b[0] - (1_000_000 - 7.5 / 1e-4)) < 1e-6, "the back of the origin crosshair");
  const strings = d.texts.filter((t) => t.text.includes(":"));
  assert.deepEqual(strings.map((t) => t.text), ["x: 3.000 mm", "y: 0.000 mm", "r: 3.000 mm", "θ: 0.0°"]);
  // a ruler to the right prefers the text below-left of the end... prefX = 1 (right of the cursor: right-aligned to its left), prefY = -1 (below)
  assert.ok(strings.every((t) => t.halign === "right" && Math.abs(t.at[0] - (4_000_000 - 15 / 1e-4)) < 1e-6));
  assert.ok(Math.abs(strings[0]!.at[1] - (4_000_000 + g.linePitch)) < 1e-6);
});

test("Interactive Offset's ruler: no ticks, an arrowhead of two 30 degree strokes 12.5 px long at the end", () => {
  const d = rulerDrawing([1_000_000, 1_000_000], [3_000_000, 1_000_000], view, { units: "mm", showTicks: false, showEndArrowHead: true }, width);
  assert.equal(d.segs.length, 3);
  const [, h1, h2] = d.segs;
  assert.deepEqual(h1!.a, [3_000_000, 1_000_000]);
  const len = 12.5 / 1e-4;
  assert.ok(Math.abs(Math.hypot(h1!.b[0] - 3_000_000, h1!.b[1] - 1_000_000) - len) < 1e-6);
  assert.ok(Math.abs(h1!.b[1] - 1_000_000 - len / 2) < 1e-6 && Math.abs(h2!.b[1] - 1_000_000 + len / 2) < 1e-6, "one each side, 30 degrees off the line");
  assert.equal(d.texts.length, 4, "only the strings");
});

test("the strings go to the quadrant that fits on screen, not across the ruler", () => {
  // the end near the bottom right corner: below does not fit, so above
  const d = rulerDrawing([2_000_000, 2_000_000], [9_500_000, 7_800_000], view, { units: "mm" }, width);
  const strings = d.texts.filter((t) => t.text.includes(":"));
  assert.ok(strings[0]!.at[1] < 7_800_000, "above the end");
});

test("C3-7: the shadow layer draws the same ruler under it, every line a shadow width wider, the texts two wider and moved back by 1.2 shadow widths", () => {
  const origin: Vec = [1_000_000, 4_000_000];
  const end: Vec = [4_000_000, 4_000_000];
  const d = rulerDrawing(origin, end, view, { units: "mm" }, width);
  const sh = rulerDrawing(origin, end, view, { units: "mm" }, width, true);
  const g = constantGlyph(1e-4);
  const small = constantGlyph(1e-4, -1);
  assert.equal(sh.segs.length, d.segs.length);
  assert.equal(sh.texts.length, d.texts.length);
  // `getTickLineWidth( textDims, true )`: 0.8 of the stroke plus the shadow width (0.15 of the glyph height)
  assert.ok(Math.abs(sh.segs[0]!.width - (g.stroke * 0.8 + g.shadow)) < 1e-9);
  // the ticks: half the label's stroke, two shadow widths wider
  assert.ok(Math.abs(sh.segs[1]!.width - (small.stroke + 2 * small.shadow) / 2) < 1e-9);
  // the back ticks: the small glyph's tick width plus its shadow
  assert.ok(Math.abs(sh.segs[31]!.width - (small.stroke * 0.8 + small.shadow)) < 1e-9);
  // the cursor strings: right-aligned left of the cursor here, so moved right by trunc( trunc( shadow ) * 1.2 ), their stroke two shadow widths wider
  const strings = d.texts.filter((t) => t.text.includes(":"));
  const shStrings = sh.texts.filter((t) => t.text.includes(":"));
  const off = Math.trunc(Math.trunc(g.shadow) * 1.2);
  assert.ok(shStrings.every((t, i) => Math.abs(t.at[0] - (strings[i]!.at[0] + off)) < 1e-6 && t.at[1] === strings[i]!.at[1]));
  assert.ok(Math.abs(shStrings[0]!.stroke - (g.stroke + 2 * g.shadow)) < 1e-9);
  // a tick label of a ruler pointing right: right-aligned at 90 + 180 degrees, moved along its own direction (`labelOffset.x += off * cos`, `y -= off * sin`)
  const lab = d.texts.find((t) => t.text === "0.500")!;
  const shLab = sh.texts.find((t) => t.text === "0.500")!;
  const loff = Math.trunc(Math.trunc(small.shadow) * 1.2);
  const rad = (lab.angleDeg * Math.PI) / 180;
  assert.equal(lab.halign, "right");
  assert.ok(Math.abs(shLab.at[0] - (lab.at[0] + loff * Math.cos(rad))) < 1e-6 && Math.abs(shLab.at[1] - (lab.at[1] - loff * Math.sin(rad))) < 1e-6);
});

test("C3-7: GetShadowColor -- black under a bright colour (weighted brightness above half), white under a dark one", () => {
  assert.equal(shadowColorOf({ r: 1, g: 1, b: 1 }), "#000000");
  assert.equal(shadowColorOf({ r: 0.2, g: 0.2, b: 0.2 }), "#ffffff");
  // 0.299 + 0.117 = 0.416: magenta is "dark"
  assert.equal(shadowColorOf({ r: 1, g: 0, b: 1 }), "#ffffff");
  assert.equal(shadowColorOf({ r: 0, g: 1, b: 0 }), "#000000");
});
