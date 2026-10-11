import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { box, seg, type Pt } from "./snapGeom";
import type { SnapScene, TrackItem } from "./snapScene";
import { pickRouteItem } from "./routePick";

const board = { parts: [], routing: { tracks: [{ id: "dragged", net: "SIG" }, { id: "other", net: "SIG" }, { id: "gnd", net: "GND" }], vias: [] } } as unknown as BoardState;

function track(owner: string, a: Pt, b: Pt, width = 250): TrackItem {
  return { id: `${owner}#0`, owner, type: "track", layers: ["F.Cu"], bbox: box(a[0], a[1], b[0], b[1]), geom: seg(a, b), hit: () => false, start: a, end: b, center: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], width };
}

const visible = () => true;

test("the track whose copper the cursor is in is picked, the nearest end first among those within a grid", () => {
  const scene: SnapScene = { items: [track("dragged", [0, 0], [4000, 0]), track("other", [0, 800], [4000, 800])] };
  assert.equal(pickRouteItem(board, scene, [2000, 60], 1000, null, visible)?.id, "dragged#0", "on the dragged track's copper");
  assert.equal(pickRouteItem(board, scene, [3900, 600], 1000, null, visible)?.id, "other#0", "within a grid of both, the nearer end");
  assert.equal(pickRouteItem(board, scene, [2000, 5000], 1000, null, visible), null, "nothing near");
});

test("a drag does not pick the tracks of the line it is dragging", () => {
  const scene: SnapScene = { items: [track("dragged", [0, 0], [4000, 0]), track("other", [0, 800], [4000, 800])] };
  // the cursor is on the dragged track: without the skip it would stick to it, with it the neighbouring track within the grid is found instead
  assert.equal(pickRouteItem(board, scene, [2000, 60], 1000, null, visible)?.owner, "dragged");
  assert.equal(pickRouteItem(board, scene, [2000, 60], 1000, null, visible, new Set(["dragged"]))?.owner, "other");
  // and when nothing else is near, nothing is: the cursor goes to the grid
  assert.equal(pickRouteItem(board, scene, [2000, 60], 100, null, visible, new Set(["dragged", "other"])), null);
});

test("only the net's tracks when a net is given, only visible ones", () => {
  const scene: SnapScene = { items: [track("gnd", [0, 0], [4000, 0]), track("other", [0, 300], [4000, 300])] };
  assert.equal(pickRouteItem(board, scene, [2000, 100], 1000, "SIG", visible)?.owner, "other");
  assert.equal(pickRouteItem(board, scene, [2000, 100], 1000, "SIG", () => false), null);
});

test("a route does not pick the item it started on: a skipped pad, via or one segment", () => {
  const scene: SnapScene = { items: [track("other", [0, 0], [4000, 0]), track("other", [0, 5000], [4000, 5000])].map((t, i) => ({ ...t, id: `other#${i}` })) };
  // the cursor is on the first segment of "other": skipping that segment (a route that started on it) leaves the second, a grid away or not
  assert.equal(pickRouteItem(board, scene, [2000, 60], 1000, null, visible)?.id, "other#0");
  assert.equal(pickRouteItem(board, scene, [2000, 60], 1000, null, visible, new Set(["other#0"])), null, "only that segment is skipped, not the whole track: the other one is too far");
  assert.equal(pickRouteItem(board, scene, [2000, 4900], 1000, null, visible, new Set(["other#0"]))?.id, "other#1");
});
