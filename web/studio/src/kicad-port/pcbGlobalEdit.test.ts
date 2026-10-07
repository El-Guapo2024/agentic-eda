import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Shape } from "../api/types";
import { DEFAULT_CLEANUP_GRAPHICS, DEFAULT_GLOBAL_DELETION, exchangeTargets, footprintNamesOf, planCleanupGraphics, planGlobalDeletions, wildMatch } from "./pcbGlobalEdit";

function board(partial: Record<string, unknown>): BoardState {
  return {
    name: "t",
    dir: "",
    outline: null,
    layers: ["F.Cu", "In1.Cu", "B.Cu"],
    snap: 100,
    parts: [],
    rules: [],
    routing: null,
    drawings: null,
    checks: [],
    activity: [],
    job: "idle",
    ...partial,
  } as unknown as BoardState;
}

const seg = (id: string, layer: string, a: [number, number], b: [number, number], w = 150): Shape => ({ kind: "segment", id, layer, stroke_width: w, filled: false, start: a, end: b });
const drawings = (shapes: Shape[], texts: unknown[] = []) => ({ shapes, texts, groups: [], dimensions: [] });
const zone = (id: string, layer: string, teardrop = false) => ({ id, net: "N", layer, teardrop, outline: [[0, 0], [1, 0], [0, 1]] });
const track = (id: string, layer: string) => ({ id, net: "N", layer, width: 200, pts: [[0, 0], [100, 0]] });
const via = (id: string, from: string, to: string) => ({ id, net: "N", x: 0, y: 0, d: 600, drill: 300, from, to });
const part = (ref: string, side: "top" | "bottom", placed = true, value = "1k", footprint = "0603") => ({ ref, value, footprint, side, placed, package: null, mpn: null, block: null, size: null });
const opts = (o: Partial<typeof DEFAULT_GLOBAL_DELETION>) => ({ ...DEFAULT_GLOBAL_DELETION, ...o });

test("the deletion dialog opens benign: nothing is deleted with the default options", () => {
  const b = board({ routing: { tracks: [track("t", "F.Cu")], vias: [], zones: [zone("z", "F.Cu")] }, drawings: drawings([seg("s", "F.SilkS", [0, 0], [1, 1])]), parts: [part("R1", "top")] });
  const plan = planGlobalDeletions(b, DEFAULT_GLOBAL_DELETION, "F.Cu");
  assert.deepEqual(plan.cmds, []);
  assert.deepEqual(plan.removed, []);
});

test("zones and teardrops are separate choices", () => {
  const b = board({ routing: { tracks: [], vias: [], zones: [zone("z", "F.Cu"), zone("td", "F.Cu", true)] } });
  assert.deepEqual(planGlobalDeletions(b, opts({ zones: true }), null).removed, ["z"]);
  assert.deepEqual(planGlobalDeletions(b, opts({ teardrops: true }), null).removed, ["td"]);
});

test("Graphics takes the non-copper layers except Edge.Cuts; Board outlines takes Edge.Cuts", () => {
  const b = board({ drawings: drawings([seg("silk", "F.SilkS", [0, 0], [1, 1]), seg("edge", "Edge.Cuts", [0, 0], [1, 1]), seg("cu", "F.Cu", [0, 0], [1, 1])]) });
  assert.deepEqual(planGlobalDeletions(b, opts({ drawings: true }), null).removed, ["silk"]);
  assert.deepEqual(planGlobalDeletions(b, opts({ boardEdges: true }), null).removed, ["edge"]);
  assert.deepEqual(planGlobalDeletions(b, opts({ drawings: true, boardEdges: true }), null).removed, ["silk", "edge"]);
});

