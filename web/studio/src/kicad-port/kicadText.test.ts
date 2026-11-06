import { test } from "node:test";
import assert from "node:assert/strict";
import {
  effectiveTextPenWidth,
  fontInterline,
  inkBox,
  isStrokeFace,
  kicadTextBox,
  knockoutBox,
  layoutKicadText,
  layoutSegments,
  linePositions,
  parseMarkup,
  worldToFrame,
  frameToWorld,
  type KicadTextSpec,
  type OutlineFace,
} from "./kicadText";

const MM = 1_000_000;
const mm = (v: number) => Math.round(v * MM);
const spec = (over: Partial<KicadTextSpec>): KicadTextSpec => ({ text: "H", pos: [mm(50), mm(50)], size: [MM, MM], pen: mm(0.1), halign: "center", valign: "center", ...over });

/** The segments of a text, mm, rounded to the 0.1 um kicad-cli's SVG writes. */
const segsMm = (s: KicadTextSpec) => layoutSegments(layoutKicadText(s)).map((g) => g.map((v) => Math.round((v / MM) * 1e4) / 1e4));

/** Each expected segment (kicad-cli's `<path d="M x y L x y">`, mm) is drawn, within 0.15 um. */
function assertSegments(got: number[][], want: number[][]) {
  assert.equal(got.length, want.length, JSON.stringify(got));
  want.forEach((w, i) => w.forEach((v, k) => assert.ok(Math.abs(got[i]![k]! - v) <= 0.00015, `segment ${i}: ${JSON.stringify(got[i])} vs ${JSON.stringify(w)}`)));
}

// The values below are kicad-cli 10.99 (4fe1ca2060)'s `pcb export svg` of `gr_text`s at 50 mm .. 100 mm (size 1 mm, pen 0.1 mm unless said).
test("FONT::getLinePositions: left/top, centre/centre and right/bottom, the whole advance (INTER_CHAR is the box's only), the 6.0 fudges", () => {
  assertSegments(segsMm(spec({ halign: "left", valign: "top" })), [
    [50.3039, 51.0424, 50.3039, 50.0424],
    [50.3039, 50.5186, 50.8753, 50.5186],
    [50.8753, 51.0424, 50.8753, 50.0424],
  ]);
  assertSegments(segsMm(spec({ pos: [mm(60), mm(50)] })), [
    [59.7143, 50.4574, 59.7143, 49.4574],
    [59.7143, 49.9336, 60.2857, 49.9336],
    [60.2857, 50.4574, 60.2857, 49.4574],
  ]);
  assertSegments(segsMm(spec({ pos: [mm(70), mm(50)], halign: "right", valign: "bottom" })), [
    [69.1247, 49.8724, 69.1247, 48.8724],
    [69.1247, 49.3486, 69.6961, 49.3486],
    [69.6961, 49.8724, 69.6961, 48.8724],
  ]);
});

test("two lines: the interline (1.68 x 0.9583 sizes), the block centred on its 1.17 + interline height, each line centred on its own", () => {
  const lines = linePositions(spec({ text: "AB\nC" }), { kind: "stroke" });
  // baselines 49.604828 and 51.214772 mm: the A's and the C's lowest points are 1/21 of the size below them
  assert.equal(lines[0]!.y, 50 * MM + 994_800 - 1_389_972);
  assert.equal(lines[1]!.y - lines[0]!.y, 1_609_944);
  const segs = segsMm(spec({ text: "AB\nC" }));
  assert.deepEqual(segs[1], [49.1667, 49.6524, 49.5, 48.6524]);
  assert.deepEqual(segs.find((s) => s[0] === 50.0238 && s[2] === 49.881), [50.0238, 51.2624, 49.881, 51.2148]);
});

