import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibraryPad } from "../api/types";
import { flipCarriedPad, rotateCarriedPad } from "./fpPadPlacer";

const pad = (over: Partial<LibraryPad> = {}): LibraryPad => ({
  number: "1",
  at: { x: 0, y: 0 },
  offset: { x: 100, y: 200 },
  size: [1000, 500],
  shape: "trapezoid",
  kind: "smd",
  drill: null,
  drill_slot: null,
  rot: 30_000,
  roundrect_ratio: null,
  trapezoid_delta: [0, 40],
  chamfer_ratio: null,
  chamfer_corners: { top_left: true, top_right: false, bottom_left: false, bottom_right: true },
  layers: ["F.Cu", "F.Paste", "F.Mask"],
  clearance_override: null,
  thermal_gap_override: null,
  thermal_spoke_width_override: null,
  ...over,
});

test("R before the click turns the carried pad by the rotation step (PAD::Rotate about its own position)", () => {
  assert.equal(rotateCarriedPad(pad(), 90).rot, 120_000);
  assert.equal(rotateCarriedPad(pad({ rot: 0 }), -90).rot, 270_000);
  assert.equal(rotateCarriedPad(pad({ rot: 300_000 }), 90).rot, 30_000);
});

test("F before the click flips the carried pad as PAD::Flip does, left/right", () => {
  const f = flipCarriedPad(pad(), "left_right")!;
  assert.deepEqual(f.offset, { x: -100, y: 200 });
  assert.deepEqual(f.trapezoid_delta, [0, 40]);
  assert.equal(f.rot, 330_000);
  assert.deepEqual(f.chamfer_corners, { top_left: false, top_right: true, bottom_left: true, bottom_right: false });
  assert.deepEqual(f.layers, ["B.Cu", "B.Paste", "B.Mask"]);
});

test("F flips top/bottom when the preference says so", () => {
  const f = flipCarriedPad(pad({ trapezoid_delta: [40, 0] }), "top_bottom")!;
  assert.deepEqual(f.offset, { x: 100, y: -200 });
  assert.deepEqual(f.trapezoid_delta, [40, 0]);
  assert.deepEqual(f.chamfer_corners, { top_left: false, top_right: true, bottom_left: true, bottom_right: false });
  const g = flipCarriedPad(pad({ trapezoid_delta: [0, 40] }), "top_bottom")!;
  assert.deepEqual(g.trapezoid_delta, [0, -40]);
  assert.deepEqual(flipCarriedPad(pad({ layers: ["*.Cu", "*.Mask"] }), "left_right")!.layers, ["*.Cu", "*.Mask"]);
});

test("a custom pad's basic shapes are flipped too (an arc's ends swap); a padstack pad is not flipped here", () => {
  const custom = pad({
    shape: "custom",
    custom: { anchor: "rect", zone: "outline", primitives: [{ shape: { kind: "arc", layer: "F.Cu", stroke_width: 1, filled: false, start: { x: 1, y: 2 }, mid: { x: 3, y: 4 }, end: { x: 5, y: 6 } } }] },
  });
  const f = flipCarriedPad(custom, "left_right")!;
  assert.deepEqual(f.custom!.primitives[0]!.shape, { kind: "arc", layer: "B.Cu", stroke_width: 1, filled: false, start: { x: -5, y: 6 }, mid: { x: -3, y: 4 }, end: { x: -1, y: 2 } });
  assert.equal(flipCarriedPad(pad({ stack: { mode: "front_inner_back", back: { shape: "circle", size: [1, 1] } } }), "left_right"), null);
});