test("locked graphics are deleted only through the locked filter, and unlocked ones through the unlocked filter", () => {
  const b = board({ locked: ["a"], drawings: drawings([seg("a", "F.SilkS", [0, 0], [1, 1]), seg("b", "F.SilkS", [0, 0], [2, 2])]) });
  assert.deepEqual(planGlobalDeletions(b, opts({ drawings: true }), null).removed, ["b"]);
  assert.deepEqual(planGlobalDeletions(b, opts({ drawings: true, drawingFilterLocked: true, drawingFilterUnlocked: false }), null).removed, ["a"]);
  assert.deepEqual(planGlobalDeletions(b, opts({ drawings: true, drawingFilterLocked: true }), null).removed, ["a", "b"]);
});

test("Text deletes the texts, also together with Graphics", () => {
  const b = board({ drawings: drawings([seg("s", "F.SilkS", [0, 0], [1, 1])], [{ id: "t1", layer: "F.SilkS" }, { id: "t2", layer: "B.SilkS" }]) });
  assert.deepEqual(planGlobalDeletions(b, opts({ texts: true }), null).removed, ["t1", "t2"]);
  assert.deepEqual(planGlobalDeletions(b, opts({ texts: true, drawings: true }), null).removed, ["s", "t1", "t2"]);
});

test("the layer filter keeps only items on the current layer", () => {
  const b = board({
    routing: { tracks: [track("tf", "F.Cu"), track("tb", "B.Cu")], vias: [via("thru", "F.Cu", "B.Cu"), via("blind", "In1.Cu", "B.Cu")], zones: [zone("zf", "F.Cu"), zone("zb", "B.Cu")] },
    drawings: drawings([seg("sf", "F.SilkS", [0, 0], [1, 1]), seg("sb", "B.SilkS", [0, 0], [1, 1])]),
  });
  const plan = planGlobalDeletions(b, opts({ tracks: true, zones: true, drawings: true, currentLayerOnly: true }), "F.Cu");
  assert.deepEqual(plan.removed.sort(), ["tf", "thru", "zf"]);
  const silk = planGlobalDeletions(b, opts({ drawings: true, currentLayerOnly: true }), "B.SilkS");
  assert.deepEqual(silk.removed, ["sb"]);
  const bottom = planGlobalDeletions(b, opts({ tracks: true, currentLayerOnly: true }), "B.Cu");
  assert.deepEqual(bottom.removed.sort(), ["blind", "tb", "thru"]);
});

test("tracks and vias go in one route command, each through its own locked filter", () => {
  const b = board({ locked: ["t1", "v1"], routing: { tracks: [track("t1", "F.Cu"), track("t2", "F.Cu")], vias: [via("v1", "F.Cu", "B.Cu"), via("v2", "F.Cu", "B.Cu")], zones: [] } });
  const plan = planGlobalDeletions(b, opts({ tracks: true }), null);
  assert.deepEqual(plan.cmds, [{ op: "commit_route", remove_track_ids: ["t2"], remove_via_ids: ["v2"] }]);
  const some = planGlobalDeletions(b, opts({ tracks: true, trackFilterLocked: true, viaFilterUnlocked: false }), null);
  assert.deepEqual(some.cmds, [{ op: "commit_route", remove_track_ids: ["t1", "t2"], remove_via_ids: [] }]);
});

test("footprints are taken off the board, through the locked filter and the layer of their side", () => {
  const b = board({ locked: ["R2"], parts: [part("R1", "top"), part("R2", "top"), part("C1", "bottom"), part("U9", "top", false)] });
  assert.deepEqual(planGlobalDeletions(b, opts({ footprints: true }), null).cmds, [
    { op: "rip", part: "R1" },
    { op: "rip", part: "C1" },
  ]);
  assert.deepEqual(planGlobalDeletions(b, opts({ footprints: true, currentLayerOnly: true }), "B.Cu").removed, ["C1"]);
});

