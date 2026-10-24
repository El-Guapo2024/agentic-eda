import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardTextBox } from "../api/pcbItemTypes";
import type { BoardState } from "../api/types";
import {
  autoPenNm,
  boardTextBoxOf,
  clampPenNm,
  columnWidthNm,
  cornersInSequence,
  drawPos,
  escapeQuoted,
  grownToFit,
  layoutBoxText,
  legacyMarginNm,
  linebreakText,
  minSizeNm,
  newTextBox,
  textBoxHitDistance,
  textBoxToCmd,
  unescapeQuoted,
  withTextAngle,
} from "./pcbTextBox";

/** A line is as wide as 0.6 of its glyph per character (the real measure is the stroke font's). */
const measure = (text: string, size: number): number => text.length * size * 0.6;

function box(partial: Partial<BoardTextBox> = {}): BoardTextBox {
  return {
    ...boardTextBoxOf({
      layer: "F.SilkS",
      text: "hello",
      start: { x: 10_000, y: 10_000 },
      end: { x: 30_000, y: 20_000 },
      margin_left_nm: 500,
      margin_top_nm: 600,
      margin_right_nm: 700,
      margin_bottom_nm: 800,
      stroke_width_nm: 100,
      size_nm: 1_000,
      thickness_nm: 150,
      halign: "left",
      valign: "top",
    }),
    ...partial,
  };
}

test("the pen of a text with none is an eighth of the glyph and a fifth of it when bold, and a pen is clamped to a sixth of the glyph", () => {
  assert.equal(autoPenNm(1000, false), 125);
  assert.equal(autoPenNm(1000, true), 200);
  assert.equal(clampPenNm(500, 1000), 167);
  assert.equal(clampPenNm(100, 1000), 100);
});

test("the default margin of a box is half its border and three quarters of its text height", () => {
  assert.equal(legacyMarginNm(120, 1000), 60 + 750);
});

test("cornersInSequence: the rectangle's corners in the order the text's frame has them", () => {
  const b = box();
  assert.deepEqual(cornersInSequence(b, 0)[0], [10_000, 10_000]);
  assert.deepEqual(cornersInSequence(b, 90_000)[0], [10_000, 20_000]);
  assert.deepEqual(cornersInSequence(b, 180_000)[0], [30_000, 20_000]);
  assert.deepEqual(cornersInSequence(b, 270_000)[0], [30_000, 10_000]);
  // the box's corners need not be ordered
  const flipped = box({ start: [30_000, 20_000], end: [10_000, 10_000] });
  assert.deepEqual(cornersInSequence(flipped, 0), cornersInSequence(b, 0));
});

test("cornersInSequence: a polygon box takes the corners furthest left, up, right and down", () => {
  const b = box({ pts: [[0, 0], [5_000, 5_000], [0, 10_000], [-5_000, 5_000]], angle: 45_000 });
  assert.deepEqual(cornersInSequence(b, 45_000), [[-5_000, 5_000], [0, 0], [5_000, 5_000], [0, 10_000]]);
});

test("linebreakText: breaks at spaces, keeps a word wider than the column whole and keeps the paragraphs", () => {
  // a character of a 1000 um glyph is 600 um wide here
  assert.deepEqual(linebreakText("aaaa bbbb cccc", 5_000, 1_000, 0, measure), ["aaaa", "bbbb", "cccc"]);
  assert.deepEqual(linebreakText("aa bb cc dd", 3_500, 1_000, 0, measure), ["aa bb", "cc dd"]);
  assert.deepEqual(linebreakText("toolongaword", 1_000, 1_000, 0, measure), ["toolongaword"]);
  assert.deepEqual(linebreakText("one\ntwo three", 100_000, 1_000, 0, measure), ["one", "two three"]);
  // the pen's width is taken off the column
  assert.deepEqual(linebreakText("aa bb", 3_500, 1_000, 1_000, measure), ["aa", "bb"]);
});

test("drawPos: the text hangs on its justified corner, the margins in from it", () => {
  assert.deepEqual(drawPos(box()), [10_500, 10_600]);
  assert.deepEqual(drawPos(box({ halign: "right", valign: "bottom" })), [30_000 - 700, 20_000 - 800]);
  assert.deepEqual(drawPos(box({ halign: "center", valign: "center" })), [20_000, 15_000]);
  // a mirrored box is read from its other side
  assert.deepEqual(drawPos(box({ mirror: true })), [30_000 - 700, 10_600]);
});

test("drawPos: the margin is turned with the text", () => {
  // 90 degrees counter-clockwise: the frame's top left is the box's bottom left, and "in from the left" is up the screen
  const p = drawPos(box({ angle: 90_000 }));
  assert.deepEqual(p, [10_000 + 600, 20_000 - 500]);
});

test("columnWidthNm: the side the text runs along less its margins", () => {
  assert.equal(columnWidthNm(box()), 20_000 - 500 - 700);
  // 90 degrees: the text runs along the box's height, and source takes the top and bottom margins off
  assert.equal(columnWidthNm(box({ angle: 90_000 })), 10_000 - 600 - 800);
});

test("layoutBoxText: lines are an interline apart and each is justified on its own", () => {
  const b = box({ text: "ab\nabcd", halign: "center" });
  const l = layoutBoxText(b, measure);
  assert.equal(l.lines.length, 2);
  assert.equal(l.lines[0]!.x, -Math.trunc(measure("ab", 1000) / 2));
  assert.equal(l.lines[1]!.x, -Math.trunc(measure("abcd", 1000) / 2));
  assert.equal(l.lines[1]!.y - l.lines[0]!.y, Math.trunc(1000 * 1.68 * 0.9583));
  assert.deepEqual(l.anchor, [20_000, 10_600]);
  assert.equal(l.angleRad, 0);
  // a bottom-justified block is shifted up by its whole height
  const bottom = layoutBoxText(box({ text: "ab\nabcd", valign: "bottom" }), measure);
  const top = layoutBoxText(box({ text: "ab\nabcd", valign: "top" }), measure);
  assert.equal(top.lines[0]!.y - bottom.lines[0]!.y, Math.trunc(1000 * 1.17) + Math.trunc(1000 * 1.68 * 0.9583));
});

