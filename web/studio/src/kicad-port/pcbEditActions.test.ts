import { test } from "node:test";
import assert from "node:assert/strict";
import {
  grabNearestUnconnectedFootprints,
  otherEndOfStart,
  polarTranslation,
  positionRelativeSelectionAnchor,
  relativeMoveVector,
  resolveToggleLock,
  routeSelectedAnchors,
  routeStartLayer,
  selectUnconnectedFootprints,
  stepCopperLayer,
  toPolarDeg,
  topLeftItem,
  unrouteSegmentReselect,
  type UnconnectedEdge,
  type UnconnectedPart,
} from "./pcbEditActions";

// ------------------------------------------------------------------ lock

test("resolveToggleLock: any locked item in the selection unlocks all (modifyLockSelected TOGGLE)", () => {
  assert.equal(resolveToggleLock(["a", "b"], new Set()), true);
  assert.equal(resolveToggleLock(["a", "b"], new Set(["b"])), false);
  assert.equal(resolveToggleLock(["a", "b"], new Set(["a", "b"])), false);
  assert.equal(resolveToggleLock(["a"], new Set(["zzz"])), true, "a lock on an unselected item is irrelevant");
});

// ------------------------------------------------------------ unconnected

// U1 pads at x=0, U2 at x=1000 and x=5000, U3 at x=2000; all on net N except one.
const parts: UnconnectedPart[] = [
  { ref: "U1", placed: true, pads: [{ num: "1", net: "N", x: 0, y: 0 }, { num: "2", net: "M", x: 0, y: 100 }] },
  { ref: "U2", placed: true, pads: [{ num: "1", net: "N", x: 1000, y: 0 }, { num: "2", net: "M", x: 5000, y: 100 }] },
  { ref: "U3", placed: true, pads: [{ num: "1", net: "N", x: 2000, y: 0 }] },
  { ref: "U4", placed: false, pads: [{ num: "1", net: "N", x: 0, y: 0 }] },
];
const edges: UnconnectedEdge[] = [
  { net: "N", from: [0, 0], to: [1000, 0] },
  { net: "N", from: [1000, 0], to: [2000, 0] },
  { net: "M", from: [0, 100], to: [5000, 100] },
];

test("selectUnconnectedFootprints: selection plus every footprint at the far end of a ratsnest line from its pads", () => {
  assert.deepEqual(selectUnconnectedFootprints(parts, edges, ["U1"]), ["U1", "U2"]);
  assert.deepEqual(selectUnconnectedFootprints(parts, edges, ["U2"]), ["U2", "U1", "U3"]);
  assert.deepEqual(selectUnconnectedFootprints(parts, edges, ["U3"]), ["U3", "U2"]);
});

test("selectUnconnectedFootprints: nothing selected, or a footprint with no ratsnest, adds nothing", () => {
  assert.deepEqual(selectUnconnectedFootprints(parts, edges, []), []);
  assert.deepEqual(selectUnconnectedFootprints(parts, [], ["U1"]), ["U1"]);
});

test("selectUnconnectedFootprints: an edge ending on a track end (no pad there) is skipped", () => {
  assert.deepEqual(selectUnconnectedFootprints(parts, [{ net: "N", from: [0, 0], to: [123, 456] }], ["U1"]), ["U1"]);
});

test("grabNearestUnconnectedFootprints: selection is replaced by the nearest footprint per pad", () => {
  // U2's pad 1 (net N, x=1000): U1 at 1000 away, U3 at 1000 away -> first edge wins (strict <); pad 2 (net M): U1.
  assert.deepEqual(grabNearestUnconnectedFootprints(parts, edges, ["U2"]), ["U1"]);
  // U1: pad 1 -> U2; pad 2 -> U2 (deduplicated).
  assert.deepEqual(grabNearestUnconnectedFootprints(parts, edges, ["U1"]), ["U2"]);
  assert.deepEqual(grabNearestUnconnectedFootprints(parts, edges, ["U3"]), ["U2"]);
});

