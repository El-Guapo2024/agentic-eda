import { test } from "node:test";
import assert from "node:assert/strict";
import type { CmdShape, Shape } from "../api/types";
import { healShapes, modifyLines, pointInRings, simplifyChain, simplifyPolygonRing, testSegmentHit } from "./pcbModify";

const seg = (id: string, ax: number, ay: number, bx: number, by: number, layer = "F.SilkS", w = 150): Shape => ({ kind: "segment", id, layer, stroke_width: w, filled: false, start: [ax, ay], end: [bx, by] });
const rect = (id: string, ax: number, ay: number, bx: number, by: number): Shape => ({ kind: "rect", id, layer: "F.Fab", stroke_width: 100, filled: false, start: [ax, ay], end: [bx, by] });
const poly = (id: string, pts: [number, number][]): Shape => ({ kind: "polygon", id, layer: "F.Fab", stroke_width: 100, filled: false, pts });

const segs = (add: CmdShape[]) => add.filter((s): s is Extract<CmdShape, { kind: "segment" }> => s.kind === "segment");
const arcs = (add: CmdShape[]) => add.filter((s): s is Extract<CmdShape, { kind: "arc" }> => s.kind === "arc");
const close = (a: { x: number; y: number }, b: [number, number], tol = 2) => Math.abs(a.x - b[0]) <= tol && Math.abs(a.y - b[1]) <= tol;

test("fillet two lines sharing a corner: an arc joins them and both lines end on the arc (LINE_FILLET_ROUTINE)", () => {
  const r = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 0, 0, 0, 10000)], { kind: "fillet", radiusUm: 1000 });
  assert.equal(r.ok, true);
  assert.equal(r.successes, 1);
  assert.equal(r.message, null);
  assert.deepEqual([...r.remove].sort(), ["a", "b"]);
  const [arc] = arcs(r.add);
  assert.ok(arc);
  assert.ok(close(arc.start, [1000, 0]));
  assert.ok(close(arc.end, [0, 1000]));
  // the shortened lines run from their far end to the arc end points
  const lines = segs(r.add);
  assert.equal(lines.length, 2);
  assert.ok(lines.some((l) => close(l.start, [1000, 0]) && close(l.end, [10000, 0])) || lines.some((l) => close(l.start, [10000, 0]) && close(l.end, [1000, 0])));
  assert.ok(lines.some((l) => close(l.start, [0, 1000]) && close(l.end, [0, 10000])) || lines.some((l) => close(l.start, [0, 10000]) && close(l.end, [0, 1000])));
  // the arc takes layer and width from the first line
  assert.equal(arc.layer, "F.SilkS");
  assert.equal(arc.stroke_width, 150);
  // the arc is a new item; the two shortened lines replace the old ones
  assert.equal(r.created.length, 1);
  assert.deepEqual(r.replaced.map((x) => x.id).sort(), ["a", "b"]);
});

test("fillet with a radius too big for the lines fails and changes nothing", () => {
  const r = modifyLines([seg("a", 0, 0, 500, 0), seg("b", 0, 0, 0, 500)], { kind: "fillet", radiusUm: 1000 });
  assert.equal(r.ok, true);
  assert.equal(r.successes, 0);
  assert.equal(r.failures, 1);
  assert.equal(r.message, "Unable to fillet the selected lines.");
  assert.deepEqual(r.remove, []);
  assert.deepEqual(r.add, []);
});

test("fillet of parallel or non-touching lines does nothing (no failure counted)", () => {
  const r = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 0, 5000, 10000, 5000)], { kind: "fillet", radiusUm: 500 });
  assert.equal(r.successes, 0);
  assert.equal(r.failures, 0);
  assert.equal(r.message, "Unable to fillet the selected lines.");
});

test("fillet needs at least two lines", () => {
  const r = modifyLines([seg("a", 0, 0, 10000, 0)], { kind: "fillet", radiusUm: 500 });
  assert.equal(r.ok, false);
  assert.equal(r.message, "A shape with at least two lines must be selected.");
});

test("fillet of a rectangle decomposes it into four lines and rounds all four corners", () => {
  const r = modifyLines([rect("r", 0, 0, 10000, 6000)], { kind: "fillet", radiusUm: 1000 });
  assert.equal(r.successes, 4);
  assert.equal(r.message, null);
  assert.deepEqual(r.remove, ["r"]);
  assert.equal(arcs(r.add).length, 4);
  assert.equal(segs(r.add).length, 4);
  // each arc is a quarter circle of 1 mm radius
  for (const a of arcs(r.add)) {
    const d = Math.hypot(a.start.x - a.end.x, a.start.y - a.end.y);
    assert.ok(Math.abs(d - 1000 * Math.SQRT2) < 4, `chord ${d}`);
  }
});