test("minSizeNm and grownToFit: the box is as high as its text and the margins, and not smaller", () => {
  const b = box({ text: "ab\ncd", end: [30_000, 10_100] });
  const min = minSizeNm(b, measure);
  assert.equal(min.w, 0);
  // two lines: (1000 + 2*round(150*1.5)) = 1450, 17 % more, an interline more, the top and bottom margins
  const ext = 1_000 + 2 * Math.round(150 * 1.5);
  assert.equal(min.h, ext + Math.round(ext * 0.17) + Math.trunc(1000 * 1.68 * 0.9583) + 600 + 800);
  const grown = grownToFit(b, measure);
  assert.equal(grown.end[1] - grown.start[1], min.h);
  assert.equal(grown.end[0], 30_000, "the width is the user's");
  // a box already big enough is the same box
  assert.equal(grownToFit(box({ end: [30_000, 40_000] }), measure).end[1], 40_000);
  // no text, no minimum
  assert.deepEqual(minSizeNm(box({ text: "", end: [30_000, 10_100] }), measure), { w: 0, h: 0 });
  // a box turned a quarter turn needs its width
  assert.deepEqual(minSizeNm(box({ text: "ab", angle: 90_000 }), measure).h, 0);
});

test("withTextAngle: a quarter turn turns the box counter-clockwise about its position, text and all", () => {
  const b = withTextAngle(box(), 90_000);
  assert.equal(b.angle, 90_000);
  assert.equal(b.pts.length, 0);
  // start stays, end (20000 right, 10000 down of it) goes 10000 right and 20000 up of it
  assert.deepEqual(b.start, [10_000, 10_000]);
  assert.deepEqual(b.end, [10_000 + 10_000, 10_000 - 20_000]);
  // the same angle is the same box
  assert.equal(withTextAngle(box(), 0).start[0], 10_000);
});

test("withTextAngle: an angle that is not a multiple of 90 makes a polygon, and 0 again makes the rectangle again", () => {
  const rot = withTextAngle(box(), 45_000);
  assert.equal(rot.pts.length, 4);
  assert.equal(rot.angle, 45_000);
  const back = withTextAngle(rot, 0);
  assert.equal(back.pts.length, 0);
  const near = (a: number, b: number) => assert.ok(Math.abs(a - b) <= 2, `${a} vs ${b}`);
  near(Math.min(back.start[0], back.end[0]), 10_000);
  near(Math.max(back.start[0], back.end[0]), 30_000);
  near(Math.min(back.start[1], back.end[1]), 10_000);
  near(Math.max(back.start[1], back.end[1]), 20_000);
});

test("textBoxHitDistance: anywhere in the box's bounding box hits, outside it is the distance from it", () => {
  const b = box();
  assert.equal(textBoxHitDistance(b, 20_000, 15_000), 0);
  assert.equal(textBoxHitDistance(b, 10_000 - 50, 15_000), 0, "the border's half width is in the box");
  assert.equal(textBoxHitDistance(b, 5_000, 15_000), 4_950);
  assert.equal(Math.round(textBoxHitDistance(b, 33_000, 24_000)), Math.round(Math.hypot(2_950, 3_950)));
});

test("newTextBox: the layer's text defaults, left and top, the default margins, mirrored on a back layer", () => {
  const board = { board_rules: { text_graphics: undefined } } as unknown as BoardState;
  const b = newTextBox(board, "F.SilkS", [0, 0], [10_000_000, 5_000_000]);
  assert.equal(b.layer, "F.SilkS");
  assert.deepEqual([b.halign, b.valign, b.mirror, b.border, b.knockout], ["left", "top", false, true, false]);
  assert.equal(b.size, 1_000_000);
  assert.equal(b.stroke_width, 100_000, "silkscreen's line width");
  const margin = legacyMarginNm(100_000, 1_000_000);
  assert.deepEqual(b.margins, [margin, margin, margin, margin]);
  assert.equal(newTextBox(board, "B.SilkS", [0, 0], [1000, 1000]).mirror, true);
  // the copper class has its own, larger text
  assert.equal(newTextBox(board, "F.Cu", [0, 0], [1000, 1000]).size, 1_500_000);
});

test("textBoxToCmd: the state record as the verb takes it, and back", () => {
  const b = box({ text: "x", line_style: "dash", bold: true, thickness_auto: true, thickness: 200, width: 900, size: 1_000 });
  const cmd = textBoxToCmd(b);
  assert.equal(cmd.thickness_nm, 0, "an automatic pen is sent as 0");
  assert.equal(cmd.width_nm, 900, "a glyph width that is not the height is sent");
  assert.equal(cmd.line_spacing_permille, 0, "KiCad's 1.0 is sent as 0");
  assert.deepEqual(cmd.start, { x: 10_000, y: 10_000 });
  assert.equal(boardTextBoxOf(cmd).line_spacing_permille, 1000);
});

test("quotes are stored as {dblquote} and shown as quotes", () => {
  assert.equal(escapeQuoted('say "hi"'), "say {dblquote}hi{dblquote}");
  assert.equal(unescapeQuoted("say {dblquote}hi{dblquote}"), 'say "hi"');
});
