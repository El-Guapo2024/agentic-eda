import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { addCornerCmds, addCornerToRing, cornerTarget, nearestOnArc, nearestOnSegment, splitArcAtCursor, splitSegmentAt, type P } from "./pcbAddCorner";

const square: P[] = [
  [0, 0],
  [10_000, 0],
  [10_000, 10_000],
  [0, 10_000],
];

test("the corner goes to the point of the nearest side, not to the cursor", () => {
  // the cursor is 700 um inside the bottom side (y = 0) at x = 4 000: the new corner is at (4 000, 0), inserted at the end of that side
  assert.deepEqual(addCornerToRing(square, [4_000, 700]), [
    [0, 0],
    [4_000, 0],
    [10_000, 0],
    [10_000, 10_000],
    [0, 10_000],
  ]);
  // far outside, nearest the right side: no distance limit
  assert.deepEqual(addCornerToRing(square, [90_000, 3_000]), [
    [0, 0],
    [10_000, 0],
    [10_000, 3_000],
    [10_000, 10_000],
    [0, 10_000],
  ]);
});

test("the closing side is a side too: its corner goes in front of the first point, as `InsertVertex( 0, .. )` does", () => {
  const out = addCornerToRing(square, [-500, 6_000])!;
  assert.deepEqual(out, [
    [0, 6_000],
    [0, 0],
    [10_000, 0],
    [10_000, 10_000],
    [0, 10_000],
  ]);
});

test("a point that is already a corner is not added twice: the middle of the side is", () => {
  // nearest the corner (10 000, 0): the point on the bottom side is the corner itself
  assert.deepEqual(addCornerToRing(square, [11_000, -200]), [
    [0, 0],
    [5_000, 0],
    [10_000, 0],
    [10_000, 10_000],
    [0, 10_000],
  ]);
  // an odd sum is cut like C++ integer division does, toward zero
  assert.deepEqual(addCornerToRing([[0, 0], [1_001, 0], [1_001, 1_000]], [-5, -5]), [[0, 0], [500, 0], [1_001, 0], [1_001, 1_000]]);
});

test("a ring of fewer than two points has no side to add to", () => {
  assert.equal(addCornerToRing([[0, 0]], [1, 1]), null);
  assert.equal(addCornerToRing([], [1, 1]), null);
});

test("SEG::NearestPoint: the foot of the perpendicular, an end when the cursor is past it, rounded to whole um", () => {
  assert.deepEqual(nearestOnSegment([0, 0], [10, 0], [4, 5]), [4, 0]);
  assert.deepEqual(nearestOnSegment([0, 0], [10, 0], [-4, 5]), [0, 0]);
  assert.deepEqual(nearestOnSegment([0, 0], [10, 0], [14, 5]), [10, 0]);
  assert.deepEqual(nearestOnSegment([0, 0], [3, 4], [3, 0]), [1, 1], "t = 9 of 25: (1.08, 1.44) rounds to (1, 1)");
});

test("Split Segment: two segments meeting at the nearest point; nothing to cut at an end", () => {
  assert.deepEqual(splitSegmentAt([0, 0], [10_000, 0], [4_000, 900]), [
    { start: [0, 0], end: [4_000, 0] },
    { start: [4_000, 0], end: [10_000, 0] },
  ]);
  assert.equal(splitSegmentAt([0, 0], [10_000, 0], [-300, 100]), null);
});

test("Split Arc: the nearest point of the arc, both halves on its circle", () => {
  // a quarter of the circle of radius 10 000 about the origin, from (10 000, 0) through its middle to (0, 10 000)
  const mid: P = [7_071, 7_071];
  const at = nearestOnArc([10_000, 0], mid, [0, 10_000], [20_000, 20_000]);
  assert.deepEqual(at, [7_071, 7_071], "straight out from the origin through the middle");
  const parts = splitArcAtCursor([10_000, 0], mid, [0, 10_000], [20_000, 0])!;
  assert.deepEqual(parts, null, "the cursor is nearest the start: nothing to cut there");
  const two = splitArcAtCursor([10_000, 0], mid, [0, 10_000], [20_000, 5_000])!;
  assert.deepEqual(two[0].start, [10_000, 0]);
  assert.deepEqual(two[1].end, [0, 10_000]);
  assert.deepEqual(two[0].end, two[1].start);
  // beyond the arc's end the nearest point is the end
  assert.deepEqual(nearestOnArc([10_000, 0], mid, [0, 10_000], [-5_000, 0]), [0, 10_000]);
});

