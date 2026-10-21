import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Shape } from "../api/types";
import { shapeHitDistance } from "../components/canvas/itemHitTest";
import { canFill, fillOf, fillWidthError, hatchDistance, isAnyFill, isHatched, reshapeCmds, SHAPE_FILL_CHOICES } from "./shapeFill";
import { mirrorShape, planMirror } from "./pcbMirror";
import { shapeHit } from "./snapScene";

const rect = (over: Partial<Extract<Shape, { kind: "rect" }>> = {}): Extract<Shape, { kind: "rect" }> => ({ id: "shp_r", kind: "rect", layer: "F.Fab", stroke_width: 200, filled: false, start: [0, 0], end: [10_000, 10_000], ...over });
// Lines of a hatch (slope -1) across the square, 2 mm apart: x + y = 2000k.
const hatchedRect = (): Shape => ({ ...rect(), fill: "hatch", hatch_width: 200, hatch_lines: [[2_000, 0, 0, 2_000], [4_000, 0, 0, 4_000], [6_000, 0, 0, 6_000]] });

test("the fill of a shape is one of the five UI_FILL_MODE choices, in that order", () => {
  assert.deepEqual(SHAPE_FILL_CHOICES.map((c) => c.label), ["None", "Solid", "Hatch", "Reverse Hatch", "Cross-hatch"]);
  assert.equal(fillOf(rect()), "none");
  assert.equal(fillOf(rect({ filled: true })), "solid");
  assert.equal(fillOf(hatchedRect()), "hatch");
  assert.equal(fillOf({ ...hatchedRect(), fill: "cross_hatch" }), "cross_hatch");
  // A solid fill wins if the state ever said both.
  assert.equal(fillOf({ ...hatchedRect(), filled: true }), "solid");
});

test("any fill is a solid or a hatched one; only a hatch is hatched; a line, an arc and a curve have no fill at all", () => {
  assert.deepEqual([rect(), rect({ filled: true }), hatchedRect()].map(isAnyFill), [false, true, true]);
  assert.deepEqual([rect(), rect({ filled: true }), hatchedRect()].map(isHatched), [false, false, true]);
  assert.deepEqual((["rect", "circle", "polygon", "segment", "arc", "bezier"] as const).map(canFill), [true, true, true, false, false, false]);
});

test("a click is within a hatch line's reach of the line, not of the empty space between the lines", () => {
  const s = hatchedRect();
  // On a line: x + y = 4000.
  assert.equal(hatchDistance(s, 2_000, 2_000), 0);
  // 1 mm off the line (along x + y), 0.707 mm perpendicular: half of the 0.2 mm line is 0.1 mm.
  const off = hatchDistance(s, 2_500, 2_500);
  assert.ok(off > 600 && off < 700, `${off}`);
  assert.equal(hatchDistance(rect(), 1, 1), Infinity, "a shape without lines is nowhere near one");
  // Between two lines (x + y = 3000 is midway between the lines at 2000 and 4000): 707 micrometres from either line's axis, 607 from its edge.
  assert.ok(Math.abs(shapeHitDistance(s, 1_500, 1_500) - 607) < 2, `${shapeHitDistance(s, 1_500, 1_500)}`);
});

test("the hit test of a hatched rectangle: its outline and its lines, not the space between", () => {
  const s = hatchedRect();
  assert.equal(shapeHitDistance(s, 2_000, 2_000), 0, "on a hatch line");
  assert.equal(shapeHitDistance(s, 0, 5_000), 0, "on the outline");
  assert.ok(shapeHitDistance(s, 7_000, 7_000) > 1_000, "inside, far from any line and from the outline");
  assert.ok(shapeHitDistance(rect({ filled: true }), 7_000, 7_000) === 0, "a solid one is hit anywhere inside");
  // The snap scene's predicate agrees.
  const hit = shapeHit(s);
  assert.equal(hit([2_000, 2_000]), true);
  assert.equal(hit([7_000, 7_000]), false);
  assert.equal(hit([0, 5_000]), true);
});

test("a fill that is not solid needs a line width, and says so as the shape dialog does", () => {
  assert.equal(fillWidthError("rect", "hatch", 0), "Line width must be greater than zero for an unfilled rectangle.");
  assert.equal(fillWidthError("circle", "none", 0), "Line width must be greater than zero for an unfilled circle.");
  assert.equal(fillWidthError("polygon", "cross_hatch", 0), "Line width must be greater than zero for an unfilled polygon.");
  assert.equal(fillWidthError("rect", "solid", 0), null);
  assert.equal(fillWidthError("rect", "hatch", 100), null);
  assert.equal(fillWidthError("segment", "none", 0), null);
});

test("a hatched shape is changed in place, any other the way the board editor always did: deleted and added", () => {
  const board = { drawings: { shapes: [hatchedRect(), rect({ id: "shp_plain", end: [5_000, 5_000] })] } } as unknown as BoardState;
  const next = { kind: "rect", layer: "F.Fab", stroke_width: 200, filled: false, start: { x: 0, y: 0 }, end: { x: 12_000, y: 10_000 } } as const;
  assert.deepEqual(reshapeCmds(board, "shp_r", next), { cmds: [{ op: "replace_shape", id: "shp_r", shape: next }], removed: [] });
  assert.deepEqual(reshapeCmds(board, "shp_plain", next), { cmds: [{ op: "delete_shape", id: "shp_plain" }, { op: "add_shape", shape: next }], removed: ["shp_plain"] });
  assert.deepEqual(reshapeCmds(null, "shp_plain", next).removed, ["shp_plain"], "no board, nothing known of the shape");
});

test("Mirror turns a hatched shape in place and keeps its id, so it keeps its hatch", () => {
  const board = { drawings: { shapes: [hatchedRect()] }, routing: { tracks: [], vias: [], zones: [] }, parts: [] } as unknown as BoardState;
  const plan = planMirror(board, ["shp_r"], [5_000, 5_000], "leftRight");
  assert.deepEqual(plan.cmds, [{ op: "replace_shape", id: "shp_r", shape: mirrorShape(hatchedRect(), [5_000, 5_000], "leftRight") }]);
  assert.deepEqual(plan.removed, []);
  const plain = { ...board, drawings: { shapes: [rect()] } } as unknown as BoardState;
  const planPlain = planMirror(plain, ["shp_r"], [5_000, 5_000], "leftRight");
  assert.deepEqual(planPlain.cmds.map((c) => c.op), ["delete_shape", "add_shape"]);
  assert.deepEqual(planPlain.removed, ["shp_r"]);
});
