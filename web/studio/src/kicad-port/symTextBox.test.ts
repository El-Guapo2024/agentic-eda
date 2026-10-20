import { test } from "node:test";
import assert from "node:assert/strict";
import { INITIAL_LAST_TEXT_BOX, boxOfTextBox, grownToFit, isVertical, lastOf, layoutLibTextBox, newTextBox, textBoxDistanceUm, textBoxMarginMm, textBoxMinSizeUm, type LibTextBox } from "./symTextBox";

/** Every glyph is 0.6 of the size wide: a measure that needs no font table. */
const measure = (text: string, sizeUm: number) => text.length * sizeUm * 0.6;

const box = (over: Partial<LibTextBox> = {}): LibTextBox => ({
  kind: "text_box",
  unit: 1,
  body_style: 1,
  text: "hello",
  start: { x: -17.78, y: 6.35 },
  end: { x: -10.16, y: -1.27 },
  angle_deg: 0,
  size_mm: 1.27,
  h_align: "left",
  v_align: "top",
  stroke_mm: 0.254,
  fill: "none",
  ...over,
});

test("a box with no margin of its own has four fifths of the text height, and one that has keeps it", () => {
  assert.equal(textBoxMarginMm({ size_mm: 1.27 }), 1.016);
  assert.equal(textBoxMarginMm({ size_mm: 0.635 }), 0.508);
  assert.equal(textBoxMarginMm({ size_mm: 1.27, margin_mm: 0 }), 1.016);
  assert.equal(textBoxMarginMm({ size_mm: 1.27, margin_mm: 0.25 }), 0.25);
});

test("the corners are the editor's: Y flipped and ordered", () => {
  assert.deepEqual(boxOfTextBox(box()), { left: -17780, top: -6350, right: -10160, bottom: 1270 });
  // drawn the other way round
  assert.deepEqual(boxOfTextBox(box({ start: { x: -10.16, y: -1.27 }, end: { x: -17.78, y: 6.35 } })), { left: -17780, top: -6350, right: -10160, bottom: 1270 });
});

test("the text is laid out in the box: wrapped to its width less the margins, from the top and left margins", () => {
  const l = layoutLibTextBox(box({ text: "aa bb cc", end: { x: -10.16 + 0, y: 6.35 - 7.62 } }), measure);
  // 7.62 mm wide, 1.016 margins: 5.588 mm of text; a glyph is 0.762 mm wide: 7 glyphs fit
  assert.deepEqual(
    l.lines.map((x) => x.text),
    ["aa bb", "cc"]
  );
  assert.equal(l.vertical, false);
  assert.deepEqual(l.origin, [-17780, -6350], "from the top left corner");
  assert.equal(l.lines[0]!.u, 1016, "a left-justified line starts at the margin");
  // text that runs up the box is laid out in the turned frame
  assert.equal(layoutLibTextBox(box({ angle_deg: 90 }), measure).vertical, true);
  assert.equal(isVertical(box({ angle_deg: 90 })), true);
  assert.equal(isVertical(box({ angle_deg: 0 })), false);
});

test("the box can be no smaller than its text: as high (as wide, vertically) as the lines and the margins", () => {
  const g = box({ text: "one", end: { x: -10.16, y: 6.35 - 1 } });
  const min = textBoxMinSizeUm(g, measure);
  // ext = 1270 + 2 * round(159 * 1.5) = 1270 + 478 = 1748; + 17 % = 297; 2 margins of 1016
  assert.equal(min.w, 0);
  assert.equal(min.h, 1748 + 297 + 2032);
  assert.equal(textBoxMinSizeUm(box({ text: "" }), measure).h, 0, "no text, no minimum");
  const v = textBoxMinSizeUm(box({ angle_deg: 90 }), measure);
  assert.equal(v.h, 0);
  assert.ok(v.w > 0, "for text that runs up the box the width is what is held");
  // a second line adds an interline: round(1270 * 1.68 * 0.9583) = 2045 (and a text too long for the box wraps to two lines, so it has them too)
  assert.equal(textBoxMinSizeUm(box({ text: "one line", end: { x: -10.16, y: 6.35 - 1 } }), measure).h, min.h + 2045, "wrapped to two lines");
  const two = textBoxMinSizeUm(box({ text: "one\ntwo" }), measure);
  assert.equal(two.h, min.h + 2045);
  // bold has the pen of a fifth
  assert.ok(textBoxMinSizeUm(box({ bold: true }), measure).h > textBoxMinSizeUm(box(), measure).h);
});

test("a box smaller than its text is grown past its start the way it was drawn", () => {
  const small = box({ text: "one", end: { x: -10.16, y: 6.35 - 1 } }); // one mm high, drawn downward
  const grown = grownToFit(small, measure);
  const min = textBoxMinSizeUm(small, measure).h / 1000; // mm
  assert.ok(Math.abs(grown.start.y - 6.35) < 1e-9 && grown.start.x === -17.78, "the start stays");
  assert.ok(Math.abs(grown.end.y - (6.35 - min)) < 1e-4, `${grown.end.y}`);
  assert.equal(grown.end.x, -10.16, "the width is not touched for text that runs along it");
  // drawn upward, it grows upward
  const up = grownToFit(box({ text: "one", start: { x: -17.78, y: -1.27 }, end: { x: -10.16, y: 0 } }), measure);
  assert.ok(up.end.y > 0 && up.end.y > 1, "the far corner went up");
  // a box that is big enough is the same graphic
  const big = box();
  assert.equal(grownToFit(big, measure), big);
  // vertical: the width
  const narrow = grownToFit(box({ angle_deg: 90, start: { x: 0, y: 0 }, end: { x: 1, y: -10 } }), measure);
  assert.ok(narrow.end.x > 1 && narrow.end.y === -10);
});

test("a click is on a text box anywhere in it", () => {
  const g = box();
  assert.equal(textBoxDistanceUm(g, -12000, -2000), 0, "inside");
  assert.equal(textBoxDistanceUm(g, -17780, -6350), 0, "on a corner");
  assert.equal(textBoxDistanceUm(g, -9160, 0), 1000, "1 mm right of it");
  assert.ok(Math.abs(textBoxDistanceUm(g, -9160, 2270) - Math.hypot(1000, 1000)) < 1e-9, "off a corner");
});

test("the tool starts a box the way the last one was, with the corners the clicks made", () => {
  const first = newTextBox([-17780, -6350], [-10160, 1270], 1, 1, INITIAL_LAST_TEXT_BOX);
  assert.deepEqual(first, {
    kind: "text_box",
    unit: 1,
    body_style: 1,
    text: "",
    start: { x: -17.78, y: 6.35 },
    end: { x: -10.16, y: -1.27 },
    angle_deg: 0,
    size_mm: 1.27,
    bold: false,
    italic: false,
    h_align: "left",
    v_align: "top",
    margin_mm: 1.016,
    stroke_mm: 0,
    fill: "none",
  });
  const edited = { ...first, bold: true, italic: true, angle_deg: 90, h_align: "right" as const, fill: "background" as const, stroke_mm: 0.5 };
  assert.deepEqual(lastOf(edited), { bold: true, italic: true, vertical: true, hAlign: "right", fill: "background", strokeMm: 0.5 });
  const second = newTextBox([0, 0], [1270, -1270], 2, 2, lastOf(edited));
  assert.deepEqual([second.unit, second.body_style, second.bold, second.italic, second.angle_deg, second.h_align, second.v_align, second.fill, second.stroke_mm], [2, 2, true, true, 90, "right", "top", "background", 0.5]);
});