test("turned 90 degrees, mirrored, a width apart from the height, italic", () => {
  assertSegments(segsMm(spec({ pos: [mm(60), mm(50)], halign: "left", angleDeg: 90 })), [
    [60.4574, 49.6961, 59.4574, 49.6961],
    [59.9336, 49.6961, 59.9336, 49.1247],
    [60.4574, 49.1247, 59.4574, 49.1247],
  ]);
  // `(size 2 1)`: 2 mm high, 1 mm wide, mirrored about its position
  assertSegments(segsMm(spec({ pos: [mm(70), mm(50)], size: [MM, 2 * MM], halign: "left", mirror: true })), [
    [69.6961, 50.92, 69.6961, 48.92],
    [69.6961, 49.8724, 69.1247, 49.8724],
    [69.1247, 50.92, 69.1247, 48.92],
  ]);
  assertSegments(segsMm(spec({ pos: [mm(80), mm(50)], halign: "left", italic: true })), [
    [80.2979, 50.4574, 80.4229, 49.4574],
    [80.3634, 49.9336, 80.9348, 49.9336],
    [80.8694, 50.4574, 80.9944, 49.4574],
  ]);
});

test("markup: an overbar a tenth of the size short at each end, 1.23 sizes up; a subscript at 0.8 of the size, 0.15 of that down", () => {
  const segs = segsMm(spec({ text: "~{H}_{2}", pos: [mm(90), mm(50)], halign: "left" }));
  assert.deepEqual(segs[3], [90.1658, 49.1798, 91.0134, 49.1798]);
  assert.deepEqual(segs[4], [91.2658, 49.8441, 91.3039, 49.806]);
  assert.deepEqual(segs[segs.length - 1], [91.2277, 50.5679, 91.7229, 50.5679]);
  assert.deepEqual(parseMarkup("a~{b_{c}}d"), [
    { kind: "text", text: "a" },
    { kind: "over", children: [{ kind: "text", text: "b" }, { kind: "sub", children: [{ kind: "text", text: "c" }] }] },
    { kind: "text", text: "d" },
  ]);
  // an unclosed markup is text; an escape inside braces is kept whole; an empty one is dropped
  assert.deepEqual(parseMarkup("~{ab"), [{ kind: "text", text: "~{ab" }]);
  assert.deepEqual(parseMarkup("~{a{slash}b}"), [{ kind: "over", children: [{ kind: "text", text: "a{slash}b" }] }]);
  assert.deepEqual(parseMarkup("x~{}y"), [{ kind: "text", text: "x" }, { kind: "text", text: "y" }]);
});

test("a bold pen of 0.2 mm, right and top, turned 30 degrees", () => {
  assertSegments(segsMm(spec({ pos: [mm(100), mm(50)], pen: mm(0.2), bold: true, halign: "right", valign: "top", angleDeg: 30 })), [
    [99.7036, 51.3688, 99.2036, 50.5028],
    [99.4417, 50.9152, 99.9366, 50.6295],
    [100.1985, 51.0831, 99.6985, 50.2171],
  ]);
});

test("the pens: GetEffectiveTextPenWidth and ClampTextPenSize", () => {
  assert.equal(effectiveTextPenWidth(150_000, [MM, MM], false), 150_000);
  // no pen of its own: an eighth, or a fifth for bold, of the width; a quarter of the smaller side at most
  assert.equal(effectiveTextPenWidth(0, [MM, MM], false), 125_000);
  assert.equal(effectiveTextPenWidth(0, [2 * MM, MM], true), 250_000);
  assert.equal(effectiveTextPenWidth(400_000, [MM, MM], false), 250_000);
});

test("EDA_TEXT::GetTextBox of the stroke font: StringBoundaryLimits' 1.5 pens round the box INTER_CHAR short, 0.17 taller, the fudge", () => {
  // kicad-cli's `<text textLength="1.1476">`: the advance 1.0476 less 0.2, and 0.15 each side
  const b = kicadTextBox(spec({ halign: "left", valign: "top" }));
  assert.equal(b.x1 - b.x0, 1_147_619);
  assert.equal(b.y1 - b.y0, 1_300_000 + 221_000);
  assert.deepEqual([b.x0, b.y0], [50 * MM, 50 * MM - 221_000]);
  const c = kicadTextBox(spec({}));
  assert.equal(c.x0, 50 * MM - Math.trunc(1_147_619 / 2));
  assert.equal(c.y0, 50 * MM - Math.trunc(1_521_000 / 2));
});

