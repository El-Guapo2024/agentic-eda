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

// r5 R5-1: `pickSingleItem`'s top layer -- a pad or via, then a track, on it before any on another layer; the end of a route takes only its layer's.
test("the top layer's items come first, another layer's are picked when it has none, and an end takes only its layer's", () => {
  const onB = (t: TrackItem): TrackItem => ({ ...t, layers: ["B.Cu"] });
  const scene: SnapScene = { items: [onB(track("gnd", [0, 0], [4000, 0])), track("other", [0, 300], [4000, 300])] };
  // the cursor is on the B.Cu track's copper and within a grid of the F.Cu one: slop 0 finds only the B.Cu one, which is the pick
  assert.equal(pickRouteItem(board, scene, [2000, 50], 1000, null, visible, undefined, { top: "F.Cu" })?.owner, "gnd");
  // both under the cursor (wide tracks): the top layer's wins, whichever it is
  const wide: SnapScene = { items: [onB(track("gnd", [0, 0], [4000, 0], 1000)), track("other", [0, 300], [4000, 300], 1000)] };
  assert.equal(pickRouteItem(board, wide, [2000, 150], 1000, null, visible, undefined, { top: "F.Cu" })?.owner, "other");
  assert.equal(pickRouteItem(board, wide, [2000, 150], 1000, null, visible, undefined, { top: "B.Cu" })?.owner, "gnd");
  // a route's end on F.Cu: the B.Cu track under the cursor is no end, and nothing is picked (the slop pass that found it ends the search)
  assert.equal(pickRouteItem(board, scene, [2000, 50], 1000, null, visible, undefined, { top: "F.Cu", only: true }), null);
});
