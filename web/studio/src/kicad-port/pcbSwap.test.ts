import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, LibraryFootprint } from "../api/types";
import { fpSwapCmd, groupSwapCentre, planSwap, swapSelection } from "./pcbSwap";
import type { Fp } from "./fpItems";

const part = (ref: string, x: number, y: number) => ({ ref, placed: true, at: [x, y], rot: 0, side: "top", pads: [{ num: "1", net: "N", x, y, w: 500, h: 500, round: false, th: false }] });

function board(locked: string[] = [], groups: { id: string; name: string; member_ids: string[] }[] = []): BoardState {
  return {
    name: "t",
    layers: ["F.Cu", "B.Cu"],
    parts: [part("U1", 0, 0), part("U2", 10_000, 0)],
    locked,
    routing: {
      tracks: [
        { id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [5000, 0]] },
        { id: "arc1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [1000, 1000], [2000, 0]], arc_mid: [1000, 1000] },
      ],
      vias: [{ id: "v1", net: "N", x: 1000, y: 2000, d: 600, drill: 300 }, { id: "v2", net: "N", x: 7000, y: 9000, d: 600, drill: 300 }],
      zones: [],
    },
    drawings: {
      shapes: [{ kind: "segment", id: "s1", layer: "F.SilkS", stroke_width: 100, filled: false, start: [0, 20_000], end: [4000, 20_000] }],
      texts: [
        { id: "a", content: "A", x: 0, y: 5000, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 100, justify: "left", mirror: false },
        { id: "b", content: "B", x: 3000, y: 6000, angle: 0, layer: "B.SilkS", size: 1000, stroke_width: 100, justify: "left", mirror: false },
      ],
      groups,
      dimensions: [],
    },
  } as unknown as BoardState;
}

test("Swap is one swap_items of the items in selection order, any kind (UI-LEFT-2 PP15)", () => {
  assert.deepEqual(planSwap(board(), ["v2", "a", "v1"]), { cmd: { op: "swap_items", ids: ["v2", "a", "v1"] }, lockedOut: false });
  // footprints are swapped by the same verb (their side and orientation are the backend's)
  assert.deepEqual(planSwap(board(), ["U2", "U1"]).cmd, { op: "swap_items", ids: ["U2", "U1"] });
});

test("Swap's filter: a pad is its footprint, a straight track goes, an arc stays (FilterCollectorForFreePads, PCB_TRACE_T)", () => {
  assert.deepEqual(swapSelection(board(), ["U1.1", "U2", "t1", "arc1"]).ids, ["U1", "U2", "arc1"]);
  // two pads of one footprint are one footprint: nothing to swap
  assert.equal(planSwap(board(), ["U1.1", "U1"]).cmd, null);
});

test("Swap's filter: an item of a group is its top group, whose centre goes with the command (FilterCollectorForHierarchy)", () => {
  const groups = [{ id: "g1", name: "", member_ids: ["s1", "b"] }];
  const { cmd } = planSwap(board([], groups), ["s1", "a", "b"]);
  // s1 and b stand for g1 (once); g1's box runs from (0, 6000)-ish to (4000, 20000)
  assert.equal((cmd as { ids: string[] }).ids.join(","), "g1,a");
  const c = (cmd as { centres: Record<string, { x: number; y: number }> }).centres.g1!;
  assert.ok(c.x > 0 && c.x < 4000 && c.y > 5000 && c.y < 20_000, JSON.stringify(c));
  // inside the entered group, its members are themselves
  assert.deepEqual(swapSelection(board([], groups), ["s1", "b"], "g1").ids, ["s1", "b"]);
});

test("Swap leaves locked items out and says so; fewer than two items is nothing (round 6 E6-3; 25ec2335 ReportFilteredLockedItems)", () => {
  assert.deepEqual(planSwap(board(["U1"]), ["U1", "U2"]), { cmd: null, lockedOut: true });
  assert.deepEqual(planSwap(board(), ["v1"]), { cmd: null, lockedOut: false });
  // a locked group member locks its group
  assert.deepEqual(swapSelection(board(["s1"], [{ id: "g1", name: "", member_ids: ["s1"] }]), ["g1", "a", "b"]), { ids: ["a", "b"], lockedOut: true });
});

