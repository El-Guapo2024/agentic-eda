import { test } from "node:test";
import assert from "node:assert/strict";
import type { Pad } from "../api/types";
import { arcToSegmentCount, circlePolygon, contourArea, nestContours, ovalPolygon, padContours, padPlacement, padShapeLocal, roundChamferedRectPolygon, toBoard, trapezoidPolygon, type Contour } from "./padShape3d";

const area = (c: Contour) => Math.abs(contourArea(c));
const bbox = (c: Contour) => {
  const xs = c.map((p) => p[0]);
  const ys = c.map((p) => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)].map((v) => Math.round(v));
};
const near = (a: number, b: number, tol: number, msg?: string) => assert.ok(Math.abs(a - b) <= tol, `${msg ?? ""} ${a} vs ${b}`);
/** The box of a polygon whose curves have their error inside (`ERROR_INSIDE`): within the 5 um error of the exact box, never outside it. */
const nearBox = (c: Contour, want: number[], msg?: string) => {
  const got = bbox(c);
  got.forEach((v, i) => assert.ok(Math.abs(v - want[i]!) <= 5000 && (i < 2 ? v >= want[i]! : v <= want[i]!), `${msg ?? ""} ${JSON.stringify(got)} vs ${JSON.stringify(want)}`));
};

test("GetArcToSegmentCount and TransformCircleToPolygon: segments for the error, a multiple of 8, the vertices on the circle", () => {
  // 0.22 mm at 5 um: acos( 1 - 5/220 ) * 2 = 24.5 degrees a segment, 15 of them; rounded up to 16.
  assert.equal(arcToSegmentCount(220_000, 5000), 15);
  const c = circlePolygon([0, 0], 220_000, 5000);
  assert.equal(c.length, 16);
  for (const [x, y] of c) near(Math.hypot(x, y), 220_000, 1);
  // Half a step off the x axis: the first vertex at 11.25 degrees, below the axis in KiCad's y-down `RotatePoint`.
  near(c[0]![0], 220_000 * Math.cos((11.25 * Math.PI) / 180), 1);
  near(c[0]![1], -220_000 * Math.sin((11.25 * Math.PI) / 180), 1);
  // A small circle still has the pad's minimum (16).
  assert.equal(circlePolygon([0, 0], 10_000, 5000, 16).length, 16);
});

test("TransformOvalToPolygon: a stadium as wide as asked, along the segment", () => {
  const o = ovalPolygon([-500_000, 0], [500_000, 0], 600_000, 5000);
  nearBox(o, [-800_000, -300_000, 800_000, 300_000]);
  // The area of a 1 mm x 0.6 mm rectangle and a 0.6 mm disc (less the polygon's error).
  const exact = 1_000_000 * 600_000 + Math.PI * 300_000 ** 2;
  assert.ok(area(o) < exact && area(o) > exact * 0.99, `${area(o)}`);
  // Turned: along y.
  nearBox(ovalPolygon([0, -500_000], [0, 500_000], 600_000, 5000), [-300_000, -800_000, 300_000, 800_000]);
});

test("TransformTrapezoidToPolygon: the slanted sides KiCad's delta gives, and a rectangle inflated with round corners", () => {
  // A 2 x 1 mm pad with a y delta of 0.8 mm (half: 0.4): its side at +y is 2.8 mm long, the one at -y 1.2 mm (TransformTrapezoidToPolygon's corners), 2 mm^2.
  const t = trapezoidPolygon([2_000_000, 1_000_000], 0, 400_000, 0, 5000);
  assert.equal(t.length, 4);
  assert.deepEqual(
    t.map(([x, y]) => [x, y]),
    [
      [-600_000, -500_000],
      [600_000, -500_000],
      [1_400_000, 500_000],
      [-1_400_000, 500_000],
    ]
  );
  near(area(t), 2_000_000_000_000, 1);
  // A rectangle with an inflate: `CornerListToPolygon( .., aInflate )`, the size grown and each corner rounded by the inflate.
  const r = trapezoidPolygon([1_000_000, 600_000], 0, 0, 100_000, 5000);
  assert.deepEqual(bbox(r), [-600_000, -400_000, 600_000, 400_000]);
  const exact = 1_200_000 * 800_000 - (4 - Math.PI) * 100_000 ** 2;
  assert.ok(area(r) <= exact + 1 && area(r) > exact * 0.995, `${area(r)} vs ${exact}`);
  // A negative inflate shrinks the rectangle, its corners kept square.
  assert.deepEqual(bbox(trapezoidPolygon([1_000_000, 600_000], 0, 0, -100_000, 5000)), [-400_000, -200_000, 400_000, 200_000]);
});

