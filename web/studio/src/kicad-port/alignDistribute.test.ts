import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { alignAxis, alignDeltas, alignItems, getDeltasForDistributeByGaps, getDeltasForDistributeByPoints, planAlign, planAlignSelection, planDistribute, planDistributeSelection, type AlignItem, type Box } from "./alignDistribute";

test("alignAxis: left/right/centerX are x, everything else is y", () => {
  assert.equal(alignAxis("left"), "x");
  assert.equal(alignAxis("right"), "x");
  assert.equal(alignAxis("centerX"), "x");
  assert.equal(alignAxis("top"), "y");
  assert.equal(alignAxis("bottom"), "y");
  assert.equal(alignAxis("centerY"), "y");
});

test("alignDeltas: fewer than 2 boxes is a no-op", () => {
  const boxes: Box[] = [[0, 0, 1000, 1000]];
  assert.deepEqual(alignDeltas(boxes, "top"), [0]);
  assert.deepEqual(alignDeltas([], "top"), []);
});

test("alignDeltas: top targets the smallest (topmost) top edge", () => {
  const boxes: Box[] = [
    [0, 500, 1000, 1500], // top = 500
    [2000, 0, 3000, 1000], // top = 0 (the extreme)
    [4000, 1000, 5000, 2000], // top = 1000
  ];
  assert.deepEqual(alignDeltas(boxes, "top"), [-500, 0, -1000]);
});

test("alignDeltas: bottom targets the largest (bottommost) bottom edge", () => {
  const boxes: Box[] = [
    [0, 0, 1000, 1000], // bottom = 1000
    [2000, 0, 3000, 2000], // bottom = 2000 (the extreme)
  ];
  assert.deepEqual(alignDeltas(boxes, "bottom"), [1000, 0]);
});

test("alignDeltas: left targets the smallest left edge, right targets the largest right edge", () => {
  const boxes: Box[] = [
    [500, 0, 1500, 1000],
    [0, 0, 2000, 1000],
  ];
  assert.deepEqual(alignDeltas(boxes, "left"), [-500, 0]);
  assert.deepEqual(alignDeltas(boxes, "right"), [500, 0]);
});

test("alignDeltas: centerX/centerY target the smallest center, same convention as top/left", () => {
  const boxes: Box[] = [
    [0, 0, 1000, 1000], // center (500, 500) -- the extreme (smallest) on both axes
    [2000, 4000, 2200, 4200], // center (2100, 4100)
  ];
  assert.deepEqual(alignDeltas(boxes, "centerX"), [0, -1600]);
  assert.deepEqual(alignDeltas(boxes, "centerY"), [0, -3600]);
});

test("getDeltasForDistributeByGaps: fewer than 3 items is a no-op", () => {
  assert.deepEqual(getDeltasForDistributeByGaps([]), []);
  assert.deepEqual(
    getDeltasForDistributeByGaps([
      [0, 100],
      [900, 1000],
    ]),
    [0, 0]
  );
});

test("getDeltasForDistributeByGaps: end caps never move; the middle item centers the leftover gap", () => {
  // Three 100-wide items: first at [0,100], last at [900,1000] (fixed),
  // middle starts at [400,500] -- the even-gap position for a 100-wide
  // middle item between them is [450,550], a +50 shift.
  const extents: [number, number][] = [
    [0, 100],
    [400, 500],
    [900, 1000],
  ];
  assert.deepEqual(getDeltasForDistributeByGaps(extents), [0, 50, 0]);
});

test("getDeltasForDistributeByGaps: four items split the remaining gap evenly between the two middle ones", () => {
  // Span from the first item's end (100) to the last item's start (900) is
  // 800, minus the two middle items' own 100+100 width = 600 left over,
  // split into 3 equal gaps of 200 each.
  const extents: [number, number][] = [
    [0, 100],
    [300, 400],
    [700, 800],
    [900, 1000],
  ];
  const deltas = getDeltasForDistributeByGaps(extents);
  assert.equal(deltas[0], 0);
  assert.equal(deltas[3], 0);
  // item 1 (currently at 300) should land at 100+200=300 -- already there.
  assert.equal(deltas[1], 0);
  // item 2 (currently at 700) should land at 300+100(item1 width)+200=600.
  assert.equal(deltas[2], -100);
});

