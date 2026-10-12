import { test } from "node:test";
import assert from "node:assert/strict";
import type { SchematicText } from "../api/types";
import { interlineUm, keepUprightIsVertical, schTextBox, schTextLines, textLie } from "./schTextItem";

// 0.6 of the size per character: no font needed
const measure = (text: string, size: number) => text.length * size * 0.6;

const text = (extra: Partial<SchematicText> = {}): SchematicText => ({ id: "txt_a", content: "hello", at: [50_000, 50_000], angle: 0, size_um: 1_270, ...extra });

test("a text from a backend that sends no justification lies left and bottom along the sheet, as it was always drawn", () => {
  assert.deepEqual(textLie({ angle: 0 }), { h: "left", v: "bottom", vertical: false });
  assert.deepEqual(textLie({ angle: 90 }), { h: "left", v: "bottom", vertical: true });
  assert.deepEqual(textLie({ angle: 0, h: "center", v: "top", vertical: true }), { h: "center", v: "top", vertical: true });
});

test("KeepUpright: -45 to 45, 135 to 225 (not 135) and 315 up are along the sheet, the rest up it", () => {
  for (const a of [0, 45, 315, 359, 180, 225, 136, 360, -10]) assert.equal(keepUprightIsVertical(a), false, `${a}`);
  for (const a of [46, 90, 135, 226, 270, 314, -90, 450]) assert.equal(keepUprightIsVertical(a), true, `${a}`);
});

test("the box of a text sits on its anchor the way it is justified", () => {
  const at = 50_000;
  const left = schTextBox(text(), measure);
  const centre = schTextBox(text({ h: "center", v: "center" }), measure);
  const right = schTextBox(text({ h: "right", v: "top" }), measure);
  const width = left.maxX - left.minX;
  // left: starts at the anchor; centre: the anchor is in the middle; right: ends at it
  assert.equal(left.minX, at);
  assert.ok(Math.abs((centre.minX + centre.maxX) / 2 - at) <= 1, "centred across");
  assert.equal(right.maxX, at);
  assert.equal(right.maxX - right.minX, width);
  // bottom: the text is above the anchor but for the pen's overhang; centre across the anchor; top: below it but for the same
  assert.ok(left.minY < at && left.maxY >= at && left.maxY - at < 400, "bottom");
  assert.ok(Math.abs((centre.minY + centre.maxY) / 2 - at) <= 1, "centred up and down");
  assert.ok(right.minY < at && at - right.minY < 400 && right.maxY > at, "top");
  // right is left mirrored about the anchor
  assert.equal(right.minX, at - width);
});

test("a text that runs up the sheet is the same box turned a quarter counter-clockwise about its anchor", () => {
  const along = schTextBox(text(), measure);
  const up = schTextBox(text({ vertical: true, angle: 90 }), measure);
  const w = along.maxX - along.minX;
  const h = along.maxY - along.minY;
  assert.equal(up.maxY - up.minY, w);
  assert.equal(up.maxX - up.minX, h);
  // left and bottom, running up: the text climbs from the anchor and hangs to its left
  assert.equal(up.maxY, 50_000);
  assert.equal(up.minY, 50_000 - w);
  assert.ok(up.maxX >= 50_000 && up.maxX - 50_000 < 400);
  // justified right and bottom it ends at the anchor and goes down
  const down = schTextBox(text({ vertical: true, angle: 90, h: "right" }), measure);
  assert.equal(down.minY, 50_000);
  assert.equal(down.maxY, 50_000 + w);
});

test("the lines of a text are an interline apart and the block is justified as a whole (FONT::getLinePositions)", () => {
  const three = (v: "top" | "center" | "bottom") => schTextLines(text({ content: "a\nbb\nccc", v }), measure);
  const lone = schTextLines(text({ content: "a", v: "bottom" }), measure)[0]!;
  const interline = interlineUm(1_270);
  assert.equal(interline, Math.round(1_270 * 1.68 * 0.9583));
  const bottom = three("bottom");
  assert.deepEqual(bottom.map((l) => l.text), ["a", "bb", "ccc"]);
  assert.equal(bottom[1]!.y - bottom[0]!.y, interline);
  assert.equal(bottom[2]!.y - bottom[1]!.y, interline);
  // bottom: the last line is where a lone line would be; top: the first one is
  assert.equal(bottom[2]!.y, lone.y);
  const loneTop = schTextLines(text({ content: "a", v: "top" }), measure)[0]!;
  assert.equal(three("top")[0]!.y, loneTop.y);
  // centre is half way between the two
  const centre = three("center");
  assert.ok(Math.abs(centre[0]!.y - (three("top")[0]!.y + bottom[0]!.y) / 2) <= 1);
  // each line is justified on its own: right ends the lines on one edge, centre puts their middles on one line
  const right = schTextLines(text({ content: "a\nbb\nccc", h: "right" }), measure);
  assert.ok(right[0]!.x > right[2]!.x, "the short line starts further right");
  const ends = right.map((l) => l.x + Math.trunc(measure(l.text, 1_270)));
  assert.ok(Math.max(...ends) - Math.min(...ends) <= 1, "they end together");
});

test("the box of several lines is taller by an interline for each line after the first", () => {
  const one = schTextBox(text({ content: "a" }), measure);
  const three = schTextBox(text({ content: "a\nb\nc" }), measure);
  assert.equal(three.maxY - three.minY, one.maxY - one.minY + 2 * interlineUm(1_270));
});
