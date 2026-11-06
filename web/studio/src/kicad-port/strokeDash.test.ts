import { test } from "node:test";
import assert from "node:assert/strict";
import { clipLine, dashLength, dashPieces, dotLength, edaAngleOf, gapLength, isDashStyle, strokeArc, strokeSegment, type DashPiece } from "./strokeDash";
import type { Shape } from "../api/types";

const len = ([a, b]: DashPiece) => Math.hypot(b[0] - a[0], b[1] - a[1]);

test("the dash, the dot and the gap scale with the width (GetDashLength, GetDotLength, GetGapLength with correction 1.0)", () => {
  assert.equal(dashLength(150_000), 11 * 150_000);
  assert.equal(gapLength(150_000), 4 * 150_000);
  assert.equal(dotLength(150_000), 0.2 * 150_000);
  // Board Setup > Formatting's ratios
  assert.equal(dashLength(100_000, { dash: 5, gap: 2 }), 400_000);
  assert.equal(gapLength(100_000, { dash: 5, gap: 2 }), 300_000);
  // `std::max( m_dashLengthRatio - correction, 1.0 )`
  assert.equal(dashLength(100_000, { dash: 1.5, gap: 0 }), 100_000);
  assert.equal(gapLength(100_000, { dash: 1, gap: -3 }), 100_000);
  assert.ok(isDashStyle("dash_dot") && !isDashStyle("solid") && !isDashStyle("default") && !isDashStyle(undefined));
});

test("a 20 mm dashed line 0.15 mm wide is nine dashes of 1.65 mm every 2.25 mm, the last one whole (what kicad-cli's Gerber has)", () => {
  const pieces = strokeSegment([0, 0], [20_000_000, 0], "dash", 150_000);
  assert.equal(pieces.length, 9);
  assert.deepEqual(pieces[0], [[0, 0], [1_650_000, 0]]);
  assert.deepEqual(pieces[8], [[18_000_000, 0], [19_650_000, 0]]);
  pieces.forEach((p, i) => assert.equal(p[0][0], i * 2_250_000));
});

test("the last stroke is cut at the end of the line (ClipLine against the segment's box), and a line going left or up runs from its own start", () => {
  // 10 mm at w = 0.15: the fifth dash starts at 9 mm and is cut at 10 mm
  const pieces = strokeSegment([0, 0], [10_000_000, 0], "dash", 150_000);
  assert.equal(pieces.length, 5);
  assert.deepEqual(pieces[4], [[9_000_000, 0], [10_000_000, 0]]);
  const back = strokeSegment([10_000_000, 5_000_000], [0, 5_000_000], "dash", 150_000);
  assert.deepEqual(back[0], [[10_000_000, 5_000_000], [8_350_000, 5_000_000]]);
  const up = strokeSegment([0, 0], [0, -3_000_000], "dot", 100_000);
  // a dot of 0.02 mm every 0.42 mm: 0, 0.42, .. 2.94
  assert.equal(up.length, 8);
  assert.deepEqual(up[1], [[0, -420_000], [0, -440_000]]);
  assert.equal(clipLine({ x: 0, y: 0, r: 10, b: 0 }, [12, 0], [15, 0]), null, "a stroke wholly past the end is a fast reject");
  assert.deepEqual(clipLine({ x: 0, y: 0, r: 10, b: 10 }, [5, 5], [15, 10]), [[5, 5], [10, 7]]);
});

test("a dash-dot-dot line is dash, gap, dot, gap, dot, gap over and over", () => {
  const w = 100_000;
  const p = strokeSegment([0, 0], [10_000_000, 0], "dash_dot_dot", w);
  // dash 1.1, gap 0.4, dot 0.02, gap 0.4, dot 0.02, gap 0.4: a period of 2.34 mm
  assert.deepEqual(p.slice(0, 4).map((x) => [x[0][0], Math.round(len(x))]), [
    [0, 1_100_000],
    [1_500_000, 20_000],
    [1_920_000, 20_000],
    [2_340_000, 1_100_000],
  ]);
});