test("a polygon's closing edge is decomposed too (EDIT_TOOL::ModifyLines)", () => {
  const r = modifyLines([poly("p", [[0, 0], [10000, 0], [0, 10000]])], { kind: "chamfer", setbackUm: 1000 });
  assert.equal(r.successes, 3);
  assert.deepEqual(r.remove, ["p"]);
  // three chamfers + three shortened sides
  assert.equal(segs(r.add).length, 6);
});

test("chamfer: a chord between the setback points and the lines shortened (LINE_CHAMFER_ROUTINE)", () => {
  const r = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 0, 0, 0, 10000)], { kind: "chamfer", setbackUm: 2000 });
  assert.equal(r.successes, 1);
  assert.equal(r.message, null);
  const lines = segs(r.add);
  assert.equal(lines.length, 3);
  const has = (ax: number, ay: number, bx: number, by: number) => lines.some((l) => (close(l.start, [ax, ay]) && close(l.end, [bx, by])) || (close(l.end, [ax, ay]) && close(l.start, [bx, by])));
  assert.ok(has(2000, 0, 0, 2000)); // the chamfer
  assert.ok(has(2000, 0, 10000, 0));
  assert.ok(has(0, 2000, 0, 10000));
});

test("chamfer ignores lines that do not share an end point and refuses a setback longer than a line", () => {
  const apart = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 5000, 5000, 5000, 10000)], { kind: "chamfer", setbackUm: 1000 });
  assert.equal(apart.failures, 0);
  assert.equal(apart.successes, 0);
  const long = modifyLines([seg("a", 0, 0, 1000, 0), seg("b", 0, 0, 0, 10000)], { kind: "chamfer", setbackUm: 2000 });
  assert.equal(long.failures, 1);
  assert.equal(long.message, "Unable to chamfer the selected lines.");
});

test("chamfer consuming a whole line deletes it (ModifyLineOrDeleteIfZeroLength)", () => {
  const r = modifyLines([seg("a", 0, 0, 2000, 0), seg("b", 0, 0, 0, 10000)], { kind: "chamfer", setbackUm: 2000 });
  assert.equal(r.successes, 1);
  assert.ok(r.remove.includes("a"));
  assert.ok(r.remove.includes("b"));
  // only the chamfer and line b's shortened version come back; a is gone
  assert.equal(segs(r.add).length, 2);
});

test("extend two lines to meet: each is lengthened to the intersection (LINE_EXTENSION_ROUTINE)", () => {
  const r = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 15000, -5000, 15000, -1000)], { kind: "extend" });
  assert.equal(r.successes, 1);
  assert.equal(r.message, null);
  const lines = segs(r.add);
  assert.equal(lines.length, 2);
  assert.ok(lines.some((l) => close(l.start, [0, 0]) && close(l.end, [15000, 0])));
  assert.ok(lines.some((l) => close(l.start, [15000, -5000]) && close(l.end, [15000, 0])));
  assert.deepEqual([...r.remove].sort(), ["a", "b"]);
});

test("extend needs exactly two lines, and parallel lines have nothing to meet at", () => {
  const three = modifyLines([seg("a", 0, 0, 10, 0), seg("b", 0, 5, 10, 5), seg("c", 0, 9, 10, 9)], { kind: "extend" });
  assert.equal(three.ok, false);
  assert.equal(three.message, "Exactly two lines must be selected to extend them.");
  const parallel = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 0, 5000, 10000, 5000)], { kind: "extend" });
  assert.equal(parallel.successes, 0);
  assert.equal(parallel.message, "Unable to extend the selected lines to meet.");
  assert.deepEqual(parallel.remove, []);
});

test("extend leaves lines that already cross alone", () => {
  const r = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 5000, -3000, 5000, 3000)], { kind: "extend" });
  assert.equal(r.successes, 0);
  assert.deepEqual(r.add, []);
});

test("dogbone: needs the corner to point into the board outline; the arc and caps replace the corner", () => {
  // An L-shaped board outline; the inner corner at (5000, 5000) points into the board.
  const outline: [number, number][] = [[0, 0], [10000, 0], [10000, 5000], [5000, 5000], [5000, 10000], [0, 10000]];
  const a = seg("a", 5000, 5000, 10000, 5000, "Edge.Cuts");
  const b = seg("b", 5000, 5000, 5000, 10000, "Edge.Cuts");
  const r = modifyLines([a, b], { kind: "dogbone", radiusUm: 500, addSlots: true, boardOutline: [outline] });
  assert.equal(r.successes, 1);
  assert.equal(arcs(r.add).length, 1);
  // A convex corner of the same outline is skipped (not inward).
  const cornerA = seg("c", 0, 0, 10000, 0, "Edge.Cuts");
  const cornerB = seg("d", 0, 0, 0, 10000, "Edge.Cuts");
  const none = modifyLines([cornerA, cornerB], { kind: "dogbone", radiusUm: 500, addSlots: true, boardOutline: [outline] });
  assert.equal(none.successes, 0);
  assert.deepEqual(none.add, []);
});

