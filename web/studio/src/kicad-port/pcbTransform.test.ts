import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Dimension, Part, Pad } from "../api/types";
import { editableSelection, flipPivot, isLocked, modificationPoint, planCarry, planFlip, planMove, planRotate, rotationPivot, selectionCenter } from "./pcbTransform";
import { padById, padIds, padParent, itemKind, itemPosition, itemBounds } from "./pcbItems";

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}

const pad = (num: string, x: number, y: number, w = 600, h = 600): Pad => ({ num, net: null, x, y, w, h, round: false, th: false });
const part = (ref: string, at: [number, number], pads: Pad[] = []): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads }) as Part;
const track = (id: string, pts: [number, number][]) => ({ id, net: "GND", layer: "F.Cu", width: 200, pts });
const dim = (id: string): Dimension =>
  ({ id, layer: "Dwgs.User", kind: "aligned", height: 2000, horizontal: null, leader_length: null, start: [0, 0], end: [10_000, 0], lines: [[[0, 0], [10_000, 0]], [[0, 2000], [10_000, 2000]]], text_at: [5000, 2500], computed_text_angle: 0, text: "10", text_size_um: 1000, stroke_width: 150 }) as unknown as Dimension;

const routing = (tracks: ReturnType<typeof track>[], vias: BoardState["routing"] extends infer R ? (R extends { vias: infer V } ? V : never) : never = [] as never) => ({ tracks, vias, zones: [], track_width_presets: [], via_presets: [], teardrop_settings: {} }) as unknown as BoardState["routing"];
const drawings = (extra: Partial<NonNullable<BoardState["drawings"]>> = {}) => ({ shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {}, ...extra }) as unknown as NonNullable<BoardState["drawings"]>;

// ------------------------------------------------------------ the working selection

test("pads: ids are REF.NUMBER, repeats get #k, and a pad is found through its footprint", () => {
  const u1 = part("U1", [0, 0], [pad("1", -1000, 0), pad("2", 1000, 0), pad("2", 1000, 600)]);
  assert.deepEqual(padIds(u1), ["U1.1", "U1.2", "U1.2#2"]);
  const b = board({ parts: [u1] });
  assert.equal(itemKind(b, "U1.2#2"), "pad");
  assert.equal(itemKind(b, "U1"), "part");
  assert.equal(padById(b, "U1.2#2")?.pad.y, 600);
  assert.equal(padParent(b, "U1.1"), "U1");
  assert.equal(padById(b, "U9.1"), null);
  assert.deepEqual(itemPosition(b, "U1.1"), [-1000, 0]);
  assert.deepEqual(itemBounds(b, "U1.1"), [-1300, -300, -700, 300]);
});

test("editableSelection: a pad stands for its footprint, unknown ids go, repeats collapse", () => {
  const b = board({ parts: [part("U1", [0, 0], [pad("1", -1000, 0)]), part("U2", [9000, 0])] });
  assert.deepEqual(editableSelection(b, ["U1.1", "U1", "gone", "U2"]), { ids: ["U1", "U2"], lockedOut: false });
});

test("editableSelection: locked items stay out -- the item, a member of a locked group, a group with a locked member", () => {
  const b = board({
    parts: [part("U1", [0, 0]), part("U2", [9000, 0]), part("U3", [18000, 0])],
    routing: routing([track("t1", [[0, 5000], [1000, 5000]])]),
    drawings: drawings({ groups: [{ id: "g_lock", name: "", member_ids: ["U2", "t1"] }, { id: "g_has_locked", name: "", member_ids: ["U3", "U4"] }] }),
    locked: ["U1", "g_lock", "U4"],
  });
  assert.ok(isLocked(b, "U1"));
  assert.ok(isLocked(b, "U2"), "its group is locked");
  assert.ok(isLocked(b, "g_has_locked"), "a member (U4) of it is locked");
  assert.deepEqual(editableSelection(b, ["U1", "U2", "t1", "U3", "g_has_locked"]), { ids: ["U3"], lockedOut: true });
  assert.deepEqual(editableSelection(b, ["U1"], { respectLocks: false }), { ids: ["U1"], lockedOut: false });
});

