import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { collectBoxSelection, DEFAULT_SELECTION_FILTER, padHitDistance, pickSelectionCandidates } from "../components/canvas/selectionCandidates";
import { pcbItemBoxes } from "./itemBoxes";

const pad = (num: string, x: number, y: number, w = 600, h = 600, round = false, th = false): Pad => ({ num, net: null, x, y, w, h, round, th });
const part = (ref: string, at: [number, number], pads: Pad[], side: "top" | "bottom" = "top"): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [4000, 2000], at, rot: 0, side, label: "above", courtyard: [at[0] - 2000, at[1] - 1000, at[0] + 2000, at[1] + 1000], pads }) as Part;
const board = (parts: Part[], extra: Partial<BoardState> = {}): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts, rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...extra }) as BoardState;

const u1 = part("U1", [0, 0], [pad("1", -1000, 0), pad("2", 1000, 0, 1200, 600, true), pad("3", 0, 800, 600, 600, true)]);

test("padHitDistance: inside a pad is not outside it, beside it is the gap, a round pad is round", () => {
  assert.ok(padHitDistance(pad("1", 0, 0), 100, 100) <= 0);
  assert.equal(padHitDistance(pad("1", 0, 0), 400, 0), 100, "the pad ends at 300");
  assert.equal(padHitDistance(pad("1", 0, 0), 400, 400), Math.hypot(100, 100), "off the corner of a rectangle");
  // a circle of diameter 600: the corner of its box is outside it
  assert.ok(padHitDistance(pad("1", 0, 0, 600, 600, true), 290, 290) > 100);
  assert.ok(padHitDistance(pad("1", 0, 0, 600, 600, true), 200, 0) < 0);
  // an oval 1200 x 600 reaches 600 along its long axis and 300 across it
  assert.ok(padHitDistance(pad("1", 0, 0, 1200, 600, true), 550, 0) < 0);
  assert.ok(padHitDistance(pad("1", 0, 0, 1200, 600, true), 0, 350) > 0);
});

test("a click on a pad picks the pad over the footprint it sits in", () => {
  const b = board([u1]);
  const hit = pickSelectionCandidates(b, -1000, 0, 150, 10, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false);
  assert.deepEqual(hit.map((c) => [c.kind, c.id]), [["pad", "U1.1"]]);
  // beside the pads, inside the courtyard: the footprint
  const body = pickSelectionCandidates(b, 0, -500, 150, 10, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false);
  assert.deepEqual(body.map((c) => [c.kind, c.id]), [["part", "U1"]]);
});

test("the Pads filter keeps pads out of a click", () => {
  const b = board([u1]);
  const hit = pickSelectionCandidates(b, -1000, 0, 150, 10, { ...DEFAULT_SELECTION_FILTER, pads: false }, {}, null, false, new Set(), false, false);
  assert.deepEqual(hit.map((c) => c.kind), ["part"]);
});

test("a pad of a locked footprint is locked with it", () => {
  const b = board([u1], { locked: ["U1"] });
  assert.deepEqual(pickSelectionCandidates(b, -1000, 0, 150, 10, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false), []);
  const allowed = pickSelectionCandidates(b, -1000, 0, 150, 10, { ...DEFAULT_SELECTION_FILTER, lockedItems: true }, {}, null, false, new Set(), false, false);
  assert.deepEqual(allowed.map((c) => c.id), ["U1.1"]);
});

test("a pad on a hidden copper layer cannot be picked, a through-hole pad on any layer can", () => {
  const smd = part("U1", [0, 0], [pad("1", 0, 0)]);
  const th = part("J1", [10_000, 0], [pad("1", 10_000, 0, 600, 600, false, true)]);
  const b = board([smd, th]);
  const hidden = { "F.Cu": false };
  assert.deepEqual(pickSelectionCandidates(b, 0, 0, 150, 10, DEFAULT_SELECTION_FILTER, hidden, null, false, new Set(), false, true).filter((c) => c.kind === "pad"), []);
  assert.deepEqual(pickSelectionCandidates(b, 10_000, 0, 150, 10, DEFAULT_SELECTION_FILTER, hidden, null, false, new Set(), false, true).filter((c) => c.kind === "pad").map((c) => c.id), ["J1.1"]);
});

test("a bottom footprint's pads are on the back", () => {
  const b = board([part("U1", [0, 0], [pad("1", 0, 0)], "bottom")]);
  const hit = pickSelectionCandidates(b, 0, 0, 150, 10, DEFAULT_SELECTION_FILTER, {}, "B.Cu", false, new Set(), false, true).find((c) => c.kind === "pad");
  assert.equal(hit?.layer, "B.Cu");
});

test("a box takes the footprints in it and leaves their pads out; only a box with nothing else takes pads", () => {
  const b = board([u1]);
  const everything = collectBoxSelection(b, [-5000, -3000, 5000, 3000], false, DEFAULT_SELECTION_FILTER, {}, null, false);
  assert.deepEqual(everything.map((h) => h.id), ["U1"]);
  // a small box around the first pad only
  const onePad = collectBoxSelection(b, [-1400, -400, -600, 400], false, DEFAULT_SELECTION_FILTER, {}, null, false);
  assert.deepEqual(onePad.map((h) => [h.kind, h.id]), [["pad", "U1.1"]]);
  // footprints off: the pads in the big box
  const padsOnly = collectBoxSelection(b, [-5000, -3000, 5000, 3000], false, { ...DEFAULT_SELECTION_FILTER, footprints: false }, {}, null, false);
  assert.deepEqual(padsOnly.map((h) => h.id).sort(), ["U1.1", "U1.2", "U1.3"]);
});

test("pads of one number repeat with #k, so each is its own pick", () => {
  const b = board([part("U1", [0, 0], [pad("1", -1000, 0), pad("1", 1000, 0)])]);
  const hit = pickSelectionCandidates(b, 1000, 0, 150, 10, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false);
  assert.deepEqual(hit.map((c) => c.id), ["U1.1#2"]);
});

test("pcbItemBoxes: every pad has its own box, so Zoom to Selection and the lasso find a selected pad", () => {
  const boxes = pcbItemBoxes(board([u1]));
  assert.deepEqual(boxes.get("U1.1"), [-1300, -300, -700, 300]);
  assert.deepEqual(boxes.get("U1.2"), [400, -300, 1600, 300]);
  assert.ok(boxes.has("U1"), "the footprint keeps its own box");
});
