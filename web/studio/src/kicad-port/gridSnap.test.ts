import { test } from "node:test";
import assert from "node:assert/strict";
import { computeNearest, alignToGrid, bestSnapPoint, collectAnchors, DEFAULT_MAGNETIC_SETTINGS, type AnchorSourceBoard, type SnapAnchor } from "./gridSnap";

test("computeNearest rounds to the nearest grid multiple from an arbitrary origin", () => {
  assert.deepEqual(computeNearest({ x: 1040, y: -1040 }, { x: 1000, y: 1000 }, { x: 0, y: 0 }), { x: 1000, y: -1000 });
  assert.deepEqual(computeNearest({ x: 1060, y: 0 }, { x: 1000, y: 1000 }, { x: 0, y: 0 }), { x: 1000, y: 0 });
  // Offset origin: grid lines are at ...,-500,500,1500,... not multiples of 1000.
  assert.deepEqual(computeNearest({ x: 1200, y: 0 }, { x: 1000, y: 1000 }, { x: 500, y: 0 }), { x: 1500, y: 0 });
});

test("computeNearest: a zero grid size means no rounding (passthrough)", () => {
  assert.deepEqual(computeNearest({ x: 1234, y: 5678 }, { x: 0, y: 0 }, { x: 0, y: 0 }), { x: 1234, y: 5678 });
});

test("alignToGrid: Ctrl disables the grid round-off entirely (tool_event.h DisableGridSnapping, Modifier(MD_CTRL))", () => {
  const point = { x: 1040, y: 1040 };
  assert.deepEqual(alignToGrid(point, 1000, { x: 0, y: 0 }, { ctrlOrCmd: false }), { x: 1000, y: 1000 });
  assert.deepEqual(alignToGrid(point, 1000, { x: 0, y: 0 }, { ctrlOrCmd: true }), point, "Ctrl held -> full precision, no rounding");
});

test("bestSnapPoint: Shift disables anchor snapping but NOT grid round-off (edit_tool_move_fct.cpp grid.SetSnap(!Shift))", () => {
  const anchors: SnapAnchor[] = [{ x: 1005, y: 1005, kind: "pad", ownerId: "R1" }];
  const point = { x: 1000, y: 1000 };
  const withoutShift = bestSnapPoint(point, 1000, { x: 0, y: 0 }, 1, 1000, anchors, { ctrlOrCmd: false, shiftKey: false });
  const withShift = bestSnapPoint(point, 1000, { x: 0, y: 0 }, 1, 1000, anchors, { ctrlOrCmd: false, shiftKey: true });
  assert.deepEqual(withoutShift.point, { x: 1005, y: 1005 }, "close pad anchor wins without Shift");
  assert.equal(withoutShift.snappedTo?.ownerId, "R1");
  assert.deepEqual(withShift.point, { x: 1000, y: 1000 }, "Shift: no anchor, but still grid-rounded (already on grid here)");
  assert.equal(withShift.snappedTo, null);
});

test("bestSnapPoint: an anchor outside the snap range is ignored, grid point used instead", () => {
  const anchors: SnapAnchor[] = [{ x: 5000, y: 5000, kind: "pad", ownerId: "R1" }];
  const result = bestSnapPoint({ x: 1040, y: 1040 }, 1000, { x: 0, y: 0 }, 1, 1000, anchors, { ctrlOrCmd: false, shiftKey: false });
  assert.deepEqual(result.point, { x: 1000, y: 1000 });
  assert.equal(result.snappedTo, null);
});

test("bestSnapPoint: the nearest of several in-range anchors wins", () => {
  const anchors: SnapAnchor[] = [
    { x: 1010, y: 1000, kind: "pad", ownerId: "far" },
    { x: 1002, y: 1000, kind: "via", ownerId: "near" },
  ];
  const result = bestSnapPoint({ x: 1000, y: 1000 }, 1000, { x: 0, y: 0 }, 1, 1000, anchors, { ctrlOrCmd: false, shiftKey: false });
  assert.equal(result.snappedTo?.ownerId, "near");
});