// --------------------------------------------------------------- reference points

test("modificationPoint: one item is its own position, several are the centre of their box snapped to the grid", () => {
  const b = board({ parts: [part("U1", [1000, 2000]), part("U2", [9000, 2000])], routing: routing([track("t1", [[300, 5000], [700, 5000]])]) });
  assert.deepEqual(modificationPoint(b, ["U1"]), [1000, 2000]);
  assert.deepEqual(modificationPoint(b, ["t1"]), [300, 5000], "a track's position is its start");
  const snap = (p: readonly [number, number]): [number, number] => [Math.round(p[0] / 1000) * 1000, Math.round(p[1] / 1000) * 1000];
  // U1 courtyard x 0..2000, U2 8000..10000, y 1500..2500: centre (5000, 2000).
  assert.deepEqual(selectionCenter(b, ["U1", "U2"]), [5000, 2000]);
  assert.deepEqual(modificationPoint(b, ["U1", "U2"], snap), [5000, 2000]);
  assert.deepEqual(modificationPoint(b, ["U1", "t1"], snap), snap(selectionCenter(b, ["U1", "t1"])!));
});

test("rotationPivot: a lone rectangle or polygon turns about its centre, a lone line about its start", () => {
  const b = board({
    drawings: drawings({
      shapes: [
        { id: "r", kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: [0, 0], end: [10_000, 4_000] },
        { id: "l", kind: "segment", layer: "F.Fab", stroke_width: 100, filled: false, start: [0, 0], end: [10_000, 0] },
        { id: "p", kind: "polygon", layer: "F.Fab", stroke_width: 100, filled: false, pts: [[0, 0], [10_000, 0], [10_000, 10_000]] },
      ],
    }),
  });
  assert.deepEqual(rotationPivot(b, ["r"]), [5000, 2000]);
  assert.deepEqual(rotationPivot(b, ["l"]), [0, 0]);
  assert.deepEqual(rotationPivot(b, ["p"]), [5000, 5000]);
});

test("flipPivot: the centre of the box, not snapped; a lone item its own position, a lone rectangle its centre", () => {
  const b = board({
    parts: [part("U1", [1000, 2000]), part("U2", [9000, 2000])],
    drawings: drawings({ shapes: [{ id: "r", kind: "rect", layer: "F.Fab", stroke_width: 100, filled: false, start: [0, 0], end: [10_000, 4_000] }] }),
  });
  assert.deepEqual(flipPivot(b, ["U1", "U2"]), [5000, 2000]);
  assert.deepEqual(flipPivot(b, ["U1"]), [1000, 2000]);
  assert.deepEqual(flipPivot(b, ["r"]), [5000, 2000]);
});

// ------------------------------------------------------------------------- plans

test("planMove: one move_items for the whole selection, tracks and vias and zones and dimensions among it", () => {
  const b = board({
    parts: [part("U1", [0, 0])],
    routing: routing([track("t1", [[0, 5000], [1000, 5000]])], [{ id: "v1", net: "GND", x: 1000, y: 5000, d: 600, drill: 300, from: "F.Cu", to: "B.Cu" }] as never),
    drawings: drawings({ dimensions: [dim("d1")] }),
  });
  const plan = planMove(b, ["t1", "v1", "d1", "U1"], 1500.4, -200);
  assert.deepEqual(plan.cmds, [{ op: "move_items", ids: ["t1", "v1", "d1", "U1"], dx: 1500, dy: -200 }]);
  assert.deepEqual(planMove(b, ["t1"], 0, 0).cmds, [], "no movement, no command");
  assert.deepEqual(planMove(b, ["nope"], 5, 5).cmds, []);
});

