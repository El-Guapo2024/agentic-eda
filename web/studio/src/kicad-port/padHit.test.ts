import { test } from "node:test";
import assert from "node:assert/strict";
import type { Pad } from "../api/types";
import { padCoverageArea, padCopperRings, padHitDistance } from "./padHit";
import { ringArea } from "./coverage";
import { pickSelectionCandidates, DEFAULT_SELECTION_FILTER } from "../components/canvas/selectionCandidates";
import type { BoardState, Part } from "../api/types";

const near = (a: number, b: number, tol: number, msg?: string) => assert.ok(Math.abs(a - b) <= tol, `${msg ?? ""} ${a} vs ${b}`);

// A pad as the backend sends it: the copper's centre and board box, its shape, its size un-turned, its turn (clockwise on the y-down board) and ratio.
const pad = (o: Partial<Pad>): Pad => ({ num: "1", net: null, x: 0, y: 0, w: 1_000_000, h: 600_000, round: false, th: false, ...o }) as Pad;

test("PAD::HitTest of a rounded rectangle: inside its corner arc it is hit, in the corner of its box outside the arc it is not (UI-LEFT-2 PP2)", () => {
  // 2 mm x 1 mm, ratio 0.25: corners of 0.25 mm radius
  const rr = pad({ shape: "round_rect", size: [2_000_000, 1_000_000], ratio: 0.25, w: 2_000_000, h: 1_000_000 });
  // the box corner (1.0, 0.5) mm is outside the arc: the arc's centre is (0.75, 0.25), the corner 0.354 mm from it, 0.104 mm past the radius
  near(padHitDistance(rr, 1_000_000, 500_000), Math.hypot(250_000, 250_000) - 250_000, 1, "the box corner");
  assert.ok(padHitDistance(rr, 990_000, 490_000) > 0, "just inside the box corner is off the copper");
  // a point on the diagonal inside the arc: (0.9, 0.4) is 0.212 mm from the arc's centre
  assert.ok(padHitDistance(rr, 900_000, 400_000) < 0);
  // on the straight sides the distance is the plain one
  near(padHitDistance(rr, 1_100_000, 0), 100_000, 1e-6);
  near(padHitDistance(rr, 0, 600_000), 100_000, 1e-6);
  // the old test took the box: the corner was a hit
  assert.ok(padHitDistance({ ...rr, size: undefined, shape: undefined }, 990_000, 490_000) <= 0, "without its size a pad is its box");
});

test("a turned rectangle is hit by its turned outline, not its board box", () => {
  // a 2 x 0.4 mm pad turned 45 degrees: its board box is about 1.7 mm square, its corners mostly empty
  const s = Math.SQRT1_2;
  const r = pad({ shape: "rect", size: [2_000_000, 400_000], rot: 45, w: Math.round((2_000_000 + 400_000) * s), h: Math.round((2_000_000 + 400_000) * s) });
  assert.ok(padHitDistance(r, 700_000, 700_000) <= 0, "along the turned long axis");
  assert.ok(padHitDistance(r, 700_000, -700_000) > 0, "across it, inside the box");
  near(padHitDistance(r, 0, 300_000), 300_000 * s - 200_000, 1, "the distance across the pad");
});

test("an oval turned a quarter is a stadium along y; a circle is round; a hole bigger than the copper is hit", () => {
  const ov = pad({ shape: "oval", size: [2_000_000, 1_000_000], rot: 90, w: 1_000_000, h: 2_000_000, round: true });
  assert.ok(padHitDistance(ov, 0, 900_000) < 0);
  assert.ok(padHitDistance(ov, 900_000, 0) > 0);
  const c = pad({ shape: "circle", size: [1_000_000, 1_000_000], w: 1_000_000, h: 1_000_000, round: true });
  near(padHitDistance(c, 300_000, 400_000), 0, 1e-6);
  assert.ok(padHitDistance(c, 450_000, 450_000) > 0, "the corner of its box");
  // `contains |= GetEffectiveHoleShape()->Collide( aPosition, aAccuracy )`: an NPTH pad with a 2 mm hole and a 1 mm pad
  const npth = { ...c, th: true, drill: 2_000_000, px: 0, py: 0 };
  assert.ok(padHitDistance(npth, 800_000, 0) < 0);
});

test("a chamfered rectangle and a custom pad are their copper's contours", () => {
  // a 1 mm square with its top-right corner cut at 45 degrees from (0.2, -0.5) to (0.5, -0.2)
  const outline: [number, number][][] = [
    [
      [-500_000, -500_000],
      [200_000, -500_000],
      [500_000, -200_000],
      [500_000, 500_000],
      [-500_000, 500_000],
    ],
  ];
  const ch = pad({ shape: "chamfered_rect", size: [1_000_000, 1_000_000], w: 1_000_000, h: 1_000_000, outline });
  assert.ok(padHitDistance(ch, 450_000, -450_000) > 0, "in the cut corner");
  assert.ok(padHitDistance(ch, -450_000, -450_000) <= 0, "in a whole corner");
  near(ringArea(padCopperRings(ch)[0]!), 1_000_000 ** 2 - 300_000 ** 2 / 2, 1);
});

test("GetCoverageArea of a pad is its copper's area: a rounded rectangle has its corners off, a circle is round", () => {
  const rr = pad({ shape: "round_rect", size: [2_000_000, 1_000_000], ratio: 0.25, w: 2_000_000, h: 1_000_000 });
  const exact = 2_000_000 * 1_000_000 - (4 - Math.PI) * 250_000 ** 2;
  // the arcs' chords lie outside them (`ERROR_OUTSIDE`): a little more, never less
  const a = padCoverageArea(rr);
  assert.ok(a >= exact && a < exact * 1.01, `${a} vs ${exact}`);
  const c = pad({ shape: "circle", size: [1_000_000, 1_000_000], w: 1_000_000, h: 1_000_000, round: true });
  const ca = padCoverageArea(c);
  // twelve chords outside a 0.5 mm radius (`GetArcToSegmentCount`, 0.02 mm): `12 tan( 15 deg )` of r squared, 2.4 % more than the circle
  near(ca, 12 * Math.tan(Math.PI / 12) * 500_000 ** 2, 1e6);
});

test("a click in the corner of a rounded pad's box picks the footprint, not the pad (selectPoint over PAD::HitTest)", () => {
  const rr = pad({ num: "1", shape: "round_rect", size: [2_000_000, 1_000_000], ratio: 0.25, w: 2_000_000, h: 1_000_000 });
  const part = { ref: "U1", value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [6_000_000, 4_000_000], at: [0, 0], rot: 0, side: "top", label: "above", courtyard: [-3_000_000, -2_000_000, 3_000_000, 2_000_000], pads: [rr] } as unknown as Part;
  const board = { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [part], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle" } as unknown as BoardState;
  // 10 nm a pixel: a tolerance of 150 nm, so only an exact hit counts
  const at = (x: number, y: number) => pickSelectionCandidates(board, x, y, 150, 10, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false).map((c) => c.kind);
  assert.deepEqual(at(990_000, 490_000), ["part"], "outside the corner arc");
  assert.deepEqual(at(900_000, 400_000), ["pad"], "inside it");
});