test("Clear board takes everything, locked or not", () => {
  const b = board({
    locked: ["a", "t1"],
    routing: { tracks: [track("t1", "F.Cu")], vias: [via("v1", "F.Cu", "B.Cu")], zones: [zone("z", "F.Cu", true)] },
    drawings: { shapes: [seg("a", "F.SilkS", [0, 0], [1, 1]), seg("e", "Edge.Cuts", [0, 0], [1, 1])], texts: [{ id: "t", layer: "F.SilkS" }], groups: [], dimensions: [{ id: "d" }] },
    parts: [part("R1", "top")],
  });
  const plan = planGlobalDeletions(b, opts({ all: true }), "F.Cu");
  assert.deepEqual(plan.removed.sort(), ["R1", "a", "d", "e", "t", "t1", "v1", "z"]);
});

test("the markers flag is passed through", () => {
  assert.ok(planGlobalDeletions(board({}), opts({ markers: true }), null).clearMarkers);
  assert.ok(!planGlobalDeletions(board({}), opts({}), null).clearMarkers);
});

const cleanup = (o: Partial<typeof DEFAULT_CLEANUP_GRAPHICS>) => ({ ...DEFAULT_CLEANUP_GRAPHICS, ...o });

test("cleanup: zero-size and duplicated graphics are listed and removed", () => {
  const shapes: Shape[] = [seg("a", "F.SilkS", [0, 0], [100, 0]), seg("a2", "F.SilkS", [0, 0], [100, 0]), seg("zero", "F.SilkS", [5, 5], [5, 5]), seg("other", "F.SilkS", [0, 0], [200, 0]), seg("wider", "F.SilkS", [0, 0], [100, 0], 300), seg("layer", "F.Fab", [0, 0], [100, 0])];
  const plan = planCleanupGraphics(shapes, cleanup({ deleteRedundant: true }), true);
  assert.deepEqual(plan.items.map((i) => [i.kind, i.ids]), [["duplicate", ["a2"]], ["null", ["zero"]]]);
  assert.deepEqual(plan.remove, ["a2", "zero"]);
});

test("cleanup: polygons are never taken for duplicates, equal circles and arcs are", () => {
  const poly: Shape = { kind: "polygon", id: "p1", layer: "F.Fab", stroke_width: 100, filled: false, pts: [[0, 0], [1, 0], [0, 1]] };
  const circle = (id: string): Shape => ({ kind: "circle", id, layer: "F.Fab", stroke_width: 100, filled: false, center: [0, 0], end: [50, 0] });
  const arc = (id: string): Shape => ({ kind: "arc", id, layer: "F.Fab", stroke_width: 100, filled: false, start: [10, 0], mid: [0, -10], end: [-10, 0] });
  const plan = planCleanupGraphics([poly, { ...poly, id: "p2" }, circle("c1"), circle("c2"), arc("a1"), arc("a2")], cleanup({ deleteRedundant: true }), true);
  assert.deepEqual(plan.remove, ["c2", "a2"]);
});

test("cleanup: four lines that make a rectangle become one rectangle, whatever the order and direction", () => {
  const w = 120;
  const shapes: Shape[] = [seg("bottom", "F.Fab", [4000, 3000], [0, 3000], w), seg("left", "F.Fab", [0, 3000], [0, 0], w), seg("top", "F.Fab", [0, 0], [4000, 0], w), seg("right", "F.Fab", [4000, 0], [4000, 3000], w)];
  const plan = planCleanupGraphics(shapes, cleanup({ mergeRects: true }), true);
  assert.equal(plan.items.length, 1);
  assert.equal(plan.items[0]!.kind, "rect");
  assert.deepEqual(plan.items[0]!.ids.sort(), ["bottom", "left", "right", "top"]);
  assert.deepEqual(plan.remove.sort(), ["bottom", "left", "right", "top"]);
  assert.deepEqual(plan.add, [{ kind: "rect", layer: "F.Fab", stroke_width: 120, filled: false, start: { x: 0, y: 0 }, end: { x: 4000, y: 3000 } }]);
});