test("planRotate: R is a counter-clockwise quarter turn, which the backend writes as a negative (clockwise-positive) angle", () => {
  const b = board({ parts: [part("U1", [1000, 2000]), part("U2", [9000, 2000])] });
  const one = planRotate(b, ["U1"], 1);
  assert.deepEqual(one.cmds, [{ op: "rotate_items", ids: ["U1"], pivot: { x: 1000, y: 2000 }, angle_millideg: -90_000 }]);
  const cw = planRotate(b, ["U1"], -1);
  assert.deepEqual(cw.cmds[0], { op: "rotate_items", ids: ["U1"], pivot: { x: 1000, y: 2000 }, angle_millideg: 90_000 });
  // Two items share the centre of their box (5000, 2000).
  const two = planRotate(b, ["U1", "U2"], 1);
  assert.deepEqual(two.cmds[0], { op: "rotate_items", ids: ["U1", "U2"], pivot: { x: 5000, y: 2000 }, angle_millideg: -90_000 });
  // A rotation step of 15 degrees (Preferences > Editing) turns by that.
  assert.equal((planRotate(b, ["U1"], 1, undefined, 15).cmds[0] as { angle_millideg: number }).angle_millideg, -15_000);
});

test("planRotate: locked items stay and say so; all locked is no command at all", () => {
  const b = board({ parts: [part("U1", [0, 0]), part("U2", [9000, 0])], locked: ["U2"] });
  const some = planRotate(b, ["U1", "U2"], 1);
  assert.deepEqual(some.ids, ["U1"]);
  assert.ok(some.lockedOut);
  const none = planRotate(b, ["U2"], 1);
  assert.deepEqual(none.cmds, []);
  assert.ok(none.lockedOut);
});

test("planFlip: left-right by default, about the box centre; a pad flips its footprint", () => {
  const b = board({ parts: [part("U1", [1000, 2000], [pad("1", 0, 0)]), part("U2", [9000, 2000])] });
  assert.deepEqual(planFlip(b, ["U1", "U2"]).cmds, [{ op: "flip_items", ids: ["U1", "U2"], pivot: { x: 5000, y: 2000 }, direction: "left_right" }]);
  assert.deepEqual(planFlip(b, ["U1.1"], "top_bottom").cmds, [{ op: "flip_items", ids: ["U1"], pivot: { x: 1000, y: 2000 }, direction: "top_bottom" }]);
});

test("planCarry: a Move that was turned and flipped on the way turns and flips about where it was picked up, then moves", () => {
  const b = board({ parts: [part("U1", [1000, 2000])] });
  const plan = planCarry(b, ["U1"], 300, -400, 1, true);
  assert.deepEqual(plan.cmds, [
    { op: "rotate_items", ids: ["U1"], pivot: { x: 1000, y: 2000 }, angle_millideg: -90_000 },
    { op: "flip_items", ids: ["U1"], pivot: { x: 1000, y: 2000 }, direction: "left_right" },
    { op: "move_items", ids: ["U1"], dx: 300, dy: -400 },
  ]);
  // Four presses of R is a full turn: nothing to send for it.
  assert.deepEqual(planCarry(b, ["U1"], 0, 0, 4, false).cmds, []);
  assert.deepEqual(planCarry(b, ["U1"], 0, 0, 3, false).cmds, [{ op: "rotate_items", ids: ["U1"], pivot: { x: 1000, y: 2000 }, angle_millideg: -270_000 }]);
});

test("a dimension and a group have a position and a box too", () => {
  const b = board({
    parts: [part("U1", [20_000, 0])],
    drawings: drawings({ dimensions: [dim("d1")], groups: [{ id: "g", name: "", member_ids: ["d1", "U1"] }] }),
  });
  assert.deepEqual(itemPosition(b, "d1"), [0, 0], "PCB_DIMENSION_BASE::GetPosition is its first feature point");
  assert.ok(itemBounds(b, "d1")![2] >= 10_000);
  const g = itemBounds(b, "g")!;
  assert.ok(g[0] <= 0 && g[2] >= 21_000, "the group's box joins its members' boxes");
  const c = itemPosition(b, "g")!;
  assert.ok(Math.abs(c[0] - (g[0] + g[2]) / 2) < 1, "PCB_GROUP::GetPosition is the centre of its box");
});
