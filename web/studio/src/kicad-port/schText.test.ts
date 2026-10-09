import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultPenUm, textOrigin } from "./schText";

// The numbers are `kicad_text_segments`' for 1.27 mm text (crates/drc/src/stroke_font.rs): a pen of 159 um, a line 1486 um tall.
test("a 1.27 mm text is written with a pen an eighth of its size", () => {
  assert.equal(defaultPenUm(1270), 159);
});

test("the baseline of centred left-justified text starts a bit in from the anchor and a little under it", () => {
  const [dx, dy] = textOrigin(2000, 1270, 159, "left", "center");
  assert.equal(dx, Math.trunc(159 / 1.52));
  assert.equal(dy, Math.trunc(1270 - 159 * 0.052 - Math.trunc((1270 * 1.17) / 2)));
});

test("centred text starts half its width back; right-justified text ends at the anchor less the pen's offset", () => {
  assert.equal(textOrigin(2000, 1270, 159, "center", "center")[0], -1000);
  assert.equal(textOrigin(2000, 1270, 159, "right", "center")[0], Math.trunc(-(2000 + 159 / 1.52)));
});

test("top-justified text hangs from the anchor, bottom-justified text stands on it", () => {
  const top = textOrigin(1000, 1270, 159, "left", "top")[1];
  const centre = textOrigin(1000, 1270, 159, "left", "center")[1];
  const bottom = textOrigin(1000, 1270, 159, "left", "bottom")[1];
  assert.ok(top > centre && centre > bottom, `${top} ${centre} ${bottom}`);
  assert.ok(Math.abs(top - bottom - Math.trunc(1270 * 1.17)) <= 1, `${top - bottom}`);
});