test("TransformRoundChamferedRectToPolygon: rounded corners of the ratio, and the chamfered corners cut", () => {
  // 1 x 0.6 mm at KiCad's 25 %: corners of 0.15 mm.
  const rr = roundChamferedRectPolygon([1_000_000, 600_000], 150_000, 0, 0, 0, 5000);
  assert.deepEqual(bbox(rr), [-500_000, -300_000, 500_000, 300_000]);
  const exact = 1_000_000 * 600_000 - (4 - Math.PI) * 150_000 ** 2;
  // Each corner's quarter turn is 4 of the arc's 16 segments, inside the arc.
  assert.ok(area(rr) <= exact + 1 && area(rr) > exact * 0.995, `${area(rr)} vs ${exact}`);
  // A 1 mm square chamfered at the top left by 20 % (0.2 mm): a triangle of 0.02 mm^2 off it, five corners.
  const ch = roundChamferedRectPolygon([1_000_000, 1_000_000], 0, 0.2, 1, 0, 5000);
  assert.equal(ch.length, 5);
  near(area(ch), 1e12 - 0.5 * 200_000 ** 2, 1);
  // The cut corner is at the top left (-x, -y in KiCad's y-down frame).
  assert.ok(ch.some(([x, y]) => x === -500_000 && y === -300_000) && ch.some(([x, y]) => x === -300_000 && y === -500_000), JSON.stringify(ch));
  // All four chamfered at 50 %: a diamond.
  assert.equal(roundChamferedRectPolygon([1_000_000, 1_000_000], 0, 0.5, 15, 0, 5000).length, 4);
});

const pad = (over: Partial<Pad>): Pad => ({ num: "1", net: null, x: 0, y: 0, w: 1_000_000, h: 600_000, round: false, th: false, kind: "smd", ...over }) as Pad;

test("createPadWithMargin: a negative or uneven margin grows a stand-in pad (the 'huge hack'), an even one inflates the effective shape", () => {
  const rect = pad({ shape: "rect", size: [1_000_000, 600_000] });
  // A paste margin of -0.05 mm: the pad 0.1 mm smaller, square corners.
  assert.deepEqual(bbox(padShapeLocal(rect, [-50_000, -50_000])![0]!), [-450_000, -250_000, 450_000, 250_000]);
  assert.equal(padShapeLocal(rect, [-50_000, -50_000])![0]!.length, 4);
  // An uneven one (a ratio on an oblong pad) likewise, per axis.
  assert.deepEqual(bbox(padShapeLocal(rect, [-100_000, -60_000])![0]!), [-400_000, -240_000, 400_000, 240_000]);
  // A margin that leaves nothing: no paste ("if( dummySize.x <= 0 || dummySize.y <= 0 ) return;").
  assert.deepEqual(padShapeLocal(rect, [-500_000, -300_000]), []);
  // A positive even margin: inflated with round corners.
  const grown = padShapeLocal(rect, [50_000, 50_000])![0]!;
  nearBox(grown, [-550_000, -350_000, 550_000, 350_000]);
  assert.ok(grown.length > 4, "round corners");
  // A rounded rectangle keeps its ratio in the stand-in (the corner radius of the smaller pad).
  const rr = padShapeLocal(pad({ shape: "round_rect", size: [1_000_000, 600_000], ratio: 0.25 }), [-100_000, -100_000])![0]!;
  assert.deepEqual(bbox(rr), [-400_000, -200_000, 400_000, 200_000]);
  // A circle and an oval.
  nearBox(padShapeLocal(pad({ shape: "circle", size: [800_000, 800_000], round: true }), [0, 0])![0]!, [-400_000, -400_000, 400_000, 400_000]);
  nearBox(padShapeLocal(pad({ shape: "oval", size: [1_600_000, 800_000], round: true }), [0, 0])![0]!, [-800_000, -400_000, 800_000, 400_000]);
  // A custom pad is the backend's outline (null here: the caller takes `outline`).
  assert.equal(padShapeLocal(pad({ shape: "custom" })), null);
});