test("the knockout box: what is drawn, grown by GetKnockoutTextMargin (a ninth of the height, or half the pen)", () => {
  const l = layoutKicadText(spec({ halign: "left", valign: "top" }));
  const ink = inkBox(l)!;
  const k = knockoutBox(l, [MM, MM])!;
  assert.equal(ink.x0 - k.x0, 111_111);
  assert.ok(Math.abs(ink.x0 - (50_303_884 - 50_000)) <= 1, String(ink.x0));
});

test("frameToWorld and worldToFrame undo each other (mirror, then the turn about the position)", () => {
  const l = { pos: [10, 20] as [number, number], angleDeg: 33, mirror: true };
  const [x, y] = frameToWorld(l, 17, -4);
  const [u, v] = worldToFrame(l, x, y);
  assert.ok(Math.abs(u - 17) < 1e-9 && Math.abs(v + 4) < 1e-9);
});

/** DejaVu Sans' metrics (2048 to the em; hhea 1901 / -483, no line gap) and the advance of H (1540) and its ink (201 .. 1338, 1493 high). */
const dejavu: OutlineFace = {
  family: "test",
  unitsPerEm: 2048,
  ascender: 1901,
  descender: -483,
  height: 2384,
  fakeBold: false,
  fakeItalic: false,
  advance: (t) => (Array.from(t).length * 1540) / 2048,
  ink: (t) => (t ? [201 / 2048, -1493 / 2048, (1540 * (Array.from(t).length - 1) + 1338) / 2048, 0] : null),
};

test("OUTLINE_FONT: the baseline a size below the top, no pen fudges, an em 1.4 times the size (kicad-cli's DejaVu Sans H)", () => {
  const font = { kind: "outline" as const, face: dejavu };
  const l = layoutKicadText(spec({ pos: [mm(80), mm(50)], halign: "left", valign: "top" }), font);
  assert.equal(l.runs.length, 1);
  assert.deepEqual([l.runs[0]!.x, l.runs[0]!.y, l.runs[0]!.emY], [mm(80), mm(51), 1.4 * MM]);
  const ink = inkBox(l)!;
  // kicad-cli: the H's polygon from 80.1373 to 80.9147 and from 49.9800 to 51.0000
  assert.ok(Math.abs(ink.x0 / MM - 80.1373) < 0.0002 && Math.abs(ink.x1 / MM - 80.9147) < 0.0002, JSON.stringify(ink));
  // kicad-cli's cap height is FreeType's hinted one (at 358 pixels to the em the H's top snaps to a whole pixel, 1/256 of the 1.4 mm em): within half a pixel
  assert.ok(Math.abs(ink.y0 / MM - 49.98) < 0.002 && Math.abs(ink.y1 / MM - 51) < 0.0002, JSON.stringify(ink));
  // centred: half the advance (1.4 x 0.752 mm) to the left
  const c = layoutKicadText(spec({ pos: [mm(80), mm(50)] }), font);
  assert.equal(c.runs[0]!.x, mm(80) - Math.trunc(Math.trunc((1.4 * MM * 1540) / 2048) / 2));
  // the interline: `height / units_per_EM` is 1 (an integer division), times 1.68
  assert.equal(fontInterline(font, MM), 1.68 * MM);
  // the box: from the ascender to the descender, no fudge taller
  const b = kicadTextBox(spec({ pos: [mm(80), mm(50)], halign: "left", valign: "top" }), font);
  assert.equal(b.y1 - b.y0, Math.trunc((1.4 * MM * 1901) / 2048) + Math.trunc((1.4 * MM * 483) / 2048));
});

test("FONT::GetFont: no face or one starting with KiCad Font is the stroke font", () => {
  assert.ok(isStrokeFace(null) && isStrokeFace("") && isStrokeFace("KiCad Font") && isStrokeFace("Default Font"));
  assert.ok(!isStrokeFace("DejaVu Sans"));
});