test("EDA_ANGLE of a vector has KiCad's own values on the axes: -180 along -x, -90 up", () => {
  assert.equal(edaAngleOf(-5, 0), -180);
  assert.equal(edaAngleOf(0, -5), -90);
  assert.equal(edaAngleOf(3, 3), 45);
  assert.equal(edaAngleOf(-3, 3), 135);
  assert.equal(edaAngleOf(3, -3), -45);
});

test("a dashed ring is walked from its end point clockwise; a dash is drawn as chords of half a degree, a dot as one", () => {
  // r = 5 mm, w = 0.1 mm: a 1.1 mm dash is 12.6 degrees, so 26 chords
  const ring = strokeArc([0, 0], [5_000_000, 0], 360, "dash", 100_000);
  const first: DashPiece[] = [];
  for (const p of ring) {
    if (first.length && (first[first.length - 1]![1][0] !== p[0][0] || first[first.length - 1]![1][1] !== p[0][1])) break;
    first.push(p);
  }
  assert.equal(first.length, 26);
  assert.deepEqual(first[0]![0], [5_000_000, 0]);
  assert.ok(first[1]![0][1] > 0, "clockwise on the screen (y down)");
  // dots: a 0.02 mm dot and a 0.4 mm gap each 0.42 mm of 31.4 mm: 75 dots, one chord each
  const dots = strokeArc([0, 0], [5_000_000, 0], 360, "dot", 100_000);
  assert.equal(dots.length, 75);
  assert.ok(dots.every((d) => len(d) < 30_000));
});

test("a shape is stroked piece by piece as makeEffectiveShapes( true ) cuts it: a rectangle's sides, a polygon's closing edge, each from its own start", () => {
  const rect: Shape = { kind: "rect", id: "r", layer: "F.SilkS", stroke_width: 100_000, filled: false, start: [0, 0], end: [10_000_000, 10_000_000] };
  const r = dashPieces(rect, { style: "dot", width: 100_000 }, 5_000);
  // 24 dots a side (0.42 mm apart over 10 mm), each side's first at its corner -- what kicad-cli plots
  assert.equal(r.length, 4 * 24);
  for (const corner of [[0, 0], [10_000_000, 0], [10_000_000, 10_000_000], [0, 10_000_000]]) assert.ok(r.some((p) => p[0][0] === corner[0] && p[0][1] === corner[1]), `a dot starts at ${corner}`);
  const poly: Shape = { kind: "polygon", id: "p", layer: "F.Fab", stroke_width: 100_000, filled: false, pts: [[0, 0], [10_000_000, 0], [0, 10_000_000]] };
  const back = dashPieces(poly, { style: "dash", width: 100_000 }, 5_000);
  assert.ok(back.some((p) => p[0][0] === 0 && p[0][1] === 10_000_000), "the closing edge from the last corner back to the first");
  const circle: Shape = { kind: "circle", id: "c", layer: "F.Fab", stroke_width: 100_000, filled: false, center: [0, 0], end: [0, 5_000_000] };
  assert.deepEqual(dashPieces(circle, { style: "dot", width: 100_000 }, 5_000)[0]![0], [0, 5_000_000], "a circle starts at its end point");
  const rounded: Shape = { ...rect, corner_radius: 2_000_000 };
  const rr = dashPieces(rounded, { style: "dash", width: 100_000 }, 5_000);
  assert.deepEqual(rr[0]![0], [2_000_000, 0], "a rounded rectangle starts right of its top left corner");
  assert.equal(dashPieces(rect, { style: "dash", width: 0 }, 5_000).length, 0);
});

test("an arc is stroked from KiCad's start, whichever order the file gave its ends in", () => {
  // the upper half of a 5 mm circle, written start -> end anticlockwise on the screen: KiCad swaps the ends, so the strokes run from (-5, 0) through (0, -5)
  const arc: Shape = { kind: "arc", id: "a", layer: "F.Fab", stroke_width: 100_000, filled: false, start: [5_000_000, 0], mid: [0, -5_000_000], end: [-5_000_000, 0] };
  const p = dashPieces(arc, { style: "dot", width: 100_000 }, 5_000);
  assert.deepEqual(p[0]![0], [-5_000_000, 0]);
  assert.ok(p.every((x) => x[0][1] <= 0 && x[1][1] <= 0), "all on the upper half");
  assert.equal(p.length, 38, "15.7 mm of arc, a dot every 0.42 mm");
});