const libFp = (): Fp =>
  ({
    name: "L:F",
    pads: [
      { id: "p1", number: "1", at: { x: 0, y: 0 }, size: [1000, 600], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", rot: 0, layers: [] },
      { id: "p2", number: "2", at: { x: 2000, y: 0 }, size: [1000, 600], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", rot: 0, layers: [] },
    ],
    graphics: [{ kind: "segment", id: "g1", layer: "F.SilkS", stroke_width: 120, filled: false, start: { x: 0, y: 1000 }, end: { x: 1000, y: 1000 } }],
    texts: [],
    fields: [],
    items: { groups: [{ id: "grp", name: "", member_ids: ["g1"] }] },
  }) as unknown as LibraryFootprint as Fp;

test("Swap in the Footprint Editor keeps pads as pads and promotes a group's member (UI-LEFT-2 PP15)", () => {
  assert.deepEqual(fpSwapCmd(libFp(), "L:F", ["p2", "p1"]), { op: "fp_item", verb: "swap", footprint: "L:F", ids: ["p2", "p1"] });
  const cmd = fpSwapCmd(libFp(), "L:F", ["g1", "p1"]) as { ids: string[]; centres: Record<string, unknown> };
  assert.deepEqual(cmd.ids, ["grp", "p1"]);
  assert.ok(cmd.centres.grp);
  assert.equal(fpSwapCmd(libFp(), "L:F", ["p1"]), null);
});

test("Swap takes tuning patterns, via stitchings, grid items and drill maps, leaves a table cell out; a group's centre is BOX2I's (UI-LEFT-3 P3-33)", () => {
  const b = board([], []);
  const dr = b.drawings as unknown as Record<string, unknown>;
  dr.tuning_patterns = [{ id: "tun1", member_ids: ["t1"] }];
  dr.groups = [{ id: "tun1", name: "Tuning Pattern", member_ids: ["t1"], generator: "tuning_pattern" }];
  dr.via_stitches = [{ id: "vst1", layer: "F.Cu", outline: [{ x: 0, y: 0 }, { x: 1000, y: 0 }, { x: 1000, y: 1000 }], via: {} }];
  dr.grid_items = [{ id: "grd1", kind: "cartesian", x: 100, y: 200, extent: [1000, 1000], spacing: [100, 100], angle: 0, phi_extent: 360, phi_spacing: 10, priority: 1, tick_interval: 0, affects: { cursor: true, routing: true, placement: true } }];
  dr.tables = [{ id: "tbl1", rows: 1, cols: 2, cells: [{ id: "tbl1:0:0", row: 0, col: 0, corners: [[0, 0], [1, 0], [1, 1], [0, 1]] }, { id: "tbl1:0:1", row: 0, col: 1, corners: [[1, 0], [2, 0], [2, 1], [1, 1]] }], bbox: [0, 0, 2, 1], layer: "F.SilkS" }];
  const sel = swapSelection(b, ["tun1", "vst1", "grd1", "tbl1:0:1", "a"]);
  assert.deepEqual(sel.ids, ["tun1", "vst1", "grd1", "a"]);
  assert.ok(!sel.ids.includes("tbl1:0:1"), "a table cell is left out");
  // a tuning pattern's position is its origin, read by the backend: no centre
  const plan = planSwap(b, ["tun1", "a"]).cmd as { centres?: Record<string, unknown> };
  assert.equal(plan.centres, undefined);
  // `GetBoundingBox().Centre()`: the 0.25 mm inflation and integer halves
  assert.deepEqual(groupSwapCentre([0, 0, 1001, 3]), { x: 500, y: 1 });
  assert.deepEqual(groupSwapCentre([-1001, -3, 0, 0]), { x: -501, y: -2 });
});
