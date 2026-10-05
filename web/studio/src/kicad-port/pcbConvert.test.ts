import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Cmd, Shape } from "../api/types";
import { arcMidOfSegment, convertAvailability, copiedLineWidth, DEFAULT_CONVERT_SETTINGS, DEFAULT_OUTSET_PARAMS, outsetShapes, planConvertToLines, planConvertToTracks, planSegmentToArc, resolveConvertSettings, ringEdges, sourceRings, type OutsetParams } from "./pcbConvert";

function board(partial: Partial<BoardState>): BoardState {
  return {
    name: "t",
    dir: "",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [],
    rules: [],
    routing: null,
    drawings: null,
    checks: [],
    activity: [],
    job: "idle",
    ...partial,
  } as BoardState;
}

const drawings = (shapes: Shape[]) => ({ shapes, texts: [], groups: [], dimensions: [] }) as unknown as BoardState["drawings"];
const seg = (id: string, ax: number, ay: number, bx: number, by: number, w = 150, layer = "F.SilkS"): Shape => ({ kind: "segment", id, layer, stroke_width: w, filled: false, start: [ax, ay], end: [bx, by] });
const rect = (id: string, x0: number, y0: number, x1: number, y1: number, w = 100, filled = false): Shape => ({ kind: "rect", id, layer: "F.Fab", stroke_width: w, filled, start: [x0, y0], end: [x1, y1] });
const poly = (id: string, pts: [number, number][]): Shape => ({ kind: "polygon", id, layer: "F.Fab", stroke_width: 120, filled: false, pts });
const zone = (id: string, outline: [number, number][]) => ({ id, net: "GND", layer: "F.Cu", teardrop: false, outline });

test("resolved settings: a non-centerline strategy with no width takes the layer's, and a positive hull gap also covers half of it", () => {
  const hull = resolveConvertSettings({ ...DEFAULT_CONVERT_SETTINGS, strategy: "bounding_hull", gapUm: 300 }, 200);
  assert.equal(hull.widthUm, 200);
  assert.equal(hull.gapUm, 400);
  const centre = resolveConvertSettings({ ...DEFAULT_CONVERT_SETTINGS, strategy: "centerline", gapUm: 300 }, 200);
  assert.equal(centre.widthUm, 0);
  assert.equal(centre.gapUm, 300);
  const zeroGap = resolveConvertSettings({ ...DEFAULT_CONVERT_SETTINGS, strategy: "bounding_hull", gapUm: 0 }, 200);
  assert.equal(zeroGap.gapUm, 0);
  const keep = resolveConvertSettings({ ...DEFAULT_CONVERT_SETTINGS, strategy: "copy_linewidth", widthUm: 80 }, 200);
  assert.equal(keep.widthUm, 80);
});

test("copy line width: the stroke of the top-left item", () => {
  const b = board({ drawings: drawings([seg("a", 5000, 0, 6000, 0, 300), seg("b", 1000, 9000, 2000, 9000, 110), seg("c", 1000, 2000, 2000, 2000, 90)]) });
  assert.equal(copiedLineWidth(b, ["a", "b", "c"]), 90);
  assert.equal(copiedLineWidth(b, ["nope"]), null);
});

test("availability follows CONVERT_TOOL::Init's conditions", () => {
  const b = board({
    drawings: drawings([seg("s1", 0, 0, 10, 0), seg("s2", 0, 0, 0, 10, 150, "F.Fab"), rect("r", 0, 0, 5, 5), poly("p", [[0, 0], [5, 0], [0, 5]]), { kind: "circle", id: "c", layer: "F.Fab", stroke_width: 1, filled: false, center: [0, 0], end: [3, 0] }]),
    routing: { tracks: [{ id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [100, 0]] }], vias: [], zones: [zone("z", [[0, 0], [9, 0], [0, 9]])] },
  } as unknown as Partial<BoardState>);
  const a = (ids: string[]) => convertAvailability(b, ids);
  assert.ok(a(["s1"]).poly && a(["s1"]).arc && a(["s1"]).tracks && a(["s1"]).outset && !a(["s1"]).lines);
  assert.ok(!a(["s1", "s2"]).poly, "two lines on different layers");
  assert.ok(a(["r"]).lines && a(["r"]).tracks && a(["r"]).poly && a(["r"]).outset && !a(["r"]).arc);
  assert.ok(a(["p", "z"]).lines && a(["p", "z"]).zone && a(["p", "z"]).keepout);
  assert.ok(a(["t1"]).poly && a(["t1"]).arc && !a(["t1"]).tracks && !a(["t1"]).lines);
  assert.ok(a(["c"]).poly && !a(["c"]).tracks && !a(["c"]).lines);
  assert.ok(!a([]).any);
});

