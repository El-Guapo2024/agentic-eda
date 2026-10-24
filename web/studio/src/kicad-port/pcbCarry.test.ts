import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { carriedIds, carryMatrix, carryPoint, carryPress, splitCarried } from "./pcbCarry";
import { planCarry } from "./pcbTransform";

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}
const pad = (num: string, x: number, y: number): Pad => ({ num, net: null, x, y, w: 600, h: 600, round: false, th: false });
const part = (ref: string, at: [number, number], pads: Pad[] = []): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads }) as Part;
const routing = (tracks: unknown[], vias: unknown[] = [], zones: unknown[] = []) => ({ tracks, vias, zones, track_width_presets: [], via_presets: [], teardrop_settings: {} }) as unknown as BoardState["routing"];
const drawings = (extra: Record<string, unknown> = {}) => ({ shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {}, ...extra }) as unknown as NonNullable<BoardState["drawings"]>;

const near = (a: readonly number[], b: readonly number[]) => a.forEach((v, i) => assert.ok(Math.abs(v - b[i]!) < 1e-6, `${a} vs ${b}`));

test("carryMatrix: a plain move is a translation", () => {
  const p = { refs: ["a"], dxNm: 1500, dyNm: -700 };
  assert.deepEqual(carryMatrix(p), [1, 0, 0, 1, 1500, -700]);
  assert.deepEqual(carryPoint(p, [10, 20]), [1510, -680]);
});

test("carryMatrix: R turns counter-clockwise on the screen about the pick-up point (Y points down)", () => {
  // (1000, 0) to the right of the pivot (0, 0): a quarter turn counter-clockwise takes it to the top, which is -y on the screen.
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 1, pivotNm: [0, 0] }, [1000, 0]), [0, -1000]);
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 2, pivotNm: [0, 0] }, [1000, 0]), [-1000, 0]);
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 3, pivotNm: [0, 0] }, [1000, 0]), [0, 1000]);
  // Shift+R is a quarter turn clockwise
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: -1, pivotNm: [0, 0] }, [1000, 0]), [0, 1000]);
  // the pivot stays where it is
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 1, pivotNm: [5000, 3000] }, [5000, 3000]), [5000, 3000]);
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 1, pivotNm: [5000, 3000] }, [6000, 3000]), [5000, 2000]);
});

test("carryMatrix: F mirrors left-right about the flip point, then the move", () => {
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, flipped: true, flipPivotNm: [5000, 0] }, [4000, 700]), [6000, 700]);
  near(carryPoint({ refs: [], dxNm: 100, dyNm: 200, flipped: true, flipPivotNm: [5000, 0] }, [4000, 700]), [6100, 900]);
});

test("carryMatrix: R turns by the rotation step of the preferences, not always a quarter", () => {
  // 45 degrees counter-clockwise about the origin takes (1000, 0) to (707.1, -707.1) on the screen (y down)
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 1, rotateStepDeg: 45, pivotNm: [0, 0] }, [1000, 0]), [1000 * Math.SQRT1_2, -1000 * Math.SQRT1_2]);
  // two presses of 45 are one quarter, exactly
  assert.deepEqual(carryPoint({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 2, rotateStepDeg: 45, pivotNm: [0, 0] }, [1000, 0]).map(Math.round), [0, -1000]);
  // 24 presses of 15 are a full turn: the identity
  near(carryMatrix({ refs: [], dxNm: 0, dyNm: 0, rotateQuarterTurns: 24, rotateStepDeg: 15, pivotNm: [3, 4] }), [1, 0, 0, 1, 0, 0]);
});

test("carryMatrix: F mirrors across the flip direction of the preferences", () => {
  // top/bottom mirrors y about the horizontal line through the flip point, x stays
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, flipped: true, flipDirection: "top_bottom", flipPivotNm: [0, 5000] }, [4000, 4000]), [4000, 6000]);
  near(carryPoint({ refs: [], dxNm: 10, dyNm: 20, flipped: true, flipDirection: "top_bottom", flipPivotNm: [0, 5000] }, [4000, 4000]), [4010, 6020]);
  // left/right is the default
  near(carryPoint({ refs: [], dxNm: 0, dyNm: 0, flipped: true, flipPivotNm: [5000, 0] }, [4000, 700]), [6000, 700]);
});