test("dogbone with no board outline skips every corner", () => {
  const r = modifyLines([seg("a", 0, 0, 10000, 0), seg("b", 0, 0, 0, 10000)], { kind: "dogbone", radiusUm: 500, addSlots: false, boardOutline: [] });
  assert.equal(r.successes, 0);
  assert.equal(r.message, "Unable to add dogbone corners to the selected lines.");
});

test("pointInRings: even-odd over an outline and a hole", () => {
  const outer: [number, number][] = [[0, 0], [100, 0], [100, 100], [0, 100]];
  const hole: [number, number][] = [[40, 40], [60, 40], [60, 60], [40, 60]];
  assert.equal(pointInRings([outer, hole], [10, 10]), true);
  assert.equal(pointInRings([outer, hole], [50, 50]), false);
  assert.equal(pointInRings([outer, hole], [150, 50]), false);
});

test("testSegmentHit: within distance of a segment (TestSegmentHit)", () => {
  assert.equal(testSegmentHit([50, 3], [0, 0], [100, 0], 5), true);
  assert.equal(testSegmentHit([50, 9], [0, 0], [100, 0], 5), false);
  assert.equal(testSegmentHit([120, 0], [0, 0], [100, 0], 5), false);
});

test("simplifyChain: a closed ring loses the vertices a longer run covers within the tolerance (SHAPE_LINE_CHAIN::Simplify)", () => {
  const ring: [number, number][] = [[0, 0], [5000, 20], [10000, 0], [10000, 10000], [0, 10000]];
  const out = simplifyChain(ring, true, 100);
  assert.equal(out.length, 4);
  assert.deepEqual(out.map((p) => p.join(",")), ["0,0", "10000,0", "10000,10000", "0,10000"]);
  // a zero tolerance keeps everything not exactly collinear
  assert.equal(simplifyChain(ring, true, 0).length, 5);
  // fewer than three points is left alone
  assert.equal(simplifyChain([[0, 0], [1, 1]], true, 100).length, 2);
});

test("simplifyPolygonRing returns null when nothing changed and the new ring in micrometres otherwise", () => {
  const square: [number, number][] = [[0, 0], [10000, 0], [10000, 10000], [0, 10000]];
  assert.equal(simplifyPolygonRing(square, 3), null);
  const noisy: [number, number][] = [[0, 0], [5000, 2], [10000, 0], [10000, 10000], [0, 10000]];
  const out = simplifyPolygonRing(noisy, 5);
  assert.ok(out);
  assert.equal(out!.length, 4);
});

test("heal shapes: two lines with a small gap are joined at the intersection of their lines (ConnectBoardShapes)", () => {
  const r = healShapes([seg("a", 0, 0, 10000, 0), seg("b", 10010, 10, 10010, 10000)], 100);
  assert.equal(r.ok, true);
  assert.deepEqual([...r.remove].sort(), ["a", "b"]);
  const lines = segs(r.add);
  assert.equal(lines.length, 2);
  assert.ok(lines.some((l) => close(l.start, [0, 0]) && close(l.end, [10010, 0])));
  assert.ok(lines.some((l) => close(l.start, [10010, 0]) && close(l.end, [10010, 10000])));
});

test("heal shapes: shapes farther apart than the tolerance are left alone", () => {
  const r = healShapes([seg("a", 0, 0, 10000, 0), seg("b", 10500, 0, 10500, 10000)], 100);
  assert.deepEqual(r.remove, []);
  assert.deepEqual(r.add, []);
});

test("heal shapes: a segment end snaps onto a neighbouring arc's end point", () => {
  const arc: Shape = { kind: "arc", id: "c", layer: "F.SilkS", stroke_width: 150, filled: false, start: [10000, 0], mid: [10707, 293], end: [11000, 1000] };
  const r = healShapes([seg("a", 0, 0, 10040, 20), arc], 100);
  assert.deepEqual(r.remove, ["a"]);
  const lines = segs(r.add);
  assert.equal(lines.length, 1);
  assert.ok(close(lines[0]!.end, [10000, 0]));
});

test("heal shapes ignores shapes it cannot chain (rectangles, polygons)", () => {
  const r = healShapes([rect("r", 0, 0, 10, 10), poly("p", [[0, 0], [1, 0], [0, 1]])], 100);
  assert.deepEqual(r.remove, []);
  assert.deepEqual(r.add, []);
});
