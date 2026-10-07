import { test } from "node:test";
import assert from "node:assert/strict";
import { alignToSegment, breakTrack, filletTracks, itemsJoinedAt, type EditBoard, type EditTrack } from "./pcbTrackEdit";
import { tessellateArc, splitArcAt, arcRouteTrack } from "./trackArc";

const track = (id: string, pts: [number, number][], extra: Partial<EditTrack> = {}): EditTrack => ({ id, net: "N", layer: "F.Cu", width: 200, pts, ...extra });
const board = (tracks: EditTrack[], extra: Partial<EditBoard> = {}): EditBoard => ({ tracks, vias: [], pads: [], layers: ["F.Cu", "B.Cu"], ...extra });

test("tessellateArc matches the Rust tessellation shape: exact end points, 33 points, all on the circle", () => {
  const pts = tessellateArc([1000, 0], [0, 1000], [-1000, 0]);
  assert.equal(pts.length, 33);
  assert.deepEqual(pts[0], [1000, 0]);
  assert.deepEqual(pts[32], [-1000, 0]);
  for (const [x, y] of pts) assert.ok(Math.abs(Math.hypot(x, y) - 1000) <= 1, `${x},${y}`);
  // the half way point passes through mid
  assert.ok(Math.abs(pts[16]![0]) <= 1 && Math.abs(pts[16]![1] - 1000) <= 1);
});

test("arcRouteTrack stores the mid point as an offset from the first point", () => {
  const t = arcRouteTrack("N", "F.Cu", 200, [1000, 0], [0, 1000], [-1000, 0]);
  assert.deepEqual(t.arc_mid_offset, { x: -1000, y: 1000 });
  assert.equal(t.pts.length, 33);
});

test("splitArcAt: two arcs sharing the break point, each through its own half", () => {
  const halves = splitArcAt([1000, 0], [0, 1000], [-1000, 0], [0, 1000])!;
  assert.ok(halves);
  const [a, b] = halves;
  assert.deepEqual(a.start, [1000, 0]);
  assert.deepEqual(a.end, [0, 1000]);
  assert.deepEqual(b.start, [0, 1000]);
  assert.deepEqual(b.end, [-1000, 0]);
  // each half's mid sits on the circle half way along its own sweep (45 and 135 degrees)
  assert.ok(Math.abs(a.mid[0] - 707) <= 1 && Math.abs(a.mid[1] - 707) <= 1);
  assert.ok(Math.abs(b.mid[0] + 707) <= 1 && Math.abs(b.mid[1] - 707) <= 1);
  // a point outside the arc's span is refused
  assert.equal(splitArcAt([1000, 0], [0, 1000], [-1000, 0], [0, -1000]), null);
});

test("alignToSegment: a grid-line crossing wins over the closest end point (PCB_GRID_HELPER::AlignToSegment)", () => {
  // a horizontal segment 0..100000 nm; the aligned pointer sits above its middle
  const seg = { a: [0, 0] as [number, number], b: [100000, 0] as [number, number] };
  const at = alignToSegment([50000, 3000], [50000, 5000], seg, true);
  assert.deepEqual(at, [50000, 0]);
  // with snapping off the aligned point comes back untouched
  assert.deepEqual(alignToSegment([50000, 3000], [50000, 5000], seg, false), [50000, 5000]);
});

test("breakTrack: a polyline splits into two tracks at the point under the pointer", () => {
  const t = track("t", [[0, 0], [10000, 0], [10000, 8000]]);
  const r = breakTrack(board([t]), t, [4000, 30], 1000);
  assert.equal(r.ok, true);
  assert.equal(r.cmd?.op, "commit_route");
  if (r.cmd?.op !== "commit_route") return;
  assert.deepEqual(r.cmd.remove_track_ids, ["t"]);
  const [left, right] = r.cmd.tracks!;
  assert.deepEqual(left!.pts, [{ x: 0, y: 0 }, { x: 4000, y: 0 }]);
  assert.deepEqual(right!.pts, [{ x: 4000, y: 0 }, { x: 10000, y: 0 }, { x: 10000, y: 8000 }]);
  assert.equal(left!.net, "N");
  assert.equal(right!.width, 200);
});

