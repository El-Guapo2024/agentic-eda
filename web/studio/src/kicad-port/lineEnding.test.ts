import { test } from "node:test";
import assert from "node:assert/strict";
import { effectiveSize, endingInks, endingPlacement, endingShape, kicadArc, shortenArc, shortenBezier, shortenDepth, shortenSegment, type LineEnding } from "./lineEnding";

const e = (style: LineEnding["style"], over: Partial<LineEnding> = {}): LineEnding => ({ style, length: 0, width: 0, stroke_width: 0, ...over });

test("an automatic ending is five times the line width, a size set on one axis is the other's too (GetEffectiveSize)", () => {
  assert.deepEqual(effectiveSize(e("arrow"), 600), [3000, 3000]);
  assert.deepEqual(effectiveSize(e("arrow", { length: 2000 }), 600), [2000, 2000]);
  assert.deepEqual(effectiveSize(e("arrow", { width: 1500 }), 600), [1500, 1500]);
  assert.deepEqual(effectiveSize(e("arrow", { length: 2000, width: 1000 }), 600), [2000, 1000]);
});

test("an arrow shortens the line by its length, a circle or a square by half, an open arrow not at all, less half the outline (GetShortenDepth)", () => {
  assert.equal(shortenDepth(e("arrow", { length: 3000 }), 600), 3000);
  assert.equal(shortenDepth(e("circle", { length: 3000 }), 600), 1500);
  assert.equal(shortenDepth(e("square", { length: 3001 }), 600), 1500, "integer halves");
  assert.equal(shortenDepth(e("arrow_open", { length: 3000 }), 600), 0);
  assert.equal(shortenDepth(e("arrow", { length: 3000, stroke_width: 600 }), 600), 2700);
  assert.equal(shortenDepth(e("none", { length: 3000 }), 600), 0);
  assert.equal(shortenDepth(undefined, 600), 0);
});

test("an arrow's tip is on the end and its base reaches back along the line (GetShapes)", () => {
  // A line from (0, 0) to (10 mm, 0): the end's tangent is 0 degrees.
  const pts = endingShape(e("arrow", { length: 3000, width: 2000 }), [10_000, 0], 0, 600);
  assert.deepEqual(pts, [[10_000, 0], [7_000, 1_000], [7_000, -1_000], [10_000, 0]]);
  // Pointing down the screen (90 degrees in KiCad's y-down frame).
  const down = endingShape(e("arrow", { length: 3000, width: 2000 }), [0, 10_000], 90, 600);
  assert.deepEqual(down.map(([x, y]) => [Math.round(x), Math.round(y)]), [[0, 10_000], [-1_000, 7_000], [1_000, 7_000], [0, 10_000]]);
  assert.equal(endingShape(e("arrow_open"), [0, 0], 0, 200).length, 3, "an open arrow is three points");
  assert.equal(endingShape(e("circle"), [0, 0], 0, 200).length, 33, "a circle is 32 segments, closed");
  assert.deepEqual(endingShape(e("square", { length: 2000 }), [0, 0], 0, 200), [[1000, 1000], [1000, -1000], [-1000, -1000], [-1000, 1000], [1000, 1000]]);
});

test("a segment's body stops short of its endings, and is not drawn when they meet (ShortenSegmentForEndings)", () => {
  assert.deepEqual(shortenSegment([0, 0], [10_000, 0], e("arrow", { length: 3000 }), e("circle", { length: 2000 }), 600), [[3000, 0], [9000, 0]]);
  assert.equal(shortenSegment([0, 0], [4_000, 0], e("arrow", { length: 3000 }), e("arrow", { length: 3000 }), 600), null);
  assert.deepEqual(shortenSegment([0, 0], [4_000, 0], undefined, undefined, 600), [[0, 0], [4000, 0]]);
});

test("the endings of a segment point out of it at both ends (GetEndingTangents)", () => {
  const at = endingPlacement({ kind: "segment", start: [0, 0], end: [0, 10_000], stroke_width: 200, end_ending: e("arrow") });
  assert.deepEqual([at?.startTangent, at?.endTangent], [270, 90]);
});

test("an arc wound the other way has its ends swapped, as SetArcGeometry does, and the endings turn back and on by 90 degrees", () => {
  // Centre (0, 0), radius 10 mm, from (10, 0) to (0, 10) through the quadrant between: increasing angle in KiCad's frame, kept as it is.
  const kept = kicadArc([10_000, 0], [7_071, 7_071], [0, 10_000]);
  assert.deepEqual(kept?.start, [10_000, 0]);
  const swapped = kicadArc([0, 10_000], [7_071, 7_071], [10_000, 0]);
  assert.deepEqual(swapped?.start, [10_000, 0], "the file's end is KiCad's start");
  const at = endingPlacement({ kind: "arc", start: [10_000, 0], mid: [7_071, 7_071], end: [0, 10_000], stroke_width: 200, start_ending: e("arrow") });
  assert.ok(at);
  assert.equal(Math.round(at.startTangent), -90, "at (10, 0) the arc leaves downwards on the screen, so the arrow points up");
  assert.equal(Math.round(at.endTangent), 180);
  const short = shortenArc(0, 90, 10_000, e("arrow", { length: 1000 }), undefined, 200);
  assert.ok(short);
  assert.ok(Math.abs(short[0] - (1000 / 10_000) * (180 / Math.PI)) < 1e-9);
});

test("a curve is cut at the arc lengths its endings take (ShortenedBezierCurve)", () => {
  const b = { start: [0, 0] as const, c1: [3000, 0] as const, c2: [7000, 0] as const, end: [10_000, 0] as const };
  const cut = shortenBezier(b, e("arrow", { length: 2000 }), e("arrow", { length: 3000 }), 200);
  assert.ok(cut);
  assert.ok(Math.abs(cut.start[0] - 2000) < 1 && Math.abs(cut.end[0] - 7000) < 1, `${cut.start} ${cut.end}`);
  assert.equal(shortenBezier(b, e("arrow", { length: 6000 }), e("arrow", { length: 6000 }), 200), null);
});

test("the inks are filled shapes, an open arrow is a stroke of the outline width or the line's (LINE_ENDING::Draw)", () => {
  const inks = endingInks({ kind: "segment", start: [0, 0], end: [10_000, 0], stroke_width: 200, start_ending: e("arrow_open"), end_ending: e("square", { stroke_width: 100 }) });
  assert.deepEqual(inks.map((i) => [i.fill, i.stroke, i.pts.length]), [[false, 200, 3], [true, 100, 5]]);
  assert.deepEqual(endingInks({ kind: "segment", start: [0, 0], end: [10_000, 0], stroke_width: 200 }), []);
});