test("source rings: a rectangle's corners, a polygon's points, a zone's outline", () => {
  const b = board({ drawings: drawings([rect("r", 0, 0, 10, 5), poly("p", [[0, 0], [4, 0], [4, 4]])]), routing: { tracks: [], vias: [], zones: [zone("z", [[1, 1], [2, 1], [2, 2]])] } } as unknown as Partial<BoardState>);
  assert.deepEqual(sourceRings(b, "r"), [[[0, 0], [10, 0], [10, 5], [0, 5]]]);
  assert.deepEqual(sourceRings(b, "p"), [[[0, 0], [4, 0], [4, 4]]]);
  assert.deepEqual(sourceRings(b, "z"), [[[1, 1], [2, 1], [2, 2]]]);
  assert.equal(sourceRings(b, "nope"), null);
  assert.deepEqual(ringEdges([[0, 0], [0, 0], [3, 0], [3, 3]]), [[[0, 0], [3, 0]], [[3, 0], [3, 3]], [[3, 3], [0, 0]]]);
});

test("Create Lines: a rectangle gives four segments with its width, and the source goes when asked", () => {
  const b = board({ drawings: drawings([rect("r", 0, 0, 10, 5, 130)]) });
  const plan = planConvertToLines(b, ["r"], "F.SilkS", 150, true);
  const adds = plan.cmds.filter((c): c is Extract<Cmd, { op: "add_shape" }> => c.op === "add_shape");
  assert.equal(adds.length, 4);
  assert.ok(adds.every((c) => c.shape.layer === "F.SilkS" && c.shape.stroke_width === 130));
  assert.deepEqual(plan.cmds[4], { op: "delete_shape", id: "r" });
  assert.deepEqual(plan.removed, ["r"]);
  const keep = planConvertToLines(b, ["r"], "F.SilkS", 150, false);
  assert.equal(keep.cmds.length, 4);
  assert.deepEqual(keep.removed, []);
});

test("Create Lines: a filled rectangle and a zone have no width of their own, so the fallback is used", () => {
  const b = board({ drawings: drawings([rect("r", 0, 0, 10, 5, 130, true)]), routing: { tracks: [], vias: [], zones: [zone("z", [[0, 0], [9, 0], [0, 9]])] } } as unknown as Partial<BoardState>);
  const plan = planConvertToLines(b, ["r", "z"], "F.Fab", 150, true);
  const widths = plan.cmds.filter((c) => c.op === "add_shape").map((c) => (c as Extract<Cmd, { op: "add_shape" }>).shape.stroke_width);
  assert.ok(widths.every((w) => w === 150));
  assert.deepEqual(plan.cmds.slice(-2), [{ op: "delete_shape", id: "r" }, { op: "delete_zone", id: "z" }]);
});

test("Create Lines: a selection with nothing to convert does nothing", () => {
  const b = board({ drawings: drawings([seg("s", 0, 0, 5, 5)]) });
  const plan = planConvertToLines(b, ["s"], "F.SilkS", 150, true);
  assert.ok(plan.empty);
  assert.deepEqual(plan.cmds, []);
});

test("Create Tracks: segments, arcs and polygon outlines become tracks on the net and layer given", () => {
  const arc: Shape = { kind: "arc", id: "a", layer: "F.SilkS", stroke_width: 200, filled: false, start: [0, 0], mid: [50, -50], end: [100, 0] };
  const b = board({ drawings: drawings([seg("s", 0, 0, 100, 0, 250), arc, poly("p", [[0, 0], [100, 0], [100, 100]])]) });
  const plan = planConvertToTracks(b, ["s", "a", "p"], "F.Cu", "GND", 200, true);
  const route = plan.cmds[0];
  assert.equal(route?.op, "commit_route");
  if (route?.op !== "commit_route") return;
  const tracks = route.tracks ?? [];
  assert.equal(tracks.length, 1 + 1 + 3);
  assert.ok(tracks.every((t) => t.net === "GND" && t.layer === "F.Cu"));
  assert.equal(tracks[0]!.width, 250);
  assert.ok(tracks[1]!.arc_mid_offset, "the arc stays an arc");
  assert.equal(tracks[2]!.width, 120, "a polygon's own width");
  assert.deepEqual(plan.removed.sort(), ["a", "p", "s"]);
  assert.equal(plan.cmds.length, 4);
});