test("grabNearestUnconnectedFootprints: picks the shorter of two edges and skips same-footprint loops", () => {
  const p: UnconnectedPart[] = [
    { ref: "A", placed: true, pads: [{ num: "1", net: "N", x: 0, y: 0 }, { num: "2", net: "N", x: 10, y: 0 }] },
    { ref: "B", placed: true, pads: [{ num: "1", net: "N", x: 5000, y: 0 }] },
    { ref: "C", placed: true, pads: [{ num: "1", net: "N", x: -300, y: 0 }] },
  ];
  const e: UnconnectedEdge[] = [
    { net: "N", from: [0, 0], to: [10, 0] }, // loop on A -> skipped
    { net: "N", from: [0, 0], to: [5000, 0] },
    { net: "N", from: [0, 0], to: [-300, 0] },
  ];
  assert.deepEqual(grabNearestUnconnectedFootprints(p, e, ["A"]), ["C"]);
});

// ------------------------------------------------------------ layer step

test("stepCopperLayer: next/prev wrap through the stack and skip hidden layers", () => {
  const layers = ["F.Cu", "In1.Cu", "B.Cu"];
  assert.equal(stepCopperLayer(layers, {}, "F.Cu", 1), "In1.Cu");
  assert.equal(stepCopperLayer(layers, {}, "B.Cu", 1), "F.Cu");
  assert.equal(stepCopperLayer(layers, {}, "F.Cu", -1), "B.Cu");
  assert.equal(stepCopperLayer(layers, { "In1.Cu": false }, "F.Cu", 1), "B.Cu");
});

test("stepCopperLayer: non-copper/no active layer jumps to B.Cu (next) or F.Cu (prev); all others hidden is a no-op", () => {
  const layers = ["F.Cu", "B.Cu"];
  assert.equal(stepCopperLayer(layers, {}, null, 1), "B.Cu");
  assert.equal(stepCopperLayer(layers, {}, "F.SilkS", 1), "B.Cu");
  assert.equal(stepCopperLayer(layers, {}, "F.SilkS", -1), "F.Cu");
  assert.equal(stepCopperLayer(layers, { "B.Cu": false }, "F.Cu", 1), null);
  assert.equal(stepCopperLayer([], {}, "F.Cu", 1), null);
});

// ------------------------------------------------------------ unroute

test("unrouteSegmentReselect: reselects the copper touching the deleted segment's ends, same net only", () => {
  const tracks = [
    { id: "t1", net: "N", pts: [[0, 0], [100, 0]] as [number, number][] },
    { id: "t2", net: "N", pts: [[100, 0], [100, 100]] as [number, number][] },
    { id: "t3", net: "N", pts: [[100, 100], [200, 100]] as [number, number][] },
    { id: "other", net: "M", pts: [[100, 0], [150, 0]] as [number, number][] },
  ];
  const vias = [{ id: "v1", net: "N", x: 0, y: 0 }, { id: "vfar", net: "N", x: 9, y: 9 }];
  assert.deepEqual(unrouteSegmentReselect(tracks, vias, new Set(["t1"])), ["t2", "v1"]);
  assert.deepEqual(unrouteSegmentReselect(tracks, vias, new Set(["t2"])), ["t1", "t3"]);
  assert.deepEqual(unrouteSegmentReselect(tracks, vias, new Set(["t1", "t2"])), ["t3", "v1"], "deleted items are never reselected");
  assert.deepEqual(unrouteSegmentReselect(tracks, vias, new Set()), []);
});

// ------------------------------------------------------- position relative

test("topLeftItem: smallest x, ties by smallest y; footprintsOnly skips the rest", () => {
  const items = [
    { isFootprint: false, x: 0, y: 0 },
    { isFootprint: true, x: 500, y: 900 },
    { isFootprint: true, x: 500, y: 100 },
    { isFootprint: true, x: 700, y: 0 },
  ];
  assert.deepEqual(topLeftItem(items, false), items[0]);
  assert.deepEqual(topLeftItem(items, true), items[2]);
  assert.equal(topLeftItem([{ isFootprint: false, x: 1, y: 1 }], true), null);
  assert.equal(topLeftItem([], false), null);
});

