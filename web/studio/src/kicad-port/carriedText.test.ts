import { test } from "node:test";
import assert from "node:assert/strict";
import type { CmdText } from "../api/types";
import { flipCarriedText, flipTextLayer, rotateCarriedText } from "./carriedText";

const text: CmdText = { content: "T", at: { x: 1_000_000, y: 2_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "left", mirror: false };

test("R and Shift+R on the carried text turn it about its own position (PCB_TEXT::Rotate, EDIT_TOOL::Rotate's one-item reference point)", () => {
  assert.deepEqual(rotateCarriedText(text, 90), { ...text, angle: 90_000 });
  assert.equal(rotateCarriedText(text, -90).angle, 270_000, "EDA_ANGLE::Normalize");
  assert.equal(rotateCarriedText({ ...text, angle: 315_000 }, 45).angle, 0);
  assert.deepEqual(rotateCarriedText(text, 90).at, text.at);
});

test("F on the carried text flips it to the other side, mirrored, its angle negated (PCB_TEXT::Flip)", () => {
  const flipped = flipCarriedText({ ...text, angle: 30_000 }, "left_right", 2);
  assert.equal(flipped.layer, "B.SilkS");
  assert.equal(flipped.mirror, true);
  assert.equal(flipped.angle, 330_000, "-30 degrees, kept in [0, 360)");
  assert.deepEqual(flipped.at, text.at);
  assert.equal(flipCarriedText({ ...text, angle: 30_000 }, "top_bottom", 2).angle, 150_000, "ANGLE_180 - angle");
  // a user layer is not side specific: it keeps its mirror and its layer
  const user = flipCarriedText({ ...text, layer: "User.1" }, "left_right", 2);
  assert.equal(user.layer, "User.1");
  assert.equal(user.mirror, false);
});

test("BOARD::FlipLayer: the inner copper layers mirror about the middle of the stackup", () => {
  assert.equal(flipTextLayer("In1.Cu", 6), "In4.Cu");
  assert.equal(flipTextLayer("In2.Cu", 6), "In3.Cu");
  assert.equal(flipTextLayer("In1.Cu", 2), "In1.Cu");
  assert.equal(flipTextLayer("F.Cu", 4), "B.Cu");
  assert.equal(flipTextLayer("Edge.Cuts", 4), "Edge.Cuts");
});