test("bestSnapPoint: snap range is clamped to the visible grid pitch when grid snapping is on (BestSnapAnchor's min(snapScale, visibleGrid) clamp)", () => {
  // snapRangePx=25 at scale=1 px/um -> 25um raw snap scale, but the
  // visible grid is coarser (say 10um) -- no wait, the clamp takes the
  // MINIMUM, so a *finer* visible grid than the raw snap scale narrows
  // the effective range. Use a visible grid smaller than 25um to prove
  // the clamp actually applies (anchor at distance 20 should miss).
  const anchors: SnapAnchor[] = [{ x: 1020, y: 1000, kind: "pad", ownerId: "R1" }];
  const clamped = bestSnapPoint({ x: 1000, y: 1000 }, 1000, { x: 0, y: 0 }, 1, /* visibleGridUm */ 10, anchors, { ctrlOrCmd: false, shiftKey: false }, 25);
  assert.equal(clamped.snappedTo, null, "snap range clamped to the 10um visible grid, so a 20um-away anchor is out of range");

  const unclamped = bestSnapPoint({ x: 1000, y: 1000 }, 1000, { x: 0, y: 0 }, 1, /* visibleGridUm */ 10, anchors, { ctrlOrCmd: true, shiftKey: false }, 25);
  assert.equal(unclamped.snappedTo?.ownerId, "R1", "Ctrl disables the grid, so the clamp (which only applies when grid snapping is on) is lifted");
});

test("bestSnapPoint: no anchors in range and grid disabled (Ctrl) returns the raw point", () => {
  const result = bestSnapPoint({ x: 1234, y: 4321 }, 1000, { x: 0, y: 0 }, 1, 1000, [], { ctrlOrCmd: true, shiftKey: false });
  assert.deepEqual(result.point, { x: 1234, y: 4321 });
});

const board: AnchorSourceBoard = {
  parts: [
    { ref: "U1", placed: true, at: [1000, 2000], pads: [{ x: 1100, y: 2000 }, { x: 900, y: 2000 }] },
    { ref: "U2", placed: false }, // unplaced -- no anchors
  ],
  routing: {
    tracks: [{ id: "t1", pts: [[0, 0], [1000, 0], [1000, 1000]] }],
    vias: [{ id: "v1", x: 500, y: 500 }],
  },
};

test("collectAnchors: footprint origin + pad centers for placed parts only", () => {
  const anchors = collectAnchors(board, { pads: true, tracks: false, footprintOrigins: true });
  assert.deepEqual(
    anchors.filter((a) => a.kind === "footprint-origin"),
    [{ x: 1000, y: 2000, kind: "footprint-origin", ownerId: "U1" }]
  );
  assert.equal(anchors.filter((a) => a.kind === "pad").length, 2, "both of U1's pads, none for the unplaced U2");
});

test("collectAnchors: track endpoints and midpoints for every segment, plus via centers", () => {
  const anchors = collectAnchors(board, { pads: false, tracks: true, footprintOrigins: false });
  const ends = anchors.filter((a) => a.kind === "track-end");
  const mids = anchors.filter((a) => a.kind === "track-mid");
  const vias = anchors.filter((a) => a.kind === "via");
  assert.deepEqual(new Set(ends.map((a) => `${a.x},${a.y}`)), new Set(["0,0", "1000,1000"]), "first and last point of the 3-point track");
  assert.deepEqual(
    new Set(mids.map((a) => `${a.x},${a.y}`)),
    new Set(["500,0", "1000,500"]),
    "midpoint of each of the track's two segments"
  );
  assert.deepEqual(vias, [{ x: 500, y: 500, kind: "via", ownerId: "v1" }]);
});

test("collectAnchors: magnetic settings gate whole anchor categories", () => {
  assert.equal(collectAnchors(board, { pads: false, tracks: false, footprintOrigins: false }).length, 0);
  assert.equal(collectAnchors(board, DEFAULT_MAGNETIC_SETTINGS).length > 0, true);
});

test("collectAnchors: excludeOwnerId drops a specific item's own anchors (e.g. don't snap a dragged part to itself)", () => {
  const anchors = collectAnchors(board, DEFAULT_MAGNETIC_SETTINGS, "U1");
  assert.equal(anchors.some((a) => a.ownerId === "U1"), false);
  assert.equal(anchors.some((a) => a.ownerId === "v1"), true);
});
