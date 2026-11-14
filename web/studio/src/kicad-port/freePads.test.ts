// "Allow free pads" (`PCBNEW_SETTINGS::m_AllowFreePads`, UI-LEFT-3 R3-7): the edit tools' `FilterCollectorForFreePads` keeps a pad as itself when it is on.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Part, Pad } from "../api/types";
import { editableSelection, isLocked, planFlip, planMove } from "./pcbTransform";
import { planSwap } from "./pcbSwap";
import { alignItems } from "./alignDistribute";
import { splitCarried } from "./pcbCarry";
import { movableItem } from "./pcbEditActions";
import { mirrorableIds, planMirror } from "./pcbMirror";
import { getAllowFreePads, setAllowFreePads } from "./freePads";

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}
const pad = (num: string, x: number, y: number): Pad => ({ num, net: null, x, y, w: 600, h: 600, round: false, th: false, px: x, py: y });
const part = (ref: string, at: [number, number], pads: Pad[] = []): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at, rot: 0, side: "top", label: "above", courtyard: [at[0] - 1000, at[1] - 500, at[0] + 1000, at[1] + 500], pads }) as Part;

const b = board({ parts: [part("U1", [0, 0], [pad("1", -1000, 0), pad("2", 1000, 0)]), part("U2", [5000, 0], [pad("1", 4000, 0), pad("2", 6000, 0)])] });

function withFreePads<T>(on: boolean, f: () => T): T {
  const was = getAllowFreePads();
  setAllowFreePads(on);
  try {
    return f();
  } finally {
    setAllowFreePads(was);
  }
}

test("off (KiCad's default): a pad stands for its footprint", () => {
  withFreePads(false, () => {
    assert.deepEqual(editableSelection(b, ["U1.2"]).ids, ["U1"]);
    assert.deepEqual(planMove(b, ["U1.2"], 100, 0).cmds, [{ op: "move_items", ids: ["U1"], dx: 100, dy: 0 }]);
    assert.deepEqual(planSwap(b, ["U1.1", "U2.1"]).cmd, { op: "swap_items", ids: ["U1", "U2"] }, "two pads of two footprints: the footprints");
    assert.deepEqual(alignItems(b, ["U1.2", "U2.1"]).map((i) => i.id), ["U1", "U2"]);
    assert.deepEqual(mirrorableIds(b, ["U1.2"]), []);
  });
});

test("on: Move, Swap, Mirror, Position Relative and the carry take the pad as itself; Flip and Duplicate promote it", () => {
  withFreePads(true, () => {
    assert.deepEqual(editableSelection(b, ["U1.2"]).ids, ["U1.2"]);
    assert.deepEqual(planMove(b, ["U1.2"], 100, 0).cmds, [{ op: "move_items", ids: ["U1.2"], dx: 100, dy: 0 }]);
    assert.deepEqual(editableSelection(b, ["U1.2"], { forcePromotion: true }).ids, ["U1"], "Duplicate's `FilterCollectorForFreePads( aCollector, true )`");
    assert.deepEqual(planFlip(b, ["U1.2"]).ids, ["U1"], "a pad is turned over with its footprint");
    assert.deepEqual(planSwap(b, ["U1.1", "U1.2"]).cmd, { op: "swap_items", ids: ["U1.1", "U1.2"] });
    assert.deepEqual(mirrorableIds(b, ["U1.2"]), ["U1.2"]);
    assert.deepEqual(planMirror(b, ["U1.2"], [0, 0], "leftRight").cmds, [{ op: "mirror_items", ids: ["U1.2"], pivot: { x: 0, y: 0 }, direction: "left_right" }]);
    assert.deepEqual(movableItem(b, "U1.2"), { kind: "pad", at: [1000, 0] });
    // the carry: U1 stays without pad 2, a copy holding pad 2 alone moves (no courtyard drawn), and the airwires move with no footprint
    const carry = splitCarried(b, ["U1.2"]);
    assert.deepEqual(carry.still.parts.find((p) => p.ref === "U1")?.pads?.map((p) => p.num), ["1"]);
    assert.deepEqual(carry.moving.parts.map((p) => [p.ref, p.pads?.map((q) => q.num), p.carriedPadsOnly]), [["U1", ["2"], true]]);
    assert.deepEqual(carry.partRefs, []);
  });
});

test("on: Align takes the pads as themselves unless all of them are pads of different footprints", () => {
  withFreePads(true, () => {
    assert.deepEqual(alignItems(b, ["U1.1", "U1.2"]).map((i) => i.id), ["U1.1", "U1.2"], "pads of one footprint align as pads");
    assert.deepEqual(alignItems(b, ["U1.2", "U2.1"]).map((i) => i.id), ["U1", "U2"], "`allPads && differentParents`: their footprints");
  });
});

test("a pad of a locked footprint is locked (`PAD::IsLocked`)", () => {
  const locked = { ...b, locked: ["U1"] } as BoardState;
  assert.equal(isLocked(locked, "U1.2"), true);
  assert.equal(isLocked(locked, "U2.1"), false);
  withFreePads(true, () => assert.deepEqual(editableSelection(locked, ["U1.2", "U2.1"]), { ids: ["U2.1"], lockedOut: true }));
});
