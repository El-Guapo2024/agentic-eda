import { test } from "node:test";
import assert from "node:assert/strict";
import { brighten, darken, netHighlightColor, hexToRgb, rgbToHex, HIGHLIGHT_FACTOR } from "./netHighlight";

test("brighten: color4d.h Brighten formula, r' = r*(1-f) + 255*f", () => {
  assert.deepEqual(brighten({ r: 0, g: 0, b: 0 }, 0.5), { r: 128, g: 128, b: 128 });
  assert.deepEqual(brighten({ r: 200, g: 200, b: 200 }, 0), { r: 200, g: 200, b: 200 }, "factor 0 is a no-op");
  assert.deepEqual(brighten({ r: 0, g: 0, b: 0 }, 1), { r: 255, g: 255, b: 255 }, "factor 1 goes fully white");
});

test("darken: color4d.h Darken formula, r' = r*(1-f)", () => {
  assert.deepEqual(darken({ r: 200, g: 100, b: 50 }, 0.5), { r: 100, g: 50, b: 25 });
  assert.deepEqual(darken({ r: 200, g: 100, b: 50 }, 0), { r: 200, g: 100, b: 50 }, "factor 0 is a no-op");
  assert.deepEqual(darken({ r: 200, g: 100, b: 50 }, 1), { r: 0, g: 0, b: 0 }, "factor 1 goes fully black");
});

test("netHighlightColor: on-net brightens, off-net darkens, both by the real m_highlightFactor (0.5)", () => {
  const base = { r: 200, g: 150, b: 60 };
  assert.deepEqual(netHighlightColor(base, true), brighten(base, HIGHLIGHT_FACTOR));
  assert.deepEqual(netHighlightColor(base, false), darken(base, HIGHLIGHT_FACTOR));
  assert.notDeepEqual(netHighlightColor(base, true), netHighlightColor(base, false));
});

test("hexToRgb / rgbToHex round-trip, including 3-digit shorthand", () => {
  assert.deepEqual(hexToRgb("#ff8800"), { r: 255, g: 136, b: 0 });
  assert.deepEqual(hexToRgb("#f80"), { r: 255, g: 136, b: 0 });
  assert.equal(rgbToHex({ r: 255, g: 136, b: 0 }), "#ff8800");
});