test("positionRelativeSelectionAnchor: footprint preferred over a more top-left non-footprint", () => {
  assert.deepEqual(
    positionRelativeSelectionAnchor([
      { isFootprint: false, x: 0, y: 0 },
      { isFootprint: true, x: 500, y: 500 },
    ]),
    { x: 500, y: 500 }
  );
  assert.deepEqual(positionRelativeSelectionAnchor([{ isFootprint: false, x: 7, y: 8 }]), { x: 7, y: 8 });
  assert.equal(positionRelativeSelectionAnchor([]), null);
});

test("relativeMoveVector: reference + offset - selection anchor (RelativeItemSelectionMove)", () => {
  assert.deepEqual(relativeMoveVector({ x: 1000, y: 2000 }, { x: 300, y: -400 }, { x: 5000, y: 5000 }), { x: -3700, y: -3400 });
  // Offset 0 from an item on itself moves nothing.
  assert.deepEqual(relativeMoveVector({ x: 5000, y: 5000 }, { x: 0, y: 0 }, { x: 5000, y: 5000 }), { x: 0, y: 0 });
});

test("polar conversions round-trip in board (Y-down) coordinates", () => {
  const p = toPolarDeg(0, 1000);
  assert.equal(p.r, 1000);
  assert.equal(p.deg, 90);
  assert.deepEqual(toPolarDeg(0, 0), { r: 0, deg: 0 });
  assert.deepEqual(polarTranslation(1000, 90), { x: 0, y: 1000 });
  assert.deepEqual(polarTranslation(1000, 0), { x: 1000, y: 0 });
  const back = polarTranslation(toPolarDeg(-3000, 4000).r, toPolarDeg(-3000, 4000).deg);
  assert.deepEqual(back, { x: -3000, y: 4000 });
});

// ------------------------------------------------------------ route selected

test("routeSelectedAnchors: one anchor per ratsnest line end on a selected footprint's pads, far end as target", () => {
  const rp = parts.map((p) => ({ ...p, side: "top" as const }));
  const a = routeSelectedAnchors(rp, edges, [], [], ["U2"]);
  assert.deepEqual(
    a.map((x) => [x.at, x.target, x.net]),
    [
      [[1000, 0], [0, 0], "N"],
      [[1000, 0], [2000, 0], "N"],
      [[5000, 100], [0, 100], "M"],
    ]
  );
});

test("routeSelectedAnchors: a selected track end / via with ratsnest lines becomes an anchor; unknown ids add nothing", () => {
  const a = routeSelectedAnchors(parts, edges, [{ id: "t", net: "N", pts: [[0, 0], [50, 0]] }], [{ id: "v", net: "M", x: 5000, y: 100 }], ["t", "v", "ghost"]);
  assert.deepEqual(
    a.map((x) => [x.at, x.target]),
    [
      [[0, 0], [1000, 0]],
      [[5000, 100], [0, 100]],
    ]
  );
});

test("routeStartLayer: active layer when the pad can sit on it, else the pad's side", () => {
  const layers = ["F.Cu", "In1.Cu", "B.Cu"];
  assert.equal(routeStartLayer({ th: true, side: "top" }, layers, "In1.Cu"), "In1.Cu");
  assert.equal(routeStartLayer({ th: false, side: "top" }, layers, "In1.Cu"), "F.Cu");
  assert.equal(routeStartLayer({ th: false, side: "bottom" }, layers, "B.Cu"), "B.Cu");
  assert.equal(routeStartLayer({ th: false, side: "bottom" }, layers, null), "B.Cu");
  assert.equal(routeStartLayer({ th: true, side: "top" }, layers, "F.SilkS"), "F.Cu");
});

test("otherEndOfStart: far end of the ratsnest line at the route start, null off any line", () => {
  assert.deepEqual(otherEndOfStart([0, 0], "N", edges), [1000, 0]);
  assert.deepEqual(otherEndOfStart([2000, 0], "N", edges), [1000, 0]);
  assert.equal(otherEndOfStart([3, 3], "N", edges), null);
  assert.equal(otherEndOfStart([0, 0], "X", edges), null);
});
