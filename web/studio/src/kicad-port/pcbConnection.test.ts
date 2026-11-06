import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part, Shape } from "../api/types";
import { connectedItemAt, connectedStartCount, expandedSelection, isExpandableGraphicShape, planSelectConnection, selectAllConnectedShapes } from "./pcbConnection";

const pad = (num: string, x: number, y: number, net: string | null): Pad => ({ num, net, x, y, w: 600, h: 600, round: false, th: false });
const part = (ref: string, at: [number, number], pads: Pad[]): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads }) as Part;
const track = (id: string, net: string, a: [number, number], b: [number, number]) => ({ id, net, layer: "F.Cu", width: 200, pts: [a, b] });

// N: t1 - t2 - t3 in a row from (0, 0) to (3000, 0), a pad of U1 at (0, 0); M: one track far away. A line on F.SilkS.
function board(): BoardState {
  return {
    name: "t",
    dir: "",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [part("U1", [0, 0], [pad("1", 0, 0, "N")])],
    rules: [],
    routing: { tracks: [track("t1", "N", [0, 0], [1000, 0]), track("t2", "N", [1000, 0], [2000, 0]), track("t3", "N", [2000, 0], [3000, 0]), track("m1", "M", [0, 5000], [1000, 5000])], vias: [], zones: [], track_width_presets: [], via_presets: [], teardrop_settings: {} },
    drawings: { shapes: [{ id: "s1", layer: "F.SilkS", stroke_width: 150, filled: false, kind: "segment", start: [0, 9000], end: [1000, 9000] }], texts: [], groups: [], dimensions: [], dimension_settings: {} },
    checks: [],
    activity: [],
    job: "idle",
  } as unknown as BoardState;
}

test("connectedStartCount: footprints and connected items start a connection, a graphic line does not", () => {
  const b = board();
  assert.equal(connectedStartCount(b, ["U1", "t1", "U1.1", "s1"]), 3);
  assert.equal(connectedStartCount(b, ["s1"]), 0);
  assert.equal(connectedStartCount(b, []), 0);
});

test("connectedItemAt: the first pad, track or via under the cursor; a footprint, a zone or a graphic is not a connected item", () => {
  assert.equal(connectedItemAt([{ kind: "part", id: "U1" }, { kind: "track", id: "t2" }, { kind: "pad", id: "U1.1" }]), "t2");
  assert.equal(connectedItemAt([{ kind: "zone", id: "z" }, { kind: "shape", id: "s1" }]), null);
  assert.equal(connectedItemAt([]), null);
});

test("with a track selected the flood walks its connection, the other net stays out", () => {
  const b = board();
  const refs = expandedSelection(b, new Set(["t1"]))!;
  assert.deepEqual(refs.sort(), ["t1", "t2", "t3"]);
});

test("a selected pad starts a connection as a track does (`BOARD_CONNECTED_ITEM`)", () => {
  const refs = expandedSelection(board(), new Set(["U1.1"]))!;
  assert.deepEqual(refs.sort(), ["U1.1", "t1", "t2", "t3"]);
});

test("Select Connection with nothing selected takes the connected item under the cursor and expands from it", () => {
  const b = board();
  const plan = planSelectConnection(b, new Set(), [{ kind: "part", id: "U1" }, { kind: "track", id: "t2" }]);
  assert.deepEqual(plan && plan.refs.sort(), ["t1", "t2", "t3"]);
  // nothing under the cursor, nothing selected: nothing to do
  assert.equal(planSelectConnection(b, new Set(), []), null);
  assert.equal(planSelectConnection(b, new Set(), [{ kind: "shape", id: "s1" }]), null, "a graphic under the cursor is not a connected item");
});

test("a selection with nothing connected in it is cleared for the item under the cursor, as `selectCursor( true, ..)` does", () => {
  const b = { ...board(), drawings: { ...board().drawings!, texts: [{ id: "x1", content: "hi", x: 0, y: 0, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 150, justify: "center", mirror: false }] } } as unknown as BoardState;
  assert.deepEqual(planSelectConnection(b, new Set(["x1"]), [{ kind: "track", id: "m1" }])!.refs.sort(), ["m1"], "m1 is its net's only track");
  assert.deepEqual(planSelectConnection(b, new Set(["x1"]), []), { refs: [] }, "and nothing under the cursor leaves the selection empty");
});

test("a selected graphic line with no collector to search with is left alone", () => {
  assert.equal(planSelectConnection(board(), new Set(["s1"]), [{ kind: "track", id: "t2" }]), null);
});