test("a pad's shape is carried to the board by its rotation, the footprint's mirror and rotation", () => {
  // A 2 x 1 mm rectangle turned 45 degrees by the footprint: a diamond-ish box whose extent is (2 + 1) / sqrt 2 each way.
  const rect = pad({ shape: "rect", size: [2_000_000, 1_000_000], x: 10_000_000, y: 5_000_000, rot: 0 });
  const c = padContours(rect, padPlacement(rect, { rot: 45, side: "top" }))[0]!;
  const half = (3_000_000 / Math.SQRT2) / 2;
  const [x0, y0, x1, y1] = bbox(c) as [number, number, number, number];
  near(x0, 10_000_000 - half, 2);
  near(x1, 10_000_000 + half, 2);
  near(y0, 5_000_000 - half, 2);
  near(y1, 5_000_000 + half, 2);
  near(area(c), 2e12, 10);
  // A trapezoid on a bottom-side footprint is mirrored in x: its wide side goes from -x to +x.
  const trap = pad({ shape: "trapezoid", size: [2_000_000, 1_000_000], delta: [800_000, 0] });
  const top = padContours(trap, padPlacement(trap, { rot: 0, side: "top" }))[0]!;
  const bottom = padContours(trap, padPlacement(trap, { rot: 0, side: "bottom" }))[0]!;
  const tallAt = (cs: Contour, sign: number) => Math.max(...cs.filter(([x]) => Math.sign(x) === sign).map(([, y]) => y)) - Math.min(...cs.filter(([x]) => Math.sign(x) === sign).map(([, y]) => y));
  assert.ok(tallAt(top, -1) > tallAt(top, 1), "on top the long side is at -x");
  assert.ok(tallAt(bottom, 1) > tallAt(bottom, -1), "mirrored on the bottom");
  // toBoard is PadFrame::to_board: the pad's rotation, then the mirror, then the footprint's rotation.
  const p = toBoard({ x: 0, y: 0, padRotDeg: 90, fpRotDeg: 0, mirror: true }, [1, 0]);
  near(p[0], 0, 1e-9);
  near(p[1], 1, 1e-9);
  // An older backend (no `size`): the box as it is on the board, no turn.
  const old = pad({ size: undefined, w: 1_000_000, h: 2_000_000 });
  assert.deepEqual(bbox(padContours(old, padPlacement(old, { rot: 90, side: "bottom" }))[0]!), [-500_000, -1_000_000, 500_000, 1_000_000]);
  // A custom pad: its outline as sent.
  const custom = pad({ shape: "custom", outline: [[[0, 0], [1, 0], [1, 1]]] });
  assert.deepEqual(padContours(custom, padPlacement(custom, { rot: 0, side: "top" })), [[[0, 0], [1, 0], [1, 1]]]);
});

test("even-odd contours nest into outlines with their holes", () => {
  const sq = (s: number, o = 0): Contour => [
    [o - s, o - s],
    [o + s, o - s],
    [o + s, o + s],
    [o - s, o + s],
  ];
  const nested = nestContours([sq(10), sq(5), sq(2), sq(1, 100)]);
  assert.equal(nested.length, 3, "the outer ring, the island inside its hole, the square apart");
  const ring = nested.find((n) => n.outer === nested[0]!.outer)!;
  assert.equal(ring.holes.length, 1);
  assert.equal(nested.filter((n) => n.holes.length === 0).length, 2);
});
