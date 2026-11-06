import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, FpTextInfo, Part } from "../api/types";
import { DEFAULT_SELECTION_FILTER, pickSelectionCandidates, pickToolCandidates } from "../components/canvas/selectionCandidates";
import { artTextAsField } from "./fpArt";
import { fieldStrokes, strokeDistance, textHitSlop } from "./textHit";

// U1: a 8 x 8 mm courtyard, its fab reference "U1" (1 mm, a 0.15 mm pen) at its centre, a pad at its left edge.
const ref: FpTextInfo = { id: "U1~t0", text: "U1", x: 0, y: 0, angle: 0, w: 1_000_000, h: 1_000_000, thickness: 150_000, layer: "F.Fab", halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: true, knockout: false };
const part = {
  ref: "U1", value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [8_000_000, 8_000_000], at: [0, 0], rot: 0, side: "top", label: "above",
  courtyard: [-4_000_000, -4_000_000, 4_000_000, 4_000_000],
  pads: [{ num: "1", x: -3_500_000, y: 0, w: 600_000, h: 1_500_000, round: false, net: null, th: false }],
  texts: [ref],
} as unknown as Part;
const board = { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [part], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle" } as unknown as BoardState;
const pick = (x: number, y: number, onePixelNm = 10_000) => pickSelectionCandidates(board, x, y, 150_000, onePixelNm, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), false, false).map((c) => c.id);

test("hitTestDistance of a footprint's text: its strokes, where KiCad lays them out; an exact hit within half MAX_SLOP pixels, none past MAX_SLOP", () => {
  const s = fieldStrokes(artTextAsField(ref));
  assert.ok(s.segs.length > 4 && s.pen === 150_000);
  // the first stroke of the U, its middle
  const [x0, y0, x1, y1] = s.segs[0]!;
  const [mx, my] = [(x0 + x1) / 2, (y0 + y1) / 2];
  assert.equal(strokeDistance(s, mx, my), 0);
  assert.equal(textHitSlop(s, mx, my, 10_000), 0);
  // 40 um off the pen's edge: sloppy by 40 - 25 um; 60 um off: no hit (`INT_MAX`)
  const off = (d: number) => { const len = Math.hypot(x1 - x0, y1 - y0); return [mx - ((y1 - y0) / len) * (75_000 + d), my + ((x1 - x0) / len) * (75_000 + d)] as const; };
  const [ax, ay] = off(40_000);
  assert.ok(Math.abs(textHitSlop(s, ax, ay, 10_000) - 15_000) < 1_000, String(textHitSlop(s, ax, ay, 10_000)));
  const [bx, by] = off(60_000);
  assert.equal(textHitSlop(s, bx, by, 10_000), Infinity);
});

test("UI-LEFT P15: a click on the fab reference's strokes picks the text; one in its box off the strokes picks the footprint (GuessSelectionCandidates: exact hits first)", () => {
  const s = fieldStrokes(artTextAsField(ref));
  const [x0, y0, x1, y1] = s.segs[0]!;
  assert.deepEqual(pick((x0 + x1) / 2, (y0 + y1) / 2), ["U1~t0"]);
  // the middle of the U's bowl: in the text's box, 0.2 mm from any stroke (KiCad's layout: the U from 0.238 to 0.81 of the size past -1.0 mm, half of "U1"'s 2.0 mm advance)
  const inBowl: [number, number] = [-476_000, -200_000];
  assert.ok(strokeDistance(s, ...inBowl) > 150_000, String(strokeDistance(s, ...inBowl)));
  assert.deepEqual(pick(...inBowl), ["U1"]);
  // zoomed out, 5 pixels reach the strokes: the text again
  assert.deepEqual(pick(...inBowl, 80_000), ["U1~t0"]);
});

test("the edit tools' client filter (FilterCollectorForFreePads, FilterCollectorForHierarchy( aCollector, true )): over the text or a pad, the footprint", () => {
  const s = fieldStrokes(artTextAsField(ref));
  const [x0, y0, x1, y1] = s.segs[0]!;
  const tool = (x: number, y: number) => pickToolCandidates(board, x, y, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set()).map((c) => [c.kind, c.id]);
  assert.deepEqual(tool((x0 + x1) / 2, (y0 + y1) / 2), [["part", "U1"]]);
  // a click there takes the pad; the tools take its footprint
  assert.deepEqual(pick(-3_500_000, 0), ["U1.1"]);
  assert.deepEqual(tool(-3_500_000, 0), [["part", "U1"]]);
});

test("the strokes are where FONT::getLinePositions puts them: a centred line on half its advance, the baseline 0.415 of the size less 0.052 of the pen below the anchor", () => {
  const s = fieldStrokes(artTextAsField(ref));
  // the U's first stroke: x = -2.0 / 2 + 0.238 mm (kicad-cli's plot centres "H" on its whole advance, kicadText.test.ts); from 0.952 to 0.143 of the size above the baseline
  const [x0, y0, , y1] = s.segs[0]!;
  const baseline = 415_000 - 0.052 * 150_000;
  assert.ok(Math.abs(x0 - (-1_000_000 + (5 / 21) * 1_000_000)) < 2, String(x0));
  assert.ok(Math.abs(y0 - (baseline - (20 / 21) * 1_000_000)) < 2 && Math.abs(y1 - (baseline - (3 / 21) * 1_000_000)) < 2, `${y0} ${y1}`);
  // at the anchor of "U1" the U's right stroke is 0.19 mm off, 0.115 mm past the pen's edge: a sloppy hit within the 5 pixels of 0.032 mm (0.035 mm past half of them)
  assert.ok(Math.abs(textHitSlop(s, 0, 0, 32_000) - 35_476) < 2, String(textHitSlop(s, 0, 0, 32_000)));
});
