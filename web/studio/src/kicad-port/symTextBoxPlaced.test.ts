import { test } from "node:test";
import assert from "node:assert/strict";
import { symbolTransformMatrix } from "../components/schematic/transform";
import { itemOrientationOf, orientedText, isVerticalAngle, type ItemOrientation } from "./symTextBoxPlaced";

const o = (nRots: 0 | 1 | 2 | 3, mirrorX: boolean, mirrorY: boolean): ItemOrientation => ({ nRots, mirrorX, mirrorY });

test("each placed transform is the first orientation `SCH_SYMBOL::GetOrientation` finds for it, with its `ORIENT_MIRROR` entry", () => {
  for (const [rot, want] of [[0, o(0, false, false)], [90, o(1, false, false)], [180, o(2, false, false)], [270, o(3, false, false)]] as const) {
    assert.deepEqual(itemOrientationOf(symbolTransformMatrix(rot, null)), want, `turned ${rot}`);
  }
  // mirrored top to bottom (KiCad's mirror X) after each turn: "mirror X + 0 / 90 / 270" are found as such, "+ 180" is "mirror Y"
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(0, "x")), o(0, true, false));
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(90, "x")), o(1, true, false));
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(180, "x")), o(0, false, true));
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(270, "x")), o(3, true, false));
  // mirrored left to right (KiCad's mirror Y): only the unturned one is done as a left-right mirror, the others as a top-bottom one after their turns
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(0, "y")), o(0, false, true));
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(90, "y")), o(3, true, false));
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(180, "y")), o(0, true, false));
  assert.deepEqual(itemOrientationOf(symbolTransformMatrix(270, "y")), o(1, true, false));
});

test("a turn runs a text box's text the other way and a mirror flips the justification of the way it then runs (`SCH_TEXTBOX::Rotate` / `MirrorHorizontally` / `MirrorVertically`)", () => {
  const text = (rot: number, mirror: "x" | "y" | null, vertical: boolean, h: "left" | "center" | "right") => orientedText(vertical, h, itemOrientationOf(symbolTransformMatrix(rot, mirror)));
  assert.deepEqual(text(90, null, false, "left"), { vertical: true, hAlign: "left" });
  assert.deepEqual(text(180, null, false, "left"), { vertical: false, hAlign: "left" }, "a half turn reads left to right from the left still");
  assert.deepEqual(text(0, "y", false, "left"), { vertical: false, hAlign: "right" }, "mirrored left to right: a horizontal text's left is its right");
  assert.deepEqual(text(0, "y", true, "left"), { vertical: true, hAlign: "left" });
  assert.deepEqual(text(0, "x", false, "left"), { vertical: false, hAlign: "left" }, "mirrored top to bottom: a horizontal text keeps its justification");
  assert.deepEqual(text(0, "x", true, "left"), { vertical: true, hAlign: "right" });
  // a quarter turn mirrored left to right is "mirror X + 270": three turns make the text vertical, the top-bottom mirror flips it
  assert.deepEqual(text(90, "y", false, "left"), { vertical: true, hAlign: "right" });
  assert.deepEqual(text(90, "y", false, "center"), { vertical: true, hAlign: "center" });
  assert.deepEqual([0, 90, 180, 270, -90].map(isVerticalAngle), [false, true, false, true, true]);
});