test("carryMatrix: turn first, then flip, then move -- the order planCarry sends them in", () => {
  const p = { refs: [], dxNm: 300, dyNm: 400, rotateQuarterTurns: 1, flipped: true, pivotNm: [0, 0] as [number, number], flipPivotNm: [1000, 0] as [number, number] };
  // (1000, 0) -> turned about (0,0): (0, -1000) -> mirrored about x = 1000: (2000, -1000) -> moved: (2300, -600)
  near(carryPoint(p, [1000, 0]), [2300, -600]);
});

test("carryMatrix agrees with planCarry's batch on a track: the drawn track is the committed one", () => {
  const b = board({ routing: routing([{ id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[1000, 2000], [4000, 2000]] }]) });
  const plan = planCarry(b, ["t1"], 500, 300, 1, false);
  const rotate = plan.cmds.find((c) => c.op === "rotate_items");
  assert.ok(rotate && rotate.op === "rotate_items");
  // the backend's angle is clockwise-positive: a counter-clockwise quarter turn is -90 degrees
  assert.equal(rotate.angle_millideg, -90_000);
  // the preview of the same carry
  const preview = { refs: ["t1"], dxNm: 500, dyNm: 300, rotateQuarterTurns: 1, pivotNm: [rotate.pivot.x, rotate.pivot.y] as [number, number] };
  // the track turns about its start (1000, 2000): its far end (4000, 2000) lands at (1000, -1000) before the move
  near(carryPoint(preview, [4000, 2000]), [1500, -700]);
  near(carryPoint(preview, [1000, 2000]), [1500, 2300]);
});

test("carriedIds: a pad stands for its footprint and a group for its members", () => {
  const b = board({
    parts: [part("U1", [0, 0], [pad("1", -500, 0)]), part("U2", [9000, 0])],
    routing: routing([{ id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 5000], [1000, 5000]] }]),
    drawings: drawings({ groups: [{ id: "g1", name: "", member_ids: ["U2", "t1"] }] }),
  });
  assert.deepEqual([...carriedIds(b, ["U1.1"])], ["U1"]);
  assert.deepEqual([...carriedIds(b, ["g1"])].sort(), ["U2", "g1", "t1"]);
});

test("splitCarried: every kind of item goes to the side it belongs on, and nothing is lost", () => {
  const b = board({
    parts: [part("U1", [0, 0], [pad("1", -500, 0)]), part("U2", [9000, 0])],
    routing: routing(
      [{ id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 5000], [1000, 5000]] }, { id: "t2", net: "N", layer: "F.Cu", width: 200, pts: [[0, 6000], [1000, 6000]] }],
      [{ id: "v1", net: "N", x: 0, y: 0, d: 600, drill: 300 }],
      [{ id: "z1", net: "GND", layer: "F.Cu", outline: [[0, 0], [1000, 0], [1000, 1000]] }]
    ),
    drawings: drawings({
      shapes: [{ id: "s1", kind: "segment", layer: "F.Fab", stroke_width: 100, filled: false, start: [0, 0], end: [10, 0] }],
      texts: [{ id: "x1", content: "hi", x: 0, y: 0, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 150, justify: "left", mirror: false }],
      dimensions: [{ id: "d1", start: [0, 0], end: [1, 1], lines: [], text_at: [0, 0] }],
      groups: [{ id: "g1", name: "", member_ids: ["t2", "v1"] }],
    }),
  });
  const { still, moving, partRefs } = splitCarried(b, ["U1.1", "t1", "z1", "s1", "x1", "d1", "g1"]);
  assert.deepEqual(partRefs, ["U1"]);
  assert.deepEqual(moving.parts.map((p) => p.ref), ["U1"]);
  assert.deepEqual(still.parts.map((p) => p.ref), ["U2"]);
  assert.deepEqual(moving.routing!.tracks.map((t) => t.id).sort(), ["t1", "t2"], "the group's member comes along");
  assert.deepEqual(still.routing!.tracks, []);
  assert.deepEqual(moving.routing!.vias.map((v) => v.id), ["v1"]);
  assert.deepEqual(moving.routing!.zones.map((z) => z.id), ["z1"]);
  assert.deepEqual(moving.drawings!.shapes.map((s) => s.id), ["s1"]);
  assert.deepEqual(moving.drawings!.texts.map((t) => t.id), ["x1"]);
  assert.deepEqual(moving.drawings!.dimensions.map((d) => d.id), ["d1"]);
  assert.deepEqual(moving.drawings!.groups.map((g) => g.id), ["g1"]);
  assert.deepEqual(still.drawings!.shapes, []);
});