test("Create Arc: a segment is offset a tenth of its length along its normal", () => {
  assert.deepEqual(arcMidOfSegment([0, 0], [1000, 0]), [500, 100]);
  assert.deepEqual(arcMidOfSegment([0, 0], [0, 1000]), [-100, 500]);
});

test("Create Arc: a segment shape becomes an arc shape through the offset mid point, the source stays", () => {
  const b = board({ drawings: drawings([seg("s", 0, 0, 1000, 0, 200, "F.Fab")]) });
  const cmds = planSegmentToArc(b, "s");
  assert.equal(cmds?.length, 1);
  assert.deepEqual(cmds?.[0], { op: "add_shape", shape: { kind: "arc", layer: "F.Fab", stroke_width: 200, filled: false, start: { x: 0, y: 0 }, mid: { x: 500, y: 100 }, end: { x: 1000, y: 0 } } });
});

test("Create Arc: a straight track becomes an arc track on its net, an arc track becomes an arc shape", () => {
  const b = board({
    routing: {
      tracks: [
        { id: "t", net: "N1", layer: "F.Cu", width: 200, pts: [[0, 0], [1000, 0]] },
        { id: "ta", net: "N2", layer: "B.Cu", width: 300, pts: [[0, 0], [250, 80], [500, 100]], arc_mid: [250, 80] },
      ],
      vias: [],
      zones: [],
    },
  } as unknown as Partial<BoardState>);
  const toArc = planSegmentToArc(b, "t");
  const route = toArc?.[0];
  assert.equal(route?.op, "commit_route");
  if (route?.op === "commit_route") {
    assert.equal(route.tracks?.[0]?.net, "N1");
    assert.deepEqual(route.tracks?.[0]?.arc_mid_offset, { x: 500, y: 100 });
  }
  const toShape = planSegmentToArc(b, "ta");
  assert.deepEqual(toShape?.[0], { op: "add_shape", shape: { kind: "arc", layer: "B.Cu", stroke_width: 300, filled: false, start: { x: 0, y: 0 }, mid: { x: 250, y: 80 }, end: { x: 500, y: 100 } } });
  assert.equal(planSegmentToArc(b, "nope"), null);
});

const outset = (over: Partial<OutsetParams> = {}): OutsetParams => ({ ...DEFAULT_OUTSET_PARAMS, ...over });

test("outset: a rectangle grows by the distance on every side", () => {
  const r = outsetShapes([rect("r", 1000, 1000, 5000, 3000, 100)], outset({ outsetUm: 250, roundCorners: false, lineWidthUm: 50 }));
  assert.equal(r.successes, 1);
  assert.deepEqual(r.add, [{ layer: "F.Fab", stroke_width: 100, filled: false, kind: "rect", start: { x: 750, y: 750 }, end: { x: 5250, y: 3250 } }]);
  assert.equal(r.message, null);
});

test("outset: a rectangle with rounded corners is four sides and four arcs", () => {
  const r = outsetShapes([rect("r", 0, 0, 4000, 2000)], outset({ outsetUm: 500, roundCorners: true, useSourceLayers: false, layer: "F.CrtYd", useSourceWidths: false, lineWidthUm: 50 }));
  assert.equal(r.add.filter((s) => s.kind === "segment").length, 4);
  assert.equal(r.add.filter((s) => s.kind === "arc").length, 4);
  assert.ok(r.add.every((s) => (s as { layer: string }).layer === "F.CrtYd" && (s as { stroke_width: number }).stroke_width === 50));
  const sides = r.add.filter((s) => s.kind === "segment") as Array<Extract<Shape, { kind: "segment" }>>;
  // the top side runs from the end of one corner to the start of the next: (-500+500, -500) -> (4500-500, -500)
  assert.ok(sides.some((s) => (s.start as unknown as { y: number }).y === -500 && (s.end as unknown as { y: number }).y === -500));
});

test("outset: the grid rounds a rectangle outwards", () => {
  const r = outsetShapes([rect("r", 1003, 1003, 4997, 2997)], outset({ outsetUm: 0, roundCorners: false, gridRoundingUm: 10 }));
  assert.equal(r.successes, 1);
  const s = r.add[0] as unknown as { start: { x: number; y: number }; end: { x: number; y: number } };
  assert.deepEqual([s.start, s.end], [{ x: 1000, y: 1000 }, { x: 5000, y: 3000 }]);
});

