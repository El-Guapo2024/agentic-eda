import { test } from "node:test";
import assert from "node:assert/strict";
import { fallbackBodyHeightMm, FALLBACK_MAX_HEIGHT_MM, FALLBACK_MIN_HEIGHT_MM, partBodyBoxUm } from "./partBody";

test("the fallback box is the F.Fab body when the server knows it, else the courtyard", () => {
  const courtyard = [0, 0, 3000, 1400] as const;
  const body = [700, 300, 2300, 1100] as const;
  assert.deepEqual(partBodyBoxUm({ body, courtyard }), body, "a 1.6 x 0.8 mm body, not the 3 x 1.4 mm courtyard around it");
  assert.deepEqual(partBodyBoxUm({ body: null, courtyard }), courtyard);
  assert.deepEqual(partBodyBoxUm({ courtyard }), courtyard);
  assert.equal(partBodyBoxUm({}), null, "an unplaced part has no box");
});

test("the fallback height follows the body's narrower side, kept between 0.4 and 2 mm", () => {
  assert.equal(fallbackBodyHeightMm(1.6, 0.8), FALLBACK_MIN_HEIGHT_MM, "a 0603 stays flat");
  assert.equal(fallbackBodyHeightMm(1.0, 0.5), FALLBACK_MIN_HEIGHT_MM, "a 0402 too");
  assert.equal(fallbackBodyHeightMm(3.9, 9.9), 1.95, "a SOIC-16");
  assert.equal(fallbackBodyHeightMm(2.54, 10.16), 1.27, "a 1x04 header");
  assert.equal(fallbackBodyHeightMm(20, 20), FALLBACK_MAX_HEIGHT_MM, "a module is capped");
  assert.equal(fallbackBodyHeightMm(10.16, 2.54), 1.27, "either order");
});