test("splitCarried: a board with no routing or drawings splits cleanly", () => {
  const b = board({ parts: [part("U1", [0, 0]), part("U2", [9000, 0])] });
  const { still, moving } = splitCarried(b, ["U2"]);
  assert.equal(still.routing, null);
  assert.equal(moving.drawings, null);
  assert.deepEqual(moving.parts.map((p) => p.ref), ["U2"]);
});

test("carriedIds: a group of groups carries every item below it and the groups themselves", () => {
  const b = board({
    parts: [part("U1", [0, 0]), part("U2", [9000, 0]), part("U3", [18000, 0])],
    drawings: drawings({ groups: [{ id: "outer", name: "", member_ids: ["inner", "U3"] }, { id: "inner", name: "", member_ids: ["U1", "U2"] }] }),
  });
  assert.deepEqual([...carriedIds(b, ["outer"])].sort(), ["U1", "U2", "U3", "inner", "outer"]);
  assert.deepEqual([...carriedIds(b, ["inner"])].sort(), ["U1", "U2", "inner"]);
});

// ------------------------------------------------- the presses in order (p6 F4: `doMoveSelection`'s reference point is the cursor)

test("a carried turn is about the grab point: a resistor grabbed by pad 2 turns about pad 2", () => {
  const b = board({ parts: [part("R1", [0, 0], [pad("1", -2000, 0), pad("2", 2000, 0)])] });
  const op = carryPress(b, ["R1"], [], { turnDegCcw: 90 }, [2000, 0])!;
  assert.deepEqual(op, { kind: "turn", degCcw: 90, pivot: [2000, 0] });
  const p = { refs: ["R1"], dxNm: 0, dyNm: 0, ops: [op] };
  // pad 2 stays where it was; the origin goes to below it
  near(carryPoint(p, [2000, 0]), [2000, 0]);
  near(carryPoint(p, [0, 0]), [2000, 2000]);
  // the drop sends the same: one turn about the grab point, then the move
  const plan = planCarry(b, ["R1"], 500, 0, 0, false, undefined, 90, "left_right", [op]);
  assert.deepEqual(plan.cmds, [
    { op: "rotate_items", ids: ["R1"], pivot: { x: 2000, y: 0 }, angle_millideg: -90000 },
    { op: "move_items", ids: ["R1"], dx: 500, dy: 0 },
  ]);
});

test("F then R equals KiCad's order: Rot(+step) after the flip, not the flip after Rot(+step)", () => {
  const b = board({ parts: [part("R1", [0, 0], [pad("1", -2000, 0), pad("2", 2000, 0)])] });
  const grab: [number, number] = [0, 0];
  const f = carryPress(b, ["R1"], [], { flip: "left_right" }, grab)!;
  const r = carryPress(b, ["R1"], [f], { turnDegCcw: 90 }, grab)!;
  const p = { refs: ["R1"], dxNm: 0, dyNm: 0, ops: [f, r] };
  // (1000, 0): mirrored to (-1000, 0), then a quarter counter-clockwise to (0, 1000)
  near(carryPoint(p, [1000, 0]), [0, 1000]);
  // R then F would give (0, -1000) -> (0, -1000): the other way
  near(carryPoint({ refs: ["R1"], dxNm: 0, dyNm: 0, ops: [r, f] }, [1000, 0]), [0, -1000]);
  const plan = planCarry(b, ["R1"], 0, 0, 0, false, undefined, 90, "left_right", [f, r]);
  assert.deepEqual(plan.cmds.map((c) => c.op), ["flip_items", "rotate_items"]);
});

test("a flip of several carried items is about the centre of their box as the presses have left it", () => {
  const b = board({ parts: [part("R1", [0, 0]), part("R2", [10000, 0])] });
  // the box is x -1000 .. 11000, y -500 .. 500: centre (5000, 0); after a quarter turn about (0, 0) it is x -500 .. 500, y -11000 .. 1000: centre (0, -5000)
  const r = carryPress(b, ["R1", "R2"], [], { turnDegCcw: 90 }, [0, 0])!;
  const f = carryPress(b, ["R1", "R2"], [r], { flip: "left_right" }, [0, 0])!;
  assert.equal(f.kind, "flip");
  near(f.pivot, [0, -5000]);
  const f0 = carryPress(b, ["R1", "R2"], [], { flip: "left_right" }, [0, 0])!;
  near(f0.pivot, [5000, 0]);
});
