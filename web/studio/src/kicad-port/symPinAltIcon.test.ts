import { test } from "node:test";
import assert from "node:assert/strict";
import { altIconStrokes, MAX_ALT_ICON_SIZE_NM, pinAltIcon } from "./symPinAltIcon";
import { pinTextOffsetNm } from "./symPinText";

const NAME = { widthNm: 3_000_000, sizeNm: 1_270_000 };
const REACH = Math.round(1.5 * Math.round(1_270_000 / 8)); // StringBoundaryLimits' reach on every side
const BOX_W = NAME.widthNm + 2 * REACH;
const BOX_H = NAME.sizeNm + 2 * REACH;

test("a name inside the body: the icon follows the name, on the pin's line", () => {
  // the pin running right: connection point at the origin, body 2.54 mm along
  const right = pinAltIcon([0, 0], [2_540_000, 0], NAME, 508_000);
  assert.equal(right.sizeNm, 1_270_000);
  assert.equal(right.vertical, false);
  assert.ok(Math.abs(right.center[0] - (2_540_000 + 508_000 + BOX_W + 1_270_000 * 0.75)) < 1e-9);
  assert.equal(right.center[1], 0);
  // running left: mirrored
  const left = pinAltIcon([10_000_000, 5_000_000], [7_460_000, 5_000_000], NAME, 508_000);
  assert.ok(Math.abs(left.center[0] - (10_000_000 - (2_540_000 + 508_000 + BOX_W + 1_270_000 * 0.75))) < 1e-9);
  assert.equal(left.center[1], 5_000_000);
  // running up (the body above the connection point): the icon above the body, turned
  const up = pinAltIcon([0, 0], [0, -2_540_000], NAME, 508_000);
  assert.equal(up.vertical, true);
  assert.equal(up.center[0], 0);
  assert.ok(Math.abs(up.center[1] + (2_540_000 + 508_000 + BOX_W + 1_270_000 * 0.75)) < 1e-9);
  // running down
  const down = pinAltIcon([0, 0], [0, 2_540_000], NAME, 508_000);
  assert.equal(down.vertical, true);
  assert.ok(Math.abs(down.center[1] - (2_540_000 + 508_000 + BOX_W + 1_270_000 * 0.75)) < 1e-9);
});

test("a name over the pin: the icon goes before the name, above the line (to the left of a vertical pin)", () => {
  const right = pinAltIcon([0, 0], [2_540_000, 0], NAME, 0);
  assert.ok(Math.abs(right.center[0] - (1_270_000 - BOX_W / 2 - 1_270_000 * 0.75)) < 1e-9);
  assert.ok(Math.abs(right.center[1] + (BOX_H / 2 + pinTextOffsetNm())) < 1e-9, "lifted by half the name's height and the text offset");
  const left = pinAltIcon([0, 0], [-2_540_000, 0], NAME, 0);
  assert.ok(Math.abs(left.center[0] + (1_270_000 - BOX_W / 2 - 1_270_000 * 0.75)) < 1e-9, "the pin's frame mirrored in x");
  assert.equal(left.center[1], right.center[1], "still above the line");
  const up = pinAltIcon([0, 0], [0, -2_540_000], NAME, 0);
  assert.equal(up.vertical, true);
  assert.ok(up.center[0] < 0, "left of the line");
  assert.ok(Math.abs(up.center[0] + (BOX_H / 2 + pinTextOffsetNm())) < 1e-9);
  assert.ok(Math.abs(up.center[1] + (1_270_000 - BOX_W / 2 - 1_270_000 * 0.75)) < 1e-9);
  const down = pinAltIcon([0, 0], [0, 2_540_000], NAME, 0);
  assert.ok(down.center[0] < 0 && Math.abs(down.center[0] - up.center[0]) < 1e-9, "left of a pin running down too");
  assert.ok(Math.abs(down.center[1] - (1_270_000 - BOX_W / 2 - 1_270_000 * 0.75)) < 1e-9);
});

test("the icon is the size of the name, but never more than 1.5 mm", () => {
  assert.equal(pinAltIcon([0, 0], [2_540_000, 0], { widthNm: 1_000_000, sizeNm: 2_000_000 }, 508_000).sizeNm, MAX_ALT_ICON_SIZE_NM);
  assert.equal(pinAltIcon([0, 0], [2_540_000, 0], { widthNm: 1_000_000, sizeNm: 1_000_000 }, 508_000).sizeNm, 1_000_000);
});

test("a pin of no length runs right", () => {
  const icon = pinAltIcon([100_000, 200_000], [100_000, 200_000], NAME, 508_000);
  assert.equal(icon.vertical, false);
  assert.ok(icon.center[0] > 100_000 && icon.center[1] === 200_000);
});

test("the glyph: two arrows pointing right and the arc from the middle down to the lower one", () => {
  const g = altIconStrokes(1_000_000);
  assert.equal(g.widthNm, 100_000);
  assert.equal(g.lines.length, 6);
  // the full top line from the right end to the left end, 250 above the middle
  assert.deepEqual(g.lines[0], [500_000, -250_000, -500_000, -250_000]);
  // the bottom line from the right end to the middle
  assert.deepEqual(g.lines[3], [500_000, 250_000, 0, 250_000]);
  // the arrowheads: 1.2 * size / 8 back, size / 8 either side of their line
  assert.deepEqual(g.lines[1], [500_000, -250_000, 500_000 - 150_000, -250_000 - 125_000]);
  assert.deepEqual(g.lines[2], [500_000, -250_000, 500_000 - 150_000, -250_000 + 125_000]);
  assert.deepEqual(g.lines[4], [500_000, 250_000, 500_000 - 150_000, 250_000 - 125_000]);
  assert.deepEqual(g.lines[5], [500_000, 250_000, 500_000 - 150_000, 250_000 + 125_000]);
  // the arc is centred on the icon, a quarter of its size in radius, from the left of the middle round to the bottom
  assert.deepEqual(g.arc, { cx: 0, cy: 0, r: 250_000, from: Math.PI, to: Math.PI / 2 });
});