/** `EDA_SHAPE::hitTest` of a line: the distance to it less half its width. */
const lineHit = (sh: Shape, x: number, y: number): number => {
  if (sh.kind !== "segment") return Infinity;
  const [ax, ay] = sh.start;
  const [bx, by] = sh.end;
  const t = Math.max(0, Math.min(1, ((x - ax) * (bx - ax) + (y - ay) * (by - ay)) / ((bx - ax) ** 2 + (by - ay) ** 2 || 1)));
  return Math.hypot(x - (ax + t * (bx - ax)), y - (ay + t * (by - ay))) - sh.stroke_width / 2;
};
const seg = (id: string, layer: string, a: [number, number], b: [number, number]): Shape => ({ id, layer, stroke_width: 150, filled: false, kind: "segment", start: a, end: b }) as Shape;

/** A chain s1 - s2 - s3 on F.SilkS (s3 starting on the middle of s2, a T), s4 on F.Fab touching s3's end, s5 on F.SilkS far away, c1 on F.Cu. */
function linesBoard(): BoardState {
  const b = board();
  b.drawings!.shapes = [
    seg("s1", "F.SilkS", [0, 9000], [1000, 9000]),
    seg("s2", "F.SilkS", [1000, 9000], [3000, 9000]),
    seg("s3", "F.SilkS", [2000, 9000], [2000, 12000]),
    seg("s4", "F.Fab", [2000, 12000], [4000, 12000]),
    seg("s5", "F.SilkS", [9000, 9000], [9500, 9000]),
    seg("c1", "F.Cu", [0, 9000], [0, 7000]),
  ];
  return b;
}

test("selectAllConnectedShapes: the lines that meet end to end on one layer, a line hit along its length too, not another layer's or copper", () => {
  const b = linesBoard();
  assert.equal(isExpandableGraphicShape(b, "c1"), false, "a line on copper is not followed this way");
  // the search is at each line's two ends: from s1, s2's ends find s1 again and nothing else (s3 starts on s2's middle, which nobody searches at)
  assert.deepEqual(selectAllConnectedShapes(b, ["s1"], { accuracyNm: 50, hit: lineHit }).sort(), ["s1", "s2"]);
  // from s3 its start, on s2's middle, hits s2 along its length (the collector hits a line anywhere), and s2's end s1; s4 is on F.Fab, s5 meets nothing
  const got = selectAllConnectedShapes(b, ["s3", "s3"], { accuracyNm: 50, hit: lineHit });
  assert.deepEqual(got, ["s3", "s2", "s1"], "depth first from a stack, each line once");
});

test("Select Connection on lines: the chain is added to the selection; a line the filter refuses is neither selected nor followed", () => {
  const b = linesBoard();
  const shapes = { accuracyNm: 50, hit: lineHit };
  assert.deepEqual(planSelectConnection(b, new Set(["s3"]), [], { shapes })!.refs.sort(), ["s1", "s2", "s3"]);
  const plan = planSelectConnection(b, new Set(["s3"]), [], { shapes, passes: (kind, id) => !(kind === "shape" && id === "s2") });
  assert.deepEqual(plan!.refs.sort(), ["s3"], "s2 refused: s1 beyond it is not reached");
});

test("the track walk takes the selection filter: a track it refuses is not selected or walked, a pad or via it refuses ends the walk there", () => {
  const b = board();
  const plan = planSelectConnection(b, new Set(["t1"]), [], { passes: (kind, id) => !(kind === "track" && id === "t2") });
  assert.deepEqual(plan!.refs.sort(), ["t1"], "t2 refused: t3 beyond it is not reached");
  // a start pad refused: nothing is walked from it
  const fromPad = expandedSelection(b, new Set(["U1.1"]), undefined, (kind) => kind !== "pad");
  assert.deepEqual(fromPad, ["U1.1"]);
});

test("with a connected item selected the cursor is not looked at", () => {
  const b = board();
  const plan = planSelectConnection(b, new Set(["m1"]), [{ kind: "track", id: "t2" }]);
  assert.deepEqual(plan && plan.refs, ["m1"], "the flood from m1 adds nothing, and t2 under the cursor is ignored");
});

test("a tuning pattern selected alone starts the connection from its tracks (expandConnection counts a PCB_GENERATOR_T, round 4 A-7)", () => {
  const b = board();
  b.drawings!.groups = [{ id: "tp1", name: "", member_ids: ["t2"], generator: "tuning_pattern" }];
  assert.equal(connectedStartCount(b, ["tp1"]), 1);
  const plan = planSelectConnection(b, new Set(["tp1"]), []);
  assert.ok(plan && plan.refs.includes("tp1") && plan.refs.includes("t1") && plan.refs.includes("t3"), JSON.stringify(plan));
});