test("breakTrack is refused at an end point, a vertex or where another item is joined", () => {
  const t = track("t", [[0, 0], [10000, 0]]);
  // at an end point (within half a track width)
  assert.equal(breakTrack(board([t]), t, [30, 20], 1000).ok, false);
  // another track ends at the break point (a T junction)
  const stub = track("s", [[4000, 0], [4000, 5000]]);
  assert.equal(breakTrack(board([t, stub]), t, [4000, 20], 1000).ok, false);
  // a via on the net at the point
  assert.equal(breakTrack(board([t], { vias: [{ net: "N", x: 4000, y: 0, from: "F.Cu", to: "B.Cu" }] }), t, [4000, 20], 1000).ok, false);
  // a pad of another net there does not stop it
  assert.equal(breakTrack(board([t], { pads: [{ net: "OTHER", x: 4000, y: 0, layers: ["F.Cu"] }] }), t, [4000, 20], 1000).ok, true);
});

test("breakTrack on an arc track makes two arc tracks", () => {
  const pts = tessellateArc([10000, 0], [0, 10000], [-10000, 0]);
  const t = track("a", pts, { arc_mid: [0, 10000] });
  const r = breakTrack(board([t]), t, [0, 10050], 1000);
  assert.equal(r.ok, true);
  if (r.cmd?.op !== "commit_route") return;
  assert.equal(r.cmd.tracks!.length, 2);
  assert.ok(r.cmd.tracks!.every((x) => x.arc_mid_offset !== undefined));
});

test("itemsJoinedAt counts track vertices, vias spanning the layer and pads on it", () => {
  const b = board([track("a", [[0, 0], [100, 0]]), track("b", [[100, 0], [100, 100]])], {
    vias: [{ net: "N", x: 100, y: 0, from: "F.Cu", to: "B.Cu" }],
    pads: [{ net: "N", x: 100, y: 0, layers: ["B.Cu"] }],
  });
  const j = itemsJoinedAt(b, [100, 0], "F.Cu", "N");
  assert.equal(j.tracks.length, 2);
  assert.equal(j.vias, 1);
  assert.equal(j.pads, 0); // the pad is on the other layer
});

test("filletTracks rounds the corner of one polyline and returns it as pieces plus an arc", () => {
  const t = track("t", [[0, 0], [10000, 0], [10000, 10000]]);
  const r = filletTracks(board([t]), [t], 1000);
  assert.equal(r.ok, true);
  assert.equal(r.message, null);
  assert.equal(r.arcs, 1);
  if (r.cmd?.op !== "commit_route") return;
  assert.deepEqual(r.cmd.remove_track_ids, ["t"]);
  const [first, second, arc] = r.cmd.tracks!;
  assert.deepEqual(first!.pts, [{ x: 0, y: 0 }, { x: 9000, y: 0 }]);
  assert.deepEqual(second!.pts, [{ x: 10000, y: 1000 }, { x: 10000, y: 10000 }]);
  assert.ok(arc!.arc_mid_offset);
  assert.deepEqual(arc!.pts[0], { x: 9000, y: 0 });
  assert.deepEqual(arc!.pts[arc!.pts.length - 1], { x: 10000, y: 1000 });
});

test("filletTracks rounds the join of two separate tracks and an end shared with a third item fails", () => {
  const a = track("a", [[0, 0], [10000, 0]]);
  const b = track("b", [[10000, 0], [10000, 10000]]);
  const ok = filletTracks(board([a, b]), [a, b], 1000);
  assert.equal(ok.ok, true);
  assert.equal(ok.arcs, 1);
  // a via on the join: "there are other elements connected at that point"
  const blocked = filletTracks(board([a, b], { vias: [{ net: "N", x: 10000, y: 0, from: "F.Cu", to: "B.Cu" }] }), [a, b], 1000);
  assert.equal(blocked.ok, false);
  assert.equal(blocked.message, "Unable to fillet the selected track segments.");
});

test("filletTracks needs two segments, skips collinear pairs, and reports an oversize radius", () => {
  const lone = track("t", [[0, 0], [10000, 0]]);
  assert.equal(filletTracks(board([lone]), [lone], 500).message, "At least two straight track segments must be selected.");
  const straight = track("t", [[0, 0], [5000, 0], [10000, 0]]);
  assert.equal(filletTracks(board([straight]), [straight], 500).ok, false);
  const corner = track("c", [[0, 0], [1000, 0], [1000, 1000]]);
  const big = filletTracks(board([corner]), [corner], 5000);
  assert.equal(big.ok, false);
});

test("filletTracks rounds every corner of a staircase", () => {
  const t = track("t", [[0, 0], [10000, 0], [10000, 10000], [20000, 10000]]);
  const r = filletTracks(board([t]), [t], 1000);
  assert.equal(r.ok, true);
  assert.equal(r.arcs, 2);
  if (r.cmd?.op !== "commit_route") return;
  // three straight pieces (the middle one shortened at both ends) and two arcs
  assert.equal(r.cmd.tracks!.length, 5);
});