test("cleanup: lines of different width, or on different layers, do not merge", () => {
  const shapes: Shape[] = [seg("l", "F.Fab", [0, 3000], [0, 0]), seg("t", "F.Fab", [0, 0], [4000, 0], 200), seg("r", "F.Fab", [4000, 0], [4000, 3000]), seg("b", "F.Fab", [4000, 3000], [0, 3000])];
  assert.equal(planCleanupGraphics(shapes, cleanup({ mergeRects: true }), true).items.length, 0);
});

test("cleanup: three lines are not a rectangle, and lines already deleted as duplicates are not used", () => {
  const three: Shape[] = [seg("l", "F.Fab", [0, 3000], [0, 0]), seg("t", "F.Fab", [0, 0], [4000, 0]), seg("r", "F.Fab", [4000, 0], [4000, 3000])];
  assert.equal(planCleanupGraphics(three, cleanup({ mergeRects: true }), true).items.length, 0);
  const dup: Shape[] = [...three, seg("b", "F.Fab", [4000, 3000], [0, 3000]), seg("b2", "F.Fab", [4000, 3000], [0, 3000])];
  const plan = planCleanupGraphics(dup, cleanup({ mergeRects: true, deleteRedundant: true }), true);
  assert.equal(plan.items.filter((i) => i.kind === "rect").length, 1);
  assert.ok(plan.remove.includes("b2"));
});

test("cleanup: the board-outline fix runs on a real cleanup, not on the preview", () => {
  const edge: Shape[] = [seg("e1", "Edge.Cuts", [0, 0], [10000, 0], 50), seg("e2", "Edge.Cuts", [10100, 0], [10100, 5000], 50)];
  const dry = planCleanupGraphics(edge, cleanup({ fixBoardOutlines: true, toleranceUm: 2000 }), true);
  assert.deepEqual(dry, { items: [], remove: [], add: [] });
  const real = planCleanupGraphics(edge, cleanup({ fixBoardOutlines: true, toleranceUm: 2000 }), false);
  assert.ok(real.remove.length + real.add.length > 0, "the 100 um gap is closed");
});

test("wildcards: * and ? match like WildCompareString, case-insensitively", () => {
  assert.ok(wildMatch("R*", "R12"));
  assert.ok(wildMatch("r?", "R1"));
  assert.ok(!wildMatch("R?", "R12"));
  assert.ok(wildMatch("*", ""));
  assert.ok(wildMatch("C1.5", "C1.5"));
  assert.ok(!wildMatch("C1.5", "C1x5"), "a dot is a dot");
});

test("exchange scope: selected, same reference pattern, same value, same footprint, all", () => {
  const b = board({ parts: [part("R1", "top", true, "10k", "Resistor_SMD:R_0603"), part("R2", "top", true, "1k", "Resistor_SMD:R_0603"), part("C1", "top", true, "10k", "Capacitor_SMD:C_0402"), part("U1", "top", false, "mcu", "QFN")] });
  const args = { selectedRef: "R1", reference: "R*", value: "10k", footprint: "Resistor_SMD:*" };
  assert.deepEqual(exchangeTargets(b, "selected", args), ["R1"]);
  assert.deepEqual(exchangeTargets(b, "reference", args), ["R1", "R2"]);
  assert.deepEqual(exchangeTargets(b, "value", args), ["R1", "C1"]);
  assert.deepEqual(exchangeTargets(b, "footprint", args), ["R1", "R2"]);
  assert.deepEqual(exchangeTargets(b, "all", args), ["R1", "R2", "C1"], "an unplaced part is not on the board");
  assert.deepEqual(exchangeTargets(b, "selected", { ...args, selectedRef: null }), []);
  assert.deepEqual(footprintNamesOf(b, ["R1", "R2", "C1"]), ["Resistor_SMD:R_0603", "Capacitor_SMD:C_0402"]);
});