test("getDeltasForDistributeByPoints: fewer than 3 points is a no-op", () => {
  assert.deepEqual(getDeltasForDistributeByPoints([0, 1000]), [0, 0]);
});

test("getDeltasForDistributeByPoints: evenly spaces the middle points between the first and last, end caps fixed", () => {
  // 0, 100, 300, 1000 -- evenly spaced 4 points over [0,1000] land at 0, 333, 667, 1000.
  const deltas = getDeltasForDistributeByPoints([0, 100, 300, 1000]);
  assert.equal(deltas[0], 0);
  assert.equal(deltas[3], 0);
  assert.equal(deltas[1], 233); // 100 -> 333
  assert.equal(deltas[2], 367); // 300 -> 667
});


// ---------------------------------------------------------------- the tool: locks, targets, every kind

const item = (id: string, box: Box, locked = false): AlignItem => ({ id, box, locked });

test("planAlign: without a lock the extreme item is the target and only the others move", () => {
  const moves = planAlign([item("a", [0, 500, 1000, 1500]), item("b", [2000, 0, 3000, 1000]), item("c", [4000, 1000, 5000, 2000])], "top");
  assert.deepEqual(moves, [
    { id: "a", dx: 0, dy: -500 },
    { id: "c", dx: 0, dy: -1000 },
  ]);
  assert.deepEqual(planAlign([item("a", [0, 0, 10, 10]), item("b", [5, 3, 15, 13])], "right").map((m) => m.id), ["a"], "the right-most edge is the target; the item already on it stays");
});

test("planAlign: a locked item is the target and never moves, whatever else is selected", () => {
  const moves = planAlign([item("free1", [0, 0, 1000, 1000]), item("pinned", [2000, 700, 3000, 1700], true), item("free2", [4000, -300, 5000, 700])], "top").sort((a, b) => (a.id < b.id ? -1 : 1));
  assert.deepEqual(moves, [
    { id: "free1", dx: 0, dy: 700 },
    { id: "free2", dx: 0, dy: 1000 },
  ]);
  assert.deepEqual(planAlign([item("a", [0, 0, 10, 10], true), item("b", [0, 5, 10, 15], true)], "top"), [], "all locked: nothing moves");
});

test("planAlign: the cursor picks the target when it sits inside an item's box, among locked items first", () => {
  const items = [item("a", [0, 0, 1000, 1000]), item("b", [2000, 500, 3000, 1500]), item("c", [4000, 900, 5000, 1900])];
  // The extreme (top-most) item is "a", but the cursor is over "c": its top (900) is the target.
  assert.deepEqual(planAlign(items, "top", [4500, 1200]), [
    { id: "a", dx: 0, dy: 900 },
    { id: "b", dx: 0, dy: 400 },
  ]);
  // Two locked items: the one under the cursor wins over the first.
  const locked = [item("L1", [0, 0, 100, 100], true), item("L2", [200, 40, 300, 140], true), item("m", [500, 500, 600, 600])];
  assert.deepEqual(planAlign(locked, "top", [250, 100]), [{ id: "m", dx: 0, dy: -460 }]);
  assert.deepEqual(planAlign(locked, "top", null), [{ id: "m", dx: 0, dy: -500 }], "no cursor: the first (extreme) locked item");
});

test("planAlign: centre and bottom edges, like the sort each action gives GetSelections", () => {
  const items = [item("a", [0, 0, 1000, 1000]), item("b", [0, 200, 1000, 400])];
  // Centres are 500 and 300: the smallest is the target, so a goes up to it.
  assert.deepEqual(planAlign(items, "centerY"), [{ id: "a", dx: 0, dy: -200 }]);
  assert.deepEqual(planAlign(items, "bottom"), [{ id: "b", dx: 0, dy: 600 }]);
});

