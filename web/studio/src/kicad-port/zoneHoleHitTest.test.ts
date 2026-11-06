import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { collectBoxSelection, DEFAULT_SELECTION_FILTER, pickSelectionCandidates } from "../components/canvas/selectionCandidates";
import { zoneArea, zoneContainsPoint, zoneEdgeDistance } from "../components/canvas/itemHitTest";
import { findNetAtCursor } from "../components/canvas/netAtCursor";

// A 20 mm pour with a 10 mm hole in its middle (5..15 mm), as `ZONE::Outline()` holds it.
const outline: [number, number][] = [
  [0, 0],
  [20_000_000, 0],
  [20_000_000, 20_000_000],
  [0, 20_000_000],
];
const hole: [number, number][] = [
  [5_000_000, 5_000_000],
  [15_000_000, 5_000_000],
  [15_000_000, 15_000_000],
  [5_000_000, 15_000_000],
];
const zone = { id: "Z", net: "GND", layer: "F.Cu", teardrop: false, outline, holes: [hole] };
const board = (zones: unknown[]): BoardState =>
  ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: { tracks: [], vias: [], zones, track_width_presets: [], via_presets: [], teardrop_settings: {} }, drawings: null, checks: [], activity: [], job: "idle" }) as unknown as BoardState;
const pick = (b: BoardState, x: number, y: number) => pickSelectionCandidates(b, x, y, 150_000, 1_000, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false).map((c) => c.id);

test("SHAPE_POLY_SET::Contains: inside the outline and in no hole", () => {
  assert.equal(zoneContainsPoint(zone, 2_000_000, 2_000_000), true);
  assert.equal(zoneContainsPoint(zone, 10_000_000, 10_000_000), false, "in the hole");
  assert.equal(zoneContainsPoint(zone, 30_000_000, 10_000_000), false);
  assert.equal(zoneContainsPoint({ outline }, 10_000_000, 10_000_000), true);
  // a degenerate hole (fewer than three corners) is no hole
  assert.equal(zoneContainsPoint({ outline, holes: [[[0, 0], [1, 1]]] }, 10_000_000, 10_000_000), true);
});

test("HitTestForEdge over every contour: a hole's side is a side of the zone; the area is the outline's less the hole's", () => {
  assert.equal(zoneEdgeDistance(zone, 10_000_000, 15_100_000), 100_000, "100 um below the hole's top side");
  assert.equal(zoneEdgeDistance(zone, 10_000_000, 10_000_000), 5_000_000, "the middle of the hole is 5 mm from its sides");
  assert.equal(zoneEdgeDistance({ outline }, 10_000_000, 10_000_000), 10_000_000);
  assert.equal(zoneArea(zone), 400e12 - 100e12);
});

test("a click in a hole does not pick the zone; one on the hole's side or in the pour does", () => {
  const b = board([zone]);
  assert.deepEqual(pick(b, 10_000_000, 10_000_000), [], "deep in the hole");
  assert.deepEqual(pick(b, 10_000_000, 5_050_000), ["Z"], "on the hole's side (50 um inside the hole)");
  assert.deepEqual(pick(b, 2_000_000, 10_000_000), ["Z"], "in the pour");
  // the side of the hole is "small" like the outline's: a click on it is exact
  const onSide = pickSelectionCandidates(b, 15_000_000, 10_000_000, 150_000, 1_000, DEFAULT_SELECTION_FILTER, {}, null, false, new Set(), false, false);
  assert.equal(onSide[0]?.areaNm2, 1);
});

test("a rule area with a hole: its inside is picked, its hole is not", () => {
  const b = board([{ ...zone, id: "R", net: "", is_rule_area: true }]);
  assert.deepEqual(pick(b, 2_000_000, 2_000_000), ["R"]);
  assert.deepEqual(pick(b, 10_000_000, 10_000_000), []);
});

test("a box takes a zone by its outline, holes or not", () => {
  const b = board([zone]);
  assert.deepEqual(collectBoxSelection(b, [-1, -1, 20_000_001, 20_000_001], false, DEFAULT_SELECTION_FILTER, {}, null, false).map((h) => h.id), ["Z"]);
  // a window round the hole only holds no zone
  assert.deepEqual(collectBoxSelection(b, [4_000_000, 4_000_000, 16_000_000, 16_000_000], false, DEFAULT_SELECTION_FILTER, {}, null, false), []);
});

test("the net under the cursor: not the zone's in its hole, the zone's on the hole's side", () => {
  const b = board([zone]);
  assert.equal(findNetAtCursor(b, 10_000_000, 10_000_000, 150_000), null);
  assert.equal(findNetAtCursor(b, 10_000_000, 5_100_000, 150_000), "GND");
  // inside the outline: only the fill is the zone's (`HitTestFilledArea` with `SetIgnoreZoneFills( false )`), an unfilled outline's inside is not (r7 C7-11)
  assert.equal(findNetAtCursor(b, 1_000_000, 1_000_000, 150_000), null);
  const fill = { id: "Z", net: "GND", layer: "F.Cu", filled: true, area_nm2: 0, fragments: [[[0, 0], [20_000_000, 0], [20_000_000, 4_000_000], [0, 4_000_000]] as [number, number][]] };
  assert.equal(findNetAtCursor(b, 1_000_000, 1_000_000, 150_000, { fills: [fill] }), "GND");
  assert.equal(findNetAtCursor(b, 1_000_000, 1_000_000, 150_000, { fills: [fill], layerVisible: { "F.Cu": false } }), null, "a hidden layer's fill is not hit");
});

test("the net under the cursor is the collector's: the shape of a pad, five pixels of slop, no-net items skipped, the active layer first", () => {
  const pads = [{ num: "1", net: "A", x: 0, y: 0, w: 3_000_000, h: 3_000_000, round: false, th: false }];
  const b = { ...board([]), parts: [{ ref: "U1", placed: true, side: "top", pads }] } as unknown as Parameters<typeof findNetAtCursor>[0];
  // 1.4 mm from the centre of a 3 mm pad: on it (the old pick took the nearest pad centre within 10 px)
  assert.equal(findNetAtCursor(b, 1_400_000, 1_400_000, 5_000), "A");
  assert.equal(findNetAtCursor(b, 1_510_000, 0, 5_000), null, "past the slop");
  const tracks = [
    { id: "t1", net: "", layer: "F.Cu", width: 200_000, pts: [[-5_000_000, 5_000_000], [5_000_000, 5_000_000]] },
    { id: "t2", net: "B", layer: "B.Cu", width: 200_000, pts: [[-5_000_000, 5_000_000], [5_000_000, 5_000_000]] },
    { id: "t3", net: "C", layer: "F.Cu", width: 200_000, pts: [[-5_000_000, 5_000_000], [5_000_000, 5_000_000]] },
  ];
  const withTracks = { ...b, routing: { tracks, vias: [], zones: [] } } as unknown as Parameters<typeof findNetAtCursor>[0];
  assert.equal(findNetAtCursor(withTracks, 0, 5_000_000, 5_000, { activeLayer: "B.Cu" }), "B", "the active layer's track first");
  assert.equal(findNetAtCursor(withTracks, 0, 5_000_000, 5_000, { activeLayer: "F.Cu" }), "C", "the no-net track is skipped (IgnoreNoNets)");
  assert.equal(findNetAtCursor(withTracks, 0, 5_000_000, 5_000, { activeLayer: "F.Cu", highContrast: true, layerVisible: {} }), "C");
});