test("outset: a rectangle that shrinks away is a failure", () => {
  const r = outsetShapes([rect("r", 0, 0, 100, 100)], outset({ outsetUm: -60 }));
  assert.equal(r.failures, 1);
  assert.equal(r.successes, 0);
  assert.equal(r.message, "Unable to outset the selected items.");
});

test("outset: a circle gets a bigger circle, or a square when corners are not rounded", () => {
  const c: Shape = { kind: "circle", id: "c", layer: "F.Fab", stroke_width: 100, filled: false, center: [1000, 2000], end: [2000, 2000] };
  const round = outsetShapes([c], outset({ outsetUm: 250 }));
  assert.deepEqual(round.add[0], { layer: "F.Fab", stroke_width: 100, filled: false, kind: "circle", center: { x: 1000, y: 2000 }, end: { x: 2250, y: 2000 } });
  const square = outsetShapes([c], outset({ outsetUm: 250, roundCorners: false }));
  assert.deepEqual(square.add[0], { layer: "F.Fab", stroke_width: 100, filled: false, kind: "rect", start: { x: -250, y: 750 }, end: { x: 2250, y: 3250 } });
  assert.equal(outsetShapes([c], outset({ outsetUm: -2000 })).failures, 1);
});

test("outset: a segment becomes a stadium (two sides, two caps) or an oriented rectangle polygon", () => {
  const s = seg("s", 0, 0, 4000, 0);
  const round = outsetShapes([s], outset({ outsetUm: 500 }));
  assert.equal(round.add.filter((x) => x.kind === "segment").length, 2);
  assert.equal(round.add.filter((x) => x.kind === "arc").length, 2);
  const flat = outsetShapes([s], outset({ outsetUm: 500, roundCorners: false }));
  assert.equal(flat.add.length, 1);
  const p = flat.add[0] as unknown as { kind: string; pts: { x: number; y: number }[] };
  assert.equal(p.kind, "polygon");
  assert.deepEqual(p.pts, [{ x: -500, y: 500 }, { x: -500, y: -500 }, { x: 4500, y: -500 }, { x: 4500, y: 500 }]);
  assert.equal(outsetShapes([s], outset({ outsetUm: 0 })).failures, 1, "a zero outset of a line has no stadium");
});

test("outset: an arc gets its two edges and two end caps; an arc smaller than the outset is skipped", () => {
  const a: Shape = { kind: "arc", id: "a", layer: "F.SilkS", stroke_width: 150, filled: false, start: [5000, 0], mid: [0, -5000], end: [-5000, 0] };
  const r = outsetShapes([a], outset({ outsetUm: 500 }));
  assert.equal(r.successes, 1);
  assert.equal(r.add.length, 4);
  assert.ok(r.add.every((s) => s.kind === "arc"));
  const small = outsetShapes([{ ...a, start: [100, 0], mid: [0, -100], end: [-100, 0] } as Shape], outset({ outsetUm: 500 }));
  assert.equal(small.successes, 0);
  assert.equal(small.failures, 0);
});

test("outset: sources are deleted only while there has been no failure", () => {
  const ok = rect("ok", 0, 0, 1000, 1000);
  const bad = rect("bad", 0, 0, 10, 10);
  const r = outsetShapes([bad, ok], outset({ outsetUm: -20, deleteSourceItems: true }));
  assert.deepEqual(r.remove, [], "the failure came first, so nothing is deleted afterwards");
  const good = outsetShapes([ok, rect("ok2", 0, 0, 2000, 2000)], outset({ outsetUm: 100, deleteSourceItems: true }));
  assert.deepEqual(good.remove, ["ok", "ok2"]);
  const mixed = outsetShapes([ok, bad], outset({ outsetUm: -30, deleteSourceItems: true }));
  assert.deepEqual(mixed.remove, ["ok"], "the first item succeeded before the failure");
  assert.equal(mixed.message, "Some of the items could not be outset.");
});

test("outset: widths come from the source when asked (a segment's, a hollow shape's) and from the dialog otherwise", () => {
  const filled = rect("f", 0, 0, 1000, 1000, 80, true);
  const r = outsetShapes([filled, rect("h", 0, 0, 1000, 1000, 70)], outset({ outsetUm: 100, roundCorners: false, lineWidthUm: 55 }));
  assert.deepEqual(r.add.map((s) => s.stroke_width), [55, 70]);
});

test("outset: polygons are not supported", () => {
  const r = outsetShapes([poly("p", [[0, 0], [10, 0], [0, 10]])], outset());
  assert.equal(r.successes + r.failures, 0);
  assert.equal(r.message, "Unable to outset the selected items.");
});