test("planDistribute: end items stay, gaps equalise; fewer than three do nothing", () => {
  const moves = planDistribute([item("a", [0, 0, 1000, 10]), item("b", [1500, 0, 2500, 10]), item("c", [9000, 0, 10_000, 10])], "x", "gaps");
  // total space 8000 (1000 -> 9000), the middle item takes 1000 of it: gaps of 3500.
  assert.deepEqual(moves, [{ id: "b", dx: 3000, dy: 0 }]);
  assert.deepEqual(planDistribute([item("a", [0, 0, 10, 10]), item("b", [20, 0, 30, 10])], "x", "centers"), []);
  assert.deepEqual(planDistribute([item("a", [0, 0, 10, 10]), item("b", [0, 12, 10, 22]), item("c", [0, 100, 10, 110])], "y", "centers"), [{ id: "b", dx: 0, dy: 38 }], "the middle centre goes to the mean of the end centres");
});

// ------------------------------------------------ over a real board: every kind, pads, groups

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}
const pad = (num: string, x: number, y: number): Pad => ({ num, net: null, x, y, w: 600, h: 600, round: false, th: false });
const part = (ref: string, at: [number, number], pads: Pad[] = []): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads }) as Part;
const drawings = (extra: object) => ({ shapes: [], texts: [], groups: [], dimensions: [], dimension_settings: {}, ...extra }) as unknown as NonNullable<BoardState["drawings"]>;

test("alignItems: a footprint by its courtyard, a track by its copper, a group by its members", () => {
  const b = board({
    parts: [part("U1", [0, 0], [pad("1", -800, 0)]), part("U2", [9000, 3000])],
    routing: { tracks: [{ id: "t1", net: "GND", layer: "F.Cu", width: 200, pts: [[2000, 5000], [4000, 5000]] }], vias: [], zones: [], track_width_presets: [], via_presets: [], teardrop_settings: {} } as unknown as BoardState["routing"],
    drawings: drawings({ groups: [{ id: "g", name: "", member_ids: ["U2", "t1"] }] }),
    locked: ["U2"],
  });
  const byId = (ids: string[]) => Object.fromEntries(alignItems(b, ids).map((i) => [i.id, i]));
  const plain = byId(["U1", "t1"]);
  assert.deepEqual(plain["t1"]!.box, [1900, 4900, 4100, 5100]);
  assert.deepEqual(plain["U1"]!.box, [-1000, -500, 1000, 500]);
  assert.equal(plain["U1"]!.locked, false);
  const group = byId(["g"])["g"]!;
  assert.deepEqual(group.box, [1900, 2500, 10_000, 5100]);
  assert.equal(group.locked, true, "a group with a locked member is locked");
  assert.deepEqual(Object.keys(byId(["g", "t1"])), ["g"], "a member of a selected group is left to the group");
});

test("alignItems: a pad selected alone stands for its footprint but aligns by the pad's box, and a footprint listed once keeps its first box", () => {
  const b = board({ parts: [part("U1", [0, 0], [pad("1", -800, 0), pad("2", 800, 0)])] });
  assert.deepEqual(alignItems(b, ["U1.2"]), [{ id: "U1", box: [500, -300, 1100, 300], locked: false }]);
  assert.deepEqual(alignItems(b, ["U1.1", "U1.2"]), [{ id: "U1", box: [-1100, -300, -500, 300], locked: false }]);
  assert.deepEqual(alignItems(b, ["U1", "U1.2"]), [{ id: "U1", box: [-1000, -500, 1000, 500], locked: false }]);
});

test("planAlignSelection / planDistributeSelection: one move_items per moved item, locked footprints are the target / left out", () => {
  const b = board({
    parts: [part("U1", [0, 0]), part("U2", [3000, 700]), part("U3", [6000, 1500])],
    locked: ["U3"],
  });
  assert.deepEqual(planAlignSelection(b, ["U1", "U2", "U3"], "top"), [
    { op: "move_items", ids: ["U1"], dx: 0, dy: 1500 },
    { op: "move_items", ids: ["U2"], dx: 0, dy: 800 },
  ]);
  // Distribute leaves the locked U3 out: two are left, nothing to do.
  assert.deepEqual(planDistributeSelection(b, ["U1", "U2", "U3"], "x", "centers"), []);
  const free = board({ parts: [part("U1", [0, 0]), part("U2", [1000, 0]), part("U3", [6000, 0])] });
  assert.deepEqual(planDistributeSelection(free, ["U1", "U2", "U3"], "x", "centers"), [{ op: "move_items", ids: ["U2"], dx: 2000, dy: 0 }]);
});
