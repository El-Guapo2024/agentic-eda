import { test } from "node:test";
import assert from "node:assert/strict";
import { COLOR_BANDS, changeLightness, eseriesColumnColours, eseriesGrid112, eseriesGrid2496, showFourthBand } from "./colorCode";
import { E12, E24, E96, eseriesValues } from "./eseries";

test("the color code chart: twelve bands, the 4th band only for <= 2 %", () => {
  assert.equal(COLOR_BANDS.length, 12);
  assert.deepEqual(COLOR_BANDS.map((b) => b.digit).slice(0, 10), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  assert.equal(COLOR_BANDS[10]!.tolerance, "± 5%");
  assert.equal(showFourthBand(0), false);
  assert.equal(showFourthBand(1), true);
});

test("ChangeLightness: 125 blends 25 % toward white, 78 22 % toward black (hand-computed from wxColourBase::AlphaBlend)", () => {
  // E3 0x98fb98 is BGR: rgb(152, 251, 152); 255 + 0.75 * (152 - 255) = 177.75 -> 177, 255 + 0.75 * (251 - 255) = 252
  assert.deepEqual(eseriesColumnColours("E3", false), ["rgb(152, 251, 152)", "rgb(177, 252, 177)"]);
  // 0.78 * 152 = 118.56 -> 118, 0.78 * 251 = 195.78 -> 195
  assert.equal(eseriesColumnColours("E3", true)[0], "rgb(118, 195, 118)");
  assert.deepEqual(changeLightness([10, 20, 30], 100), [10, 20, 30]);
});

test("the E-Series grids: E1/E3/E6/E12 spans and the four stripes of E24/E48/E96", () => {
  const g = eseriesGrid112(eseriesValues(E12));
  assert.deepEqual(g.map((c) => c.map((x) => x.text)), [["100"], ["100", "220", "470"], ["100", "150", "220", "330", "470", "680"], eseriesValues(E12).map(String)]);
  assert.deepEqual(g[1]!.map((x) => x.rowSpan), [4, 4, 4]);
  const h = eseriesGrid2496(eseriesValues(E24), eseriesValues(E96));
  assert.equal(h.length, 15);
  assert.equal(h[3], "gap");
  const e24 = h[4] as { text: string }[];
  assert.deepEqual(e24.map((x) => x.text), ["180", "200", "220", "240", "270", "300"]);
  assert.equal((h[2] as unknown[]).length, 24);
  assert.equal((h[14] as { text: string }[])[23]!.text, "976");
});
