import { test } from "node:test";
import assert from "node:assert/strict";
import { alphaOf, hasNetCollision, overlayAlphas, withAlpha } from "./schNetCollision";

test("the overlay has something to show when a net would join or a connection come apart", () => {
  assert.equal(hasNetCollision(undefined), false);
  assert.equal(hasNetCollision(null), false);
  assert.equal(hasNetCollision({ collisions: [], disconnections: [] }), false);
  assert.equal(hasNetCollision({ collisions: [{ at: [0, 0], radius_um: 1372 }], disconnections: [] }), true);
  assert.equal(hasNetCollision({ collisions: [], disconnections: [{ a: [0, 0], b: [10, 0], radius_um: 152.4 }] }), true);
});

test("the theme's drag-collision colour, #e6090dcc, is red at 0.8: the fill keeps 0.35 of that and the stroke all of it", () => {
  assert.ok(Math.abs(alphaOf("#e6090dcc") - 0.8) < 0.001);
  const a = overlayAlphas(alphaOf("#e6090dcc"));
  assert.ok(Math.abs(a.fill - 0.28) < 0.001 && Math.abs(a.stroke - 0.8) < 0.001, JSON.stringify(a));
  assert.equal(withAlpha("#e6090dcc", 0.28), "rgba(230, 9, 13, 0.28)");
  // a colour without an alpha is opaque; the alphas never go below 0.05 or above 1, and a zero alpha reads as 1
  assert.equal(alphaOf("#e6090d"), 1);
  assert.deepEqual(overlayAlphas(0), { fill: 0.35, stroke: 1 });
  assert.deepEqual(overlayAlphas(0.01), { fill: 0.05, stroke: 0.05 });
  assert.equal(withAlpha("not a colour", 0.5), "not a colour");
});
