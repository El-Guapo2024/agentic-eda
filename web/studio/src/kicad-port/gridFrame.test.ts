import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardGridItem } from "../api/pcbItemTypes";
import type { BoardState } from "../api/types";
import { carryFrameTurn, frameFootprint, gridFrameAngleAt, gridFrameRotationDelta, gridOrientationAt, placementFrameTurn } from "./gridFrame";
import { alignWithGridItems, newGridItem } from "./gridItem";

const MM = 1_000_000;

function grid(patch: Partial<BoardGridItem>): BoardGridItem {
  return { ...newGridItem(50 * MM, 0), id: "grd_a", extent: [10 * MM, 10 * MM], angle: 30, ...patch };
}

function board(grids: BoardGridItem[]): BoardState {
  const part = (ref: string, x: number) => ({ ref, placed: true, at: [x, 0], rot: 0, pads: [{ number: "1" }] });
  return { parts: [part("U1", 0), part("U2", 5 * MM)], drawings: { grid_items: grids } } as unknown as BoardState;
}

test("GridFrameRotationDelta: never more than a quarter turn (or the rotation step)", () => {
  assert.equal(gridFrameRotationDelta(0, 30, 90), 30);
  assert.equal(gridFrameRotationDelta(0, 60, 90), -30, "60 is a quarter turn less 30");
  assert.equal(gridFrameRotationDelta(0, 45, 90), -45, "KiROUND( 0.5 ) is 1");
  assert.equal(gridFrameRotationDelta(0, -45, 90), 45);
  assert.equal(gridFrameRotationDelta(0, 100, 90), 10);
  assert.equal(gridFrameRotationDelta(0, 30, 15), 0, "a 15 degree step: 30 is two steps");
  assert.equal(gridFrameRotationDelta(30, 30, 90), 0);
  assert.equal(gridFrameRotationDelta(30, 0, 90), -30);
});

test("GetOrientationAt: a cartesian grid's orientation, a polar grid's spoke", () => {
  assert.equal(gridOrientationAt(grid({}), [0, 0]), 30);
  const polar = grid({ kind: "polar", x: 0, y: 0, angle: 12 });
  assert.equal(gridOrientationAt(polar, [10, 0]), 0);
  assert.equal(gridOrientationAt(polar, [0, -10]), 90, "up the screen is counter-clockwise");
  assert.equal(gridOrientationAt(polar, [0, 0]), 12, "at the centre: its orientation");
  // `FindActiveGridAt( .., PLACEMENT )`: only a grid that affects placement, and not a selected one.
  assert.equal(gridFrameAngleAt([grid({})], [50 * MM, 0], "placement"), 30);
  assert.equal(gridFrameAngleAt([grid({ affects: { cursor: true, routing: true, placement: false } })], [50 * MM, 0], "placement"), 0);
  assert.equal(gridFrameAngleAt([grid({})], [50 * MM, 0], "placement", new Set(["grd_a"])), 0);
  assert.equal(gridFrameAngleAt([grid({})], [0, 0], "placement"), 0, "outside every grid");
  assert.deepEqual(placementFrameTurn([grid({ angle: 60 })], [50 * MM, 0], 0, 90), { turnDeg: -30, frameDeg: 60 }, "a footprint fresh from the library");
});

test("a footprint carried into a placement grid turns to its frame about its own position, and back out of it", () => {
  const b = board([grid({})]);
  const first = carryFrameTurn(b, ["U1"], [], 50 * MM, 0, [0, 0], undefined, 90)!;
  assert.deepEqual(first.ops, [{ kind: "turn", degCcw: 30, pivot: [0, 0] }], "about the footprint's position before the move");
  assert.equal(first.frameDeg, 30);
  const inside = carryFrameTurn(b, ["U1"], first.ops, 52 * MM, MM, [0, 0], first.frameDeg, 90)!;
  assert.equal(inside.ops.length, 1, "still in the same frame: no new turn");
  const out = carryFrameTurn(b, ["U1"], inside.ops, 0, 0, [0, 0], inside.frameDeg, 90)!;
  assert.deepEqual(out.ops[1], { kind: "turn", degCcw: -30, pivot: [0, 0] });
  // Picked up inside the grid: "the first cursor move must not rotate it again".
  const inGrid = board([grid({ x: 0 })]);
  assert.equal(carryFrameTurn(inGrid, ["U1"], [], MM, 0, [0, 0], undefined, 90)!.ops.length, 0);
  // Two footprints turn about the cursor (the grab point); a lone pad, or anything without a footprint, does not turn.
  const two = carryFrameTurn(b, ["U1", "U2"], [], 50 * MM, 0, [3 * MM, 0], undefined, 90)!;
  assert.deepEqual(two.ops, [{ kind: "turn", degCcw: 30, pivot: [3 * MM, 0] }]);
  assert.equal(carryFrameTurn(b, ["trk_a"], [], 50 * MM, 0, [0, 0], undefined, 90), null);
  assert.deepEqual(frameFootprint(b, ["U1", "U2"]), { single: null, any: true });
  // A grid that affects the cursor but not placement snaps the cursor and turns nothing; one that affects placement only turns and does not snap.
  const cursorOnly = grid({ affects: { cursor: true, routing: true, placement: false } });
  assert.equal(carryFrameTurn(board([cursorOnly]), ["U1"], [], 50 * MM, 0, [0, 0], undefined, 90)!.ops.length, 0);
  const placementOnly = grid({ affects: { cursor: false, routing: false, placement: true } });
  assert.deepEqual(alignWithGridItems([placementOnly], [50.3 * MM, 0.2 * MM], [50 * MM, 0], 1000), [50 * MM, 0], "the editor's grid point: the grid does not own the cursor");
});