const board = (shapes: unknown[], zones: unknown[] = []): BoardState =>
  ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: { tracks: [], vias: [], zones, track_width_presets: [], via_presets: [], teardrop_settings: {} }, drawings: { shapes, texts: [], groups: [], dimensions: [], dimension_settings: {} }, checks: [], activity: [], job: "idle" }) as unknown as BoardState;
const common = { layer: "F.SilkS", stroke_width: 150, filled: false };

test("Create Corner is offered for one zone, polygon, segment or arc and for nothing else", () => {
  const b = board(
    [
      { id: "seg", kind: "segment", start: [0, 0], end: [10_000, 0], ...common },
      { id: "rect", kind: "rect", start: [0, 0], end: [1, 1], ...common },
      { id: "poly", kind: "polygon", pts: [[0, 0], [1000, 0], [0, 1000]], ...common },
      { id: "arc", kind: "arc", start: [10_000, 0], mid: [7_071, 7_071], end: [0, 10_000], ...common },
    ],
    [{ id: "z", net: "GND", layer: "F.Cu", outline: [[0, 0], [1000, 0], [1000, 1000]], teardrop: false }, { id: "td", net: "GND", layer: "F.Cu", outline: [[0, 0], [1, 0], [1, 1]], teardrop: true }]
  );
  assert.equal(cornerTarget(b, new Set(["seg"]))?.kind, "segment");
  assert.equal(cornerTarget(b, new Set(["poly"]))?.kind, "polygon");
  assert.equal(cornerTarget(b, new Set(["arc"]))?.kind, "arc");
  assert.equal(cornerTarget(b, new Set(["z"]))?.kind, "zone");
  assert.equal(cornerTarget(b, new Set(["rect"])), null, "a rectangle has no corner to add");
  assert.equal(cornerTarget(b, new Set(["td"])), null, "a teardrop is generated");
  assert.equal(cornerTarget(b, new Set(["seg", "poly"])), null, "one item at a time");
  assert.equal(cornerTarget(b, new Set()), null);
});

test("the commands of Add Corner: a zone's outline set, a polygon replaced in place, a segment and an arc cut in two", () => {
  const b = board(
    [
      { id: "seg", kind: "segment", start: [0, 0], end: [10_000, 0], ...common },
      { id: "poly", kind: "polygon", pts: [[0, 0], [10_000, 0], [10_000, 10_000], [0, 10_000]], ...common },
      { id: "arc", kind: "arc", start: [10_000, 0], mid: [7_071, 7_071], end: [0, 10_000], ...common },
    ],
    [{ id: "z", net: "GND", layer: "F.Cu", outline: [[0, 0], [10_000, 0], [10_000, 10_000], [0, 10_000]], teardrop: false }]
  );
  const zone = addCornerCmds(cornerTarget(b, new Set(["z"]))!, [4_000, 700])!;
  assert.deepEqual(zone, [{ op: "set_zone_outline", id: "z", outline: [{ x: 0, y: 0 }, { x: 4_000, y: 0 }, { x: 10_000, y: 0 }, { x: 10_000, y: 10_000 }, { x: 0, y: 10_000 }] }]);

  const poly = addCornerCmds(cornerTarget(b, new Set(["poly"]))!, [10_900, 6_000])!;
  assert.equal(poly.length, 1);
  assert.deepEqual(poly[0], { op: "replace_shape", id: "poly", shape: { kind: "polygon", layer: "F.SilkS", stroke_width: 150, filled: false, pts: [{ x: 0, y: 0 }, { x: 10_000, y: 0 }, { x: 10_000, y: 6_000 }, { x: 10_000, y: 10_000 }, { x: 0, y: 10_000 }] } });

  const seg = addCornerCmds(cornerTarget(b, new Set(["seg"]))!, [4_000, 900])!;
  assert.deepEqual(seg, [
    { op: "replace_shape", id: "seg", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: { x: 0, y: 0 }, end: { x: 4_000, y: 0 } } },
    { op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: { x: 4_000, y: 0 }, end: { x: 10_000, y: 0 } } },
  ]);
  assert.equal(addCornerCmds(cornerTarget(b, new Set(["seg"]))!, [-500, 0]), null, "at an end there is nothing to cut");

  const arc = addCornerCmds(cornerTarget(b, new Set(["arc"]))!, [20_000, 20_000])!;
  assert.equal(arc.length, 2);
  assert.equal(arc[0]!.op, "replace_shape");
  assert.equal(arc[1]!.op, "add_shape");
});
