import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibraryPad, PadCustom } from "../api/types";
import { padBox } from "./itemBoxes";
import { padContains } from "./libEditorHit";

const NO_CORNERS = { top_left: false, top_right: false, bottom_left: false, bottom_right: false };

function pad(over: Partial<LibraryPad>): LibraryPad {
  return {
    id: "p1",
    number: "1",
    at: { x: 0, y: 0 },
    offset: { x: 0, y: 0 },
    size: [800, 800],
    shape: "rect",
    kind: "smd",
    drill: null,
    drill_slot: null,
    rot: 0,
    roundrect_ratio: null,
    trapezoid_delta: null,
    chamfer_ratio: null,
    chamfer_corners: NO_CORNERS,
    layers: ["F.Cu"],
    clearance_override: null,
    thermal_gap_override: null,
    thermal_spoke_width_override: null,
    ...over,
  };
}

const arm: PadCustom = {
  anchor: "circle",
  zone: "outline",
  primitives: [
    { shape: { kind: "segment", layer: "", stroke_width: 200, filled: false, start: { x: 0, y: 0 }, end: { x: 3000, y: 0 } } },
    { shape: { kind: "circle", layer: "", stroke_width: 0, filled: true, center: { x: 0, y: 2000 }, end: { x: 300, y: 2000 } }, proxy: true },
  ],
};

test("a click lands on a custom pad where its anchor or one of its copper shapes is, not on a proxy item", () => {
  const p = pad({ shape: "custom", custom: arm });
  assert.equal(padContains(p, 300, 0), true, "on the circular anchor");
  assert.equal(padContains(p, 2000, 80), true, "on the arm, within half its width");
  assert.equal(padContains(p, 2000, 150), false, "beside the arm");
  assert.equal(padContains(p, 0, 2000), false, "a proxy circle is not copper");
  assert.equal(padContains(pad({ shape: "custom", custom: null }), 300, 0), true, "no data: the anchor circle");
});

test("a custom pad is hit where it is on the footprint: turned with the pad and moved by its offset", () => {
  const p = pad({ shape: "custom", custom: arm, at: { x: 10_000, y: 0 }, rot: 90_000, offset: { x: 0, y: 0 } });
  assert.equal(padContains(p, 10_000, 2000), true, "the arm points along +y after a quarter turn");
  assert.equal(padContains(p, 12_000, 0), false);
  const shifted = pad({ shape: "custom", custom: arm, offset: { x: 0, y: 1000 } });
  assert.equal(padContains(shifted, 0, 1000), true, "the anchor sits at the offset");
  assert.equal(padContains(shifted, 0, 0), false);
});

test("a rectangular custom base is hit as its rectangle and the pad box holds the shapes", () => {
  const p = pad({ shape: "custom", size: [1000, 400], custom: { ...arm, anchor: "rect" } });
  assert.equal(padContains(p, 450, 150), true);
  assert.equal(padContains(p, 450, 250), false);
  assert.deepEqual(padBox(p).map(Math.round), [-500, -200, 3100, 200]);
  assert.deepEqual(padBox(pad({ size: [1000, 400] })).map(Math.round), [-500, -200, 500, 200], "an ordinary pad's box is its size");
});
