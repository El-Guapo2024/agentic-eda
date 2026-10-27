import { test } from "node:test";
import assert from "node:assert/strict";
import { selectAllConnectedTracks, type ConnPad, type ConnSeg, type ConnStart, type ConnVia, type StopCondition } from "./expandConnection";

const seg = (track: string, start: [number, number], end: [number, number], layer = "F.Cu", i = 0): ConnSeg => ({ track, seg: i, layer, start, end });
const via = (id: string, at: [number, number], diameter = 600): ConnVia => ({ id, at, diameter, layers: ["F.Cu", "B.Cu"] });
const pad = (id: string, at: [number, number], layers = ["F.Cu"]): ConnPad => ({ id, at, layers });

function walk(segs: ConnSeg[], vias: ConnVia[], pads: ConnPad[], starts: ConnStart[], stop: StopCondition) {
  const selected = { trackIds: new Set<string>(), viaIds: new Set<string>() };
  selectAllConnectedTracks({ segs, vias, pads }, starts, stop, selected);
  return { tracks: [...selected.trackIds].sort(), vias: [...selected.viaIds].sort() };
}

test("selectAllConnectedTracks: a chain with no junction is walked end to end", () => {
  const segs = [seg("t1", [0, 0], [100, 0]), seg("t2", [100, 0], [200, 0]), seg("t3", [200, 0], [300, 0])];
  assert.deepEqual(walk(segs, [], [], [{ kind: "track", id: "t1" }], "junction").tracks, ["t1", "t2", "t3"]);
});

test("STOP_AT_JUNCTION stops at three tracks, a via or a pad that is not a start pad; STOP_AT_PAD only at the pad", () => {
  const branch = [seg("t1", [0, 0], [100, 0]), seg("t2", [100, 0], [200, 0]), seg("t3", [100, 0], [100, 100])];
  assert.deepEqual(walk(branch, [], [], [{ kind: "track", id: "t1" }], "junction").tracks, ["t1"]);
  assert.deepEqual(walk(branch, [], [], [{ kind: "track", id: "t1" }], "pad").tracks, ["t1", "t2", "t3"]);
  const through = [seg("t1", [0, 0], [100, 0]), seg("t2", [100, 0], [200, 0])];
  assert.deepEqual(walk(through, [via("v1", [100, 0])], [], [{ kind: "track", id: "t1" }], "junction"), { tracks: ["t1"], vias: [] });
  assert.deepEqual(walk(through, [via("v1", [100, 0])], [], [{ kind: "track", id: "t1" }], "pad"), { tracks: ["t1", "t2"], vias: ["v1"] });
});

test("stop_at_junction_halts_at_a_non_start_pad: tracks A, B and C meet at R2.1, and A gives {A, B} at STOP_AT_PAD... and {A} at the junction (round 3 D-6)", () => {
  // A ends on R2.1; B goes from R2.1 on, C too: KiCad's `gotNonStartPad` halts the walk at the pad, ours walked through it
  const segs = [seg("A", [0, 0], [100, 0]), seg("B", [100, 0], [200, 0]), seg("C", [100, 0], [100, 100])];
  const pads = [pad("R2.1", [100, 0])];
  assert.deepEqual(walk(segs, [], pads, [{ kind: "track", id: "A" }], "junction").tracks, ["A"]);
  assert.deepEqual(walk(segs, [], pads, [{ kind: "track", id: "A" }], "pad").tracks, ["A"]);
  assert.deepEqual(walk(segs, [], pads, [{ kind: "track", id: "A" }], "never").tracks, ["A", "B", "C"]);
  // two tracks through a pad that is a start pad go on
  assert.deepEqual(walk([seg("A", [0, 0], [100, 0]), seg("B", [100, 0], [200, 0])], [], pads, [{ kind: "pad", id: "R2.1" }], "junction").tracks, ["A", "B"]);
});

test("a_branch_at_an_inner_vertex_is_followed (round 3 D-6)", () => {
  // T1 is a polyline (0,0)-(100,0)-(200,0); T2 leaves its inner vertex
  const segs = [seg("T1", [0, 0], [100, 0], "F.Cu", 0), seg("T1", [100, 0], [200, 0], "F.Cu", 1), seg("T2", [100, 0], [100, 300])];
  assert.deepEqual(walk(segs, [], [], [{ kind: "track", id: "T1" }], "pad").tracks, ["T1", "T2"]);
  // the inner vertex is a junction of three segments: STOP_AT_JUNCTION does not cross it
  assert.deepEqual(walk(segs, [], [], [{ kind: "track", id: "T2" }], "junction").tracks, ["T2"]);
});

test("tracks on different layers that end on one point join only through a via; a via off the point joins within its radius (round 3 D-6)", () => {
  const segs = [seg("f", [0, 0], [100, 0], "F.Cu"), seg("b", [100, 0], [200, 0], "B.Cu")];
  assert.deepEqual(walk(segs, [], [], [{ kind: "track", id: "f" }], "never").tracks, ["f"]);
  assert.deepEqual(walk(segs, [via("v", [100, 200], 600)], [], [{ kind: "track", id: "f" }], "never"), { tracks: ["b", "f"], vias: ["v"] });
});

test("the walk ignores nets (IGNORE_NETS): copper of another net that ends on the point is walked into", () => {
  // the seg type has no net: every track is walked, as `GetConnectedItems( .., IGNORE_NETS )` hands them
  assert.deepEqual(walk([seg("n1", [0, 0], [100, 0]), seg("n2", [100, 0], [200, 0])], [], [], [{ kind: "track", id: "n1" }], "junction").tracks, ["n1", "n2"]);
});
